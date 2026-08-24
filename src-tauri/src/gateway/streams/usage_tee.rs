//! Usage: Streaming tee wrappers that emit usage/cost and enqueue request logs.

use crate::gateway::response_fixer;
use crate::usage;
use axum::body::{Body, Bytes};
use axum::http::HeaderMap;
use futures_core::Stream;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use super::super::events::{emit_gateway_debug_log, emit_gateway_debug_log_lazy};
use super::super::model_route_mapping;
use super::super::proxy::{
    is_fake_200_non_stream_body, upstream_client_error_rules, upstream_error_response_rules,
    GatewayErrorCode,
};
use super::super::util::{
    lossy_utf8_preview, now_unix_millis, now_unix_seconds, MAX_DEBUG_BODY_PREVIEW_BYTES,
};
use super::plugin_chunk::PLUGIN_STREAM_ERROR_MARKER;
use super::request_end::{emit_request_event_and_spawn_request_log, StreamRequestCompletion};
use super::terminal_firewall::CodexTerminalFirewall;
use super::types::{StreamTerminalEvidence, StreamTerminalOrigin};
#[cfg(test)]
use super::RelayBodyStream;
use super::StreamFinalizeCtx;

pub(in crate::gateway) struct UpstreamModelObserverStream<S, B>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    upstream: S,
    tracker: Arc<Mutex<usage::SseUsageTracker>>,
    observed_model: Arc<Mutex<Option<String>>>,
    observed_conflicting_model: Arc<Mutex<Option<String>>>,
    observed_reasoning_effort: Arc<Mutex<Option<String>>>,
    _marker: std::marker::PhantomData<B>,
}

impl<S, B> UpstreamModelObserverStream<S, B>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    pub(in crate::gateway) fn new(
        upstream: S,
        tracker: Arc<Mutex<usage::SseUsageTracker>>,
        observed_model: Arc<Mutex<Option<String>>>,
        observed_conflicting_model: Arc<Mutex<Option<String>>>,
        observed_reasoning_effort: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            upstream,
            tracker,
            observed_model,
            observed_conflicting_model,
            observed_reasoning_effort,
            _marker: std::marker::PhantomData,
        }
    }

    fn finalize_observed_route(&mut self) {
        let _ = finalize_upstream_route_observation(
            &self.tracker,
            &self.observed_model,
            &self.observed_conflicting_model,
            &self.observed_reasoning_effort,
        );
    }
}

fn finalize_upstream_route_observation(
    tracker: &Arc<Mutex<usage::SseUsageTracker>>,
    observed_model: &Arc<Mutex<Option<String>>>,
    observed_conflicting_model: &Arc<Mutex<Option<String>>>,
    observed_reasoning_effort: &Arc<Mutex<Option<String>>>,
) -> (usage::ModelRouteEvidence, Option<usage::UsageExtract>) {
    let (evidence, usage) = {
        let mut tracker = match tracker.lock() {
            Ok(tracker) => tracker,
            Err(poisoned) => poisoned.into_inner(),
        };
        let usage = tracker.finalize();
        (tracker.best_effort_route_evidence(), usage)
    };

    if let Some(model) = evidence.first_model.as_ref() {
        if let Ok(mut observed) = observed_model.lock() {
            *observed = Some(model.clone());
        }
    }
    if let Some(model) = evidence.first_conflicting_model.as_ref() {
        if let Ok(mut observed) = observed_conflicting_model.lock() {
            *observed = Some(model.clone());
        }
    }
    if let Some(effort) = evidence.reasoning_effort.as_ref() {
        if let Ok(mut observed) = observed_reasoning_effort.lock() {
            *observed = Some(effort.clone());
        }
    }
    (evidence, usage)
}

impl<S, B> Stream for UpstreamModelObserverStream<S, B>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + Unpin,
{
    type Item = Result<B, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        match Pin::new(&mut this.upstream).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                match this.tracker.lock() {
                    Ok(mut tracker) => tracker.ingest_chunk(chunk.as_ref()),
                    Err(poisoned) => poisoned.into_inner().ingest_chunk(chunk.as_ref()),
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(err))) => {
                this.finalize_observed_route();
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(None) => {
                this.finalize_observed_route();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S, B> Drop for UpstreamModelObserverStream<S, B>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    fn drop(&mut self) {
        self.finalize_observed_route();
    }
}

fn is_codex_responses_path(cli_key: &str, path: &str) -> bool {
    if cli_key != "codex" {
        return false;
    }
    matches!(
        path.trim_end_matches('/'),
        "/v1/responses" | "/responses" | "/v1/codex/responses"
    )
}

#[allow(clippy::too_many_arguments)]
fn is_codex_client_abort_successish(
    cli_key: &str,
    path: &str,
    status: u16,
    saw_stream_output: bool,
    completion_seen: bool,
    usage_seen: bool,
    terminal_error_seen: bool,
    completion_delivered: bool,
    require_completion_delivered: bool,
) -> bool {
    is_codex_responses_path(cli_key, path)
        && (200..300).contains(&status)
        && saw_stream_output
        && completion_seen
        && usage_seen
        && (!require_completion_delivered || completion_delivered)
        && !terminal_error_seen
}

fn is_codex_drop_successish(
    cli_key: &str,
    path: &str,
    status: u16,
    saw_stream_output: bool,
    completion_seen: bool,
    usage_seen: bool,
    terminal_error_seen: bool,
) -> bool {
    is_codex_responses_path(cli_key, path)
        && (200..300).contains(&status)
        && saw_stream_output
        // If completion/usage is observed, tolerate terminal markers during teardown.
        && (usage_seen || completion_seen || !terminal_error_seen)
}

fn is_codex_stream_terminal_error_successish(
    cli_key: &str,
    path: &str,
    status: u16,
    saw_stream_output: bool,
    completion_seen: bool,
    usage_seen: bool,
) -> bool {
    is_codex_stream_tail_error_successish(
        cli_key,
        path,
        status,
        saw_stream_output,
        completion_seen,
        usage_seen,
    )
}

fn is_codex_stream_tail_error_successish(
    cli_key: &str,
    path: &str,
    status: u16,
    saw_stream_output: bool,
    completion_seen: bool,
    usage_seen: bool,
) -> bool {
    is_codex_responses_path(cli_key, path)
        && (200..300).contains(&status)
        && saw_stream_output
        && (usage_seen || completion_seen)
}

fn is_codex_body_buffer_drop_successish(
    cli_key: &str,
    path: &str,
    status: u16,
    saw_stream_output: bool,
    usage_seen: bool,
) -> bool {
    is_codex_responses_path(cli_key, path)
        && (200..300).contains(&status)
        && saw_stream_output
        && usage_seen
}

fn is_plugin_stream_error_chunk(chunk: &[u8]) -> bool {
    chunk
        .windows(PLUGIN_STREAM_ERROR_MARKER.len())
        .any(|window| window == PLUGIN_STREAM_ERROR_MARKER.as_bytes())
}

/// Does this stream speak the Responses protocol (`event:`-tagged frames dispatched by event
/// type)? **Both** codex and grok do — see `configured_model_route.rs`'s
/// `is_supported_inference_request`, where `"grok" => is_responses_path(..) || .."/chat/completions"`.
/// Framing therefore follows the request path, not the CLI: a grok Responses stream needs the
/// same `event: response.failed` shape as codex, while grok on `/chat/completions` needs the
/// data-only shape. Path set kept identical to [`is_codex_responses_path`] on purpose.
fn is_responses_protocol_stream(cli_key: &str, path: &str) -> bool {
    if !matches!(cli_key, "codex" | "grok") {
        return false;
    }
    matches!(
        path.trim_end_matches('/'),
        "/v1/responses" | "/responses" | "/v1/codex/responses"
    )
}

/// Wrap a rewrite's error payload into an SSE frame the target CLI's stream parser accepts.
///
/// The payload itself comes from `UpstreamErrorResponseRewrite::client_error_payload`, so a
/// stream tail and a pre-commit HTTP envelope always carry the same error object. Only the
/// per-protocol framing differs:
/// - Responses protocol (codex or grok on `/v1/responses`): `event: response.failed` with the
///   error under `response.error`
/// - claude: `event: error`, payload already shaped as the Anthropic error event
/// - `/v1/chat/completions` (codex or grok), gemini: data-only frame (these streams carry no
///   `event:` lines, and adding one risks confusing SDKs)
///
/// Never uses `data: [DONE]`: `proxy::sse::parse_sse_frame` treats it as a non-event, so an
/// error carried that way would be invisible.
fn synthetic_tail_frame(cli_key: &str, path: &str, payload: &serde_json::Value) -> Option<Bytes> {
    if is_responses_protocol_stream(cli_key, path) {
        let error = payload.get("error")?.clone();
        return Some(crate::gateway::proxy::sse::sse_event_frame(
            "response.failed",
            &serde_json::json!({
                "type": "response.failed",
                "response": {
                    "status": "failed",
                    "error": error,
                }
            }),
        ));
    }
    match cli_key {
        "claude" => Some(crate::gateway::proxy::sse::sse_event_frame(
            "error", payload,
        )),
        "codex" | "grok" | "gemini" => Some(crate::gateway::proxy::sse::sse_data_frame(payload)),
        _ => None,
    }
}

fn spawn_touch_activity<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    last_activity_ms: i64,
    details: Option<String>,
) {
    if ctx.observe {
        ctx.active_requests
            .touch(ctx.trace_id.as_str(), last_activity_ms);
    }

    let db = ctx.db.clone();
    let trace_id = ctx.trace_id.clone();
    let cli_key = ctx.cli_key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) =
            crate::request_logs::touch_activity(&db, &trace_id, &cli_key, last_activity_ms, details)
        {
            tracing::warn!(
                trace_id = %trace_id,
                cli = %cli_key,
                error = %err,
                "request log activity touch failed"
            );
        }
    });
}

struct NextFuture<'a, S: Stream + Unpin>(&'a mut S);

impl<'a, S: Stream + Unpin> Future for NextFuture<'a, S> {
    type Output = Option<S::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.0).poll_next(cx)
    }
}

async fn next_item<S: Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    NextFuture(stream).await
}

pub(in crate::gateway) struct UsageSseTeeStream<S, B, R = tauri::Wry>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    upstream: S,
    tracker: usage::SseUsageTracker,
    ctx: StreamFinalizeCtx<R>,
    first_byte_ms: Option<u128>,
    idle_timeout: Option<Duration>,
    idle_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    finalized: bool,
    defer_terminal_error: bool,
    stop_after_terminal_error: bool,
    /// Gateway-authored tail frame awaiting delivery by the relay task.
    ///
    /// Only used when `relay_owns_tail` is set. The relay must send this straight downstream
    /// instead of letting it flow back as a stream item, because the relay pipes items through
    /// `CodexTerminalFirewall`, whose job is policing *upstream* frames — it would classify our
    /// synthesized `response.failed` as a terminal internal error and drop it after commit,
    /// silently defeating the injection in exactly the codex case it exists for.
    pending_tail: Option<Bytes>,
    relay_owns_tail: bool,
    /// Set once a tail frame has been produced: the next poll ends the stream with a clean EOF
    /// rather than the transport `Err`, so downstream sees a normal end of chunked body.
    stop_after_tail: bool,
    completion_override: Option<bool>,
}

