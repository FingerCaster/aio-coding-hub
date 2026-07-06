//! Usage: Handle successful event-stream upstream responses inside `failover_loop::run`.

use super::attempt_executor::{PreparedSendOutcome, RetryLoopState};
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
use futures_core::Stream;
use serde_json::Value;
use std::fmt::Display;
use std::future::poll_fn;
use std::pin::Pin;
use std::time::{Duration, Instant};

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
    seconds: Option<u32>,
    source: &'static str,
}

fn resolve_effective_stream_idle_timeout(
    provider_seconds: Option<u32>,
    global_timeout: Option<Duration>,
) -> EffectiveStreamIdleTimeout {
    if let Some(seconds) = provider_seconds.filter(|seconds| *seconds > 0) {
        return EffectiveStreamIdleTimeout {
            duration: Some(Duration::from_secs(seconds as u64)),
            seconds: Some(seconds),
            source: "provider",
        };
    }

    EffectiveStreamIdleTimeout {
        duration: global_timeout,
        seconds: global_timeout.map(|timeout| timeout.as_secs().min(u64::from(u32::MAX)) as u32),
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

fn is_codex_chat_completions_event_stream_path(cli_key: &str, path: &str) -> bool {
    cli_key == "codex"
        && matches!(
            path.trim_end_matches('/'),
            "/v1/chat/completions" | "/chat/completions"
        )
}

fn should_buffer_codex_responses_for_empty_detection(cli_key: &str, path: &str) -> bool {
    is_codex_responses_event_stream_path(cli_key, path)
}

fn should_buffer_native_codex_responses_for_empty_detection(
    cli_key: &str,
    path: &str,
    active_bridge_type: Option<&str>,
) -> bool {
    active_bridge_type.is_none() && should_buffer_codex_responses_for_empty_detection(cli_key, path)
}

fn should_buffer_native_codex_responses_for_reasoning_guard(
    guard_enabled: bool,
    cli_key: &str,
    path: &str,
    active_bridge_type: Option<&str>,
    has_content_length: bool,
) -> bool {
    guard_enabled
        && active_bridge_type.is_none()
        && cli_key == "codex"
        && (is_codex_responses_event_stream_path(cli_key, path)
            || (has_content_length && is_codex_chat_completions_event_stream_path(cli_key, path)))
}

fn codex_chat_completions_sse_guard_value(cli_key: &str, raw: &[u8]) -> Option<serde_json::Value> {
    let usage = usage::parse_usage_from_json_or_sse_bytes(cli_key, raw)?;
    let usage_value = serde_json::from_str::<serde_json::Value>(&usage.usage_json).ok()?;
    let mut value = serde_json::json!({
        "object": "chat.completion",
        "usage": usage_value,
    });
    if let Some(model) = usage::parse_model_from_json_or_sse_bytes(cli_key, raw) {
        value["model"] = serde_json::Value::String(model);
    }
    Some(value)
}

fn buffered_stream_error_code(
    cli_key: &str,
    path: &str,
    status: u16,
    raw: &[u8],
) -> Option<&'static str> {
    let mut tracker = usage::SseUsageTracker::new(cli_key);
    tracker.ingest_chunk(raw);
    let usage = tracker.finalize();
    if tracker.fake_200_detected() {
        return Some(GatewayErrorCode::Fake200.as_str());
    }
    if tracker.is_empty_success(path, status, usage.as_ref()) {
        return Some(GatewayErrorCode::EmptyResponse.as_str());
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContinuationRepairStatus {
    NotApplicable,
    Unavailable,
    BudgetExhausted,
    MissingEncrypted,
    Repaired,
    CappedMaxOutputTokens,
    StillMatched,
    Failed,
}

impl ContinuationRepairStatus {
    fn as_str(self) -> &'static str {
        match self {
            ContinuationRepairStatus::NotApplicable => "not_applicable",
            ContinuationRepairStatus::Unavailable => "unavailable",
            ContinuationRepairStatus::BudgetExhausted => "budget_exhausted",
            ContinuationRepairStatus::MissingEncrypted => "missing_encrypted",
            ContinuationRepairStatus::Repaired => "repaired",
            ContinuationRepairStatus::CappedMaxOutputTokens => "capped_max_output_tokens",
            ContinuationRepairStatus::StillMatched => "still_matched",
            ContinuationRepairStatus::Failed => "failed",
        }
    }

    fn default_failure_kind(self) -> Option<&'static str> {
        match self {
            ContinuationRepairStatus::NotApplicable | ContinuationRepairStatus::Repaired => None,
            ContinuationRepairStatus::Unavailable => Some("unavailable"),
            ContinuationRepairStatus::BudgetExhausted => Some("budget_exhausted"),
            ContinuationRepairStatus::MissingEncrypted => Some("missing_encrypted"),
            ContinuationRepairStatus::CappedMaxOutputTokens => Some("capped_max_output_tokens"),
            ContinuationRepairStatus::StillMatched => Some("still_matched"),
            ContinuationRepairStatus::Failed => Some("failed"),
        }
    }
}

struct ContinuationRepairOutcome {
    status: ContinuationRepairStatus,
    client_raw: Option<Bytes>,
    client_usage: Option<usage::UsageExtract>,
    provider_repair_usage: Option<usage::UsageExtract>,
    round_trace: Vec<Value>,
    reconstruction_status: Option<&'static str>,
    visible_assembly_kind: Option<&'static str>,
    canonical_response_id: Option<String>,
    canonical_response_id_continuity: Option<&'static str>,
    aggregate_raw_bytes: usize,
    repair_elapsed_ms: Option<u128>,
    cumulative_elapsed_ms: Option<u128>,
    round_durations_ms: Vec<u128>,
    reasoning_tokens: Option<i64>,
    reasoning_tokens_pointer: Option<&'static str>,
    sent_rounds: u32,
    failure_kind: Option<&'static str>,
    reason: Option<String>,
}

impl ContinuationRepairOutcome {
    fn not_applicable() -> Self {
        Self {
            status: ContinuationRepairStatus::NotApplicable,
            client_raw: None,
            client_usage: None,
            provider_repair_usage: None,
            round_trace: Vec::new(),
            reconstruction_status: None,
            visible_assembly_kind: None,
            canonical_response_id: None,
            canonical_response_id_continuity: None,
            aggregate_raw_bytes: 0,
            repair_elapsed_ms: None,
            cumulative_elapsed_ms: None,
            round_durations_ms: Vec::new(),
            reasoning_tokens: None,
            reasoning_tokens_pointer: None,
            sent_rounds: 0,
            failure_kind: None,
            reason: None,
        }
    }

    fn terminal(
        status: ContinuationRepairStatus,
        token: Option<codex_reasoning_features::ExtractedReasoningTokens>,
        sent_rounds: u32,
        reason: impl Into<Option<String>>,
    ) -> Self {
        Self::terminal_with_kind(
            status,
            token,
            sent_rounds,
            status.default_failure_kind(),
            reason,
        )
    }

    fn terminal_with_kind(
        status: ContinuationRepairStatus,
        token: Option<codex_reasoning_features::ExtractedReasoningTokens>,
        sent_rounds: u32,
        failure_kind: impl Into<Option<&'static str>>,
        reason: impl Into<Option<String>>,
    ) -> Self {
        Self {
            status,
            client_raw: None,
            client_usage: None,
            provider_repair_usage: None,
            round_trace: Vec::new(),
            reconstruction_status: None,
            visible_assembly_kind: None,
            canonical_response_id: None,
            canonical_response_id_continuity: None,
            aggregate_raw_bytes: 0,
            repair_elapsed_ms: None,
            cumulative_elapsed_ms: None,
            round_durations_ms: Vec::new(),
            reasoning_tokens: token.map(|value| value.reasoning_tokens),
            reasoning_tokens_pointer: token.map(|value| value.pointer),
            sent_rounds,
            failure_kind: failure_kind.into(),
            reason: reason.into(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn terminal_with_kind_and_trace(
        status: ContinuationRepairStatus,
        token: Option<codex_reasoning_features::ExtractedReasoningTokens>,
        sent_rounds: u32,
        failure_kind: impl Into<Option<&'static str>>,
        reason: impl Into<Option<String>>,
        rounds: &[codex_reasoning_continuation::ContinuationRepairRound],
        aggregate_raw_bytes: usize,
        repair_elapsed_ms: Option<u128>,
        cumulative_elapsed_ms: Option<u128>,
        round_durations_ms: &[u128],
    ) -> Self {
        let mut outcome =
            Self::terminal_with_kind(status, token, sent_rounds, failure_kind, reason);
        outcome.round_trace = codex_reasoning_continuation::sanitized_round_trace(rounds);
        outcome.aggregate_raw_bytes = aggregate_raw_bytes;
        outcome.repair_elapsed_ms = repair_elapsed_ms;
        outcome.cumulative_elapsed_ms = cumulative_elapsed_ms;
        outcome.round_durations_ms = round_durations_ms.to_vec();
        outcome
    }

    fn repaired_bplus(
        reconstruction: codex_reasoning_continuation::ContinuationReconstruction,
        token: Option<codex_reasoning_features::ExtractedReasoningTokens>,
        sent_rounds: u32,
        repair_elapsed_ms: u128,
        cumulative_elapsed_ms: u128,
        round_durations_ms: Vec<u128>,
    ) -> Self {
        Self {
            status: ContinuationRepairStatus::Repaired,
            client_raw: Some(reconstruction.client_raw),
            client_usage: Some(reconstruction.client_usage),
            provider_repair_usage: Some(reconstruction.provider_repair_usage),
            round_trace: reconstruction.round_trace,
            reconstruction_status: Some(reconstruction.reconstruction_status),
            visible_assembly_kind: Some(reconstruction.visible_assembly_kind),
            canonical_response_id: Some(reconstruction.canonical_response_id),
            canonical_response_id_continuity: Some(reconstruction.canonical_response_id_continuity),
            aggregate_raw_bytes: reconstruction.aggregate_raw_bytes,
            repair_elapsed_ms: Some(repair_elapsed_ms),
            cumulative_elapsed_ms: Some(cumulative_elapsed_ms),
            round_durations_ms,
            reasoning_tokens: token.map(|value| value.reasoning_tokens),
            reasoning_tokens_pointer: token.map(|value| value.pointer),
            sent_rounds,
            failure_kind: None,
            reason: None,
        }
    }

    fn repaired_folded(
        client_raw: Bytes,
        token: Option<codex_reasoning_features::ExtractedReasoningTokens>,
        sent_rounds: u32,
        aggregate_raw_bytes: usize,
        repair_elapsed_ms: u128,
        cumulative_elapsed_ms: u128,
        round_durations_ms: Vec<u128>,
    ) -> Self {
        Self {
            status: ContinuationRepairStatus::Repaired,
            client_raw: Some(client_raw),
            client_usage: None,
            provider_repair_usage: None,
            round_trace: Vec::new(),
            reconstruction_status: None,
            visible_assembly_kind: None,
            canonical_response_id: None,
            canonical_response_id_continuity: None,
            aggregate_raw_bytes,
            repair_elapsed_ms: Some(repair_elapsed_ms),
            cumulative_elapsed_ms: Some(cumulative_elapsed_ms),
            round_durations_ms,
            reasoning_tokens: token.map(|value| value.reasoning_tokens),
            reasoning_tokens_pointer: token.map(|value| value.pointer),
            sent_rounds,
            failure_kind: None,
            reason: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_codex_reasoning_continuation_repair<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &PreparedProvider,
    retry_state: &mut RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    attempt_started: Instant,
    first_raw: Bytes,
    first_aggregated: &Value,
    post_match_strategy: crate::settings::CodexReasoningGuardPostMatchStrategy,
    upstream_stream_idle_timeout: Option<Duration>,
    enable_response_fixer: bool,
    response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    max_rounds: u32,
    max_output_tokens: u32,
) -> ContinuationRepairOutcome
where
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    let mut current = first_aggregated.clone();
    let mut current_token = codex_reasoning_features::extract_reasoning_tokens(&current);
    if !codex_reasoning_continuation::is_truncation_continuation_pattern(
        current_token.map(|value| value.reasoning_tokens),
    ) {
        return ContinuationRepairOutcome::not_applicable();
    }
    if !codex_reasoning_continuation::latest_reasoning_has_encrypted_content(&current) {
        return ContinuationRepairOutcome::terminal(
            ContinuationRepairStatus::MissingEncrypted,
            current_token,
            0,
            Some("matched continuation pattern but no encrypted reasoning item".to_string()),
        );
    }

    let repair_started = Instant::now();
    let mut rounds = vec![codex_reasoning_continuation::ContinuationRepairRound::new(
        codex_reasoning_continuation::ContinuationRepairRoundKind::Initial,
        first_raw,
        current.clone(),
        Some(0),
    )];
    let mut aggregate_raw_bytes = rounds[0].raw_sse.len();
    let mut round_durations_ms = Vec::new();
    macro_rules! terminal_with_trace {
        ($status:expr, $token:expr, $sent_rounds:expr, $failure_kind:expr, $reason:expr $(,)?) => {
            ContinuationRepairOutcome::terminal_with_kind_and_trace(
                $status,
                $token,
                $sent_rounds,
                $failure_kind,
                $reason,
                &rounds,
                aggregate_raw_bytes,
                Some(repair_started.elapsed().as_millis()),
                Some(attempt_started.elapsed().as_millis()),
                &round_durations_ms,
            )
        };
    }
    if aggregate_raw_bytes > MAX_NON_SSE_BODY_BYTES {
        return terminal_with_trace!(
            ContinuationRepairStatus::Failed,
            current_token,
            0,
            Some("aggregate_bytes"),
            Some(format!(
                "continuation repair initial raw bytes exceeded aggregate cap ({} > {})",
                aggregate_raw_bytes, MAX_NON_SSE_BODY_BYTES
            )),
        );
    }
    let mut replay_tail = Vec::new();
    let mut sent_rounds = 0u32;
    let mut cumulative_output_tokens = codex_reasoning_continuation::output_tokens(&current);

    loop {
        current_token = codex_reasoning_features::extract_reasoning_tokens(&current);
        if !codex_reasoning_continuation::is_truncation_continuation_pattern(
            current_token.map(|value| value.reasoning_tokens),
        ) {
            if post_match_strategy.is_experimental_continuation_repair() {
                return match codex_reasoning_continuation::reconstruct_bplus_client_sse(
                    &rounds,
                    MAX_NON_SSE_BODY_BYTES,
                ) {
                    Ok(reconstruction) => ContinuationRepairOutcome::repaired_bplus(
                        reconstruction,
                        current_token,
                        sent_rounds,
                        repair_started.elapsed().as_millis(),
                        attempt_started.elapsed().as_millis(),
                        round_durations_ms,
                    ),
                    Err(err) => terminal_with_trace!(
                        ContinuationRepairStatus::Failed,
                        current_token,
                        sent_rounds,
                        Some("bplus_reconstruction"),
                        Some(err),
                    ),
                };
            }

            let responses = rounds
                .iter()
                .map(|round| round.aggregated.clone())
                .collect::<Vec<_>>();
            return match codex_reasoning_continuation::fold_responses_to_sse(&responses) {
                Ok(raw) => ContinuationRepairOutcome::repaired_folded(
                    raw,
                    current_token,
                    sent_rounds,
                    aggregate_raw_bytes,
                    repair_started.elapsed().as_millis(),
                    attempt_started.elapsed().as_millis(),
                    round_durations_ms,
                ),
                Err(err) => terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("fold"),
                    Some(err),
                ),
            };
        }
        if max_output_tokens > 0 && cumulative_output_tokens >= max_output_tokens as u64 {
            return terminal_with_trace!(
                ContinuationRepairStatus::CappedMaxOutputTokens,
                current_token,
                sent_rounds,
                ContinuationRepairStatus::CappedMaxOutputTokens.default_failure_kind(),
                Some("continuation max output token cap reached".to_string()),
            );
        }
        if sent_rounds >= max_rounds {
            return terminal_with_trace!(
                ContinuationRepairStatus::StillMatched,
                current_token,
                sent_rounds,
                ContinuationRepairStatus::StillMatched.default_failure_kind(),
                Some("continuation still matched after max rounds".to_string()),
            );
        }
        if !codex_reasoning_continuation::latest_reasoning_has_encrypted_content(&current) {
            return terminal_with_trace!(
                ContinuationRepairStatus::MissingEncrypted,
                current_token,
                sent_rounds,
                ContinuationRepairStatus::MissingEncrypted.default_failure_kind(),
                Some("continuation round matched but encrypted reasoning was missing".to_string()),
            );
        }
        replay_tail.extend(codex_reasoning_continuation::reasoning_items(&current));
        replay_tail.push(codex_reasoning_continuation::commentary_marker_item());
        let payload = match codex_reasoning_continuation::build_continuation_payload(
            prepared.upstream_body_bytes.as_ref(),
            &replay_tail,
        ) {
            Ok(payload) => payload,
            Err(err) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("payload"),
                    Some(err),
                );
            }
        };

        let mut continuation_prepared = prepared.clone();
        continuation_prepared.upstream_body_bytes = payload;
        continuation_prepared.strip_request_content_encoding = true;
        continuation_prepared.request_body_mutated_before_attempt = true;

        let round_started = Instant::now();
        let send_outcome = super::attempt_executor::send_prepared_upstream(
            ctx,
            input,
            &mut continuation_prepared,
            retry_state,
            retry_index,
            attempt_index,
            None,
        )
        .await;
        let resp = match send_outcome {
            PreparedSendOutcome::Response(resp, _) => resp,
            PreparedSendOutcome::Timeout(_) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("upstream_timeout"),
                    Some("continuation upstream timeout".to_string()),
                );
            }
            PreparedSendOutcome::ReqwestError(err, _) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("upstream_transport"),
                    Some(format!("continuation upstream request error: {err}")),
                );
            }
            PreparedSendOutcome::UrlBuildFailed(err) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("request_build"),
                    Some(format!("continuation upstream URL error: {}", err.error)),
                );
            }
            PreparedSendOutcome::OAuthInjectFailed(_) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("auth"),
                    Some("continuation auth injection failed".to_string()),
                );
            }
            PreparedSendOutcome::PluginBlocked(reason) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("plugin"),
                    Some(format!("continuation request blocked by plugin: {reason}")),
                );
            }
        };

        if !resp.status().is_success() {
            return terminal_with_trace!(
                ContinuationRepairStatus::Failed,
                current_token,
                sent_rounds,
                Some("upstream_http"),
                Some(format!(
                    "continuation upstream returned HTTP {}",
                    resp.status().as_u16()
                )),
            );
        }
        let mut headers = resp.headers().clone();
        if !is_event_stream(&headers) {
            return terminal_with_trace!(
                ContinuationRepairStatus::Failed,
                current_token,
                sent_rounds,
                Some("upstream_protocol"),
                Some("continuation upstream did not return event-stream".to_string()),
            );
        }
        let raw = match read_buffered_event_stream_body(
            resp,
            &mut headers,
            upstream_stream_idle_timeout,
            enable_response_fixer,
            response_fixer_stream_config,
            ctx.special_settings,
        )
        .await
        {
            Ok(raw) => raw,
            Err(err) => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("upstream_stream"),
                    Some(err),
                );
            }
        };
        if raw.is_empty() {
            return terminal_with_trace!(
                ContinuationRepairStatus::Failed,
                current_token,
                sent_rounds,
                Some("upstream_stream"),
                Some("continuation upstream returned empty event-stream".to_string()),
            );
        }
        let round_duration_ms = round_started.elapsed().as_millis();
        aggregate_raw_bytes = match aggregate_raw_bytes.checked_add(raw.len()) {
            Some(total) => total,
            None => {
                return terminal_with_trace!(
                    ContinuationRepairStatus::Failed,
                    current_token,
                    sent_rounds,
                    Some("aggregate_bytes"),
                    Some("continuation repair aggregate raw bytes overflowed".to_string()),
                );
            }
        };
        if aggregate_raw_bytes > MAX_NON_SSE_BODY_BYTES {
            return terminal_with_trace!(
                ContinuationRepairStatus::Failed,
                current_token,
                sent_rounds,
                Some("aggregate_bytes"),
                Some(format!(
                    "continuation repair aggregate raw bytes exceeded cap ({} > {})",
                    aggregate_raw_bytes, MAX_NON_SSE_BODY_BYTES
                )),
            );
        }
        let aggregated =
            match crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref()) {
                Ok(value) => value,
                Err(_) => {
                    return terminal_with_trace!(
                        ContinuationRepairStatus::Failed,
                        current_token,
                        sent_rounds,
                        Some("aggregate"),
                        Some("failed_to_aggregate_continuation_event_stream".to_string()),
                    );
                }
            };
        sent_rounds = sent_rounds.saturating_add(1);
        cumulative_output_tokens = cumulative_output_tokens
            .saturating_add(codex_reasoning_continuation::output_tokens(&aggregated));
        current = aggregated.clone();
        round_durations_ms.push(round_duration_ms);
        rounds.push(codex_reasoning_continuation::ContinuationRepairRound::new(
            codex_reasoning_continuation::ContinuationRepairRoundKind::Continuation,
            raw,
            aggregated,
            Some(round_duration_ms),
        ));
    }
}

