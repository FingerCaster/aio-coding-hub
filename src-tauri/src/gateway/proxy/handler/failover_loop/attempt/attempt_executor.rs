//! Usage: Single attempt execution (build request, send upstream, return result).
//!
//! Encapsulates URL construction, header assembly, auth injection, body
//! cleaning, and the upstream send for one retry attempt.

use super::provider_iterator::PreparedProvider;
use super::*;
use crate::gateway::plugins::context::{GatewayPluginHookName, GatewayRequestHookInput};
use crate::gateway::proxy::abort_guard::RequestAbortGuard;
use crate::gateway::proxy::request_context::RequestContext;
use std::sync::{Arc, Mutex};

/// Mutable per-provider state that persists across retries within one provider.
pub(super) struct RetryLoopState {
    pub(super) claude_api_key_bearer_fallback: bool,
    pub(super) oauth_reactive_refreshed_once: bool,
    pub(super) codex_previous_response_id_rectifier_retried: bool,
    pub(super) thinking_signature_rectifier_retried: bool,
    pub(super) thinking_budget_rectifier_retried: bool,
    pub(super) allow_next_retry_beyond_max_attempts: bool,
    pub(super) codex_reasoning_guard_hits: u32,
    pub(super) codex_reasoning_guard_current_model: Option<String>,
    pub(super) codex_reasoning_guard_next_retry_wave: Option<CodexReasoningGuardNextRetryWave>,
}

impl Clone for RetryLoopState {
    fn clone(&self) -> Self {
        Self {
            claude_api_key_bearer_fallback: self.claude_api_key_bearer_fallback,
            oauth_reactive_refreshed_once: self.oauth_reactive_refreshed_once,
            codex_previous_response_id_rectifier_retried: self
                .codex_previous_response_id_rectifier_retried,
            thinking_signature_rectifier_retried: self.thinking_signature_rectifier_retried,
            thinking_budget_rectifier_retried: self.thinking_budget_rectifier_retried,
            allow_next_retry_beyond_max_attempts: false,
            codex_reasoning_guard_hits: self.codex_reasoning_guard_hits,
            codex_reasoning_guard_current_model: self.codex_reasoning_guard_current_model.clone(),
            codex_reasoning_guard_next_retry_wave: None,
        }
    }
}

impl RetryLoopState {
    pub(super) fn remember_codex_reasoning_guard_retry_wave(
        &mut self,
        wave: Option<codex_reasoning_guard::CodexReasoningGuardRetryWave>,
    ) {
        self.codex_reasoning_guard_next_retry_wave =
            wave.map(|wave| CodexReasoningGuardNextRetryWave {
                concurrency: wave.concurrency,
                interval_ms: wave.interval_ms,
            });
    }
}

