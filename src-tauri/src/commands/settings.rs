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
pub(crate) async fn settings_update_channel_set(
    app: tauri::AppHandle,
    channel: crate::settings::UpdateChannel,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<SettingsView, String> {
    let app_for_discard = app.clone();
    let view = settings_service::settings_update_channel_set(app, channel, confirm).await?;
    let stale_channel = match channel {
        crate::settings::UpdateChannel::Stable => crate::settings::UpdateChannel::Beta,
        crate::settings::UpdateChannel::Beta => crate::settings::UpdateChannel::Stable,
    };
    let discarded = crate::commands::desktop::discard_updater_resources_for_channel(
        &app_for_discard,
        stale_channel,
    );
    tracing::info!(
        update_channel = %channel,
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
