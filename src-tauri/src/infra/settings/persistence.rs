//! Usage: Settings file read/write, cache layer, path resolution, and JSON parsing.

use super::defaults::*;
use super::migration::{
    normalize_cli_priority_order, normalize_codex_home_override, repair_settings,
};
use super::types::{
    AppSettings, CodexHomeMode, CodexReasoningGuardCompareMode, CodexReasoningGuardExhaustedAction,
    CodexReasoningGuardRetryPolicy, CodexReasoningGuardRuleMode,
    CodexReasoningGuardTemplateFilterField, CodexReasoningGuardTemplateFilterOperator,
    GatewayListenMode, WslHostAddressMode,
};
use crate::app_paths;
use crate::shared::error::AppResult;
use crate::shared::fs::read_file_with_max_len;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};
use std::time::Instant;
use tauri::Manager;

static LOG_RETENTION_DAYS_FAIL_OPEN_WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct CachedSettings {
    path: PathBuf,
    data: AppSettings,
    last_updated: Instant,
}

static SETTINGS_CACHE: OnceLock<RwLock<Option<CachedSettings>>> = OnceLock::new();

fn cache_settings(path: &Path, settings: &AppSettings) {
    let cache = SETTINGS_CACHE.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = cache.write() {
        *guard = Some(CachedSettings {
            path: path.to_path_buf(),
            data: settings.clone(),
            last_updated: Instant::now(),
        });
    }
}

fn settings_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<PathBuf> {
    Ok(app_paths::app_data_dir(app)?.join("settings.json"))
}

fn legacy_settings_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Option<PathBuf>> {
    let Some(config_dir) = app.path().config_dir().ok() else {
        return Ok(None);
    };

    Ok(Some(
        config_dir.join(LEGACY_IDENTIFIER).join("settings.json"),
    ))
}

fn invalid_settings_json(reason: impl std::fmt::Display) -> crate::shared::error::AppError {
    format!("SEC_INVALID_INPUT: invalid settings.json: {reason}").into()
}

fn read_settings_json_file(path: &Path) -> AppResult<String> {
    let bytes = read_file_with_max_len(path, SETTINGS_FILE_MAX_BYTES)
        .map_err(|e| format!("failed to read settings: {e}"))?;
    String::from_utf8(bytes).map_err(|e| invalid_settings_json(format!("expected UTF-8: {e}")))
}

fn ensure_settings_file_len(bytes: &[u8]) -> AppResult<()> {
    if bytes.len() > SETTINGS_FILE_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: settings.json too large (max {SETTINGS_FILE_MAX_BYTES} bytes)"
        )
        .into());
    }
    Ok(())
}

fn validate_update_releases_url(value: &str) -> AppResult<()> {
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(());
    }
    if raw.len() > MAX_UPDATE_RELEASES_URL_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: update_releases_url must be <= {MAX_UPDATE_RELEASES_URL_LEN} characters"
        )
        .into());
    }

    let parsed = reqwest::Url::parse(raw)
        .map_err(|err| format!("SEC_INVALID_INPUT: invalid update_releases_url: {err}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("SEC_INVALID_INPUT: update_releases_url must use http or https".into());
    }
    if parsed.host_str().is_none() {
        return Err("SEC_INVALID_INPUT: update_releases_url must include a host".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("SEC_INVALID_INPUT: update_releases_url must not include credentials".into());
    }

    Ok(())
}

fn validate_no_control_chars(field: &str, value: &str) -> AppResult<()> {
    if value.chars().any(char::is_control) {
        return Err(
            format!("SEC_INVALID_INPUT: {field} must not contain control characters").into(),
        );
    }
    Ok(())
}

fn validate_non_empty_bounded_string(field: &str, value: &str, max_len: usize) -> AppResult<()> {
    let raw = value.trim();
    if raw.is_empty() {
        return Err(format!("SEC_INVALID_INPUT: {field} cannot be empty").into());
    }
    if raw.len() > max_len {
        return Err(format!("SEC_INVALID_INPUT: {field} must be <= {max_len} characters").into());
    }
    validate_no_control_chars(field, raw)
}

fn validate_optional_bounded_string(field: &str, value: &str, max_len: usize) -> AppResult<()> {
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(());
    }
    if raw.len() > max_len {
        return Err(format!("SEC_INVALID_INPUT: {field} must be <= {max_len} characters").into());
    }
    validate_no_control_chars(field, raw)
}

fn validate_codex_reasoning_guard_values(field: &str, values: &[i64]) -> AppResult<()> {
    if values.is_empty() {
        return Err(format!("SEC_INVALID_INPUT: {field} must not be empty").into());
    }
    if values.len() > MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: {field} must contain <= {MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN} values"
        )
        .into());
    }
    if values
        .iter()
        .any(|value| *value < 0 || *value > MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE)
    {
        return Err(format!(
            "SEC_INVALID_INPUT: {field} values must be between 0 and {MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE}"
        )
        .into());
    }
    Ok(())
}

fn validate_codex_reasoning_guard_template_string_values(
    field: &str,
    values: &[String],
) -> AppResult<()> {
    if values.is_empty() {
        return Err(format!("SEC_INVALID_INPUT: {field} must not be empty").into());
    }
    if values.len() > MAX_CODEX_REASONING_GUARD_STRING_FILTER_LIST_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: {field} must contain <= {MAX_CODEX_REASONING_GUARD_STRING_FILTER_LIST_LEN} values"
        )
        .into());
    }

    let mut seen = HashSet::new();
    for (index, value) in values.iter().enumerate() {
        let item_field = format!("{field}[{index}]");
        let trimmed = value.trim();
        validate_non_empty_bounded_string(
            &item_field,
            trimmed,
            MAX_CODEX_REASONING_GUARD_STRING_FILTER_VALUE_LEN,
        )?;
        if !seen.insert(trimmed.to_string()) {
            return Err(
                format!("SEC_INVALID_INPUT: duplicate {item_field} value {trimmed}").into(),
            );
        }
    }

    Ok(())
}

