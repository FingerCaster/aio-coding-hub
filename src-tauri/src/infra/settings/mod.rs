//! Usage: Persisted application settings (schema + read/write helpers).

mod defaults;
mod migration;
mod persistence;
mod types;

// Re-export public API (preserves identical surface for all consumers).
pub use defaults::{
    DEFAULT_CODEX_PROVIDER_TEST_MODEL, DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS,
    DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX,
    DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS,
    DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET,
    DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS, DEFAULT_CODEX_REASONING_GUARD_ENABLED,
    DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL, DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET,
    DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS, DEFAULT_CX2CC_FALLBACK_MODEL,
    DEFAULT_GATEWAY_PORT, DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS,
    DEFAULT_PROVIDER_COOLDOWN_SECONDS, DEFAULT_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS,
    DEFAULT_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS,
    DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS, MAX_GATEWAY_PORT,
    MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS, SCHEMA_VERSION,
};
pub(crate) use migration::sanitize_upstream_retry_policy;
pub(crate) use persistence::validate_bounds;
pub use persistence::{clear_cache, log_retention_days_fail_open, read, write};
pub use types::{
    AppSettings, CodexHomeMode, CodexReasoningGuardCompareMode, CodexReasoningGuardExhaustedAction,
    CodexReasoningGuardModelRule, CodexReasoningGuardRetryPolicy, GatewayListenMode,
    HomeUsagePeriod, UpstreamRetryPolicy, UpstreamTransportRetryKind, WslHostAddressMode,
    WslTargetCli,
};
