//! Usage: Strict Codex provider sync / backup / rollback core.

use crate::infra::codex_retry_gateway::{CodexProviderSyncPlan, CodexRouteMode};
use crate::shared::error::AppResult;
use crate::shared::fs::{
    is_symlink, read_file_with_max_len, read_optional_file_with_max_len, write_file_atomic,
    write_file_atomic_if_changed,
};
use crate::shared::time::{now_unix_millis, now_unix_seconds};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

pub const PROVIDER_SYNC_LOCK_FILE: &str = "tmp/provider-sync.lock";
pub const PROVIDER_SYNC_BACKUP_ROOT: &str = "backups_state/provider-sync";
const PROVIDER_SYNC_TRANSACTION_ROOT: &str = "tmp/provider-sync-transaction";
const PROVIDER_SYNC_TRANSACTION_STAGING_PREFIX: &str = "provider-sync-transaction.staging-";
const PROVIDER_SYNC_TRANSACTION_MANIFEST: &str = "journal.json";
const PROVIDER_SYNC_TRANSACTION_APPLIED_MARKER: &str = "applied.json";
const PROVIDER_SYNC_TRANSACTION_QUARANTINE_ROOT: &str = "backups_state/provider-sync-quarantine";
const PROVIDER_SYNC_TRANSACTION_QUARANTINE_REASON_MAX_CHARS: usize = 4096;
const PROVIDER_SYNC_TRANSACTION_SCHEMA_VERSION: u32 = 2;
const PROVIDER_SYNC_TRANSACTION_MANIFEST_MAX_BYTES: usize = 1024 * 1024;
const PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS: usize = 4096;
const PROVIDER_SYNC_SNAPSHOT_MAX_BYTES: usize = 128 * 1024 * 1024;
const PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES: usize = 256 * 1024 * 1024;
const PROVIDER_SYNC_KEEP_COUNT: usize = 5;
const PROVIDER_SYNC_MAX_BYTES: usize = 1024 * 1024;
const MANAGED_PROVIDER_AIO: &str = "aio";
const MANAGED_PROVIDER_OPENAI: &str = "OpenAI";
const PROVIDER_SYNC_MANAGED_BACKUP_MANIFEST: &str = "provider-sync.json";
const CODEX_APP_RUNNING_OVERRIDE_NONE: u8 = 0;
const CODEX_APP_RUNNING_OVERRIDE_FALSE: u8 = 1;
const CODEX_APP_RUNNING_OVERRIDE_TRUE: u8 = 2;

static CODEX_APP_RUNNING_OVERRIDE: AtomicU8 = AtomicU8::new(CODEX_APP_RUNNING_OVERRIDE_NONE);

