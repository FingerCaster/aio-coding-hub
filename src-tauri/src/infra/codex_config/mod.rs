//! Usage: Read / patch Codex user-level `config.toml` ($CODEX_HOME/config.toml).

mod parsing;
mod patching;
pub(crate) mod provider_projection;
mod types;

pub use types::{
    CodexConfigPatch, CodexConfigState, CodexConfigTomlState, CodexConfigTomlValidationError,
    CodexConfigTomlValidationResult,
};

use crate::codex_paths;
use crate::shared::fs::{
    is_symlink, read_optional_file_with_max_len, write_file_atomic_if_changed,
};
use parsing::{make_state_from_bytes, validate_codex_config_toml_raw};
use patching::patch_config_toml;
use std::fs;
use std::path::{Path, PathBuf};
use types::CodexConfigStateMeta;

const CODEX_CONFIG_MAX_BYTES: usize = 1024 * 1024;

fn ensure_codex_config_len(bytes: &[u8], label: &str) -> crate::shared::error::AppResult<()> {
    if bytes.len() > CODEX_CONFIG_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} too large (max {CODEX_CONFIG_MAX_BYTES} bytes)"
        )
        .into());
    }
    Ok(())
}

fn read_optional_codex_config_file(
    path: &Path,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    read_optional_file_with_max_len(path, CODEX_CONFIG_MAX_BYTES)
}

#[derive(Debug)]
pub(crate) struct CodexCliProxyBackupSnapshot {
    manifest_path: PathBuf,
    manifest_existed: bool,
    manifest_bytes: Option<Vec<u8>>,
    backup_path: PathBuf,
    backup_existed: bool,
    backup_bytes: Option<Vec<u8>>,
    committed_manifest: (bool, Option<Vec<u8>>),
    committed_backup: (bool, Option<Vec<u8>>),
}

#[derive(Debug, Default)]
struct CodexCliProxyBackupRollback {
    manifest_restored: bool,
    backup_restored: bool,
    errors: Vec<String>,
}

impl CodexCliProxyBackupRollback {
    fn complete() -> Self {
        Self {
            manifest_restored: true,
            backup_restored: true,
            errors: Vec::new(),
        }
    }

    fn is_complete(&self) -> bool {
        self.manifest_restored && self.backup_restored && self.errors.is_empty()
    }

    fn append_failures(&self, failures: &mut Vec<String>) {
        if !self.backup_restored {
            failures.push("proxy backup changed after this save; rollback was skipped".to_string());
        }
        if !self.manifest_restored {
            failures
                .push("proxy manifest changed after this save; rollback was skipped".to_string());
        }
        failures.extend(self.errors.iter().cloned());
    }
}

pub(crate) fn sync_codex_cli_proxy_backup_if_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    next_bytes: &[u8],
) -> crate::shared::error::AppResult<Option<CodexCliProxyBackupSnapshot>> {
    ensure_codex_config_len(next_bytes, "codex config backup")?;
    let manifest_path = crate::app_paths::app_data_dir(app)?
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let manifest_snapshot = snapshot_optional_file(&manifest_path)?;
    let Some(backup_path) = super::cli_proxy::backup_file_path_for_enabled_manifest(
        app,
        "codex",
        "codex_config_toml",
        "config.toml",
    )?
    else {
        return Ok(None);
    };

    let committed_manifest = snapshot_optional_file(&manifest_path).map_err(|error| {
        crate::shared::error::AppError::new(
            "CODEX_CONFIG_BACKUP_RECOVERY_REQUIRED",
            format!(
                "the proxy manifest may have changed but its committed state could not be captured: {error}"
            ),
        )
    })?;

    let backup_snapshot = match snapshot_optional_file(&backup_path) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let manifest_restored = restore_optional_file_if_current(
                &manifest_path,
                &manifest_snapshot,
                &committed_manifest,
            );
            return match manifest_restored {
                Ok(true) => Err(format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}").into()),
                Ok(false) => Err(crate::shared::error::AppError::new(
                    "CODEX_CONFIG_BACKUP_RECOVERY_REQUIRED",
                    format!(
                        "proxy backup inspection failed ({err}); the proxy manifest changed concurrently"
                    ),
                )),
                Err(rollback_error) => Err(crate::shared::error::AppError::new(
                    "CODEX_CONFIG_BACKUP_RECOVERY_REQUIRED",
                    format!(
                        "proxy backup inspection failed ({err}); manifest rollback failed: {rollback_error}"
                    ),
                )),
            };
        }
    };
    let snapshot = CodexCliProxyBackupSnapshot {
        manifest_path,
        manifest_existed: manifest_snapshot.0,
        manifest_bytes: manifest_snapshot.1,
        backup_path,
        backup_existed: backup_snapshot.0,
        backup_bytes: backup_snapshot.1,
        committed_manifest,
        committed_backup: (true, Some(next_bytes.to_vec())),
    };

    if let Err(err) = write_file_atomic_if_changed(&snapshot.backup_path, next_bytes)
        .map_err(|err| format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}"))
    {
        let rollback = restore_codex_cli_proxy_backup_snapshot_if_current(&snapshot);
        return if rollback.is_complete() {
            Err(err.into())
        } else {
            let mut failures = Vec::new();
            rollback.append_failures(&mut failures);
            Err(crate::shared::error::AppError::new(
                "CODEX_CONFIG_BACKUP_RECOVERY_REQUIRED",
                format!("{err}; {}", failures.join("; ")),
            ))
        };
    }

    Ok(Some(snapshot))
}

