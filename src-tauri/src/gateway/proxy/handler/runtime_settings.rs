//! Runtime settings resolution for the gateway proxy handler.

use crate::gateway::response_fixer;
use crate::settings;

pub(super) const DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER: u32 = 5;
pub(super) const DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY: u32 = 5;

#[derive(Debug, Clone)]
pub(super) struct HandlerRuntimeSettings {
    pub(super) verbose_provider_error: bool,
    pub(super) intercept_warmup: bool,
    pub(super) enable_thinking_signature_rectifier: bool,
    pub(super) enable_thinking_budget_rectifier: bool,
    pub(super) enable_billing_header_rectifier: bool,
    pub(super) cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings,
    pub(super) enable_response_fixer: bool,
    pub(super) response_fixer_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) response_fixer_non_stream_config: response_fixer::ResponseFixerConfig,
    pub(super) provider_base_url_ping_cache_ttl_seconds: u32,
    pub(super) enable_codex_session_id_completion: bool,
    pub(super) codex_reasoning_guard_enabled: bool,
    pub(super) codex_reasoning_guard_rule_mode: settings::CodexReasoningGuardRuleMode,
    pub(super) codex_reasoning_guard_compare_mode: settings::CodexReasoningGuardCompareMode,
    pub(super) codex_reasoning_guard_reasoning_equals: Vec<i64>,
    pub(super) codex_reasoning_guard_model_rules: Vec<settings::CodexReasoningGuardModelRule>,
    pub(super) codex_reasoning_guard_active_template_id: String,
    pub(super) codex_reasoning_guard_custom_templates:
        Vec<settings::CodexReasoningGuardRuleTemplate>,
    pub(super) codex_reasoning_guard_post_match_strategy:
        settings::CodexReasoningGuardPostMatchStrategy,
    pub(super) codex_reasoning_guard_immediate_retry_budget: u32,
    pub(super) codex_reasoning_guard_delayed_retry_budget: u32,
    pub(super) codex_reasoning_guard_delayed_retry_ms: u32,
    pub(super) codex_reasoning_guard_exhausted_action: settings::CodexReasoningGuardExhaustedAction,
    pub(super) codex_reasoning_guard_retry_policy: settings::CodexReasoningGuardRetryPolicy,
    pub(super) codex_reasoning_guard_concurrent_max: u32,
    pub(super) codex_reasoning_guard_concurrent_interval_ms: u32,
    pub(super) codex_reasoning_guard_concurrent_max_attempts: u32,
    pub(super) codex_reasoning_guard_model_fallbacks: Vec<String>,
    pub(super) codex_reasoning_guard_continuation_max_output_tokens: u32,
    pub(super) enable_claude_metadata_user_id_injection: bool,
    pub(super) max_attempts_per_provider: u32,
    pub(super) max_providers_to_try: u32,
    pub(super) upstream_retry_policy: settings::UpstreamRetryPolicy,
    pub(super) provider_cooldown_secs: i64,
    pub(super) upstream_first_byte_timeout_secs: u32,
    pub(super) upstream_stream_idle_timeout_secs: u32,
    pub(super) upstream_request_timeout_non_streaming_secs: u32,
}

