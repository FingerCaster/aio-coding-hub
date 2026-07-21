//! Maintain the complete Codex model catalog used by AIO-managed profiles.

use super::protocol;
use crate::shared::error::{db_err, AppError, AppResult};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const GENERATED_CATALOG_FILE_NAME: &str = "managed-model-catalog.json";
const GENERATED_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const USER_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_BASE_MODEL_COUNT: usize = 1_000;
const MAX_MANAGED_PROFILE_COUNT: usize = 256;
const OWNER_METADATA_KEY: &str = "_aio_managed_model_catalog";
const OWNER_SCHEMA_VERSION: u64 = 1;
const MANAGED_BY: &str = "aio-coding-hub";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedCatalogProfile {
    pub(crate) profile_name_key: String,
    pub(crate) model_uuid: String,
    pub(crate) provider_name: String,
    pub(crate) remote_model_id: String,
}

impl ManagedCatalogProfile {
    pub(crate) fn new(
        profile_name_key: impl Into<String>,
        model_uuid: impl Into<String>,
        provider_name: impl Into<String>,
        remote_model_id: impl Into<String>,
    ) -> AppResult<Self> {
        let profile = Self {
            profile_name_key: profile_name_key.into(),
            model_uuid: model_uuid.into(),
            provider_name: provider_name.into(),
            remote_model_id: remote_model_id.into(),
        };
        validate_profile(&profile)?;
        Ok(profile)
    }

    fn alias(&self) -> String {
        format!("aio/{}", self.profile_name_key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug)]
struct PreparedCatalogChange {
    baseline: crate::cli_proxy::CodexProxyBaseline,
    config_before: FileSnapshot,
    config_after: Vec<u8>,
    generated_before: FileSnapshot,
    generated_after: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct ManagedCatalogPlan {
    change: Option<PreparedCatalogChange>,
}

#[derive(Debug)]
struct AppliedFileChange {
    before: FileSnapshot,
    after: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct AppliedManagedCatalog {
    config: Option<AppliedFileChange>,
    generated: Option<AppliedFileChange>,
}

impl ManagedCatalogPlan {
    fn inactive() -> Self {
        Self { change: None }
    }

    pub(crate) fn apply<R: tauri::Runtime>(
        self,
        app: &tauri::AppHandle<R>,
    ) -> AppResult<AppliedManagedCatalog> {
        let Some(change) = self.change else {
            return Ok(AppliedManagedCatalog {
                config: None,
                generated: None,
            });
        };

        let current_baseline = match crate::cli_proxy::codex_enabled_proxy_baseline(app)? {
            Some(result) => result,
            None => {
                return Err(AppError::new(
                    "CODEX_MANAGED_MODEL_PROXY_DISABLED",
                    "Codex CLI proxy was disabled while applying the managed model catalog",
                ))
            }
        };
        if current_baseline != change.baseline {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
                "Codex CLI proxy baseline changed while preparing the managed model catalog",
            ));
        }

        ensure_snapshot_unchanged(
            &change.config_before,
            crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
        )?;
        ensure_snapshot_unchanged(&change.generated_before, GENERATED_CATALOG_MAX_BYTES)?;

        let mut generated_change = None;
        if change.generated_before.bytes != change.generated_after {
            apply_generated_catalog_state(
                &change.generated_before.path,
                change.generated_after.as_deref(),
            )?;
            generated_change = Some(AppliedFileChange {
                before: change.generated_before.clone(),
                after: change.generated_after.clone(),
            });
        }

        let mut config_change = None;
        if change.config_before.bytes.as_deref() != Some(change.config_after.as_slice()) {
            if let Err(error) = crate::cli_proxy::write_cli_proxy_file_atomic(
                &change.config_before.path,
                &change.config_after,
            ) {
                if let Some(applied) = generated_change.take() {
                    let _ = rollback_file_change(&applied, GENERATED_CATALOG_MAX_BYTES);
                }
                return Err(AppError::new(
                    "CODEX_MANAGED_MODEL_CONFIG_WRITE_FAILED",
                    format!("failed to update Codex config.toml: {error}"),
                ));
            }
            config_change = Some(AppliedFileChange {
                before: change.config_before,
                after: Some(change.config_after),
            });
        }

