//! Codex-specific CLI proxy configuration helpers.

use crate::shared::error::AppResult;
use std::path::{Path, PathBuf};

use super::{
    apply_proxy_config, build_manifest_from_captured, build_manifest_with_current_target_paths,
    capture_current_target_state, codex_manifest_snapshot, expected_codex_manifest_snapshot,
    read_cli_proxy_file, read_optional_cli_proxy_file, restore_file_snapshot_conditionally,
    write_captured_backups, write_cli_proxy_file_atomic, write_manifest, AppliedProxyConfig,
    CliProxyResult, FileSnapshot, PLACEHOLDER_KEY,
};

pub(super) fn codex_config_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<PathBuf> {
    crate::codex_paths::codex_config_toml_path(app)
}

pub(super) fn codex_auth_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<PathBuf> {
    crate::codex_paths::codex_auth_json_path(app)
}

pub(super) fn preflight_proxy_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
) -> AppResult<()> {
    let current = read_optional_cli_proxy_file(&codex_config_path(app)?)?;
    let _ = build_codex_config_toml_for_existing_proxy(
        current,
        &format!("{}/v1", base_origin.trim_end_matches('/')),
        None,
        CodexConfigPlatform::current(),
        super::codex_oauth_compatible_proxy_mode(app),
    )?;
    Ok(())
}

pub(super) fn is_codex_proxy_target_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let config_path = match codex_config_path(app) {
        Ok(path) => path,
        Err(_) => return false,
    };

    let config = match read_cli_proxy_file(&config_path) {
        Ok(content) => content,
        Err(_) => return false,
    };
    let has_proxy_provider =
        crate::infra::codex_config::provider_projection::has_managed_provider_identity(&config);
    if super::codex_oauth_compatible_proxy_mode(app) {
        return has_proxy_provider;
    }

    let auth_path = match codex_auth_path(app) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let auth_bytes = match read_cli_proxy_file(&auth_path) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    let auth = match serde_json::from_slice::<serde_json::Value>(&auth_bytes) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let has_proxy_auth = auth.get("OPENAI_API_KEY").and_then(|value| value.as_str())
        == Some(PLACEHOLDER_KEY)
        && auth.get("auth_mode").and_then(|value| value.as_str()) == Some("apikey");

    has_proxy_provider && has_proxy_auth
}

