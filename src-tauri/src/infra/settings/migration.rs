//! Usage: Schema repair and input sanitization for persisted settings.

use super::defaults::*;
use super::types::{AppSettings, CodexHomeMode, UpstreamRetryPolicy};
use crate::shared::error::AppResult;
use std::collections::HashSet;

const SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK: u32 = 36;

pub(super) fn normalize_cli_priority_order(input: &[String]) -> Vec<String> {
    let mut order = Vec::with_capacity(crate::shared::cli_key::SUPPORTED_CLI_KEYS.len());

    for cli_key in input {
        if !crate::shared::cli_key::is_supported_cli_key(cli_key) {
            continue;
        }
        if order.iter().any(|item| item == cli_key) {
            continue;
        }
        order.push(cli_key.clone());
    }

    for cli_key in crate::shared::cli_key::SUPPORTED_CLI_KEYS {
        if order.iter().any(|item| item == cli_key) {
            continue;
        }
        order.push(cli_key.to_string());
    }

    order
}

pub(super) fn normalize_codex_home_override(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.eq_ignore_ascii_case("config.toml") {
        return String::new();
    }

    for suffix in ["/config.toml", "\\config.toml"] {
        if trimmed.len() > suffix.len()
            && trimmed[trimmed.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
        {
            return trimmed[..trimmed.len() - suffix.len()]
                .trim_end_matches(['/', '\\'])
                .to_string();
        }
    }

    trimmed.to_string()
}

pub(super) fn sanitize_codex_home_override(settings: &mut AppSettings) -> bool {
    let normalized = normalize_codex_home_override(&settings.codex_home_override);
    let mut changed = settings.codex_home_override != normalized;
    settings.codex_home_override = normalized;

    if settings.codex_home_mode != CodexHomeMode::Custom && !settings.codex_home_override.is_empty()
    {
        settings.codex_home_override.clear();
        changed = true;
    }

    if settings.codex_home_mode == CodexHomeMode::Custom && settings.codex_home_override.is_empty()
    {
        settings.codex_home_mode = CodexHomeMode::UserHomeDefault;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_cli_priority_order(settings: &mut AppSettings) -> bool {
    let normalized = normalize_cli_priority_order(&settings.cli_priority_order);
    let changed = settings.cli_priority_order != normalized;
    settings.cli_priority_order = normalized;
    changed
}

pub(super) fn sanitize_codex_provider_test_model(settings: &mut AppSettings) -> bool {
    let normalized = settings.codex_provider_test_model.trim();
    let next = if normalized.is_empty() {
        DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string()
    } else {
        normalized.to_string()
    };
    let changed = settings.codex_provider_test_model != next;
    settings.codex_provider_test_model = next;
    changed
}

pub(super) fn sanitize_failover_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.failover_max_attempts_per_provider == 0 {
        settings.failover_max_attempts_per_provider = DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER;
        changed = true;
    }
    if settings.failover_max_providers_to_try == 0 {
        settings.failover_max_providers_to_try = DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY;
        changed = true;
    }

    if settings.failover_max_attempts_per_provider > MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER {
        settings.failover_max_attempts_per_provider = MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER;
        changed = true;
    }
    if settings.failover_max_providers_to_try > MAX_FAILOVER_MAX_PROVIDERS_TO_TRY {
        settings.failover_max_providers_to_try = MAX_FAILOVER_MAX_PROVIDERS_TO_TRY;
        changed = true;
    }

    let providers = settings.failover_max_providers_to_try.max(1);
    let max_attempts_for_providers = (MAX_FAILOVER_TOTAL_ATTEMPTS / providers).max(1);
    if settings.failover_max_attempts_per_provider > max_attempts_for_providers {
        settings.failover_max_attempts_per_provider = max_attempts_for_providers;
        changed = true;
    }

    changed
}

pub fn sanitize_upstream_retry_policy(policy: &mut UpstreamRetryPolicy) -> bool {
    let mut changed = false;

    policy.status_codes.retain(|status| {
        let keep = (400..=599).contains(status);
        changed |= !keep;
        keep
    });
    policy.status_codes.sort_unstable();
    policy.status_codes.dedup();
    if policy.status_codes.len() > MAX_UPSTREAM_RETRY_POLICY_STATUS_CODES {
        policy
            .status_codes
            .truncate(MAX_UPSTREAM_RETRY_POLICY_STATUS_CODES);
        changed = true;
    }

    let mut seen_transport_errors = HashSet::new();
    policy.transport_errors.retain(|kind| {
        let keep = seen_transport_errors.insert(*kind);
        changed |= !keep;
        keep
    });
    if policy.transport_errors.len() > MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS {
        policy
            .transport_errors
            .truncate(MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS);
        changed = true;
    }

    if policy.max_retries > MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES {
        policy.max_retries = MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES;
        changed = true;
    }
    if policy.backoff_ms > MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS {
        policy.backoff_ms = MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS;
        changed = true;
    }

    if policy.enabled && (policy.status_codes.is_empty() || policy.transport_errors.is_empty()) {
        let defaults = UpstreamRetryPolicy::default();
        if policy.status_codes.is_empty() {
            policy.status_codes = defaults.status_codes;
            changed = true;
        }
        if policy.transport_errors.is_empty() {
            policy.transport_errors = defaults.transport_errors;
            changed = true;
        }
    }

    changed
}

pub(super) fn sanitize_circuit_breaker_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.circuit_breaker_failure_threshold == 0 {
        settings.circuit_breaker_failure_threshold = DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD;
        changed = true;
    }
    if settings.circuit_breaker_open_duration_minutes == 0 {
        settings.circuit_breaker_open_duration_minutes =
            DEFAULT_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES;
        changed = true;
    }

    if settings.circuit_breaker_failure_threshold > MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD {
        settings.circuit_breaker_failure_threshold = MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD;
        changed = true;
    }
    if settings.circuit_breaker_open_duration_minutes > MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES {
        settings.circuit_breaker_open_duration_minutes = MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_log_retention_days(settings: &mut AppSettings) -> bool {
    if settings.log_retention_days > MAX_LOG_RETENTION_DAYS {
        settings.log_retention_days = MAX_LOG_RETENTION_DAYS;
        return true;
    }
    false
}

pub(super) fn sanitize_request_log_retention_days(settings: &mut AppSettings) -> bool {
    if settings.request_log_retention_days > MAX_REQUEST_LOG_RETENTION_DAYS {
        settings.request_log_retention_days = MAX_REQUEST_LOG_RETENTION_DAYS;
        return true;
    }
    false
}

pub(super) fn sanitize_provider_cooldown_seconds(settings: &mut AppSettings) -> bool {
    if settings.provider_cooldown_seconds > MAX_PROVIDER_COOLDOWN_SECONDS {
        settings.provider_cooldown_seconds = MAX_PROVIDER_COOLDOWN_SECONDS;
        return true;
    }
    false
}

pub(super) fn sanitize_provider_base_url_ping_cache_ttl_seconds(
    settings: &mut AppSettings,
) -> bool {
    let mut changed = false;

    if settings.provider_base_url_ping_cache_ttl_seconds == 0 {
        settings.provider_base_url_ping_cache_ttl_seconds =
            DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS;
        changed = true;
    }

    if settings.provider_base_url_ping_cache_ttl_seconds
        > MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
    {
        settings.provider_base_url_ping_cache_ttl_seconds =
            MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_upstream_timeouts(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.upstream_first_byte_timeout_seconds > MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS {
        settings.upstream_first_byte_timeout_seconds = MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS;
        changed = true;
    }
    if settings.upstream_stream_idle_timeout_seconds > MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS {
        settings.upstream_stream_idle_timeout_seconds = MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
        changed = true;
    }
    if settings.upstream_stream_idle_timeout_seconds > 0
        && settings.upstream_stream_idle_timeout_seconds < MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS
    {
        settings.upstream_stream_idle_timeout_seconds = MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
        changed = true;
    }
    if settings.upstream_request_timeout_non_streaming_seconds
        > MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS
    {
        settings.upstream_request_timeout_non_streaming_seconds =
            MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_response_fixer_limits(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.response_fixer_max_json_depth == 0 {
        settings.response_fixer_max_json_depth = DEFAULT_RESPONSE_FIXER_MAX_JSON_DEPTH;
        changed = true;
    }
    if settings.response_fixer_max_json_depth > MAX_RESPONSE_FIXER_MAX_JSON_DEPTH {
        settings.response_fixer_max_json_depth = MAX_RESPONSE_FIXER_MAX_JSON_DEPTH;
        changed = true;
    }

    if settings.response_fixer_max_fix_size == 0 {
        settings.response_fixer_max_fix_size = DEFAULT_RESPONSE_FIXER_MAX_FIX_SIZE;
        changed = true;
    }
    if settings.response_fixer_max_fix_size > MAX_RESPONSE_FIXER_MAX_FIX_SIZE {
        settings.response_fixer_max_fix_size = MAX_RESPONSE_FIXER_MAX_FIX_SIZE;
        changed = true;
    }

    changed
}

fn migrate_legacy_upstream_timeouts(
    settings: &mut AppSettings,
    schema_version_present: bool,
    raw_settings_json: &serde_json::Value,
) -> bool {
    let is_legacy_schema = !schema_version_present
        || settings.schema_version < SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS;
    let timeout_keys_present = raw_settings_json
        .get("upstream_first_byte_timeout_seconds")
        .is_some()
        || raw_settings_json
            .get("upstream_stream_idle_timeout_seconds")
            .is_some()
        || raw_settings_json
            .get("upstream_request_timeout_non_streaming_seconds")
            .is_some();

    if !is_legacy_schema || timeout_keys_present {
        return false;
    }

    let mut changed = false;
    if settings.upstream_first_byte_timeout_seconds != 0 {
        settings.upstream_first_byte_timeout_seconds = 0;
        changed = true;
    }
    if settings.upstream_stream_idle_timeout_seconds != 0 {
        settings.upstream_stream_idle_timeout_seconds = 0;
        changed = true;
    }
    if settings.upstream_request_timeout_non_streaming_seconds != 0 {
        settings.upstream_request_timeout_non_streaming_seconds = 0;
        changed = true;
    }
    changed
}

fn migrate_stream_idle_timeout_default(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !schema_version_present
        || settings.schema_version >= SCHEMA_VERSION_RAISE_STREAM_IDLE_TIMEOUT_DEFAULT
    {
        return false;
    }
    if settings.upstream_stream_idle_timeout_seconds == 120 {
        settings.upstream_stream_idle_timeout_seconds =
            DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
        return true;
    }
    false
}

fn migrate_codex_home_mode(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    if schema_version_present && settings.schema_version >= SCHEMA_VERSION_ADD_CODEX_HOME_MODE {
        return false;
    }

    let next = if settings.codex_home_override.trim().is_empty() {
        CodexHomeMode::UserHomeDefault
    } else {
        CodexHomeMode::Custom
    };
    if settings.codex_home_mode != next {
        settings.codex_home_mode = next;
        return true;
    }
    false
}

fn migrate_cx2cc_defaults(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    if schema_version_present && settings.schema_version >= SCHEMA_VERSION_ADD_CX2CC_SETTINGS {
        return false;
    }

    let mut changed = false;
    for field in [
        &mut settings.cx2cc_fallback_model_opus,
        &mut settings.cx2cc_fallback_model_sonnet,
        &mut settings.cx2cc_fallback_model_haiku,
        &mut settings.cx2cc_fallback_model_main,
    ] {
        if field.trim().is_empty() {
            *field = DEFAULT_CX2CC_FALLBACK_MODEL.to_string();
            changed = true;
        }
    }

    if !settings.cx2cc_disable_response_storage {
        settings.cx2cc_disable_response_storage = true;
        changed = true;
    }
    if !settings.cx2cc_enable_reasoning_to_thinking {
        settings.cx2cc_enable_reasoning_to_thinking = true;
        changed = true;
    }
    if !settings.cx2cc_drop_stop_sequences {
        settings.cx2cc_drop_stop_sequences = true;
        changed = true;
    }
    if !settings.cx2cc_clean_schema {
        settings.cx2cc_clean_schema = true;
        changed = true;
    }
    if !settings.cx2cc_filter_batch_tool {
        settings.cx2cc_filter_batch_tool = true;
        changed = true;
    }

    changed
}

fn migrate_update_releases_url(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    if schema_version_present
        && settings.schema_version >= SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK
    {
        return false;
    }

    let current = settings.update_releases_url.trim();
    if current.is_empty() || current == LEGACY_UPDATE_RELEASES_URL {
        settings.update_releases_url = DEFAULT_UPDATE_RELEASES_URL.to_string();
        return true;
    }
    false
}

fn migrate_codex_provider_test_model(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if schema_version_present
        && settings.schema_version >= SCHEMA_VERSION_ADD_CODEX_PROVIDER_TEST_MODEL
    {
        return false;
    }
    sanitize_codex_provider_test_model(settings)
}

fn migrate_retry_gateway_defaults(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if schema_version_present && settings.schema_version >= SCHEMA_VERSION_ADD_CODEX_RETRY_GATEWAY {
        return false;
    }

    let mut changed = false;
    if settings
        .codex_retry_gateway_selected_commit
        .trim()
        .is_empty()
    {
        settings.codex_retry_gateway_selected_commit =
            DEFAULT_CODEX_RETRY_GATEWAY_SELECTED_COMMIT.to_string();
        changed = true;
    }
    if settings.codex_retry_gateway_preferred_port == 0 {
        settings.codex_retry_gateway_preferred_port = DEFAULT_CODEX_RETRY_GATEWAY_PORT;
        changed = true;
    }
    if settings.codex_retry_gateway_node_override.is_empty() {
        settings.codex_retry_gateway_node_override =
            DEFAULT_CODEX_RETRY_GATEWAY_NODE_OVERRIDE.to_string();
    }
    changed
}

pub(super) fn repair_settings(
    settings: &mut AppSettings,
    schema_version_present: bool,
    raw_settings_json: &serde_json::Value,
) -> AppResult<bool> {
    let mut repaired = false;

    repaired |=
        migrate_legacy_upstream_timeouts(settings, schema_version_present, raw_settings_json);
    repaired |= migrate_stream_idle_timeout_default(settings, schema_version_present);
    repaired |= migrate_codex_home_mode(settings, schema_version_present);
    repaired |= migrate_cx2cc_defaults(settings, schema_version_present);
    repaired |= migrate_update_releases_url(settings, schema_version_present);
    repaired |= migrate_codex_provider_test_model(settings, schema_version_present);
    repaired |= migrate_retry_gateway_defaults(settings, schema_version_present);

    repaired |= sanitize_log_retention_days(settings);
    repaired |= sanitize_request_log_retention_days(settings);
    repaired |= sanitize_failover_settings(settings);
    repaired |= sanitize_upstream_retry_policy(&mut settings.upstream_retry_policy);
    repaired |= sanitize_circuit_breaker_settings(settings);
    repaired |= sanitize_provider_cooldown_seconds(settings);
    repaired |= sanitize_provider_base_url_ping_cache_ttl_seconds(settings);
    repaired |= sanitize_upstream_timeouts(settings);
    repaired |= sanitize_response_fixer_limits(settings);
    repaired |= sanitize_codex_home_override(settings);
    repaired |= sanitize_codex_provider_test_model(settings);
    repaired |= sanitize_cli_priority_order(settings);

    if settings.schema_version != SCHEMA_VERSION {
        settings.schema_version = SCHEMA_VERSION;
        repaired = true;
    }

    let canonical = super::persistence::canonical_settings_json(settings)?;
    repaired |= raw_settings_json != &canonical;
    Ok(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_codex_home_override_trims_and_drops_config_toml() {
        assert_eq!(
            normalize_codex_home_override(r" C:\Users\me\.codex\config.toml "),
            r"C:\Users\me\.codex"
        );
        assert_eq!(normalize_codex_home_override("config.toml"), "");
    }

    #[test]
    fn sanitize_upstream_retry_policy_deduplicates_and_defaults_enabled_empty_policy() {
        let mut policy = UpstreamRetryPolicy {
            enabled: true,
            status_codes: vec![200, 503, 503, 504],
            transport_errors: vec![],
            max_retries: 999,
            backoff_ms: 999_999,
            counts_toward_circuit_breaker: false,
        };

        assert!(sanitize_upstream_retry_policy(&mut policy));
        assert_eq!(policy.status_codes, vec![503, 504]);
        assert!(!policy.transport_errors.is_empty());
        assert_eq!(policy.max_retries, MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES);
        assert_eq!(policy.backoff_ms, MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS);
    }

    #[test]
    fn repair_settings_updates_legacy_release_url_and_schema() {
        let mut settings = AppSettings {
            schema_version: 35,
            update_releases_url: LEGACY_UPDATE_RELEASES_URL.to_string(),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "schema_version": 35,
            "update_releases_url": LEGACY_UPDATE_RELEASES_URL
        });

        assert!(repair_settings(&mut settings, true, &raw).unwrap());
        assert_eq!(settings.update_releases_url, DEFAULT_UPDATE_RELEASES_URL);
        assert_eq!(settings.schema_version, SCHEMA_VERSION);
    }
}