        Ok(AppliedManagedCatalog {
            config: config_change,
            generated: generated_change,
        })
    }
}

impl AppliedManagedCatalog {
    pub(crate) fn rollback(self) -> AppResult<()> {
        if let Some(config) = self.config.as_ref() {
            rollback_file_change(config, crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES)?;
        }
        if let Some(generated) = self.generated.as_ref() {
            rollback_file_change(generated, GENERATED_CATALOG_MAX_BYTES)?;
        }
        Ok(())
    }
}

enum BaseCatalogSource {
    User {
        bytes: Vec<u8>,
        fingerprint: String,
    },
    Bundled {
        launch: crate::cli_manager::CodexLaunchSpec,
        fingerprint: String,
    },
}

impl BaseCatalogSource {
    fn fingerprint(&self) -> &str {
        match self {
            Self::User { fingerprint, .. } | Self::Bundled { fingerprint, .. } => fingerprint,
        }
    }

    fn load<R: tauri::Runtime>(self, app: &tauri::AppHandle<R>) -> AppResult<Vec<u8>> {
        match self {
            Self::User { bytes, .. } => Ok(bytes),
            Self::Bundled { launch, .. } => {
                let codex_home = crate::codex_paths::codex_home_dir(app)?;
                protocol::fetch_bundled_catalog(&launch, &codex_home).map_err(|error| {
                    let (code, message) = match error {
                        protocol::ProtocolError::Timeout => (
                            "CODEX_MANAGED_MODEL_BUNDLED_TIMEOUT",
                            "Codex debug models --bundled timed out",
                        ),
                        protocol::ProtocolError::Spawn => (
                            "CODEX_MANAGED_MODEL_BUNDLED_UNAVAILABLE",
                            "failed to run Codex debug models --bundled",
                        ),
                        protocol::ProtocolError::Malformed | protocol::ProtocolError::JsonRpc => (
                            "CODEX_MANAGED_MODEL_BUNDLED_INVALID",
                            "Codex debug models --bundled returned an invalid catalog",
                        ),
                    };
                    AppError::new(code, message)
                })
            }
        }
    }
}

#[derive(Debug)]
struct OwnedCatalogMetadata {
    profile_set_sha256: String,
    base_source_fingerprint: String,
}

pub(crate) fn load_profiles(conn: &Connection) -> AppResult<Vec<ManagedCatalogProfile>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT profile.profile_name_key, profile.model_uuid, provider.name, model.remote_model_id
FROM codex_managed_profiles profile
JOIN provider_models model ON model.model_uuid = profile.model_uuid
JOIN providers provider ON provider.id = model.provider_id
ORDER BY profile.profile_name_key ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare managed model catalog query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ManagedCatalogProfile {
                profile_name_key: row.get(0)?,
                model_uuid: row.get(1)?,
                provider_name: row.get(2)?,
                remote_model_id: row.get(3)?,
            })
        })
        .map_err(|error| db_err!("failed to query managed model catalog profiles: {error}"))?;
    let profiles = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read managed model catalog profile: {error}"))?;
    validate_profiles(&profiles)?;
    Ok(profiles)
}