pub(super) fn rebind_codex_manifest_after_home_change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    mut manifest: super::CliProxyManifest,
    base_origin: &str,
    apply_live: bool,
    sync_catalog: bool,
    trace_id: String,
) -> AppResult<CliProxyResult> {
    let captured = capture_current_target_state(app, "codex")?;
    let previous_manifest = manifest.clone();
    let manifest_before = codex_manifest_snapshot(app)?;
    let target_already_proxy_managed = is_proxy_config_applied(app, base_origin)
        || previous_manifest
            .base_origin
            .as_deref()
            .is_some_and(|origin| is_proxy_config_applied(app, origin))
        || is_codex_proxy_target_state(app);

    let origin = Some(base_origin.to_string());
    let rebind_msg = |live: bool| {
        if live {
            "已重绑 Codex 目录并写入当前网关配置".to_string()
        } else {
            "已重绑 Codex 目录基线，待网关启动后接管".to_string()
        }
    };

    if target_already_proxy_managed {
        manifest = build_manifest_with_current_target_paths(app, &manifest, base_origin)?;
        let manifest_committed = expected_codex_manifest_snapshot(app, &manifest)?;

        if let Err(err) = write_manifest(app, "codex", &manifest) {
            let rollback = restore_file_snapshot_conditionally(
                &manifest_before,
                &manifest_committed,
                super::CLI_PROXY_MANIFEST_MAX_BYTES,
            );
            let (code, message) = match rollback {
                Ok(()) => ("CLI_PROXY_REBIND_MANIFEST_WRITE_FAILED", err.to_string()),
                Err(rollback_error) => (
                    "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                    format!("{err}; rollback failed: {rollback_error}"),
                ),
            };
            return Ok(CliProxyResult::failure(
                trace_id, "codex", true, code, message, origin,
            ));
        }

        let restored_proxy = match super::restore_backups_exactly_from_manifest(app, &manifest) {
            Ok(applied) => applied,
            Err(err) => {
                let rollback =
                    rollback_rebind_stages(&manifest_before, &manifest_committed, None, None);
                let recovery_required =
                    err.code() == "CLI_PROXY_APPLY_RECOVERY_REQUIRED" || rollback.is_err();
                let (code, message) = match rollback {
                    Ok(()) if !recovery_required => {
                        ("CLI_PROXY_REBIND_RESTORE_FAILED", err.to_string())
                    }
                    Ok(()) => ("CLI_PROXY_REBIND_RECOVERY_REQUIRED", err.to_string()),
                    Err(rollback_error) => (
                        "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                        format!("{err}; rollback failed: {rollback_error}"),
                    ),
                };
                return Ok(CliProxyResult::failure(
                    trace_id, "codex", true, code, message, origin,
                ));
            }
        };

        let applied_proxy = if apply_live {
            match apply_proxy_config(app, "codex", base_origin) {
                Ok(applied) => Some(applied),
                Err(err) => {
                    let rollback = rollback_rebind_stages(
                        &manifest_before,
                        &manifest_committed,
                        Some(&restored_proxy),
                        None,
                    );
                    let recovery_required =
                        err.code() == "CLI_PROXY_APPLY_RECOVERY_REQUIRED" || rollback.is_err();
                    let (code, message) = match rollback {
                        Ok(()) if !recovery_required => {
                            ("CLI_PROXY_REBIND_APPLY_FAILED", err.to_string())
                        }
                        Ok(()) => ("CLI_PROXY_REBIND_RECOVERY_REQUIRED", err.to_string()),
                        Err(rollback_error) => (
                            "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                            format!("{err}; rollback failed: {rollback_error}"),
                        ),
                    };
                    return Ok(CliProxyResult::failure(
                        trace_id, "codex", true, code, message, origin,
                    ));
                }
            }
        } else {
            None
        };

        if sync_catalog {
            if let Err(err) = crate::codex_model_catalog::managed::sync_current_locked(app) {
                let rollback = rollback_rebind_stages(
                    &manifest_before,
                    &manifest_committed,
                    applied_proxy.as_ref(),
                    Some(&restored_proxy),
                );
                let recovery_required =
                    super::managed_catalog_error_requires_recovery(&err) || rollback.is_err();
                let (code, message) = match rollback {
                    Ok(()) if !recovery_required => {
                        ("CLI_PROXY_MANAGED_MODEL_SYNC_FAILED", err.to_string())
                    }
                    Ok(()) => ("CLI_PROXY_REBIND_RECOVERY_REQUIRED", err.to_string()),
                    Err(rollback_error) => (
                        "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                        format!("{err}; rollback failed: {rollback_error}"),
                    ),
                };
                return Ok(CliProxyResult::failure(
                    trace_id, "codex", true, code, message, origin,
                ));
            }
        }

        return Ok(CliProxyResult::success(
            trace_id,
            "codex",
            true,
            rebind_msg(apply_live),
            origin,
        ));
    }

    let backup_applied = write_captured_backups(app, "codex", &captured)?;
    manifest = build_manifest_from_captured(&manifest, base_origin, captured);
    let manifest_committed = match expected_codex_manifest_snapshot(app, &manifest) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let rollback = backup_applied.rollback();
            let (code, message) = match rollback {
                Ok(()) => ("CLI_PROXY_REBIND_MANIFEST_WRITE_FAILED", error.to_string()),
                Err(rollback_error) => (
                    "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                    format!("{error}; backup rollback failed: {rollback_error}"),
                ),
            };
            return Ok(CliProxyResult::failure(
                trace_id, "codex", true, code, message, origin,
            ));
        }
    };

    if let Err(err) = write_manifest(app, "codex", &manifest) {
        let rollback = rollback_rebind_stages(
            &manifest_before,
            &manifest_committed,
            None,
            Some(&backup_applied),
        );
        let (code, message) = match rollback {
            Ok(()) => ("CLI_PROXY_REBIND_MANIFEST_WRITE_FAILED", err.to_string()),
            Err(rollback_error) => (
                "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                format!("{err}; rollback failed: {rollback_error}"),
            ),
        };
        return Ok(CliProxyResult::failure(
            trace_id, "codex", true, code, message, origin,
        ));
    }

    let applied_proxy = if apply_live {
        match apply_proxy_config(app, "codex", base_origin) {
            Ok(applied) => Some(applied),
            Err(err) => {
                let rollback = rollback_rebind_stages(
                    &manifest_before,
                    &manifest_committed,
                    None,
                    Some(&backup_applied),
                );
                let recovery_required =
                    err.code() == "CLI_PROXY_APPLY_RECOVERY_REQUIRED" || rollback.is_err();
                let (code, message) = match rollback {
                    Ok(()) if !recovery_required => {
                        ("CLI_PROXY_REBIND_APPLY_FAILED", err.to_string())
                    }
                    Ok(()) => ("CLI_PROXY_REBIND_RECOVERY_REQUIRED", err.to_string()),
                    Err(rollback_error) => (
                        "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                        format!("{err}; rollback failed: {rollback_error}"),
                    ),
                };
                return Ok(CliProxyResult::failure(
                    trace_id, "codex", true, code, message, origin,
                ));
            }
        }
    } else {
        None
    };

    if sync_catalog {
        if let Err(err) = crate::codex_model_catalog::managed::sync_current_locked(app) {
            let rollback = rollback_rebind_stages(
                &manifest_before,
                &manifest_committed,
                applied_proxy.as_ref(),
                Some(&backup_applied),
            );
            let recovery_required =
                super::managed_catalog_error_requires_recovery(&err) || rollback.is_err();
            let (code, message) = match rollback {
                Ok(()) if !recovery_required => {
                    ("CLI_PROXY_MANAGED_MODEL_SYNC_FAILED", err.to_string())
                }
                Ok(()) => ("CLI_PROXY_REBIND_RECOVERY_REQUIRED", err.to_string()),
                Err(rollback_error) => (
                    "CLI_PROXY_REBIND_RECOVERY_REQUIRED",
                    format!("{err}; rollback failed: {rollback_error}"),
                ),
            };
            return Ok(CliProxyResult::failure(
                trace_id, "codex", true, code, message, origin,
            ));
        }
    }

    Ok(CliProxyResult::success(
        trace_id,
        "codex",
        true,
        rebind_msg(apply_live),
        origin,
    ))
}