fn snapshot_optional_file(path: &Path) -> crate::shared::error::AppResult<(bool, Option<Vec<u8>>)> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(format!(
                    "SEC_INVALID_INPUT: backup target is not a file path={}",
                    path.display()
                )
                .into());
            }
            let bytes = fs::read(path).map_err(|err| {
                format!("failed to snapshot backup target {}: {err}", path.display())
            })?;
            Ok((true, Some(bytes)))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok((false, None)),
        Err(err) => Err(format!("failed to read backup target {}: {err}", path.display()).into()),
    }
}

fn restore_optional_file(
    path: &Path,
    snapshot: &(bool, Option<Vec<u8>>),
) -> crate::shared::error::AppResult<()> {
    match snapshot {
        (true, Some(bytes)) => {
            let _ = write_file_atomic_if_changed(path, bytes)?;
        }
        (false, _) => remove_path_if_exists(path)?,
        (true, None) => {}
    }
    Ok(())
}

fn restore_optional_file_if_current(
    path: &Path,
    before: &(bool, Option<Vec<u8>>),
    committed: &(bool, Option<Vec<u8>>),
) -> crate::shared::error::AppResult<bool> {
    let current = snapshot_optional_file(path)?;
    if current == *before {
        return Ok(true);
    }
    if current != *committed {
        return Ok(false);
    }
    restore_optional_file(path, before)?;
    Ok(true)
}

fn restore_codex_config_if_current(
    path: &Path,
    before: Option<&[u8]>,
    committed: &[u8],
) -> crate::shared::error::AppResult<bool> {
    let current = read_optional_codex_config_file(path)?;
    if current.as_deref() == before {
        return Ok(true);
    }
    if current.as_deref() != Some(committed) {
        return Ok(false);
    }

    match before {
        Some(bytes) => {
            let _ = write_file_atomic_if_changed(path, bytes)?;
        }
        None => remove_path_if_exists(path)?,
    }
    Ok(true)
}

fn restore_codex_cli_proxy_backup_snapshot_if_current(
    snapshot: &CodexCliProxyBackupSnapshot,
) -> CodexCliProxyBackupRollback {
    let mut rollback = CodexCliProxyBackupRollback::default();
    match restore_optional_file_if_current(
        &snapshot.backup_path,
        &(snapshot.backup_existed, snapshot.backup_bytes.clone()),
        &snapshot.committed_backup,
    ) {
        Ok(restored) => rollback.backup_restored = restored,
        Err(error) => rollback
            .errors
            .push(format!("proxy backup rollback failed: {error}")),
    }
    match restore_optional_file_if_current(
        &snapshot.manifest_path,
        &(snapshot.manifest_existed, snapshot.manifest_bytes.clone()),
        &snapshot.committed_manifest,
    ) {
        Ok(restored) => rollback.manifest_restored = restored,
        Err(error) => rollback
            .errors
            .push(format!("proxy manifest rollback failed: {error}")),
    }
    rollback
}