fn validate_codex_reasoning_guard_template_filter(
    field_prefix: &str,
    filter: &super::types::CodexReasoningGuardTemplateFilter,
) -> AppResult<()> {
    validate_non_empty_bounded_string(
        &format!("{field_prefix}.id"),
        &filter.id,
        MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN,
    )?;

    match filter.field {
        CodexReasoningGuardTemplateFilterField::DurationMs
        | CodexReasoningGuardTemplateFilterField::Tps
        | CodexReasoningGuardTemplateFilterField::OutputTokens
        | CodexReasoningGuardTemplateFilterField::InputTokens
        | CodexReasoningGuardTemplateFilterField::TotalTokens
        | CodexReasoningGuardTemplateFilterField::ReasoningTokens => match filter.operator {
            CodexReasoningGuardTemplateFilterOperator::Equals
            | CodexReasoningGuardTemplateFilterOperator::NotEquals
            | CodexReasoningGuardTemplateFilterOperator::LessThan
            | CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
            | CodexReasoningGuardTemplateFilterOperator::GreaterThan
            | CodexReasoningGuardTemplateFilterOperator::GreaterThanOrEqual => {
                let Some(value) = filter.number_value else {
                    return Err(format!(
                        "SEC_INVALID_INPUT: {field_prefix}.number_value is required"
                    )
                    .into());
                };
                if !value.is_finite() || value < 0.0 {
                    return Err(format!(
                        "SEC_INVALID_INPUT: {field_prefix}.number_value must be a non-negative finite number"
                    )
                    .into());
                }
            }
            CodexReasoningGuardTemplateFilterOperator::In
            | CodexReasoningGuardTemplateFilterOperator::NotIn => {
                return Err(format!(
                    "SEC_INVALID_INPUT: {field_prefix}.operator does not support numeric fields"
                )
                .into());
            }
        },
        CodexReasoningGuardTemplateFilterField::FinalAnswerOnly
        | CodexReasoningGuardTemplateFilterField::HasToolCall
        | CodexReasoningGuardTemplateFilterField::HasReasoningItem
        | CodexReasoningGuardTemplateFilterField::CommentaryObserved => match filter.operator {
            CodexReasoningGuardTemplateFilterOperator::Equals
            | CodexReasoningGuardTemplateFilterOperator::NotEquals => {
                if filter.bool_value.is_none() {
                    return Err(format!(
                        "SEC_INVALID_INPUT: {field_prefix}.bool_value is required"
                    )
                    .into());
                }
            }
            CodexReasoningGuardTemplateFilterOperator::LessThan
            | CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
            | CodexReasoningGuardTemplateFilterOperator::GreaterThan
            | CodexReasoningGuardTemplateFilterOperator::GreaterThanOrEqual
            | CodexReasoningGuardTemplateFilterOperator::In
            | CodexReasoningGuardTemplateFilterOperator::NotIn => {
                return Err(format!(
                    "SEC_INVALID_INPUT: {field_prefix}.operator does not support boolean fields"
                )
                .into());
            }
        },
        CodexReasoningGuardTemplateFilterField::RequestReasoningEffort
        | CodexReasoningGuardTemplateFilterField::RequestedModel => match filter.operator {
            CodexReasoningGuardTemplateFilterOperator::Equals
            | CodexReasoningGuardTemplateFilterOperator::NotEquals => {
                let value = filter.string_value.as_deref().unwrap_or_default();
                validate_non_empty_bounded_string(
                    &format!("{field_prefix}.string_value"),
                    value,
                    MAX_CODEX_REASONING_GUARD_STRING_FILTER_VALUE_LEN,
                )?;
            }
            CodexReasoningGuardTemplateFilterOperator::In
            | CodexReasoningGuardTemplateFilterOperator::NotIn => {
                validate_codex_reasoning_guard_template_string_values(
                    &format!("{field_prefix}.string_values"),
                    &filter.string_values,
                )?;
            }
            CodexReasoningGuardTemplateFilterOperator::LessThan
            | CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
            | CodexReasoningGuardTemplateFilterOperator::GreaterThan
            | CodexReasoningGuardTemplateFilterOperator::GreaterThanOrEqual => {
                return Err(format!(
                    "SEC_INVALID_INPUT: {field_prefix}.operator does not support string fields"
                )
                .into());
            }
        },
    }

    Ok(())
}

