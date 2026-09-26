//! Usage: CLI environment / integration related Tauri commands.

use crate::{
    blocking, claude_hooks, claude_settings, cli_manager, codex_config, codex_model_catalog,
    codex_provider_sync, gemini_config, grok_config,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_pi_info_get(
    app: tauri::AppHandle,
) -> Result<cli_manager::SimpleCliInfo, String> {
    blocking::run("cli_manager_pi_info_get", move || {
        cli_manager::native_cli_info_get(&app, "pi")
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_omp_info_get(
    app: tauri::AppHandle,
) -> Result<cli_manager::SimpleCliInfo, String> {
    blocking::run("cli_manager_omp_info_get", move || {
        cli_manager::native_cli_info_get(&app, "omp")
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_claude_info_get(
    app: tauri::AppHandle,
) -> Result<cli_manager::ClaudeCliInfo, String> {
    blocking::run("cli_manager_claude_info_get", move || {
        cli_manager::claude_info_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_info_get(
    app: tauri::AppHandle,
) -> Result<cli_manager::SimpleCliInfo, String> {
    blocking::run("cli_manager_codex_info_get", move || {
        cli_manager::codex_info_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_model_catalog_get(
    app: tauri::AppHandle,
) -> Result<codex_model_catalog::CodexModelCatalogState, String> {
    blocking::run("cli_manager_codex_model_catalog_get", move || {
        let _lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
        crate::codex_model_catalog::managed::sync_current_locked(&app)?;
        codex_model_catalog::codex_model_catalog_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_model_context_candidates_get(
    app: tauri::AppHandle,
) -> Result<codex_model_catalog::CodexModelContextCandidatesState, String> {
    blocking::run(
        "cli_manager_codex_model_context_candidates_get",
        move || {
            let _lifecycle = crate::codex_managed_profiles::lock_profile_lifecycle();
            codex_model_catalog::codex_model_context_candidates_get_locked(&app)
        },
    )
    .await
    .map_err(Into::into)
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexManagedCatalogUpgradeRequest {
    pub(crate) disable_invalid_rules: bool,
    pub(crate) model_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexManagedCatalogUpgradeStatus {
    Inactive,
    Applied,
    Blocked,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexManagedCatalogUpgradeResult {
    pub(crate) status: CodexManagedCatalogUpgradeStatus,
    pub(crate) settings: Option<crate::app::settings_service::SettingsView>,
    pub(crate) invalid_rules:
        Vec<crate::codex_model_catalog::managed::CodexManagedCatalogInvalidRule>,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_managed_catalog_upgrade(
    app: tauri::AppHandle,
    request: CodexManagedCatalogUpgradeRequest,
) -> Result<CodexManagedCatalogUpgradeResult, String> {
    blocking::run("cli_manager_codex_managed_catalog_upgrade", move || {
        let mode = if request.disable_invalid_rules {
            crate::app::settings_service::CodexManagedCatalogUpgradeMode::DisableInvalid {
                model_ids: request.model_ids,
            }
        } else if !request.model_ids.is_empty() {
            return Err(crate::shared::error::AppError::new(
                "CODEX_MANAGED_CATALOG_UPGRADE_STALE",
                "an ordinary catalog upgrade cannot select context rules",
            ));
        } else {
            crate::app::settings_service::CodexManagedCatalogUpgradeMode::Apply
        };
        let outcome = crate::app::settings_service::codex_managed_catalog_upgrade_sync(&app, mode)?;
        Ok(match outcome {
            crate::app::settings_service::CodexManagedCatalogUpgradeOutcome::Inactive => {
                CodexManagedCatalogUpgradeResult {
                    status: CodexManagedCatalogUpgradeStatus::Inactive,
                    settings: None,
                    invalid_rules: Vec::new(),
                }
            }
            crate::app::settings_service::CodexManagedCatalogUpgradeOutcome::Applied(settings) => {
                CodexManagedCatalogUpgradeResult {
                    status: CodexManagedCatalogUpgradeStatus::Applied,
                    settings: Some(*settings),
                    invalid_rules: Vec::new(),
                }
            }
            crate::app::settings_service::CodexManagedCatalogUpgradeOutcome::Blocked(
                invalid_rules,
            ) => CodexManagedCatalogUpgradeResult {
                status: CodexManagedCatalogUpgradeStatus::Blocked,
                settings: None,
                invalid_rules,
            },
        })
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_config_get(
    app: tauri::AppHandle,
) -> Result<codex_config::CodexConfigState, String> {
    blocking::run("cli_manager_codex_config_get", move || {
        codex_config::codex_config_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_config_set(
    app: tauri::AppHandle,
    patch: codex_config::CodexConfigPatch,
    sync_history: Option<bool>,
) -> Result<codex_config::CodexConfigState, String> {
    blocking::run("cli_manager_codex_config_set", move || {
        codex_config::codex_config_set_with_options(&app, patch, sync_history.unwrap_or(false))
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_config_toml_get(
    app: tauri::AppHandle,
) -> Result<codex_config::CodexConfigTomlState, String> {
    blocking::run("cli_manager_codex_config_toml_get", move || {
        codex_config::codex_config_toml_get_raw(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_config_toml_validate(
    toml: String,
) -> Result<codex_config::CodexConfigTomlValidationResult, String> {
    blocking::run("cli_manager_codex_config_toml_validate", move || {
        codex_config::codex_config_toml_validate_raw(toml)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_config_toml_set(
    app: tauri::AppHandle,
    toml: String,
) -> Result<codex_config::CodexConfigState, String> {
    blocking::run("cli_manager_codex_config_toml_set", move || {
        codex_config::codex_config_toml_set_raw(&app, toml)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_provider_sync(
    app: tauri::AppHandle,
) -> Result<codex_provider_sync::CodexProviderSyncResult, String> {
    blocking::run("cli_manager_codex_provider_sync", move || {
        codex_provider_sync::codex_provider_sync_current(&app, "manual")
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_gemini_info_get(
    app: tauri::AppHandle,
) -> Result<cli_manager::SimpleCliInfo, String> {
    blocking::run("cli_manager_gemini_info_get", move || {
        cli_manager::gemini_info_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_gemini_config_get(
    app: tauri::AppHandle,
) -> Result<gemini_config::GeminiConfigState, String> {
    blocking::run("cli_manager_gemini_config_get", move || {
        gemini_config::gemini_config_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_gemini_config_set(
    app: tauri::AppHandle,
    patch: gemini_config::GeminiConfigPatch,
) -> Result<gemini_config::GeminiConfigState, String> {
    blocking::run("cli_manager_gemini_config_set", move || {
        gemini_config::gemini_config_set(&app, patch)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_grok_info_get(
    app: tauri::AppHandle,
) -> Result<cli_manager::SimpleCliInfo, String> {
    blocking::run("cli_manager_grok_info_get", move || {
        cli_manager::simple_cli_info_get(&app, "grok")
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_grok_config_get(
    app: tauri::AppHandle,
) -> Result<grok_config::GrokConfigState, String> {
    blocking::run("cli_manager_grok_config_get", move || {
        grok_config::get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_grok_config_set(
    app: tauri::AppHandle,
    preferences: grok_config::GrokProxyPreferences,
) -> Result<grok_config::GrokConfigState, String> {
    blocking::run("cli_manager_grok_config_set", move || {
        crate::cli_proxy::set_grok_preferences(&app, preferences)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_claude_env_set(
    app: tauri::AppHandle,
    mcp_timeout_ms: Option<u64>,
    disable_error_reporting: bool,
) -> Result<cli_manager::ClaudeEnvState, String> {
    blocking::run("cli_manager_claude_env_set", move || {
        cli_manager::claude_env_set(&app, mcp_timeout_ms, disable_error_reporting)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_claude_settings_get(
    app: tauri::AppHandle,
) -> Result<claude_settings::ClaudeSettingsState, String> {
    blocking::run("cli_manager_claude_settings_get", move || {
        claude_settings::claude_settings_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_claude_settings_set(
    app: tauri::AppHandle,
    patch: claude_settings::ClaudeSettingsPatch,
) -> Result<claude_settings::ClaudeSettingsState, String> {
    blocking::run("cli_manager_claude_settings_set", move || {
        claude_settings::claude_settings_set(&app, patch)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_claude_hooks_get(
    app: tauri::AppHandle,
) -> Result<claude_hooks::ClaudeHooksState, String> {
    blocking::run("cli_manager_claude_hooks_get", move || {
        claude_hooks::claude_hooks_get(&app)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_claude_hooks_set(
    app: tauri::AppHandle,
    input: claude_hooks::ClaudeHooksSetInput,
) -> Result<claude_hooks::ClaudeHooksState, String> {
    blocking::run("cli_manager_claude_hooks_set", move || {
        claude_hooks::claude_hooks_set(&app, input)
    })
    .await
    .map_err(Into::into)
}