fn rollback_codex_config_and_proxy_backup_if_current(
    path: &Path,
    config_before: Option<&[u8]>,
    config_committed: &[u8],
    backup_snapshot: Option<&CodexCliProxyBackupSnapshot>,
) -> Vec<String> {
    let mut failures = Vec::new();

    // These files have separate committed tokens. Always attempt both restores
    // so drift or an I/O failure on one side cannot strand the other side.
    match restore_codex_config_if_current(path, config_before, config_committed) {
        Ok(true) => {}
        Ok(false) => {
            failures.push("config changed after this save; rollback was skipped".to_string())
        }
        Err(error) => failures.push(format!("config rollback failed: {error}")),
    }

    let backup_rollback = backup_snapshot
        .map(restore_codex_cli_proxy_backup_snapshot_if_current)
        .unwrap_or_else(CodexCliProxyBackupRollback::complete);
    backup_rollback.append_failures(&mut failures);
    failures
}

fn remove_path_if_exists(path: &Path) -> crate::shared::error::AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Err(format!(
            "SEC_INVALID_INPUT: refusing to remove unexpected backup directory path={}",
            path.display()
        )
        .into()),
        Ok(_) => fs::remove_file(path)
            .map_err(|err| format!("failed to remove file {}: {err}", path.display()).into()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("failed to inspect path {}: {err}", path.display()).into()),
    }
}

fn apply_managed_catalog_for_config_save<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &Path,
    previous: Option<&[u8]>,
    proposed: &[u8],
    backup_snapshot: Option<&CodexCliProxyBackupSnapshot>,
) -> crate::shared::error::AppResult<crate::codex_model_catalog::managed::AppliedManagedCatalog> {
    let sync_error =
        match crate::codex_model_catalog::managed::sync_current_after_config_save_locked(
            app, previous, proposed,
        ) {
            Ok(applied) => return Ok(applied),
            Err(error) => error,
        };

    let mut recovery_errors = rollback_codex_config_and_proxy_backup_if_current(
        path,
        previous,
        proposed,
        backup_snapshot,
    );

    if recovery_errors.is_empty() && !managed_catalog_sync_requires_recovery(&sync_error) {
        return Err(crate::shared::error::AppError::new(
            "CODEX_CONFIG_MANAGED_CATALOG_SYNC_FAILED",
            format!(
                "Codex config remained unchanged after catalog preparation failed: {sync_error}"
            ),
        ));
    }

    if managed_catalog_sync_requires_recovery(&sync_error) {
        recovery_errors.push(format!("managed catalog rollback failed: {sync_error}"));
    }
    Err(crate::shared::error::AppError::new(
        "CODEX_CONFIG_MANAGED_CATALOG_RECOVERY_REQUIRED",
        format!(
            "catalog reconciliation failed ({sync_error}); {}",
            recovery_errors.join("; ")
        ),
    ))
}

fn managed_catalog_sync_requires_recovery(error: &crate::shared::error::AppError) -> bool {
    error.code() == "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED"
}

fn rollback_config_after_post_catalog_failure(
    path: &Path,
    previous: Option<&[u8]>,
    committed: &[u8],
    backup_snapshot: Option<&CodexCliProxyBackupSnapshot>,
    catalog_commit: crate::codex_model_catalog::managed::AppliedManagedCatalog,
    original: crate::shared::error::AppError,
) -> crate::shared::error::AppError {
    let mut recovery_errors = Vec::new();
    if original.code() == "CODEX_PROVIDER_SYNC_ROLLBACK_FAILED" {
        recovery_errors.push(format!("history rollback failed: {original}"));
    }
    if let Err(error) = catalog_commit.rollback() {
        recovery_errors.push(format!("catalog rollback failed: {error}"));
    }
    recovery_errors.extend(rollback_codex_config_and_proxy_backup_if_current(
        path,
        previous,
        committed,
        backup_snapshot,
    ));

    if recovery_errors.is_empty() {
        original
    } else {
        crate::shared::error::AppError::new(
            "CODEX_CONFIG_MANAGED_CATALOG_RECOVERY_REQUIRED",
            format!(
                "post-catalog Codex config operation failed ({original}); {}",
                recovery_errors.join("; ")
            ),
        )
    }
}

