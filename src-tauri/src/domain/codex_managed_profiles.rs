//! Usage: Managed Codex profile metadata and ownership-safe profile files.

use crate::db;
use crate::shared::error::{db_err, AppError, AppResult};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(test)]
static FAIL_NEXT_PROFILE_METADATA_DELETE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static FAIL_NEXT_PROFILE_METADATA_INSERT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static PROFILE_LIFECYCLE_LOCK_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

const PROFILE_NAME_MAX_BYTES: usize = 64;
const PROFILE_FILE_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexManagedProfileFileStatus {
    Managed,
    Missing,
    Modified,
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexManagedProfile {
    pub profile_uuid: String,
    pub profile_name: String,
    pub model_uuid: String,
    pub provider_id: i64,
    pub provider_uuid: String,
    pub provider_name: String,
    pub remote_model_id: String,
    pub canonical_model: String,
    pub file_status: CodexManagedProfileFileStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexManagedProfileDeleteResult {
    pub deleted: bool,
    pub external_file_preserved: bool,
}

#[derive(Debug)]
struct ProfileRow {
    profile_uuid: String,
    profile_name: String,
    profile_name_key: String,
    model_uuid: String,
    provider_id: i64,
    provider_uuid: String,
    provider_name: String,
    remote_model_id: String,
    codex_home_path: String,
    content_sha256: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug)]
struct ModelTarget {
    model_uuid: String,
    provider_id: i64,
    provider_uuid: String,
    provider_name: String,
    remote_model_id: String,
}

enum OwnedFileState {
    Missing,
    Owned,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileCapturePurpose {
    Delete,
    CreateCompensation,
}

enum IsolatedProfileFile {
    Missing,
    Owned(PathBuf),
    Modified(PathBuf),
}

#[cfg(test)]
struct ProfileCaptureTestHook {
    purpose: ProfileCapturePurpose,
    replacement: Vec<u8>,
}

fn profile_lifecycle_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn lock_profile_lifecycle() -> MutexGuard<'static, ()> {
    #[cfg(test)]
    PROFILE_LIFECYCLE_LOCK_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    profile_lifecycle_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
pub(crate) fn reset_profile_lifecycle_lock_attempts_for_test() {
    PROFILE_LIFECYCLE_LOCK_ATTEMPTS.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn profile_lifecycle_lock_attempts_for_test() -> usize {
    PROFILE_LIFECYCLE_LOCK_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst)
}

fn validate_profile_name(profile_name: &str) -> AppResult<String> {
    let bytes = profile_name.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= PROFILE_NAME_MAX_BYTES
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "profile_name must match [A-Za-z0-9][A-Za-z0-9_-]* and be at most 64 bytes",
        ));
    }
    let profile_name_key = profile_name.to_ascii_lowercase();
    if crate::shared::uuid::is_canonical_uuid_v4(&profile_name_key) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "profile_name must not use the reserved UUID alias form",
        ));
    }
    Ok(profile_name_key)
}

pub(crate) fn is_valid_profile_name_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= PROFILE_NAME_MAX_BYTES
        && bytes[0].is_ascii_lowercase_or_digit()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase_or_digit() || matches!(byte, b'_' | b'-'))
}

trait AsciiLowercaseOrDigit {
    fn is_ascii_lowercase_or_digit(&self) -> bool;
}

impl AsciiLowercaseOrDigit for u8 {
    fn is_ascii_lowercase_or_digit(&self) -> bool {
        self.is_ascii_lowercase() || self.is_ascii_digit()
    }
}

fn validate_uuid(value: &str, field: &str) -> AppResult<()> {
    if !crate::shared::uuid::is_canonical_uuid_v4(value) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            format!("{field} must be a canonical UUIDv4"),
        ));
    }
    Ok(())
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

fn render_profile(profile_name_key: &str) -> AppResult<Vec<u8>> {
    if !is_valid_profile_name_key(profile_name_key)
        || crate::shared::uuid::is_canonical_uuid_v4(profile_name_key)
    {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "profile_name_key is invalid",
        ));
    }
    let canonical_model = format!("aio/{profile_name_key}");
    let mut document = toml_edit::DocumentMut::new();
    document["model"] = toml_edit::value(&canonical_model);
    document["model_provider"] = toml_edit::value("aio");
    let content = document.to_string();
    let parsed = content.parse::<toml_edit::DocumentMut>().map_err(|error| {
        AppError::new(
            "SYSTEM_ERROR",
            format!("failed to validate generated Codex profile TOML: {error}"),
        )
    })?;
    if parsed.get("model").and_then(toml_edit::Item::as_str) != Some(&canonical_model)
        || parsed
            .get("model_provider")
            .and_then(toml_edit::Item::as_str)
            != Some("aio")
    {
        return Err(AppError::new(
            "SYSTEM_ERROR",
            "generated Codex profile TOML failed round-trip validation",
        ));
    }
    Ok(content.into_bytes())
}

fn profile_path(codex_home_path: &str, profile_name: &str) -> AppResult<PathBuf> {
    validate_profile_name(profile_name).map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile has an invalid profile_name",
        )
    })?;
    let home = PathBuf::from(codex_home_path);
    if !home.is_absolute() {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile has a non-absolute codex_home_path",
        ));
    }
    validate_stored_codex_home(&home)?;
    Ok(home.join(format!("{profile_name}.config.toml")))
}

#[cfg(windows)]
fn paths_match_after_canonicalization(canonical: &Path, stored: &Path) -> bool {
    canonical
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .eq_ignore_ascii_case(
            stored
                .to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/'),
        )
}

#[cfg(not(windows))]
fn paths_match_after_canonicalization(canonical: &Path, stored: &Path) -> bool {
    canonical == stored
}

fn metadata_is_unsafe_codex_home_component(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;

        // Junctions and other reparse points are not necessarily reported as
        // symbolic links by std::fs, but following them would violate the
        // stored Codex-home ownership boundary.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }

    false
}