pub(crate) fn prepare_for_profiles<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
) -> AppResult<ManagedCatalogPlan> {
    validate_profiles(profiles)?;
    let Some(baseline) = crate::cli_proxy::codex_enabled_proxy_baseline(app)? else {
        return Ok(ManagedCatalogPlan::inactive());
    };
    if !crate::cli_proxy::codex_proxy_config_is_applied(app, &baseline.base_origin) {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_PROXY_DRIFT",
            "Codex CLI proxy configuration is not currently applied",
        ));
    }

    let generated_path = managed_catalog_path(app)?;
    let generated_before = snapshot_generated_file(&generated_path)?;
    let existing_metadata = generated_before
        .bytes
        .as_deref()
        .map(validate_owned_catalog)
        .transpose()?;

    let original_catalog_path = parse_original_catalog_path(baseline.config_bytes.as_deref())?;
    if let Some(path) = original_catalog_path.as_deref() {
        reject_generated_path_as_base(path, &generated_path)?;
    }

    let config_before = snapshot_cli_proxy_file(&baseline.config_path)?;
    let current_config = config_before.bytes.as_deref().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_MISSING",
            "Codex config.toml disappeared while the CLI proxy was enabled",
        )
    })?;
    validate_current_catalog_binding(
        current_config,
        original_catalog_path.as_deref(),
        &generated_path,
    )?;

    let generated_after = if profiles.is_empty() {
        None
    } else {
        let profile_set_sha256 = profile_set_sha256(profiles)?;
        let source = base_catalog_source(app, original_catalog_path.as_deref())?;
        if existing_metadata.as_ref().is_some_and(|metadata| {
            metadata.profile_set_sha256 == profile_set_sha256
                && metadata.base_source_fingerprint == source.fingerprint()
        }) {
            generated_before.bytes.clone()
        } else {
            let source_fingerprint = source.fingerprint().to_string();
            let base_bytes = source.load(app)?;
            Some(generate_catalog(
                &base_bytes,
                profiles,
                &profile_set_sha256,
                &source_fingerprint,
            )?)
        }
    };

    let desired_catalog_path = (!profiles.is_empty()).then_some(generated_path.as_path());
    let config_after = patch_model_catalog_config(
        current_config,
        baseline.config_bytes.as_deref(),
        desired_catalog_path,
    )?;

    Ok(ManagedCatalogPlan {
        change: Some(PreparedCatalogChange {
            baseline,
            config_before,
            config_after,
            generated_before,
            generated_after,
        }),
    })
}

pub(crate) fn sync_current_locked<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<()> {
    let db = crate::db::init(app)?;
    let conn = db.open_connection()?;
    let profiles = load_profiles(&conn)?;
    let _applied = prepare_for_profiles(app, &profiles)?.apply(app)?;
    Ok(())
}

fn managed_catalog_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<PathBuf> {
    let root = crate::app_paths::app_data_dir(app)?
        .join("cli-proxy")
        .join("codex");
    std::fs::create_dir_all(&root).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
            "failed to create the managed Codex catalog directory",
        )
    })?;
    let root = std::fs::canonicalize(&root).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
            "failed to resolve the managed Codex catalog directory",
        )
    })?;
    Ok(root.join(GENERATED_CATALOG_FILE_NAME))
}

fn validate_profile(profile: &ManagedCatalogProfile) -> AppResult<()> {
    let key = profile.profile_name_key.as_bytes();
    let valid_key = !key.is_empty()
        && key.len() <= 64
        && key[0].is_ascii_lowercase_or_digit()
        && key
            .iter()
            .all(|byte| byte.is_ascii_lowercase_or_digit() || matches!(byte, b'_' | b'-'));
    if !valid_key
        || crate::shared::uuid::is_canonical_uuid_v4(&profile.profile_name_key)
        || !crate::shared::uuid::is_canonical_uuid_v4(&profile.model_uuid)
    {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile identity is invalid",
        ));
    }
    if profile.remote_model_id.trim().is_empty() || profile.remote_model_id.len() > 256 {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile remote model is invalid",
        ));
    }
    Ok(())
}

trait AsciiLowercaseOrDigit {
    fn is_ascii_lowercase_or_digit(&self) -> bool;
}

impl AsciiLowercaseOrDigit for u8 {
    fn is_ascii_lowercase_or_digit(&self) -> bool {
        self.is_ascii_lowercase() || self.is_ascii_digit()
    }
}

fn validate_profiles(profiles: &[ManagedCatalogProfile]) -> AppResult<()> {
    if profiles.len() > MAX_MANAGED_PROFILE_COUNT {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_PROFILE_LIMIT",
            "too many managed Codex profiles to build a bounded model catalog",
        ));
    }
    let mut aliases = HashSet::with_capacity(profiles.len());
    for profile in profiles {
        validate_profile(profile)?;
        if !aliases.insert(profile.alias()) {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "managed Codex profile aliases are not unique",
            ));
        }
    }
    Ok(())
}

fn snapshot_cli_proxy_file(path: &Path) -> AppResult<FileSnapshot> {
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        bytes: crate::cli_proxy::read_optional_cli_proxy_file(path)?,
    })
}

