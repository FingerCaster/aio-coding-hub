//! Display-only remote account usage for API-key providers.

use crate::domain::plugins::{
    PluginHostCompatibility, PluginInstallSource, PluginManifest, PluginRuntime, PluginStatus,
};
use crate::providers::{ProviderExtensionValues, ProviderExtensionValuesInput};
use chrono::{Datelike, Duration, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const ACCOUNT_USAGE_PLUGIN_ID: &str = "core.provider-account-usage";
pub(crate) const ACCOUNT_USAGE_NAMESPACE: &str = "accountUsage";
const TEXT_MAX_CHARS: usize = 96;
pub(crate) const SUB2API_RESPONSE_BODY_LIMIT: usize = 32 * 1024;
const NEWAPI_STATUS_BODY_LIMIT: usize = 16 * 1024;
const NEWAPI_SUBSCRIPTION_BODY_LIMIT: usize = 8 * 1024;
const NEWAPI_USAGE_BODY_LIMIT: usize = 8 * 1024;
const NEWAPI_ACCOUNT_BODY_LIMIT: usize = 16 * 1024;
const NEWAPI_ACCESS_TOKEN_MAX_BYTES: usize = 64 * 1024;
const ACCOUNT_USAGE_REFRESH_INTERVAL_MIN_SECONDS: i64 = 60;
const ACCOUNT_USAGE_REFRESH_INTERVAL_MAX_SECONDS: i64 = 300;
const ACCOUNT_USAGE_REFRESH_INTERVAL_DEFAULT_SECONDS: i64 = 300;
const NEWAPI_UNLIMITED_TOKEN_HARD_LIMIT_USD: f64 = 100_000_000.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProviderAccountUsageAdapterKind {
    Sub2api,
    Newapi,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NewapiQueryMode {
    Billing,
    Account,
}

impl ProviderAccountUsageAdapterKind {
    pub(crate) fn endpoint_label(self) -> &'static str {
        match self {
            Self::Sub2api => "/v1/usage",
            Self::Newapi => "NewAPI billing",
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
    pub plan_remaining: Option<f64>,
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
            plan_remaining: None,
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
            plan_remaining: None,
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
    pub new_api_query_mode: NewapiQueryMode,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderAccountUsageCredentials {
    pub new_api_user_id: Option<String>,
    pub new_api_access_token: Option<String>,
}

#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderAccountUsageCredentialsPatch {
    pub new_api_user_id: Option<String>,
    pub new_api_access_token: Option<String>,
    #[serde(default)]
    pub clear_new_api_access_token: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderAccountUsageConfigState {
    Missing,
    Disabled,
    Invalid(String),
    Configured(ProviderAccountUsageConfig),
}

pub(crate) fn sanitize_account_usage_extension_value(values: &Value) -> Value {
    let adapter_kind = match values.get("adapterKind").and_then(Value::as_str) {
        Some("sub2api") => "sub2api",
        Some("newapi") => "newapi",
        _ => "disabled",
    };
    let new_api_query_mode = match values.get("newApiQueryMode").and_then(Value::as_str) {
        Some("account") => "account",
        _ => "billing",
    };
    let timed_refresh_enabled = values
        .get("timedRefreshEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let refresh_interval_seconds = values
        .get("refreshIntervalSeconds")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map(|value| value.round() as i64)
        .unwrap_or(ACCOUNT_USAGE_REFRESH_INTERVAL_DEFAULT_SECONDS)
        .clamp(
            ACCOUNT_USAGE_REFRESH_INTERVAL_MIN_SECONDS,
            ACCOUNT_USAGE_REFRESH_INTERVAL_MAX_SECONDS,
        );

    serde_json::json!({
        "adapterKind": adapter_kind,
        "newApiQueryMode": new_api_query_mode,
        "timedRefreshEnabled": timed_refresh_enabled,
        "refreshIntervalSeconds": refresh_interval_seconds,
    })
}

pub(crate) fn normalize_newapi_user_id(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("SEC_INVALID_INPUT: NewAPI User ID must be a positive integer".to_string());
    }
    let parsed = value
        .parse::<i64>()
        .map_err(|_| "SEC_INVALID_INPUT: NewAPI User ID is out of range".to_string())?;
    if parsed == 0 {
        return Err("SEC_INVALID_INPUT: NewAPI User ID must be positive".to_string());
    }
    Ok(parsed.to_string())
}

fn normalize_newapi_access_token(raw: &str) -> Result<Option<String>, String> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > NEWAPI_ACCESS_TOKEN_MAX_BYTES {
        return Err("SEC_INVALID_INPUT: NewAPI account access token is too large".to_string());
    }
    let authorization = format!("Bearer {value}");
    if reqwest::header::HeaderValue::from_str(&authorization).is_err() {
        return Err("SEC_INVALID_INPUT: NewAPI account access token is invalid".to_string());
    }
    Ok(Some(value.to_string()))
}

pub(crate) fn load_account_usage_credentials(
    conn: &Connection,
    provider_id: i64,
) -> crate::shared::error::AppResult<ProviderAccountUsageCredentials> {
    let row = conn
        .query_row(
            r#"
SELECT newapi_user_id, newapi_access_token_plaintext
FROM provider_account_usage_credentials
WHERE provider_id = ?1
"#,
            params![provider_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(|error| {
            crate::shared::error::db_err!("failed to query provider account credentials: {error}")
        })?;

    let Some((new_api_user_id, new_api_access_token)) = row else {
        return Ok(ProviderAccountUsageCredentials::default());
    };
    let new_api_user_id = new_api_user_id
        .as_deref()
        .map(normalize_newapi_user_id)
        .transpose()
        .map_err(|_| "DB_INVALID_DATA: NewAPI account User ID is invalid".to_string())?;
    let new_api_access_token = new_api_access_token
        .as_deref()
        .map(normalize_newapi_access_token)
        .transpose()
        .map_err(|_| "DB_INVALID_DATA: NewAPI account access token is invalid".to_string())?
        .flatten();
    Ok(ProviderAccountUsageCredentials {
        new_api_user_id,
        new_api_access_token,
    })
}

fn write_account_usage_credentials(
    conn: &Connection,
    provider_id: i64,
    credentials: &ProviderAccountUsageCredentials,
) -> crate::shared::error::AppResult<()> {
    if credentials.new_api_user_id.is_none() && credentials.new_api_access_token.is_none() {
        conn.execute(
            "DELETE FROM provider_account_usage_credentials WHERE provider_id = ?1",
            params![provider_id],
        )
        .map_err(|error| {
            crate::shared::error::db_err!("failed to clear provider account credentials: {error}")
        })?;
        return Ok(());
    }

    conn.execute(
        r#"
INSERT INTO provider_account_usage_credentials(
  provider_id,
  newapi_user_id,
  newapi_access_token_plaintext,
  updated_at
) VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(provider_id) DO UPDATE SET
  newapi_user_id = excluded.newapi_user_id,
  newapi_access_token_plaintext = excluded.newapi_access_token_plaintext,
  updated_at = excluded.updated_at
"#,
        params![
            provider_id,
            credentials.new_api_user_id,
            credentials.new_api_access_token,
            crate::shared::time::now_unix_seconds(),
        ],
    )
    .map_err(|error| {
        crate::shared::error::db_err!("failed to save provider account credentials: {error}")
    })?;
    Ok(())
}

pub(crate) fn apply_account_usage_credentials_patch(
    conn: &Connection,
    provider_id: i64,
    patch: Option<&ProviderAccountUsageCredentialsPatch>,
) -> crate::shared::error::AppResult<bool> {
    let Some(patch) = patch else {
        return Ok(false);
    };
    if patch.clear_new_api_access_token
        && patch
            .new_api_access_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(
            "SEC_INVALID_INPUT: cannot set and clear NewAPI account access token together"
                .to_string()
                .into(),
        );
    }

    let existing = load_account_usage_credentials(conn, provider_id)?;
    let new_api_user_id = patch
        .new_api_user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_newapi_user_id)
        .transpose()?;
    let new_api_access_token = if patch.clear_new_api_access_token {
        None
    } else {
        match patch.new_api_access_token.as_deref() {
            Some(value) if !value.trim().is_empty() => normalize_newapi_access_token(value)?,
            _ => existing.new_api_access_token,
        }
    };
    write_account_usage_credentials(
        conn,
        provider_id,
        &ProviderAccountUsageCredentials {
            new_api_user_id,
            new_api_access_token,
        },
    )?;
    Ok(true)
}

pub(crate) fn copy_account_usage_credentials(
    conn: &Connection,
    source_provider_id: i64,
    target_provider_id: i64,
) -> crate::shared::error::AppResult<()> {
    let credentials = load_account_usage_credentials(conn, source_provider_id)?;
    write_account_usage_credentials(conn, target_provider_id, &credentials)
}

pub(crate) fn restore_account_usage_credentials(
    conn: &Connection,
    provider_id: i64,
    new_api_user_id: Option<&str>,
    new_api_access_token: Option<&str>,
) -> crate::shared::error::AppResult<()> {
    let new_api_user_id = new_api_user_id.map(normalize_newapi_user_id).transpose()?;
    let new_api_access_token = new_api_access_token
        .map(normalize_newapi_access_token)
        .transpose()?
        .flatten();
    write_account_usage_credentials(
        conn,
        provider_id,
        &ProviderAccountUsageCredentials {
            new_api_user_id,
            new_api_access_token,
        },
    )
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
            new_api_query_mode: NewapiQueryMode::Billing,
        }),
        "newapi" => {
            let new_api_query_mode = match values.get("newApiQueryMode").and_then(Value::as_str) {
                Some("account") => NewapiQueryMode::Account,
                _ => NewapiQueryMode::Billing,
            };
            ProviderAccountUsageConfigState::Configured(ProviderAccountUsageConfig {
                adapter_kind: ProviderAccountUsageAdapterKind::Newapi,
                new_api_query_mode,
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
    tx: &rusqlite::Connection,
    values: Option<&[ProviderExtensionValuesInput]>,
) -> crate::shared::error::AppResult<()> {
    if !extension_values_need_account_usage_owner(values) {
        return Ok(());
    }

    crate::infra::plugins::repository::insert_plugin_with_conn(
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
            return Err("SEC_INVALID_INPUT: NewAPI requires the billing endpoint set".to_string());
        }
    }

    url.set_path(&segments.join("/"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewapiBillingUrls {
    pub status: reqwest::Url,
    pub subscription: reqwest::Url,
    pub usage: reqwest::Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewapiAccountUrls {
    pub status: reqwest::Url,
    pub account: reqwest::Url,
}

pub(crate) fn build_newapi_billing_urls(base_url: &str) -> Result<NewapiBillingUrls, String> {
    let root = normalize_newapi_root(base_url)?;
    let status = newapi_url_at(&root, &["api", "status"])?;
    let subscription = newapi_url_at(&root, &["v1", "dashboard", "billing", "subscription"])?;
    let usage = newapi_url_at(&root, &["v1", "dashboard", "billing", "usage"])?;
    Ok(NewapiBillingUrls {
        status,
        subscription,
        usage,
    })
}

pub(crate) fn build_newapi_account_urls(base_url: &str) -> Result<NewapiAccountUrls, String> {
    let root = normalize_newapi_root(base_url)?;
    Ok(NewapiAccountUrls {
        status: newapi_url_at(&root, &["api", "status"])?,
        account: newapi_url_at(&root, &["api", "user", "self"])?,
    })
}

fn normalize_newapi_root(base_url: &str) -> Result<reqwest::Url, String> {
    let mut root = reqwest::Url::parse(base_url.trim())
        .map_err(|err| format!("SEC_INVALID_INPUT: invalid provider base URL: {err}"))?;
    if !matches!(root.scheme(), "http" | "https")
        || !root.has_host()
        || !root.username().is_empty()
        || root.password().is_some()
    {
        return Err("SEC_INVALID_INPUT: invalid provider base URL origin".to_string());
    }

    let mut segments: Vec<String> = root
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if segments.last().is_some_and(|segment| segment == "v1") {
        segments.pop();
    }
    root.set_path(&segments.join("/"));
    root.set_query(None);
    root.set_fragment(None);
    Ok(root)
}

fn newapi_url_at(root: &reqwest::Url, suffix: &[&str]) -> Result<reqwest::Url, String> {
    let mut url = root.clone();
    let mut segments: Vec<String> = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    segments.extend(suffix.iter().map(|segment| (*segment).to_string()));
    url.set_path(&segments.join("/"));
    if url.origin() != root.origin() {
        return Err("SEC_INVALID_INPUT: NewAPI endpoint origin mismatch".to_string());
    }
    Ok(url)
}

pub(crate) fn newapi_usage_date_range(
    has_payment_method: bool,
    now_unix: i64,
) -> Result<(String, String), String> {
    let end = Utc
        .timestamp_opt(now_unix, 0)
        .single()
        .ok_or_else(|| "SYSTEM_ERROR: invalid account usage timestamp".to_string())?
        .date_naive();
    let start = if has_payment_method {
        end.with_day(1)
            .ok_or_else(|| "SYSTEM_ERROR: invalid billing month".to_string())?
    } else {
        end.checked_sub_signed(Duration::days(99))
            .ok_or_else(|| "SYSTEM_ERROR: invalid billing date range".to_string())?
    };
    Ok((
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    ))
}

pub(crate) async fn fetch_newapi_account_usage(
    base_url: &str,
    api_key: &str,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    let urls = match build_newapi_billing_urls(base_url) {
        Ok(urls) => urls,
        Err(message) => {
            return ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Newapi),
                ProviderAccountUsageStatus::ConfigurationRequired,
                message,
            );
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!(
            "aio-coding-hub-provider-account-usage/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return newapi_failed_result(
                ProviderAccountUsageStatus::QueryFailed,
                fetched_at,
                "NewAPI 账户用量查询失败",
            );
        }
    };

    let status = match get_newapi_json(
        client.get(urls.status.clone()),
        NEWAPI_STATUS_BODY_LIMIT,
        false,
        fetched_at,
    )
    .await
    {
        Ok(body) => body,
        Err(result) => return result,
    };
    let subscription = match get_newapi_json(
        client.get(urls.subscription.clone()).bearer_auth(api_key),
        NEWAPI_SUBSCRIPTION_BODY_LIMIT,
        true,
        fetched_at,
    )
    .await
    {
        Ok(body) => body,
        Err(result) => return result,
    };

    let subscription_payload = data_or_root(&subscription);
    let Some(has_payment_method) = subscription_payload
        .get("has_payment_method")
        .and_then(Value::as_bool)
    else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI subscription 响应缺少有效 payment method 字段",
        );
    };
    let (start_date, end_date) = match newapi_usage_date_range(has_payment_method, now_unix) {
        Ok(range) => range,
        Err(message) => {
            return newapi_failed_result(
                ProviderAccountUsageStatus::QueryFailed,
                fetched_at,
                &message,
            );
        }
    };
    let usage = match get_newapi_json(
        client
            .get(urls.usage.clone())
            .bearer_auth(api_key)
            .query(&[("start_date", start_date), ("end_date", end_date)]),
        NEWAPI_USAGE_BODY_LIMIT,
        true,
        fetched_at,
    )
    .await
    {
        Ok(body) => body,
        Err(result) => return result,
    };

    parse_newapi_billing_responses(&status, &subscription, &usage, fetched_at, now_unix)
}

pub(crate) async fn fetch_newapi_user_account_usage(
    base_url: &str,
    access_token: &str,
    user_id: &str,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    let user_id = match normalize_newapi_user_id(user_id) {
        Ok(user_id) => user_id,
        Err(message) => {
            return ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Newapi),
                ProviderAccountUsageStatus::ConfigurationRequired,
                message,
            );
        }
    };
    let access_token = match normalize_newapi_access_token(access_token) {
        Ok(Some(access_token)) => access_token,
        _ => {
            return ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Newapi),
                ProviderAccountUsageStatus::ConfigurationRequired,
                "需配置账户凭据",
            );
        }
    };
    let urls = match build_newapi_account_urls(base_url) {
        Ok(urls) => urls,
        Err(message) => {
            return ProviderAccountUsageResult::local_status(
                Some(ProviderAccountUsageAdapterKind::Newapi),
                ProviderAccountUsageStatus::ConfigurationRequired,
                message,
            );
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!(
            "aio-coding-hub-provider-account-usage/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return newapi_failed_result(
                ProviderAccountUsageStatus::QueryFailed,
                fetched_at,
                "NewAPI 账户用量查询失败",
            );
        }
    };

    let status = match get_newapi_json(
        client.get(urls.status),
        NEWAPI_STATUS_BODY_LIMIT,
        false,
        fetched_at,
    )
    .await
    {
        Ok(body) => body,
        Err(result) => return result,
    };
    let account = match get_newapi_json(
        client
            .get(urls.account)
            .bearer_auth(&access_token)
            .header("New-Api-User", &user_id),
        NEWAPI_ACCOUNT_BODY_LIMIT,
        true,
        fetched_at,
    )
    .await
    {
        Ok(body) => body,
        Err(result) => return result,
    };

    parse_newapi_account_responses(&status, &account, &user_id, fetched_at, now_unix)
}