async fn read_buffered_event_stream_body(
    resp: reqwest::Response,
    response_headers: &mut HeaderMap,
    upstream_stream_idle_timeout: Option<Duration>,
    enable_response_fixer: bool,
    response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    special_settings: &std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) -> Result<Bytes, String> {
    let mut body_stream = resp.bytes_stream();
    let raw = read_buffered_event_stream_chunks(
        &mut body_stream,
        upstream_stream_idle_timeout,
        !has_non_identity_content_encoding(response_headers),
    )
    .await?;

    let raw = if has_gzip_content_encoding(response_headers) {
        maybe_gunzip_response_body_bytes_with_limit(raw, response_headers, MAX_NON_SSE_BODY_BYTES)
    } else {
        raw
    };

    if enable_response_fixer && !has_non_identity_content_encoding(response_headers) {
        response_headers.remove(header::CONTENT_LENGTH);
        let fixer_outcome = response_fixer::process_non_stream(raw, response_fixer_stream_config);
        if let Some(setting) = fixer_outcome.special_setting {
            response_fixer::push_special_setting(special_settings, setting);
        }
        Ok(fixer_outcome.body)
    } else {
        Ok(raw)
    }
}

async fn read_buffered_event_stream_chunks<S, E>(
    stream: &mut S,
    upstream_stream_idle_timeout: Option<Duration>,
    detect_terminal_events: bool,
) -> Result<Bytes, String>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: Display,
{
    let Some(body_timeout) = upstream_stream_idle_timeout else {
        return Err("continuation event-stream terminal event timeout disabled".to_string());
    };
    let mut raw = Vec::new();
    let mut terminal_scan_cursor = 0usize;
    let started = Instant::now();
    loop {
        let Some(remaining) = body_timeout.checked_sub(started.elapsed()) else {
            return Err("continuation event-stream terminal event timeout".to_string());
        };
        let next_chunk =
            match tokio::time::timeout(remaining.min(body_timeout), stream_next(stream)).await {
                Ok(Some(Ok(chunk))) => Some(chunk),
                Ok(Some(Err(err))) => {
                    return Err(format!("failed to read continuation event-stream: {err}"));
                }
                Ok(None) => None,
                Err(_) => {
                    if body_timeout.checked_sub(started.elapsed()).is_none() {
                        return Err("continuation event-stream terminal event timeout".to_string());
                    }
                    return Err("continuation event-stream idle timeout".to_string());
                }
            };
        let Some(chunk) = next_chunk else {
            break;
        };
        raw.extend_from_slice(chunk.as_ref());
        if raw.len() > MAX_NON_SSE_BODY_BYTES {
            return Err(format!(
                "continuation event-stream exceeded gateway buffer limit ({} bytes)",
                MAX_NON_SSE_BODY_BYTES
            ));
        }
        if detect_terminal_events
            && buffered_event_stream_has_terminal_event(&raw, &mut terminal_scan_cursor)?
        {
            break;
        }
    }

    Ok(Bytes::from(raw))
}