fn validate_codex_reasoning_guard_templates(settings: &AppSettings) -> AppResult<()> {
    validate_non_empty_bounded_string(
        "codex_reasoning_guard_active_template_id",
        &settings.codex_reasoning_guard_active_template_id,
        MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN,
    )?;

    if settings.codex_reasoning_guard_custom_templates.len()
        > MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_custom_templates must contain <= {MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES} templates"
        )
        .into());
    }

    let mut seen_template_ids = HashSet::new();
    for (template_index, template) in settings
        .codex_reasoning_guard_custom_templates
        .iter()
        .enumerate()
    {
        let template_prefix = format!("codex_reasoning_guard_custom_templates[{template_index}]");
        let template_id = template.id.trim();
        validate_non_empty_bounded_string(
            &format!("{template_prefix}.id"),
            template_id,
            MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN,
        )?;
        if is_reserved_codex_reasoning_guard_template_id(template_id) {
            return Err(format!(
                "SEC_INVALID_INPUT: codex reasoning guard custom template id {template_id} is reserved"
            )
            .into());
        }
        if !seen_template_ids.insert(template_id.to_string()) {
            return Err(format!(
                "SEC_INVALID_INPUT: duplicate codex reasoning guard template id {template_id}"
            )
            .into());
        }
        validate_non_empty_bounded_string(
            &format!("{template_prefix}.name"),
            &template.name,
            MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN,
        )?;
        validate_optional_bounded_string(
            &format!("{template_prefix}.description"),
            &template.description,
            MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN,
        )?;
        if template.rules.is_empty() {
            return Err(
                format!("SEC_INVALID_INPUT: {template_prefix}.rules must not be empty").into(),
            );
        }
        if template.rules.len() > MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES {
            return Err(format!(
                "SEC_INVALID_INPUT: {template_prefix}.rules must contain <= {MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES} rules"
            )
            .into());
        }

        let mut seen_rule_ids = HashSet::new();
        let mut seen_tokens = HashSet::new();
        let mut catch_all_wildcard_seen = false;
        for (rule_index, rule) in template.rules.iter().enumerate() {
            let rule_prefix = format!("{template_prefix}.rules[{rule_index}]");
            let rule_id = rule.id.trim();
            validate_non_empty_bounded_string(
                &format!("{rule_prefix}.id"),
                rule_id,
                MAX_CODEX_REASONING_GUARD_TEMPLATE_ID_LEN,
            )?;
            if !seen_rule_ids.insert(rule_id.to_string()) {
                return Err(format!(
                    "SEC_INVALID_INPUT: duplicate codex reasoning guard rule id {rule_id}"
                )
                .into());
            }
            validate_non_empty_bounded_string(
                &format!("{rule_prefix}.name"),
                &rule.name,
                MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN,
            )?;
            if let Some(token) = rule.reasoning_tokens {
                if !(0..=MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE).contains(&token) {
                    return Err(format!(
                        "SEC_INVALID_INPUT: {rule_prefix}.reasoning_tokens must be between 0 and {MAX_CODEX_REASONING_GUARD_REASONING_TOKEN_VALUE}"
                    )
                    .into());
                }
                if !seen_tokens.insert(token) {
                    return Err(format!(
                        "SEC_INVALID_INPUT: duplicate codex reasoning guard template token {token}"
                    )
                    .into());
                }
            } else if rule.filters.is_empty() && catch_all_wildcard_seen {
                return Err(format!(
                    "SEC_INVALID_INPUT: {template_prefix}.rules may contain only one catch-all wildcard rule"
                )
                .into());
            } else {
                catch_all_wildcard_seen |= rule.filters.is_empty();
            }

            if rule.filters.len() > MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS {
                return Err(format!(
                    "SEC_INVALID_INPUT: {rule_prefix}.filters must contain <= {MAX_CODEX_REASONING_GUARD_TEMPLATE_RULE_FILTERS} filters"
                )
                .into());
            }
            let mut seen_filter_ids = HashSet::new();
            for (filter_index, filter) in rule.filters.iter().enumerate() {
                let filter_prefix = format!("{rule_prefix}.filters[{filter_index}]");
                let filter_id = filter.id.trim();
                if !seen_filter_ids.insert(filter_id.to_string()) {
                    return Err(format!(
                        "SEC_INVALID_INPUT: duplicate codex reasoning guard filter id {filter_id}"
                    )
                    .into());
                }
                validate_codex_reasoning_guard_template_filter(&filter_prefix, filter)?;
            }
        }
    }

    let active_template_id = settings.codex_reasoning_guard_active_template_id.trim();
    if !is_builtin_codex_reasoning_guard_template_id(active_template_id)
        && !seen_template_ids.contains(active_template_id)
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_active_template_id does not reference a built-in or custom template: {active_template_id}"
        )
        .into());
    }

    Ok(())
}

fn is_builtin_codex_reasoning_guard_template_id(template_id: &str) -> bool {
    matches!(
        template_id,
        CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID
            | CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
    )
}

fn is_reserved_codex_reasoning_guard_template_id(template_id: &str) -> bool {
    matches!(
        template_id,
        CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID
            | CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
            | CODEX_REASONING_GUARD_TEMPLATE_LEGACY_COMPATIBILITY_ID
    )
}

pub(super) fn parse_settings_json(
    content: &str,
) -> AppResult<(AppSettings, bool, serde_json::Value)> {
    let raw: serde_json::Value = serde_json::from_str(content).map_err(invalid_settings_json)?;
    let schema_version_present = raw.get("schema_version").is_some();
    let settings: AppSettings =
        serde_json::from_value(raw.clone()).map_err(invalid_settings_json)?;
    Ok((settings, schema_version_present, raw))
}

pub(super) fn canonical_settings_json(settings: &AppSettings) -> AppResult<serde_json::Value> {
    let mut serialized =
        serde_json::to_value(settings).map_err(|e| format!("failed to serialize settings: {e}"))?;

    let serialized_obj = serialized.as_object_mut().ok_or_else(|| {
        "failed to serialize settings: expected settings to serialize as an object".to_string()
    })?;

    // Keep the full managed snapshot on disk so future default changes do not
    // reinterpret omitted keys as a new user choice after an upgrade.
    if !serialized_obj.contains_key("schema_version") {
        serialized_obj.insert(
            "schema_version".to_string(),
            serde_json::json!(SCHEMA_VERSION),
        );
    }

    Ok(serialized)
}

pub fn read<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<AppSettings> {
    let cache = SETTINGS_CACHE.get_or_init(|| RwLock::new(None));
    let path = settings_path(app)?;

    if let Ok(guard) = cache.read() {
        if let Some(cached) = guard.as_ref() {
            if cached.path == path && cached.last_updated.elapsed() < CACHE_TTL {
                return Ok(cached.data.clone());
            }
        }
    }

    if !path.exists() {
        if let Some(legacy_path) = legacy_settings_path(app)? {
            if legacy_path.exists() {
                let content = read_settings_json_file(&legacy_path)?;
                let (settings, schema_version_present, raw_settings_json) =
                    parse_settings_json(&content)?;

                if settings.preferred_port < 1024 {
                    return Err(
                        "SEC_INVALID_INPUT: invalid settings.json: preferred_port must be between 1024 and 65535"
                            .to_string()
                            .into(),
                    );
                }
                if settings.log_retention_days == 0 {
                    return Err(
                        "SEC_INVALID_INPUT: invalid settings.json: log_retention_days must be >= 1"
                            .to_string()
                            .into(),
                    );
                }

                // Best-effort migration: copy legacy settings into the new dotdir (do not delete legacy file).
                let mut settings = settings;
                let repaired =
                    repair_settings(&mut settings, schema_version_present, &raw_settings_json)?;
                validate_bounds(&settings)?;
                if repaired {
                    // best-effort: persist sanitized defaults
                }
                let _ = write(app, &settings);
                cache_settings(&path, &settings);
                return Ok(settings);
            }
        }

        let settings = AppSettings::default();
        // Best-effort: create default settings.json on first read to make the config discoverable/editable.
        let _ = write(app, &settings);
        cache_settings(&path, &settings);
        return Ok(settings);
    }

    let content = read_settings_json_file(&path)?;
    let (mut settings, schema_version_present, raw_settings_json) = parse_settings_json(&content)?;

    if settings.preferred_port < 1024 {
        return Err(
            "SEC_INVALID_INPUT: invalid settings.json: preferred_port must be between 1024 and 65535"
                .to_string()
                .into(),
        );
    }
    if settings.log_retention_days == 0 {
        return Err(
            "SEC_INVALID_INPUT: invalid settings.json: log_retention_days must be >= 1"
                .to_string()
                .into(),
        );
    }

    let repaired = repair_settings(&mut settings, schema_version_present, &raw_settings_json)?;
    validate_bounds(&settings)?;
    if repaired {
        // Best-effort: persist repaired values while keeping read semantics.
        let _ = write(app, &settings);
    }
    cache_settings(&path, &settings);

    Ok(settings)
}