fn codex_process_check_failed_message(command: &str, detail: impl AsRef<str>) -> String {
    format!(
        "CODEX_PROVIDER_SYNC_PROCESS_CHECK_FAILED: unable to verify whether Codex App is closed before syncing provider settings. Process check command `{command}` failed: {}. Please confirm Codex App is fully closed, then retry.",
        detail.as_ref()
    )
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct CodexProviderSyncResult {
    pub status: String,
    pub target_provider: String,
    pub trigger: String,
    pub backup_dir: Option<String>,
    pub changed_session_files: Vec<String>,
    pub sqlite_provider_rows_updated: usize,
    pub sqlite_user_event_rows_updated: usize,
    pub sqlite_cwd_rows_updated: usize,
    pub updated_workspace_roots: Vec<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexProviderSyncContext {
    pub trigger: String,
    pub target_provider: String,
    pub config_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexProviderSyncRouteContext {
    pub operation_id: String,
    pub target_generation: u64,
    pub target_mode: CodexRouteMode,
    pub target_live_config_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexProviderSyncCurrentRoute {
    pub generation: u64,
    pub mode: CodexRouteMode,
    pub live_config_sha256: String,
    pub live_matches_projection: bool,
    pub auth_matches_projection: bool,
    pub pending_operation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexProviderSyncRecoveryOutcome {
    None,
    Restored,
    Finalized,
    Quarantined,
    StaleLockRemoved,
}

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    existed: bool,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderSyncTransactionPhase {
    Prepared,
    Applied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistentFileSnapshot {
    target_rel: String,
    existed: bool,
    backup_rel: Option<String>,
    byte_len: Option<u64>,
    sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistentRouteContext {
    operation_id: String,
    target_generation: u64,
    target_mode: CodexRouteMode,
    target_live_config_sha256: String,
}

impl From<CodexProviderSyncRouteContext> for PersistentRouteContext {
    fn from(value: CodexProviderSyncRouteContext) -> Self {
        Self {
            operation_id: value.operation_id,
            target_generation: value.target_generation,
            target_mode: value.target_mode,
            target_live_config_sha256: value.target_live_config_sha256,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProviderSyncTransactionJournal {
    schema_version: u32,
    operation_id: String,
    phase: ProviderSyncTransactionPhase,
    target_provider: String,
    target_config_sha256: String,
    route: Option<PersistentRouteContext>,
    backup_dir_rel: String,
    backup_root_existed: bool,
    snapshots: Vec<PersistentFileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProviderSyncAppliedMarker {
    operation_id: String,
}

#[must_use = "provider sync rollback tokens must be explicitly committed or rolled back"]
pub(crate) struct CodexProviderSyncRollback {
    snapshots: Option<Vec<FileSnapshot>>,
    transaction_home: Option<PathBuf>,
    backup_home: Option<PathBuf>,
    backup_dir: Option<PathBuf>,
    remove_empty_backup_root: bool,
}

impl CodexProviderSyncRollback {
    fn new(
        snapshots: Vec<FileSnapshot>,
        transaction_home: Option<PathBuf>,
        backup_home: Option<PathBuf>,
        backup_dir: Option<PathBuf>,
        remove_empty_backup_root: bool,
    ) -> Self {
        Self {
            snapshots: Some(snapshots),
            transaction_home,
            backup_home,
            backup_dir,
            remove_empty_backup_root,
        }
    }

    pub(crate) fn commit(mut self) {
        self.snapshots = None;
        if let Some(home) = self.transaction_home.take() {
            if let Err(error) = clear_provider_sync_transaction(&home) {
                tracing::error!(
                    error = %error,
                    "provider sync transaction journal cleanup failed after commit"
                );
            }
        }
        self.backup_dir = None;
        self.remove_empty_backup_root = false;
        if let Some(home) = self.backup_home.take() {
            match prune_managed_backups(&home) {
                Ok(Some(warning)) => tracing::warn!(%warning, "provider sync backup prune warning"),
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    error = %error,
                    "provider sync backup prune failed after transaction commit"
                ),
            }
        }
    }

    pub(crate) fn rollback(mut self) -> AppResult<()> {
        self.rollback_inner()
    }

    fn rollback_inner(&mut self) -> AppResult<()> {
        let mut errors = Vec::new();
        if let Some(mut snapshots) = self.snapshots.take() {
            if let Err(error) = restore_snapshots(&mut snapshots) {
                errors.push(error.to_string());
            }
        }
        if let (Some(home), Some(backup_dir)) = (self.backup_home.take(), self.backup_dir.take()) {
            if let Err(error) = remove_created_provider_sync_backup(
                &home,
                &backup_dir,
                self.remove_empty_backup_root,
            ) {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            if let Some(home) = self.transaction_home.take() {
                if let Err(error) = clear_provider_sync_transaction(&home) {
                    errors.push(error.to_string());
                }
            }
        }
        self.remove_empty_backup_root = false;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("CODEX_PROVIDER_SYNC_ROLLBACK_FAILED: {}", errors.join("; ")).into())
        }
    }
}

impl Drop for CodexProviderSyncRollback {
    fn drop(&mut self) {
        if self.snapshots.is_some() || self.transaction_home.is_some() || self.backup_dir.is_some()
        {
            if let Err(error) = self.rollback_inner() {
                tracing::error!(
                    error = %error,
                    "provider sync rollback token failed to restore during drop"
                );
            }
        }
    }
}

#[derive(Debug, Clone)]
struct SyncChangeSet {
    config_bytes: Option<Vec<u8>>,
    session_changes: Vec<SessionChange>,
    sqlite_changes: Vec<SqliteDbChange>,
    global_state_change: Option<GlobalStateChange>,
    updated_workspace_roots: Vec<String>,
    warning: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionChange {
    path: PathBuf,
    original_text: Vec<u8>,
    next_text: Vec<u8>,
}

#[derive(Debug, Clone)]
struct SqliteDbChange {
    path: PathBuf,
    provider_rows_updated: usize,
    user_event_rows_updated: usize,
    cwd_rows_updated: usize,
}

#[derive(Debug, Clone)]
struct GlobalStateChange {
    path: PathBuf,
    original_bytes: Option<Vec<u8>>,
    next_bytes: Option<Vec<u8>>,
    bak_path: PathBuf,
    bak_next_bytes: Option<Option<Vec<u8>>>,
    updated_workspace_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BackupManifest {
    version: u8,
    trigger: String,
    target_provider: String,
    created_at: String,
    managed_by: String,
    config_path: Option<String>,
    session_files: Vec<String>,
    sqlite_files: Vec<String>,
    global_state_path: Option<String>,
}

pub fn codex_provider_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    context: CodexProviderSyncContext,
) -> AppResult<CodexProviderSyncResult> {
    codex_provider_sync_transaction(app, context, |_| Ok(())).map(|(result, _)| result)
}

pub(crate) fn codex_provider_sync_preflight() -> AppResult<()> {
    reject_running_codex_for_provider_sync(codex_app_is_running()?)
}

fn reject_running_codex_for_provider_sync(running: bool) -> AppResult<()> {
    if running {
        Err("CODEX_PROVIDER_SYNC_PROCESS_RUNNING: Codex App is running".into())
    } else {
        Ok(())
    }
}

pub(crate) fn codex_provider_sync_transaction<R: tauri::Runtime, T, F>(
    app: &tauri::AppHandle<R>,
    context: CodexProviderSyncContext,
    after_apply: F,
) -> AppResult<(CodexProviderSyncResult, T)>
where
    F: FnOnce(&CodexProviderSyncResult) -> AppResult<T>,
{
    let (result, extra, rollback) = codex_provider_sync_transaction_with_target_resolver(
        app,
        context,
        after_apply,
        resolve_target_provider,
        false,
        None,
    )?;
    if let Some(rollback) = rollback {
        rollback.commit();
    }
    Ok((result, extra))
}

pub(crate) fn codex_provider_sync_transaction_reversible<R: tauri::Runtime, T, F>(
    app: &tauri::AppHandle<R>,
    context: CodexProviderSyncContext,
    after_apply: F,
) -> AppResult<(
    CodexProviderSyncResult,
    T,
    Option<CodexProviderSyncRollback>,
)>
where
    F: FnOnce(&CodexProviderSyncResult) -> AppResult<T>,
{
    codex_provider_sync_transaction_with_target_resolver(
        app,
        context,
        after_apply,
        resolve_target_provider,
        true,
        None,
    )
}

pub(crate) fn codex_provider_sync_transaction_reversible_for_route<R: tauri::Runtime, T, F>(
    app: &tauri::AppHandle<R>,
    context: CodexProviderSyncContext,
    route_context: CodexProviderSyncRouteContext,
    after_apply: F,
) -> AppResult<(
    CodexProviderSyncResult,
    T,
    Option<CodexProviderSyncRollback>,
)>
where
    F: FnOnce(&CodexProviderSyncResult) -> AppResult<T>,
{
    codex_provider_sync_transaction_with_target_resolver(
        app,
        context,
        after_apply,
        resolve_target_provider,
        true,
        Some(route_context),
    )
}

pub(crate) fn codex_provider_sync_transaction_reversible_for_trusted_route<
    R: tauri::Runtime,
    T,
    F,
>(
    app: &tauri::AppHandle<R>,
    context: CodexProviderSyncContext,
    route_context: CodexProviderSyncRouteContext,
    after_apply: F,
) -> AppResult<(
    CodexProviderSyncResult,
    T,
    Option<CodexProviderSyncRollback>,
)>
where
    F: FnOnce(&CodexProviderSyncResult) -> AppResult<T>,
{
    codex_provider_sync_transaction_with_target_resolver(
        app,
        context,
        after_apply,
        resolve_trusted_target_provider,
        true,
        Some(route_context),
    )
}

fn codex_provider_sync_transaction_with_target_resolver<R: tauri::Runtime, T, F>(
    app: &tauri::AppHandle<R>,
    context: CodexProviderSyncContext,
    after_apply: F,
    resolve_target: fn(&str) -> AppResult<String>,
    defer_backup_prune: bool,
    route_context: Option<CodexProviderSyncRouteContext>,
) -> AppResult<(
    CodexProviderSyncResult,
    T,
    Option<CodexProviderSyncRollback>,
)>
where
    F: FnOnce(&CodexProviderSyncResult) -> AppResult<T>,
{
    let home = crate::codex_paths::codex_home_dir(app)?;
    let target_provider = resolve_target(&context.target_provider)?;
    if codex_app_is_running()? {
        return Err("CODEX_PROVIDER_SYNC_PROCESS_RUNNING: Codex App is running".into());
    }
    let lock_path = home.join(PROVIDER_SYNC_LOCK_FILE);
    let _lock_guard = acquire_lock(&lock_path)?;

    if codex_app_is_running()? {
        return Err("CODEX_PROVIDER_SYNC_PROCESS_RUNNING: Codex App is running".into());
    }

    let config_path = crate::codex_paths::codex_config_toml_path(app)?;
    if config_path.exists() && is_symlink(&config_path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            config_path.display()
        )
        .into());
    }

    let current_config = read_optional_file_with_max_len(&config_path, PROVIDER_SYNC_MAX_BYTES)?;
    let current_config_text = optional_config_bytes_to_utf8(current_config.clone())?;
    let current_provider = read_current_provider(&current_config_text)?;

    let change_set = build_change_set(
        app,
        &home,
        &context,
        &current_config_text,
        current_provider.as_deref(),
    )?;

    if change_set.session_changes.is_empty()
        && change_set.sqlite_changes.iter().all(|change| {
            change.provider_rows_updated == 0
                && change.user_event_rows_updated == 0
                && change.cwd_rows_updated == 0
        })
        && change_set.global_state_change.is_none()
        && change_set.config_bytes.is_none()
    {
        let sync_result = CodexProviderSyncResult {
            status: "up_to_date".to_string(),
            target_provider,
            trigger: context.trigger,
            backup_dir: None,
            changed_session_files: Vec::new(),
            sqlite_provider_rows_updated: 0,
            sqlite_user_event_rows_updated: 0,
            sqlite_cwd_rows_updated: 0,
            updated_workspace_roots: Vec::new(),
            warning: None,
        };
        let extra = after_apply(&sync_result)?;
        return Ok((sync_result, extra, None));
    }

    let backup_root_existed = home.join(PROVIDER_SYNC_BACKUP_ROOT).exists();
    let backup_dir = create_backup(&home, &context, &change_set)?;
    let mut snapshots = snapshot_paths(&home, &config_path, &change_set)?;
    let operation_id = route_context
        .as_ref()
        .map(|route| route.operation_id.clone())
        .unwrap_or_else(|| format!("provider-sync-{}-{}", now_unix_millis(), std::process::id()));
    let target_config_sha256 = sha256_hex(
        change_set
            .config_bytes
            .as_deref()
            .or(current_config.as_deref())
            .unwrap_or_default(),
    );
    if let Err(error) = prepare_provider_sync_transaction(
        &home,
        &operation_id,
        &target_provider,
        &target_config_sha256,
        route_context,
        &backup_dir,
        backup_root_existed,
        &snapshots,
    ) {
        let cleanup = remove_created_provider_sync_backup(&home, &backup_dir, !backup_root_existed);
        return match cleanup {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!(
                "CODEX_PROVIDER_SYNC_ROLLBACK_FAILED: failed to prepare transaction: {error}; backup cleanup error: {cleanup_error}"
            )
            .into()),
        };
    }
    let mut writes_started = false;
    let result = (|| -> AppResult<(CodexProviderSyncResult, T)> {
        if let Some(bytes) = change_set.config_bytes.as_ref() {
            writes_started = true;
            let _ = write_file_atomic_if_changed(&config_path, bytes)?;
        }
        for change in &change_set.session_changes {
            writes_started = true;
            let _ = write_file_atomic_if_changed(&change.path, &change.next_text)?;
        }
        if !change_set.sqlite_changes.is_empty() {
            writes_started = true;
        }
        let sqlite_counts = apply_sqlite_changes(&change_set.sqlite_changes, &target_provider)?;
        if let Some(global_state) = change_set.global_state_change.as_ref() {
            writes_started = true;
            apply_global_state_change(global_state)?;
        }

        let mut sync_result = CodexProviderSyncResult {
            status: "synced".to_string(),
            target_provider,
            trigger: context.trigger,
            backup_dir: Some(backup_dir.to_string_lossy().to_string()),
            changed_session_files: change_set
                .session_changes
                .iter()
                .map(|change| change.path.to_string_lossy().to_string())
                .collect(),
            sqlite_provider_rows_updated: sqlite_counts.provider_rows_updated,
            sqlite_user_event_rows_updated: sqlite_counts.user_event_rows_updated,
            sqlite_cwd_rows_updated: sqlite_counts.cwd_rows_updated,
            updated_workspace_roots: change_set.updated_workspace_roots,
            warning: change_set.warning,
        };
        let extra = after_apply(&sync_result)?;
        mark_provider_sync_transaction_applied(&home, &operation_id)?;
        if !defer_backup_prune {
            sync_result.warning = prune_managed_backups(&home)
                .ok()
                .and_then(|warning| warning)
                .or(sync_result.warning);
        }
        Ok((sync_result, extra))
    })();

    match result {
        Ok((sync_result, extra)) => Ok((
            sync_result,
            extra,
            writes_started.then(|| {
                CodexProviderSyncRollback::new(
                    snapshots,
                    Some(home.clone()),
                    defer_backup_prune.then(|| home.clone()),
                    defer_backup_prune.then(|| backup_dir.clone()),
                    defer_backup_prune && !backup_root_existed,
                )
            }),
        )),
        Err(err) => {
            let mut rollback_errors = Vec::new();
            if writes_started {
                if let Err(rollback_err) = restore_snapshots(&mut snapshots) {
                    rollback_errors.push(rollback_err.to_string());
                }
            }
            if let Err(rollback_err) =
                remove_created_provider_sync_backup(&home, &backup_dir, !backup_root_existed)
            {
                rollback_errors.push(rollback_err.to_string());
            }
            if rollback_errors.is_empty() {
                if let Err(rollback_err) = clear_provider_sync_transaction(&home) {
                    rollback_errors.push(rollback_err.to_string());
                }
            }
            if rollback_errors.is_empty() {
                Err(err)
            } else {
                Err(format!(
                    "CODEX_PROVIDER_SYNC_ROLLBACK_FAILED: failed after {err}; rollback error: {}",
                    rollback_errors.join("; ")
                )
                .into())
            }
        }
    }
}

pub fn codex_provider_sync_current<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    trigger: impl Into<String>,
) -> AppResult<CodexProviderSyncResult> {
    let config_path = crate::codex_paths::codex_config_toml_path(app)?;
    let current_config = read_optional_file_with_max_len(&config_path, PROVIDER_SYNC_MAX_BYTES)?;
    let current_config_text = optional_config_bytes_to_utf8(current_config)?;
    let target_provider = codex_provider_target_from_current_config_text(&current_config_text)?;
    codex_provider_sync(
        app,
        CodexProviderSyncContext {
            trigger: trigger.into(),
            target_provider,
            config_bytes: None,
        },
    )
}

pub fn codex_provider_sync_from_config_bytes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    trigger: impl Into<String>,
    config_bytes: Vec<u8>,
) -> AppResult<CodexProviderSyncResult> {
    let config_text = String::from_utf8(config_bytes.clone())
        .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let target_provider = codex_provider_target_from_config_text(&config_text)?;
    codex_provider_sync(
        app,
        CodexProviderSyncContext {
            trigger: trigger.into(),
            target_provider,
            config_bytes: Some(config_bytes),
        },
    )
}

pub(crate) fn codex_provider_identity_from_config_text(config_text: &str) -> AppResult<String> {
    Ok(read_current_provider(config_text)?.ok_or_else(|| {
        "CODEX_PROVIDER_SYNC_INVALID_TARGET: unsupported provider target=(missing)".to_string()
    })?)
}

pub fn codex_provider_target_from_config_text(config_text: &str) -> AppResult<String> {
    let current_provider = codex_provider_identity_from_config_text(config_text)?;
    resolve_target_provider(&current_provider)
}

pub(crate) fn codex_provider_target_from_patch_config_text(config_text: &str) -> AppResult<String> {
    match read_current_provider(config_text)? {
        Some(provider) => resolve_target_provider(&provider),
        None => Ok(MANAGED_PROVIDER_AIO.to_string()),
    }
}

pub fn codex_provider_target_from_current_config_text(config_text: &str) -> AppResult<String> {
    Ok(read_current_provider(config_text)?.unwrap_or_else(|| MANAGED_PROVIDER_AIO.to_string()))
}

pub(crate) fn codex_provider_sync_plan_for_target(
    current_config_text: &str,
    target_provider: &str,
) -> AppResult<CodexProviderSyncPlan> {
    codex_provider_sync_plan_for_target_with_resolver(
        current_config_text,
        target_provider,
        resolve_target_provider,
    )
}

pub(crate) fn codex_provider_sync_plan_for_trusted_target(
    current_config_text: &str,
    target_provider: &str,
) -> AppResult<CodexProviderSyncPlan> {
    codex_provider_sync_plan_for_target_with_resolver(
        current_config_text,
        target_provider,
        resolve_trusted_target_provider,
    )
}

fn codex_provider_sync_plan_for_target_with_resolver(
    current_config_text: &str,
    target_provider: &str,
    resolve_target: fn(&str) -> AppResult<String>,
) -> AppResult<CodexProviderSyncPlan> {
    let current_provider = read_current_provider(current_config_text)?;
    let target_provider = resolve_target(target_provider)?;
    let change_required = current_provider.as_deref() != Some(target_provider.as_str());
    Ok(CodexProviderSyncPlan {
        current_provider,
        target_provider,
        change_required,
        codex_must_be_closed: change_required,
    })
}

pub(crate) fn codex_provider_sync_plan_for_config_text(
    current_config_text: &str,
    target_config_text: &str,
) -> AppResult<CodexProviderSyncPlan> {
    let target_provider = codex_provider_target_from_config_text(target_config_text)?;
    codex_provider_sync_plan_for_target(current_config_text, &target_provider)
}

fn resolve_target_provider(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    match trimmed {
        MANAGED_PROVIDER_AIO | MANAGED_PROVIDER_OPENAI => Ok(trimmed.to_string()),
        _ => Err(format!(
            "CODEX_PROVIDER_SYNC_INVALID_TARGET: unsupported provider target={trimmed}"
        )
        .into()),
    }
}

fn resolve_trusted_target_provider(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(
            "CODEX_PROVIDER_SYNC_INVALID_TARGET: unsupported provider target=(missing)".into(),
        );
    }
    Ok(trimmed.to_string())
}

fn read_current_provider(text: &str) -> AppResult<Option<String>> {
    if text.trim().is_empty() {
        return Ok(None);
    }

    let value = toml::from_str::<toml::Value>(text)
        .map_err(|err| format!("CODEX_PROVIDER_SYNC_INVALID_CONFIG: invalid config.toml: {err}"))?;
    let provider = value
        .as_table()
        .and_then(|table| table.get("model_provider"))
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(ToString::to_string);
    Ok(provider)
}

fn optional_config_bytes_to_utf8(bytes: Option<Vec<u8>>) -> AppResult<String> {
    match bytes {
        Some(bytes) => String::from_utf8(bytes).map_err(|_| {
            "CODEX_PROVIDER_SYNC_INVALID_CONFIG: config.toml must be valid UTF-8".into()
        }),
        None => Ok(String::new()),
    }
}

fn acquire_lock(path: &Path) -> AppResult<LockGuard> {
    if path.exists() {
        return Err(format!("CODEX_PROVIDER_SYNC_LOCKED: {}", path.display()).into());
    }
    if let Some(parent) = path.parent() {
        ensure_safe_operational_dir(parent, "Codex provider sync lock parent")?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create lock dir {}: {e}", parent.display()))?;
    }
    fs::create_dir(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("CODEX_PROVIDER_SYNC_LOCKED: {}", path.display())
        } else {
            format!("failed to acquire lock {}: {e}", path.display())
        }
    })?;
    fs::write(
        path.join("owner.json"),
        serde_json::json!({
            "pid": std::process::id(),
            "startedAt": now_unix_millis(),
        })
        .to_string(),
    )
    .map_err(|e| format!("failed to write lock owner {}: {e}", path.display()))?;
    Ok(LockGuard {
        path: path.to_path_buf(),
    })
}

struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn codex_app_is_running() -> AppResult<bool> {
    match CODEX_APP_RUNNING_OVERRIDE.load(Ordering::SeqCst) {
        CODEX_APP_RUNNING_OVERRIDE_FALSE => return Ok(false),
        CODEX_APP_RUNNING_OVERRIDE_TRUE => return Ok(true),
        _ => {}
    }

    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Codex.exe", "/NH"])
            .output()
            .map_err(|err| codex_process_check_failed_message("tasklist", err.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                format!("exit status {}; stderr: {}", output.status, stderr)
            };
            return Err(codex_process_check_failed_message("tasklist", detail).into());
        }
        let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
        Ok(text.contains("codex.exe"))
    }

    #[cfg(not(windows))]
    codex_app_is_running_from_ps()
}

#[cfg(not(windows))]
fn codex_app_is_running_from_ps() -> AppResult<bool> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "comm="])
        .output()
        .map_err(|err| codex_process_check_failed_message("ps", err.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            format!("exit status {}; stderr: {}", output.status, stderr)
        };
        return Err(codex_process_check_failed_message("ps", detail).into());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().any(process_name_is_codex_app))
}