async fn stream_next<S, E>(stream: &mut S) -> Option<Result<Bytes, E>>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

fn buffered_event_stream_has_terminal_event(
    raw: &[u8],
    cursor: &mut usize,
) -> Result<bool, String> {
    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[*cursor..]) {
        let event_end = *cursor + relative_end;
        let frame = std::str::from_utf8(&raw[*cursor..event_end])
            .map_err(|err| format!("invalid utf-8 in continuation SSE frame: {err}"))?;
        *cursor = event_end;
        if crate::gateway::proxy::sse::sse_frame_has_terminal_event(frame) {
            return Ok(true);
        }
    }
    Ok(false)
}

struct ContinuationSpecialSettingContext<'a> {
    provider_id: i64,
    provider_name: &'a str,
    retry_index: u32,
    post_match_strategy: crate::settings::CodexReasoningGuardPostMatchStrategy,
    max_rounds: u32,
    max_output_tokens: u32,
    upstream_first_byte_timeout_secs: Option<u32>,
    upstream_stream_idle_timeout_secs: Option<u32>,
    upstream_stream_idle_timeout_source: &'static str,
}

fn continuation_timeout_source(outcome: &ContinuationRepairOutcome) -> Option<&'static str> {
    match outcome.failure_kind {
        Some("upstream_timeout") => Some("first_byte"),
        Some("upstream_stream")
            if outcome
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("terminal event timeout disabled")) =>
        {
            Some("terminal_event_disabled")
        }
        Some("upstream_stream")
            if outcome
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("terminal event timeout")) =>
        {
            Some("terminal_event")
        }
        Some("upstream_stream")
            if outcome
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("idle timeout")) =>
        {
            Some("stream_idle")
        }
        _ => None,
    }
}