fn unsafe_codex_home_error() -> AppError {
    AppError::new(
        "CODEX_MANAGED_PROFILE_HOME_UNSAFE",
        "stored Codex home was replaced or is not a stable directory",
    )
}

fn validate_codex_home_layout(home: &Path) -> AppResult<bool> {
    // Validate every existing ancestor so a missing home cannot hide a
    // replaced parent symlink or reparse point.
    let mut home_exists = false;
    for ancestor in home.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata_is_unsafe_codex_home_component(&metadata) {
                    return Err(unsafe_codex_home_error());
                }
                if ancestor == home {
                    home_exists = true;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(unsafe_codex_home_error()),
        }
    }

    Ok(home_exists)
}

fn validate_stored_codex_home(home: &Path) -> AppResult<()> {
    let home_exists = validate_codex_home_layout(home)?;

    if home_exists {
        let canonical = std::fs::canonicalize(home).map_err(|_| unsafe_codex_home_error())?;
        if !paths_match_after_canonicalization(&canonical, home) {
            return Err(unsafe_codex_home_error());
        }
    }

    Ok(())
}

fn inspect_owned_file(path: &Path, expected_sha256: &str) -> OwnedFileState {
    match crate::shared::fs::read_optional_file_with_max_len(path, PROFILE_FILE_MAX_BYTES) {
        Ok(None) => OwnedFileState::Missing,
        Ok(Some(bytes)) if sha256_hex(&bytes) == expected_sha256 => OwnedFileState::Owned,
        Ok(Some(_)) | Err(_) => OwnedFileState::Modified,
    }
}

fn file_status(path: &Path, expected_sha256: &str) -> CodexManagedProfileFileStatus {
    match inspect_owned_file(path, expected_sha256) {
        OwnedFileState::Missing => CodexManagedProfileFileStatus::Missing,
        OwnedFileState::Owned => CodexManagedProfileFileStatus::Managed,
        OwnedFileState::Modified => CodexManagedProfileFileStatus::Modified,
    }
}

#[cfg(test)]
fn profile_capture_test_hook() -> &'static Mutex<Option<ProfileCaptureTestHook>> {
    static HOOK: OnceLock<Mutex<Option<ProfileCaptureTestHook>>> = OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
fn install_profile_capture_test_hook(purpose: ProfileCapturePurpose, replacement: &[u8]) {
    *profile_capture_test_hook()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ProfileCaptureTestHook {
        purpose,
        replacement: replacement.to_vec(),
    });
}

#[cfg(test)]
fn run_profile_capture_test_hook(purpose: ProfileCapturePurpose, target: &Path) -> AppResult<()> {
    let replacement = {
        let mut guard = profile_capture_test_hook()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.as_ref().is_some_and(|hook| hook.purpose == purpose) {
            guard.take().map(|hook| hook.replacement)
        } else {
            None
        }
    };
    if let Some(replacement) = replacement {
        std::fs::write(target, replacement).map_err(|error| {
            AppError::new(
                "SYSTEM_ERROR",
                format!("failed to run managed profile capture test hook: {error}"),
            )
        })?;
    }
    Ok(())
}

#[cfg(not(test))]
fn run_profile_capture_test_hook(_purpose: ProfileCapturePurpose, _target: &Path) -> AppResult<()> {
    Ok(())
}

fn profile_recovery_required(
    target: &Path,
    isolated: &Path,
    reason: impl std::fmt::Display,
) -> AppError {
    AppError::new(
        "CODEX_MANAGED_PROFILE_RECOVERY_REQUIRED",
        format!(
            "{reason}; refusing to overwrite {}; preserved isolated file at {}",
            target.display(),
            isolated.display()
        ),
    )
}

fn missing_isolated_profile_recovery_required(
    target: &Path,
    isolated: &Path,
    reason: impl std::fmt::Display,
) -> AppError {
    AppError::new(
        "CODEX_MANAGED_PROFILE_RECOVERY_REQUIRED",
        format!(
            "{reason}; manual recovery is required for {}; no isolated file was found at {}",
            target.display(),
            isolated.display()
        ),
    )
}

fn capture_profile_file(
    target: &Path,
    expected_sha256: &str,
    purpose: ProfileCapturePurpose,
) -> AppResult<IsolatedProfileFile> {
    let parent = target.parent().ok_or_else(|| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile path has no parent directory",
        )
    })?;
    for _ in 0..32 {
        let isolated = parent.join(format!(
            ".aio-profile-capture-{}.tmp",
            crate::shared::uuid::new_uuid_v4()
        ));
        match crate::shared::fs::rename_file_no_replace(target, &isolated) {
            Ok(()) => {
                if let Err(error) = run_profile_capture_test_hook(purpose, target) {
                    return Err(profile_recovery_required(target, &isolated, error));
                }
                return match inspect_owned_file(&isolated, expected_sha256) {
                    OwnedFileState::Missing => Err(missing_isolated_profile_recovery_required(
                        target,
                        &isolated,
                        "isolated profile file disappeared before ownership verification",
                    )),
                    OwnedFileState::Owned => Ok(IsolatedProfileFile::Owned(isolated)),
                    OwnedFileState::Modified => Ok(IsolatedProfileFile::Modified(isolated)),
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(IsolatedProfileFile::Missing)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                let code = match purpose {
                    ProfileCapturePurpose::Delete => "FS_DELETE_FAILED",
                    ProfileCapturePurpose::CreateCompensation => {
                        "CODEX_MANAGED_PROFILE_RECOVERY_REQUIRED"
                    }
                };
                return Err(AppError::new(
                    code,
                    format!(
                        "failed to isolate managed Codex profile {} safely: {error}",
                        target.display()
                    ),
                ));
            }
        }
    }
    Err(AppError::new(
        "CODEX_MANAGED_PROFILE_RECOVERY_REQUIRED",
        format!(
            "failed to allocate an isolated recovery path for {}",
            target.display()
        ),
    ))
}

