//! Usage: Shared helpers to record SystemError attempts and apply failover decisions.

use super::*;
use crate::circuit_breaker;
use crate::gateway::events::decision_chain as dc;
use crate::gateway::proxy::status_override;
use crate::gateway::proxy::{is_claude_count_tokens_request, provider_router};
use std::time::Duration;

pub(super) struct RecordSystemFailureArgs<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) ctx: CommonCtx<'a, R>,
    pub(super) provider_ctx: ProviderCtx<'a>,
    pub(super) attempt_ctx: AttemptCtx<'a>,
    pub(super) loop_state: LoopState<'a, R>,
    pub(super) status: Option<u16>,
    pub(super) error_code: &'static str,
    pub(super) decision: FailoverDecision,
    pub(super) outcome: String,
    pub(super) reason: String,
    pub(super) record_circuit_failure: bool,
    /// Delay for a configured transport retry, applied only after the final decision remains retry.
    pub(super) configured_retry_backoff: Option<Duration>,
    /// First-byte timeout seconds in effect; `Some` only for timeout-class failures.
    pub(super) timeout_secs: Option<u32>,
}

pub(super) async fn record_system_failure_and_decide<R: tauri::Runtime>(
    args: RecordSystemFailureArgs<'_, R>,
) -> LoopControl {
    record_system_failure_and_decide_impl(args, CooldownPolicy::Apply).await
}

pub(super) async fn record_system_failure_and_decide_no_cooldown<R: tauri::Runtime>(
    args: RecordSystemFailureArgs<'_, R>,
) -> LoopControl {
    record_system_failure_and_decide_impl(args, CooldownPolicy::Skip).await
}

#[derive(Debug, Clone, Copy)]
enum CooldownPolicy {
    Apply,
    Skip,
}

