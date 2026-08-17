//! Maintain the complete Codex model catalog used by AIO-managed profiles.

use super::protocol;
use crate::shared::error::{db_err, AppError, AppResult};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const GENERATED_CATALOG_FILE_NAME: &str = "managed-model-catalog.json";
const GENERATED_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const USER_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_BASE_MODEL_COUNT: usize = 1_000;
const MAX_MANAGED_PROFILE_COUNT: usize = 256;
const OWNER_METADATA_KEY: &str = "_aio_managed_model_catalog";
const LEGACY_OWNER_SCHEMA_VERSION: u64 = 1;
const OWNER_SCHEMA_VERSION: u64 = 2;
const MANAGED_BY: &str = "aio-coding-hub";
pub(crate) const GPT56_372K_CONTEXT_TOKENS: u64 = 372_000;
const GPT56_372K_POLICY_VERSION: u64 = 1;
const GPT56_372K_MODEL_SLUGS: [&str; 3] = ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedCatalogProfile {
    pub(crate) profile_name_key: String,
    pub(crate) model_uuid: String,
    pub(crate) provider_name: String,
    pub(crate) remote_model_id: String,
    pub(crate) capabilities: crate::provider_models::ProviderModelCapabilities,
}

struct RawManagedCatalogProfile {
    profile_name_key: String,
    model_uuid: String,
    provider_name: String,
    remote_model_id: String,
    capabilities_configured: bool,
    supported_reasoning_efforts_json: String,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManagedCatalogPolicy {
    pub(crate) gpt56_372k_context_enabled: bool,
}

impl ManagedCatalogPolicy {
    pub(crate) fn from_settings(settings: &crate::settings::AppSettings) -> Self {
        Self {
            gpt56_372k_context_enabled: settings.codex_gpt56_372k_context_enabled,
        }
    }
}

impl ManagedCatalogProfile {
    pub(crate) fn new(
        profile_name_key: impl Into<String>,
        model_uuid: impl Into<String>,
        provider_name: impl Into<String>,
        remote_model_id: impl Into<String>,
        capabilities: crate::provider_models::ProviderModelCapabilities,
    ) -> AppResult<Self> {
        let profile = Self {
            profile_name_key: profile_name_key.into(),
            model_uuid: model_uuid.into(),
            provider_name: provider_name.into(),
            remote_model_id: remote_model_id.into(),
            capabilities,
        };
        validate_profile(&profile)?;
        Ok(profile)
    }

    fn alias(&self) -> String {
        format!("aio/{}", self.profile_name_key)
    }

    pub(crate) fn set_capabilities(
        &mut self,
        capabilities: crate::provider_models::ProviderModelCapabilities,
    ) -> AppResult<()> {
        capabilities.validate().map_err(|_| {
            AppError::new(
                "DB_INVALID_DATA",
                "managed Codex model capabilities are invalid",
            )
        })?;
        self.capabilities = capabilities;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug)]
struct PreparedCatalogChange {
    ownership: CatalogOwnershipContext,
    base_source_guard: Option<BaseCatalogGuard>,
    baseline_backup: Option<AppliedFileChange>,
    config_before: FileSnapshot,
    config_after: Vec<u8>,
    generated_before: FileSnapshot,
    generated_after: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CatalogOwnershipContext {
    ProxyApplied(crate::cli_proxy::CodexProxyBaseline),
    ProxyRestoredDirect(crate::cli_proxy::CodexProxyBaseline),
    Direct { config_path: PathBuf },
}

struct PreparedCatalogBaseline {
    catalog_path: Option<PathBuf>,
    backup_change: Option<AppliedFileChange>,
}

#[derive(Debug)]
pub(crate) struct ManagedCatalogPlan {
    change: PreparedCatalogChange,
}

#[derive(Debug, Clone)]
struct AppliedFileChange {
    before: FileSnapshot,
    after: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct AppliedManagedCatalog {
    config: Option<AppliedFileChange>,
    generated: Option<AppliedFileChange>,
    baseline_backup: Option<AppliedFileChange>,
    direction: CatalogApplyDirection,
}

impl ManagedCatalogPlan {
    pub(crate) fn apply<R: tauri::Runtime>(
        self,
        app: &tauri::AppHandle<R>,
    ) -> AppResult<AppliedManagedCatalog> {
        let change = self.change;

        if catalog_ownership_context(app)? != change.ownership {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
                "Codex catalog ownership changed while preparing the managed model catalog",
            ));
        }
        if let Some(guard) = change.base_source_guard.as_ref() {
            guard.ensure_unchanged(app)?;
        }

        ensure_snapshot_unchanged(
            &change.config_before,
            crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
        )?;
        ensure_snapshot_unchanged(&change.generated_before, GENERATED_CATALOG_MAX_BYTES)?;

        apply_prepared_catalog_files(
            change.baseline_backup,
            change.config_before,
            change.config_after,
            change.generated_before,
            change.generated_after,
        )
    }
}

fn apply_prepared_catalog_files(
    baseline_backup: Option<AppliedFileChange>,
    config_before: FileSnapshot,
    config_after: Vec<u8>,
    generated_before: FileSnapshot,
    generated_after: Option<Vec<u8>>,
) -> AppResult<AppliedManagedCatalog> {
    let direction = if generated_after.is_some() {
        CatalogApplyDirection::ActivateOrRefresh
    } else {
        CatalogApplyDirection::Deactivate
    };
    let mut applied = AppliedManagedCatalog {
        config: None,
        generated: None,
        baseline_backup: None,
        direction,
    };

    if let Some(planned) = baseline_backup.as_ref() {
        ensure_snapshot_unchanged(&planned.before, crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES)?;
        if planned.before.bytes != planned.after {
            let after = planned.after.as_deref().ok_or_else(|| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                    "the repaired Codex proxy baseline cannot be empty",
                )
            })?;
            record_catalog_apply_stage(CatalogApplyStage::Baseline);
            injected_catalog_apply_failure(CatalogApplyStage::Baseline)?;
            crate::cli_proxy::write_cli_proxy_file_atomic(&planned.before.path, after).map_err(
                |error| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CONFIG_WRITE_FAILED",
                        format!("failed to repair Codex proxy baseline: {error}"),
                    )
                },
            )?;
            applied.baseline_backup = Some(planned.clone());
        }
    }

    let result = match direction {
        CatalogApplyDirection::ActivateOrRefresh => {
            apply_generated_catalog_change(&generated_before, generated_after.as_deref())
                .map(|change| applied.generated = change)
                .and_then(|()| {
                    apply_catalog_config_change(&config_before, &config_after)
                        .map(|change| applied.config = change)
                })
        }
        CatalogApplyDirection::Deactivate => {
            apply_catalog_config_change(&config_before, &config_after)
                .map(|change| applied.config = change)
                .and_then(|()| {
                    apply_generated_catalog_change(&generated_before, generated_after.as_deref())
                        .map(|change| applied.generated = change)
                })
        }
    };
    match result {
        Ok(()) => Ok(applied),
        Err(error) => Err(rollback_catalog_apply_failure(error, applied)),
    }
}

fn apply_generated_catalog_change(
    generated_before: &FileSnapshot,
    generated_after: Option<&[u8]>,
) -> AppResult<Option<AppliedFileChange>> {
    if generated_before.bytes.as_deref() == generated_after {
        return Ok(None);
    }
    record_catalog_apply_stage(CatalogApplyStage::Generated);
    injected_catalog_apply_failure(CatalogApplyStage::Generated)?;
    apply_generated_catalog_state(&generated_before.path, generated_after)?;
    Ok(Some(AppliedFileChange {
        before: generated_before.clone(),
        after: generated_after.map(<[u8]>::to_vec),
    }))
}