#[cfg(not(windows))]
fn process_name_is_codex_app(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed == "Codex" || trimmed == "Codex.exe" {
        return true;
    }
    Path::new(trimmed)
        .file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|stem| stem == "Codex")
}

#[doc(hidden)]
pub(crate) fn set_codex_app_running_override_for_tests(running: Option<bool>) {
    let value = match running {
        Some(false) => CODEX_APP_RUNNING_OVERRIDE_FALSE,
        Some(true) => CODEX_APP_RUNNING_OVERRIDE_TRUE,
        None => CODEX_APP_RUNNING_OVERRIDE_NONE,
    };
    CODEX_APP_RUNNING_OVERRIDE.store(value, Ordering::SeqCst);
}

fn build_change_set<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    home: &Path,
    context: &CodexProviderSyncContext,
    current_config_text: &str,
    current_provider: Option<&str>,
) -> AppResult<SyncChangeSet> {
    let mut config_bytes = None;

    if let Some(bytes) = context.config_bytes.as_ref() {
        let next_config_text = String::from_utf8(bytes.clone())
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
        ensure_within_codex_len(next_config_text.as_bytes(), "codex config.toml")?;
        if next_config_text != current_config_text {
            config_bytes = Some(next_config_text.into_bytes());
        }
    }

    let session_changes =
        collect_session_changes(home, current_provider, &context.target_provider)?;
    let sqlite_changes = collect_sqlite_changes(home, current_provider, &context.target_provider)?;
    let global_state_change =
        collect_global_state_change(home, current_provider, &context.target_provider)?;

    let updated_workspace_roots = global_state_change
        .as_ref()
        .map(|change| change.updated_workspace_roots.clone())
        .unwrap_or_default();

    Ok(SyncChangeSet {
        config_bytes,
        session_changes,
        sqlite_changes,
        global_state_change,
        updated_workspace_roots,
        warning: None,
    })
}

fn ensure_within_codex_len(bytes: &[u8], label: &str) -> AppResult<()> {
    if bytes.len() > PROVIDER_SYNC_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} too large (max {PROVIDER_SYNC_MAX_BYTES} bytes)"
        )
        .into());
    }
    Ok(())
}