impl<S, B, R> UsageSseTeeStream<S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    pub(in crate::gateway) fn new(
        upstream: S,
        ctx: StreamFinalizeCtx<R>,
        idle_timeout: Option<Duration>,
        initial_first_byte_ms: Option<u128>,
    ) -> Self {
        let tracker = usage::SseUsageTracker::new(&ctx.cli_key);
        let tracker = if ctx.detect_stream_internal_errors {
            tracker.with_stream_internal_error_classifier(
                ctx.upstream_retry_policy.stream_internal_errors.enabled,
                &ctx.upstream_retry_policy
                    .stream_internal_errors
                    .passthrough_keywords,
                &ctx.upstream_retry_policy
                    .stream_internal_errors
                    .legacy_retry_keywords,
                false,
            )
        } else {
            tracker
        };
        Self {
            upstream,
            tracker,
            ctx,
            first_byte_ms: initial_first_byte_ms,
            idle_timeout,
            idle_sleep: idle_timeout.map(|d| Box::pin(tokio::time::sleep(d))),
            finalized: false,
            defer_terminal_error: false,
            stop_after_terminal_error: false,
            pending_tail: None,
            relay_owns_tail: false,
            stop_after_tail: false,
            completion_override: None,
        }
    }

    pub(in crate::gateway) fn with_defer_terminal_error(mut self) -> Self {
        self.defer_terminal_error = true;
        self
    }

    /// Hand tail-frame delivery to the relay task (see `pending_tail`).
    pub(in crate::gateway) fn with_relay_owned_tail(mut self) -> Self {
        self.relay_owns_tail = true;
        self
    }

    fn take_pending_tail(&mut self) -> Option<Bytes> {
        self.pending_tail.take()
    }

    /// Match a gateway-synthesized terminal failure against the final-error rewrite rules and,
    /// on a hit, build the protocol-legal tail frame plus its audit record.
    ///
    /// Ordering matters (design §3.4): the audit special setting is pushed here, i.e. before the
    /// caller's `finalize()` serializes special settings into the request log. Returns `None`
    /// whenever nothing should change — no rules configured, no rule matched, or no known frame
    /// shape for this protocol — in which case the caller keeps its current behavior byte for byte.
    fn build_synthetic_tail(&mut self, error_code: GatewayErrorCode) -> Option<Bytes> {
        if self.ctx.upstream_error_response_rules.is_empty() {
            return None;
        }

        let rewrite = upstream_error_response_rules::match_synthetic_failure_rule(
            &self.ctx.upstream_error_response_rules,
            &self.ctx.cli_key,
            self.ctx.provider_id,
            &self.ctx.provider_name,
            error_code,
            // Post-commit headers are already on the wire, so no upstream header can be
            // honored here; an empty map keeps `Retry-After` extraction from inventing one.
            &HeaderMap::new(),
        )?;
        let payload = rewrite.client_error_payload(&self.ctx.cli_key)?;
        let frame = synthetic_tail_frame(&self.ctx.cli_key, &self.ctx.path, &payload)?;

        response_fixer::push_special_setting(
            &self.ctx.special_settings,
            rewrite.special_setting_for_stream_tail(),
        );
        emit_gateway_debug_log(
            &self.ctx.app,
            format!(
                "[SSE] appending gateway error tail frame — trace_id={} cli_key={} path={} error_code={}",
                self.ctx.trace_id,
                self.ctx.cli_key,
                self.ctx.path,
                error_code.as_str(),
            ),
        );
        Some(frame)
    }

    fn poll_next_inner(
        &mut self,
        cx: &mut Context<'_>,
        enforce_idle_timeout: bool,
        finalize_terminal: bool,
    ) -> Poll<Option<Result<B, reqwest::Error>>> {
        if self.stop_after_terminal_error || self.stop_after_tail {
            return Poll::Ready(None);
        }

        let next = Pin::new(&mut self.upstream).poll_next(cx);

        match next {
            Poll::Pending => {
                // Upstream has no data right now; only then check the idle timer.
                // Drain path (enforce_idle_timeout=false) keeps its own deadline.
                if enforce_idle_timeout {
                    if let Some(sleep) = self.idle_sleep.as_mut() {
                        if sleep.as_mut().poll(cx).is_ready() {
                            // This branch ends the stream with a clean EOF and no `Err`, so an
                            // injection hung only off the `Err` arm would never fire for 524
                            // (PRD R1).
                            let tail =
                                self.build_synthetic_tail(GatewayErrorCode::StreamIdleTimeout);
                            self.finalize(
                                Some(GatewayErrorCode::StreamIdleTimeout.as_str()),
                                StreamTerminalEvidence::new(
                                    StreamTerminalOrigin::IdleTimeout,
                                    self.tracker.completion_seen(),
                                    false,
                                    false,
                                    self.tracker.terminal_error_seen(),
                                ),
                            );
                            if let Some(frame) = tail {
                                self.stop_after_tail = true;
                                if self.relay_owns_tail {
                                    self.pending_tail = Some(frame);
                                    return Poll::Ready(None);
                                }
                                return Poll::Ready(Some(Ok(B::from(frame))));
                            }
                            return Poll::Ready(None);
                        }
                    }
                }
                Poll::Pending
            }
            Poll::Ready(None) => {
                // When defer_terminal_error is set and the tracker saw a terminal
                // error, skip finalization here — the relay task will decide the
                // final error_code with Codex-specific tolerance logic.
                if finalize_terminal && !self.defer_terminal_error {
                    self.finalize(
                        self.ctx.error_code,
                        StreamTerminalEvidence::new(
                            StreamTerminalOrigin::NormalEof,
                            self.tracker.completion_seen(),
                            true,
                            false,
                            self.tracker.terminal_error_seen(),
                        ),
                    );
                }
                Poll::Ready(None)
            }
            Poll::Ready(Some(Ok(chunk))) => {
                if self.first_byte_ms.is_none() {
                    self.first_byte_ms = Some(self.ctx.attempt_started.elapsed().as_millis());
                }
                // Reuse existing Box allocation via Sleep::reset() to avoid heap churn per chunk
                if let Some(d) = self.idle_timeout {
                    if let Some(ref mut sleep) = self.idle_sleep {
                        sleep.as_mut().reset(tokio::time::Instant::now() + d);
                    } else {
                        self.idle_sleep = Some(Box::pin(tokio::time::sleep(d)));
                    }
                }
                emit_gateway_debug_log_lazy(&self.ctx.app, || {
                    format!(
                        "[SSE_CHUNK] trace_id={} len={}\n  {}",
                        self.ctx.trace_id,
                        chunk.as_ref().len(),
                        lossy_utf8_preview(chunk.as_ref(), MAX_DEBUG_BODY_PREVIEW_BYTES),
                    )
                });
                let was_terminal_error = self.tracker.terminal_error_seen();
                self.tracker.ingest_chunk(chunk.as_ref());
                if let Ok(mut activity) = self.ctx.activity.lock() {
                    if activity.observe_chunk_at(now_unix_millis().min(i64::MAX as u64) as i64) {
                        spawn_touch_activity(
                            &self.ctx,
                            activity.last_activity_ms(),
                            activity.details_json(None),
                        );
                    }
                }
                if self.tracker.terminal_error_seen() {
                    if !was_terminal_error {
                        emit_gateway_debug_log(
                            &self.ctx.app,
                            format!(
                                "[SSE] terminal_error_seen triggered — trace_id={} cli_key={} path={} fake_200={}",
                                self.ctx.trace_id,
                                self.ctx.cli_key,
                                self.ctx.path,
                                self.tracker.fake_200_detected(),
                            ),
                        );
                    }
                    if !self.defer_terminal_error {
                        let code = if self.tracker.fake_200_detected() {
                            GatewayErrorCode::Fake200.as_str()
                        } else {
                            GatewayErrorCode::StreamError.as_str()
                        };
                        self.finalize(
                            Some(code),
                            StreamTerminalEvidence::new(
                                StreamTerminalOrigin::TerminalFrame,
                                self.tracker.completion_seen(),
                                false,
                                false,
                                true,
                            ),
                        );
                        if is_plugin_stream_error_chunk(chunk.as_ref()) {
                            self.stop_after_terminal_error = true;
                            return Poll::Ready(Some(Ok(chunk)));
                        }
                        return Poll::Ready(None);
                    }
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(err))) => {
                let completion_seen = self.tracker.completion_seen();
                let codex_successish = is_codex_stream_tail_error_successish(
                    &self.ctx.cli_key,
                    &self.ctx.path,
                    self.ctx.status,
                    self.first_byte_ms.is_some(),
                    completion_seen,
                    completion_seen,
                );
                if codex_successish {
                    emit_gateway_debug_log(
                        &self.ctx.app,
                        format!(
                            "[SSE] tolerating late stream read error after completion trace_id={} cli_key={} path={} err={}",
                            self.ctx.trace_id, self.ctx.cli_key, self.ctx.path, err
                        ),
                    );
                    if finalize_terminal {
                        self.finalize(
                            None,
                            StreamTerminalEvidence::new(
                                StreamTerminalOrigin::UpstreamReadError,
                                completion_seen,
                                false,
                                completion_seen,
                                self.tracker.terminal_error_seen(),
                            ),
                        );
                    }
                    Poll::Ready(None)
                } else {
                    // Main injection point: upstream read error mid-stream (GW_STREAM_ERROR).
                    // Shared by every protocol — the codex relay and the claude/gemini/grok
                    // direct paths both reach here through `poll_next`.
                    let tail = if finalize_terminal {
                        self.build_synthetic_tail(GatewayErrorCode::StreamError)
                    } else {
                        // Drain path: the client is already gone, so nothing to inject for.
                        None
                    };
                    if finalize_terminal {
                        self.finalize(
                            Some(GatewayErrorCode::StreamError.as_str()),
                            StreamTerminalEvidence::new(
                                StreamTerminalOrigin::UpstreamReadError,
                                completion_seen,
                                false,
                                false,
                                self.tracker.terminal_error_seen(),
                            ),
                        );
                    }
                    match tail {
                        Some(frame) => {
                            self.stop_after_tail = true;
                            if self.relay_owns_tail {
                                // The relay swaps this `Err` for the tail frame; returning the
                                // frame here instead would route it through the firewall.
                                self.pending_tail = Some(frame);
                                Poll::Ready(Some(Err(err)))
                            } else {
                                Poll::Ready(Some(Ok(B::from(frame))))
                            }
                        }
                        // No rule matched: keep the pre-existing transport error verbatim.
                        None => Poll::Ready(Some(Err(err))),
                    }
                }
            }
        }
    }

    fn finalize(
        &mut self,
        error_code: Option<&'static str>,
        mut terminal_evidence: StreamTerminalEvidence,
    ) {
        if self.finalized {
            return;
        }
        self.finalized = true;

        let usage = self.tracker.finalize();
        let completion_seen = self
            .completion_override
            .unwrap_or_else(|| self.tracker.completion_seen());
        terminal_evidence.completion_seen |= completion_seen;
        terminal_evidence.usage_seen |= usage.is_some();
        terminal_evidence.terminal_error_seen |= self.tracker.terminal_error_seen();
        let terminal_signal = if error_code.is_some() {
            Some("error")
        } else if completion_seen {
            Some("completed")
        } else {
            None
        };
        if let Ok(activity) = self.ctx.activity.lock() {
            spawn_touch_activity(
                &self.ctx,
                activity.last_activity_ms(),
                // Post-commit stream failures are ~93% of real stream faults; without the origin
                // and terminal evidence here their activity record cannot say *how* the stream
                // ended (PRD R10). `terminal_details_json` only adds fields — `terminal_signal`
                // keeps its existing meaning and name.
                activity.terminal_details_json(terminal_signal, terminal_evidence),
            );
        }

        // Propagate fake 200 detection from tracker to finalize context.
        if self.tracker.fake_200_detected() {
            self.ctx.fake_200_detected = true;
        }
        let effective_error_code = if error_code.is_none()
            && self
                .tracker
                .is_empty_success(&self.ctx.path, self.ctx.status, usage.as_ref())
        {
            Some(GatewayErrorCode::EmptyResponse.as_str())
        } else {
            error_code
        };
        let (route_evidence, upstream_usage) = finalize_upstream_route_observation(
            &self.ctx.upstream_route_tracker,
            &self.ctx.observed_upstream_model,
            &self.ctx.observed_upstream_conflicting_model,
            &self.ctx.observed_upstream_reasoning_effort,
        );
        let usage_metrics = if self.ctx.use_upstream_usage_metrics {
            upstream_usage.as_ref().or(usage.as_ref())
        } else {
            usage.as_ref()
        }
        .map(|usage| usage.metrics.clone());
        if let Some(setting) = model_route_mapping::build_model_route_mapping_setting_from_shared(
            model_route_mapping::SharedModelRouteSettingInput {
                cli_key: &self.ctx.cli_key,
                requested_model: self.ctx.requested_upstream_model.as_deref(),
                actual_model: route_evidence.first_model.as_deref(),
                conflicting_actual_model: route_evidence.first_conflicting_model.as_deref(),
                actual_reasoning_effort: route_evidence.reasoning_effort.as_deref(),
                special_settings: &self.ctx.special_settings,
                provider_id: self.ctx.provider_id,
                provider_name: &self.ctx.provider_name,
            },
        ) {
            response_fixer::push_model_route_mapping_special_setting(
                &self.ctx.special_settings,
                setting,
            );
        }
        let requested_model = self
            .ctx
            .requested_model
            .clone()
            .or(route_evidence.first_model);

        emit_request_event_and_spawn_request_log(
            &self.ctx,
            StreamRequestCompletion::from_error_code(
                effective_error_code,
                self.first_byte_ms,
                self.first_byte_ms,
                requested_model,
                usage_metrics,
                usage,
            )
            .with_terminal_signal(terminal_signal)
            .with_terminal_evidence(terminal_evidence)
            .with_stream_internal_error(self.tracker.stream_internal_error_evidence().cloned()),
        );
    }
}