fn continuation_fallback_action(status: ContinuationRepairStatus) -> &'static str {
    match status {
        ContinuationRepairStatus::Repaired => "none",
        ContinuationRepairStatus::NotApplicable => "not_applicable",
        _ => "existing_guard_failover_or_exhausted",
    }
}

fn push_continuation_special_setting(
    special_settings: &std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    context: ContinuationSpecialSettingContext<'_>,
    outcome: &ContinuationRepairOutcome,
) {
    if outcome.status == ContinuationRepairStatus::NotApplicable {
        return;
    }
    let mut setting = serde_json::json!({
            "type": "codex_reasoning_continuation",
            "scope": "attempt",
            "providerId": context.provider_id,
            "providerName": context.provider_name,
            "retryAttemptNumber": context.retry_index,
            "status": outcome.status.as_str(),
            "sentRounds": outcome.sent_rounds,
            "maxRounds": context.max_rounds,
            "maxOutputTokens": context.max_output_tokens,
            "reasoningTokens": outcome.reasoning_tokens,
            "reasoningTokensPointer": outcome.reasoning_tokens_pointer,
            "failureKind": outcome.failure_kind,
            "reason": outcome.reason.as_deref(),
    });

    if context
        .post_match_strategy
        .is_experimental_continuation_repair()
    {
        let object = setting
            .as_object_mut()
            .expect("continuation special setting is an object");
        object.insert(
            "timing".to_string(),
            serde_json::json!({
                "observedCumulativeDurationMs":
                outcome
                    .cumulative_elapsed_ms
                    .map(|value| value.min(u128::from(u64::MAX)) as u64),
                "observedRepairDurationMs":
                outcome
                    .repair_elapsed_ms
                    .map(|value| value.min(u128::from(u64::MAX)) as u64),
                "observedRoundDurationsMs":
                    outcome
                        .round_durations_ms
                        .iter()
                        .map(|value| Value::from((*value).min(u128::from(u64::MAX)) as u64))
                        .collect::<Vec<_>>(),
            }),
        );
        object.insert(
            "timeoutPolicy".to_string(),
            serde_json::json!({
                "source": "global_upstream_timeout_settings",
                "firstByteTimeoutSeconds": context.upstream_first_byte_timeout_secs,
                "streamIdleTimeoutSeconds": context.upstream_stream_idle_timeout_secs,
                "streamIdleTimeoutSource": context.upstream_stream_idle_timeout_source,
                "terminalEventTimeoutSeconds": context.upstream_stream_idle_timeout_secs,
                "terminalEventTimeoutSource": context.upstream_stream_idle_timeout_source,
                "privateRoundTimeoutSeconds": Value::Null,
                "privateRepairWallClockCapSeconds": Value::Null,
            }),
        );
        object.insert(
            "timeoutSource".to_string(),
            serde_json::to_value(continuation_timeout_source(outcome)).unwrap_or(Value::Null),
        );
        object.insert(
            "fallbackAction".to_string(),
            Value::String(continuation_fallback_action(outcome.status).to_string()),
        );
        object.insert(
            "downstreamHeadersCommittedDuringRepair".to_string(),
            Value::Bool(false),
        );
        object.insert(
            "aggregateRawBytes".to_string(),
            serde_json::to_value(outcome.aggregate_raw_bytes).unwrap_or(Value::Null),
        );
        object.insert(
            "clientContractVersion".to_string(),
            Value::String(codex_reasoning_continuation::BPLUS_CLIENT_CONTRACT_VERSION.to_string()),
        );
        object.insert(
            "reconstructionStatus".to_string(),
            serde_json::to_value(outcome.reconstruction_status).unwrap_or(Value::Null),
        );
        object.insert(
            "visibleAssemblyKind".to_string(),
            serde_json::to_value(outcome.visible_assembly_kind).unwrap_or(Value::Null),
        );
        object.insert(
            "clientUsageKind".to_string(),
            serde_json::to_value(if outcome.client_usage.is_some() {
                Some("delivered_client_body_usage")
            } else {
                None
            })
            .unwrap_or(Value::Null),
        );
        object.insert(
            "providerUsageKind".to_string(),
            serde_json::to_value(if outcome.provider_repair_usage.is_some() {
                Some("provider_repair_usage")
            } else {
                None
            })
            .unwrap_or(Value::Null),
        );
        object.insert(
            "clientUsage".to_string(),
            usage_extract_json_value(outcome.client_usage.as_ref()).unwrap_or(Value::Null),
        );
        object.insert(
            "providerRepairUsage".to_string(),
            usage_extract_json_value(outcome.provider_repair_usage.as_ref()).unwrap_or(Value::Null),
        );
        object.insert(
            "canonicalResponseId".to_string(),
            serde_json::to_value(outcome.canonical_response_id.as_deref()).unwrap_or(Value::Null),
        );
        object.insert(
            "canonicalResponseIdContinuity".to_string(),
            serde_json::to_value(outcome.canonical_response_id_continuity).unwrap_or(Value::Null),
        );
        object.insert(
            "rounds".to_string(),
            Value::Array(outcome.round_trace.clone()),
        );
        object.insert(
            "nonVisiblePolicy".to_string(),
            serde_json::json!({
                "reasoningItems": outcome.round_trace.iter().filter_map(|entry| {
                    entry
                        .get("hasReasoning")
                        .and_then(Value::as_bool)
                        .is_some_and(|value| value)
                        .then_some(1u64)
                }).sum::<u64>(),
                "commentaryMarkers": outcome.round_trace.iter().filter_map(|entry| {
                    entry
                        .get("hasCommentaryMarker")
                        .and_then(Value::as_bool)
                        .is_some_and(|value| value)
                        .then_some(1u64)
                }).sum::<u64>(),
                "toolCallRounds": outcome.round_trace.iter().filter_map(|entry| {
                    entry
                        .get("hasToolCall")
                        .and_then(Value::as_bool)
                        .is_some_and(|value| value)
                        .then_some(1u64)
                }).sum::<u64>(),
                "policyResult": outcome.reconstruction_status.unwrap_or(outcome.status.as_str()),
            }),
        );
        object.insert(
            "phase0SampleAudit".to_string(),
            serde_json::json!({
                "cleanAppendEnabled": codex_reasoning_continuation::BPLUS_CLEAN_APPEND_ENABLED,
                "realUpstreamTranscriptContinuity": codex_reasoning_continuation::BPLUS_RESPONSE_ID_CONTINUITY,
            }),
        );
    }

    response_fixer::push_special_setting(special_settings, setting);
}

