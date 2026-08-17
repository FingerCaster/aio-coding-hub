//! Usage: Manage local CLI proxy configuration files (infra adapter).

mod claude;
mod codex;
mod gemini;
mod grok;

use crate::app_paths;
use crate::shared::fs::{
    read_file_with_max_len, read_optional_file_with_max_len, write_file_atomic,
};
use crate::shared::time::now_unix_seconds;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANAGED_BY: &str = "aio-coding-hub";
pub(crate) const PLACEHOLDER_KEY: &str = "aio-coding-hub";
const CLI_PROXY_MANIFEST_MAX_BYTES: usize = 256 * 1024;
pub(super) const CLI_PROXY_FILE_MAX_BYTES: usize = 1024 * 1024;
const CODEX_MANAGED_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const CODEX_MANAGED_CATALOG_FILE_NAME: &str = "managed-model-catalog.json";

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
type CodexOauthSyncTestHook = Box<dyn FnMut() -> Option<String> + Send>;

#[cfg(test)]
type CodexOauthAfterLockTestHook = Box<dyn FnMut() + Send>;

#[cfg(test)]
fn codex_oauth_sync_test_hook() -> &'static std::sync::Mutex<Option<CodexOauthSyncTestHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<CodexOauthSyncTestHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn codex_oauth_after_lock_test_hook(
) -> &'static std::sync::Mutex<Option<CodexOauthAfterLockTestHook>> {
    static HOOK: std::sync::OnceLock<std::sync::Mutex<Option<CodexOauthAfterLockTestHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
pub(crate) fn set_codex_oauth_sync_test_hook(hook: CodexOauthSyncTestHook) {
    *codex_oauth_sync_test_hook()
        .lock()
        .expect("Codex OAuth sync test hook") = Some(hook);
}

#[cfg(test)]
pub(crate) fn clear_codex_oauth_sync_test_hook() {
    *codex_oauth_sync_test_hook()
        .lock()
        .expect("Codex OAuth sync test hook") = None;
}

#[cfg(test)]
pub(crate) fn set_codex_oauth_after_lock_test_hook(hook: CodexOauthAfterLockTestHook) {
    *codex_oauth_after_lock_test_hook()
        .lock()
        .expect("Codex OAuth after-lock test hook") = Some(hook);
}

#[cfg(test)]
pub(crate) fn clear_codex_oauth_after_lock_test_hook() {
    *codex_oauth_after_lock_test_hook()
        .lock()
        .expect("Codex OAuth after-lock test hook") = None;
}

#[cfg(test)]
fn run_codex_oauth_sync_test_hook() -> Option<String> {
    codex_oauth_sync_test_hook()
        .lock()
        .expect("Codex OAuth sync test hook")
        .as_mut()
        .and_then(|hook| hook())
}

#[cfg(test)]
fn run_codex_oauth_after_lock_test_hook() {
    if let Some(hook) = codex_oauth_after_lock_test_hook()
        .lock()
        .expect("Codex OAuth after-lock test hook")
        .as_mut()
    {
        hook();
    }
}

// -- Public types -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CliProxyStatus {
    pub cli_key: String,
    pub enabled: bool,
    pub base_origin: Option<String>,
    pub current_gateway_origin: Option<String>,
    pub applied_to_current_gateway: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CliProxyResult {
    pub trace_id: String,
    pub cli_key: String,
    pub enabled: bool,
    pub ok: bool,
    pub error_code: Option<String>,
    pub message: String,
    pub base_origin: Option<String>,
}

impl CliProxyResult {
    fn success(
        trace_id: String,
        cli_key: &str,
        enabled: bool,
        message: String,
        base_origin: Option<String>,
    ) -> Self {
        Self {
            trace_id,
            cli_key: cli_key.to_string(),
            enabled,
            ok: true,
            error_code: None,
            message,
            base_origin,
        }
    }

    fn failure(
        trace_id: String,
        cli_key: &str,
        enabled: bool,
        error_code: &str,
        message: String,
        base_origin: Option<String>,
    ) -> Self {
        Self {
            trace_id,
            cli_key: cli_key.to_string(),
            enabled,
            ok: false,
            error_code: Some(error_code.to_string()),
            message,
            base_origin,
        }
    }
}

// -- Internal types ---------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupFileEntry {
    kind: String,
    path: String,
    existed: bool,
    backup_rel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliProxyManifest {
    schema_version: u32,
    managed_by: String,
    cli_key: String,
    enabled: bool,
    base_origin: Option<String>,
    created_at: i64,
    updated_at: i64,
    files: Vec<BackupFileEntry>,
}

#[derive(Debug, Clone)]
struct TargetFile {
    kind: &'static str,
    path: PathBuf,
    backup_name: &'static str,
}

#[derive(Debug, Clone)]
struct PendingBackupEntry {
    kind: String,
    path: PathBuf,
    backup_name: &'static str,
    existed: bool,
    backup_bytes: Option<Vec<u8>>,
}

fn codex_oauth_compatible_proxy_mode<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    crate::settings::read(app)
        .map(|settings| settings.codex_oauth_compatible_proxy_mode)
        .unwrap_or(false)
}

fn should_skip_manifest_entry_for_current_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    kind: &str,
) -> bool {
    cli_key == "codex" && kind == "codex_auth_json" && codex_oauth_compatible_proxy_mode(app)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    path: PathBuf,
    existed: bool,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct BoundedFileSnapshot {
    snapshot: FileSnapshot,
    max_len: usize,
}

/// Snapshot captured before config import publishes a different Codex home.
/// Applying the rebind adds the new-home files to this set and returns a token
/// that can conditionally restore every file owned by the rebind.
#[derive(Debug)]
pub(crate) struct PreparedCodexHomeRebind {
    before: Vec<BoundedFileSnapshot>,
    expected_targets: Vec<(String, PathBuf)>,
}

#[derive(Debug)]
pub(crate) struct AppliedCodexHomeRebind {
    changes: Vec<(BoundedFileSnapshot, FileSnapshot)>,
}

impl AppliedCodexHomeRebind {
    pub(crate) fn rollback(self) -> crate::shared::error::AppResult<()> {
        let mut errors = Vec::new();
        for (before, committed) in self.changes.iter().rev() {
            if let Err(error) =
                restore_file_snapshot_conditionally(&before.snapshot, committed, before.max_len)
            {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::shared::error::AppError::new(
                "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                format!(
                    "config import could not roll back the Codex home rebind: {}",
                    errors.join("; ")
                ),
            ))
        }
    }
}

#[derive(Debug)]
struct AppliedProxyConfig {
    changes: Vec<(FileSnapshot, FileSnapshot)>,
}

impl AppliedProxyConfig {
    fn committed_snapshots_for(&self, paths: &[FileSnapshot]) -> Vec<FileSnapshot> {
        paths
            .iter()
            .map(|before| {
                self.changes
                    .iter()
                    .find(|(change_before, _)| change_before.path == before.path)
                    .map(|(_, committed)| committed.clone())
                    .unwrap_or_else(|| before.clone())
            })
            .collect()
    }

    fn rollback(&self) -> crate::shared::error::AppResult<()> {
        let mut errors = Vec::new();
        for (before, committed) in self.changes.iter().rev() {
            if let Err(error) =
                restore_file_snapshot_conditionally(before, committed, CLI_PROXY_FILE_MAX_BYTES)
            {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "proxy config rollback could not restore all owned files: {}",
                errors.join("; ")
            )
            .into())
        }
    }
}

fn apply_file_snapshot_changes(
    prepared: Vec<(FileSnapshot, FileSnapshot)>,
    max_len: usize,
) -> crate::shared::error::AppResult<AppliedProxyConfig> {
    for (before, _) in &prepared {
        if snapshot_file_with_max_len(before.path.as_path(), max_len)? != *before {
            return Err(format!(
                "CLI_PROXY_CONFIG_DRIFT: {} changed while preparing proxy projection",
                before.path.display()
            )
            .into());
        }
    }

    let mut applied = AppliedProxyConfig {
        changes: Vec::with_capacity(prepared.len()),
    };
    for (before, committed) in prepared {
        if let Err(error) = restore_file_snapshot_exact(&committed, max_len) {
            // A failed atomic finalization may still have committed the target.
            // Include this stage so rollback accepts either before or committed.
            applied.changes.push((before, committed));
            return match applied.rollback() {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(crate::shared::error::AppError::new(
                    "CLI_PROXY_APPLY_RECOVERY_REQUIRED",
                    format!(
                        "proxy config write failed ({error}); rollback failed: {rollback_error}"
                    ),
                )),
            };
        }
        applied.changes.push((before, committed));
    }
    Ok(applied)
}

#[derive(Debug)]
struct CodexCatalogLifecycleSnapshot {
    targets_before: Vec<FileSnapshot>,
    manifest_before: FileSnapshot,
    generated_before: FileSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexProxyBaseline {
    pub(crate) config_path: PathBuf,
    pub(crate) config_backup_path: Option<PathBuf>,
    pub(crate) config_bytes: Option<Vec<u8>>,
    pub(crate) base_origin: String,
}

// -- Shared helpers ---------------------------------------------------------

fn new_trace_id(prefix: &str) -> String {
    let ts = now_unix_seconds();
    let seq = TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{ts}-{seq}")
}

fn validate_cli_key(cli_key: &str) -> crate::shared::error::AppResult<()> {
    crate::shared::cli_key::validate_cli_key(cli_key)
}

fn home_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    crate::app_paths::home_dir(app)
}

fn cli_proxy_root_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(app_paths::app_data_dir(app)?
        .join("cli-proxy")
        .join(cli_key))
}

fn cli_proxy_files_dir(root: &Path) -> PathBuf {
    root.join("files")
}

fn cli_proxy_safety_dir(root: &Path) -> PathBuf {
    root.join("restore-safety")
}

fn cli_proxy_manifest_path(root: &Path) -> PathBuf {
    root.join("manifest.json")
}

fn ensure_cli_proxy_bytes_len(
    bytes: &[u8],
    max_len: usize,
    label: &str,
) -> crate::shared::error::AppResult<()> {
    if bytes.len() > max_len {
        return Err(format!("SEC_INVALID_INPUT: {label} too large (max {max_len} bytes)").into());
    }
    Ok(())
}

pub(super) fn read_optional_cli_proxy_file(
    path: &Path,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    read_optional_file_with_max_len(path, CLI_PROXY_FILE_MAX_BYTES)
}

pub(super) fn read_cli_proxy_file(path: &Path) -> crate::shared::error::AppResult<Vec<u8>> {
    read_file_with_max_len(path, CLI_PROXY_FILE_MAX_BYTES)
}

pub(super) fn write_cli_proxy_file_atomic(
    path: &Path,
    bytes: &[u8],
) -> crate::shared::error::AppResult<()> {
    ensure_cli_proxy_bytes_len(
        bytes,
        CLI_PROXY_FILE_MAX_BYTES,
        &format!("CLI proxy file {}", path.display()),
    )?;
    write_file_atomic(path, bytes)
}

fn read_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<Option<CliProxyManifest>> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let path = cli_proxy_manifest_path(&root);
    let Some(content) = read_optional_file_with_max_len(&path, CLI_PROXY_MANIFEST_MAX_BYTES)?
    else {
        return Ok(None);
    };

    let manifest: CliProxyManifest = serde_json::from_slice(&content)
        .map_err(|e| format!("failed to parse manifest.json: {e}"))?;

    if manifest.managed_by != MANAGED_BY {
        return Err(format!(
            "manifest managed_by mismatch: expected {MANAGED_BY}, got {}",
            manifest.managed_by
        )
        .into());
    }

    Ok(Some(manifest))
}