fn restore_isolated_file(target: &Path, isolated: &Path, reason: &str) -> AppResult<()> {
    crate::shared::fs::rename_file_no_replace(isolated, target)
        .map_err(|error| profile_recovery_required(target, isolated, format!("{reason}: {error}")))
}

fn restore_owned_isolated_file(
    target: &Path,
    isolated: &Path,
    expected_sha256: &str,
) -> AppResult<()> {
    match inspect_owned_file(isolated, expected_sha256) {
        OwnedFileState::Owned => restore_isolated_file(
            target,
            isolated,
            "profile metadata was not deleted and the owned file could not be restored safely",
        ),
        OwnedFileState::Missing => Err(missing_isolated_profile_recovery_required(
            target,
            isolated,
            "profile metadata was not deleted and the isolated owned file is missing",
        )),
        OwnedFileState::Modified => Err(profile_recovery_required(
            target,
            isolated,
            "profile metadata was not deleted and the isolated owned file was modified",
        )),
    }
}

fn remove_owned_isolated_file(
    target: &Path,
    isolated: &Path,
    expected_sha256: &str,
) -> AppResult<()> {
    match inspect_owned_file(isolated, expected_sha256) {
        OwnedFileState::Missing => Ok(()),
        OwnedFileState::Modified => Err(profile_recovery_required(
            target,
            isolated,
            "isolated owned profile changed before cleanup",
        )),
        OwnedFileState::Owned => match std::fs::remove_file(isolated) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(profile_recovery_required(
                target,
                isolated,
                format!("failed to remove isolated owned profile: {error}"),
            )),
        },
    }
}

fn compensate_created_profile_file(target: &Path, expected_sha256: &str) -> AppResult<()> {
    match capture_profile_file(
        target,
        expected_sha256,
        ProfileCapturePurpose::CreateCompensation,
    )? {
        IsolatedProfileFile::Missing => Ok(()),
        IsolatedProfileFile::Owned(isolated) => {
            remove_owned_isolated_file(target, &isolated, expected_sha256)
        }
        IsolatedProfileFile::Modified(isolated) => restore_isolated_file(
            target,
            &isolated,
            "generated profile was replaced before compensation and could not be preserved safely",
        ),
    }
}

fn target_path_is_occupied(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}

