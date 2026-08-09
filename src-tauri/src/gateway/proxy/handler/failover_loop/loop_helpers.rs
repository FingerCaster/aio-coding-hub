//! Usage: Helper types and functions for the failover loop orchestrator.
//!
//! Contains `FinalizeOwnedCommon`, skip-attempt helpers, and the
//! "all providers unavailable" finalization predicate.

use super::*;

pub(super) struct FinalizeOwnedCommon {
    pub(super) cli_key: String,
    pub(super) method_hint: String,
    pub(super) forwarded_path: String,
    pub(super) query: Option<String>,
    pub(super) trace_id: String,
    pub(super) session_id: Option<String>,
    pub(super) requested_model: Option<String>,
    pub(super) special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
}

pub(super) fn finalize_owned_from_input<R: tauri::Runtime>(
    input: &RequestContext<R>,
) -> FinalizeOwnedCommon {
    FinalizeOwnedCommon {
        cli_key: input.cli_key.clone(),
        method_hint: input.method_hint.clone(),
        forwarded_path: input.forwarded_path.clone(),
        query: input.query.clone(),
        trace_id: input.trace_id.clone(),
        session_id: input.session_id.clone(),
        requested_model: input.requested_model.clone(),
        special_settings: input.special_settings.clone(),
    }
}

pub(super) struct SkippedProviderAttempt<'a> {
    pub(super) provider_id: i64,
    pub(super) provider_name: &'a str,
    pub(super) base_url: &'a str,
    pub(super) error_category: &'static str,
    pub(super) error_code: &'static str,
    pub(super) reason: String,
    pub(super) reason_code: Option<&'static str>,
    pub(super) attempt_started_ms: u128,
    /// Circuit snapshot at gate-deny time; `Some` only for circuit-gate skips
    /// so non-circuit skip paths keep their serialized shape unchanged.
    pub(super) circuit: Option<crate::circuit_breaker::CircuitSnapshot>,
    pub(super) probe_trigger: Option<&'static str>,
    pub(super) probe_result: Option<&'static str>,
}

pub(super) fn push_skipped_provider_attempt(
    attempts: &mut Vec<FailoverAttempt>,
    skipped: SkippedProviderAttempt<'_>,
) {
    let circuit = skipped.circuit.as_ref();
    attempts.push(FailoverAttempt {
        provider_id: skipped.provider_id,
        provider_name: skipped.provider_name.to_string(),
        base_url: skipped.base_url.to_string(),
        outcome: "skipped".to_string(),
        status: None,
        provider_index: None,
        retry_index: None,
        session_reuse: None,
        error_category: Some(skipped.error_category),
        error_code: Some(skipped.error_code),
        decision: Some("skip"),
        reason: Some(skipped.reason),
        selection_method: Some(dc::SELECTION_METHOD_FILTERED),
        reason_code: skipped.reason_code,
        attempt_started_ms: Some(skipped.attempt_started_ms),
        attempt_duration_ms: Some(0),
        // Gate skip did not change the circuit state; before == after.
        circuit_state_before: circuit.map(|s| s.state.as_str()),
        circuit_state_after: circuit.map(|s| s.state.as_str()),
        circuit_failure_count: circuit.map(|s| s.failure_count),
        circuit_failure_threshold: circuit.map(|s| s.failure_threshold),
        probe: skipped.probe_result.map(|_| true),
        probe_trigger: skipped.probe_trigger,
        probe_result: skipped.probe_result,
        probe_generation: None,
        circuit_recover_at_unix: circuit.and_then(|snapshot| {
            if skipped.probe_result == Some("cooldown") {
                snapshot
                    .next_probe_at
                    .or(snapshot.open_until)
                    .or(snapshot.cooldown_until)
            } else {
                snapshot.open_until.or(snapshot.cooldown_until)
            }
        }),
        circuit_trigger_error_code: circuit.and_then(|s| s.last_trigger_error_code),
        provider_bridged: None,
        timeout_secs: None,
        stream_internal_error: None,
        requested_upstream_model: None,
    });
}

pub(super) fn is_gate_only_skipped_attempt(attempt: &FailoverAttempt) -> bool {
    if attempt.decision != Some("skip") {
        return false;
    }

    if attempt.provider_index.is_some() || attempt.retry_index.is_some() {
        return false;
    }

    matches!(
        attempt.reason_code,
        Some(
            dc::REASON_CIRCUIT_OPEN
                | dc::REASON_CIRCUIT_COOLDOWN
                | dc::REASON_RATE_LIMITED
                | dc::REASON_PROVIDER_DISABLED
                | dc::REASON_PROVIDER_ENABLE_CHECK_FAILED
                | dc::REASON_PROVIDER_TARGET_SELF_LOOP
                | dc::REASON_PROVIDER_TARGET_VALIDATION_FAILED
                | dc::REASON_ACCOUNT_USAGE_ZERO_BALANCE
                | dc::REASON_ACCOUNT_USAGE_EXPIRED
        )
    )
}

pub(super) fn is_probe_observation(attempt: &FailoverAttempt) -> bool {
    attempt.provider_index.is_none()
        && attempt.retry_index.is_none()
        && attempt.probe_result == Some("not_triggered")
}

pub(super) fn counted_provider_attempts(attempts: &[FailoverAttempt]) -> usize {
    attempts
        .iter()
        .filter(|attempt| attempt.provider_index.is_some() && attempt.retry_index.is_some())
        .count()
}

