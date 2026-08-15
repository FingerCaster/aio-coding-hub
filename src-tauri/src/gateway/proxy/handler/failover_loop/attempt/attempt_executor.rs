//! Usage: Single attempt execution (build request, send upstream, return result).
//!
//! Encapsulates URL construction, header assembly, auth injection, body
//! cleaning, and the upstream send for one retry attempt.

use super::provider_iterator::PreparedProvider;
use super::*;
use crate::gateway::plugins::context::{GatewayPluginHookName, GatewayRequestHookInput};
use crate::gateway::proxy::abort_guard::RequestAbortGuard;
use crate::gateway::proxy::request_context::RequestContext;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::Semaphore;

const GATEWAY_PROVIDER_ENABLE_CHECK_MAX_CONCURRENT: usize = 4;
static GATEWAY_PROVIDER_ENABLE_CHECK_LIMITER: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn gateway_provider_enable_check_limiter() -> Arc<Semaphore> {
    GATEWAY_PROVIDER_ENABLE_CHECK_LIMITER
        .get_or_init(|| Arc::new(Semaphore::new(GATEWAY_PROVIDER_ENABLE_CHECK_MAX_CONCURRENT)))
        .clone()
}

/// Mutable per-provider state that persists across retries within one provider.
pub(super) struct RetryLoopState {
    pub(super) claude_api_key_bearer_fallback: bool,
    pub(super) oauth_reactive_refreshed_once: bool,
    pub(super) codex_previous_response_id_rectifier_retried: bool,
    pub(super) thinking_signature_rectifier_retried: bool,
    pub(super) thinking_budget_rectifier_retried: bool,
    pub(super) configured_transient_retries_used: u32,
    pub(super) allow_next_retry_beyond_max_attempts: bool,
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
            configured_transient_retries_used: self.configured_transient_retries_used,
            allow_next_retry_beyond_max_attempts: false,
        }
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
            configured_transient_retries_used: 0,
            allow_next_retry_beyond_max_attempts: false,
        }
    }
}

/// Timing captured at the start of an attempt, before the upstream send.
pub(super) struct AttemptTiming {
    pub(super) attempt_started_ms: u128,
    pub(super) attempt_started: Instant,
    pub(super) reasoning_effort: Option<String>,
    pub(super) upstream_sent: bool,
    target: AttemptTarget,
}

impl AttemptTiming {
    pub(super) fn response_header_timeout(
        &self,
        configured_timeout: Option<std::time::Duration>,
    ) -> Option<std::time::Duration> {
        self.target.effective_first_byte_timeout(configured_timeout)
    }

    pub(super) fn sse_first_chunk_timeout(
        &self,
        configured_timeout: Option<std::time::Duration>,
    ) -> Option<std::time::Duration> {
        self.target.effective_first_byte_timeout(configured_timeout)
    }
}

/// Final per-attempt target ownership, minted only after the prepared internal
/// intent and the ordinary self-loop validator agree on the exact target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AttemptTarget {
    ExternalProvider,
    InternalCodexReentry,
}

impl AttemptTarget {
    fn after_validation(
        internal_reentry_intent_matched: bool,
        target_validation: &Result<
            crate::gateway::http_client::ValidatedGatewayTarget,
            crate::gateway::http_client::GatewayTargetValidationError,
        >,
    ) -> Self {
        if internal_reentry_intent_matched
            && matches!(
                target_validation,
                Err(crate::gateway::http_client::GatewayTargetValidationError::SelfLoop)
            )
        {
            Self::InternalCodexReentry
        } else {
            Self::ExternalProvider
        }
    }

    fn is_internal_codex_reentry(self) -> bool {
        self == Self::InternalCodexReentry
    }

    fn effective_first_byte_timeout(
        self,
        configured_timeout: Option<std::time::Duration>,
    ) -> Option<std::time::Duration> {
        match self {
            Self::ExternalProvider => configured_timeout,
            // The local scheduling hop delegates Provider timeout ownership to
            // the inner Codex gateway request. Other timeout families remain.
            Self::InternalCodexReentry => None,
        }
    }
}

