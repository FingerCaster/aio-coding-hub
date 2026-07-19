//! Usage: Handle upstream send timeout inside `failover_loop::run`.

use super::upstream_retry_policy::{
    should_record_circuit_failure, transient_failure_decision, RetryPolicyMatch,
};
use super::*;
use crate::gateway::proxy::is_claude_count_tokens_request;

fn timeout_decision(
    is_count_tokens: bool,
    policy: &crate::settings::UpstreamRetryPolicy,
    configured_retries_used: u32,
    retry_index: u32,
    max_attempts_per_provider: u32,
) -> (FailoverDecision, bool) {
    transient_failure_decision(
        is_count_tokens,
        RetryPolicyMatch::Transport(crate::settings::UpstreamTransportRetryKind::Timeout),
        policy,
        configured_retries_used,
        retry_index,
        max_attempts_per_provider,
    )
}

pub(super) async fn handle_timeout<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    loop_state: LoopState<'_, R>,
    configured_transient_retries_used: &mut u32,
) -> LoopControl {
    let is_count_tokens =
        is_claude_count_tokens_request(ctx.cli_key.as_str(), ctx.forwarded_path.as_str());
    let error_code = GatewayErrorCode::UpstreamTimeout.as_str();
    let (decision, configured_retry) = timeout_decision(
        is_count_tokens,
        provider_ctx.upstream_retry_policy,
        *configured_transient_retries_used,
        attempt_ctx.retry_index,
        provider_ctx.provider_max_attempts,
    );

    let timeout_secs = ctx.upstream_first_byte_timeout_secs;
    let outcome = format!(
        "request_timeout: category={} code={} decision={} timeout_secs={}",
        ErrorCategory::SystemError.as_str(),
        error_code,
        decision.as_str(),
        timeout_secs,
    );
    if configured_retry {
        *configured_transient_retries_used = configured_transient_retries_used.saturating_add(1);
    }

    if is_count_tokens {
        return record_system_failure_and_decide_no_cooldown(RecordSystemFailureArgs {
            ctx,
            provider_ctx,
            attempt_ctx,
            loop_state,
            status: None,
            error_code,
            decision,
            outcome,
            reason: "request timeout".to_string(),
            record_circuit_failure: true,
            timeout_secs: Some(timeout_secs),
        })
        .await;
    }

    record_system_failure_and_decide(RecordSystemFailureArgs {
        ctx,
        provider_ctx,
        attempt_ctx,
        loop_state,
        status: None,
        error_code,
        decision,
        outcome,
        reason: "request timeout".to_string(),
        record_circuit_failure: should_record_circuit_failure(
            provider_ctx.upstream_retry_policy,
            configured_retry,
        ),
        timeout_secs: Some(timeout_secs),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{timeout_decision, FailoverDecision};
    use crate::settings::UpstreamRetryPolicy;

    #[test]
    fn timeout_decision_aborts_for_count_tokens() {
        let (decision, configured_retry) =
            timeout_decision(true, &UpstreamRetryPolicy::default(), 0, 1, 5);
        assert!(matches!(decision, FailoverDecision::Abort));
        assert!(!configured_retry);
    }

    #[test]
    fn timeout_decision_retries_configured_timeout_before_limit() {
        let (decision, configured_retry) =
            timeout_decision(false, &UpstreamRetryPolicy::default(), 0, 1, 5);
        assert!(matches!(decision, FailoverDecision::RetrySameProvider));
        assert!(configured_retry);
    }

    #[test]
    fn timeout_decision_switches_when_timeout_not_configured() {
        let policy = UpstreamRetryPolicy {
            transport_errors: vec![],
            ..Default::default()
        };
        let (decision, configured_retry) = timeout_decision(false, &policy, 0, 1, 5);
        assert!(matches!(decision, FailoverDecision::SwitchProvider));
        assert!(!configured_retry);
    }

    #[test]
    fn timeout_decision_switches_configured_timeout_at_limit() {
        let (decision, configured_retry) =
            timeout_decision(false, &UpstreamRetryPolicy::default(), 0, 5, 5);
        assert!(matches!(decision, FailoverDecision::SwitchProvider));
        assert!(!configured_retry);
    }
}