fn snapshot_generated_file(path: &Path) -> AppResult<FileSnapshot> {
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        bytes: crate::shared::fs::read_optional_file_with_max_len(
            path,
            GENERATED_CATALOG_MAX_BYTES,
        )?,
    })
}

fn read_snapshot(path: &Path, max_len: usize) -> AppResult<Option<Vec<u8>>> {
    crate::shared::fs::read_optional_file_with_max_len(path, max_len)
}

fn ensure_snapshot_unchanged(snapshot: &FileSnapshot, max_len: usize) -> AppResult<()> {
    if read_snapshot(&snapshot.path, max_len)? != snapshot.bytes {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            format!(
                "{} changed while preparing the managed model catalog",
                snapshot.path.display()
            ),
        ));
    }
    Ok(())
}

fn rollback_file_change(change: &AppliedFileChange, max_len: usize) -> AppResult<()> {
    let current = read_snapshot(&change.before.path, max_len)?;
    if current != change.after {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            format!(
                "{} changed after the managed model catalog update; refusing to overwrite it",
                change.before.path.display()
            ),
        ));
    }
    match change.before.bytes.as_deref() {
        Some(bytes) => crate::shared::fs::write_file_atomic(&change.before.path, bytes),
        None => match std::fs::remove_file(&change.before.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                format!("failed to remove {}: {error}", change.before.path.display()),
            )),
        },
    }
}

fn apply_generated_catalog_state(path: &Path, bytes: Option<&[u8]>) -> AppResult<()> {
    match bytes {
        Some(bytes) => crate::shared::fs::write_file_atomic(path, bytes).map_err(|_| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                "failed to write the AIO-managed Codex model catalog",
            )
        }),
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AppError::new(
                "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                "failed to remove the inactive AIO-managed Codex model catalog",
            )),
        },
    }
}

fn parse_original_catalog_path(config: Option<&[u8]>) -> AppResult<Option<PathBuf>> {
    parse_catalog_path(config, "original")
}

fn parse_catalog_path(config: Option<&[u8]>, source: &str) -> AppResult<Option<PathBuf>> {
    let Some(config) = config else {
        return Ok(None);
    };
    let text = std::str::from_utf8(config).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("the {source} Codex config.toml is not UTF-8"),
        )
    })?;
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("the {source} Codex config.toml is invalid TOML"),
        )
    })?;
    let Some(item) = document.get("model_catalog_json") else {
        return Ok(None);
    };
    let value = item.as_str().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("model_catalog_json in the {source} Codex config must be a string"),
        )
    })?;
    let path = PathBuf::from(value);
    if value.is_empty() || !path.is_absolute() {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("model_catalog_json in the {source} Codex config must be an absolute path"),
        ));
    }
    Ok(Some(path))
}

fn validate_current_catalog_binding(
    current: &[u8],
    original: Option<&Path>,
    generated: &Path,
) -> AppResult<()> {
    let current_path = parse_catalog_path(Some(current), "current")?;
    let matches_original = current_path.as_deref() == original;
    let matches_generated = current_path.as_deref() == Some(generated);
    if !matches_original && !matches_generated {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "model_catalog_json changed outside AIO while the Codex proxy was enabled",
        ));
    }
    Ok(())
}

fn patch_model_catalog_config(
    current: &[u8],
    original: Option<&[u8]>,
    generated_path: Option<&Path>,
) -> AppResult<Vec<u8>> {
    let current = std::str::from_utf8(current).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "current Codex config.toml is not UTF-8",
        )
    })?;
    let mut document = current.parse::<toml_edit::DocumentMut>().map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "current Codex config.toml is invalid TOML",
        )
    })?;
    if document
        .get("model_catalog_json")
        .is_some_and(|item| item.as_str().is_none())
    {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "current model_catalog_json must be a string",
        ));
    }

    let desired = match generated_path {
        Some(path) => Some(
            path.to_str()
                .ok_or_else(|| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                        "managed model catalog path must be valid UTF-8",
                    )
                })?
                .to_string(),
        ),
        None => parse_original_catalog_path(original)?
            .and_then(|path| path.to_str().map(str::to_string)),
    };
    match desired.as_deref() {
        Some(path) => document["model_catalog_json"] = toml_edit::value(path),
        None => {
            document.remove("model_catalog_json");
        }
    }
    let output = document.to_string().into_bytes();
    let reparsed = std::str::from_utf8(&output)
        .ok()
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                "generated Codex config.toml failed validation",
            )
        })?;
    if reparsed
        .get("model_catalog_json")
        .and_then(toml_edit::Item::as_str)
        != desired.as_deref()
    {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "generated model_catalog_json failed round-trip validation",
        ));
    }
    Ok(output)
}