#[cfg(windows)]
fn normalize_path_for_prefix_match(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn candidate_within_codex_home(
    canonical_home: &Path,
    candidate: &Path,
    label: &str,
) -> AppResult<bool> {
    let Ok(canonical_candidate) = fs::canonicalize(candidate) else {
        return Ok(false);
    };

    #[cfg(windows)]
    {
        let candidate_s = normalize_path_for_prefix_match(&canonical_candidate);
        let home_s = normalize_path_for_prefix_match(canonical_home);
        if candidate_s == home_s || candidate_s.starts_with(&(home_s.clone() + "/")) {
            return Ok(true);
        }
    }

    #[cfg(not(windows))]
    {
        if canonical_candidate.starts_with(canonical_home) {
            return Ok(true);
        }
    }

    Err(format!(
        "SEC_INVALID_INPUT: {label} resolved outside Codex home path={}",
        candidate.display()
    )
    .into())
}

fn non_symlink_metadata(path: &Path, label: &str) -> AppResult<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "SEC_INVALID_INPUT: refusing to follow symlink {label} path={}",
                    path.display()
                )
                .into());
            }
            Ok(Some(metadata))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!(
            "failed to read metadata for {label} {}: {err}",
            path.display()
        )
        .into()),
    }
}

fn ensure_safe_operational_dir(path: &Path, label: &str) -> AppResult<()> {
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if !ancestor.exists() {
            continue;
        }
        let Some(metadata) = non_symlink_metadata(ancestor, label)? else {
            continue;
        };
        if !metadata.is_dir() {
            return Err(format!(
                "SEC_INVALID_INPUT: {label} is not a directory path={}",
                ancestor.display()
            )
            .into());
        }
    }
    Ok(())
}

fn collect_session_changes(
    home: &Path,
    _current_provider: Option<&str>,
    target_provider: &str,
) -> AppResult<Vec<SessionChange>> {
    let mut changes = Vec::new();
    let canonical_home = fs::canonicalize(home)
        .map_err(|e| format!("failed to canonicalize Codex home {}: {e}", home.display()))?;
    for dir in ["sessions", "archived_sessions"] {
        let root = home.join(dir);
        let Some(metadata) = non_symlink_metadata(&root, "Codex session root")? else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        if !candidate_within_codex_home(&canonical_home, &root, "Codex session root")? {
            continue;
        }
        collect_rollout_changes(&canonical_home, &root, target_provider, &mut changes)?;
    }
    Ok(changes)
}

fn collect_rollout_changes(
    canonical_home: &Path,
    root: &Path,
    target_provider: &str,
    out: &mut Vec<SessionChange>,
) -> AppResult<()> {
    let Some(metadata) = non_symlink_metadata(root, "Codex session root")? else {
        return Ok(());
    };
    if !metadata.is_dir() {
        return Ok(());
    }
    if !candidate_within_codex_home(canonical_home, root, "Codex session root")? {
        return Ok(());
    }
    for entry in
        fs::read_dir(root).map_err(|e| format!("failed to read {}: {e}", root.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read dir entry {}: {e}", root.display()))?;
        let path = entry.path();
        let Some(metadata) = non_symlink_metadata(&path, "Codex session entry")? else {
            continue;
        };
        if !candidate_within_codex_home(canonical_home, &path, "Codex session entry")? {
            continue;
        }
        if metadata.is_dir() {
            collect_rollout_changes(canonical_home, &path, target_provider, out)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        {
            continue;
        }
        let bytes = fs::read(&path)
            .map_err(|e| format!("failed to read rollout file {}: {e}", path.display()))?;
        let next_bytes = rewrite_rollout_session_meta_providers(&bytes, target_provider)?;
        if next_bytes != bytes {
            out.push(SessionChange {
                path,
                original_text: bytes,
                next_text: next_bytes,
            });
        }
    }
    Ok(())
}

fn rewrite_rollout_session_meta_providers(
    bytes: &[u8],
    target_provider: &str,
) -> AppResult<Vec<u8>> {
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| "SEC_INVALID_INPUT: rollout jsonl must be valid UTF-8".to_string())?;
    let mut out = String::with_capacity(text.len());
    // The complete file is atomically replaced, so a provider may grow or shrink.
    // Only parsed session_meta rows are reserialized; every other JSONL segment
    // and its original line ending remain byte-for-byte unchanged.
    for segment in text.split_inclusive('\n') {
        let (line, ending) = split_line_ending(segment);
        let next_line = match serde_json::from_str::<Value>(line) {
            Ok(mut value) if value.get("type").and_then(Value::as_str) == Some("session_meta") => {
                if let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) {
                    payload.insert(
                        "model_provider".to_string(),
                        Value::String(target_provider.to_string()),
                    );
                    serde_json::to_string(&value)
                        .map_err(|e| format!("failed to rewrite rollout row: {e}"))?
                } else {
                    line.to_string()
                }
            }
            _ => line.to_string(),
        };
        out.push_str(&next_line);
        out.push_str(ending);
    }
    Ok(out.into_bytes())
}

fn split_line_ending(segment: &str) -> (&str, &str) {
    if let Some(line) = segment.strip_suffix("\r\n") {
        (line, "\r\n")
    } else if let Some(line) = segment.strip_suffix('\n') {
        (line, "\n")
    } else {
        (segment, "")
    }
}

fn collect_sqlite_changes(
    home: &Path,
    current_provider: Option<&str>,
    target_provider: &str,
) -> AppResult<Vec<SqliteDbChange>> {
    let mut changes = Vec::new();
    let canonical_home = fs::canonicalize(home)
        .map_err(|e| format!("failed to canonicalize Codex home {}: {e}", home.display()))?;
    for db_path in codex_session_db_paths_from_home(home)? {
        let Some(metadata) = non_symlink_metadata(&db_path, "Codex sqlite db")? else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        if !candidate_within_codex_home(&canonical_home, &db_path, "Codex sqlite db")? {
            continue;
        }
        let change = collect_sqlite_change(&db_path, current_provider, target_provider)?;
        if change.provider_rows_updated > 0
            || change.user_event_rows_updated > 0
            || change.cwd_rows_updated > 0
        {
            changes.push(change);
        }
    }
    Ok(changes)
}

fn codex_session_db_paths_from_home(home: &Path) -> AppResult<Vec<PathBuf>> {
    let mut paths = codex_sqlite_dir_session_dbs(home)?;
    let legacy = home.join("state_5.sqlite");
    if !paths.iter().any(|path| path == &legacy) {
        paths.push(legacy);
    }
    Ok(paths)
}

fn codex_sqlite_dir_session_dbs(home: &Path) -> AppResult<Vec<PathBuf>> {
    let sqlite_dir = home.join("sqlite");
    let Some(metadata) = non_symlink_metadata(&sqlite_dir, "Codex sqlite dir")? else {
        return Ok(Vec::new());
    };
    if !metadata.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(&sqlite_dir)
        .map_err(|e| format!("failed to read sqlite dir {}: {e}", sqlite_dir.display()))?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            format!(
                "failed to read sqlite dir entry {}: {e}",
                sqlite_dir.display()
            )
        })?;
        let path = entry.path();
        let Some(metadata) = non_symlink_metadata(&path, "Codex sqlite candidate")? else {
            continue;
        };
        if !metadata.is_file() || !is_sqlite_candidate(&path) || !has_session_table(&path) {
            continue;
        }
        candidates.push(path);
    }
    candidates.sort_by_key(|path| {
        (
            path.file_name()
                .map(|name| name != std::ffi::OsStr::new("codex-dev.db"))
                .unwrap_or(true),
            path.file_name().map(|name| name.to_os_string()),
        )
    });
    Ok(candidates)
}

fn is_sqlite_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("db") | Some("sqlite") | Some("sqlite3")
    )
}

fn codex_sqlite_sidecar_paths(db_path: &Path) -> [PathBuf; 3] {
    [
        db_path.to_path_buf(),
        PathBuf::from(format!("{}-wal", db_path.to_string_lossy())),
        PathBuf::from(format!("{}-shm", db_path.to_string_lossy())),
    ]
}

fn has_session_table(path: &Path) -> bool {
    ["threads", "automation_runs", "inbox_items"]
        .iter()
        .any(|table| sqlite_has_table(path, table))
}

fn sqlite_has_table(path: &Path, table: &str) -> bool {
    let Ok(db) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    db.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}

fn collect_sqlite_change(
    path: &Path,
    _current_provider: Option<&str>,
    target_provider: &str,
) -> AppResult<SqliteDbChange> {
    let existed = path.exists();
    if !existed {
        return Ok(SqliteDbChange {
            path: path.to_path_buf(),
            provider_rows_updated: 0,
            user_event_rows_updated: 0,
            cwd_rows_updated: 0,
        });
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("failed to open sqlite db {}: {e}", path.display()))?;
    let columns = sqlite_columns(&conn, "threads")?;
    if !columns.contains("model_provider") {
        return Ok(SqliteDbChange {
            path: path.to_path_buf(),
            provider_rows_updated: 0,
            user_event_rows_updated: 0,
            cwd_rows_updated: 0,
        });
    }
    let provider_rows_updated = conn
        .query_row(
            "SELECT COUNT(*) FROM threads WHERE COALESCE(model_provider, '') <> ?1",
            [target_provider],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| {
            format!(
                "failed to count sqlite provider rows {}: {e}",
                path.display()
            )
        })? as usize;
    let user_event_rows_updated = if columns.contains("has_user_event") {
        count_user_event_rows(&conn)?
    } else {
        0
    };
    Ok(SqliteDbChange {
        path: path.to_path_buf(),
        provider_rows_updated,
        user_event_rows_updated,
        cwd_rows_updated: 0,
    })
}

fn sqlite_columns(conn: &Connection, table: &str) -> AppResult<HashSet<String>> {
    let mut stmt = conn
        .prepare(&format!(
            "PRAGMA table_info(\"{}\")",
            table.replace('"', "\"\"")
        ))
        .map_err(|e| format!("failed to inspect sqlite columns {table}: {e}"))?;
    let mut cols = HashSet::new();
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("failed to inspect sqlite columns {table}: {e}"))?;
    for row in rows {
        cols.insert(row.map_err(|e| format!("failed to read sqlite column {table}: {e}"))?);
    }
    Ok(cols)
}

