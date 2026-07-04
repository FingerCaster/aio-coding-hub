//! Usage: Settings struct, field types, enums, and Default implementations.

use super::defaults::*;
use serde::{Deserialize, Serialize};

fn default_codex_provider_test_model() -> String {
    DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string()
}

fn default_codex_reasoning_guard_hit_label() -> String {
    DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL.to_string()
}

fn default_codex_reasoning_guard_active_template_id() -> String {
    CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID.to_string()
}

fn default_codex_reasoning_guard_immediate_retry_budget() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET
}

fn default_codex_reasoning_guard_delayed_retry_budget() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET
}

fn default_codex_reasoning_guard_delayed_retry_ms() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS
}

fn default_codex_reasoning_guard_concurrent_max() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX
}

fn default_codex_reasoning_guard_concurrent_interval_ms() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS
}

fn default_codex_reasoning_guard_concurrent_max_attempts() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS
}

fn default_codex_reasoning_guard_continuation_max_rounds() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS
}

fn default_codex_reasoning_guard_continuation_max_output_tokens() -> u32 {
    DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum GatewayListenMode {
    #[default]
    Localhost,
    WslAuto,
    Lan,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum WslHostAddressMode {
    #[default]
    Auto,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
pub enum HomeUsagePeriod {
    #[serde(rename = "last7")]
    #[specta(rename = "last7")]
    Last7,
    #[serde(rename = "last15")]
    #[specta(rename = "last15")]
    #[default]
    Last15,
    #[serde(rename = "last30")]
    #[specta(rename = "last30")]
    Last30,
    #[serde(rename = "month")]
    #[specta(rename = "month")]
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexHomeMode {
    #[default]
    UserHomeDefault,
    FollowCodexHome,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardCompareMode {
    #[default]
    Equals,
    LessThanOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardRuleMode {
    #[default]
    ReasoningTokens,
    FinalAnswerOnlyHighXhigh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardExhaustedAction {
    #[default]
    ReturnError,
    SwitchProvider,
    SwitchModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardRetryPolicy {
    #[default]
    Single,
    Concurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardPostMatchStrategy {
    RetrySameProvider,
    #[default]
    ContinuationRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardTemplateRuleAction {
    #[default]
    Intercept,
    NoIntercept,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardTemplateRuleFormula {
    #[default]
    ReasoningTokens518NMinus2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardTemplateRuleLogic {
    #[serde(rename = "and")]
    #[default]
    And,
    #[serde(rename = "or")]
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardTemplateFilterField {
    DurationMs,
    Tps,
    OutputTokens,
    InputTokens,
    TotalTokens,
    #[default]
    ReasoningTokens,
    FinalAnswerOnly,
    HasToolCall,
    HasReasoningItem,
    CommentaryObserved,
    RequestReasoningEffort,
    RequestedModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningGuardTemplateFilterOperator {
    #[default]
    Equals,
    NotEquals,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    In,
    NotIn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct CodexReasoningGuardTemplateFilter {
    pub id: String,
    pub field: CodexReasoningGuardTemplateFilterField,
    pub operator: CodexReasoningGuardTemplateFilterOperator,
    pub number_value: Option<f64>,
    pub bool_value: Option<bool>,
    pub string_value: Option<String>,
    pub string_values: Vec<String>,
}

impl Default for CodexReasoningGuardTemplateFilter {
    fn default() -> Self {
        Self {
            id: String::new(),
            field: CodexReasoningGuardTemplateFilterField::ReasoningTokens,
            operator: CodexReasoningGuardTemplateFilterOperator::Equals,
            number_value: None,
            bool_value: None,
            string_value: None,
            string_values: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct CodexReasoningGuardTemplateRule {
    pub id: String,
    pub name: String,
    pub reasoning_tokens: Option<i64>,
    pub reasoning_tokens_formula: Option<CodexReasoningGuardTemplateRuleFormula>,
    pub action: CodexReasoningGuardTemplateRuleAction,
    pub logic: CodexReasoningGuardTemplateRuleLogic,
    pub filters: Vec<CodexReasoningGuardTemplateFilter>,
}

impl Default for CodexReasoningGuardTemplateRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            reasoning_tokens: None,
            reasoning_tokens_formula: None,
            action: CodexReasoningGuardTemplateRuleAction::Intercept,
            logic: CodexReasoningGuardTemplateRuleLogic::And,
            filters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct CodexReasoningGuardRuleTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<CodexReasoningGuardTemplateRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamTransportRetryKind {
    Connect,
    Timeout,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct UpstreamRetryPolicy {
    pub enabled: bool,
    pub status_codes: Vec<u16>,
    pub transport_errors: Vec<UpstreamTransportRetryKind>,
    pub max_retries: u32,
    pub backoff_ms: u32,
    pub counts_toward_circuit_breaker: bool,
}

impl Default for UpstreamRetryPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            status_codes: vec![502, 503, 504],
            transport_errors: vec![
                UpstreamTransportRetryKind::Connect,
                UpstreamTransportRetryKind::Timeout,
                UpstreamTransportRetryKind::Read,
            ],
            max_retries: 1,
            backoff_ms: 100,
            counts_toward_circuit_breaker: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct CodexReasoningGuardModelRule {
    pub requested_model: String,
    #[serde(default)]
    pub compare_mode: CodexReasoningGuardCompareMode,
    pub reasoning_equals: Vec<i64>,
}

impl Default for CodexReasoningGuardModelRule {
    fn default() -> Self {
        Self {
            requested_model: String::new(),
            compare_mode: CodexReasoningGuardCompareMode::default(),
            reasoning_equals: DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default)]
pub struct WslTargetCli {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
}

impl Default for WslTargetCli {
    fn default() -> Self {
        Self {
            claude: true,
            codex: true,
            gemini: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub preferred_port: u16,
    #[serde(default = "default_show_home_heatmap")]
    pub show_home_heatmap: bool,
    #[serde(default = "default_show_home_usage")]
    pub show_home_usage: bool,
    #[serde(default)]
    pub home_usage_period: HomeUsagePeriod,
    // Gateway listen mode (aligned with code-switch-r): localhost / wsl_auto / lan / custom.
    pub gateway_listen_mode: GatewayListenMode,
    // Custom listen address input (host or host:port).
    pub gateway_custom_listen_address: String,
    // WSL auto-config enable switch and target CLI selection.
    pub wsl_auto_config: bool,
    pub wsl_target_cli: WslTargetCli,
    #[serde(default = "default_cli_priority_order")]
    pub cli_priority_order: Vec<String>,
    // WSL host address mode (auto-detect or custom) and custom address.
    pub wsl_host_address_mode: WslHostAddressMode,
    pub wsl_custom_host_address: String,
    // Windows-side Codex config location mode.
    pub codex_home_mode: CodexHomeMode,
    // Optional Codex config directory override. Empty = default resolution.
    pub codex_home_override: String,
    // Codex CLI proxy OAuth compatible mode. When enabled, proxy takeover
    // manages config.toml only and leaves auth.json untouched.
    pub codex_oauth_compatible_proxy_mode: bool,
    #[serde(default = "default_codex_provider_test_model")]
    pub codex_provider_test_model: String,
    #[serde(default = "default_codex_reasoning_guard_hit_label")]
    pub codex_reasoning_guard_hit_label: String,
    // Codex reasoning guard: detect degraded reasoning token signatures and
    // retry the same provider without affecting circuit breaker state.
    pub codex_reasoning_guard_enabled: bool,
    #[serde(default)]
    pub codex_reasoning_guard_rule_mode: CodexReasoningGuardRuleMode,
    #[serde(default)]
    pub codex_reasoning_guard_compare_mode: CodexReasoningGuardCompareMode,
    pub codex_reasoning_guard_reasoning_equals: Vec<i64>,
    #[serde(default)]
    pub codex_reasoning_guard_model_rules: Vec<CodexReasoningGuardModelRule>,
    #[serde(default = "default_codex_reasoning_guard_active_template_id")]
    pub codex_reasoning_guard_active_template_id: String,
    #[serde(default)]
    pub codex_reasoning_guard_custom_templates: Vec<CodexReasoningGuardRuleTemplate>,
    #[serde(default)]
    pub codex_reasoning_guard_post_match_strategy: CodexReasoningGuardPostMatchStrategy,
    #[serde(default = "default_codex_reasoning_guard_immediate_retry_budget")]
    pub codex_reasoning_guard_immediate_retry_budget: u32,
    #[serde(default = "default_codex_reasoning_guard_delayed_retry_budget")]
    pub codex_reasoning_guard_delayed_retry_budget: u32,
    #[serde(default = "default_codex_reasoning_guard_delayed_retry_ms")]
    pub codex_reasoning_guard_delayed_retry_ms: u32,
    #[serde(default)]
    pub codex_reasoning_guard_exhausted_action: CodexReasoningGuardExhaustedAction,
    #[serde(default)]
    pub codex_reasoning_guard_retry_policy: CodexReasoningGuardRetryPolicy,
    #[serde(default = "default_codex_reasoning_guard_concurrent_max")]
    pub codex_reasoning_guard_concurrent_max: u32,
    #[serde(default = "default_codex_reasoning_guard_concurrent_interval_ms")]
    pub codex_reasoning_guard_concurrent_interval_ms: u32,
    #[serde(default = "default_codex_reasoning_guard_concurrent_max_attempts")]
    pub codex_reasoning_guard_concurrent_max_attempts: u32,
    #[serde(default)]
    pub codex_reasoning_guard_model_fallbacks: Vec<String>,
    // Legacy field kept for settings compatibility. Since schema 48, runtime
    // behavior ignores this value and uses codex_reasoning_guard_post_match_strategy.
    #[serde(default)]
    pub codex_reasoning_guard_continuation_repair_enabled: bool,
    // Legacy setting kept for settings compatibility. Continuation repair rounds
    // now use codex_reasoning_guard_immediate_retry_budget.
    #[serde(default = "default_codex_reasoning_guard_continuation_max_rounds")]
    pub codex_reasoning_guard_continuation_max_rounds: u32,
    #[serde(default = "default_codex_reasoning_guard_continuation_max_output_tokens")]
    pub codex_reasoning_guard_continuation_max_output_tokens: u32,
    // Deprecated compatibility fields. Runtime guard budget decisions use the
    // explicit budget fields above as the single source of truth.
    pub codex_reasoning_guard_backoff_after_hits: u32,
    pub codex_reasoning_guard_backoff_ms: u32,
    pub auto_start: bool,
    // Start with window hidden when auto-starting (silent startup).
    pub start_minimized: bool,
    pub tray_enabled: bool,
    // Startup crash recovery for CLI proxy takeover (default enabled).
    pub enable_cli_proxy_startup_recovery: bool,
    pub log_retention_days: u32,
    pub provider_cooldown_seconds: u32,
    pub provider_base_url_ping_cache_ttl_seconds: u32,
    pub upstream_first_byte_timeout_seconds: u32,
    pub upstream_stream_idle_timeout_seconds: u32,
    pub upstream_request_timeout_non_streaming_seconds: u32,
    pub update_releases_url: String,
    pub failover_max_attempts_per_provider: u32,
    pub failover_max_providers_to_try: u32,
    #[serde(default)]
    pub upstream_retry_policy: UpstreamRetryPolicy,
    pub circuit_breaker_failure_threshold: u32,
    pub circuit_breaker_open_duration_minutes: u32,
    // Circuit breaker notice toggle (default disabled).
    pub enable_circuit_breaker_notice: bool,
    // CCH-aligned gateway feature toggles.
    pub verbose_provider_error: bool,
    pub intercept_anthropic_warmup_requests: bool,
    pub enable_thinking_signature_rectifier: bool,
    pub enable_thinking_budget_rectifier: bool,
    // Billing header rectifier: strip x-anthropic-billing-header from system prompt (default enabled).
    pub enable_billing_header_rectifier: bool,
    // Codex Session ID completion (default enabled).
    pub enable_codex_session_id_completion: bool,
    // Claude metadata.user_id injection (default enabled).
    pub enable_claude_metadata_user_id_injection: bool,
    // Cache anomaly monitor (default disabled).
    pub enable_cache_anomaly_monitor: bool,
    // Debug log mode: emit detailed request/response data to gateway:log events (default disabled).
    pub enable_debug_log: bool,
    // Task complete notification (default enabled).
    pub enable_task_complete_notify: bool,
    // Notification sound toggle - play custom sound when notifications fire (default enabled).
    pub enable_notification_sound: bool,
    // Response fixer (default enabled).
    pub enable_response_fixer: bool,
    pub response_fixer_fix_encoding: bool,
    pub response_fixer_fix_sse_format: bool,
    pub response_fixer_fix_truncated_json: bool,
    pub response_fixer_max_json_depth: u32,
    pub response_fixer_max_fix_size: u32,
    // CX2CC bridge settings.
    pub cx2cc_fallback_model_opus: String,
    pub cx2cc_fallback_model_sonnet: String,
    pub cx2cc_fallback_model_haiku: String,
    pub cx2cc_fallback_model_main: String,
    pub cx2cc_model_reasoning_effort: String,
    pub cx2cc_service_tier: String,
    pub cx2cc_disable_response_storage: bool,
    pub cx2cc_enable_reasoning_to_thinking: bool,
    pub cx2cc_drop_stop_sequences: bool,
    pub cx2cc_clean_schema: bool,
    pub cx2cc_filter_batch_tool: bool,
    // Upstream proxy settings for gateway outbound requests.
    pub upstream_proxy_enabled: bool,
    pub upstream_proxy_url: String,
    pub upstream_proxy_username: String,
    pub upstream_proxy_password: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            preferred_port: DEFAULT_GATEWAY_PORT,
            show_home_heatmap: DEFAULT_SHOW_HOME_HEATMAP,
            show_home_usage: DEFAULT_SHOW_HOME_USAGE,
            home_usage_period: HomeUsagePeriod::default(),
            gateway_listen_mode: GatewayListenMode::Localhost,
            gateway_custom_listen_address: String::new(),
            wsl_auto_config: false,
            wsl_target_cli: WslTargetCli::default(),
            cli_priority_order: default_cli_priority_order(),
            wsl_host_address_mode: WslHostAddressMode::Auto,
            wsl_custom_host_address: "127.0.0.1".to_string(),
            codex_home_mode: CodexHomeMode::default(),
            codex_home_override: String::new(),
            codex_oauth_compatible_proxy_mode: DEFAULT_CODEX_OAUTH_COMPATIBLE_PROXY_MODE,
            codex_provider_test_model: DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string(),
            codex_reasoning_guard_hit_label: DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL.to_string(),
            codex_reasoning_guard_enabled: DEFAULT_CODEX_REASONING_GUARD_ENABLED,
            codex_reasoning_guard_rule_mode: CodexReasoningGuardRuleMode::default(),
            codex_reasoning_guard_compare_mode: CodexReasoningGuardCompareMode::default(),
            codex_reasoning_guard_reasoning_equals: DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS
                .to_vec(),
            codex_reasoning_guard_model_rules: Vec::new(),
            codex_reasoning_guard_active_template_id:
                CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID.to_string(),
            codex_reasoning_guard_custom_templates: Vec::new(),
            codex_reasoning_guard_post_match_strategy:
                CodexReasoningGuardPostMatchStrategy::default(),
            codex_reasoning_guard_immediate_retry_budget:
                DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET,
            codex_reasoning_guard_delayed_retry_budget:
                DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET,
            codex_reasoning_guard_delayed_retry_ms: DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS,
            codex_reasoning_guard_exhausted_action: CodexReasoningGuardExhaustedAction::default(),
            codex_reasoning_guard_retry_policy: CodexReasoningGuardRetryPolicy::default(),
            codex_reasoning_guard_concurrent_max: DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX,
            codex_reasoning_guard_concurrent_interval_ms:
                DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS,
            codex_reasoning_guard_concurrent_max_attempts:
                DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS,
            codex_reasoning_guard_model_fallbacks: Vec::new(),
            codex_reasoning_guard_continuation_repair_enabled:
                DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_REPAIR_ENABLED,
            codex_reasoning_guard_continuation_max_rounds:
                DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS,
            codex_reasoning_guard_continuation_max_output_tokens:
                DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS,
            codex_reasoning_guard_backoff_after_hits:
                DEFAULT_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS,
            codex_reasoning_guard_backoff_ms: DEFAULT_CODEX_REASONING_GUARD_BACKOFF_MS,
            auto_start: false,
            start_minimized: false,
            tray_enabled: true,
            enable_cli_proxy_startup_recovery: DEFAULT_ENABLE_CLI_PROXY_STARTUP_RECOVERY,
            log_retention_days: DEFAULT_LOG_RETENTION_DAYS,
            provider_cooldown_seconds: DEFAULT_PROVIDER_COOLDOWN_SECONDS,
            provider_base_url_ping_cache_ttl_seconds:
                DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS,
            upstream_first_byte_timeout_seconds: DEFAULT_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS,
            upstream_stream_idle_timeout_seconds: DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS,
            upstream_request_timeout_non_streaming_seconds:
                DEFAULT_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS,
            update_releases_url: DEFAULT_UPDATE_RELEASES_URL.to_string(),
            failover_max_attempts_per_provider: DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER,
            failover_max_providers_to_try: DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY,
            upstream_retry_policy: UpstreamRetryPolicy::default(),
            circuit_breaker_failure_threshold: DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD,
            circuit_breaker_open_duration_minutes: DEFAULT_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES,
            enable_circuit_breaker_notice: DEFAULT_ENABLE_CIRCUIT_BREAKER_NOTICE,
            verbose_provider_error: DEFAULT_VERBOSE_PROVIDER_ERROR,
            intercept_anthropic_warmup_requests: DEFAULT_INTERCEPT_ANTHROPIC_WARMUP_REQUESTS,
            enable_thinking_signature_rectifier: DEFAULT_ENABLE_THINKING_SIGNATURE_RECTIFIER,
            enable_thinking_budget_rectifier: DEFAULT_ENABLE_THINKING_BUDGET_RECTIFIER,
            enable_billing_header_rectifier: DEFAULT_ENABLE_BILLING_HEADER_RECTIFIER,
            enable_codex_session_id_completion: DEFAULT_ENABLE_CODEX_SESSION_ID_COMPLETION,
            enable_claude_metadata_user_id_injection:
                DEFAULT_ENABLE_CLAUDE_METADATA_USER_ID_INJECTION,
            enable_cache_anomaly_monitor: DEFAULT_ENABLE_CACHE_ANOMALY_MONITOR,
            enable_debug_log: DEFAULT_ENABLE_DEBUG_LOG,
            enable_task_complete_notify: DEFAULT_ENABLE_TASK_COMPLETE_NOTIFY,
            enable_notification_sound: DEFAULT_ENABLE_NOTIFICATION_SOUND,
            enable_response_fixer: DEFAULT_ENABLE_RESPONSE_FIXER,
            response_fixer_fix_encoding: DEFAULT_RESPONSE_FIXER_FIX_ENCODING,
            response_fixer_fix_sse_format: DEFAULT_RESPONSE_FIXER_FIX_SSE_FORMAT,
            response_fixer_fix_truncated_json: DEFAULT_RESPONSE_FIXER_FIX_TRUNCATED_JSON,
            response_fixer_max_json_depth: DEFAULT_RESPONSE_FIXER_MAX_JSON_DEPTH,
            response_fixer_max_fix_size: DEFAULT_RESPONSE_FIXER_MAX_FIX_SIZE,
            cx2cc_fallback_model_opus: DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            cx2cc_fallback_model_sonnet: DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            cx2cc_fallback_model_haiku: DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            cx2cc_fallback_model_main: DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            cx2cc_model_reasoning_effort: String::new(),
            cx2cc_service_tier: String::new(),
            cx2cc_disable_response_storage: true,
            cx2cc_enable_reasoning_to_thinking: true,
            cx2cc_drop_stop_sequences: true,
            cx2cc_clean_schema: true,
            cx2cc_filter_batch_tool: true,
            upstream_proxy_enabled: false,
            upstream_proxy_url: String::new(),
            upstream_proxy_username: String::new(),
            upstream_proxy_password: String::new(),
        }
    }
}

fn default_show_home_heatmap() -> bool {
    DEFAULT_SHOW_HOME_HEATMAP
}

fn default_show_home_usage() -> bool {
    DEFAULT_SHOW_HOME_USAGE
}

pub(super) fn default_cli_priority_order() -> Vec<String> {
    crate::shared::cli_key::SUPPORTED_CLI_KEYS
        .iter()
        .map(|cli_key| (*cli_key).to_string())
        .collect()
}