fn reject_generated_path_as_base(base: &Path, generated: &Path) -> AppResult<()> {
    let same = if base == generated {
        true
    } else {
        match (
            std::fs::canonicalize(base),
            std::fs::canonicalize(generated),
        ) {
            (Ok(base), Ok(generated)) => base == generated,
            _ => false,
        }
    };
    if same {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the AIO-generated catalog cannot be used as its own base catalog",
        ));
    }
    Ok(())
}

fn base_catalog_source<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    original_catalog_path: Option<&Path>,
) -> AppResult<BaseCatalogSource> {
    if let Some(path) = original_catalog_path {
        let bytes = crate::shared::fs::read_file_with_max_len(path, USER_CATALOG_MAX_BYTES)
            .map_err(|_| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_BASE_CATALOG_UNAVAILABLE",
                    "failed to read the user-configured Codex model catalog",
                )
            })?;
        let fingerprint = sha256_hex(
            &[
                b"user\0".as_slice(),
                path.to_string_lossy().as_bytes(),
                b"\0",
                sha256_hex(&bytes).as_bytes(),
            ]
            .concat(),
        );
        return Ok(BaseCatalogSource::User { bytes, fingerprint });
    }

    let launch = crate::cli_manager::codex_launch_spec(app)?.ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CLI_NOT_FOUND",
            "Codex CLI was not found",
        )
    })?;
    let metadata = std::fs::metadata(&launch.executable).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CLI_NOT_FOUND",
            "the resolved Codex executable is unavailable",
        )
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let descriptor = format!(
        "bundled\0{}\0{}\0{}\0{}",
        launch.executable.to_string_lossy(),
        launch.version.as_deref().unwrap_or(""),
        metadata.len(),
        modified
    );
    Ok(BaseCatalogSource::Bundled {
        launch,
        fingerprint: sha256_hex(descriptor.as_bytes()),
    })
}

fn profile_set_sha256(profiles: &[ManagedCatalogProfile]) -> AppResult<String> {
    let payload = profiles
        .iter()
        .map(|profile| {
            json!({
                "alias": profile.alias(),
                "model_uuid": profile.model_uuid,
                "provider_name": profile.provider_name,
                "remote_model_id": profile.remote_model_id,
            })
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&payload).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to serialize the managed Codex profile set",
        )
    })?;
    Ok(sha256_hex(&bytes))
}