fn write_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create {}: {e}", root.display()))?;
    let path = cli_proxy_manifest_path(&root);

    let bytes = serialize_manifest(manifest)?;
    write_file_atomic(&path, &bytes)?;
    Ok(())
}

fn serialize_manifest(manifest: &CliProxyManifest) -> crate::shared::error::AppResult<Vec<u8>> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| format!("failed to serialize manifest.json: {e}"))?;
    ensure_cli_proxy_bytes_len(&bytes, CLI_PROXY_MANIFEST_MAX_BYTES, "CLI proxy manifest")?;
    Ok(bytes)
}

pub(crate) fn codex_enabled_proxy_baseline<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Option<CodexProxyBaseline>> {
    let Some(manifest) = read_manifest(app, "codex")? else {
        return Ok(None);
    };
    if !manifest.enabled {
        return Ok(None);
    }
    let entry = manifest
        .files
        .iter()
        .find(|entry| entry.kind == "codex_config_toml")
        .ok_or_else(|| {
            "CLI_PROXY_INVALID_MANIFEST: missing Codex config backup entry".to_string()
        })?;
    let config_path = codex::codex_config_path(app)?;
    if Path::new(&entry.path) != config_path {
        return Err(
            "CODEX_MANAGED_MODEL_PROXY_REBIND_REQUIRED: Codex proxy target path changed".into(),
        );
    }

    let (config_bytes, config_backup_path) = if entry.existed {
        let rel = entry.backup_rel.as_deref().ok_or_else(|| {
            "CLI_PROXY_INVALID_MANIFEST: missing Codex config backup path".to_string()
        })?;
        let root = cli_proxy_root_dir(app, "codex")?;
        let files_dir = cli_proxy_files_dir(&root);
        let backup_path = safe_backup_path(&files_dir, rel)?;
        (
            Some(
                read_cli_proxy_file(&backup_path)
                    .map_err(|err| format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}"))?,
            ),
            Some(backup_path),
        )
    } else {
        (None, None)
    };
    let base_origin = manifest.base_origin.clone().ok_or_else(|| {
        "CLI_PROXY_INVALID_MANIFEST: enabled Codex proxy is missing base origin".to_string()
    })?;

    Ok(Some(CodexProxyBaseline {
        config_path,
        config_backup_path,
        config_bytes,
        base_origin,
    }))
}

pub(crate) fn codex_proxy_config_is_applied<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
) -> bool {
    is_proxy_config_applied(app, "codex", base_origin)
}

// -- Dispatch: target_files -------------------------------------------------

fn target_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<TargetFile>> {
    validate_cli_key(cli_key)?;

    match cli_key {
        "claude" => Ok(vec![TargetFile {
            kind: "claude_settings_json",
            path: claude::claude_settings_path(app)?,
            backup_name: "settings.json",
        }]),
        "codex" => codex_target_files_for_settings(app, &crate::settings::read(app)?),
        "gemini" => Ok(vec![TargetFile {
            kind: "gemini_env",
            path: gemini::gemini_env_path(app)?,
            backup_name: ".env",
        }]),
        "grok" => Ok(vec![TargetFile {
            kind: "grok_config_toml",
            path: grok::grok_config_path(app)?,
            backup_name: "config.toml",
        }]),
        _ => Err(format!("SEC_INVALID_INPUT: unknown cli_key={cli_key}").into()),
    }
}

fn codex_target_files_for_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_settings: &crate::settings::AppSettings,
) -> crate::shared::error::AppResult<Vec<TargetFile>> {
    let home = crate::codex_paths::codex_home_dir_for_settings(app, app_settings)?;
    let mut files = vec![TargetFile {
        kind: "codex_config_toml",
        path: home.join("config.toml"),
        backup_name: "config.toml",
    }];
    if !app_settings.codex_oauth_compatible_proxy_mode {
        files.push(TargetFile {
            kind: "codex_auth_json",
            path: home.join("auth.json"),
            backup_name: "auth.json",
        });
    }
    Ok(files)
}

// -- Dispatch: is_proxy_config_applied --------------------------------------

fn is_proxy_config_applied<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    base_origin: &str,
) -> bool {
    match cli_key {
        "claude" => claude::is_proxy_config_applied(app, base_origin),
        "codex" => codex::is_proxy_config_applied(app, base_origin),
        "gemini" => gemini::is_proxy_config_applied(app, base_origin),
        "grok" => grok::is_proxy_config_applied(app, base_origin),
        _ => false,
    }
}

// -- Dispatch: apply_proxy_config -------------------------------------------

fn apply_proxy_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    base_origin: &str,
) -> crate::shared::error::AppResult<AppliedProxyConfig> {
    validate_cli_key(cli_key)?;

    let targets = target_files(app, cli_key)?;
    let mut prepared_writes: Vec<(FileSnapshot, FileSnapshot)> = Vec::with_capacity(targets.len());

    for t in targets {
        if should_skip_manifest_entry_for_current_settings(app, cli_key, t.kind) {
            continue;
        }
        let current = read_optional_cli_proxy_file(&t.path)?;

        let bytes = match cli_key {
            "claude" => {
                match claude::build_claude_settings_json(
                    current.clone(),
                    &format!("{base_origin}/claude"),
                ) {
                    Ok(b) => b,
                    Err(err) => {
                        // Preserve the original file — never clobber user data on parse failure.
                        if let Some(original_bytes) = current.as_ref() {
                            let backup_path = t.path.with_extension("json.invalid-backup");
                            let _ = write_cli_proxy_file_atomic(&backup_path, original_bytes);
                            tracing::warn!(
                                "cli_proxy: preserved invalid config as {}",
                                backup_path.display()
                            );
                        }
                        return Err(err);
                    }
                }
            }
            "codex" => {
                if t.kind == "codex_config_toml" {
                    let (canonical, previous_base_url) =
                        codex_projection_baseline(app, current.clone())?;
                    let build_result = project_codex_config_from_baseline(
                        app,
                        canonical,
                        current.as_deref(),
                        base_origin,
                        previous_base_url.as_deref(),
                    );
                    match build_result {
                        Ok(b) => b,
                        Err(err) => {
                            if err.to_string().contains("CLI_PROXY_INVALID_TOML") {
                                if let Some(original_bytes) = current.as_ref() {
                                    let backup_path = t.path.with_extension("toml.invalid-backup");
                                    let _ =
                                        write_cli_proxy_file_atomic(&backup_path, original_bytes);
                                }
                            }
                            return Err(err);
                        }
                    }
                } else {
                    match codex::build_codex_auth_json(current.clone()) {
                        Ok(b) => b,
                        Err(err) => {
                            if err.to_string().contains("CLI_PROXY_INVALID_AUTH_JSON") {
                                if let Some(original_bytes) = current.as_ref() {
                                    let backup_path = t.path.with_extension("json.invalid-backup");
                                    let _ =
                                        write_cli_proxy_file_atomic(&backup_path, original_bytes);
                                }
                            }
                            return Err(err);
                        }
                    }
                }
            }
            "gemini" => {
                gemini::build_gemini_env(current.clone(), &format!("{base_origin}/gemini"))?
            }
            "grok" => {
                grok::apply_proxy_config(app, base_origin)?;
                continue;
            }
            _ => return Err(format!("SEC_INVALID_INPUT: unknown cli_key={cli_key}").into()),
        };

        ensure_cli_proxy_bytes_len(
            &bytes,
            CLI_PROXY_FILE_MAX_BYTES,
            &format!("CLI proxy file {}", t.path.display()),
        )?;
        prepared_writes.push((
            FileSnapshot {
                path: t.path.clone(),
                existed: current.is_some(),
                bytes: current,
            },
            FileSnapshot {
                path: t.path,
                existed: true,
                bytes: Some(bytes),
            },
        ));
    }

    apply_file_snapshot_changes(prepared_writes, CLI_PROXY_FILE_MAX_BYTES)
}

pub(crate) fn project_codex_config_from_baseline<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    baseline: Option<Vec<u8>>,
    current: Option<&[u8]>,
    base_origin: &str,
    previous_base_url: Option<&str>,
) -> crate::shared::error::AppResult<Vec<u8>> {
    let projected = codex::build_codex_config_toml_for_existing_proxy(
        baseline.clone(),
        &format!("{}/v1", base_origin.trim_end_matches('/')),
        previous_base_url,
        codex::CodexConfigPlatform::current(),
        codex_oauth_compatible_proxy_mode(app),
    )?;
    crate::codex_model_catalog::managed::preserve_active_binding(
        app,
        baseline.as_deref(),
        current,
        &projected,
    )
}

fn codex_projection_baseline<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    fallback: Option<Vec<u8>>,
) -> crate::shared::error::AppResult<(Option<Vec<u8>>, Option<String>)> {
    let Some(manifest) = read_manifest(app, "codex")? else {
        return Ok((fallback, None));
    };
    let previous_base_url = manifest
        .base_origin
        .as_deref()
        .map(|origin| format!("{}/v1", origin.trim_end_matches('/')));
    let Some(entry) = manifest
        .files
        .iter()
        .find(|entry| entry.kind == "codex_config_toml")
    else {
        return Ok((fallback, previous_base_url));
    };
    if !entry.existed {
        return Ok((None, previous_base_url));
    }
    let rel = entry.backup_rel.as_deref().ok_or_else(|| {
        "CLI_PROXY_INVALID_MANIFEST: missing Codex config backup path".to_string()
    })?;
    let root = cli_proxy_root_dir(app, "codex")?;
    let backup_path = safe_backup_path(&cli_proxy_files_dir(&root), rel)?;
    Ok((Some(read_cli_proxy_file(&backup_path)?), previous_base_url))
}

// -- Dispatch: restore_from_manifest ----------------------------------------

fn restore_from_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    restore_from_manifest_with_applied(app, manifest).map(|_| ())
}