pub(super) fn should_finalize_as_all_providers_unavailable(attempts: &[FailoverAttempt]) -> bool {
    attempts
        .iter()
        .filter(|attempt| !is_probe_observation(attempt))
        .all(is_gate_only_skipped_attempt)
}

pub(super) fn apply_cx2cc_request_settings(
    responses_body: &mut serde_json::Value,
    cx2cc_settings: &crate::gateway::proxy::cx2cc::settings::Cx2ccSettings,
) {
    if let Some(ref effort) = cx2cc_settings.model_reasoning_effort {
        responses_body["reasoning"] = serde_json::json!({ "effort": effort });
    }
    if let Some(ref tier) = cx2cc_settings.service_tier {
        responses_body["service_tier"] = serde_json::json!(tier);
    }
    if cx2cc_settings.disable_response_storage {
        responses_body["store"] = serde_json::json!(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(
        provider_index: Option<u32>,
        retry_index: Option<u32>,
        probe_result: Option<&'static str>,
    ) -> FailoverAttempt {
        FailoverAttempt {
            provider_id: 1,
            provider_name: "provider".to_string(),
            base_url: "https://provider.example".to_string(),
            outcome: "skipped".to_string(),
            status: None,
            provider_index,
            retry_index,
            session_reuse: None,
            error_category: None,
            error_code: None,
            decision: Some("skip"),
            reason: None,
            selection_method: Some(dc::SELECTION_METHOD_FILTERED),
            reason_code: None,
            attempt_started_ms: Some(0),
            attempt_duration_ms: Some(0),
            circuit_state_before: None,
            circuit_state_after: None,
            circuit_failure_count: None,
            circuit_failure_threshold: None,
            probe: probe_result.map(|_| false),
            probe_trigger: None,
            probe_result,
            probe_generation: None,
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: None,
            timeout_secs: None,
            requested_upstream_model: None,
            stream_internal_error: None,
        }
    }

    #[test]
    fn not_triggered_observation_does_not_consume_attempt_count_or_change_unavailable() {
        let observation = attempt(None, None, Some("not_triggered"));
        let real_attempt = attempt(Some(1), Some(1), None);

        assert_eq!(
            counted_provider_attempts(std::slice::from_ref(&observation)),
            0
        );
        assert!(should_finalize_as_all_providers_unavailable(&[observation]));
        assert_eq!(counted_provider_attempts(&[real_attempt]), 1);
    }

    #[test]
    fn provider_safety_gate_skips_are_all_unavailable_evidence() {
        for reason_code in [
            dc::REASON_PROVIDER_DISABLED,
            dc::REASON_PROVIDER_ENABLE_CHECK_FAILED,
            dc::REASON_PROVIDER_TARGET_SELF_LOOP,
            dc::REASON_PROVIDER_TARGET_VALIDATION_FAILED,
        ] {
            let mut skipped = attempt(None, None, None);
            skipped.reason_code = Some(reason_code);

            assert!(is_gate_only_skipped_attempt(&skipped));
            assert!(should_finalize_as_all_providers_unavailable(&[skipped]));
        }
    }

    #[test]
    fn probe_gate_skips_are_unavailable_and_cooldown_reports_next_probe_deadline() {
        let snapshot = crate::circuit_breaker::CircuitSnapshot {
            state: crate::circuit_breaker::CircuitState::Open,
            failure_count: 1,
            failure_threshold: 1,
            open_until: Some(2_000),
            cooldown_until: None,
            probe_reference_at: Some(1_000),
            next_probe_at: Some(1_030),
            natural_probe_due_at: Some(1_300),
            recovery_guard_until: None,
            recovery_epoch: 0,
            probe_in_flight: false,
            state_revision: 1,
            last_trigger_error_code: None,
        };
        let mut attempts = Vec::new();
        push_skipped_provider_attempt(
            &mut attempts,
            SkippedProviderAttempt {
                provider_id: 1,
                provider_name: "provider",
                base_url: "https://provider.example",
                error_category: "circuit_breaker",
                error_code: "GW_PROVIDER_CIRCUIT_OPEN",
                reason: "provider skipped by circuit breaker (cooldown)".to_string(),
                reason_code: Some(dc::REASON_CIRCUIT_COOLDOWN),
                attempt_started_ms: 0,
                circuit: Some(snapshot.clone()),
                probe_trigger: Some("aggressive_turn"),
                probe_result: Some("cooldown"),
            },
        );
        push_skipped_provider_attempt(
            &mut attempts,
            SkippedProviderAttempt {
                provider_id: 1,
                provider_name: "provider",
                base_url: "https://provider.example",
                error_category: "circuit_breaker",
                error_code: "GW_PROVIDER_CIRCUIT_OPEN",
                reason: "provider skipped by circuit breaker (in_flight)".to_string(),
                reason_code: Some(dc::REASON_CIRCUIT_OPEN),
                attempt_started_ms: 0,
                circuit: Some(snapshot),
                probe_trigger: Some("aggressive_turn"),
                probe_result: Some("in_flight"),
            },
        );

        assert_eq!(attempts[0].circuit_recover_at_unix, Some(1_030));
        assert!(attempts.iter().all(is_gate_only_skipped_attempt));
        assert!(should_finalize_as_all_providers_unavailable(&attempts));
    }
}
