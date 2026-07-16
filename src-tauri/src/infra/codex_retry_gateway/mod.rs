//! Managed external Codex retry gateway infrastructure.

mod bridge;
mod config;
mod contracts;
mod git_source;
mod managed_state;
mod node;
mod process;
mod runtime;
mod source;
mod util;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use bridge::reset_bridge_runtime_for_tests;
#[allow(unused_imports)]
pub(crate) use bridge::{revoke_bridge_details_session, BridgeDetailsSession, BridgeRuntimeHandle};
pub(crate) use config::{
    managed_gateway_config, managed_gateway_state, normalize_preferred_port,
    ManagedGatewayStateInput,
};
#[cfg(test)]
pub(crate) use config::{MANAGED_PROVIDER_AIO, MANAGED_PROVIDER_OPENAI};
pub(crate) use contracts::*;
#[allow(unused_imports)]
pub(crate) use managed_state::{
    read_manager_state, write_manager_state, CodexRetryGatewayManagedProcessRecord,
    CodexRetryGatewayManagerPaths, CodexRetryGatewayManagerState,
    FileCodexRetryGatewayTransitionStore, TRANSITION_SNAPSHOT_AGGREGATE_MAX_BYTES,
};
#[allow(unused_imports)]
pub(crate) use node::{
    resolve_node_runtime, set_node_override, CodexRetryGatewayResolvedNode,
    CodexRetryGatewayResolvedNodeVersion,
};
#[cfg(test)]
pub(crate) use process::process_start_identity_for_tests;
#[allow(unused_imports)]
pub(crate) use process::{
    reconcile_runtime_process, start_runtime_process, stop_runtime_process,
    update_managed_provider_projection, verify_managed_provider_projection,
    CodexRetryGatewayHealthSnapshot, CodexRetryGatewayManagedProcess,
    CodexRetryGatewayProcessReconcileResult,
};
#[allow(unused_imports)]
pub(crate) use runtime::{
    apply_selected_commit, build_enable_plan, capture_runtime_enable_rollback,
    create_details_session, current_status, ensure_runtime_uninstall_ready,
    install_lifecycle_callback, reconcile_pending_runtime_launch, record_route_recovery_warning,
    record_runtime_recovery_failure, require_enable_confirmations, retry_runtime_recovery,
    revoke_details_session, rollback_runtime_enable, rollback_selected_commit,
    runtime_recovery_due, runtime_update_candidate, set_runtime_enabled, set_runtime_node_override,
    stop_runtime_for_shutdown, uninstall_runtime, validate_selected_commit,
};
#[allow(unused_imports)]
pub(crate) use source::{
    install_source_commit, resolve_commit_candidate, CodexRetryGatewayCommitCandidate,
    CodexRetryGatewayCommitSelection, CodexRetryGatewayInstalledSource,
    CodexRetryGatewaySourceHttpConfig,
};
pub(crate) use util::{metadata_is_symlink_or_reparse, now_unix_ms, random_hex};