/// Restore a manifest while retaining a conditional before/committed token for
/// every target.  The token is required by callers that still have later
/// catalog or manifest stages to complete: a failure in those stages must not
/// blindly write the old target bytes over a concurrent user edit.
fn restore_from_manifest_with_applied<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<AppliedProxyConfig> {
    let cli_key = manifest.cli_key.as_str();
    validate_cli_key(cli_key)?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    let safety_dir = cli_proxy_safety_dir(&root);
    std::fs::create_dir_all(&safety_dir)
        .map_err(|e| format!("failed to create {}: {e}", safety_dir.display()))?;

    let ts = now_unix_seconds();
    let mut applied = AppliedProxyConfig {
        changes: Vec::with_capacity(manifest.files.len()),
    };

    for entry in &manifest.files {
        if should_skip_manifest_entry_for_current_settings(app, cli_key, &entry.kind) {
            continue;
        }

        let target_path = PathBuf::from(&entry.path);
        let before = match snapshot_file(&target_path) {
            Ok(snapshot) => snapshot,
            Err(error) => return Err(finish_applied_restore_error(applied, error)),
        };
        if entry.kind == "grok_config_toml" {
            if let Err(error) = ensure_snapshot_unchanged_for_restore(&before) {
                return Err(finish_manifest_restore_error(
                    applied,
                    error,
                    &target_path,
                    &before,
                ));
            }
            let backup_path = entry.backup_rel.as_ref().map(|rel| files_dir.join(rel));
            if let Err(error) =
                grok::merge_restore_grok_config(&target_path, backup_path.as_deref())
            {
                return Err(finish_manifest_restore_error(
                    applied,
                    error,
                    &target_path,
                    &before,
                ));
            }
            let committed = match snapshot_file(&target_path) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return Err(finish_manifest_restore_error(
                        applied,
                        error,
                        &target_path,
                        &before,
                    ));
                }
            };
            applied.changes.push((before, committed));
            continue;
        }
        if entry.existed {
            let Some(rel) = entry.backup_rel.as_ref() else {
                return Err(finish_manifest_restore_error(
                    applied,
                    format!("missing backup_rel for {}", entry.kind).into(),
                    &target_path,
                    &before,
                ));
            };
            let backup_path = match safe_backup_path(&files_dir, rel) {
                Ok(path) => path,
                Err(error) => {
                    return Err(finish_manifest_restore_error(
                        applied,
                        error,
                        &target_path,
                        &before,
                    ));
                }
            };

            // Check the target immediately before the write.  This closes the
            // read/modify/write window for callers that race a home/config
            // change with disable or restore.
            if let Err(error) = ensure_snapshot_unchanged_for_restore(&before) {
                return Err(finish_manifest_restore_error(
                    applied,
                    error,
                    &target_path,
                    &before,
                ));
            }

            // Use merge-restore for known file kinds to preserve user changes
            // made while the proxy was enabled.
            let restore_result = match entry.kind.as_str() {
                "claude_settings_json" => {
                    claude::merge_restore_claude_settings_json(&target_path, &backup_path)
                }
                "codex_auth_json" => {
                    codex::merge_restore_codex_auth_json(&target_path, &backup_path)
                }
                "codex_config_toml" => {
                    codex::merge_restore_codex_config_toml(&target_path, &backup_path)
                }
                "gemini_env" => gemini::merge_restore_gemini_env(&target_path, &backup_path),
                _ => Ok(()),
            };
            if let Err(error) = restore_result {
                return Err(finish_manifest_restore_error(
                    applied,
                    error,
                    &target_path,
                    &before,
                ));
            }

            if !matches!(
                entry.kind.as_str(),
                "claude_settings_json" | "codex_auth_json" | "codex_config_toml" | "gemini_env"
            ) {
                // Fallback: full restore for unknown file kinds.
                let bytes = match read_cli_proxy_file(&backup_path) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return Err(finish_manifest_restore_error(
                            applied,
                            error,
                            &target_path,
                            &before,
                        ));
                    }
                };
                if let Err(error) = write_cli_proxy_file_atomic(&target_path, &bytes) {
                    return Err(finish_manifest_restore_error(
                        applied,
                        error,
                        &target_path,
                        &before,
                    ));
                }
            }

            let committed = match snapshot_file(&target_path) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return Err(finish_manifest_restore_error(
                        applied,
                        error,
                        &target_path,
                        &before,
                    ));
                }
            };
            applied.changes.push((before, committed));
            continue;
        }

        if !before.existed {
            // Keep an explicit no-op token so callers can align this entry
            // with a later lifecycle snapshot even when the target is absent.
            applied.changes.push((before.clone(), before));
            continue;
        }

        // If the file did not exist before enabling proxy, restore to "absent".
        // Safety copy current content before removal.
        if let Err(error) = ensure_snapshot_unchanged_for_restore(&before) {
            return Err(finish_manifest_restore_error(
                applied,
                error,
                &target_path,
                &before,
            ));
        }
        if let Some(bytes) = before.bytes.as_deref() {
            let safe_name = format!("{ts}_{}_before_remove", entry.kind);
            let safe_path = safety_dir.join(safe_name);
            if let Err(error) = write_cli_proxy_file_atomic(&safe_path, bytes) {
                return Err(finish_manifest_restore_error(
                    applied,
                    error,
                    &target_path,
                    &before,
                ));
            }
        }

        if let Err(error) = std::fs::remove_file(&target_path)
            .map_err(|e| format!("failed to remove {}: {e}", target_path.display()).into())
        {
            return Err(finish_manifest_restore_error(
                applied,
                error,
                &target_path,
                &before,
            ));
        }
        let committed = match snapshot_file(&target_path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Err(finish_manifest_restore_error(
                    applied,
                    error,
                    &target_path,
                    &before,
                ));
            }
        };
        applied.changes.push((before, committed));
    }

    Ok(applied)
}

fn ensure_snapshot_unchanged_for_restore(
    before: &FileSnapshot,
) -> crate::shared::error::AppResult<()> {
    let current = snapshot_file_with_max_len(&before.path, CLI_PROXY_FILE_MAX_BYTES)?;
    if current == *before {
        Ok(())
    } else {
        Err(format!(
            "CLI_PROXY_CONFIG_DRIFT: {} changed while restoring proxy manifest",
            before.path.display()
        )
        .into())
    }
}

fn finish_manifest_restore_error(
    applied: AppliedProxyConfig,
    error: crate::shared::error::AppError,
    failed_path: &Path,
    failed_before: &FileSnapshot,
) -> crate::shared::error::AppError {
    let mut recovery_errors = Vec::new();
    if let Ok(current) = snapshot_file(failed_path) {
        if current != *failed_before {
            recovery_errors.push(format!(
                "{} changed while its restore result was uncertain",
                failed_path.display()
            ));
        }
    } else {
        recovery_errors.push(format!(
            "could not inspect {} after restore failure",
            failed_path.display()
        ));
    }
    if let Err(rollback_error) = applied.rollback() {
        recovery_errors.push(rollback_error.to_string());
    }
    if recovery_errors.is_empty() {
        error
    } else {
        crate::shared::error::AppError::new(
            "CLI_PROXY_RESTORE_RECOVERY_REQUIRED",
            format!("{error}; {}", recovery_errors.join("; ")),
        )
    }
}

fn finish_applied_restore_error(
    applied: AppliedProxyConfig,
    error: crate::shared::error::AppError,
) -> crate::shared::error::AppError {
    match applied.rollback() {
        Ok(()) => error,
        Err(rollback_error) => crate::shared::error::AppError::new(
            "CLI_PROXY_RESTORE_RECOVERY_REQUIRED",
            format!("{error}; rollback failed: {rollback_error}"),
        ),
    }
}

// -- Shared backup / snapshot helpers ---------------------------------------

pub fn backup_file_path_for_enabled_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    kind: &str,
    backup_name: &str,
) -> crate::shared::error::AppResult<Option<PathBuf>> {
    validate_cli_key(cli_key)?;

    let Some(mut manifest) = read_manifest(app, cli_key)? else {
        return Ok(None);
    };
    if !manifest.enabled {
        return Ok(None);
    }

    let target = target_files(app, cli_key)?
        .into_iter()
        .find(|t| t.kind == kind)
        .ok_or_else(|| {
            format!("SEC_INVALID_INPUT: unknown cli backup kind={kind} for cli_key={cli_key}")
        })?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let mut changed = false;
    let target_path = target.path.to_string_lossy().to_string();

    let backup_rel = if let Some(entry) = manifest.files.iter_mut().find(|entry| entry.kind == kind)
    {
        if entry.path != target_path {
            entry.path = target_path.clone();
            changed = true;
        }
        if !entry.existed {
            entry.existed = true;
            changed = true;
        }
        if entry.backup_rel.is_none() {
            entry.backup_rel = Some(backup_name.to_string());
            changed = true;
        }
        entry.backup_rel.clone()
    } else {
        let backup_rel = Some(backup_name.to_string());
        manifest.files.push(BackupFileEntry {
            kind: kind.to_string(),
            path: target_path,
            existed: true,
            backup_rel: backup_rel.clone(),
        });
        changed = true;
        backup_rel
    };

    // Validate the resolved backup path before committing a repaired manifest.
    // After a successful manifest write there must be no fallible path step, so
    // callers can use the resulting bytes as an exact committed token.
    let backup_path = backup_rel
        .map(|rel| safe_backup_path(&files_dir, &rel))
        .transpose()?;

    if changed {
        manifest.updated_at = now_unix_seconds();
        write_manifest(app, cli_key, &manifest)?;
    }

    Ok(backup_path)
}

fn safe_backup_path(files_dir: &Path, rel: &str) -> crate::shared::error::AppResult<PathBuf> {
    let rel_path = Path::new(rel);
    if rel.trim().is_empty()
        || rel_path.is_absolute()
        || rel_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("SEC_INVALID_INPUT: invalid CLI proxy backup_rel={rel}").into());
    }

    let mut path = files_dir.to_path_buf();
    for component in rel_path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(format!("SEC_INVALID_INPUT: invalid CLI proxy backup_rel={rel}").into());
        };
        path.push(part);
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "SEC_INVALID_INPUT: refusing to use symlink CLI proxy backup path={}",
                    path.display()
                )
                .into());
            }
        }
    }

    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "SEC_INVALID_INPUT: refusing to use symlink CLI proxy backup path={}",
                path.display()
            )
            .into());
        }
    }
    Ok(path)
}

fn backup_for_enable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    base_origin: &str,
    existing: Option<CliProxyManifest>,
) -> crate::shared::error::AppResult<CliProxyManifest> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let now = now_unix_seconds();
    let targets = target_files(app, cli_key)?;

    let mut entries = Vec::with_capacity(targets.len());
    for t in targets {
        let read_bytes = read_optional_cli_proxy_file(&t.path)?;
        let existed = read_bytes.is_some();
        let backup_rel = if let Some(bytes) = read_bytes {
            let backup_path = files_dir.join(t.backup_name);
            write_cli_proxy_file_atomic(&backup_path, &bytes)?;
            Some(t.backup_name.to_string())
        } else {
            None
        };

        entries.push(BackupFileEntry {
            kind: t.kind.to_string(),
            path: t.path.to_string_lossy().to_string(),
            existed,
            backup_rel,
        });
    }

    let created_at = existing.as_ref().map(|m| m.created_at).unwrap_or(now);

    Ok(CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: cli_key.to_string(),
        enabled: true,
        base_origin: Some(base_origin.to_string()),
        created_at,
        updated_at: now,
        files: entries,
    })
}

fn ensure_manifest_has_current_targets<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    manifest: &mut CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    ensure_manifest_has_current_targets_with_applied(app, cli_key, manifest).map(|_| ())
}

fn ensure_manifest_has_current_targets_with_applied<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    manifest: &mut CliProxyManifest,
) -> crate::shared::error::AppResult<AppliedProxyConfig> {
    let targets = target_files(app, cli_key)?;
    if targets
        .iter()
        .all(|target| manifest.files.iter().any(|entry| entry.kind == target.kind))
    {
        return Ok(AppliedProxyConfig {
            changes: Vec::new(),
        });
    }

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let mut entries = Vec::new();
    let mut prepared = Vec::new();
    for target in targets {
        if manifest.files.iter().any(|entry| entry.kind == target.kind) {
            continue;
        }

        let read_bytes = read_optional_cli_proxy_file(&target.path)?;
        let existed = read_bytes.is_some();
        let backup_rel = if let Some(bytes) = read_bytes.as_ref() {
            let backup_path = files_dir.join(target.backup_name);
            prepared.push((
                snapshot_file(&backup_path)?,
                FileSnapshot {
                    path: backup_path,
                    existed: true,
                    bytes: Some(bytes.clone()),
                },
            ));
            Some(target.backup_name.to_string())
        } else {
            None
        };

        entries.push(BackupFileEntry {
            kind: target.kind.to_string(),
            path: target.path.to_string_lossy().to_string(),
            existed,
            backup_rel,
        });
    }

    let applied = apply_file_snapshot_changes(prepared, CLI_PROXY_FILE_MAX_BYTES)?;
    manifest.files.extend(entries);
    Ok(applied)
}