impl<S, B, R> Stream for UsageSseTeeStream<S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    type Item = Result<B, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        this.poll_next_inner(cx, true, true)
    }
}

struct DrainNextFuture<'a, S, B, R>(&'a mut UsageSseTeeStream<S, B, R>)
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin;

impl<'a, S, B, R> Future for DrainNextFuture<'a, S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    type Output = Option<Result<B, reqwest::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_next_inner(cx, false, false)
    }
}

async fn next_drain_item<S, B, R>(
    stream: &mut UsageSseTeeStream<S, B, R>,
) -> Option<Result<B, reqwest::Error>>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    DrainNextFuture(stream).await
}

impl<S, B, R> Drop for UsageSseTeeStream<S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]> + From<Bytes>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    fn drop(&mut self) {
        if !self.finalized {
            // Best-effort flush for trailing partial SSE data before deciding abort/success.
            let usage = self.tracker.finalize();
            let usage_seen = usage.is_some();
            let completion_seen = self
                .completion_override
                .unwrap_or_else(|| self.tracker.completion_seen());
            let terminal_error_seen = self.tracker.terminal_error_seen();

            let codex_successish = is_codex_drop_successish(
                &self.ctx.cli_key,
                &self.ctx.path,
                self.ctx.status,
                self.first_byte_ms.is_some(),
                completion_seen,
                usage_seen,
                terminal_error_seen,
            );

            if codex_successish {
                self.finalize(
                    None,
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::DirectDrop,
                        completion_seen,
                        false,
                        usage_seen,
                        terminal_error_seen,
                    ),
                );
            } else {
                self.finalize(
                    Some(GatewayErrorCode::StreamAborted.as_str()),
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::DirectDrop,
                        completion_seen,
                        false,
                        usage_seen,
                        terminal_error_seen,
                    ),
                );
            }
        }
    }
}

const SSE_RELAY_BUFFER_CAPACITY: usize = 32;

struct DownstreamRelayItem {
    item: Result<Bytes, reqwest::Error>,
    completion_seen: bool,
}

struct DownstreamRelayBodyStream {
    rx: tokio::sync::mpsc::Receiver<DownstreamRelayItem>,
    completion_delivered: Arc<AtomicBool>,
}

impl DownstreamRelayBodyStream {
    fn new(
        rx: tokio::sync::mpsc::Receiver<DownstreamRelayItem>,
        completion_delivered: Arc<AtomicBool>,
    ) -> Self {
        Self {
            rx,
            completion_delivered,
        }
    }
}

impl Stream for DownstreamRelayBodyStream {
    type Item = Result<Bytes, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.rx).poll_recv(cx) {
            Poll::Ready(Some(item)) => {
                if item.completion_seen && item.item.is_ok() {
                    self.completion_delivered.store(true, Ordering::Release);
                }
                Poll::Ready(Some(item.item))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(in crate::gateway) fn spawn_usage_sse_relay_body<S, R>(
    upstream: S,
    ctx: StreamFinalizeCtx<R>,
    idle_timeout: Option<Duration>,
    initial_first_byte_ms: Option<u128>,
) -> Body
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin + Send + 'static,
    R: tauri::Runtime + 'static,
    R::Handle: Unpin,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<DownstreamRelayItem>(SSE_RELAY_BUFFER_CAPACITY);
    let completion_delivered = Arc::new(AtomicBool::new(false));
    let body_completion_delivered = Arc::clone(&completion_delivered);

    let mut tee = UsageSseTeeStream::new(upstream, ctx, idle_timeout, initial_first_byte_ms)
        .with_defer_terminal_error()
        .with_relay_owned_tail();

    tokio::spawn(async move {
        let mut forwarded_chunks: i64 = 0;
        let mut forwarded_bytes: i64 = 0;
        let mut drained_chunks: i64 = 0;
        let mut drained_bytes: i64 = 0;
        let mut client_abort_detected_by: Option<&'static str> = None;
        let mut downstream_closed = false;
        let mut upstream_ended_normally = false;
        let mut relay_drain_timed_out = false;

        let is_codex_responses = is_codex_responses_path(&tee.ctx.cli_key, &tee.ctx.path);
        let firewall_enabled =
            is_codex_responses && tee.ctx.upstream_retry_policy.stream_internal_errors.enabled;
        let mut terminal_firewall = firewall_enabled.then(|| {
            CodexTerminalFirewall::new(
                &tee.ctx
                    .upstream_retry_policy
                    .stream_internal_errors
                    .passthrough_keywords,
            )
        });
        let mut visible_completion_seen = false;
        let mut firewall_terminal_error = false;
        let mut firewall_stopped_normally = false;
        let mut drain_deadline: Option<tokio::time::Instant> = None;
        let drain_grace = {
            // Codex ChatGPT backend: response.completed (carrying usage) may arrive
            // several seconds after the last output chunk; use a longer drain window.
            let cap = if is_codex_responses {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(5)
            };
            let floor = Duration::from_millis(500);
            match idle_timeout {
                Some(d) if d < floor => floor,
                Some(d) if d > cap => cap,
                Some(d) => d,
                None => {
                    if is_codex_responses {
                        Duration::from_secs(10)
                    } else {
                        Duration::from_secs(2)
                    }
                }
            }
        };

        loop {
            if downstream_closed {
                if !is_codex_responses {
                    break;
                }
                // Keep draining until completion/deadline/end-of-stream.
                // Some Codex backend flows can emit transient error-like markers before
                // `response.completed` (with usage) arrives.
                if tee.tracker.completion_seen() {
                    break;
                }
                let Some(deadline) = drain_deadline else {
                    break;
                };
                let now = tokio::time::Instant::now();
                if now >= deadline {
                    relay_drain_timed_out = true;
                    break;
                }

                let remaining = deadline.saturating_duration_since(now);
                match tokio::time::timeout(remaining, next_drain_item(&mut tee)).await {
                    Ok(Some(Ok(chunk))) => {
                        let chunk_len = chunk.len().min(i64::MAX as usize) as i64;
                        drained_chunks = drained_chunks.saturating_add(1);
                        drained_bytes = drained_bytes.saturating_add(chunk_len);
                    }
                    Ok(Some(Err(_))) => {
                        break;
                    }
                    Ok(None) => {
                        upstream_ended_normally = true;
                        break;
                    }
                    Err(_) => {
                        relay_drain_timed_out = true;
                        break;
                    }
                }
                continue;
            }

            tokio::select! {
                biased;
                // 如果客户端提前断开，但上游短时间没有新 chunk，就会卡在 next_item().await。
                // 这里通过监听 rx 端被 drop 来更早感知断开，避免误记 GW_STREAM_ABORTED。
                // 如果断开和 idle timeout 同时 ready，断开应优先进入 Codex drain。
                _ = tx.closed() => {
                    client_abort_detected_by = Some("rx_closed");
                    downstream_closed = true;
                    if is_codex_responses {
                        drain_deadline = Some(tokio::time::Instant::now() + drain_grace);
                        continue;
                    }
                    break;
                }
                item = next_item(&mut tee) => {
                    let Some(item) = item else {
                        upstream_ended_normally = true;
                        if let Some(firewall) = terminal_firewall.as_mut() {
                            let output = firewall.finish();
                            visible_completion_seen |= output.completion_seen;
                            if let Some(reason) = output.fail_closed_reason {
                                firewall_terminal_error = true;
                                response_fixer::push_special_setting(
                                    &tee.ctx.special_settings,
                                    serde_json::json!({
                                        "type": "stream_terminal_firewall",
                                        "disposition": "dropped_after_commit",
                                        "reason": reason,
                                    }),
                                );
                            }
                        }
                        // Idle timeout ends the stream with a clean EOF, so a matched rule's tail
                        // frame arrives here. Sent after the firewall has finished and without
                        // passing through it — the frame is gateway-authored, not upstream bytes.
                        if let Some(frame) = tee.take_pending_tail() {
                            let _ = tx
                                .send(DownstreamRelayItem {
                                    item: Ok(frame),
                                    completion_seen: visible_completion_seen,
                                })
                                .await;
                        }
                        break;
                    };

                    match item {
                        Ok(mut chunk) => {
                            let mut completion_seen = tee.tracker.completion_seen();
                            let mut stop_after_chunk = false;
                            if let Some(firewall) = terminal_firewall.as_mut() {
                                let output = firewall.ingest(chunk.as_ref());
                                visible_completion_seen |= output.completion_seen;
                                completion_seen = visible_completion_seen;
                                stop_after_chunk = output.stop;
                                let terminal_error = output.terminal_error;
                                firewall_terminal_error |= terminal_error;
                                firewall_stopped_normally |= output.stop && !terminal_error;

                                if let Some(evidence) = output.evidence.as_ref() {
                                    tee.tracker.set_stream_internal_error_disposition(
                                        evidence.disposition.as_str(),
                                    );
                                    response_fixer::push_special_setting(
                                        &tee.ctx.special_settings,
                                        serde_json::json!({
                                            "type": "stream_terminal_firewall",
                                            "disposition": evidence.disposition,
                                            "classification": evidence.classification,
                                        }),
                                    );
                                } else if let Some(reason) = output.fail_closed_reason {
                                    response_fixer::push_special_setting(
                                        &tee.ctx.special_settings,
                                        serde_json::json!({
                                            "type": "stream_terminal_firewall",
                                            "disposition": "dropped_after_commit",
                                            "reason": reason,
                                        }),
                                    );
                                }
                                chunk = output.bytes;
                            }

                            if chunk.is_empty() {
                                if stop_after_chunk {
                                    break;
                                }
                                continue;
                            }
                            let chunk_len = chunk.len().min(i64::MAX as usize) as i64;

                            if tx
                                .send(DownstreamRelayItem {
                                    item: Ok(chunk),
                                    completion_seen,
                                })
                                .await
                                .is_err()
                            {
                                client_abort_detected_by = Some("send_failed");
                                downstream_closed = true;
                                if is_codex_responses {
                                    drain_deadline = Some(tokio::time::Instant::now() + drain_grace);
                                    continue;
                                }
                                break;
                            }

                            forwarded_chunks = forwarded_chunks.saturating_add(1);
                            forwarded_bytes = forwarded_bytes.saturating_add(chunk_len);
                            if stop_after_chunk {
                                break;
                            }
                        }
                        Err(err) => {
                            // A mid-frame truncation — the common real-world shape — leaves a
                            // partial frame in the firewall, so `finish()` fails closed here far
                            // more often than not. The tail frame must still go out: the firewall
                            // dropped incomplete *upstream* bytes, which is exactly when the
                            // client most needs to be told the stream failed.
                            let mut firewall_dropped_tail_bytes = false;
                            if let Some(firewall) = terminal_firewall.as_mut() {
                                let output = firewall.finish();
                                visible_completion_seen |= output.completion_seen;
                                if let Some(reason) = output.fail_closed_reason {
                                    firewall_terminal_error = true;
                                    response_fixer::push_special_setting(
                                        &tee.ctx.special_settings,
                                        serde_json::json!({
                                            "type": "stream_terminal_firewall",
                                            "disposition": "dropped_after_commit",
                                            "reason": reason,
                                        }),
                                    );
                                    firewall_dropped_tail_bytes = true;
                                }
                            }
                            // A matched rule replaces the raw transport error with a
                            // protocol-legal error frame plus a clean end of body, so the client
                            // sees the operator's message instead of a truncated stream.
                            match tee.take_pending_tail() {
                                Some(frame) => {
                                    let _ = tx
                                        .send(DownstreamRelayItem {
                                            item: Ok(frame),
                                            completion_seen: visible_completion_seen,
                                        })
                                        .await;
                                }
                                // No rule matched: keep the pre-existing behavior exactly — the
                                // transport error is forwarded, except when the firewall failed
                                // closed, where it already decided to end the body silently.
                                None if !firewall_dropped_tail_bytes => {
                                    // 尽力把流错误透传给客户端
                                    let _ = tx
                                        .send(DownstreamRelayItem {
                                            item: Err(err),
                                            completion_seen: false,
                                        })
                                        .await;
                                }
                                None => {}
                            }
                            break;
                        }
                    }
                }
            }
        }

        // When the stream ended normally (no client abort) but the tracker
        // detected a terminal-error-like SSE event, apply Codex-specific
        // tolerance: if we already saw output AND completion/usage, treat the
        // request as successful instead of marking it GW_STREAM_ERROR.
        let terminal_error_seen = firewall_terminal_error || tee.tracker.terminal_error_seen();
        if client_abort_detected_by.is_none() && terminal_error_seen {
            let completion_seen = if firewall_enabled {
                visible_completion_seen
            } else {
                tee.tracker.completion_seen()
            };
            if firewall_enabled {
                tee.completion_override = Some(completion_seen);
            }
            let saw_stream_output = tee.first_byte_ms.is_some()
                || forwarded_chunks > 0
                || forwarded_bytes > 0
                || drained_chunks > 0
                || drained_bytes > 0;

            // For Codex /v1/responses, completion_seen implies response.completed
            // was received (which carries usage). We treat this as a proxy for
            // usage_seen to avoid consuming tracker state before tee.finalize().
            let codex_successish = is_codex_stream_terminal_error_successish(
                &tee.ctx.cli_key,
                &tee.ctx.path,
                tee.ctx.status,
                saw_stream_output,
                completion_seen,
                completion_seen,
            );

            if codex_successish {
                tee.finalize(
                    None,
                    StreamTerminalEvidence::new(
                        if upstream_ended_normally {
                            StreamTerminalOrigin::NormalEof
                        } else {
                            StreamTerminalOrigin::TerminalFrame
                        },
                        completion_seen,
                        upstream_ended_normally,
                        completion_seen,
                        true,
                    ),
                );
            } else {
                let code = if tee.tracker.fake_200_detected() {
                    GatewayErrorCode::Fake200.as_str()
                } else {
                    GatewayErrorCode::StreamError.as_str()
                };
                tee.finalize(
                    Some(code),
                    StreamTerminalEvidence::new(
                        if upstream_ended_normally {
                            StreamTerminalOrigin::NormalEof
                        } else {
                            StreamTerminalOrigin::TerminalFrame
                        },
                        completion_seen,
                        upstream_ended_normally,
                        completion_seen,
                        true,
                    ),
                );
            }
        }

        if client_abort_detected_by.is_none()
            && !tee.finalized
            && !terminal_error_seen
            && (upstream_ended_normally || firewall_stopped_normally)
        {
            let completion_seen = if firewall_enabled {
                visible_completion_seen
            } else {
                tee.tracker.completion_seen()
            };
            if firewall_enabled {
                tee.completion_override = Some(completion_seen);
            }
            let (origin, normal_eof) = if upstream_ended_normally {
                (StreamTerminalOrigin::NormalEof, true)
            } else {
                (StreamTerminalOrigin::ProtocolTerminal, false)
            };
            tee.finalize(
                tee.ctx.error_code,
                StreamTerminalEvidence::new(
                    origin,
                    completion_seen,
                    normal_eof,
                    completion_seen,
                    false,
                ),
            );
        }

        if let Some(detected_by) = client_abort_detected_by {
            let duration_ms = tee.ctx.started.elapsed().as_millis().min(i64::MAX as u128) as i64;
            let ttfb_ms = tee.first_byte_ms.and_then(|v| {
                if v >= duration_ms as u128 {
                    return None;
                }
                Some(v.min(i64::MAX as u128) as i64)
            });
            // Flush pending partial SSE data before deciding abort/success.
            let usage = tee.tracker.finalize();
            let usage_seen = usage.is_some();
            let completion_seen = tee.tracker.completion_seen();
            let terminal_error_seen = tee.tracker.terminal_error_seen();
            let completion_delivered = completion_delivered.load(Ordering::Acquire);
            let require_completion_delivered = tee
                .ctx
                .dispatch_ownership
                .as_ref()
                .is_some_and(|ownership| ownership.is_probe());
            let saw_stream_output = tee.first_byte_ms.is_some()
                || forwarded_chunks > 0
                || forwarded_bytes > 0
                || drained_chunks > 0
                || drained_bytes > 0;

            let codex_successish = is_codex_client_abort_successish(
                &tee.ctx.cli_key,
                &tee.ctx.path,
                tee.ctx.status,
                saw_stream_output,
                completion_seen,
                usage_seen,
                terminal_error_seen,
                completion_delivered,
                require_completion_delivered,
            );
            if codex_successish {
                tee.finalize(
                    None,
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::CompletionDelivered,
                        completion_seen,
                        upstream_ended_normally,
                        usage_seen,
                        terminal_error_seen,
                    ),
                );
            } else {
                response_fixer::push_special_setting(
                    &tee.ctx.special_settings,
                    serde_json::json!({
                        "type": "client_abort",
                        "scope": "stream",
                        "reason": "client_disconnected",
                        "detected_by": detected_by,
                        "duration_ms": duration_ms,
                        "ttfb_ms": ttfb_ms,
                        "forwarded_chunks": forwarded_chunks,
                        "forwarded_bytes": forwarded_bytes,
                        "drained_chunks": drained_chunks,
                        "drained_bytes": drained_bytes,
                        "upstream_ended_normally": upstream_ended_normally,
                        "completion_seen": completion_seen,
                        "completion_delivered": completion_delivered,
                        "terminal_error_seen": terminal_error_seen,
                        "saw_stream_output": saw_stream_output,
                        "ts": now_unix_seconds() as i64,
                    }),
                );
                tee.finalize(
                    Some(GatewayErrorCode::StreamAborted.as_str()),
                    StreamTerminalEvidence::new(
                        if relay_drain_timed_out {
                            StreamTerminalOrigin::RelayDrainTimeout
                        } else {
                            StreamTerminalOrigin::ClientAbort
                        },
                        completion_seen,
                        upstream_ended_normally,
                        usage_seen,
                        terminal_error_seen,
                    ),
                );
            }
        }
    });

    Body::from_stream(DownstreamRelayBodyStream::new(
        rx,
        body_completion_delivered,
    ))
}

