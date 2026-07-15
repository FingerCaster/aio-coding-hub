//! Stable IPC boundary for the managed external Codex retry gateway.

use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayApplyCommitRequest, CodexRetryGatewayCommitValidation,
    CodexRetryGatewayDetailsSession, CodexRetryGatewayEnablePlan,
    CodexRetryGatewayGenerationRequest, CodexRetryGatewayNodeStatus,
    CodexRetryGatewaySetEnabledRequest, CodexRetryGatewaySetNodeOverrideRequest,
    CodexRetryGatewayStatus, CodexRetryGatewayUninstallRequest, CodexRetryGatewayUpdateCandidate,
    CodexRetryGatewayValidateCommitRequest,
};

fn runtime_not_ready<T>() -> Result<T, String> {
    Err("CODEX_RETRY_GATEWAY_NOT_READY: runtime implementation is not installed".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_status() -> Result<CodexRetryGatewayStatus, String> {
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_enable_plan() -> Result<CodexRetryGatewayEnablePlan, String>
{
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_set_enabled(
    request: CodexRetryGatewaySetEnabledRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    let _ = request;
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_check_update(
) -> Result<Option<CodexRetryGatewayUpdateCandidate>, String> {
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_validate_commit(
    request: CodexRetryGatewayValidateCommitRequest,
) -> Result<CodexRetryGatewayCommitValidation, String> {
    let _ = request;
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_apply_commit(
    request: CodexRetryGatewayApplyCommitRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    let _ = request;
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_set_node_override(
    request: CodexRetryGatewaySetNodeOverrideRequest,
) -> Result<CodexRetryGatewayNodeStatus, String> {
    let _ = request;
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_retry(
    request: CodexRetryGatewayGenerationRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    let _ = request;
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_uninstall(
    request: CodexRetryGatewayUninstallRequest,
) -> Result<CodexRetryGatewayStatus, String> {
    let _ = request;
    runtime_not_ready()
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn codex_retry_gateway_create_details_session(
) -> Result<CodexRetryGatewayDetailsSession, String> {
    runtime_not_ready()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn foundation_commands_fail_closed_until_runtime_is_wired() {
        let error = codex_retry_gateway_status().await.unwrap_err();
        assert!(error.starts_with("CODEX_RETRY_GATEWAY_NOT_READY:"));
    }
}
