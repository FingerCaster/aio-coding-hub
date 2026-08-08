//! Usage: Handle successful event-stream upstream responses inside `failover_loop::run`.

use super::attempt_executor::RetryLoopState;
use super::provider_iterator::PreparedProvider;
use super::upstream_retry_policy::{
    configured_retry_backoff_delay, should_record_circuit_failure, transient_failure_decision,
    RetryPolicyMatch,
};
use super::*;
use crate::domain::provider_oauth_limits;
use crate::gateway::proxy::gemini_oauth;
use crate::gateway::proxy::protocol_bridge;
use crate::gateway::proxy::provider_router;
use crate::gateway::proxy::request_context::RequestContext;
use crate::gateway::proxy::status_override;
use crate::gateway::proxy::upstream_client_error_rules;
use futures_core::Stream;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::Instant;

const MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES: usize = 1024 * 1024;

type DecodedEventStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>;

fn decode_event_stream<S>(upstream: S, gzip: bool) -> DecodedEventStream
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    if gzip {
        Box::pin(GunzipStream::new(upstream))
    } else {
        Box::pin(upstream)
    }
}

fn prepend_and_decode_event_stream(
    first_chunk: Option<Bytes>,
    upstream: DecodedEventStream,
    gzip: bool,
) -> DecodedEventStream {
    decode_event_stream(FirstChunkStream::new(first_chunk, upstream), gzip)
}

async fn next_event_stream_chunk(
    stream: &mut DecodedEventStream,
) -> Result<Option<Bytes>, reqwest::Error> {
    std::future::poll_fn(|cx| stream.as_mut().poll_next(cx))
        .await
        .transpose()
}

struct ProbeTerminalCommit {
    snapshot: crate::circuit_breaker::CircuitSnapshot,
}

fn effective_terminal_failure_status(status: StatusCode, error_code: &'static str) -> StatusCode {
    status_override::effective_status(Some(status.as_u16()), Some(error_code))
        .and_then(|status| StatusCode::from_u16(status).ok())
        .unwrap_or(StatusCode::BAD_GATEWAY)
}

#[allow(clippy::too_many_arguments)]
fn mark_last_stream_attempt_terminal_failure(
    attempts: &mut [FailoverAttempt],
    status: StatusCode,
    error_code: &'static str,
    reason: String,
    duration_ms: u128,
    circuit_after: &crate::circuit_breaker::CircuitSnapshot,
) {
    let Some(last) = attempts.last_mut() else {
        return;
    };
    let category = ErrorCategory::ProviderError;
    let decision = FailoverDecision::Abort;

    last.outcome = format!("stream_error: code={error_code}");
    last.status = Some(status.as_u16());
    last.error_category = Some(category.as_str());
    last.error_code = Some(error_code);
    last.decision = Some(decision.as_str());
    last.reason = Some(reason);
    last.reason_code = Some(category.reason_code());
    last.attempt_duration_ms = Some(duration_ms);
    last.circuit_state_after = Some(circuit_after.state.as_str());
    last.circuit_failure_count = Some(circuit_after.failure_count);
    last.circuit_failure_threshold = Some(circuit_after.failure_threshold);
    if last.probe == Some(true) {
        last.probe_result = Some("failed");
    }
}

fn complete_probe_failure<R: tauri::Runtime>(
    common: &CommonCtxOwned<'_, R>,
    provider: &ProviderCtxOwned,
    counted_failure: bool,
    error_code: Option<&'static str>,
) -> Option<ProbeTerminalCommit> {
    let ownership = provider
        .dispatch_ownership
        .as_ref()
        .filter(|ownership| ownership.is_probe())?;
    let now_unix = now_unix_seconds() as i64;
    let result = ownership.complete_probe_failure(now_unix, counted_failure, error_code)?;
    match result {
        crate::circuit_breaker::ProbeCommitResult::Applied(change) => {
            if let Some(transition) = change.transition.as_ref() {
                crate::gateway::events::emit_circuit_transition(
                    &common.state.app,
                    common.trace_id.as_str(),
                    common.cli_key.as_str(),
                    provider.provider_id,
                    provider.provider_name_base.as_str(),
                    provider.provider_base_url_base.as_str(),
                    transition,
                    now_unix,
                    error_code,
                    (error_code == Some(GatewayErrorCode::UpstreamTimeout.as_str()))
                        .then_some(common.upstream_first_byte_timeout_secs),
                );
            }
            Some(ProbeTerminalCommit {
                snapshot: change.after,
            })
        }
        crate::circuit_breaker::ProbeCommitResult::Stale(snapshot) => {
            Some(ProbeTerminalCommit { snapshot })
        }
    }
}

fn resolve_requested_model_for_log(
    requested_model: Option<String>,
    fallback_model: Option<&str>,
    cli_key: &str,
    body_bytes: &[u8],
) -> Option<String> {
    fallback_model
        .map(str::to_string)
        .or(requested_model)
        .or_else(|| {
            if body_bytes.is_empty() {
                None
            } else {
                usage::parse_model_from_json_or_sse_bytes(cli_key, body_bytes)
            }
        })
}

fn stream_transport_decision(
    kind: crate::settings::UpstreamTransportRetryKind,
    policy: &crate::settings::UpstreamRetryPolicy,
    configured_retries_used: u32,
    retry_index: u32,
    max_attempts_per_provider: u32,
) -> (FailoverDecision, bool) {
    transient_failure_decision(
        false,
        RetryPolicyMatch::Transport(kind),
        policy,
        configured_retries_used,
        retry_index,
        max_attempts_per_provider,
    )
}

#[derive(Clone, Copy)]
struct EffectiveStreamIdleTimeout {
    duration: Option<Duration>,
    #[cfg(test)]
    seconds: Option<u32>,
    #[cfg(test)]
    source: &'static str,
}

fn resolve_effective_stream_idle_timeout(
    provider_seconds: Option<u32>,
    global_timeout: Option<Duration>,
) -> EffectiveStreamIdleTimeout {
    if let Some(seconds) = provider_seconds.filter(|seconds| *seconds > 0) {
        return EffectiveStreamIdleTimeout {
            duration: Some(Duration::from_secs(seconds as u64)),
            #[cfg(test)]
            seconds: Some(seconds),
            #[cfg(test)]
            source: "provider",
        };
    }

    EffectiveStreamIdleTimeout {
        duration: global_timeout,
        #[cfg(test)]
        seconds: global_timeout.map(|timeout| timeout.as_secs().min(u64::from(u32::MAX)) as u32),
        #[cfg(test)]
        source: "global",
    }
}

fn is_codex_responses_event_stream_path(cli_key: &str, path: &str) -> bool {
    cli_key == "codex"
        && matches!(
            path.trim_end_matches('/'),
            "/v1/responses" | "/responses" | "/v1/codex/responses"
        )
}

fn is_native_codex_responses_event_stream_path(
    cli_key: &str,
    path: &str,
    active_bridge_type: Option<&str>,
    provider_bridged: bool,
) -> bool {
    active_bridge_type.is_none()
        && !provider_bridged
        && is_codex_responses_event_stream_path(cli_key, path)
}

fn is_completion_sse_frame(event_name: &str, data: &serde_json::Value) -> bool {
    matches!(event_name, "response.completed")
        || data.get("type").and_then(serde_json::Value::as_str) == Some("response.completed")
        || data
            .get("status")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| matches!(status, "done" | "completed" | "success"))
        || data
            .get("response")
            .and_then(|response| response.get("status"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| matches!(status, "done" | "completed" | "success"))
}