pub(in crate::gateway) struct UsageBodyBufferTeeStream<S, B, R = tauri::Wry>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    upstream: S,
    ctx: StreamFinalizeCtx<R>,
    first_byte_ms: Option<u128>,
    buffer: Vec<u8>,
    max_bytes: usize,
    truncated: bool,
    total_timeout: Option<Duration>,
    total_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    finalized: bool,
}

impl<S, B, R> UsageBodyBufferTeeStream<S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    pub(in crate::gateway) fn new(
        upstream: S,
        ctx: StreamFinalizeCtx<R>,
        max_bytes: usize,
        total_timeout: Option<Duration>,
    ) -> Self {
        let remaining = total_timeout.and_then(|d| d.checked_sub(ctx.started.elapsed()));
        Self {
            upstream,
            ctx,
            first_byte_ms: None,
            buffer: Vec::new(),
            max_bytes,
            truncated: false,
            total_timeout,
            total_sleep: remaining.map(|d| Box::pin(tokio::time::sleep(d))),
            finalized: false,
        }
    }

    fn finalize(
        &mut self,
        error_code: Option<&'static str>,
        mut terminal_evidence: StreamTerminalEvidence,
    ) {
        if self.finalized {
            return;
        }
        self.finalized = true;

        let effective_error_code = if error_code.is_none()
            && !self.truncated
            && !self.buffer.is_empty()
            && is_fake_200_non_stream_body(&self.buffer)
        {
            Some(GatewayErrorCode::Fake200.as_str())
        } else {
            error_code
        };
        if effective_error_code == Some(GatewayErrorCode::Fake200.as_str()) {
            self.ctx.fake_200_detected = true;
            self.ctx.fake_200_quota_exhausted =
                upstream_client_error_rules::match_quota_exhausted(&self.buffer);
        }

        let usage = if self.truncated || self.buffer.is_empty() {
            None
        } else {
            usage::parse_usage_from_json_or_sse_bytes(&self.ctx.cli_key, &self.buffer)
        };
        terminal_evidence.usage_seen |= usage.is_some();
        let usage_metrics = usage.as_ref().map(|u| u.metrics.clone());
        let route_evidence = if self.truncated || self.buffer.is_empty() {
            usage::ModelRouteEvidence::default()
        } else {
            usage::parse_model_route_evidence_from_json_or_sse_bytes(
                &self.ctx.cli_key,
                &self.buffer,
            )
        };
        if let Some(setting) = model_route_mapping::build_model_route_mapping_setting_from_shared(
            model_route_mapping::SharedModelRouteSettingInput {
                cli_key: &self.ctx.cli_key,
                requested_model: self.ctx.requested_upstream_model.as_deref(),
                actual_model: route_evidence.first_model.as_deref(),
                conflicting_actual_model: route_evidence.first_conflicting_model.as_deref(),
                actual_reasoning_effort: route_evidence.reasoning_effort.as_deref(),
                special_settings: &self.ctx.special_settings,
                provider_id: self.ctx.provider_id,
                provider_name: &self.ctx.provider_name,
            },
        ) {
            response_fixer::push_model_route_mapping_special_setting(
                &self.ctx.special_settings,
                setting,
            );
        }
        let requested_model = self.ctx.requested_model.clone().or_else(|| {
            if self.truncated || self.buffer.is_empty() {
                None
            } else {
                route_evidence.first_model.clone()
            }
        });

        emit_request_event_and_spawn_request_log(
            &self.ctx,
            StreamRequestCompletion::from_error_code(
                effective_error_code,
                self.first_byte_ms,
                self.first_byte_ms,
                requested_model,
                usage_metrics,
                usage,
            )
            .with_terminal_evidence(terminal_evidence),
        );
    }
}