fn usage_extract_json_value(usage: Option<&usage::UsageExtract>) -> Option<Value> {
    usage.and_then(|usage| serde_json::from_str::<Value>(&usage.usage_json).ok())
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
        provider_cooldown_secs,
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
        circuit_before: _,
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
            ),
        ))
    };
    if let Some(change) = &change {
        *circuit_snapshot = change.after.clone();
    }

    if !oauth_quota_exhausted && provider_cooldown_secs > 0 {
        let snap = provider_router::trigger_cooldown(
            state.circuit.as_ref(),
            provider_id,
            now_unix,
            provider_cooldown_secs,
        );
        *circuit_snapshot = snap;
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
    input: &RequestContext<R>,
    provider_ctx: ProviderCtx<'_>,
    attempt_ctx: AttemptCtx<'_>,
    prepared: PreparedProvider,
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
        attempt_index,
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
    // Empty-success classification needs terminal SSE usage before response headers are sent,
    // otherwise the gateway cannot return 502 or fail over to the next provider.
    let should_buffer_codex_responses_event_stream =
        should_buffer_native_codex_responses_for_empty_detection(
            &common.cli_key,
            &common.forwarded_path,
            active_bridge_type,
        );
    let should_buffer_empty_response = should_buffer_codex_responses_event_stream;
    let should_buffer_codex_reasoning_guard =
        should_buffer_native_codex_responses_for_reasoning_guard(
            common.codex_reasoning_guard_enabled,
            &common.cli_key,
            &common.forwarded_path,
            active_bridge_type,
            response_headers.get(header::CONTENT_LENGTH).is_some(),
        );
    let should_buffer_codex_event_stream =
        should_buffer_empty_response || should_buffer_codex_reasoning_guard;

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
            })
            .await;
        }

        if should_buffer_codex_event_stream {
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
                            active_requested_model,
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
                                    active_requested_model,
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
                                    active_requested_model,
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
                                    active_requested_model,
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
                            active_requested_model,
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

            let mut raw = if should_buffer_codex_responses_event_stream
                && enable_response_fixer
                && !has_non_identity_content_encoding(&response_headers)
            {
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

            let plugin_pipeline = common.state.plugin_pipeline.clone();
            if plugin_pipeline.has_plugins_for_hook(
                crate::gateway::plugins::context::GatewayPluginHookName::ResponseChunk,
            ) {
                raw = apply_plugin_chunk_hooks(
                    plugin_pipeline,
                    common.state.db.clone(),
                    common.trace_id.clone(),
                    raw,
                    1,
                )
                .await;
                response_headers.remove(header::CONTENT_LENGTH);
            }

            let mut stream_completion_error_code = is_plugin_stream_error_chunk(raw.as_ref())
                .then_some(GatewayErrorCode::Fake200.as_str());
            if stream_completion_error_code.is_none() {
                if let Some(error_code) = buffered_stream_error_code(
                    common.cli_key.as_str(),
                    common.forwarded_path.as_str(),
                    status.as_u16(),
                    raw.as_ref(),
                ) {
                    if error_code == GatewayErrorCode::Fake200.as_str()
                        && !should_buffer_codex_responses_event_stream
                    {
                        raw = Bytes::new();
                        stream_completion_error_code = Some(error_code);
                    } else {
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
                            raw.as_ref(),
                            error_code,
                        )
                        .await;
                    }
                }
            }

            let aggregated = if should_buffer_codex_responses_event_stream {
                match crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref()) {
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

                        return record_system_failure_and_decide_no_cooldown(
                            RecordSystemFailureArgs {
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
                                reason: format!(
                                    "failed to aggregate Codex responses event-stream: {err}"
                                ),
                                record_circuit_failure: true,
                            },
                        )
                        .await;
                    }
                }
            } else {
                codex_chat_completions_sse_guard_value(common.cli_key.as_str(), raw.as_ref())
                    .unwrap_or_else(|| serde_json::json!({}))
            };

            let special_settings_snapshot =
                codex_reasoning_features::special_settings_snapshot(&common.special_settings);
            let feature_sample = codex_reasoning_features::build_complete_sample(
                common.cli_key.as_str(),
                common.codex_reasoning_guard_rule_mode,
                Some(&input.base_headers),
                input.introspection_json.as_ref(),
                special_settings_snapshot.as_slice(),
                &aggregated,
            );

            let active_guard_model =
                provider_ctx_owned
                    .active_requested_model
                    .clone()
                    .or_else(|| {
                        current_codex_reasoning_guard_model(input, retry_state)
                            .map(ToOwned::to_owned)
                    });
            let mut repaired_client_usage: Option<usage::UsageExtract> = None;
            let mut repaired_provider_repair_usage: Option<usage::UsageExtract> = None;

            if let Some(decision) = if should_buffer_codex_reasoning_guard {
                codex_reasoning_guard::evaluate_decision(
                    codex_reasoning_guard::CodexReasoningGuardDecisionEvaluationInput {
                        base: codex_reasoning_guard::CodexReasoningGuardEvaluationInput {
                            cli_key: common.cli_key.as_str(),
                            requested_model: active_guard_model.as_deref(),
                            value: &aggregated,
                            rule_mode: common.codex_reasoning_guard_rule_mode,
                            feature_sample: feature_sample.as_ref(),
                        },
                        active_template_id: common
                            .codex_reasoning_guard_active_template_id
                            .as_str(),
                        custom_templates: common.codex_reasoning_guard_custom_templates.as_slice(),
                        duration_ms: Some(started.elapsed().as_millis()),
                        ttfb_ms: initial_first_byte_ms,
                    },
                )
            } else {
                None
            } {
                let matched = &decision.matched;
                if decision.action
                    == crate::settings::CodexReasoningGuardTemplateRuleAction::NoIntercept
                {
                    if let Some(sample) = feature_sample.as_ref() {
                        codex_reasoning_features::push_special_setting(
                            &common.special_settings,
                            sample,
                        );
                    }
                    codex_reasoning_guard::push_decision_special_setting(
                        &common.special_settings,
                        provider_id,
                        provider_ctx_owned.provider_name_base.as_str(),
                        retry_index,
                        matched,
                    );
                } else if common
                    .codex_reasoning_guard_post_match_strategy
                    .is_continuation_repair()
                {
                    let current_token =
                        codex_reasoning_features::extract_reasoning_tokens(&aggregated);
                    let continuation_outcome = if common
                        .codex_reasoning_guard_immediate_retry_budget
                        == 0
                    {
                        ContinuationRepairOutcome::terminal(
                            ContinuationRepairStatus::BudgetExhausted,
                            current_token,
                            0,
                            Some("continuation repair retry budget is 0".to_string()),
                        )
                    } else if !prepared.codex_reasoning_continuation_request_eligible
                        || !codex_reasoning_continuation::request_reasoning_enabled(
                            prepared.upstream_body_bytes.as_ref(),
                        )
                    {
                        ContinuationRepairOutcome::terminal(
                                    ContinuationRepairStatus::Unavailable,
                                    current_token,
                                    0,
                                    Some(
                                        "continuation repair requires native Codex Responses streaming with encrypted reasoning replay"
                                            .to_string(),
                                    ),
                                )
                    } else {
                        run_codex_reasoning_continuation_repair(
                            ctx,
                            input,
                            &prepared,
                            retry_state,
                            retry_index,
                            attempt_index,
                            attempt_started,
                            raw.clone(),
                            &aggregated,
                            common.codex_reasoning_guard_post_match_strategy,
                            upstream_stream_idle_timeout,
                            enable_response_fixer,
                            response_fixer_stream_config,
                            common.codex_reasoning_guard_immediate_retry_budget,
                            common.codex_reasoning_guard_continuation_max_output_tokens,
                        )
                        .await
                    };
                    push_continuation_special_setting(
                        &common.special_settings,
                        ContinuationSpecialSettingContext {
                            provider_id,
                            provider_name: provider_ctx_owned.provider_name_base.as_str(),
                            retry_index,
                            post_match_strategy: common.codex_reasoning_guard_post_match_strategy,
                            max_rounds: common.codex_reasoning_guard_immediate_retry_budget,
                            max_output_tokens: common
                                .codex_reasoning_guard_continuation_max_output_tokens,
                            upstream_first_byte_timeout_secs: upstream_first_byte_timeout
                                .map(|_| upstream_first_byte_timeout_secs),
                            upstream_stream_idle_timeout_secs: effective_stream_idle_timeout
                                .seconds,
                            upstream_stream_idle_timeout_source: effective_stream_idle_timeout
                                .source,
                        },
                        &continuation_outcome,
                    );
                    if continuation_outcome.status == ContinuationRepairStatus::Repaired {
                        repaired_client_usage = continuation_outcome.client_usage.clone();
                        repaired_provider_repair_usage =
                            continuation_outcome.provider_repair_usage.clone();
                        if let Some(client_raw) = continuation_outcome.client_raw.clone() {
                            raw = client_raw;
                            response_headers.remove(header::CONTENT_LENGTH);
                            response_headers.remove(header::CONTENT_ENCODING);
                            response_headers.insert(
                                header::CONTENT_TYPE,
                                HeaderValue::from_static("text/event-stream; charset=utf-8"),
                            );
                        }
                        if let Ok(repaired_aggregated) =
                            crate::gateway::proxy::sse::aggregate_responses_event_stream(
                                raw.as_ref(),
                            )
                        {
                            let repaired_sample = codex_reasoning_features::build_complete_sample(
                                common.cli_key.as_str(),
                                common.codex_reasoning_guard_rule_mode,
                                Some(&input.base_headers),
                                input.introspection_json.as_ref(),
                                special_settings_snapshot.as_slice(),
                                &repaired_aggregated,
                            );
                            if let Some(sample) = repaired_sample.as_ref() {
                                codex_reasoning_features::push_special_setting(
                                    &common.special_settings,
                                    sample,
                                );
                            }
                        }
                        let strategy_decision =
                            codex_reasoning_guard::continuation_repaired_decision(
                                retry_state.codex_reasoning_guard_hits,
                                common.codex_reasoning_guard_immediate_retry_budget,
                                continuation_outcome.sent_rounds,
                                common.codex_reasoning_guard_exhausted_action,
                            );
                        codex_reasoning_guard::push_special_setting_with_strategy(
                            &common.special_settings,
                            provider_id,
                            provider_ctx_owned.provider_name_base.as_str(),
                            retry_index,
                            matched,
                            strategy_decision,
                            common.codex_reasoning_guard_post_match_strategy,
                            Some("continuation_repaired"),
                            Some(continuation_outcome.sent_rounds),
                            continuation_outcome.failure_kind,
                            continuation_outcome.reason.as_deref(),
                            StatusCode::OK.as_u16(),
                        );
                    } else {
                        if let Some(sample) = feature_sample.as_ref() {
                            codex_reasoning_features::push_special_setting(
                                &common.special_settings,
                                sample,
                            );
                        }
                        let budget_decision =
                            codex_reasoning_guard::continuation_exhausted_decision(
                                retry_state.codex_reasoning_guard_hits,
                                common.codex_reasoning_guard_immediate_retry_budget,
                                common.codex_reasoning_guard_exhausted_action,
                            );
                        codex_reasoning_guard::push_special_setting_with_strategy(
                            &common.special_settings,
                            provider_id,
                            provider_ctx_owned.provider_name_base.as_str(),
                            retry_index,
                            matched,
                            budget_decision,
                            common.codex_reasoning_guard_post_match_strategy,
                            Some(continuation_outcome.status.as_str()),
                            Some(continuation_outcome.sent_rounds),
                            continuation_outcome.failure_kind,
                            continuation_outcome.reason.as_deref(),
                            StatusCode::BAD_GATEWAY.as_u16(),
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
                            provider_ctx_owned.provider_bridged,
                            matched,
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
                                codex_reasoning_guard::CodexReasoningGuardBudgetAction::SwitchModel => {
                                    "codex_reasoning_guard_switch_model"
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
                                    return LoopControl::ContinueRetry;
                                }
                                codex_reasoning_guard::CodexReasoningGuardBudgetAction::ReturnError => {
                                    *last_outcome = Some(AttemptOutcome::new(
                                        ErrorCategory::SystemError.as_str(),
                                        codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                                    ));
                                    let duration_ms = started.elapsed().as_millis();
                                    let requested_model_for_log = active_guard_model
                                        .clone()
                                        .or_else(|| common.requested_model.clone());
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
                                            special_settings_json:
                                                response_fixer::special_settings_json(
                                                    &common.special_settings,
                                                ),
                                            session_id: common.session_id.clone(),
                                            requested_model: requested_model_for_log,
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
                                        "Codex reasoning guard continuation repair failed"
                                            .to_string(),
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
                                codex_reasoning_guard::CodexReasoningGuardBudgetAction::SwitchModel => {
                                    retry_state.codex_reasoning_guard_next_retry_wave = None;
                                    if let Some(next_model) =
                                        codex_reasoning_guard::select_next_model_fallback(
                                            active_guard_model.as_deref(),
                                            common.codex_reasoning_guard_model_fallbacks.as_slice(),
                                        )
                                    {
                                        return LoopControl::SwitchModel(next_model.to_string());
                                    }

                                    *last_outcome = Some(AttemptOutcome::new(
                                        ErrorCategory::SystemError.as_str(),
                                        codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                                    ));
                                    let duration_ms = started.elapsed().as_millis();
                                    let requested_model_for_log = active_guard_model
                                        .clone()
                                        .or_else(|| common.requested_model.clone());
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
                                            special_settings_json:
                                                response_fixer::special_settings_json(
                                                    &common.special_settings,
                                                ),
                                            session_id: common.session_id.clone(),
                                            requested_model: requested_model_for_log,
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
                                        "Codex reasoning guard model fallback exhausted"
                                            .to_string(),
                                        attempts.clone(),
                                    ));
                                }
                            }
                    }
                } else {
                    if let Some(sample) = feature_sample.as_ref() {
                        codex_reasoning_features::push_special_setting(
                            &common.special_settings,
                            sample,
                        );
                    }
                    let budget_decision = codex_reasoning_guard::budget_decision(
                        retry_state.codex_reasoning_guard_hits,
                        codex_reasoning_guard::CodexReasoningGuardBudgetConfig {
                            immediate_budget: common.codex_reasoning_guard_immediate_retry_budget,
                            delayed_budget: common.codex_reasoning_guard_delayed_retry_budget,
                            delayed_retry_ms: common.codex_reasoning_guard_delayed_retry_ms,
                            exhausted_action: common.codex_reasoning_guard_exhausted_action,
                            retry_policy: common.codex_reasoning_guard_retry_policy,
                            concurrent_max: common.codex_reasoning_guard_concurrent_max,
                            concurrent_interval_ms: common
                                .codex_reasoning_guard_concurrent_interval_ms,
                            concurrent_max_attempts: common
                                .codex_reasoning_guard_concurrent_max_attempts,
                        },
                    );
                    codex_reasoning_guard::push_special_setting(
                        &common.special_settings,
                        provider_id,
                        provider_ctx_owned.provider_name_base.as_str(),
                        retry_index,
                        matched,
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
                        provider_ctx_owned.provider_bridged,
                        matched,
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
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::SwitchModel => {
                        "codex_reasoning_guard_switch_model"
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
                        retry_state
                            .remember_codex_reasoning_guard_retry_wave(budget_decision.retry_wave);
                        codex_reasoning_guard::apply_delay_if_needed(budget_decision).await;
                        return LoopControl::ContinueRetry;
                    }
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::ReturnError => {
                        *last_outcome = Some(AttemptOutcome::new(
                            ErrorCategory::SystemError.as_str(),
                            codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                        ));
                        let duration_ms = started.elapsed().as_millis();
                        let requested_model_for_log = active_guard_model
                            .clone()
                            .or_else(|| common.requested_model.clone());
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
                                special_settings_json: response_fixer::special_settings_json(
                                    &common.special_settings,
                                ),
                                session_id: common.session_id.clone(),
                                requested_model: requested_model_for_log,
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
                    codex_reasoning_guard::CodexReasoningGuardBudgetAction::SwitchModel => {
                        retry_state.codex_reasoning_guard_next_retry_wave = None;
                        if let Some(next_model) = codex_reasoning_guard::select_next_model_fallback(
                            active_guard_model.as_deref(),
                            common.codex_reasoning_guard_model_fallbacks.as_slice(),
                        ) {
                            return LoopControl::SwitchModel(next_model.to_string());
                        }

                        *last_outcome = Some(AttemptOutcome::new(
                            ErrorCategory::SystemError.as_str(),
                            codex_reasoning_guard::CODEX_REASONING_GUARD_ERROR_CODE,
                        ));
                        let duration_ms = started.elapsed().as_millis();
                        let requested_model_for_log = active_guard_model
                            .clone()
                            .or_else(|| common.requested_model.clone());
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
                                special_settings_json: response_fixer::special_settings_json(
                                    &common.special_settings,
                                ),
                                session_id: common.session_id.clone(),
                                requested_model: requested_model_for_log,
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
                            "Codex reasoning guard model fallback exhausted".to_string(),
                            attempts.clone(),
                        ));
                    }
                    }
                }
            } else if let Some(sample) = feature_sample.as_ref() {
                codex_reasoning_features::push_special_setting(&common.special_settings, sample);
            }

            if let (Some(namespace), Some(input)) =
                (responses_cache_namespace, responses_cache_input)
            {
                protocol_bridge::response_cache::cache_completed_response(
                    namespace,
                    input,
                    &aggregated,
                );
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
                provider_bridged: Some(provider_ctx_owned.provider_bridged),
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

            let parsed_usage =
                usage::parse_usage_from_json_or_sse_bytes(common.cli_key.as_str(), &raw);
            let client_usage = repaired_client_usage
                .clone()
                .or_else(|| parsed_usage.clone());
            let usage = repaired_provider_repair_usage
                .clone()
                .or_else(|| parsed_usage.clone());
            let usage_metrics = client_usage.as_ref().map(|u| u.metrics.clone());
            let log_usage_metrics = repaired_provider_repair_usage
                .as_ref()
                .map(|u| u.metrics.clone());
            let requested_model_for_log = resolve_requested_model_for_log(
                common.requested_model.clone(),
                provider_ctx_owned
                    .active_requested_model
                    .as_deref()
                    .or(retry_state.codex_reasoning_guard_current_model.as_deref()),
                common.cli_key.as_str(),
                &raw,
            );

            let now_unix = now_unix_seconds() as i64;
            if let Some(error_code) = stream_completion_error_code {
                let change = provider_router::record_failure_and_emit_transition(
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
                *circuit_snapshot = change.after.clone();
                if common.provider_cooldown_secs > 0 {
                    *circuit_snapshot = provider_router::trigger_cooldown(
                        common.state.circuit.as_ref(),
                        provider_id,
                        now_unix,
                        common.provider_cooldown_secs,
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
            } else {
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
            }

            let duration_ms = started.elapsed().as_millis();
            let completion = if let Some(error_code) = stream_completion_error_code {
                RequestCompletion::failure_with_visible_ttfb(
                    status.as_u16(),
                    Some(ErrorCategory::ProviderError.as_str()),
                    error_code,
                    initial_first_byte_ms,
                    Some(duration_ms),
                )
            } else {
                RequestCompletion::success_with_visible_ttfb(
                    status.as_u16(),
                    initial_first_byte_ms,
                    Some(duration_ms),
                    usage_metrics,
                    log_usage_metrics,
                    usage,
                )
            };

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
                    special_settings_json: response_fixer::special_settings_json(
                        &common.special_settings,
                    ),
                    session_id: common.session_id.clone(),
                    requested_model: requested_model_for_log,
                    created_at_ms: common.created_at_ms,
                    created_at: common.created_at,
                })
                .with_completion(completion),
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

        if !common.codex_reasoning_guard_enabled {
            let special_settings_snapshot =
                codex_reasoning_features::special_settings_snapshot(&common.special_settings);
            let request_only_sample = codex_reasoning_features::build_request_only_sample(
                common.cli_key.as_str(),
                common.codex_reasoning_guard_rule_mode,
                Some(&input.base_headers),
                input.introspection_json.as_ref(),
                special_settings_snapshot.as_slice(),
                codex_reasoning_features::SKIPPED_GUARD_DISABLED_STREAM_NOT_BUFFERED,
            );
            if let Some(sample) = request_only_sample.as_ref().filter(|sample| {
                sample.request_kind
                    == Some(codex_reasoning_features::REQUEST_KIND_CONTEXT_COMPACTION)
            }) {
                codex_reasoning_features::push_special_setting(&common.special_settings, sample);
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
            provider_bridged: Some(provider_ctx_owned.provider_bridged),
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
    use super::{
        buffered_event_stream_has_terminal_event, read_buffered_event_stream_chunks,
        resolve_effective_stream_idle_timeout, resolve_requested_model_for_log,
        should_buffer_native_codex_responses_for_empty_detection,
        should_buffer_native_codex_responses_for_reasoning_guard,
    };
    use axum::body::Bytes;
    use futures_core::Stream;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    struct SequenceThenPendingStream {
        chunks: VecDeque<Result<Bytes, &'static str>>,
    }

    impl SequenceThenPendingStream {
        fn new(chunks: Vec<Result<Bytes, &'static str>>) -> Self {
            Self {
                chunks: VecDeque::from(chunks),
            }
        }
    }

    impl Stream for SequenceThenPendingStream {
        type Item = Result<Bytes, &'static str>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.chunks
                .pop_front()
                .map_or(Poll::Pending, |chunk| Poll::Ready(Some(chunk)))
        }
    }

    struct DelayedKeepaliveStream {
        interval: Duration,
        sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    }

    impl DelayedKeepaliveStream {
        fn new(interval: Duration) -> Self {
            Self {
                interval,
                sleep: None,
            }
        }
    }

    impl Stream for DelayedKeepaliveStream {
        type Item = Result<Bytes, &'static str>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if self.sleep.is_none() {
                self.sleep = Some(Box::pin(tokio::time::sleep(self.interval)));
            }
            let sleep = self.sleep.as_mut().expect("sleep initialized");
            match sleep.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    self.sleep = None;
                    Poll::Ready(Some(Ok(Bytes::from_static(b": keepalive\n\n"))))
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }

    #[test]
    fn native_codex_stream_buffering_excludes_bridge_paths() {
        assert!(should_buffer_native_codex_responses_for_empty_detection(
            "codex",
            "/v1/responses",
            None
        ));
        assert!(!should_buffer_native_codex_responses_for_empty_detection(
            "codex",
            "/v1/responses",
            Some("codex_to_openai_chat")
        ));
        assert!(should_buffer_native_codex_responses_for_reasoning_guard(
            true,
            "codex",
            "/v1/responses",
            None,
            false
        ));
        assert!(should_buffer_native_codex_responses_for_reasoning_guard(
            true,
            "codex",
            "/v1/chat/completions",
            None,
            true
        ));
        assert!(!should_buffer_native_codex_responses_for_reasoning_guard(
            true,
            "codex",
            "/v1/chat/completions",
            None,
            false
        ));
        assert!(!should_buffer_native_codex_responses_for_reasoning_guard(
            true,
            "codex",
            "/v1/responses",
            Some("codex_to_openai_chat"),
            true
        ));
        assert!(!should_buffer_native_codex_responses_for_reasoning_guard(
            false,
            "codex",
            "/v1/responses",
            None,
            true
        ));
    }

    #[test]
    fn resolve_requested_model_for_log_prefers_reasoning_guard_fallback_model() {
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
    fn continuation_terminal_scan_stops_after_complete_terminal_frame() {
        let mut raw = Vec::new();
        let mut cursor = 0usize;
        raw.extend_from_slice(
            b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\"}\n\n",
        );
        assert!(!buffered_event_stream_has_terminal_event(&raw, &mut cursor).unwrap());
        assert_eq!(cursor, raw.len());

        raw.extend_from_slice(
            b"event: response.completed\ndata: {\"response\":{\"id\":\"r\"}}\n\n",
        );
        assert!(buffered_event_stream_has_terminal_event(&raw, &mut cursor).unwrap());
        assert_eq!(cursor, raw.len());
    }

    #[test]
    fn continuation_terminal_scan_waits_for_split_frame_and_handles_done() {
        let mut raw = b"data: [DO".to_vec();
        let mut cursor = 0usize;
        assert!(!buffered_event_stream_has_terminal_event(&raw, &mut cursor).unwrap());
        assert_eq!(cursor, 0);

        raw.extend_from_slice(b"NE]\n\n: keepalive\n\n");
        assert!(buffered_event_stream_has_terminal_event(&raw, &mut cursor).unwrap());
        assert_eq!(cursor, "data: [DONE]\n\n".len());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_reader_returns_on_terminal_event_without_waiting_for_eof() {
        let mut stream = SequenceThenPendingStream::new(vec![
            Ok(Bytes::from_static(
                b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\"}\n\n",
            )),
            Ok(Bytes::from_static(
                b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_done\"}}\n\n",
            )),
        ]);

        let raw = tokio::time::timeout(
            Duration::from_millis(100),
            read_buffered_event_stream_chunks(&mut stream, Some(Duration::from_secs(5)), true),
        )
        .await
        .expect("terminal event should finish before EOF")
        .expect("read stream");
        let text = std::str::from_utf8(raw.as_ref()).expect("utf-8");

        assert!(text.contains("response.output_item.done"));
        assert!(text.contains("response.completed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_reader_returns_on_incomplete_terminal_event_without_waiting_for_eof() {
        let mut stream = SequenceThenPendingStream::new(vec![Ok(Bytes::from_static(
            b"event: response.incomplete\ndata: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_incomplete\",\"status\":\"incomplete\"}}\n\n",
        ))]);

        let raw = tokio::time::timeout(
            Duration::from_millis(100),
            read_buffered_event_stream_chunks(&mut stream, Some(Duration::from_secs(5)), true),
        )
        .await
        .expect("incomplete terminal event should finish before EOF")
        .expect("read stream");
        let text = std::str::from_utf8(raw.as_ref()).expect("utf-8");

        assert!(text.contains("response.incomplete"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_reader_uses_configured_terminal_event_timeout_without_private_default() {
        let mut stream = DelayedKeepaliveStream::new(Duration::from_millis(2));

        let err =
            read_buffered_event_stream_chunks(&mut stream, Some(Duration::from_millis(20)), true)
                .await
                .expect_err("keepalive-only stream should hit configured terminal event timeout");

        assert_eq!(err, "continuation event-stream terminal event timeout");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_reader_fails_closed_when_stream_timeout_is_disabled() {
        let mut stream = DelayedKeepaliveStream::new(Duration::from_millis(2));

        let err = read_buffered_event_stream_chunks(&mut stream, None, true)
            .await
            .expect_err("disabled stream timeout should fail closed in continuation repair");

        assert_eq!(
            err,
            "continuation event-stream terminal event timeout disabled"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_reader_can_disable_terminal_scan_for_encoded_streams() {
        let mut stream =
            SequenceThenPendingStream::new(vec![Ok(Bytes::from_static(&[0x1f, 0x8b, 0x08, 0x00]))]);

        let err =
            read_buffered_event_stream_chunks(&mut stream, Some(Duration::from_millis(20)), false)
                .await
                .expect_err("encoded stream without EOF should hit terminal event timeout");

        assert_eq!(err, "continuation event-stream terminal event timeout");
    }
}