async fn get_newapi_json(
    request: reqwest::RequestBuilder,
    body_limit: usize,
    authenticated: bool,
    fetched_at: i64,
) -> Result<Value, ProviderAccountUsageResult> {
    let response = request
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| {
            newapi_failed_result(
                ProviderAccountUsageStatus::QueryFailed,
                fetched_at,
                "NewAPI 账户用量查询失败",
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let mapped = if authenticated
            && matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
            ProviderAccountUsageStatus::AuthFailed
        } else {
            ProviderAccountUsageStatus::QueryFailed
        };
        return Err(newapi_failed_result(
            mapped,
            fetched_at,
            if mapped == ProviderAccountUsageStatus::AuthFailed {
                "NewAPI 账户接口认证失败"
            } else {
                "NewAPI 账户用量接口返回非成功状态"
            },
        ));
    }
    let text = crate::shared::http_body::read_text_with_limit(
        response,
        body_limit,
        "NewAPI account usage",
    )
    .await
    .map_err(|_| {
        newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 账户用量响应超过限制或读取失败",
        )
    })?;
    let body: Value = serde_json::from_str(&text).map_err(|_| {
        newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 账户用量接口返回了无效 JSON",
        )
    })?;
    if let Some(result) = newapi_application_error(&body, authenticated, fetched_at) {
        return Err(result);
    }
    Ok(body)
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
    let rate_limit_daily = match parse_sub2api_daily_rate_limit(body) {
        Ok(rate_limit) => rate_limit,
        Err(message) => {
            let mut result = ProviderAccountUsageResult::fetched(
                ProviderAccountUsageAdapterKind::Sub2api,
                ProviderAccountUsageStatus::QueryFailed,
                fetched_at,
            );
            result.message = Some(message.to_string());
            return result;
        }
    };
    let root_balance = number_at(body, &["balance"]);
    let remaining = number_at(body, &["remaining"]);
    let explicit_plan_remaining = number_at(body, &["plan_remaining", "planRemaining"]);
    let plan_remaining = explicit_plan_remaining;
    let balance = if root_balance.is_some() || plan_remaining.is_none() {
        root_balance.or(remaining)
    } else {
        None
    };
    let subscription = body.get("subscription").unwrap_or(&Value::Null);
    let plan_name =
        subscription_plan_name(body).or_else(|| string_at(body, &["planName", "plan_name"]));
    let daily_used = rate_limit_daily
        .map(|(_, used)| used)
        .or_else(|| number_at(subscription, &["daily_usage_usd", "dailyUsageUsd"]));
    let daily_total = rate_limit_daily
        .map(|(limit, _)| limit)
        .or_else(|| number_at(subscription, &["daily_limit_usd", "dailyLimitUsd"]));
    let weekly_used = number_at(subscription, &["weekly_usage_usd", "weeklyUsageUsd"]);
    let weekly_total = number_at(subscription, &["weekly_limit_usd", "weeklyLimitUsd"]);
    let monthly_used = number_at(subscription, &["monthly_usage_usd", "monthlyUsageUsd"]);
    let monthly_total = number_at(subscription, &["monthly_limit_usd", "monthlyLimitUsd"]);
    let expires_at = value_at(subscription, &["expires_at", "expiresAt"])
        .or_else(|| value_at(body, &["expires_at", "expiresAt"]))
        .and_then(parse_timestamp_value);

    if is_valid.is_none()
        && balance.is_none()
        && plan_remaining.is_none()
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

    let status =
        status_from_account_parts(is_valid, &[balance, plan_remaining], expires_at, now_unix);
    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Sub2api,
        status,
        fetched_at,
    );
    result.plan_name = plan_name;
    result.balance = balance;
    result.plan_remaining = plan_remaining;
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

