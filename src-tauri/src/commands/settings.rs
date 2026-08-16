//! Usage: Thin IPC wrappers for settings commands.

use crate::app::settings_service;
use crate::app_state::DbInitState;
use crate::shared::ipc_confirm::RiskyIpcConfirm;

pub(crate) use crate::app::settings_service::{
    CircuitBreakerNoticeUpdate, CodexSessionIdCompletionUpdate, GatewayRectifierSettingsUpdate,
    SettingsMutationResult, SettingsPatch, SettingsUpdate, SettingsView,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_get(app: tauri::AppHandle) -> Result<SettingsView, String> {
    settings_service::settings_get(app).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    update: SettingsUpdate,
) -> Result<SettingsMutationResult, String> {
    settings_service::settings_set_impl(app, db_state.inner(), update).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_patch(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    patch: SettingsPatch,
) -> Result<SettingsMutationResult, String> {
    settings_service::settings_patch_impl(app, db_state.inner(), patch).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_codex_gpt56_372k_context_set(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<SettingsView, String> {
    settings_service::settings_codex_gpt56_372k_context_set(app, enabled).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_update_channel_set(
    app: tauri::AppHandle,
    channel: crate::settings::UpdateChannel,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<SettingsView, String> {
    let app_for_cleanup = app.clone();
    let view = settings_service::settings_update_channel_set(app, channel, confirm).await?;
    let discarded = crate::commands::desktop::discard_stale_updater_resources(&app_for_cleanup);
    tracing::info!(
        requested_update_channel = %channel,
        discarded_resources = discarded,
        "update channel settings committed"
    );
    Ok(view)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_gateway_rectifier_set(
    app: tauri::AppHandle,
    update: GatewayRectifierSettingsUpdate,
) -> Result<SettingsView, String> {
    settings_service::settings_gateway_rectifier_set(app, update).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_circuit_breaker_notice_set(
    app: tauri::AppHandle,
    update: CircuitBreakerNoticeUpdate,
) -> Result<SettingsView, String> {
    settings_service::settings_circuit_breaker_notice_set(app, update).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn settings_codex_session_id_completion_set(
    app: tauri::AppHandle,
    update: CodexSessionIdCompletionUpdate,
) -> Result<SettingsView, String> {
    settings_service::settings_codex_session_id_completion_set(app, update).await
}