/// Result of building + sending one attempt.
pub(super) enum AttemptSendOutcome {
    Response(reqwest::Response, AttemptTiming),
    Timeout(AttemptTiming),
    ReqwestError(reqwest::Error, AttemptTiming),
    /// URL build failure already recorded; caller should apply the returned LoopControl.
    UrlBuildFailed(LoopControl),
    /// OAuth adapter injection failed; break out of retry loop for this provider.
    OAuthInjectFailed,
    /// Plugin blocked the request before the upstream send.
    PluginBlocked(String),
    /// A request plugin changed a server-managed model binding before send.
    ManagedModelInvalid(String),
    /// A configured route matched but could not be applied atomically.
    ConfiguredModelRouteApplyFailed(
        crate::gateway::configured_model_route::ConfiguredModelRouteApplyError,
    ),
    /// Dispatch ownership became stale at the transport boundary; no network
    /// call was made and the outer loop may continue with the stable provider.
    DispatchRejected,
    /// The Provider or its bridge source was disabled after route selection.
    ProviderDisabled(i64),
    /// The authoritative Provider enabled-state read failed closed.
    ProviderEnableCheckFailed,
    /// The final Provider target failed the local gateway self-loop guard.
    ProviderTargetRejected(crate::gateway::http_client::GatewayTargetValidationError),
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
    ManagedModelInvalid(String),
    ConfiguredModelRouteApplyFailed(
        crate::gateway::configured_model_route::ConfiguredModelRouteApplyError,
    ),
    DispatchRejected,
    ProviderDisabled(i64),
    ProviderEnableCheckFailed,
    ProviderTargetRejected(crate::gateway::http_client::GatewayTargetValidationError),
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
            let provider_ctx = ProviderCtx {
                active_requested_model: None,
                ..build_provider_ctx(prepared)
            };
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
        PreparedSendOutcome::ManagedModelInvalid(reason) => {
            AttemptSendOutcome::ManagedModelInvalid(reason)
        }
        PreparedSendOutcome::ConfiguredModelRouteApplyFailed(error) => {
            AttemptSendOutcome::ConfiguredModelRouteApplyFailed(error)
        }
        PreparedSendOutcome::DispatchRejected => AttemptSendOutcome::DispatchRejected,
        PreparedSendOutcome::ProviderDisabled(provider_id) => {
            AttemptSendOutcome::ProviderDisabled(provider_id)
        }
        PreparedSendOutcome::ProviderEnableCheckFailed => {
            AttemptSendOutcome::ProviderEnableCheckFailed
        }
        PreparedSendOutcome::ProviderTargetRejected(error) => {
            AttemptSendOutcome::ProviderTargetRejected(error)
        }
    }
}

