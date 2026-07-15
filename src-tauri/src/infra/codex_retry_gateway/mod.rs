//! Managed external Codex retry gateway infrastructure.

mod bridge;
mod config;
mod contracts;
mod managed_state;
mod node;
mod process;
mod runtime;
mod source;
mod util;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use bridge::{install_bridge_runtime_for_tests, reset_bridge_runtime_for_tests};
#[allow(unused_imports)]
pub(crate) use bridge::{BridgeDetailsSession, BridgeRuntimeHandle};
pub(crate) use config::{managed_gateway_config, managed_gateway_state};
pub(crate) use contracts::*;
#[allow(unused_imports)]
pub(crate) use managed_state::{
    CodexRetryGatewayManagedProcessRecord, CodexRetryGatewayManagerPaths,
    CodexRetryGatewayManagerState, FileCodexRetryGatewayTransitionStore,
};
#[allow(unused_imports)]
pub(crate) use node::{
    resolve_node_runtime, set_node_override, CodexRetryGatewayResolvedNode,
    CodexRetryGatewayResolvedNodeVersion,
};
#[allow(unused_imports)]
pub(crate) use process::{
    reconcile_runtime_process, start_runtime_process, stop_runtime_process,
    CodexRetryGatewayHealthSnapshot, CodexRetryGatewayManagedProcess,
    CodexRetryGatewayProcessReconcileResult,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use runtime::install_lifecycle_callback_for_tests;
#[allow(unused_imports)]
pub(crate) use runtime::{
    apply_selected_commit, build_enable_plan, create_details_session, current_status,
    retry_runtime_recovery, runtime_update_candidate, set_runtime_enabled,
    set_runtime_node_override, uninstall_runtime, validate_selected_commit,
};
#[allow(unused_imports)]
pub(crate) use source::{
    install_source_commit, resolve_commit_candidate, CodexRetryGatewayCommitCandidate,
    CodexRetryGatewayCommitSelection, CodexRetryGatewayInstalledSource,
    CodexRetryGatewaySourceHttpConfig,
};
