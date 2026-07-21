//! Usage: Shared context types for `failover_loop` internal submodules.

use crate::circuit_breaker;
use crate::gateway::events::{ClaudeModelMapping, FailoverAttempt};
use crate::gateway::proxy::abort_guard::RequestAbortGuard;
use crate::gateway::proxy::cx2cc::settings::Cx2ccSettings;
use crate::gateway::proxy::gemini_oauth;
use crate::gateway::response_fixer;
use crate::gateway::runtime::GatewayAppState;
use crate::gateway::streams::StreamFinalizeCtx;
use axum::response::Response;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) const MAX_NON_SSE_BODY_BYTES: usize = 20 * 1024 * 1024;

pub(super) struct CommonCtxArgs<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) state: &'a GatewayAppState<R>,
    pub(super) cli_key: &'a String,
    pub(super) forwarded_path: &'a String,
    pub(super) observe: bool,
    pub(super) method_hint: &'a String,
    pub(super) query: &'a Option<String>,
    pub(super) trace_id: &'a String,
    pub(super) started: Instant,
    pub(super) created_at_ms: i64,
    pub(super) created_at: i64,
    pub(super) session_id: &'a Option<String>,
    pub(super) requested_model: &'a Option<String>,
    pub(super) managed_model_route:
        Option<&'a crate::gateway::managed_model_route::ManagedModelRoute>,
    pub(super) cx2cc_settings: &'a Cx2ccSettings,
    pub(super) effective_sort_mode_id: Option<i64>,
    pub(super) special_settings: &'a Arc<Mutex<Vec<serde_json::Value>>>,
    pub(super) provider_health_neutral: bool,
    pub(super) provider_cooldown_secs: i64,
    pub(super) upstream_first_byte_timeout_secs: u32,
    pub(super) upstream_first_byte_timeout: Option<Duration>,
    pub(super) upstream_stream_idle_timeout: Option<Duration>,
    pub(super) upstream_request_timeout_non_streaming: Option<Duration>,
    pub(super) verbose_provider_error: bool,
    pub(super) enable_response_fixer: bool,
    pub(super) response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) response_fixer_non_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) introspection_body: &'a [u8],
}

pub(super) struct CommonCtx<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) state: &'a GatewayAppState<R>,
    pub(super) cli_key: &'a String,
    pub(super) forwarded_path: &'a String,
    pub(super) observe: bool,
    pub(super) method_hint: &'a String,
    pub(super) query: &'a Option<String>,
    pub(super) trace_id: &'a String,
    pub(super) started: Instant,
    pub(super) created_at_ms: i64,
    pub(super) created_at: i64,
    pub(super) session_id: &'a Option<String>,
    pub(super) requested_model: &'a Option<String>,
    pub(super) managed_model_route:
        Option<&'a crate::gateway::managed_model_route::ManagedModelRoute>,
    pub(super) cx2cc_settings: &'a Cx2ccSettings,
    pub(super) effective_sort_mode_id: Option<i64>,
    pub(super) special_settings: &'a Arc<Mutex<Vec<serde_json::Value>>>,
    pub(super) provider_health_neutral: bool,
    pub(super) provider_cooldown_secs: i64,
    pub(super) upstream_first_byte_timeout_secs: u32,
    pub(super) upstream_first_byte_timeout: Option<Duration>,
    pub(super) upstream_stream_idle_timeout: Option<Duration>,
    pub(super) upstream_request_timeout_non_streaming: Option<Duration>,
    pub(super) verbose_provider_error: bool,
    pub(super) enable_response_fixer: bool,
    pub(super) response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) response_fixer_non_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) introspection_body: &'a [u8],
}

impl<'a, R: tauri::Runtime> Copy for CommonCtx<'a, R> {}

