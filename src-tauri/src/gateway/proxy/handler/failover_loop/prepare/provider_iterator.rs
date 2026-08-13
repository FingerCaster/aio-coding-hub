//! Usage: Provider traversal + skip logic (gate, credential, OAuth, CX2CC, Gemini, Codex).
//!
//! Encapsulates all per-provider preparation that runs before the retry loop.

use super::provider_checks;
use super::*;
use crate::gateway::events::ClaudeModelMapping;
use crate::gateway::proxy::gemini_oauth::GeminiOAuthResponseMode;
use std::collections::HashSet;

/// All mutable state accumulated by the provider preparation phase that the
/// retry loop (and later finalization) needs.
#[derive(Clone)]
pub(super) struct PreparedProvider {
    pub(super) provider_id: i64,
    pub(super) provider_uuid: String,
    pub(super) provider_name_base: String,
    pub(super) provider_base_url_base: String,
    pub(super) provider_base_url_display: String,
    pub(super) auth_mode: String,
    pub(super) provider_index: u32,
    pub(super) provider_bridged: bool,
    pub(super) session_reuse: Option<bool>,
    pub(super) effective_credential: String,
    pub(super) provider_base_max_attempts: u32,
    pub(super) provider_max_attempts: u32,
    pub(super) oauth_adapter:
        Option<&'static dyn crate::gateway::oauth::provider_trait::OAuthProvider>,
    pub(super) upstream_forwarded_path: String,
    pub(super) upstream_query: Option<String>,
    pub(super) upstream_body_bytes: Bytes,
    pub(super) active_requested_model: Option<String>,
    pub(super) configured_model_route:
        Option<crate::gateway::configured_model_route::ConfiguredModelRoute>,
    pub(super) strip_request_content_encoding: bool,
    pub(super) request_body_mutated_before_attempt: bool,
    pub(super) gemini_oauth_response_mode: Option<GeminiOAuthResponseMode>,
    pub(super) use_codex_chatgpt_backend: bool,
    pub(super) codex_chatgpt_account_id: Option<String>,
    pub(super) cx2cc_active: bool,
    pub(super) active_bridge_type: Option<String>,
    pub(super) bridge_source: Option<(crate::providers::ProviderForGateway, String)>,
    pub(super) cx2cc_source: Option<(crate::providers::ProviderForGateway, String)>,
    pub(super) cx2cc_codex_session_id: Option<String>,
    pub(super) internal_codex_reentry: Option<cx2cc_preparation::InternalCodexReentry>,
    pub(super) circuit_snapshot: crate::circuit_breaker::CircuitSnapshot,
    pub(super) dispatch_ownership:
        Option<std::sync::Arc<crate::gateway::proxy::dispatch::ProviderDispatchOwnership>>,
    pub(super) anthropic_stream_requested: bool,
    pub(super) stream_idle_timeout_seconds: Option<u32>,
    pub(super) upstream_retry_policy: crate::settings::UpstreamRetryPolicy,
    pub(super) claude_model_mapping: Option<ClaudeModelMapping>,
}

/// Counters accumulated across all providers in the iteration loop.
pub(super) struct IterationCounters {
    pub(super) providers_tried: usize,
    ready_limit: usize,
    pub(super) earliest_available_unix: Option<i64>,
    pub(super) skipped_open: usize,
    pub(super) skipped_cooldown: usize,
    pub(super) skipped_limits: usize,
    pub(super) skipped_account_usage: usize,
}

impl IterationCounters {
    pub(super) fn new(ready_limit: usize) -> Self {
        Self {
            providers_tried: 0,
            ready_limit,
            earliest_available_unix: None,
            skipped_open: 0,
            skipped_cooldown: 0,
            skipped_limits: 0,
            skipped_account_usage: 0,
        }
    }

    pub(super) fn release_ready_slot(&mut self) {
        self.providers_tried = self.providers_tried.saturating_sub(1);
    }
}