fn count_user_event_rows(conn: &Connection) -> AppResult<usize> {
    let mut stmt = conn
        .prepare("SELECT COUNT(*) FROM threads WHERE COALESCE(has_user_event, 0) <> 1")
        .map_err(|e| format!("failed to count has_user_event rows: {e}"))?;
    let count: i64 = stmt
        .query_row([], |row| row.get(0))
        .map_err(|e| format!("failed to count has_user_event rows: {e}"))?;
    Ok(count as usize)
}

fn apply_sqlite_changes(
    changes: &[SqliteDbChange],
    target_provider: &str,
) -> AppResult<SqliteCounts> {
    let mut totals = SqliteCounts::default();
    for change in changes {
        if !change.path.exists() {
            continue;
        }
        let mut conn = Connection::open(&change.path)
            .map_err(|e| format!("failed to open sqlite db {}: {e}", change.path.display()))?;
        let columns = sqlite_columns(&conn, "threads")?;
        if !columns.contains("model_provider") {
            continue;
        }
        let tx = conn.transaction().map_err(|e| {
            format!(
                "failed to start sqlite transaction {}: {e}",
                change.path.display()
            )
        })?;
        totals.provider_rows_updated += tx
            .execute(
                "UPDATE threads SET model_provider = ?1 WHERE COALESCE(model_provider, '') <> ?1",
                [target_provider],
            )
            .map_err(|e| {
                format!(
                    "failed to update sqlite provider rows {}: {e}",
                    change.path.display()
                )
            })?;
        if columns.contains("has_user_event") {
            totals.user_event_rows_updated += tx
                .execute(
                    "UPDATE threads SET has_user_event = 1 WHERE COALESCE(has_user_event, 0) <> 1",
                    [],
                )
                .map_err(|e| {
                    format!(
                        "failed to update sqlite user_event rows {}: {e}",
                        change.path.display()
                    )
                })?;
        }
        tx.commit().map_err(|e| {
            format!(
                "failed to commit sqlite transaction {}: {e}",
                change.path.display()
            )
        })?;
    }
    Ok(totals)
}

#[derive(Default)]
struct SqliteCounts {
    provider_rows_updated: usize,
    user_event_rows_updated: usize,
    cwd_rows_updated: usize,
}

fn collect_global_state_change(
    home: &Path,
    _current_provider: Option<&str>,
    target_provider: &str,
) -> AppResult<Option<GlobalStateChange>> {
    let path = home.join(".codex-global-state.json");
    let Some(metadata) = non_symlink_metadata(&path, "Codex global state")? else {
        return Ok(None);
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    let original_bytes = fs::read(&path)
        .map_err(|e| format!("failed to snapshot global state {}: {e}", path.display()))?;
    let original: Value = serde_json::from_slice(&original_bytes)
        .map_err(|e| format!("failed to parse global state {}: {e}", path.display()))?;
    let mut next = normalized_global_state(&original);
    next.insert(
        "model_provider".to_string(),
        Value::String(target_provider.to_string()),
    );
    let next_value = Value::Object(next.clone());
    let mut next_bytes = serde_json::to_vec_pretty(&next_value)
        .map_err(|e| format!("failed to serialize global state {}: {e}", path.display()))?;
    next_bytes.push(b'\n');
    if next_bytes == original_bytes {
        return Ok(None);
    }
    let bak_path = home.join(".codex-global-state.json.bak");
    Ok(Some(GlobalStateChange {
        path,
        original_bytes: Some(original_bytes),
        next_bytes: Some(next_bytes),
        bak_path,
        bak_next_bytes: Some(None),
        updated_workspace_roots: next
            .get("electron-saved-workspace-roots")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    }))
}

fn normalized_global_state(state: &Value) -> Map<String, Value> {
    let Some(obj) = state.as_object() else {
        return Map::new();
    };
    obj.clone()
}

fn apply_global_state_change(change: &GlobalStateChange) -> AppResult<()> {
    if let Some(bytes) = change.next_bytes.as_ref() {
        let _ = write_file_atomic_if_changed(&change.path, bytes)?;
    }
    match change.bak_next_bytes.as_ref() {
        Some(Some(bytes)) => {
            let _ = write_file_atomic_if_changed(&change.bak_path, bytes)?;
        }
        Some(None) if change.bak_path.exists() => {
            fs::remove_file(&change.bak_path)
                .map_err(|e| format!("failed to remove bak {}: {e}", change.bak_path.display()))?;
        }
        Some(None) | None => {}
    }
    Ok(())
}

fn create_backup(
    home: &Path,
    context: &CodexProviderSyncContext,
    change_set: &SyncChangeSet,
) -> AppResult<PathBuf> {
    let root = home.join(PROVIDER_SYNC_BACKUP_ROOT);
    ensure_safe_operational_dir(&root, "Codex provider sync backup root")?;
    fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create backup root {}: {e}", root.display()))?;
    let mut backup_dir = root.join(format!("{}-{}", now_unix_seconds(), std::process::id()));
    let mut suffix = 0usize;
    while backup_dir.exists() {
        suffix += 1;
        backup_dir = root.join(format!(
            "{}-{}-{suffix}",
            now_unix_seconds(),
            std::process::id()
        ));
    }
    fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("failed to create backup dir {}: {e}", backup_dir.display()))?;

    let mut manifest = BackupManifest {
        version: 1,
        trigger: context.trigger.clone(),
        target_provider: context.target_provider.clone(),
        created_at: now_unix_millis().to_string(),
        managed_by: "Codex provider sync".to_string(),
        config_path: None,
        session_files: Vec::new(),
        sqlite_files: Vec::new(),
        global_state_path: None,
    };

    let config_path = home.join("config.toml");
    if let Some(metadata) = non_symlink_metadata(&config_path, "Codex config.toml backup source")? {
        if !metadata.is_file() {
            return Err(format!(
                "SEC_INVALID_INPUT: Codex config.toml backup source is not a file path={}",
                config_path.display()
            )
            .into());
        }
        let target = backup_dir.join("config.toml");
        fs::copy(&config_path, &target)
            .map_err(|e| format!("failed to backup {}: {e}", config_path.display()))?;
        manifest.config_path = Some(target.to_string_lossy().to_string());
    }

    for change in &change_set.session_changes {
        let target = backup_dir.join(change.path.strip_prefix(home).unwrap_or(&change.path));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create backup parent {}: {e}", parent.display()))?;
        }
        fs::write(&target, &change.original_text).map_err(|e| {
            format!(
                "failed to backup session file {}: {e}",
                change.path.display()
            )
        })?;
        manifest
            .session_files
            .push(target.to_string_lossy().to_string());
    }

    for change in &change_set.sqlite_changes {
        for source in codex_sqlite_sidecar_paths(&change.path) {
            let Some(metadata) = non_symlink_metadata(&source, "Codex sqlite backup source")?
            else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let target = backup_dir.join(source.strip_prefix(home).unwrap_or(&source));
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("failed to create backup parent {}: {e}", parent.display())
                })?;
            }
            fs::copy(&source, &target)
                .map_err(|e| format!("failed to backup sqlite file {}: {e}", source.display()))?;
            manifest
                .sqlite_files
                .push(target.to_string_lossy().to_string());
        }
    }

    if let Some(change) = change_set.global_state_change.as_ref() {
        let target = backup_dir.join(".codex-global-state.json");
        fs::write(
            &target,
            change.original_bytes.as_deref().unwrap_or_default(),
        )
        .map_err(|e| {
            format!(
                "failed to backup global state {}: {e}",
                change.path.display()
            )
        })?;
        manifest.global_state_path = Some(target.to_string_lossy().to_string());
    }

    fs::write(
        backup_dir.join(PROVIDER_SYNC_MANAGED_BACKUP_MANIFEST),
        serde_json::to_vec_pretty(&manifest)
            .map_err(|e| format!("failed to serialize backup manifest: {e}"))?,
    )
    .map_err(|e| format!("failed to write backup manifest: {e}"))?;

    Ok(backup_dir)
}

fn snapshot_paths(
    home: &Path,
    config_path: &Path,
    change_set: &SyncChangeSet,
) -> AppResult<Vec<FileSnapshot>> {
    let mut snapshots = Vec::new();
    let mut aggregate_bytes = 0usize;
    push_snapshot_with_budget(&mut snapshots, config_path, &mut aggregate_bytes)?;
    push_snapshot_with_budget(
        &mut snapshots,
        &home.join("config.toml.bak"),
        &mut aggregate_bytes,
    )?;
    for change in &change_set.session_changes {
        push_snapshot_with_budget(&mut snapshots, &change.path, &mut aggregate_bytes)?;
    }
    for change in &change_set.sqlite_changes {
        for path in codex_sqlite_sidecar_paths(&change.path) {
            push_snapshot_with_budget(&mut snapshots, &path, &mut aggregate_bytes)?;
        }
    }
    if let Some(change) = change_set.global_state_change.as_ref() {
        push_snapshot_with_budget(&mut snapshots, &change.path, &mut aggregate_bytes)?;
        push_snapshot_with_budget(&mut snapshots, &change.bak_path, &mut aggregate_bytes)?;
    }
    if snapshots.len() > PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync snapshot count exceeds {}",
            PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS
        )
        .into());
    }
    Ok(snapshots)
}

#[cfg(test)]
fn snapshot_path(path: &Path) -> AppResult<FileSnapshot> {
    let mut aggregate_bytes = 0usize;
    snapshot_path_with_budget(path, &mut aggregate_bytes)
}

fn push_snapshot_with_budget(
    snapshots: &mut Vec<FileSnapshot>,
    path: &Path,
    aggregate_bytes: &mut usize,
) -> AppResult<()> {
    snapshots.push(snapshot_path_with_budget(path, aggregate_bytes)?);
    Ok(())
}