fn apply_catalog_config_change(
    config_before: &FileSnapshot,
    config_after: &[u8],
) -> AppResult<Option<AppliedFileChange>> {
    if config_before.bytes.as_deref() == Some(config_after) {
        return Ok(None);
    }
    record_catalog_apply_stage(CatalogApplyStage::Config);
    injected_catalog_apply_failure(CatalogApplyStage::Config)?;
    crate::cli_proxy::write_cli_proxy_file_atomic(&config_before.path, config_after).map_err(
        |error| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_WRITE_FAILED",
                format!("failed to update Codex config.toml: {error}"),
            )
        },
    )?;
    Ok(Some(AppliedFileChange {
        before: config_before.clone(),
        after: Some(config_after.to_vec()),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogApplyDirection {
    ActivateOrRefresh,
    Deactivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogApplyStage {
    Baseline,
    Generated,
    Config,
}

#[cfg(not(test))]
fn record_catalog_apply_stage(_stage: CatalogApplyStage) {}

#[cfg(not(test))]
fn record_catalog_rollback_stage(_stage: CatalogApplyStage) {}

#[cfg(not(test))]
fn injected_catalog_apply_failure(_stage: CatalogApplyStage) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
thread_local! {
    static CATALOG_APPLY_FAILURE: std::cell::Cell<Option<CatalogApplyStage>> = const {
        std::cell::Cell::new(None)
    };
    static CATALOG_APPLY_TRACE: std::cell::RefCell<Vec<CatalogApplyStage>> = const {
        std::cell::RefCell::new(Vec::new())
    };
    static CATALOG_ROLLBACK_TRACE: std::cell::RefCell<Vec<CatalogApplyStage>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

#[cfg(test)]
fn record_catalog_apply_stage(stage: CatalogApplyStage) {
    CATALOG_APPLY_TRACE.with(|trace| trace.borrow_mut().push(stage));
}

#[cfg(test)]
fn record_catalog_rollback_stage(stage: CatalogApplyStage) {
    CATALOG_ROLLBACK_TRACE.with(|trace| trace.borrow_mut().push(stage));
}

#[cfg(test)]
fn injected_catalog_apply_failure(stage: CatalogApplyStage) -> AppResult<()> {
    let should_fail = CATALOG_APPLY_FAILURE.with(|failure| {
        if failure.get() == Some(stage) {
            failure.set(None);
            true
        } else {
            false
        }
    });
    if should_fail {
        Err(AppError::new(
            "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED",
            format!("injected {stage:?} catalog write failure"),
        ))
    } else {
        Ok(())
    }
}

impl AppliedManagedCatalog {
    pub(crate) fn rollback(self) -> AppResult<()> {
        let mut errors = Vec::new();
        match self.direction {
            CatalogApplyDirection::ActivateOrRefresh => {
                rollback_catalog_change(
                    self.config.as_ref(),
                    crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
                    Some(CatalogApplyStage::Config),
                    &mut errors,
                );
                rollback_catalog_change(
                    self.generated.as_ref(),
                    GENERATED_CATALOG_MAX_BYTES,
                    Some(CatalogApplyStage::Generated),
                    &mut errors,
                );
            }
            CatalogApplyDirection::Deactivate => {
                rollback_catalog_change(
                    self.generated.as_ref(),
                    GENERATED_CATALOG_MAX_BYTES,
                    Some(CatalogApplyStage::Generated),
                    &mut errors,
                );
                rollback_catalog_change(
                    self.config.as_ref(),
                    crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
                    Some(CatalogApplyStage::Config),
                    &mut errors,
                );
            }
        }
        rollback_catalog_change(
            self.baseline_backup.as_ref(),
            crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
            Some(CatalogApplyStage::Baseline),
            &mut errors,
        );
        if !errors.is_empty() {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                format!(
                    "failed to roll back managed catalog transaction: {}",
                    errors.join("; ")
                ),
            ));
        }
        Ok(())
    }
}

fn rollback_catalog_change(
    change: Option<&AppliedFileChange>,
    max_len: usize,
    stage: Option<CatalogApplyStage>,
    errors: &mut Vec<String>,
) {
    let Some(change) = change else {
        return;
    };
    if let Some(stage) = stage {
        record_catalog_rollback_stage(stage);
    }
    if let Err(error) = rollback_file_change(change, max_len) {
        errors.push(error.to_string());
    }
}

fn rollback_catalog_apply_failure(original: AppError, applied: AppliedManagedCatalog) -> AppError {
    match applied.rollback() {
        Ok(()) => original,
        Err(rollback_error) => AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            format!(
                "managed catalog update failed ({original}); rollback also failed: {rollback_error}"
            ),
        ),
    }
}

enum BaseCatalogSource {
    User {
        path: PathBuf,
        bytes: Vec<u8>,
        fingerprint: String,
    },
    Bundled {
        launch: crate::cli_manager::CodexLaunchSpec,
        descriptor: BundledCatalogDescriptor,
        fingerprint: String,
    },
}

#[derive(Debug, Clone)]
enum BaseCatalogGuard {
    User {
        path: PathBuf,
        bytes: Vec<u8>,
        fingerprint: String,
    },
    Bundled {
        descriptor: BundledCatalogDescriptor,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundledCatalogDescriptor {
    executable: PathBuf,
    runtime_path: OsString,
    version: Option<String>,
    executable_len: u64,
    executable_modified_nanos: u128,
}

impl BaseCatalogSource {
    fn fingerprint(&self) -> &str {
        match self {
            Self::User { fingerprint, .. } | Self::Bundled { fingerprint, .. } => fingerprint,
        }
    }

    fn guard(&self) -> BaseCatalogGuard {
        match self {
            Self::User {
                path,
                bytes,
                fingerprint,
            } => BaseCatalogGuard::User {
                path: path.clone(),
                bytes: bytes.clone(),
                fingerprint: fingerprint.clone(),
            },
            Self::Bundled { descriptor, .. } => BaseCatalogGuard::Bundled {
                descriptor: descriptor.clone(),
            },
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

impl BaseCatalogGuard {
    fn ensure_unchanged<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) -> AppResult<()> {
        match self {
            Self::User {
                path,
                bytes,
                fingerprint,
            } => ensure_user_base_catalog_unchanged(path, bytes, fingerprint),
            Self::Bundled { descriptor } => {
                let current_launch = crate::cli_manager::codex_launch_spec(app)
                    .map_err(|_| base_catalog_drift_error())?
                    .ok_or_else(base_catalog_drift_error)?;
                let current_descriptor = bundled_catalog_descriptor(&current_launch)
                    .map_err(|_| base_catalog_drift_error())?;
                if &current_descriptor != descriptor {
                    return Err(base_catalog_drift_error());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
struct OwnedCatalogMetadata {
    schema_version: u64,
    profile_set_sha256: String,
    base_source_fingerprint: String,
    projection_sha256: Option<String>,
    gpt56_372k_context_enabled: bool,
    original_catalog_path: Option<PathBuf>,
}

impl OwnedCatalogMetadata {
    fn is_legacy_v1(&self) -> bool {
        self.schema_version == LEGACY_OWNER_SCHEMA_VERSION
    }
}

#[derive(Clone, Copy)]
enum CatalogReconcileIntent<'a> {
    Background,
    ProposedConfigSave {
        previous_config: Option<&'a [u8]>,
        proposed_config: &'a [u8],
    },
}

pub(crate) fn load_profiles(conn: &Connection) -> AppResult<Vec<ManagedCatalogProfile>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT profile.profile_name_key, profile.model_uuid, provider.name, model.remote_model_id,
       model.capabilities_configured, model.supported_reasoning_efforts_json,
       model.default_reasoning_effort, model.context_window
FROM codex_managed_profiles profile
JOIN provider_models model ON model.model_uuid = profile.model_uuid
JOIN providers provider ON provider.id = model.provider_id
ORDER BY profile.profile_name_key ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare managed model catalog query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(RawManagedCatalogProfile {
                profile_name_key: row.get(0)?,
                model_uuid: row.get(1)?,
                provider_name: row.get(2)?,
                remote_model_id: row.get(3)?,
                capabilities_configured: row.get::<_, i64>(4)? != 0,
                supported_reasoning_efforts_json: row.get(5)?,
                default_reasoning_effort: row.get(6)?,
                context_window: row.get(7)?,
            })
        })
        .map_err(|error| db_err!("failed to query managed model catalog profiles: {error}"))?;
    let raw_profiles = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read managed model catalog profile: {error}"))?;
    let mut profiles = Vec::with_capacity(raw_profiles.len());
    for raw in raw_profiles {
        if !raw.capabilities_configured {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "managed Codex profile references an unconfigured model",
            ));
        }
        let capabilities = crate::provider_models::decode_stored_capabilities(
            true,
            &raw.supported_reasoning_efforts_json,
            raw.default_reasoning_effort.as_deref(),
            raw.context_window,
        )?;
        profiles.push(ManagedCatalogProfile {
            profile_name_key: raw.profile_name_key,
            model_uuid: raw.model_uuid,
            provider_name: raw.provider_name,
            remote_model_id: raw.remote_model_id,
            capabilities,
        });
    }
    validate_profiles(&profiles)?;
    Ok(profiles)
}

pub(crate) fn prepare_for_profiles<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
) -> AppResult<ManagedCatalogPlan> {
    let settings = crate::settings::read(app)?;
    prepare_for_profiles_with_policy(
        app,
        profiles,
        ManagedCatalogPolicy::from_settings(&settings),
    )
}

pub(crate) fn prepare_for_profiles_with_policy<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
    policy: ManagedCatalogPolicy,
) -> AppResult<ManagedCatalogPlan> {
    prepare_for_profiles_with_policy_and_intent(
        app,
        profiles,
        policy,
        CatalogReconcileIntent::Background,
    )
}

fn prepare_for_profiles_with_policy_and_intent<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
    policy: ManagedCatalogPolicy,
    intent: CatalogReconcileIntent<'_>,
) -> AppResult<ManagedCatalogPlan> {
    validate_profiles(profiles)?;
    let ownership = catalog_ownership_context(app)?;

    let generated_path = managed_catalog_path(app)?;
    let generated_before = snapshot_generated_file(&generated_path)?;
    let existing_metadata = generated_before
        .bytes
        .as_deref()
        .map(validate_owned_catalog)
        .transpose()?;

    let (baseline_backup, config_before, current_config_bytes, original_catalog_path) =
        match &ownership {
            CatalogOwnershipContext::ProxyApplied(baseline)
            | CatalogOwnershipContext::ProxyRestoredDirect(baseline) => {
                let prepared_baseline = prepare_catalog_baseline(
                    baseline,
                    &generated_path,
                    existing_metadata.as_ref(),
                )?;
                let config_before = snapshot_cli_proxy_file(&baseline.config_path)?;
                validate_config_save_snapshot(intent, &config_before)?;
                let current = match intent {
                    CatalogReconcileIntent::ProposedConfigSave {
                        proposed_config, ..
                    } => proposed_config.to_vec(),
                    CatalogReconcileIntent::Background => match &ownership {
                        CatalogOwnershipContext::ProxyApplied(_) => {
                            config_before.bytes.clone().ok_or_else(|| {
                                AppError::new(
                                    "CODEX_MANAGED_MODEL_CONFIG_MISSING",
                                    "Codex config.toml disappeared while the CLI proxy was enabled",
                                )
                            })?
                        }
                        CatalogOwnershipContext::ProxyRestoredDirect(_) => {
                            config_before.bytes.clone().unwrap_or_default()
                        }
                        CatalogOwnershipContext::Direct { .. } => unreachable!(),
                    },
                };
                (
                    prepared_baseline.backup_change,
                    config_before,
                    current,
                    prepared_baseline.catalog_path,
                )
            }
            CatalogOwnershipContext::Direct { config_path } => {
                let config_before = snapshot_cli_proxy_file(config_path)?;
                validate_config_save_snapshot(intent, &config_before)?;
                let current = match intent {
                    CatalogReconcileIntent::Background => {
                        config_before.bytes.clone().unwrap_or_default()
                    }
                    CatalogReconcileIntent::ProposedConfigSave {
                        proposed_config, ..
                    } => proposed_config.to_vec(),
                };
                let current_catalog_path = parse_catalog_path(Some(&current), "current")?;
                let original = match intent {
                    CatalogReconcileIntent::Background => direct_original_catalog_path(
                        current_catalog_path.as_deref(),
                        &generated_path,
                        existing_metadata.as_ref(),
                    )?,
                    CatalogReconcileIntent::ProposedConfigSave {
                        previous_config,
                        proposed_config,
                    } => explicit_save_original_catalog_path(
                        previous_config,
                        proposed_config,
                        &generated_path,
                        existing_metadata.as_ref(),
                    )?,
                };
                (None, config_before, current, original)
            }
        };
    let current_config = current_config_bytes.as_slice();
    validate_current_catalog_binding(
        current_config,
        original_catalog_path.as_deref(),
        &generated_path,
    )?;

    let needs_generated_catalog = !profiles.is_empty() || policy.gpt56_372k_context_enabled;
    let (generated_after, base_source_guard) = if !needs_generated_catalog {
        (None, None)
    } else {
        let profile_set_sha256 = profile_set_sha256(profiles)?;
        let source = base_catalog_source(app, original_catalog_path.as_deref())?;
        let source_guard = source.guard();
        let expected_projection_sha256 = projection_sha256(
            &profile_set_sha256,
            source.fingerprint(),
            policy.gpt56_372k_context_enabled,
            original_catalog_path.as_deref(),
        )?;
        if existing_metadata.as_ref().is_some_and(|metadata| {
            metadata.schema_version == OWNER_SCHEMA_VERSION
                && metadata.profile_set_sha256 == profile_set_sha256
                && metadata.base_source_fingerprint == source.fingerprint()
                && metadata.projection_sha256.as_deref()
                    == Some(expected_projection_sha256.as_str())
                && metadata.gpt56_372k_context_enabled == policy.gpt56_372k_context_enabled
                && metadata.original_catalog_path == original_catalog_path
        }) {
            (generated_before.bytes.clone(), Some(source_guard))
        } else {
            let source_fingerprint = source.fingerprint().to_string();
            let base_bytes = source.load(app)?;
            (
                Some(generate_catalog(
                    &base_bytes,
                    profiles,
                    &profile_set_sha256,
                    &source_fingerprint,
                    policy.gpt56_372k_context_enabled,
                    original_catalog_path.as_deref(),
                )?),
                Some(source_guard),
            )
        }
    };

    let desired_catalog_path = needs_generated_catalog.then_some(generated_path.as_path());
    let config_after = patch_model_catalog_config_with_original_path(
        current_config,
        original_catalog_path.as_deref(),
        desired_catalog_path,
    )?;

    Ok(ManagedCatalogPlan {
        change: PreparedCatalogChange {
            ownership,
            base_source_guard,
            baseline_backup,
            config_before,
            config_after,
            generated_before,
            generated_after,
        },
    })
}

fn validate_config_save_snapshot(
    intent: CatalogReconcileIntent<'_>,
    config_before: &FileSnapshot,
) -> AppResult<()> {
    let CatalogReconcileIntent::ProposedConfigSave {
        previous_config, ..
    } = intent
    else {
        return Ok(());
    };
    if config_before.bytes.as_deref() != previous_config {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "Codex config.toml changed while preparing the explicit config save",
        ));
    }
    Ok(())
}