fn should_fail_profile_metadata_insert() -> bool {
    #[cfg(test)]
    {
        FAIL_NEXT_PROFILE_METADATA_INSERT.swap(false, std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn read_profiles(db: &db::Db, profile_uuid: Option<&str>) -> AppResult<Vec<ProfileRow>> {
    let conn = db.open_connection()?;
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT profile.profile_uuid, profile.profile_name, profile.profile_name_key, profile.model_uuid,
       model.provider_id, provider.provider_uuid, provider.name, model.remote_model_id,
       profile.codex_home_path, profile.content_sha256,
       profile.created_at, profile.updated_at
FROM codex_managed_profiles profile
JOIN provider_models model ON model.model_uuid = profile.model_uuid
JOIN providers provider ON provider.id = model.provider_id
WHERE (?1 IS NULL OR profile.profile_uuid = ?1)
ORDER BY profile.profile_name_key ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare managed profile query: {error}"))?;
    let rows = statement
        .query_map(params![profile_uuid], |row| {
            Ok(ProfileRow {
                profile_uuid: row.get(0)?,
                profile_name: row.get(1)?,
                profile_name_key: row.get(2)?,
                model_uuid: row.get(3)?,
                provider_id: row.get(4)?,
                provider_uuid: row.get(5)?,
                provider_name: row.get(6)?,
                remote_model_id: row.get(7)?,
                codex_home_path: row.get(8)?,
                content_sha256: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|error| db_err!("failed to query managed profiles: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read managed profile row: {error}"))
}

fn project_profile(row: ProfileRow) -> AppResult<CodexManagedProfile> {
    validate_uuid(&row.profile_uuid, "stored profile_uuid").map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile has an invalid profile_uuid",
        )
    })?;
    validate_uuid(&row.model_uuid, "stored model_uuid").map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile has an invalid model_uuid",
        )
    })?;
    validate_uuid(&row.provider_uuid, "stored provider_uuid").map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile has an invalid provider_uuid",
        )
    })?;
    if !is_valid_profile_name_key(&row.profile_name_key)
        || validate_profile_name(&row.profile_name).ok().as_deref()
            != Some(row.profile_name_key.as_str())
    {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile has an invalid profile name key",
        ));
    }
    let path = profile_path(&row.codex_home_path, &row.profile_name)?;
    Ok(CodexManagedProfile {
        canonical_model: format!("aio/{}", row.profile_name_key),
        file_status: file_status(&path, &row.content_sha256),
        profile_uuid: row.profile_uuid,
        profile_name: row.profile_name,
        model_uuid: row.model_uuid,
        provider_id: row.provider_id,
        provider_uuid: row.provider_uuid,
        provider_name: row.provider_name,
        remote_model_id: row.remote_model_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub fn list(db: &db::Db) -> AppResult<Vec<CodexManagedProfile>> {
    let _guard = lock_profile_lifecycle();
    read_profiles(db, None)?
        .into_iter()
        .map(project_profile)
        .collect()
}

pub fn create<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
    profile_name: &str,
    model_uuid: &str,
) -> AppResult<CodexManagedProfile> {
    let _guard = lock_profile_lifecycle();
    let profile_name_key = validate_profile_name(profile_name)?;
    validate_uuid(model_uuid, "model_uuid")?;

    let mut conn = db.open_connection()?;
    let already_managed: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM codex_managed_profiles WHERE profile_name_key = ?1)",
            params![profile_name_key],
            |row| row.get(0),
        )
        .map_err(|error| db_err!("failed to query managed profile name: {error}"))?;
    if already_managed {
        return Err(AppError::new(
            "CODEX_MANAGED_PROFILE_NAME_EXISTS",
            "a managed Codex profile already uses this name",
        ));
    }

    let target = conn
        .query_row(
            r#"
SELECT model.model_uuid, model.provider_id, provider.provider_uuid,
       provider.name, model.remote_model_id
FROM provider_models model
JOIN providers provider ON provider.id = model.provider_id
WHERE model.model_uuid = ?1
  AND provider.cli_key = 'codex'
  AND provider.source_provider_id IS NULL
  AND provider.bridge_type IS NULL
"#,
            params![model_uuid],
            |row| {
                Ok(ModelTarget {
                    model_uuid: row.get(0)?,
                    provider_id: row.get(1)?,
                    provider_uuid: row.get(2)?,
                    provider_name: row.get(3)?,
                    remote_model_id: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| db_err!("failed to query managed profile model: {error}"))?
        .ok_or_else(|| AppError::new("DB_NOT_FOUND", "provider model not found"))?;

    let mut catalog_profiles = crate::codex_model_catalog::managed::load_profiles(&conn)?;
    catalog_profiles.push(
        crate::codex_model_catalog::managed::ManagedCatalogProfile::new(
            profile_name_key.clone(),
            target.model_uuid.clone(),
            target.provider_name.clone(),
            target.remote_model_id.clone(),
        )?,
    );
    catalog_profiles.sort_by(|left, right| left.profile_name_key.cmp(&right.profile_name_key));

    let codex_home = crate::codex_paths::codex_home_dir(app)?;
    validate_codex_home_layout(&codex_home)?;
    std::fs::create_dir_all(&codex_home).map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!("failed to create Codex home: {error}"),
        )
    })?;
    validate_codex_home_layout(&codex_home)?;
    let canonical_home = std::fs::canonicalize(&codex_home).map_err(|error| {
        AppError::new(
            "FS_WRITE_FAILED",
            format!("failed to resolve Codex home: {error}"),
        )
    })?;
    validate_stored_codex_home(&canonical_home)?;
    let canonical_home_text = canonical_home.to_str().ok_or_else(|| {
        AppError::new(
            "SEC_INVALID_INPUT",
            "Codex home path must be valid UTF-8 to manage profile files",
        )
    })?;
    let path = canonical_home.join(format!("{profile_name}.config.toml"));
    let catalog_plan =
        crate::codex_model_catalog::managed::prepare_for_profiles(app, &catalog_profiles)?;
    let content = render_profile(&profile_name_key)?;
    let content_sha256 = sha256_hex(&content);
    crate::shared::fs::write_file_atomic_create_new(&path, &content).map_err(|error| {
        if error.code() == "FS_ALREADY_EXISTS" {
            AppError::new(
                "CODEX_MANAGED_PROFILE_FILE_EXISTS",
                "refusing to overwrite an existing Codex profile file",
            )
        } else {
            AppError::new(
                "CODEX_MANAGED_PROFILE_WRITE_FAILED",
                "failed to create managed Codex profile file",
            )
        }
    })?;

    let profile_uuid = crate::shared::uuid::new_uuid_v4();
    let now = crate::shared::time::now_unix_seconds();
    let transaction = conn
        .transaction()
        .map_err(|error| db_err!("failed to start managed Codex profile transaction: {error}"))?;
    let insert = if should_fail_profile_metadata_insert() {
        Err(rusqlite::Error::InvalidQuery)
    } else {
        transaction.execute(
            r#"
INSERT INTO codex_managed_profiles(
  profile_uuid, profile_name, profile_name_key, model_uuid,
  codex_home_path, content_sha256, created_at, updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
"#,
            params![
                profile_uuid,
                profile_name,
                profile_name_key,
                target.model_uuid,
                canonical_home_text,
                content_sha256,
                now
            ],
        )
    };
    if let Err(error) = insert {
        drop(transaction);
        compensate_created_profile_file(&path, &content_sha256)?;
        return Err(db_err!("failed to insert managed Codex profile: {error}"));
    }

    let applied_catalog = match catalog_plan.apply(app) {
        Ok(applied) => applied,
        Err(error) => {
            drop(transaction);
            compensate_created_profile_file(&path, &content_sha256)?;
            return Err(error);
        }
    };
    if let Err(error) = transaction.commit() {
        let catalog_rollback = applied_catalog.rollback();
        let file_rollback = compensate_created_profile_file(&path, &content_sha256);
        catalog_rollback?;
        file_rollback?;
        return Err(db_err!("failed to commit managed Codex profile: {error}"));
    }

    Ok(CodexManagedProfile {
        profile_uuid,
        profile_name: profile_name.to_string(),
        model_uuid: target.model_uuid.clone(),
        provider_id: target.provider_id,
        provider_uuid: target.provider_uuid,
        provider_name: target.provider_name,
        remote_model_id: target.remote_model_id,
        canonical_model: format!("aio/{profile_name_key}"),
        file_status: CodexManagedProfileFileStatus::Managed,
        created_at: now,
        updated_at: now,
    })
}

