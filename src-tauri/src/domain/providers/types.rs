//! Types for provider configuration and gateway selection.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub(super) const DEFAULT_PRIORITY: i64 = 100;
pub(super) const MAX_MODEL_NAME_LEN: usize = 200;
pub(crate) const CX2CC_BRIDGE_TYPE: &str = "cx2cc";
pub(crate) const CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE: &str = "codex_to_openai_chat";
pub(crate) const CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE: &str = "codex_to_openai_responses";
pub(crate) const CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE: &str = "codex_to_anthropic_messages";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DailyResetMode {
    Fixed,
    Rolling,
}

impl DailyResetMode {
    pub(super) fn parse(input: &str) -> Option<Self> {
        match input.trim() {
            "fixed" => Some(Self::Fixed),
            "rolling" => Some(Self::Rolling),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Rolling => "rolling",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthMode {
    ApiKey,
    Oauth,
}

impl ProviderAuthMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Oauth => "oauth",
        }
    }
}

pub(crate) fn is_cx2cc_bridge(bridge_type: Option<&str>) -> bool {
    bridge_type == Some(CX2CC_BRIDGE_TYPE)
}

pub(crate) fn has_bridged_input_semantics(
    source_provider_id: Option<i64>,
    bridge_type: Option<&str>,
) -> bool {
    source_provider_id.is_some() || is_cx2cc_bridge(bridge_type)
}

pub(crate) fn is_codex_bridge_type(bridge_type: &str) -> bool {
    matches!(
        bridge_type,
        CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE
            | CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE
            | CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE
    )
}

pub(crate) fn is_supported_bridge_type(bridge_type: &str) -> bool {
    bridge_type == CX2CC_BRIDGE_TYPE || is_codex_bridge_type(bridge_type)
}

