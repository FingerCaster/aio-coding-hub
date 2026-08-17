//! Usage: Persisted application settings (schema + read/write helpers).

mod defaults;
mod migration;
mod persistence;
mod types;

static UPDATE_CHANNEL_TRANSITION_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();
static UPDATE_CHANNEL_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn lock_update_channel_transition() -> std::sync::MutexGuard<'static, ()> {
    UPDATE_CHANNEL_TRANSITION_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn update_channel_epoch() -> u64 {
    UPDATE_CHANNEL_EPOCH.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) fn mark_update_channel_transition() {
    UPDATE_CHANNEL_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

// Re-export public API (preserves identical surface for all consumers).
#[allow(unused_imports)]
pub use defaults::MAX_CODEX_MODEL_CONTEXT_RULES;
pub use defaults::{
    DEFAULT_CAPACITY_RETRY_KEYWORD, DEFAULT_CODEX_INFINITE_RETRY_TEST_INTERVAL_MS,
    DEFAULT_CODEX_PROVIDER_TEST_MODEL, DEFAULT_CX2CC_FALLBACK_MODEL, DEFAULT_GATEWAY_PORT,
    DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS, DEFAULT_PROVIDER_COOLDOWN_SECONDS,
    DEFAULT_STREAM_INTERNAL_ERROR_GUARD_MS, DEFAULT_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS,
    DEFAULT_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS,
    DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS, MAX_GATEWAY_PORT,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_DESCRIPTION_CHARS, MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORDS,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORD_CHARS, MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_NAME_CHARS, MAX_UPSTREAM_ERROR_RESPONSE_RULE_PRIORITY,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_PROVIDER_IDS, MAX_UPSTREAM_ERROR_RESPONSE_RULE_STATUS_CODES,
    MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS, MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS,
    SCHEMA_VERSION,
};
pub(crate) use migration::{
    migrate_to_current_schema, normalize_codex_model_context_rules_for_write,
    normalize_cx2cc_reasoning_effort_mappings_for_write, normalize_model_routing_policy_for_write,
    normalize_upstream_error_response_rules_for_write, normalize_upstream_retry_policy_for_write,
    sanitize_model_routing_policy, sanitize_upstream_retry_policy,
};
pub(crate) use persistence::validate_bounds;
pub use persistence::{
    clear_cache, compare_and_swap, log_retention_days_fail_open, read,
    request_log_retention_days_fail_open, set_settings_finalize_failpoint_for_tests,
    set_settings_finalize_restore_failpoint_for_tests, update, write,
};
#[allow(unused_imports)]
pub use types::ModelRoutingRule;
pub use types::{
    default_cx2cc_reasoning_effort_mappings, AppSettings, CodexHomeMode, CodexModelContextRule,
    Cx2ccReasoningEffortMapping, GatewayListenMode, HomeUsagePeriod, ModelRoutingPolicy,
    ProviderFailbackStrategy, UpdateChannel, UpstreamErrorMessageBehavior,
    UpstreamErrorResponseMatchMode, UpstreamErrorResponseRule, UpstreamErrorStatusBehavior,
    UpstreamHttpRetryRule, UpstreamRetryPolicy, UpstreamStreamInternalErrorPolicy,
    UpstreamTransportRetryKind, WslHostAddressMode, WslTargetCli,
};
