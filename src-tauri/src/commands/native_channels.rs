//! Thin, typed IPC for consumer-owned AIO channel bindings.
use crate::app::native_channel_service as service;
use crate::app_state::DbInitState;
use crate::domain::native_channels::*;
use crate::domain::native_gateway::{GatewayModelsSnapshot, NativeModelSpec};
use crate::shared::gateway_protocol::GatewayProtocol;

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_channel_models_discover(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    provider_id: i64,
    provider_uuid: String,
    protocol: GatewayProtocol,
) -> Result<crate::app::native_channel_discovery::ChannelModelDiscovery, String> {
    crate::app::native_channel_discovery::native_channel_models_discover(
        app,
        db_state,
        target_id,
        provider_id,
        provider_uuid,
        protocol,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_channel_catalog_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
) -> Result<ChannelCatalog, String> {
    service::native_channel_catalog_preview(app, db_state, target_id).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_channel_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ChannelLifecycleInput,
) -> Result<ChannelPreview, String> {
    service::native_channel_preview(app, db_state, input).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_channel_apply(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ChannelLifecycleInput,
) -> Result<ChannelMutationResult, String> {
    service::native_channel_apply(app, db_state, input).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_channel_models_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    provider_id: i64,
    provider_uuid: String,
    protocol: GatewayProtocol,
) -> Result<GatewayModelsSnapshot, String> {
    service::native_channel_models_get(
        app,
        db_state,
        target_id,
        provider_id,
        provider_uuid,
        protocol,
    )
    .await
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn native_channel_models_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    provider_id: i64,
    provider_uuid: String,
    protocol: GatewayProtocol,
    expected_revision: String,
    models: Vec<NativeModelSpec>,
) -> Result<GatewayModelsSnapshot, String> {
    service::native_channel_models_set(
        app,
        db_state,
        target_id,
        provider_id,
        provider_uuid,
        protocol,
        expected_revision,
        models,
    )
    .await
}
