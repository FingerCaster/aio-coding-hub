//! Shared contracts for the managed external Codex retry gateway.

use crate::shared::error::AppResult;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

pub(crate) const CODEX_RETRY_GATEWAY_REPOSITORY: &str = "nonononull/codex-retry-gateway";
pub(crate) const CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT: &str =
    "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2";
pub(crate) const CODEX_RETRY_GATEWAY_DEFAULT_PORT: u16 = 4610;
#[allow(dead_code)]
pub(crate) const CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME: &str = "codex-retry-gateway:status";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRouteMode {
    #[default]
    Unproxied,
    DirectAio,
    Guarded,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayRuntimePhase {
    #[default]
    Disabled,
    Preparing,
    Starting,
    Guarded,
    BypassedRecovering,
    RecoveryPaused,
    Updating,
    Stopping,
    CleanupNeeded,
    Uninstalling,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayTrustState {
    #[default]
    Unavailable,
    AioReviewedRecommendation,
    OfficialMainUnreviewed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayNodeResolutionSource {
    #[default]
    Unavailable,
    CodexSibling,
    AioDiscovery,
    ProcessPath,
    ManualOverride,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayProcessPhase {
    #[default]
    Stopped,
    Starting,
    Healthy,
    Unhealthy,
    OwnershipMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayErrorCategory {
    SourceResolution,
    SourceArchive,
    NodeMissing,
    NodeUnsupported,
    PortConflict,
    OwnershipMismatch,
    HealthTimeout,
    RouteApply,
    RouteVerify,
    ProviderSync,
    BridgeSession,
    UpdateRollback,
    Cleanup,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayError {
    pub code: String,
    pub category: CodexRetryGatewayErrorCategory,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayNodeStatus {
    pub available: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub source: CodexRetryGatewayNodeResolutionSource,
    pub error: Option<CodexRetryGatewayError>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayProcessStatus {
    pub phase: CodexRetryGatewayProcessPhase,
    pub owned: bool,
    pub healthy: bool,
    pub process_id: Option<u32>,
    pub listener: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayUpdateCandidate {
    pub commit: String,
    pub current_commit: Option<String>,
    pub previous_commit: Option<String>,
    pub rollback_commit: Option<String>,
    pub official_main_commit: String,
    pub commits_ahead: Option<u32>,
    pub summary: Option<String>,
    pub trust_state: CodexRetryGatewayTrustState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexProviderSyncPlan {
    pub current_provider: Option<String>,
    pub target_provider: String,
    pub change_required: bool,
    pub codex_must_be_closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayStatus {
    pub generation: u64,
    pub desired_enabled: bool,
    pub runtime_phase: CodexRetryGatewayRuntimePhase,
    pub route_mode: CodexRouteMode,
    pub cli_proxy_enabled: bool,
    pub cli_proxy_applied: bool,
    pub effective_port: Option<u16>,
    pub repository: String,
    pub license: Option<String>,
    pub selected_commit: String,
    pub active_commit: Option<String>,
    pub previous_commit: Option<String>,
    pub recommended_commit: String,
    pub trust_state: CodexRetryGatewayTrustState,
    pub node_status: CodexRetryGatewayNodeStatus,
    pub process_status: CodexRetryGatewayProcessStatus,
    pub update_candidate: Option<CodexRetryGatewayUpdateCandidate>,
    pub wsl_codex_unprotected: bool,
    pub last_error: Option<CodexRetryGatewayError>,
    pub details_available: bool,
    pub operation_pending: bool,
}

impl Default for CodexRetryGatewayStatus {
    fn default() -> Self {
        Self {
            generation: 0,
            desired_enabled: false,
            runtime_phase: CodexRetryGatewayRuntimePhase::Disabled,
            route_mode: CodexRouteMode::Unproxied,
            cli_proxy_enabled: false,
            cli_proxy_applied: false,
            effective_port: None,
            repository: CODEX_RETRY_GATEWAY_REPOSITORY.to_string(),
            license: None,
            selected_commit: CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string(),
            active_commit: None,
            previous_commit: None,
            recommended_commit: CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string(),
            trust_state: CodexRetryGatewayTrustState::AioReviewedRecommendation,
            node_status: CodexRetryGatewayNodeStatus::default(),
            process_status: CodexRetryGatewayProcessStatus::default(),
            update_candidate: None,
            wsl_codex_unprotected: false,
            last_error: None,
            details_available: false,
            operation_pending: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayEnablePlan {
    pub generation: u64,
    pub selected_commit: String,
    pub trust_state: CodexRetryGatewayTrustState,
    pub first_download_required: bool,
    pub unreviewed_commit: bool,
    pub cli_proxy_enable_required: bool,
    pub provider_sync: CodexProviderSyncPlan,
    pub node_status: CodexRetryGatewayNodeStatus,
    pub preferred_port: u16,
    pub wsl_codex_unprotected: bool,
}

impl Default for CodexRetryGatewayEnablePlan {
    fn default() -> Self {
        Self {
            generation: 0,
            selected_commit: CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.to_string(),
            trust_state: CodexRetryGatewayTrustState::AioReviewedRecommendation,
            first_download_required: false,
            unreviewed_commit: false,
            cli_proxy_enable_required: false,
            provider_sync: CodexProviderSyncPlan {
                target_provider: "aio".to_string(),
                ..CodexProviderSyncPlan::default()
            },
            node_status: CodexRetryGatewayNodeStatus::default(),
            preferred_port: CODEX_RETRY_GATEWAY_DEFAULT_PORT,
            wsl_codex_unprotected: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewayEnableConfirmation {
    pub accepted_first_download: bool,
    pub accepted_unreviewed_commit: bool,
    pub accepted_cli_proxy_enable: bool,
    pub accepted_provider_sync: bool,
    pub accepted_wsl_unprotected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewaySetEnabledRequest {
    pub enabled: bool,
    pub plan_generation: u64,
    pub confirmation: CodexRetryGatewayEnableConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewayValidateCommitRequest {
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayCommitValidation {
    pub requested_commit: String,
    pub canonical_commit: Option<String>,
    pub official_main_commit: Option<String>,
    pub official_main_ancestor: bool,
    pub trust_state: Option<CodexRetryGatewayTrustState>,
    pub summary: Option<String>,
    pub error: Option<CodexRetryGatewayError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewayApplyCommitRequest {
    pub plan_generation: u64,
    pub commit: String,
    pub accepted_update: bool,
    pub accepted_unreviewed_commit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewaySetNodeOverrideRequest {
    pub generation: u64,
    pub executable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewayGenerationRequest {
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryGatewayUninstallRequest {
    pub generation: u64,
    pub confirmed_data_removal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub(crate) struct CodexRetryGatewayDetailsSession {
    pub generation: u64,
    pub iframe_url: String,
    pub browser_url: String,
    pub expires_at_ms: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AioGatewayOrigin {
    pub url: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExternalGatewayOrigin {
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayOperationKind {
    Enable,
    DisableGateway,
    DisableCliProxy,
    Update,
    Recover,
    Uninstall,
    Startup,
    Shutdown,
    ProviderModeChange,
    ExternalRestore,
}

pub(crate) const CODEX_RETRY_GATEWAY_ROUTE_TRANSITION_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayRouteSnapshotRoot {
    CodexHome,
    CliProxyState,
    GatewayManagedState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewayRouteSnapshot {
    pub root: CodexRetryGatewayRouteSnapshotRoot,
    pub root_path_sha256: String,
    pub target_rel: String,
    pub existed: bool,
    pub backup_rel: Option<String>,
    pub backup_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewayRouteTransition {
    pub schema_version: u32,
    pub operation_id: String,
    pub operation_kind: CodexRetryGatewayOperationKind,
    pub prior_generation: u64,
    pub target_generation: u64,
    pub prior_mode: CodexRouteMode,
    pub target_mode: CodexRouteMode,
    pub prior_canonical_config_sha256: String,
    pub prior_live_config_sha256: String,
    pub canonical_config_sha256: String,
    pub live_config_sha256: String,
    pub source_commit: Option<String>,
    pub process_should_run: bool,
    pub snapshots: Vec<CodexRetryGatewayRouteSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexRetryGatewayRouteCallbackReason {
    ProcessUnhealthy,
    ExternalRestore,
    UpdateBypass,
    AppExit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewayRouteCallbackRequest {
    pub generation: u64,
    pub reason: CodexRetryGatewayRouteCallbackReason,
}

#[allow(dead_code)]
pub(crate) trait CodexRetryGatewayTransitionStore: Send + Sync {
    fn load_pending(&self) -> AppResult<Option<CodexRetryGatewayRouteTransition>>;
    fn prepare(
        &self,
        transition: &CodexRetryGatewayRouteTransition,
        snapshot_bytes: &[Option<Vec<u8>>],
    ) -> AppResult<()>;
    fn read_snapshot(
        &self,
        transition: &CodexRetryGatewayRouteTransition,
        snapshot: &CodexRetryGatewayRouteSnapshot,
    ) -> AppResult<Vec<u8>>;
    fn commit(&self, operation_id: &str, generation: u64) -> AppResult<()>;
    fn clear(&self, operation_id: &str) -> AppResult<()>;
}

pub(crate) type CodexRetryGatewayLifecycleFuture =
    Pin<Box<dyn Future<Output = AppResult<()>> + Send + 'static>>;

pub(crate) type CodexRetryGatewayStatusFuture =
    Pin<Box<dyn Future<Output = AppResult<CodexRetryGatewayStatus>> + Send + 'static>>;

pub(crate) trait CodexRetryGatewayLifecycleCallback: Send + Sync {
    fn request_gateway_disable(
        &self,
        request: CodexRetryGatewayRouteCallbackRequest,
    ) -> CodexRetryGatewayLifecycleFuture;

    fn current_gateway_status(&self) -> CodexRetryGatewayStatusFuture {
        Box::pin(async {
            Err("CODEX_RETRY_GATEWAY_BRIDGE_STATUS_UNAVAILABLE: authoritative status callback is unavailable"
                .into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_is_a_canonical_full_sha() {
        assert_eq!(
            CODEX_RETRY_GATEWAY_STATUS_EVENT_NAME,
            "codex-retry-gateway:status"
        );
        assert_eq!(CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT.len(), 40);
        assert!(CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    #[test]
    fn typed_origins_keep_aio_and_external_targets_distinct() {
        let aio = AioGatewayOrigin {
            url: "http://127.0.0.1:37123/v1".to_string(),
        };
        let external = ExternalGatewayOrigin {
            url: "http://127.0.0.1:4610/v1".to_string(),
        };
        assert_ne!(aio.url, external.url);
    }

    #[test]
    fn route_mode_uses_stable_snake_case_wire_values() {
        assert_eq!(
            serde_json::to_string(&CodexRouteMode::DirectAio).unwrap(),
            "\"direct_aio\""
        );
    }

    #[test]
    fn disabled_status_fixture_is_truthful() {
        let status = CodexRetryGatewayStatus::default();
        assert!(!status.desired_enabled);
        assert_eq!(status.route_mode, CodexRouteMode::Unproxied);
        assert_eq!(
            status.runtime_phase,
            CodexRetryGatewayRuntimePhase::Disabled
        );
        assert!(!status.details_available);
        assert_eq!(status.effective_port, None);
    }

    #[test]
    fn enable_confirmation_defaults_to_no_consent() {
        let confirmation = CodexRetryGatewayEnableConfirmation::default();
        assert!(!confirmation.accepted_first_download);
        assert!(!confirmation.accepted_unreviewed_commit);
        assert!(!confirmation.accepted_cli_proxy_enable);
        assert!(!confirmation.accepted_provider_sync);
        assert!(!confirmation.accepted_wsl_unprotected);
    }
}
