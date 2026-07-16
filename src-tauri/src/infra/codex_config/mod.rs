//! Usage: Read / patch Codex user-level `config.toml` ($CODEX_HOME/config.toml).

mod parsing;
mod patching;
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
    )
    .inspect_err(|_err| {
        let _ = restore_optional_file(&manifest_path, &manifest_snapshot);
    })?
    else {
        return Ok(None);
    };

    let backup_snapshot = match snapshot_optional_file(&backup_path) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let _ = restore_optional_file(&manifest_path, &manifest_snapshot);
            return Err(format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}").into());
        }
    };
    let snapshot = CodexCliProxyBackupSnapshot {
        manifest_path,
        manifest_existed: manifest_snapshot.0,
        manifest_bytes: manifest_snapshot.1,
        backup_path,
        backup_existed: backup_snapshot.0,
        backup_bytes: backup_snapshot.1,
    };

    if let Err(err) = write_file_atomic_if_changed(&snapshot.backup_path, next_bytes)
        .map_err(|err| format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}"))
    {
        let _ = restore_codex_cli_proxy_backup_snapshot(&snapshot);
        return Err(err.into());
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

pub(crate) fn restore_codex_cli_proxy_backup_snapshot(
    snapshot: &CodexCliProxyBackupSnapshot,
) -> crate::shared::error::AppResult<()> {
    restore_optional_file(
        &snapshot.backup_path,
        &(snapshot.backup_existed, snapshot.backup_bytes.clone()),
    )?;
    restore_optional_file(
        &snapshot.manifest_path,
        &(snapshot.manifest_existed, snapshot.manifest_bytes.clone()),
    )?;
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> crate::shared::error::AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)
            .map_err(|err| format!("failed to remove dir {}: {err}", path.display()).into()),
        Ok(_) => fs::remove_file(path)
            .map_err(|err| format!("failed to remove file {}: {err}", path.display()).into()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("failed to inspect path {}: {err}", path.display()).into()),
    }
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
    let path = codex_paths::codex_config_toml_path(app)?;
    if path.exists() && is_symlink(&path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            path.display()
        )
        .into());
    }

    let bytes = codex_config_normalize_raw_toml(toml)?;
    let backup_bytes = bytes.clone();
    let backup_snapshot = sync_codex_cli_proxy_backup_if_enabled(app, &backup_bytes)?;
    if let Err(err) = write_file_atomic_if_changed(&path, &bytes) {
        if let Some(snapshot) = backup_snapshot.as_ref() {
            restore_codex_cli_proxy_backup_snapshot(snapshot)?;
        }
        return Err(err);
    }
    codex_config_get(app)
}

pub fn codex_config_set<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    patch: CodexConfigPatch,
) -> crate::shared::error::AppResult<CodexConfigState> {
    let path = codex_paths::codex_config_toml_path(app)?;
    if path.exists() && is_symlink(&path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            path.display()
        )
        .into());
    }

    let current = read_optional_codex_config_file(&path)?;
    let requires_provider_sync = patch_requires_provider_sync(&patch);
    let next = codex_config_next_bytes(current, patch)?;
    ensure_codex_config_len(&next, "codex config.toml")?;
    let backup_bytes = next.clone();
    let backup_snapshot = sync_codex_cli_proxy_backup_if_enabled(app, &backup_bytes)?;

    if requires_provider_sync {
        let next_text = String::from_utf8(next.clone())
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
        let target_provider = codex_config_patch_target_provider(&next_text)?;
        if let Err(err) = crate::infra::codex_provider_sync::codex_provider_sync(
            app,
            crate::infra::codex_provider_sync::CodexProviderSyncContext {
                trigger: "codex_config_set".to_string(),
                target_provider,
                config_bytes: Some(next),
            },
        ) {
            if let Some(snapshot) = backup_snapshot.as_ref() {
                restore_codex_cli_proxy_backup_snapshot(snapshot)?;
            }
            return Err(err);
        }
    } else if let Err(err) = write_file_atomic_if_changed(&path, &next) {
        if let Some(snapshot) = backup_snapshot.as_ref() {
            restore_codex_cli_proxy_backup_snapshot(snapshot)?;
        }
        return Err(err);
    }

    codex_config_get(app)
}

#[cfg(test)]
mod tests;
