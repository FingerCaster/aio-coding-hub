//! Usage: Small helpers to build/emit attempt events consistently across failover_loop.

use super::context::{requested_model_for_audit, AttemptCtx, CommonCtx, ProviderCtx};
use crate::gateway::events::{emit_attempt_event, emit_circuit_transition, GatewayAttemptEvent};
use crate::gateway::proxy::GatewayErrorCode;
use crate::gateway::response_fixer;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) struct AttemptCircuitFields {
    pub(super) state_before: Option<&'static str>,
    pub(super) state_after: Option<&'static str>,
    pub(super) failure_count: Option<u32>,
    pub(super) failure_threshold: Option<u32>,
}

#[allow(clippy::too_many_arguments)]
fn emit_probe_commit_transition_from_parts<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    trace_id: &str,
    cli_key: &str,
    upstream_first_byte_timeout_secs: u32,
    provider_id: i64,
    provider_name: &str,
    base_url: &str,
    change: &crate::circuit_breaker::CircuitChange,
    now_unix: i64,
    trigger_error_code: Option<&'static str>,
) -> bool {
    let Some(transition) = change.transition.as_ref() else {
        return false;
    };
    emit_circuit_transition(
        app,
        trace_id,
        cli_key,
        provider_id,
        provider_name,
        base_url,
        transition,
        now_unix,
        trigger_error_code,
        (trigger_error_code == Some(GatewayErrorCode::UpstreamTimeout.as_str()))
            .then_some(upstream_first_byte_timeout_secs),
    );
    true
}

/// Finish a dispatched probe before a terminal request record is emitted.
/// Calling this again is harmless: ownership/lease CAS turns repeats stale.
#[allow(clippy::too_many_arguments)]
pub(super) fn finalize_probe_failure_and_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    trace_id: &str,
    cli_key: &str,
    upstream_first_byte_timeout_secs: u32,
    ownership: Option<&Arc<crate::gateway::proxy::dispatch::ProviderDispatchOwnership>>,
    provider_id: i64,
    provider_name: &str,
    base_url: &str,
    circuit_snapshot: &mut crate::circuit_breaker::CircuitSnapshot,
    trigger_error_code: Option<&'static str>,
) -> bool {
    let Some(ownership) = ownership.filter(|ownership| ownership.is_probe()) else {
        return false;
    };
    let now_unix = crate::gateway::util::now_unix_seconds() as i64;
    match ownership.complete_probe_failure(now_unix, false, trigger_error_code) {
        Some(crate::circuit_breaker::ProbeCommitResult::Applied(change)) => {
            emit_probe_commit_transition_from_parts(
                app,
                trace_id,
                cli_key,
                upstream_first_byte_timeout_secs,
                provider_id,
                provider_name,
                base_url,
                &change,
                now_unix,
                trigger_error_code,
            );
            *circuit_snapshot = change.after;
            true
        }
        Some(crate::circuit_breaker::ProbeCommitResult::Stale(snapshot)) => {
            *circuit_snapshot = snapshot;
            false
        }
        None => false,
    }
}

pub(super) async fn emit_attempt_event_and_log<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    outcome: String,
    status: Option<u16>,
    circuit: AttemptCircuitFields,
) {
    if !ctx.observe {
        return;
    }

    let ProviderCtx {
        provider_id,
        provider_name_base,
        provider_base_url_base,
        active_requested_model,
        provider_index: _,
        session_reuse,
        claude_model_mapping,
        dispatch_ownership,
        ..
    } = provider_ctx;
    let AttemptCtx {
        attempt_index,
        retry_index: _,
        attempt_started_ms,
        attempt_started,
        circuit_before: _,
        ..
    } = attempt_ctx;

    let attempt_event = GatewayAttemptEvent {
        trace_id: ctx.trace_id.clone(),
        cli_key: ctx.cli_key.clone(),
        session_id: ctx.session_id.clone(),
        method: ctx.method_hint.clone(),
        path: ctx.forwarded_path.clone(),
        query: ctx.query.clone(),
        requested_model: requested_model_for_audit(
            ctx.special_settings,
            ctx.managed_model_route,
            ctx.requested_model.as_deref(),
            active_requested_model,
        ),
        requested_upstream_model: active_requested_model.map(str::to_string),
        special_settings_json: response_fixer::special_settings_json(ctx.special_settings),
        attempt_index,
        provider_id,
        session_reuse,
        provider_name: provider_name_base.clone(),
        base_url: provider_base_url_base.clone(),
        outcome,
        status,
        attempt_started_ms,
        attempt_duration_ms: attempt_started.elapsed().as_millis(),
        circuit_state_before: circuit.state_before,
        circuit_state_after: circuit.state_after,
        circuit_failure_count: circuit.failure_count,
        circuit_failure_threshold: circuit.failure_threshold,
        probe: dispatch_ownership
            .filter(|ownership| ownership.is_probe())
            .map(|_| true),
        probe_trigger: dispatch_ownership
            .filter(|ownership| ownership.is_probe())
            .and_then(|ownership| ownership.probe_trigger())
            .map(|trigger| trigger.as_str()),
        probe_result: dispatch_ownership
            .filter(|ownership| ownership.is_probe())
            .map(|_| "started"),
        probe_generation: dispatch_ownership
            .filter(|ownership| ownership.is_probe())
            .and_then(|ownership| ownership.probe_generation()),
        claude_model_mapping: claude_model_mapping.cloned(),
    };

    let state = ctx.state;
    emit_attempt_event(&state.app, attempt_event);
}

pub(super) async fn emit_attempt_event_and_log_with_circuit_before<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    outcome: String,
    status: Option<u16>,
) {
    let circuit_before = attempt_ctx.circuit_before;
    emit_attempt_event_and_log(
        ctx,
        provider_ctx,
        attempt_ctx,
        outcome,
        status,
        AttemptCircuitFields {
            state_before: Some(circuit_before.state.as_str()),
            state_after: None,
            failure_count: Some(circuit_before.failure_count),
            failure_threshold: Some(circuit_before.failure_threshold),
        },
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit_breaker::{
        CircuitBreaker, CircuitBreakerConfig, CircuitState, ProbeAcquireResult, ProbeLeaseGuard,
        ProbeTrigger,
    };
    use crate::gateway::proxy::dispatch::RequestDispatchIntent;
    use std::collections::HashMap;

    #[test]
    fn return_path_finalizer_completes_probe_before_terminal_log() {
        let app = tauri::test::mock_app();
        let circuit = Arc::new(CircuitBreaker::new(
            CircuitBreakerConfig {
                failure_threshold: 1,
                provider_cooldown_secs: 0,
                ..CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        circuit.record_failure(1, 1_000, None);
        let token = match circuit.try_acquire_probe(
            1,
            "return-finalizer",
            ProbeTrigger::AggressiveTurn,
            1_000,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("dispatch ownership");
        assert!(ownership.commit_at_transport_boundary(1_000));
        let mut snapshot = circuit.snapshot(1, 1_000);

        assert!(finalize_probe_failure_and_emit(
            app.handle(),
            "trace-return",
            "claude",
            30,
            Some(&ownership),
            1,
            "provider",
            "https://provider.example",
            &mut snapshot,
            Some(GatewayErrorCode::Upstream5xx.as_str()),
        ));
        assert_eq!(snapshot.state, CircuitState::Open);
        assert!(!snapshot.probe_in_flight);
        assert!(
            !circuit
                .snapshot(1, crate::gateway::util::now_unix_seconds() as i64)
                .probe_in_flight
        );
    }
}
