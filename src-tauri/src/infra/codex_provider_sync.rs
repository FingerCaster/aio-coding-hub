//! Usage: Strict Codex provider sync / backup / rollback core.

use crate::shared::error::AppResult;
use crate::shared::fs::{
    is_symlink, read_optional_file_with_max_len, write_file_atomic_if_changed,
};
use crate::shared::time::{now_unix_millis, now_unix_seconds};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

pub const PROVIDER_SYNC_LOCK_FILE: &str = "tmp/provider-sync.lock";
pub const PROVIDER_SYNC_BACKUP_ROOT: &str = "backups_state/provider-sync";
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

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    existed: bool,
    bytes: Option<Vec<u8>>,
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
    let home = crate::codex_paths::codex_home_dir(app)?;
    let target_provider = resolve_target_provider(&context.target_provider)?;
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
    let current_config_text = optional_config_bytes_to_utf8(current_config)?;
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
        return Ok(CodexProviderSyncResult {
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
        });
    }

    let backup_dir = create_backup(&home, &context, &change_set)?;
    let mut snapshots = snapshot_paths(&home, &config_path, &change_set)?;
    let mut writes_started = false;
    let result = (|| -> AppResult<CodexProviderSyncResult> {
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

        let warning = prune_managed_backups(&home)
            .ok()
            .and_then(|warning| warning);
        Ok(CodexProviderSyncResult {
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
            warning: warning.or(change_set.warning),
        })
    })();

    match result {
        Ok(out) => Ok(out),
        Err(err) => {
            if writes_started {
                if let Err(rollback_err) = restore_snapshots(&mut snapshots) {
                    return Err(format!(
                        "CODEX_PROVIDER_SYNC_ROLLBACK_FAILED: failed to restore snapshots after {err}; rollback error: {rollback_err}"
                    )
                    .into());
                }
            }
            Err(err)
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

pub fn codex_provider_target_from_config_text(config_text: &str) -> AppResult<String> {
    let current_provider = read_current_provider(config_text)?.ok_or_else(|| {
        "CODEX_PROVIDER_SYNC_INVALID_TARGET: unsupported provider target=(missing)".to_string()
    })?;
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
        let output = std::process::Command::new("cmd")
            .args(["/C", "tasklist /FI \"IMAGENAME eq Codex.exe\" /NH"])
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
    Ok(text.lines().any(|line| process_name_is_codex_app(line)))
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
        Some(None) => {
            if change.bak_path.exists() {
                fs::remove_file(&change.bak_path).map_err(|e| {
                    format!("failed to remove bak {}: {e}", change.bak_path.display())
                })?;
            }
        }
        None => {}
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
    snapshots.push(snapshot_path(config_path)?);
    snapshots.push(snapshot_path(&home.join("config.toml.bak"))?);
    for change in &change_set.session_changes {
        snapshots.push(snapshot_path(&change.path)?);
    }
    for change in &change_set.sqlite_changes {
        for path in codex_sqlite_sidecar_paths(&change.path) {
            snapshots.push(snapshot_path(&path)?);
        }
    }
    if let Some(change) = change_set.global_state_change.as_ref() {
        snapshots.push(snapshot_path(&change.path)?);
        snapshots.push(snapshot_path(&change.bak_path)?);
    }
    Ok(snapshots)
}

fn snapshot_path(path: &Path) -> AppResult<FileSnapshot> {
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
    let bytes =
        fs::read(path).map_err(|e| format!("failed to snapshot {}: {e}", path.display()))?;
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