fn parse_sub2api_daily_rate_limit(body: &Value) -> Result<Option<(f64, f64)>, &'static str> {
    let Some(rate_limits) = body.get("rate_limits") else {
        return Ok(None);
    };
    let Some(rate_limits) = rate_limits.as_array() else {
        return Err("sub2api rate_limits 响应格式无效");
    };

    let mut daily = None;
    for rate_limit in rate_limits {
        if rate_limit.get("window").and_then(Value::as_str) != Some("1d") {
            continue;
        }
        if daily.is_some() {
            return Err("sub2api rate_limits 包含重复的 1d 周期");
        }
        let number = |key: &str| {
            rate_limit
                .get(key)
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value >= 0.0)
        };
        let Some(limit) = number("limit") else {
            return Err("sub2api 1d 周期额度无效");
        };
        let Some(used) = number("used") else {
            return Err("sub2api 1d 周期已用量无效");
        };
        let Some(remaining) = number("remaining") else {
            return Err("sub2api 1d 周期剩余额度无效");
        };
        if used > limit {
            return Err("sub2api 1d 周期额度不一致");
        }
        let expected_remaining = limit - used;
        let tolerance = 1e-9_f64.max(limit.abs() * 1e-9);
        if (expected_remaining - remaining).abs() > tolerance {
            return Err("sub2api 1d 周期额度不一致");
        }
        let Some(window_start) = rate_limit
            .get("window_start")
            .and_then(parse_timestamp_value)
        else {
            return Err("sub2api 1d 周期起点无效");
        };
        let Some(reset_at) = rate_limit.get("reset_at").and_then(parse_timestamp_value) else {
            return Err("sub2api 1d 周期重置时间无效");
        };
        if reset_at <= window_start {
            return Err("sub2api 1d 周期时间范围无效");
        }
        daily = Some((limit, used));
    }
    Ok(daily)
}

fn parse_newapi_response(
    body: &Value,
    fetched_at: i64,
    _now_unix: i64,
) -> ProviderAccountUsageResult {
    if let Some(result) = newapi_application_error(body, true, fetched_at) {
        return result;
    }
    newapi_failed_result(
        ProviderAccountUsageStatus::QueryFailed,
        fetched_at,
        "NewAPI billing 响应必须按多端点契约解析",
    )
}