pub(super) enum PreparationOutcome {
    Ready(Box<PreparedProvider>),
    ReadyLimitReached,
    Skipped,
}

fn configured_model_route_for_request<T>(
    cx2cc_active: bool,
    trusted_internal_reentry: bool,
    resolve: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if cx2cc_active || trusted_internal_reentry {
        None
    } else {
        resolve()
    }
}

/// Structured skip reason used by CX2CC preparation and other skip paths.
pub(super) struct SkipReason {
    pub(super) error_category: &'static str,
    pub(super) error_code: &'static str,
    pub(super) reason: String,
}

/// Prepare a single provider for the retry loop.
pub(super) async fn prepare_provider<R: tauri::Runtime>(
    ctx: CommonCtx<'_, R>,
    input: &RequestContext<R>,
    provider: &crate::providers::ProviderForGateway,
    counters: &mut IterationCounters,
    attempts: &mut Vec<FailoverAttempt>,
    failed_provider_ids: &HashSet<i64>,
    anthropic_stream_requested: bool,
) -> PreparationOutcome {
    let provider_id = provider.id;
    let provider_name_base = if provider.name.trim().is_empty() {
        format!("Provider #{} (auto-fixed)", provider.id)
    } else {
        provider.name.clone()
    };
    let provider_base_url_display = provider
        .base_urls
        .first()
        .cloned()
        .unwrap_or_else(String::new);

    if failed_provider_ids.contains(&provider_id) {
        return PreparationOutcome::Skipped;
    }

    let identity = provider_checks::ProviderIdentity {
        provider_id,
        provider_name_base: &provider_name_base,
        provider_base_url_display: &provider_base_url_display,
    };
    let gate_allow =
        match provider_checks::run_gates(ctx, input, provider, &identity, counters, attempts) {
            Some(allow) => allow,
            None => return PreparationOutcome::Skipped,
        };

    let bridge_type = provider.bridge_type.as_deref();
    let is_cx2cc_bridge = provider.is_cx2cc_bridge();
    let provider_bridged =
        crate::providers::has_bridged_input_semantics(provider.source_provider_id, bridge_type);

    let mut effective_credential = if is_cx2cc_bridge {
        String::new()
    } else {
        match resolve_effective_credential(&input.state, &input.cli_key, provider).await {
            Ok(value) => value,
            Err(err) => {
                provider_checks::skip_with_reason(
                    attempts,
                    provider_id,
                    &provider_name_base,
                    &provider_base_url_display,
                    input.started.elapsed().as_millis(),
                    SkipReason {
                        error_category: "auth",
                        error_code: GatewayErrorCode::InternalError.as_str(),
                        reason: format!("provider skipped by credential resolution: {err}"),
                    },
                );
                return PreparationOutcome::Skipped;
            }
        }
    };

    let upstream_retry_policy = effective_upstream_retry_policy(
        &input.upstream_retry_policy,
        provider.upstream_retry_policy_override.as_ref(),
    );

    let needs_oauth_reactive_refresh_retry = provider.auth_mode == "oauth";
    let needs_codex_previous_response_id_retry = codex_request_has_previous_response_id(input);
    let provider_base_max_attempts = provider_max_attempts_for_request(
        input.max_attempts_per_provider,
        needs_oauth_reactive_refresh_retry,
        needs_codex_previous_response_id_retry,
        0,
        input.is_codex_model_discovery,
    );
    let provider_max_attempts = provider_max_attempts_for_request(
        input.max_attempts_per_provider,
        needs_oauth_reactive_refresh_retry,
        needs_codex_previous_response_id_retry,
        configured_transient_retry_budget(&upstream_retry_policy),
        input.is_codex_model_discovery,
    );

    let mut provider_base_url_base = match provider_checks::resolve_base_url(
        input,
        provider,
        provider_id,
        &provider_name_base,
        &provider_base_url_display,
        attempts,
    )
    .await
    {
        Some(url) => url,
        None => return PreparationOutcome::Skipped,
    };

    let mut use_codex_chatgpt_backend =
        is_codex_chatgpt_backend(&input.cli_key, provider, &provider_base_url_base);
    let mut codex_chatgpt_account_id = if use_codex_chatgpt_backend {
        provider_checks::extract_codex_chatgpt_account_id(&input.state.db, provider.id)
    } else {
        None
    };

    let oauth_adapter = match provider_checks::resolve_oauth(
        input,
        provider,
        provider_id,
        &provider_name_base,
        &provider_base_url_display,
        attempts,
    ) {
        Some(adapter) => adapter,
        None => return PreparationOutcome::Skipped,
    };

    let mut upstream_forwarded_path = input.forwarded_path.clone();
    let mut upstream_query = input.query.clone();
    let mut upstream_body_bytes = input.request_body_state.decoded_clone();
    let mut strip_request_content_encoding = input.strip_request_content_encoding_seed;
    let mut gemini_oauth_response_mode = None;

    if let Some(adapter) = &oauth_adapter {
        if adapter.provider_type() == "gemini_oauth" {
            match provider_checks::prepare_gemini_oauth(
                input,
                &effective_credential,
                &mut provider_base_url_base,
            )
            .await
            {
                Some(prepared) => {
                    upstream_forwarded_path = prepared.forwarded_path;
                    upstream_query = prepared.query;
                    upstream_body_bytes = prepared.body_bytes;
                    strip_request_content_encoding = prepared.strip_request_content_encoding;
                    gemini_oauth_response_mode = Some(prepared.response_mode);
                }
                None => {
                    provider_checks::skip_with_reason(
                        attempts,
                        provider_id,
                        &provider_name_base,
                        &provider_base_url_display,
                        input.started.elapsed().as_millis(),
                        SkipReason {
                            error_category: "auth",
                            error_code: GatewayErrorCode::InternalError.as_str(),
                            reason: "provider skipped by gemini oauth translation".into(),
                        },
                    );
                    return PreparationOutcome::Skipped;
                }
            }
        }
    }

    // --- CX2CC translation ---
    let mut cx2cc_active = false;
    let mut active_bridge_type: Option<String> = None;
    let mut bridge_source: Option<(crate::providers::ProviderForGateway, String)> = None;
    let mut cx2cc_source: Option<(crate::providers::ProviderForGateway, String)> = None;
    let mut cx2cc_codex_session_id: Option<String> = None;
    let mut internal_codex_reentry = None;
    if is_cx2cc_bridge {
        let outcome = cx2cc_preparation::prepare(cx2cc_preparation::Cx2ccPreparationInput {
            ctx,
            input,
            provider_id,
            provider_name_base: &provider_name_base,
            source_id: provider.source_provider_id,
            anthropic_stream_requested,
            upstream_body_bytes,
            use_codex_chatgpt_backend,
            codex_chatgpt_account_id,
        })
        .await;
        match outcome {
            cx2cc_preparation::Cx2ccOutcome::Ready(boxed) => {
                let result = *boxed;
                cx2cc_active = result.cx2cc_active;
                active_bridge_type = Some(crate::providers::CX2CC_BRIDGE_TYPE.to_string());
                cx2cc_source = result.cx2cc_source;
                bridge_source = cx2cc_source.clone();
                cx2cc_codex_session_id = result.cx2cc_codex_session_id;
                internal_codex_reentry = result.internal_codex_reentry;
                effective_credential = result.effective_credential;
                provider_base_url_base = result.provider_base_url_base;
                upstream_forwarded_path = result.upstream_forwarded_path;
                upstream_query = result.upstream_query;
                upstream_body_bytes = result.upstream_body_bytes;
                strip_request_content_encoding = result.strip_request_content_encoding;
                use_codex_chatgpt_backend = result.use_codex_chatgpt_backend;
                codex_chatgpt_account_id = result.codex_chatgpt_account_id;
            }
            cx2cc_preparation::Cx2ccOutcome::Skipped(reason) => {
                provider_checks::skip_with_reason(
                    attempts,
                    provider_id,
                    &provider_name_base,
                    &provider_base_url_display,
                    input.started.elapsed().as_millis(),
                    reason,
                );
                return PreparationOutcome::Skipped;
            }
        }
    }
    let circuit_snapshot = gate_allow.circuit_after;
    let dispatch_ownership = gate_allow.dispatch_ownership;
    if counters.providers_tried >= counters.ready_limit {
        return PreparationOutcome::ReadyLimitReached;
    }
    counters.providers_tried = counters.providers_tried.saturating_add(1);
    let provider_index = counters.providers_tried as u32;
    let session_reuse = match input.session_bound_provider_id {
        Some(id) => (id == provider_id && provider_index == 1).then_some(true),
        None => None,
    };
    let active_requested_model = input
        .managed_model_route
        .as_ref()
        .map(|route| route.remote_model_id.as_str())
        .or(input.requested_model.as_deref());
    let provider_ctx = ProviderCtx {
        provider_id,
        provider_name_base: &provider_name_base,
        provider_base_url_base: &provider_base_url_base,
        active_requested_model,
        auth_mode: provider.auth_mode.as_str(),
        provider_index,
        provider_bridged,
        session_reuse,
        provider_max_attempts,
        stream_idle_timeout_seconds: provider.stream_idle_timeout_seconds,
        upstream_retry_policy: &upstream_retry_policy,
        claude_model_mapping: None,
        dispatch_ownership: dispatch_ownership.as_ref(),
    };

    let mut claude_model_mapping = None;
    if should_apply_claude_model_mapping(cx2cc_active, &upstream_forwarded_path) {
        claude_model_mapping = claude_model_mapping::apply_if_needed(
            ctx,
            provider,
            provider_ctx,
            input.requested_model_location,
            input.introspection_json.as_ref(),
            claude_model_mapping::UpstreamRequestMut {
                forwarded_path: &mut upstream_forwarded_path,
                query: &mut upstream_query,
                body_bytes: &mut upstream_body_bytes,
                strip_request_content_encoding: &mut strip_request_content_encoding,
            },
        );
    }

    claude_metadata_user_id_injection::apply_if_needed(
        claude_metadata_user_id_injection::ApplyClaudeMetadataUserIdInjectionInput {
            ctx,
            provider_id,
            enabled: input.enable_claude_metadata_user_id_injection,
            session_id: input.session_id.as_deref(),
            base_headers: &input.base_headers,
            forwarded_path: upstream_forwarded_path.as_str(),
            upstream_body_bytes: &mut upstream_body_bytes,
            strip_request_content_encoding: &mut strip_request_content_encoding,
        },
    );

    if use_codex_chatgpt_backend {
        maybe_apply_codex_chatgpt_request_compat(
            &mut upstream_forwarded_path,
            &mut upstream_body_bytes,
            &mut strip_request_content_encoding,
        );
    }

    let configured_model_route = configured_model_route_for_request(
        cx2cc_active,
        input.trusted_internal_reentry.is_some(),
        || {
            crate::gateway::configured_model_route::resolve(
                &input.cli_key,
                &input.method_hint,
                &input.forwarded_path,
                input.requested_model.as_deref(),
                input.managed_model_route.is_some(),
                &input.model_routing_policy,
                provider.model_routing_policy_override.as_ref(),
                provider_id,
                &provider_name_base,
            )
        },
    );

    let request_body_mutated_before_attempt = input.request_body_state.is_mutated()
        || upstream_body_bytes != input.request_body_state.decoded_clone()
        || strip_request_content_encoding;

    PreparationOutcome::Ready(Box::new(PreparedProvider {
        provider_id,
        provider_uuid: provider.provider_uuid.clone(),
        provider_name_base,
        provider_base_url_base,
        provider_base_url_display,
        auth_mode: provider.auth_mode.clone(),
        provider_index,
        provider_bridged,
        session_reuse,
        effective_credential,
        provider_base_max_attempts,
        provider_max_attempts,
        oauth_adapter,
        upstream_forwarded_path,
        upstream_query,
        upstream_body_bytes,
        active_requested_model: active_requested_model.map(str::to_string),
        configured_model_route,
        strip_request_content_encoding,
        request_body_mutated_before_attempt,
        gemini_oauth_response_mode,
        use_codex_chatgpt_backend,
        codex_chatgpt_account_id,
        cx2cc_active,
        active_bridge_type,
        bridge_source,
        cx2cc_source,
        cx2cc_codex_session_id,
        internal_codex_reentry,
        circuit_snapshot,
        dispatch_ownership,
        anthropic_stream_requested,
        stream_idle_timeout_seconds: provider.stream_idle_timeout_seconds,
        upstream_retry_policy,
        claude_model_mapping,
    }))
}

