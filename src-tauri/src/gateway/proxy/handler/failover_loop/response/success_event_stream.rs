//! Usage: Handle successful event-stream upstream responses inside `failover_loop::run`.

use super::attempt_executor::RetryLoopState;
use super::upstream_retry_policy::{
    should_record_circuit_failure, transient_failure_decision, RetryPolicyMatch,
};
use super::*;
use crate::gateway::proxy::gemini_oauth;
use crate::gateway::proxy::protocol_bridge;
use crate::gateway::proxy::provider_router;
use std::time::Duration;

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

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_success_event_stream<R>(
    ctx: CommonCtx<'_, R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
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

    let started = common.started;
    let upstream_first_byte_timeout_secs = common.upstream_first_byte_timeout_secs;
    let upstream_first_byte_timeout = common.upstream_first_byte_timeout;
    // Per-provider idle timeout overrides the global setting if configured.
    let upstream_stream_idle_timeout = provider_ctx_owned
        .stream_idle_timeout_seconds
        .and_then(|secs| {
            if secs > 0 {
                Some(Duration::from_secs(secs as u64))
            } else {
                None
            }
        })
        .or(common.upstream_stream_idle_timeout);
    let enable_response_fixer = common.enable_response_fixer;
    let response_fixer_stream_config = common.response_fixer_stream_config;
    let provider_max_attempts = provider_ctx_owned.provider_max_attempts;

    let provider_id = provider_ctx_owned.provider_id;
    let provider_index = provider_ctx_owned.provider_index;
    let session_reuse = provider_ctx_owned.session_reuse;

    let AttemptCtx {
        attempt_index: _,
        retry_index,
        attempt_started_ms,
        attempt_started,
        circuit_before,
        gemini_oauth_response_mode,
        cx2cc_active,
        active_bridge_type,
        anthropic_stream_requested: _,
    } = attempt_ctx;
    let selection_method = dc::selection_method(provider_index, retry_index, session_reuse);
    let reason_code = dc::success_reason_code(provider_index, retry_index);
    let should_buffer_codex_reasoning_guard = common.codex_reasoning_guard_enabled
        && common.cli_key == "codex"
        && matches!(
            common.forwarded_path.trim_end_matches('/'),
            "/v1/responses" | "/responses"
        );

    let LoopState {
        attempts,
        failed_provider_ids,
        last_outcome,
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
            })
            .await;
        }

        if should_buffer_codex_reasoning_guard {
            let mut raw = Vec::new();

            if let Some(chunk) = first_chunk.take() {
                raw.extend_from_slice(chunk.as_ref());
                if raw.len() > MAX_NON_SSE_BODY_BYTES {
                    let error_code = GatewayErrorCode::UpstreamBodyReadError.as_str();
                    let decision = FailoverDecision::SwitchProvider;
                    let outcome = format!(
                        "stream_buffer_too_large: category={} code={} decision={} limit_bytes={}",
                        ErrorCategory::SystemError.as_str(),
                        error_code,
                        decision.as_str(),
                        MAX_NON_SSE_BODY_BYTES,
                    );

                    return record_system_failure_and_decide(RecordSystemFailureArgs {
                        ctx,
                        provider_ctx,
                        attempt_ctx,
                        loop_state: LoopState {
                            attempts,
                            failed_provider_ids,
                            last_outcome,
                            circuit_snapshot,
                            abort_guard,
                        },
                        status: Some(status.as_u16()),
                        error_code,
                        decision,
                        outcome,
                        reason: format!(
                            "event-stream body exceeded gateway buffer limit ({} bytes)",
                            MAX_NON_SSE_BODY_BYTES
                        ),
                        record_circuit_failure: true,
                    })
                    .await;
                }
            }

            loop {
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
                                "stream_buffer_read_error: category={} code={} decision={}",
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
                                    circuit_snapshot,
                                    abort_guard,
                                },
                                status: Some(status.as_u16()),
                                error_code,
                                decision,
                                outcome,
                                reason: format!("failed to buffer event-stream body: {err}"),
                                record_circuit_failure: should_record_circuit_failure(
                                    &provider_ctx_owned.upstream_retry_policy,
                                    configured_retry,
                                ),
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
                            let outcome = format!(
                                "stream_buffer_idle_timeout: category={} code={} decision={} timeout_secs={}",
                                ErrorCategory::SystemError.as_str(),
                                error_code,
                                decision.as_str(),
                                upstream_stream_idle_timeout
                                    .map(|value| value.as_secs())
                                    .unwrap_or_default(),
                            );

                            return record_system_failure_and_decide(RecordSystemFailureArgs {
                                ctx,
                                provider_ctx,
                                attempt_ctx,
                                loop_state: LoopState {
                                    attempts,
                                    failed_provider_ids,
                                    last_outcome,
                                    circuit_snapshot,
                                    abort_guard,
                                },
                                status: Some(status.as_u16()),
                                error_code,
                                decision,
                                outcome,
                                reason: "event-stream idle timeout while buffering".to_string(),
                                record_circuit_failure: should_record_circuit_failure(
                                    &provider_ctx_owned.upstream_retry_policy,
                                    configured_retry,
                                ),
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
                                "stream_buffer_read_error: category={} code={} decision={}",
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
                                    circuit_snapshot,
                                    abort_guard,
                                },
                                status: Some(status.as_u16()),
                                error_code,
                                decision,
                                outcome,
                                reason: format!("failed to buffer event-stream body: {err}"),
                                record_circuit_failure: should_record_circuit_failure(
                                    &provider_ctx_owned.upstream_retry_policy,
                                    configured_retry,
                                ),
                            })
                            .await;
                        }
                    },
                };

                let Some(chunk) = next_chunk else {
                    break;
                };
                if initial_first_byte_ms.is_none() {
                    initial_first_byte_ms = Some(attempt_started.elapsed().as_millis());
                }
                raw.extend_from_slice(chunk.as_ref());
                if raw.len() > MAX_NON_SSE_BODY_BYTES {
                    let error_code = GatewayErrorCode::UpstreamBodyReadError.as_str();
                    let decision = FailoverDecision::SwitchProvider;
                    let outcome = format!(
                        "stream_buffer_too_large: category={} code={} decision={} limit_bytes={}",
                        ErrorCategory::SystemError.as_str(),
                        error_code,
                        decision.as_str(),
                        MAX_NON_SSE_BODY_BYTES,
                    );

                    return record_system_failure_and_decide(RecordSystemFailureArgs {
                        ctx,
                        provider_ctx,
                        attempt_ctx,
                        loop_state: LoopState {
                            attempts,
                            failed_provider_ids,
                            last_outcome,
                            circuit_snapshot,
                            abort_guard,
                        },
                        status: Some(status.as_u16()),
                        error_code,
                        decision,
                        outcome,
                        reason: format!(
                            "event-stream body exceeded gateway buffer limit ({} bytes)",
                            MAX_NON_SSE_BODY_BYTES
                        ),
                        record_circuit_failure: true,
                    })
                    .await;
                }
            }

            let raw = if has_gzip_content_encoding(&response_headers) {
                let mut headers_for_decode = response_headers.clone();
                let decoded = maybe_gunzip_response_body_bytes_with_limit(
                    Bytes::from(raw),
                    &mut headers_for_decode,
                    MAX_NON_SSE_BODY_BYTES,
                );
                response_headers = headers_for_decode;
                decoded
            } else {
                Bytes::from(raw)
            };

            let raw =
                if enable_response_fixer && !has_non_identity_content_encoding(&response_headers) {
                    response_headers.remove(header::CONTENT_LENGTH);
                    response_headers.insert(
                        "x-cch-response-fixer",
                        HeaderValue::from_static("processed"),
                    );
                    let fixer_outcome =
                        response_fixer::process_non_stream(raw, response_fixer_stream_config);
                    if let Some(setting) = fixer_outcome.special_setting {
                        response_fixer::push_special_setting(&common.special_settings, setting);
                    }
                    fixer_outcome.body
                } else {
                    raw
                };

            let aggregated = match protocol_bridge::stream::aggregate_responses_event_stream(
                raw.as_ref(),
            ) {
                Ok(value) => value,
                Err(err) => {
                    let error_code = GatewayErrorCode::InternalError.as_str();
                    let decision = FailoverDecision::SwitchProvider;
                    let outcome = format!(
                            "codex_event_stream_aggregate_error: category={} code={} decision={} err={err}",
                            ErrorCategory::SystemError.as_str(),
                            error_code,
                            decision.as_str(),
                        );

                    return record_system_failure_and_decide_no_cooldown(RecordSystemFailureArgs {
                        ctx,
                        provider_ctx,
                        attempt_ctx,
                        loop_state: LoopState {
                            attempts,
                            failed_provider_ids,
                            last_outcome,
                            circuit_snapshot,
                            abort_guard,
                        },
                        status: Some(status.as_u16()),
                        error_code,
                        decision,
                        outcome,
                        reason: format!("failed to aggregate Codex responses event-stream: {err}"),
                        record_circuit_failure: true,
                    })
                    .await;
                }
            };

            if let Some(matched) = codex_reasoning_guard::detect_from_json(
                common.cli_key.as_str(),
                common.requested_model.as_deref(),
                &aggregated,
                common.codex_reasoning_guard_compare_mode,
                common.codex_reasoning_guard_reasoning_equals.as_slice(),
                common.codex_reasoning_guard_model_rules.as_slice(),
            ) {
                let budget_decision = codex_reasoning_guard::budget_decision(
                    retry_state.codex_reasoning_guard_hits,
                    common.codex_reasoning_guard_immediate_retry_budget,
                    common.codex_reasoning_guard_delayed_retry_budget,
                    common.codex_reasoning_guard_delayed_retry_ms,
                    common.codex_reasoning_guard_exhausted_action,
                );
                codex_reasoning_guard::push_special_setting(
                    &common.special_settings,
                    provider_id,
                    provider_ctx_owned.provider_name_base.as_str(),
                    retry_index,
                    &matched,
                    budget_decision,
                );
                codex_reasoning_guard::record_guard_retry_attempt(
                    attempts,
                    provider_id,
                    provider_ctx_owned.provider_name_base.as_str(),
                    provider_ctx_owned.provider_base_url_base.as_str(),
                    provider_index,
                    retry_index,
                    session_reuse,
                    attempt_started_ms,
                    attempt_started.elapsed().as_millis(),
                    circuit_before.state.as_str(),
                    circuit_before.failure_count,
                    circuit_before.failure_threshold,
                    &matched,
                    budget_decision,
                );
                let outcome = match budget_decision.action {
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::RetrySameProvider => {
                        "codex_reasoning_guard_retry"
                    }
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::ReturnError => {
                        "codex_reasoning_guard_exhausted"
                    }
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::SwitchProvider => {
                        "codex_reasoning_guard_switch_provider"
                    }
                };
                emit_attempt_event_and_log(
                    ctx,
                    provider_ctx,
                    attempt_ctx,
                    outcome.to_string(),
                    Some(StatusCode::BAD_GATEWAY.as_u16()),
                    AttemptCircuitFields {
                        state_before: Some(circuit_before.state.as_str()),
                        state_after: Some(circuit_before.state.as_str()),
                        failure_count: Some(circuit_before.failure_count),
                        failure_threshold: Some(circuit_before.failure_threshold),
                    },
                )
                .await;
                match budget_decision.action {
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::RetrySameProvider => {
                        retry_state.codex_reasoning_guard_hits =
                            retry_state.codex_reasoning_guard_hits.saturating_add(1);
                        retry_state.allow_next_retry_beyond_max_attempts = true;
                        codex_reasoning_guard::apply_delay_if_needed(budget_decision).await;
                        return LoopControl::ContinueRetry;
                    }
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::ReturnError => {
                        *last_outcome = Some(AttemptOutcome::new(
                            ErrorCategory::SystemError.as_str(),
                            codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                        ));
                        let duration_ms = started.elapsed().as_millis();
                        emit_request_event_and_enqueue_request_log(
                            RequestEndArgs::from_context(RequestEndContextArgs {
                                deps: RequestEndDeps::new(
                                    &common.state.app,
                                    &common.state.db,
                                    &common.state.log_tx,
                                    &common.state.plugin_pipeline,
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
                                special_settings_json: response_fixer::special_settings_json(
                                    &common.special_settings,
                                ),
                                session_id: common.session_id.clone(),
                                requested_model: common.requested_model.clone(),
                                created_at_ms: common.created_at_ms,
                                created_at: common.created_at,
                            })
                            .with_completion(
                                RequestCompletion::failure_with_visible_ttfb(
                                    StatusCode::BAD_GATEWAY.as_u16(),
                                    Some(ErrorCategory::SystemError.as_str()),
                                    codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                                    initial_first_byte_ms,
                                    Some(duration_ms),
                                ),
                            ),
                        )
                        .await;
                        abort_guard.disarm();
                        return LoopControl::Return(error_response(
                            StatusCode::BAD_GATEWAY,
                            common.trace_id.clone(),
                            codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                            "Codex reasoning guard retry budget exhausted".to_string(),
                            attempts.clone(),
                        ));
                    }
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::SwitchProvider => {
                        failed_provider_ids.insert(provider_id);
                        *last_outcome = Some(AttemptOutcome::new(
                            ErrorCategory::SystemError.as_str(),
                            codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                        ));
                        return LoopControl::BreakRetry;
                    }
                }
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
                Some(raw.as_ref()),
                &common.special_settings,
            );

            let usage = usage::parse_usage_from_json_or_sse_bytes(common.cli_key.as_str(), &raw);
            let usage_metrics = usage.as_ref().map(|u| u.metrics.clone());
            let requested_model_for_log = common.requested_model.clone().or_else(|| {
                if raw.is_empty() {
                    None
                } else {
                    usage::parse_model_from_json_or_sse_bytes(common.cli_key.as_str(), &raw)
                }
            });

            let now_unix = now_unix_seconds() as i64;
            let change = provider_router::record_success_and_emit_transition(
                provider_router::RecordCircuitArgs::from_state(
                    common.state,
                    common.trace_id.as_str(),
                    common.cli_key.as_str(),
                    provider_id,
                    provider_ctx_owned.provider_name_base.as_str(),
                    provider_ctx_owned.provider_base_url_base.as_str(),
                    now_unix,
                ),
            );
            if let Some(last) = attempts.last_mut() {
                last.circuit_state_after = Some(change.after.state.as_str());
                last.circuit_failure_count = Some(change.after.failure_count);
                last.circuit_failure_threshold = Some(change.after.failure_threshold);
            }
            if let Some(session_id) = common.session_id.as_deref() {
                common.state.session.bind_success(
                    &common.cli_key,
                    session_id,
                    provider_id,
                    common.effective_sort_mode_id,
                    now_unix,
                );
            }

            let duration_ms = started.elapsed().as_millis();
            emit_request_event_and_enqueue_request_log(
                RequestEndArgs::from_context(RequestEndContextArgs {
                    deps: RequestEndDeps::new(
                        &common.state.app,
                        &common.state.db,
                        &common.state.log_tx,
                        &common.state.plugin_pipeline,
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
                    special_settings_json: response_fixer::special_settings_json(
                        &common.special_settings,
                    ),
                    session_id: common.session_id.clone(),
                    requested_model: requested_model_for_log,
                    created_at_ms: common.created_at_ms,
                    created_at: common.created_at,
                })
                .with_completion(RequestCompletion::success_with_visible_ttfb(
                    status.as_u16(),
                    initial_first_byte_ms,
                    Some(duration_ms),
                    usage_metrics,
                    None,
                    usage,
                )),
            )
            .await;

            let mut builder = Response::builder().status(status);
            for (k, v) in response_headers.iter() {
                builder = builder.header(k, v);
            }
            builder = builder.header("x-trace-id", common.trace_id.as_str());
            abort_guard.disarm();
            return LoopControl::Return(match builder.body(Body::from(raw)) {
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

        let body = match (enable_response_fixer_for_this_response, should_gunzip) {
            (true, true) => {
                let upstream =
                    GunzipStream::new(FirstChunkStream::new(first_chunk, resp.bytes_stream()));
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type(
                    upstream,
                    active_bridge_type,
                    common.requested_model.clone(),
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
            }
            (true, false) => {
                let upstream = FirstChunkStream::new(first_chunk, resp.bytes_stream());
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type(
                    upstream,
                    active_bridge_type,
                    common.requested_model.clone(),
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
            }
            (false, true) => {
                let upstream =
                    GunzipStream::new(FirstChunkStream::new(first_chunk, resp.bytes_stream()));
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type(
                    upstream,
                    active_bridge_type,
                    common.requested_model.clone(),
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
            }
            (false, false) => {
                let upstream = FirstChunkStream::new(first_chunk, resp.bytes_stream());
                let upstream =
                    gemini_oauth::GeminiOAuthSseStream::new(upstream, gemini_oauth_response_mode);
                let upstream = protocol_bridge::stream::BridgeStream::for_bridge_type(
                    upstream,
                    active_bridge_type,
                    common.requested_model.clone(),
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