/// Build request headers, inject auth, clean body, run before-send plugins, and
/// send one prepared upstream request. When supplied, `abort_guard` tracks the
/// active request for cancellation.
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_prepared_upstream<R>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    prepared: &mut PreparedProvider,
    retry_state: &mut RetryLoopState,
    retry_index: u32,
    attempt_index: u32,
    first_byte_timeout: Option<std::time::Duration>,
    mut abort_guard: Option<&mut RequestAbortGuard<R>>,
) -> PreparedSendOutcome
where
    R: tauri::Runtime,
    R::Handle: Unpin,
{
    if let Some(abort_guard) = abort_guard.as_deref_mut() {
        // Every attempt owns this slot, including attempts without a probe.
        // Clear the previous provider before any local preparation can fail or await.
        abort_guard.replace_dispatch_ownership(None);
    }
    crate::gateway::response_fixer::clear_configured_model_route(&input.special_settings);
    let attempt_started_ms = input.started.elapsed().as_millis();
    let circuit_before = prepared.circuit_snapshot.clone();

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
    let mut configured_route_marker = None;
    if let Some(route) = prepared.configured_model_route.clone() {
        let priced_cli_key = prepared
            .bridge_source
            .as_ref()
            .map(|(_, source_cli_key)| source_cli_key.as_str())
            .unwrap_or(input.cli_key.as_str())
            .to_string();
        let outcome = match crate::gateway::configured_model_route::apply(
            &route,
            &prepared.upstream_forwarded_path,
            prepared.upstream_query.as_deref(),
            &body_state_for_attempt.decoded_clone(),
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                return PreparedSendOutcome::ConfiguredModelRouteApplyFailed(error);
            }
        };
        prepared.upstream_forwarded_path = outcome.path.clone();
        prepared.upstream_query = outcome.query.clone();
        if outcome.body != body_state_for_attempt.decoded_clone() {
            body_state_for_attempt.replace_decoded(outcome.body.clone());
            prepared.upstream_body_bytes = outcome.body.clone();
            prepared.strip_request_content_encoding = true;
            prepared.request_body_mutated_before_attempt = true;
        }
        prepared.active_requested_model = outcome.effective_model.clone();
        configured_route_marker = Some((route, priced_cli_key, outcome));
    }
    if let Err(reason) = sync_final_wire_model(input, prepared, &body_state_for_attempt) {
        return PreparedSendOutcome::ManagedModelInvalid(reason);
    }
    if let Some(route) = input.managed_model_route.as_ref() {
        crate::gateway::managed_model_route::mark_applied(
            &input.special_settings,
            route,
            route.remote_model_id.as_str(),
        );
    }
    let url = match try_build_url(prepared) {
        Ok(url) => url,
        Err(error) => {
            return PreparedSendOutcome::UrlBuildFailed(PreparedUrlBuildFailure {
                error,
                attempt_started_ms,
                circuit_before,
            });
        }
    };
    // Matching consumes the typed intent. It can only authorize the exact self-loop
    // exception below; ordinary target validation still runs for every request.
    let internal_reentry_intent_matched =
        cx2cc_preparation::InternalCodexReentry::consume_and_match(
            &mut prepared.internal_codex_reentry,
            &input.trace_id,
            prepared.provider_id,
            &input.req_method,
            &url,
        );
    // Resolve first, but defer returning its result until after the authoritative
    // enabled-state read so the outer Provider switch remains the master gate.
    // The enabled-state read is therefore also the final async preparation step.
    let target_validation = crate::gateway::http_client::validate_gateway_target(&url).await;
    let attempt_target =
        AttemptTarget::after_validation(internal_reentry_intent_matched, &target_validation);
    let db = ctx.state.db.clone();
    let provider_id = prepared.provider_id;
    let provider_uuid = prepared.provider_uuid.clone();
    let bridge_source = prepared
        .bridge_source
        .as_ref()
        .map(|(source, _)| (source.id, source.provider_uuid.clone()));
    match gateway_provider_enabled_check(db, provider_id, provider_uuid, bridge_source).await {
        Ok(Some(disabled_provider_id)) => {
            tracing::info!(
                trace_id = %input.trace_id,
                cli_key = %input.cli_key,
                provider_id = prepared.provider_id,
                disabled_provider_id,
                retry_index,
                "provider skipped because the global Provider switch is disabled"
            );
            return PreparedSendOutcome::ProviderDisabled(disabled_provider_id);
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(
                trace_id = %input.trace_id,
                cli_key = %input.cli_key,
                provider_id = prepared.provider_id,
                retry_index,
                error = %error,
                "Provider enabled-state check failed before upstream send"
            );
            return PreparedSendOutcome::ProviderEnableCheckFailed;
        }
    }
    let pinned_client = match target_validation {
        Ok(target) => match target.into_pinned_client() {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(
                    trace_id = %input.trace_id,
                    cli_key = %input.cli_key,
                    provider_id = prepared.provider_id,
                    retry_index,
                    error,
                    "failed to build DNS-pinned Provider target client"
                );
                return PreparedSendOutcome::ProviderTargetRejected(
                    crate::gateway::http_client::GatewayTargetValidationError::ResolutionFailed,
                );
            }
        },
        Err(crate::gateway::http_client::GatewayTargetValidationError::SelfLoop)
            if attempt_target.is_internal_codex_reentry() =>
        {
            None
        }
        Err(error) => {
            tracing::warn!(
                trace_id = %input.trace_id,
                cli_key = %input.cli_key,
                provider_id = prepared.provider_id,
                retry_index,
                reason = error.message(),
                "provider target rejected before upstream send"
            );
            return PreparedSendOutcome::ProviderTargetRejected(error);
        }
    };
    if let Some((route, priced_cli_key, outcome)) = configured_route_marker.as_ref() {
        crate::gateway::configured_model_route::mark_applied(
            &input.special_settings,
            route,
            priced_cli_key,
            outcome,
        );
    }
    let reasoning_effort = reasoning_effort::extract(
        body_state_for_attempt.decoded(),
        &prepared.upstream_forwarded_path,
        prepared.gemini_oauth_response_mode,
    );
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
    if attempt_target.is_internal_codex_reentry() {
        let Some(nonce) = ctx
            .state
            .internal_reentry
            .issue(prepared.provider_id, &input.trace_id)
        else {
            return PreparedSendOutcome::ProviderTargetRejected(
                crate::gateway::http_client::GatewayTargetValidationError::ResolutionFailed,
            );
        };
        let Ok(value) = axum::http::HeaderValue::from_str(&nonce) else {
            return PreparedSendOutcome::ProviderTargetRejected(
                crate::gateway::http_client::GatewayTargetValidationError::ResolutionFailed,
            );
        };
        headers.insert(
            crate::gateway::internal_reentry::INTERNAL_REENTRY_HEADER,
            value,
        );
    }

    let mut timing = AttemptTiming {
        attempt_started_ms,
        attempt_started: Instant::now(),
        reasoning_effort,
        upstream_sent: true,
        target: attempt_target,
    };

    let dispatch_ownership = prepared.dispatch_ownership.clone();
    let client = if attempt_target.is_internal_codex_reentry() {
        ctx.state.direct_internal_reentry_client()
    } else {
        pinned_client.unwrap_or_else(|| ctx.state.client())
    };
    let effective_first_byte_timeout = timing.response_header_timeout(first_byte_timeout);
    let send_result = send::send_upstream_with_first_byte_timeout(
        client,
        input.req_method.clone(),
        url,
        headers,
        upstream_body,
        effective_first_byte_timeout,
        || {
            if let Some(ownership) = dispatch_ownership.as_ref() {
                if !ownership.commit_at_transport_boundary(now_unix_seconds() as i64) {
                    return false;
                }
            }
            if let Some(abort_guard) = abort_guard.as_deref_mut() {
                abort_guard.replace_dispatch_ownership(dispatch_ownership.clone());
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
            true
        },
    )
    .await;

    if let send::SendResult::Err(error) = &send_result {
        if error.is_connect() {
            timing.upstream_sent = false;
        }
    }
    if !matches!(&send_result, send::SendResult::DispatchRejected) {
        if let Some(abort_guard) = abort_guard {
            abort_guard.update_in_flight_attempt_send_state(
                timing.reasoning_effort.clone(),
                timing.upstream_sent,
            );
        }
    }

    match send_result {
        send::SendResult::Ok(resp) => PreparedSendOutcome::Response(resp, timing),
        send::SendResult::Timeout => PreparedSendOutcome::Timeout(timing),
        send::SendResult::Err(err) => PreparedSendOutcome::ReqwestError(err, timing),
        send::SendResult::DispatchRejected => PreparedSendOutcome::DispatchRejected,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn gateway_provider_enabled_check(
    db: crate::db::Db,
    provider_id: i64,
    provider_uuid: String,
    bridge_source: Option<(i64, String)>,
) -> crate::shared::error::AppResult<Option<i64>> {
    let permit = gateway_provider_enable_check_limiter()
        .acquire_owned()
        .await
        .map_err(|_| {
            crate::shared::error::AppError::new(
                "TASK_JOIN",
                "gateway_provider_enabled_check: limiter closed",
            )
        })?;

    crate::blocking::run("gateway_provider_enabled_check", move || {
        let _permit = permit;
        crate::providers::first_disabled_provider_for_gateway(
            &db,
            provider_id,
            &provider_uuid,
            bridge_source
                .as_ref()
                .map(|(source_id, source_uuid)| (*source_id, source_uuid.as_str())),
        )
    })
    .await
}

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

fn sync_final_wire_model<R: tauri::Runtime>(
    input: &RequestContext<R>,
    prepared: &mut PreparedProvider,
    body: &crate::gateway::proxy::request_body::GatewayRequestBody,
) -> Result<(), String> {
    if !should_sync_final_wire_model(
        &input.cli_key,
        input.managed_model_route.is_some(),
        prepared.configured_model_route.is_some(),
    ) {
        return Ok(());
    }

    let body_json = serde_json::from_slice::<serde_json::Value>(body.decoded().as_ref()).ok();
    let final_model = crate::gateway::util::infer_requested_model_info(
        &prepared.upstream_forwarded_path,
        prepared.upstream_query.as_deref(),
        body_json.as_ref(),
    )
    .model;

    if let Some(route) = input.managed_model_route.as_ref() {
        validate_managed_wire_model(route, prepared.provider_id, final_model.as_deref())?;
    }

    prepared.active_requested_model = final_model;
    Ok(())
}

fn should_sync_final_wire_model(
    cli_key: &str,
    managed_model_route: bool,
    configured_model_route: bool,
) -> bool {
    managed_model_route || configured_model_route || cli_key == "codex"
}

fn validate_managed_wire_model(
    route: &crate::gateway::managed_model_route::ManagedModelRoute,
    provider_id: i64,
    final_model: Option<&str>,
) -> Result<(), String> {
    if provider_id != route.provider_id || final_model != Some(route.remote_model_id.as_str()) {
        return Err("managed model binding changed before upstream send".to_string());
    }
    Ok(())
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
        configured_retry_backoff: None,
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
        anthropic_stream_requested: prepared.anthropic_stream_requested,
        reasoning_effort: None,
        upstream_sent: false,
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
        dispatch_ownership: prepared.dispatch_ownership.as_ref(),
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
    let probe_ownership = prepared
        .dispatch_ownership
        .as_ref()
        .filter(|ownership| ownership.is_probe());
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
        selection_method: if probe_ownership.is_some() {
            Some(dc::SELECTION_METHOD_CIRCUIT_PROBE)
        } else {
            dc::selection_method(prepared.provider_index, retry_index, prepared.session_reuse)
        },
        reason_code: None,
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(0),
        circuit_state_before: Some(circuit_before.state.as_str()),
        circuit_state_after: None,
        circuit_failure_count: Some(circuit_before.failure_count),
        circuit_failure_threshold: Some(circuit_before.failure_threshold),
        probe: probe_ownership.map(|_| true),
        probe_trigger: probe_ownership
            .and_then(|ownership| ownership.probe_trigger())
            .map(|trigger| trigger.as_str()),
        probe_result: probe_ownership.map(|_| "started"),
        probe_generation: probe_ownership.and_then(|ownership| ownership.probe_generation()),
        circuit_recover_at_unix: None,
        circuit_trigger_error_code: None,
        provider_bridged: Some(prepared.provider_bridged),
        timeout_secs: None,
        stream_internal_error: None,
        requested_upstream_model: prepared.active_requested_model.clone(),
        reasoning_effort: None,
        upstream_sent: false,
    };
    let audit_requested_model = requested_model_for_audit(
        &input.special_settings,
        input.managed_model_route.as_ref(),
        input.requested_model.as_deref(),
        prepared.active_requested_model.as_deref(),
    );
    abort_guard.update_requested_model(audit_requested_model.clone());
    let started_event = input.observe_request.then(|| {
        bound_attempt_event(GatewayAttemptEvent {
            trace_id: input.trace_id.clone(),
            cli_key: input.cli_key.clone(),
            session_id: input.session_id.clone(),
            method: input.method_hint.clone(),
            path: input.forwarded_path.clone(),
            query: input.query.clone(),
            requested_model: audit_requested_model,
            requested_upstream_model: prepared.active_requested_model.clone(),
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
            probe: probe_ownership.map(|_| true),
            probe_trigger: probe_ownership
                .and_then(|ownership| ownership.probe_trigger())
                .map(|trigger| trigger.as_str()),
            probe_result: probe_ownership.map(|_| "started"),
            probe_generation: probe_ownership.and_then(|ownership| ownership.probe_generation()),
            claude_model_mapping: prepared.claude_model_mapping.clone(),
            reasoning_effort: None,
            upstream_sent: false,
        })
    });
    if let Some(started_event) = started_event.as_ref() {
        let elapsed_ms = i64::try_from(attempt_started_ms).unwrap_or(i64::MAX);
        input.state.active_requests.record_attempt_start(
            started_event.clone(),
            input.created_at_ms.saturating_add(elapsed_ms),
        );
    }
    abort_guard.capture_in_flight_attempt(&started_attempt);
    if let Some(started_event) = started_event {
        emit_attempt_event(&input.state.app, started_event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn timing_for_target(target: AttemptTarget) -> AttemptTiming {
        AttemptTiming {
            attempt_started_ms: 0,
            attempt_started: Instant::now(),
            reasoning_effort: None,
            upstream_sent: true,
            target,
        }
    }

    #[test]
    fn final_wire_model_sync_is_scoped_to_codex_or_managed_routes() {
        assert!(should_sync_final_wire_model("codex", false, false));
        assert!(should_sync_final_wire_model("claude", true, false));
        assert!(should_sync_final_wire_model("claude", false, true));
        assert!(!should_sync_final_wire_model("claude", false, false));
        assert!(!should_sync_final_wire_model("grok", false, false));
    }

    #[test]
    fn confirmed_internal_reentry_delegates_both_first_byte_deadlines() {
        let configured = Some(std::time::Duration::from_secs(120));
        let validation = Err(crate::gateway::http_client::GatewayTargetValidationError::SelfLoop);
        let timing = timing_for_target(AttemptTarget::after_validation(true, &validation));

        assert_eq!(timing.response_header_timeout(configured), None);
        assert_eq!(timing.sse_first_chunk_timeout(configured), None);
        assert_eq!(timing.response_header_timeout(None), None);
        assert_eq!(timing.sse_first_chunk_timeout(None), None);
    }

    #[test]
    fn internal_reentry_requires_matching_intent_and_current_gateway_target() {
        use crate::gateway::http_client::{GatewayTargetValidationError, ValidatedGatewayTarget};

        let self_loop = Err(GatewayTargetValidationError::SelfLoop);
        let resolution_failed = Err(GatewayTargetValidationError::ResolutionFailed);
        let external = Ok(ValidatedGatewayTarget::default());

        assert_eq!(
            AttemptTarget::after_validation(true, &self_loop),
            AttemptTarget::InternalCodexReentry
        );
        for (intent_matched, validation) in [
            (false, &self_loop),
            (true, &resolution_failed),
            (false, &resolution_failed),
            (true, &external),
            (false, &external),
        ] {
            assert_eq!(
                AttemptTarget::after_validation(intent_matched, validation),
                AttemptTarget::ExternalProvider
            );
        }
    }

    #[test]
    fn ordinary_and_explicit_source_targets_retain_both_first_byte_deadlines() {
        let configured = Some(std::time::Duration::from_secs(120));
        let validation = Ok(crate::gateway::http_client::ValidatedGatewayTarget::default());
        let timing = timing_for_target(AttemptTarget::after_validation(false, &validation));

        assert_eq!(timing.response_header_timeout(configured), configured);
        assert_eq!(timing.sse_first_chunk_timeout(configured), configured);
        assert_eq!(timing.response_header_timeout(None), None);
        assert_eq!(timing.sse_first_chunk_timeout(None), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn delegated_header_budget_outlives_configured_timeout_with_wall_clock_cap() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind delayed local scheduling hop");
        let address = listener.local_addr().expect("delayed hop address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept delayed hop");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await;
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await
                .expect("write delayed response");
        });

        let validation = Err(crate::gateway::http_client::GatewayTargetValidationError::SelfLoop);
        let timing = timing_for_target(AttemptTarget::after_validation(true, &validation));
        let configured_timeout = Some(std::time::Duration::from_millis(25));
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("build direct test client");
        let send = send::send_upstream_with_first_byte_timeout(
            client,
            axum::http::Method::GET,
            reqwest::Url::parse(&format!("http://{address}/nested")).expect("delayed hop URL"),
            axum::http::HeaderMap::new(),
            Bytes::new(),
            timing.response_header_timeout(configured_timeout),
            || true,
        );

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), send)
            .await
            .expect("delegated scheduling hop exceeded wall-clock cap");
        match result {
            send::SendResult::Ok(response) => {
                assert_eq!(response.status(), reqwest::StatusCode::OK)
            }
            _ => panic!("delegated scheduling hop should outlive the configured timeout"),
        }
        server.await.expect("delayed hop task");
    }

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
}
