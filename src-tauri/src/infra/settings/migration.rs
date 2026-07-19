//! Usage: Schema migrations and input sanitization for settings upgrades.

use super::defaults::*;
use super::types::{AppSettings, CodexHomeMode, UpstreamRetryPolicy};
use crate::shared::error::AppResult;
use std::collections::HashSet;

pub(super) const SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK: u32 = 36;

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

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn normalize_body_contains_for_write(rule_index: usize, values: &mut Vec<String>) -> AppResult<()> {
    if values.len() > MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].body_contains must contain <= {MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS} entries"
        )
        .into());
    }

    let mut normalized = Vec::with_capacity(values.len());
    let mut seen = HashSet::new();
    for (content_index, value) in values.iter().enumerate() {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(format!(
                "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].body_contains[{content_index}] must not be empty"
            )
            .into());
        }
        if trimmed.chars().count() > MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS {
            return Err(format!(
                "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].body_contains[{content_index}] must be <= {MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS} characters"
            )
            .into());
        }
        let lowercase = trimmed.to_lowercase();
        if lowercase.chars().count() > MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS {
            return Err(format!(
                "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].body_contains[{content_index}] must be <= {MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS} characters after normalization"
            )
            .into());
        }
        if seen.insert(lowercase.clone()) {
            normalized.push(lowercase);
        }
    }
    *values = normalized;
    Ok(())
}

pub fn normalize_upstream_retry_policy_for_write(
    policy: &mut UpstreamRetryPolicy,
) -> AppResult<bool> {
    let original = policy.clone();
    if policy.http_rules.len() > MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.http_rules must contain <= {MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES} entries"
        )
        .into());
    }

    for (rule_index, rule) in policy.http_rules.iter_mut().enumerate() {
        if !(400..=599).contains(&rule.status_code) {
            return Err(format!(
                "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].status_code must be within [400, 599]"
            )
            .into());
        }
        normalize_body_contains_for_write(rule_index, &mut rule.body_contains)?;
        if rule.description.chars().any(char::is_control) {
            return Err(format!(
                "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].description must not contain control characters"
            )
            .into());
        }
        rule.description = rule.description.trim().to_string();
        if rule.description.chars().count() > MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS {
            return Err(format!(
                "SEC_INVALID_INPUT: upstream_retry_policy.http_rules[{rule_index}].description must be <= {MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS} characters"
            )
            .into());
        }
    }

    if policy.transport_errors.len() > MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.transport_errors must contain <= {MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS} entries"
        )
        .into());
    }
    let mut seen_transport_errors = HashSet::new();
    policy
        .transport_errors
        .retain(|kind| seen_transport_errors.insert(*kind));

    if policy.max_retries > MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.max_retries must be <= {MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES}"
        )
        .into());
    }
    if policy.backoff_ms > MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.backoff_ms must be <= {MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS}"
        )
        .into());
    }
    if policy.enabled
        && !policy.http_rules.iter().any(|rule| rule.enabled)
        && policy.transport_errors.is_empty()
    {
        return Err("SEC_INVALID_INPUT: upstream_retry_policy must include at least one enabled HTTP rule or transport error when enabled".into());
    }

    Ok(*policy != original)
}

pub fn sanitize_upstream_retry_policy(policy: &mut UpstreamRetryPolicy) -> bool {
    let original = policy.clone();
    policy
        .http_rules
        .retain(|rule| (400..=599).contains(&rule.status_code));
    policy
        .http_rules
        .truncate(MAX_UPSTREAM_RETRY_POLICY_HTTP_RULES);

    for rule in &mut policy.http_rules {
        let originally_had_body_contents = !rule.body_contains.is_empty();
        let mut normalized = Vec::new();
        let mut seen = HashSet::new();
        for value in rule
            .body_contains
            .iter()
            .take(MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS)
        {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lowercase = truncate_chars(
                &trimmed.to_lowercase(),
                MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS,
            );
            if !lowercase.is_empty() && seen.insert(lowercase.clone()) {
                normalized.push(lowercase);
            }
        }
        rule.body_contains = normalized;
        if originally_had_body_contents && rule.body_contains.is_empty() {
            rule.enabled = false;
        }

        let description: String = rule
            .description
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        rule.description = truncate_chars(
            description.trim(),
            MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS,
        );
    }

    let mut seen_transport_errors = HashSet::new();
    policy
        .transport_errors
        .retain(|kind| seen_transport_errors.insert(*kind));
    policy
        .transport_errors
        .truncate(MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS);
    policy.max_retries = policy
        .max_retries
        .min(MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES);
    policy.backoff_ms = policy.backoff_ms.min(MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS);

    if policy.enabled
        && !policy.http_rules.iter().any(|rule| rule.enabled)
        && policy.transport_errors.is_empty()
    {
        policy.enabled = false;
    }

    *policy != original
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

// -- Schema migrations --

fn migrate_disable_upstream_timeouts(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if schema_version_present && settings.schema_version >= SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS
    {
        return false;
    }

    let mut changed = false;

    if !schema_version_present {
        changed = true;
    }

    if settings.schema_version != SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS {
        settings.schema_version = SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS;
        changed = true;
    }

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

fn migrate_bump_schema_version(
    settings: &mut AppSettings,
    schema_version_present: bool,
    target_version: u32,
) -> bool {
    if schema_version_present && settings.schema_version >= target_version {
        return false;
    }

    let mut changed = false;

    if !schema_version_present {
        changed = true;
    }

    if settings.schema_version != target_version {
        settings.schema_version = target_version;
        changed = true;
    }

    changed
}

fn migrate_add_gateway_rectifiers(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_GATEWAY_RECTIFIERS,
    )
}

fn migrate_add_circuit_breaker_notice(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CIRCUIT_BREAKER_NOTICE,
    )
}