fn generate_catalog(
    base_bytes: &[u8],
    profiles: &[ManagedCatalogProfile],
    profile_set_sha256: &str,
    base_source_fingerprint: &str,
) -> AppResult<Vec<u8>> {
    let mut root: Value = serde_json::from_slice(base_bytes).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog is not valid JSON",
        )
    })?;
    let object = root.as_object_mut().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog root must be an object",
        )
    })?;
    if object.contains_key(OWNER_METADATA_KEY) {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog contains reserved AIO metadata",
        ));
    }
    let models = object
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "the base Codex model catalog must contain a models array",
            )
        })?;
    if models.is_empty() || models.len() > MAX_BASE_MODEL_COUNT {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog has an invalid model count",
        ));
    }

    let mut slugs = HashSet::with_capacity(models.len() + profiles.len());
    let mut template = None;
    for model in models.iter() {
        let model_object = model.as_object().ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "every base Codex model must be an object",
            )
        })?;
        let slug = model_object
            .get("slug")
            .and_then(Value::as_str)
            .filter(|slug| !slug.is_empty() && slug.len() <= 256)
            .ok_or_else(|| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                    "every base Codex model must have a valid slug",
                )
            })?;
        if !slugs.insert(slug.to_string()) {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "the base Codex model catalog contains duplicate slugs",
            ));
        }
        if template.is_none()
            && model_object.get("visibility").and_then(Value::as_str) == Some("list")
        {
            template = Some(model_object.clone());
        }
    }
    let template = template.ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog has no visible template model",
        )
    })?;

    for (index, profile) in profiles.iter().enumerate() {
        let alias = profile.alias();
        if !slugs.insert(alias.clone()) {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_ALIAS_CONFLICT",
                format!("the base Codex model catalog already contains {alias}"),
            ));
        }
        models.push(Value::Object(build_managed_model(
            &template, profile, index,
        )));
    }

    let base_catalog_sha256 = sha256_hex(base_bytes);
    let aliases = profiles
        .iter()
        .map(ManagedCatalogProfile::alias)
        .collect::<Vec<_>>();
    let mut payload_root = root.clone();
    payload_root
        .as_object_mut()
        .expect("validated catalog object")
        .remove(OWNER_METADATA_KEY);
    let payload_sha256 = sha256_hex(
        &serde_json::to_vec(&json!({
            "catalog": payload_root,
            "profile_set_sha256": profile_set_sha256,
            "base_catalog_sha256": base_catalog_sha256,
            "base_source_fingerprint": base_source_fingerprint,
            "managed_aliases": aliases,
        }))
        .map_err(|_| {
            AppError::new(
                "SYSTEM_ERROR",
                "failed to hash the managed Codex model catalog",
            )
        })?,
    );
    root.as_object_mut()
        .expect("validated catalog object")
        .insert(
            OWNER_METADATA_KEY.to_string(),
            json!({
                "schema_version": OWNER_SCHEMA_VERSION,
                "managed_by": MANAGED_BY,
                "payload_sha256": payload_sha256,
                "profile_set_sha256": profile_set_sha256,
                "base_catalog_sha256": base_catalog_sha256,
                "base_source_fingerprint": base_source_fingerprint,
                "managed_aliases": aliases,
            }),
        );
    let mut output = serde_json::to_vec_pretty(&root).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to serialize the managed Codex model catalog",
        )
    })?;
    output.push(b'\n');
    if output.len() > GENERATED_CATALOG_MAX_BYTES {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CATALOG_LIMIT",
            "the generated Codex model catalog exceeds the size limit",
        ));
    }
    Ok(output)
}

fn build_managed_model(
    template: &Map<String, Value>,
    profile: &ManagedCatalogProfile,
    index: usize,
) -> Map<String, Value> {
    let alias = profile.alias();
    let mut model = template.clone();
    model.insert("slug".to_string(), json!(alias));
    model.insert(
        "display_name".to_string(),
        json!(format!("AIO / {}", profile.profile_name_key)),
    );
    model.insert(
        "description".to_string(),
        json!(bounded_description(profile)),
    );
    model.insert("default_reasoning_level".to_string(), Value::Null);
    model.insert("supported_reasoning_levels".to_string(), json!([]));
    model.insert("visibility".to_string(), json!("list"));
    model.insert("supported_in_api".to_string(), json!(true));
    model.insert(
        "priority".to_string(),
        json!(10_000_i64.saturating_add(index as i64)),
    );
    model.insert("additional_speed_tiers".to_string(), json!([]));
    model.insert("service_tiers".to_string(), json!([]));
    model.insert("default_service_tier".to_string(), Value::Null);
    model.insert("availability_nux".to_string(), Value::Null);
    model.insert("upgrade".to_string(), Value::Null);
    model.insert("model_messages".to_string(), Value::Null);
    model.insert(
        "include_skills_usage_instructions".to_string(),
        json!(false),
    );
    model.insert("supports_reasoning_summaries".to_string(), json!(false));
    model.insert("default_reasoning_summary".to_string(), json!("none"));
    model.insert("support_verbosity".to_string(), json!(false));
    model.insert("default_verbosity".to_string(), Value::Null);
    model.insert("apply_patch_tool_type".to_string(), Value::Null);
    model.insert("web_search_tool_type".to_string(), json!("text"));
    model.insert("supports_parallel_tool_calls".to_string(), json!(false));
    model.insert("supports_image_detail_original".to_string(), json!(false));
    model.insert("context_window".to_string(), Value::Null);
    model.insert("max_context_window".to_string(), Value::Null);
    model.insert("auto_compact_token_limit".to_string(), Value::Null);
    model.insert("comp_hash".to_string(), Value::Null);
    model.insert("effective_context_window_percent".to_string(), json!(95));
    model.insert("experimental_supported_tools".to_string(), json!([]));
    model.insert("input_modalities".to_string(), json!(["text"]));
    model.insert("supports_search_tool".to_string(), json!(false));
    model.insert("use_responses_lite".to_string(), json!(false));
    model.insert("auto_review_model_override".to_string(), Value::Null);
    model.insert("tool_mode".to_string(), Value::Null);
    model.insert("multi_agent_version".to_string(), Value::Null);
    model
}