pub fn log_retention_days_fail_open<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> u32 {
    match read(app) {
        Ok(cfg) => cfg.log_retention_days,
        Err(err) => {
            if !LOG_RETENTION_DAYS_FAIL_OPEN_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    default = DEFAULT_LOG_RETENTION_DAYS,
                    "settings read failed, using default log retention days: {}",
                    err
                );
            }
            DEFAULT_LOG_RETENTION_DAYS
        }
    }
}

pub(crate) fn validate_bounds(settings: &AppSettings) -> AppResult<()> {
    if settings.preferred_port < 1024 {
        return Err("SEC_INVALID_INPUT: preferred_port must be between 1024 and 65535".into());
    }
    if settings.gateway_listen_mode == GatewayListenMode::Custom {
        crate::shared::listen_address::parse_custom_listen_address(
            &settings.gateway_custom_listen_address,
        )?;
    }
    if settings.wsl_host_address_mode == WslHostAddressMode::Custom {
        crate::shared::listen_address::parse_custom_host_address(
            &settings.wsl_custom_host_address,
        )?;
    }
    if settings.upstream_proxy_url.len() > MAX_UPSTREAM_PROXY_URL_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_proxy_url must be <= {MAX_UPSTREAM_PROXY_URL_LEN} characters"
        )
        .into());
    }
    if settings.upstream_proxy_username.len() > MAX_UPSTREAM_PROXY_USERNAME_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_proxy_username must be <= {MAX_UPSTREAM_PROXY_USERNAME_LEN} characters"
        )
        .into());
    }
    if settings.upstream_proxy_password.len() > MAX_UPSTREAM_PROXY_PASSWORD_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_proxy_password must be <= {MAX_UPSTREAM_PROXY_PASSWORD_LEN} characters"
        )
        .into());
    }
    for (field, value) in [
        (
            "cx2cc_fallback_model_opus",
            settings.cx2cc_fallback_model_opus.as_str(),
        ),
        (
            "cx2cc_fallback_model_sonnet",
            settings.cx2cc_fallback_model_sonnet.as_str(),
        ),
        (
            "cx2cc_fallback_model_haiku",
            settings.cx2cc_fallback_model_haiku.as_str(),
        ),
        (
            "cx2cc_fallback_model_main",
            settings.cx2cc_fallback_model_main.as_str(),
        ),
    ] {
        validate_non_empty_bounded_string(field, value, MAX_CX2CC_MODEL_NAME_LEN)?;
    }
    validate_non_empty_bounded_string(
        "codex_provider_test_model",
        &settings.codex_provider_test_model,
        MAX_CODEX_PROVIDER_TEST_MODEL_NAME_LEN,
    )?;
    validate_optional_bounded_string(
        "cx2cc_model_reasoning_effort",
        &settings.cx2cc_model_reasoning_effort,
        MAX_CX2CC_OPTIONAL_FIELD_LEN,
    )?;
    validate_optional_bounded_string(
        "cx2cc_service_tier",
        &settings.cx2cc_service_tier,
        MAX_CX2CC_OPTIONAL_FIELD_LEN,
    )?;
    validate_codex_reasoning_guard_values(
        "codex_reasoning_guard_reasoning_equals",
        &settings.codex_reasoning_guard_reasoning_equals,
    )?;
    if settings.codex_reasoning_guard_backoff_after_hits
        > MAX_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_backoff_after_hits must be <= {MAX_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_backoff_ms > MAX_CODEX_REASONING_GUARD_BACKOFF_MS {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_backoff_ms must be <= {MAX_CODEX_REASONING_GUARD_BACKOFF_MS}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_immediate_retry_budget
        > MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_immediate_retry_budget must be <= {MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_delayed_retry_budget
        > MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_delayed_retry_budget must be <= {MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_delayed_retry_ms > MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_delayed_retry_ms must be <= {MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS}"
        )
        .into());
    }
    match settings.codex_reasoning_guard_rule_mode {
        CodexReasoningGuardRuleMode::ReasoningTokens
        | CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh => {}
    }
    match settings.codex_reasoning_guard_compare_mode {
        CodexReasoningGuardCompareMode::Equals
        | CodexReasoningGuardCompareMode::LessThanOrEqual => {}
    }
    match settings.codex_reasoning_guard_exhausted_action {
        CodexReasoningGuardExhaustedAction::ReturnError
        | CodexReasoningGuardExhaustedAction::SwitchProvider
        | CodexReasoningGuardExhaustedAction::SwitchModel => {}
    }
    match settings.codex_reasoning_guard_retry_policy {
        CodexReasoningGuardRetryPolicy::Single | CodexReasoningGuardRetryPolicy::Concurrent => {}
    }
    if settings.codex_reasoning_guard_concurrent_max == 0
        || settings.codex_reasoning_guard_concurrent_max > MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_concurrent_max must be between 1 and {MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_concurrent_interval_ms
        > MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_concurrent_interval_ms must be <= {MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_concurrent_max_attempts
        > MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_concurrent_max_attempts must be <= {MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_continuation_max_rounds == 0
        || settings.codex_reasoning_guard_continuation_max_rounds
            > MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_continuation_max_rounds must be between 1 and {MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_continuation_max_output_tokens
        > MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_continuation_max_output_tokens must be <= {MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS}"
        )
        .into());
    }
    if settings.codex_reasoning_guard_model_fallbacks.len()
        > MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_model_fallbacks must contain <= {MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN} models"
        )
        .into());
    }
    let mut seen_fallback_models = HashSet::new();
    for (index, model) in settings
        .codex_reasoning_guard_model_fallbacks
        .iter()
        .enumerate()
    {
        let field = format!("codex_reasoning_guard_model_fallbacks[{index}]");
        let model = model.trim();
        validate_non_empty_bounded_string(&field, model, MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN)?;
        if !seen_fallback_models.insert(model.to_string()) {
            return Err(format!(
                "SEC_INVALID_INPUT: duplicate codex reasoning guard model fallback for {model}"
            )
            .into());
        }
    }
    if settings.codex_reasoning_guard_model_rules.len() > MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN
    {
        return Err(format!(
            "SEC_INVALID_INPUT: codex_reasoning_guard_model_rules must contain <= {MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN} rules"
        )
        .into());
    }
    let mut seen_requested_models = HashSet::new();
    for (index, rule) in settings
        .codex_reasoning_guard_model_rules
        .iter()
        .enumerate()
    {
        let field_prefix = format!("codex_reasoning_guard_model_rules[{index}]");
        let requested_model = rule.requested_model.trim();
        validate_non_empty_bounded_string(
            &format!("{field_prefix}.requested_model"),
            requested_model,
            MAX_CODEX_REASONING_GUARD_MODEL_NAME_LEN,
        )?;
        if !seen_requested_models.insert(requested_model.to_string()) {
            return Err(format!(
                "SEC_INVALID_INPUT: duplicate codex reasoning guard model rule for {requested_model}"
            )
            .into());
        }
        match rule.compare_mode {
            CodexReasoningGuardCompareMode::Equals
            | CodexReasoningGuardCompareMode::LessThanOrEqual => {}
        }
        validate_codex_reasoning_guard_values(
            &format!("{field_prefix}.reasoning_equals"),
            &rule.reasoning_equals,
        )?;
    }
    validate_codex_reasoning_guard_templates(settings)?;
    validate_update_releases_url(&settings.update_releases_url)?;
    if settings.log_retention_days == 0 {
        return Err("SEC_INVALID_INPUT: log_retention_days must be >= 1".into());
    }
    if settings.log_retention_days > MAX_LOG_RETENTION_DAYS {
        return Err(format!(
            "SEC_INVALID_INPUT: log_retention_days must be <= {MAX_LOG_RETENTION_DAYS}"
        )
        .into());
    }
    if settings.provider_cooldown_seconds > MAX_PROVIDER_COOLDOWN_SECONDS {
        return Err(format!(
            "SEC_INVALID_INPUT: provider_cooldown_seconds must be <= {MAX_PROVIDER_COOLDOWN_SECONDS}"
        )
        .into());
    }
    if settings.provider_base_url_ping_cache_ttl_seconds == 0 {
        return Err(
            "SEC_INVALID_INPUT: provider_base_url_ping_cache_ttl_seconds must be >= 1".into(),
        );
    }
    if settings.provider_base_url_ping_cache_ttl_seconds
        > MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: provider_base_url_ping_cache_ttl_seconds must be <= {MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS}"
        )
        .into());
    }
    if settings.upstream_first_byte_timeout_seconds > MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_first_byte_timeout_seconds must be <= {MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS}"
        )
        .into());
    }
    if settings.upstream_stream_idle_timeout_seconds > MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_stream_idle_timeout_seconds must be <= {MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS}"
        )
        .into());
    }
    if settings.upstream_stream_idle_timeout_seconds > 0
        && settings.upstream_stream_idle_timeout_seconds < MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_stream_idle_timeout_seconds must be 0 (disabled) or >= {MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS}"
        )
        .into());
    }
    if settings.upstream_request_timeout_non_streaming_seconds
        > MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_request_timeout_non_streaming_seconds must be <= {MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS}"
        )
        .into());
    }
    if settings.response_fixer_max_json_depth == 0 {
        return Err("SEC_INVALID_INPUT: response_fixer_max_json_depth must be >= 1".into());
    }
    if settings.response_fixer_max_json_depth > MAX_RESPONSE_FIXER_MAX_JSON_DEPTH {
        return Err(format!(
            "SEC_INVALID_INPUT: response_fixer_max_json_depth must be <= {MAX_RESPONSE_FIXER_MAX_JSON_DEPTH}"
        )
        .into());
    }
    if settings.response_fixer_max_fix_size == 0 {
        return Err("SEC_INVALID_INPUT: response_fixer_max_fix_size must be >= 1".into());
    }
    if settings.response_fixer_max_fix_size > MAX_RESPONSE_FIXER_MAX_FIX_SIZE {
        return Err(format!(
            "SEC_INVALID_INPUT: response_fixer_max_fix_size must be <= {MAX_RESPONSE_FIXER_MAX_FIX_SIZE}"
        )
        .into());
    }
    if settings.failover_max_attempts_per_provider == 0 {
        return Err("SEC_INVALID_INPUT: failover_max_attempts_per_provider must be >= 1".into());
    }
    if settings.failover_max_providers_to_try == 0 {
        return Err("SEC_INVALID_INPUT: failover_max_providers_to_try must be >= 1".into());
    }
    if settings.upstream_retry_policy.status_codes.len() > MAX_UPSTREAM_RETRY_POLICY_STATUS_CODES {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.status_codes must contain <= {MAX_UPSTREAM_RETRY_POLICY_STATUS_CODES} entries"
        )
        .into());
    }
    if settings
        .upstream_retry_policy
        .status_codes
        .iter()
        .any(|status| !(400..=599).contains(status))
    {
        return Err(
            "SEC_INVALID_INPUT: upstream_retry_policy.status_codes must be within [400, 599]"
                .into(),
        );
    }
    if settings.upstream_retry_policy.transport_errors.len()
        > MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.transport_errors must contain <= {MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS} entries"
        )
        .into());
    }
    if settings.upstream_retry_policy.max_retries > MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.max_retries must be <= {MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES}"
        )
        .into());
    }
    if settings.upstream_retry_policy.backoff_ms > MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS {
        return Err(format!(
            "SEC_INVALID_INPUT: upstream_retry_policy.backoff_ms must be <= {MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS}"
        )
        .into());
    }
    if settings.upstream_retry_policy.enabled
        && settings.upstream_retry_policy.status_codes.is_empty()
        && settings.upstream_retry_policy.transport_errors.is_empty()
    {
        return Err("SEC_INVALID_INPUT: upstream_retry_policy must include at least one status code or transport error when enabled".into());
    }
    if settings.failover_max_attempts_per_provider > MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER {
        return Err(format!(
            "SEC_INVALID_INPUT: failover_max_attempts_per_provider must be <= {MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER}"
        )
        .into());
    }
    if settings.failover_max_providers_to_try > MAX_FAILOVER_MAX_PROVIDERS_TO_TRY {
        return Err(format!(
            "SEC_INVALID_INPUT: failover_max_providers_to_try must be <= {MAX_FAILOVER_MAX_PROVIDERS_TO_TRY}"
        )
        .into());
    }
    if settings
        .failover_max_attempts_per_provider
        .saturating_mul(settings.failover_max_providers_to_try)
        > MAX_FAILOVER_TOTAL_ATTEMPTS
    {
        return Err(format!(
            "SEC_INVALID_INPUT: failover limits too high: failover_max_attempts_per_provider * failover_max_providers_to_try must be <= {MAX_FAILOVER_TOTAL_ATTEMPTS}"
        )
        .into());
    }

    if settings.circuit_breaker_failure_threshold == 0 {
        return Err("SEC_INVALID_INPUT: circuit_breaker_failure_threshold must be >= 1".into());
    }
    if settings.circuit_breaker_open_duration_minutes == 0 {
        return Err("SEC_INVALID_INPUT: circuit_breaker_open_duration_minutes must be >= 1".into());
    }
    if settings.circuit_breaker_failure_threshold > MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD {
        return Err(format!(
            "SEC_INVALID_INPUT: circuit_breaker_failure_threshold must be <= {MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD}"
        )
        .into());
    }
    if settings.circuit_breaker_open_duration_minutes > MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES {
        return Err(format!(
            "SEC_INVALID_INPUT: circuit_breaker_open_duration_minutes must be <= {MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES}"
        )
        .into());
    }

    Ok(())
}