fn capture_current_target_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<PendingBackupEntry>> {
    let targets = target_files(app, cli_key)?;
    let mut captured = Vec::with_capacity(targets.len());

    for target in targets {
        let backup_bytes = read_optional_cli_proxy_file(&target.path)?;

        captured.push(PendingBackupEntry {
            kind: target.kind.to_string(),
            path: target.path,
            backup_name: target.backup_name,
            existed: backup_bytes.is_some(),
            backup_bytes,
        });
    }

    Ok(captured)
}

fn manifest_target_paths_changed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<bool> {
    let targets = target_files(app, manifest.cli_key.as_str())?;
    for target in targets {
        let Some(entry) = manifest
            .files
            .iter()
            .find(|entry| entry.kind == target.kind)
        else {
            continue;
        };
        let changed = if manifest.cli_key == "grok" {
            !crate::grok_config::paths_equivalent(Path::new(&entry.path), &target.path)?
        } else {
            Path::new(&entry.path) != target.path
        };
        if changed {
            return Ok(true);
        }
    }

    Ok(false)
}

fn write_captured_backups<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    captured: &[PendingBackupEntry],
) -> crate::shared::error::AppResult<AppliedProxyConfig> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let mut prepared = Vec::with_capacity(captured.len());
    for entry in captured {
        if let Some(bytes) = entry.backup_bytes.as_ref() {
            let backup_path = files_dir.join(entry.backup_name);
            prepared.push((
                snapshot_file(&backup_path)?,
                FileSnapshot {
                    path: backup_path,
                    existed: true,
                    bytes: Some(bytes.clone()),
                },
            ));
        }
    }

    apply_file_snapshot_changes(prepared, CLI_PROXY_FILE_MAX_BYTES)
}

fn snapshot_file(path: &Path) -> crate::shared::error::AppResult<FileSnapshot> {
    snapshot_file_with_max_len(path, CLI_PROXY_FILE_MAX_BYTES)
}

fn snapshot_file_with_max_len(
    path: &Path,
    max_len: usize,
) -> crate::shared::error::AppResult<FileSnapshot> {
    let bytes = read_optional_file_with_max_len(path, max_len)?;

    Ok(FileSnapshot {
        path: path.to_path_buf(),
        existed: bytes.is_some(),
        bytes,
    })
}

fn restore_file_snapshots(snapshots: &[FileSnapshot]) -> crate::shared::error::AppResult<()> {
    for snapshot in snapshots {
        if let Some(bytes) = snapshot.bytes.as_ref() {
            if let Some(parent) = snapshot.path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
            }
            write_cli_proxy_file_atomic(&snapshot.path, bytes)?;
            continue;
        }

        if snapshot.existed {
            return Err(format!(
                "snapshot for {} marked existed but no bytes captured",
                snapshot.path.display()
            )
            .into());
        }

        if snapshot.path.exists() {
            std::fs::remove_file(&snapshot.path)
                .map_err(|e| format!("failed to remove {}: {e}", snapshot.path.display()))?;
        }
    }

    Ok(())
}

fn restore_file_snapshot_exact(
    snapshot: &FileSnapshot,
    max_len: usize,
) -> crate::shared::error::AppResult<()> {
    if let Some(bytes) = snapshot.bytes.as_ref() {
        ensure_cli_proxy_bytes_len(
            bytes,
            max_len,
            &format!("snapshot {}", snapshot.path.display()),
        )?;
        if let Some(parent) = snapshot.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        write_file_atomic(&snapshot.path, bytes)?;
        return Ok(());
    }

    if snapshot.existed {
        return Err(format!(
            "snapshot for {} marked existed but no bytes captured",
            snapshot.path.display()
        )
        .into());
    }
    match std::fs::remove_file(&snapshot.path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {}: {error}", snapshot.path.display()).into()),
    }
}

fn restore_file_snapshot_conditionally(
    before: &FileSnapshot,
    committed: &FileSnapshot,
    max_len: usize,
) -> crate::shared::error::AppResult<()> {
    if before.path != committed.path {
        return Err(format!(
            "snapshot path mismatch during rollback: {} != {}",
            before.path.display(),
            committed.path.display()
        )
        .into());
    }

    let current = snapshot_file_with_max_len(&before.path, max_len)?;
    if current == *before {
        return Ok(());
    }
    if current != *committed {
        return Err(format!(
            "external drift detected while rolling back {}",
            before.path.display()
        )
        .into());
    }
    restore_file_snapshot_exact(before, max_len)
}

fn restore_file_snapshots_conditionally(
    before: &[FileSnapshot],
    committed: &[FileSnapshot],
    max_len: usize,
) -> crate::shared::error::AppResult<()> {
    let mut errors = Vec::new();
    for before_snapshot in before {
        let Some(committed_snapshot) = committed
            .iter()
            .find(|candidate| candidate.path == before_snapshot.path)
        else {
            errors.push(format!(
                "missing committed snapshot for {}",
                before_snapshot.path.display()
            ));
            continue;
        };
        if let Err(error) =
            restore_file_snapshot_conditionally(before_snapshot, committed_snapshot, max_len)
        {
            errors.push(error.to_string());
        }
    }
    for committed_snapshot in committed {
        if !before
            .iter()
            .any(|candidate| candidate.path == committed_snapshot.path)
        {
            errors.push(format!(
                "unexpected committed snapshot for {}",
                committed_snapshot.path.display()
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("conditional rollback failed: {}", errors.join("; ")).into())
    }
}

fn push_bounded_snapshot(
    snapshots: &mut Vec<BoundedFileSnapshot>,
    path: PathBuf,
    max_len: usize,
) -> crate::shared::error::AppResult<()> {
    if snapshots
        .iter()
        .any(|candidate| candidate.snapshot.path == path)
    {
        return Ok(());
    }
    snapshots.push(BoundedFileSnapshot {
        snapshot: snapshot_file_with_max_len(&path, max_len)?,
        max_len,
    });
    Ok(())
}

fn capture_codex_home_rebind_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    snapshots: &mut Vec<BoundedFileSnapshot>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let root = cli_proxy_root_dir(app, "codex")?;
    let files_dir = cli_proxy_files_dir(&root);

    capture_codex_home_rebind_targets(app, snapshots, &target_files(app, "codex")?)?;
    for entry in &manifest.files {
        push_bounded_snapshot(
            snapshots,
            PathBuf::from(&entry.path),
            CLI_PROXY_FILE_MAX_BYTES,
        )?;
        if let Some(rel) = entry.backup_rel.as_deref() {
            push_bounded_snapshot(
                snapshots,
                safe_backup_path(&files_dir, rel)?,
                CLI_PROXY_FILE_MAX_BYTES,
            )?;
        }
    }
    push_bounded_snapshot(
        snapshots,
        cli_proxy_manifest_path(&root),
        CLI_PROXY_MANIFEST_MAX_BYTES,
    )?;
    push_bounded_snapshot(
        snapshots,
        codex_managed_catalog_path(app)?,
        CODEX_MANAGED_CATALOG_MAX_BYTES,
    )?;
    Ok(())
}

fn capture_codex_home_rebind_targets<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    snapshots: &mut Vec<BoundedFileSnapshot>,
    targets: &[TargetFile],
) -> crate::shared::error::AppResult<()> {
    let files_dir = cli_proxy_files_dir(&cli_proxy_root_dir(app, "codex")?);
    for target in targets {
        push_bounded_snapshot(snapshots, target.path.clone(), CLI_PROXY_FILE_MAX_BYTES)?;
        push_bounded_snapshot(
            snapshots,
            files_dir.join(target.backup_name),
            CLI_PROXY_FILE_MAX_BYTES,
        )?;
    }
    Ok(())
}

pub(crate) fn prepare_codex_home_rebind_for_config_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_settings: &crate::settings::AppSettings,
    next_settings: &crate::settings::AppSettings,
) -> crate::shared::error::AppResult<Option<PreparedCodexHomeRebind>> {
    if previous_settings.codex_home_mode == next_settings.codex_home_mode
        && previous_settings.codex_home_override == next_settings.codex_home_override
        && previous_settings.codex_oauth_compatible_proxy_mode
            == next_settings.codex_oauth_compatible_proxy_mode
    {
        return Ok(None);
    }
    let current_targets = target_files(app, "codex")?;
    let previous_targets = codex_target_files_for_settings(app, previous_settings)?;
    let current_signature = current_targets
        .iter()
        .map(|target| (target.kind.to_string(), target.path.clone()))
        .collect::<Vec<_>>();
    let previous_signature = previous_targets
        .iter()
        .map(|target| (target.kind.to_string(), target.path.clone()))
        .collect::<Vec<_>>();
    if current_signature != previous_signature {
        return Err(crate::shared::error::AppError::new(
            "CLI_PROXY_REBIND_REQUIRED",
            "canonical Codex targets changed before config import could prepare the home rebind",
        ));
    }

    let Some(manifest) = read_manifest(app, "codex")? else {
        return Ok(None);
    };
    if !manifest.enabled {
        return Ok(None);
    }

    let mut before = Vec::new();
    capture_codex_home_rebind_files(app, &mut before, &manifest)?;
    let next_targets = codex_target_files_for_settings(app, next_settings)?;
    capture_codex_home_rebind_targets(app, &mut before, &next_targets)?;
    let expected_targets = next_targets
        .into_iter()
        .map(|target| (target.kind.to_string(), target.path))
        .collect();
    Ok(Some(PreparedCodexHomeRebind {
        before,
        expected_targets,
    }))
}

pub(crate) fn apply_codex_home_rebind_for_config_import_locked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    prepared: PreparedCodexHomeRebind,
    base_origin: &str,
) -> crate::shared::error::AppResult<AppliedCodexHomeRebind> {
    let manifest = read_manifest(app, "codex")?.ok_or_else(|| {
        crate::shared::error::AppError::new(
            "CLI_PROXY_REBIND_REQUIRED",
            "enabled Codex proxy manifest disappeared during config import",
        )
    })?;
    if !manifest.enabled {
        return Err(crate::shared::error::AppError::new(
            "CLI_PROXY_REBIND_REQUIRED",
            "Codex proxy was disabled during config import",
        ));
    }

    let current_targets = target_files(app, "codex")?;
    let current_signature = current_targets
        .iter()
        .map(|target| (target.kind.to_string(), target.path.clone()))
        .collect::<Vec<_>>();
    if current_signature != prepared.expected_targets {
        return Err(crate::shared::error::AppError::new(
            "CLI_PROXY_REBIND_REQUIRED",
            "Codex targets changed after config import published the new home",
        ));
    }
    for before in &prepared.before {
        let current = snapshot_file_with_max_len(&before.snapshot.path, before.max_len)?;
        if current != before.snapshot {
            return Err(crate::shared::error::AppError::new(
                "CLI_PROXY_REBIND_REQUIRED",
                format!(
                    "Codex home rebind input drifted during config import: {}",
                    before.snapshot.path.display()
                ),
            ));
        }
    }

    let mut before = prepared.before;
    capture_codex_home_rebind_files(app, &mut before, &manifest)?;
    let result = codex::rebind_codex_home_for_config_import(app, base_origin)?;
    if !result.ok {
        return Err(crate::shared::error::AppError::new(
            result
                .error_code
                .as_deref()
                .unwrap_or("CLI_PROXY_REBIND_FAILED"),
            result.message,
        ));
    }

    let mut changes = Vec::with_capacity(before.len());
    let mut capture_errors = Vec::new();
    for before in before {
        match snapshot_file_with_max_len(&before.snapshot.path, before.max_len) {
            Ok(committed) => changes.push((before, committed)),
            Err(error) => capture_errors.push(error.to_string()),
        }
    }
    if !capture_errors.is_empty() {
        let rollback_error = AppliedCodexHomeRebind { changes }.rollback().err();
        if let Some(error) = rollback_error {
            capture_errors.push(error.to_string());
        }
        return Err(crate::shared::error::AppError::new(
            "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
            format!(
                "could not capture the committed Codex home rebind: {}",
                capture_errors.join("; ")
            ),
        ));
    }
    Ok(AppliedCodexHomeRebind { changes })
}