impl RetryLoopState {
    pub(super) fn new() -> Self {
        Self {
            claude_api_key_bearer_fallback: false,
            oauth_reactive_refreshed_once: false,
            codex_previous_response_id_rectifier_retried: false,
            thinking_signature_rectifier_retried: false,
            thinking_budget_rectifier_retried: false,
            allow_next_retry_beyond_max_attempts: false,
            codex_reasoning_guard_hits: 0,
            codex_reasoning_guard_current_model: None,
            codex_reasoning_guard_next_retry_wave: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardNextRetryWave {
    pub(super) concurrency: u32,
    pub(super) interval_ms: u32,
}

/// Timing captured at the start of an attempt, before the upstream send.
pub(super) struct AttemptTiming {
    pub(super) attempt_started_ms: u128,
    pub(super) attempt_started: Instant,
}

/// Result of building + sending one attempt.
pub(super) enum AttemptSendOutcome {
    Response(reqwest::Response, AttemptTiming),
    BufferedNonStreamResponse {
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
        provider_ttfb_ms: Option<u128>,
        timing: AttemptTiming,
    },
    Timeout(AttemptTiming),
    ReqwestError(reqwest::Error, AttemptTiming),
    /// URL build failure already recorded; caller should apply the returned LoopControl.
    UrlBuildFailed(LoopControl),
    /// OAuth adapter injection failed; break out of retry loop for this provider.
    OAuthInjectFailed,
    /// Plugin blocked the request before the upstream send.
    PluginBlocked(String),
}

/// URL build failure from the shared prepared-send primitive.
pub(super) struct PreparedUrlBuildFailure {
    pub(super) error: String,
    pub(super) attempt_started_ms: u128,
    pub(super) circuit_before: crate::circuit_breaker::CircuitSnapshot,
}

/// Result of the shared prepared-send primitive before retry-loop side effects
/// such as recording URL/OAuth failures into the public attempt list.
pub(super) enum PreparedSendOutcome {
    Response(reqwest::Response, AttemptTiming),
    Timeout(AttemptTiming),
    ReqwestError(reqwest::Error, AttemptTiming),
    UrlBuildFailed(PreparedUrlBuildFailure),
    OAuthInjectFailed(Box<FailoverAttempt>),
    PluginBlocked(String),
}

/// Build request headers, inject auth, clean body, send upstream, and return
/// the raw outcome. The caller (retry engine / response router) handles the
/// result.
pub(super) async fn execute_attempt<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &mut PreparedProvider,
    retry_state: &mut RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    loop_state: &mut LoopState<'_, R>,
) -> AttemptSendOutcome
where
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    match send_prepared_upstream(
        ctx,
        input,
        prepared,
        retry_state,
        retry_index,
        attempt_index,
        ctx.upstream_first_byte_timeout,
        Some(loop_state.abort_guard),
    )
    .await
    {
        PreparedSendOutcome::Response(resp, timing) => AttemptSendOutcome::Response(resp, timing),
        PreparedSendOutcome::Timeout(timing) => AttemptSendOutcome::Timeout(timing),
        PreparedSendOutcome::ReqwestError(err, timing) => {
            AttemptSendOutcome::ReqwestError(err, timing)
        }
        PreparedSendOutcome::UrlBuildFailed(failure) => {
            let circuit_before = failure.circuit_before;
            let attempt_ctx = build_attempt_ctx(
                attempt_index,
                retry_index,
                failure.attempt_started_ms,
                &circuit_before,
                prepared,
            );
            let provider_ctx = build_provider_ctx(prepared);
            let ctrl = handle_url_build_failure(
                ctx,
                input,
                attempt_ctx,
                provider_ctx,
                failure.error,
                loop_state,
            )
            .await;
            AttemptSendOutcome::UrlBuildFailed(ctrl)
        }
        PreparedSendOutcome::OAuthInjectFailed(failed_attempt) => {
            loop_state.attempts.push(*failed_attempt);
            AttemptSendOutcome::OAuthInjectFailed
        }
        PreparedSendOutcome::PluginBlocked(reason) => AttemptSendOutcome::PluginBlocked(reason),
    }
}

/// Build request headers, inject auth, clean body, run before-send plugins, and
/// send one prepared upstream request. `abort_guard` is supplied only for normal
/// retry-loop attempts; continuation repair rounds intentionally reuse the send
/// semantics without publishing themselves as top-level attempts.
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_prepared_upstream<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &mut PreparedProvider,
    retry_state: &mut RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    first_byte_timeout: Option<std::time::Duration>,
    abort_guard: Option<&mut RequestAbortGuard<R>>,
) -> PreparedSendOutcome
where
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    let attempt_started_ms = input.started.elapsed().as_millis();
    let circuit_before = prepared.circuit_snapshot.clone();

    // --- Build URL ---
    let url = match try_build_url(prepared) {
        Ok(u) => u,
        Err(err) => {
            return PreparedSendOutcome::UrlBuildFailed(PreparedUrlBuildFailure {
                error: err,
                attempt_started_ms,
                circuit_before,
            });
        }
    };

    // --- Emit "started" attempt event ---
    if let Some(abort_guard) = abort_guard {
        emit_started_event(
            input,
            prepared,
            attempt_index,
            retry_index,
            attempt_started_ms,
            &circuit_before,
            abort_guard,
        );
    }

    // --- Build headers + inject auth ---
    let mut headers = input.base_headers.clone();
    ensure_cli_required_headers(&input.cli_key, &mut headers);
    if let Some((_, source_cli_key)) = prepared.bridge_source.as_ref() {
        ensure_cli_required_headers(source_cli_key, &mut headers);
    }
    codex_session_id_completion::inject_session_headers_if_needed(
        &mut headers,
        prepared.cx2cc_codex_session_id.as_deref(),
    );

    if let Err(failed_attempt) = attempt_auth::inject_auth(
        ctx,
        input,
        prepared,
        retry_state,
        &attempt_auth::AuthErrorCtx {
            attempt_index,
            retry_index,
            attempt_started_ms,
            circuit_before: &circuit_before,
        },
        &mut headers,
    ) {
        return PreparedSendOutcome::OAuthInjectFailed(failed_attempt);
    }

    // --- Clean body + send upstream ---
    let clean_outcome = request_sanitizer::clean_body(input, prepared);
    apply_body_sanitizer_outcome(
        ctx.special_settings,
        prepared.provider_id,
        &prepared.provider_name_base,
        &clean_outcome,
    );

    let mut body_state_for_attempt = input.request_body_state.clone();
    let body_changed_before_hook = prepared.request_body_mutated_before_attempt
        || clean_outcome.changed()
        || clean_outcome.body != body_state_for_attempt.decoded_clone();
    if body_changed_before_hook {
        body_state_for_attempt.replace_decoded(clean_outcome.body.clone());
    }

    let mut semantic_headers = body_state_for_attempt.semantic_headers(&headers);
    let hook_input = GatewayRequestHookInput {
        hook_name: GatewayPluginHookName::RequestBeforeSend,
        trace_id: input.trace_id.clone(),
        cli_key: input.cli_key.clone(),
        method: input.req_method.clone(),
        path: input.forwarded_path.clone(),
        query: input.query.clone(),
        headers: semantic_headers.clone(),
        body: body_state_for_attempt.decoded_clone(),
        requested_model: prepared
            .active_requested_model
            .clone()
            .or_else(|| input.requested_model.clone()),
    };
    match ctx.state.plugin_pipeline.run_request_hook(hook_input).await {
        Ok(output) => {
            crate::gateway::plugins::audit::persist_gateway_plugin_diagnostics(
                &ctx.state.db,
                &input.trace_id,
                output.audit_events.clone(),
                output.execution_reports.clone(),
            );
            if let Some(blocked) = output.blocked {
                tracing::warn!(
                    trace_id = %input.trace_id,
                    provider_id = prepared.provider_id,
                    status = blocked.status,
                    reason = %blocked.reason,
                    "plugin blocked gateway request before upstream send"
                );
                return PreparedSendOutcome::PluginBlocked(blocked.reason);
            }
            semantic_headers = output.headers;
            sync_before_send_body_output(prepared, &mut body_state_for_attempt, output.body);
        }
        Err(mut err) => {
            crate::gateway::plugins::audit::persist_gateway_plugin_error_audit_events(
                &ctx.state.db,
                &input.trace_id,
                &mut err,
            );
            tracing::warn!(
                trace_id = %input.trace_id,
                provider_id = prepared.provider_id,
                "plugin beforeSend hook failed: {}",
                err
            );
            return PreparedSendOutcome::PluginBlocked(format!(
                "gateway plugin request hook failed: {err}"
            ));
        }
    }

    headers = semantic_headers;
    let upstream_body = body_state_for_attempt
        .finalize_for_upstream(&mut headers, crate::gateway::util::max_request_body_bytes());

    emit_upstream_attempt_fingerprint(
        ctx,
        input,
        prepared,
        retry_index,
        &url,
        &headers,
        &upstream_body,
    );

    let timing = AttemptTiming {
        attempt_started_ms,
        attempt_started: Instant::now(),
    };

    let send_result = send::send_upstream_with_first_byte_timeout(
        ctx,
        input.req_method.clone(),
        url,
        headers,
        upstream_body,
        first_byte_timeout,
    )
    .await;

    match send_result {
        send::SendResult::Ok(resp) => PreparedSendOutcome::Response(resp, timing),
        send::SendResult::Timeout => PreparedSendOutcome::Timeout(timing),
        send::SendResult::Err(err) => PreparedSendOutcome::ReqwestError(err, timing),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sync_before_send_body_output(
    prepared: &mut PreparedProvider,
    body_state_for_attempt: &mut crate::gateway::proxy::request_body::GatewayRequestBody,
    output_body: Bytes,
) {
    let previous_body = body_state_for_attempt.decoded_clone();
    body_state_for_attempt.replace_decoded(output_body.clone());
    if output_body == previous_body {
        return;
    }

    prepared.upstream_body_bytes = output_body;
    prepared.strip_request_content_encoding = true;
    prepared.request_body_mutated_before_attempt = true;
}

fn try_build_url(prepared: &PreparedProvider) -> Result<reqwest::Url, String> {
    build_target_url(
        &prepared.provider_base_url_base,
        &prepared.upstream_forwarded_path,
        prepared.upstream_query.as_deref(),
    )
    .map_err(|e| e.to_string())
}

fn apply_body_sanitizer_outcome(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    provider_name_base: &str,
    clean_outcome: &request_sanitizer::CleanBodyOutcome,
) {
    if !clean_outcome.changed() {
        return;
    }
    response_fixer::push_special_setting(
        special_settings,
        serde_json::json!({
            "type": "request_body_sanitizer",
            "scope": "attempt",
            "hit": true,
            "providerId": provider_id,
            "providerName": provider_name_base,
            "reason": "claude_oauth_empty_text_blocks",
            "removedEmptyTextBlocks": clean_outcome.removed_empty_text_blocks,
        }),
    );
}

fn emit_upstream_attempt_fingerprint<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &PreparedProvider,
    retry_index: u32,
    url: &reqwest::Url,
    headers: &HeaderMap,
    body: &Bytes,
) {
    let fingerprint = crate::gateway::upstream_fingerprint::compute_upstream_request_fingerprint(
        &input.req_method,
        url,
        headers,
        body,
    );
    tracing::debug!(
        trace_id = %input.trace_id,
        cli_key = %input.cli_key,
        provider_id = prepared.provider_id,
        retry_index,
        upstream_fingerprint_key = fingerprint.key,
        upstream_fingerprint_debug = %fingerprint.debug,
        "computed upstream attempt request fingerprint"
    );
    emit_gateway_debug_log_lazy(&ctx.state.app, || {
        format!(
            "[UPSTREAM_FP] trace_id={} provider={} (id={}) retry={} key={} debug={}",
            input.trace_id,
            prepared.provider_name_base,
            prepared.provider_id,
            retry_index,
            fingerprint.key,
            fingerprint.debug,
        )
    });
}