fn is_terminal_error_sse_frame(event_name: &str, data: &serde_json::Value) -> bool {
    matches!(
        event_name,
        "error" | "response.error" | "response.failed" | "response.incomplete"
    ) || data
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|event_type| {
            matches!(
                event_type,
                "error" | "response.error" | "response.failed" | "response.incomplete"
            ) || event_type.ends_with(".error")
        })
        || data
            .get("status")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed" | "incomplete"))
        || data
            .get("response")
            .and_then(|response| response.get("status"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed" | "incomplete"))
}

enum BufferedStreamPrefixDecision {
    NeedMore,
    StartStreaming {
        guard_cap_reached: bool,
    },
    ProviderFailure {
        error_code: &'static str,
        evidence: Option<usage::StreamInternalErrorEvidence>,
    },
    FinalizeAsEmptyBody(&'static str),
}

#[derive(Default)]
struct BufferedStreamPrefixState {
    cursor: usize,
    meaningful_output_started_at: Option<Instant>,
    completion_seen: bool,
}

impl BufferedStreamPrefixState {
    fn guard_remaining(&self, guard: Duration) -> Option<Duration> {
        let started = self.meaningful_output_started_at?;
        Some(guard.saturating_sub(started.elapsed()))
    }
}

struct BufferedStreamPrefixConfig<'a> {
    cli_key: &'a str,
    path: &'a str,
    status: u16,
    active_bridge_type: Option<&'a str>,
    provider_bridged: bool,
    retry_policy: &'a crate::settings::UpstreamRetryPolicy,
    guard: Duration,
}

fn inspect_buffered_event_stream_prefix(
    config: &BufferedStreamPrefixConfig<'_>,
    state: &mut BufferedStreamPrefixState,
    raw: &[u8],
) -> BufferedStreamPrefixDecision {
    let inspect_empty_success = is_native_codex_responses_event_stream_path(
        config.cli_key,
        config.path,
        config.active_bridge_type,
        config.provider_bridged,
    );
    let buffer_cap_reached = if inspect_empty_success {
        raw.len() >= MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES
    } else {
        raw.len() > MAX_NON_SSE_BODY_BYTES
    };
    if buffer_cap_reached {
        return BufferedStreamPrefixDecision::StartStreaming {
            guard_cap_reached: inspect_empty_success,
        };
    }

    let mut saw_non_error_frame = false;

    while let Some(relative_end) =
        crate::gateway::proxy::sse::find_sse_event_end(&raw[state.cursor..])
    {
        let event_end = state.cursor + relative_end;
        let frame = match std::str::from_utf8(&raw[state.cursor..event_end]) {
            Ok(frame) => frame,
            Err(_) => {
                return BufferedStreamPrefixDecision::StartStreaming {
                    guard_cap_reached: false,
                }
            }
        };
        state.cursor = event_end;

        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame) else {
            continue;
        };
        if inspect_empty_success {
            if let Some(evidence) = usage::classify_codex_stream_internal_error(
                &event_name,
                &data,
                config.retry_policy.enabled && config.retry_policy.stream_internal_errors.enabled,
                &config.retry_policy.stream_internal_errors.retry_keywords,
                &config
                    .retry_policy
                    .stream_internal_errors
                    .non_retry_keywords,
                "buffered_before_commit",
            ) {
                // Capacity failures must never be committed to Codex. Disabling
                // configured retries changes the decision to provider switch,
                // but does not opt back into forwarding the upstream SSE error.
                if evidence.is_retryable()
                    || usage::is_codex_capacity_stream_internal_error(&event_name, &data)
                {
                    return BufferedStreamPrefixDecision::ProviderFailure {
                        error_code: GatewayErrorCode::Fake200.as_str(),
                        evidence: Some(evidence),
                    };
                }
                return BufferedStreamPrefixDecision::StartStreaming {
                    guard_cap_reached: false,
                };
            }
            if usage::has_codex_meaningful_output(&data)
                && state.meaningful_output_started_at.is_none()
            {
                state.meaningful_output_started_at = Some(Instant::now());
            }
            state.completion_seen |= is_completion_sse_frame(&event_name, &data);
        } else if is_terminal_error_sse_frame(&event_name, &data) {
            return {
                BufferedStreamPrefixDecision::FinalizeAsEmptyBody(
                    GatewayErrorCode::Fake200.as_str(),
                )
            };
        }

        saw_non_error_frame = true;
    }

    if inspect_empty_success && state.completion_seen {
        let mut tracker = usage::SseUsageTracker::new(config.cli_key);
        tracker.ingest_chunk(raw);
        let usage = tracker.finalize();
        if tracker.is_empty_success(config.path, config.status, usage.as_ref()) {
            return BufferedStreamPrefixDecision::ProviderFailure {
                error_code: GatewayErrorCode::EmptyResponse.as_str(),
                evidence: None,
            };
        }
        return BufferedStreamPrefixDecision::StartStreaming {
            guard_cap_reached: false,
        };
    }

    if inspect_empty_success {
        if state
            .meaningful_output_started_at
            .is_some_and(|started| started.elapsed() >= config.guard)
        {
            return BufferedStreamPrefixDecision::StartStreaming {
                guard_cap_reached: false,
            };
        }
    } else if saw_non_error_frame {
        return BufferedStreamPrefixDecision::StartStreaming {
            guard_cap_reached: false,
        };
    }

    BufferedStreamPrefixDecision::NeedMore
}

