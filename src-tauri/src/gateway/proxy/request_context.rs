//! Usage: Request context (SSOT) for gateway proxy forwarding.

use super::abort_guard::RequestAbortGuard;
use super::request_body::GatewayRequestBody;
use crate::gateway::response_fixer;
use crate::gateway::runtime::GatewayAppState;
use crate::gateway::util::{strip_hop_headers, RequestedModelLocation};
use crate::infra::settings::MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
use crate::providers;
use axum::body::Bytes;
use axum::http::{header, HeaderMap, HeaderValue, Method};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(super) struct RequestContext<R: tauri::Runtime = tauri::Wry> {
    pub(super) state: GatewayAppState<R>,
    pub(super) cli_key: String,
    pub(super) forwarded_path: String,
    pub(super) observe_request: bool,
    pub(super) req_method: Method,
    pub(super) method_hint: String,
    pub(super) query: Option<String>,
    pub(super) trace_id: String,
    pub(super) started: Instant,
    pub(super) created_at_ms: i64,
    pub(super) created_at: i64,
    pub(super) session_id: Option<String>,
    pub(super) requested_model: Option<String>,
    pub(super) requested_model_location: Option<RequestedModelLocation>,
    pub(super) effective_sort_mode_id: Option<i64>,
    pub(super) providers: Vec<providers::ProviderForGateway>,
    pub(super) session_bound_provider_id: Option<i64>,
    pub(super) base_headers: HeaderMap,
    pub(super) body_bytes: Bytes,
    pub(super) request_body_state: GatewayRequestBody,
    pub(super) introspection_json: Option<serde_json::Value>,
    pub(super) strip_request_content_encoding_seed: bool,
    pub(super) special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
    pub(super) provider_base_url_ping_cache_ttl_seconds: u32,
    pub(super) verbose_provider_error: bool,
    pub(super) enable_codex_session_id_completion: bool,
    pub(super) codex_reasoning_guard_enabled: bool,
    pub(super) codex_reasoning_guard_rule_mode: crate::settings::CodexReasoningGuardRuleMode,
    pub(super) codex_reasoning_guard_compare_mode: crate::settings::CodexReasoningGuardCompareMode,
    pub(super) codex_reasoning_guard_reasoning_equals: Vec<i64>,
    pub(super) codex_reasoning_guard_model_rules:
        Vec<crate::settings::CodexReasoningGuardModelRule>,
    pub(super) codex_reasoning_guard_active_template_id: String,
    pub(super) codex_reasoning_guard_custom_templates:
        Vec<crate::settings::CodexReasoningGuardRuleTemplate>,
    pub(super) codex_reasoning_guard_post_match_strategy:
        crate::settings::CodexReasoningGuardPostMatchStrategy,
    pub(super) codex_reasoning_guard_immediate_retry_budget: u32,
    pub(super) codex_reasoning_guard_delayed_retry_budget: u32,
    pub(super) codex_reasoning_guard_delayed_retry_ms: u32,
    pub(super) codex_reasoning_guard_exhausted_action:
        crate::settings::CodexReasoningGuardExhaustedAction,
    pub(super) codex_reasoning_guard_retry_policy: crate::settings::CodexReasoningGuardRetryPolicy,
    pub(super) codex_reasoning_guard_concurrent_max: u32,
    pub(super) codex_reasoning_guard_concurrent_interval_ms: u32,
    pub(super) codex_reasoning_guard_concurrent_max_attempts: u32,
    pub(super) codex_reasoning_guard_model_fallbacks: Vec<String>,
    pub(super) codex_reasoning_guard_continuation_max_output_tokens: u32,
    pub(super) max_attempts_per_provider: u32,
    pub(super) max_providers_to_try: u32,
    pub(super) upstream_retry_policy: crate::settings::UpstreamRetryPolicy,
    pub(super) provider_cooldown_secs: i64,
    pub(super) upstream_first_byte_timeout_secs: u32,
    pub(super) upstream_first_byte_timeout: Option<Duration>,
    pub(super) upstream_stream_idle_timeout: Option<Duration>,
    pub(super) upstream_request_timeout_non_streaming: Option<Duration>,
    pub(super) fingerprint_key: u64,
    pub(super) fingerprint_debug: String,
    pub(super) unavailable_fingerprint_key: u64,
    pub(super) unavailable_fingerprint_debug: String,
    pub(super) abort_guard: RequestAbortGuard<R>,
    pub(super) enable_thinking_signature_rectifier: bool,
    pub(super) enable_thinking_budget_rectifier: bool,
    pub(super) enable_claude_metadata_user_id_injection: bool,
    #[allow(dead_code)]
    pub(super) cx2cc_settings: super::cx2cc::settings::Cx2ccSettings,
    pub(super) enable_response_fixer: bool,
    pub(super) response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) response_fixer_non_stream_config: response_fixer::ResponseFixerConfig,
}