fn codex_manifest_snapshot<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<FileSnapshot> {
    let path = cli_proxy_manifest_path(&cli_proxy_root_dir(app, "codex")?);
    snapshot_file_with_max_len(&path, CLI_PROXY_MANIFEST_MAX_BYTES)
}

fn expected_codex_manifest_snapshot<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<FileSnapshot> {
    Ok(FileSnapshot {
        path: cli_proxy_manifest_path(&cli_proxy_root_dir(app, "codex")?),
        existed: true,
        bytes: Some(serialize_manifest(manifest)?),
    })
}

fn codex_managed_catalog_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    let root = cli_proxy_root_dir(app, "codex")?;
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create {}: {e}", root.display()))?;
    let root = std::fs::canonicalize(&root)
        .map_err(|e| format!("failed to resolve {}: {e}", root.display()))?;
    Ok(root.join(CODEX_MANAGED_CATALOG_FILE_NAME))
}

fn capture_codex_catalog_lifecycle<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<CodexCatalogLifecycleSnapshot> {
    let targets_before = snapshot_target_files(&capture_current_target_state(app, "codex")?)?;
    let manifest_before = codex_manifest_snapshot(app)?;
    let generated_before = snapshot_file_with_max_len(
        &codex_managed_catalog_path(app)?,
        CODEX_MANAGED_CATALOG_MAX_BYTES,
    )?;
    Ok(CodexCatalogLifecycleSnapshot {
        targets_before,
        manifest_before,
        generated_before,
    })
}

fn current_codex_target_snapshots<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<FileSnapshot>> {
    snapshot_target_files(&capture_current_target_state(app, "codex")?)
}

fn rollback_codex_catalog_lifecycle(
    before: &CodexCatalogLifecycleSnapshot,
    targets_committed: Option<&[FileSnapshot]>,
    manifest_committed: Option<&FileSnapshot>,
) -> crate::shared::error::AppResult<()> {
    let mut errors = Vec::new();

    if let Err(error) = restore_file_snapshot_conditionally(
        &before.generated_before,
        &before.generated_before,
        CODEX_MANAGED_CATALOG_MAX_BYTES,
    ) {
        errors.push(error.to_string());
    }

    if let Err(error) = restore_file_snapshot_conditionally(
        &before.manifest_before,
        manifest_committed.unwrap_or(&before.manifest_before),
        CLI_PROXY_MANIFEST_MAX_BYTES,
    ) {
        errors.push(error.to_string());
    }

    if let Err(error) = restore_file_snapshots_conditionally(
        &before.targets_before,
        targets_committed.unwrap_or(&before.targets_before),
        CLI_PROXY_FILE_MAX_BYTES,
    ) {
        errors.push(error.to_string());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Codex lifecycle rollback could not restore all owned files: {}",
            errors.join("; ")
        )
        .into())
    }
}

fn restore_backups_exactly_from_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<AppliedProxyConfig> {
    let cli_key = manifest.cli_key.as_str();
    validate_cli_key(cli_key)?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);

    let mut prepared = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        if should_skip_manifest_entry_for_current_settings(app, cli_key, &entry.kind) {
            continue;
        }

        let target_path = PathBuf::from(&entry.path);
        let before = snapshot_file(&target_path)?;
        let committed = if entry.existed {
            let Some(rel) = entry.backup_rel.as_ref() else {
                return Err(format!("missing backup_rel for {}", entry.kind).into());
            };
            let backup_path = safe_backup_path(&files_dir, rel)?;
            let bytes = read_cli_proxy_file(&backup_path)?;
            FileSnapshot {
                path: target_path,
                existed: true,
                bytes: Some(bytes),
            }
        } else {
            FileSnapshot {
                path: target_path,
                existed: false,
                bytes: None,
            }
        };
        prepared.push((before, committed));
    }

    apply_file_snapshot_changes(prepared, CLI_PROXY_FILE_MAX_BYTES)
}

fn snapshot_backup_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    captured: &[PendingBackupEntry],
) -> crate::shared::error::AppResult<Vec<FileSnapshot>> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    captured
        .iter()
        .map(|entry| snapshot_file(&files_dir.join(entry.backup_name)))
        .collect()
}

fn snapshot_target_files(
    captured: &[PendingBackupEntry],
) -> crate::shared::error::AppResult<Vec<FileSnapshot>> {
    captured
        .iter()
        .map(|entry| {
            Ok(FileSnapshot {
                path: entry.path.clone(),
                existed: entry.existed,
                bytes: entry.backup_bytes.clone(),
            })
        })
        .collect()
}

fn build_manifest_from_captured(
    existing: &CliProxyManifest,
    base_origin: &str,
    captured: Vec<PendingBackupEntry>,
) -> CliProxyManifest {
    let now = now_unix_seconds();
    let files = captured
        .into_iter()
        .map(|entry| BackupFileEntry {
            kind: entry.kind,
            path: entry.path.to_string_lossy().to_string(),
            existed: entry.existed,
            backup_rel: entry.existed.then(|| entry.backup_name.to_string()),
        })
        .collect();

    CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: existing.cli_key.clone(),
        enabled: existing.enabled,
        base_origin: Some(base_origin.to_string()),
        created_at: existing.created_at,
        updated_at: now,
        files,
    }
}

fn build_manifest_with_current_target_paths<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    existing: &CliProxyManifest,
    base_origin: &str,
) -> crate::shared::error::AppResult<CliProxyManifest> {
    let now = now_unix_seconds();
    let files = target_files(app, existing.cli_key.as_str())?
        .into_iter()
        .map(|target| {
            let existing_entry = existing
                .files
                .iter()
                .find(|entry| entry.kind == target.kind)
                .ok_or_else(|| format!("missing manifest entry for {}", target.kind))?;

            Ok(BackupFileEntry {
                kind: existing_entry.kind.clone(),
                path: target.path.to_string_lossy().to_string(),
                existed: existing_entry.existed,
                backup_rel: existing_entry.backup_rel.clone(),
            })
        })
        .collect::<crate::shared::error::AppResult<Vec<_>>>()?;

    Ok(CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: existing.cli_key.clone(),
        enabled: existing.enabled,
        base_origin: Some(base_origin.to_string()),
        created_at: existing.created_at,
        updated_at: now,
        files,
    })
}

fn rollback_proxy_projection_and_manifest(
    applied_proxy: &AppliedProxyConfig,
    manifest_before: &FileSnapshot,
    manifest_committed: &FileSnapshot,
) -> crate::shared::error::AppResult<()> {
    let mut errors = Vec::new();
    if let Err(error) = applied_proxy.rollback() {
        errors.push(error.to_string());
    }
    if let Err(error) = restore_file_snapshot_conditionally(
        manifest_before,
        manifest_committed,
        CLI_PROXY_MANIFEST_MAX_BYTES,
    ) {
        errors.push(error.to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "proxy projection rollback could not restore all owned files: {}",
            errors.join("; ")
        )
        .into())
    }
}

fn managed_catalog_error_requires_recovery(error: &crate::shared::error::AppError) -> bool {
    error.code() == "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED"
}

// -- Public API -------------------------------------------------------------

pub fn status_all<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    current_base_origin: Option<&str>,
) -> crate::shared::error::AppResult<Vec<CliProxyStatus>> {
    let mut out = Vec::new();
    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::CliProxy)
    {
        let manifest = read_manifest(app, cli_key)?;
        let enabled = manifest.as_ref().map(|m| m.enabled).unwrap_or(false);
        let manifest_base_origin = manifest.as_ref().and_then(|m| m.base_origin.clone());
        let applied_to_current_gateway = if enabled {
            current_base_origin
                .map(|base_origin| is_proxy_config_applied(app, cli_key, base_origin))
        } else {
            None
        };
        out.push(CliProxyStatus {
            cli_key: cli_key.to_string(),
            enabled,
            base_origin: manifest_base_origin,
            current_gateway_origin: current_base_origin.map(str::to_string),
            applied_to_current_gateway,
        });
    }
    Ok(out)
}

pub fn is_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<bool> {
    validate_cli_key(cli_key)?;
    let Some(manifest) = read_manifest(app, cli_key)? else {
        return Ok(false);
    };
    Ok(manifest.enabled)
}

pub fn set_grok_preferences<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    preferences: crate::grok_config::GrokProxyPreferences,
) -> crate::shared::error::AppResult<crate::grok_config::GrokConfigState> {
    grok::set_preferences(app, preferences)
}