fn rollback_rebind_stages(
    manifest_before: &FileSnapshot,
    manifest_committed: &FileSnapshot,
    applied_proxy: Option<&AppliedProxyConfig>,
    backup_applied: Option<&AppliedProxyConfig>,
) -> AppResult<()> {
    let mut errors = Vec::new();
    if let Some(applied_proxy) = applied_proxy {
        if let Err(error) = applied_proxy.rollback() {
            errors.push(error.to_string());
        }
    }
    if let Some(backup_applied) = backup_applied {
        if let Err(error) = backup_applied.rollback() {
            errors.push(error.to_string());
        }
    }
    if let Err(error) = restore_file_snapshot_conditionally(
        manifest_before,
        manifest_committed,
        super::CLI_PROXY_MANIFEST_MAX_BYTES,
    ) {
        errors.push(error.to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Codex home rebind rollback could not restore all owned files: {}",
            errors.join("; ")
        )
        .into())
    }
}

/// Merge-restore Codex `auth.json`: only revert the proxy-managed keys
/// (`OPENAI_API_KEY`, `auth_mode`) and restore `tokens` / `last_refresh` from
/// the backup if they existed, while preserving any other user changes.
pub(super) fn merge_restore_codex_auth_json(
    target_path: &Path,
    backup_path: &Path,
) -> AppResult<()> {
    const PROXY_INSERTED_KEYS: &[&str] = &["OPENAI_API_KEY", "auth_mode"];
    const PROXY_REMOVED_KEYS: &[&str] = &["tokens", "last_refresh"];

    let current_bytes = read_optional_cli_proxy_file(target_path)?;
    let backup_bytes = read_cli_proxy_file(backup_path)?;

    let mut current: serde_json::Value = match current_bytes {
        Some(b) if !b.is_empty() => {
            serde_json::from_slice(&b).unwrap_or_else(|_| serde_json::json!({}))
        }
        _ => serde_json::json!({}),
    };

    let backup: serde_json::Value =
        serde_json::from_slice(&backup_bytes).unwrap_or_else(|_| serde_json::json!({}));

    if let Some(obj) = current.as_object_mut() {
        let backup_obj = backup.as_object();

        // Revert inserted keys
        for key in PROXY_INSERTED_KEYS {
            if let Some(original) = backup_obj.and_then(|b| b.get(*key)) {
                obj.insert(key.to_string(), original.clone());
            } else {
                obj.remove(*key);
            }
        }

        // Restore keys that the proxy removed
        for key in PROXY_REMOVED_KEYS {
            if let Some(original) = backup_obj.and_then(|b| b.get(*key)) {
                obj.insert(key.to_string(), original.clone());
            }
        }
    }

    let mut bytes = serde_json::to_vec_pretty(&current)
        .map_err(|e| format!("failed to serialize auth.json: {e}"))?;
    bytes.push(b'\n');
    write_cli_proxy_file_atomic(target_path, &bytes)?;
    Ok(())
}

