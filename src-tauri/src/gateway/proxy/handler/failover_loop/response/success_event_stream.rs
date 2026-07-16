//! Usage: Handle successful event-stream upstream responses inside `failover_loop::run`.

use super::attempt_executor::RetryLoopState;
use super::provider_iterator::PreparedProvider;
use super::upstream_retry_policy::{
    should_record_circuit_failure, transient_failure_decision, RetryPolicyMatch,
};
use super::*;
use crate::domain::provider_oauth_limits;
use crate::gateway::proxy::gemini_oauth;
use crate::gateway::proxy::protocol_bridge;
use crate::gateway::proxy::provider_router;
use crate::gateway::proxy::request_context::RequestContext;
use crate::gateway::proxy::status_override;
use crate::gateway::proxy::upstream_client_error_rules;
use std::time::Duration;

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
    retry_index: u32,
    max_attempts_per_provider: u32,
) -> (FailoverDecision, bool) {
    transient_failure_decision(
        false,
        RetryPolicyMatch::Transport(kind),
        policy,
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
) -> bool {
    active_bridge_type.is_none() && is_codex_responses_event_stream_path(cli_key, path)
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
    StartStreaming,
    ProviderFailure(&'static str),
    FinalizeAsEmptyBody(&'static str),
}

fn inspect_buffered_event_stream_prefix(
    cli_key: &str,
    path: &str,
    status: u16,
    active_bridge_type: Option<&str>,
    raw: &[u8],
) -> BufferedStreamPrefixDecision {
    if raw.len() > MAX_NON_SSE_BODY_BYTES {
        return BufferedStreamPrefixDecision::StartStreaming;
    }

    let inspect_empty_success =
        is_native_codex_responses_event_stream_path(cli_key, path, active_bridge_type);
    let mut cursor = 0usize;
    let mut saw_non_error_frame = false;
    let mut saw_completion_frame = false;

    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = match std::str::from_utf8(&raw[cursor..event_end]) {
            Ok(frame) => frame,
            Err(_) => return BufferedStreamPrefixDecision::StartStreaming,
        };
        cursor = event_end;

        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame) else {
            continue;
        };
        if is_terminal_error_sse_frame(&event_name, &data) {
            return if inspect_empty_success {
                BufferedStreamPrefixDecision::ProviderFailure(GatewayErrorCode::Fake200.as_str())
            } else {
                BufferedStreamPrefixDecision::FinalizeAsEmptyBody(
                    GatewayErrorCode::Fake200.as_str(),
                )
            };
        }

        saw_non_error_frame = true;
        if inspect_empty_success {
            saw_completion_frame |= is_completion_sse_frame(&event_name, &data);
        }
    }

    if inspect_empty_success && saw_completion_frame {
        let mut tracker = usage::SseUsageTracker::new(cli_key);
        tracker.ingest_chunk(raw);
        let usage = tracker.finalize();
        if tracker.fake_200_detected() || tracker.terminal_error_seen() {
            return BufferedStreamPrefixDecision::ProviderFailure(
                GatewayErrorCode::Fake200.as_str(),
            );
        }
        if tracker.is_empty_success(path, status, usage.as_ref()) {
            return BufferedStreamPrefixDecision::ProviderFailure(
                GatewayErrorCode::EmptyResponse.as_str(),
            );
        }
        return BufferedStreamPrefixDecision::StartStreaming;
    }

    if saw_non_error_frame {
        return BufferedStreamPrefixDecision::StartStreaming;
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
) -> LoopControl {
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
        ..
    } = provider_ctx;
    let AttemptCtx {
        retry_index,
        attempt_started_ms,
        attempt_started,
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
    let decision = FailoverDecision::SwitchProvider;
    let effective_status =
        status_override::effective_status(Some(status.as_u16()), Some(error_code));
    let now_unix = now_unix_seconds() as i64;
    let quota_exhausted = error_code == GatewayErrorCode::Fake200.as_str()
        && upstream_client_error_rules::match_quota_exhausted(raw);
    let oauth_quota_exhausted = quota_exhausted && auth_mode == "oauth";
    let outcome = if error_code == GatewayErrorCode::Fake200.as_str() {
        format!("stream_error: code={error_code}")
    } else {
        format!(
            "empty_response: category={} code={} decision={}",
            category.as_str(),
            error_code,
            decision.as_str()
        )
    };

    let change = if oauth_quota_exhausted {
        if let Err(err) =
            provider_oauth_limits::save_exhausted_snapshot(&state.db, provider_id, None)
        {
            tracing::warn!(
                provider_id,
                "failed to save OAuth exhausted quota snapshot: {err}"
            );
        }
        None
    } else {
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
    };
    if let Some(change) = &change {
        *circuit_snapshot = change.after.clone();
    }

    if !oauth_quota_exhausted && provider_cooldown_secs > 0 {
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
        } else {
            (None, None, None)
        };
    let circuit_state_before = change.as_ref().map(|change| change.before.state.as_str());

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
        selection_method: dc::selection_method(provider_index, retry_index, session_reuse),
        reason_code: Some(category.reason_code()),
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(attempt_started.elapsed().as_millis()),
        circuit_state_before,
        circuit_state_after,
        circuit_failure_count,
        circuit_failure_threshold,
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        timeout_secs: None,
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

    failed_provider_ids.insert(provider_id);
    *last_outcome = Some(AttemptOutcome::new(category.as_str(), error_code));
    LoopControl::BreakRetry
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
        selection_method: dc::selection_method(provider_index, retry_index, session_reuse),
        reason_code: Some(dc::success_reason_code(provider_index, retry_index)),
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(attempt_started.elapsed().as_millis()),
        circuit_state_before: Some(circuit_before.state.as_str()),
        circuit_state_after: None,
        circuit_failure_count: Some(circuit_before.failure_count),
        circuit_failure_threshold: Some(circuit_before.failure_threshold),
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        provider_bridged: Some(provider_ctx_owned.provider_bridged),
        timeout_secs: None,
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

    let requested_model_for_log = resolve_requested_model_for_log(
        common.requested_model.clone(),
        provider_ctx_owned.active_requested_model.as_deref(),
        common.cli_key.as_str(),
        raw,
    );
    let actual_model = usage::parse_model_from_json_or_sse_bytes(common.cli_key.as_str(), raw);
    let actual_reasoning_effort =
        usage::parse_reasoning_effort_from_json_or_sse_bytes(common.cli_key.as_str(), raw);
    if let Some(setting) =
        crate::gateway::model_route_mapping::build_model_route_mapping_setting_from_shared(
            common.cli_key.as_str(),
            provider_ctx_owned
                .active_requested_model
                .as_deref()
                .or(common.requested_model.as_deref()),
            actual_model.as_deref(),
            actual_reasoning_effort.as_deref(),
            &common.special_settings,
            provider_id,
            provider_ctx_owned.provider_name_base.as_str(),
        )
    {
        response_fixer::push_model_route_mapping_special_setting(&common.special_settings, setting);
    }

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
    *circuit_snapshot = change.after.clone();
    if common.provider_cooldown_secs > 0 {
        *circuit_snapshot = provider_router::trigger_cooldown(
            common.state.circuit.as_ref(),
            provider_id,
            now_unix,
            common.provider_cooldown_secs,
            common.provider_health_neutral,
        );
    }
    if let Some(last) = attempts.last_mut() {
        last.outcome = format!("stream_error: code={error_code}");
        last.error_category = Some(ErrorCategory::ProviderError.as_str());
        last.error_code = Some(error_code);
        last.attempt_duration_ms = Some(attempt_started.elapsed().as_millis());
        last.circuit_state_after = Some(circuit_snapshot.state.as_str());
        last.circuit_failure_count = Some(circuit_snapshot.failure_count);
        last.circuit_failure_threshold = Some(circuit_snapshot.failure_threshold);
    }

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
            status.as_u16(),
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
    _retry_state: &mut RetryLoopState,
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

    let AttemptCtx {
        retry_index,
        attempt_started_ms,
        attempt_started,
        circuit_before,
        gemini_oauth_response_mode,
        cx2cc_active,
        active_bridge_type,
        responses_cache_namespace,
        responses_cache_input,
        anthropic_stream_requested: _,
        ..
    } = attempt_ctx;
    let selection_method = dc::selection_method(provider_index, retry_index, session_reuse);
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

        let mut resp = resp;

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
                    match tokio::time::timeout(remaining, resp.chunk()).await {
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
                    retry_index,
                    provider_max_attempts,
                );

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
                    timeout_secs: Some(upstream_first_byte_timeout_secs),
                })
                .await;
            }
            FirstChunkProbe::Timeout => {
                let error_code = GatewayErrorCode::UpstreamTimeout.as_str();
                let (decision, configured_retry) = stream_transport_decision(
                    crate::settings::UpstreamTransportRetryKind::Timeout,
                    &provider_ctx_owned.upstream_retry_policy,
                    retry_index,
                    provider_max_attempts,
                );

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
                retry_index,
                provider_max_attempts,
            );

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
                timeout_secs: Some(upstream_first_byte_timeout_secs),
            })
            .await;
        }

        let mut buffered_prefix = first_chunk
            .take()
            .map(|chunk| chunk.to_vec())
            .unwrap_or_default();
        loop {
            match inspect_buffered_event_stream_prefix(
                common.cli_key.as_str(),
                common.forwarded_path.as_str(),
                status.as_u16(),
                active_bridge_type,
                buffered_prefix.as_slice(),
            ) {
                BufferedStreamPrefixDecision::ProviderFailure(error_code) => {
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
                    )
                    .await;
                }
                BufferedStreamPrefixDecision::StartStreaming => {
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

            let next_chunk = match upstream_stream_idle_timeout {
                Some(total) => match tokio::time::timeout(total, resp.chunk()).await {
                    Ok(Ok(chunk)) => chunk,
                    Ok(Err(err)) => {
                        let error_code = GatewayErrorCode::StreamError.as_str();
                        let (decision, configured_retry) = stream_transport_decision(
                            crate::settings::UpstreamTransportRetryKind::Read,
                            &provider_ctx_owned.upstream_retry_policy,
                            retry_index,
                            provider_max_attempts,
                        );
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
                            timeout_secs: None,
                        })
                        .await;
                    }
                    Err(_) => {
                        let error_code = GatewayErrorCode::UpstreamTimeout.as_str();
                        let (decision, configured_retry) = stream_transport_decision(
                            crate::settings::UpstreamTransportRetryKind::Timeout,
                            &provider_ctx_owned.upstream_retry_policy,
                            retry_index,
                            provider_max_attempts,
                        );
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
                            reason: "event-stream idle timeout while inspecting prefix".to_string(),
                            record_circuit_failure: should_record_circuit_failure(
                                &provider_ctx_owned.upstream_retry_policy,
                                configured_retry,
                            ),
                            timeout_secs,
                        })
                        .await;
                    }
                },
                None => match resp.chunk().await {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        let error_code = GatewayErrorCode::StreamError.as_str();
                        let (decision, configured_retry) = stream_transport_decision(
                            crate::settings::UpstreamTransportRetryKind::Read,
                            &provider_ctx_owned.upstream_retry_policy,
                            retry_index,
                            provider_max_attempts,
                        );
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
                            timeout_secs: None,
                        })
                        .await;
                    }
                },
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
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: Some(provider_ctx_owned.provider_bridged),
            timeout_secs: None,
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
            None,
            None,
            attempt_started,
        );

        let should_gunzip = has_gzip_content_encoding(&response_headers);
        if should_gunzip {
            // 上游可能无视 accept-encoding: identity 返回 gzip；
            response_headers.remove(header::CONTENT_ENCODING);
            response_headers.remove(header::CONTENT_LENGTH);
        }

        let enable_response_fixer_for_this_response =
            enable_response_fixer && !has_non_identity_content_encoding(&response_headers);

        if enable_response_fixer_for_this_response {
            response_headers.remove(header::CONTENT_LENGTH);
            response_headers.insert(
                "x-cch-response-fixer",
                HeaderValue::from_static("processed"),
            );
        }

        let use_sse_relay = common.cli_key == "codex"
            && matches!(
                common.forwarded_path.trim_end_matches('/'),
                "/v1/responses" | "/responses"
            );
        let plugin_pipeline = common.state.plugin_pipeline.clone();
        let plugin_db = common.state.db.clone();
        let trace_id = common.trace_id.clone();
        let upstream_route_tracker = ctx.upstream_route_tracker.clone();
        let observed_upstream_model = ctx.observed_upstream_model.clone();
        let observed_upstream_reasoning_effort = ctx.observed_upstream_reasoning_effort.clone();
        let active_requested_model_for_bridge = provider_ctx_owned
            .active_requested_model
            .clone()
            .or_else(|| common.requested_model.clone());

        let body = match (enable_response_fixer_for_this_response, should_gunzip) {
            (true, true) => {
                let upstream =
                    GunzipStream::new(FirstChunkStream::new(first_chunk, resp.bytes_stream()));
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = UpstreamModelObserverStream::new(
                    upstream,
                    upstream_route_tracker.clone(),
                    observed_upstream_model.clone(),
                    observed_upstream_reasoning_effort.clone(),
                );
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type_with_cache(
                    upstream,
                    active_bridge_type,
                    active_requested_model_for_bridge.clone(),
                    common.cx2cc_settings.clone(),
                    responses_cache_namespace.map(str::to_string),
                    responses_cache_input.map(|items| items.to_vec()),
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
            }
            (true, false) => {
                let upstream = FirstChunkStream::new(first_chunk, resp.bytes_stream());
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = UpstreamModelObserverStream::new(
                    upstream,
                    upstream_route_tracker.clone(),
                    observed_upstream_model.clone(),
                    observed_upstream_reasoning_effort.clone(),
                );
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type_with_cache(
                    upstream,
                    active_bridge_type,
                    active_requested_model_for_bridge.clone(),
                    common.cx2cc_settings.clone(),
                    responses_cache_namespace.map(str::to_string),
                    responses_cache_input.map(|items| items.to_vec()),
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
            }
            (false, true) => {
                let upstream =
                    GunzipStream::new(FirstChunkStream::new(first_chunk, resp.bytes_stream()));
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = UpstreamModelObserverStream::new(
                    upstream,
                    upstream_route_tracker.clone(),
                    observed_upstream_model.clone(),
                    observed_upstream_reasoning_effort.clone(),
                );
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type_with_cache(
                    upstream,
                    active_bridge_type,
                    active_requested_model_for_bridge.clone(),
                    common.cx2cc_settings.clone(),
                    responses_cache_namespace.map(str::to_string),
                    responses_cache_input.map(|items| items.to_vec()),
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
            }
            (false, false) => {
                let upstream = FirstChunkStream::new(first_chunk, resp.bytes_stream());
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = UpstreamModelObserverStream::new(
                    upstream,
                    upstream_route_tracker,
                    observed_upstream_model,
                    observed_upstream_reasoning_effort,
                );
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type_with_cache(
                    upstream,
                    active_bridge_type,
                    active_requested_model_for_bridge.clone(),
                    common.cx2cc_settings.clone(),
                    responses_cache_namespace.map(str::to_string),
                    responses_cache_input.map(|items| items.to_vec()),
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
    use super::{resolve_effective_stream_idle_timeout, resolve_requested_model_for_log};
    use std::time::Duration;

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
}