fn snapshot_path_with_budget(path: &Path, aggregate_bytes: &mut usize) -> AppResult<FileSnapshot> {
    let Some(metadata) = non_symlink_metadata(path, "Codex provider sync snapshot")? else {
        return Ok(FileSnapshot {
            path: path.to_path_buf(),
            existed: false,
            bytes: None,
        });
    };
    if !metadata.is_file() {
        return Err(format!(
            "SEC_INVALID_INPUT: snapshot target is not a file path={}",
            path.display()
        )
        .into());
    };
    if metadata.len() > PROVIDER_SYNC_SNAPSHOT_MAX_BYTES as u64 {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync snapshot exceeds {} bytes path={}",
            PROVIDER_SYNC_SNAPSHOT_MAX_BYTES,
            path.display()
        )
        .into());
    }
    if (*aggregate_bytes as u64).saturating_add(metadata.len())
        > PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES as u64
    {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync snapshot aggregate exceeds {} bytes",
            PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES
        )
        .into());
    }
    let bytes = read_file_with_max_len(path, PROVIDER_SYNC_SNAPSHOT_MAX_BYTES)?;
    let next_aggregate = aggregate_bytes.saturating_add(bytes.len());
    if next_aggregate > PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync snapshot aggregate exceeds {} bytes",
            PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES
        )
        .into());
    }
    *aggregate_bytes = next_aggregate;
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        existed: true,
        bytes: Some(bytes),
    })
}

fn restore_snapshots(snapshots: &mut [FileSnapshot]) -> AppResult<()> {
    for snapshot in snapshots.iter().rev() {
        if snapshot.existed {
            if let Some(bytes) = snapshot.bytes.as_ref() {
                fs::write(&snapshot.path, bytes)
                    .map_err(|e| format!("failed to restore {}: {e}", snapshot.path.display()))?;
            }
        } else if snapshot.path.exists() {
            fs::remove_file(&snapshot.path).map_err(|e| {
                format!("failed to remove restored {}: {e}", snapshot.path.display())
            })?;
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validated_relative_path(value: &str, label: &str) -> AppResult<PathBuf> {
    if value.is_empty() {
        return Err(format!("SEC_INVALID_INPUT: {label} must not be empty").into());
    }
    let path = PathBuf::from(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} must be a normalized relative path={value}"
        )
        .into());
    }
    Ok(path)
}

fn path_relative_to_home(home: &Path, path: &Path, label: &str) -> AppResult<String> {
    let relative = path.strip_prefix(home).map_err(|_| {
        format!(
            "SEC_INVALID_INPUT: {label} resolved outside Codex home path={}",
            path.display()
        )
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(format!(
                "SEC_INVALID_INPUT: {label} must be a normalized relative path={}",
                path.display()
            )
            .into());
        };
        parts.push(
            value
                .to_str()
                .ok_or_else(|| {
                    format!(
                        "SEC_INVALID_INPUT: {label} path is not valid Unicode path={}",
                        path.display()
                    )
                })?
                .to_string(),
        );
    }
    let normalized = parts.join("/");
    let _ = validated_relative_path(&normalized, label)?;
    Ok(normalized)
}

fn provider_sync_transaction_root(home: &Path) -> PathBuf {
    home.join(PROVIDER_SYNC_TRANSACTION_ROOT)
}

fn provider_sync_transaction_manifest_path(home: &Path) -> PathBuf {
    provider_sync_transaction_root(home).join(PROVIDER_SYNC_TRANSACTION_MANIFEST)
}

fn provider_sync_transaction_applied_path(home: &Path) -> PathBuf {
    provider_sync_transaction_root(home).join(PROVIDER_SYNC_TRANSACTION_APPLIED_MARKER)
}

#[allow(clippy::too_many_arguments)]
fn prepare_provider_sync_transaction(
    home: &Path,
    operation_id: &str,
    target_provider: &str,
    target_config_sha256: &str,
    route_context: Option<CodexProviderSyncRouteContext>,
    backup_dir: &Path,
    backup_root_existed: bool,
    snapshots: &[FileSnapshot],
) -> AppResult<()> {
    if snapshots.len() > PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync snapshot count exceeds {}",
            PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS
        )
        .into());
    }
    let mut aggregate_bytes = 0usize;
    for snapshot in snapshots {
        match (snapshot.existed, snapshot.bytes.as_deref()) {
            (true, Some(bytes)) => {
                if bytes.len() > PROVIDER_SYNC_SNAPSHOT_MAX_BYTES {
                    return Err(format!(
                        "SEC_INVALID_INPUT: provider sync snapshot exceeds {} bytes path={}",
                        PROVIDER_SYNC_SNAPSHOT_MAX_BYTES,
                        snapshot.path.display()
                    )
                    .into());
                }
                aggregate_bytes = aggregate_bytes.saturating_add(bytes.len());
                if aggregate_bytes > PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES {
                    return Err(format!(
                        "SEC_INVALID_INPUT: provider sync snapshot aggregate exceeds {} bytes",
                        PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES
                    )
                    .into());
                }
            }
            (false, None) => {}
            _ => {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_SNAPSHOT_INVALID: snapshot existence metadata is inconsistent path={}",
                    snapshot.path.display()
                )
                .into())
            }
        }
    }

    let root = provider_sync_transaction_root(home);
    if root.exists() {
        return Err(format!(
            "CODEX_PROVIDER_SYNC_RECOVERY_REQUIRED: pending transaction exists at {}",
            root.display()
        )
        .into());
    }
    cleanup_provider_sync_transaction_staging(home)?;

    let tmp_root = home.join("tmp");
    ensure_safe_operational_dir(&tmp_root, "Codex provider sync transaction parent")?;
    fs::create_dir_all(&tmp_root).map_err(|error| {
        format!(
            "failed to create provider sync transaction parent {}: {error}",
            tmp_root.display()
        )
    })?;
    let staging = tmp_root.join(format!(
        "{PROVIDER_SYNC_TRANSACTION_STAGING_PREFIX}{}-{}",
        std::process::id(),
        now_unix_millis()
    ));
    fs::create_dir(&staging).map_err(|error| {
        format!(
            "failed to create provider sync transaction staging {}: {error}",
            staging.display()
        )
    })?;

    let prepared = (|| -> AppResult<ProviderSyncTransactionJournal> {
        let files_root = staging.join("files");
        fs::create_dir(&files_root).map_err(|error| {
            format!(
                "failed to create provider sync snapshot root {}: {error}",
                files_root.display()
            )
        })?;
        let mut persistent_snapshots = Vec::with_capacity(snapshots.len());
        for (index, snapshot) in snapshots.iter().enumerate() {
            let target_rel =
                path_relative_to_home(home, &snapshot.path, "Codex provider sync snapshot target")?;
            let (backup_rel, byte_len, sha256) = if snapshot.existed {
                let bytes = snapshot.bytes.as_deref().ok_or_else(|| {
                    format!(
                        "CODEX_PROVIDER_SYNC_SNAPSHOT_INVALID: existing snapshot has no bytes path={}",
                        snapshot.path.display()
                    )
                })?;
                let relative = format!("files/{index:08}.bin");
                write_file_atomic(&staging.join(&relative), bytes)?;
                (
                    Some(relative),
                    Some(bytes.len() as u64),
                    Some(sha256_hex(bytes)),
                )
            } else {
                (None, None, None)
            };
            persistent_snapshots.push(PersistentFileSnapshot {
                target_rel,
                existed: snapshot.existed,
                backup_rel,
                byte_len,
                sha256,
            });
        }

        Ok(ProviderSyncTransactionJournal {
            schema_version: PROVIDER_SYNC_TRANSACTION_SCHEMA_VERSION,
            operation_id: operation_id.to_string(),
            phase: ProviderSyncTransactionPhase::Prepared,
            target_provider: target_provider.to_string(),
            target_config_sha256: target_config_sha256.to_string(),
            route: route_context.map(Into::into),
            backup_dir_rel: path_relative_to_home(
                home,
                backup_dir,
                "Codex provider sync backup directory",
            )?,
            backup_root_existed,
            snapshots: persistent_snapshots,
        })
    })();

    let journal = match prepared {
        Ok(journal) => journal,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let mut bytes = serde_json::to_vec_pretty(&journal)
        .map_err(|error| format!("failed to serialize provider sync transaction: {error}"))?;
    bytes.push(b'\n');
    if let Err(error) = write_file_atomic(&staging.join(PROVIDER_SYNC_TRANSACTION_MANIFEST), &bytes)
    {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staging, &root) {
        let _ = fs::remove_dir_all(&staging);
        return Err(format!(
            "failed to publish provider sync transaction {}: {error}",
            root.display()
        )
        .into());
    }
    Ok(())
}