pub(super) fn handler_runtime_settings(
    settings_cfg: Option<&settings::AppSettings>,
    is_claude_count_tokens: bool,
) -> HandlerRuntimeSettings {
    let verbose_provider_error = settings_cfg
        .map(|cfg| cfg.verbose_provider_error)
        .unwrap_or(true);

    let enable_thinking_signature_rectifier = settings_cfg
        .map(|cfg| cfg.enable_thinking_signature_rectifier)
        .unwrap_or(true)
        && !is_claude_count_tokens;

    let enable_thinking_budget_rectifier = settings_cfg
        .map(|cfg| cfg.enable_thinking_budget_rectifier)
        .unwrap_or(true)
        && !is_claude_count_tokens;
    let enable_billing_header_rectifier = settings_cfg
        .map(|cfg| cfg.enable_billing_header_rectifier)
        .unwrap_or(true);
    let cx2cc_settings = settings_cfg
        .map(crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::from_app_settings)
        .unwrap_or_default();

    let enable_response_fixer = settings_cfg
        .map(|cfg| cfg.enable_response_fixer)
        .unwrap_or(true);
    let response_fixer_fix_encoding = settings_cfg
        .map(|cfg| cfg.response_fixer_fix_encoding)
        .unwrap_or(true);
    let response_fixer_fix_sse_format = settings_cfg
        .map(|cfg| cfg.response_fixer_fix_sse_format)
        .unwrap_or(true);
    let response_fixer_fix_truncated_json = settings_cfg
        .map(|cfg| cfg.response_fixer_fix_truncated_json)
        .unwrap_or(true);
    let response_fixer_max_json_depth = settings_cfg
        .map(|cfg| cfg.response_fixer_max_json_depth)
        .unwrap_or(response_fixer::DEFAULT_MAX_JSON_DEPTH as u32);
    let response_fixer_max_fix_size = settings_cfg
        .map(|cfg| cfg.response_fixer_max_fix_size)
        .unwrap_or(response_fixer::DEFAULT_MAX_FIX_SIZE as u32);

    let mut max_attempts_per_provider = settings_cfg
        .map(|cfg| cfg.failover_max_attempts_per_provider.max(1))
        .unwrap_or(DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER);
    let mut max_providers_to_try = settings_cfg
        .map(|cfg| cfg.failover_max_providers_to_try.max(1))
        .unwrap_or(DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY);

    if is_claude_count_tokens {
        max_attempts_per_provider = 1;
        max_providers_to_try = 1;
    }

    HandlerRuntimeSettings {
        verbose_provider_error,
        intercept_warmup: settings_cfg
            .map(|cfg| cfg.intercept_anthropic_warmup_requests)
            .unwrap_or(false),
        enable_thinking_signature_rectifier,
        enable_thinking_budget_rectifier,
        enable_billing_header_rectifier,
        cx2cc_settings,
        enable_response_fixer,
        response_fixer_stream_config: response_fixer::ResponseFixerConfig {
            fix_encoding: response_fixer_fix_encoding,
            fix_sse_format: response_fixer_fix_sse_format,
            fix_truncated_json: response_fixer_fix_truncated_json,
            max_json_depth: response_fixer_max_json_depth as usize,
            max_fix_size: response_fixer_max_fix_size as usize,
        },
        response_fixer_non_stream_config: response_fixer::ResponseFixerConfig {
            fix_encoding: response_fixer_fix_encoding,
            fix_sse_format: false,
            fix_truncated_json: response_fixer_fix_truncated_json,
            max_json_depth: response_fixer_max_json_depth as usize,
            max_fix_size: response_fixer_max_fix_size as usize,
        },
        provider_base_url_ping_cache_ttl_seconds: settings_cfg
            .map(|cfg| cfg.provider_base_url_ping_cache_ttl_seconds)
            .unwrap_or(settings::DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS),
        enable_codex_session_id_completion: settings_cfg
            .map(|cfg| cfg.enable_codex_session_id_completion)
            .unwrap_or(true),
        codex_reasoning_guard_enabled: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_enabled)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_ENABLED),
        codex_reasoning_guard_rule_mode: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_rule_mode)
            .unwrap_or_default(),
        codex_reasoning_guard_compare_mode: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_compare_mode)
            .unwrap_or_default(),
        codex_reasoning_guard_reasoning_equals: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_reasoning_equals.clone())
            .unwrap_or_else(|| settings::DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS.to_vec()),
        codex_reasoning_guard_model_rules: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_model_rules.clone())
            .unwrap_or_default(),
        codex_reasoning_guard_active_template_id: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_active_template_id.clone())
            .unwrap_or_else(|| {
                settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string()
            }),
        codex_reasoning_guard_custom_templates: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_custom_templates.clone())
            .unwrap_or_default(),
        codex_reasoning_guard_post_match_strategy: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_post_match_strategy)
            .unwrap_or_default(),
        codex_reasoning_guard_immediate_retry_budget: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_immediate_retry_budget)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET),
        codex_reasoning_guard_delayed_retry_budget: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_delayed_retry_budget)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET),
        codex_reasoning_guard_delayed_retry_ms: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_delayed_retry_ms)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS),
        codex_reasoning_guard_exhausted_action: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_exhausted_action)
            .unwrap_or_default(),
        codex_reasoning_guard_retry_policy: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_retry_policy)
            .unwrap_or_default(),
        codex_reasoning_guard_concurrent_max: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_concurrent_max)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX),
        codex_reasoning_guard_concurrent_interval_ms: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_concurrent_interval_ms)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS),
        codex_reasoning_guard_concurrent_max_attempts: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_concurrent_max_attempts)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS),
        codex_reasoning_guard_model_fallbacks: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_model_fallbacks.clone())
            .unwrap_or_default(),
        codex_reasoning_guard_continuation_max_output_tokens: settings_cfg
            .map(|cfg| cfg.codex_reasoning_guard_continuation_max_output_tokens)
            .unwrap_or(settings::DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS),
        enable_claude_metadata_user_id_injection: settings_cfg
            .map(|cfg| cfg.enable_claude_metadata_user_id_injection)
            .unwrap_or(true)
            && !is_claude_count_tokens,
        max_attempts_per_provider,
        max_providers_to_try,
        upstream_retry_policy: settings_cfg
            .map(|cfg| cfg.upstream_retry_policy.clone())
            .unwrap_or_default(),
        provider_cooldown_secs: settings_cfg
            .map(|cfg| cfg.provider_cooldown_seconds as i64)
            .unwrap_or(settings::DEFAULT_PROVIDER_COOLDOWN_SECONDS as i64),
        upstream_first_byte_timeout_secs: settings_cfg
            .map(|cfg| cfg.upstream_first_byte_timeout_seconds)
            .unwrap_or(settings::DEFAULT_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS),
        upstream_stream_idle_timeout_secs: settings_cfg
            .map(|cfg| cfg.upstream_stream_idle_timeout_seconds)
            .unwrap_or(settings::DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS),
        upstream_request_timeout_non_streaming_secs: settings_cfg
            .map(|cfg| cfg.upstream_request_timeout_non_streaming_seconds)
            .unwrap_or(settings::DEFAULT_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_reasoning_guard_runtime_budget_uses_explicit_fields_only() {
        let settings = settings::AppSettings {
            codex_reasoning_guard_rule_mode:
                settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            codex_reasoning_guard_immediate_retry_budget: 2,
            codex_reasoning_guard_delayed_retry_budget: 3,
            codex_reasoning_guard_delayed_retry_ms: 4_000,
            codex_reasoning_guard_exhausted_action:
                settings::CodexReasoningGuardExhaustedAction::SwitchProvider,
            codex_reasoning_guard_retry_policy:
                settings::CodexReasoningGuardRetryPolicy::Concurrent,
            codex_reasoning_guard_concurrent_max: 4,
            codex_reasoning_guard_concurrent_interval_ms: 250,
            codex_reasoning_guard_concurrent_max_attempts: 12,
            codex_reasoning_guard_model_fallbacks: vec!["gpt-5.4".to_string()],
            codex_reasoning_guard_post_match_strategy:
                settings::CodexReasoningGuardPostMatchStrategy::RetrySameProvider,
            codex_reasoning_guard_continuation_max_output_tokens: 12_345,
            codex_reasoning_guard_backoff_after_hits: 99,
            codex_reasoning_guard_backoff_ms: 60_000,
            ..Default::default()
        };

        let runtime = handler_runtime_settings(Some(&settings), false);

        assert_eq!(
            runtime.codex_reasoning_guard_rule_mode,
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh
        );
        assert_eq!(runtime.codex_reasoning_guard_immediate_retry_budget, 2);
        assert_eq!(runtime.codex_reasoning_guard_delayed_retry_budget, 3);
        assert_eq!(runtime.codex_reasoning_guard_delayed_retry_ms, 4_000);
        assert_eq!(
            runtime.codex_reasoning_guard_exhausted_action,
            settings::CodexReasoningGuardExhaustedAction::SwitchProvider
        );
        assert_eq!(
            runtime.codex_reasoning_guard_retry_policy,
            settings::CodexReasoningGuardRetryPolicy::Concurrent
        );
        assert_eq!(runtime.codex_reasoning_guard_concurrent_max, 4);
        assert_eq!(runtime.codex_reasoning_guard_concurrent_interval_ms, 250);
        assert_eq!(runtime.codex_reasoning_guard_concurrent_max_attempts, 12);
        assert_eq!(
            runtime.codex_reasoning_guard_model_fallbacks,
            vec!["gpt-5.4".to_string()]
        );
        assert_eq!(
            runtime.codex_reasoning_guard_post_match_strategy,
            settings::CodexReasoningGuardPostMatchStrategy::RetrySameProvider
        );
        assert_eq!(
            runtime.codex_reasoning_guard_continuation_max_output_tokens,
            12_345
        );
    }

    #[test]
    fn codex_reasoning_guard_runtime_settings_snapshot_rule_collections() {
        let mut settings = settings::AppSettings {
            codex_reasoning_guard_rule_mode: settings::CodexReasoningGuardRuleMode::ReasoningTokens,
            codex_reasoning_guard_compare_mode:
                settings::CodexReasoningGuardCompareMode::LessThanOrEqual,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: vec![settings::CodexReasoningGuardModelRule {
                requested_model: "gpt-5.5".to_string(),
                compare_mode: settings::CodexReasoningGuardCompareMode::Equals,
                reasoning_equals: vec![516],
            }],
            codex_reasoning_guard_model_fallbacks: vec!["gpt-5.4".to_string()],
            codex_reasoning_guard_continuation_max_output_tokens: 8_000,
            ..Default::default()
        };

        let runtime = handler_runtime_settings(Some(&settings), false);
        settings.codex_reasoning_guard_rule_mode =
            settings::CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh;
        settings.codex_reasoning_guard_compare_mode =
            settings::CodexReasoningGuardCompareMode::Equals;
        settings.codex_reasoning_guard_reasoning_equals = vec![1024];
        settings.codex_reasoning_guard_model_rules.clear();
        settings.codex_reasoning_guard_model_fallbacks = vec!["gpt-5.3-codex".to_string()];
        settings.codex_reasoning_guard_continuation_max_output_tokens = 0;

        assert_eq!(
            runtime.codex_reasoning_guard_rule_mode,
            settings::CodexReasoningGuardRuleMode::ReasoningTokens
        );
        assert_eq!(
            runtime.codex_reasoning_guard_compare_mode,
            settings::CodexReasoningGuardCompareMode::LessThanOrEqual
        );
        assert_eq!(runtime.codex_reasoning_guard_reasoning_equals, vec![516]);
        assert_eq!(runtime.codex_reasoning_guard_model_rules.len(), 1);
        assert_eq!(
            runtime.codex_reasoning_guard_model_rules[0].requested_model,
            "gpt-5.5"
        );
        assert_eq!(
            runtime.codex_reasoning_guard_model_fallbacks,
            vec!["gpt-5.4".to_string()]
        );
        assert_eq!(
            runtime.codex_reasoning_guard_continuation_max_output_tokens,
            8_000
        );
    }
}
