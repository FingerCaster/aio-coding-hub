//! Stable IPC boundary for the managed external Codex retry gateway.

use crate::app::codex_retry_gateway_service;
use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayApplyCommitRequest, CodexRetryGatewayCommitValidation,
    CodexRetryGatewayDetailsSession, CodexRetryGatewayEnablePlan,
    CodexRetryGatewayGenerationRequest, CodexRetryGatewayNodeStatus,
    CodexRetryGatewayRevokeDetailsSessionRequest, CodexRetryGatewaySetEnabledRequest,
    CodexRetryGatewaySetEnabledResult, CodexRetryGatewaySetNodeOverrideRequest,
    CodexRetryGatewayStatus, CodexRetryGatewayUninstallRequest, CodexRetryGatewayUpdateCandidate,
    CodexRetryGatewayValidateCommitRequest,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_status(
    app: tauri::AppHandle,
) -> Result<CodexRetryGatewayStatus, String> {
    codex_retry_gateway_service::status(&app)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_enable_plan(
    app: tauri::AppHandle,
) -> Result<CodexRetryGatewayEnablePlan, String> {
    codex_retry_gateway_service::enable_plan(&app)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_set_enabled(
    app: tauri::AppHandle,
    request: CodexRetryGatewaySetEnabledRequest,
) -> Result<CodexRetryGatewaySetEnabledResult, String> {
    codex_retry_gateway_service::set_enabled(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_check_update(
    app: tauri::AppHandle,
) -> Result<Option<CodexRetryGatewayUpdateCandidate>, String> {
    codex_retry_gateway_service::check_update(&app)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_validate_commit(
    app: tauri::AppHandle,
    request: CodexRetryGatewayValidateCommitRequest,
) -> Result<CodexRetryGatewayCommitValidation, String> {
    codex_retry_gateway_service::validate_commit(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_apply_commit(
    app: tauri::AppHandle,
    request: CodexRetryGatewayApplyCommitRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    codex_retry_gateway_service::apply_commit(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_set_node_override(
    app: tauri::AppHandle,
    request: CodexRetryGatewaySetNodeOverrideRequest,
) -> Result<CodexRetryGatewayNodeStatus, String> {
    codex_retry_gateway_service::set_node_override(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_retry(
    app: tauri::AppHandle,
    request: CodexRetryGatewayGenerationRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    codex_retry_gateway_service::retry(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_uninstall(
    app: tauri::AppHandle,
    request: CodexRetryGatewayUninstallRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    codex_retry_gateway_service::uninstall(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_create_details_session(
    app: tauri::AppHandle,
) -> Result<CodexRetryGatewayDetailsSession, String> {
    codex_retry_gateway_service::details_session(&app)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_revoke_details_session(
    request: CodexRetryGatewayRevokeDetailsSessionRequest,
) -> Result<(), String> {
    codex_retry_gateway_service::revoke_details_session(request)
        .await
        .map_err(Into::into)
}