fn codex_request_has_previous_response_id<R: tauri::Runtime>(input: &RequestContext<R>) -> bool {
    codex_body_has_previous_response_id(&input.cli_key, &input.body_bytes)
}

fn codex_body_has_previous_response_id(cli_key: &str, body: &[u8]) -> bool {
    // grok 与 codex 同走 OpenAI Responses API，rectifier 重试额度同样适用。
    if !matches!(cli_key, "codex" | "grok") {
        return false;
    }

    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|root| {
            root.get("previous_response_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .map(str::to_string)
        })
        .is_some_and(|value| !value.is_empty())
}

fn provider_max_attempts_for_request(
    configured_max_attempts: u32,
    needs_oauth_reactive_refresh_retry: bool,
    needs_codex_previous_response_id_retry: bool,
    configured_transient_retries: u32,
    strict_configured_limit: bool,
) -> u32 {
    if strict_configured_limit {
        return configured_max_attempts.max(1);
    }

    let required_internal_retries = u32::from(needs_oauth_reactive_refresh_retry)
        + u32::from(needs_codex_previous_response_id_retry)
        + configured_transient_retries;
    // Circuit failures accumulate across requests; the breaker threshold must
    // not silently expand this request's configured retry budget.
    configured_max_attempts.max(1 + required_internal_retries)
}

