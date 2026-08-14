use crate::app_state::{ensure_db_ready, DbInitState};
use crate::gateway::events::GATEWAY_STATUS_EVENT_NAME;
use crate::gateway_control::app_ensure_gateway_running;
use crate::shared::ipc_confirm::RiskyIpcConfirm;
use crate::{base_url_probe, blocking, providers};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri_plugin_clipboard_manager::ClipboardExt;

const ENV_CLAUDE_DISABLE_NONESSENTIAL_TRAFFIC: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";
const ENV_DISABLE_ERROR_REPORTING: &str = "DISABLE_ERROR_REPORTING";
const ENV_DISABLE_TELEMETRY: &str = "DISABLE_TELEMETRY";
const ENV_MCP_TIMEOUT: &str = "MCP_TIMEOUT";
const ENV_ANTHROPIC_BASE_URL: &str = "ANTHROPIC_BASE_URL";
const ENV_ANTHROPIC_AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";
const ENV_ANTHROPIC_DEFAULT_OPUS_MODEL: &str = "ANTHROPIC_DEFAULT_OPUS_MODEL";
const ENV_ANTHROPIC_DEFAULT_SONNET_MODEL: &str = "ANTHROPIC_DEFAULT_SONNET_MODEL";
const ENV_ANTHROPIC_DEFAULT_HAIKU_MODEL: &str = "ANTHROPIC_DEFAULT_HAIKU_MODEL";
const ENV_CLAUDE_CODE_MAX_CONTEXT_TOKENS: &str = "CLAUDE_CODE_MAX_CONTEXT_TOKENS";
const ENV_CLAUDE_CODE_AUTO_COMPACT_WINDOW: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
const CX2CC_CLAUDE_OPUS_MODEL_ALIAS: &str = "aio-cx2cc-opus";
const CX2CC_CLAUDE_SONNET_MODEL_ALIAS: &str = "aio-cx2cc-sonnet";
const CX2CC_CLAUDE_HAIKU_MODEL_ALIAS: &str = "aio-cx2cc-haiku";
const CLAUDE_LAUNCHER_DIR_NAME: &str = "claude-launchers";
const CLAUDE_LAUNCHER_ARTIFACT_TTL_SECS: u64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeTerminalContextWindowProjection {
    NotApplicable,
    Exact(u64),
    Mixed(u64),
    Unknown(&'static str),
}

impl ClaudeTerminalContextWindowProjection {
    fn context_window(self) -> Option<u64> {
        match self {
            Self::Exact(context_window) | Self::Mixed(context_window) => Some(context_window),
            Self::NotApplicable | Self::Unknown(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectiveCx2ccSlot {
    remote_model_id: String,
    custom_context_window: Option<u64>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_claude_terminal_launch_command(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
) -> Result<String, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let gateway_base_origin = blocking::run("provider_claude_terminal_launch_gateway_origin", {
        let app = app.clone();
        let db = db.clone();
        move || ensure_gateway_base_origin(&app, &db)
    })
    .await?;

    blocking::run("provider_claude_terminal_launch_command", move || {
        let launch = providers::claude_terminal_launch_context(&db, provider_id)?;
        let context_window_projection = if launch.is_cx2cc {
            match resolve_cx2cc_terminal_context_window(&app, &db, &launch) {
                Ok(projection) => projection,
                Err(error) => {
                    tracing::warn!(
                        provider_id,
                        error_code = error.code(),
                        "cx2cc terminal context projection unavailable; using Claude defaults"
                    );
                    ClaudeTerminalContextWindowProjection::Unknown("resolver_unavailable")
                }
            }
        } else {
            ClaudeTerminalContextWindowProjection::NotApplicable
        };
        log_context_window_projection(provider_id, context_window_projection);
        let claude_base_url = build_claude_gateway_base_url(&gateway_base_origin, provider_id);
        create_claude_terminal_launch_command(
            &app,
            provider_id,
            &claude_base_url,
            &launch.api_key_plaintext,
            context_window_projection.context_window(),
        )
    })
    .await
    .map_err(Into::into)
}

fn resolve_cx2cc_terminal_context_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &crate::db::Db,
    launch: &providers::ClaudeTerminalLaunchContext,
) -> crate::shared::error::AppResult<ClaudeTerminalContextWindowProjection> {
    let settings = crate::settings::read(app)?;
    let slots = match effective_cx2cc_slots(launch, &settings) {
        Ok(slots) => slots,
        Err(reason) => return Ok(ClaudeTerminalContextWindowProjection::Unknown(reason)),
    };

    let needs_catalog = slots
        .iter()
        .any(|slot| slot.custom_context_window.is_none());
    let provider_identities = if needs_catalog {
        match launch.source_provider_id {
            Some(provider_id) => {
                let Some(provider_uuid) = launch.source_provider_uuid.as_ref() else {
                    return Ok(ClaudeTerminalContextWindowProjection::Unknown(
                        "source_identity_unavailable",
                    ));
                };
                vec![(provider_id, provider_uuid.clone())]
            }
            None => providers::list_enabled_for_gateway_using_active_mode(db, "codex")?
                .providers
                .into_iter()
                .map(|provider| (provider.id, provider.provider_uuid))
                .collect(),
        }
    } else {
        Vec::new()
    };

    resolve_effective_slot_context_window(&slots, |remote_model_id| {
        let candidates = context_window_candidates(&provider_identities, remote_model_id);
        Ok(
            match crate::provider_models::resolve_context_window_projection(db, &candidates)? {
                crate::provider_models::ProviderModelContextWindowProjection::Exact {
                    context_window,
                } => ClaudeTerminalContextWindowProjection::Exact(context_window),
                crate::provider_models::ProviderModelContextWindowProjection::Mixed {
                    conservative_context_window,
                } => ClaudeTerminalContextWindowProjection::Mixed(conservative_context_window),
                crate::provider_models::ProviderModelContextWindowProjection::Unknown {
                    reason,
                } => ClaudeTerminalContextWindowProjection::Unknown(reason.as_str()),
            },
        )
    })
}

fn effective_cx2cc_slots(
    launch: &providers::ClaudeTerminalLaunchContext,
    settings: &crate::settings::AppSettings,
) -> Result<Vec<EffectiveCx2ccSlot>, &'static str> {
    let models = &launch.claude_models;
    let slots = [
        (
            models.opus_model.as_deref(),
            settings.cx2cc_fallback_model_opus.as_str(),
            models.opus_context_window,
        ),
        (
            models.haiku_model.as_deref(),
            settings.cx2cc_fallback_model_haiku.as_str(),
            models.haiku_context_window,
        ),
        (
            models.sonnet_model.as_deref(),
            settings.cx2cc_fallback_model_sonnet.as_str(),
            models.sonnet_context_window,
        ),
        (
            models.main_model.as_deref(),
            settings.cx2cc_fallback_model_main.as_str(),
            models.main_context_window,
        ),
    ];

    let minimum = crate::provider_models::MODEL_CONTEXT_WINDOW_MIN_TOKENS as u64;
    let maximum = crate::provider_models::MODEL_CONTEXT_WINDOW_MAX_TOKENS as u64;
    slots
        .into_iter()
        .map(|(custom_model, fallback_model, custom_context_window)| {
            if custom_context_window.is_some() && custom_model.is_none() {
                return Err("custom_context_model_unavailable");
            }
            if custom_context_window.is_some_and(|value| !(minimum..=maximum).contains(&value)) {
                return Err("custom_context_invalid");
            }
            let remote_model_id = custom_model.unwrap_or(fallback_model).trim();
            if remote_model_id.is_empty() {
                return Err("process_model_unavailable");
            }
            Ok(EffectiveCx2ccSlot {
                remote_model_id: remote_model_id.to_string(),
                custom_context_window,
            })
        })
        .collect()
}

fn resolve_effective_slot_context_window(
    slots: &[EffectiveCx2ccSlot],
    mut resolve_catalog: impl FnMut(
        &str,
    ) -> crate::shared::error::AppResult<
        ClaudeTerminalContextWindowProjection,
    >,
) -> crate::shared::error::AppResult<ClaudeTerminalContextWindowProjection> {
    if slots.is_empty() {
        return Ok(ClaudeTerminalContextWindowProjection::Unknown(
            "process_model_unavailable",
        ));
    }

    let mut catalog_cache = HashMap::new();
    let mut windows = Vec::with_capacity(slots.len());
    let mut has_mixed_projection = false;
    for slot in slots {
        let projection = if let Some(context_window) = slot.custom_context_window {
            ClaudeTerminalContextWindowProjection::Exact(context_window)
        } else if let Some(projection) = catalog_cache.get(&slot.remote_model_id).copied() {
            projection
        } else {
            let projection = resolve_catalog(&slot.remote_model_id)?;
            catalog_cache.insert(slot.remote_model_id.clone(), projection);
            projection
        };

        match projection {
            ClaudeTerminalContextWindowProjection::Exact(context_window) => {
                windows.push(context_window);
            }
            ClaudeTerminalContextWindowProjection::Mixed(context_window) => {
                has_mixed_projection = true;
                windows.push(context_window);
            }
            ClaudeTerminalContextWindowProjection::Unknown(reason) => {
                return Ok(ClaudeTerminalContextWindowProjection::Unknown(reason));
            }
            ClaudeTerminalContextWindowProjection::NotApplicable => {
                return Ok(ClaudeTerminalContextWindowProjection::Unknown(
                    "catalog_projection_unavailable",
                ));
            }
        }
    }

    let Some(minimum) = windows.iter().copied().min() else {
        return Ok(ClaudeTerminalContextWindowProjection::Unknown(
            "process_model_unavailable",
        ));
    };
    if has_mixed_projection || windows.iter().any(|window| *window != minimum) {
        Ok(ClaudeTerminalContextWindowProjection::Mixed(minimum))
    } else {
        Ok(ClaudeTerminalContextWindowProjection::Exact(minimum))
    }
}

fn context_window_candidates(
    provider_identities: &[(i64, String)],
    remote_model_id: &str,
) -> Vec<crate::provider_models::ProviderModelContextWindowCandidate> {
    let mut candidates = Vec::with_capacity(provider_identities.len());
    for (provider_id, provider_uuid) in provider_identities {
        candidates.push(
            crate::provider_models::ProviderModelContextWindowCandidate {
                provider_id: *provider_id,
                provider_uuid: provider_uuid.clone(),
                remote_model_id: remote_model_id.to_string(),
            },
        );
    }
    candidates
}

fn log_context_window_projection(
    provider_id: i64,
    projection: ClaudeTerminalContextWindowProjection,
) {
    match projection {
        ClaudeTerminalContextWindowProjection::NotApplicable => {}
        ClaudeTerminalContextWindowProjection::Exact(context_window) => tracing::info!(
            provider_id,
            projection = "exact",
            context_window,
            "cx2cc terminal context projected"
        ),
        ClaudeTerminalContextWindowProjection::Mixed(context_window) => tracing::info!(
            provider_id,
            projection = "mixed",
            context_window,
            "cx2cc terminal context projected conservatively"
        ),
        ClaudeTerminalContextWindowProjection::Unknown(reason) => tracing::info!(
            provider_id,
            projection = "unknown",
            reason,
            "cx2cc terminal context not projected; using Claude defaults"
        ),
    }
}

fn ensure_gateway_base_origin(
    app: &tauri::AppHandle,
    db: &crate::db::Db,
) -> crate::shared::error::AppResult<String> {
    let status = app_ensure_gateway_running(app, db.clone(), None)?;

    crate::app::heartbeat_watchdog::gated_emit(app, GATEWAY_STATUS_EVENT_NAME, status.clone());

    status
        .base_url
        .ok_or_else(|| "SYSTEM_ERROR: gateway base_url missing".to_string().into())
}

fn build_claude_gateway_base_url(gateway_base_origin: &str, provider_id: i64) -> String {
    format!(
        "{}/claude/_aio/provider/{provider_id}",
        gateway_base_origin.trim_end_matches('/')
    )
}

fn is_claude_launcher_artifact_file_name(name: &str) -> bool {
    name.starts_with("claude_") || name.starts_with("aio_claude_launcher_")
}

fn claude_launch_artifact_paths(
    dir: &Path,
    provider_id: i64,
    pid: u32,
    now: i64,
) -> (PathBuf, PathBuf) {
    let config_path = dir.join(format!("claude_{provider_id}_{pid}_{now}.json"));
    let script_path = if cfg!(target_os = "windows") {
        dir.join(format!("aio_claude_launcher_{provider_id}_{pid}_{now}.ps1"))
    } else {
        dir.join(format!("aio_claude_launcher_{provider_id}_{pid}_{now}.sh"))
    };
    (config_path, script_path)
}

fn claude_launcher_artifacts_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    let dir = crate::infra::app_paths::app_data_dir(app)?.join(CLAUDE_LAUNCHER_DIR_NAME);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("SYSTEM_ERROR: create claude launcher dir failed: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

fn prune_stale_claude_launch_artifacts(dir: &Path, now: std::time::SystemTime) {
    let ttl = std::time::Duration::from_secs(CLAUDE_LAUNCHER_ARTIFACT_TTL_SECS);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !is_claude_launcher_artifact_file_name(name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified_at) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified_at) else {
            continue;
        };
        if age > ttl {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_claude_launcher_file(
    path: &Path,
    content: impl AsRef<[u8]>,
    _executable: bool,
) -> crate::shared::error::AppResult<()> {
    std::fs::write(path, content)
        .map_err(|e| format!("SYSTEM_ERROR: write launcher asset failed: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if _executable { 0o700 } else { 0o600 };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    Ok(())
}

fn create_claude_terminal_launch_command<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    provider_id: i64,
    base_url: &str,
    api_key_plaintext: &str,
    context_window: Option<u64>,
) -> crate::shared::error::AppResult<String> {
    let now = crate::shared::time::now_unix_seconds();
    let pid = std::process::id();
    let artifact_dir = claude_launcher_artifacts_dir(app)?;
    prune_stale_claude_launch_artifacts(&artifact_dir, std::time::SystemTime::now());
    let (config_path, script_path) =
        claude_launch_artifact_paths(&artifact_dir, provider_id, pid, now);

    let settings_json = build_claude_settings_json(base_url, api_key_plaintext, context_window)?;
    write_claude_launcher_file(&config_path, settings_json, false)
        .map_err(|e| format!("SYSTEM_ERROR: write claude settings failed: {e}"))?;

    let (script_content, launch_command) = build_claude_launch_assets(&script_path, &config_path);
    if let Err(err) = write_claude_launcher_file(&script_path, script_content, true) {
        let _ = std::fs::remove_file(&config_path);
        return Err(format!("SYSTEM_ERROR: write launch script failed: {err}").into());
    }

    Ok(launch_command)
}

fn build_claude_launch_assets(script_path: &Path, config_path: &Path) -> (String, String) {
    if cfg!(target_os = "windows") {
        let script_content = build_claude_launcher_powershell_script(config_path, script_path);
        let launch_command = build_powershell_launch_command(script_path);
        (script_content, launch_command)
    } else {
        let script_content = build_claude_launcher_bash_script(config_path, script_path);
        let launch_command = build_bash_launch_command(script_path);
        (script_content, launch_command)
    }
}

fn build_claude_settings_json(
    base_url: &str,
    api_key_plaintext: &str,
    context_window: Option<u64>,
) -> crate::shared::error::AppResult<String> {
    let mut env = serde_json::Map::from_iter([
        (
            ENV_CLAUDE_DISABLE_NONESSENTIAL_TRAFFIC.to_string(),
            json!("1"),
        ),
        (ENV_DISABLE_ERROR_REPORTING.to_string(), json!("1")),
        (ENV_DISABLE_TELEMETRY.to_string(), json!("1")),
        (ENV_MCP_TIMEOUT.to_string(), json!("60000")),
        (ENV_ANTHROPIC_BASE_URL.to_string(), json!(base_url)),
        (
            ENV_ANTHROPIC_AUTH_TOKEN.to_string(),
            json!(api_key_plaintext),
        ),
    ]);
    if let Some(context_window) = context_window {
        let context_window = context_window.to_string();
        env.insert(
            ENV_ANTHROPIC_DEFAULT_OPUS_MODEL.to_string(),
            json!(CX2CC_CLAUDE_OPUS_MODEL_ALIAS),
        );
        env.insert(
            ENV_ANTHROPIC_DEFAULT_SONNET_MODEL.to_string(),
            json!(CX2CC_CLAUDE_SONNET_MODEL_ALIAS),
        );
        env.insert(
            ENV_ANTHROPIC_DEFAULT_HAIKU_MODEL.to_string(),
            json!(CX2CC_CLAUDE_HAIKU_MODEL_ALIAS),
        );
        env.insert(
            ENV_CLAUDE_CODE_MAX_CONTEXT_TOKENS.to_string(),
            json!(context_window),
        );
        env.insert(
            ENV_CLAUDE_CODE_AUTO_COMPACT_WINDOW.to_string(),
            json!(context_window),
        );
    }
    let value = json!({ "env": env });

    serde_json::to_string_pretty(&value)
        .map_err(|e| format!("SYSTEM_ERROR: serialize claude settings failed: {e}").into())
}

fn build_claude_launcher_bash_script(config_path: &Path, script_path: &Path) -> String {
    let config_var = bash_single_quote(&config_path.to_string_lossy());
    let script_var = bash_single_quote(&script_path.to_string_lossy());

    format!(
        "#!/bin/bash\n\
config_path={config_var}\n\
script_path={script_var}\n\
cleanup() {{\n\
  rm -f \"$config_path\" \"$script_path\"\n\
}}\n\
trap cleanup EXIT INT TERM HUP\n\
echo \"Using provider-specific claude config:\"\n\
echo \"$config_path\"\n\
claude --settings \"$config_path\"\n\
cleanup\n\
trap - EXIT INT TERM HUP\n\
exec bash --norc --noprofile\n"
    )
}

fn build_claude_launcher_powershell_script(config_path: &Path, script_path: &Path) -> String {
    let config_var = powershell_single_quote(&config_path.to_string_lossy());
    let script_var = powershell_single_quote(&script_path.to_string_lossy());

    format!(
        "$configPath = {config_var}\n\
$scriptPath = {script_var}\n\
try {{\n\
  Write-Output \"Using provider-specific claude config:\"\n\
  Write-Output $configPath\n\
  claude --settings $configPath\n\
}} finally {{\n\
  Remove-Item -LiteralPath $configPath -ErrorAction SilentlyContinue\n\
  Remove-Item -LiteralPath $scriptPath -ErrorAction SilentlyContinue\n\
}}\n"
    )
}

fn build_bash_launch_command(script_path: &Path) -> String {
    format!("bash {}", bash_single_quote(&script_path.to_string_lossy()))
}

fn build_powershell_launch_command(script_path: &Path) -> String {
    format!(
        "powershell -NoLogo -NoExit -ExecutionPolicy Bypass -File {}",
        windows_double_quote(&script_path.to_string_lossy())
    )
}

fn bash_single_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn powershell_single_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "''"))
}

fn windows_double_quote(value: &str) -> String {
    format!("\"{value}\"")
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn provider_copy_api_key_to_clipboard(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<bool, String> {
    RiskyIpcConfirm::require(
        confirm,
        "provider_copy_api_key_to_clipboard",
        format!("provider:{provider_id}:api_key"),
    )?;
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    let api_key = blocking::run(
        "provider_copy_api_key_to_clipboard",
        move || -> crate::shared::error::AppResult<String> {
            let conn = db.open_connection()?;
            let provider = providers::get_by_id(&conn, provider_id)?;
            if provider.auth_mode != "api_key" || provider.source_provider_id.is_some() {
                return Err("SEC_INVALID_INPUT: provider does not own a direct api_key"
                    .to_string()
                    .into());
            }

            let api_key = providers::get_api_key_plaintext(&db, provider_id)?;
            if api_key.trim().is_empty() {
                return Err("SEC_INVALID_INPUT: provider api_key is not configured"
                    .to_string()
                    .into());
            }

            Ok(api_key)
        },
    )
    .await?;

    app.clipboard().write_text(api_key).map_err(|err| {
        format!("SYSTEM_ERROR: failed to write provider api_key to clipboard: {err}")
    })?;
    tracing::info!(provider_id, "provider api_key copied to clipboard");
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn base_url_ping_ms(base_url: String) -> Result<u64, String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("aio-coding-hub-ping/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("PING_HTTP_CLIENT_INIT: {e}"))?;
    base_url_probe::probe_base_url_ms(&client, &base_url, std::time::Duration::from_secs(3)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "windows"))]
    use std::process::{Command, Stdio};
    #[cfg(not(target_os = "windows"))]
    use tempfile::tempdir;

    fn cx2cc_launch_context(
        models: crate::providers::ClaudeModels,
    ) -> providers::ClaudeTerminalLaunchContext {
        providers::ClaudeTerminalLaunchContext {
            api_key_plaintext: "cx2cc-test".to_string(),
            is_cx2cc: true,
            claude_models: models,
            source_provider_id: None,
            source_provider_uuid: None,
        }
    }

    #[test]
    fn bash_single_quote_escapes_single_quote() {
        assert_eq!(bash_single_quote("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    fn powershell_single_quote_escapes_single_quote() {
        assert_eq!(powershell_single_quote("a'b"), "'a''b'");
    }

    #[test]
    fn build_settings_contains_required_envs() {
        let json_text =
            build_claude_settings_json("https://example.com", "sk-test", Some(1_000_000)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json_text).unwrap();
        let env = value
            .get("env")
            .and_then(|v| v.as_object())
            .expect("env object");

        assert_eq!(
            env.get(ENV_CLAUDE_DISABLE_NONESSENTIAL_TRAFFIC)
                .and_then(|v| v.as_str()),
            Some("1")
        );
        assert_eq!(
            env.get(ENV_DISABLE_ERROR_REPORTING)
                .and_then(|v| v.as_str()),
            Some("1")
        );
        assert_eq!(
            env.get(ENV_DISABLE_TELEMETRY).and_then(|v| v.as_str()),
            Some("1")
        );
        assert_eq!(
            env.get(ENV_MCP_TIMEOUT).and_then(|v| v.as_str()),
            Some("60000")
        );
        assert_eq!(
            env.get(ENV_ANTHROPIC_BASE_URL).and_then(|v| v.as_str()),
            Some("https://example.com")
        );
        assert_eq!(
            env.get(ENV_ANTHROPIC_AUTH_TOKEN).and_then(|v| v.as_str()),
            Some("sk-test")
        );
        assert_eq!(
            env.get(ENV_CLAUDE_CODE_MAX_CONTEXT_TOKENS)
                .and_then(|v| v.as_str()),
            Some("1000000")
        );
        assert_eq!(
            env.get(ENV_CLAUDE_CODE_AUTO_COMPACT_WINDOW)
                .and_then(|v| v.as_str()),
            Some("1000000")
        );
        assert_eq!(
            env.get(ENV_ANTHROPIC_DEFAULT_OPUS_MODEL)
                .and_then(|v| v.as_str()),
            Some(CX2CC_CLAUDE_OPUS_MODEL_ALIAS)
        );
        assert_eq!(
            env.get(ENV_ANTHROPIC_DEFAULT_SONNET_MODEL)
                .and_then(|v| v.as_str()),
            Some(CX2CC_CLAUDE_SONNET_MODEL_ALIAS)
        );
        assert_eq!(
            env.get(ENV_ANTHROPIC_DEFAULT_HAIKU_MODEL)
                .and_then(|v| v.as_str()),
            Some(CX2CC_CLAUDE_HAIKU_MODEL_ALIAS)
        );
        assert!(!env.contains_key("ANTHROPIC_MODEL"));
        assert!(!env.contains_key("DISABLE_COMPACT"));
    }

    #[test]
    fn build_settings_omits_context_envs_when_projection_is_unknown() {
        let json_text = build_claude_settings_json("https://example.com", "sk-test", None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json_text).unwrap();
        let env = value
            .get("env")
            .and_then(|v| v.as_object())
            .expect("env object");

        assert!(!env.contains_key(ENV_CLAUDE_CODE_MAX_CONTEXT_TOKENS));
        assert!(!env.contains_key(ENV_CLAUDE_CODE_AUTO_COMPACT_WINDOW));
        assert!(!env.contains_key(ENV_ANTHROPIC_DEFAULT_OPUS_MODEL));
        assert!(!env.contains_key(ENV_ANTHROPIC_DEFAULT_SONNET_MODEL));
        assert!(!env.contains_key(ENV_ANTHROPIC_DEFAULT_HAIKU_MODEL));
        assert!(!env.contains_key("ANTHROPIC_MODEL"));
        assert!(!env.contains_key("DISABLE_COMPACT"));
    }

    #[test]
    fn cx2cc_family_aliases_activate_unknown_models_without_losing_mapper_family() {
        for (alias, family) in [
            (CX2CC_CLAUDE_OPUS_MODEL_ALIAS, "opus"),
            (CX2CC_CLAUDE_SONNET_MODEL_ALIAS, "sonnet"),
            (CX2CC_CLAUDE_HAIKU_MODEL_ALIAS, "haiku"),
        ] {
            assert!(!alias.starts_with("claude-"));
            assert!(!alias.to_ascii_lowercase().contains("[1m]"));
            assert!(alias.contains(family));
        }
    }

    #[test]
    fn effective_slots_keep_each_mapper_slot_and_ignore_reasoning_model() {
        let settings = crate::settings::AppSettings {
            cx2cc_fallback_model_opus: "gpt-fallback".to_string(),
            cx2cc_fallback_model_sonnet: "gpt-fallback".to_string(),
            cx2cc_fallback_model_haiku: "gpt-fallback".to_string(),
            cx2cc_fallback_model_main: "gpt-fallback".to_string(),
            ..Default::default()
        };
        let launch = cx2cc_launch_context(crate::providers::ClaudeModels {
            main_model: Some("gpt-5.6-sol".to_string()),
            haiku_model: Some("gpt-5.6-sol".to_string()),
            sonnet_model: Some("gpt-5.6-sol".to_string()),
            opus_model: Some("gpt-5.6-sol".to_string()),
            reasoning_model: Some("not-used-by-cx2cc-mapper".to_string()),
            main_context_window: Some(1_000_000),
            haiku_context_window: Some(800_000),
            sonnet_context_window: Some(600_000),
            opus_context_window: Some(400_000),
        });

        assert_eq!(
            effective_cx2cc_slots(&launch, &settings).expect("effective slots"),
            vec![
                EffectiveCx2ccSlot {
                    remote_model_id: "gpt-5.6-sol".to_string(),
                    custom_context_window: Some(400_000),
                },
                EffectiveCx2ccSlot {
                    remote_model_id: "gpt-5.6-sol".to_string(),
                    custom_context_window: Some(800_000),
                },
                EffectiveCx2ccSlot {
                    remote_model_id: "gpt-5.6-sol".to_string(),
                    custom_context_window: Some(600_000),
                },
                EffectiveCx2ccSlot {
                    remote_model_id: "gpt-5.6-sol".to_string(),
                    custom_context_window: Some(1_000_000),
                },
            ]
        );
    }

    #[test]
    fn effective_slots_use_per_slot_fallback_without_copying_custom_context() {
        let settings = crate::settings::AppSettings {
            cx2cc_fallback_model_opus: "fallback-opus".to_string(),
            cx2cc_fallback_model_sonnet: "fallback-sonnet".to_string(),
            cx2cc_fallback_model_haiku: "fallback-haiku".to_string(),
            cx2cc_fallback_model_main: "fallback-main".to_string(),
            ..Default::default()
        };
        let launch = cx2cc_launch_context(crate::providers::ClaudeModels {
            opus_model: Some("gpt-5.6-sol".to_string()),
            opus_context_window: Some(900_000),
            ..Default::default()
        });

        assert_eq!(
            effective_cx2cc_slots(&launch, &settings).expect("effective slots"),
            vec![
                EffectiveCx2ccSlot {
                    remote_model_id: "gpt-5.6-sol".to_string(),
                    custom_context_window: Some(900_000),
                },
                EffectiveCx2ccSlot {
                    remote_model_id: "fallback-haiku".to_string(),
                    custom_context_window: None,
                },
                EffectiveCx2ccSlot {
                    remote_model_id: "fallback-sonnet".to_string(),
                    custom_context_window: None,
                },
                EffectiveCx2ccSlot {
                    remote_model_id: "fallback-main".to_string(),
                    custom_context_window: None,
                },
            ]
        );
    }

    #[test]
    fn slot_projection_prefers_custom_context_and_deduplicates_catalog_fallbacks() {
        let slots = vec![
            EffectiveCx2ccSlot {
                remote_model_id: "shared".to_string(),
                custom_context_window: Some(900_000),
            },
            EffectiveCx2ccSlot {
                remote_model_id: "shared".to_string(),
                custom_context_window: None,
            },
            EffectiveCx2ccSlot {
                remote_model_id: "shared".to_string(),
                custom_context_window: None,
            },
            EffectiveCx2ccSlot {
                remote_model_id: "other".to_string(),
                custom_context_window: None,
            },
        ];
        let mut calls = Vec::new();
        let projection = resolve_effective_slot_context_window(&slots, |model| {
            calls.push(model.to_string());
            Ok(ClaudeTerminalContextWindowProjection::Exact(900_000))
        })
        .expect("projection");

        assert_eq!(
            projection,
            ClaudeTerminalContextWindowProjection::Exact(900_000)
        );
        assert_eq!(calls, vec!["shared", "other"]);
    }

    #[test]
    fn slot_projection_keeps_same_model_custom_contexts_separate_and_uses_minimum() {
        let slots = [
            EffectiveCx2ccSlot {
                remote_model_id: "shared".to_string(),
                custom_context_window: Some(1_000_000),
            },
            EffectiveCx2ccSlot {
                remote_model_id: "shared".to_string(),
                custom_context_window: Some(400_000),
            },
        ];
        let projection = resolve_effective_slot_context_window(&slots, |_| {
            panic!("custom context must bypass catalog resolution")
        })
        .expect("projection");

        assert_eq!(
            projection,
            ClaudeTerminalContextWindowProjection::Mixed(400_000)
        );
    }

    #[test]
    fn slot_projection_fails_closed_on_unknown_and_preserves_catalog_mixed() {
        let slots = [EffectiveCx2ccSlot {
            remote_model_id: "catalog".to_string(),
            custom_context_window: None,
        }];
        let unknown = resolve_effective_slot_context_window(&slots, |_| {
            Ok(ClaudeTerminalContextWindowProjection::Unknown(
                "catalog_unavailable",
            ))
        })
        .expect("unknown projection");
        assert_eq!(
            unknown,
            ClaudeTerminalContextWindowProjection::Unknown("catalog_unavailable")
        );

        let mixed = resolve_effective_slot_context_window(&slots, |_| {
            Ok(ClaudeTerminalContextWindowProjection::Mixed(300_000))
        })
        .expect("mixed projection");
        assert_eq!(mixed, ClaudeTerminalContextWindowProjection::Mixed(300_000));
    }

    #[test]
    fn effective_slots_fail_closed_for_unpaired_or_invalid_custom_context() {
        let settings = crate::settings::AppSettings::default();
        let unpaired = cx2cc_launch_context(crate::providers::ClaudeModels {
            main_context_window: Some(1_000_000),
            ..Default::default()
        });
        assert_eq!(
            effective_cx2cc_slots(&unpaired, &settings),
            Err("custom_context_model_unavailable")
        );

        let invalid = cx2cc_launch_context(crate::providers::ClaudeModels {
            main_model: Some("gpt-5.6-sol".to_string()),
            main_context_window: Some(1_023),
            ..Default::default()
        });
        assert_eq!(
            effective_cx2cc_slots(&invalid, &settings),
            Err("custom_context_invalid")
        );
    }

    #[test]
    fn context_projection_queries_every_provider_for_one_mapper_target() {
        let providers = [
            (1, "provider-one".to_string()),
            (2, "provider-two".to_string()),
        ];

        assert_eq!(
            context_window_candidates(&providers, "gpt-5.6-sol"),
            vec![
                crate::provider_models::ProviderModelContextWindowCandidate {
                    provider_id: 1,
                    provider_uuid: "provider-one".to_string(),
                    remote_model_id: "gpt-5.6-sol".to_string(),
                },
                crate::provider_models::ProviderModelContextWindowCandidate {
                    provider_id: 2,
                    provider_uuid: "provider-two".to_string(),
                    remote_model_id: "gpt-5.6-sol".to_string(),
                },
            ]
        );
    }

    #[test]
    fn build_claude_gateway_base_url_trims_trailing_slash() {
        let url = build_claude_gateway_base_url("http://127.0.0.1:18080/", 12);
        assert_eq!(url, "http://127.0.0.1:18080/claude/_aio/provider/12");
    }

    #[test]
    fn bash_launch_script_includes_cleanup_and_claude_settings() {
        let config_path = Path::new("/tmp/claude_x.json");
        let script_path = Path::new("/tmp/aio_launcher.sh");
        let script = build_claude_launcher_bash_script(config_path, script_path);

        assert!(script.contains("cleanup() {"));
        assert!(script.contains("trap cleanup EXIT INT TERM HUP"));
        assert!(script.contains("claude --settings \"$config_path\""));
        assert!(script.contains("cleanup"));
        assert!(script.contains("exec bash --norc --noprofile"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn bash_launch_script_cleans_sensitive_files_before_shell_handoff() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("claude.json");
        let script_path = temp.path().join("launcher.sh");
        let fake_claude_path = temp.path().join("claude");
        let output_path = temp.path().join("claude-args.txt");

        fs::write(&config_path, "{}").expect("write config");
        fs::write(
            &script_path,
            build_claude_launcher_bash_script(&config_path, &script_path),
        )
        .expect("write script");
        fs::write(
            &fake_claude_path,
            "#!/bin/bash\nprintf '%s\n' \"$@\" > \"$OUTPUT_PATH\"\nexit 0\n",
        )
        .expect("write fake claude");

        let mut perms = fs::metadata(&fake_claude_path)
            .expect("fake claude metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_claude_path, perms).expect("chmod fake claude");

        let path_env = match std::env::var("PATH") {
            Ok(path) => format!("{}:{}", temp.path().display(), path),
            Err(_) => temp.path().display().to_string(),
        };
        let status = Command::new("bash")
            .arg(&script_path)
            .env("PATH", path_env)
            .env("OUTPUT_PATH", &output_path)
            .stdin(Stdio::null())
            .status()
            .expect("run launcher");

        assert!(status.success());
        assert!(!config_path.exists(), "config file should be removed");
        assert!(!script_path.exists(), "launcher script should be removed");

        let claude_args = fs::read_to_string(&output_path).expect("read fake claude args");
        assert!(claude_args.contains("--settings"));
        assert!(claude_args.contains(config_path.to_string_lossy().as_ref()));
    }

    #[test]
    fn powershell_launch_script_includes_cleanup_and_claude_settings() {
        let config_path = Path::new(r"C:\\Temp\\claude_x.json");
        let script_path = Path::new(r"C:\\Temp\\aio_launcher.ps1");
        let script = build_claude_launcher_powershell_script(config_path, script_path);

        assert!(script.contains("Write-Output \"Using provider-specific claude config:\""));
        assert!(script.contains("claude --settings $configPath"));
        assert!(
            script.contains("Remove-Item -LiteralPath $configPath -ErrorAction SilentlyContinue")
        );
        assert!(
            script.contains("Remove-Item -LiteralPath $scriptPath -ErrorAction SilentlyContinue")
        );
    }

    #[test]
    fn powershell_launch_command_uses_expected_flags() {
        let script_path = Path::new(r"C:\\Temp\\aio_launcher.ps1");
        let command = build_powershell_launch_command(script_path);

        assert!(command.starts_with("powershell -NoLogo -NoExit -ExecutionPolicy Bypass -File"));
        assert!(command.contains("\"C:\\\\Temp\\\\aio_launcher.ps1\""));
    }

    #[test]
    fn claude_launch_artifact_paths_use_requested_directory() {
        let dir = Path::new("/tmp/aio-launchers");
        let (config_path, script_path) = claude_launch_artifact_paths(dir, 9, 77, 1234);

        assert_eq!(config_path, dir.join("claude_9_77_1234.json"));
        if cfg!(target_os = "windows") {
            assert_eq!(script_path, dir.join("aio_claude_launcher_9_77_1234.ps1"));
        } else {
            assert_eq!(script_path, dir.join("aio_claude_launcher_9_77_1234.sh"));
        }
    }

    #[test]
    fn detects_claude_launcher_artifact_file_names() {
        assert!(is_claude_launcher_artifact_file_name("claude_1_2_3.json"));
        assert!(is_claude_launcher_artifact_file_name(
            "aio_claude_launcher_1_2_3.sh"
        ));
        assert!(!is_claude_launcher_artifact_file_name("providers.json"));
    }
}