pub fn write<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &AppSettings,
) -> AppResult<AppSettings> {
    let mut settings = settings.clone();
    settings.cli_priority_order = normalize_cli_priority_order(&settings.cli_priority_order);
    settings.update_releases_url = settings.update_releases_url.trim().to_string();
    settings.upstream_proxy_url = settings.upstream_proxy_url.trim().to_string();
    settings.upstream_proxy_username = settings.upstream_proxy_username.trim().to_string();
    settings.codex_provider_test_model = settings.codex_provider_test_model.trim().to_string();
    settings.codex_reasoning_guard_hit_label =
        settings.codex_reasoning_guard_hit_label.trim().to_string();
    settings.cx2cc_fallback_model_opus = settings.cx2cc_fallback_model_opus.trim().to_string();
    settings.cx2cc_fallback_model_sonnet = settings.cx2cc_fallback_model_sonnet.trim().to_string();
    settings.cx2cc_fallback_model_haiku = settings.cx2cc_fallback_model_haiku.trim().to_string();
    settings.cx2cc_fallback_model_main = settings.cx2cc_fallback_model_main.trim().to_string();
    settings.cx2cc_model_reasoning_effort =
        settings.cx2cc_model_reasoning_effort.trim().to_string();
    settings.cx2cc_service_tier = settings.cx2cc_service_tier.trim().to_string();
    settings.codex_home_override = normalize_codex_home_override(&settings.codex_home_override);
    if settings.codex_home_mode != CodexHomeMode::Custom {
        settings.codex_home_override.clear();
    }
    if settings.codex_home_mode == CodexHomeMode::Custom && settings.codex_home_override.is_empty()
    {
        settings.codex_home_mode = CodexHomeMode::UserHomeDefault;
    }

    validate_bounds(&settings)?;

    let path = settings_path(app)?;
    let tmp_path = path.with_file_name("settings.json.tmp");
    let backup_path = path.with_file_name("settings.json.bak");

    let canonical = canonical_settings_json(&settings)?;
    let content = serde_json::to_vec_pretty(&canonical)
        .map_err(|e| format!("failed to serialize settings: {e}"))?;
    ensure_settings_file_len(&content)?;

    std::fs::write(&tmp_path, content)
        .map_err(|e| format!("failed to write temp settings file: {e}"))?;

    if backup_path.exists() {
        let _ = std::fs::remove_file(&backup_path);
    }

    if path.exists() {
        std::fs::rename(&path, &backup_path)
            .map_err(|e| format!("failed to create settings backup: {e}"))?;
    }

    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        let _ = std::fs::rename(&backup_path, &path);
        return Err(format!("failed to finalize settings: {e}").into());
    }

    if backup_path.exists() {
        let _ = std::fs::remove_file(&backup_path);
    }

    cache_settings(&path, &settings);

    Ok(settings)
}