impl<R: tauri::Runtime> RequestContext<R> {
    pub(super) fn clone_for_concurrent_probe(&self) -> Self {
        let mut abort_guard = RequestAbortGuard::new(
            self.state.app.clone(),
            self.state.db.clone(),
            self.state.log_tx.clone(),
            self.state.plugin_pipeline.clone(),
            self.state.active_requests.clone(),
            self.trace_id.clone(),
            self.cli_key.clone(),
            self.method_hint.clone(),
            self.forwarded_path.clone(),
            self.observe_request,
            self.query.clone(),
            self.session_id.clone(),
            self.requested_model.clone(),
            self.created_at_ms,
            self.created_at,
            self.started,
        );
        abort_guard.disarm();

        Self {
            state: self.state.clone(),
            cli_key: self.cli_key.clone(),
            forwarded_path: self.forwarded_path.clone(),
            observe_request: self.observe_request,
            req_method: self.req_method.clone(),
            method_hint: self.method_hint.clone(),
            query: self.query.clone(),
            trace_id: self.trace_id.clone(),
            started: self.started,
            created_at_ms: self.created_at_ms,
            created_at: self.created_at,
            session_id: self.session_id.clone(),
            requested_model: self.requested_model.clone(),
            requested_model_location: self.requested_model_location,
            effective_sort_mode_id: self.effective_sort_mode_id,
            providers: self.providers.clone(),
            session_bound_provider_id: self.session_bound_provider_id,
            base_headers: self.base_headers.clone(),
            body_bytes: self.body_bytes.clone(),
            request_body_state: self.request_body_state.clone(),
            introspection_json: self.introspection_json.clone(),
            strip_request_content_encoding_seed: self.strip_request_content_encoding_seed,
            special_settings: Arc::clone(&self.special_settings),
            provider_base_url_ping_cache_ttl_seconds: self.provider_base_url_ping_cache_ttl_seconds,
            verbose_provider_error: self.verbose_provider_error,
            enable_codex_session_id_completion: self.enable_codex_session_id_completion,
            codex_reasoning_guard_enabled: self.codex_reasoning_guard_enabled,
            codex_reasoning_guard_rule_mode: self.codex_reasoning_guard_rule_mode,
            codex_reasoning_guard_compare_mode: self.codex_reasoning_guard_compare_mode,
            codex_reasoning_guard_reasoning_equals: self
                .codex_reasoning_guard_reasoning_equals
                .clone(),
            codex_reasoning_guard_model_rules: self.codex_reasoning_guard_model_rules.clone(),
            codex_reasoning_guard_active_template_id: self
                .codex_reasoning_guard_active_template_id
                .clone(),
            codex_reasoning_guard_custom_templates: self
                .codex_reasoning_guard_custom_templates
                .clone(),
            codex_reasoning_guard_post_match_strategy: self
                .codex_reasoning_guard_post_match_strategy,
            codex_reasoning_guard_immediate_retry_budget: self
                .codex_reasoning_guard_immediate_retry_budget,
            codex_reasoning_guard_delayed_retry_budget: self
                .codex_reasoning_guard_delayed_retry_budget,
            codex_reasoning_guard_delayed_retry_ms: self.codex_reasoning_guard_delayed_retry_ms,
            codex_reasoning_guard_exhausted_action: self.codex_reasoning_guard_exhausted_action,
            codex_reasoning_guard_retry_policy: self.codex_reasoning_guard_retry_policy,
            codex_reasoning_guard_concurrent_max: self.codex_reasoning_guard_concurrent_max,
            codex_reasoning_guard_concurrent_interval_ms: self
                .codex_reasoning_guard_concurrent_interval_ms,
            codex_reasoning_guard_concurrent_max_attempts: self
                .codex_reasoning_guard_concurrent_max_attempts,
            codex_reasoning_guard_model_fallbacks: self
                .codex_reasoning_guard_model_fallbacks
                .clone(),
            codex_reasoning_guard_continuation_max_output_tokens: self
                .codex_reasoning_guard_continuation_max_output_tokens,
            max_attempts_per_provider: self.max_attempts_per_provider,
            max_providers_to_try: self.max_providers_to_try,
            upstream_retry_policy: self.upstream_retry_policy.clone(),
            provider_cooldown_secs: self.provider_cooldown_secs,
            upstream_first_byte_timeout_secs: self.upstream_first_byte_timeout_secs,
            upstream_first_byte_timeout: self.upstream_first_byte_timeout,
            upstream_stream_idle_timeout: self.upstream_stream_idle_timeout,
            upstream_request_timeout_non_streaming: self.upstream_request_timeout_non_streaming,
            fingerprint_key: self.fingerprint_key,
            fingerprint_debug: self.fingerprint_debug.clone(),
            unavailable_fingerprint_key: self.unavailable_fingerprint_key,
            unavailable_fingerprint_debug: self.unavailable_fingerprint_debug.clone(),
            abort_guard,
            enable_thinking_signature_rectifier: self.enable_thinking_signature_rectifier,
            enable_thinking_budget_rectifier: self.enable_thinking_budget_rectifier,
            enable_claude_metadata_user_id_injection: self.enable_claude_metadata_user_id_injection,
            cx2cc_settings: self.cx2cc_settings.clone(),
            enable_response_fixer: self.enable_response_fixer,
            response_fixer_stream_config: self.response_fixer_stream_config,
            response_fixer_non_stream_config: self.response_fixer_non_stream_config,
        }
    }