async fn record_system_failure_and_decide_impl<R: tauri::Runtime>(
    args: RecordSystemFailureArgs<'_, R>,
    cooldown_policy: CooldownPolicy,
) -> LoopControl {
    let RecordSystemFailureArgs {
        ctx,
        provider_ctx,
        attempt_ctx,
        loop_state,
        status,
        error_code,
        mut decision,
        mut outcome,
        reason,
        record_circuit_failure,
        configured_retry_backoff,
        timeout_secs,
    } = args;
    let dispatch_ownership = provider_ctx.dispatch_ownership;
    let probe_active = dispatch_ownership.is_some_and(|ownership| ownership.is_probe());
    let ProviderCtx {
        provider_id,
        provider_name_base,
        provider_base_url_base,
        provider_index,
        session_reuse,
        ..
    } = provider_ctx;

    let AttemptCtx {
        attempt_index: _,
        retry_index,
        attempt_started_ms,
        attempt_started,
        circuit_before,
        ..
    } = attempt_ctx;

    let LoopState {
        attempts,
        failed_provider_ids,
        last_outcome,
        active_requested_model: _,
        circuit_snapshot,
        abort_guard: _,
    } = loop_state;

    let category = ErrorCategory::SystemError;
    let effective_status = status_override::effective_status(status, Some(error_code));

    let is_count_tokens =
        is_claude_count_tokens_request(ctx.cli_key.as_str(), ctx.forwarded_path.as_str());
    let now_unix = now_unix_seconds() as i64;

    let health_bypass = ctx.provider_health_mode.bypasses_circuit();
    let mut circuit_state_before = (!health_bypass).then_some(circuit_before.state.as_str());
    let mut circuit_state_after: Option<&'static str> = None;
    let mut circuit_failure_count = (!health_bypass).then_some(circuit_before.failure_count);
    let circuit_failure_threshold = (!health_bypass).then_some(circuit_before.failure_threshold);

    if !health_bypass && !is_count_tokens && record_circuit_failure {
        let change = if let Some(ownership) = dispatch_ownership.filter(|value| value.is_probe()) {
            match ownership.record_probe_attempt_failure(now_unix, true, Some(error_code)) {
                Some(circuit_breaker::ProbeCommitResult::Applied(change)) => change,
                Some(circuit_breaker::ProbeCommitResult::Stale(snapshot)) => {
                    circuit_breaker::CircuitChange {
                        before: snapshot.clone(),
                        after: snapshot,
                        transition: None,
                    }
                }
                None => circuit_breaker::CircuitChange {
                    before: circuit_before.clone(),
                    after: circuit_before.clone(),
                    transition: None,
                },
            }
        } else {
            provider_router::record_failure_and_emit_transition(
                provider_router::RecordCircuitArgs::from_state(
                    ctx.state,
                    ctx.trace_id.as_str(),
                    ctx.cli_key.as_str(),
                    provider_id,
                    provider_name_base.as_str(),
                    provider_base_url_base.as_str(),
                    now_unix,
                )
                .with_provider_health_neutral(ctx.provider_health_neutral)
                .with_trigger(Some(error_code), Some(ctx.upstream_first_byte_timeout_secs)),
            )
        };
        *circuit_snapshot = change.after.clone();
        circuit_state_before = Some(change.before.state.as_str());
        circuit_state_after = Some(change.after.state.as_str());
        circuit_failure_count = Some(change.after.failure_count);

        let recorded_decision = decision;
        decision = system_failure_decision_after_circuit_record(
            decision,
            false,
            Some(change.after.state),
            probe_active,
        );
        outcome =
            system_failure_outcome_after_decision_override(outcome, recorded_decision, decision);
    }

    attempts.push(FailoverAttempt {
        provider_id,
        provider_name: provider_name_base.clone(),
        base_url: provider_base_url_base.clone(),
        outcome: outcome.clone(),
        status: effective_status,
        provider_index: Some(provider_index),
        retry_index: Some(retry_index),
        session_reuse,
        error_category: Some(category.as_str()),
        error_code: Some(error_code),
        decision: Some(decision.as_str()),
        reason: Some(reason),
        selection_method: if probe_active {
            Some(dc::SELECTION_METHOD_CIRCUIT_PROBE)
        } else {
            dc::selection_method(provider_index, retry_index, session_reuse)
        },
        reason_code: Some(category.reason_code()),
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(attempt_started.elapsed().as_millis()),
        circuit_state_before,
        circuit_state_after,
        circuit_failure_count,
        circuit_failure_threshold,
        probe: probe_active.then_some(true),
        probe_trigger: dispatch_ownership
            .filter(|ownership| ownership.is_probe())
            .and_then(|ownership| ownership.probe_trigger())
            .map(|trigger| trigger.as_str()),
        probe_result: probe_active.then_some("failed"),
        probe_generation: dispatch_ownership
            .filter(|ownership| ownership.is_probe())
            .and_then(|ownership| ownership.probe_generation()),
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        provider_bridged: Some(provider_ctx.provider_bridged),
        timeout_secs,
        stream_internal_error: None,
        requested_upstream_model: provider_ctx.active_requested_model.map(str::to_string),
        reasoning_effort: attempt_ctx.reasoning_effort.map(str::to_string),
        upstream_sent: attempt_ctx.upstream_sent,
    });

    emit_attempt_event_and_log_with_circuit_before(
        ctx,
        provider_ctx,
        attempt_ctx,
        outcome,
        effective_status,
    )
    .await;

    *last_outcome = Some(AttemptOutcome::new(category.as_str(), error_code));

    let should_apply_cooldown = matches!(cooldown_policy, CooldownPolicy::Apply)
        && !health_bypass
        && !probe_active
        && !is_claude_count_tokens_request(ctx.cli_key.as_str(), ctx.forwarded_path.as_str());

    if should_apply_cooldown {
        let provider_cooldown_secs = ctx.provider_cooldown_secs;
        if provider_cooldown_secs > 0
            && matches!(
                decision,
                FailoverDecision::SwitchProvider | FailoverDecision::Abort
            )
        {
            let now_unix = now_unix_seconds() as i64;
            let snap = provider_router::trigger_cooldown(
                ctx.state.circuit.as_ref(),
                provider_id,
                now_unix,
                provider_cooldown_secs,
                ctx.provider_health_neutral,
            );
            *circuit_snapshot = snap;
        }
    }

    apply_configured_retry_backoff(decision, configured_retry_backoff).await;
    match decision {
        FailoverDecision::RetrySameProvider => LoopControl::ContinueRetry,
        FailoverDecision::SwitchProvider => {
            failed_provider_ids.insert(provider_id);
            LoopControl::BreakRetry
        }
        FailoverDecision::Abort => LoopControl::BreakRetry,
    }
}

fn configured_retry_backoff_for_final_decision(
    decision: FailoverDecision,
    delay: Option<Duration>,
) -> Option<Duration> {
    matches!(decision, FailoverDecision::RetrySameProvider)
        .then_some(delay)
        .flatten()
}

pub(super) async fn apply_configured_retry_backoff(
    decision: FailoverDecision,
    delay: Option<Duration>,
) {
    if let Some(delay) = configured_retry_backoff_for_final_decision(decision, delay) {
        tokio::time::sleep(delay).await;
    }
}

