//! Thin IPC boundary; generated nodes never receive frontend-supplied raw JSON.
use crate::app::native_gateway_service as service;
use crate::app_state::DbInitState;
use crate::domain::native_gateway::*;

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_models_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
) -> Result<GatewayModelsSnapshot, String> {
    service::native_gateway_models_get(app, db_state, provider_id, provider_uuid).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_models_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
    provider_uuid: String,
    expected_revision: String,
    models: Vec<NativeModelSpec>,
) -> Result<GatewayModelsSnapshot, String> {
    service::native_gateway_models_set(
        app,
        db_state,
        provider_id,
        provider_uuid,
        expected_revision,
        models,
    )
    .await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_catalog_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
) -> Result<GatewayCatalogPreview, String> {
    service::native_gateway_catalog_preview(app, db_state, target_id).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_apply(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: GatewayLifecycleInput,
) -> Result<GatewayMutationResult, String> {
    service::native_gateway_mutate(app, db_state, input, false).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_remove(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: GatewayLifecycleInput,
) -> Result<GatewayMutationResult, String> {
    service::native_gateway_mutate(app, db_state, input, true).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_import_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    native_key: String,
) -> Result<GatewayImportPreview, String> {
    service::native_gateway_import_preview(app, db_state, target_id, native_key).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_gateway_import_confirm(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: GatewayImportConfirmInput,
) -> Result<Vec<crate::providers::ProviderSummary>, String> {
    service::native_gateway_import_confirm(app, db_state, input).await
}