    pub(super) fn from_handler_parts(parts: RequestContextParts<R>) -> Self {
        let RequestContextParts {
            state,
            cli_key,
            forwarded_path,
            observe_request,
            req_method,
            method_hint,
            query,
            trace_id,
            started,
            created_at_ms,
            created_at,
            session_id,
            requested_model,
            requested_model_location,
            effective_sort_mode_id,
            providers,
            session_bound_provider_id,
            headers,
            body_bytes,
            request_body_state,
            introspection_json,
            strip_request_content_encoding_seed,
            special_settings,
            provider_base_url_ping_cache_ttl_seconds,
            verbose_provider_error,
            enable_codex_session_id_completion,
            codex_reasoning_guard_enabled,
            codex_reasoning_guard_rule_mode,
            codex_reasoning_guard_compare_mode,
            codex_reasoning_guard_reasoning_equals,
            codex_reasoning_guard_model_rules,
            codex_reasoning_guard_active_template_id,
            codex_reasoning_guard_custom_templates,
            codex_reasoning_guard_post_match_strategy,
            codex_reasoning_guard_immediate_retry_budget,
            codex_reasoning_guard_delayed_retry_budget,
            codex_reasoning_guard_delayed_retry_ms,
            codex_reasoning_guard_exhausted_action,
            codex_reasoning_guard_retry_policy,
            codex_reasoning_guard_concurrent_max,
            codex_reasoning_guard_concurrent_interval_ms,
            codex_reasoning_guard_concurrent_max_attempts,
            codex_reasoning_guard_model_fallbacks,
            codex_reasoning_guard_continuation_max_output_tokens,
            max_attempts_per_provider,
            max_providers_to_try,
            upstream_retry_policy,
            provider_cooldown_secs,
            upstream_first_byte_timeout_secs,
            upstream_stream_idle_timeout_secs,
            upstream_request_timeout_non_streaming_secs,
            fingerprint_key,
            fingerprint_debug,
            unavailable_fingerprint_key,
            unavailable_fingerprint_debug,
            enable_thinking_signature_rectifier,
            enable_thinking_budget_rectifier,
            enable_claude_metadata_user_id_injection,
            cx2cc_settings,
            enable_response_fixer,
            response_fixer_stream_config,
            response_fixer_non_stream_config,
        } = parts;

        let max_attempts_per_provider = Self::normalize_max_attempts_per_provider(
            &cli_key,
            enable_thinking_signature_rectifier,
            enable_thinking_budget_rectifier,
            max_attempts_per_provider,
        );
        let (
            upstream_first_byte_timeout,
            upstream_stream_idle_timeout,
            upstream_request_timeout_non_streaming,
        ) = Self::upstream_timeouts_from_secs(
            upstream_first_byte_timeout_secs,
            upstream_stream_idle_timeout_secs,
            upstream_request_timeout_non_streaming_secs,
        );

        let abort_guard = RequestAbortGuard::new(
            state.app.clone(),
            state.db.clone(),
            state.log_tx.clone(),
            state.plugin_pipeline.clone(),
            state.active_requests.clone(),
            trace_id.clone(),
            cli_key.clone(),
            method_hint.clone(),
            forwarded_path.clone(),
            observe_request,
            query.clone(),
            session_id.clone(),
            requested_model.clone(),
            created_at_ms,
            created_at,
            started,
        );

        let base_headers = build_base_headers(headers);

        Self {
            state,
            cli_key,
            forwarded_path,
            observe_request,
            req_method,
            method_hint,
            query,
            trace_id,
            started,
            created_at_ms,
            created_at,
            session_id,
            requested_model,
            requested_model_location,
            effective_sort_mode_id,
            providers,
            session_bound_provider_id,
            base_headers,
            body_bytes,
            request_body_state,
            introspection_json,
            strip_request_content_encoding_seed,
            special_settings,
            provider_base_url_ping_cache_ttl_seconds,
            verbose_provider_error,
            enable_codex_session_id_completion,
            codex_reasoning_guard_enabled,
            codex_reasoning_guard_rule_mode,
            codex_reasoning_guard_compare_mode,
            codex_reasoning_guard_reasoning_equals,
            codex_reasoning_guard_model_rules,
            codex_reasoning_guard_active_template_id,
            codex_reasoning_guard_custom_templates,
            codex_reasoning_guard_post_match_strategy,
            codex_reasoning_guard_immediate_retry_budget,
            codex_reasoning_guard_delayed_retry_budget,
            codex_reasoning_guard_delayed_retry_ms,
            codex_reasoning_guard_exhausted_action,
            codex_reasoning_guard_retry_policy,
            codex_reasoning_guard_concurrent_max,
            codex_reasoning_guard_concurrent_interval_ms,
            codex_reasoning_guard_concurrent_max_attempts,
            codex_reasoning_guard_model_fallbacks,
            codex_reasoning_guard_continuation_max_output_tokens,
            max_attempts_per_provider,
            max_providers_to_try,
            upstream_retry_policy,
            provider_cooldown_secs,
            upstream_first_byte_timeout_secs,
            upstream_first_byte_timeout,
            upstream_stream_idle_timeout,
            upstream_request_timeout_non_streaming,
            fingerprint_key,
            fingerprint_debug,
            unavailable_fingerprint_key,
            unavailable_fingerprint_debug,
            abort_guard,
            enable_thinking_signature_rectifier,
            enable_thinking_budget_rectifier,
            enable_claude_metadata_user_id_injection,
            cx2cc_settings,
            enable_response_fixer,
            response_fixer_stream_config,
            response_fixer_non_stream_config,
        }
    }