fn migrate_add_provider_base_url_ping_cache_ttl(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_PROVIDER_BASE_URL_PING_CACHE_TTL,
    )
}

fn migrate_add_codex_session_id_completion(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_SESSION_ID_COMPLETION,
    )
}

fn migrate_add_gateway_network_settings(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_GATEWAY_NETWORK_SETTINGS,
    )
}

fn migrate_add_response_fixer_limits(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_RESPONSE_FIXER_LIMITS,
    )
}

fn migrate_add_cli_proxy_startup_recovery(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CLI_PROXY_STARTUP_RECOVERY,
    )
}

fn migrate_add_cache_anomaly_monitor(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CACHE_ANOMALY_MONITOR,
    )
}

fn migrate_add_wsl_host_address_mode(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_WSL_HOST_ADDRESS_MODE,
    )
}

fn migrate_add_task_complete_notify(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_TASK_COMPLETE_NOTIFY,
    )
}

fn migrate_add_cch_base_config(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CCH_BASE_CONFIG,
    )
}

fn migrate_add_start_minimized(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_START_MINIMIZED,
    )
}

fn migrate_add_show_home_heatmap(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_SHOW_HOME_HEATMAP,
    )
}

fn migrate_add_home_usage_period(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_HOME_USAGE_PERIOD,
    )
}

fn migrate_add_show_home_usage(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_SHOW_HOME_USAGE,
    )
}

fn migrate_add_codex_home_override(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_HOME_OVERRIDE,
    )
}

fn migrate_add_codex_home_mode(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    let needs_mode_default =
        !schema_version_present || settings.schema_version < SCHEMA_VERSION_ADD_CODEX_HOME_MODE;
    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_HOME_MODE,
    );

    if needs_mode_default {
        let next = if settings.codex_home_override.trim().is_empty() {
            CodexHomeMode::UserHomeDefault
        } else {
            CodexHomeMode::Custom
        };
        if settings.codex_home_mode != next {
            settings.codex_home_mode = next;
            changed = true;
        }
    }

    changed
}

fn migrate_add_notification_sound(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_NOTIFICATION_SOUND,
    )
}

fn migrate_add_cx2cc_settings(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CX2CC_SETTINGS,
    );
    if !changed {
        return false;
    }

    for field in [
        &mut settings.cx2cc_fallback_model_opus,
        &mut settings.cx2cc_fallback_model_sonnet,
        &mut settings.cx2cc_fallback_model_haiku,
        &mut settings.cx2cc_fallback_model_main,
    ] {
        if field.is_empty() {
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

fn migrate_enable_default_upstream_timeouts(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS,
    )
}

fn migrate_add_billing_header_rectifier(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_BILLING_HEADER_RECTIFIER,
    )
}

fn migrate_add_cli_priority_order(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CLI_PRIORITY_ORDER,
    );
    if !changed {
        return false;
    }

    changed |= sanitize_cli_priority_order(settings);
    changed
}

fn migrate_raise_stream_idle_timeout_default(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_RAISE_STREAM_IDLE_TIMEOUT_DEFAULT,
    );
    if !changed {
        return false;
    }

    if settings.upstream_stream_idle_timeout_seconds == 120 {
        settings.upstream_stream_idle_timeout_seconds =
            DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
        changed = true;
    }

    changed
}

fn migrate_add_upstream_proxy(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_PROXY,
    )
}

fn migrate_add_upstream_proxy_credentials(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_PROXY_CREDENTIALS,
    )
}

fn migrate_add_codex_oauth_compatible_proxy_mode(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_OAUTH_COMPATIBLE_PROXY_MODE,
    )
}