fn configured_transient_retry_budget(policy: &crate::settings::UpstreamRetryPolicy) -> u32 {
    if policy.enabled {
        policy.max_retries
    } else {
        0
    }
}

fn effective_upstream_retry_policy(
    global_policy: &crate::settings::UpstreamRetryPolicy,
    provider_override: Option<&crate::settings::UpstreamRetryPolicy>,
) -> crate::settings::UpstreamRetryPolicy {
    provider_override
        .cloned()
        .unwrap_or_else(|| global_policy.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        codex_body_has_previous_response_id, configured_model_route_for_request,
        configured_transient_retry_budget, effective_upstream_retry_policy,
        provider_max_attempts_for_request, IterationCounters,
    };
    use crate::settings::{UpstreamHttpRetryRule, UpstreamRetryPolicy, UpstreamTransportRetryKind};

    fn body(value: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&value).expect("serialize body")
    }

    #[test]
    fn releasing_ready_slot_is_saturating() {
        let mut counters = IterationCounters::new(1);
        counters.providers_tried = 1;

        counters.release_ready_slot();
        assert_eq!(counters.providers_tried, 0);

        counters.release_ready_slot();
        assert_eq!(counters.providers_tried, 0);
    }

    #[test]
    fn codex_request_has_previous_response_id_detects_codex_continuation() {
        let body = body(serde_json::json!({
            "previous_response_id": "resp_123"
        }));

        assert!(codex_body_has_previous_response_id("codex", &body));
        assert!(codex_body_has_previous_response_id("grok", &body));
    }

    #[test]
    fn codex_request_has_previous_response_id_ignores_other_cli_or_missing_value() {
        let with_previous = body(serde_json::json!({
            "previous_response_id": "resp_123"
        }));
        let without_previous = body(serde_json::json!({}));

        assert!(!codex_body_has_previous_response_id(
            "claude",
            &with_previous
        ));
        assert!(!codex_body_has_previous_response_id(
            "codex",
            &without_previous
        ));
        assert!(!codex_body_has_previous_response_id(
            "grok",
            &without_previous
        ));
    }

    #[test]
    fn provider_max_attempts_reserves_budget_for_internal_retries() {
        assert_eq!(
            provider_max_attempts_for_request(1, false, false, 0, false),
            1
        );
        assert_eq!(
            provider_max_attempts_for_request(1, true, false, 0, false),
            2
        );
        assert_eq!(
            provider_max_attempts_for_request(1, false, true, 0, false),
            2
        );
        assert_eq!(
            provider_max_attempts_for_request(1, true, true, 0, false),
            3
        );
        assert_eq!(
            provider_max_attempts_for_request(5, true, true, 0, false),
            5
        );
    }

    #[test]
    fn provider_max_attempts_reserves_budget_for_configured_transient_retries() {
        assert_eq!(
            provider_max_attempts_for_request(1, false, false, 1, false),
            2
        );
        assert_eq!(
            provider_max_attempts_for_request(1, true, true, 2, false),
            5
        );
    }

    #[test]
    fn provider_max_attempts_does_not_reserve_budget_for_disabled_transient_retries() {
        let disabled = UpstreamRetryPolicy {
            enabled: false,
            max_retries: 5,
            ..Default::default()
        };

        assert_eq!(configured_transient_retry_budget(&disabled), 0);
    }

    #[test]
    fn provider_max_attempts_preserves_explicit_cap_without_retry_reasons() {
        assert_eq!(
            provider_max_attempts_for_request(1, false, false, 0, false),
            1
        );
    }

    #[test]
    fn provider_max_attempts_honors_strict_request_limit() {
        assert_eq!(provider_max_attempts_for_request(1, true, true, 3, true), 1);
    }

    #[test]
    fn provider_retry_policy_inherits_or_replaces_the_complete_global_policy() {
        let global = UpstreamRetryPolicy::default();
        assert_eq!(effective_upstream_retry_policy(&global, None), global);

        let provider_override = UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![UpstreamHttpRetryRule::status_only(429)],
            transport_errors: vec![UpstreamTransportRetryKind::Connect],
            stream_internal_errors: crate::settings::UpstreamStreamInternalErrorPolicy {
                enabled: true,
                passthrough_keywords: Vec::new(),
                legacy_retry_keywords: Vec::new(),
            },
            max_retries: 3,
            backoff_ms: 750,
            counts_toward_circuit_breaker: true,
        };
        let effective = effective_upstream_retry_policy(&global, Some(&provider_override));
        assert_eq!(effective, provider_override);
        assert!(effective
            .stream_internal_errors
            .passthrough_keywords
            .is_empty());

        let explicitly_disabled = UpstreamRetryPolicy {
            enabled: false,
            ..provider_override
        };
        assert_eq!(
            effective_upstream_retry_policy(&global, Some(&explicitly_disabled)),
            explicitly_disabled
        );
    }

    #[test]
    fn cx2cc_first_hop_skips_configured_route_resolution() {
        let called = std::cell::Cell::new(false);
        let route = configured_model_route_for_request(true, false, || {
            called.set(true);
            Some("must-not-replace-mapper-model")
        });
        assert_eq!(route, None);
        assert!(!called.get());
    }

    #[test]
    fn trusted_reentry_skips_configured_route_resolution() {
        let called = std::cell::Cell::new(false);
        let route = configured_model_route_for_request(false, true, || {
            called.set(true);
            Some("must-not-replace-mapper-model")
        });
        assert_eq!(route, None);
        assert!(!called.get());
    }

    #[test]
    fn ordinary_request_keeps_configured_route_resolution() {
        assert_eq!(
            configured_model_route_for_request(false, false, || Some("configured-model")),
            Some("configured-model")
        );
    }
}