pub fn delete<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
    profile_uuid: &str,
) -> AppResult<CodexManagedProfileDeleteResult> {
    let _guard = lock_profile_lifecycle();
    validate_uuid(profile_uuid, "profile_uuid")?;
    let row = read_profiles(db, Some(profile_uuid))?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::new("DB_NOT_FOUND", "managed Codex profile not found"))?;
    let mut conn = db.open_connection()?;
    let mut catalog_profiles = crate::codex_model_catalog::managed::load_profiles(&conn)?;
    let previous_count = catalog_profiles.len();
    catalog_profiles.retain(|profile| profile.profile_name_key != row.profile_name_key);
    if catalog_profiles.len() + 1 != previous_count {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile catalog identity is inconsistent",
        ));
    }
    let catalog_plan =
        crate::codex_model_catalog::managed::prepare_for_profiles(app, &catalog_profiles)?;
    let path = profile_path(&row.codex_home_path, &row.profile_name)?;
    let captured = capture_profile_file(&path, &row.content_sha256, ProfileCapturePurpose::Delete)?;
    let (owned_isolated, mut external_file_preserved) = match captured {
        IsolatedProfileFile::Missing => (None, false),
        IsolatedProfileFile::Owned(isolated) => (Some(isolated), false),
        IsolatedProfileFile::Modified(isolated) => {
            restore_isolated_file(
                &path,
                &isolated,
                "modified profile could not be restored safely before metadata deletion",
            )?;
            (None, true)
        }
    };
    let transaction = conn
        .transaction()
        .map_err(|error| db_err!("failed to start managed Codex profile transaction: {error}"))?;

    #[cfg(test)]
    let delete_result =
        if FAIL_NEXT_PROFILE_METADATA_DELETE.swap(false, std::sync::atomic::Ordering::SeqCst) {
            Err(rusqlite::Error::InvalidQuery)
        } else {
            transaction.execute(
                "DELETE FROM codex_managed_profiles WHERE profile_uuid = ?1",
                params![profile_uuid],
            )
        };
    #[cfg(not(test))]
    let delete_result = transaction.execute(
        "DELETE FROM codex_managed_profiles WHERE profile_uuid = ?1",
        params![profile_uuid],
    );
    let deleted = match delete_result {
        Ok(deleted) if deleted > 0 => deleted,
        Ok(_) => {
            drop(transaction);
            if let Some(isolated) = owned_isolated.as_deref() {
                restore_owned_isolated_file(&path, isolated, &row.content_sha256)?;
            }
            return Err(AppError::new(
                "DB_NOT_FOUND",
                "managed Codex profile not found",
            ));
        }
        Err(error) => {
            drop(transaction);
            if let Some(isolated) = owned_isolated.as_deref() {
                restore_owned_isolated_file(&path, isolated, &row.content_sha256)?;
            }
            return Err(db_err!(
                "failed to delete managed Codex profile metadata: {error}"
            ));
        }
    };

    let applied_catalog = match catalog_plan.apply(app) {
        Ok(applied) => applied,
        Err(error) => {
            drop(transaction);
            if let Some(isolated) = owned_isolated.as_deref() {
                restore_owned_isolated_file(&path, isolated, &row.content_sha256)?;
            }
            return Err(error);
        }
    };
    if let Err(error) = transaction.commit() {
        let catalog_rollback = applied_catalog.rollback();
        let file_rollback = if let Some(isolated) = owned_isolated.as_deref() {
            restore_owned_isolated_file(&path, isolated, &row.content_sha256)
        } else {
            Ok(())
        };
        catalog_rollback?;
        file_rollback?;
        return Err(db_err!(
            "failed to commit managed Codex profile deletion: {error}"
        ));
    }
    if let Some(isolated) = owned_isolated.as_deref() {
        remove_owned_isolated_file(&path, isolated, &row.content_sha256)?;
    }
    external_file_preserved |= target_path_is_occupied(&path);
    Ok(CodexManagedProfileDeleteResult {
        deleted: deleted > 0,
        external_file_preserved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::MutexGuard;
    use tauri::Manager as _;

    struct ProfileTestApp {
        _lock: MutexGuard<'static, ()>,
        previous_home: Option<OsString>,
        previous_dotdir: Option<OsString>,
        home: tempfile::TempDir,
        app: tauri::App<tauri::test::MockRuntime>,
        db: db::Db,
    }

    impl ProfileTestApp {
        fn new() -> Self {
            let lock = crate::test_support::test_env_lock();
            let home = tempfile::tempdir().expect("tempdir");
            let previous_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
            let previous_dotdir = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
            std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
            std::env::set_var(
                "AIO_CODING_HUB_DOTDIR_NAME",
                ".aio-coding-hub-managed-profile-test",
            );
            crate::test_support::clear_settings_cache();
            let app = tauri::test::mock_app();
            app.manage(crate::resident::ResidentState::default());
            let db = crate::db::init(app.handle()).expect("init db");
            Self {
                _lock: lock,
                previous_home,
                previous_dotdir,
                home,
                app,
                db,
            }
        }

        fn handle(&self) -> tauri::AppHandle<tauri::test::MockRuntime> {
            self.app.handle().clone()
        }

        fn seed_model(&self) -> String {
            let provider = crate::providers::upsert(
                &self.db,
                crate::providers::ProviderUpsertParams {
                    provider_id: None,
                    cli_key: "codex".to_string(),
                    name: "Managed Profile Provider".to_string(),
                    base_urls: vec!["https://example.invalid/v1".to_string()],
                    base_url_mode: crate::providers::ProviderBaseUrlMode::Order,
                    auth_mode: Some(crate::providers::ProviderAuthMode::ApiKey),
                    api_key: Some("test-key".to_string()),
                    enabled: true,
                    cost_multiplier: 1.0,
                    priority: Some(100),
                    claude_models: None,
                    model_mapping: None,
                    availability_test_model: None,
                    limit_5h_usd: None,
                    limit_daily_usd: None,
                    daily_reset_mode: Some(crate::providers::DailyResetMode::Fixed),
                    daily_reset_time: Some("00:00:00".to_string()),
                    limit_weekly_usd: None,
                    limit_monthly_usd: None,
                    limit_total_usd: None,
                    tags: None,
                    note: None,
                    source_provider_id: None,
                    bridge_type: None,
                    stream_idle_timeout_seconds: None,
                    extension_values: None,
                    account_usage_credentials_patch: None,
                    account_usage_credentials_copy_from_provider_id: None,
                    upstream_retry_policy_override: None,
                    upstream_retry_policy_override_specified: false,
                },
            )
            .expect("seed provider");
            crate::provider_models::manual_upsert(
                &self.db,
                provider.id,
                &provider.provider_uuid,
                "grok-4.5",
            )
            .expect("seed model")
            .models
            .into_iter()
            .find(|model| model.remote_model_id == "grok-4.5")
            .expect("manual model")
            .model_uuid
        }
    }

    impl Drop for ProfileTestApp {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
                None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
            }
            match self.previous_dotdir.take() {
                Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
                None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
            }
            crate::test_support::clear_settings_cache();
        }
    }

    fn profile_capture_paths(codex_home: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(codex_home)
            .expect("read Codex home")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".aio-profile-capture-"))
            })
            .collect()
    }

    #[test]
    fn profile_name_validation_is_ascii_and_case_folded() {
        assert_eq!(
            validate_profile_name("Grok_45-Test").unwrap(),
            "grok_45-test"
        );
        for value in [
            "",
            "-bad",
            "has space",
            "../escape",
            "模型",
            "550e8400-e29b-41d4-a716-446655440000",
        ] {
            assert!(validate_profile_name(value).is_err(), "accepted {value}");
        }
        assert!(validate_profile_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn generated_profile_uses_current_top_level_format() {
        let bytes = render_profile("grok-profile").expect("render");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("model = \"aio/grok-profile\""));
        assert!(text.contains("model_provider = \"aio\""));
        assert!(!text.contains("[profiles."));
    }

    #[test]
    fn enabled_proxy_projects_profiles_into_picker_catalog_and_restores_on_delete() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        std::fs::create_dir_all(&codex_home).expect("create Codex home");
        let base_catalog_path = test_app.home.path().join("user-model-catalog.json");
        let base_catalog: serde_json::Value = serde_json::from_str(
            r#"{
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
                "additional_speed_tiers": [],
                "service_tiers": [],
                "default_service_tier": null,
                "availability_nux": null,
                "upgrade": null,
                "base_instructions": "base instructions",
                "model_messages": null,
                "include_skills_usage_instructions": false,
                "supports_reasoning_summaries": false,
                "default_reasoning_summary": "none",
                "support_verbosity": false,
                "default_verbosity": null,
                "apply_patch_tool_type": null,
                "web_search_tool_type": "text",
                "truncation_policy": {"mode": "tokens", "limit": 10000},
                "supports_parallel_tool_calls": false,
                "supports_image_detail_original": false,
                "context_window": 128000,
                "max_context_window": 128000,
                "auto_compact_token_limit": 100000,
                "comp_hash": null,
                "effective_context_window_percent": 95,
                "experimental_supported_tools": [],
                "input_modalities": ["text"],
                "supports_search_tool": false,
                "use_responses_lite": false,
                "auto_review_model_override": null,
                "tool_mode": null,
                "multi_agent_version": null,
                "future_required_field": {"kept": true}
            }]
        }"#,
        )
        .expect("parse base catalog fixture");
        std::fs::write(
            &base_catalog_path,
            serde_json::to_vec_pretty(&base_catalog).expect("serialize base catalog"),
        )
        .expect("write base catalog");
        let config_path = crate::codex_paths::codex_config_toml_path(&app).expect("config path");
        let mut original_config = toml_edit::DocumentMut::new();
        original_config["model_catalog_json"] =
            toml_edit::value(base_catalog_path.to_string_lossy().to_string());
        std::fs::write(&config_path, original_config.to_string()).expect("write config");

        let enabled = crate::cli_proxy::set_enabled(&app, "codex", true, "http://127.0.0.1:38123")
            .expect("enable proxy");
        assert!(enabled.ok, "{}", enabled.message);

        let model_uuid = test_app.seed_model();
        let profile = create(&app, &test_app.db, "Grok", &model_uuid).expect("create profile");
        assert_eq!(profile.canonical_model, "aio/grok");
        let profile_bytes =
            std::fs::read(codex_home.join("Grok.config.toml")).expect("read managed profile");
        assert!(String::from_utf8(profile_bytes)
            .expect("profile UTF-8")
            .contains("model = \"aio/grok\""));

        let active_config = std::fs::read_to_string(&config_path).expect("read active config");
        let active_config = active_config
            .parse::<toml_edit::DocumentMut>()
            .expect("parse active config");
        let generated_path = PathBuf::from(
            active_config["model_catalog_json"]
                .as_str()
                .expect("generated catalog path"),
        );
        assert_ne!(generated_path, base_catalog_path);
        let generated_bytes = std::fs::read(&generated_path).expect("read generated catalog");
        let generated: serde_json::Value =
            serde_json::from_slice(&generated_bytes).expect("parse generated catalog");
        assert_eq!(
            generated["future_top_level"]["kept"],
            serde_json::json!(true)
        );
        assert_eq!(
            generated["models"][0]["slug"],
            serde_json::json!("gpt-base")
        );
        assert_eq!(
            generated["models"][1]["slug"],
            serde_json::json!("aio/grok")
        );
        assert_eq!(
            generated["models"][1]["future_required_field"]["kept"],
            serde_json::json!(true)
        );

        let mut modified_generated = generated.clone();
        modified_generated["models"][1]["description"] = serde_json::json!("external modification");
        std::fs::write(
            &generated_path,
            serde_json::to_vec_pretty(&modified_generated).expect("serialize modification"),
        )
        .expect("modify generated catalog");
        let create_error = create(&app, &test_app.db, "second", &model_uuid)
            .expect_err("external catalog modification must fail closed");
        assert_eq!(create_error.code(), "CODEX_MANAGED_MODEL_CATALOG_MODIFIED");
        assert!(!codex_home.join("second.config.toml").exists());
        assert_eq!(list(&test_app.db).expect("one managed profile").len(), 1);
        std::fs::write(&generated_path, generated_bytes).expect("restore generated catalog");

        delete(&app, &test_app.db, &profile.profile_uuid).expect("delete profile");
        assert!(!generated_path.exists());
        let restored_config = std::fs::read_to_string(&config_path).expect("read restored config");
        let restored_config = restored_config
            .parse::<toml_edit::DocumentMut>()
            .expect("parse restored config");
        assert_eq!(
            restored_config["model_catalog_json"].as_str(),
            base_catalog_path.to_str()
        );

        let disabled =
            crate::cli_proxy::set_enabled(&app, "codex", false, "http://127.0.0.1:38123")
                .expect("disable proxy");
        assert!(disabled.ok, "{}", disabled.message);
    }

    #[test]
    #[ignore = "requires an installed Codex CLI; run manually for picker compatibility"]
    fn installed_codex_reads_the_generated_picker_alias() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let enabled = crate::cli_proxy::set_enabled(&app, "codex", true, "http://127.0.0.1:38124")
            .expect("enable proxy");
        assert!(enabled.ok, "{}", enabled.message);

        let launch = crate::cli_manager::codex_launch_spec(&app)
            .expect("resolve Codex")
            .expect("Codex CLI installed");
        assert!(launch.executable.is_file());
        let model_uuid = test_app.seed_model();
        let profile =
            create(&app, &test_app.db, "real-smoke", &model_uuid).expect("create managed profile");
        let catalog = crate::codex_model_catalog::codex_model_catalog_get(&app)
            .expect("read model/list from installed Codex");
        assert!(catalog
            .models
            .iter()
            .any(|model| model.model == "aio/real-smoke"));

        delete(&app, &test_app.db, &profile.profile_uuid).expect("delete profile");
        let disabled =
            crate::cli_proxy::set_enabled(&app, "codex", false, "http://127.0.0.1:38124")
                .expect("disable proxy");
        assert!(disabled.ok, "{}", disabled.message);
    }

    #[test]
    fn create_delete_and_external_file_states_preserve_ownership() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        std::fs::create_dir_all(&codex_home).expect("create Codex home");

        let external_path = codex_home.join("external.config.toml");
        std::fs::write(&external_path, b"model = \"external\"\n").expect("external file");
        let error = create(&app, &test_app.db, "external", &model_uuid)
            .expect_err("external file must not be overwritten");
        assert_eq!(error.code(), "CODEX_MANAGED_PROFILE_FILE_EXISTS");
        assert_eq!(
            std::fs::read(&external_path).expect("read external"),
            b"model = \"external\"\n"
        );

        let managed = create(&app, &test_app.db, "managed", &model_uuid).expect("create managed");
        assert_eq!(managed.file_status, CodexManagedProfileFileStatus::Managed);
        assert!(crate::shared::uuid::is_canonical_uuid_v4(
            &managed.provider_uuid
        ));
        assert_eq!(
            list(&test_app.db).expect("list managed")[0].provider_uuid,
            managed.provider_uuid
        );
        let managed_path = codex_home.join("managed.config.toml");
        assert!(managed_path.exists());
        let deleted = delete(&app, &test_app.db, &managed.profile_uuid).expect("delete managed");
        assert!(deleted.deleted);
        assert!(!deleted.external_file_preserved);
        assert!(!managed_path.exists());

        let missing = create(&app, &test_app.db, "missing", &model_uuid).expect("create missing");
        let missing_path = codex_home.join("missing.config.toml");
        std::fs::remove_file(&missing_path).expect("remove externally");
        let listed = list(&test_app.db).expect("list missing");
        assert_eq!(
            listed
                .iter()
                .find(|profile| profile.profile_uuid == missing.profile_uuid)
                .expect("missing profile")
                .file_status,
            CodexManagedProfileFileStatus::Missing
        );
        let deleted = delete(&app, &test_app.db, &missing.profile_uuid).expect("delete missing");
        assert!(deleted.deleted);
        assert!(!deleted.external_file_preserved);

        let modified =
            create(&app, &test_app.db, "modified", &model_uuid).expect("create modified");
        let modified_path = codex_home.join("modified.config.toml");
        std::fs::write(&modified_path, b"model = \"user-edited\"\n").expect("modify profile");
        let listed = list(&test_app.db).expect("list modified");
        assert_eq!(
            listed
                .iter()
                .find(|profile| profile.profile_uuid == modified.profile_uuid)
                .expect("modified profile")
                .file_status,
            CodexManagedProfileFileStatus::Modified
        );
        let deleted = delete(&app, &test_app.db, &modified.profile_uuid).expect("unlink modified");
        assert!(deleted.deleted);
        assert!(deleted.external_file_preserved);
        assert_eq!(
            std::fs::read(&modified_path).expect("modified file remains"),
            b"model = \"user-edited\"\n"
        );
        assert!(list(&test_app.db).expect("final list").is_empty());
        assert!(test_app.home.path().exists());
    }

    #[test]
    fn delete_never_removes_external_replacement_after_owned_file_capture() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let profile = create(&app, &test_app.db, "delete-race", &model_uuid).expect("create");
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        let path = codex_home.join("delete-race.config.toml");
        let replacement = b"model = \"external-after-capture\"\n";

        install_profile_capture_test_hook(ProfileCapturePurpose::Delete, replacement);
        let deleted = delete(&app, &test_app.db, &profile.profile_uuid).expect("delete metadata");

        assert!(deleted.deleted);
        assert!(deleted.external_file_preserved);
        assert_eq!(
            std::fs::read(&path).expect("external replacement"),
            replacement
        );
        assert!(profile_capture_paths(&codex_home).is_empty());
        assert!(list(&test_app.db).expect("metadata removed").is_empty());
    }

    #[test]
    fn create_failure_compensation_never_removes_external_replacement() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        let path = codex_home.join("create-race.config.toml");
        let replacement = b"model = \"external-after-capture\"\n";

        FAIL_NEXT_PROFILE_METADATA_INSERT.store(true, std::sync::atomic::Ordering::SeqCst);
        install_profile_capture_test_hook(ProfileCapturePurpose::CreateCompensation, replacement);
        let error = create(&app, &test_app.db, "create-race", &model_uuid)
            .expect_err("injected metadata failure");

        assert_eq!(error.code(), "DB_ERROR");
        assert_eq!(
            std::fs::read(&path).expect("external replacement"),
            replacement
        );
        assert!(profile_capture_paths(&codex_home).is_empty());
        assert!(list(&test_app.db)
            .expect("metadata not inserted")
            .is_empty());
    }

    #[test]
    fn modified_capture_collision_preserves_both_files_and_reports_recovery_path() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let profile = create(&app, &test_app.db, "collision", &model_uuid).expect("create");
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        let path = codex_home.join("collision.config.toml");
        let captured_external = b"model = \"external-before-capture\"\n";
        let target_external = b"model = \"external-after-capture\"\n";
        std::fs::write(&path, captured_external).expect("replace before capture");

        install_profile_capture_test_hook(ProfileCapturePurpose::Delete, target_external);
        let error = delete(&app, &test_app.db, &profile.profile_uuid)
            .expect_err("occupied target requires explicit recovery");

        assert_eq!(error.code(), "CODEX_MANAGED_PROFILE_RECOVERY_REQUIRED");
        assert_eq!(
            std::fs::read(&path).expect("target external"),
            target_external
        );
        let captures = profile_capture_paths(&codex_home);
        assert_eq!(captures.len(), 1);
        assert_eq!(
            std::fs::read(&captures[0]).expect("captured external"),
            captured_external
        );
        let recovery_file_name = captures[0]
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 recovery file name");
        assert!(error.to_string().contains(recovery_file_name));
        assert_eq!(list(&test_app.db).expect("metadata retained").len(), 1);

        std::fs::remove_file(&path).expect("remove target external");
        std::fs::remove_file(&captures[0]).expect("remove recovery capture");
        delete(&app, &test_app.db, &profile.profile_uuid).expect("delete retained metadata");
    }

    #[test]
    fn delete_restores_owned_file_when_metadata_delete_fails() {
        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let profile = create(&app, &test_app.db, "recovery", &model_uuid).expect("create");
        let path = crate::codex_paths::codex_home_dir(&app)
            .expect("Codex home")
            .join("recovery.config.toml");
        let expected = std::fs::read(&path).expect("read before delete");

        FAIL_NEXT_PROFILE_METADATA_DELETE.store(true, std::sync::atomic::Ordering::SeqCst);
        let error =
            delete(&app, &test_app.db, &profile.profile_uuid).expect_err("injected failure");
        assert_eq!(error.code(), "DB_ERROR");
        assert_eq!(std::fs::read(&path).expect("restored file"), expected);
        let listed = list(&test_app.db).expect("metadata remains");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].file_status,
            CodexManagedProfileFileStatus::Managed
        );

        delete(&app, &test_app.db, &profile.profile_uuid).expect("cleanup delete");
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn create_fails_closed_for_preexisting_codex_home_symlink() {
        use std::os::unix::fs::symlink;

        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        let outside_home = tempfile::tempdir().expect("outside home");
        symlink(outside_home.path(), &codex_home).expect("symlink Codex home");

        let error = create(&app, &test_app.db, "unsafe-home", &model_uuid)
            .expect_err("pre-existing Codex home symlink must fail");

        assert_eq!(error.code(), "CODEX_MANAGED_PROFILE_HOME_UNSAFE");
        assert!(!outside_home.path().join("unsafe-home.config.toml").exists());
        assert!(list(&test_app.db)
            .expect("metadata remains empty")
            .is_empty());
        std::fs::remove_file(&codex_home).expect("remove Codex home symlink");
    }

    #[cfg(unix)]
    #[test]
    fn list_and_delete_fail_closed_when_stored_codex_home_is_replaced() {
        use std::os::unix::fs::symlink;

        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let profile = create(&app, &test_app.db, "home-replaced", &model_uuid).expect("create");
        let codex_home = crate::codex_paths::codex_home_dir(&app).expect("Codex home");
        let original_home = test_app.home.path().join(".codex-original");
        let outside_home = tempfile::tempdir().expect("outside home");
        std::fs::write(
            outside_home.path().join("home-replaced.config.toml"),
            b"model = \"external\"\n",
        )
        .expect("external profile");

        std::fs::rename(&codex_home, &original_home).expect("move original Codex home");
        symlink(outside_home.path(), &codex_home).expect("replace Codex home with symlink");

        let list_error = list(&test_app.db).expect_err("listing replaced home must fail");
        assert_eq!(list_error.code(), "CODEX_MANAGED_PROFILE_HOME_UNSAFE");
        let delete_error = delete(&app, &test_app.db, &profile.profile_uuid)
            .expect_err("deleting through replaced home must fail");
        assert_eq!(delete_error.code(), "CODEX_MANAGED_PROFILE_HOME_UNSAFE");
        assert!(outside_home
            .path()
            .join("home-replaced.config.toml")
            .exists());
        assert!(original_home.join("home-replaced.config.toml").exists());

        std::fs::remove_file(&codex_home).expect("remove replacement symlink");
        std::fs::rename(&original_home, &codex_home).expect("restore original Codex home");
        delete(&app, &test_app.db, &profile.profile_uuid).expect("cleanup profile");
    }

    #[cfg(unix)]
    #[test]
    fn create_rejects_non_utf8_codex_home_without_writing_metadata() {
        use std::os::unix::ffi::OsStringExt as _;

        let test_app = ProfileTestApp::new();
        let app = test_app.handle();
        let model_uuid = test_app.seed_model();
        let original_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
        let invalid_home = test_app
            .home
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'n', b'o', b'n', 0xff]));
        std::fs::create_dir_all(&invalid_home).expect("invalid UTF-8 home");
        std::env::set_var("AIO_CODING_HUB_HOME_DIR", &invalid_home);
        let error = create(&app, &test_app.db, "nonutf", &model_uuid)
            .expect_err("non UTF-8 home must fail");
        match original_home {
            Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
            None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
        }
        assert_eq!(error.code(), "SEC_INVALID_INPUT");
        assert!(list(&test_app.db).expect("list").is_empty());
    }
}