impl<S, B, R> Stream for UsageBodyBufferTeeStream<S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    type Item = Result<B, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        if let Some(total) = this.total_timeout {
            if this.ctx.started.elapsed() >= total {
                this.finalize(
                    Some(GatewayErrorCode::UpstreamTimeout.as_str()),
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::TotalTimeout,
                        false,
                        false,
                        false,
                        false,
                    ),
                );
                return Poll::Ready(None);
            }
        }

        let next = Pin::new(&mut this.upstream).poll_next(cx);

        match next {
            Poll::Pending => {
                if let Some(timer) = this.total_sleep.as_mut() {
                    if timer.as_mut().poll(cx).is_ready() {
                        this.finalize(
                            Some(GatewayErrorCode::UpstreamTimeout.as_str()),
                            StreamTerminalEvidence::new(
                                StreamTerminalOrigin::TotalTimeout,
                                false,
                                false,
                                false,
                                false,
                            ),
                        );
                        return Poll::Ready(None);
                    }
                }
                Poll::Pending
            }
            Poll::Ready(None) => {
                this.finalize(
                    this.ctx.error_code,
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::BufferedBodyEof,
                        false,
                        true,
                        false,
                        false,
                    ),
                );
                Poll::Ready(None)
            }
            Poll::Ready(Some(Ok(chunk))) => {
                if this.first_byte_ms.is_none() {
                    this.first_byte_ms = Some(this.ctx.attempt_started.elapsed().as_millis());
                }
                if !this.truncated {
                    let bytes = chunk.as_ref();
                    if this.buffer.len().saturating_add(bytes.len()) <= this.max_bytes {
                        this.buffer.extend_from_slice(bytes);
                    } else {
                        this.truncated = true;
                        this.buffer.clear();
                    }
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(err))) => {
                this.finalize(
                    Some(GatewayErrorCode::StreamError.as_str()),
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::UpstreamReadError,
                        false,
                        false,
                        false,
                        false,
                    ),
                );
                Poll::Ready(Some(Err(err)))
            }
        }
    }
}