/// Clear the in-process settings cache.  Only available for integration tests
/// where each `TestApp` uses a distinct temp directory.
pub fn clear_cache() {
    let cache = SETTINGS_CACHE.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = cache.write() {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_codex_reasoning_guard_template() -> super::super::types::CodexReasoningGuardRuleTemplate
    {
        super::super::types::CodexReasoningGuardRuleTemplate {
            id: "custom-template".to_string(),
            name: "Custom template".to_string(),
            description: String::new(),
            rules: vec![super::super::types::CodexReasoningGuardTemplateRule {
                id: "rule-516".to_string(),
                name: "516 rule".to_string(),
                reasoning_tokens: Some(516),
                action: super::super::types::CodexReasoningGuardTemplateRuleAction::Intercept,
                logic: super::super::types::CodexReasoningGuardTemplateRuleLogic::And,
                filters: vec![super::super::types::CodexReasoningGuardTemplateFilter {
                    id: "duration".to_string(),
                    field: CodexReasoningGuardTemplateFilterField::DurationMs,
                    operator: CodexReasoningGuardTemplateFilterOperator::LessThan,
                    number_value: Some(30_000.0),
                    bool_value: None,
                    string_value: None,
                    string_values: Vec::new(),
                }],
            }],
        }
    }

    // -- parse_settings_json --

    #[test]
    fn parse_settings_json_detects_schema_version_present() {
        let json = r#"{"schema_version": 14, "preferred_port": 37123}"#;
        let (settings, schema_version_present, _) = parse_settings_json(json).unwrap();
        assert!(schema_version_present);
        assert_eq!(settings.schema_version, 14);
        assert_eq!(settings.preferred_port, 37123);
    }

    #[test]
    fn parse_settings_json_detects_schema_version_absent() {
        let json = r#"{"preferred_port": 37123}"#;
        let (settings, schema_version_present, _) = parse_settings_json(json).unwrap();
        assert!(!schema_version_present);
        // schema_version defaults via serde
        assert_eq!(settings.preferred_port, 37123);
    }

    #[test]
    fn parse_settings_json_uses_defaults_for_missing_fields() {
        let json = r#"{}"#;
        let (settings, _, _) = parse_settings_json(json).unwrap();
        assert_eq!(settings.preferred_port, DEFAULT_GATEWAY_PORT);
        assert_eq!(settings.log_retention_days, DEFAULT_LOG_RETENTION_DAYS);
        assert!(settings.tray_enabled);
        assert!(!settings.auto_start);
    }

    #[test]
    fn parse_settings_json_rejects_invalid_json() {
        assert!(parse_settings_json("not json").is_err());
    }

    #[test]
    fn canonical_settings_json_keeps_default_fields() {
        let canonical = canonical_settings_json(&AppSettings::default()).unwrap();
        let serialized_defaults = serde_json::to_value(AppSettings::default()).unwrap();
        assert_eq!(canonical, serialized_defaults);
    }

    #[test]
    fn app_settings_default_contains_codex_reasoning_guard_budget_defaults() {
        let settings = AppSettings::default();

        assert_eq!(settings.codex_reasoning_guard_immediate_retry_budget, 5);
        assert_eq!(settings.codex_reasoning_guard_delayed_retry_budget, 5);
        assert_eq!(settings.codex_reasoning_guard_delayed_retry_ms, 1_000);
        assert_eq!(
            settings.codex_reasoning_guard_exhausted_action,
            CodexReasoningGuardExhaustedAction::ReturnError
        );
        assert!(!settings.codex_reasoning_guard_continuation_repair_enabled);
        assert_eq!(settings.codex_reasoning_guard_continuation_max_rounds, 3);
        assert_eq!(
            settings.codex_reasoning_guard_continuation_max_output_tokens,
            0
        );
    }

    #[test]
    fn parse_settings_json_defaults_missing_codex_reasoning_guard_budget_fields() {
        let json = r#"{
            "schema_version": 41,
            "codex_reasoning_guard_backoff_after_hits": 99,
            "codex_reasoning_guard_backoff_ms": 60000
        }"#;

        let (settings, schema_version_present, _) = parse_settings_json(json).unwrap();

        assert!(schema_version_present);
        assert_eq!(settings.schema_version, 41);
        assert_eq!(settings.codex_reasoning_guard_immediate_retry_budget, 5);
        assert_eq!(settings.codex_reasoning_guard_delayed_retry_budget, 5);
        assert_eq!(settings.codex_reasoning_guard_delayed_retry_ms, 1_000);
        assert_eq!(
            settings.codex_reasoning_guard_exhausted_action,
            CodexReasoningGuardExhaustedAction::ReturnError
        );
        assert!(!settings.codex_reasoning_guard_continuation_repair_enabled);
        assert_eq!(settings.codex_reasoning_guard_continuation_max_rounds, 3);
        assert_eq!(
            settings.codex_reasoning_guard_continuation_max_output_tokens,
            0
        );
        assert_eq!(settings.codex_reasoning_guard_backoff_after_hits, 99);
        assert_eq!(settings.codex_reasoning_guard_backoff_ms, 60_000);
    }

    #[test]
    fn repair_settings_migrates_duplicate_legacy_equals_without_invalid_template() {
        let json = r#"{
            "schema_version": 45,
            "codex_reasoning_guard_rule_mode": "reasoning_tokens",
            "codex_reasoning_guard_compare_mode": "equals",
            "codex_reasoning_guard_reasoning_equals": [777, 777, 888, 777]
        }"#;
        let (mut settings, schema_version_present, raw_settings_json) =
            parse_settings_json(json).unwrap();

        assert!(
            repair_settings(&mut settings, schema_version_present, &raw_settings_json).unwrap()
        );
        validate_bounds(&settings).unwrap();
        assert_eq!(
            settings.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        assert_eq!(
            settings.codex_reasoning_guard_custom_templates[0]
                .rules
                .iter()
                .filter_map(|rule| rule.reasoning_tokens)
                .collect::<Vec<_>>(),
            vec![777, 888]
        );
    }

    #[test]
    fn canonical_settings_json_keeps_non_default_fields() {
        let settings = AppSettings {
            auto_start: true,
            ..Default::default()
        };
        let canonical = canonical_settings_json(&settings).unwrap();
        let expected = serde_json::to_value(settings).unwrap();
        assert_eq!(canonical, expected);
    }

    #[test]
    fn canonical_settings_json_detects_truncated_snapshots() {
        let raw = serde_json::json!({
            "schema_version": SCHEMA_VERSION
        });
        let settings = AppSettings::default();
        let canonical = canonical_settings_json(&settings).unwrap();
        assert_ne!(raw, canonical);
    }

    #[test]
    fn validate_bounds_rejects_log_retention_above_cap() {
        let settings = AppSettings {
            log_retention_days: MAX_LOG_RETENTION_DAYS + 1,
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("log_retention_days must be <="));
    }

    #[test]
    fn validate_bounds_rejects_excessive_failover_product() {
        let settings = AppSettings {
            failover_max_attempts_per_provider: 20,
            failover_max_providers_to_try: 20,
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("failover limits too high"));
    }

    #[test]
    fn validate_bounds_rejects_codex_reasoning_guard_budget_above_caps() {
        let settings = AppSettings {
            codex_reasoning_guard_immediate_retry_budget:
                MAX_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET + 1,
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("codex_reasoning_guard_immediate_retry_budget must be <="));

        let settings = AppSettings {
            codex_reasoning_guard_delayed_retry_budget:
                MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET + 1,
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("codex_reasoning_guard_delayed_retry_budget must be <="));

        let settings = AppSettings {
            codex_reasoning_guard_delayed_retry_ms: MAX_CODEX_REASONING_GUARD_DELAYED_RETRY_MS + 1,
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("codex_reasoning_guard_delayed_retry_ms must be <="));
    }

    #[test]
    fn validate_bounds_rejects_invalid_codex_reasoning_guard_continuation_caps() {
        let settings = AppSettings {
            codex_reasoning_guard_continuation_max_rounds: 0,
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("codex_reasoning_guard_continuation_max_rounds must be between"));

        let settings = AppSettings {
            codex_reasoning_guard_continuation_max_rounds:
                MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS + 1,
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("codex_reasoning_guard_continuation_max_rounds must be between"));

        let settings = AppSettings {
            codex_reasoning_guard_continuation_max_output_tokens:
                MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS + 1,
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("codex_reasoning_guard_continuation_max_output_tokens must be <="));
    }

    #[test]
    fn validate_bounds_accepts_valid_codex_reasoning_guard_template() {
        let settings = AppSettings {
            codex_reasoning_guard_active_template_id: "custom-template".to_string(),
            codex_reasoning_guard_custom_templates: vec![valid_codex_reasoning_guard_template()],
            ..Default::default()
        };

        validate_bounds(&settings).expect("valid template should pass");
    }

    #[test]
    fn validate_bounds_rejects_unknown_codex_reasoning_guard_active_template_id() {
        let settings = AppSettings {
            codex_reasoning_guard_active_template_id: "missing-template".to_string(),
            codex_reasoning_guard_custom_templates: vec![valid_codex_reasoning_guard_template()],
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains(
            "codex_reasoning_guard_active_template_id does not reference a built-in or custom template"
        ));
    }

    #[test]
    fn validate_bounds_rejects_reserved_codex_reasoning_guard_template_ids() {
        let mut template = valid_codex_reasoning_guard_template();
        template.id = CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string();
        let settings = AppSettings {
            codex_reasoning_guard_active_template_id: "custom-template".to_string(),
            codex_reasoning_guard_custom_templates: vec![template],
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("custom template id builtin-legacy-reasoning-tokens is reserved"));
    }

    #[test]
    fn validate_bounds_rejects_duplicate_codex_reasoning_guard_template_tokens_and_wildcards() {
        let mut duplicate_tokens = valid_codex_reasoning_guard_template();
        duplicate_tokens
            .rules
            .push(super::super::types::CodexReasoningGuardTemplateRule {
                id: "rule-516-duplicate".to_string(),
                name: "duplicate".to_string(),
                reasoning_tokens: Some(516),
                action: super::super::types::CodexReasoningGuardTemplateRuleAction::Intercept,
                logic: super::super::types::CodexReasoningGuardTemplateRuleLogic::And,
                filters: Vec::new(),
            });
        let settings = AppSettings {
            codex_reasoning_guard_active_template_id: duplicate_tokens.id.clone(),
            codex_reasoning_guard_custom_templates: vec![duplicate_tokens],
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("duplicate codex reasoning guard template token 516"));

        let mut duplicate_wildcards = valid_codex_reasoning_guard_template();
        duplicate_wildcards.rules = vec![
            super::super::types::CodexReasoningGuardTemplateRule {
                id: "wildcard-a".to_string(),
                name: "wildcard a".to_string(),
                reasoning_tokens: None,
                action: super::super::types::CodexReasoningGuardTemplateRuleAction::Intercept,
                logic: super::super::types::CodexReasoningGuardTemplateRuleLogic::And,
                filters: Vec::new(),
            },
            super::super::types::CodexReasoningGuardTemplateRule {
                id: "wildcard-b".to_string(),
                name: "wildcard b".to_string(),
                reasoning_tokens: None,
                action: super::super::types::CodexReasoningGuardTemplateRuleAction::Intercept,
                logic: super::super::types::CodexReasoningGuardTemplateRuleLogic::And,
                filters: Vec::new(),
            },
        ];
        let settings = AppSettings {
            codex_reasoning_guard_active_template_id: duplicate_wildcards.id.clone(),
            codex_reasoning_guard_custom_templates: vec![duplicate_wildcards],
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("rules may contain only one catch-all wildcard rule"));
    }

    #[test]
    fn validate_bounds_rejects_invalid_codex_reasoning_guard_template_filter_operator() {
        let mut template = valid_codex_reasoning_guard_template();
        template.rules[0].filters[0].operator = CodexReasoningGuardTemplateFilterOperator::In;
        template.rules[0].filters[0].string_values = vec!["30".to_string()];
        let settings = AppSettings {
            codex_reasoning_guard_active_template_id: template.id.clone(),
            codex_reasoning_guard_custom_templates: vec![template],
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("operator does not support numeric fields"));
    }

    #[test]
    fn validate_bounds_rejects_invalid_custom_listen_address() {
        let settings = AppSettings {
            gateway_listen_mode: GatewayListenMode::Custom,
            gateway_custom_listen_address: "http://127.0.0.1:37123".to_string(),
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("custom listen address must be host or host:port"));
    }

    #[test]
    fn validate_bounds_rejects_invalid_wsl_custom_host_address() {
        let settings = AppSettings {
            wsl_host_address_mode: WslHostAddressMode::Custom,
            wsl_custom_host_address: "127.0.0.1:37123".to_string(),
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("custom host address"));
    }

    #[test]
    fn validate_bounds_rejects_non_http_update_releases_url() {
        let settings = AppSettings {
            update_releases_url: "file:///tmp/releases.json".to_string(),
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("update_releases_url must use http or https"));
    }

    #[test]
    fn validate_bounds_rejects_credentialed_update_releases_url() {
        let settings = AppSettings {
            update_releases_url: "https://user:secret@example.invalid/releases".to_string(),
            ..Default::default()
        };

        let err = validate_bounds(&settings).unwrap_err().to_string();

        assert!(err.contains("update_releases_url must not include credentials"));
    }

    #[test]
    fn validate_bounds_rejects_oversized_upstream_proxy_fields() {
        let settings = AppSettings {
            upstream_proxy_url: "x".repeat(MAX_UPSTREAM_PROXY_URL_LEN + 1),
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("upstream_proxy_url must be <="));

        let settings = AppSettings {
            upstream_proxy_username: "x".repeat(MAX_UPSTREAM_PROXY_USERNAME_LEN + 1),
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("upstream_proxy_username must be <="));

        let settings = AppSettings {
            upstream_proxy_password: "x".repeat(MAX_UPSTREAM_PROXY_PASSWORD_LEN + 1),
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("upstream_proxy_password must be <="));
    }

    #[test]
    fn validate_bounds_rejects_invalid_cx2cc_strings() {
        let settings = AppSettings {
            cx2cc_fallback_model_main: " ".to_string(),
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("cx2cc_fallback_model_main cannot be empty"));

        let settings = AppSettings {
            cx2cc_fallback_model_main: "x".repeat(MAX_CX2CC_MODEL_NAME_LEN + 1),
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("cx2cc_fallback_model_main must be <="));

        let settings = AppSettings {
            cx2cc_service_tier: "priority\ninjected".to_string(),
            ..Default::default()
        };
        let err = validate_bounds(&settings).unwrap_err().to_string();
        assert!(err.contains("cx2cc_service_tier must not contain control characters"));
    }
}