fn migrate_update_releases_url_to_fork(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if schema_version_present
        && settings.schema_version >= SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK
    {
        return false;
    }

    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK,
    );

    let current = settings.update_releases_url.trim().to_string();
    if current.is_empty() || current == LEGACY_UPDATE_RELEASES_URL {
        settings.update_releases_url = DEFAULT_UPDATE_RELEASES_URL.to_string();
        changed = true;
    }

    changed
}

fn migrate_add_codex_provider_test_model(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_PROVIDER_TEST_MODEL,
    );
    if !changed {
        return false;
    }

    if settings.codex_provider_test_model.trim().is_empty() {
        settings.codex_provider_test_model = DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string();
        changed = true;
    }

    changed
}

fn migrate_add_upstream_retry_policy(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_RETRY_POLICY,
    );
    if !changed {
        return false;
    }

    changed |= sanitize_upstream_retry_policy(&mut settings.upstream_retry_policy);
    changed
}

fn migrate_remove_codex_reasoning_guard(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_REMOVE_CODEX_REASONING_GUARD,
    )
}

fn migrate_add_grok_proxy_preferences(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_GROK_PROXY_PREFERENCES,
    )
}

fn migrate_add_image_gen_storage_dir(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v51: Add image gen storage dir override (default None = app data dir/image-gen).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_IMAGE_GEN_STORAGE_DIR,
    )
}

fn migrate_add_image_gen_storage_roots(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_IMAGE_GEN_STORAGE_ROOTS,
    )
}

fn migrate_add_upstream_http_retry_rules(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_HTTP_RETRY_RULES,
    )
}

type SettingsMigration = fn(&mut AppSettings, bool) -> bool;

const SETTINGS_MIGRATIONS: &[SettingsMigration] = &[
    migrate_disable_upstream_timeouts,
    migrate_add_gateway_rectifiers,
    migrate_add_circuit_breaker_notice,
    migrate_add_provider_base_url_ping_cache_ttl,
    migrate_add_codex_session_id_completion,
    migrate_add_gateway_network_settings,
    migrate_add_response_fixer_limits,
    migrate_add_cli_proxy_startup_recovery,
    migrate_add_cache_anomaly_monitor,
    migrate_add_wsl_host_address_mode,
    migrate_add_task_complete_notify,
    migrate_add_cch_base_config,
    migrate_add_start_minimized,
    migrate_add_show_home_heatmap,
    migrate_add_home_usage_period,
    migrate_add_show_home_usage,
    migrate_add_codex_home_override,
    migrate_add_codex_home_mode,
    migrate_add_notification_sound,
    migrate_add_cx2cc_settings,
    migrate_enable_default_upstream_timeouts,
    migrate_add_billing_header_rectifier,
    migrate_add_cli_priority_order,
    migrate_raise_stream_idle_timeout_default,
    migrate_add_upstream_proxy,
    migrate_add_upstream_proxy_credentials,
    migrate_add_codex_oauth_compatible_proxy_mode,
    migrate_update_releases_url_to_fork,
    migrate_add_codex_provider_test_model,
    migrate_add_upstream_retry_policy,
    migrate_remove_codex_reasoning_guard,
    migrate_add_grok_proxy_preferences,
    migrate_add_image_gen_storage_dir,
    migrate_add_image_gen_storage_roots,
    migrate_add_upstream_http_retry_rules,
];

fn apply_settings_migrations(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    let mut changed = false;
    for migration in SETTINGS_MIGRATIONS {
        changed |= migration(settings, schema_version_present);
    }
    changed
}