fn direct_original_catalog_path(
    current_catalog_path: Option<&Path>,
    generated_path: &Path,
    existing_metadata: Option<&OwnedCatalogMetadata>,
) -> AppResult<Option<PathBuf>> {
    let current_is_generated =
        current_catalog_path.is_some_and(|path| catalog_paths_match(path, generated_path));
    let Some(metadata) = existing_metadata else {
        if current_is_generated {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "the active AIO Codex catalog has no recoverable ownership metadata",
            ));
        }
        return Ok(current_catalog_path.map(Path::to_path_buf));
    };
    if metadata.is_legacy_v1() {
        if current_is_generated {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "a legacy AIO Codex catalog cannot recover its original direct-mode binding",
            ));
        }
        // Older releases could leave an inactive v1 generated file behind after
        // restoring direct config. Its metadata lacks an original binding, but
        // the unbound current config is already the authoritative base.
        return Ok(current_catalog_path.map(Path::to_path_buf));
    }
    if current_is_generated || current_catalog_path == metadata.original_catalog_path.as_deref() {
        return Ok(metadata.original_catalog_path.clone());
    }
    Err(AppError::new(
        "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
        "model_catalog_json changed outside AIO while the managed catalog was active",
    ))
}

fn explicit_save_original_catalog_path(
    previous_config: Option<&[u8]>,
    committed_config: &[u8],
    generated_path: &Path,
    existing_metadata: Option<&OwnedCatalogMetadata>,
) -> AppResult<Option<PathBuf>> {
    let previous_catalog_path = parse_catalog_path(previous_config, "previous")?;
    let previous_original = direct_original_catalog_path(
        previous_catalog_path.as_deref(),
        generated_path,
        existing_metadata,
    )?;
    let committed_catalog_path = parse_catalog_path(Some(committed_config), "committed")?;
    if committed_catalog_path
        .as_deref()
        .is_some_and(|path| catalog_paths_match(path, generated_path))
    {
        if existing_metadata.is_none() {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "an explicit Codex config save referenced an unowned AIO catalog",
            ));
        }
        return Ok(previous_original);
    }
    Ok(committed_catalog_path)
}

fn catalog_ownership_context<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CatalogOwnershipContext> {
    if let Some(baseline) = crate::cli_proxy::codex_enabled_proxy_baseline(app)? {
        if crate::cli_proxy::codex_proxy_config_is_applied(app, &baseline.base_origin) {
            return Ok(CatalogOwnershipContext::ProxyApplied(baseline));
        }
        return Ok(CatalogOwnershipContext::ProxyRestoredDirect(baseline));
    }
    Ok(CatalogOwnershipContext::Direct {
        config_path: crate::codex_paths::codex_config_toml_path(app)?,
    })
}

pub(crate) fn sync_current_locked<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<()> {
    #[cfg(test)]
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
    let db = crate::db::init(app)?;
    let conn = db.open_connection()?;
    let profiles = load_profiles(&conn)?;
    let _applied = prepare_for_profiles(app, &profiles)?.apply(app)?;
    Ok(())
}

pub(crate) fn sync_current_after_config_save_locked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_config: Option<&[u8]>,
    proposed_config: &[u8],
) -> AppResult<AppliedManagedCatalog> {
    #[cfg(test)]
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
    let db = crate::db::init(app)?;
    let conn = db.open_connection()?;
    let profiles = load_profiles(&conn)?;
    let settings = crate::settings::read(app)?;
    let policy = ManagedCatalogPolicy::from_settings(&settings);
    prepare_for_profiles_with_policy_and_intent(
        app,
        &profiles,
        policy,
        CatalogReconcileIntent::ProposedConfigSave {
            previous_config,
            proposed_config,
        },
    )?
    .apply(app)
}

pub(crate) fn preserve_active_binding<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    baseline: Option<&[u8]>,
    current: Option<&[u8]>,
    projected: &[u8],
) -> AppResult<Vec<u8>> {
    let generated = managed_catalog_path(app)?;
    let original = parse_original_catalog_path(baseline)?;
    let current_path = parse_catalog_path(current, "current")?;
    validate_current_catalog_binding(current.unwrap_or_default(), original.as_deref(), &generated)?;
    let generated_path =
        (current_path.as_deref() == Some(generated.as_path())).then_some(generated.as_path());
    patch_model_catalog_config(projected, baseline, generated_path)
}

#[cfg(test)]
thread_local! {
    static OAUTH_SYNC_CATALOG_INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_sync_current_invocations_for_test() {
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn sync_current_invocations_for_test() -> usize {
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(std::cell::Cell::get)
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
    profile.capabilities.validate().map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile capabilities are invalid",
        )
    })?;
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

fn prepare_catalog_baseline(
    baseline: &crate::cli_proxy::CodexProxyBaseline,
    generated_path: &Path,
    existing_metadata: Option<&OwnedCatalogMetadata>,
) -> AppResult<PreparedCatalogBaseline> {
    let configured_catalog_path = parse_original_catalog_path(baseline.config_bytes.as_deref())?;
    if !configured_catalog_path
        .as_deref()
        .is_some_and(|path| catalog_paths_match(path, generated_path))
    {
        if let Some(path) = configured_catalog_path.as_deref() {
            reject_generated_path_as_base(path, generated_path)?;
        }
        return Ok(PreparedCatalogBaseline {
            catalog_path: configured_catalog_path,
            backup_change: None,
        });
    }

    let backup_path = baseline.config_backup_path.as_ref().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the polluted Codex proxy baseline has no recoverable backup path",
        )
    })?;
    let before = snapshot_cli_proxy_file(backup_path)?;
    if before.bytes != baseline.config_bytes {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "the Codex proxy baseline changed while preparing catalog repair",
        ));
    }
    let current = before.bytes.as_deref().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the polluted Codex proxy baseline disappeared",
        )
    })?;
    let recovered_catalog_path = existing_metadata
        .filter(|metadata| !metadata.is_legacy_v1())
        .and_then(|metadata| metadata.original_catalog_path.clone());
    let repaired = patch_model_catalog_config_with_original_path(
        current,
        recovered_catalog_path.as_deref(),
        None,
    )?;
    Ok(PreparedCatalogBaseline {
        catalog_path: recovered_catalog_path,
        backup_change: Some(AppliedFileChange {
            before,
            after: Some(repaired),
        }),
    })
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
            "model_catalog_json changed outside AIO while the managed catalog was active",
        ));
    }
    Ok(())
}

fn patch_model_catalog_config(
    current: &[u8],
    original: Option<&[u8]>,
    generated_path: Option<&Path>,
) -> AppResult<Vec<u8>> {
    let original_path = parse_original_catalog_path(original)?;
    patch_model_catalog_config_with_original_path(current, original_path.as_deref(), generated_path)
}