fn bounded_description(profile: &ManagedCatalogProfile) -> String {
    let raw = format!(
        "AIO managed route · {} · {}",
        profile.provider_name, profile.remote_model_id
    );
    raw.chars().take(512).collect()
}

fn validate_owned_catalog(bytes: &[u8]) -> AppResult<OwnedCatalogMetadata> {
    let root: Value = serde_json::from_slice(bytes).map_err(|_| modified_catalog_error())?;
    let object = root.as_object().ok_or_else(modified_catalog_error)?;
    let metadata = object
        .get(OWNER_METADATA_KEY)
        .and_then(Value::as_object)
        .ok_or_else(modified_catalog_error)?;
    if metadata.get("schema_version").and_then(Value::as_u64) != Some(OWNER_SCHEMA_VERSION)
        || metadata.get("managed_by").and_then(Value::as_str) != Some(MANAGED_BY)
    {
        return Err(modified_catalog_error());
    }
    let payload_sha256 = required_metadata_string(metadata, "payload_sha256")?;
    let profile_set_sha256 = required_metadata_string(metadata, "profile_set_sha256")?;
    let base_catalog_sha256 = required_metadata_string(metadata, "base_catalog_sha256")?;
    let base_source_fingerprint = required_metadata_string(metadata, "base_source_fingerprint")?;
    let aliases = metadata
        .get("managed_aliases")
        .and_then(Value::as_array)
        .ok_or_else(modified_catalog_error)?;
    if aliases.iter().any(|alias| alias.as_str().is_none()) {
        return Err(modified_catalog_error());
    }

    let mut payload_root = root.clone();
    payload_root
        .as_object_mut()
        .ok_or_else(modified_catalog_error)?
        .remove(OWNER_METADATA_KEY);
    let expected = sha256_hex(
        &serde_json::to_vec(&json!({
            "catalog": payload_root,
            "profile_set_sha256": profile_set_sha256,
            "base_catalog_sha256": base_catalog_sha256,
            "base_source_fingerprint": base_source_fingerprint,
            "managed_aliases": aliases,
        }))
        .map_err(|_| modified_catalog_error())?,
    );
    if payload_sha256 != expected {
        return Err(modified_catalog_error());
    }
    Ok(OwnedCatalogMetadata {
        profile_set_sha256: profile_set_sha256.to_string(),
        base_source_fingerprint: base_source_fingerprint.to_string(),
    })
}

fn required_metadata_string<'a>(metadata: &'a Map<String, Value>, key: &str) -> AppResult<&'a str> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(modified_catalog_error)
}