async fn handle_url_build_failure<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    attempt_ctx: AttemptCtx<'_>,
    provider_ctx: ProviderCtx<'_>,
    err: String,
    loop_state: &mut LoopState<'_, R>,
) -> LoopControl {
    tracing::warn!(
        trace_id = %input.trace_id,
        cli_key = %input.cli_key,
        provider_id = provider_ctx.provider_id,
        provider_name = %provider_ctx.provider_name_base,
        base_url = %provider_ctx.provider_base_url_base,
        "build_target_url failed: {err}"
    );
    let error_code = GatewayErrorCode::InternalError.as_str();
    let decision = FailoverDecision::SwitchProvider;
    let outcome = format!(
        "build_target_url_error: category={} code={} decision={} err={err}",
        ErrorCategory::SystemError.as_str(),
        error_code,
        decision.as_str(),
    );
    record_system_failure_and_decide_no_cooldown(RecordSystemFailureArgs {
        ctx,
        provider_ctx,
        attempt_ctx,
        loop_state: loop_state.reborrow(),
        status: None,
        error_code,
        decision,
        outcome,
        reason: format!("invalid base_url: {err}"),
        record_circuit_failure: true,
        timeout_secs: None,
    })
    .await
}

fn build_attempt_ctx<'a>(
    attempt_index: u32,
    retry_index: u32,
    attempt_started_ms: u128,
    circuit_before: &'a crate::circuit_breaker::CircuitSnapshot,
    prepared: &'a PreparedProvider,
) -> AttemptCtx<'a> {
    AttemptCtx {
        attempt_index,
        retry_index,
        provider_max_attempts: prepared.provider_max_attempts,
        attempt_started_ms,
        attempt_started: Instant::now(),
        circuit_before,
        gemini_oauth_response_mode: prepared.gemini_oauth_response_mode,
        cx2cc_active: prepared.cx2cc_active,
        active_bridge_type: prepared.active_bridge_type.as_deref(),
        responses_cache_namespace: prepared.responses_cache_namespace.as_deref(),
        responses_cache_input: prepared.responses_cache_input.as_deref(),
        anthropic_stream_requested: prepared.anthropic_stream_requested,
    }
}