#[allow(clippy::too_many_arguments)]
async fn record_buffered_provider_failure<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    loop_state: LoopState<'_, R>,
    status: StatusCode,
    raw: &[u8],
    error_code: &'static str,
    mut evidence: Option<usage::StreamInternalErrorEvidence>,
    retry_state: &mut RetryLoopState,
) -> LoopControl {
    let dispatch_ownership = provider_ctx.dispatch_ownership;
    let probe_ownership = dispatch_ownership.filter(|ownership| ownership.is_probe());
    let probe_active = probe_ownership.is_some();
    let probe_trigger = probe_ownership
        .and_then(|ownership| ownership.probe_trigger())
        .map(|trigger| trigger.as_str());
    let probe_generation = probe_ownership.and_then(|ownership| ownership.probe_generation());
    crate::gateway::model_route_mapping::observe_model_route_from_bytes(
        crate::gateway::model_route_mapping::ModelRouteBytesInput {
            cli_key: ctx.cli_key.as_str(),
            requested_model: provider_ctx
                .active_requested_model
                .or(ctx.requested_model.as_deref()),
            response_bytes: raw,
            special_settings: ctx.special_settings,
            provider_id: provider_ctx.provider_id,
            provider_name: provider_ctx.provider_name_base.as_str(),
        },
    );

    let retry_policy = provider_ctx.upstream_retry_policy;
    let provider_max_attempts = provider_ctx.provider_max_attempts;
    let CommonCtx {
        state,
        trace_id,
        cli_key,
        provider_health_neutral,
        provider_cooldown_secs,
        upstream_first_byte_timeout_secs,
        ..
    } = ctx;
    let ProviderCtx {
        provider_id,
        provider_name_base,
        provider_base_url_base,
        auth_mode,
        provider_index,
        provider_bridged,
        session_reuse,
        active_requested_model,
        ..
    } = provider_ctx;
    let AttemptCtx {
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
        circuit_snapshot,
        ..
    } = loop_state;

    let category = ErrorCategory::ProviderError;
    let (mut decision, configured_retry) = if evidence
        .as_ref()
        .is_some_and(usage::StreamInternalErrorEvidence::is_retryable)
    {
        transient_failure_decision(
            false,
            RetryPolicyMatch::StreamInternalError,
            retry_policy,
            retry_state.configured_transient_retries_used,
            retry_index,
            provider_max_attempts,
        )
    } else {
        (FailoverDecision::SwitchProvider, false)
    };
    if configured_retry {
        retry_state.configured_transient_retries_used = retry_state
            .configured_transient_retries_used
            .saturating_add(1);
    }
    let effective_status =
        status_override::effective_status(Some(status.as_u16()), Some(error_code));
    let now_unix = now_unix_seconds() as i64;
    let quota_exhausted = error_code == GatewayErrorCode::Fake200.as_str()
        && upstream_client_error_rules::match_quota_exhausted(raw);
    let oauth_quota_exhausted = quota_exhausted && auth_mode == "oauth";
    let record_circuit_failure = should_record_circuit_failure(retry_policy, configured_retry);

    if oauth_quota_exhausted {
        if let Err(err) =
            provider_oauth_limits::save_exhausted_snapshot(&state.db, provider_id, None)
        {
            tracing::warn!(
                provider_id,
                "failed to save OAuth exhausted quota snapshot: {err}"
            );
        }
    }
    let change = if let Some(ownership) = probe_ownership {
        match ownership.record_probe_attempt_failure(
            now_unix,
            !oauth_quota_exhausted && !provider_health_neutral,
            Some(error_code),
        ) {
            Some(crate::circuit_breaker::ProbeCommitResult::Applied(change)) => Some(change),
            Some(crate::circuit_breaker::ProbeCommitResult::Stale(snapshot)) => {
                Some(crate::circuit_breaker::CircuitChange {
                    before: snapshot.clone(),
                    after: snapshot,
                    transition: None,
                })
            }
            None => None,
        }
    } else if oauth_quota_exhausted {
        None
    } else if record_circuit_failure {
        Some(provider_router::record_failure_and_emit_transition(
            provider_router::RecordCircuitArgs::from_state(
                state,
                trace_id.as_str(),
                cli_key.as_str(),
                provider_id,
                provider_name_base.as_str(),
                provider_base_url_base.as_str(),
                now_unix,
            )
            .with_trigger(Some(error_code), Some(upstream_first_byte_timeout_secs))
            .with_provider_health_neutral(provider_health_neutral),
        ))
    } else {
        None
    };
    if let Some(change) = &change {
        *circuit_snapshot = change.after.clone();
        if matches!(decision, FailoverDecision::RetrySameProvider)
            && matches!(
                change.after.state,
                crate::circuit_breaker::CircuitState::Open
            )
        {
            decision = FailoverDecision::SwitchProvider;
        }
    }

    if !probe_active
        && !oauth_quota_exhausted
        && provider_cooldown_secs > 0
        && matches!(
            decision,
            FailoverDecision::SwitchProvider | FailoverDecision::Abort
        )
    {
        *circuit_snapshot = provider_router::trigger_cooldown(
            state.circuit.as_ref(),
            provider_id,
            now_unix,
            provider_cooldown_secs,
            provider_health_neutral,
        );
    }

    let (circuit_state_after, circuit_failure_count, circuit_failure_threshold) =
        if let Some(change) = &change {
            (
                Some(change.after.state.as_str()),
                Some(change.after.failure_count),
                Some(change.after.failure_threshold),
            )
        } else if oauth_quota_exhausted {
            (None, None, None)
        } else {
            (
                None,
                Some(circuit_before.failure_count),
                Some(circuit_before.failure_threshold),
            )
        };
    let circuit_state_before = Some(
        change
            .as_ref()
            .map(|change| change.before.state.as_str())
            .unwrap_or(circuit_before.state.as_str()),
    );
    let outcome = if error_code == GatewayErrorCode::Fake200.as_str() {
        format!(
            "stream_internal_error: category={} code={} decision={}",
            category.as_str(),
            error_code,
            decision.as_str()
        )
    } else {
        format!(
            "empty_response: category={} code={} decision={}",
            category.as_str(),
            error_code,
            decision.as_str()
        )
    };
    if let Some(evidence) = evidence.as_mut() {
        evidence.set_disposition(match decision {
            FailoverDecision::RetrySameProvider => "retry_same_provider",
            FailoverDecision::SwitchProvider => "switch_provider",
            FailoverDecision::Abort => "retry_exhausted",
        });
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
        provider_bridged: Some(provider_bridged),
        error_category: Some(category.as_str()),
        error_code: Some(error_code),
        decision: Some(decision.as_str()),
        reason: Some(buffered_provider_failure_reason(
            error_code,
            quota_exhausted,
        )),
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
        probe_trigger,
        probe_result: probe_active.then_some("failed"),
        probe_generation,
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        timeout_secs: None,
        stream_internal_error: evidence,
        requested_upstream_model: active_requested_model.map(str::to_string),
    });

    emit_attempt_event_and_log(
        ctx,
        provider_ctx,
        attempt_ctx,
        outcome,
        effective_status,
        AttemptCircuitFields {
            state_before: circuit_state_before,
            state_after: circuit_state_after,
            failure_count: circuit_failure_count,
            failure_threshold: circuit_failure_threshold,
        },
    )
    .await;

    *last_outcome = Some(AttemptOutcome::new(category.as_str(), error_code));
    if matches!(decision, FailoverDecision::RetrySameProvider) {
        super::attempt_record::apply_configured_retry_backoff(
            decision,
            configured_retry_backoff_delay(retry_policy, configured_retry),
        )
        .await;
        LoopControl::ContinueRetry
    } else {
        if matches!(decision, FailoverDecision::SwitchProvider) {
            failed_provider_ids.insert(provider_id);
        }
        LoopControl::BreakRetry
    }
}

#[allow(clippy::too_many_arguments)]
async fn finalize_buffered_stream_error_response<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    loop_state: LoopState<'_, R>,
    status: StatusCode,
    mut response_headers: HeaderMap,
    raw: &[u8],
    initial_first_byte_ms: Option<u128>,
    error_code: &'static str,
) -> LoopControl {
    let common = CommonCtxOwned::from(ctx);
    let provider_ctx_owned = ProviderCtxOwned::from(provider_ctx);
    let AttemptCtx {
        retry_index,
        attempt_started_ms,
        attempt_started,
        circuit_before,
        ..
    } = attempt_ctx;
    let LoopState {
        attempts,
        last_outcome,
        circuit_snapshot,
        abort_guard,
        ..
    } = loop_state;
    let provider_id = provider_ctx_owned.provider_id;
    let provider_index = provider_ctx_owned.provider_index;
    let session_reuse = provider_ctx_owned.session_reuse;
    let probe_ownership = provider_ctx_owned
        .dispatch_ownership
        .as_ref()
        .filter(|ownership| ownership.is_probe());
    let probe_active = probe_ownership.is_some();
    let probe_trigger = probe_ownership
        .and_then(|ownership| ownership.probe_trigger())
        .map(|trigger| trigger.as_str());
    let probe_generation = probe_ownership.and_then(|ownership| ownership.probe_generation());
    let outcome = "success".to_string();

    attempts.push(FailoverAttempt {
        provider_id,
        provider_name: provider_ctx_owned.provider_name_base.clone(),
        base_url: provider_ctx_owned.provider_base_url_base.clone(),
        outcome: outcome.clone(),
        status: Some(status.as_u16()),
        provider_index: Some(provider_index),
        retry_index: Some(retry_index),
        session_reuse,
        error_category: None,
        error_code: None,
        decision: Some("success"),
        reason: None,
        selection_method: if probe_active {
            Some(dc::SELECTION_METHOD_CIRCUIT_PROBE)
        } else {
            dc::selection_method(provider_index, retry_index, session_reuse)
        },
        reason_code: Some(dc::success_reason_code(provider_index, retry_index)),
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(attempt_started.elapsed().as_millis()),
        circuit_state_before: Some(circuit_before.state.as_str()),
        circuit_state_after: None,
        circuit_failure_count: Some(circuit_before.failure_count),
        circuit_failure_threshold: Some(circuit_before.failure_threshold),
        probe: probe_active.then_some(true),
        probe_trigger,
        probe_result: probe_active.then_some("started"),
        probe_generation,
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        provider_bridged: Some(provider_ctx_owned.provider_bridged),
        timeout_secs: None,
        stream_internal_error: None,
        requested_upstream_model: provider_ctx_owned.active_requested_model.clone(),
    });

    emit_attempt_event_and_log_with_circuit_before(
        ctx,
        provider_ctx,
        attempt_ctx,
        outcome,
        Some(status.as_u16()),
    )
    .await;

    codex_service_tier::append_result_if_detected(
        common.cli_key.as_str(),
        common.introspection_body.as_slice(),
        None,
        &common.special_settings,
    );

    let requested_model_for_log = if common.managed_model_route.is_some()
        || response_fixer::has_configured_model_route(&common.special_settings)
    {
        requested_model_for_audit(
            &common.special_settings,
            common.managed_model_route.as_ref(),
            common.requested_model.as_deref(),
            provider_ctx_owned.active_requested_model.as_deref(),
        )
    } else {
        resolve_requested_model_for_log(
            common.requested_model.clone(),
            provider_ctx_owned.active_requested_model.as_deref(),
            common.cli_key.as_str(),
            raw,
        )
    };
    crate::gateway::model_route_mapping::observe_model_route_from_bytes(
        crate::gateway::model_route_mapping::ModelRouteBytesInput {
            cli_key: common.cli_key.as_str(),
            requested_model: provider_ctx_owned
                .active_requested_model
                .as_deref()
                .or(common.requested_model.as_deref()),
            response_bytes: raw,
            special_settings: &common.special_settings,
            provider_id,
            provider_name: provider_ctx_owned.provider_name_base.as_str(),
        },
    );

    let probe_commit = complete_probe_failure(
        &common,
        &provider_ctx_owned,
        !common.provider_health_neutral,
        Some(error_code),
    );
    if let Some(probe_commit) = probe_commit {
        *circuit_snapshot = probe_commit.snapshot;
    } else {
        let now_unix = now_unix_seconds() as i64;
        let change = provider_router::record_failure_and_emit_transition(
            provider_router::RecordCircuitArgs::from_state(
                common.state,
                common.trace_id.as_str(),
                common.cli_key.as_str(),
                provider_id,
                provider_ctx_owned.provider_name_base.as_str(),
                provider_ctx_owned.provider_base_url_base.as_str(),
                now_unix,
            )
            .with_trigger(
                Some(error_code),
                Some(common.upstream_first_byte_timeout_secs),
            )
            .with_provider_health_neutral(common.provider_health_neutral),
        );
        *circuit_snapshot = change.after;
    }
    if !probe_active && common.provider_cooldown_secs > 0 {
        let now_unix = now_unix_seconds() as i64;
        *circuit_snapshot = provider_router::trigger_cooldown(
            common.state.circuit.as_ref(),
            provider_id,
            now_unix,
            common.provider_cooldown_secs,
            common.provider_health_neutral,
        );
    }
    let effective_status = effective_terminal_failure_status(status, error_code);
    mark_last_stream_attempt_terminal_failure(
        attempts,
        effective_status,
        error_code,
        buffered_provider_failure_reason(error_code, false),
        attempt_started.elapsed().as_millis(),
        circuit_snapshot,
    );

    *last_outcome = Some(AttemptOutcome::new(
        ErrorCategory::ProviderError.as_str(),
        error_code,
    ));
    let duration_ms = common.started.elapsed().as_millis();
    emit_request_event_and_enqueue_request_log(
        RequestEndArgs::from_context(RequestEndContextArgs {
            deps: RequestEndDeps::new(
                &common.state.app,
                &common.state.db,
                &common.state.log_tx,
                &common.state.plugin_pipeline,
                &common.state.active_requests,
            ),
            trace_id: common.trace_id.as_str(),
            cli_key: common.cli_key.as_str(),
            method: common.method_hint.as_str(),
            path: common.forwarded_path.as_str(),
            observe: common.observe,
            query: common.query.as_deref(),
            excluded_from_stats: false,
            duration_ms,
            attempts: attempts.as_slice(),
            special_settings_json: response_fixer::special_settings_json(&common.special_settings),
            session_id: common.session_id.clone(),
            requested_model: requested_model_for_log,
            created_at_ms: common.created_at_ms,
            created_at: common.created_at,
        })
        .with_completion(RequestCompletion::failure_with_visible_ttfb(
            effective_status.as_u16(),
            Some(ErrorCategory::ProviderError.as_str()),
            error_code,
            initial_first_byte_ms,
            Some(duration_ms),
        )),
    )
    .await;

    response_headers.remove(header::CONTENT_LENGTH);
    response_headers.remove(header::CONTENT_ENCODING);
    let mut builder = Response::builder().status(status);
    for (k, v) in response_headers.iter() {
        builder = builder.header(k, v);
    }
    builder = builder.header("x-trace-id", common.trace_id.as_str());
    abort_guard.disarm();
    LoopControl::Return(match builder.body(Body::from(Bytes::new())) {
        Ok(response) => response,
        Err(_) => {
            let mut fallback = (
                StatusCode::INTERNAL_SERVER_ERROR,
                GatewayErrorCode::ResponseBuildError.as_str(),
            )
                .into_response();
            fallback.headers_mut().insert(
                "x-trace-id",
                HeaderValue::from_str(common.trace_id.as_str())
                    .unwrap_or(HeaderValue::from_static("unknown")),
            );
            fallback
        }
    })
}