pub(super) fn repair_settings(
    settings: &mut AppSettings,
    schema_version_present: bool,
    raw_settings_json: &serde_json::Value,
) -> AppResult<bool> {
    let mut repaired = apply_settings_migrations(settings, schema_version_present);
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
    let canonical = super::persistence::canonical_settings_json(settings)?;
    repaired |= raw_settings_json != &canonical;
    Ok(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings::types::default_cli_priority_order;

    #[test]
    fn sanitize_failover_resets_zero_attempts_to_default() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 0,
            failover_max_providers_to_try: 3,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(
            s.failover_max_attempts_per_provider,
            DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER
        );
    }

    #[test]
    fn sanitize_failover_resets_zero_providers_to_default() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 3,
            failover_max_providers_to_try: 0,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(
            s.failover_max_providers_to_try,
            DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY
        );
    }

    #[test]
    fn sanitize_failover_clamps_excessive_attempts() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 999,
            failover_max_providers_to_try: 1,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(
            s.failover_max_attempts_per_provider,
            MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER
        );
    }

    #[test]
    fn sanitize_failover_clamps_total_product() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 20,
            failover_max_providers_to_try: 20,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(s.failover_max_attempts_per_provider, 5);
    }

    #[test]
    fn sanitize_failover_no_change_for_valid_values() {
        let mut s = AppSettings::default();
        assert!(!sanitize_failover_settings(&mut s));
    }

    #[test]
    fn sanitize_circuit_breaker_resets_zero_threshold() {
        let mut s = AppSettings {
            circuit_breaker_failure_threshold: 0,
            ..Default::default()
        };
        assert!(sanitize_circuit_breaker_settings(&mut s));
        assert_eq!(
            s.circuit_breaker_failure_threshold,
            DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD
        );
    }

    #[test]
    fn sanitize_circuit_breaker_clamps_excessive_duration() {
        let mut s = AppSettings {
            circuit_breaker_open_duration_minutes: 99_999,
            ..Default::default()
        };
        assert!(sanitize_circuit_breaker_settings(&mut s));
        assert_eq!(
            s.circuit_breaker_open_duration_minutes,
            MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES
        );
    }

    #[test]
    fn sanitize_circuit_breaker_no_change_for_valid_values() {
        let mut s = AppSettings::default();
        assert!(!sanitize_circuit_breaker_settings(&mut s));
    }

    #[test]
    fn sanitize_log_retention_days_clamps_excessive_value() {
        let mut s = AppSettings {
            log_retention_days: MAX_LOG_RETENTION_DAYS + 1,
            ..Default::default()
        };
        assert!(sanitize_log_retention_days(&mut s));
        assert_eq!(s.log_retention_days, MAX_LOG_RETENTION_DAYS);
    }

    #[test]
    fn sanitize_log_retention_days_leaves_valid_value() {
        let mut s = AppSettings {
            log_retention_days: 30,
            ..Default::default()
        };
        assert!(!sanitize_log_retention_days(&mut s));
        assert_eq!(s.log_retention_days, 30);
    }

    #[test]
    fn sanitize_cooldown_clamps_excessive_value() {
        let mut s = AppSettings {
            provider_cooldown_seconds: MAX_PROVIDER_COOLDOWN_SECONDS + 1,
            ..Default::default()
        };
        assert!(sanitize_provider_cooldown_seconds(&mut s));
        assert_eq!(s.provider_cooldown_seconds, MAX_PROVIDER_COOLDOWN_SECONDS);
    }

    #[test]
    fn sanitize_cooldown_allows_zero() {
        let mut s = AppSettings {
            provider_cooldown_seconds: 0,
            ..Default::default()
        };
        assert!(!sanitize_provider_cooldown_seconds(&mut s));
        assert_eq!(s.provider_cooldown_seconds, 0);
    }

    #[test]
    fn sanitize_ping_cache_ttl_resets_zero_to_default() {
        let mut s = AppSettings {
            provider_base_url_ping_cache_ttl_seconds: 0,
            ..Default::default()
        };
        assert!(sanitize_provider_base_url_ping_cache_ttl_seconds(&mut s));
        assert_eq!(
            s.provider_base_url_ping_cache_ttl_seconds,
            DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
        );
    }

    #[test]
    fn sanitize_ping_cache_ttl_clamps_excessive_value() {
        let mut s = AppSettings {
            provider_base_url_ping_cache_ttl_seconds: MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
                + 1,
            ..Default::default()
        };
        assert!(sanitize_provider_base_url_ping_cache_ttl_seconds(&mut s));
        assert_eq!(
            s.provider_base_url_ping_cache_ttl_seconds,
            MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
        );
    }

    #[test]
    fn sanitize_upstream_timeouts_clamps_excessive_values() {
        let mut s = AppSettings {
            upstream_first_byte_timeout_seconds: MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS + 1,
            upstream_stream_idle_timeout_seconds: MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS + 1,
            upstream_request_timeout_non_streaming_seconds:
                MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS + 1,
            ..Default::default()
        };
        assert!(sanitize_upstream_timeouts(&mut s));
        assert_eq!(
            s.upstream_first_byte_timeout_seconds,
            MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS
        );
        assert_eq!(
            s.upstream_stream_idle_timeout_seconds,
            MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS
        );
        assert_eq!(
            s.upstream_request_timeout_non_streaming_seconds,
            MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS
        );
    }

    #[test]
    fn sanitize_upstream_timeouts_allows_zero_disabled() {
        let mut s = AppSettings {
            upstream_first_byte_timeout_seconds: 0,
            upstream_stream_idle_timeout_seconds: 0,
            upstream_request_timeout_non_streaming_seconds: 0,
            ..Default::default()
        };
        assert!(!sanitize_upstream_timeouts(&mut s));
    }

    #[test]
    fn sanitize_response_fixer_resets_zero_depth_to_default() {
        let mut s = AppSettings {
            response_fixer_max_json_depth: 0,
            ..Default::default()
        };
        assert!(sanitize_response_fixer_limits(&mut s));
        assert_eq!(
            s.response_fixer_max_json_depth,
            DEFAULT_RESPONSE_FIXER_MAX_JSON_DEPTH
        );
    }

    #[test]
    fn sanitize_response_fixer_clamps_excessive_depth() {
        let mut s = AppSettings {
            response_fixer_max_json_depth: MAX_RESPONSE_FIXER_MAX_JSON_DEPTH + 1,
            ..Default::default()
        };
        assert!(sanitize_response_fixer_limits(&mut s));
        assert_eq!(
            s.response_fixer_max_json_depth,
            MAX_RESPONSE_FIXER_MAX_JSON_DEPTH
        );
    }

    #[test]
    fn sanitize_response_fixer_resets_zero_size_to_default() {
        let mut s = AppSettings {
            response_fixer_max_fix_size: 0,
            ..Default::default()
        };
        assert!(sanitize_response_fixer_limits(&mut s));
        assert_eq!(
            s.response_fixer_max_fix_size,
            DEFAULT_RESPONSE_FIXER_MAX_FIX_SIZE
        );
    }

    #[test]
    fn migrate_bump_skips_when_already_at_target() {
        let mut s = AppSettings {
            schema_version: 10,
            ..Default::default()
        };
        assert!(!migrate_bump_schema_version(&mut s, true, 10));
        assert_eq!(s.schema_version, 10);
    }

    #[test]
    fn migrate_bump_skips_when_above_target() {
        let mut s = AppSettings {
            schema_version: 12,
            ..Default::default()
        };
        assert!(!migrate_bump_schema_version(&mut s, true, 10));
        assert_eq!(s.schema_version, 12);
    }

    #[test]
    fn migrate_bump_applies_when_below_target() {
        let mut s = AppSettings {
            schema_version: 8,
            ..Default::default()
        };
        assert!(migrate_bump_schema_version(&mut s, true, 10));
        assert_eq!(s.schema_version, 10);
    }

    #[test]
    fn migrate_bump_forces_write_when_schema_version_absent() {
        let mut s = AppSettings {
            schema_version: 10,
            ..Default::default()
        };
        assert!(migrate_bump_schema_version(&mut s, false, 10));
    }

    #[test]
    fn migrate_disable_upstream_timeouts_resets_nonzero_values() {
        let mut s = AppSettings {
            schema_version: 5,
            upstream_first_byte_timeout_seconds: 30,
            upstream_stream_idle_timeout_seconds: 60,
            upstream_request_timeout_non_streaming_seconds: 120,
            ..Default::default()
        };
        assert!(migrate_disable_upstream_timeouts(&mut s, true));
        assert_eq!(s.upstream_first_byte_timeout_seconds, 0);
        assert_eq!(s.upstream_stream_idle_timeout_seconds, 0);
        assert_eq!(s.upstream_request_timeout_non_streaming_seconds, 0);
        assert_eq!(s.schema_version, SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS);
    }

    #[test]
    fn migrate_disable_upstream_timeouts_skips_when_already_migrated() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS,
            upstream_first_byte_timeout_seconds: 30,
            ..Default::default()
        };
        assert!(!migrate_disable_upstream_timeouts(&mut s, true));
        assert_eq!(s.upstream_first_byte_timeout_seconds, 30);
    }

    #[test]
    fn migrate_enable_default_upstream_timeouts_preserves_disabled_values() {
        let mut s = AppSettings {
            schema_version: 26,
            upstream_first_byte_timeout_seconds: 0,
            upstream_stream_idle_timeout_seconds: 0,
            ..Default::default()
        };

        assert!(migrate_enable_default_upstream_timeouts(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS
        );
        assert_eq!(s.upstream_first_byte_timeout_seconds, 0);
        assert_eq!(s.upstream_stream_idle_timeout_seconds, 0);
    }

    #[test]
    fn migrate_enable_default_upstream_timeouts_keeps_existing_nonzero_values() {
        let mut s = AppSettings {
            schema_version: 26,
            upstream_first_byte_timeout_seconds: 15,
            upstream_stream_idle_timeout_seconds: 45,
            ..Default::default()
        };

        assert!(migrate_enable_default_upstream_timeouts(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS
        );
        assert_eq!(s.upstream_first_byte_timeout_seconds, 15);
        assert_eq!(s.upstream_stream_idle_timeout_seconds, 45);
    }

    #[test]
    fn gateway_listen_mode_default_is_localhost() {
        assert_eq!(
            super::super::types::GatewayListenMode::default(),
            super::super::types::GatewayListenMode::Localhost,
        );
    }

    #[test]
    fn app_settings_default_has_current_schema_version() {
        let s = AppSettings::default();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn app_settings_default_has_expected_port() {
        let s = AppSettings::default();
        assert_eq!(s.preferred_port, DEFAULT_GATEWAY_PORT);
    }

    #[test]
    fn migrate_remove_codex_reasoning_guard_advances_schema_49() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_REMOVE_CODEX_REASONING_GUARD - 1,
            ..Default::default()
        };

        assert!(migrate_remove_codex_reasoning_guard(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_REMOVE_CODEX_REASONING_GUARD
        );
    }

    #[test]
    fn app_settings_default_shows_home_heatmap() {
        let s = AppSettings::default();
        assert!(s.show_home_heatmap);
    }

    #[test]
    fn app_settings_default_shows_home_usage() {
        let s = AppSettings::default();
        assert!(s.show_home_usage);
    }

    #[test]
    fn app_settings_default_has_empty_codex_home_override() {
        let s = AppSettings::default();
        assert!(s.codex_home_override.is_empty());
    }

    #[test]
    fn app_settings_default_uses_user_home_default_codex_mode() {
        let s = AppSettings::default();
        assert_eq!(s.codex_home_mode, CodexHomeMode::UserHomeDefault);
    }

    #[test]
    fn app_settings_default_uses_last15_home_usage_period() {
        use super::super::types::HomeUsagePeriod;

        let s = AppSettings::default();
        assert_eq!(s.home_usage_period, HomeUsagePeriod::Last15);
    }

    #[test]
    fn app_settings_default_sets_cli_priority_order() {
        let s = AppSettings::default();
        assert_eq!(s.cli_priority_order, default_cli_priority_order());
    }

    #[test]
    fn app_settings_default_cache_anomaly_monitor_disabled() {
        let s = AppSettings::default();
        assert!(!s.enable_cache_anomaly_monitor);
    }

    #[test]
    fn app_settings_default_codex_oauth_compatible_proxy_mode_disabled() {
        let s = AppSettings::default();
        assert!(!s.codex_oauth_compatible_proxy_mode);
    }

    #[test]
    fn migrate_add_codex_oauth_compatible_proxy_mode_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 32,
            ..Default::default()
        };
        assert!(migrate_add_codex_oauth_compatible_proxy_mode(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_OAUTH_COMPATIBLE_PROXY_MODE
        );
        assert!(!s.codex_oauth_compatible_proxy_mode);
    }

    #[test]
    fn migrate_update_releases_url_to_fork_rewrites_legacy_default() {
        let mut s = AppSettings {
            schema_version: 35,
            update_releases_url: LEGACY_UPDATE_RELEASES_URL.to_string(),
            ..Default::default()
        };
        assert!(migrate_update_releases_url_to_fork(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK);
        assert_eq!(s.update_releases_url, DEFAULT_UPDATE_RELEASES_URL);
    }

    #[test]
    fn migrate_update_releases_url_to_fork_preserves_custom_url() {
        let mut s = AppSettings {
            schema_version: 35,
            update_releases_url: "https://mirror.example.invalid/releases".to_string(),
            ..Default::default()
        };
        assert!(migrate_update_releases_url_to_fork(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK);
        assert_eq!(
            s.update_releases_url,
            "https://mirror.example.invalid/releases"
        );
    }

    #[test]
    fn migrate_add_cache_anomaly_monitor_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 14,
            ..Default::default()
        };
        assert!(migrate_add_cache_anomaly_monitor(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CACHE_ANOMALY_MONITOR);
    }

    #[test]
    fn migrate_add_wsl_host_address_mode_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 15,
            ..Default::default()
        };
        assert!(migrate_add_wsl_host_address_mode(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_WSL_HOST_ADDRESS_MODE);
    }

    #[test]
    fn migrate_add_show_home_heatmap_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 19,
            ..Default::default()
        };
        assert!(migrate_add_show_home_heatmap(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_SHOW_HOME_HEATMAP);
    }

    #[test]
    fn migrate_add_home_usage_period_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 20,
            ..Default::default()
        };
        assert!(migrate_add_home_usage_period(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_HOME_USAGE_PERIOD);
    }

    #[test]
    fn migrate_add_show_home_usage_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 21,
            ..Default::default()
        };
        assert!(migrate_add_show_home_usage(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_SHOW_HOME_USAGE);
    }

    #[test]
    fn migrate_add_codex_home_override_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 22,
            ..Default::default()
        };
        assert!(migrate_add_codex_home_override(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CODEX_HOME_OVERRIDE);
    }

    #[test]
    fn migrate_add_codex_home_mode_bumps_schema_version_and_defaults_to_user_home() {
        let mut s = AppSettings {
            schema_version: 23,
            ..Default::default()
        };
        assert!(migrate_add_codex_home_mode(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CODEX_HOME_MODE);
        assert_eq!(s.codex_home_mode, CodexHomeMode::UserHomeDefault);
    }

    #[test]
    fn migrate_add_codex_home_mode_preserves_legacy_custom_override_as_custom_mode() {
        let mut s = AppSettings {
            schema_version: 23,
            codex_home_override: r"D:\Work\.codex".to_string(),
            ..Default::default()
        };
        assert!(migrate_add_codex_home_mode(&mut s, true));
        assert_eq!(s.codex_home_mode, CodexHomeMode::Custom);
    }

    #[test]
    fn sanitize_cli_priority_order_normalizes_invalid_duplicates_and_missing() {
        let mut s = AppSettings {
            cli_priority_order: vec![
                "codex".to_string(),
                "unknown".to_string(),
                "codex".to_string(),
                "claude".to_string(),
            ],
            ..Default::default()
        };
        assert!(sanitize_cli_priority_order(&mut s));
        assert_eq!(
            s.cli_priority_order,
            vec![
                "codex".to_string(),
                "claude".to_string(),
                "gemini".to_string(),
                "grok".to_string()
            ]
        );
    }

    #[test]
    fn migrate_add_cli_priority_order_bumps_schema_and_fills_default_order() {
        let mut s = AppSettings {
            schema_version: 28,
            cli_priority_order: Vec::new(),
            ..Default::default()
        };
        assert!(migrate_add_cli_priority_order(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CLI_PRIORITY_ORDER);
        assert_eq!(s.cli_priority_order, default_cli_priority_order());
    }

    #[test]
    fn normalize_codex_home_override_trims_and_drops_config_toml() {
        assert_eq!(
            normalize_codex_home_override(r" C:\Users\me\.codex\config.toml "),
            r"C:\Users\me\.codex"
        );
        assert_eq!(normalize_codex_home_override("config.toml"), "");
    }

    #[test]
    fn sanitize_upstream_retry_policy_repairs_rules_without_broadening_empty_content() {
        let mut policy = UpstreamRetryPolicy {
            enabled: true,
            http_rules: vec![
                crate::settings::UpstreamHttpRetryRule::status_only(200),
                crate::settings::UpstreamHttpRetryRule {
                    enabled: true,
                    status_code: 503,
                    body_contains: vec![" LIMIT ".to_string(), "limit".to_string()],
                    description: " retry\nrule ".to_string(),
                },
                crate::settings::UpstreamHttpRetryRule {
                    enabled: true,
                    status_code: 504,
                    body_contains: vec!["   ".to_string()],
                    description: String::new(),
                },
            ],
            transport_errors: vec![],
            max_retries: 999,
            backoff_ms: 999_999,
            counts_toward_circuit_breaker: false,
        };

        assert!(sanitize_upstream_retry_policy(&mut policy));
        assert_eq!(policy.http_rules.len(), 2);
        assert_eq!(policy.http_rules[0].body_contains, vec!["limit"]);
        assert_eq!(policy.http_rules[0].description, "retryrule");
        assert!(!policy.http_rules[1].enabled);
        assert!(policy.enabled);
        assert!(policy.transport_errors.is_empty());
        assert_eq!(policy.max_retries, MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES);
        assert_eq!(policy.backoff_ms, MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS);
    }

    #[test]
    fn normalize_upstream_retry_policy_for_write_rejects_invalid_rules() {
        let mut policy = UpstreamRetryPolicy {
            http_rules: vec![crate::settings::UpstreamHttpRetryRule {
                enabled: false,
                status_code: 399,
                body_contains: Vec::new(),
                description: String::new(),
            }],
            ..Default::default()
        };
        assert!(normalize_upstream_retry_policy_for_write(&mut policy).is_err());

        let mut policy = UpstreamRetryPolicy {
            http_rules: vec![crate::settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 503,
                body_contains: vec![" ".to_string()],
                description: String::new(),
            }],
            transport_errors: Vec::new(),
            ..Default::default()
        };
        assert!(normalize_upstream_retry_policy_for_write(&mut policy).is_err());

        let mut missing_status: UpstreamRetryPolicy = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "http_rules": [{
                "enabled": true,
                "body_contains": [],
                "description": "missing status"
            }],
            "transport_errors": []
        }))
        .expect("deserialize write-shaped policy");
        assert_eq!(missing_status.http_rules[0].status_code, 0);
        assert!(normalize_upstream_retry_policy_for_write(&mut missing_status).is_err());

        let mut missing_body: UpstreamRetryPolicy = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "http_rules": [{
                "enabled": true,
                "status_code": 503,
                "description": "missing body condition field"
            }],
            "transport_errors": []
        }))
        .expect("deserialize incomplete write-shaped policy");
        assert_eq!(missing_body.http_rules[0].body_contains, vec![""]);
        assert!(normalize_upstream_retry_policy_for_write(&mut missing_body).is_err());

        let mut null_rules: UpstreamRetryPolicy = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "http_rules": null,
            "transport_errors": []
        }))
        .expect("deserialize null rules for load repair");
        assert_eq!(null_rules.http_rules[0].status_code, 0);
        assert!(normalize_upstream_retry_policy_for_write(&mut null_rules).is_err());

        let mut lowercase_expansion = UpstreamRetryPolicy {
            http_rules: vec![crate::settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 503,
                body_contains: vec!["İ".repeat(MAX_UPSTREAM_RETRY_POLICY_BODY_CONTAINS_CHARS)],
                description: String::new(),
            }],
            transport_errors: Vec::new(),
            ..Default::default()
        };
        assert!(normalize_upstream_retry_policy_for_write(&mut lowercase_expansion).is_err());
    }

    #[test]
    fn normalize_upstream_retry_policy_for_write_canonicalizes_content_and_description() {
        let mut policy = UpstreamRetryPolicy {
            http_rules: vec![crate::settings::UpstreamHttpRetryRule {
                enabled: true,
                status_code: 599,
                body_contains: vec![" Quota ".to_string(), "quota".to_string()],
                description: " Temporary quota ".to_string(),
            }],
            transport_errors: Vec::new(),
            ..Default::default()
        };
        assert!(normalize_upstream_retry_policy_for_write(&mut policy).unwrap());
        assert_eq!(policy.http_rules[0].body_contains, vec!["quota"]);
        assert_eq!(policy.http_rules[0].description, "Temporary quota");
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

    #[test]
    fn migrate_add_grok_proxy_preferences_bumps_schema_without_initializing_preferences() {
        let mut settings = AppSettings {
            schema_version: SCHEMA_VERSION_REMOVE_CODEX_REASONING_GUARD,
            ..Default::default()
        };

        assert!(migrate_add_grok_proxy_preferences(&mut settings, true));
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_ADD_GROK_PROXY_PREFERENCES
        );
        assert_eq!(settings.grok_proxy_preferences, None);
    }

    #[test]
    fn migrate_add_image_gen_storage_dir_bumps_schema_without_initializing_dir() {
        let mut settings = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_GROK_PROXY_PREFERENCES,
            ..Default::default()
        };

        assert!(migrate_add_image_gen_storage_dir(&mut settings, true));
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_ADD_IMAGE_GEN_STORAGE_DIR
        );
        assert_eq!(settings.image_gen_storage_dir, None);
    }

    #[test]
    fn migrate_add_image_gen_storage_roots_bumps_schema_with_empty_allowlist() {
        let mut settings = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_IMAGE_GEN_STORAGE_DIR,
            ..Default::default()
        };

        assert!(migrate_add_image_gen_storage_roots(&mut settings, true));
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_ADD_IMAGE_GEN_STORAGE_ROOTS
        );
        assert!(settings.image_gen_storage_roots.is_empty());
    }

    #[test]
    fn migrate_add_upstream_http_retry_rules_bumps_schema_to_53() {
        let mut settings = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_IMAGE_GEN_STORAGE_ROOTS,
            ..Default::default()
        };

        assert!(migrate_add_upstream_http_retry_rules(&mut settings, true));
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_ADD_UPSTREAM_HTTP_RETRY_RULES
        );
    }

    #[test]
    fn normalize_codex_home_override_keeps_directory_input() {
        assert_eq!(
            normalize_codex_home_override(r"  C:\Users\me\.codex  "),
            r"C:\Users\me\.codex"
        );
    }

    #[test]
    fn normalize_codex_home_override_converts_config_toml_to_parent_dir() {
        assert_eq!(
            normalize_codex_home_override(r"C:\Users\me\.codex\config.toml"),
            r"C:\Users\me\.codex"
        );
    }

    #[test]
    fn sanitize_codex_home_override_trims_and_normalizes() {
        let mut s = AppSettings {
            codex_home_mode: CodexHomeMode::Custom,
            codex_home_override: " ~/.codex/config.toml ".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_home_override(&mut s));
        assert_eq!(s.codex_home_override, "~/.codex");
    }

    #[test]
    fn sanitize_codex_home_override_demotes_empty_custom_mode_to_user_home_default() {
        let mut s = AppSettings {
            codex_home_mode: CodexHomeMode::Custom,
            codex_home_override: "   ".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_home_override(&mut s));
        assert_eq!(s.codex_home_mode, CodexHomeMode::UserHomeDefault);
        assert!(s.codex_home_override.is_empty());
    }

    #[test]
    fn sanitize_codex_home_override_clears_override_when_mode_is_not_custom() {
        let mut s = AppSettings {
            codex_home_mode: CodexHomeMode::FollowCodexHome,
            codex_home_override: r"D:\Work\.codex".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_home_override(&mut s));
        assert_eq!(s.codex_home_mode, CodexHomeMode::FollowCodexHome);
        assert!(s.codex_home_override.is_empty());
    }
}