impl<'a, R: tauri::Runtime> Clone for CommonCtx<'a, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, R: tauri::Runtime> CommonCtx<'a, R> {
    pub(super) fn new(args: CommonCtxArgs<'a, R>) -> Self {
        Self {
            state: args.state,
            cli_key: args.cli_key,
            forwarded_path: args.forwarded_path,
            observe: args.observe,
            method_hint: args.method_hint,
            query: args.query,
            trace_id: args.trace_id,
            started: args.started,
            created_at_ms: args.created_at_ms,
            created_at: args.created_at,
            session_id: args.session_id,
            requested_model: args.requested_model,
            managed_model_route: args.managed_model_route,
            cx2cc_settings: args.cx2cc_settings,
            effective_sort_mode_id: args.effective_sort_mode_id,
            special_settings: args.special_settings,
            provider_health_neutral: args.provider_health_neutral,
            provider_cooldown_secs: args.provider_cooldown_secs,
            upstream_first_byte_timeout_secs: args.upstream_first_byte_timeout_secs,
            upstream_first_byte_timeout: args.upstream_first_byte_timeout,
            upstream_stream_idle_timeout: args.upstream_stream_idle_timeout,
            upstream_request_timeout_non_streaming: args.upstream_request_timeout_non_streaming,
            verbose_provider_error: args.verbose_provider_error,
            enable_response_fixer: args.enable_response_fixer,
            response_fixer_stream_config: args.response_fixer_stream_config,
            response_fixer_non_stream_config: args.response_fixer_non_stream_config,
            introspection_body: args.introspection_body,
        }
    }
}

impl<'a, R: tauri::Runtime> From<CommonCtxArgs<'a, R>> for CommonCtx<'a, R> {
    fn from(args: CommonCtxArgs<'a, R>) -> Self {
        Self::new(args)
    }
}

pub(super) struct CommonCtxOwned<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) state: &'a GatewayAppState<R>,
    pub(super) cli_key: String,
    pub(super) forwarded_path: String,
    pub(super) observe: bool,
    pub(super) method_hint: String,
    pub(super) query: Option<String>,
    pub(super) trace_id: String,
    pub(super) started: Instant,
    pub(super) created_at_ms: i64,
    pub(super) created_at: i64,
    pub(super) session_id: Option<String>,
    pub(super) requested_model: Option<String>,
    pub(super) managed_model_route: Option<crate::gateway::managed_model_route::ManagedModelRoute>,
    pub(super) cx2cc_settings: Cx2ccSettings,
    pub(super) effective_sort_mode_id: Option<i64>,
    pub(super) special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
    pub(super) provider_health_neutral: bool,
    pub(super) provider_cooldown_secs: i64,
    pub(super) upstream_first_byte_timeout_secs: u32,
    pub(super) upstream_first_byte_timeout: Option<Duration>,
    pub(super) upstream_stream_idle_timeout: Option<Duration>,
    pub(super) upstream_request_timeout_non_streaming: Option<Duration>,
    pub(super) enable_response_fixer: bool,
    pub(super) response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) response_fixer_non_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) introspection_body: Vec<u8>,
}

