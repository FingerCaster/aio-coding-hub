//! Usage: Persisted application settings (schema + read/write helpers).

mod defaults;
mod migration;
mod persistence;
mod types;

// Re-export public API (preserves identical surface for all consumers).
pub use defaults::{
    DEFAULT_CODEX_PROVIDER_TEST_MODEL, DEFAULT_CX2CC_FALLBACK_MODEL, DEFAULT_GATEWAY_PORT,
    DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS, DEFAULT_PROVIDER_COOLDOWN_SECONDS,
    DEFAULT_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS,
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
    normalize_upstream_error_response_rules_for_write, normalize_upstream_retry_policy_for_write,
    sanitize_upstream_retry_policy,
};
pub(crate) use persistence::validate_bounds;
pub use persistence::{
    clear_cache, compare_and_swap, log_retention_days_fail_open, read,
    request_log_retention_days_fail_open, set_settings_finalize_failpoint_for_tests,
    set_settings_finalize_restore_failpoint_for_tests, update, write,
};
pub use types::{
    AppSettings, CodexHomeMode, GatewayListenMode, HomeUsagePeriod, ProviderFailbackStrategy,
    UpstreamErrorMessageBehavior, UpstreamErrorResponseMatchMode, UpstreamErrorResponseRule,
    UpstreamErrorStatusBehavior, UpstreamHttpRetryRule, UpstreamRetryPolicy,
    UpstreamTransportRetryKind, WslHostAddressMode, WslTargetCli,
};