pub(crate) fn parse_newapi_billing_responses(
    status_body: &Value,
    subscription_body: &Value,
    usage_body: &Value,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    if let Some(result) = newapi_application_error(status_body, false, fetched_at) {
        return result;
    }
    if let Some(result) = newapi_application_error(subscription_body, true, fetched_at) {
        return result;
    }
    if let Some(result) = newapi_application_error(usage_body, true, fetched_at) {
        return result;
    }

    let status = data_or_root(status_body);
    let subscription = data_or_root(subscription_body);
    let usage = data_or_root(usage_body);
    let Some(unit) = status
        .get("quota_display_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI status 响应缺少展示单位",
        );
    };
    if unit != "USD" {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 展示单位暂不受支持",
        );
    }

    let Some(total) = required_nonnegative_number(subscription, "hard_limit_usd") else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI subscription 响应缺少有效总额度",
        );
    };
    if subscription
        .get("has_payment_method")
        .and_then(Value::as_bool)
        .is_none()
    {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI subscription 响应缺少有效 payment method 字段",
        );
    }
    let Some(access_until) = required_integer(subscription, "access_until") else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI subscription 响应缺少有效过期时间",
        );
    };
    let Some(total_usage) = required_nonnegative_number(usage, "total_usage") else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI usage 响应缺少有效已用量",
        );
    };
    if total == NEWAPI_UNLIMITED_TOKEN_HARD_LIMIT_USD {
        let mut result = ProviderAccountUsageResult::fetched(
            ProviderAccountUsageAdapterKind::Newapi,
            ProviderAccountUsageStatus::Available,
            fetched_at,
        );
        result.message = Some("模型令牌无限额度".to_string());
        return result;
    }
    let used = total_usage / 100.0;
    if !used.is_finite() {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI usage 响应缺少有效已用量",
        );
    }
    let balance = total - used;
    if !balance.is_finite() {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI billing 端点返回不一致的额度",
        );
    }

    let expires_at = (access_until > 0).then_some(access_until);
    let account_status = status_from_account_parts(None, &[Some(balance)], expires_at, now_unix);
    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Newapi,
        account_status,
        fetched_at,
    );
    result.balance = Some(balance);
    result.used = Some(used);
    result.total = Some(total);
    result.unit = Some("USD".to_string());
    result.expires_at = expires_at;
    result
}

pub(crate) fn parse_newapi_account_responses(
    status_body: &Value,
    account_body: &Value,
    expected_user_id: &str,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult {
    if let Some(result) = newapi_application_error(status_body, false, fetched_at) {
        return result;
    }
    if let Some(result) = newapi_application_error(account_body, true, fetched_at) {
        return result;
    }
    if account_body.get("success").and_then(Value::as_bool) != Some(true) {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 用户账户响应缺少有效成功标志",
        );
    }

    let status = data_or_root(status_body);
    let account = data_or_root(account_body);
    if status.get("quota_display_type").and_then(Value::as_str) != Some("USD") {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 展示单位暂不受支持",
        );
    }
    let Some(quota_per_unit) = status
        .get("quota_per_unit")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI status 响应缺少有效换算基数",
        );
    };
    let Some(account_user_id) = account
        .get("id")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
    else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 用户账户响应缺少有效身份",
        );
    };
    if account_user_id.to_string() != expected_user_id {
        return newapi_failed_result(
            ProviderAccountUsageStatus::AuthFailed,
            fetched_at,
            "NewAPI 用户账户身份不匹配",
        );
    }
    let Some(quota) = account
        .get("quota")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
    else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 用户账户响应缺少有效余额",
        );
    };
    let Some(used_quota) = account
        .get("used_quota")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
    else {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 用户账户响应缺少有效历史消耗",
        );
    };
    let balance = quota / quota_per_unit;
    let used = used_quota / quota_per_unit;
    if !balance.is_finite() || !used.is_finite() {
        return newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 用户账户额度换算失败",
        );
    }

    let account_status = status_from_account_parts(None, &[Some(balance)], None, now_unix);
    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Newapi,
        account_status,
        fetched_at,
    );
    result.balance = Some(balance);
    result.used = Some(used);
    result.unit = Some("USD".to_string());
    result
}

fn data_or_root(body: &Value) -> &Value {
    body.get("data")
        .filter(|value| value.is_object())
        .unwrap_or(body)
}

fn newapi_application_error(
    body: &Value,
    authenticated: bool,
    fetched_at: i64,
) -> Option<ProviderAccountUsageResult> {
    if body.get("success").and_then(Value::as_bool) == Some(false) {
        return Some(newapi_failed_result(
            if authenticated {
                ProviderAccountUsageStatus::AuthFailed
            } else {
                ProviderAccountUsageStatus::QueryFailed
            },
            fetched_at,
            if authenticated {
                "NewAPI 账户接口认证失败"
            } else {
                "NewAPI status 接口返回应用错误"
            },
        ));
    }
    if body.get("error").is_some_and(|error| !error.is_null()) {
        return Some(newapi_failed_result(
            ProviderAccountUsageStatus::QueryFailed,
            fetched_at,
            "NewAPI 账户用量接口返回错误对象",
        ));
    }
    None
}

fn required_nonnegative_number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn required_integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key)?.as_i64()
}

fn newapi_failed_result(
    status: ProviderAccountUsageStatus,
    fetched_at: i64,
    message: &str,
) -> ProviderAccountUsageResult {
    let mut result = ProviderAccountUsageResult::fetched(
        ProviderAccountUsageAdapterKind::Newapi,
        status,
        fetched_at,
    );
    result.message = Some(message.to_string());
    result
}