fn buffered_provider_failure_reason(error_code: &str, quota_exhausted: bool) -> String {
    if error_code == GatewayErrorCode::Fake200.as_str() {
        if quota_exhausted {
            "successful HTTP status with quota exhausted SSE error event".to_string()
        } else {
            "successful HTTP status with SSE error event".to_string()
        }
    } else {
        "successful Codex Responses stream completed with no meaningful output and output_tokens=0"
            .to_string()
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_success_event_stream<R>(
    ctx: CommonCtx<'_, R>,
    _input: &RequestContext<R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    _prepared: PreparedProvider,
    loop_state: LoopState<'_, R>,
    retry_state: &mut RetryLoopState,
    resp: reqwest::Response,
    status: StatusCode,
    mut response_headers: HeaderMap,
) -> LoopControl
where
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    let common = CommonCtxOwned::from(ctx);
    let provider_ctx_owned = ProviderCtxOwned::from(provider_ctx);

    let upstream_first_byte_timeout_secs = common.upstream_first_byte_timeout_secs;
    let upstream_first_byte_timeout = common.upstream_first_byte_timeout;
    let effective_stream_idle_timeout = resolve_effective_stream_idle_timeout(
        provider_ctx_owned.stream_idle_timeout_seconds,
        common.upstream_stream_idle_timeout,
    );
    let upstream_stream_idle_timeout = effective_stream_idle_timeout.duration;
    let enable_response_fixer = common.enable_response_fixer;
    let response_fixer_stream_config = common.response_fixer_stream_config;
    let provider_max_attempts = provider_ctx_owned.provider_max_attempts;

    let provider_id = provider_ctx_owned.provider_id;
    let provider_index = provider_ctx_owned.provider_index;
    let session_reuse = provider_ctx_owned.session_reuse;
    let probe_ownership = provider_ctx_owned
        .dispatch_ownership
        .as_ref()
        .filter(|ownership| ownership.is_probe());
    let probe_active = probe_ownership.is_some();
    let probe_trigger = probe_ownership
        .and_then(|ownership| ownership.probe_trigger())
        .map(|trigger| trigger.as_str());
    let probe_generation = probe_ownership.and_then(|ownership| ownership.probe_generation());

    let AttemptCtx {
        retry_index,
        attempt_started_ms,
        attempt_started,
        circuit_before,
        gemini_oauth_response_mode,
        cx2cc_active,
        active_bridge_type,
        anthropic_stream_requested: _,
        ..
    } = attempt_ctx;
    let selection_method = if probe_active {
        Some(dc::SELECTION_METHOD_CIRCUIT_PROBE)
    } else {
        dc::selection_method(provider_index, retry_index, session_reuse)
    };
    let reason_code = dc::success_reason_code(provider_index, retry_index);

    let LoopState {
        attempts,
        failed_provider_ids,
        last_outcome,
        active_requested_model,
        circuit_snapshot,
        abort_guard,
    } = loop_state;

    if is_event_stream(&response_headers) {
        strip_hop_headers(&mut response_headers);
        tracing::info!(
            trace_id = %common.trace_id,
            provider_id,
            cx2cc_active,
            "handling successful upstream event-stream response"
        );
        if cx2cc_active {
            emit_gateway_log(
                &common.state.app,
                "info",
                "CX2CC_SUCCESS_EVENT_STREAM",
                format!(
                    "[CX2CC] handling successful upstream event-stream response trace_id={} provider_id={}",
                    common.trace_id, provider_id
                ),
            );
        }

        let should_gunzip = has_gzip_content_encoding(&response_headers);
        if should_gunzip {
            // Decode once: before the native Codex guard, or later for legacy SSE paths.
            response_headers.remove(header::CONTENT_ENCODING);
            response_headers.remove(header::CONTENT_LENGTH);
        }
        let decode_gzip_before_guard = should_gunzip
            && is_native_codex_responses_event_stream_path(
                common.cli_key.as_str(),
                common.forwarded_path.as_str(),
                active_bridge_type,
                provider_ctx_owned.provider_bridged,
            );
        let mut upstream = decode_event_stream(resp.bytes_stream(), decode_gzip_before_guard);

        enum FirstChunkProbe {
            Skipped,
            Ok(Option<Bytes>, Option<u128>),
            ReadError(reqwest::Error),
            Timeout,
        }

        let probe = match upstream_first_byte_timeout {
            Some(total) => {
                let elapsed = attempt_started.elapsed();
                if elapsed >= total {
                    FirstChunkProbe::Timeout
                } else {
                    let remaining = total - elapsed;
                    match tokio::time::timeout(remaining, next_event_stream_chunk(&mut upstream))
                        .await
                    {
                        Ok(Ok(Some(chunk))) => FirstChunkProbe::Ok(
                            Some(chunk),
                            Some(attempt_started.elapsed().as_millis()),
                        ),
                        Ok(Ok(None)) => FirstChunkProbe::Ok(None, None),
                        Ok(Err(err)) => FirstChunkProbe::ReadError(err),
                        Err(_) => FirstChunkProbe::Timeout,
                    }
                }
            }
            None => FirstChunkProbe::Skipped,
        };
        let probe_is_empty_event_stream = matches!(probe, FirstChunkProbe::Ok(None, None));

        let mut first_chunk: Option<Bytes> = None;
        let mut initial_first_byte_ms: Option<u128> = None;

        match probe {
            FirstChunkProbe::Ok(chunk, ttfb_ms) => {
                first_chunk = chunk;
                initial_first_byte_ms = ttfb_ms;
            }
            FirstChunkProbe::ReadError(err) => {
                let error_code = GatewayErrorCode::StreamError.as_str();
                let (decision, configured_retry) = stream_transport_decision(
                    crate::settings::UpstreamTransportRetryKind::Read,
                    &provider_ctx_owned.upstream_retry_policy,
                    retry_state.configured_transient_retries_used,
                    retry_index,
                    provider_max_attempts,
                );
                if configured_retry {
                    retry_state.configured_transient_retries_used = retry_state
                        .configured_transient_retries_used
                        .saturating_add(1);
                }

                let outcome = format!(
                    "stream_first_chunk_error: category={} code={} decision={} timeout_secs={}",
                    ErrorCategory::SystemError.as_str(),
                    error_code,
                    decision.as_str(),
                    upstream_first_byte_timeout_secs,
                );

                return record_system_failure_and_decide(RecordSystemFailureArgs {
                    ctx,
                    provider_ctx,
                    attempt_ctx,
                    loop_state: LoopState {
                        attempts,
                        failed_provider_ids,
                        last_outcome,
                        active_requested_model,
                        circuit_snapshot,
                        abort_guard,
                    },
                    status: Some(status.as_u16()),
                    error_code,
                    decision,
                    outcome,
                    reason: format!("first chunk read error (event-stream): {err}"),
                    record_circuit_failure: should_record_circuit_failure(
                        &provider_ctx_owned.upstream_retry_policy,
                        configured_retry,
                    ),
                    configured_retry_backoff: configured_retry_backoff_delay(
                        &provider_ctx_owned.upstream_retry_policy,
                        configured_retry,
                    ),
                    timeout_secs: Some(upstream_first_byte_timeout_secs),
                })
                .await;
            }
            FirstChunkProbe::Timeout => {
                let error_code = GatewayErrorCode::UpstreamTimeout.as_str();
                let (decision, configured_retry) = stream_transport_decision(
                    crate::settings::UpstreamTransportRetryKind::Timeout,
                    &provider_ctx_owned.upstream_retry_policy,
                    retry_state.configured_transient_retries_used,
                    retry_index,
                    provider_max_attempts,
                );
                if configured_retry {
                    retry_state.configured_transient_retries_used = retry_state
                        .configured_transient_retries_used
                        .saturating_add(1);
                }

                let outcome = format!(
                    "stream_first_byte_timeout: category={} code={} decision={} timeout_secs={}",
                    ErrorCategory::SystemError.as_str(),
                    error_code,
                    decision.as_str(),
                    upstream_first_byte_timeout_secs,
                );

                return record_system_failure_and_decide(RecordSystemFailureArgs {
                    ctx,
                    provider_ctx,
                    attempt_ctx,
                    loop_state: LoopState {
                        attempts,
                        failed_provider_ids,
                        last_outcome,
                        active_requested_model,
                        circuit_snapshot,
                        abort_guard,
                    },
                    status: Some(status.as_u16()),
                    error_code,
                    decision,
                    outcome,
                    reason: "first byte timeout (event-stream)".to_string(),
                    record_circuit_failure: should_record_circuit_failure(
                        &provider_ctx_owned.upstream_retry_policy,
                        configured_retry,
                    ),
                    configured_retry_backoff: configured_retry_backoff_delay(
                        &provider_ctx_owned.upstream_retry_policy,
                        configured_retry,
                    ),
                    timeout_secs: Some(upstream_first_byte_timeout_secs),
                })
                .await;
            }
            FirstChunkProbe::Skipped => {}
        }

        if upstream_first_byte_timeout.is_some()
            && first_chunk.is_none()
            && initial_first_byte_ms.is_none()
            && probe_is_empty_event_stream
        {
            let error_code = GatewayErrorCode::StreamError.as_str();
            let (decision, configured_retry) = stream_transport_decision(
                crate::settings::UpstreamTransportRetryKind::Read,
                &provider_ctx_owned.upstream_retry_policy,
                retry_state.configured_transient_retries_used,
                retry_index,
                provider_max_attempts,
            );
            if configured_retry {
                retry_state.configured_transient_retries_used = retry_state
                    .configured_transient_retries_used
                    .saturating_add(1);
            }

            let outcome = format!(
                "stream_first_chunk_eof: category={} code={} decision={} timeout_secs={}",
                ErrorCategory::SystemError.as_str(),
                error_code,
                decision.as_str(),
                upstream_first_byte_timeout_secs,
            );

            return record_system_failure_and_decide(RecordSystemFailureArgs {
                ctx,
                provider_ctx,
                attempt_ctx,
                loop_state: LoopState {
                    attempts,
                    failed_provider_ids,
                    last_outcome,
                    active_requested_model,
                    circuit_snapshot,
                    abort_guard,
                },
                status: Some(status.as_u16()),
                error_code,
                decision,
                outcome,
                reason: "upstream returned empty event-stream".to_string(),
                record_circuit_failure: should_record_circuit_failure(
                    &provider_ctx_owned.upstream_retry_policy,
                    configured_retry,
                ),
                configured_retry_backoff: configured_retry_backoff_delay(
                    &provider_ctx_owned.upstream_retry_policy,
                    configured_retry,
                ),
                timeout_secs: Some(upstream_first_byte_timeout_secs),
            })
            .await;
        }

        let mut buffered_prefix = first_chunk
            .take()
            .map(|chunk| chunk.to_vec())
            .unwrap_or_default();
        let mut prefix_state = BufferedStreamPrefixState::default();
        let prefix_config = BufferedStreamPrefixConfig {
            cli_key: common.cli_key.as_str(),
            path: common.forwarded_path.as_str(),
            status: status.as_u16(),
            active_bridge_type,
            provider_bridged: provider_ctx_owned.provider_bridged,
            retry_policy: &provider_ctx_owned.upstream_retry_policy,
            guard: common.stream_internal_error_guard,
        };
        loop {
            match inspect_buffered_event_stream_prefix(
                &prefix_config,
                &mut prefix_state,
                buffered_prefix.as_slice(),
            ) {
                BufferedStreamPrefixDecision::ProviderFailure {
                    error_code,
                    evidence,
                } => {
                    return record_buffered_provider_failure(
                        ctx,
                        provider_ctx,
                        attempt_ctx,
                        LoopState {
                            attempts,
                            failed_provider_ids,
                            last_outcome,
                            active_requested_model,
                            circuit_snapshot,
                            abort_guard,
                        },
                        status,
                        buffered_prefix.as_slice(),
                        error_code,
                        evidence,
                        retry_state,
                    )
                    .await;
                }
                BufferedStreamPrefixDecision::StartStreaming { guard_cap_reached } => {
                    if guard_cap_reached {
                        tracing::warn!(
                            trace_id = %common.trace_id,
                            provider_id,
                            buffered_bytes = buffered_prefix.len(),
                            cap_bytes = MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES,
                            "stream-internal-error guard reached its buffer cap; committing response"
                        );
                        response_fixer::push_special_setting(
                            &common.special_settings,
                            serde_json::json!({
                                "type": "stream_internal_error_guard",
                                "reason": "buffer_cap_reached",
                                "buffered_bytes": buffered_prefix.len(),
                                "cap_bytes": MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES,
                            }),
                        );
                    }
                    first_chunk =
                        (!buffered_prefix.is_empty()).then(|| Bytes::from(buffered_prefix));
                    break;
                }
                BufferedStreamPrefixDecision::FinalizeAsEmptyBody(error_code) => {
                    return finalize_buffered_stream_error_response(
                        ctx,
                        provider_ctx,
                        attempt_ctx,
                        LoopState {
                            attempts,
                            failed_provider_ids,
                            last_outcome,
                            active_requested_model,
                            circuit_snapshot,
                            abort_guard,
                        },
                        status,
                        response_headers,
                        buffered_prefix.as_slice(),
                        initial_first_byte_ms,
                        error_code,
                    )
                    .await;
                }
                BufferedStreamPrefixDecision::NeedMore => {}
            }

            let guard_remaining = prefix_state.guard_remaining(common.stream_internal_error_guard);
            let wait = match (upstream_stream_idle_timeout, guard_remaining) {
                (Some(idle), Some(guard)) => Some((idle.min(guard), guard <= idle)),
                (Some(idle), None) => Some((idle, false)),
                (None, Some(guard)) => Some((guard, true)),
                (None, None) => None,
            };
            let chunk_result = match wait {
                Some((wait, guard_timeout)) => {
                    match tokio::time::timeout(wait, next_event_stream_chunk(&mut upstream)).await {
                        Ok(result) => result,
                        Err(_) if guard_timeout => {
                            first_chunk =
                                (!buffered_prefix.is_empty()).then(|| Bytes::from(buffered_prefix));
                            break;
                        }
                        Err(_) => {
                            let error_code = GatewayErrorCode::UpstreamTimeout.as_str();
                            let (decision, configured_retry) = stream_transport_decision(
                                crate::settings::UpstreamTransportRetryKind::Timeout,
                                &provider_ctx_owned.upstream_retry_policy,
                                retry_state.configured_transient_retries_used,
                                retry_index,
                                provider_max_attempts,
                            );
                            if configured_retry {
                                retry_state.configured_transient_retries_used = retry_state
                                    .configured_transient_retries_used
                                    .saturating_add(1);
                            }
                            let timeout_secs = upstream_stream_idle_timeout
                                .map(|value| value.as_secs().min(u64::from(u32::MAX)) as u32);
                            let outcome = format!(
                                "stream_prefix_idle_timeout: category={} code={} decision={} timeout_secs={}",
                                ErrorCategory::SystemError.as_str(),
                                error_code,
                                decision.as_str(),
                                timeout_secs.unwrap_or_default(),
                            );

                            return record_system_failure_and_decide(RecordSystemFailureArgs {
                                ctx,
                                provider_ctx,
                                attempt_ctx,
                                loop_state: LoopState {
                                    attempts,
                                    failed_provider_ids,
                                    last_outcome,
                                    active_requested_model,
                                    circuit_snapshot,
                                    abort_guard,
                                },
                                status: Some(status.as_u16()),
                                error_code,
                                decision,
                                outcome,
                                reason: "event-stream idle timeout while inspecting prefix"
                                    .to_string(),
                                record_circuit_failure: should_record_circuit_failure(
                                    &provider_ctx_owned.upstream_retry_policy,
                                    configured_retry,
                                ),
                                configured_retry_backoff: configured_retry_backoff_delay(
                                    &provider_ctx_owned.upstream_retry_policy,
                                    configured_retry,
                                ),
                                timeout_secs,
                            })
                            .await;
                        }
                    }
                }
                None => next_event_stream_chunk(&mut upstream).await,
            };

            let next_chunk = match chunk_result {
                Ok(chunk) => chunk,
                Err(err) => {
                    let error_code = GatewayErrorCode::StreamError.as_str();
                    let (decision, configured_retry) = stream_transport_decision(
                        crate::settings::UpstreamTransportRetryKind::Read,
                        &provider_ctx_owned.upstream_retry_policy,
                        retry_state.configured_transient_retries_used,
                        retry_index,
                        provider_max_attempts,
                    );
                    if configured_retry {
                        retry_state.configured_transient_retries_used = retry_state
                            .configured_transient_retries_used
                            .saturating_add(1);
                    }
                    let outcome = format!(
                        "stream_prefix_read_error: category={} code={} decision={}",
                        ErrorCategory::SystemError.as_str(),
                        error_code,
                        decision.as_str(),
                    );

                    return record_system_failure_and_decide(RecordSystemFailureArgs {
                        ctx,
                        provider_ctx,
                        attempt_ctx,
                        loop_state: LoopState {
                            attempts,
                            failed_provider_ids,
                            last_outcome,
                            active_requested_model,
                            circuit_snapshot,
                            abort_guard,
                        },
                        status: Some(status.as_u16()),
                        error_code,
                        decision,
                        outcome,
                        reason: format!("failed to inspect event-stream prefix: {err}"),
                        record_circuit_failure: should_record_circuit_failure(
                            &provider_ctx_owned.upstream_retry_policy,
                            configured_retry,
                        ),
                        configured_retry_backoff: configured_retry_backoff_delay(
                            &provider_ctx_owned.upstream_retry_policy,
                            configured_retry,
                        ),
                        timeout_secs: None,
                    })
                    .await;
                }
            };

            let Some(chunk) = next_chunk else {
                first_chunk = (!buffered_prefix.is_empty()).then(|| Bytes::from(buffered_prefix));
                break;
            };
            if initial_first_byte_ms.is_none() {
                initial_first_byte_ms = Some(attempt_started.elapsed().as_millis());
            }
            buffered_prefix.extend_from_slice(chunk.as_ref());
        }

        let outcome = "success".to_string();

        attempts.push(FailoverAttempt {
            provider_id,
            provider_name: provider_ctx_owned.provider_name_base.clone(),
            base_url: provider_ctx_owned.provider_base_url_base.clone(),
            outcome: outcome.clone(),
            status: Some(status.as_u16()),
            provider_index: Some(provider_index),
            retry_index: Some(retry_index),
            session_reuse,
            error_category: None,
            error_code: None,
            decision: Some("success"),
            reason: None,
            selection_method,
            reason_code: Some(reason_code),
            attempt_started_ms: Some(attempt_started_ms),
            attempt_duration_ms: Some(attempt_started.elapsed().as_millis()),
            circuit_state_before: Some(circuit_before.state.as_str()),
            circuit_state_after: None,
            circuit_failure_count: Some(circuit_before.failure_count),
            circuit_failure_threshold: Some(circuit_before.failure_threshold),
            probe: probe_active.then_some(true),
            probe_trigger,
            probe_result: probe_active.then_some("started"),
            probe_generation,
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: Some(provider_ctx_owned.provider_bridged),
            timeout_secs: None,
            stream_internal_error: None,
            requested_upstream_model: provider_ctx_owned.active_requested_model.clone(),
        });

        emit_attempt_event_and_log_with_circuit_before(
            ctx,
            provider_ctx,
            attempt_ctx,
            outcome,
            Some(status.as_u16()),
        )
        .await;

        codex_service_tier::append_result_if_detected(
            common.cli_key.as_str(),
            common.introspection_body.as_slice(),
            None,
            &common.special_settings,
        );

        let ctx = build_stream_finalize_ctx(
            &common,
            &provider_ctx_owned,
            attempts.as_slice(),
            status.as_u16(),
            attempt_started,
            is_native_codex_responses_event_stream_path(
                common.cli_key.as_str(),
                common.forwarded_path.as_str(),
                active_bridge_type,
                provider_ctx_owned.provider_bridged,
            ),
        );

        let enable_response_fixer_for_this_response =
            enable_response_fixer && !has_non_identity_content_encoding(&response_headers);

        if enable_response_fixer_for_this_response {
            response_headers.remove(header::CONTENT_LENGTH);
            response_headers.insert(
                "x-cch-response-fixer",
                HeaderValue::from_static("processed"),
            );
        }

        let use_sse_relay = is_codex_responses_event_stream_path(
            common.cli_key.as_str(),
            common.forwarded_path.as_str(),
        );
        let plugin_pipeline = common.state.plugin_pipeline.clone();
        let plugin_db = common.state.db.clone();
        let trace_id = common.trace_id.clone();
        let upstream_route_tracker = ctx.upstream_route_tracker.clone();
        let observed_upstream_model = ctx.observed_upstream_model.clone();
        let observed_upstream_conflicting_model = ctx.observed_upstream_conflicting_model.clone();
        let observed_upstream_reasoning_effort = ctx.observed_upstream_reasoning_effort.clone();
        let active_requested_model_for_bridge = provider_ctx_owned
            .active_requested_model
            .clone()
            .or_else(|| common.requested_model.clone());
        let upstream = prepend_and_decode_event_stream(
            first_chunk,
            upstream,
            should_gunzip && !decode_gzip_before_guard,
        );

        let body = if enable_response_fixer_for_this_response {
            let upstream =
                gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
            let upstream = UpstreamModelObserverStream::new(
                upstream,
                upstream_route_tracker.clone(),
                observed_upstream_model.clone(),
                observed_upstream_conflicting_model.clone(),
                observed_upstream_reasoning_effort.clone(),
            );
            let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type(
                upstream,
                active_bridge_type,
                active_requested_model_for_bridge.clone(),
                common.cx2cc_settings.clone(),
            );
            let upstream = response_fixer::ResponseFixerStream::new(
                upstream,
                response_fixer_stream_config,
                common.special_settings.clone(),
            );
            let upstream = MaybePluginChunkStream::new(
                upstream,
                plugin_pipeline.clone(),
                plugin_db.clone(),
                trace_id.clone(),
            );
            if use_sse_relay {
                spawn_usage_sse_relay_body(
                    upstream,
                    ctx,
                    upstream_stream_idle_timeout,
                    initial_first_byte_ms,
                )
            } else {
                let stream = UsageSseTeeStream::new(
                    upstream,
                    ctx,
                    upstream_stream_idle_timeout,
                    initial_first_byte_ms,
                );
                Body::from_stream(stream)
            }
        } else {
            let upstream =
                gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
            let upstream = UpstreamModelObserverStream::new(
                upstream,
                upstream_route_tracker,
                observed_upstream_model,
                observed_upstream_conflicting_model,
                observed_upstream_reasoning_effort,
            );
            let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type(
                upstream,
                active_bridge_type,
                active_requested_model_for_bridge.clone(),
                common.cx2cc_settings.clone(),
            );
            let upstream = MaybePluginChunkStream::new(
                upstream,
                plugin_pipeline.clone(),
                plugin_db.clone(),
                trace_id.clone(),
            );
            if use_sse_relay {
                spawn_usage_sse_relay_body(
                    upstream,
                    ctx,
                    upstream_stream_idle_timeout,
                    initial_first_byte_ms,
                )
            } else {
                let stream = UsageSseTeeStream::new(
                    upstream,
                    ctx,
                    upstream_stream_idle_timeout,
                    initial_first_byte_ms,
                );
                Body::from_stream(stream)
            }
        };

        let mut builder = Response::builder().status(status);
        for (k, v) in response_headers.iter() {
            builder = builder.header(k, v);
        }
        builder = builder.header("x-trace-id", common.trace_id.as_str());

        abort_guard.disarm();
        return LoopControl::Return(match builder.body(body) {
            Ok(r) => r,
            Err(_) => {
                let mut fallback = (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    GatewayErrorCode::ResponseBuildError.as_str(),
                )
                    .into_response();
                fallback.headers_mut().insert(
                    "x-trace-id",
                    HeaderValue::from_str(common.trace_id.as_str())
                        .unwrap_or(HeaderValue::from_static("unknown")),
                );
                fallback
            }
        });
    }

    unreachable!("expected event-stream response")
}

#[cfg(test)]
mod tests {
    use super::{
        effective_terminal_failure_status, inspect_buffered_event_stream_prefix,
        is_native_codex_responses_event_stream_path, mark_last_stream_attempt_terminal_failure,
        resolve_effective_stream_idle_timeout, resolve_requested_model_for_log,
        BufferedStreamPrefixConfig, BufferedStreamPrefixDecision, BufferedStreamPrefixState,
        MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES,
    };
    use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
    use crate::gateway::events::FailoverAttempt;
    use crate::gateway::proxy::{ErrorCategory, GatewayErrorCode};
    use axum::http::StatusCode;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    fn native_prefix_config(
        retry_policy: &crate::settings::UpstreamRetryPolicy,
        guard: Duration,
    ) -> BufferedStreamPrefixConfig<'_> {
        BufferedStreamPrefixConfig {
            cli_key: "codex",
            path: "/v1/responses",
            status: 200,
            active_bridge_type: None,
            provider_bridged: false,
            retry_policy,
            guard,
        }
    }

    fn started_probe_attempt() -> FailoverAttempt {
        FailoverAttempt {
            provider_id: 1,
            provider_name: "test-provider".to_string(),
            base_url: "https://upstream.example".to_string(),
            outcome: "success".to_string(),
            status: Some(200),
            provider_index: Some(1),
            retry_index: Some(1),
            session_reuse: Some(false),
            error_category: None,
            error_code: None,
            decision: Some("success"),
            reason: None,
            selection_method: Some("circuit_probe"),
            reason_code: Some("request_success"),
            attempt_started_ms: Some(0),
            attempt_duration_ms: Some(1),
            circuit_state_before: Some("OPEN"),
            circuit_state_after: None,
            circuit_failure_count: Some(1),
            circuit_failure_threshold: Some(1),
            probe: Some(true),
            probe_trigger: Some("aggressive_turn"),
            probe_result: Some("started"),
            probe_generation: Some(1),
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: Some(false),
            timeout_secs: None,
            requested_upstream_model: Some("gpt-5".to_string()),
            stream_internal_error: None,
        }
    }

    #[test]
    fn buffered_terminal_failure_rewrites_all_started_attempt_fields() {
        let circuit = Arc::new(CircuitBreaker::new(
            CircuitBreakerConfig {
                failure_threshold: 1,
                ..CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        let snapshot = circuit
            .record_failure(1, 1_700_000_000, Some(GatewayErrorCode::Fake200.as_str()))
            .after;
        let mut attempts = vec![started_probe_attempt()];
        let status =
            effective_terminal_failure_status(StatusCode::OK, GatewayErrorCode::Fake200.as_str());

        mark_last_stream_attempt_terminal_failure(
            &mut attempts,
            status,
            GatewayErrorCode::Fake200.as_str(),
            "successful HTTP status with SSE error event".to_string(),
            37,
            &snapshot,
        );

        let attempt = &attempts[0];
        assert_ne!(attempt.outcome, "success");
        assert_eq!(attempt.status, Some(502));
        assert_eq!(attempt.error_category, Some("PROVIDER_ERROR"));
        assert_eq!(attempt.error_code, Some(GatewayErrorCode::Fake200.as_str()));
        assert_eq!(attempt.decision, Some("abort"));
        assert_eq!(
            attempt.reason.as_deref(),
            Some("successful HTTP status with SSE error event")
        );
        assert_eq!(
            attempt.reason_code,
            Some(ErrorCategory::ProviderError.reason_code())
        );
        assert_eq!(attempt.attempt_duration_ms, Some(37));
        assert_eq!(attempt.probe_result, Some("failed"));
        assert_eq!(attempt.circuit_state_after, Some("OPEN"));
        assert_eq!(attempt.circuit_failure_count, Some(1));
        assert_eq!(attempt.circuit_failure_threshold, Some(1));
    }

    #[test]
    fn resolve_requested_model_for_log_prefers_fallback_model() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-5.4-mini\",\"status\":\"in_progress\",\"output\":[]}}\n\n"
        );

        let requested_model = resolve_requested_model_for_log(
            Some("gpt-5.5".to_string()),
            Some("gpt-5.4"),
            "codex",
            raw.as_bytes(),
        );

        assert_eq!(requested_model.as_deref(), Some("gpt-5.4"));
    }

    #[test]
    fn resolve_requested_model_for_log_falls_back_to_sse_payload_model() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-5.4-mini\",\"status\":\"in_progress\",\"output\":[]}}\n\n"
        );

        let requested_model = resolve_requested_model_for_log(None, None, "codex", raw.as_bytes());

        assert_eq!(requested_model.as_deref(), Some("gpt-5.4-mini"));
    }

    #[test]
    fn effective_stream_idle_timeout_uses_one_policy_for_execution_and_diagnostics() {
        let provider_override =
            resolve_effective_stream_idle_timeout(Some(90), Some(Duration::from_secs(300)));
        assert_eq!(provider_override.duration, Some(Duration::from_secs(90)));
        assert_eq!(provider_override.seconds, Some(90));
        assert_eq!(provider_override.source, "provider");

        let provider_zero_inherits_global =
            resolve_effective_stream_idle_timeout(Some(0), Some(Duration::from_secs(300)));
        assert_eq!(
            provider_zero_inherits_global.duration,
            Some(Duration::from_secs(300))
        );
        assert_eq!(provider_zero_inherits_global.seconds, Some(300));
        assert_eq!(provider_zero_inherits_global.source, "global");

        let disabled_global = resolve_effective_stream_idle_timeout(None, None);
        assert_eq!(disabled_global.duration, None);
        assert_eq!(disabled_global.seconds, None);
        assert_eq!(disabled_global.source, "global");
    }

    #[test]
    fn stream_internal_error_detection_is_native_codex_responses_only() {
        assert!(is_native_codex_responses_event_stream_path(
            "codex",
            "/v1/responses",
            None,
            false,
        ));
        assert!(is_native_codex_responses_event_stream_path(
            "codex",
            "/responses",
            None,
            false,
        ));
        assert!(is_native_codex_responses_event_stream_path(
            "codex",
            "/v1/codex/responses",
            None,
            false,
        ));
        assert!(!is_native_codex_responses_event_stream_path(
            "claude",
            "/v1/responses",
            None,
            false,
        ));
        assert!(!is_native_codex_responses_event_stream_path(
            "codex",
            "/v1/chat/completions",
            None,
            false,
        ));
        assert!(!is_native_codex_responses_event_stream_path(
            "codex",
            "/v1/responses",
            Some("cx2cc"),
            false,
        ));
        assert!(!is_native_codex_responses_event_stream_path(
            "codex",
            "/v1/responses",
            None,
            true,
        ));
    }

    #[test]
    fn buffered_native_stream_retries_capacity_before_commit() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\n",
            "event: response.in_progress\n",
            "data: {\"type\":\"response.in_progress\",\"response\":{\"id\":\"resp_1\"}}\n\n",
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"Selected model is at capacity\"}}\n\n"
        )
        .as_bytes();
        let mut state = BufferedStreamPrefixState::default();
        let decision = inspect_buffered_event_stream_prefix(
            &native_prefix_config(
                &crate::settings::UpstreamRetryPolicy::default(),
                Duration::from_millis(500),
            ),
            &mut state,
            raw,
        );

        match decision {
            BufferedStreamPrefixDecision::ProviderFailure {
                error_code,
                evidence: Some(evidence),
            } => {
                assert_eq!(error_code, "GW_FAKE_200");
                assert_eq!(evidence.classification, "retryable");
                assert_eq!(
                    evidence.message.as_deref(),
                    Some("Selected model is at capacity")
                );
                assert_eq!(evidence.matched_keyword, None);
            }
            _ => panic!("capacity terminal event should be retried before commit"),
        }
    }

    #[test]
    fn buffered_native_stream_retries_builtin_capacity_codes_before_commit() {
        let cases = [
            (
                "response.failed",
                r#"{"type":"response.failed","error":{"code":"SERVER_IS_OVERLOADED"}}"#,
                "SERVER_IS_OVERLOADED",
            ),
            (
                "response.error",
                r#"{"type":"response.error","response":{"error":{"code":"SlOw_DoWn"}}}"#,
                "SlOw_DoWn",
            ),
            (
                "error",
                r#"{"type":"error","code":"server_is_overloaded"}"#,
                "server_is_overloaded",
            ),
        ];

        for (event_name, data, expected_code) in cases {
            let raw = format!("event: {event_name}\ndata: {data}\n\n");
            let mut state = BufferedStreamPrefixState::default();
            let decision = inspect_buffered_event_stream_prefix(
                &native_prefix_config(
                    &crate::settings::UpstreamRetryPolicy::default(),
                    Duration::from_millis(500),
                ),
                &mut state,
                raw.as_bytes(),
            );

            match decision {
                BufferedStreamPrefixDecision::ProviderFailure {
                    error_code,
                    evidence: Some(evidence),
                } => {
                    assert_eq!(error_code, "GW_FAKE_200");
                    assert_eq!(evidence.classification, "retryable");
                    assert_eq!(evidence.error_code.as_deref(), Some(expected_code));
                    assert!(evidence.matched_keyword.is_none());
                }
                _ => panic!("built-in capacity code should be retried before commit"),
            }
        }
    }

    #[test]
    fn buffered_native_stream_forwards_nonretry_and_unknown_terminal_errors() {
        let terminal_errors = [
            r#"{"error":{"message":"content policy rejected"}}"#,
            r#"{"error":{"message":"unrecognized terminal failure"}}"#,
            r#"{"error":{"code":"service_unavailable_error"}}"#,
        ];
        for data in terminal_errors {
            let raw = format!("event: response.failed\ndata: {data}\n\n");
            let mut state = BufferedStreamPrefixState::default();
            assert!(matches!(
                inspect_buffered_event_stream_prefix(
                    &native_prefix_config(
                        &crate::settings::UpstreamRetryPolicy::default(),
                        Duration::from_millis(500),
                    ),
                    &mut state,
                    raw.as_bytes(),
                ),
                BufferedStreamPrefixDecision::StartStreaming {
                    guard_cap_reached: false
                }
            ));
        }
    }

    #[test]
    fn buffered_native_stream_intercepts_capacity_when_retries_are_disabled() {
        let disabled_policy = crate::settings::UpstreamRetryPolicy {
            enabled: false,
            stream_internal_errors: crate::settings::UpstreamStreamInternalErrorPolicy {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut state = BufferedStreamPrefixState::default();
        let decision = inspect_buffered_event_stream_prefix(
            &native_prefix_config(&disabled_policy, Duration::from_millis(500)),
            &mut state,
            b"event: response.failed\ndata: {\"response\":{\"error\":{\"code\":\"SLOW_DOWN\"}}}\n\n",
        );

        match decision {
            BufferedStreamPrefixDecision::ProviderFailure {
                error_code,
                evidence: Some(evidence),
            } => {
                assert_eq!(error_code, "GW_FAKE_200");
                assert_eq!(evidence.classification, "disabled");
                assert_eq!(evidence.error_code.as_deref(), Some("SLOW_DOWN"));
                assert_eq!(evidence.disposition, "buffered_before_commit");
            }
            _ => panic!("capacity terminal event must not reach Codex when retries are disabled"),
        }
    }

    #[test]
    fn buffered_native_stream_does_not_promote_legacy_broad_terminal_signals() {
        let raw = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            "event: response.custom.error\n",
            "data: {\"type\":\"response.custom.error\",\"status\":\"failed\",\"error\":{\"message\":\"Selected model is at capacity\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );
        let mut state = BufferedStreamPrefixState::default();

        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(
                    &crate::settings::UpstreamRetryPolicy::default(),
                    Duration::from_millis(500),
                ),
                &mut state,
                raw.as_bytes(),
            ),
            BufferedStreamPrefixDecision::StartStreaming {
                guard_cap_reached: false
            }
        ));
    }

    #[test]
    fn buffered_native_stream_commits_completed_output_without_waiting_for_guard() {
        let raw = concat!(
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}]}}\n\n"
        );
        let mut state = BufferedStreamPrefixState::default();
        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(
                    &crate::settings::UpstreamRetryPolicy::default(),
                    Duration::from_secs(5),
                ),
                &mut state,
                raw.as_bytes(),
            ),
            BufferedStreamPrefixDecision::StartStreaming {
                guard_cap_reached: false
            }
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn buffered_native_stream_waits_through_preamble_then_commits_at_guard_or_cap() {
        let policy = crate::settings::UpstreamRetryPolicy::default();
        let mut preamble_state = BufferedStreamPrefixState::default();
        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(&policy, Duration::from_millis(500)),
                &mut preamble_state,
                b": keepalive\n\n",
            ),
            BufferedStreamPrefixDecision::NeedMore
        ));

        let meaningful = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
        let mut guarded_prefix = b": keepalive\n\n".to_vec();
        guarded_prefix.extend_from_slice(meaningful);
        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(&policy, Duration::from_millis(500)),
                &mut preamble_state,
                &guarded_prefix,
            ),
            BufferedStreamPrefixDecision::NeedMore
        ));

        tokio::time::advance(Duration::from_millis(499)).await;
        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(&policy, Duration::from_millis(500)),
                &mut preamble_state,
                &guarded_prefix,
            ),
            BufferedStreamPrefixDecision::NeedMore
        ));

        tokio::time::advance(Duration::from_millis(1)).await;
        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(&policy, Duration::from_millis(500)),
                &mut preamble_state,
                &guarded_prefix,
            ),
            BufferedStreamPrefixDecision::StartStreaming {
                guard_cap_reached: false
            }
        ));

        let mut cap_state = BufferedStreamPrefixState::default();
        let oversized = vec![b' '; MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES + 1];
        assert!(matches!(
            inspect_buffered_event_stream_prefix(
                &native_prefix_config(&policy, Duration::from_millis(500)),
                &mut cap_state,
                &oversized,
            ),
            BufferedStreamPrefixDecision::StartStreaming {
                guard_cap_reached: true
            }
        ));
    }
}