fn take_first_chars(value: &str, max_chars: usize) -> String {
    if value.chars().nth(max_chars).is_none() {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

#[derive(Clone)]
pub struct ProviderUpsertParams {
    pub provider_id: Option<i64>,
    pub cli_key: String,
    pub name: String,
    pub base_urls: Vec<String>,
    pub base_url_mode: ProviderBaseUrlMode,
    pub auth_mode: Option<ProviderAuthMode>,
    pub api_key: Option<String>,
    pub enabled: bool,
    pub cost_multiplier: f64,
    pub priority: Option<i64>,
    pub claude_models: Option<ClaudeModels>,
    pub model_mapping: Option<ModelMapping>,
    pub availability_test_model: Option<String>,
    pub limit_5h_usd: Option<f64>,
    pub limit_daily_usd: Option<f64>,
    pub daily_reset_mode: Option<DailyResetMode>,
    pub daily_reset_time: Option<String>,
    pub limit_weekly_usd: Option<f64>,
    pub limit_monthly_usd: Option<f64>,
    pub limit_total_usd: Option<f64>,
    pub tags: Option<Vec<String>>,
    pub note: Option<String>,
    pub source_provider_id: Option<i64>,
    pub bridge_type: Option<String>,
    pub stream_idle_timeout_seconds: Option<u32>,
    pub extension_values: Option<Vec<ProviderExtensionValuesInput>>,
    pub account_usage_credentials_patch:
        Option<crate::domain::provider_account_usage::ProviderAccountUsageCredentialsPatch>,
    pub account_usage_credentials_copy_from_provider_id: Option<i64>,
    pub upstream_retry_policy_override: Option<crate::settings::UpstreamRetryPolicy>,
    pub upstream_retry_policy_override_specified: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct ClaudeModels {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haiku_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonnet_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus_model: Option<String>,
}

pub(super) fn normalize_model_slot(raw: Option<String>) -> Option<String> {
    let value = raw.map(|v| v.trim().to_string());
    let value = value.as_deref().unwrap_or("");
    if value.is_empty() {
        return None;
    }
    if value.chars().nth(MAX_MODEL_NAME_LEN).is_some() {
        return Some(take_first_chars(value, MAX_MODEL_NAME_LEN));
    }
    Some(value.to_string())
}

impl ClaudeModels {
    pub(super) fn normalized(self) -> Self {
        Self {
            main_model: normalize_model_slot(self.main_model),
            reasoning_model: normalize_model_slot(self.reasoning_model),
            haiku_model: normalize_model_slot(self.haiku_model),
            sonnet_model: normalize_model_slot(self.sonnet_model),
            opus_model: normalize_model_slot(self.opus_model),
        }
    }

    pub(crate) fn has_any(&self) -> bool {
        self.main_model.is_some()
            || self.reasoning_model.is_some()
            || self.haiku_model.is_some()
            || self.sonnet_model.is_some()
            || self.opus_model.is_some()
    }

    pub(crate) fn map_model(&self, original_model: &str, has_thinking: bool) -> String {
        let model_lower = original_model.to_ascii_lowercase();

        // 1) 按模型类型匹配（子串）
        if model_lower.contains("haiku") {
            if let Some(model) = self.haiku_model.as_deref() {
                return model.to_string();
            }
        }
        if model_lower.contains("opus") {
            if let Some(model) = self.opus_model.as_deref() {
                return model.to_string();
            }
        }
        if model_lower.contains("sonnet") {
            if let Some(model) = self.sonnet_model.as_deref() {
                return model.to_string();
            }
        }

        // 2) thinking 模式在未命中具体模型槽位时使用推理模型
        if has_thinking {
            if let Some(model) = self.reasoning_model.as_deref() {
                return model.to_string();
            }
        }

        // 3) 主模型兜底
        if let Some(model) = self.main_model.as_deref() {
            return model.to_string();
        }

        // 4) 无映射：保持原样
        original_model.to_string()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct ModelMapping {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub exact: BTreeMap<String, String>,
}

impl ModelMapping {
    pub(super) fn normalized(self) -> Self {
        let exact = self
            .exact
            .into_iter()
            .filter_map(|(key, value)| {
                let key = normalize_model_slot(Some(key))?;
                let value = normalize_model_slot(Some(value))?;
                Some((key, value))
            })
            .collect();

        Self {
            default_model: normalize_model_slot(self.default_model),
            exact,
        }
    }

    pub(crate) fn map_model(&self, original_model: &str) -> String {
        if let Some(mapped) = self.exact.get(original_model) {
            return mapped.clone();
        }
        if let Some(mapped) = self.default_model.as_deref() {
            return mapped.to_string();
        }
        original_model.to_string()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderBaseUrlMode {
    Order,
    Ping,
}

impl ProviderBaseUrlMode {
    pub(super) fn parse(input: &str) -> Option<Self> {
        match input.trim() {
            "order" => Some(Self::Order),
            "ping" => Some(Self::Ping),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Order => "order",
            Self::Ping => "ping",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExtensionValues {
    pub plugin_id: String,
    pub namespace: String,
    pub values: serde_json::Value,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExtensionValuesInput {
    pub plugin_id: String,
    pub namespace: String,
    pub values: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ProviderSummary {
    pub id: i64,
    pub cli_key: String,
    pub name: String,
    pub base_urls: Vec<String>,
    pub base_url_mode: ProviderBaseUrlMode,
    pub claude_models: ClaudeModels,
    pub model_mapping: ModelMapping,
    pub availability_test_model: Option<String>,
    pub enabled: bool,
    pub priority: i64,
    pub cost_multiplier: f64,
    pub limit_5h_usd: Option<f64>,
    pub limit_daily_usd: Option<f64>,
    pub daily_reset_mode: DailyResetMode,
    pub daily_reset_time: String,
    pub limit_weekly_usd: Option<f64>,
    pub limit_monthly_usd: Option<f64>,
    pub limit_total_usd: Option<f64>,
    pub tags: Vec<String>,
    pub note: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub auth_mode: String,
    pub oauth_provider_type: Option<String>,
    pub oauth_email: Option<String>,
    pub oauth_expires_at: Option<i64>,
    pub oauth_last_error: Option<String>,
    pub source_provider_id: Option<i64>,
    pub bridge_type: Option<String>,
    pub stream_idle_timeout_seconds: Option<u32>,
    pub extension_values: Vec<ProviderExtensionValues>,
    pub upstream_retry_policy_override: Option<crate::settings::UpstreamRetryPolicy>,
    pub api_key_configured: bool,
    pub newapi_account_user_id: Option<String>,
    pub newapi_account_access_token_configured: bool,
}

#[derive(Clone)]
pub(crate) struct ProviderAccountUsageFetchContext {
    pub base_urls: Vec<String>,
    pub auth_mode: String,
    pub source_provider_id: Option<i64>,
    pub extension_values: Vec<ProviderExtensionValues>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ProviderRouteRow {
    pub provider_id: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderForGateway {
    pub id: i64,
    pub name: String,
    pub base_urls: Vec<String>,
    pub base_url_mode: ProviderBaseUrlMode,
    pub api_key_plaintext: String,
    pub claude_models: ClaudeModels,
    pub model_mapping: ModelMapping,
    pub limit_5h_usd: Option<f64>,
    pub limit_daily_usd: Option<f64>,
    pub daily_reset_mode: DailyResetMode,
    pub daily_reset_time: String,
    pub limit_weekly_usd: Option<f64>,
    pub limit_monthly_usd: Option<f64>,
    pub limit_total_usd: Option<f64>,
    pub auth_mode: String,
    pub oauth_provider_type: Option<String>,
    pub source_provider_id: Option<i64>,
    #[allow(dead_code)] // Will be read when failover_loop uses bridge_type for dispatch.
    pub bridge_type: Option<String>,
    pub stream_idle_timeout_seconds: Option<u32>,
    pub extension_values: Vec<ProviderExtensionValues>,
    pub upstream_retry_policy_override: Option<crate::settings::UpstreamRetryPolicy>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderTransportContext {
    pub provider_id: i64,
    pub base_urls: Vec<String>,
    pub api_key_plaintext: String,
    pub auth_mode: String,
    pub oauth_provider_type: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct GatewayProvidersSelection {
    pub sort_mode_id: Option<i64>,
    pub providers: Vec<ProviderForGateway>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaudeTerminalLaunchContext {
    /// The credential to pass as ANTHROPIC_API_KEY to `claude` CLI.
    /// For `api_key` mode this is the stored api_key; for `oauth` mode it is the OAuth access token.
    pub api_key_plaintext: String,
}

impl ProviderForGateway {
    pub(crate) fn is_cx2cc_bridge(&self) -> bool {
        is_cx2cc_bridge(self.bridge_type.as_deref())
    }

    pub(crate) fn transport_context(&self) -> ProviderTransportContext {
        ProviderTransportContext {
            provider_id: self.id,
            base_urls: self.base_urls.clone(),
            api_key_plaintext: self.api_key_plaintext.clone(),
            auth_mode: self.auth_mode.clone(),
            oauth_provider_type: self.oauth_provider_type.clone(),
        }
    }

    pub(crate) fn get_effective_claude_model(
        &self,
        requested_model: &str,
        has_thinking: bool,
    ) -> String {
        self.claude_models.map_model(requested_model, has_thinking)
    }
}

#[derive(Debug, Clone)]
pub(super) struct DecodedProviderRow {
    pub id: i64,
    pub name: String,
    pub base_urls: Vec<String>,
    pub base_url_mode: ProviderBaseUrlMode,
    pub claude_models: ClaudeModels,
    pub model_mapping: ModelMapping,
    pub availability_test_model: Option<String>,
    pub limit_5h_usd: Option<f64>,
    pub limit_daily_usd: Option<f64>,
    pub daily_reset_mode: DailyResetMode,
    pub daily_reset_time: String,
    pub limit_weekly_usd: Option<f64>,
    pub limit_monthly_usd: Option<f64>,
    pub limit_total_usd: Option<f64>,
    pub auth_mode: String,
    pub oauth_provider_type: Option<String>,
    pub source_provider_id: Option<i64>,
    pub bridge_type: Option<String>,
    pub upstream_retry_policy_override: Option<crate::settings::UpstreamRetryPolicy>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderOAuthDetails {
    pub id: i64,
    pub cli_key: String,
    pub oauth_provider_type: String,
    pub oauth_access_token: String,
    pub oauth_refresh_token: Option<String>,
    pub oauth_id_token: Option<String>,
    pub oauth_token_uri: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<String>,
    pub oauth_expires_at: Option<i64>,
    pub oauth_email: Option<String>,
    pub oauth_refresh_lead_s: i64,
    pub oauth_last_refreshed_at: Option<i64>,
}

pub(super) fn claude_models_from_json(raw: &str) -> ClaudeModels {
    serde_json::from_str::<ClaudeModels>(raw)
        .ok()
        .unwrap_or_default()
        .normalized()
}

pub(super) fn model_mapping_from_json(raw: &str) -> ModelMapping {
    let parsed = match serde_json::from_str::<Value>(raw) {
        Ok(parsed) => parsed,
        Err(_) => return ModelMapping::default(),
    };

    if parsed
        .as_object()
        .is_some_and(|object| object.contains_key("default_model") || object.contains_key("exact"))
    {
        return serde_json::from_value::<ModelMapping>(parsed)
            .ok()
            .unwrap_or_default()
            .normalized();
    }

    let legacy_exact: BTreeMap<String, String> = serde_json::from_value(parsed).unwrap_or_default();
    ModelMapping {
        default_model: None,
        exact: legacy_exact,
    }
    .normalized()
}

pub(super) fn tags_from_json(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

pub(super) fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .filter(|v| seen.insert(v.clone()))
        .collect()
}