impl<'a, R: tauri::Runtime> From<CommonCtx<'a, R>> for CommonCtxOwned<'a, R> {
    fn from(ctx: CommonCtx<'a, R>) -> Self {
        Self {
            state: ctx.state,
            cli_key: ctx.cli_key.clone(),
            forwarded_path: ctx.forwarded_path.clone(),
            observe: ctx.observe,
            method_hint: ctx.method_hint.clone(),
            query: ctx.query.clone(),
            trace_id: ctx.trace_id.clone(),
            started: ctx.started,
            created_at_ms: ctx.created_at_ms,
            created_at: ctx.created_at,
            session_id: ctx.session_id.clone(),
            requested_model: ctx.requested_model.clone(),
            managed_model_route: ctx.managed_model_route.cloned(),
            cx2cc_settings: ctx.cx2cc_settings.clone(),
            effective_sort_mode_id: ctx.effective_sort_mode_id,
            special_settings: Arc::clone(ctx.special_settings),
            provider_health_neutral: ctx.provider_health_neutral,
            provider_cooldown_secs: ctx.provider_cooldown_secs,
            upstream_first_byte_timeout_secs: ctx.upstream_first_byte_timeout_secs,
            upstream_first_byte_timeout: ctx.upstream_first_byte_timeout,
            upstream_stream_idle_timeout: ctx.upstream_stream_idle_timeout,
            upstream_request_timeout_non_streaming: ctx.upstream_request_timeout_non_streaming,
            enable_response_fixer: ctx.enable_response_fixer,
            response_fixer_stream_config: ctx.response_fixer_stream_config,
            response_fixer_non_stream_config: ctx.response_fixer_non_stream_config,
            introspection_body: ctx.introspection_body.to_vec(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProviderCtx<'a> {
    pub(super) provider_id: i64,
    pub(super) provider_name_base: &'a String,
    pub(super) provider_base_url_base: &'a String,
    pub(super) active_requested_model: Option<&'a str>,
    pub(super) auth_mode: &'a str,
    pub(super) provider_index: u32,
    pub(super) provider_bridged: bool,
    pub(super) session_reuse: Option<bool>,
    pub(super) provider_max_attempts: u32,
    pub(super) stream_idle_timeout_seconds: Option<u32>,
    pub(super) upstream_retry_policy: &'a crate::settings::UpstreamRetryPolicy,
    pub(super) claude_model_mapping: Option<&'a ClaudeModelMapping>,
}

pub(super) struct ProviderCtxOwned {
    pub(super) provider_id: i64,
    pub(super) provider_name_base: String,
    pub(super) provider_base_url_base: String,
    pub(super) active_requested_model: Option<String>,
    pub(super) auth_mode: String,
    pub(super) provider_index: u32,
    pub(super) provider_bridged: bool,
    pub(super) session_reuse: Option<bool>,
    pub(super) provider_max_attempts: u32,
    pub(super) stream_idle_timeout_seconds: Option<u32>,
    pub(super) upstream_retry_policy: crate::settings::UpstreamRetryPolicy,
}

impl<'a> From<ProviderCtx<'a>> for ProviderCtxOwned {
    fn from(ctx: ProviderCtx<'a>) -> Self {
        Self {
            provider_id: ctx.provider_id,
            provider_name_base: ctx.provider_name_base.clone(),
            provider_base_url_base: ctx.provider_base_url_base.clone(),
            active_requested_model: ctx.active_requested_model.map(str::to_string),
            auth_mode: ctx.auth_mode.to_string(),
            provider_index: ctx.provider_index,
            provider_bridged: ctx.provider_bridged,
            session_reuse: ctx.session_reuse,
            provider_max_attempts: ctx.provider_max_attempts,
            stream_idle_timeout_seconds: ctx.stream_idle_timeout_seconds,
            upstream_retry_policy: ctx.upstream_retry_policy.clone(),
        }
    }
}

pub(super) fn build_stream_finalize_ctx<R: tauri::Runtime>(
    ctx: &CommonCtxOwned<'_, R>,
    provider_ctx: &ProviderCtxOwned,
    attempts: &[FailoverAttempt],
    status: u16,
    error_category: Option<&'static str>,
    error_code: Option<&'static str>,
    attempt_started: Instant,
) -> StreamFinalizeCtx<R> {
    let attempts_json = serde_json::to_string(attempts).unwrap_or_else(|_| "[]".to_string());

    StreamFinalizeCtx {
        app: ctx.state.app.clone(),
        db: ctx.state.db.clone(),
        log_tx: ctx.state.log_tx.clone(),
        plugin_pipeline: ctx.state.plugin_pipeline.clone(),
        circuit: ctx.state.circuit.clone(),
        session: ctx.state.session.clone(),
        session_id: ctx.session_id.clone(),
        sort_mode_id: ctx.effective_sort_mode_id,
        trace_id: ctx.trace_id.clone(),
        cli_key: ctx.cli_key.clone(),
        method: ctx.method_hint.clone(),
        path: ctx.forwarded_path.clone(),
        observe: ctx.observe,
        query: ctx.query.clone(),
        excluded_from_stats: false,
        special_settings: Arc::clone(&ctx.special_settings),
        provider_health_neutral: ctx.provider_health_neutral,
        status,
        error_category,
        error_code,
        started: ctx.started,
        attempt_started,
        attempts: attempts.to_vec(),
        attempts_json,
        requested_model:
            crate::gateway::managed_model_route::ManagedModelRoute::audit_requested_model(
                ctx.managed_model_route.as_ref(),
                ctx.requested_model.as_deref(),
                provider_ctx.active_requested_model.as_deref(),
            ),
        requested_upstream_model: provider_ctx.active_requested_model.clone(),
        managed_model_route: ctx.managed_model_route.is_some(),
        created_at_ms: ctx.created_at_ms,
        created_at: ctx.created_at,
        provider_cooldown_secs: ctx.provider_cooldown_secs,
        upstream_first_byte_timeout_secs: ctx.upstream_first_byte_timeout_secs,
        provider_id: provider_ctx.provider_id,
        provider_name: provider_ctx.provider_name_base.clone(),
        base_url: provider_ctx.provider_base_url_base.clone(),
        auth_mode: provider_ctx.auth_mode.clone(),
        upstream_route_tracker: Arc::new(Mutex::new(crate::usage::SseUsageTracker::new(
            &ctx.cli_key,
        ))),
        observed_upstream_model: Arc::new(Mutex::new(None)),
        observed_upstream_conflicting_model: Arc::new(Mutex::new(None)),
        observed_upstream_reasoning_effort: Arc::new(Mutex::new(None)),
        fake_200_detected: false,
        fake_200_quota_exhausted: false,
        activity: Arc::new(Mutex::new(
            crate::gateway::streams::StreamActivityTracker::new(
                &ctx.trace_id,
                &ctx.cli_key,
                ctx.created_at_ms,
            ),
        )),
        active_requests: ctx.state.active_requests.clone(),
    }
}

#[derive(Clone, Copy)]
pub(super) struct AttemptCtx<'a> {
    pub(super) attempt_index: u32,
    pub(super) retry_index: u32,
    #[allow(dead_code)]
    pub(super) provider_max_attempts: u32,
    pub(super) attempt_started_ms: u128,
    pub(super) attempt_started: Instant,
    pub(super) circuit_before: &'a circuit_breaker::CircuitSnapshot,
    pub(super) gemini_oauth_response_mode: Option<gemini_oauth::GeminiOAuthResponseMode>,
    pub(super) cx2cc_active: bool,
    pub(super) active_bridge_type: Option<&'a str>,
    pub(super) responses_cache_namespace: Option<&'a str>,
    pub(super) responses_cache_input: Option<&'a [serde_json::Value]>,
    pub(super) anthropic_stream_requested: bool,
}

pub(super) struct LoopState<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) attempts: &'a mut Vec<FailoverAttempt>,
    pub(super) failed_provider_ids: &'a mut HashSet<i64>,
    pub(super) last_outcome: &'a mut Option<AttemptOutcome>,
    pub(super) active_requested_model: &'a mut Option<String>,
    pub(super) circuit_snapshot: &'a mut circuit_breaker::CircuitSnapshot,
    pub(super) abort_guard: &'a mut RequestAbortGuard<R>,
}