#[cfg(not(test))]
fn injected_post_catalog_failure() -> crate::shared::error::AppResult<()> {
    Ok(())
}

#[cfg(test)]
thread_local! {
    static FAIL_NEXT_POST_CATALOG_CONFIRMATION: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
fn injected_post_catalog_failure() -> crate::shared::error::AppResult<()> {
    let should_fail = FAIL_NEXT_POST_CATALOG_CONFIRMATION.with(|failure| failure.replace(false));
    if should_fail {
        Err(crate::shared::error::AppError::new(
            "CODEX_CONFIG_TEST_POST_CATALOG_FAILURE",
            "injected post-catalog Codex config failure",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn fail_next_post_catalog_confirmation_for_test() {
    FAIL_NEXT_POST_CATALOG_CONFIRMATION.with(|failure| failure.set(true));
}

pub(crate) fn codex_config_next_bytes(
    current: Option<Vec<u8>>,
    patch: CodexConfigPatch,
) -> crate::shared::error::AppResult<Vec<u8>> {
    patch_config_toml(current, patch)
}

pub(crate) fn codex_config_normalize_raw_toml(
    mut toml: String,
) -> crate::shared::error::AppResult<Vec<u8>> {
    ensure_codex_config_len(toml.as_bytes(), "codex config.toml")?;
    let validation = validate_codex_config_toml_raw(&toml);
    if !validation.ok {
        let err = validation.error.unwrap_or(CodexConfigTomlValidationError {
            message: "invalid TOML".to_string(),
            line: None,
            column: None,
        });

        let mut msg = format!("SEC_INVALID_INPUT: invalid config.toml: {}", err.message);
        match (err.line, err.column) {
            (Some(line), Some(column)) => msg.push_str(&format!(" (line {line}, column {column})")),
            (Some(line), None) => msg.push_str(&format!(" (line {line})")),
            _ => {}
        }
        return Err(msg.into());
    }

    if !toml.ends_with('\n') {
        toml.push('\n');
    }
    ensure_codex_config_len(toml.as_bytes(), "codex config.toml")?;
    Ok(toml.into_bytes())
}

pub(crate) fn codex_config_patch_target_provider(
    toml: &str,
) -> crate::shared::error::AppResult<String> {
    crate::infra::codex_provider_sync::codex_provider_target_from_patch_config_text(toml)
}

fn patch_requires_provider_sync(patch: &CodexConfigPatch) -> bool {
    patch.features_remote_compaction.is_some()
}

#[cfg(windows)]
fn normalize_path_for_prefix_match(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

#[cfg(windows)]
fn path_is_under_allowed_root(dir: &Path, allowed_root: &Path) -> bool {
    let dir_s = normalize_path_for_prefix_match(dir);
    let root_s = normalize_path_for_prefix_match(allowed_root);
    dir_s == root_s || dir_s.starts_with(&(root_s + "/"))
}

#[cfg(not(windows))]
fn path_is_under_allowed_root(dir: &Path, allowed_root: &Path) -> bool {
    dir.starts_with(allowed_root)
}

pub fn codex_config_get<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<CodexConfigState> {
    let path = codex_paths::codex_config_toml_path(app)?;
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    let user_default_path = codex_paths::codex_home_dir_user_default(app)?.join("config.toml");
    let user_default_dir = user_default_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let follow_path = codex_paths::codex_home_dir_follow_env_or_default(app)?.join("config.toml");
    let follow_dir = follow_path.parent().unwrap_or(Path::new("")).to_path_buf();
    let bytes = read_optional_codex_config_file(&path)?;

    let can_open_config_dir = crate::app_paths::home_dir(app)
        .ok()
        .map(|home| {
            let allowed_root = home.join(".codex");
            path_is_under_allowed_root(&dir, &allowed_root)
                || follow_dir == dir
                || codex_paths::configured_codex_home_dir(app)
                    .as_ref()
                    .is_some_and(|configured_dir| configured_dir == &dir)
        })
        .unwrap_or(false);

    make_state_from_bytes(
        CodexConfigStateMeta {
            config_dir: dir.to_string_lossy().to_string(),
            config_path: path.to_string_lossy().to_string(),
            user_home_default_dir: user_default_dir.to_string_lossy().to_string(),
            user_home_default_path: user_default_path.to_string_lossy().to_string(),
            follow_codex_home_dir: follow_dir.to_string_lossy().to_string(),
            follow_codex_home_path: follow_path.to_string_lossy().to_string(),
            can_open_config_dir,
        },
        bytes,
    )
}

pub fn codex_config_toml_get_raw<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<CodexConfigTomlState> {
    let path = codex_paths::codex_config_toml_path(app)?;
    let bytes = read_optional_codex_config_file(&path)?;
    let exists = bytes.is_some();

    let toml = match bytes {
        Some(bytes) => String::from_utf8(bytes)
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?,
        None => String::new(),
    };

    Ok(CodexConfigTomlState {
        config_path: path.to_string_lossy().to_string(),
        exists,
        toml,
    })
}

pub fn codex_config_toml_validate_raw(
    toml: String,
) -> crate::shared::error::AppResult<CodexConfigTomlValidationResult> {
    Ok(validate_codex_config_toml_raw(&toml))
}

pub fn codex_config_toml_set_raw<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    toml: String,
) -> crate::shared::error::AppResult<CodexConfigState> {
    let _lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
    let path = codex_paths::codex_config_toml_path(app)?;
    if path.exists() && is_symlink(&path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            path.display()
        )
        .into());
    }

    let bytes = codex_config_normalize_raw_toml(toml)?;
    let current = read_optional_codex_config_file(&path)?;
    let baseline = super::cli_proxy::codex_enabled_proxy_baseline(app)?;
    let (backup_bytes, next) = match baseline.as_ref() {
        Some(baseline) => {
            let baseline_before = baseline.config_bytes.as_deref().unwrap_or_default();
            let previous_base_url = format!("{}/v1", baseline.base_origin.trim_end_matches('/'));
            let expected = super::cli_proxy::project_codex_config_from_baseline(
                app,
                baseline.config_bytes.clone(),
                current.as_deref(),
                &baseline.base_origin,
                Some(&previous_base_url),
            )?;
            let merged = provider_projection::merge_raw_user_changes(
                baseline_before,
                &expected,
                current.as_deref().unwrap_or_default(),
                &bytes,
            )?;
            let previous_provider =
                provider_projection::desired_provider_key_from_config(baseline_before)?;
            let target_provider = provider_projection::desired_provider_key_from_config(&merged)?;
            let requires_provider_sync = previous_provider != target_provider;
            let backup_bytes = if requires_provider_sync {
                provider_projection::reconcile_provider_identity(&merged, target_provider, None)?
            } else {
                merged
            };
            let next = super::cli_proxy::project_codex_config_from_baseline(
                app,
                Some(backup_bytes.clone()),
                current.as_deref(),
                &baseline.base_origin,
                Some(&previous_base_url),
            )?;
            (backup_bytes, next)
        }
        None => {
            let previous_provider = provider_projection::desired_provider_key_from_config(
                current.as_deref().unwrap_or_default(),
            )?;
            let target_provider = provider_projection::desired_provider_key_from_config(&bytes)?;
            let requires_provider_sync = previous_provider != target_provider;
            let next = if requires_provider_sync {
                provider_projection::reconcile_provider_identity(&bytes, target_provider, None)?
            } else {
                bytes
            };
            (next.clone(), next)
        }
    };
    ensure_codex_config_len(&backup_bytes, "codex config backup")?;
    ensure_codex_config_len(&next, "codex config.toml")?;
    let backup_snapshot = sync_codex_cli_proxy_backup_if_enabled(app, &backup_bytes)?;
    let catalog_commit = apply_managed_catalog_for_config_save(
        app,
        &path,
        current.as_deref(),
        &next,
        backup_snapshot.as_ref(),
    )?;
    match injected_post_catalog_failure().and_then(|()| codex_config_get(app)) {
        Ok(state) => Ok(state),
        Err(error) => Err(rollback_config_after_post_catalog_failure(
            &path,
            current.as_deref(),
            &next,
            backup_snapshot.as_ref(),
            catalog_commit,
            error,
        )),
    }
}

pub fn codex_config_set_with_options<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    patch: CodexConfigPatch,
    sync_history: bool,
) -> crate::shared::error::AppResult<CodexConfigState> {
    let _lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
    let path = codex_paths::codex_config_toml_path(app)?;
    if path.exists() && is_symlink(&path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            path.display()
        )
        .into());
    }

    let current = read_optional_codex_config_file(&path)?;
    let baseline = super::cli_proxy::codex_enabled_proxy_baseline(app)?;
    let requires_provider_sync = patch_requires_provider_sync(&patch);
    let history_source_provider = if requires_provider_sync && sync_history {
        let source = baseline
            .as_ref()
            .and_then(|baseline| baseline.config_bytes.as_deref())
            .or(current.as_deref())
            .unwrap_or_default();
        let source = std::str::from_utf8(source)
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
        Some(codex_config_patch_target_provider(source)?)
    } else {
        None
    };
    let backup_bytes = match baseline.as_ref() {
        Some(baseline) => {
            let baseline_before = baseline.config_bytes.as_deref().unwrap_or_default();
            let previous_base_url = format!("{}/v1", baseline.base_origin.trim_end_matches('/'));
            let expected = super::cli_proxy::project_codex_config_from_baseline(
                app,
                baseline.config_bytes.clone(),
                current.as_deref(),
                &baseline.base_origin,
                Some(&previous_base_url),
            )?;
            let baseline_with_external_changes = provider_projection::merge_raw_user_changes(
                baseline_before,
                &expected,
                current.as_deref().unwrap_or_default(),
                current.as_deref().unwrap_or_default(),
            )?;
            codex_config_next_bytes(Some(baseline_with_external_changes), patch.clone())?
        }
        None => codex_config_next_bytes(current.clone(), patch.clone())?,
    };
    let next = match baseline.as_ref() {
        Some(baseline) => {
            let previous_base_url = format!("{}/v1", baseline.base_origin.trim_end_matches('/'));
            super::cli_proxy::project_codex_config_from_baseline(
                app,
                Some(backup_bytes.clone()),
                current.as_deref(),
                &baseline.base_origin,
                Some(&previous_base_url),
            )?
        }
        None => backup_bytes.clone(),
    };
    ensure_codex_config_len(&backup_bytes, "codex config backup")?;
    ensure_codex_config_len(&next, "codex config.toml")?;
    let target_provider = if requires_provider_sync {
        let next_text = std::str::from_utf8(&next)
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
        Some(codex_config_patch_target_provider(next_text)?)
    } else {
        None
    };
    if requires_provider_sync && sync_history {
        crate::infra::codex_provider_sync::codex_provider_sync_history_preflight()?;
    }
    let backup_snapshot = sync_codex_cli_proxy_backup_if_enabled(app, &backup_bytes)?;

    let catalog_commit = apply_managed_catalog_for_config_save(
        app,
        &path,
        current.as_deref(),
        &next,
        backup_snapshot.as_ref(),
    )?;

    let confirmed = match injected_post_catalog_failure().and_then(|()| codex_config_get(app)) {
        Ok(confirmed) => confirmed,
        Err(error) => {
            return Err(rollback_config_after_post_catalog_failure(
                &path,
                current.as_deref(),
                &next,
                backup_snapshot.as_ref(),
                catalog_commit,
                error,
            ));
        }
    };

    if requires_provider_sync && sync_history {
        let source_provider = history_source_provider
            .as_deref()
            .expect("history source must exist when history sync is required");
        let target_provider = target_provider
            .as_deref()
            .expect("provider target must exist when history sync is required");
        let history_result = if source_provider == target_provider {
            crate::infra::codex_provider_sync::codex_provider_sync(
                app,
                crate::infra::codex_provider_sync::CodexProviderSyncContext {
                    trigger: "codex_config_set_history".to_string(),
                    target_provider: target_provider.to_string(),
                    config_bytes: None,
                    sync_history: true,
                },
            )
        } else {
            crate::infra::codex_provider_sync::codex_provider_sync_history_only(
                app,
                "codex_config_set_history",
                source_provider,
                target_provider,
            )
        };
        if let Err(error) = history_result {
            return Err(rollback_config_after_post_catalog_failure(
                &path,
                current.as_deref(),
                &next,
                backup_snapshot.as_ref(),
                catalog_commit,
                error,
            ));
        }
    }

    Ok(confirmed)
}

#[cfg(test)]
mod tests;
