//! Display-only remote account usage for API-key providers.

use crate::domain::plugins::{
    PluginHostCompatibility, PluginInstallSource, PluginManifest, PluginRuntime, PluginStatus,
};
use crate::providers::{ProviderExtensionValues, ProviderExtensionValuesInput};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const ACCOUNT_USAGE_PLUGIN_ID: &str = "core.provider-account-usage";
pub(crate) const ACCOUNT_USAGE_NAMESPACE: &str = "accountUsage";
const NEWAPI_QUOTA_UNIT_DIVISOR: f64 = 500_000.0;
const TEXT_MAX_CHARS: usize = 96;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProviderAccountUsageAdapterKind {
    Sub2api,
    Newapi,
}

impl ProviderAccountUsageAdapterKind {
    pub(crate) fn endpoint_label(self) -> &'static str {
        match self {
            Self::Sub2api => "/v1/usage",
            Self::Newapi => "/api/user/self",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderAccountUsageStatus {
    Unsupported,
    ConfigurationRequired,
    Available,
    ZeroBalance,
    Expired,
    AuthFailed,
    QueryFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderAccountUsageFreshness {
    NotFetched,
    Fresh,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub(crate) struct ProviderAccountUsageResult {
    pub adapter_kind: Option<ProviderAccountUsageAdapterKind>,
    pub status: ProviderAccountUsageStatus,
    pub freshness: ProviderAccountUsageFreshness,
    pub plan_name: Option<String>,
    pub balance: Option<f64>,
    pub used: Option<f64>,
    pub total: Option<f64>,
    pub unit: Option<String>,
    pub unit_note: Option<String>,
    pub daily_used: Option<f64>,
    pub daily_total: Option<f64>,
    pub weekly_used: Option<f64>,
    pub weekly_total: Option<f64>,
    pub monthly_used: Option<f64>,
    pub monthly_total: Option<f64>,
    pub expires_at: Option<i64>,
    pub last_fetched_at: Option<i64>,
    pub message: Option<String>,
}

impl ProviderAccountUsageResult {
    pub(crate) fn local_status(
        adapter_kind: Option<ProviderAccountUsageAdapterKind>,
        status: ProviderAccountUsageStatus,
        message: impl Into<String>,
    ) -> Self {
        Self {
            adapter_kind,
            status,
            freshness: ProviderAccountUsageFreshness::NotFetched,
            plan_name: None,
            balance: None,
            used: None,
            total: None,
            unit: None,
            unit_note: None,
            daily_used: None,
            daily_total: None,
            weekly_used: None,
            weekly_total: None,
            monthly_used: None,
            monthly_total: None,
            expires_at: None,
            last_fetched_at: None,
            message: Some(message.into()),
        }
    }

    pub(crate) fn fetched(
        adapter_kind: ProviderAccountUsageAdapterKind,
        status: ProviderAccountUsageStatus,
        last_fetched_at: i64,
    ) -> Self {
        Self {
            adapter_kind: Some(adapter_kind),
            status,
            freshness: ProviderAccountUsageFreshness::Fresh,
            plan_name: None,
            balance: None,
            used: None,
            total: None,
            unit: None,
            unit_note: None,
            daily_used: None,
            daily_total: None,
            weekly_used: None,
            weekly_total: None,
            monthly_used: None,
            monthly_total: None,
            expires_at: None,
            last_fetched_at: Some(last_fetched_at),
            message: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderAccountUsageConfig {
    pub adapter_kind: ProviderAccountUsageAdapterKind,
    pub new_api_user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderAccountUsageConfigState {
    Missing,
    Disabled,
    Invalid(String),
    Configured(ProviderAccountUsageConfig),
}

pub(crate) fn config_from_extension_values(
    values: &[ProviderExtensionValues],
) -> ProviderAccountUsageConfigState {
    let Some(row) = values.iter().find(|value| {
        value.plugin_id == ACCOUNT_USAGE_PLUGIN_ID && value.namespace == ACCOUNT_USAGE_NAMESPACE
    }) else {
        return ProviderAccountUsageConfigState::Missing;
    };

    config_from_value(&row.values)
}

fn config_from_value(values: &Value) -> ProviderAccountUsageConfigState {
    let adapter_kind = values
        .get("adapterKind")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");

    match adapter_kind {
        "" | "disabled" => ProviderAccountUsageConfigState::Disabled,
        "sub2api" => ProviderAccountUsageConfigState::Configured(ProviderAccountUsageConfig {
            adapter_kind: ProviderAccountUsageAdapterKind::Sub2api,
            new_api_user_id: None,
        }),
        "newapi" => {
            let new_api_user_id = values
                .get("newApiUserId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(take_first_chars);
            ProviderAccountUsageConfigState::Configured(ProviderAccountUsageConfig {
                adapter_kind: ProviderAccountUsageAdapterKind::Newapi,
                new_api_user_id,
            })
        }
        other => ProviderAccountUsageConfigState::Invalid(format!(
            "unsupported account usage adapterKind={other}"
        )),
    }
}

pub(crate) fn extension_values_need_account_usage_owner(
    values: Option<&[ProviderExtensionValuesInput]>,
) -> bool {
    values.is_some_and(|values| {
        values.iter().any(|value| {
            value.plugin_id.trim() == ACCOUNT_USAGE_PLUGIN_ID
                && value.namespace.trim() == ACCOUNT_USAGE_NAMESPACE
        })
    })
}

pub(crate) fn ensure_account_usage_extension_owner_with_tx(
    tx: &rusqlite::Transaction<'_>,
    values: Option<&[ProviderExtensionValuesInput]>,
) -> crate::shared::error::AppResult<()> {
    if !extension_values_need_account_usage_owner(values) {
        return Ok(());
    }

    crate::infra::plugins::repository::insert_plugin_with_tx(
        tx,
        crate::infra::plugins::repository::InsertPluginInput {
            manifest: account_usage_owner_manifest(),
            install_source: PluginInstallSource::Official,
            status: PluginStatus::Uninstalled,
            installed_dir: None,
        },
    )?;
    Ok(())
}

fn account_usage_owner_manifest() -> PluginManifest {
    PluginManifest {
        id: ACCOUNT_USAGE_PLUGIN_ID.to_string(),
        name: "Core Provider Account Usage".to_string(),
        version: "1.0.0".to_string(),
        api_version: "1.0.0".to_string(),
        runtime: PluginRuntime::ExtensionHost {
            language: "typescript".to_string(),
        },
        hooks: Vec::new(),
        permissions: Vec::new(),
        main: Some("core/provider-account-usage.js".to_string()),
        activation_events: Vec::new(),
        contributes: None,
        capabilities: Vec::new(),
        host_compatibility: PluginHostCompatibility {
            app: ">=0.60.0 <1.0.0".to_string(),
            plugin_api: "^1.0.0".to_string(),
            platforms: Vec::new(),
        },
        entry: None,
        config_schema: None,
        config_version: None,
        description: Some(
            "Internal owner for provider account usage extension values.".to_string(),
        ),
        author: None,
        homepage: None,
        repository: None,
        license: None,
        checksum: None,
        signature: None,
        category: None,
    }
}

pub(crate) fn build_account_usage_url(
    base_url: &str,
    adapter_kind: ProviderAccountUsageAdapterKind,
) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|err| format!("SEC_INVALID_INPUT: invalid provider base URL: {err}"))?;

    let mut segments: Vec<String> = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    match adapter_kind {
        ProviderAccountUsageAdapterKind::Sub2api => {
            if segments.last().is_some_and(|segment| segment == "v1") {
                segments.push("usage".to_string());
            } else {
                segments.extend(["v1".to_string(), "usage".to_string()]);
            }
        }
        ProviderAccountUsageAdapterKind::Newapi => {
            if segments.last().is_some_and(|segment| segment == "v1") {
                segments.pop();
            }
            segments.extend(["api".to_string(), "user".to_string(), "self".to_string()]);
        }
    }

    url.set_path(&segments.join("/"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

pub(crate) fn parse_account_usage_response(
    adapter_kind: ProviderAccountUsageAdapterKind,
    body: &Value,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    match adapter_kind {
        ProviderAccountUsageAdapterKind::Sub2api => {
            parse_sub2api_response(body, fetched_at, now_unix)
        }
        ProviderAccountUsageAdapterKind::Newapi => {
            parse_newapi_response(body, fetched_at, now_unix)
        }
    }
}

pub(crate) fn http_status_result(
    adapter_kind: ProviderAccountUsageAdapterKind,
    status: reqwest::StatusCode,
    fetched_at: i64,
) -> ProviderAccountUsageResult {
    let mapped = if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        ProviderAccountUsageStatus::AuthFailed
    } else if adapter_kind == ProviderAccountUsageAdapterKind::Newapi
        && matches!(status.as_u16(), 400 | 404 | 422)
    {
        ProviderAccountUsageStatus::ConfigurationRequired
    } else {
        ProviderAccountUsageStatus::QueryFailed
    };

    let message = match mapped {
        ProviderAccountUsageStatus::AuthFailed => "账户用量接口认证失败".to_string(),
        ProviderAccountUsageStatus::ConfigurationRequired => {
            "NewAPI 账户用量接口需要补充或修正用户配置".to_string()
        }
        _ => format!(
            "账户用量接口返回 HTTP {} for {}",
            status.as_u16(),
            adapter_kind.endpoint_label()
        ),
    };

    let mut result = ProviderAccountUsageResult::fetched(adapter_kind, mapped, fetched_at);
    result.message = Some(message);
    result
}

pub(crate) fn redact_secret(input: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        input.to_string()
    } else {
        input.replace(secret, "[REDACTED]")
    }
}

fn parse_sub2api_response(
    body: &Value,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    let is_valid = body.get("isValid").and_then(Value::as_bool);
    let balance = number_at(body, &["remaining"]);
    let subscription = body.get("subscription").unwrap_or(&Value::Null);
    let plan_name = string_at(body, &["planName", "plan_name"]);
    let daily_used = number_at(subscription, &["daily_usage_usd", "dailyUsageUsd"]);
    let daily_total = number_at(subscription, &["daily_limit_usd", "dailyLimitUsd"]);
    let weekly_used = number_at(subscription, &["weekly_usage_usd", "weeklyUsageUsd"]);
    let weekly_total = number_at(subscription, &["weekly_limit_usd", "weeklyLimitUsd"]);
    let monthly_used = number_at(subscription, &["monthly_usage_usd", "monthlyUsageUsd"]);
    let monthly_total = number_at(subscription, &["monthly_limit_usd", "monthlyLimitUsd"]);
    let expires_at = value_at(subscription, &["expires_at", "expiresAt"])
        .or_else(|| value_at(body, &["expires_at", "expiresAt"]))
        .and_then(parse_timestamp_value);

    if is_valid.is_none()
        && balance.is_none()
        && plan_name.is_none()
        && expires_at.is_none()
        && daily_used.is_none()
        && daily_total.is_none()
        && weekly_used.is_none()
        && weekly_total.is_none()
        && monthly_used.is_none()
        && monthly_total.is_none()
    {
        let mut result = ProviderAccountUsageResult::fetched(
            ProviderAccountUsageAdapterKind::Sub2api,
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
        );
        result.message = Some("sub2api 响应缺少账户用量字段".to_string());
        return result;
    }

    let status = status_from_account_parts(is_valid, balance, expires_at, now_unix);
    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Sub2api,
        status,
        fetched_at,
    );
    result.plan_name = plan_name;
    result.balance = balance;
    result.unit = Some("USD".to_string());
    result.daily_used = daily_used;
    result.daily_total = daily_total;
    result.weekly_used = weekly_used;
    result.weekly_total = weekly_total;
    result.monthly_used = monthly_used;
    result.monthly_total = monthly_total;
    result.expires_at = expires_at;
    result
}

fn parse_newapi_response(
    body: &Value,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    let data = body
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(body);
    let remaining_quota = number_at(data, &["quota"]);
    let used_quota = number_at(data, &["used_quota", "usedQuota"]).unwrap_or(0.0);

    let Some(remaining_quota) = remaining_quota else {
        let mut result = ProviderAccountUsageResult::fetched(
            ProviderAccountUsageAdapterKind::Newapi,
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
        );
        result.message = Some("NewAPI 响应缺少 quota 字段".to_string());
        return result;
    };

    let balance = remaining_quota / NEWAPI_QUOTA_UNIT_DIVISOR;
    let used = used_quota / NEWAPI_QUOTA_UNIT_DIVISOR;
    let total = (remaining_quota + used_quota) / NEWAPI_QUOTA_UNIT_DIVISOR;
    let expires_at = value_at(
        data,
        &["expired_time", "expiredTime", "expires_at", "expiresAt"],
    )
    .and_then(parse_timestamp_value);
    let status = status_from_account_parts(None, Some(balance), expires_at, now_unix);

    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Newapi,
        status,
        fetched_at,
    );
    result.balance = Some(balance);
    result.used = Some(used);
    result.total = Some(total);
    result.unit = Some("USD".to_string());
    result.unit_note =
        Some("NewAPI quota uses the default 500000 quota-per-USD divisor.".to_string());
    result.expires_at = expires_at;
    result
}

fn status_from_account_parts(
    is_valid: Option<bool>,
    balance: Option<f64>,
    expires_at: Option<i64>,
    now_unix: i64,
) -> ProviderAccountUsageStatus {
    if is_valid == Some(false) {
        return ProviderAccountUsageStatus::AuthFailed;
    }
    if expires_at.is_some_and(|expires_at| expires_at <= now_unix) {
        return ProviderAccountUsageStatus::Expired;
    }
    if balance.is_some_and(|balance| balance <= 0.0) {
        return ProviderAccountUsageStatus::ZeroBalance;
    }
    ProviderAccountUsageStatus::Available
}

fn value_at<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| value.get(*key))
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    value_at(value, keys)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(take_first_chars)
}

fn number_at(value: &Value, keys: &[&str]) -> Option<f64> {
    value_at(value, keys).and_then(number_from_value)
}

fn number_from_value(value: &Value) -> Option<f64> {
    if let Some(number) = value.as_f64().filter(|value| value.is_finite()) {
        return Some(number);
    }

    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw.trim_start_matches('$').replace(',', "");
    normalized
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn parse_timestamp_value(value: &Value) -> Option<i64> {
    if let Some(timestamp) = value.as_i64().filter(|timestamp| *timestamp > 0) {
        return Some(normalize_unix_timestamp(timestamp));
    }

    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(timestamp) = text.parse::<i64>() {
        return (timestamp > 0).then(|| normalize_unix_timestamp(timestamp));
    }

    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|value| value.timestamp())
        .filter(|timestamp| *timestamp > 0)
}

fn normalize_unix_timestamp(timestamp: i64) -> i64 {
    if timestamp > 10_000_000_000 {
        timestamp / 1_000
    } else {
        timestamp
    }
}

fn take_first_chars(value: &str) -> String {
    if value.chars().nth(TEXT_MAX_CHARS).is_none() {
        return value.to_string();
    }
    value.chars().take(TEXT_MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sub2api_accepts_plan_name_shapes_and_numeric_strings() {
        let body = json!({
            "isValid": true,
            "plan_name": "Pro",
            "remaining": "12.50",
            "subscription": {
                "daily_usage_usd": "1.25",
                "daily_limit_usd": "10",
                "weekly_usage_usd": "2",
                "weekly_limit_usd": "70",
                "monthly_usage_usd": "3",
                "monthly_limit_usd": "300",
                "expires_at": "2030-01-01T00:00:00Z"
            }
        });

        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &body,
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.plan_name.as_deref(), Some("Pro"));
        assert_eq!(result.balance, Some(12.5));
        assert_eq!(result.daily_used, Some(1.25));
        assert_eq!(result.weekly_total, Some(70.0));
        assert_eq!(result.monthly_total, Some(300.0));
        assert_eq!(result.expires_at, Some(1_893_456_000));
    }

    #[test]
    fn sub2api_is_valid_false_maps_to_auth_failed_without_throwing() {
        let body = json!({
            "isValid": false,
            "planName": "Expired",
            "remaining": 8
        });

        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &body,
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::AuthFailed);
        assert_eq!(result.plan_name.as_deref(), Some("Expired"));
        assert_eq!(result.balance, Some(8.0));
    }

    #[test]
    fn sub2api_unknown_success_payload_maps_to_query_failed() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({ "ok": true }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(result.message.as_deref().unwrap_or("").contains("sub2api"));
    }

    #[test]
    fn newapi_accepts_root_and_data_payload_shapes() {
        let root = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Newapi,
            &json!({ "quota": 500000, "used_quota": 1000000 }),
            100,
            1_800_000_000,
        );
        let data = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Newapi,
            &json!({ "data": { "quota": "250000", "used_quota": "250000" } }),
            101,
            1_800_000_000,
        );

        assert_eq!(root.balance, Some(1.0));
        assert_eq!(root.used, Some(2.0));
        assert_eq!(root.total, Some(3.0));
        assert_eq!(data.balance, Some(0.5));
        assert_eq!(data.used, Some(0.5));
        assert_eq!(data.total, Some(1.0));
        assert_eq!(data.status, ProviderAccountUsageStatus::Available);
    }

    #[test]
    fn newapi_missing_quota_maps_to_query_failed() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Newapi,
            &json!({ "data": { "used_quota": 500000 } }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(result.message.as_deref().unwrap_or("").contains("quota"));
    }

    #[test]
    fn build_urls_trim_duplicate_v1_segments() {
        assert_eq!(
            build_account_usage_url(
                "https://sub.example.test/v1/",
                ProviderAccountUsageAdapterKind::Sub2api
            )
            .unwrap(),
            "https://sub.example.test/v1/usage"
        );
        assert_eq!(
            build_account_usage_url(
                "https://newapi.example.test/v1?x=1",
                ProviderAccountUsageAdapterKind::Newapi
            )
            .unwrap(),
            "https://newapi.example.test/api/user/self"
        );
    }

    #[test]
    fn redaction_removes_api_key_material() {
        let redacted = redact_secret("request failed for sk-secret-value", "sk-secret-value");
        assert_eq!(redacted, "request failed for [REDACTED]");
    }
}