fn modified_catalog_error() -> AppError {
    AppError::new(
        "CODEX_MANAGED_MODEL_CATALOG_MODIFIED",
        "the AIO-managed Codex model catalog was modified externally",
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_catalog() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "future_top_level": {"kept": true},
            "models": [{
                "slug": "gpt-base",
                "display_name": "GPT Base",
                "description": "base",
                "default_reasoning_level": "high",
                "supported_reasoning_levels": [{"effort": "high", "description": "deep"}],
                "shell_type": "shell_command",
                "visibility": "list",
                "supported_in_api": true,
                "priority": 1,
                "additional_speed_tiers": ["fast"],
                "service_tiers": [{"id": "priority", "name": "Fast", "description": "fast"}],
                "availability_nux": {"message": "new"},
                "upgrade": {"model": "next", "migration_markdown": "move"},
                "base_instructions": "base instructions",
                "model_messages": {"instructions_template": "large"},
                "supports_reasoning_summaries": true,
                "default_reasoning_summary": "auto",
                "support_verbosity": true,
                "default_verbosity": "high",
                "apply_patch_tool_type": "freeform",
                "web_search_tool_type": "text_and_image",
                "truncation_policy": {"mode": "tokens", "limit": 10000},
                "supports_parallel_tool_calls": true,
                "context_window": 272000,
                "max_context_window": 272000,
                "comp_hash": "hash",
                "effective_context_window_percent": 95,
                "experimental_supported_tools": ["future"],
                "input_modalities": ["text", "image"],
                "supports_search_tool": true,
                "use_responses_lite": true,
                "tool_mode": "code_mode_only",
                "multi_agent_version": "v2",
                "future_required_field": {"kept": true}
            }]
        }))
        .expect("base catalog")
    }

    fn profile() -> ManagedCatalogProfile {
        ManagedCatalogProfile::new(
            "grok",
            "11111111-1111-4111-8111-111111111111",
            "xAI",
            "grok-4.5",
        )
        .expect("profile")
    }

    #[test]
    fn generated_catalog_preserves_base_and_clears_unproven_capabilities() {
        let output = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
        )
        .expect("generate");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(root["future_top_level"]["kept"], json!(true));
        assert_eq!(
            root["models"][0]["future_required_field"]["kept"],
            json!(true)
        );
        let managed = &root["models"][1];
        assert_eq!(managed["slug"], json!("aio/grok"));
        assert_eq!(managed["visibility"], json!("list"));
        assert_eq!(managed["supported_reasoning_levels"], json!([]));
        assert_eq!(managed["default_reasoning_level"], Value::Null);
        assert_eq!(managed["additional_speed_tiers"], json!([]));
        assert_eq!(managed["service_tiers"], json!([]));
        assert_eq!(managed["supports_parallel_tool_calls"], json!(false));
        assert_eq!(managed["supports_search_tool"], json!(false));
        assert_eq!(managed["input_modalities"], json!(["text"]));
        assert_eq!(managed["future_required_field"]["kept"], json!(true));
        validate_owned_catalog(&output).expect("owned");
    }

    #[test]
    fn ownership_hash_detects_external_model_changes() {
        let mut output = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
        )
        .expect("generate");
        let mut root: Value = serde_json::from_slice(&output).expect("json");
        root["models"][1]["description"] = json!("externally changed");
        output = serde_json::to_vec(&root).expect("serialize");
        assert_eq!(
            validate_owned_catalog(&output)
                .expect_err("modified")
                .code(),
            "CODEX_MANAGED_MODEL_CATALOG_MODIFIED"
        );
    }

    #[test]
    fn base_catalog_alias_conflicts_fail_closed() {
        let mut base: Value = serde_json::from_slice(&base_catalog()).expect("json");
        base["models"][0]["slug"] = json!("aio/grok");
        let error = generate_catalog(
            &serde_json::to_vec(&base).expect("serialize"),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
        )
        .expect_err("conflict");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_ALIAS_CONFLICT");
    }

    #[test]
    fn config_patch_round_trips_windows_paths_and_restores_original_value() {
        let current = br#"model_provider = "aio"
[model_providers.aio]
base_url = "http://127.0.0.1:37123/v1"
"#;
        let original = br#"model_catalog_json = "D:\\Catalogs\\custom.json"
"#;
        let generated = Path::new(r"D:\AIO Data\managed-model-catalog.json");
        let patched = patch_model_catalog_config(current, Some(original), Some(generated))
            .expect("patch generated");
        let parsed = std::str::from_utf8(&patched)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(parsed["model_catalog_json"].as_str(), generated.to_str());

        let restored =
            patch_model_catalog_config(&patched, Some(original), None).expect("restore original");
        let parsed = std::str::from_utf8(&restored)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            parsed["model_catalog_json"].as_str(),
            Some(r"D:\Catalogs\custom.json")
        );
    }
}
