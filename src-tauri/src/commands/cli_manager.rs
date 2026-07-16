//! Usage: CLI environment / integration related Tauri commands.

use crate::{
    blocking, claude_hooks, claude_settings, cli_manager, codex_config, codex_model_catalog,
    codex_provider_sync, gemini_config,
};

async fn run_locked_codex_mutation<T>(
    label: &'static str,
    operation: impl FnOnce() -> crate::shared::error::AppResult<T> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    blocking::run(label, operation).await.map_err(Into::into)
}

pub(crate) async fn run_codex_config_stage<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    label: &'static str,
    operation: impl FnOnce() -> crate::shared::error::AppResult<codex_config::CodexConfigMutationStage>
        + Send
        + 'static,
) -> Result<codex_config::CodexConfigState, String> {
    let _gateway_lifecycle = crate::app::gateway_lifecycle_lock::lock().await;
    let mut staged = blocking::run(label, operation)
        .await
        .map_err(String::from)?;
    let Some(transaction) = staged.transaction.take() else {
        return Ok(staged.state);
    };
    let Some(expected_record) = transaction.verification_record().cloned() else {
        commit_config_stage(transaction).await?;
        crate::app::codex_retry_gateway_service::emit_current_status(&app).await;
        return Ok(staged.state);
    };

    let paths =
        match crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(&app) {
            Ok(paths) => paths,
            Err(error) => {
                return rollback_failed_config_stage(transaction, error).await;
            }
        };
    if let Err(error) = crate::infra::codex_retry_gateway::verify_managed_provider_projection(
        &paths,
        &expected_record,
    )
    .await
    {
        return rollback_failed_config_stage(transaction, error).await;
    }

    commit_config_stage(transaction).await?;
    crate::app::codex_retry_gateway_service::emit_current_status(&app).await;
    Ok(staged.state)
}

async fn commit_config_stage(
    transaction: codex_config::CodexConfigMutationTransaction,
) -> Result<(), String> {
    blocking::run("cli_manager_codex_config_commit", move || {
        transaction.commit()
    })
    .await
    .map_err(String::from)
}

async fn rollback_failed_config_stage(
    transaction: codex_config::CodexConfigMutationTransaction,
    cause: crate::shared::error::AppError,
) -> Result<codex_config::CodexConfigState, String> {
    match blocking::run("cli_manager_codex_config_rollback", move || {
        transaction.rollback()
    })
    .await
    {
        Ok(()) => Err(cause.to_string()),
        Err(rollback_error) => Err(format!(
            "CODEX_CONFIG_MANAGED_ROLLBACK_FAILED: {cause}; rollback error: {rollback_error}"
        )),
    }
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
        codex_model_catalog::codex_model_catalog_get(&app)
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
) -> Result<codex_config::CodexConfigState, String> {
    let app_for_mutation = app.clone();
    run_codex_config_stage(app, "cli_manager_codex_config_set", move || {
        codex_config::codex_config_set_staged(&app_for_mutation, patch)
    })
    .await
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
    let app_for_mutation = app.clone();
    run_codex_config_stage(app, "cli_manager_codex_config_toml_set", move || {
        codex_config::codex_config_toml_set_raw_staged(&app_for_mutation, toml)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cli_manager_codex_provider_sync(
    app: tauri::AppHandle,
) -> Result<codex_provider_sync::CodexProviderSyncResult, String> {
    run_locked_codex_mutation("cli_manager_codex_provider_sync", move || {
        codex_provider_sync::codex_provider_sync_current(&app, "manual")
    })
    .await
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

#[cfg(test)]
mod tests {
    use super::run_locked_codex_mutation;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn codex_config_mutations_wait_for_gateway_lifecycle_lock() {
        let first_guard = crate::app::gateway_lifecycle_lock::lock().await;
        let entered = Arc::new(AtomicBool::new(false));
        let entered_for_task = entered.clone();
        let task = tokio::spawn(async move {
            run_locked_codex_mutation("test_codex_config_lifecycle", move || {
                entered_for_task.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
            .expect("mutation should complete");
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!entered.load(Ordering::SeqCst));
        drop(first_guard);
        tokio::time::timeout(Duration::from_millis(250), task)
            .await
            .expect("mutation should enter after lifecycle lock is released")
            .expect("mutation task should not panic");
        assert!(entered.load(Ordering::SeqCst));
    }
}