    fn normalize_max_attempts_per_provider(
        cli_key: &str,
        enable_thinking_signature_rectifier: bool,
        enable_thinking_budget_rectifier: bool,
        max_attempts_per_provider: u32,
    ) -> u32 {
        if cli_key == "claude"
            && (enable_thinking_signature_rectifier || enable_thinking_budget_rectifier)
        {
            max_attempts_per_provider.max(2)
        } else {
            max_attempts_per_provider
        }
    }

    fn upstream_timeouts_from_secs(
        upstream_first_byte_timeout_secs: u32,
        upstream_stream_idle_timeout_secs: u32,
        upstream_request_timeout_non_streaming_secs: u32,
    ) -> (Option<Duration>, Option<Duration>, Option<Duration>) {
        // Values below 60s cause premature stream disconnects during long AI thinking phases.
        let clamped_idle = if upstream_stream_idle_timeout_secs > 0 {
            upstream_stream_idle_timeout_secs.max(MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS)
        } else {
            0
        };
        (
            duration_from_secs(upstream_first_byte_timeout_secs),
            duration_from_secs(clamped_idle),
            duration_from_secs(upstream_request_timeout_non_streaming_secs),
        )
    }
}

fn build_base_headers(mut headers: HeaderMap) -> HeaderMap {
    strip_hop_headers(&mut headers);
    headers.remove(header::HOST);
    headers.remove(header::CONTENT_LENGTH);
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("identity"),
    );
    headers
}