/// Merge-restore Codex `config.toml`: revert the proxy-managed root keys
/// (`model_provider`, `preferred_auth_method`, `model_catalog_json`) and the
/// `[model_providers.aio]` section / `[windows] sandbox` while preserving user
/// changes.
pub(super) fn merge_restore_codex_config_toml(
    target_path: &Path,
    backup_path: &Path,
) -> AppResult<()> {
    let current_bytes = read_optional_cli_proxy_file(target_path)?;
    let backup_bytes = read_cli_proxy_file(backup_path)?;

    let current =
        crate::infra::codex_config::provider_projection::restore_managed_provider_projection(
            current_bytes.as_deref().unwrap_or_default(),
            &backup_bytes,
        )?;
    let current_str = String::from_utf8(current)
        .map_err(|_| "CLI_PROXY_INVALID_TOML: config.toml must be valid UTF-8".to_string())?;
    let backup_str = String::from_utf8_lossy(&backup_bytes).to_string();

    let mut lines: Vec<String> = if current_str.is_empty() {
        Vec::new()
    } else {
        current_str.lines().map(|l| l.to_string()).collect()
    };

    let backup_lines: Vec<String> = if backup_str.is_empty() {
        Vec::new()
    } else {
        backup_str.lines().map(|l| l.to_string()).collect()
    };

    // --- Revert root `preferred_auth_method` ---
    let backup_auth_method = find_root_key_value(&backup_lines, "preferred_auth_method");
    revert_root_key(
        &mut lines,
        "preferred_auth_method",
        backup_auth_method.as_deref(),
    );

    // --- Revert root `model_catalog_json` ---
    let backup_model_catalog = find_root_key_value(&backup_lines, "model_catalog_json");
    revert_root_key(
        &mut lines,
        "model_catalog_json",
        backup_model_catalog.as_deref(),
    );

    // --- Revert `[windows] sandbox` ---
    // If the backup did not have `[windows]` sandbox, remove the one the proxy added.
    let backup_had_windows_sandbox = has_windows_sandbox(&backup_lines);
    if !backup_had_windows_sandbox {
        remove_windows_sandbox(&mut lines);
    }

    let mut out = lines.join("\n");
    out.push('\n');
    write_cli_proxy_file_atomic(target_path, out.as_bytes())?;
    Ok(())
}

// -- TOML helpers for merge-restore -----------------------------------------

