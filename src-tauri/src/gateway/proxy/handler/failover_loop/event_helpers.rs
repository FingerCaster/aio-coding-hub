//! Usage: Small helpers to build/emit attempt events consistently across failover_loop.

use super::context::{
    requested_model_for_audit, AttemptCtx, CommonCtx, CommonCtxOwned, ProviderCtx,
};
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

pub(super) fn observe_infinite_attempt_usage<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    usage: Option<&crate::usage::UsageExtract>,
    cost_usd_femto: Option<u128>,
) {
    let Some(ledger) = ctx.infinite_retry_ledger else {
        return;
    };
    let key = crate::gateway::infinite_retry::AttemptKey::new(
        provider_ctx.provider_id,
        attempt_ctx.retry_index,
        attempt_ctx.attempt_started_ms,
    );
    let sample = usage
        .map(|usage| crate::gateway::infinite_retry::UsageSample::from_metrics(&usage.metrics));
    let cost_usd_femto = cost_usd_femto.or_else(|| {
        let usage = usage?;
        let special_settings_json = response_fixer::special_settings_json(ctx.special_settings);
        crate::request_logs::calculate_attempt_cost_usd_femto(
            &ctx.state.app,
            &ctx.state.db,
            ctx.cli_key.as_str(),
            provider_ctx
                .active_requested_model
                .or(ctx.requested_model.as_deref()),
            special_settings_json.as_deref(),
            provider_ctx.provider_id,
            &usage.metrics,
        )
        .ok()
        .flatten()
    });
    if let Ok(mut ledger) = ledger.lock() {
        ledger.observe_attempt_usage(key, sample, cost_usd_femto);
    }
}

#[derive(Default)]
pub(super) struct InfiniteRetryTerminalProjection {
    pub(super) log_usage_metrics: Option<crate::usage::UsageMetrics>,
    pub(super) log_cost_usd_femto: Option<i64>,
    pub(super) activity_details_json: Option<String>,
}

pub(super) async fn finalize_infinite_retry_activity<R: tauri::Runtime + 'static>(
    ctx: &CommonCtxOwned<'_, R>,
    stop_reason: &'static str,
) -> InfiniteRetryTerminalProjection {
    let Some(ledger) = ctx.infinite_retry_ledger.as_ref() else {
        return InfiniteRetryTerminalProjection::default();
    };
    let (snapshot, log_usage_metrics, log_cost_usd_femto) = match ledger.lock() {
        Ok(mut ledger) => {
            ledger.stop(stop_reason);
            (
                ledger.snapshot(),
                ledger.usage_metrics(),
                ledger.cost_usd_femto(),
            )
        }
        Err(_) => return InfiniteRetryTerminalProjection::default(),
    };
    let now_ms = crate::gateway::util::now_unix_millis() as i64;
    ctx.state.active_requests.update_infinite_retry_progress(
        ctx.trace_id.as_str(),
        snapshot.phase,
        snapshot.rounds.parse().unwrap_or(u64::MAX),
        snapshot.attempts.parse().unwrap_or(u64::MAX),
        now_ms,
    );
    let activity_details_json = serde_json::to_string(&snapshot).ok();

    let database = ctx.state.db.clone();
    let trace_id = ctx.trace_id.clone();
    let cli_key = ctx.cli_key.clone();
    let providers = snapshot.providers;
    let details_for_touch = activity_details_json.clone();
    let _ = crate::blocking::run("gateway_infinite_retry_terminal_activity", move || {
        crate::infinite_retry_provider_usage::replace_for_trace(
            &database,
            trace_id.as_str(),
            providers.as_slice(),
            now_ms,
        )?;
        crate::request_logs::touch_activity(
            &database,
            trace_id.as_str(),
            cli_key.as_str(),
            now_ms,
            details_for_touch,
        )
        .map(|_| ())
    })
    .await;

    InfiniteRetryTerminalProjection {
        log_usage_metrics,
        log_cost_usd_femto,
        activity_details_json,
    }
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
    if let Some(ledger) = ctx.infinite_retry_ledger {
        let key = crate::gateway::infinite_retry::AttemptKey::new(
            provider_ctx.provider_id,
            attempt_ctx.retry_index,
            attempt_ctx.attempt_started_ms,
        );
        let (outcome_code, failure_category) =
            crate::gateway::infinite_retry::classify_attempt_outcome(&outcome, status);
        if let Ok(mut ledger) = ledger.lock() {
            ledger.record_attempt_once(
                key,
                provider_ctx.provider_name_base,
                status,
                outcome_code,
                failure_category,
                attempt_ctx
                    .attempt_started
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            );
        }
    }

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

    let health_bypass = ctx.provider_health_mode.bypasses_circuit();
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
        circuit_state_before: (!health_bypass).then_some(circuit.state_before).flatten(),
        circuit_state_after: (!health_bypass).then_some(circuit.state_after).flatten(),
        circuit_failure_count: (!health_bypass).then_some(circuit.failure_count).flatten(),
        circuit_failure_threshold: (!health_bypass)
            .then_some(circuit.failure_threshold)
            .flatten(),
        probe: (!health_bypass)
            .then_some(dispatch_ownership)
            .flatten()
            .filter(|ownership| ownership.is_probe())
            .map(|_| true),
        probe_trigger: (!health_bypass)
            .then_some(dispatch_ownership)
            .flatten()
            .filter(|ownership| ownership.is_probe())
            .and_then(|ownership| ownership.probe_trigger())
            .map(|trigger| trigger.as_str()),
        probe_result: (!health_bypass)
            .then_some(dispatch_ownership)
            .flatten()
            .filter(|ownership| ownership.is_probe())
            .map(|_| "started"),
        probe_generation: (!health_bypass)
            .then_some(dispatch_ownership)
            .flatten()
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