fn patch_model_catalog_config_with_original_path(
    current: &[u8],
    original_path: Option<&Path>,
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
        None => original_path.and_then(|path| path.to_str().map(str::to_string)),
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
    if catalog_paths_match(base, generated) {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the AIO-generated catalog cannot be used as its own base catalog",
        ));
    }
    Ok(())
}

fn catalog_paths_match(base: &Path, generated: &Path) -> bool {
    if base == generated {
        true
    } else {
        match (
            std::fs::canonicalize(base),
            std::fs::canonicalize(generated),
        ) {
            (Ok(base), Ok(generated)) => base == generated,
            _ => false,
        }
    }
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
        let fingerprint = user_catalog_fingerprint(path, &bytes);
        return Ok(BaseCatalogSource::User {
            path: path.to_path_buf(),
            bytes,
            fingerprint,
        });
    }

    let launch = crate::cli_manager::codex_launch_spec(app)?.ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CLI_NOT_FOUND",
            "Codex CLI was not found",
        )
    })?;
    let descriptor = bundled_catalog_descriptor(&launch)?;
    let fingerprint = bundled_catalog_fingerprint(&descriptor);
    Ok(BaseCatalogSource::Bundled {
        launch,
        descriptor,
        fingerprint,
    })
}

fn user_catalog_fingerprint(path: &Path, bytes: &[u8]) -> String {
    sha256_hex(
        &[
            b"user\0".as_slice(),
            path.to_string_lossy().as_bytes(),
            b"\0",
            sha256_hex(bytes).as_bytes(),
        ]
        .concat(),
    )
}

fn ensure_user_base_catalog_unchanged(
    path: &Path,
    expected_bytes: &[u8],
    expected_fingerprint: &str,
) -> AppResult<()> {
    let current = crate::shared::fs::read_file_with_max_len(path, USER_CATALOG_MAX_BYTES)
        .map_err(|_| base_catalog_drift_error())?;
    if current != expected_bytes || user_catalog_fingerprint(path, &current) != expected_fingerprint
    {
        return Err(base_catalog_drift_error());
    }
    Ok(())
}

fn bundled_catalog_descriptor(
    launch: &crate::cli_manager::CodexLaunchSpec,
) -> AppResult<BundledCatalogDescriptor> {
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
    Ok(BundledCatalogDescriptor {
        executable: launch.executable.clone(),
        runtime_path: launch.runtime_path.clone(),
        version: launch.version.clone(),
        executable_len: metadata.len(),
        executable_modified_nanos: modified,
    })
}

fn bundled_catalog_fingerprint(descriptor: &BundledCatalogDescriptor) -> String {
    let payload = format!(
        "bundled\0{}\0{}\0{}\0{}\0{}",
        descriptor.executable.to_string_lossy(),
        descriptor.runtime_path.to_string_lossy(),
        descriptor.version.as_deref().unwrap_or(""),
        descriptor.executable_len,
        descriptor.executable_modified_nanos,
    );
    sha256_hex(payload.as_bytes())
}

fn base_catalog_drift_error() -> AppError {
    AppError::new(
        "CODEX_MANAGED_MODEL_BASE_CATALOG_DRIFT",
        "the Codex base model catalog changed while preparing the managed catalog",
    )
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
                "supported_reasoning_efforts": profile.capabilities.supported_reasoning_efforts
                    .iter()
                    .map(|effort| effort.as_str())
                    .collect::<Vec<_>>(),
                "default_reasoning_effort": profile.capabilities.default_reasoning_effort
                    .map(crate::provider_models::ProviderModelReasoningEffort::as_str),
                "context_window": profile.capabilities.context_window,
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

fn projection_sha256(
    profile_set_sha256: &str,
    base_source_fingerprint: &str,
    gpt56_372k_context_enabled: bool,
    original_catalog_path: Option<&Path>,
) -> AppResult<String> {
    let original_catalog_path = catalog_path_string(original_catalog_path)?;
    let bytes = serde_json::to_vec(&json!({
        "schema_version": OWNER_SCHEMA_VERSION,
        "profile_set_sha256": profile_set_sha256,
        "base_source_fingerprint": base_source_fingerprint,
        "original_catalog_path": original_catalog_path,
        "gpt56_372k_policy_version": GPT56_372K_POLICY_VERSION,
        "gpt56_372k_context_enabled": gpt56_372k_context_enabled,
        "gpt56_372k_context_tokens": GPT56_372K_CONTEXT_TOKENS,
        "gpt56_372k_model_slugs": GPT56_372K_MODEL_SLUGS,
    }))
    .map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to hash the managed Codex catalog projection",
        )
    })?;
    Ok(sha256_hex(&bytes))
}

fn catalog_path_string(path: Option<&Path>) -> AppResult<Option<String>> {
    path.map(|path| {
        path.to_str().map(str::to_string).ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                "model_catalog_json path must be valid UTF-8",
            )
        })
    })
    .transpose()
}

fn generate_catalog(
    base_bytes: &[u8],
    profiles: &[ManagedCatalogProfile],
    profile_set_sha256: &str,
    base_source_fingerprint: &str,
    gpt56_372k_context_enabled: bool,
    original_catalog_path: Option<&Path>,
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
            if gpt56_372k_context_enabled && GPT56_372K_MODEL_SLUGS.contains(&slug) {
                return Err(AppError::new(
                    "CODEX_GPT56_372K_MODELS_MISSING",
                    "the Codex model catalog must contain exactly one of every supported GPT-5.6 model",
                ));
            }
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
    let template = if profiles.is_empty() {
        None
    } else {
        Some(template.ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "the base Codex model catalog has no visible template model",
            )
        })?)
    };

    if gpt56_372k_context_enabled {
        apply_gpt56_372k_context_policy(models)?;
    }

    for (index, profile) in profiles.iter().enumerate() {
        let alias = profile.alias();
        if !slugs.insert(alias.clone()) {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_ALIAS_CONFLICT",
                format!("the base Codex model catalog already contains {alias}"),
            ));
        }
        models.push(Value::Object(build_managed_model(
            template.as_ref().expect("profiles require a template"),
            profile,
            index,
        )));
    }

    let base_catalog_sha256 = sha256_hex(base_bytes);
    let aliases = profiles
        .iter()
        .map(ManagedCatalogProfile::alias)
        .collect::<Vec<_>>();
    let projection_sha256 = projection_sha256(
        profile_set_sha256,
        base_source_fingerprint,
        gpt56_372k_context_enabled,
        original_catalog_path,
    )?;
    let original_catalog_path = catalog_path_string(original_catalog_path)?;
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
            "projection_sha256": projection_sha256,
            "gpt56_372k_policy_version": GPT56_372K_POLICY_VERSION,
            "gpt56_372k_context_enabled": gpt56_372k_context_enabled,
            "gpt56_372k_context_tokens": GPT56_372K_CONTEXT_TOKENS,
            "gpt56_372k_model_slugs": GPT56_372K_MODEL_SLUGS,
            "original_catalog_path": original_catalog_path,
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
                "projection_sha256": projection_sha256,
                "gpt56_372k_policy_version": GPT56_372K_POLICY_VERSION,
                "gpt56_372k_context_enabled": gpt56_372k_context_enabled,
                "gpt56_372k_context_tokens": GPT56_372K_CONTEXT_TOKENS,
                "gpt56_372k_model_slugs": GPT56_372K_MODEL_SLUGS,
                "original_catalog_path": original_catalog_path,
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

fn apply_gpt56_372k_context_policy(models: &mut [Value]) -> AppResult<()> {
    let mut matched = HashSet::with_capacity(GPT56_372K_MODEL_SLUGS.len());
    for model in models {
        let object = model.as_object_mut().ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "every base Codex model must be an object",
            )
        })?;
        let Some(slug) = object
            .get("slug")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if !GPT56_372K_MODEL_SLUGS.contains(&slug.as_str()) {
            continue;
        }
        if object
            .get("context_window")
            .and_then(Value::as_u64)
            .is_none()
            || object
                .get("max_context_window")
                .and_then(Value::as_u64)
                .is_none()
        {
            return Err(AppError::new(
                "CODEX_GPT56_372K_CATALOG_INVALID",
                format!("Codex model {slug} has invalid context window fields"),
            ));
        }
        object.insert(
            "context_window".to_string(),
            json!(GPT56_372K_CONTEXT_TOKENS),
        );
        object.insert(
            "max_context_window".to_string(),
            json!(GPT56_372K_CONTEXT_TOKENS),
        );
        matched.insert(slug);
    }
    if matched.len() != GPT56_372K_MODEL_SLUGS.len() {
        return Err(AppError::new(
            "CODEX_GPT56_372K_MODELS_MISSING",
            "the Codex model catalog does not contain all supported GPT-5.6 models",
        ));
    }
    Ok(())
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
    let reasoning_levels = profile
        .capabilities
        .supported_reasoning_efforts
        .iter()
        .map(|effort| {
            json!({
                "effort": effort.as_str(),
                "description": reasoning_effort_description(*effort),
            })
        })
        .collect::<Vec<_>>();
    model.insert(
        "default_reasoning_level".to_string(),
        profile
            .capabilities
            .default_reasoning_effort
            .map(|effort| json!(effort.as_str()))
            .unwrap_or(Value::Null),
    );
    model.insert(
        "supported_reasoning_levels".to_string(),
        json!(reasoning_levels),
    );
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
    model.insert(
        "supports_reasoning_summaries".to_string(),
        json!(!profile.capabilities.supported_reasoning_efforts.is_empty()),
    );
    model.insert("default_reasoning_summary".to_string(), json!("none"));
    model.insert("support_verbosity".to_string(), json!(false));
    model.insert("default_verbosity".to_string(), Value::Null);
    model.insert("apply_patch_tool_type".to_string(), Value::Null);
    model.insert("web_search_tool_type".to_string(), json!("text"));
    model.insert("supports_parallel_tool_calls".to_string(), json!(false));
    model.insert("supports_image_detail_original".to_string(), json!(false));
    let context_window = profile
        .capabilities
        .context_window
        .map(Value::from)
        .unwrap_or(Value::Null);
    model.insert("context_window".to_string(), context_window.clone());
    model.insert("max_context_window".to_string(), context_window);
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