pub fn set_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    enabled: bool,
    base_origin: &str,
) -> crate::shared::error::AppResult<CliProxyResult> {
    validate_cli_key(cli_key)?;
    if !base_origin.starts_with("http://") && !base_origin.starts_with("https://") {
        return Err("SEC_INVALID_INPUT: base_origin must start with http:// or https://".into());
    }
    let _grok_transaction = if cli_key == "grok" {
        Some(grok::transaction_lock()?)
    } else {
        None
    };
    let _codex_profile_lifecycle = if cli_key == "codex" {
        Some(crate::codex_managed_profiles::lock_profile_lifecycle())
    } else {
        None
    };

    let trace_id = new_trace_id("cli-proxy");
    let existing = read_manifest(app, cli_key)?;

    if enabled {
        let should_backup = existing.as_ref().map(|m| !m.enabled).unwrap_or(true);
        let origin = Some(base_origin.to_string());
        if cli_key == "codex" && should_backup {
            if let Err(err) = codex::preflight_proxy_config(app, base_origin) {
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_ENABLE_FAILED",
                    err.to_string(),
                    origin,
                ));
            }
        }
        let mut manifest = match if should_backup {
            backup_for_enable(app, cli_key, base_origin, existing.clone())
        } else {
            Ok(existing.unwrap())
        } {
            Ok(m) => m,
            Err(err) => {
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_BACKUP_FAILED",
                    err.to_string(),
                    origin,
                ));
            }
        };

        // Persist snapshot before applying changes to ensure we can restore on failure.
        if should_backup {
            manifest.enabled = false;
            manifest.base_origin = Some(base_origin.to_string());
            manifest.updated_at = now_unix_seconds();
            if let Err(err) = write_manifest(app, cli_key, &manifest) {
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_MANIFEST_WRITE_FAILED",
                    err.to_string(),
                    origin,
                ));
            }
        } else if let Err(err) = ensure_manifest_has_current_targets(app, cli_key, &mut manifest) {
            return Ok(CliProxyResult::failure(
                trace_id,
                cli_key,
                true,
                "CLI_PROXY_BACKUP_FAILED",
                err.to_string(),
                origin,
            ));
        }

        let codex_manifest_before_apply = if cli_key == "codex" {
            match codex_manifest_snapshot(app) {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    return Ok(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        !should_backup,
                        "CLI_PROXY_BACKUP_FAILED",
                        err.to_string(),
                        origin,
                    ));
                }
            }
        } else {
            None
        };

        return match apply_proxy_config(app, cli_key, base_origin) {
            Ok(applied_proxy) => {
                manifest.enabled = true;
                manifest.base_origin = Some(base_origin.to_string());
                manifest.updated_at = now_unix_seconds();
                let codex_manifest_committed = if cli_key == "codex" {
                    match expected_codex_manifest_snapshot(app, &manifest) {
                        Ok(snapshot) => Some(snapshot),
                        Err(err) => {
                            let rollback = applied_proxy.rollback();
                            let (code, message) = match rollback {
                                Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", err.to_string()),
                                Err(rollback_error) => (
                                    "CLI_PROXY_ENABLE_RECOVERY_REQUIRED",
                                    format!("{err}; proxy rollback failed: {rollback_error}"),
                                ),
                            };
                            return Ok(CliProxyResult::failure(
                                trace_id,
                                cli_key,
                                !should_backup,
                                code,
                                message,
                                origin,
                            ));
                        }
                    }
                } else {
                    None
                };
                if let Err(err) = write_manifest(app, cli_key, &manifest) {
                    let rollback = match (
                        codex_manifest_before_apply.as_ref(),
                        codex_manifest_committed.as_ref(),
                    ) {
                        (Some(before), Some(committed)) => rollback_proxy_projection_and_manifest(
                            &applied_proxy,
                            before,
                            committed,
                        ),
                        _ => Ok(()),
                    };
                    let (code, message) = match rollback {
                        Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", err.to_string()),
                        Err(rollback_error) => (
                            "CLI_PROXY_ENABLE_RECOVERY_REQUIRED",
                            format!("{err}; rollback failed: {rollback_error}"),
                        ),
                    };
                    return Ok(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        !should_backup,
                        code,
                        message,
                        origin,
                    ));
                }
                if cli_key == "codex" {
                    if let Err(err) = crate::codex_model_catalog::managed::sync_current_locked(app)
                    {
                        let rollback = rollback_proxy_projection_and_manifest(
                            &applied_proxy,
                            codex_manifest_before_apply
                                .as_ref()
                                .expect("Codex enable captured manifest before projection"),
                            codex_manifest_committed
                                .as_ref()
                                .expect("Codex enable prepared committed manifest"),
                        );
                        let recovery_required =
                            managed_catalog_error_requires_recovery(&err) || rollback.is_err();
                        let code = if recovery_required {
                            "CLI_PROXY_ENABLE_RECOVERY_REQUIRED"
                        } else {
                            "CLI_PROXY_MANAGED_MODEL_SYNC_FAILED"
                        };
                        let message = match rollback {
                            Ok(()) => err.to_string(),
                            Err(rollback_error) => {
                                format!("{err}; rollback failed: {rollback_error}")
                            }
                        };
                        return Ok(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            !should_backup,
                            code,
                            message,
                            origin,
                        ));
                    }
                }

                Ok(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "已开启代理：已备份直连配置并写入网关地址".to_string(),
                    origin,
                ))
            }
            Err(err) => {
                let error_message = err.to_string();
                let is_parse_error = error_message.contains("CLI_PROXY_INVALID_")
                    || error_message.contains("GROK_CONFIG_INVALID_");

                // Only rollback if we actually wrote proxy config (not on parse
                // failure where the file was never modified). On parse failure
                // the invalid file is already preserved as .invalid-backup by
                // apply_proxy_config, so restoring would clobber user changes.
                if should_backup && !is_parse_error && cli_key != "codex" {
                    let _ = restore_from_manifest(app, &manifest);
                    manifest.enabled = false;
                    manifest.updated_at = now_unix_seconds();
                    let _ = write_manifest(app, cli_key, &manifest);
                }

                let recovery_required =
                    cli_key == "codex" && err.code() == "CLI_PROXY_APPLY_RECOVERY_REQUIRED";
                Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    !should_backup,
                    if recovery_required {
                        "CLI_PROXY_ENABLE_RECOVERY_REQUIRED"
                    } else {
                        "CLI_PROXY_ENABLE_FAILED"
                    },
                    err.to_string(),
                    origin,
                ))
            }
        };
    }

    let Some(mut manifest) = existing else {
        return Ok(CliProxyResult::failure(
            trace_id,
            cli_key,
            false,
            "CLI_PROXY_NO_BACKUP",
            "未找到备份，无法自动恢复；请手动处理".to_string(),
            Some(base_origin.to_string()),
        ));
    };

    let previous_manifest = manifest.clone();
    let codex_lifecycle = if cli_key == "codex" {
        match capture_codex_catalog_lifecycle(app) {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    manifest.enabled,
                    "CLI_PROXY_BACKUP_FAILED",
                    err.to_string(),
                    manifest.base_origin.clone(),
                ));
            }
        }
    } else {
        None
    };

    match restore_from_manifest_with_applied(app, &manifest) {
        Ok(restored_proxy) => {
            let codex_targets_committed = codex_lifecycle
                .as_ref()
                .map(|lifecycle| restored_proxy.committed_snapshots_for(&lifecycle.targets_before));

            manifest.enabled = false;
            manifest.updated_at = now_unix_seconds();
            let codex_manifest_committed = if cli_key == "codex" {
                match expected_codex_manifest_snapshot(app, &manifest) {
                    Ok(snapshot) => Some(snapshot),
                    Err(error) => {
                        let rollback = rollback_codex_catalog_lifecycle(
                            codex_lifecycle
                                .as_ref()
                                .expect("Codex disable captured lifecycle state"),
                            codex_targets_committed.as_deref(),
                            None,
                        );
                        let (code, message) = match rollback {
                            Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", error.to_string()),
                            Err(rollback_error) => (
                                "CLI_PROXY_DISABLE_RECOVERY_REQUIRED",
                                format!("{error}; rollback failed: {rollback_error}"),
                            ),
                        };
                        return Ok(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            previous_manifest.enabled,
                            code,
                            message,
                            previous_manifest.base_origin.clone(),
                        ));
                    }
                }
            } else {
                None
            };
            if let Err(err) = write_manifest(app, cli_key, &manifest) {
                let rollback = match codex_lifecycle.as_ref() {
                    Some(snapshot) => rollback_codex_catalog_lifecycle(
                        snapshot,
                        codex_targets_committed.as_deref(),
                        codex_manifest_committed.as_ref(),
                    ),
                    None => Ok(()),
                };
                let (code, message) = match rollback {
                    Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", err.to_string()),
                    Err(rollback_error) => (
                        "CLI_PROXY_DISABLE_RECOVERY_REQUIRED",
                        format!("{err}; rollback failed: {rollback_error}"),
                    ),
                };
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    previous_manifest.enabled,
                    code,
                    message,
                    previous_manifest.base_origin.clone(),
                ));
            }

            if cli_key == "codex" {
                if let Err(err) = crate::codex_model_catalog::managed::sync_current_locked(app) {
                    let rollback = rollback_codex_catalog_lifecycle(
                        codex_lifecycle
                            .as_ref()
                            .expect("Codex disable captured lifecycle state"),
                        codex_targets_committed.as_deref(),
                        codex_manifest_committed.as_ref(),
                    );
                    let recovery_required =
                        managed_catalog_error_requires_recovery(&err) || rollback.is_err();
                    let (code, message, enabled) = match rollback {
                        Ok(()) if !recovery_required => (
                            "CLI_PROXY_MANAGED_MODEL_SYNC_FAILED",
                            err.to_string(),
                            previous_manifest.enabled,
                        ),
                        Ok(()) => (
                            "CLI_PROXY_DISABLE_RECOVERY_REQUIRED",
                            err.to_string(),
                            false,
                        ),
                        Err(rollback_error) => (
                            "CLI_PROXY_DISABLE_RECOVERY_REQUIRED",
                            format!("{err}; rollback failed: {rollback_error}"),
                            false,
                        ),
                    };
                    return Ok(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        enabled,
                        code,
                        message,
                        previous_manifest.base_origin.clone(),
                    ));
                }
            }

            Ok(CliProxyResult::success(
                trace_id,
                cli_key,
                false,
                "已关闭代理：已恢复备份直连配置".to_string(),
                manifest.base_origin.clone(),
            ))
        }
        Err(err) => {
            let rollback = match codex_lifecycle.as_ref() {
                Some(snapshot) => rollback_codex_catalog_lifecycle(snapshot, None, None),
                None => Ok(()),
            };
            let recovery_required =
                err.code() == "CLI_PROXY_RESTORE_RECOVERY_REQUIRED" || rollback.is_err();
            let (code, message) = match rollback {
                Ok(()) if !recovery_required => ("CLI_PROXY_DISABLE_FAILED", err.to_string()),
                Ok(()) => ("CLI_PROXY_DISABLE_RECOVERY_REQUIRED", err.to_string()),
                Err(rollback_error) => (
                    "CLI_PROXY_DISABLE_RECOVERY_REQUIRED",
                    format!("{err}; rollback failed: {rollback_error}"),
                ),
            };
            Ok(CliProxyResult::failure(
                trace_id,
                cli_key,
                previous_manifest.enabled,
                code,
                message,
                previous_manifest.base_origin.clone(),
            ))
        }
    }
}