fn read_provider_sync_transaction(
    home: &Path,
) -> AppResult<Option<ProviderSyncTransactionJournal>> {
    let root = provider_sync_transaction_root(home);
    let Some(metadata) = non_symlink_metadata(&root, "Codex provider sync transaction root")?
    else {
        return Ok(None);
    };
    if !metadata.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync transaction root is not a directory path={}",
            root.display()
        )
        .into());
    }
    ensure_safe_operational_dir(&root, "Codex provider sync transaction root")?;
    let manifest_path = provider_sync_transaction_manifest_path(home);
    let bytes = read_optional_file_with_max_len(
        &manifest_path,
        PROVIDER_SYNC_TRANSACTION_MANIFEST_MAX_BYTES,
    )?
    .ok_or_else(|| {
        format!(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: transaction manifest is missing path={}",
            manifest_path.display()
        )
    })?;
    let mut journal: ProviderSyncTransactionJournal =
        serde_json::from_slice(&bytes).map_err(|error| {
            format!(
                "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: invalid transaction manifest {}: {error}",
                manifest_path.display()
            )
        })?;
    if journal.schema_version != PROVIDER_SYNC_TRANSACTION_SCHEMA_VERSION {
        return Err(format!(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: unsupported transaction schema version={}",
            journal.schema_version
        )
        .into());
    }
    if journal.phase != ProviderSyncTransactionPhase::Prepared {
        return Err(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: transaction manifest must start in prepared phase"
                .into(),
        );
    }
    if journal.operation_id.trim().is_empty()
        || journal.target_config_sha256.len() != 64
        || !journal
            .target_config_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: invalid transaction identity or target hash"
                .into(),
        );
    }

    let _ = validated_relative_path(
        &journal.backup_dir_rel,
        "Codex provider sync backup directory",
    )?;
    if journal.snapshots.len() > PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS {
        return Err(format!(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot count exceeds {}",
            PROVIDER_SYNC_TRANSACTION_MAX_SNAPSHOTS
        )
        .into());
    }
    let mut targets = HashSet::new();
    let mut backups = HashSet::new();
    let mut aggregate_bytes = 0u64;
    for snapshot in &journal.snapshots {
        let _ =
            validated_relative_path(&snapshot.target_rel, "Codex provider sync snapshot target")?;
        if !targets.insert(snapshot.target_rel.clone()) {
            return Err(format!(
                "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: duplicate snapshot target {}",
                snapshot.target_rel
            )
            .into());
        }
        match (
            snapshot.existed,
            snapshot.backup_rel.as_deref(),
            snapshot.byte_len,
            snapshot.sha256.as_deref(),
        ) {
            (true, Some(relative), Some(byte_len), Some(sha256)) => {
                let path = validated_relative_path(
                    relative,
                    "Codex provider sync snapshot backup",
                )?;
                if path.components().next()
                    != Some(Component::Normal(std::ffi::OsStr::new("files")))
                    || !backups.insert(path)
                {
                    return Err(format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot backup is outside files root or duplicated path={relative}"
                    )
                    .into());
                }
                if byte_len > PROVIDER_SYNC_SNAPSHOT_MAX_BYTES as u64 {
                    return Err(format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot exceeds {} bytes target={}",
                        PROVIDER_SYNC_SNAPSHOT_MAX_BYTES, snapshot.target_rel
                    )
                    .into());
                }
                aggregate_bytes = aggregate_bytes.saturating_add(byte_len);
                if aggregate_bytes > PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES as u64 {
                    return Err(format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot aggregate exceeds {} bytes",
                        PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES
                    )
                    .into());
                }
                if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot hash is invalid target={}",
                        snapshot.target_rel
                    )
                    .into());
                }
            }
            (false, None, None, None) => {}
            _ => {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot existence metadata is inconsistent target={}",
                    snapshot.target_rel
                )
                .into())
            }
        }
    }

    let applied_path = provider_sync_transaction_applied_path(home);
    if let Some(bytes) = read_optional_file_with_max_len(
        &applied_path,
        PROVIDER_SYNC_TRANSACTION_MANIFEST_MAX_BYTES,
    )? {
        let marker: ProviderSyncAppliedMarker =
            serde_json::from_slice(&bytes).map_err(|error| {
                format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: invalid applied marker {}: {error}",
                    applied_path.display()
                )
            })?;
        if marker.operation_id != journal.operation_id {
            return Err(
                "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: applied marker operation does not match journal"
                    .into(),
            );
        }
        journal.phase = ProviderSyncTransactionPhase::Applied;
    }
    Ok(Some(journal))
}

fn mark_provider_sync_transaction_applied(home: &Path, operation_id: &str) -> AppResult<()> {
    let journal = read_provider_sync_transaction(home)?.ok_or_else(|| {
        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: transaction disappeared before apply".to_string()
    })?;
    if journal.operation_id != operation_id {
        return Err(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: transaction operation changed before apply"
                .into(),
        );
    }
    let marker = ProviderSyncAppliedMarker {
        operation_id: operation_id.to_string(),
    };
    let mut bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|error| format!("failed to serialize provider sync applied marker: {error}"))?;
    bytes.push(b'\n');
    write_file_atomic(&provider_sync_transaction_applied_path(home), &bytes)
}

struct PreparedPersistentProviderSyncSnapshot {
    target: PathBuf,
    existed: bool,
    bytes: Option<Vec<u8>>,
}

fn preflight_persistent_provider_sync_snapshots(
    home: &Path,
    journal: &ProviderSyncTransactionJournal,
) -> AppResult<Vec<PreparedPersistentProviderSyncSnapshot>> {
    let transaction_root = provider_sync_transaction_root(home);
    let mut aggregate_bytes = 0usize;
    let mut prepared = Vec::with_capacity(journal.snapshots.len());
    for snapshot in &journal.snapshots {
        let target = home.join(validated_relative_path(
            &snapshot.target_rel,
            "Codex provider sync snapshot target",
        )?);
        if let Some(parent) = target.parent() {
            ensure_safe_operational_dir(parent, "Codex provider sync recovery target parent")?;
        }
        if let Some(metadata) =
            non_symlink_metadata(&target, "Codex provider sync recovery target")?
        {
            if !metadata.is_file() {
                return Err(format!(
                    "SEC_INVALID_INPUT: provider sync recovery target is not a file path={}",
                    target.display()
                )
                .into());
            }
        }
        let bytes = if snapshot.existed {
            let backup = transaction_root.join(validated_relative_path(
                snapshot.backup_rel.as_deref().ok_or_else(|| {
                    format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot backup missing target={}",
                        snapshot.target_rel
                    )
                })?,
                "Codex provider sync snapshot backup",
            )?);
            let Some(metadata) =
                non_symlink_metadata(&backup, "Codex provider sync persistent snapshot backup")?
            else {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot backup missing path={}",
                    backup.display()
                )
                .into());
            };
            if !metadata.is_file() {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot backup is not a file path={}",
                    backup.display()
                )
                .into());
            }
            let expected_len = snapshot.byte_len.ok_or_else(|| {
                format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot length missing target={}",
                    snapshot.target_rel
                )
            })?;
            if metadata.len() != expected_len {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot length mismatch path={} expected={} actual={}",
                    backup.display(),
                    expected_len,
                    metadata.len()
                )
                .into());
            }
            let bytes = read_file_with_max_len(&backup, PROVIDER_SYNC_SNAPSHOT_MAX_BYTES)?;
            if bytes.len() as u64 != expected_len {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot changed while reading path={}",
                    backup.display()
                )
                .into());
            }
            let expected_sha256 = snapshot.sha256.as_deref().ok_or_else(|| {
                format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot hash missing target={}",
                    snapshot.target_rel
                )
            })?;
            if sha256_hex(&bytes) != expected_sha256 {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot hash mismatch path={}",
                    backup.display()
                )
                .into());
            }
            aggregate_bytes = aggregate_bytes.saturating_add(bytes.len());
            if aggregate_bytes > PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES {
                return Err(format!(
                    "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: snapshot aggregate exceeds {} bytes",
                    PROVIDER_SYNC_SNAPSHOT_AGGREGATE_MAX_BYTES
                )
                .into());
            }
            Some(bytes)
        } else {
            None
        };
        prepared.push(PreparedPersistentProviderSyncSnapshot {
            target,
            existed: snapshot.existed,
            bytes,
        });
    }
    Ok(prepared)
}

fn restore_persistent_provider_sync_snapshots(
    home: &Path,
    journal: &ProviderSyncTransactionJournal,
) -> AppResult<()> {
    let prepared = preflight_persistent_provider_sync_snapshots(home, journal)?;
    for snapshot in prepared.into_iter().rev() {
        if snapshot.existed {
            write_file_atomic(
                &snapshot.target,
                snapshot.bytes.as_deref().ok_or_else(|| {
                    format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: preflight bytes missing path={}",
                        snapshot.target.display()
                    )
                })?,
            )?;
        } else {
            match fs::remove_file(&snapshot.target) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: failed to remove created target {}: {error}",
                        snapshot.target.display()
                    )
                    .into())
                }
            }
        }
    }
    Ok(())
}

fn applied_provider_sync_matches_current_state(
    home: &Path,
    journal: &ProviderSyncTransactionJournal,
    current_route: Option<&CodexProviderSyncCurrentRoute>,
) -> AppResult<bool> {
    if journal.phase != ProviderSyncTransactionPhase::Applied {
        return Ok(false);
    }
    if let Some(expected) = journal.route.as_ref() {
        let Some(current) = current_route else {
            return Ok(false);
        };
        if current.generation != expected.target_generation
            || current.mode != expected.target_mode
            || current.live_config_sha256 != expected.target_live_config_sha256
            || !current.live_matches_projection
            || !current.auth_matches_projection
            || current
                .pending_operation_id
                .as_ref()
                .is_some_and(|operation_id| operation_id != &expected.operation_id)
        {
            return Ok(false);
        }
    }
    let config_path = home.join("config.toml");
    let config = read_optional_file_with_max_len(&config_path, PROVIDER_SYNC_MAX_BYTES)?;
    Ok(sha256_hex(config.as_deref().unwrap_or_default()) == journal.target_config_sha256)
}

pub(crate) fn recover_interrupted_provider_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    current_route: Option<&CodexProviderSyncCurrentRoute>,
) -> AppResult<CodexProviderSyncRecoveryOutcome> {
    let home = crate::codex_paths::codex_home_dir(app)?;
    recover_interrupted_provider_sync_from_home(&home, current_route, None)
}

pub(crate) fn has_pending_provider_sync_recovery<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<bool> {
    let home = crate::codex_paths::codex_home_dir(app)?;
    Ok(read_provider_sync_transaction(&home)?.is_some())
}

