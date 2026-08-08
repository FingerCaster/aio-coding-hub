use super::provider_order;
use crate::gateway::proxy::failover::should_reuse_provider;
use crate::gateway::runtime::GatewayAppState;
use crate::providers;
use crate::{circuit_breaker, session_manager};
use tauri::Manager;

pub(super) mod probe_planner;

pub(super) struct ProviderSelection {
    pub(super) effective_sort_mode_id: Option<i64>,
    pub(super) providers: Vec<providers::ProviderForGateway>,
    pub(super) bound_provider_order: Option<Vec<i64>>,
    pub(super) active_sort_mode_id: Option<i64>,
    pub(super) session_bound_sort_mode_id: Option<Option<i64>>,
    pub(super) latest_provider_order: Vec<i64>,
    pub(super) route_changed: bool,
}

pub(super) fn select_providers_with_session_binding<R: tauri::Runtime>(
    state: &GatewayAppState<R>,
    cli_key: &str,
    session_id: Option<&str>,
    binding_request: Option<session_manager::SessionBindingRequest>,
    created_at: i64,
) -> crate::shared::error::AppResult<ProviderSelection> {
    let session_snapshot =
        session_id.and_then(|sid| state.session.routing_snapshot(cli_key, sid, created_at));
    let selection = providers::list_enabled_for_gateway_using_active_mode(&state.db, cli_key)?;
    let active_sort_mode_id = selection.sort_mode_id;
    let effective_sort_mode_id = selection.sort_mode_id;
    let providers = selection.providers;
    let latest_provider_order: Vec<i64> = providers.iter().map(|provider| provider.id).collect();
    let bound_sort_mode_id = session_snapshot
        .as_ref()
        .map(|snapshot| snapshot.route.sort_mode_id);
    let bound_provider_order = session_snapshot
        .as_ref()
        .map(|snapshot| snapshot.route.provider_order.clone());
    let route_changed = session_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.route.sort_mode_id != effective_sort_mode_id
            || snapshot.route.provider_order != latest_provider_order
    });

    if let (Some(sid), Some(binding_request)) = (session_id, binding_request) {
        if session_snapshot.is_none() {
            let _ = state.session.bind_sort_mode_with_recovery_epoch(
                cli_key,
                sid,
                session_manager::SessionBindingCreation::new(
                    effective_sort_mode_id,
                    Some(latest_provider_order.clone()),
                    state.circuit.recovery_epoch(),
                    state
                        .app
                        .try_state::<crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState>()
                        .map_or(0, |runtime| runtime.global_recovery_epoch()),
                    binding_request,
                ),
                created_at,
            );
        }
    }

    Ok(ProviderSelection {
        effective_sort_mode_id,
        providers,
        bound_provider_order,
        active_sort_mode_id,
        session_bound_sort_mode_id: bound_sort_mode_id,
        latest_provider_order,
        route_changed,
    })
}

pub(super) fn resolve_session_routing_decision(
    headers: &axum::http::HeaderMap,
    introspection_json: Option<&serde_json::Value>,
    is_claude_count_tokens: bool,
) -> SessionRoutingDecision {
    let extracted_session_id =
        session_manager::SessionManager::extract_session_id_from_json(headers, introspection_json);

    let session_id = if is_claude_count_tokens {
        None
    } else {
        extracted_session_id
    };

    let allow_session_reuse = if is_claude_count_tokens {
        false
    } else {
        should_reuse_provider(introspection_json)
    };

    SessionRoutingDecision {
        session_id,
        allow_session_reuse,
    }
}

pub(super) fn apply_session_reuse_provider_binding(
    allow_session_reuse: bool,
    providers: &mut [providers::ProviderForGateway],
    bound_provider_id: Option<i64>,
    bound_provider_order: Option<&[i64]>,
) -> Option<i64> {
    if !allow_session_reuse {
        return None;
    }
    let bound_provider_id = bound_provider_id?;

    provider_order::apply_session_provider_preference(
        providers,
        bound_provider_id,
        bound_provider_order,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_session_bound_provider_id(
    session: &session_manager::SessionManager,
    circuit: &circuit_breaker::CircuitBreaker,
    cli_key: &str,
    session_id: Option<&str>,
    created_at: i64,
    allow_session_reuse: bool,
    forced_provider_id: Option<i64>,
    providers: &mut [providers::ProviderForGateway],
    bound_provider_order: Option<&[i64]>,
) -> Option<i64> {
    let bound_provider_id =
        session_id.and_then(|sid| session.get_bound_provider(cli_key, sid, created_at));

    if allow_session_reuse && forced_provider_id.is_none() {
        if let (Some(session_id), Some(bound_provider_id)) = (session_id, bound_provider_id) {
            if !providers.iter().any(|p| p.id == bound_provider_id) {
                // The bound provider is no longer eligible for the current session's provider list
                // (e.g. sort_mode/provider membership changed). Clear the stale binding so it
                // cannot bypass selection constraints.
                session.clear_bound_provider(cli_key, session_id, created_at);
            } else {
                let allow = circuit.should_allow(bound_provider_id, created_at).allow;
                if !allow {
                    // Keep the provider in the candidate list so the common failover gate owns
                    // the authoritative circuit decision and records the skipped attempt.
                    return None;
                }
            }
        }
    }

    apply_session_reuse_provider_binding(
        allow_session_reuse,
        providers,
        bound_provider_id,
        bound_provider_order,
    )
}

pub(super) struct SessionRoutingDecision {
    pub(super) session_id: Option<String>,
    pub(super) allow_session_reuse: bool,
}

#[cfg(test)]
mod tests;