/// Find the value of a root-level `key = "value"` line (before any `[table]` header).
pub(super) fn find_root_key_value(lines: &[String], key: &str) -> Option<String> {
    let first_table = lines
        .iter()
        .position(|l| l.trim().starts_with('['))
        .unwrap_or(lines.len());
    for line in &lines[..first_table] {
        let trimmed = line.trim_start();
        if trimmed.starts_with(key) {
            if let Some((_, v)) = trimmed.split_once('=') {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Revert a root-level key to its backup value, or remove it if backup didn't have it.
pub(super) fn revert_root_key(lines: &mut Vec<String>, key: &str, backup_value: Option<&str>) {
    let first_table = lines
        .iter()
        .position(|l| l.trim().starts_with('['))
        .unwrap_or(lines.len());

    let pos = lines[..first_table]
        .iter()
        .position(|l| l.trim_start().starts_with(key));

    match (pos, backup_value) {
        (Some(idx), Some(val)) => {
            lines[idx] = format!("{key} = {val}");
        }
        (Some(idx), None) => {
            lines.remove(idx);
        }
        (None, Some(val)) => {
            // Backup had it but current doesn't -- shouldn't happen, but restore it
            lines.insert(0, format!("{key} = {val}"));
        }
        (None, None) => {} // Neither has it, nothing to do
    }
}

/// Check if backup lines contain a `[windows]` section with `sandbox` key.
pub(super) fn has_windows_sandbox(lines: &[String]) -> bool {
    let Some(start) = lines.iter().position(|l| l.trim() == "[windows]") else {
        return false;
    };
    let end = find_next_table_header(lines, start.saturating_add(1));
    lines[start + 1..end]
        .iter()
        .any(|l| l.trim_start().starts_with("sandbox"))
}

/// Remove the `sandbox` key from the `[windows]` section; remove the section if empty.
pub(super) fn remove_windows_sandbox(lines: &mut Vec<String>) {
    let Some(start) = lines.iter().position(|l| l.trim() == "[windows]") else {
        return;
    };
    let end = find_next_table_header(lines, start.saturating_add(1));

    // Remove sandbox line
    let mut i = start + 1;
    while i < end && i < lines.len() {
        if lines[i].trim_start().starts_with("sandbox") {
            lines.remove(i);
            break;
        }
        i += 1;
    }

    // If only the header remains (with optional blank lines), remove the whole section
    let new_end = find_next_table_header(lines, start.saturating_add(1));
    let body_empty = lines[start + 1..new_end]
        .iter()
        .all(|l| l.trim().is_empty());
    if body_empty {
        lines.drain(start..new_end);
    }
}

pub(super) fn find_next_table_header(lines: &[String], from: usize) -> usize {
    lines[from..]
        .iter()
        .position(|line| line.trim().starts_with('['))
        .map(|offset| from + offset)
        .unwrap_or(lines.len())
}

fn move_model_provider_base_before_nested(lines: &mut Vec<String>, provider_key: &str) {
    let base_headers = [
        format!("[model_providers.{provider_key}]"),
        format!("[model_providers.\"{provider_key}\"]"),
        format!("[model_providers.'{provider_key}']"),
    ];
    let nested_prefixes = [
        format!("[model_providers.{provider_key}."),
        format!("[model_providers.\"{provider_key}\"."),
        format!("[model_providers.'{provider_key}'."),
    ];
    let Some(base_start) = lines
        .iter()
        .position(|line| base_headers.iter().any(|header| line.trim() == header))
    else {
        return;
    };
    let Some(nested_start) = lines.iter().position(|line| {
        nested_prefixes
            .iter()
            .any(|prefix| line.trim().starts_with(prefix))
    }) else {
        return;
    };
    if base_start < nested_start {
        return;
    }

    let base_end = find_next_table_header(lines, base_start.saturating_add(1));
    let block: Vec<String> = lines.drain(base_start..base_end).collect();
    lines.splice(nested_start..nested_start, block);
}

/// Upsert a root-level `key = "value"` line before any `[table]` header.
/// If `trailing_blank` is true and the inserted line is followed by a non-blank
/// line, an empty separator line is added after it.
fn upsert_root_toml_key(lines: &mut Vec<String>, key: &str, value: &str, trailing_blank: bool) {
    let first_table = lines
        .iter()
        .position(|l| l.trim().starts_with('['))
        .unwrap_or(lines.len());

    if let Some(line) = lines
        .iter_mut()
        .take(first_table)
        .find(|line| line.trim_start().starts_with(key))
    {
        *line = format!("{key} = \"{value}\"");
        return;
    }

    let mut insert_at = 0;
    while insert_at < first_table {
        let trimmed = lines[insert_at].trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            insert_at += 1;
            continue;
        }
        break;
    }

    lines.insert(insert_at, format!("{key} = \"{value}\""));
    if trailing_blank && insert_at + 1 < lines.len() && !lines[insert_at + 1].trim().is_empty() {
        lines.insert(insert_at + 1, String::new());
    }
}

pub(super) fn upsert_root_preferred_auth_method(lines: &mut Vec<String>, value: &str) {
    upsert_root_toml_key(lines, "preferred_auth_method", value, false);
}

pub(super) fn remove_root_preferred_auth_method_if_api_key(lines: &mut Vec<String>) {
    let first_table = lines
        .iter()
        .position(|l| l.trim().starts_with('['))
        .unwrap_or(lines.len());

    let Some(pos) = lines[..first_table]
        .iter()
        .position(|l| l.trim_start().starts_with("preferred_auth_method"))
    else {
        return;
    };

    let Some((_, value)) = lines[pos].trim_start().split_once('=') else {
        return;
    };

    let normalized = value.trim().trim_matches('"').trim_matches('\'');
    if normalized == "apikey" {
        lines.remove(pos);
    }
}

fn has_root_preferred_auth_method_api_key(config: &str) -> bool {
    let lines: Vec<String> = config.lines().map(|line| line.to_string()).collect();
    find_root_key_value(&lines, "preferred_auth_method")
        .as_deref()
        .map(|value| value.trim().trim_matches('"').trim_matches('\'') == "apikey")
        .unwrap_or(false)
}

pub(super) fn upsert_windows_sandbox(lines: &mut Vec<String>) {
    let header = "[windows]";
    if let Some(start) = lines.iter().position(|l| l.trim() == header) {
        let end = find_next_table_header(lines, start.saturating_add(1));
        let has_sandbox = lines[start + 1..end]
            .iter()
            .any(|l| l.trim_start().starts_with("sandbox"));
        if !has_sandbox {
            lines.insert(start + 1, "sandbox = \"elevated\"".to_string());
        }
    } else {
        if !lines.is_empty() && !lines.last().unwrap_or(&String::new()).trim().is_empty() {
            lines.push(String::new());
        }
        lines.push(header.to_string());
        lines.push("sandbox = \"elevated\"".to_string());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CodexConfigPlatform {
    Windows,
    Other,
}

impl CodexConfigPlatform {
    pub(super) fn current() -> Self {
        if std::env::consts::OS == "windows" {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

#[cfg(test)]
pub(super) fn build_codex_config_toml(
    current: Option<Vec<u8>>,
    base_url: &str,
    platform: CodexConfigPlatform,
) -> AppResult<Vec<u8>> {
    build_codex_config_toml_with_auth_strategy(current, base_url, None, platform, false)
}

#[cfg(test)]
pub(super) fn build_codex_config_toml_oauth_compatible(
    current: Option<Vec<u8>>,
    base_url: &str,
    platform: CodexConfigPlatform,
) -> AppResult<Vec<u8>> {
    build_codex_config_toml_with_auth_strategy(current, base_url, None, platform, true)
}

pub(super) fn build_codex_config_toml_for_existing_proxy(
    current: Option<Vec<u8>>,
    base_url: &str,
    previous_base_url: Option<&str>,
    platform: CodexConfigPlatform,
    oauth_compatible: bool,
) -> AppResult<Vec<u8>> {
    build_codex_config_toml_with_auth_strategy(
        current,
        base_url,
        previous_base_url,
        platform,
        oauth_compatible,
    )
}

fn build_codex_config_toml_with_auth_strategy(
    current: Option<Vec<u8>>,
    base_url: &str,
    previous_base_url: Option<&str>,
    platform: CodexConfigPlatform,
    oauth_compatible: bool,
) -> AppResult<Vec<u8>> {
    let projected = crate::infra::codex_config::provider_projection::project_active_provider(
        current.as_deref().unwrap_or_default(),
        base_url,
        previous_base_url,
    )?;
    let provider_key =
        crate::infra::codex_config::provider_projection::desired_provider_key_from_config(
            &projected,
        )?;
    let input = String::from_utf8(projected)
        .map_err(|_| "CLI_PROXY_INVALID_TOML: config.toml must be valid UTF-8".to_string())?;

    let mut lines: Vec<String> = if input.is_empty() {
        Vec::new()
    } else {
        input.lines().map(|l| l.to_string()).collect()
    };
    move_model_provider_base_before_nested(&mut lines, provider_key.as_str());

    if oauth_compatible {
        remove_root_preferred_auth_method_if_api_key(&mut lines);
    } else {
        upsert_root_preferred_auth_method(&mut lines, "apikey");
    }
    if platform == CodexConfigPlatform::Windows {
        upsert_windows_sandbox(&mut lines);
    }

    let mut out = lines.join("\n");
    out.push('\n');
    Ok(out.into_bytes())
}

pub(super) fn build_codex_auth_json(current: Option<Vec<u8>>) -> AppResult<Vec<u8>> {
    let mut value = match current {
        Some(bytes) if bytes.is_empty() => serde_json::json!({}),
        Some(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|e| format!("CLI_PROXY_INVALID_AUTH_JSON: failed to parse auth.json: {e}"))?,
        None => serde_json::json!({}),
    };

    let obj = value.as_object_mut().ok_or_else(|| {
        crate::shared::error::AppError::from(
            "CLI_PROXY_INVALID_AUTH_JSON: auth.json root must be a JSON object",
        )
    })?;
    obj.insert(
        "OPENAI_API_KEY".to_string(),
        serde_json::Value::String(PLACEHOLDER_KEY.to_string()),
    );
    obj.insert(
        "auth_mode".to_string(),
        serde_json::Value::String("apikey".to_string()),
    );
    // Remove OAuth residuals that would confuse Codex CLI into chatgpt auth mode.
    obj.remove("tokens");
    obj.remove("last_refresh");

    let mut out = serde_json::to_vec_pretty(&value)
        .map_err(|e| format!("failed to serialize auth.json: {e}"))?;
    out.push(b'\n');
    Ok(out)
}

/// Check whether Codex proxy config is currently applied.
pub(super) fn is_proxy_config_applied<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
) -> bool {
    let config_path = match codex_config_path(app) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let config = match read_cli_proxy_file(&config_path) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !crate::infra::codex_config::provider_projection::is_managed_projection_applied(
        &config,
        &format!("{base_origin}/v1"),
    ) {
        return false;
    }

    if super::codex_oauth_compatible_proxy_mode(app) {
        return !has_root_preferred_auth_method_api_key(&String::from_utf8_lossy(&config));
    }

    let auth_path = match codex_auth_path(app) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let auth_bytes = match read_cli_proxy_file(&auth_path) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let auth = match serde_json::from_slice::<serde_json::Value>(&auth_bytes) {
        Ok(v) => v,
        Err(_) => return false,
    };
    auth.get("OPENAI_API_KEY").and_then(|value| value.as_str()) == Some(PLACEHOLDER_KEY)
        && auth.get("auth_mode").and_then(|value| value.as_str()) == Some("apikey")
}

/// Public entry point called from `sync_enabled` and `rebind_codex_home_after_change`.
pub(super) fn rebind_codex_home_after_change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> AppResult<CliProxyResult> {
    rebind_codex_home_after_change_with_catalog_sync(app, base_origin, apply_live, true)
}

pub(super) fn rebind_codex_home_for_config_import<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
) -> AppResult<CliProxyResult> {
    rebind_codex_home_after_change_with_catalog_sync(app, base_origin, false, false)
}

fn rebind_codex_home_after_change_with_catalog_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
    sync_catalog: bool,
) -> AppResult<CliProxyResult> {
    if !base_origin.starts_with("http://") && !base_origin.starts_with("https://") {
        return Err("SEC_INVALID_INPUT: base_origin must start with http:// or https://".into());
    }

    let trace_id = super::new_trace_id("cli-proxy-codex-home-rebind");
    let origin = Some(base_origin.to_string());
    let Some(manifest) = super::read_manifest(app, "codex")? else {
        return Ok(CliProxyResult::success(
            trace_id,
            "codex",
            false,
            "Codex 代理未启用，无需重绑".to_string(),
            origin,
        ));
    };

    if !manifest.enabled {
        return Ok(CliProxyResult::success(
            trace_id,
            "codex",
            false,
            "Codex 代理未启用，无需重绑".to_string(),
            origin,
        ));
    }

    if !super::manifest_target_paths_changed(app, &manifest)? {
        let msg = if apply_live {
            "Codex 目录未变化，无需重绑"
        } else {
            "Codex 目录未变化，待网关启动后按现有配置接管"
        };
        return Ok(CliProxyResult::success(
            trace_id,
            "codex",
            true,
            msg.to_string(),
            origin,
        ));
    }

    rebind_codex_manifest_after_home_change(
        app,
        manifest,
        base_origin,
        apply_live,
        sync_catalog,
        trace_id,
    )
}