pub fn startup_repair_incomplete_enable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<CliProxyResult>> {
    let mut out = Vec::new();

    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::CliProxy)
    {
        let _codex_profile_lifecycle = if cli_key == "codex" {
            Some(crate::codex_managed_profiles::lock_profile_lifecycle())
        } else {
            None
        };
        let Some(mut manifest) = read_manifest(app, cli_key)? else {
            continue;
        };
        if manifest.enabled {
            continue;
        }

        let Some(base_origin) = manifest.base_origin.clone() else {
            continue;
        };

        if !is_proxy_config_applied(app, cli_key, &base_origin) {
            continue;
        }

        let trace_id = new_trace_id("cli-proxy-startup-repair");

        let codex_manifest_before = if cli_key == "codex" {
            match codex_manifest_snapshot(app) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    out.push(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        false,
                        "CLI_PROXY_STARTUP_REPAIR_FAILED",
                        error.to_string(),
                        Some(base_origin),
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        manifest.enabled = true;
        manifest.updated_at = now_unix_seconds();
        let codex_manifest_committed = if cli_key == "codex" {
            match expected_codex_manifest_snapshot(app, &manifest) {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    out.push(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        false,
                        "CLI_PROXY_STARTUP_REPAIR_FAILED",
                        error.to_string(),
                        Some(base_origin),
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        match write_manifest(app, cli_key, &manifest) {
            Ok(()) => {
                if cli_key == "codex" {
                    if let Err(error) =
                        crate::codex_model_catalog::managed::sync_current_locked(app)
                    {
                        let rollback = restore_file_snapshot_conditionally(
                            codex_manifest_before
                                .as_ref()
                                .expect("Codex startup repair captured manifest"),
                            codex_manifest_committed
                                .as_ref()
                                .expect("Codex startup repair prepared manifest"),
                            CLI_PROXY_MANIFEST_MAX_BYTES,
                        );
                        let recovery_required =
                            managed_catalog_error_requires_recovery(&error) || rollback.is_err();
                        let (code, message) = match rollback {
                            Ok(()) if !recovery_required => {
                                ("CLI_PROXY_MANAGED_MODEL_SYNC_FAILED", error.to_string())
                            }
                            Ok(()) => (
                                "CLI_PROXY_STARTUP_REPAIR_RECOVERY_REQUIRED",
                                error.to_string(),
                            ),
                            Err(rollback_error) => (
                                "CLI_PROXY_STARTUP_REPAIR_RECOVERY_REQUIRED",
                                format!("{error}; manifest rollback failed: {rollback_error}"),
                            ),
                        };
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            recovery_required,
                            code,
                            message,
                            Some(base_origin),
                        ));
                        continue;
                    }
                }
                out.push(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "启动自愈：已修复异常中断导致的启用状态不一致".to_string(),
                    Some(base_origin),
                ));
            }
            Err(err) => {
                let rollback = match (
                    codex_manifest_before.as_ref(),
                    codex_manifest_committed.as_ref(),
                ) {
                    (Some(before), Some(committed)) => restore_file_snapshot_conditionally(
                        before,
                        committed,
                        CLI_PROXY_MANIFEST_MAX_BYTES,
                    ),
                    _ => Ok(()),
                };
                let (code, message) = match rollback {
                    Ok(()) => ("CLI_PROXY_STARTUP_REPAIR_FAILED", err.to_string()),
                    Err(rollback_error) => (
                        "CLI_PROXY_STARTUP_REPAIR_RECOVERY_REQUIRED",
                        format!("{err}; manifest rollback failed: {rollback_error}"),
                    ),
                };
                out.push(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    code,
                    message,
                    Some(base_origin),
                ));
            }
        }
    }

    Ok(out)
}

pub fn sync_codex_oauth_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> crate::shared::error::AppResult<CliProxyResult> {
    if !base_origin.starts_with("http://") && !base_origin.starts_with("https://") {
        return Err("SEC_INVALID_INPUT: base_origin must start with http:// or https://".into());
    }
    let trace_id = new_trace_id("cli-proxy-codex-oauth-sync");
    let origin = Some(base_origin.to_string());
    let _lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
    #[cfg(test)]
    run_codex_oauth_after_lock_test_hook();
    let Some(mut manifest) = read_manifest(app, "codex")? else {
        return Ok(CliProxyResult::success(
            trace_id,
            "codex",
            false,
            "Codex 代理未启用，无需同步 OAuth 模式".to_string(),
            origin,
        ));
    };
    if !manifest.enabled {
        return Ok(CliProxyResult::success(
            trace_id,
            "codex",
            false,
            "Codex 代理未启用，无需同步 OAuth 模式".to_string(),
            origin,
        ));
    }
    if manifest_target_paths_changed(app, &manifest)? {
        return Ok(CliProxyResult::failure(
            trace_id,
            "codex",
            true,
            "CLI_PROXY_REBIND_REQUIRED",
            "Codex 目录已变化，请先完成目录重绑".to_string(),
            origin,
        ));
    }

    let captured = capture_codex_oauth_target_state(app)?;
    let targets_before = snapshot_target_files(&captured)?;
    let manifest_before = codex_manifest_snapshot(app)?;
    let mut targets_after_oauth_restore = targets_before.clone();
    let mut manifest_committed = manifest_before.clone();
    let mut ensured_backups = None;
    let mut applied_proxy = None;

    let apply_result = (|| -> crate::shared::error::AppResult<()> {
        ensured_backups = Some(ensure_manifest_has_current_targets_with_applied(
            app,
            "codex",
            &mut manifest,
        )?);
        if codex_oauth_compatible_proxy_mode(app) {
            restore_codex_auth_for_oauth_mode(app, &manifest)?;
            targets_after_oauth_restore =
                snapshot_target_files(&capture_codex_oauth_target_state(app)?)?;
        }
        applied_proxy = Some(apply_proxy_config(app, "codex", base_origin)?);
        #[cfg(test)]
        if let Some(error) = run_codex_oauth_sync_test_hook() {
            return Err(format!("CODEX_OAUTH_PROXY_SYNC_FAILED: {error}").into());
        }
        if !codex::is_proxy_config_applied(app, base_origin) {
            return Err("CODEX_OAUTH_PROXY_SYNC_FAILED: projected config validation failed".into());
        }
        manifest.base_origin = Some(base_origin.to_string());
        manifest.updated_at = now_unix_seconds();
        manifest_committed = expected_codex_manifest_snapshot(app, &manifest)?;
        write_manifest(app, "codex", &manifest)?;
        Ok(())
    })();

    if let Err(error) = apply_result {
        let mut recovery_errors = Vec::new();
        if let Some(applied_proxy) = applied_proxy.as_ref() {
            if let Err(rollback_error) = applied_proxy.rollback() {
                recovery_errors.push(rollback_error.to_string());
            }
        }
        if let Err(rollback_error) = restore_file_snapshots_conditionally(
            &targets_before,
            &targets_after_oauth_restore,
            CLI_PROXY_FILE_MAX_BYTES,
        ) {
            recovery_errors.push(rollback_error.to_string());
        }
        if let Some(ensured_backups) = ensured_backups.as_ref() {
            if let Err(rollback_error) = ensured_backups.rollback() {
                recovery_errors.push(rollback_error.to_string());
            }
        }
        if let Err(rollback_error) = restore_file_snapshot_conditionally(
            &manifest_before,
            &manifest_committed,
            CLI_PROXY_MANIFEST_MAX_BYTES,
        ) {
            recovery_errors.push(rollback_error.to_string());
        }
        let original_requires_recovery = matches!(
            error.code(),
            "CLI_PROXY_APPLY_RECOVERY_REQUIRED" | "CLI_PROXY_RESTORE_RECOVERY_REQUIRED"
        );
        if original_requires_recovery || !recovery_errors.is_empty() {
            let recovery_detail = if recovery_errors.is_empty() {
                error.to_string()
            } else {
                format!("{error}; rollback failed: {}", recovery_errors.join("; "))
            };
            return Ok(CliProxyResult::failure(
                trace_id,
                "codex",
                true,
                "CODEX_OAUTH_PROXY_RECOVERY_REQUIRED",
                recovery_detail,
                origin,
            ));
        }
        return Ok(CliProxyResult::failure(
            trace_id,
            "codex",
            true,
            "CODEX_OAUTH_PROXY_SYNC_FAILED",
            error.to_string(),
            origin,
        ));
    }

    let message = if apply_live {
        "已同步 Codex OAuth 兼容代理模式"
    } else {
        "已更新 Codex OAuth 兼容代理配置，网关启动后继续使用"
    };
    Ok(CliProxyResult::success(
        trace_id,
        "codex",
        true,
        message.to_string(),
        origin,
    ))
}

fn capture_codex_oauth_target_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<PendingBackupEntry>> {
    let targets = [
        (
            "codex_config_toml",
            codex::codex_config_path(app)?,
            "config.toml",
        ),
        ("codex_auth_json", codex::codex_auth_path(app)?, "auth.json"),
    ];
    targets
        .into_iter()
        .map(|(kind, path, backup_name)| {
            let bytes = read_optional_cli_proxy_file(&path)?;
            Ok(PendingBackupEntry {
                kind: kind.to_string(),
                path,
                backup_name,
                existed: bytes.is_some(),
                backup_bytes: bytes,
            })
        })
        .collect()
}

fn restore_codex_auth_for_oauth_mode<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let Some(entry) = manifest
        .files
        .iter()
        .find(|entry| entry.kind == "codex_auth_json")
    else {
        return Ok(());
    };
    let target = PathBuf::from(&entry.path);
    if !entry.existed {
        if target.exists() {
            std::fs::remove_file(&target)
                .map_err(|error| format!("failed to remove {}: {error}", target.display()))?;
        }
        return Ok(());
    }
    let rel = entry
        .backup_rel
        .as_deref()
        .ok_or_else(|| "CLI_PROXY_INVALID_MANIFEST: missing Codex auth backup path".to_string())?;
    let root = cli_proxy_root_dir(app, "codex")?;
    let backup = safe_backup_path(&cli_proxy_files_dir(&root), rel)?;
    codex::merge_restore_codex_auth_json(&target, &backup)
}

