//! Stable IPC boundary for the managed external Codex retry gateway.

use crate::infra::codex_retry_gateway::{
    apply_selected_commit, build_enable_plan, create_details_session, current_status,
    retry_runtime_recovery, runtime_update_candidate, set_runtime_enabled,
    set_runtime_node_override, uninstall_runtime, validate_selected_commit,
    CodexRetryGatewayApplyCommitRequest, CodexRetryGatewayCommitValidation,
    CodexRetryGatewayDetailsSession, CodexRetryGatewayEnablePlan,
    CodexRetryGatewayGenerationRequest, CodexRetryGatewayNodeStatus,
    CodexRetryGatewaySetEnabledRequest, CodexRetryGatewaySetNodeOverrideRequest,
    CodexRetryGatewayStatus, CodexRetryGatewayUninstallRequest, CodexRetryGatewayUpdateCandidate,
    CodexRetryGatewayValidateCommitRequest,
};

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_status(
    app: tauri::AppHandle,
) -> Result<CodexRetryGatewayStatus, String> {
    current_status(&app).await.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_enable_plan(
    app: tauri::AppHandle,
) -> Result<CodexRetryGatewayEnablePlan, String> {
    build_enable_plan(&app).await.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_set_enabled(
    app: tauri::AppHandle,
    request: CodexRetryGatewaySetEnabledRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    set_runtime_enabled(&app, request).await.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_check_update(
    app: tauri::AppHandle,
) -> Result<Option<CodexRetryGatewayUpdateCandidate>, String> {
    runtime_update_candidate(&app).await.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_validate_commit(
    request: CodexRetryGatewayValidateCommitRequest,
) -> Result<CodexRetryGatewayCommitValidation, String> {
    validate_selected_commit(request).await.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_apply_commit(
    app: tauri::AppHandle,
    request: CodexRetryGatewayApplyCommitRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    apply_selected_commit(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_set_node_override(
    app: tauri::AppHandle,
    request: CodexRetryGatewaySetNodeOverrideRequest,
) -> Result<CodexRetryGatewayNodeStatus, String> {
    set_runtime_node_override(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_retry(
    app: tauri::AppHandle,
    request: CodexRetryGatewayGenerationRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    retry_runtime_recovery(&app, request)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_uninstall(
    app: tauri::AppHandle,
    request: CodexRetryGatewayUninstallRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    uninstall_runtime(&app, request).await.map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_create_details_session(
    app: tauri::AppHandle,
) -> Result<CodexRetryGatewayDetailsSession, String> {
    create_details_session(&app).await.map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::codex_retry_gateway_validate_commit;
    use crate::infra::codex_retry_gateway::CodexRetryGatewayValidateCommitRequest;

    #[tokio::test]
    async fn validate_commit_command_fails_closed_for_non_sha_input() {
        let validation =
            codex_retry_gateway_validate_commit(CodexRetryGatewayValidateCommitRequest {
                commit: "main".to_string(),
            })
            .await
            .expect("validation");
        assert!(validation.error.is_some());
    }
}