fn reasoning_effort_description(
    effort: crate::provider_models::ProviderModelReasoningEffort,
) -> &'static str {
    use crate::provider_models::ProviderModelReasoningEffort as Effort;
    match effort {
        Effort::None => "No additional reasoning",
        Effort::Minimal => "Minimal reasoning",
        Effort::Low => "Light reasoning",
        Effort::Medium => "Balanced reasoning",
        Effort::High => "Deep reasoning",
        Effort::XHigh => "Very deep reasoning",
        Effort::Max => "Maximum reasoning",
        Effort::Ultra => "Ultra reasoning",
    }
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
    let schema_version = metadata
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(modified_catalog_error)?;
    if !matches!(
        schema_version,
        LEGACY_OWNER_SCHEMA_VERSION | OWNER_SCHEMA_VERSION
    ) || metadata.get("managed_by").and_then(Value::as_str) != Some(MANAGED_BY)
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

    if schema_version == LEGACY_OWNER_SCHEMA_VERSION {
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
        return Ok(OwnedCatalogMetadata {
            schema_version,
            profile_set_sha256: profile_set_sha256.to_string(),
            base_source_fingerprint: base_source_fingerprint.to_string(),
            projection_sha256: None,
            gpt56_372k_context_enabled: false,
            original_catalog_path: None,
        });
    }

    let projection_sha256_value = required_metadata_string(metadata, "projection_sha256")?;
    if metadata
        .get("gpt56_372k_policy_version")
        .and_then(Value::as_u64)
        != Some(GPT56_372K_POLICY_VERSION)
        || metadata
            .get("gpt56_372k_context_tokens")
            .and_then(Value::as_u64)
            != Some(GPT56_372K_CONTEXT_TOKENS)
        || metadata.get("gpt56_372k_model_slugs") != Some(&json!(GPT56_372K_MODEL_SLUGS))
    {
        return Err(modified_catalog_error());
    }
    let gpt56_372k_context_enabled = metadata
        .get("gpt56_372k_context_enabled")
        .and_then(Value::as_bool)
        .ok_or_else(modified_catalog_error)?;
    let original_catalog_path = match metadata.get("original_catalog_path") {
        Some(Value::Null) => None,
        Some(Value::String(value)) => {
            let path = PathBuf::from(value);
            if value.is_empty() || !path.is_absolute() {
                return Err(modified_catalog_error());
            }
            Some(path)
        }
        _ => return Err(modified_catalog_error()),
    };
    let expected_projection_sha256 = projection_sha256(
        profile_set_sha256,
        base_source_fingerprint,
        gpt56_372k_context_enabled,
        original_catalog_path.as_deref(),
    )
    .map_err(|_| modified_catalog_error())?;
    if projection_sha256_value != expected_projection_sha256 {
        return Err(modified_catalog_error());
    }
    let expected_payload_sha256 = sha256_hex(
        &serde_json::to_vec(&json!({
            "catalog": payload_root,
            "profile_set_sha256": profile_set_sha256,
            "base_catalog_sha256": base_catalog_sha256,
            "base_source_fingerprint": base_source_fingerprint,
            "managed_aliases": aliases,
            "projection_sha256": projection_sha256_value,
            "gpt56_372k_policy_version": GPT56_372K_POLICY_VERSION,
            "gpt56_372k_context_enabled": gpt56_372k_context_enabled,
            "gpt56_372k_context_tokens": GPT56_372K_CONTEXT_TOKENS,
            "gpt56_372k_model_slugs": GPT56_372K_MODEL_SLUGS,
            "original_catalog_path": original_catalog_path.as_ref().and_then(|path| path.to_str()),
        }))
        .map_err(|_| modified_catalog_error())?,
    );
    if payload_sha256 != expected_payload_sha256 {
        return Err(modified_catalog_error());
    }
    Ok(OwnedCatalogMetadata {
        schema_version,
        profile_set_sha256: profile_set_sha256.to_string(),
        base_source_fingerprint: base_source_fingerprint.to_string(),
        projection_sha256: Some(projection_sha256_value.to_string()),
        gpt56_372k_context_enabled,
        original_catalog_path,
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

    fn gpt56_base_catalog(context_window: u64) -> Vec<u8> {
        let mut models = GPT56_372K_MODEL_SLUGS
            .iter()
            .enumerate()
            .map(|(index, slug)| {
                json!({
                    "slug": slug,
                    "display_name": slug,
                    "description": "base GPT-5.6",
                    "visibility": if index == 0 { "list" } else { "hide" },
                    "context_window": context_window,
                    "max_context_window": context_window,
                    "auto_compact_token_limit": null,
                    "effective_context_window_percent": 95,
                    "future_model_field": {"kept": slug},
                })
            })
            .collect::<Vec<_>>();
        models.extend([
            json!({
                "slug": "gpt-5.6-future",
                "visibility": "hide",
                "context_window": 280000,
                "max_context_window": 280000,
                "future_model_field": {"kept": true},
            }),
            json!({
                "slug": "gpt-base",
                "visibility": "hide",
                "context_window": 272000,
                "max_context_window": 272000,
            }),
            json!({
                "slug": "aio/existing",
                "visibility": "hide",
                "context_window": 512000,
                "max_context_window": 512000,
                "aio_capability": {"kept": true},
            }),
        ]);
        serde_json::to_vec(&json!({
            "future_top_level": {"kept": true},
            "models": models,
        }))
        .expect("GPT-5.6 base catalog")
    }

    fn model_by_slug<'a>(root: &'a Value, slug: &str) -> &'a Value {
        root["models"]
            .as_array()
            .expect("models")
            .iter()
            .find(|model| model["slug"].as_str() == Some(slug))
            .expect("model slug")
    }

    fn config_with_catalog(path: Option<&Path>) -> Vec<u8> {
        let mut document = toml_edit::DocumentMut::new();
        document["model"] = toml_edit::value("gpt-5.6-sol");
        if let Some(path) = path {
            document["model_catalog_json"] = toml_edit::value(path.to_str().expect("UTF-8 path"));
        }
        document.to_string().into_bytes()
    }

    fn profile() -> ManagedCatalogProfile {
        ManagedCatalogProfile::new(
            "grok",
            "11111111-1111-4111-8111-111111111111",
            "xAI",
            "grok-4.5",
            crate::provider_models::ProviderModelCapabilities {
                supported_reasoning_efforts: vec![
                    crate::provider_models::ProviderModelReasoningEffort::Low,
                    crate::provider_models::ProviderModelReasoningEffort::Medium,
                    crate::provider_models::ProviderModelReasoningEffort::High,
                ],
                default_reasoning_effort: Some(
                    crate::provider_models::ProviderModelReasoningEffort::Medium,
                ),
                context_window: Some(128_000),
            },
        )
        .expect("profile")
    }

    fn legacy_v1_catalog() -> Vec<u8> {
        let current = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            None,
        )
        .expect("generate v2 fixture source");
        let mut root: Value = serde_json::from_slice(&current).expect("json");
        let metadata = root[OWNER_METADATA_KEY].clone();
        root.as_object_mut()
            .expect("catalog object")
            .remove(OWNER_METADATA_KEY);
        let payload_sha256 = sha256_hex(
            &serde_json::to_vec(&json!({
                "catalog": root,
                "profile_set_sha256": metadata["profile_set_sha256"],
                "base_catalog_sha256": metadata["base_catalog_sha256"],
                "base_source_fingerprint": metadata["base_source_fingerprint"],
                "managed_aliases": metadata["managed_aliases"],
            }))
            .expect("legacy payload"),
        );
        root.as_object_mut().expect("catalog object").insert(
            OWNER_METADATA_KEY.to_string(),
            json!({
                "schema_version": LEGACY_OWNER_SCHEMA_VERSION,
                "managed_by": MANAGED_BY,
                "payload_sha256": payload_sha256,
                "profile_set_sha256": metadata["profile_set_sha256"],
                "base_catalog_sha256": metadata["base_catalog_sha256"],
                "base_source_fingerprint": metadata["base_source_fingerprint"],
                "managed_aliases": metadata["managed_aliases"],
            }),
        );
        let mut output = serde_json::to_vec_pretty(&root).expect("legacy catalog");
        output.push(b'\n');
        output
    }

    #[test]
    fn generated_catalog_preserves_base_and_sets_managed_reasoning_capabilities() {
        let output = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            None,
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
        assert_eq!(
            managed["supported_reasoning_levels"],
            json!([
                {
                    "effort": "low",
                    "description": "Light reasoning"
                },
                {
                    "effort": "medium",
                    "description": "Balanced reasoning"
                },
                {
                    "effort": "high",
                    "description": "Deep reasoning"
                }
            ])
        );
        assert_eq!(managed["default_reasoning_level"], json!("medium"));
        assert_eq!(managed["supports_reasoning_summaries"], json!(true));
        assert_eq!(managed["context_window"], json!(128_000));
        assert_eq!(managed["max_context_window"], json!(128_000));
        assert_eq!(managed["auto_compact_token_limit"], Value::Null);
        assert_eq!(managed["additional_speed_tiers"], json!([]));
        assert_eq!(managed["service_tiers"], json!([]));
        assert_eq!(managed["supports_parallel_tool_calls"], json!(false));
        assert_eq!(managed["supports_search_tool"], json!(false));
        assert_eq!(managed["input_modalities"], json!(["text"]));
        assert_eq!(managed["future_required_field"]["kept"], json!(true));
        validate_owned_catalog(&output).expect("owned");
    }

    #[test]
    fn gpt56_policy_rewrites_only_exact_targets_and_preserves_aio_capabilities() {
        let original_catalog = std::env::temp_dir().join("codex-user-models.json");
        let output = generate_catalog(
            &gpt56_base_catalog(272_000),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            Some(&original_catalog),
        )
        .expect("generate 372K catalog");
        let root: Value = serde_json::from_slice(&output).expect("json");

        for slug in GPT56_372K_MODEL_SLUGS {
            let model = model_by_slug(&root, slug);
            assert_eq!(model["context_window"], json!(372_000));
            assert_eq!(model["max_context_window"], json!(372_000));
            assert_eq!(model["effective_context_window_percent"], json!(95));
            assert_eq!(model["auto_compact_token_limit"], Value::Null);
            assert_eq!(model["future_model_field"]["kept"], json!(slug));
        }
        assert_eq!(
            model_by_slug(&root, "gpt-5.6-future")["context_window"],
            json!(280_000)
        );
        assert_eq!(
            model_by_slug(&root, "aio/existing")["context_window"],
            json!(512_000)
        );
        assert_eq!(
            model_by_slug(&root, "aio/existing")["aio_capability"]["kept"],
            json!(true)
        );
        assert_eq!(
            model_by_slug(&root, "aio/grok")["context_window"],
            json!(128_000)
        );
        assert_eq!(root["future_top_level"]["kept"], json!(true));

        let metadata = &root[OWNER_METADATA_KEY];
        assert_eq!(metadata["schema_version"], json!(OWNER_SCHEMA_VERSION));
        assert_eq!(
            metadata["gpt56_372k_policy_version"],
            json!(GPT56_372K_POLICY_VERSION)
        );
        assert_eq!(
            metadata["gpt56_372k_context_tokens"],
            json!(GPT56_372K_CONTEXT_TOKENS)
        );
        assert_eq!(
            metadata["gpt56_372k_model_slugs"],
            json!(GPT56_372K_MODEL_SLUGS)
        );
        assert_eq!(metadata["gpt56_372k_context_enabled"], json!(true));
        assert_eq!(
            metadata["original_catalog_path"],
            json!(original_catalog.to_str().expect("UTF-8 path"))
        );
        validate_owned_catalog(&output).expect("owned catalog");
    }

    #[test]
    fn decimal_380928_is_not_treated_as_the_372k_policy_value() {
        let base = gpt56_base_catalog(380_928);
        let output = generate_catalog(
            &base,
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            None,
        )
        .expect("generate policy-off catalog");
        let root: Value = serde_json::from_slice(&output).expect("json");
        for slug in GPT56_372K_MODEL_SLUGS {
            let model = model_by_slug(&root, slug);
            assert_eq!(model["context_window"], json!(380_928));
            assert_eq!(model["max_context_window"], json!(380_928));
        }
        assert_eq!(
            root[OWNER_METADATA_KEY]["gpt56_372k_context_enabled"],
            json!(false)
        );
        assert_eq!(
            root[OWNER_METADATA_KEY]["gpt56_372k_context_tokens"],
            json!(372_000)
        );

        let enabled = generate_catalog(
            &base,
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect("generate policy-on catalog");
        let enabled: Value = serde_json::from_slice(&enabled).expect("json");
        for slug in GPT56_372K_MODEL_SLUGS {
            assert_eq!(
                model_by_slug(&enabled, slug)["context_window"],
                json!(372_000)
            );
        }
    }

    #[test]
    fn gpt56_policy_fails_closed_for_missing_duplicate_or_invalid_targets() {
        let mut missing: Value =
            serde_json::from_slice(&gpt56_base_catalog(272_000)).expect("json");
        missing["models"]
            .as_array_mut()
            .expect("models")
            .retain(|model| model["slug"].as_str() != Some("gpt-5.6-luna"));
        let error = generate_catalog(
            &serde_json::to_vec(&missing).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect_err("missing target must fail");
        assert_eq!(error.code(), "CODEX_GPT56_372K_MODELS_MISSING");

        let mut duplicate: Value =
            serde_json::from_slice(&gpt56_base_catalog(272_000)).expect("json");
        let duplicate_model = model_by_slug(&duplicate, "gpt-5.6-sol").clone();
        duplicate["models"]
            .as_array_mut()
            .expect("models")
            .push(duplicate_model);
        let error = generate_catalog(
            &serde_json::to_vec(&duplicate).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect_err("duplicate target must fail");
        assert_eq!(error.code(), "CODEX_GPT56_372K_MODELS_MISSING");

        let mut invalid: Value =
            serde_json::from_slice(&gpt56_base_catalog(272_000)).expect("json");
        let invalid_model = invalid["models"]
            .as_array_mut()
            .expect("models")
            .iter_mut()
            .find(|model| model["slug"].as_str() == Some("gpt-5.6-terra"))
            .expect("target");
        invalid_model["max_context_window"] = json!("272000");
        let error = generate_catalog(
            &serde_json::to_vec(&invalid).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect_err("invalid target field must fail");
        assert_eq!(error.code(), "CODEX_GPT56_372K_CATALOG_INVALID");
    }

    #[test]
    fn gpt56_policy_keeps_non_target_duplicate_as_base_catalog_error() {
        let mut duplicate: Value =
            serde_json::from_slice(&gpt56_base_catalog(272_000)).expect("json");
        let duplicate_model = model_by_slug(&duplicate, "gpt-base").clone();
        duplicate["models"]
            .as_array_mut()
            .expect("models")
            .push(duplicate_model);

        let error = generate_catalog(
            &serde_json::to_vec(&duplicate).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect_err("non-target duplicate must fail");

        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID");
    }

    #[test]
    fn policy_only_catalog_does_not_require_a_managed_profile_template() {
        let mut base: Value = serde_json::from_slice(&gpt56_base_catalog(272_000)).expect("json");
        for model in base["models"].as_array_mut().expect("models") {
            model["visibility"] = json!("hide");
        }
        let output = generate_catalog(
            &serde_json::to_vec(&base).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect("policy-only catalog");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            model_by_slug(&root, "gpt-5.6-sol")["context_window"],
            json!(372_000)
        );
    }

    #[test]
    fn v2_projection_is_byte_stable_and_detects_policy_metadata_tampering() {
        let base = gpt56_base_catalog(272_000);
        let profile_sha = profile_set_sha256(&[profile()]).expect("profile hash");
        let first = generate_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect("first generation");
        let second = generate_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect("second generation");
        assert_eq!(first, second);

        let projection_of = |bytes: &[u8]| {
            let root: Value = serde_json::from_slice(bytes).expect("json");
            root[OWNER_METADATA_KEY]["projection_sha256"]
                .as_str()
                .expect("projection hash")
                .to_string()
        };
        let first_projection = projection_of(&first);
        let disabled = generate_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            false,
            None,
        )
        .expect("disabled projection");
        assert_ne!(first_projection, projection_of(&disabled));
        let different_base = generate_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "c".repeat(64).as_str(),
            true,
            None,
        )
        .expect("different base projection");
        assert_ne!(first_projection, projection_of(&different_base));
        let original_path = std::env::temp_dir().join("projection-user-catalog.json");
        let different_binding = generate_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            true,
            Some(&original_path),
        )
        .expect("different binding projection");
        assert_ne!(first_projection, projection_of(&different_binding));
        let mut changed_profile = profile();
        changed_profile.capabilities.context_window = Some(256_000);
        let changed_profile_sha = profile_set_sha256(std::slice::from_ref(&changed_profile))
            .expect("changed profile hash");
        let different_profile = generate_catalog(
            &base,
            &[changed_profile],
            &changed_profile_sha,
            "b".repeat(64).as_str(),
            true,
            None,
        )
        .expect("different profile projection");
        assert_ne!(first_projection, projection_of(&different_profile));

        let mut root: Value = serde_json::from_slice(&first).expect("json");
        root[OWNER_METADATA_KEY]["gpt56_372k_context_tokens"] = json!(380_928);
        let tampered = serde_json::to_vec(&root).expect("serialize");
        assert_eq!(
            validate_owned_catalog(&tampered)
                .expect_err("policy metadata drift")
                .code(),
            "CODEX_MANAGED_MODEL_CATALOG_MODIFIED"
        );
    }

    #[test]
    fn generated_catalog_supports_explicit_no_reasoning_and_unknown_context() {
        let mut profile = profile();
        profile
            .set_capabilities(crate::provider_models::ProviderModelCapabilities {
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
                context_window: None,
            })
            .expect("no reasoning capabilities");
        let output = generate_catalog(
            &base_catalog(),
            &[profile],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            None,
        )
        .expect("generate");
        let root: Value = serde_json::from_slice(&output).expect("json");
        let managed = &root["models"][1];
        assert_eq!(managed["supported_reasoning_levels"], json!([]));
        assert_eq!(managed["default_reasoning_level"], Value::Null);
        assert_eq!(managed["supports_reasoning_summaries"], json!(false));
        assert_eq!(managed["context_window"], Value::Null);
        assert_eq!(managed["max_context_window"], Value::Null);
    }

    #[test]
    fn ownership_hash_detects_external_model_changes() {
        let mut output = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            None,
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
    fn user_base_catalog_guard_detects_prepare_apply_byte_drift() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("user-models.json");
        let prepared = b"first-catalog".to_vec();
        std::fs::write(&path, &prepared).expect("write prepared catalog");
        let fingerprint = user_catalog_fingerprint(&path, &prepared);

        ensure_user_base_catalog_unchanged(&path, &prepared, &fingerprint)
            .expect("unchanged source");

        let changed = b"other-catalog";
        assert_eq!(changed.len(), prepared.len());
        std::fs::write(&path, changed).expect("rewrite source with same byte length");
        let error = ensure_user_base_catalog_unchanged(&path, &prepared, &fingerprint)
            .expect_err("changed source must invalidate the prepared plan");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_BASE_CATALOG_DRIFT");
    }

    #[test]
    fn bundled_base_catalog_descriptor_tracks_launch_and_executable_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp.path().join("codex-test.exe");
        std::fs::write(&executable, b"first").expect("write executable");
        let launch = crate::cli_manager::CodexLaunchSpec {
            executable: executable.clone(),
            runtime_path: OsString::from("runtime-a"),
            version: Some("codex-cli 0.147.0".to_string()),
        };
        let prepared = bundled_catalog_descriptor(&launch).expect("prepared descriptor");

        std::fs::write(&executable, b"second-longer").expect("replace executable");
        let replaced = bundled_catalog_descriptor(&launch).expect("replaced descriptor");
        assert_ne!(prepared, replaced);
        assert_ne!(
            bundled_catalog_fingerprint(&prepared),
            bundled_catalog_fingerprint(&replaced)
        );

        let mut changed_launch = launch;
        changed_launch.runtime_path = OsString::from("runtime-b");
        changed_launch.version = Some("codex-cli 0.147.1".to_string());
        let changed_launch =
            bundled_catalog_descriptor(&changed_launch).expect("launch descriptor");
        assert_ne!(replaced, changed_launch);
    }

    #[test]
    fn legacy_v1_catalog_is_validated_and_regenerated_from_a_proxy_baseline() {
        let legacy = legacy_v1_catalog();
        let legacy_metadata = validate_owned_catalog(&legacy).expect("valid legacy catalog");
        assert!(legacy_metadata.is_legacy_v1());
        assert!(legacy_metadata.projection_sha256.is_none());

        let temp = tempfile::tempdir().expect("tempdir");
        let generated_path = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let user_catalog_path = temp.path().join("user-catalog.json");
        let backup_path = temp.path().join("config.toml.backup");
        let mut baseline_document = toml_edit::DocumentMut::new();
        baseline_document["model_catalog_json"] =
            toml_edit::value(user_catalog_path.to_string_lossy().to_string());
        let baseline_bytes = baseline_document.to_string().into_bytes();
        std::fs::write(&backup_path, &baseline_bytes).expect("write baseline");
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: temp.path().join("config.toml"),
            config_backup_path: Some(backup_path),
            config_bytes: Some(baseline_bytes),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };
        let prepared = prepare_catalog_baseline(&baseline, &generated_path, Some(&legacy_metadata))
            .expect("proxy baseline");
        assert_eq!(
            prepared.catalog_path.as_deref(),
            Some(user_catalog_path.as_path())
        );

        let migrated = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            prepared.catalog_path.as_deref(),
        )
        .expect("regenerate v2 catalog");
        let migrated_metadata = validate_owned_catalog(&migrated).expect("valid v2 catalog");
        assert_eq!(migrated_metadata.schema_version, OWNER_SCHEMA_VERSION);
        assert!(migrated_metadata.projection_sha256.is_some());
        assert_eq!(
            migrated_metadata.original_catalog_path.as_deref(),
            Some(user_catalog_path.as_path())
        );
    }

    #[test]
    fn direct_mode_rejects_an_orphaned_legacy_v1_catalog() {
        let metadata = validate_owned_catalog(&legacy_v1_catalog()).expect("legacy metadata");
        let generated_path = std::env::temp_dir().join(GENERATED_CATALOG_FILE_NAME);
        let error = direct_original_catalog_path(
            Some(generated_path.as_path()),
            &generated_path,
            Some(&metadata),
        )
        .expect_err("legacy direct ownership is ambiguous");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
    }

    #[test]
    fn direct_mode_ignores_an_inactive_legacy_v1_catalog() {
        let metadata = validate_owned_catalog(&legacy_v1_catalog()).expect("legacy metadata");
        let generated_path = std::env::temp_dir().join(GENERATED_CATALOG_FILE_NAME);
        let user_catalog_path = std::env::temp_dir().join("legacy-user-catalog.json");

        assert_eq!(
            direct_original_catalog_path(
                Some(user_catalog_path.as_path()),
                &generated_path,
                Some(&metadata),
            )
            .expect("inactive legacy output must not block a restored direct binding"),
            Some(user_catalog_path),
        );
        assert_eq!(
            direct_original_catalog_path(None, &generated_path, Some(&metadata))
                .expect("inactive legacy output must not block the bundled binding"),
            None,
        );
    }

    #[test]
    fn direct_mode_uses_v2_original_binding_and_rejects_third_party_drift() {
        let original_path = std::env::temp_dir().join("original-catalog.json");
        let generated_path = std::env::temp_dir().join(GENERATED_CATALOG_FILE_NAME);
        let output = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            Some(&original_path),
        )
        .expect("generate v2 catalog");
        let metadata = validate_owned_catalog(&output).expect("metadata");
        assert_eq!(
            direct_original_catalog_path(Some(&generated_path), &generated_path, Some(&metadata))
                .expect("active generated binding"),
            Some(original_path.clone())
        );
        assert_eq!(
            direct_original_catalog_path(Some(&original_path), &generated_path, Some(&metadata))
                .expect("known restored binding"),
            Some(original_path)
        );

        let external_path = std::env::temp_dir().join("external-catalog.json");
        let error =
            direct_original_catalog_path(Some(&external_path), &generated_path, Some(&metadata))
                .expect_err("external binding drift");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_CONFIG_DRIFT");
    }

    #[test]
    fn explicit_direct_config_save_updates_original_binding_but_preserves_owned_binding() {
        let temp = tempfile::tempdir().expect("tempdir");
        let original_path = temp.path().join("original-catalog.json");
        let next_path = temp.path().join("next-catalog.json");
        let generated_path = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let output = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            Some(&original_path),
        )
        .expect("generate v2 catalog");
        let metadata = validate_owned_catalog(&output).expect("metadata");
        let previous = config_with_catalog(Some(&generated_path));
        let committed = config_with_catalog(Some(&next_path));

        assert_eq!(
            explicit_save_original_catalog_path(
                Some(&previous),
                &committed,
                &generated_path,
                Some(&metadata),
            )
            .expect("accept explicit user catalog"),
            Some(next_path)
        );
        assert_eq!(
            explicit_save_original_catalog_path(
                Some(&previous),
                &config_with_catalog(None),
                &generated_path,
                Some(&metadata),
            )
            .expect("accept bundled catalog intent"),
            None
        );
        assert_eq!(
            explicit_save_original_catalog_path(
                Some(&previous),
                &config_with_catalog(Some(&generated_path)),
                &generated_path,
                Some(&metadata),
            )
            .expect("preserve owned binding on unrelated save"),
            Some(original_path)
        );
    }

    #[test]
    fn explicit_direct_config_save_cannot_adopt_unowned_or_legacy_generated_catalog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let generated_path = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let ordinary = config_with_catalog(Some(&temp.path().join("user-catalog.json")));
        let committed = config_with_catalog(Some(&generated_path));
        let error =
            explicit_save_original_catalog_path(Some(&ordinary), &committed, &generated_path, None)
                .expect_err("unowned generated path");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");

        let legacy = validate_owned_catalog(&legacy_v1_catalog()).expect("legacy metadata");
        let previous = config_with_catalog(Some(&generated_path));
        let next = config_with_catalog(Some(&temp.path().join("next-catalog.json")));
        let error = explicit_save_original_catalog_path(
            Some(&previous),
            &next,
            &generated_path,
            Some(&legacy),
        )
        .expect_err("legacy direct save remains ambiguous");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
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
            false,
            None,
        )
        .expect_err("conflict");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_ALIAS_CONFLICT");
    }

    #[test]
    fn config_patch_round_trips_native_paths_and_restores_original_value() {
        let current = br#"model_provider = "aio"
[model_providers.aio]
base_url = "http://127.0.0.1:37123/v1"
"#;
        let original_path = std::env::temp_dir()
            .join("Codex Catalogs")
            .join("custom.json");
        let mut original = toml_edit::DocumentMut::new();
        original["model_catalog_json"] = toml_edit::value(
            original_path
                .to_str()
                .expect("temporary path must be UTF-8"),
        );
        let original = original.to_string().into_bytes();
        let generated = std::env::temp_dir()
            .join("AIO Data")
            .join("managed-model-catalog.json");
        let patched = patch_model_catalog_config(
            current,
            Some(original.as_slice()),
            Some(generated.as_path()),
        )
        .expect("patch generated");
        let parsed = std::str::from_utf8(&patched)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(parsed["model_catalog_json"].as_str(), generated.to_str());

        let restored = patch_model_catalog_config(&patched, Some(original.as_slice()), None)
            .expect("restore original");
        let parsed = std::str::from_utf8(&restored)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            parsed["model_catalog_json"].as_str(),
            original_path.to_str()
        );
    }

    fn catalog_transaction_fixture(
        dir: &Path,
    ) -> (
        Option<AppliedFileChange>,
        FileSnapshot,
        Vec<u8>,
        FileSnapshot,
        Option<Vec<u8>>,
    ) {
        let baseline_path = dir.join("config.toml.backup");
        let config_path = dir.join("config.toml");
        let generated_path = dir.join(GENERATED_CATALOG_FILE_NAME);
        let baseline_before = b"baseline-before".to_vec();
        let config_before = b"config-before".to_vec();
        let generated_before = b"generated-before".to_vec();
        std::fs::write(&baseline_path, &baseline_before).expect("write baseline fixture");
        std::fs::write(&config_path, &config_before).expect("write config fixture");
        std::fs::write(&generated_path, &generated_before).expect("write catalog fixture");

        (
            Some(AppliedFileChange {
                before: FileSnapshot {
                    path: baseline_path,
                    bytes: Some(baseline_before),
                },
                after: Some(b"baseline-after".to_vec()),
            }),
            FileSnapshot {
                path: config_path,
                bytes: Some(config_before),
            },
            b"config-after".to_vec(),
            FileSnapshot {
                path: generated_path,
                bytes: Some(generated_before),
            },
            Some(b"generated-after".to_vec()),
        )
    }

    fn reset_catalog_transaction_test_state() {
        CATALOG_APPLY_FAILURE.with(|failure| failure.set(None));
        CATALOG_APPLY_TRACE.with(|trace| trace.borrow_mut().clear());
        CATALOG_ROLLBACK_TRACE.with(|trace| trace.borrow_mut().clear());
    }

    fn catalog_apply_trace() -> Vec<CatalogApplyStage> {
        CATALOG_APPLY_TRACE.with(|trace| trace.borrow().clone())
    }

    fn catalog_rollback_trace() -> Vec<CatalogApplyStage> {
        CATALOG_ROLLBACK_TRACE.with(|trace| trace.borrow().clone())
    }

    #[test]
    fn catalog_file_transaction_applies_and_rolls_back_all_files() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, config_after, generated, generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();

        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            config_after.clone(),
            generated,
            generated_after.clone(),
        )
        .expect("apply catalog transaction");

        assert_eq!(std::fs::read(&baseline_path).unwrap(), b"baseline-after");
        assert_eq!(std::fs::read(&config_path).unwrap(), config_after);
        assert_eq!(
            std::fs::read(&generated_path).unwrap(),
            generated_after.unwrap()
        );
        assert_eq!(
            catalog_apply_trace(),
            vec![
                CatalogApplyStage::Baseline,
                CatalogApplyStage::Generated,
                CatalogApplyStage::Config,
            ]
        );

        applied.rollback().expect("roll back catalog transaction");
        assert_eq!(
            catalog_rollback_trace(),
            vec![
                CatalogApplyStage::Config,
                CatalogApplyStage::Generated,
                CatalogApplyStage::Baseline,
            ]
        );
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
        assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
        assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
    }

    #[test]
    fn catalog_file_transaction_rolls_back_prior_stages_after_write_failures() {
        for failure_stage in [
            CatalogApplyStage::Baseline,
            CatalogApplyStage::Generated,
            CatalogApplyStage::Config,
        ] {
            reset_catalog_transaction_test_state();
            let temp = tempfile::tempdir().expect("tempdir");
            let (baseline, config, config_after, generated, generated_after) =
                catalog_transaction_fixture(temp.path());
            let baseline_path = baseline.as_ref().unwrap().before.path.clone();
            let config_path = config.path.clone();
            let generated_path = generated.path.clone();
            CATALOG_APPLY_FAILURE.with(|failure| failure.set(Some(failure_stage)));

            let error = apply_prepared_catalog_files(
                baseline,
                config,
                config_after,
                generated,
                generated_after,
            )
            .expect_err("injected catalog write should fail");

            assert_eq!(error.code(), "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED");
            assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
            assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
            assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
        }
    }

    #[test]
    fn catalog_rollback_reports_recovery_required_and_continues_other_files() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, config_after, generated, generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();
        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            config_after,
            generated,
            generated_after,
        )
        .expect("apply catalog transaction");
        std::fs::write(&config_path, b"external-change").expect("create rollback drift");

        let error = applied
            .rollback()
            .expect_err("config drift must block rollback");

        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
        assert_eq!(std::fs::read(config_path).unwrap(), b"external-change");
        assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
    }

    #[test]
    fn catalog_deactivation_restores_binding_before_delete_and_rolls_back_in_reverse() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, _config_after, generated, _generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();
        let restored_config = b"config-original-binding".to_vec();

        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            restored_config.clone(),
            generated,
            None,
        )
        .expect("deactivate catalog");

        assert_eq!(std::fs::read(&config_path).unwrap(), restored_config);
        assert!(!generated_path.exists());
        assert_eq!(
            catalog_apply_trace(),
            vec![
                CatalogApplyStage::Baseline,
                CatalogApplyStage::Config,
                CatalogApplyStage::Generated,
            ]
        );

        applied.rollback().expect("roll back deactivation");
        assert_eq!(
            catalog_rollback_trace(),
            vec![
                CatalogApplyStage::Generated,
                CatalogApplyStage::Config,
                CatalogApplyStage::Baseline,
            ]
        );
        assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
        assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
    }

    #[test]
    fn catalog_deactivation_failure_restores_every_committed_prior_stage() {
        for failure_stage in [
            CatalogApplyStage::Baseline,
            CatalogApplyStage::Config,
            CatalogApplyStage::Generated,
        ] {
            reset_catalog_transaction_test_state();
            let temp = tempfile::tempdir().expect("tempdir");
            let (baseline, config, _config_after, generated, _generated_after) =
                catalog_transaction_fixture(temp.path());
            let baseline_path = baseline.as_ref().unwrap().before.path.clone();
            let config_path = config.path.clone();
            let generated_path = generated.path.clone();
            CATALOG_APPLY_FAILURE.with(|failure| failure.set(Some(failure_stage)));

            let error = apply_prepared_catalog_files(
                baseline,
                config,
                b"config-original-binding".to_vec(),
                generated,
                None,
            )
            .expect_err("injected deactivation failure");

            assert_eq!(error.code(), "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED");
            assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
            assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
            assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
            let expected_trace = match failure_stage {
                CatalogApplyStage::Baseline => vec![CatalogApplyStage::Baseline],
                CatalogApplyStage::Config => {
                    vec![CatalogApplyStage::Baseline, CatalogApplyStage::Config]
                }
                CatalogApplyStage::Generated => {
                    vec![
                        CatalogApplyStage::Baseline,
                        CatalogApplyStage::Config,
                        CatalogApplyStage::Generated,
                    ]
                }
            };
            assert_eq!(catalog_apply_trace(), expected_trace);
        }
    }

    #[test]
    fn catalog_deactivation_rollback_restores_config_even_if_generated_drifted() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, _config_after, generated, _generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();
        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            b"config-original-binding".to_vec(),
            generated,
            None,
        )
        .expect("deactivate catalog");
        std::fs::write(&generated_path, b"external-generated").expect("external drift");

        let error = applied.rollback().expect_err("generated rollback drift");

        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
        assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
        assert_eq!(
            std::fs::read(generated_path).unwrap(),
            b"external-generated"
        );
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
        assert_eq!(
            catalog_rollback_trace(),
            vec![
                CatalogApplyStage::Generated,
                CatalogApplyStage::Config,
                CatalogApplyStage::Baseline,
            ]
        );
    }

    #[test]
    fn generated_catalog_binding_in_proxy_backup_is_prepared_for_repair() {
        let dir = std::env::temp_dir().join(format!(
            "aio-catalog-baseline-repair-{}",
            crate::shared::uuid::new_uuid_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let generated = dir.join(GENERATED_CATALOG_FILE_NAME);
        let backup_path = dir.join("config.toml.backup");
        let mut document = toml_edit::DocumentMut::new();
        document["model"] = toml_edit::value("gpt-5");
        document["model_catalog_json"] = toml_edit::value(generated.to_string_lossy().to_string());
        let polluted = document.to_string().into_bytes();
        std::fs::write(&backup_path, &polluted).expect("write polluted backup");
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: dir.join("config.toml"),
            config_backup_path: Some(backup_path),
            config_bytes: Some(polluted),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };

        let prepared =
            prepare_catalog_baseline(&baseline, &generated, None).expect("prepare repair");

        assert!(prepared.catalog_path.is_none());
        assert!(prepared.backup_change.is_some());
        let repaired = std::str::from_utf8(
            prepared
                .backup_change
                .as_ref()
                .and_then(|change| change.after.as_deref())
                .expect("repaired baseline bytes"),
        )
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
        assert!(repaired.get("model_catalog_json").is_none());
        assert_eq!(repaired["model"].as_str(), Some("gpt-5"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn v2_generated_binding_in_proxy_backup_recovers_original_user_catalog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let generated = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let original_catalog = temp.path().join("user-catalog.json");
        let generated_bytes = generate_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            false,
            Some(&original_catalog),
        )
        .expect("generate v2 catalog");
        std::fs::write(&generated, &generated_bytes).expect("write generated catalog");
        let metadata = validate_owned_catalog(&generated_bytes).expect("v2 metadata");

        let backup_path = temp.path().join("config.toml.backup");
        let polluted = config_with_catalog(Some(&generated));
        std::fs::write(&backup_path, &polluted).expect("write proxy backup");
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: temp.path().join("config.toml"),
            config_backup_path: Some(backup_path),
            config_bytes: Some(polluted),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };

        let prepared = prepare_catalog_baseline(&baseline, &generated, Some(&metadata))
            .expect("recover original binding");

        assert_eq!(
            prepared.catalog_path.as_deref(),
            Some(original_catalog.as_path())
        );
        let repaired = prepared
            .backup_change
            .as_ref()
            .and_then(|change| change.after.as_deref())
            .expect("repaired baseline bytes");
        assert_eq!(
            parse_original_catalog_path(Some(repaired)).expect("repaired catalog path"),
            Some(original_catalog)
        );
    }

    #[test]
    fn user_catalog_binding_is_not_prepared_for_repair() {
        let dir = std::env::temp_dir().join(format!(
            "aio-user-catalog-baseline-{}",
            crate::shared::uuid::new_uuid_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let generated = dir.join(GENERATED_CATALOG_FILE_NAME);
        let user_catalog = dir.join("user-catalog.json");
        let mut document = toml_edit::DocumentMut::new();
        document["model_catalog_json"] =
            toml_edit::value(user_catalog.to_string_lossy().to_string());
        let original = document.to_string().into_bytes();
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: dir.join("config.toml"),
            config_backup_path: Some(dir.join("config.toml.backup")),
            config_bytes: Some(original.clone()),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };

        let prepared =
            prepare_catalog_baseline(&baseline, &generated, None).expect("keep user path");

        assert_eq!(prepared.catalog_path, Some(user_catalog));
        assert!(prepared.backup_change.is_none());
        assert_eq!(baseline.config_bytes, Some(original));
        let _ = std::fs::remove_dir_all(dir);
    }
}