pub fn sync_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> crate::shared::error::AppResult<Vec<CliProxyResult>> {
    if !base_origin.starts_with("http://") && !base_origin.starts_with("https://") {
        return Err("SEC_INVALID_INPUT: base_origin must start with http:// or https://".into());
    }

    let mut out = Vec::new();
    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::CliProxy)
    {
        let Some(mut manifest) = read_manifest(app, cli_key)? else {
            continue;
        };
        if !manifest.enabled {
            continue;
        }

        let _grok_transaction = if cli_key == "grok" {
            Some(grok::transaction_lock()?)
        } else {
            None
        };
        let _codex_profile_lifecycle = if cli_key == "codex" {
            Some(crate::codex_managed_profiles::lock_profile_lifecycle())
        } else {
            None
        };

        let trace_id = new_trace_id("cli-proxy-sync");
        let needs_target_rebind =
            matches!(cli_key, "codex" | "grok") && manifest_target_paths_changed(app, &manifest)?;

        if needs_target_rebind {
            out.push(match cli_key {
                "codex" => codex::rebind_codex_manifest_after_home_change(
                    app,
                    manifest,
                    base_origin,
                    apply_live,
                    true,
                    trace_id,
                )?,
                "grok" => grok::rebind_grok_manifest_after_home_change(
                    app,
                    manifest,
                    base_origin,
                    apply_live,
                    trace_id,
                )?,
                _ => unreachable!("rebind capability checked above"),
            });
            continue;
        }

        if !apply_live {
            let previous_manifest = manifest.clone();
            let codex_lifecycle = if cli_key == "codex" {
                match capture_codex_catalog_lifecycle(app) {
                    Ok(snapshot) => Some(snapshot),
                    Err(error) => {
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_BACKUP_FAILED",
                            error.to_string(),
                            previous_manifest.base_origin.clone(),
                        ));
                        continue;
                    }
                }
            } else {
                None
            };
            let mut codex_manifest_committed = None;
            if manifest.base_origin.as_deref() != Some(base_origin) {
                manifest.base_origin = Some(base_origin.to_string());
                manifest.updated_at = now_unix_seconds();
                if cli_key == "codex" {
                    match expected_codex_manifest_snapshot(app, &manifest) {
                        Ok(snapshot) => codex_manifest_committed = Some(snapshot),
                        Err(error) => {
                            out.push(CliProxyResult::failure(
                                trace_id,
                                cli_key,
                                true,
                                "CLI_PROXY_MANIFEST_WRITE_FAILED",
                                error.to_string(),
                                previous_manifest.base_origin.clone(),
                            ));
                            continue;
                        }
                    }
                }
                if let Err(error) = write_manifest(app, cli_key, &manifest) {
                    let rollback = match codex_lifecycle.as_ref() {
                        Some(snapshot) => rollback_codex_catalog_lifecycle(
                            snapshot,
                            None,
                            codex_manifest_committed.as_ref(),
                        ),
                        None => Ok(()),
                    };
                    let (code, message) = match rollback {
                        Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", error.to_string()),
                        Err(rollback_error) => (
                            "CLI_PROXY_SYNC_RECOVERY_REQUIRED",
                            format!("{error}; rollback failed: {rollback_error}"),
                        ),
                    };
                    out.push(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        true,
                        code,
                        message,
                        previous_manifest.base_origin.clone(),
                    ));
                    continue;
                }
            }
            if cli_key == "codex" {
                match crate::codex_model_catalog::managed::sync_current_locked(app) {
                    Ok(()) => {}
                    Err(err) => {
                        let rollback = rollback_codex_catalog_lifecycle(
                            codex_lifecycle
                                .as_ref()
                                .expect("Codex offline sync captured lifecycle state"),
                            None,
                            codex_manifest_committed.as_ref(),
                        );
                        let recovery_required =
                            managed_catalog_error_requires_recovery(&err) || rollback.is_err();
                        let (code, message) = match rollback {
                            Ok(()) if !recovery_required => {
                                ("CLI_PROXY_MANAGED_MODEL_SYNC_FAILED", err.to_string())
                            }
                            Ok(()) => ("CLI_PROXY_SYNC_RECOVERY_REQUIRED", err.to_string()),
                            Err(rollback_error) => (
                                "CLI_PROXY_SYNC_RECOVERY_REQUIRED",
                                format!("{err}; rollback failed: {rollback_error}"),
                            ),
                        };
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            code,
                            message,
                            previous_manifest.base_origin.clone(),
                        ));
                        continue;
                    }
                }
            }
            out.push(CliProxyResult::success(
                trace_id,
                cli_key,
                true,
                "已更新代理目标端口，待网关启动后接管".to_string(),
                Some(base_origin.to_string()),
            ));
            continue;
        }

        if manifest.base_origin.as_deref() == Some(base_origin)
            && is_proxy_config_applied(app, cli_key, base_origin)
        {
            if cli_key == "codex" {
                match crate::codex_model_catalog::managed::sync_current_locked(app) {
                    Ok(()) => {}
                    Err(err) => {
                        let code = if managed_catalog_error_requires_recovery(&err) {
                            "CLI_PROXY_SYNC_RECOVERY_REQUIRED"
                        } else {
                            "CLI_PROXY_MANAGED_MODEL_SYNC_FAILED"
                        };
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            code,
                            err.to_string(),
                            Some(base_origin.to_string()),
                        ));
                        continue;
                    }
                }
            }
            out.push(CliProxyResult::success(
                trace_id,
                cli_key,
                true,
                "已是最新，无需同步".to_string(),
                Some(base_origin.to_string()),
            ));
            continue;
        }

        if let Err(err) = ensure_manifest_has_current_targets(app, cli_key, &mut manifest) {
            out.push(CliProxyResult::failure(
                trace_id,
                cli_key,
                true,
                "CLI_PROXY_BACKUP_FAILED",
                err.to_string(),
                Some(base_origin.to_string()),
            ));
            continue;
        }

        let codex_manifest_before_apply = if cli_key == "codex" {
            match codex_manifest_snapshot(app) {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    out.push(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        true,
                        "CLI_PROXY_BACKUP_FAILED",
                        err.to_string(),
                        Some(base_origin.to_string()),
                    ));
                    continue;
                }
            }
        } else {
            None
        };

        match apply_proxy_config(app, cli_key, base_origin) {
            Ok(applied_proxy) => {
                manifest.base_origin = Some(base_origin.to_string());
                manifest.updated_at = now_unix_seconds();
                let codex_manifest_committed = if cli_key == "codex" {
                    match expected_codex_manifest_snapshot(app, &manifest) {
                        Ok(snapshot) => Some(snapshot),
                        Err(error) => {
                            let rollback = applied_proxy.rollback();
                            let (code, message) = match rollback {
                                Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", error.to_string()),
                                Err(rollback_error) => (
                                    "CLI_PROXY_SYNC_RECOVERY_REQUIRED",
                                    format!("{error}; proxy rollback failed: {rollback_error}"),
                                ),
                            };
                            out.push(CliProxyResult::failure(
                                trace_id,
                                cli_key,
                                true,
                                code,
                                message,
                                Some(base_origin.to_string()),
                            ));
                            continue;
                        }
                    }
                } else {
                    None
                };
                if let Err(err) = write_manifest(app, cli_key, &manifest) {
                    let rollback = match (
                        codex_manifest_before_apply.as_ref(),
                        codex_manifest_committed.as_ref(),
                    ) {
                        (Some(before), Some(committed)) => rollback_proxy_projection_and_manifest(
                            &applied_proxy,
                            before,
                            committed,
                        ),
                        _ => Ok(()),
                    };
                    let (code, message) = match rollback {
                        Ok(()) => ("CLI_PROXY_MANIFEST_WRITE_FAILED", err.to_string()),
                        Err(rollback_error) => (
                            "CLI_PROXY_SYNC_RECOVERY_REQUIRED",
                            format!("{err}; rollback failed: {rollback_error}"),
                        ),
                    };
                    out.push(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        true,
                        code,
                        message,
                        Some(base_origin.to_string()),
                    ));
                    continue;
                }
                if cli_key == "codex" {
                    if let Err(err) = crate::codex_model_catalog::managed::sync_current_locked(app)
                    {
                        let rollback = rollback_proxy_projection_and_manifest(
                            &applied_proxy,
                            codex_manifest_before_apply
                                .as_ref()
                                .expect("Codex sync captured manifest before projection"),
                            codex_manifest_committed
                                .as_ref()
                                .expect("Codex sync prepared committed manifest"),
                        );
                        let recovery_required =
                            managed_catalog_error_requires_recovery(&err) || rollback.is_err();
                        let code = if recovery_required {
                            "CLI_PROXY_SYNC_RECOVERY_REQUIRED"
                        } else {
                            "CLI_PROXY_MANAGED_MODEL_SYNC_FAILED"
                        };
                        let message = match rollback {
                            Ok(()) => err.to_string(),
                            Err(rollback_error) => {
                                format!("{err}; rollback failed: {rollback_error}")
                            }
                        };
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            code,
                            message,
                            Some(base_origin.to_string()),
                        ));
                        continue;
                    }
                }
                out.push(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "已同步代理配置到新端口".to_string(),
                    Some(base_origin.to_string()),
                ));
            }
            Err(err) => {
                let recovery_required =
                    cli_key == "codex" && err.code() == "CLI_PROXY_APPLY_RECOVERY_REQUIRED";
                out.push(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    true,
                    if recovery_required {
                        "CLI_PROXY_SYNC_RECOVERY_REQUIRED"
                    } else {
                        "CLI_PROXY_SYNC_FAILED"
                    },
                    err.to_string(),
                    Some(base_origin.to_string()),
                ));
            }
        }
    }
    Ok(out)
}

pub fn rebind_codex_home_after_change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> crate::shared::error::AppResult<CliProxyResult> {
    let _profile_lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
    codex::rebind_codex_home_after_change(app, base_origin, apply_live)
}

pub fn restore_enabled_keep_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<CliProxyResult>> {
    let mut out = Vec::new();
    for cli_key in
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::CliProxy)
    {
        let Some(manifest) = read_manifest(app, cli_key)? else {
            continue;
        };
        if !manifest.enabled {
            continue;
        }

        let _grok_transaction = if cli_key == "grok" {
            Some(grok::transaction_lock()?)
        } else {
            None
        };
        let _codex_profile_lifecycle = if cli_key == "codex" {
            Some(crate::codex_managed_profiles::lock_profile_lifecycle())
        } else {
            None
        };

        let trace_id = new_trace_id("cli-proxy-restore");
        let codex_lifecycle = if cli_key == "codex" {
            match capture_codex_catalog_lifecycle(app) {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    out.push(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        true,
                        "CLI_PROXY_BACKUP_FAILED",
                        err.to_string(),
                        manifest.base_origin.clone(),
                    ));
                    continue;
                }
            }
        } else {
            None
        };

        match restore_from_manifest(app, &manifest) {
            Ok(()) => {
                if cli_key == "codex" {
                    let targets_committed = match current_codex_target_snapshots(app) {
                        Ok(snapshots) => snapshots,
                        Err(error) => {
                            out.push(CliProxyResult::failure(
                                trace_id,
                                cli_key,
                                true,
                                "CLI_PROXY_RESTORE_RECOVERY_REQUIRED",
                                format!(
                                    "direct config was restored but its committed state could not be captured: {error}"
                                ),
                                manifest.base_origin.clone(),
                            ));
                            continue;
                        }
                    };
                    if let Err(err) = crate::codex_model_catalog::managed::sync_current_locked(app)
                    {
                        let rollback = rollback_codex_catalog_lifecycle(
                            codex_lifecycle
                                .as_ref()
                                .expect("Codex restore captured lifecycle state"),
                            Some(&targets_committed),
                            None,
                        );
                        let recovery_required =
                            managed_catalog_error_requires_recovery(&err) || rollback.is_err();
                        let (code, message) = match rollback {
                            Ok(()) if !recovery_required => {
                                ("CLI_PROXY_MANAGED_MODEL_SYNC_FAILED", err.to_string())
                            }
                            Ok(()) => ("CLI_PROXY_RESTORE_RECOVERY_REQUIRED", err.to_string()),
                            Err(rollback_error) => (
                                "CLI_PROXY_RESTORE_RECOVERY_REQUIRED",
                                format!("{err}; rollback failed: {rollback_error}"),
                            ),
                        };
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            code,
                            message,
                            manifest.base_origin.clone(),
                        ));
                        continue;
                    }
                }
                out.push(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "已恢复备份直连配置（保留启用状态）".to_string(),
                    manifest.base_origin.clone(),
                ));
            }
            Err(err) => {
                let rollback = match codex_lifecycle.as_ref() {
                    Some(snapshot) => rollback_codex_catalog_lifecycle(snapshot, None, None),
                    None => Ok(()),
                };
                let recovery_required =
                    err.code() == "CLI_PROXY_RESTORE_RECOVERY_REQUIRED" || rollback.is_err();
                let (code, message) = match rollback {
                    Ok(()) if !recovery_required => ("CLI_PROXY_RESTORE_FAILED", err.to_string()),
                    Ok(()) => ("CLI_PROXY_RESTORE_RECOVERY_REQUIRED", err.to_string()),
                    Err(rollback_error) => (
                        "CLI_PROXY_RESTORE_RECOVERY_REQUIRED",
                        format!("{err}; rollback failed: {rollback_error}"),
                    ),
                };
                out.push(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    true,
                    code,
                    message,
                    manifest.base_origin.clone(),
                ));
            }
        }
    }
    Ok(out)
}

// Re-export submodule items for tests (tests use `super::*`).
#[cfg(test)]
use claude::{build_claude_settings_json, merge_restore_claude_settings_json};
#[cfg(test)]
use codex::{
    build_codex_auth_json, build_codex_config_toml, build_codex_config_toml_oauth_compatible,
    codex_auth_path, codex_config_path, merge_restore_codex_auth_json,
    merge_restore_codex_config_toml, CodexConfigPlatform,
};
#[cfg(test)]
use gemini::merge_restore_gemini_env;

#[cfg(test)]
mod tests;