#[derive(Clone, Copy)]
pub(super) struct AttemptOutcome {
    pub(super) error_category: &'static str,
    pub(super) error_code: &'static str,
}

impl AttemptOutcome {
    pub(super) fn new(error_category: &'static str, error_code: &'static str) -> Self {
        Self {
            error_category,
            error_code,
        }
    }
}

pub(super) struct FailoverRunState {
    pub(super) attempts: Vec<FailoverAttempt>,
    pub(super) failed_provider_ids: HashSet<i64>,
    pub(super) last_outcome: Option<AttemptOutcome>,
    pub(super) active_requested_model: Option<String>,
}

impl FailoverRunState {
    pub(super) fn new() -> Self {
        Self {
            attempts: Vec::new(),
            failed_provider_ids: HashSet::new(),
            last_outcome: None,
            active_requested_model: None,
        }
    }
}

impl<'a, R: tauri::Runtime> LoopState<'a, R> {
    pub(super) fn new(
        attempts: &'a mut Vec<FailoverAttempt>,
        failed_provider_ids: &'a mut HashSet<i64>,
        last_outcome: &'a mut Option<AttemptOutcome>,
        active_requested_model: &'a mut Option<String>,
        circuit_snapshot: &'a mut circuit_breaker::CircuitSnapshot,
        abort_guard: &'a mut RequestAbortGuard<R>,
    ) -> Self {
        Self {
            attempts,
            failed_provider_ids,
            last_outcome,
            active_requested_model,
            circuit_snapshot,
            abort_guard,
        }
    }

    /// Reborrow all fields into a new `LoopState` with a shorter lifetime.
    ///
    /// Use this when passing loop state by value to a callee while retaining
    /// access in the caller after the callee returns.
    pub(super) fn reborrow(&mut self) -> LoopState<'_, R> {
        LoopState {
            attempts: self.attempts,
            failed_provider_ids: self.failed_provider_ids,
            last_outcome: self.last_outcome,
            active_requested_model: self.active_requested_model,
            circuit_snapshot: self.circuit_snapshot,
            abort_guard: self.abort_guard,
        }
    }
}

pub(super) enum LoopControl {
    ContinueRetry,
    BreakRetry,
    Return(Response),
}