fn status_from_account_parts(
    is_valid: Option<bool>,
    spendable_amounts: &[Option<f64>],
    expires_at: Option<i64>,
    now_unix: i64,
) -> ProviderAccountUsageStatus {
    if is_valid == Some(false) {
        return ProviderAccountUsageStatus::AuthFailed;
    }
    if expires_at.is_some_and(|expires_at| expires_at <= now_unix) {
        return ProviderAccountUsageStatus::Expired;
    }
    let mut has_known_amount = false;
    let mut has_positive_amount = false;
    for amount in spendable_amounts.iter().flatten() {
        has_known_amount = true;
        if *amount > 0.0 {
            has_positive_amount = true;
            break;
        }
    }
    if has_known_amount && !has_positive_amount {
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

fn subscription_plan_name(body: &Value) -> Option<String> {
    let array_name = body
        .get("subscriptions")
        .and_then(Value::as_array)
        .and_then(|subscriptions| {
            subscriptions
                .iter()
                .find(|subscription| subscription_is_current(subscription))
                .and_then(|subscription| string_at(subscription, &["planName", "plan_name"]))
                .or_else(|| unique_subscription_plan_name(subscriptions))
        });

    array_name.or_else(|| {
        body.get("subscription")
            .and_then(|subscription| string_at(subscription, &["planName", "plan_name"]))
    })
}

fn subscription_is_current(subscription: &Value) -> bool {
    let current_keys = ["active", "isActive", "current", "isCurrent", "enabled"];
    if current_keys
        .iter()
        .any(|key| subscription.get(*key).and_then(Value::as_bool) == Some(true))
    {
        return true;
    }

    string_at(subscription, &["status", "state"]).is_some_and(|status| {
        matches!(
            status.to_ascii_lowercase().as_str(),
            "active" | "current" | "enabled" | "valid"
        )
    })
}

fn unique_subscription_plan_name(subscriptions: &[Value]) -> Option<String> {
    let mut unique_name: Option<String> = None;
    for subscription in subscriptions {
        let Some(name) = string_at(subscription, &["planName", "plan_name"]) else {
            continue;
        };
        if unique_name
            .as_deref()
            .is_some_and(|existing| existing == name)
        {
            continue;
        }
        if unique_name.is_some() {
            return None;
        }
        unique_name = Some(name);
    }
    unique_name
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
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::Mutex;

    async fn spawn_http_sequence(
        responses: Vec<(u16, String)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = stream.read(&mut buffer).await.expect("read request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                captured
                    .lock()
                    .await
                    .push(String::from_utf8_lossy(&request).into_owned());
                let reason = if status == 200 {
                    "OK"
                } else if status == 302 {
                    "Found"
                } else {
                    "Error"
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
        });
        (format!("http://{address}"), requests, task)
    }

    async fn spawn_subscription_redirect(
        target: &str,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind redirect server");
        let address = listener.local_addr().expect("redirect server address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let target = target.to_string();
        let task = tokio::spawn(async move {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = stream.read(&mut buffer).await.expect("read request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                captured
                    .lock()
                    .await
                    .push(String::from_utf8_lossy(&request).into_owned());
                let response = if index == 0 {
                    let body = json!({ "data": { "quota_display_type": "USD" } }).to_string();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                    )
                };
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
        });
        (format!("http://{address}"), requests, task)
    }

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
        assert_eq!(result.plan_remaining, None);
        assert_eq!(result.daily_used, Some(1.25));
        assert_eq!(result.weekly_total, Some(70.0));
        assert_eq!(result.monthly_total, Some(300.0));
        assert_eq!(result.expires_at, Some(1_893_456_000));
    }

    #[test]
    fn sub2api_mixed_balance_and_package_payload_keeps_amounts_separate() {
        let body = json!({
            "isValid": true,
            "planName": "套餐+余额",
            "balance": 0,
            "remaining": 42,
            "plan_remaining": 42,
            "subscriptions": [
                {
                    "planName": "Starter",
                    "active": false
                },
                {
                    "planName": "Super Ultra",
                    "active": true,
                    "remaining": 12,
                    "todayRemaining": 6
                }
            ]
        });

        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &body,
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.plan_name.as_deref(), Some("Super Ultra"));
        assert_eq!(result.balance, Some(0.0));
        assert_eq!(result.plan_remaining, Some(42.0));
    }

    #[test]
    fn sub2api_mixed_payload_status_priority_preserves_auth_and_expiry() {
        let invalid = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({
                "isValid": false,
                "balance": 0,
                "plan_remaining": 42,
                "remaining": 42
            }),
            100,
            1_800_000_000,
        );
        let expired = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({
                "isValid": true,
                "balance": 0,
                "plan_remaining": 42,
                "remaining": 42,
                "expires_at": 1_700_000_000
            }),
            100,
            1_800_000_000,
        );

        assert_eq!(invalid.status, ProviderAccountUsageStatus::AuthFailed);
        assert_eq!(expired.status, ProviderAccountUsageStatus::Expired);
    }

    #[test]
    fn sub2api_mixed_payload_zero_balance_requires_no_positive_spendable_amounts() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({
                "isValid": true,
                "balance": 0,
                "plan_remaining": 0,
                "remaining": 0
            }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::ZeroBalance);
        assert_eq!(result.balance, Some(0.0));
        assert_eq!(result.plan_remaining, Some(0.0));
    }

    #[test]
    fn sub2api_does_not_promote_ambiguous_remaining_to_plan_allowance() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({
                "isValid": true,
                "balance": 0,
                "remaining": 42
            }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::ZeroBalance);
        assert_eq!(result.balance, Some(0.0));
        assert_eq!(result.plan_remaining, None);
    }

    #[test]
    fn sub2api_explicit_plan_remaining_without_cash_balance_does_not_duplicate_remaining() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({
                "isValid": true,
                "remaining": 42,
                "plan_remaining": 42
            }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.balance, None);
        assert_eq!(result.plan_remaining, Some(42.0));
    }

    #[test]
    fn sub2api_multiple_subscription_names_without_selector_falls_back_to_root_plan() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Sub2api,
            &json!({
                "isValid": true,
                "planName": "套餐+余额",
                "balance": 3,
                "plan_remaining": 42,
                "subscriptions": [
                    { "planName": "Starter" },
                    { "planName": "Super Ultra" }
                ]
            }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.plan_name.as_deref(), Some("套餐+余额"));
        assert_eq!(result.balance, Some(3.0));
        assert_eq!(result.plan_remaining, Some(42.0));
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
        assert_eq!(result.plan_remaining, None);
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

    fn valid_newapi_billing() -> (Value, Value, Value) {
        (
            json!({ "success": true, "data": { "quota_display_type": "USD" } }),
            json!({
                "hard_limit_usd": 12.5,
                "has_payment_method": true,
                "access_until": 1_900_000_000
            }),
            json!({ "total_usage": 250.0 }),
        )
    }

    #[test]
    fn newapi_billing_normalizes_usd_formula_and_expiry() {
        let (status, subscription, usage) = valid_newapi_billing();
        let result =
            parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);

        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.total, Some(12.5));
        assert_eq!(result.used, Some(2.5));
        assert_eq!(result.balance, Some(10.0));
        assert_eq!(result.unit.as_deref(), Some("USD"));
        assert_eq!(result.expires_at, Some(1_900_000_000));
        assert!(result.unit_note.is_none());
    }

    #[test]
    fn newapi_billing_allows_finite_overage_and_uses_shared_status_mapping() {
        let (status, mut subscription, usage) = valid_newapi_billing();
        subscription["hard_limit_usd"] = json!(1.0);
        let result =
            parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);

        assert_eq!(result.total, Some(1.0));
        assert_eq!(result.used, Some(2.5));
        assert_eq!(result.balance, Some(-1.5));
        assert_eq!(result.status, ProviderAccountUsageStatus::ZeroBalance);
    }

    #[test]
    fn newapi_live_shaped_application_error_is_auth_failed_before_legacy_fields() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Newapi,
            &json!({ "success": false, "message": "synthetic upstream detail" }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::AuthFailed);
        assert_eq!(result.message.as_deref(), Some("NewAPI 账户接口认证失败"));
        assert!(!result.message.as_deref().unwrap_or("").contains("quota"));
        assert!(!result
            .message
            .as_deref()
            .unwrap_or("")
            .contains("synthetic"));
    }

    #[test]
    fn newapi_legacy_quota_shape_fails_closed_without_old_divisor() {
        let result = parse_account_usage_response(
            ProviderAccountUsageAdapterKind::Newapi,
            &json!({ "quota": 500_000, "used_quota": 100_000 }),
            100,
            1_800_000_000,
        );

        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(result.balance.is_none());
        assert!(result.used.is_none());
        assert!(result.total.is_none());
        assert!(result.message.as_deref().unwrap_or("").contains("多端点"));
    }

    #[test]
    fn newapi_application_errors_precede_required_field_validation() {
        let (status, subscription, usage) = valid_newapi_billing();
        let auth = parse_newapi_billing_responses(
            &status,
            &json!({ "success": false, "message": "do not expose" }),
            &usage,
            100,
            1_800_000_000,
        );
        let root_error = parse_newapi_billing_responses(
            &status,
            &subscription,
            &json!({ "error": { "message": "do not expose" } }),
            100,
            1_800_000_000,
        );

        assert_eq!(auth.status, ProviderAccountUsageStatus::AuthFailed);
        assert_eq!(root_error.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(auth.total.is_none());
        assert!(root_error.total.is_none());
        assert!(!auth.message.as_deref().unwrap_or("").contains("expose"));
    }

    #[test]
    fn newapi_rejects_missing_nonfinite_and_negative_required_numbers() {
        let (status, subscription, usage) = valid_newapi_billing();
        let cases = [
            (json!({}), usage.clone()),
            (
                json!({
                    "hard_limit_usd": "NaN",
                    "has_payment_method": true,
                    "access_until": 0
                }),
                usage.clone(),
            ),
            (
                json!({
                    "hard_limit_usd": "Infinity",
                    "has_payment_method": true,
                    "access_until": 0
                }),
                usage.clone(),
            ),
            (
                json!({
                    "hard_limit_usd": -1,
                    "has_payment_method": true,
                    "access_until": 0
                }),
                usage.clone(),
            ),
            (subscription.clone(), json!({ "total_usage": "NaN" })),
            (subscription.clone(), json!({ "total_usage": "Infinity" })),
            (subscription, json!({ "total_usage": -1 })),
        ];

        for (subscription, usage) in cases {
            let result =
                parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);
            assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
            assert!(result.total.is_none());
        }
    }

    #[test]
    fn newapi_rejects_unknown_or_non_usd_units_without_labeling_usd() {
        let (_, subscription, usage) = valid_newapi_billing();
        for unit in ["CNY", "TOKENS", "unknown", "usd"] {
            let result = parse_newapi_billing_responses(
                &json!({ "data": { "quota_display_type": unit } }),
                &subscription,
                &usage,
                100,
                1_800_000_000,
            );
            assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
            assert!(result.unit.is_none());
        }
    }

    #[test]
    fn newapi_access_until_zero_has_no_expiry_and_past_expiry_wins() {
        let (status, mut subscription, usage) = valid_newapi_billing();
        subscription["access_until"] = json!(0);
        let no_expiry =
            parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);
        subscription["access_until"] = json!(1_700_000_000);
        let expired =
            parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);

        assert_eq!(no_expiry.expires_at, None);
        assert_eq!(no_expiry.status, ProviderAccountUsageStatus::Available);
        assert_eq!(expired.status, ProviderAccountUsageStatus::Expired);
    }

    #[test]
    fn newapi_access_until_requires_exact_in_range_json_integer() {
        let (status, mut subscription, usage) = valid_newapi_billing();
        let exact = 9_007_199_254_740_993_i64;
        subscription["access_until"] = json!(exact);
        let exact_result =
            parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);

        assert_eq!(exact_result.expires_at, Some(exact));

        for invalid in [json!(1.0), json!(9_223_372_036_854_775_808_u64)] {
            subscription["access_until"] = invalid;
            let result =
                parse_newapi_billing_responses(&status, &subscription, &usage, 100, 1_800_000_000);
            assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
            assert!(result.expires_at.is_none());
            assert!(result.total.is_none());
        }
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
        for base in [
            "https://newapi.example.test",
            "https://newapi.example.test/v1/?x=1#fragment",
        ] {
            let urls = build_newapi_billing_urls(base).unwrap();
            assert_eq!(
                urls.status.as_str(),
                "https://newapi.example.test/api/status"
            );
            assert_eq!(
                urls.subscription.as_str(),
                "https://newapi.example.test/v1/dashboard/billing/subscription"
            );
            assert_eq!(
                urls.usage.as_str(),
                "https://newapi.example.test/v1/dashboard/billing/usage"
            );
            let account_urls = build_newapi_account_urls(base).unwrap();
            assert_eq!(
                account_urls.status.as_str(),
                "https://newapi.example.test/api/status"
            );
            assert_eq!(
                account_urls.account.as_str(),
                "https://newapi.example.test/api/user/self"
            );
        }
    }

    #[tokio::test]
    async fn newapi_rejects_credentialed_and_invalid_base_urls_without_requests() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind zero-request server");
        let address = listener.local_addr().expect("server address");
        let no_request = tokio::spawn(async move {
            tokio::time::timeout(std::time::Duration::from_millis(300), listener.accept())
                .await
                .is_err()
        });

        let credentialed = fetch_newapi_account_usage(
            &format!("http://user:SYNTHETIC_SECRET@{address}"),
            "SYNTHETIC_KEY",
            1,
            1,
        )
        .await;
        let invalid = fetch_newapi_account_usage("not a url", "SYNTHETIC_KEY", 1, 1).await;
        assert_eq!(
            credentialed.status,
            ProviderAccountUsageStatus::ConfigurationRequired
        );
        assert_eq!(
            invalid.status,
            ProviderAccountUsageStatus::ConfigurationRequired
        );
        assert!(no_request.await.expect("zero-request task"));
        assert!(!credentialed
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("SYNTHETIC_SECRET"));
    }

    #[test]
    fn newapi_date_window_tracks_payment_method_semantics() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 17, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        assert_eq!(
            newapi_usage_date_range(true, now).unwrap(),
            ("2026-07-01".to_string(), "2026-07-17".to_string())
        );
        assert_eq!(
            newapi_usage_date_range(false, now).unwrap(),
            ("2026-04-09".to_string(), "2026-07-17".to_string())
        );
    }

    #[tokio::test]
    async fn newapi_http_contract_uses_expected_urls_dates_auth_and_headers() {
        let responses = vec![
            (
                200,
                json!({ "data": { "quota_display_type": "USD" } }).to_string(),
            ),
            (
                200,
                json!({
                    "hard_limit_usd": 12.5,
                    "has_payment_method": true,
                    "access_until": 1_900_000_000
                })
                .to_string(),
            ),
            (200, json!({ "total_usage": 250 }).to_string()),
        ];
        let (base, requests, server) = spawn_http_sequence(responses).await;
        let now = Utc
            .with_ymd_and_hms(2026, 7, 17, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp();

        let result =
            fetch_newapi_account_usage(&format!("{base}/v1"), "synthetic-test-key", now, now).await;
        server.await.unwrap();

        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /api/status HTTP/1.1"));
        assert!(requests[1].starts_with("GET /v1/dashboard/billing/subscription HTTP/1.1"));
        assert!(requests[2].starts_with(
            "GET /v1/dashboard/billing/usage?start_date=2026-07-01&end_date=2026-07-17 HTTP/1.1"
        ));
        let status_headers = requests[0].to_ascii_lowercase();
        assert!(!status_headers.contains("authorization:"));
        for request in &requests[1..] {
            let headers = request.to_ascii_lowercase();
            assert!(headers.contains("authorization: bearer synthetic-test-key"));
            assert!(!headers.contains("new-api-user:"));
        }
        for request in requests.iter() {
            assert!(request
                .to_ascii_lowercase()
                .contains("accept: application/json"));
        }
    }

    #[tokio::test]
    async fn newapi_http_contract_fails_closed_on_partial_endpoint_failure() {
        let responses = vec![
            (
                200,
                json!({ "data": { "quota_display_type": "USD" } }).to_string(),
            ),
            (
                200,
                json!({
                    "hard_limit_usd": 12.5,
                    "has_payment_method": false,
                    "access_until": 0
                })
                .to_string(),
            ),
            (500, json!({ "success": false }).to_string()),
        ];
        let (base, requests, server) = spawn_http_sequence(responses).await;
        let result =
            fetch_newapi_account_usage(&base, "synthetic-test-key", 1_800_000_000, 1_800_000_000)
                .await;
        server.await.unwrap();

        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(result.balance.is_none());
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 3);
        assert!(requests[2].contains("start_date=2026-10-08&end_date=2027-01-15"));
    }

    #[tokio::test]
    async fn newapi_http_contract_classifies_application_auth_error_before_fields() {
        let responses = vec![
            (
                200,
                json!({ "data": { "quota_display_type": "USD" } }).to_string(),
            ),
            (
                200,
                json!({ "success": false, "message": "synthetic upstream detail" }).to_string(),
            ),
        ];
        let (base, requests, server) = spawn_http_sequence(responses).await;
        let result =
            fetch_newapi_account_usage(&base, "synthetic-test-key", 1_800_000_000, 1_800_000_000)
                .await;
        server.await.unwrap();

        assert_eq!(result.status, ProviderAccountUsageStatus::AuthFailed);
        assert_eq!(result.message.as_deref(), Some("NewAPI 账户接口认证失败"));
        assert!(!result
            .message
            .as_deref()
            .unwrap_or("")
            .contains("synthetic"));
        assert_eq!(requests.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn newapi_http_contract_enforces_status_body_cap() {
        let oversized = format!(
            "{{\"padding\":\"{}\"}}",
            "x".repeat(NEWAPI_STATUS_BODY_LIMIT)
        );
        let (base, requests, server) = spawn_http_sequence(vec![(200, oversized)]).await;
        let result =
            fetch_newapi_account_usage(&base, "synthetic-test-key", 1_800_000_000, 1_800_000_000)
                .await;
        server.await.unwrap();

        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(result.balance.is_none());
        assert_eq!(requests.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn newapi_http_contract_enforces_subscription_and_usage_caps_all_or_nothing() {
        let valid_status = json!({ "data": { "quota_display_type": "USD" } }).to_string();
        let valid_subscription = json!({
            "hard_limit_usd": 12.5,
            "has_payment_method": true,
            "access_until": 1_900_000_000
        })
        .to_string();
        for (responses, expected_requests) in [
            (
                vec![
                    (200, valid_status.clone()),
                    (
                        200,
                        format!(
                            "{{\"padding\":\"{}\"}}",
                            "x".repeat(NEWAPI_SUBSCRIPTION_BODY_LIMIT)
                        ),
                    ),
                ],
                2,
            ),
            (
                vec![
                    (200, valid_status.clone()),
                    (200, valid_subscription.clone()),
                    (
                        200,
                        format!(
                            "{{\"padding\":\"{}\"}}",
                            "x".repeat(NEWAPI_USAGE_BODY_LIMIT)
                        ),
                    ),
                ],
                3,
            ),
        ] {
            let (base, requests, server) = spawn_http_sequence(responses).await;
            let result =
                fetch_newapi_account_usage(&base, "SYNTHETIC_KEY", 1_800_000_000, 1_800_000_000)
                    .await;
            server.await.expect("server");
            assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
            assert!(result.balance.is_none());
            assert!(result.total.is_none());
            assert_eq!(requests.lock().await.len(), expected_requests);
        }
    }

    #[tokio::test]
    async fn newapi_real_redirect_is_not_followed_and_bearer_is_not_forwarded() {
        let target_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind redirect target");
        let target_addr = target_listener.local_addr().expect("target addr");
        let target = tokio::spawn(async move {
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                target_listener.accept(),
            )
            .await
            .is_ok()
        });
        let (base, requests, server) =
            spawn_subscription_redirect(&format!("http://{target_addr}/capture")).await;
        let result = fetch_newapi_account_usage(
            &base,
            "SYNTHETIC_BEARER_SECRET",
            1_800_000_000,
            1_800_000_000,
        )
        .await;
        server.await.expect("redirect server");
        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(
            !target.await.expect("target task"),
            "redirect target was contacted"
        );
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic_bearer_secret"));
    }

    #[test]
    fn account_usage_config_defaults_newapi_to_billing_and_sanitizes_private_fields() {
        let configured = config_from_value(&json!({
            "adapterKind": "newapi",
            "newApiUserId": "42",
            "newApiAccessToken": "SYNTHETIC_PRIVATE_VALUE"
        }));
        assert_eq!(
            configured,
            ProviderAccountUsageConfigState::Configured(ProviderAccountUsageConfig {
                adapter_kind: ProviderAccountUsageAdapterKind::Newapi,
                new_api_query_mode: NewapiQueryMode::Billing,
            })
        );

        let sanitized = sanitize_account_usage_extension_value(&json!({
            "adapterKind": "newapi",
            "newApiQueryMode": "account",
            "newApiUserId": "42",
            "newApiAccessToken": "SYNTHETIC_PRIVATE_VALUE",
            "timedRefreshEnabled": false,
            "refreshIntervalSeconds": 120
        }));
        assert_eq!(
            sanitized,
            json!({
                "adapterKind": "newapi",
                "newApiQueryMode": "account",
                "timedRefreshEnabled": false,
                "refreshIntervalSeconds": 120
            })
        );
        let serialized = sanitized.to_string();
        assert!(!serialized.contains("UserId"));
        assert!(!serialized.contains("AccessToken"));
        assert!(!serialized.contains("SYNTHETIC_PRIVATE_VALUE"));
    }

    #[test]
    fn newapi_user_id_uses_the_positive_signed_go_integer_range() {
        assert_eq!(normalize_newapi_user_id("00042").unwrap(), "42");
        assert_eq!(
            normalize_newapi_user_id(&i64::MAX.to_string()).unwrap(),
            i64::MAX.to_string()
        );

        for invalid in ["0", "-1", "9223372036854775808", "18446744073709551615"] {
            assert!(normalize_newapi_user_id(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn newapi_billing_recognizes_only_the_exact_unlimited_sentinel() {
        let (status, mut subscription, usage) = valid_newapi_billing();
        subscription["hard_limit_usd"] = json!(NEWAPI_UNLIMITED_TOKEN_HARD_LIMIT_USD);
        let unlimited = parse_newapi_billing_responses(
            &status,
            &subscription,
            &usage,
            1_800_000_000,
            1_800_000_000,
        );
        assert_eq!(unlimited.status, ProviderAccountUsageStatus::Available);
        assert_eq!(unlimited.message.as_deref(), Some("模型令牌无限额度"));
        assert!(unlimited.total.is_none());
        assert!(unlimited.used.is_none());
        assert!(unlimited.balance.is_none());
        assert!(unlimited.unit.is_none());

        for finite_total in [
            NEWAPI_UNLIMITED_TOKEN_HARD_LIMIT_USD - 1.0,
            NEWAPI_UNLIMITED_TOKEN_HARD_LIMIT_USD + 1.0,
        ] {
            subscription["hard_limit_usd"] = json!(finite_total);
            let finite = parse_newapi_billing_responses(
                &status,
                &subscription,
                &usage,
                1_800_000_000,
                1_800_000_000,
            );
            assert_eq!(finite.total, Some(finite_total));
            assert!(finite.balance.is_some());
        }
    }

    #[test]
    fn newapi_account_parser_maps_balance_and_historical_usage_without_total() {
        let result = parse_newapi_account_responses(
            &json!({
                "data": { "quota_display_type": "USD", "quota_per_unit": 100.0 }
            }),
            &json!({
                "success": true,
                "data": { "id": 42, "quota": -250.0, "used_quota": 375.0 }
            }),
            "42",
            1_800_000_000,
            1_800_000_000,
        );
        assert_eq!(result.status, ProviderAccountUsageStatus::ZeroBalance);
        assert_eq!(result.balance, Some(-2.5));
        assert_eq!(result.used, Some(3.75));
        assert!(result.total.is_none());
        assert_eq!(result.unit.as_deref(), Some("USD"));
    }

    #[test]
    fn newapi_account_parser_fails_closed_on_success_identity_unit_and_number_errors() {
        let valid_status = json!({
            "data": { "quota_display_type": "USD", "quota_per_unit": 100.0 }
        });
        let valid_account = json!({
            "success": true,
            "data": { "id": 42, "quota": 500.0, "used_quota": 125.0 }
        });
        for (status, account, expected_status) in [
            (
                json!({ "data": { "quota_display_type": "TOKENS", "quota_per_unit": 100.0 } }),
                valid_account.clone(),
                ProviderAccountUsageStatus::QueryFailed,
            ),
            (
                valid_status.clone(),
                json!({ "success": true, "data": { "id": 7, "quota": 500.0, "used_quota": 125.0 } }),
                ProviderAccountUsageStatus::AuthFailed,
            ),
            (
                valid_status.clone(),
                json!({ "success": true, "data": { "id": 9_223_372_036_854_775_808_u64, "quota": 500.0, "used_quota": 125.0 } }),
                ProviderAccountUsageStatus::QueryFailed,
            ),
            (
                valid_status.clone(),
                json!({ "success": true, "data": { "id": 42, "quota": 500.0, "used_quota": -1.0 } }),
                ProviderAccountUsageStatus::QueryFailed,
            ),
            (
                valid_status.clone(),
                json!({
                    "success": false,
                    "message": "SYNTHETIC_UPSTREAM_MESSAGE",
                    "data": { "id": 42, "quota": 500.0, "used_quota": 125.0 }
                }),
                ProviderAccountUsageStatus::AuthFailed,
            ),
            (
                valid_status.clone(),
                json!({ "data": { "id": 42, "quota": 500.0, "used_quota": 125.0 } }),
                ProviderAccountUsageStatus::QueryFailed,
            ),
        ] {
            let result = parse_newapi_account_responses(
                &status,
                &account,
                "42",
                1_800_000_000,
                1_800_000_000,
            );
            assert_eq!(result.status, expected_status);
            assert!(result.balance.is_none());
            assert!(result.used.is_none());
            assert!(result.total.is_none());
            assert!(result.unit.is_none());
            assert!(!result
                .message
                .as_deref()
                .unwrap_or_default()
                .contains("SYNTHETIC_UPSTREAM_MESSAGE"));
        }
    }

    #[tokio::test]
    async fn newapi_account_http_contract_separates_public_and_private_headers() {
        let responses = vec![
            (
                200,
                json!({
                    "data": { "quota_display_type": "USD", "quota_per_unit": 100.0 }
                })
                .to_string(),
            ),
            (
                200,
                json!({
                    "success": true,
                    "data": { "id": 42, "quota": 500.0, "used_quota": 125.0 }
                })
                .to_string(),
            ),
        ];
        let (base, requests, server) = spawn_http_sequence(responses).await;
        let result = fetch_newapi_user_account_usage(
            &format!("{base}/v1"),
            "SYNTHETIC_ACCOUNT_SECRET",
            "42",
            1_800_000_000,
            1_800_000_000,
        )
        .await;
        server.await.expect("account server");
        assert_eq!(result.balance, Some(5.0));
        assert_eq!(result.used, Some(1.25));

        let requests = requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("GET /api/status "));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
        assert!(!requests[0].to_ascii_lowercase().contains("new-api-user:"));
        assert!(requests[1].starts_with("GET /api/user/self "));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic_account_secret"));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("new-api-user: 42"));
    }

    #[tokio::test]
    async fn newapi_account_http_contract_enforces_body_cap_all_or_nothing() {
        let responses = vec![
            (
                200,
                json!({
                    "data": { "quota_display_type": "USD", "quota_per_unit": 100.0 }
                })
                .to_string(),
            ),
            (
                200,
                format!(
                    "{{\"padding\":\"{}\"}}",
                    "x".repeat(NEWAPI_ACCOUNT_BODY_LIMIT)
                ),
            ),
        ];
        let (base, requests, server) = spawn_http_sequence(responses).await;
        let result = fetch_newapi_user_account_usage(
            &base,
            "SYNTHETIC_ACCOUNT_SECRET",
            "42",
            1_800_000_000,
            1_800_000_000,
        )
        .await;
        server.await.expect("account cap server");
        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(result.balance.is_none());
        assert!(result.used.is_none());
        assert!(result.unit.is_none());
        assert_eq!(requests.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn newapi_account_redirect_is_not_followed_or_forwarded() {
        let target_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind account redirect target");
        let target_addr = target_listener.local_addr().expect("target addr");
        let target = tokio::spawn(async move {
            tokio::time::timeout(
                std::time::Duration::from_millis(500),
                target_listener.accept(),
            )
            .await
            .is_ok()
        });
        let (base, requests, server) =
            spawn_subscription_redirect(&format!("http://{target_addr}/capture")).await;
        let result = fetch_newapi_user_account_usage(
            &base,
            "SYNTHETIC_ACCOUNT_SECRET",
            "42",
            1_800_000_000,
            1_800_000_000,
        )
        .await;
        server.await.expect("account redirect server");
        assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
        assert!(!target.await.expect("target task"));
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic_account_secret"));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("new-api-user: 42"));
    }

    #[tokio::test]
    async fn newapi_account_missing_credentials_returns_before_network_setup() {
        let result = fetch_newapi_user_account_usage(
            "https://example.invalid/v1",
            "",
            "42",
            1_800_000_000,
            1_800_000_000,
        )
        .await;
        assert_eq!(
            result.status,
            ProviderAccountUsageStatus::ConfigurationRequired
        );
        assert_eq!(result.freshness, ProviderAccountUsageFreshness::NotFetched);
    }

    #[test]
    fn sub2api_rate_limits_maps_only_a_valid_unique_daily_window() {
        let result = parse_sub2api_response(
            &json!({
                "isValid": true,
                "rate_limits": [{
                    "window": "1d",
                    "limit": 12.0,
                    "used": 4.5,
                    "remaining": 7.5,
                    "window_start": 1_800_000_000,
                    "reset_at": 1_800_086_400
                }]
            }),
            1_800_000_000,
            1_800_000_000,
        );
        assert_eq!(result.status, ProviderAccountUsageStatus::Available);
        assert_eq!(result.daily_total, Some(12.0));
        assert_eq!(result.daily_used, Some(4.5));
        assert!(result.balance.is_none());
        assert!(result.plan_remaining.is_none());

        let unknown = parse_sub2api_response(
            &json!({
                "isValid": true,
                "rate_limits": [{ "window": "7d", "limit": 70.0 }]
            }),
            1_800_000_000,
            1_800_000_000,
        );
        assert_eq!(unknown.status, ProviderAccountUsageStatus::Available);
        assert!(unknown.daily_total.is_none());
        assert!(unknown.weekly_total.is_none());
    }

    #[test]
    fn sub2api_rate_limits_known_daily_window_fails_closed_when_malformed() {
        let cases = [
            json!({
                "isValid": true,
                "rate_limits": [
                    { "window": "1d", "limit": 10.0, "used": 2.0, "remaining": 8.0, "window_start": 1_800_000_000, "reset_at": 1_800_086_400 },
                    { "window": "1d", "limit": 10.0, "used": 2.0, "remaining": 8.0, "window_start": 1_800_000_000, "reset_at": 1_800_086_400 }
                ]
            }),
            json!({
                "isValid": true,
                "rate_limits": [{ "window": "1d", "limit": 10.0, "used": -1.0, "remaining": 11.0, "window_start": 1_800_000_000, "reset_at": 1_800_086_400 }]
            }),
            json!({
                "isValid": true,
                "rate_limits": [{ "window": "1d", "limit": 10.0, "used": 2.0, "remaining": 9.0, "window_start": 1_800_000_000, "reset_at": 1_800_086_400 }]
            }),
            json!({
                "isValid": true,
                "rate_limits": [{ "window": "1d", "limit": 10.0, "used": 2.0, "remaining": 8.0, "window_start": 1_800_086_400, "reset_at": 1_800_000_000 }]
            }),
        ];
        for body in cases {
            let result = parse_sub2api_response(&body, 1_800_000_000, 1_800_000_000);
            assert_eq!(result.status, ProviderAccountUsageStatus::QueryFailed);
            assert!(result.daily_total.is_none());
            assert!(result.daily_used.is_none());
            assert!(result.balance.is_none());
        }
    }

    #[test]
    fn account_credentials_patch_preserves_replaces_and_clears_as_a_group() {
        let conn = Connection::open_in_memory().expect("credentials db");
        conn.execute_batch(
            r#"
PRAGMA foreign_keys = ON;
CREATE TABLE providers(id INTEGER PRIMARY KEY);
CREATE TABLE provider_account_usage_credentials(
  provider_id INTEGER PRIMARY KEY,
  newapi_user_id TEXT,
  newapi_access_token_plaintext TEXT,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
INSERT INTO providers(id) VALUES (1);
"#,
        )
        .expect("credentials schema");

        apply_account_usage_credentials_patch(
            &conn,
            1,
            Some(&ProviderAccountUsageCredentialsPatch {
                new_api_user_id: Some("00042".to_string()),
                new_api_access_token: Some("SYNTHETIC_ACCOUNT_SECRET_A".to_string()),
                clear_new_api_access_token: false,
            }),
        )
        .expect("initial credentials");
        let initial = load_account_usage_credentials(&conn, 1).expect("load initial");
        assert_eq!(initial.new_api_user_id.as_deref(), Some("42"));
        assert_eq!(
            initial.new_api_access_token.as_deref(),
            Some("SYNTHETIC_ACCOUNT_SECRET_A")
        );

        apply_account_usage_credentials_patch(
            &conn,
            1,
            Some(&ProviderAccountUsageCredentialsPatch {
                new_api_user_id: Some("7".to_string()),
                new_api_access_token: None,
                clear_new_api_access_token: false,
            }),
        )
        .expect("preserve token");
        let preserved = load_account_usage_credentials(&conn, 1).expect("load preserved");
        assert_eq!(preserved.new_api_user_id.as_deref(), Some("7"));
        assert_eq!(
            preserved.new_api_access_token.as_deref(),
            Some("SYNTHETIC_ACCOUNT_SECRET_A")
        );

        apply_account_usage_credentials_patch(
            &conn,
            1,
            Some(&ProviderAccountUsageCredentialsPatch {
                new_api_user_id: None,
                new_api_access_token: None,
                clear_new_api_access_token: true,
            }),
        )
        .expect("clear credentials");
        let cleared = load_account_usage_credentials(&conn, 1).expect("load cleared");
        assert!(cleared.new_api_user_id.is_none());
        assert!(cleared.new_api_access_token.is_none());
        let row_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM provider_account_usage_credentials",
                [],
                |row| row.get(0),
            )
            .expect("credential row count");
        assert_eq!(row_count, 0);
    }

    #[test]
    fn redaction_removes_api_key_material() {
        let redacted = redact_secret("request failed for sk-secret-value", "sk-secret-value");
        assert_eq!(redacted, "request failed for [REDACTED]");
    }
}