fn duration_from_secs(secs: u32) -> Option<Duration> {
    if secs == 0 {
        None
    } else {
        Some(Duration::from_secs(secs as u64))
    }
}

pub(super) struct RequestContextParts<R: tauri::Runtime = tauri::Wry> {
    pub(super) state: GatewayAppState<R>,
    pub(super) cli_key: String,
    pub(super) forwarded_path: String,
    pub(super) observe_request: bool,
    pub(super) req_method: Method,
    pub(super) method_hint: String,
    pub(super) query: Option<String>,
    pub(super) trace_id: String,
    pub(super) started: Instant,
    pub(super) created_at_ms: i64,
    pub(super) created_at: i64,
    pub(super) session_id: Option<String>,
    pub(super) requested_model: Option<String>,
    pub(super) requested_model_location: Option<RequestedModelLocation>,
    pub(super) effective_sort_mode_id: Option<i64>,
    pub(super) providers: Vec<providers::ProviderForGateway>,
    pub(super) session_bound_provider_id: Option<i64>,
    pub(super) headers: HeaderMap,
    pub(super) body_bytes: Bytes,
    pub(super) request_body_state: GatewayRequestBody,
    pub(super) introspection_json: Option<serde_json::Value>,
    pub(super) strip_request_content_encoding_seed: bool,
    pub(super) special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
    pub(super) provider_base_url_ping_cache_ttl_seconds: u32,
    pub(super) verbose_provider_error: bool,
    pub(super) enable_codex_session_id_completion: bool,
    pub(super) codex_reasoning_guard_enabled: bool,
    pub(super) codex_reasoning_guard_rule_mode: crate::settings::CodexReasoningGuardRuleMode,
    pub(super) codex_reasoning_guard_compare_mode: crate::settings::CodexReasoningGuardCompareMode,
    pub(super) codex_reasoning_guard_reasoning_equals: Vec<i64>,
    pub(super) codex_reasoning_guard_model_rules:
        Vec<crate::settings::CodexReasoningGuardModelRule>,
    pub(super) codex_reasoning_guard_active_template_id: String,
    pub(super) codex_reasoning_guard_custom_templates:
        Vec<crate::settings::CodexReasoningGuardRuleTemplate>,
    pub(super) codex_reasoning_guard_post_match_strategy:
        crate::settings::CodexReasoningGuardPostMatchStrategy,
    pub(super) codex_reasoning_guard_immediate_retry_budget: u32,
    pub(super) codex_reasoning_guard_delayed_retry_budget: u32,
    pub(super) codex_reasoning_guard_delayed_retry_ms: u32,
    pub(super) codex_reasoning_guard_exhausted_action:
        crate::settings::CodexReasoningGuardExhaustedAction,
    pub(super) codex_reasoning_guard_retry_policy: crate::settings::CodexReasoningGuardRetryPolicy,
    pub(super) codex_reasoning_guard_concurrent_max: u32,
    pub(super) codex_reasoning_guard_concurrent_interval_ms: u32,
    pub(super) codex_reasoning_guard_concurrent_max_attempts: u32,
    pub(super) codex_reasoning_guard_model_fallbacks: Vec<String>,
    pub(super) codex_reasoning_guard_continuation_max_output_tokens: u32,
    pub(super) max_attempts_per_provider: u32,
    pub(super) max_providers_to_try: u32,
    pub(super) upstream_retry_policy: crate::settings::UpstreamRetryPolicy,
    pub(super) provider_cooldown_secs: i64,
    pub(super) upstream_first_byte_timeout_secs: u32,
    pub(super) upstream_stream_idle_timeout_secs: u32,
    pub(super) upstream_request_timeout_non_streaming_secs: u32,
    pub(super) fingerprint_key: u64,
    pub(super) fingerprint_debug: String,
    pub(super) unavailable_fingerprint_key: u64,
    pub(super) unavailable_fingerprint_debug: String,
    pub(super) enable_thinking_signature_rectifier: bool,
    pub(super) enable_thinking_budget_rectifier: bool,
    pub(super) enable_claude_metadata_user_id_injection: bool,
    pub(super) cx2cc_settings: super::cx2cc::settings::Cx2ccSettings,
    pub(super) enable_response_fixer: bool,
    pub(super) response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) response_fixer_non_stream_config: response_fixer::ResponseFixerConfig,
}