impl<S, B, R> Drop for UsageBodyBufferTeeStream<S, B, R>
where
    S: Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    fn drop(&mut self) {
        if !self.finalized {
            let usage_seen = !self.truncated
                && !self.buffer.is_empty()
                && usage::parse_usage_from_json_or_sse_bytes(&self.ctx.cli_key, &self.buffer)
                    .is_some();

            let codex_successish = is_codex_body_buffer_drop_successish(
                &self.ctx.cli_key,
                &self.ctx.path,
                self.ctx.status,
                self.first_byte_ms.is_some(),
                usage_seen,
            );

            if codex_successish {
                self.finalize(
                    None,
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::DirectDrop,
                        false,
                        false,
                        usage_seen,
                        false,
                    ),
                );
            } else {
                self.finalize(
                    Some(GatewayErrorCode::StreamAborted.as_str()),
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::DirectDrop,
                        false,
                        false,
                        usage_seen,
                        false,
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_codex_body_buffer_drop_successish, is_codex_client_abort_successish,
        is_codex_drop_successish, is_codex_responses_path, is_codex_stream_tail_error_successish,
        is_codex_stream_terminal_error_successish, is_plugin_stream_error_chunk, next_item,
        spawn_touch_activity, spawn_usage_sse_relay_body, RelayBodyStream, StreamFinalizeCtx,
        UpstreamModelObserverStream, UsageSseTeeStream,
    };
    use crate::gateway::active_requests::{ActiveRequestRegistry, ActiveRequestStart};
    use crate::gateway::events::FailoverAttempt;
    use crate::gateway::proxy::dispatch::RequestDispatchIntent;
    use crate::gateway::proxy::GatewayErrorCode;
    use crate::gateway::streams::StreamActivityTracker;
    use crate::{circuit_breaker, db, request_logs, session_manager, usage};
    use axum::body::Bytes;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn test_stream_finalize_ctx(
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
        active_requests: Arc<ActiveRequestRegistry>,
    ) -> StreamFinalizeCtx<tauri::test::MockRuntime> {
        let session = Arc::new(session_manager::SessionManager::new());
        let session_binding_request = session.begin_binding_request();
        StreamFinalizeCtx {
            app,
            db,
            log_tx,
            plugin_pipeline: crate::gateway::plugins::pipeline::GatewayPluginPipeline::empty_shared(
            ),
            circuit: Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig::default(),
                HashMap::new(),
                None,
            )),
            dispatch_ownership: None,
            session,
            session_id: Some("sess-usage-tee-drain".to_string()),
            session_binding_request,
            sort_mode_id: None,
            is_compact_request: false,
            trace_id: "trace-usage-tee-drain".to_string(),
            cli_key: "codex".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            observe: true,
            query: None,
            excluded_from_stats: false,
            special_settings: Arc::new(Mutex::new(Vec::new())),
            provider_health_neutral: false,
            status: 200,
            error_category: None,
            error_code: None,
            started: Instant::now(),
            attempt_started: Instant::now(),
            attempts: Vec::new(),
            attempts_json: "[]".to_string(),
            requested_model: None,
            requested_upstream_model: None,
            managed_model_route: false,
            created_at_ms: 1_700_000_000_000,
            created_at: 1_700_000_000,
            provider_cooldown_secs: 0,
            upstream_first_byte_timeout_secs: 300,
            upstream_retry_policy: crate::settings::UpstreamRetryPolicy::default(),
            upstream_error_response_rules: Vec::new(),
            detect_stream_internal_errors: true,
            provider_id: 1,
            provider_name: "test-provider".to_string(),
            base_url: "https://upstream.example".to_string(),
            auth_mode: "api_key".to_string(),
            use_upstream_usage_metrics: false,
            upstream_route_tracker: Arc::new(Mutex::new(usage::SseUsageTracker::new("codex"))),
            observed_upstream_model: Arc::new(Mutex::new(None)),
            observed_upstream_conflicting_model: Arc::new(Mutex::new(None)),
            observed_upstream_reasoning_effort: Arc::new(Mutex::new(None)),
            fake_200_detected: false,
            fake_200_quota_exhausted: false,
            activity: Arc::new(Mutex::new(StreamActivityTracker::new(
                "trace-usage-tee-drain",
                "codex",
                1_700_000_000_000,
            ))),
            active_requests,
        }
    }

    fn active_request_start(trace_id: &str) -> ActiveRequestStart {
        ActiveRequestStart {
            trace_id: trace_id.to_string(),
            cli_key: "codex".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            query: None,
            session_id: Some("sess-usage-tee-drain".to_string()),
            requested_model: Some("gpt-5".to_string()),
            created_at_ms: 1_700_000_000_000,
            codex_infinite_retry_test: false,
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
            reasoning_effort: None,
            upstream_sent: true,
        }
    }

    /// Rule that matches purely on the gateway-synthesized status code and overrides the text,
    /// mirroring what an operator configures in the UI for "intercept 502 / 524".
    fn synthetic_status_rule(status_code: u16) -> crate::settings::UpstreamErrorResponseRule {
        crate::settings::UpstreamErrorResponseRule {
            id: "6d1f0a52-2c74-4a1b-9f6e-1b0d3c8a5e77".to_string(),
            name: "stream failure".to_string(),
            description: String::new(),
            enabled: true,
            priority: 10,
            status_codes: vec![status_code],
            keywords: Vec::new(),
            match_mode: crate::settings::UpstreamErrorResponseMatchMode::All,
            cli_keys: Vec::new(),
            provider_ids: Vec::new(),
            status_behavior: crate::settings::UpstreamErrorStatusBehavior::Override {
                status_code: 503,
            },
            message_behavior: crate::settings::UpstreamErrorMessageBehavior::Override {
                message: "上游流式响应中断，请重试".to_string(),
            },
        }
    }

    #[test]
    fn synthetic_tail_frame_uses_each_protocols_own_stream_shape() {
        let payload = serde_json::json!({
            "error": { "type": "upstream_error", "code": "upstream_error", "message": "boom" }
        });

        // codex /v1/responses: response.failed carrying the error under `response.error`.
        let codex = super::synthetic_tail_frame("codex", "/v1/responses", &payload)
            .expect("codex responses frame");
        let codex = String::from_utf8(codex.to_vec()).expect("utf8");
        assert!(codex.starts_with("event: response.failed\ndata: "));
        assert!(codex.ends_with("\n\n"));
        let codex_data: serde_json::Value = serde_json::from_str(
            codex
                .trim_end()
                .strip_prefix("event: response.failed\ndata: ")
                .expect("codex data line"),
        )
        .expect("codex json");
        assert_eq!(codex_data["type"], "response.failed");
        assert_eq!(codex_data["response"]["status"], "failed");
        assert_eq!(codex_data["response"]["error"]["message"], "boom");

        // codex on an OpenAI-compatible path has no `event:` lines: data-only frame.
        let codex_chat = super::synthetic_tail_frame("codex", "/v1/chat/completions", &payload)
            .expect("codex chat frame");
        let codex_chat = String::from_utf8(codex_chat.to_vec()).expect("utf8");
        assert!(codex_chat.starts_with("data: "));
        assert!(!codex_chat.contains("event:"));

        // grok also speaks the Responses protocol (`configured_model_route.rs`'s
        // `is_supported_inference_request`), and its Responses SSE really does carry `event:`
        // lines — see the `mock_runtime_router_grok_responses_sse_is_transparent_and_logged`
        // fixture. Framing must follow the path, not the CLI, or a grok client gets a bare
        // `data:` blob with no dispatchable event type and the truncation stays silent.
        let grok_responses = super::synthetic_tail_frame("grok", "/v1/responses", &payload)
            .expect("grok responses frame");
        let grok_responses = String::from_utf8(grok_responses.to_vec()).expect("utf8");
        assert!(grok_responses.starts_with("event: response.failed\ndata: "));
        assert!(grok_responses.contains("\"status\":\"failed\""));

        // claude: `event: error`, payload already shaped as the Anthropic error event.
        let claude_payload = serde_json::json!({
            "type": "error",
            "error": { "type": "upstream_error", "message": "boom" }
        });
        let claude = super::synthetic_tail_frame("claude", "/v1/messages", &claude_payload)
            .expect("claude frame");
        let claude = String::from_utf8(claude.to_vec()).expect("utf8");
        assert!(claude.starts_with("event: error\ndata: "));

        for cli_key in ["gemini", "grok"] {
            let frame = super::synthetic_tail_frame(cli_key, "/v1/chat/completions", &payload)
                .expect("data-only frame");
            let frame = String::from_utf8(frame.to_vec()).expect("utf8");
            assert!(frame.starts_with("data: "), "{cli_key} must be data-only");
            assert!(
                !frame.contains("event:"),
                "{cli_key} must have no event line"
            );
        }

        // Never `data: [DONE]`: proxy::sse::parse_sse_frame treats it as a non-event, so an
        // error carried that way would be invisible to every client.
        for cli_key in ["codex", "claude", "gemini", "grok"] {
            let frame = super::synthetic_tail_frame(cli_key, "/v1/responses", &payload)
                .or_else(|| super::synthetic_tail_frame(cli_key, "/v1/responses", &claude_payload))
                .expect("frame");
            assert!(!String::from_utf8_lossy(frame.as_ref()).contains("[DONE]"));
        }

        assert!(
            super::synthetic_tail_frame("unknown-cli", "/v1/chat/completions", &payload).is_none()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn idle_timeout_appends_rule_frame_then_ends_the_body_cleanly() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-idle-tail-inject.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        // 524 is the status the gateway synthesizes for GW_STREAM_IDLE_TIMEOUT.
        ctx.upstream_error_response_rules = vec![synthetic_status_rule(524)];
        let special_settings = Arc::clone(&ctx.special_settings);
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(300)),
            None,
        );
        let mut body_stream = body.into_data_stream();

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send output chunk");
        let first = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("output chunk should arrive")
            .expect("body should yield output chunk")
            .expect("output chunk should be ok");
        assert!(first.as_ref().starts_with(b"data:"));

        // Upstream goes silent; the idle timeout fires and the matched rule's frame is appended.
        let tail = tokio::time::timeout(Duration::from_secs(2), next_item(&mut body_stream))
            .await
            .expect("idle timeout should produce the tail frame")
            .expect("body should yield the tail frame")
            .expect("tail frame should be ok");
        let tail = String::from_utf8(tail.to_vec()).expect("utf8 tail frame");
        assert!(tail.starts_with("event: response.failed\ndata: "));
        assert!(tail.contains("上游流式响应中断，请重试"));
        // The gateway's internal code must never reach the client.
        assert!(!tail.contains("GW_STREAM_IDLE_TIMEOUT"));

        // Then a clean EOF, never a transport error.
        let end = tokio::time::timeout(Duration::from_secs(2), next_item(&mut body_stream))
            .await
            .expect("body stream should end after the tail frame");
        assert!(end.is_none());

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        // The failure is still recorded as a failure: injection changes what the client sees,
        // not how the gateway classifies the attempt.
        assert_eq!(
            log.error_code,
            Some(GatewayErrorCode::StreamIdleTimeout.as_str().to_string())
        );

        // Audit metadata reached the request log's special settings (design §3.4 ordering).
        let settings = special_settings.lock().expect("special settings");
        let audit = settings
            .iter()
            .find(|value| value["type"] == "upstream_error_response_rule")
            .expect("rewrite audit entry");
        assert_eq!(audit["syntheticErrorCode"], "GW_STREAM_IDLE_TIMEOUT");
        assert_eq!(audit["upstreamStatusSynthetic"], true);
        assert_eq!(audit["upstreamStatus"], 524);
        // The rule's configured client status is recorded so the marker still validates in the UI
        // (`requestLogSpecialSettings.ts` fails anything outside 400..=599 closed), but it was
        // NOT applied: headers were committed long before, so the client keeps 200.
        assert_eq!(audit["clientStatus"], 503);
        assert_eq!(audit["clientStatusApplied"], false);
        assert_eq!(audit["scope"], "stream_tail");
        // Never persist the rewrite text itself.
        assert!(!serde_json::to_string(&*settings)
            .expect("settings json")
            .contains("上游流式响应中断"));
        drop(upstream_tx);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn idle_timeout_without_a_matching_rule_keeps_the_current_silent_eof() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-idle-tail-nomatch.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        // Configured for 502 only: an idle timeout synthesizes 524 and must not match.
        ctx.upstream_error_response_rules = vec![synthetic_status_rule(502)];
        let special_settings = Arc::clone(&ctx.special_settings);
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(300)),
            None,
        );
        let mut body_stream = body.into_data_stream();

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send output chunk");
        let first = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("output chunk should arrive")
            .expect("body should yield output chunk")
            .expect("output chunk should be ok");
        assert!(first.as_ref().starts_with(b"data:"));

        let end = tokio::time::timeout(Duration::from_secs(2), next_item(&mut body_stream))
            .await
            .expect("idle timeout should end the body stream");
        assert!(end.is_none(), "no rule matched: behavior must be unchanged");

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(
            log.error_code,
            Some(GatewayErrorCode::StreamIdleTimeout.as_str().to_string())
        );
        assert!(!special_settings
            .lock()
            .expect("special settings")
            .iter()
            .any(|value| value["type"] == "upstream_error_response_rule"));
        drop(upstream_tx);
    }

    /// claude / gemini / grok (and codex on OpenAI-compatible paths) do **not** use the relay:
    /// `use_sse_relay` is `is_codex_responses_event_stream_path`, so they stream straight from
    /// `UsageSseTeeStream`. That path delivers the tail frame as a stream item instead of going
    /// through `pending_tail`, and it needs its own coverage (PRD R4).
    #[tokio::test(flavor = "current_thread")]
    async fn direct_stream_path_delivers_the_tail_frame_as_a_stream_item() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-direct-tail-inject.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        ctx.cli_key = "claude".to_string();
        ctx.path = "/v1/messages".to_string();
        ctx.upstream_error_response_rules = vec![synthetic_status_rule(524)];
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        // No relay: the tee itself is the response body.
        let mut stream = UsageSseTeeStream::new(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(300)),
            None,
        );

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n",
            )))
            .await
            .expect("send output chunk");
        let first = tokio::time::timeout(Duration::from_secs(1), next_item(&mut stream))
            .await
            .expect("output chunk should arrive")
            .expect("stream should yield output chunk")
            .expect("output chunk should be ok");
        assert!(first.as_ref().starts_with(b"event: content_block_delta"));

        // Idle timeout fires; the rule's frame arrives as the next item, in Anthropic's shape.
        let tail = tokio::time::timeout(Duration::from_secs(2), next_item(&mut stream))
            .await
            .expect("idle timeout should produce the tail frame")
            .expect("stream should yield the tail frame")
            .expect("tail frame should be ok");
        let tail = String::from_utf8(tail.as_ref().to_vec()).expect("utf8 tail frame");
        assert!(tail.starts_with("event: error\ndata: "));
        assert!(tail.contains("上游流式响应中断，请重试"));
        assert!(!tail.contains("GW_STREAM_IDLE_TIMEOUT"));

        // Then a clean EOF, never a transport error.
        let end = tokio::time::timeout(Duration::from_secs(2), next_item(&mut stream))
            .await
            .expect("stream should end after the tail frame");
        assert!(end.is_none());

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(
            log.error_code,
            Some(GatewayErrorCode::StreamIdleTimeout.as_str().to_string())
        );
        drop(upstream_tx);
    }

    fn arm_probe(ctx: &mut StreamFinalizeCtx<tauri::test::MockRuntime>, now_unix: i64) {
        ctx.circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                provider_cooldown_secs: 0,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        ctx.circuit
            .record_failure(ctx.provider_id, now_unix, Some("TEST_PROBE_OPEN"));
        let token = match ctx.circuit.try_acquire_probe(
            ctx.provider_id,
            ctx.trace_id.as_str(),
            circuit_breaker::ProbeTrigger::AggressiveTurn,
            now_unix,
        ) {
            circuit_breaker::ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(
            ctx.provider_id,
            Some(circuit_breaker::ProbeTrigger::AggressiveTurn),
            None,
        )
        .claim_for_provider(
            ctx.provider_id,
            Some(circuit_breaker::ProbeLeaseGuard::new(
                Arc::clone(&ctx.circuit),
                token,
            )),
        )
        .expect("claim probe ownership");
        assert!(ownership.commit_at_transport_boundary(now_unix));
        ctx.dispatch_ownership = Some(ownership);
    }

    #[test]
    fn codex_responses_path_accepts_v1_and_backend_style_paths() {
        assert!(is_codex_responses_path("codex", "/v1/responses"));
        assert!(is_codex_responses_path("codex", "/responses"));
        assert!(is_codex_responses_path("codex", "/v1/responses/"));
        assert!(is_codex_responses_path("codex", "/v1/codex/responses"));
        assert!(!is_codex_responses_path("claude", "/v1/responses"));
        assert!(!is_codex_responses_path("codex", "/v1/chat/completions"));
    }

    #[test]
    fn stream_activity_tracker_flushes_at_most_every_30_seconds() {
        let mut tracker = StreamActivityTracker::new("trace-a", "codex", 1_000);
        assert!(!tracker.observe_chunk_at(10_000));
        assert!(!tracker.observe_chunk_at(30_999));
        assert!(tracker.observe_chunk_at(31_000));
        assert!(!tracker.observe_chunk_at(45_000));
        assert!(tracker.observe_chunk_at(61_000));
    }

    #[test]
    fn spawn_touch_activity_updates_active_registry_immediately() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-touch.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let ctx =
            test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests.clone());

        spawn_touch_activity(&ctx, 1_700_000_045_000, None);

        assert_eq!(
            active_requests.snapshot()[0].last_activity_ms,
            1_700_000_045_000
        );
    }

    #[test]
    fn plugin_stream_error_chunk_is_still_detected_without_rewriting_marker() {
        let chunk = Bytes::from_static(
            b": aio-plugin-error\nevent: error\ndata: {\"error\":\"plugin_failed\"}\n\n",
        );
        assert!(is_plugin_stream_error_chunk(chunk.as_ref()));
        assert_eq!(
            std::str::from_utf8(chunk.as_ref()).expect("utf8"),
            ": aio-plugin-error\nevent: error\ndata: {\"error\":\"plugin_failed\"}\n\n"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upstream_route_observer_tracks_chunked_model_and_reasoning_effort() {
        let observed_model = Arc::new(Mutex::new(None));
        let observed_conflict = Arc::new(Mutex::new(None));
        let observed_effort = Arc::new(Mutex::new(None));
        let route_tracker = Arc::new(Mutex::new(usage::SseUsageTracker::new("codex")));
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\ndata: {\"type\":\"response.compl",
            )))
            .await
            .expect("send first chunk");
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"eted\",\"response\":{\"model\":\"gpt-5.5\",\"reasoning\":{\"effort\":\"high\"}}}\n\n",
            )))
            .await
            .expect("send second chunk");
        drop(upstream_tx);

        let mut observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(upstream_rx),
            route_tracker,
            observed_model.clone(),
            observed_conflict,
            observed_effort.clone(),
        );
        while let Some(item) = next_item(&mut observer).await {
            item.expect("observed chunk");
        }

        assert_eq!(
            observed_model.lock().expect("model lock").as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(
            observed_effort.lock().expect("effort lock").as_deref(),
            Some("high")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upstream_route_observer_finalizes_unterminated_tail_before_error() {
        let observed_model = Arc::new(Mutex::new(None));
        let observed_conflict = Arc::new(Mutex::new(None));
        let observed_effort = Arc::new(Mutex::new(None));
        let route_tracker = Arc::new(Mutex::new(usage::SseUsageTracker::new("codex")));
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\ndata: {\"response\":{\"model\":\"gpt-5.5\",\"reasoning_effort\":\"xhigh\"}}",
            )))
            .await
            .expect("send unterminated event");
        let read_error = reqwest::Client::new()
            .get("://invalid-url")
            .build()
            .expect_err("invalid URL should produce a reqwest error");
        upstream_tx
            .send(Err(read_error))
            .await
            .expect("send read error");
        drop(upstream_tx);

        let mut observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(upstream_rx),
            route_tracker,
            observed_model.clone(),
            observed_conflict,
            observed_effort.clone(),
        );
        next_item(&mut observer)
            .await
            .expect("body chunk")
            .expect("body chunk should succeed");
        assert!(next_item(&mut observer).await.expect("read error").is_err());

        assert_eq!(
            observed_model.lock().expect("model lock").as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(
            observed_effort.lock().expect("effort lock").as_deref(),
            Some("xhigh")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn upstream_route_observer_does_not_snapshot_parse_small_pending_fragments() {
        let observed_model = Arc::new(Mutex::new(None));
        let observed_conflict = Arc::new(Mutex::new(None));
        let observed_effort = Arc::new(Mutex::new(None));
        let route_tracker = Arc::new(Mutex::new(usage::SseUsageTracker::new("codex")));
        let payload = format!(
            "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"model\":\"gpt-5.5\",\"reasoning\":{{\"effort\":\"high\"}},\"padding\":\"{}\"}}}}",
            "x".repeat(1024 * 1024 - 4096)
        );
        let chunks = payload
            .as_bytes()
            .chunks(1024)
            .map(Bytes::copy_from_slice)
            .collect::<Vec<_>>();
        let chunk_count = chunks.len();
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(chunk_count + 1);
        for chunk in chunks {
            upstream_tx.send(Ok(chunk)).await.expect("send fragment");
        }

        let mut observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(upstream_rx),
            route_tracker.clone(),
            observed_model.clone(),
            observed_conflict,
            observed_effort.clone(),
        );
        for _ in 0..chunk_count {
            next_item(&mut observer)
                .await
                .expect("fragment")
                .expect("fragment should succeed");
        }

        assert_eq!(
            route_tracker
                .lock()
                .expect("route tracker lock")
                .event_json_parse_attempts(),
            0
        );
        assert!(observed_model.lock().expect("model lock").is_none());
        assert!(observed_effort.lock().expect("effort lock").is_none());

        drop(upstream_tx);
        assert!(next_item(&mut observer).await.is_none());
        assert_eq!(
            observed_model.lock().expect("model lock").as_deref(),
            Some("gpt-5.5")
        );
        assert_eq!(
            observed_effort.lock().expect("effort lock").as_deref(),
            Some("high")
        );
        assert_eq!(
            route_tracker
                .lock()
                .expect("route tracker lock")
                .event_json_parse_attempts(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn outer_tee_drop_logs_unterminated_route_before_observer_drop() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-observer-drop.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        ctx.requested_model = Some("gpt-5.5".to_string());
        ctx.requested_upstream_model = Some("gpt-5.5".to_string());
        let observed_model = ctx.observed_upstream_model.clone();
        let observed_conflict = ctx.observed_upstream_conflicting_model.clone();
        let observed_effort = ctx.observed_upstream_reasoning_effort.clone();
        let route_tracker = ctx.upstream_route_tracker.clone();

        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5.4-mini\",\"reasoning\":{\"effort\":\"high\"},\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}",
            )))
            .await
            .expect("send unterminated completion");

        let observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(upstream_rx),
            route_tracker.clone(),
            observed_model.clone(),
            observed_conflict,
            observed_effort.clone(),
        );
        let mut stream = UsageSseTeeStream::new(observer, ctx, None, None);
        next_item(&mut stream)
            .await
            .expect("body chunk")
            .expect("body chunk should succeed");

        assert!(observed_model.lock().expect("model lock").is_none());
        assert!(observed_effort.lock().expect("effort lock").is_none());
        assert_eq!(
            route_tracker
                .lock()
                .expect("route tracker lock")
                .event_json_parse_attempts(),
            0
        );

        drop(stream);
        drop(upstream_tx);

        assert_eq!(
            observed_model.lock().expect("model lock").as_deref(),
            Some("gpt-5.4-mini")
        );
        assert_eq!(
            observed_effort.lock().expect("effort lock").as_deref(),
            Some("high")
        );

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        let special_settings = log
            .special_settings_json
            .as_deref()
            .expect("route setting json");
        assert!(special_settings.contains("\"actualModel\":\"gpt-5.4-mini\""));
        assert!(special_settings.contains("\"actualReasoningEffort\":\"high\""));
    }

    #[test]
    fn codex_client_abort_successish_rejects_untrusted_terminal_states() {
        assert!(!is_codex_client_abort_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            true,
            true,
            true,
            true
        ));
        assert!(!is_codex_client_abort_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false,
            true,
            false,
            true,
            true
        ));
        assert!(!is_codex_client_abort_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            false,
            false,
            true,
            true
        ));
        assert!(!is_codex_client_abort_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            true,
            false,
            false,
            true
        ));
    }

    #[test]
    fn codex_client_abort_successish_requires_forwarded_completion_for_probe() {
        assert!(is_codex_client_abort_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            true,
            false,
            true,
            true
        ));
        assert!(is_codex_client_abort_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            true,
            false,
            false,
            false
        ));
        assert!(!is_codex_client_abort_successish(
            "claude",
            "/v1/responses",
            200,
            true,
            true,
            true,
            false,
            true,
            true
        ));
        assert!(!is_codex_client_abort_successish(
            "codex",
            "/v1/chat/completions",
            200,
            true,
            true,
            true,
            false,
            true,
            true
        ));
    }

    #[test]
    fn codex_drop_successish_allows_completion_or_usage_with_terminal_marker() {
        assert!(is_codex_drop_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            false,
            true
        ));
        assert!(is_codex_drop_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false,
            true,
            true
        ));
        assert!(!is_codex_drop_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false,
            false,
            true
        ));
    }

    #[test]
    fn codex_body_buffer_drop_successish_when_usage_seen() {
        assert!(is_codex_body_buffer_drop_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true
        ));
        assert!(is_codex_body_buffer_drop_successish(
            "codex",
            "/responses",
            204,
            true,
            true
        ));
    }

    #[test]
    fn codex_body_buffer_drop_successish_requires_usage_and_stream_output() {
        assert!(!is_codex_body_buffer_drop_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false
        ));
        assert!(!is_codex_body_buffer_drop_successish(
            "codex",
            "/v1/responses",
            200,
            false,
            true
        ));
        assert!(!is_codex_body_buffer_drop_successish(
            "claude",
            "/v1/responses",
            200,
            true,
            true
        ));
        assert!(!is_codex_body_buffer_drop_successish(
            "codex",
            "/v1/chat/completions",
            200,
            true,
            true
        ));
    }

    #[test]
    fn codex_stream_terminal_error_successish_with_completion_and_usage() {
        assert!(is_codex_stream_terminal_error_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            true
        ));
        assert!(is_codex_stream_terminal_error_successish(
            "codex",
            "/responses",
            200,
            true,
            true,
            false
        ));
        assert!(is_codex_stream_terminal_error_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false,
            true
        ));
    }

    #[test]
    fn codex_stream_terminal_error_not_successish_without_completion_or_usage() {
        assert!(!is_codex_stream_terminal_error_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false,
            false
        ));
    }

    #[test]
    fn codex_stream_terminal_error_not_successish_for_non_codex() {
        assert!(!is_codex_stream_terminal_error_successish(
            "claude",
            "/v1/responses",
            200,
            true,
            true,
            true
        ));
    }

    #[test]
    fn codex_stream_terminal_error_not_successish_without_stream_output() {
        assert!(!is_codex_stream_terminal_error_successish(
            "codex",
            "/v1/responses",
            200,
            false,
            true,
            true
        ));
    }

    #[test]
    fn codex_stream_terminal_error_not_successish_on_error_status() {
        assert!(!is_codex_stream_terminal_error_successish(
            "codex",
            "/v1/responses",
            500,
            true,
            true,
            true
        ));
    }

    #[test]
    fn codex_stream_tail_error_successish_after_completion() {
        assert!(is_codex_stream_tail_error_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            true,
            false
        ));
        assert!(!is_codex_stream_tail_error_successish(
            "codex",
            "/v1/responses",
            200,
            true,
            false,
            false
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_disconnect_drain_preserves_usage_for_non_probe() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-drain.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let ctx = test_stream_finalize_ctx(app_handle, db, log_tx, active_requests.clone());
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(10)),
            None,
        );

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send first output chunk");

        let mut body_stream = body.into_data_stream();
        let first = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("first output chunk should arrive")
            .expect("body should yield first output")
            .expect("first output should be ok");
        assert!(first.as_ref().starts_with(b"data:"));

        drop(body_stream);

        let completion_tx = upstream_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = completion_tx
                .send(Ok(Bytes::from_static(
                    b"event: response.completed\n\
                      data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
                )))
                .await;
        });

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");

        assert_eq!(log.error_code, None);
        assert_eq!(log.status, Some(200));
        assert_eq!(log.input_tokens, Some(1));
        assert_eq!(log.output_tokens, Some(2));
        assert_eq!(log.total_tokens, Some(3));
        assert!(!log
            .special_settings_json
            .as_deref()
            .is_some_and(|value| value.contains("\"client_abort\"")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cx2cc_stream_logs_raw_provider_metrics_and_client_usage_json() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-cx2cc.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(
            app.handle().clone(),
            db,
            log_tx,
            Arc::clone(&active_requests),
        );
        ctx.cli_key = "claude".to_string();
        ctx.path = "/v1/messages".to_string();
        ctx.detect_stream_internal_errors = false;
        ctx.use_upstream_usage_metrics = true;
        ctx.upstream_route_tracker = Arc::new(Mutex::new(usage::SseUsageTracker::new("codex")));
        let raw_usage_tracker = Arc::clone(&ctx.upstream_route_tracker);

        let raw_sse = Bytes::from_static(
            b"event: response.created\n\
              data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_cache\",\"model\":\"gpt-5.4\",\"status\":\"in_progress\",\"output\":[]}}\n\n\
              event: response.completed\n\
              data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_cache\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"cached response\"}]}],\"usage\":{\"input_tokens\":100,\"output_tokens\":10,\"total_tokens\":110,\"input_tokens_details\":{\"cached_tokens\":80}}}}\n\n",
        );
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
        upstream_tx.send(Ok(raw_sse)).await.expect("send raw SSE");
        drop(upstream_tx);

        let observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(upstream_rx),
            Arc::clone(&ctx.upstream_route_tracker),
            Arc::clone(&ctx.observed_upstream_model),
            Arc::clone(&ctx.observed_upstream_conflicting_model),
            Arc::clone(&ctx.observed_upstream_reasoning_effort),
        );
        let bridged = crate::gateway::proxy::protocol_bridge::stream::BridgeStream::for_cx2cc(
            observer,
            true,
            Some("claude-sonnet-4-20250514".to_string()),
            crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::default(),
        );
        let mut stream = UsageSseTeeStream::new(bridged, ctx, None, None);
        while let Some(chunk) = next_item(&mut stream).await {
            chunk.expect("bridged SSE chunk");
        }

        let raw_usage = match raw_usage_tracker.lock() {
            Ok(mut tracker) => tracker.finalize(),
            Err(poisoned) => poisoned.into_inner().finalize(),
        }
        .expect("raw provider usage");
        assert_eq!(raw_usage.metrics.input_tokens, Some(100));
        assert_eq!(raw_usage.metrics.cache_read_input_tokens, Some(80));

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(log.input_tokens, Some(100));
        assert_eq!(log.output_tokens, Some(10));
        assert_eq!(log.total_tokens, Some(110));
        assert_eq!(log.cache_read_input_tokens, Some(80));

        let client_usage: serde_json::Value = serde_json::from_str(
            log.usage_json
                .as_deref()
                .expect("translated client usage_json"),
        )
        .expect("valid client usage_json");
        assert_eq!(client_usage["input_tokens"], 20);
        assert_eq!(client_usage["output_tokens"], 10);
        assert_eq!(client_usage["cache_read_input_tokens"], 80);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_rx_closed_after_forwarded_completion_is_success_without_upstream_eof() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-probe-delivered.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(
            app.handle().clone(),
            db,
            log_tx,
            Arc::clone(&active_requests),
        );
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        ctx.session_binding_request = ctx.session.begin_binding_request();
        arm_probe(&mut ctx, now_unix);
        ctx.attempts = vec![started_probe_attempt()];
        ctx.attempts_json = serde_json::to_string(&ctx.attempts).expect("attempts json");
        let circuit = Arc::clone(&ctx.circuit);
        let session = Arc::clone(&ctx.session);
        let cli_key = ctx.cli_key.clone();
        let session_id = ctx.session_id.clone().expect("session");
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(500)),
            None,
        );
        let mut body_stream = body.into_data_stream();

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send output");
        tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("output should arrive")
            .expect("body should yield output")
            .expect("output should be ok");

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            )))
            .await
            .expect("send completion");
        let completion = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("completion should arrive")
            .expect("body should yield completion")
            .expect("completion should be ok");
        assert!(completion
            .as_ref()
            .windows(b"response.completed".len())
            .any(|window| window == b"response.completed"));

        // The downstream closes only after consuming the trusted completion.
        // Keep upstream_tx alive to prove this path does not depend on transport EOF.
        drop(body_stream);

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(log.status, Some(200));
        assert!(log.error_code.is_none());
        assert_eq!(log.output_tokens, Some(2));
        assert!(!log
            .special_settings_json
            .as_deref()
            .is_some_and(|value| value.contains("\"client_abort\"")));
        let attempts: serde_json::Value =
            serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts.as_array().map(Vec::len), Some(1));
        assert_eq!(attempts[0]["outcome"], "success");
        assert_eq!(attempts[0]["status"], 200);
        assert_eq!(attempts[0]["decision"], "success");
        assert_eq!(attempts[0]["reason_code"], "request_success");
        assert_eq!(attempts[0]["probe_result"], "success");
        assert_eq!(attempts[0]["circuit_state_after"], "CLOSED");
        assert_eq!(
            circuit.snapshot(1, now_unix).state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(
            session.get_bound_provider(&cli_key, &session_id, now_unix),
            Some(1)
        );
        assert!(active_requests.snapshot().is_empty());
        drop(upstream_tx);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_protocol_terminal_is_success_without_claiming_upstream_eof() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-protocol-terminal.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(
            app.handle().clone(),
            db,
            log_tx,
            Arc::clone(&active_requests),
        );
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        ctx.session_binding_request = ctx.session.begin_binding_request();
        arm_probe(&mut ctx, now_unix);
        ctx.attempts = vec![started_probe_attempt()];
        ctx.attempts_json = serde_json::to_string(&ctx.attempts).expect("attempts json");
        let circuit = Arc::clone(&ctx.circuit);
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(500)),
            None,
        );
        let mut body_stream = body.into_data_stream();

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send output");
        tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("output should arrive")
            .expect("body should yield output")
            .expect("output should be ok");

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n\
                  data: [DONE]\n\n",
            )))
            .await
            .expect("send protocol terminal");
        let terminal = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("terminal chunk should arrive")
            .expect("body should yield terminal chunk")
            .expect("terminal chunk should be ok");
        assert!(terminal
            .as_ref()
            .windows(b"response.completed".len())
            .any(|window| window == b"response.completed"));
        assert!(
            tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
                .await
                .expect("protocol terminal should close downstream")
                .is_none()
        );

        // Keep the sender alive until finalization to prove transport EOF was not observed.
        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(log.status, Some(200));
        assert!(log.error_code.is_none());
        assert_eq!(log.output_tokens, Some(2));
        let activity: serde_json::Value = serde_json::from_str(
            log.activity_details_json
                .as_deref()
                .expect("terminal activity details"),
        )
        .expect("activity details json");
        assert_eq!(activity["terminal_origin"], "protocol_terminal");
        assert_eq!(activity["normal_eof"], false);
        assert_eq!(activity["completion_seen"], true);
        assert_eq!(activity["usage_seen"], true);
        let attempts: serde_json::Value =
            serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts[0]["probe_result"], "success");
        assert_eq!(
            circuit.snapshot(1, now_unix).state,
            circuit_breaker::CircuitState::Closed
        );
        assert!(active_requests.snapshot().is_empty());
        drop(upstream_tx);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_disconnect_then_completion_without_eof_stays_open() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-probe-abort.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(
            app.handle().clone(),
            db,
            log_tx,
            Arc::clone(&active_requests),
        );
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        ctx.session_binding_request = ctx.session.begin_binding_request();
        arm_probe(&mut ctx, now_unix);
        ctx.attempts = vec![started_probe_attempt()];
        ctx.attempts_json = serde_json::to_string(&ctx.attempts).expect("attempts json");
        let circuit = Arc::clone(&ctx.circuit);
        let session = Arc::clone(&ctx.session);
        let cli_key = ctx.cli_key.clone();
        let session_id = ctx.session_id.clone().expect("session");
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(500)),
            None,
        );
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send first output chunk");

        let mut body_stream = body.into_data_stream();
        tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("first output chunk should arrive")
            .expect("body should yield first output")
            .expect("first output should be ok");
        drop(body_stream);

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            )))
            .await
            .expect("send completion after disconnect");

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(
            log.error_code.as_deref(),
            Some(GatewayErrorCode::StreamAborted.as_str())
        );
        let attempts: serde_json::Value =
            serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts.as_array().map(Vec::len), Some(1));
        assert_ne!(attempts[0]["outcome"], "success");
        assert_eq!(attempts[0]["status"], 499);
        assert_eq!(attempts[0]["decision"], "abort");
        assert_eq!(attempts[0]["reason_code"], "aborted");
        assert_eq!(attempts[0]["probe_result"], "failed");
        assert_eq!(attempts[0]["circuit_state_after"], "OPEN");
        assert_eq!(
            circuit.snapshot(1, now_unix).state,
            circuit_breaker::CircuitState::Open
        );
        assert_eq!(
            session.get_bound_provider(&cli_key, &session_id, now_unix),
            Some(2)
        );
        assert!(active_requests.snapshot().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_direct_drop_after_completion_rewrites_final_attempt_as_failure() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-probe-direct-drop.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(
            app.handle().clone(),
            db,
            log_tx,
            Arc::clone(&active_requests),
        );
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        ctx.session_binding_request = ctx.session.begin_binding_request();
        arm_probe(&mut ctx, now_unix);
        ctx.attempts = vec![started_probe_attempt()];
        ctx.attempts_json = serde_json::to_string(&ctx.attempts).expect("attempts json");
        let circuit = Arc::clone(&ctx.circuit);
        let session = Arc::clone(&ctx.session);
        let cli_key = ctx.cli_key.clone();
        let session_id = ctx.session_id.clone().expect("session");
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            )))
            .await
            .expect("send completion");

        let mut stream = UsageSseTeeStream::new(RelayBodyStream::new(upstream_rx), ctx, None, None);
        next_item(&mut stream)
            .await
            .expect("completion chunk")
            .expect("completion chunk should succeed");
        drop(stream);
        drop(upstream_tx);

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(
            log.error_code.as_deref(),
            Some(GatewayErrorCode::StreamAborted.as_str())
        );
        let attempts: serde_json::Value =
            serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts.as_array().map(Vec::len), Some(1));
        assert_ne!(attempts[0]["outcome"], "success");
        assert_eq!(attempts[0]["status"], 499);
        assert_eq!(attempts[0]["decision"], "abort");
        assert_eq!(attempts[0]["reason_code"], "aborted");
        assert_eq!(attempts[0]["probe_result"], "failed");
        assert_eq!(attempts[0]["circuit_state_after"], "OPEN");
        assert_eq!(
            circuit.snapshot(1, now_unix).state,
            circuit_breaker::CircuitState::Open
        );
        assert_eq!(
            session.get_bound_provider(&cli_key, &session_id, now_unix),
            Some(2)
        );
        assert!(active_requests.snapshot().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stream_idle_timeout_fires_after_configured_silence() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-idle-timeout.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(
            RelayBodyStream::new(upstream_rx),
            ctx,
            Some(Duration::from_millis(500)),
            None,
        );
        let mut body_stream = body.into_data_stream();

        // Chunk gaps (100ms) are below the idle window (500ms) but sum above
        // it: all chunks arriving proves each chunk resets the timer. The 5x
        // margin absorbs scheduler hiccups on loaded CI runners; tokio::time
        // pause is not an option because the request log is delivered from
        // tauri's separate real-time runtime.
        for _ in 0..3 {
            upstream_tx
                .send(Ok(Bytes::from_static(
                    b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
                )))
                .await
                .expect("send output chunk");
            let chunk = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
                .await
                .expect("output chunk should arrive")
                .expect("body should yield output chunk")
                .expect("output chunk should be ok");
            assert!(chunk.as_ref().starts_with(b"data:"));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Upstream goes silent without closing; downstream stays connected.
        // The idle timeout must end the body stream.
        let end = tokio::time::timeout(Duration::from_secs(2), next_item(&mut body_stream))
            .await
            .expect("idle timeout should end the body stream");
        assert!(end.is_none());

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(
            log.error_code,
            Some(GatewayErrorCode::StreamIdleTimeout.as_str().to_string())
        );
        drop(upstream_tx);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stream_idle_timeout_disabled_keeps_stream_open() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-idle-disabled.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);

        let body = spawn_usage_sse_relay_body(RelayBodyStream::new(upstream_rx), ctx, None, None);
        let mut body_stream = body.into_data_stream();

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
            )))
            .await
            .expect("send first output chunk");
        let first = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("first output chunk should arrive")
            .expect("body should yield first output")
            .expect("first output should be ok");
        assert!(first.as_ref().starts_with(b"data:"));

        // Silence longer than any small idle window; disabled timeout must not
        // interrupt the stream.
        tokio::time::sleep(Duration::from_millis(50)).await;

        upstream_tx
            .send(Ok(Bytes::from_static(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"world\"}\n\n",
            )))
            .await
            .expect("send second output chunk");
        let second = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("second output chunk should arrive")
            .expect("body should yield second output")
            .expect("second output should be ok");
        assert!(second.as_ref().starts_with(b"data:"));

        // Let the stream end normally.
        drop(upstream_tx);
        let end = tokio::time::timeout(Duration::from_secs(1), next_item(&mut body_stream))
            .await
            .expect("body stream should end after upstream closes");
        assert!(end.is_none());

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert_eq!(log.error_code, None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sse_route_mapping_prefers_route_observed_before_bridge() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-route.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(app_handle, db, log_tx, active_requests);
        ctx.requested_model = Some("gpt-5.5".to_string());
        ctx.requested_upstream_model = Some("gpt-5.5".to_string());

        let (raw_tx, raw_rx) = tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        raw_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5.4-mini\",\"reasoning\":{\"effort\":\"low\"}}}\n\n",
            )))
            .await
            .expect("send raw upstream completion");
        drop(raw_tx);
        let mut observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(raw_rx),
            ctx.upstream_route_tracker.clone(),
            ctx.observed_upstream_model.clone(),
            ctx.observed_upstream_conflicting_model.clone(),
            ctx.observed_upstream_reasoning_effort.clone(),
        );
        while let Some(chunk) = next_item(&mut observer).await {
            chunk.expect("raw upstream chunk");
        }

        // Simulate a bridge rewriting the response route after the observer.
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5.5\",\"reasoning\":{\"effort\":\"high\"},\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            )))
            .await
            .expect("send completion");
        drop(upstream_tx);
        let mut stream = UsageSseTeeStream::new(RelayBodyStream::new(upstream_rx), ctx, None, None);

        while let Some(chunk) = next_item(&mut stream).await {
            chunk.expect("stream chunk");
        }

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        let special_settings = log
            .special_settings_json
            .as_deref()
            .expect("route setting json");
        assert!(special_settings.contains("\"type\":\"model_route_mapping\""));
        assert!(special_settings.contains("\"requestedModel\":\"gpt-5.5\""));
        assert!(special_settings.contains("\"actualModel\":\"gpt-5.4-mini\""));
        assert!(special_settings.contains("\"actualReasoningEffort\":\"low\""));
        assert!(special_settings.contains("\"actualReasoningEffortSource\":\"response\""));
        assert!(!special_settings.contains("\"actualModel\":\"gpt-5.5\""));
        assert!(!special_settings.contains("\"actualReasoningEffort\":\"high\""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sse_route_mapping_does_not_use_bridge_injected_effort() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("usage-tee-route-injection.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-usage-tee-drain"));
        let mut ctx = test_stream_finalize_ctx(app_handle, db, log_tx, active_requests);
        ctx.requested_model = Some("gpt-5.5".to_string());
        ctx.requested_upstream_model = Some("gpt-5.5".to_string());
        ctx.special_settings
            .lock()
            .expect("special settings lock")
            .push(serde_json::json!({
                "type": "codex_reasoning_effort",
                "effort": "high"
            }));

        let (raw_tx, raw_rx) = tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        raw_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5.5\"}}\n\n",
            )))
            .await
            .expect("send raw upstream completion");
        drop(raw_tx);
        let mut observer = UpstreamModelObserverStream::new(
            RelayBodyStream::new(raw_rx),
            ctx.upstream_route_tracker.clone(),
            ctx.observed_upstream_model.clone(),
            ctx.observed_upstream_conflicting_model.clone(),
            ctx.observed_upstream_reasoning_effort.clone(),
        );
        while let Some(chunk) = next_item(&mut observer).await {
            chunk.expect("raw upstream chunk");
        }

        // Simulate a bridge adding an effort that was absent from the upstream payload.
        let (upstream_tx, upstream_rx) =
            tokio::sync::mpsc::channel::<Result<Bytes, reqwest::Error>>(4);
        upstream_tx
            .send(Ok(Bytes::from_static(
                b"event: response.completed\n\
                  data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"model\":\"gpt-5.4-mini\",\"reasoning\":{\"effort\":\"medium\"},\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            )))
            .await
            .expect("send bridged completion");
        drop(upstream_tx);
        let mut stream = UsageSseTeeStream::new(RelayBodyStream::new(upstream_rx), ctx, None, None);

        while let Some(chunk) = next_item(&mut stream).await {
            chunk.expect("stream chunk");
        }

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        assert!(!log
            .special_settings_json
            .as_deref()
            .is_some_and(|settings| settings.contains("\"type\":\"model_route_mapping\"")));
    }
}