fn recover_interrupted_provider_sync_from_home(
    home: &Path,
    current_route: Option<&CodexProviderSyncCurrentRoute>,
    codex_running_override: Option<bool>,
) -> AppResult<CodexProviderSyncRecoveryOutcome> {
    let journal = match read_provider_sync_transaction(home) {
        Ok(journal) => journal,
        Err(error) => {
            quarantine_corrupt_provider_sync_transaction(home, &error.to_string())?;
            cleanup_provider_sync_transaction_staging(home)?;
            let _ = remove_stale_provider_sync_lock(home)?;
            return Ok(CodexProviderSyncRecoveryOutcome::Quarantined);
        }
    };
    let Some(journal) = journal else {
        cleanup_provider_sync_transaction_staging(home)?;
        return if remove_stale_provider_sync_lock(home)? {
            Ok(CodexProviderSyncRecoveryOutcome::StaleLockRemoved)
        } else {
            Ok(CodexProviderSyncRecoveryOutcome::None)
        };
    };

    let outcome = if applied_provider_sync_matches_current_state(home, &journal, current_route)? {
        if let Err(error) = prune_managed_backups(home) {
            tracing::warn!(error = %error, "provider sync backup prune failed during recovery");
        }
        CodexProviderSyncRecoveryOutcome::Finalized
    } else {
        let codex_running = match codex_running_override {
            Some(running) => running,
            None => codex_app_is_running()?,
        };
        reject_running_codex_for_provider_sync(codex_running)?;
        if let Err(error) = restore_persistent_provider_sync_snapshots(home, &journal) {
            quarantine_corrupt_provider_sync_transaction(home, &error.to_string())?;
            cleanup_provider_sync_transaction_staging(home)?;
            let _ = remove_stale_provider_sync_lock(home)?;
            return Ok(CodexProviderSyncRecoveryOutcome::Quarantined);
        }
        let backup_dir = home.join(validated_relative_path(
            &journal.backup_dir_rel,
            "Codex provider sync backup directory",
        )?);
        remove_created_provider_sync_backup(home, &backup_dir, !journal.backup_root_existed)?;
        CodexProviderSyncRecoveryOutcome::Restored
    };

    clear_provider_sync_transaction(home)?;
    cleanup_provider_sync_transaction_staging(home)?;
    let _ = remove_stale_provider_sync_lock(home)?;
    Ok(outcome)
}

fn quarantine_corrupt_provider_sync_transaction(home: &Path, reason: &str) -> AppResult<PathBuf> {
    let transaction_root = provider_sync_transaction_root(home);
    let metadata = fs::symlink_metadata(&transaction_root).map_err(|error| {
        format!(
            "CODEX_PROVIDER_SYNC_RECOVERY_FAILED: failed to inspect corrupt transaction {}: {error}",
            transaction_root.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync transaction root is a link path={}",
            transaction_root.display()
        )
        .into());
    }

    let quarantine_root = home.join(PROVIDER_SYNC_TRANSACTION_QUARANTINE_ROOT);
    ensure_safe_operational_dir(
        &quarantine_root,
        "Codex provider sync transaction quarantine root",
    )?;
    fs::create_dir_all(&quarantine_root).map_err(|error| {
        format!(
            "failed to create provider sync transaction quarantine root {}: {error}",
            quarantine_root.display()
        )
    })?;
    let quarantine_dir = quarantine_root.join(format!(
        "{}-{}",
        crate::infra::codex_retry_gateway::now_unix_ms(),
        crate::infra::codex_retry_gateway::random_hex(8)
    ));
    fs::create_dir(&quarantine_dir).map_err(|error| {
        format!(
            "failed to create provider sync transaction quarantine {}: {error}",
            quarantine_dir.display()
        )
    })?;

    let reason = reason
        .chars()
        .take(PROVIDER_SYNC_TRANSACTION_QUARANTINE_REASON_MAX_CHARS)
        .collect::<String>();
    let mut quarantine_metadata = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": 1,
        "reason": reason,
    }))
    .map_err(|error| format!("failed to serialize provider sync quarantine metadata: {error}"))?;
    quarantine_metadata.push(b'\n');
    write_file_atomic(
        &quarantine_dir.join("quarantine.json"),
        &quarantine_metadata,
    )?;
    let retained_transaction = quarantine_dir.join("transaction");
    fs::rename(&transaction_root, &retained_transaction).map_err(|error| {
        format!(
            "failed to quarantine provider sync transaction {}: {error}",
            transaction_root.display()
        )
    })?;
    tracing::warn!(
        quarantine = %quarantine_dir.display(),
        "quarantined corrupt provider sync transaction and retained it for diagnosis"
    );
    Ok(quarantine_dir)
}

fn clear_provider_sync_transaction(home: &Path) -> AppResult<()> {
    let root = provider_sync_transaction_root(home);
    let Some(metadata) = non_symlink_metadata(&root, "Codex provider sync transaction root")?
    else {
        return Ok(());
    };
    if !metadata.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync transaction root is not a directory path={}",
            root.display()
        )
        .into());
    }
    fs::remove_dir_all(&root).map_err(|error| {
        format!(
            "failed to clear provider sync transaction {}: {error}",
            root.display()
        )
        .into()
    })
}

fn cleanup_provider_sync_transaction_staging(home: &Path) -> AppResult<()> {
    let tmp_root = home.join("tmp");
    let Some(metadata) =
        non_symlink_metadata(&tmp_root, "Codex provider sync transaction staging parent")?
    else {
        return Ok(());
    };
    if !metadata.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync transaction staging parent is not a directory path={}",
            tmp_root.display()
        )
        .into());
    }
    for entry in fs::read_dir(&tmp_root)
        .map_err(|error| format!("failed to read {}: {error}", tmp_root.display()))?
    {
        let path = entry
            .map_err(|error| format!("failed to read {}: {error}", tmp_root.display()))?
            .path();
        let matches_prefix = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(PROVIDER_SYNC_TRANSACTION_STAGING_PREFIX));
        if !matches_prefix {
            continue;
        }
        let Some(metadata) =
            non_symlink_metadata(&path, "Codex provider sync transaction staging directory")?
        else {
            continue;
        };
        if !metadata.is_dir() {
            return Err(format!(
                "SEC_INVALID_INPUT: provider sync transaction staging path is not a directory path={}",
                path.display()
            )
            .into());
        }
        fs::remove_dir_all(&path).map_err(|error| {
            format!(
                "failed to remove provider sync transaction staging {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn remove_stale_provider_sync_lock(home: &Path) -> AppResult<bool> {
    let lock_path = home.join(PROVIDER_SYNC_LOCK_FILE);
    let Some(metadata) = non_symlink_metadata(&lock_path, "Codex provider sync lock")? else {
        return Ok(false);
    };
    if !metadata.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: provider sync lock is not a directory path={}",
            lock_path.display()
        )
        .into());
    }
    fs::remove_dir_all(&lock_path).map_err(|error| {
        format!(
            "failed to remove stale lock {}: {error}",
            lock_path.display()
        )
    })?;
    Ok(true)
}

fn remove_created_provider_sync_backup(
    home: &Path,
    backup_dir: &Path,
    remove_empty_root: bool,
) -> AppResult<()> {
    if !backup_dir.exists() {
        return Ok(());
    }
    let root = home.join(PROVIDER_SYNC_BACKUP_ROOT);
    if backup_dir.parent() != Some(root.as_path())
        || managed_backup_created_at(backup_dir)?.is_none()
    {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to remove unverified provider sync backup {}",
            backup_dir.display()
        )
        .into());
    }
    fs::remove_dir_all(backup_dir).map_err(|error| {
        format!(
            "failed to remove rolled-back provider sync backup {}: {error}",
            backup_dir.display()
        )
    })?;
    if remove_empty_root
        && fs::read_dir(&root)
            .map_err(|error| format!("failed to inspect backup root {}: {error}", root.display()))?
            .next()
            .is_none()
    {
        fs::remove_dir(&root).map_err(|error| {
            format!(
                "failed to remove rolled-back provider sync backup root {}: {error}",
                root.display()
            )
        })?;
    }
    Ok(())
}

fn prune_managed_backups(home: &Path) -> AppResult<Option<String>> {
    let root = home.join(PROVIDER_SYNC_BACKUP_ROOT);
    if !root.exists() {
        return Ok(None);
    }
    let mut managed: Vec<(i128, String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|e| format!("failed to read backup root {}: {e}", root.display()))?
    {
        let path = entry
            .map_err(|e| format!("failed to read backup entry {}: {e}", root.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let Some(created_at) = managed_backup_created_at(&path)? else {
            continue;
        };
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        managed.push((created_at, file_name, path));
    }
    managed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    for (_, _, path) in managed.into_iter().skip(PROVIDER_SYNC_KEEP_COUNT) {
        if let Err(err) = fs::remove_dir_all(&path) {
            return Ok(Some(format!(
                "provider sync backup prune failed for {}: {err}",
                path.display()
            )));
        }
    }
    Ok(None)
}

fn managed_backup_created_at(path: &Path) -> AppResult<Option<i128>> {
    let manifest_path = path.join(PROVIDER_SYNC_MANAGED_BACKUP_MANIFEST);
    let Ok(bytes) = fs::read(&manifest_path) else {
        return Ok(None);
    };
    let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    if manifest.get("managed_by").and_then(Value::as_str) != Some("Codex provider sync") {
        return Ok(None);
    }
    let Some(created_at) = manifest.get("created_at").and_then(Value::as_str) else {
        return Ok(None);
    };
    Ok(created_at.parse::<i128>().ok())
}

#[cfg(test)]
mod tests;