fn build_provider_ctx(prepared: &PreparedProvider) -> ProviderCtx<'_> {
    ProviderCtx {
        provider_id: prepared.provider_id,
        provider_name_base: &prepared.provider_name_base,
        provider_base_url_base: &prepared.provider_base_url_base,
        active_requested_model: prepared.active_requested_model.as_deref(),
        auth_mode: prepared.auth_mode.as_str(),
        provider_index: prepared.provider_index,
        provider_bridged: prepared.provider_bridged,
        session_reuse: prepared.session_reuse,
        provider_max_attempts: prepared.provider_max_attempts,
        stream_idle_timeout_seconds: prepared.stream_idle_timeout_seconds,
        upstream_retry_policy: &prepared.upstream_retry_policy,
        claude_model_mapping: prepared.claude_model_mapping.as_ref(),
    }
}

fn emit_started_event<R: tauri::Runtime>(
    input: &RequestContext<R>,
    prepared: &PreparedProvider,
    attempt_index: u32,
    retry_index: u32,
    attempt_started_ms: u128,
    circuit_before: &crate::circuit_breaker::CircuitSnapshot,
    abort_guard: &mut RequestAbortGuard<R>,
) {
    let started_attempt = FailoverAttempt {
        provider_id: prepared.provider_id,
        provider_name: prepared.provider_name_base.clone(),
        base_url: prepared.provider_base_url_base.clone(),
        outcome: "started".to_string(),
        status: None,
        provider_index: Some(prepared.provider_index),
        retry_index: Some(retry_index),
        session_reuse: prepared.session_reuse,
        error_category: None,
        error_code: None,
        decision: None,
        reason: None,
        selection_method: dc::selection_method(
            prepared.provider_index,
            retry_index,
            prepared.session_reuse,
        ),
        reason_code: None,
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(0),
        circuit_state_before: Some(circuit_before.state.as_str()),
        circuit_state_after: None,
        circuit_failure_count: Some(circuit_before.failure_count),
        circuit_failure_threshold: Some(circuit_before.failure_threshold),
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        provider_bridged: Some(prepared.provider_bridged),
        timeout_secs: None,
    };
    abort_guard.update_requested_model(
        prepared
            .active_requested_model
            .clone()
            .or_else(|| input.requested_model.clone()),
    );
    abort_guard.capture_in_flight_attempt(&started_attempt);
    if input.observe_request {
        emit_attempt_event(
            &input.state.app,
            GatewayAttemptEvent {
                trace_id: input.trace_id.clone(),
                cli_key: input.cli_key.clone(),
                session_id: input.session_id.clone(),
                method: input.method_hint.clone(),
                path: input.forwarded_path.clone(),
                query: input.query.clone(),
                requested_model: prepared
                    .active_requested_model
                    .clone()
                    .or_else(|| input.requested_model.clone()),
                special_settings_json: crate::gateway::response_fixer::special_settings_json(
                    &input.special_settings,
                ),
                attempt_index,
                provider_id: prepared.provider_id,
                session_reuse: prepared.session_reuse,
                provider_name: prepared.provider_name_base.clone(),
                base_url: prepared.provider_base_url_base.clone(),
                outcome: "started".to_string(),
                status: None,
                attempt_started_ms,
                attempt_duration_ms: 0,
                circuit_state_before: Some(circuit_before.state.as_str()),
                circuit_state_after: None,
                circuit_failure_count: Some(circuit_before.failure_count),
                circuit_failure_threshold: Some(circuit_before.failure_threshold),
                claude_model_mapping: prepared.claude_model_mapping.clone(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn body_sanitizer_outcome_records_setting_without_touching_headers() {
        let special_settings = Arc::new(Mutex::new(Vec::new()));
        let clean_outcome = request_sanitizer::CleanBodyOutcome {
            body: Bytes::from_static(br#"{"messages":[]}"#),
            removed_empty_text_blocks: 2,
        };

        apply_body_sanitizer_outcome(&special_settings, 42, "Claude OAuth", &clean_outcome);

        let settings = special_settings.lock().unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(
            settings[0],
            json!({
                "type": "request_body_sanitizer",
                "scope": "attempt",
                "hit": true,
                "providerId": 42,
                "providerName": "Claude OAuth",
                "reason": "claude_oauth_empty_text_blocks",
                "removedEmptyTextBlocks": 2,
            })
        );
    }

    #[test]
    fn body_sanitizer_outcome_is_noop_when_body_unchanged() {
        let special_settings = Arc::new(Mutex::new(Vec::new()));
        let clean_outcome = request_sanitizer::CleanBodyOutcome {
            body: Bytes::from_static(br#"{"messages":[]}"#),
            removed_empty_text_blocks: 0,
        };

        apply_body_sanitizer_outcome(&special_settings, 42, "Claude OAuth", &clean_outcome);

        assert!(special_settings.lock().unwrap().is_empty());
    }

    #[test]
    fn retry_state_remembers_codex_reasoning_guard_next_wave() {
        let wave = codex_reasoning_guard::retry_wave(
            2,
            crate::settings::CodexReasoningGuardRetryPolicy::Concurrent,
            5,
            1_000,
            10,
        );
        let mut retry_state = RetryLoopState::new();

        retry_state.remember_codex_reasoning_guard_retry_wave(Some(wave));

        assert_eq!(
            retry_state.codex_reasoning_guard_next_retry_wave,
            Some(CodexReasoningGuardNextRetryWave {
                concurrency: 3,
                interval_ms: 1_000,
            })
        );
    }

    #[test]
    fn retry_state_probe_clone_does_not_reuse_pending_wave() {
        let mut retry_state = RetryLoopState::new();
        retry_state.codex_reasoning_guard_current_model = Some("gpt-5.4".to_string());
        retry_state.codex_reasoning_guard_next_retry_wave =
            Some(CodexReasoningGuardNextRetryWave {
                concurrency: 4,
                interval_ms: 500,
            });

        let cloned = retry_state.clone();

        assert_eq!(
            cloned.codex_reasoning_guard_current_model.as_deref(),
            Some("gpt-5.4")
        );
        assert!(cloned.codex_reasoning_guard_next_retry_wave.is_none());
    }
}