fn system_failure_decision_after_circuit_record(
    decision: FailoverDecision,
    is_count_tokens: bool,
    circuit_state_after: Option<circuit_breaker::CircuitState>,
    probe_active: bool,
) -> FailoverDecision {
    if !probe_active
        && !is_count_tokens
        && matches!(decision, FailoverDecision::RetrySameProvider)
        && matches!(
            circuit_state_after,
            Some(circuit_breaker::CircuitState::Open)
        )
    {
        return FailoverDecision::SwitchProvider;
    }

    decision
}

fn system_failure_outcome_after_decision_override(
    outcome: String,
    before: FailoverDecision,
    after: FailoverDecision,
) -> String {
    if matches!(before, FailoverDecision::RetrySameProvider)
        && matches!(after, FailoverDecision::SwitchProvider)
    {
        return outcome.replace("decision=retry", "decision=switch");
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::{
        apply_configured_retry_backoff, circuit_breaker,
        configured_retry_backoff_for_final_decision, system_failure_decision_after_circuit_record,
        system_failure_outcome_after_decision_override, FailoverDecision,
    };
    use std::time::Duration;

    #[test]
    fn system_failure_switches_when_circuit_opens() {
        let decision = system_failure_decision_after_circuit_record(
            FailoverDecision::RetrySameProvider,
            false,
            Some(circuit_breaker::CircuitState::Open),
            false,
        );

        assert!(matches!(decision, FailoverDecision::SwitchProvider));
    }

    #[test]
    fn configured_retry_backoff_requires_final_same_provider_retry() {
        let delay = Some(Duration::from_millis(250));

        assert_eq!(
            configured_retry_backoff_for_final_decision(FailoverDecision::RetrySameProvider, delay,),
            delay
        );
        assert_eq!(
            configured_retry_backoff_for_final_decision(FailoverDecision::SwitchProvider, delay),
            None
        );
        assert_eq!(
            configured_retry_backoff_for_final_decision(FailoverDecision::Abort, delay),
            None
        );
    }

    #[tokio::test(start_paused = true)]
    async fn configured_retry_backoff_waits_exactly_for_same_provider_retry() {
        let started = tokio::time::Instant::now();

        apply_configured_retry_backoff(
            FailoverDecision::RetrySameProvider,
            Some(Duration::from_millis(250)),
        )
        .await;

        assert_eq!(started.elapsed(), Duration::from_millis(250));
    }

    #[tokio::test(start_paused = true)]
    async fn configured_retry_backoff_does_not_wait_for_zero_switch_or_abort() {
        for (decision, delay) in [
            (FailoverDecision::RetrySameProvider, None),
            (
                FailoverDecision::SwitchProvider,
                Some(Duration::from_millis(250)),
            ),
            (FailoverDecision::Abort, Some(Duration::from_millis(250))),
        ] {
            let started = tokio::time::Instant::now();
            apply_configured_retry_backoff(decision, delay).await;
            assert_eq!(started.elapsed(), Duration::ZERO);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn configured_retry_backoff_does_not_wait_after_circuit_opens() {
        let decision = system_failure_decision_after_circuit_record(
            FailoverDecision::RetrySameProvider,
            false,
            Some(circuit_breaker::CircuitState::Open),
            false,
        );
        let started = tokio::time::Instant::now();

        apply_configured_retry_backoff(decision, Some(Duration::from_millis(250))).await;

        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[test]
    fn system_failure_preserves_retry_when_circuit_stays_closed() {
        let decision = system_failure_decision_after_circuit_record(
            FailoverDecision::RetrySameProvider,
            false,
            Some(circuit_breaker::CircuitState::Closed),
            false,
        );

        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
    }

    #[test]
    fn system_failure_preserves_count_tokens_abort() {
        let decision = system_failure_decision_after_circuit_record(
            FailoverDecision::Abort,
            true,
            Some(circuit_breaker::CircuitState::Open),
            false,
        );

        assert!(matches!(decision, FailoverDecision::Abort));
    }

    #[test]
    fn probe_system_failure_preserves_same_provider_retry_when_circuit_stays_open() {
        let decision = system_failure_decision_after_circuit_record(
            FailoverDecision::RetrySameProvider,
            false,
            Some(circuit_breaker::CircuitState::Open),
            true,
        );

        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
    }

    #[test]
    fn system_failure_outcome_tracks_retry_to_switch_override() {
        let outcome = system_failure_outcome_after_decision_override(
            "request_timeout: category=SYSTEM_ERROR code=GW_UPSTREAM_TIMEOUT decision=retry timeout_secs=30"
                .to_string(),
            FailoverDecision::RetrySameProvider,
            FailoverDecision::SwitchProvider,
        );

        assert!(outcome.contains("decision=switch"));
        assert!(!outcome.contains("decision=retry"));
    }
}
