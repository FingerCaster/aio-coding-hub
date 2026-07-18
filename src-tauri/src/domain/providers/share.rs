//! Strict single-provider share format, preview projection, and atomic import.

use super::queries::{
    next_sort_order, replace_extension_values, retry_policy_override_from_json,
    retry_policy_override_to_json,
};
use super::types::{
    claude_models_from_json, model_mapping_from_json, normalize_tags, ClaudeModels, DailyResetMode,
    ModelMapping, ProviderAuthMode, ProviderBaseUrlMode, ProviderExtensionValuesInput,
    ProviderSummary, CX2CC_BRIDGE_TYPE,
};
use super::validation::{
    base_urls_from_row, normalize_base_urls, normalize_note, normalize_reset_time_hms_strict,
    normalize_stream_idle_timeout_seconds, validate_claude_models, validate_cli_key,
    validate_limit_usd, validate_max_chars,
};
use crate::db;
use crate::domain::plugin_contributions::TargetCliKey;
use crate::domain::plugins::{PluginManifest, PluginStatus};
use crate::shared::error::{db_err, AppError, AppResult};
use crate::shared::time::now_unix_seconds;
use rusqlite::{named_params, params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;

pub(crate) const PROVIDER_SHARE_MAX_BYTES: usize = 8 * 1024 * 1024;
const PROVIDER_SHARE_KIND: &str = "aio-coding-hub.provider-share";
const PROVIDER_SHARE_SCHEMA_VERSION: u32 = 1;
const DEFAULT_OAUTH_REFRESH_LEAD_SECONDS: i64 = 3600;
const MAX_PROVIDER_NAME_CHARS: usize = 256;
const MAX_AUTH_SECRET_BYTES: usize = 256 * 1024;
const MAX_AUTH_TEXT_CHARS: usize = 4096;
const MAX_EXTENSION_ID_CHARS: usize = 256;
const MAX_EXTENSION_COUNT: usize = 128;
const MAX_TAG_COUNT: usize = 128;
const MAX_TAG_CHARS: usize = 128;
const MAX_SHARE_FILENAME_BYTES: usize = 240;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderShareEnvelopeV1 {
    #[serde(rename = "type")]
    kind: String,
    schema_version: u32,
    pub(crate) provider: ProviderShareProviderV1,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderShareProviderV1 {
    pub(crate) cli_key: String,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    configuration: ProviderShareConfigurationV1,
    authentication: ProviderShareAuthenticationV1,
    extensions: Vec<ProviderShareExtensionV1>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderShareConfigurationV1 {
    base_urls: Vec<String>,
    base_url_mode: ProviderBaseUrlMode,
    priority: i64,
    cost_multiplier: f64,
    claude_models: ProviderShareClaudeModelsV1,
    model_mapping: ProviderShareModelMappingV1,
    availability_test_model: Option<String>,
    limits: ProviderShareLimitsV1,
    tags: Vec<String>,
    note: String,
    bridge_type: Option<String>,
    stream_idle_timeout_seconds: Option<u32>,
    upstream_retry_policy_override: Option<ProviderShareRetryPolicyV1>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProviderShareClaudeModelsV1 {
    main_model: Option<String>,
    reasoning_model: Option<String>,
    haiku_model: Option<String>,
    sonnet_model: Option<String>,
    opus_model: Option<String>,
}

impl From<ClaudeModels> for ProviderShareClaudeModelsV1 {
    fn from(value: ClaudeModels) -> Self {
        Self {
            main_model: value.main_model,
            reasoning_model: value.reasoning_model,
            haiku_model: value.haiku_model,
            sonnet_model: value.sonnet_model,
            opus_model: value.opus_model,
        }
    }
}

impl From<ProviderShareClaudeModelsV1> for ClaudeModels {
    fn from(value: ProviderShareClaudeModelsV1) -> Self {
        Self {
            main_model: value.main_model,
            reasoning_model: value.reasoning_model,
            haiku_model: value.haiku_model,
            sonnet_model: value.sonnet_model,
            opus_model: value.opus_model,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProviderShareModelMappingV1 {
    default_model: Option<String>,
    exact: BTreeMap<String, String>,
}

impl From<ModelMapping> for ProviderShareModelMappingV1 {
    fn from(value: ModelMapping) -> Self {
        Self {
            default_model: value.default_model,
            exact: value.exact,
        }
    }
}

impl From<ProviderShareModelMappingV1> for ModelMapping {
    fn from(value: ProviderShareModelMappingV1) -> Self {
        Self {
            default_model: value.default_model,
            exact: value.exact,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderShareLimitsV1 {
    limit_5h_usd: Option<f64>,
    limit_daily_usd: Option<f64>,
    daily_reset_mode: DailyResetMode,
    daily_reset_time: String,
    limit_weekly_usd: Option<f64>,
    limit_monthly_usd: Option<f64>,
    limit_total_usd: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderShareRetryPolicyV1 {
    enabled: bool,
    status_codes: Vec<u16>,
    transport_errors: Vec<crate::settings::UpstreamTransportRetryKind>,
    max_retries: u32,
    backoff_ms: u32,
    counts_toward_circuit_breaker: bool,
}

impl From<crate::settings::UpstreamRetryPolicy> for ProviderShareRetryPolicyV1 {
    fn from(value: crate::settings::UpstreamRetryPolicy) -> Self {
        Self {
            enabled: value.enabled,
            status_codes: value.status_codes,
            transport_errors: value.transport_errors,
            max_retries: value.max_retries,
            backoff_ms: value.backoff_ms,
            counts_toward_circuit_breaker: value.counts_toward_circuit_breaker,
        }
    }
}

impl From<ProviderShareRetryPolicyV1> for crate::settings::UpstreamRetryPolicy {
    fn from(value: ProviderShareRetryPolicyV1) -> Self {
        Self {
            enabled: value.enabled,
            status_codes: value.status_codes,
            transport_errors: value.transport_errors,
            max_retries: value.max_retries,
            backoff_ms: value.backoff_ms,
            counts_toward_circuit_breaker: value.counts_toward_circuit_breaker,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum ProviderShareAuthenticationV1 {
    ApiKey {
        api_key: String,
    },
    Oauth {
        provider_type: Option<String>,
        access_token: Option<String>,
        refresh_token: Option<String>,
        id_token: Option<String>,
        token_uri: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        expires_at: Option<i64>,
        email: Option<String>,
        refresh_lead_seconds: i64,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderShareExtensionV1 {
    plugin_id: String,
    plugin_version: String,
    namespace: String,
    values: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderShareCredentialStatus {
    Configured,
    NeedsApiKey,
    NotRequired,
    Available,
    Refreshable,
    NeedsLogin,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderShareExtensionCompatibility {
    Compatible,
    MissingPlugin,
    PluginUnavailable,
    VersionMismatch,
    NamespaceMismatch,
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderShareExtensionPreview {
    pub(crate) plugin_id: String,
    pub(crate) namespace: String,
    pub(crate) required_version: String,
    pub(crate) installed_version: Option<String>,
    pub(crate) compatibility: ProviderShareExtensionCompatibility,
}

pub(crate) struct ProviderSharePreviewDraft {
    pub(crate) cli_key: String,
    pub(crate) source_name: String,
    pub(crate) final_name: String,
    pub(crate) source_enabled: bool,
    pub(crate) auth_mode: ProviderAuthMode,
    pub(crate) credential_status: ProviderShareCredentialStatus,
    pub(crate) extensions: Vec<ProviderShareExtensionPreview>,
    pub(crate) can_import: bool,
}

struct ProviderShareDbRow {
    cli_key: String,
    name: String,
    base_url: String,
    base_urls_json: String,
    base_url_mode: String,
    api_key_plaintext: String,
    auth_mode: String,
    oauth_provider_type: Option<String>,
    oauth_access_token: Option<String>,
    oauth_refresh_token: Option<String>,
    oauth_id_token: Option<String>,
    oauth_token_uri: Option<String>,
    oauth_client_id: Option<String>,
    oauth_client_secret: Option<String>,
    oauth_expires_at: Option<i64>,
    oauth_email: Option<String>,
    oauth_refresh_lead_s: i64,
    claude_models_json: String,
    model_mapping_json: String,
    availability_test_model: Option<String>,
    enabled: bool,
    priority: i64,
    cost_multiplier: f64,
    limit_5h_usd: Option<f64>,
    limit_daily_usd: Option<f64>,
    daily_reset_mode: String,
    daily_reset_time: String,
    limit_weekly_usd: Option<f64>,
    limit_monthly_usd: Option<f64>,
    limit_total_usd: Option<f64>,
    tags_json: String,
    note: String,
    source_provider_id: Option<i64>,
    bridge_type: Option<String>,
    stream_idle_timeout_seconds: Option<i64>,
    upstream_retry_policy_json: Option<String>,
}

#[derive(Deserialize)]
struct ProviderShareHeader {
    #[serde(rename = "type")]
    kind: String,
    schema_version: u32,
}

struct CappedJsonWriter {
    bytes: Vec<u8>,
}

impl CappedJsonWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn finish(mut self) -> AppResult<Vec<u8>> {
        if self.bytes.len() >= PROVIDER_SHARE_MAX_BYTES {
            return Err(provider_share_too_large());
        }
        self.bytes.push(b'\n');
        Ok(self.bytes)
    }
}

impl std::io::Write for CappedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let next_len = self
            .bytes
            .len()
            .checked_add(buffer.len())
            .ok_or_else(|| std::io::Error::other("provider share size overflow"))?;
        if next_len >= PROVIDER_SHARE_MAX_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider share exceeds size limit",
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn provider_share_too_large() -> AppError {
    AppError::new(
        "SEC_INVALID_INPUT",
        format!("provider share exceeds maximum encoded size of {PROVIDER_SHARE_MAX_BYTES} bytes"),
    )
}

fn provider_share_schema_error() -> AppError {
    AppError::new(
        "SEC_INVALID_INPUT",
        "provider share JSON does not match schema version 1",
    )
}

fn validate_secret_bytes(field: &str, value: &str) -> AppResult<()> {
    if value.len() > MAX_AUTH_SECRET_BYTES {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            format!("provider share {field} is too large"),
        ));
    }
    Ok(())
}

fn validate_optional_secret(field: &str, value: Option<&str>) -> AppResult<()> {
    if let Some(value) = value {
        validate_secret_bytes(field, value)?;
    }
    Ok(())
}

fn validate_optional_auth_text(field: &str, value: Option<&str>) -> AppResult<()> {
    if let Some(value) = value {
        validate_max_chars(field, value, MAX_AUTH_TEXT_CHARS)?;
    }
    Ok(())
}

fn validate_oauth_token_uri(value: Option<&str>) -> AppResult<()> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| AppError::new("SEC_INVALID_INPUT", "provider share token_uri is invalid"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share token_uri is invalid",
        ));
    }
    Ok(())
}

fn normalize_bridge_configuration(provider: &mut ProviderShareProviderV1) -> AppResult<()> {
    let bridge_type = provider
        .configuration
        .bridge_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match bridge_type {
        None => provider.configuration.bridge_type = None,
        Some(CX2CC_BRIDGE_TYPE) if provider.cli_key == "claude" => {
            provider.configuration.bridge_type = Some(CX2CC_BRIDGE_TYPE.to_string());
        }
        Some(_) => {
            return Err(AppError::new(
                "SEC_INVALID_INPUT",
                "provider share depends on an external source provider",
            ));
        }
    }
    Ok(())
}

fn normalize_provider_share_v1(
    mut envelope: ProviderShareEnvelopeV1,
) -> AppResult<ProviderShareEnvelopeV1> {
    if envelope.kind != PROVIDER_SHARE_KIND
        || envelope.schema_version != PROVIDER_SHARE_SCHEMA_VERSION
    {
        return Err(provider_share_schema_error());
    }

    let provider = &mut envelope.provider;
    if provider.cli_key.trim() != provider.cli_key {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share cli_key is invalid",
        ));
    }
    validate_cli_key(&provider.cli_key)
        .map_err(|_| AppError::new("SEC_INVALID_INPUT", "provider share cli_key is invalid"))?;

    provider.name = provider.name.trim().to_string();
    if provider.name.is_empty() {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share name is required",
        ));
    }
    validate_max_chars(
        "provider share name",
        &provider.name,
        MAX_PROVIDER_NAME_CHARS,
    )?;
    normalize_bridge_configuration(provider)?;

    let is_oauth = matches!(
        provider.authentication,
        ProviderShareAuthenticationV1::Oauth { .. }
    );
    let is_bridge = provider.configuration.bridge_type.is_some();
    provider.configuration.base_urls = if is_oauth || is_bridge {
        Vec::new()
    } else {
        normalize_base_urls(std::mem::take(&mut provider.configuration.base_urls)).map_err(
            |_| {
                AppError::new(
                    "SEC_INVALID_INPUT",
                    "provider share base_urls configuration is invalid",
                )
            },
        )?
    };

    if !provider.configuration.cost_multiplier.is_finite()
        || !(0.0..=1000.0).contains(&provider.configuration.cost_multiplier)
    {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share cost_multiplier is invalid",
        ));
    }
    if !(0..=1000).contains(&provider.configuration.priority) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share priority is invalid",
        ));
    }

    let mut claude_models: ClaudeModels = provider.configuration.claude_models.clone().into();
    validate_claude_models(&claude_models)?;
    claude_models = claude_models.normalized();
    if provider.cli_key != "claude" && claude_models.has_any() {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share claude_models is invalid for this CLI",
        ));
    }
    if provider.cli_key != "claude" {
        claude_models = ClaudeModels::default();
    }
    provider.configuration.claude_models = claude_models.into();

    let mut model_mapping: ModelMapping = provider.configuration.model_mapping.clone().into();
    model_mapping = model_mapping.normalized();
    if provider.cli_key != "codex"
        && (model_mapping.default_model.is_some() || !model_mapping.exact.is_empty())
    {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share model_mapping is invalid for this CLI",
        ));
    }
    if provider.cli_key != "codex" {
        model_mapping = ModelMapping::default();
    }
    provider.configuration.model_mapping = model_mapping.into();

    provider.configuration.availability_test_model =
        super::types::normalize_model_slot(provider.configuration.availability_test_model.take());
    if provider.cli_key != "codex" && provider.configuration.availability_test_model.is_some() {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share availability_test_model is invalid for this CLI",
        ));
    }

    let limits = &mut provider.configuration.limits;
    limits.limit_5h_usd = validate_limit_usd("limit_5h_usd", limits.limit_5h_usd)?;
    limits.limit_daily_usd = validate_limit_usd("limit_daily_usd", limits.limit_daily_usd)?;
    limits.limit_weekly_usd = validate_limit_usd("limit_weekly_usd", limits.limit_weekly_usd)?;
    limits.limit_monthly_usd = validate_limit_usd("limit_monthly_usd", limits.limit_monthly_usd)?;
    limits.limit_total_usd = validate_limit_usd("limit_total_usd", limits.limit_total_usd)?;
    limits.daily_reset_time =
        normalize_reset_time_hms_strict("daily_reset_time", &limits.daily_reset_time)?;

    if provider.configuration.tags.len() > MAX_TAG_COUNT {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share contains too many tags",
        ));
    }
    for tag in &provider.configuration.tags {
        validate_max_chars("provider share tag", tag, MAX_TAG_CHARS)?;
    }
    provider.configuration.tags = normalize_tags(std::mem::take(&mut provider.configuration.tags));
    provider.configuration.note = normalize_note(Some(&provider.configuration.note))?;
    provider.configuration.stream_idle_timeout_seconds =
        normalize_stream_idle_timeout_seconds(provider.configuration.stream_idle_timeout_seconds)?;

    if let Some(policy) = provider.configuration.upstream_retry_policy_override.take() {
        let mut policy: crate::settings::UpstreamRetryPolicy = policy.into();
        crate::settings::sanitize_upstream_retry_policy(&mut policy);
        provider.configuration.upstream_retry_policy_override = Some(policy.into());
    }

    match &mut provider.authentication {
        ProviderShareAuthenticationV1::ApiKey { api_key } => {
            validate_secret_bytes("api_key", api_key)?;
            if api_key.trim().is_empty() {
                api_key.clear();
            }
        }
        ProviderShareAuthenticationV1::Oauth {
            provider_type,
            access_token,
            refresh_token,
            id_token,
            token_uri,
            client_id,
            client_secret,
            email,
            refresh_lead_seconds,
            ..
        } => {
            if is_bridge {
                return Err(AppError::new(
                    "SEC_INVALID_INPUT",
                    "provider share bridge authentication is invalid",
                ));
            }
            validate_optional_auth_text("oauth provider_type", provider_type.as_deref())?;
            crate::gateway::oauth::registry::resolve_oauth_adapter(
                &provider.cli_key,
                0,
                provider_type.as_deref(),
            )
            .map_err(|_| {
                AppError::new(
                    "SEC_INVALID_INPUT",
                    "provider share oauth provider_type is invalid for this CLI",
                )
            })?;
            validate_optional_secret("oauth access_token", access_token.as_deref())?;
            validate_optional_secret("oauth refresh_token", refresh_token.as_deref())?;
            validate_optional_secret("oauth id_token", id_token.as_deref())?;
            validate_optional_auth_text("oauth token_uri", token_uri.as_deref())?;
            validate_oauth_token_uri(token_uri.as_deref())?;
            validate_optional_auth_text("oauth client_id", client_id.as_deref())?;
            validate_optional_secret("oauth client_secret", client_secret.as_deref())?;
            validate_optional_auth_text("oauth email", email.as_deref())?;
            if *refresh_lead_seconds <= 0 {
                return Err(AppError::new(
                    "SEC_INVALID_INPUT",
                    "provider share oauth refresh_lead_seconds is invalid",
                ));
            }
        }
    }

    if provider.extensions.len() > MAX_EXTENSION_COUNT {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share contains too many extension values",
        ));
    }
    let mut seen_extensions = HashSet::new();
    for extension in &mut provider.extensions {
        extension.plugin_id = extension.plugin_id.trim().to_string();
        extension.plugin_version = extension.plugin_version.trim().to_string();
        extension.namespace = extension.namespace.trim().to_string();
        if extension.plugin_id.is_empty()
            || extension.plugin_version.is_empty()
            || extension.namespace.is_empty()
        {
            return Err(AppError::new(
                "SEC_INVALID_INPUT",
                "provider share extension identity is invalid",
            ));
        }
        validate_max_chars(
            "provider share extension plugin_id",
            &extension.plugin_id,
            MAX_EXTENSION_ID_CHARS,
        )?;
        validate_max_chars(
            "provider share extension plugin_version",
            &extension.plugin_version,
            MAX_EXTENSION_ID_CHARS,
        )?;
        validate_max_chars(
            "provider share extension namespace",
            &extension.namespace,
            MAX_EXTENSION_ID_CHARS,
        )?;
        if !seen_extensions.insert((extension.plugin_id.clone(), extension.namespace.clone())) {
            return Err(AppError::new(
                "SEC_INVALID_INPUT",
                "provider share contains duplicate extension values",
            ));
        }
        if extension.plugin_id == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
            && extension.namespace == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
        {
            extension.values =
                crate::domain::provider_account_usage::sanitize_account_usage_extension_value(
                    &extension.values,
                );
        }
    }
    provider.extensions.sort_by(|left, right| {
        left.plugin_id
            .cmp(&right.plugin_id)
            .then_with(|| left.namespace.cmp(&right.namespace))
    });

    Ok(envelope)
}

pub(crate) fn parse_provider_share_v1(bytes: &[u8]) -> AppResult<ProviderShareEnvelopeV1> {
    if bytes.is_empty() {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share content is empty",
        ));
    }
    if bytes.len() > PROVIDER_SHARE_MAX_BYTES {
        return Err(provider_share_too_large());
    }
    std::str::from_utf8(bytes).map_err(|_| {
        AppError::new(
            "SEC_INVALID_INPUT",
            "provider share content must be valid UTF-8",
        )
    })?;

    let header: ProviderShareHeader =
        serde_json::from_slice(bytes).map_err(|_| provider_share_schema_error())?;
    if header.kind != PROVIDER_SHARE_KIND {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "file is not an AIO Coding Hub provider share",
        ));
    }
    if header.schema_version != PROVIDER_SHARE_SCHEMA_VERSION {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider share schema version is not supported; update AIO Coding Hub",
        ));
    }

    let envelope: ProviderShareEnvelopeV1 =
        serde_json::from_slice(bytes).map_err(|_| provider_share_schema_error())?;
    normalize_provider_share_v1(envelope)
}

pub(crate) fn serialize_provider_share_v1(
    envelope: &ProviderShareEnvelopeV1,
) -> AppResult<Vec<u8>> {
    let envelope = normalize_provider_share_v1(envelope.clone())?;
    let mut writer = CappedJsonWriter::new();
    serde_json::to_writer_pretty(&mut writer, &envelope).map_err(|error| {
        if error.is_io() {
            provider_share_too_large()
        } else {
            AppError::new("SYSTEM_ERROR", "failed to serialize provider share")
        }
    })?;
    writer
        .flush()
        .map_err(|_| AppError::new("SYSTEM_ERROR", "failed to serialize provider share"))?;
    writer.finish()
}

pub(crate) fn provider_share_default_filename(cli_key: &str, provider_name: &str) -> String {
    const PREFIX: &str = "aio-coding-hub-provider-";
    const EXTENSION: &str = ".json";
    let mut safe_name = provider_name
        .trim()
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .take(80)
        .collect::<String>();
    while safe_name.ends_with(['.', ' ']) {
        safe_name.pop();
    }
    let fixed_bytes = PREFIX
        .len()
        .saturating_add(cli_key.len())
        .saturating_add(1)
        .saturating_add(EXTENSION.len());
    let name_byte_budget = MAX_SHARE_FILENAME_BYTES.saturating_sub(fixed_bytes);
    if safe_name.len() > name_byte_budget {
        let mut truncated = String::new();
        for character in safe_name.chars() {
            if truncated.len().saturating_add(character.len_utf8()) > name_byte_budget {
                break;
            }
            truncated.push(character);
        }
        safe_name = truncated;
        while safe_name.ends_with(['.', ' ']) {
            safe_name.pop();
        }
    }
    if safe_name.is_empty() {
        safe_name.push_str("provider");
    }
    format!("{PREFIX}{cli_key}-{safe_name}{EXTENSION}")
}

fn query_provider_share_row(conn: &Connection, provider_id: i64) -> AppResult<ProviderShareDbRow> {
    conn.query_row(
        r#"
SELECT
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  api_key_plaintext,
  auth_mode,
  oauth_provider_type,
  oauth_access_token,
  oauth_refresh_token,
  oauth_id_token,
  oauth_token_uri,
  oauth_client_id,
  oauth_client_secret,
  oauth_expires_at,
  oauth_email,
  oauth_refresh_lead_s,
  claude_models_json,
  model_mapping_json,
  availability_test_model,
  enabled,
  priority,
  cost_multiplier,
  limit_5h_usd,
  limit_daily_usd,
  daily_reset_mode,
  daily_reset_time,
  limit_weekly_usd,
  limit_monthly_usd,
  limit_total_usd,
  tags_json,
  note,
  source_provider_id,
  bridge_type,
  stream_idle_timeout_seconds,
  upstream_retry_policy_json
FROM providers
WHERE id = ?1
"#,
        params![provider_id],
        |row| {
            Ok(ProviderShareDbRow {
                cli_key: row.get("cli_key")?,
                name: row.get("name")?,
                base_url: row.get("base_url")?,
                base_urls_json: row.get("base_urls_json")?,
                base_url_mode: row.get("base_url_mode")?,
                api_key_plaintext: row.get("api_key_plaintext")?,
                auth_mode: row
                    .get::<_, Option<String>>("auth_mode")?
                    .unwrap_or_else(|| "api_key".to_string()),
                oauth_provider_type: row.get("oauth_provider_type")?,
                oauth_access_token: row.get("oauth_access_token")?,
                oauth_refresh_token: row.get("oauth_refresh_token")?,
                oauth_id_token: row.get("oauth_id_token")?,
                oauth_token_uri: row.get("oauth_token_uri")?,
                oauth_client_id: row.get("oauth_client_id")?,
                oauth_client_secret: row.get("oauth_client_secret")?,
                oauth_expires_at: row.get("oauth_expires_at")?,
                oauth_email: row.get("oauth_email")?,
                oauth_refresh_lead_s: row.get("oauth_refresh_lead_s")?,
                claude_models_json: row.get("claude_models_json")?,
                model_mapping_json: row.get("model_mapping_json")?,
                availability_test_model: row.get("availability_test_model")?,
                enabled: row.get::<_, i64>("enabled")? != 0,
                priority: row.get("priority")?,
                cost_multiplier: row.get("cost_multiplier")?,
                limit_5h_usd: row.get("limit_5h_usd")?,
                limit_daily_usd: row.get("limit_daily_usd")?,
                daily_reset_mode: row.get("daily_reset_mode")?,
                daily_reset_time: row.get("daily_reset_time")?,
                limit_weekly_usd: row.get("limit_weekly_usd")?,
                limit_monthly_usd: row.get("limit_monthly_usd")?,
                limit_total_usd: row.get("limit_total_usd")?,
                tags_json: row.get("tags_json")?,
                note: row.get("note")?,
                source_provider_id: row.get("source_provider_id")?,
                bridge_type: row.get("bridge_type")?,
                stream_idle_timeout_seconds: row.get("stream_idle_timeout_seconds")?,
                upstream_retry_policy_json: row.get("upstream_retry_policy_json")?,
            })
        },
    )
    .optional()
    .map_err(|error| db_err!("failed to query provider share row: {error}"))?
    .ok_or_else(|| AppError::new("DB_NOT_FOUND", "provider not found"))
}

fn query_provider_share_extensions(
    conn: &Connection,
    provider_id: i64,
) -> AppResult<Vec<ProviderShareExtensionV1>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT
  extension_values.plugin_id,
  plugins.current_version,
  extension_values.namespace,
  extension_values.values_json
FROM provider_extension_values extension_values
LEFT JOIN plugins ON plugins.plugin_id = extension_values.plugin_id
WHERE extension_values.provider_id = ?1
ORDER BY extension_values.plugin_id ASC, extension_values.namespace ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare provider share extensions: {error}"))?;
    let rows = statement
        .query_map(params![provider_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| db_err!("failed to query provider share extensions: {error}"))?;

    let mut extensions = Vec::new();
    for row in rows {
        let (plugin_id, plugin_version, namespace, values_json) =
            row.map_err(|error| db_err!("failed to read provider share extension: {error}"))?;
        let plugin_version = plugin_version.ok_or_else(|| {
            AppError::new(
                "PROVIDER_SHARE_EXTENSION_UNAVAILABLE",
                format!("extension owner plugin {plugin_id} has no installed version"),
            )
        })?;
        let mut values = serde_json::from_str(&values_json).map_err(|_| {
            AppError::new(
                "DB_INVALID_DATA",
                format!("provider extension values are invalid for plugin {plugin_id}"),
            )
        })?;
        if plugin_id == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
            && namespace == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
        {
            values = crate::domain::provider_account_usage::sanitize_account_usage_extension_value(
                &values,
            );
        }
        extensions.push(ProviderShareExtensionV1 {
            plugin_id,
            plugin_version,
            namespace,
            values,
        });
    }
    Ok(extensions)
}

pub(crate) fn export_provider_share_v1(
    db: &db::Db,
    provider_id: i64,
) -> AppResult<ProviderShareEnvelopeV1> {
    if provider_id <= 0 {
        return Err(AppError::new("SEC_INVALID_INPUT", "provider_id is invalid"));
    }
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction()
        .map_err(|error| db_err!("failed to start provider share snapshot: {error}"))?;
    let row = query_provider_share_row(&tx, provider_id)?;
    if row.source_provider_id.is_some() {
        return Err(AppError::new(
            "PROVIDER_SHARE_REFERENCED_PROVIDER",
            "providers that reference another provider cannot be shared",
        ));
    }

    let base_url_mode = ProviderBaseUrlMode::parse(&row.base_url_mode)
        .ok_or_else(|| AppError::new("DB_INVALID_DATA", "provider base_url_mode is invalid"))?;
    let daily_reset_mode = DailyResetMode::parse(&row.daily_reset_mode)
        .ok_or_else(|| AppError::new("DB_INVALID_DATA", "provider daily_reset_mode is invalid"))?;
    let authentication = match row.auth_mode.as_str() {
        "api_key" => ProviderShareAuthenticationV1::ApiKey {
            api_key: row.api_key_plaintext,
        },
        "oauth" => ProviderShareAuthenticationV1::Oauth {
            provider_type: row.oauth_provider_type,
            access_token: row.oauth_access_token,
            refresh_token: row.oauth_refresh_token,
            id_token: row.oauth_id_token,
            token_uri: row.oauth_token_uri,
            client_id: row.oauth_client_id,
            client_secret: row.oauth_client_secret,
            expires_at: row.oauth_expires_at,
            email: row.oauth_email,
            refresh_lead_seconds: if row.oauth_refresh_lead_s > 0 {
                row.oauth_refresh_lead_s
            } else {
                DEFAULT_OAUTH_REFRESH_LEAD_SECONDS
            },
        },
        _ => {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "provider auth_mode is invalid",
            ));
        }
    };
    let upstream_retry_policy_override =
        retry_policy_override_from_json(row.upstream_retry_policy_json).map(Into::into);
    let extensions = query_provider_share_extensions(&tx, provider_id)?;
    tx.commit()
        .map_err(|error| db_err!("failed to finish provider share snapshot: {error}"))?;

    normalize_provider_share_v1(ProviderShareEnvelopeV1 {
        kind: PROVIDER_SHARE_KIND.to_string(),
        schema_version: PROVIDER_SHARE_SCHEMA_VERSION,
        provider: ProviderShareProviderV1 {
            cli_key: row.cli_key.clone(),
            name: row.name,
            enabled: row.enabled,
            configuration: ProviderShareConfigurationV1 {
                base_urls: base_urls_from_row(&row.base_url, &row.base_urls_json),
                base_url_mode,
                priority: row.priority,
                cost_multiplier: row.cost_multiplier,
                claude_models: claude_models_from_json(&row.claude_models_json).into(),
                model_mapping: model_mapping_from_json(&row.model_mapping_json).into(),
                availability_test_model: row.availability_test_model,
                limits: ProviderShareLimitsV1 {
                    limit_5h_usd: row.limit_5h_usd,
                    limit_daily_usd: row.limit_daily_usd,
                    daily_reset_mode,
                    daily_reset_time: row.daily_reset_time,
                    limit_weekly_usd: row.limit_weekly_usd,
                    limit_monthly_usd: row.limit_monthly_usd,
                    limit_total_usd: row.limit_total_usd,
                },
                tags: super::types::tags_from_json(&row.tags_json),
                note: row.note,
                bridge_type: row.bridge_type,
                stream_idle_timeout_seconds: super::validation::parse_positive_optional_u32(
                    row.stream_idle_timeout_seconds,
                ),
                upstream_retry_policy_override,
            },
            authentication,
            extensions,
        },
    })
}

fn normalized_provider_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn provider_name_with_suffix(source_name: &str, suffix: &str) -> String {
    let suffix_chars = suffix.chars().count();
    let prefix_chars = MAX_PROVIDER_NAME_CHARS.saturating_sub(suffix_chars);
    let mut prefix = source_name.chars().take(prefix_chars).collect::<String>();
    while prefix.ends_with(char::is_whitespace) {
        prefix.pop();
    }
    format!("{prefix}{suffix}")
}

fn available_provider_name(
    conn: &Connection,
    cli_key: &str,
    source_name: &str,
) -> AppResult<String> {
    let mut statement = conn
        .prepare_cached("SELECT name FROM providers WHERE cli_key = ?1")
        .map_err(|error| db_err!("failed to prepare provider name query: {error}"))?;
    let rows = statement
        .query_map(params![cli_key], |row| row.get::<_, String>(0))
        .map_err(|error| db_err!("failed to query provider names: {error}"))?;
    let mut used = HashSet::new();
    for row in rows {
        used.insert(normalized_provider_name(&row.map_err(|error| {
            db_err!("failed to read provider name: {error}")
        })?));
    }

    let source_name = source_name.trim();
    if !used.contains(&normalized_provider_name(source_name)) {
        return Ok(source_name.to_string());
    }
    let base_name = provider_name_with_suffix(source_name, " 副本");
    if !used.contains(&normalized_provider_name(&base_name)) {
        return Ok(base_name);
    }
    for index in 2_u64.. {
        let candidate = provider_name_with_suffix(source_name, &format!(" 副本 {index}"));
        if !used.contains(&normalized_provider_name(&candidate)) {
            return Ok(candidate);
        }
    }
    Err(AppError::new(
        "SYSTEM_ERROR",
        "failed to allocate provider import name",
    ))
}

fn target_cli_matches(target: &TargetCliKey, cli_key: &str) -> bool {
    matches!(
        (target, cli_key),
        (TargetCliKey::Claude, "claude")
            | (TargetCliKey::Codex, "codex")
            | (TargetCliKey::Gemini, "gemini")
    )
}

fn manifest_owns_extension_namespace(
    manifest: &PluginManifest,
    plugin_id: &str,
    plugin_version: &str,
    cli_key: &str,
    namespace: &str,
) -> bool {
    if manifest.id != plugin_id || manifest.version != plugin_version {
        return false;
    }
    if !manifest
        .capabilities
        .iter()
        .any(|capability| capability == "provider.extensionValues")
    {
        return false;
    }
    let Some(contributes) = manifest.contributes.as_ref() else {
        return false;
    };
    if contributes.providers.is_empty() {
        return false;
    }
    contributes.providers.iter().any(|provider| {
        provider.extension_namespace == namespace
            && provider
                .target_cli_keys
                .iter()
                .any(|target| target_cli_matches(target, cli_key))
    })
}

fn extension_preview(
    conn: &Connection,
    cli_key: &str,
    extension: &ProviderShareExtensionV1,
) -> AppResult<ProviderShareExtensionPreview> {
    if extension.plugin_id == crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID {
        let compatibility = if extension.namespace
            == crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE
        {
            ProviderShareExtensionCompatibility::Compatible
        } else {
            ProviderShareExtensionCompatibility::NamespaceMismatch
        };
        return Ok(ProviderShareExtensionPreview {
            plugin_id: extension.plugin_id.clone(),
            namespace: extension.namespace.clone(),
            required_version: extension.plugin_version.clone(),
            installed_version: Some(extension.plugin_version.clone()),
            compatibility,
        });
    }

    let plugin = conn
        .query_row(
            "SELECT current_version, status, manifest_json FROM plugins WHERE plugin_id = ?1",
            params![extension.plugin_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| db_err!("failed to query extension owner plugin: {error}"))?;

    let Some((installed_version, status_raw, manifest_json)) = plugin else {
        return Ok(ProviderShareExtensionPreview {
            plugin_id: extension.plugin_id.clone(),
            namespace: extension.namespace.clone(),
            required_version: extension.plugin_version.clone(),
            installed_version: None,
            compatibility: ProviderShareExtensionCompatibility::MissingPlugin,
        });
    };

    let status = PluginStatus::parse(&status_raw);
    let available = matches!(
        status,
        Some(
            PluginStatus::Installed
                | PluginStatus::Enabled
                | PluginStatus::Disabled
                | PluginStatus::UpdateAvailable
        )
    );
    let compatibility = if !available {
        ProviderShareExtensionCompatibility::PluginUnavailable
    } else if installed_version.as_deref() != Some(extension.plugin_version.as_str()) {
        ProviderShareExtensionCompatibility::VersionMismatch
    } else {
        match serde_json::from_str::<PluginManifest>(&manifest_json) {
            Ok(manifest) if manifest.id != extension.plugin_id => {
                ProviderShareExtensionCompatibility::NamespaceMismatch
            }
            Ok(manifest) if manifest.version != extension.plugin_version => {
                ProviderShareExtensionCompatibility::VersionMismatch
            }
            Ok(manifest)
                if manifest_owns_extension_namespace(
                    &manifest,
                    &extension.plugin_id,
                    &extension.plugin_version,
                    cli_key,
                    &extension.namespace,
                ) =>
            {
                ProviderShareExtensionCompatibility::Compatible
            }
            _ => ProviderShareExtensionCompatibility::NamespaceMismatch,
        }
    };

    Ok(ProviderShareExtensionPreview {
        plugin_id: extension.plugin_id.clone(),
        namespace: extension.namespace.clone(),
        required_version: extension.plugin_version.clone(),
        installed_version,
        compatibility,
    })
}

fn credential_status(provider: &ProviderShareProviderV1) -> ProviderShareCredentialStatus {
    if provider.configuration.bridge_type.as_deref() == Some(CX2CC_BRIDGE_TYPE) {
        return ProviderShareCredentialStatus::NotRequired;
    }
    match &provider.authentication {
        ProviderShareAuthenticationV1::ApiKey { api_key } => {
            if api_key.trim().is_empty() {
                ProviderShareCredentialStatus::NeedsApiKey
            } else {
                ProviderShareCredentialStatus::Configured
            }
        }
        ProviderShareAuthenticationV1::Oauth {
            access_token,
            refresh_token,
            token_uri,
            client_id,
            expires_at,
            ..
        } => {
            let access_available = access_token
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                && expires_at.is_none_or(|expires_at| expires_at > now_unix_seconds());
            if access_available {
                ProviderShareCredentialStatus::Available
            } else if refresh_token
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
                && token_uri
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
                && client_id
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
            {
                ProviderShareCredentialStatus::Refreshable
            } else {
                ProviderShareCredentialStatus::NeedsLogin
            }
        }
    }
}

pub(crate) fn preview_provider_share_v1(
    db: &db::Db,
    envelope: &ProviderShareEnvelopeV1,
) -> AppResult<ProviderSharePreviewDraft> {
    let envelope = normalize_provider_share_v1(envelope.clone())?;
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction()
        .map_err(|error| db_err!("failed to start provider share preview: {error}"))?;
    let final_name =
        available_provider_name(&tx, &envelope.provider.cli_key, &envelope.provider.name)?;
    let mut extensions = Vec::with_capacity(envelope.provider.extensions.len());
    for extension in &envelope.provider.extensions {
        extensions.push(extension_preview(
            &tx,
            &envelope.provider.cli_key,
            extension,
        )?);
    }
    tx.commit()
        .map_err(|error| db_err!("failed to finish provider share preview: {error}"))?;
    let can_import = extensions.iter().all(|extension| {
        extension.compatibility == ProviderShareExtensionCompatibility::Compatible
    });
    let auth_mode = match envelope.provider.authentication {
        ProviderShareAuthenticationV1::ApiKey { .. } => ProviderAuthMode::ApiKey,
        ProviderShareAuthenticationV1::Oauth { .. } => ProviderAuthMode::Oauth,
    };

    Ok(ProviderSharePreviewDraft {
        cli_key: envelope.provider.cli_key.clone(),
        source_name: envelope.provider.name.clone(),
        final_name,
        source_enabled: envelope.provider.enabled,
        auth_mode,
        credential_status: credential_status(&envelope.provider),
        extensions,
        can_import,
    })
}

struct AuthenticationDbFields<'a> {
    auth_mode: &'static str,
    api_key: &'a str,
    oauth_provider_type: Option<&'a str>,
    oauth_access_token: Option<&'a str>,
    oauth_refresh_token: Option<&'a str>,
    oauth_id_token: Option<&'a str>,
    oauth_token_uri: Option<&'a str>,
    oauth_client_id: Option<&'a str>,
    oauth_client_secret: Option<&'a str>,
    oauth_expires_at: Option<i64>,
    oauth_email: Option<&'a str>,
    oauth_refresh_lead_s: i64,
}

fn authentication_db_fields(
    authentication: &ProviderShareAuthenticationV1,
) -> AuthenticationDbFields<'_> {
    match authentication {
        ProviderShareAuthenticationV1::ApiKey { api_key } => AuthenticationDbFields {
            auth_mode: "api_key",
            api_key,
            oauth_provider_type: None,
            oauth_access_token: None,
            oauth_refresh_token: None,
            oauth_id_token: None,
            oauth_token_uri: None,
            oauth_client_id: None,
            oauth_client_secret: None,
            oauth_expires_at: None,
            oauth_email: None,
            oauth_refresh_lead_s: DEFAULT_OAUTH_REFRESH_LEAD_SECONDS,
        },
        ProviderShareAuthenticationV1::Oauth {
            provider_type,
            access_token,
            refresh_token,
            id_token,
            token_uri,
            client_id,
            client_secret,
            expires_at,
            email,
            refresh_lead_seconds,
        } => AuthenticationDbFields {
            auth_mode: "oauth",
            api_key: "",
            oauth_provider_type: provider_type.as_deref(),
            oauth_access_token: access_token.as_deref(),
            oauth_refresh_token: refresh_token.as_deref(),
            oauth_id_token: id_token.as_deref(),
            oauth_token_uri: token_uri.as_deref(),
            oauth_client_id: client_id.as_deref(),
            oauth_client_secret: client_secret.as_deref(),
            oauth_expires_at: *expires_at,
            oauth_email: email.as_deref(),
            oauth_refresh_lead_s: *refresh_lead_seconds,
        },
    }
}

pub(crate) fn import_provider_share_v1(
    db: &db::Db,
    envelope: &ProviderShareEnvelopeV1,
    expected_final_name: &str,
    expected_extensions: &[ProviderShareExtensionPreview],
) -> AppResult<ProviderSummary> {
    let envelope = normalize_provider_share_v1(envelope.clone())?;
    let provider = &envelope.provider;
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction()
        .map_err(|error| db_err!("failed to start provider share import: {error}"))?;

    let final_name = available_provider_name(&tx, &provider.cli_key, &provider.name)?;
    if final_name != expected_final_name {
        return Err(AppError::new(
            "PROVIDER_SHARE_PREVIEW_STALE",
            "provider names changed after preview; preview the share again",
        ));
    }
    let mut current_extensions = Vec::with_capacity(provider.extensions.len());
    for extension in &provider.extensions {
        current_extensions.push(extension_preview(&tx, &provider.cli_key, extension)?);
    }
    if current_extensions != expected_extensions {
        return Err(AppError::new(
            "PROVIDER_SHARE_PREVIEW_STALE",
            "provider extension compatibility changed after preview; preview the share again",
        ));
    }
    if let Some(extension) = expected_extensions.iter().find(|extension| {
        extension.compatibility != ProviderShareExtensionCompatibility::Compatible
    }) {
        return Err(AppError::new(
            "PROVIDER_SHARE_EXTENSION_INCOMPATIBLE",
            format!(
                "provider extension is not compatible for plugin {}",
                extension.plugin_id
            ),
        ));
    }

    let configuration = &provider.configuration;
    let base_urls_json = serde_json::to_string(&configuration.base_urls)
        .map_err(|_| AppError::new("SYSTEM_ERROR", "failed to serialize provider base URLs"))?;
    let base_url = configuration
        .base_urls
        .first()
        .map(String::as_str)
        .unwrap_or("");
    let claude_models: ClaudeModels = configuration.claude_models.clone().into();
    let claude_models_json = serde_json::to_string(&claude_models)
        .map_err(|_| AppError::new("SYSTEM_ERROR", "failed to serialize Claude models"))?;
    let model_mapping: ModelMapping = configuration.model_mapping.clone().into();
    let model_mapping_json = serde_json::to_string(&model_mapping)
        .map_err(|_| AppError::new("SYSTEM_ERROR", "failed to serialize model mapping"))?;
    let tags_json = serde_json::to_string(&configuration.tags)
        .map_err(|_| AppError::new("SYSTEM_ERROR", "failed to serialize provider tags"))?;
    let retry_policy = configuration
        .upstream_retry_policy_override
        .clone()
        .map(Into::into);
    let retry_policy_json = retry_policy_override_to_json(retry_policy)?;
    let auth = authentication_db_fields(&provider.authentication);
    let sort_order = next_sort_order(&tx, &provider.cli_key)?;
    let now = now_unix_seconds();

    tx.execute(
        r#"
INSERT INTO providers(
  cli_key,
  name,
  base_url,
  base_urls_json,
  base_url_mode,
  auth_mode,
  claude_models_json,
  availability_test_model,
  supported_models_json,
  model_mapping_json,
  api_key_plaintext,
  enabled,
  priority,
  sort_order,
  cost_multiplier,
  limit_5h_usd,
  limit_daily_usd,
  daily_reset_mode,
  daily_reset_time,
  limit_weekly_usd,
  limit_monthly_usd,
  limit_total_usd,
  tags_json,
  note,
  oauth_provider_type,
  oauth_access_token,
  oauth_refresh_token,
  oauth_id_token,
  oauth_token_uri,
  oauth_client_id,
  oauth_client_secret,
  oauth_expires_at,
  oauth_email,
  oauth_refresh_lead_s,
  oauth_last_refreshed_at,
  oauth_last_error,
  source_provider_id,
  bridge_type,
  stream_idle_timeout_seconds,
  upstream_retry_policy_json,
  created_at,
  updated_at
) VALUES (
  :cli_key,
  :name,
  :base_url,
  :base_urls_json,
  :base_url_mode,
  :auth_mode,
  :claude_models_json,
  :availability_test_model,
  '{}',
  :model_mapping_json,
  :api_key,
  0,
  :priority,
  :sort_order,
  :cost_multiplier,
  :limit_5h_usd,
  :limit_daily_usd,
  :daily_reset_mode,
  :daily_reset_time,
  :limit_weekly_usd,
  :limit_monthly_usd,
  :limit_total_usd,
  :tags_json,
  :note,
  :oauth_provider_type,
  :oauth_access_token,
  :oauth_refresh_token,
  :oauth_id_token,
  :oauth_token_uri,
  :oauth_client_id,
  :oauth_client_secret,
  :oauth_expires_at,
  :oauth_email,
  :oauth_refresh_lead_s,
  NULL,
  NULL,
  NULL,
  :bridge_type,
  :stream_idle_timeout_seconds,
  :upstream_retry_policy_json,
  :now,
  :now
)
"#,
        named_params! {
            ":cli_key": provider.cli_key,
            ":name": final_name,
            ":base_url": base_url,
            ":base_urls_json": base_urls_json,
            ":base_url_mode": configuration.base_url_mode.as_str(),
            ":auth_mode": auth.auth_mode,
            ":claude_models_json": claude_models_json,
            ":availability_test_model": configuration.availability_test_model,
            ":model_mapping_json": model_mapping_json,
            ":api_key": auth.api_key,
            ":priority": configuration.priority,
            ":sort_order": sort_order,
            ":cost_multiplier": configuration.cost_multiplier,
            ":limit_5h_usd": configuration.limits.limit_5h_usd,
            ":limit_daily_usd": configuration.limits.limit_daily_usd,
            ":daily_reset_mode": configuration.limits.daily_reset_mode.as_str(),
            ":daily_reset_time": configuration.limits.daily_reset_time,
            ":limit_weekly_usd": configuration.limits.limit_weekly_usd,
            ":limit_monthly_usd": configuration.limits.limit_monthly_usd,
            ":limit_total_usd": configuration.limits.limit_total_usd,
            ":tags_json": tags_json,
            ":note": configuration.note,
            ":oauth_provider_type": auth.oauth_provider_type,
            ":oauth_access_token": auth.oauth_access_token,
            ":oauth_refresh_token": auth.oauth_refresh_token,
            ":oauth_id_token": auth.oauth_id_token,
            ":oauth_token_uri": auth.oauth_token_uri,
            ":oauth_client_id": auth.oauth_client_id,
            ":oauth_client_secret": auth.oauth_client_secret,
            ":oauth_expires_at": auth.oauth_expires_at,
            ":oauth_email": auth.oauth_email,
            ":oauth_refresh_lead_s": auth.oauth_refresh_lead_s,
            ":bridge_type": configuration.bridge_type,
            ":stream_idle_timeout_seconds": configuration.stream_idle_timeout_seconds,
            ":upstream_retry_policy_json": retry_policy_json,
            ":now": now,
        },
    )
    .map_err(|error| match error {
        rusqlite::Error::SqliteFailure(sqlite, _)
            if sqlite.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AppError::new(
                "PROVIDER_SHARE_PREVIEW_STALE",
                "provider names changed after preview; preview the share again",
            )
        }
        other => db_err!("failed to insert shared provider: {other}"),
    })?;
    let provider_id = tx.last_insert_rowid();
    let extension_values = provider
        .extensions
        .iter()
        .map(|extension| ProviderExtensionValuesInput {
            plugin_id: extension.plugin_id.clone(),
            namespace: extension.namespace.clone(),
            values: extension.values.clone(),
        })
        .collect::<Vec<_>>();
    crate::domain::provider_account_usage::ensure_account_usage_extension_owner_with_tx(
        &tx,
        Some(&extension_values),
    )?;
    replace_extension_values(&tx, provider_id, Some(&extension_values))?;
    let imported = super::queries::get_by_id(&tx, provider_id)?;
    tx.commit()
        .map_err(|error| db_err!("failed to commit provider share import: {error}"))?;
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_params(name: &str, cli_key: &str) -> super::super::types::ProviderUpsertParams {
        super::super::types::ProviderUpsertParams {
            provider_id: None,
            cli_key: cli_key.to_string(),
            name: name.to_string(),
            base_urls: vec!["https://api.example.invalid/v1".to_string()],
            base_url_mode: ProviderBaseUrlMode::Order,
            auth_mode: Some(ProviderAuthMode::ApiKey),
            api_key: Some("SYNTHETIC_API_KEY".to_string()),
            enabled: true,
            cost_multiplier: 1.0,
            priority: Some(100),
            claude_models: None,
            model_mapping: None,
            availability_test_model: None,
            limit_5h_usd: None,
            limit_daily_usd: None,
            daily_reset_mode: Some(DailyResetMode::Fixed),
            daily_reset_time: Some("00:00:00".to_string()),
            limit_weekly_usd: None,
            limit_monthly_usd: None,
            limit_total_usd: None,
            tags: None,
            note: None,
            source_provider_id: None,
            bridge_type: None,
            stream_idle_timeout_seconds: None,
            extension_values: None,
            account_usage_credentials_patch: None,
            account_usage_credentials_copy_from_provider_id: None,
            upstream_retry_policy_override: None,
            upstream_retry_policy_override_specified: false,
        }
    }

    fn minimal_share() -> ProviderShareEnvelopeV1 {
        ProviderShareEnvelopeV1 {
            kind: PROVIDER_SHARE_KIND.to_string(),
            schema_version: PROVIDER_SHARE_SCHEMA_VERSION,
            provider: ProviderShareProviderV1 {
                cli_key: "claude".to_string(),
                name: "测试供应商".to_string(),
                enabled: true,
                configuration: ProviderShareConfigurationV1 {
                    base_urls: vec!["https://example.invalid/v1".to_string()],
                    base_url_mode: ProviderBaseUrlMode::Order,
                    priority: 100,
                    cost_multiplier: 1.0,
                    claude_models: ProviderShareClaudeModelsV1::default(),
                    model_mapping: ProviderShareModelMappingV1::default(),
                    availability_test_model: None,
                    limits: ProviderShareLimitsV1 {
                        limit_5h_usd: None,
                        limit_daily_usd: None,
                        daily_reset_mode: DailyResetMode::Fixed,
                        daily_reset_time: "00:00:00".to_string(),
                        limit_weekly_usd: None,
                        limit_monthly_usd: None,
                        limit_total_usd: None,
                    },
                    tags: Vec::new(),
                    note: String::new(),
                    bridge_type: None,
                    stream_idle_timeout_seconds: None,
                    upstream_retry_policy_override: None,
                },
                authentication: ProviderShareAuthenticationV1::ApiKey {
                    api_key: "synthetic-key".to_string(),
                },
                extensions: Vec::new(),
            },
        }
    }

    fn provider_extension_manifest(plugin_id: &str, version: &str) -> PluginManifest {
        serde_json::from_value(serde_json::json!({
            "id": plugin_id,
            "name": "Provider Share Test Plugin",
            "version": version,
            "apiVersion": "1.0.0",
            "runtime": {"kind": "extensionHost", "language": "typescript"},
            "main": "dist/index.js",
            "capabilities": ["provider.extensionValues"],
            "contributes": {
                "providers": [{
                    "providerType": "example.provider-share.synthetic",
                    "displayName": "Synthetic Provider",
                    "targetCliKeys": ["claude"],
                    "extensionNamespace": "providerConfig"
                }]
            },
            "hostCompatibility": {"app": ">=0.60.0 <1.0.0", "pluginApi": "^1.0.0"}
        }))
        .expect("plugin manifest")
    }

    fn insert_provider_extension_plugin(db: &db::Db, plugin_id: &str, version: &str) {
        crate::infra::plugins::repository::insert_plugin(
            db,
            crate::infra::plugins::repository::InsertPluginInput {
                manifest: provider_extension_manifest(plugin_id, version),
                install_source: crate::domain::plugins::PluginInstallSource::Local,
                status: PluginStatus::Disabled,
                installed_dir: None,
            },
        )
        .expect("insert plugin");
    }

    fn share_with_extension(
        name: &str,
        plugin_id: &str,
        plugin_version: &str,
    ) -> ProviderShareEnvelopeV1 {
        let mut share = minimal_share();
        share.provider.name = name.to_string();
        share.provider.extensions.push(ProviderShareExtensionV1 {
            plugin_id: plugin_id.to_string(),
            plugin_version: plugin_version.to_string(),
            namespace: "providerConfig".to_string(),
            values: serde_json::json!({"synthetic": true}),
        });
        share
    }

    #[test]
    fn strict_v1_round_trip_is_deterministic_and_newline_terminated() {
        let share = minimal_share();
        let first = serialize_provider_share_v1(&share).expect("serialize");
        let parsed = parse_provider_share_v1(&first).expect("parse");
        let second = serialize_provider_share_v1(&parsed).expect("serialize again");

        assert_eq!(first, second);
        assert!(first.ends_with(b"\n"));
    }

    #[test]
    fn strict_v1_rejects_unknown_root_and_nested_fields() {
        let bytes = serialize_provider_share_v1(&minimal_share()).expect("serialize");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["unknown"] = serde_json::json!(true);
        assert!(parse_provider_share_v1(value.to_string().as_bytes()).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["provider"]["configuration"]["unknown"] = serde_json::json!(true);
        assert!(parse_provider_share_v1(value.to_string().as_bytes()).is_err());
    }

    #[test]
    fn strict_v1_rejects_future_version_and_oversized_content() {
        let bytes = serialize_provider_share_v1(&minimal_share()).expect("serialize");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["schema_version"] = serde_json::json!(2);
        let error = parse_provider_share_v1(value.to_string().as_bytes())
            .err()
            .expect("future version must fail");
        assert!(error.to_string().contains("update AIO Coding Hub"));

        let oversized = vec![b' '; PROVIDER_SHARE_MAX_BYTES + 1];
        assert!(parse_provider_share_v1(&oversized)
            .err()
            .expect("oversized content must fail")
            .to_string()
            .contains("maximum encoded size"));
    }

    #[test]
    fn strict_v1_rejects_oauth_provider_type_from_another_cli() {
        let mut share = minimal_share();
        let codex_provider_type = crate::gateway::oauth::registry::global_registry()
            .get_by_cli_key("codex")
            .expect("codex oauth adapter")
            .provider_type()
            .to_string();
        share.provider.authentication = ProviderShareAuthenticationV1::Oauth {
            provider_type: Some(codex_provider_type),
            access_token: None,
            refresh_token: None,
            id_token: None,
            token_uri: None,
            client_id: None,
            client_secret: None,
            expires_at: None,
            email: None,
            refresh_lead_seconds: DEFAULT_OAUTH_REFRESH_LEAD_SECONDS,
        };

        let error = normalize_provider_share_v1(share)
            .err()
            .expect("cross-CLI oauth provider type must fail");
        assert!(error
            .to_string()
            .contains("oauth provider_type is invalid for this CLI"));
    }

    #[test]
    fn sensitive_share_types_do_not_derive_debug() {
        let source = include_str!("share.rs");
        for declaration in [
            "pub(crate) struct ProviderShareEnvelopeV1",
            "pub(crate) struct ProviderShareProviderV1",
            "struct ProviderShareConfigurationV1",
            "struct ProviderShareClaudeModelsV1",
            "struct ProviderShareModelMappingV1",
            "struct ProviderShareLimitsV1",
            "struct ProviderShareRetryPolicyV1",
            "enum ProviderShareAuthenticationV1",
            "struct ProviderShareExtensionV1",
            "struct ProviderShareDbRow",
            "struct AuthenticationDbFields",
        ] {
            let item_start = source.find(declaration).expect("sensitive share type");
            let attribute_start = source[..item_start]
                .rfind("\n\n")
                .map_or(0, |index| index + 2);
            let attributes = &source[attribute_start..item_start];
            assert!(
                !attributes.contains("Debug"),
                "{declaration} must not derive Debug"
            );
        }
    }

    #[test]
    fn default_filename_preserves_unicode_and_replaces_unsafe_characters() {
        assert_eq!(
            provider_share_default_filename("claude", " 供应商:<测试>. "),
            "aio-coding-hub-provider-claude-供应商__测试_.json"
        );
        assert_eq!(
            provider_share_default_filename("codex", "..."),
            "aio-coding-hub-provider-codex-provider.json"
        );
        let emoji_filename = provider_share_default_filename("claude", &"😀".repeat(100));
        assert!(emoji_filename.len() <= MAX_SHARE_FILENAME_BYTES);
        assert!(emoji_filename.ends_with(".json"));
        assert!(emoji_filename.is_char_boundary(emoji_filename.len()));
    }

    #[test]
    fn import_allows_empty_api_key_and_forces_disabled_duplicate_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share.db")).expect("init db");
        let share = minimal_share();
        let first = import_provider_share_v1(&db, &share, "测试供应商", &[]).expect("first import");
        assert!(!first.enabled);

        let preview = preview_provider_share_v1(&db, &share).expect("preview duplicate");
        assert_eq!(preview.final_name, "测试供应商 副本");

        let mut empty_key_share = share.clone();
        empty_key_share.provider.name = "空密钥".to_string();
        empty_key_share.provider.authentication = ProviderShareAuthenticationV1::ApiKey {
            api_key: " \t ".to_string(),
        };
        let imported = import_provider_share_v1(&db, &empty_key_share, "空密钥", &[])
            .expect("empty key import");
        assert!(!imported.api_key_configured);
        assert!(!imported.enabled);
    }

    #[test]
    fn preview_rejects_wrong_builtin_account_usage_namespace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-extension.db"))
            .expect("init db");
        let mut share = minimal_share();
        share.provider.extensions.push(ProviderShareExtensionV1 {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            plugin_version: "1.0.0".to_string(),
            namespace: "forgedNamespace".to_string(),
            values: serde_json::json!({"enabled": true}),
        });

        let preview = preview_provider_share_v1(&db, &share).expect("preview");
        assert!(!preview.can_import);
        assert_eq!(
            preview.extensions[0].compatibility,
            ProviderShareExtensionCompatibility::NamespaceMismatch
        );
    }

    #[test]
    fn extension_capability_without_provider_namespace_does_not_grant_compatibility() {
        let manifest_without_contribution: PluginManifest =
            serde_json::from_value(serde_json::json!({
                "id": "example.plugin",
                "name": "Example",
                "version": "1.0.0",
                "apiVersion": "1.0.0",
                "runtime": {"kind": "extensionHost", "language": "javascript"},
                "capabilities": ["provider.extensionValues"],
                "hostCompatibility": {"app": "*", "pluginApi": "^1.0.0"}
            }))
            .expect("manifest");
        assert!(!manifest_owns_extension_namespace(
            &manifest_without_contribution,
            "example.plugin",
            "1.0.0",
            "claude",
            "providerConfig"
        ));

        let manifest_with_empty_providers: PluginManifest =
            serde_json::from_value(serde_json::json!({
                "id": "example.plugin",
                "name": "Example",
                "version": "1.0.0",
                "apiVersion": "1.0.0",
                "runtime": {"kind": "extensionHost", "language": "javascript"},
                "capabilities": ["provider.extensionValues"],
                "contributes": {"providers": []},
                "hostCompatibility": {"app": "*", "pluginApi": "^1.0.0"}
            }))
            .expect("manifest");
        assert!(!manifest_owns_extension_namespace(
            &manifest_with_empty_providers,
            "example.plugin",
            "1.0.0",
            "claude",
            "providerConfig"
        ));
    }

    #[test]
    fn duplicate_name_suffix_stays_within_name_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-name-limit.db"))
            .expect("init db");
        let mut share = minimal_share();
        share.provider.name = "长".repeat(MAX_PROVIDER_NAME_CHARS);
        import_provider_share_v1(&db, &share, &share.provider.name, &[]).expect("first import");

        let preview = preview_provider_share_v1(&db, &share).expect("duplicate preview");
        assert_eq!(preview.final_name.chars().count(), MAX_PROVIDER_NAME_CHARS);
        assert!(preview.final_name.ends_with(" 副本"));
        import_provider_share_v1(&db, &share, &preview.final_name, &preview.extensions)
            .expect("duplicate import");
    }

    #[test]
    fn database_oauth_credentials_round_trip_without_loss() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-oauth.db"))
            .expect("init db");
        let adapter = crate::gateway::oauth::registry::global_registry()
            .get_by_cli_key("codex")
            .expect("codex oauth adapter");
        let provider_type = adapter.provider_type().to_string();
        let token_uri = adapter.endpoints().token_url.to_string();
        let mut share = minimal_share();
        share.provider.cli_key = "codex".to_string();
        share.provider.name = "Complete OAuth Provider".to_string();
        share.provider.authentication = ProviderShareAuthenticationV1::Oauth {
            provider_type: Some(provider_type.clone()),
            access_token: Some("SYNTHETIC_ACCESS_TOKEN".to_string()),
            refresh_token: Some("SYNTHETIC_REFRESH_TOKEN".to_string()),
            id_token: Some("SYNTHETIC_ID_TOKEN".to_string()),
            token_uri: Some(token_uri.clone()),
            client_id: Some("SYNTHETIC_CLIENT_ID".to_string()),
            client_secret: Some("SYNTHETIC_CLIENT_SECRET".to_string()),
            expires_at: Some(4_102_444_800),
            email: Some("synthetic@example.invalid".to_string()),
            refresh_lead_seconds: 7_200,
        };

        let preview = preview_provider_share_v1(&db, &share).expect("preview oauth");
        assert_eq!(
            preview.credential_status,
            ProviderShareCredentialStatus::Available
        );
        let imported =
            import_provider_share_v1(&db, &share, &preview.final_name, &preview.extensions)
                .expect("import oauth");
        assert!(!imported.enabled);
        let stored =
            super::super::queries::get_oauth_details(&db, imported.id).expect("read oauth details");
        assert_eq!(stored.oauth_provider_type, provider_type);
        assert_eq!(stored.oauth_access_token, "SYNTHETIC_ACCESS_TOKEN");
        assert_eq!(
            stored.oauth_refresh_token.as_deref(),
            Some("SYNTHETIC_REFRESH_TOKEN")
        );
        assert_eq!(stored.oauth_id_token.as_deref(), Some("SYNTHETIC_ID_TOKEN"));
        assert_eq!(stored.oauth_token_uri.as_deref(), Some(token_uri.as_str()));
        assert_eq!(
            stored.oauth_client_id.as_deref(),
            Some("SYNTHETIC_CLIENT_ID")
        );
        assert_eq!(
            stored.oauth_client_secret.as_deref(),
            Some("SYNTHETIC_CLIENT_SECRET")
        );
        assert_eq!(stored.oauth_expires_at, Some(4_102_444_800));
        assert_eq!(
            stored.oauth_email.as_deref(),
            Some("synthetic@example.invalid")
        );
        assert_eq!(stored.oauth_refresh_lead_s, 7_200);

        let exported = export_provider_share_v1(&db, imported.id).expect("export oauth");
        assert!(!exported.provider.enabled);
        let ProviderShareAuthenticationV1::Oauth {
            provider_type: exported_provider_type,
            access_token,
            refresh_token,
            id_token,
            token_uri: exported_token_uri,
            client_id,
            client_secret,
            expires_at,
            email,
            refresh_lead_seconds,
        } = exported.provider.authentication
        else {
            panic!("exported authentication must remain oauth");
        };
        assert_eq!(
            exported_provider_type.as_deref(),
            Some(provider_type.as_str())
        );
        assert_eq!(access_token.as_deref(), Some("SYNTHETIC_ACCESS_TOKEN"));
        assert_eq!(refresh_token.as_deref(), Some("SYNTHETIC_REFRESH_TOKEN"));
        assert_eq!(id_token.as_deref(), Some("SYNTHETIC_ID_TOKEN"));
        assert_eq!(exported_token_uri.as_deref(), Some(token_uri.as_str()));
        assert_eq!(client_id.as_deref(), Some("SYNTHETIC_CLIENT_ID"));
        assert_eq!(client_secret.as_deref(), Some("SYNTHETIC_CLIENT_SECRET"));
        assert_eq!(expires_at, Some(4_102_444_800));
        assert_eq!(email.as_deref(), Some("synthetic@example.invalid"));
        assert_eq!(refresh_lead_seconds, 7_200);
    }

    #[test]
    fn referenced_provider_export_is_rejected_but_standalone_cx2cc_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-bridge.db"))
            .expect("init db");
        let mut source_share = minimal_share();
        source_share.provider.name = "Source Provider".to_string();
        let source = import_provider_share_v1(&db, &source_share, "Source Provider", &[])
            .expect("import source");

        let mut referenced_share = minimal_share();
        referenced_share.provider.name = "Referenced Provider".to_string();
        let referenced =
            import_provider_share_v1(&db, &referenced_share, "Referenced Provider", &[])
                .expect("import referenced");
        let conn = db.open_connection().expect("open db");
        conn.execute(
            "UPDATE providers SET source_provider_id = ?1, bridge_type = ?2 WHERE id = ?3",
            params![source.id, CX2CC_BRIDGE_TYPE, referenced.id],
        )
        .expect("mark provider as referenced");
        drop(conn);
        let error = export_provider_share_v1(&db, referenced.id)
            .err()
            .expect("referenced provider export must fail");
        assert_eq!(
            error.code(),
            "PROVIDER_SHARE_REFERENCED_PROVIDER",
            "{error}"
        );

        let mut standalone_share = minimal_share();
        standalone_share.provider.name = "Standalone cx2cc".to_string();
        standalone_share.provider.configuration.bridge_type = Some(CX2CC_BRIDGE_TYPE.to_string());
        standalone_share.provider.configuration.claude_models = ProviderShareClaudeModelsV1 {
            main_model: Some("claude-synthetic-main".to_string()),
            reasoning_model: Some("claude-synthetic-reasoning".to_string()),
            haiku_model: Some("claude-synthetic-haiku".to_string()),
            sonnet_model: Some("claude-synthetic-sonnet".to_string()),
            opus_model: Some("claude-synthetic-opus".to_string()),
        };
        standalone_share.provider.authentication = ProviderShareAuthenticationV1::ApiKey {
            api_key: String::new(),
        };
        let standalone_preview =
            preview_provider_share_v1(&db, &standalone_share).expect("preview standalone cx2cc");
        assert_eq!(
            standalone_preview.credential_status,
            ProviderShareCredentialStatus::NotRequired
        );
        let standalone = import_provider_share_v1(
            &db,
            &standalone_share,
            &standalone_preview.final_name,
            &standalone_preview.extensions,
        )
        .expect("import standalone cx2cc");
        assert_eq!(standalone.source_provider_id, None);
        assert_eq!(standalone.bridge_type.as_deref(), Some(CX2CC_BRIDGE_TYPE));
        let exported = export_provider_share_v1(&db, standalone.id).expect("export cx2cc");
        assert_eq!(
            exported.provider.configuration.bridge_type.as_deref(),
            Some(CX2CC_BRIDGE_TYPE)
        );
        assert!(exported.provider.configuration.base_urls.is_empty());
        assert_eq!(
            exported
                .provider
                .configuration
                .claude_models
                .main_model
                .as_deref(),
            Some("claude-synthetic-main")
        );
        assert_eq!(
            exported
                .provider
                .configuration
                .claude_models
                .reasoning_model
                .as_deref(),
            Some("claude-synthetic-reasoning")
        );
        assert_eq!(
            exported
                .provider
                .configuration
                .claude_models
                .haiku_model
                .as_deref(),
            Some("claude-synthetic-haiku")
        );
        assert_eq!(
            exported
                .provider
                .configuration
                .claude_models
                .sonnet_model
                .as_deref(),
            Some("claude-synthetic-sonnet")
        );
        assert_eq!(
            exported
                .provider
                .configuration
                .claude_models
                .opus_model
                .as_deref(),
            Some("claude-synthetic-opus")
        );
    }

    #[test]
    fn plugin_manifest_identity_and_version_must_match_the_database_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-plugin-identity.db"))
            .expect("init db");
        let plugin_id = "example.provider-share.identity";
        insert_provider_extension_plugin(&db, plugin_id, "1.0.0");
        let share = share_with_extension("Plugin Identity", plugin_id, "1.0.0");

        let conn = db.open_connection().expect("open db");
        let wrong_id = serde_json::to_string(&provider_extension_manifest(
            "example.provider-share.other",
            "1.0.0",
        ))
        .expect("serialize wrong-id manifest");
        conn.execute(
            "UPDATE plugins SET manifest_json = ?1 WHERE plugin_id = ?2",
            params![wrong_id, plugin_id],
        )
        .expect("tamper manifest id");
        drop(conn);
        let wrong_id_preview = preview_provider_share_v1(&db, &share).expect("preview wrong id");
        assert_eq!(
            wrong_id_preview.extensions[0].compatibility,
            ProviderShareExtensionCompatibility::NamespaceMismatch
        );

        let conn = db.open_connection().expect("open db");
        let wrong_version = serde_json::to_string(&provider_extension_manifest(plugin_id, "2.0.0"))
            .expect("serialize wrong-version manifest");
        conn.execute(
            "UPDATE plugins SET manifest_json = ?1 WHERE plugin_id = ?2",
            params![wrong_version, plugin_id],
        )
        .expect("tamper manifest version");
        drop(conn);
        let wrong_version_preview =
            preview_provider_share_v1(&db, &share).expect("preview wrong version");
        assert_eq!(
            wrong_version_preview.extensions[0].compatibility,
            ProviderShareExtensionCompatibility::VersionMismatch
        );

        let conn = db.open_connection().expect("open db");
        let matching = serde_json::to_string(&provider_extension_manifest(plugin_id, "1.0.0"))
            .expect("serialize matching manifest");
        conn.execute(
            "UPDATE plugins SET manifest_json = ?1 WHERE plugin_id = ?2",
            params![matching, plugin_id],
        )
        .expect("restore matching manifest");
        drop(conn);
        let matching_preview = preview_provider_share_v1(&db, &share).expect("preview matching");
        assert!(matching_preview.can_import);
    }

    #[test]
    fn extension_compatibility_changes_require_a_fresh_preview() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-plugin-snapshot.db"))
            .expect("init db");

        let missing_plugin_id = "example.provider-share.missing-snapshot";
        let missing_share = share_with_extension("Missing Snapshot", missing_plugin_id, "1.0.0");
        let missing_preview =
            preview_provider_share_v1(&db, &missing_share).expect("preview missing plugin");
        assert_eq!(
            missing_preview.extensions[0].compatibility,
            ProviderShareExtensionCompatibility::MissingPlugin
        );
        insert_provider_extension_plugin(&db, missing_plugin_id, "1.0.0");
        let missing_error = import_provider_share_v1(
            &db,
            &missing_share,
            &missing_preview.final_name,
            &missing_preview.extensions,
        )
        .expect_err("missing-to-compatible change must invalidate preview");
        assert_eq!(missing_error.code(), "PROVIDER_SHARE_PREVIEW_STALE");

        let version_plugin_id = "example.provider-share.version-snapshot";
        insert_provider_extension_plugin(&db, version_plugin_id, "2.0.0");
        let version_share = share_with_extension("Version Snapshot", version_plugin_id, "1.0.0");
        let version_preview =
            preview_provider_share_v1(&db, &version_share).expect("preview version mismatch");
        assert_eq!(
            version_preview.extensions[0].compatibility,
            ProviderShareExtensionCompatibility::VersionMismatch
        );
        let conn = db.open_connection().expect("open db");
        let matching =
            serde_json::to_string(&provider_extension_manifest(version_plugin_id, "1.0.0"))
                .expect("serialize matching manifest");
        conn.execute(
            "UPDATE plugins SET current_version = ?1, manifest_json = ?2 WHERE plugin_id = ?3",
            params!["1.0.0", matching, version_plugin_id],
        )
        .expect("make plugin compatible");
        drop(conn);
        let version_error = import_provider_share_v1(
            &db,
            &version_share,
            &version_preview.final_name,
            &version_preview.extensions,
        )
        .expect_err("version-to-compatible change must invalidate preview");
        assert_eq!(version_error.code(), "PROVIDER_SHARE_PREVIEW_STALE");
        assert!(super::super::queries::list_by_cli(&db, "claude")
            .expect("list providers")
            .is_empty());
    }

    #[test]
    fn third_party_plugin_version_mismatch_blocks_import_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-plugin-version.db"))
            .expect("init db");
        insert_provider_extension_plugin(&db, "example.provider-share", "2.0.0");
        let share =
            share_with_extension("Plugin Version Mismatch", "example.provider-share", "1.0.0");
        let preview = preview_provider_share_v1(&db, &share).expect("preview mismatch");
        assert!(!preview.can_import);
        assert_eq!(preview.extensions.len(), 1);
        assert_eq!(
            preview.extensions[0].compatibility,
            ProviderShareExtensionCompatibility::VersionMismatch
        );
        let error = import_provider_share_v1(&db, &share, &preview.final_name, &preview.extensions)
            .expect_err("mismatched plugin import must fail");
        assert_eq!(error.code(), "PROVIDER_SHARE_EXTENSION_INCOMPATIBLE");
        assert!(super::super::queries::list_by_cli(&db, "claude")
            .expect("list providers")
            .is_empty());
    }

    #[test]
    fn database_export_and_import_round_trip_complete_api_key_configuration() {
        let source_dir = tempfile::tempdir().expect("source tempdir");
        let source_db = crate::db::init_for_tests(&source_dir.path().join("source-api-key.db"))
            .expect("init source db");
        let mut input = provider_params("Complete API Provider", "codex");
        input.base_urls = vec![
            "https://primary.example.invalid/v1".to_string(),
            "https://secondary.example.invalid/v1".to_string(),
        ];
        input.base_url_mode = ProviderBaseUrlMode::Ping;
        input.priority = Some(321);
        input.cost_multiplier = 1.75;
        input.model_mapping = Some(ModelMapping {
            default_model: Some("gpt-synthetic-default".to_string()),
            exact: BTreeMap::from([("gpt-source".to_string(), "gpt-synthetic-target".to_string())]),
        });
        input.availability_test_model = Some("gpt-synthetic-test".to_string());
        input.limit_5h_usd = Some(5.5);
        input.limit_daily_usd = Some(12.5);
        input.daily_reset_time = Some("03:04:05".to_string());
        input.limit_weekly_usd = Some(60.0);
        input.limit_monthly_usd = Some(240.0);
        input.limit_total_usd = Some(999.0);
        input.tags = Some(vec!["synthetic".to_string(), "shared".to_string()]);
        input.note = Some("SYNTHETIC_NOTE".to_string());
        input.stream_idle_timeout_seconds = Some(75);
        input.extension_values = Some(vec![ProviderExtensionValuesInput {
            plugin_id: crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID.to_string(),
            namespace: crate::domain::provider_account_usage::ACCOUNT_USAGE_NAMESPACE.to_string(),
            values: serde_json::json!({
                "adapterKind": "newapi",
                "newApiQueryMode": "account",
                "newApiUserId": "999",
                "systemAccessToken": "SYNTHETIC_EXTENSION_SECRET",
                "timedRefreshEnabled": false,
                "refreshIntervalSeconds": 120
            }),
        }]);
        input.account_usage_credentials_patch = Some(
            crate::domain::provider_account_usage::ProviderAccountUsageCredentialsPatch {
                new_api_user_id: Some("42".to_string()),
                new_api_access_token: Some("SYNTHETIC_ACCOUNT_SECRET".to_string()),
                clear_new_api_access_token: false,
            },
        );
        input.upstream_retry_policy_override = Some(crate::settings::UpstreamRetryPolicy {
            enabled: true,
            status_codes: vec![429, 502],
            transport_errors: vec![crate::settings::UpstreamTransportRetryKind::Timeout],
            max_retries: 2,
            backoff_ms: 321,
            counts_toward_circuit_breaker: true,
        });
        input.upstream_retry_policy_override_specified = true;
        let source = super::super::queries::upsert(&source_db, input).expect("create source");

        let exported = export_provider_share_v1(&source_db, source.id).expect("export");
        let bytes = serialize_provider_share_v1(&exported).expect("serialize");
        let serialized = std::str::from_utf8(&bytes).expect("utf8");
        assert!(serialized.contains("SYNTHETIC_API_KEY"));
        assert!(!serialized.contains("SYNTHETIC_ACCOUNT_SECRET"));
        assert!(!serialized.contains("SYNTHETIC_EXTENSION_SECRET"));
        assert!(!serialized.contains("newApiUserId"));
        for excluded in [
            "created_at",
            "updated_at",
            "sort_order",
            "supported_models_json",
            "oauth_last_refreshed_at",
        ] {
            assert!(!serialized.contains(excluded), "must exclude {excluded}");
        }
        let parsed = parse_provider_share_v1(&bytes).expect("parse");
        assert_eq!(
            serialize_provider_share_v1(&parsed).expect("serialize parsed"),
            bytes
        );

        let target_dir = tempfile::tempdir().expect("target tempdir");
        let target_db = crate::db::init_for_tests(&target_dir.path().join("target-api-key.db"))
            .expect("init target db");
        let preview = preview_provider_share_v1(&target_db, &parsed).expect("preview");
        assert!(preview.can_import);
        let imported = import_provider_share_v1(
            &target_db,
            &parsed,
            &preview.final_name,
            &preview.extensions,
        )
        .expect("import target");

        assert!(!imported.enabled);
        assert_eq!(imported.cli_key, "codex");
        assert_eq!(imported.base_urls.len(), 2);
        assert_eq!(imported.base_url_mode, ProviderBaseUrlMode::Ping);
        assert_eq!(imported.priority, 321);
        assert_eq!(imported.cost_multiplier, 1.75);
        assert_eq!(
            imported.model_mapping,
            ModelMapping {
                default_model: Some("gpt-synthetic-default".to_string()),
                exact: BTreeMap::from([(
                    "gpt-source".to_string(),
                    "gpt-synthetic-target".to_string(),
                )]),
            }
        );
        assert_eq!(
            imported.availability_test_model.as_deref(),
            Some("gpt-synthetic-test")
        );
        assert_eq!(imported.limit_5h_usd, Some(5.5));
        assert_eq!(imported.limit_daily_usd, Some(12.5));
        assert_eq!(imported.daily_reset_time, "03:04:05");
        assert_eq!(imported.limit_weekly_usd, Some(60.0));
        assert_eq!(imported.limit_monthly_usd, Some(240.0));
        assert_eq!(imported.limit_total_usd, Some(999.0));
        assert_eq!(imported.tags, vec!["synthetic", "shared"]);
        assert_eq!(imported.note, "SYNTHETIC_NOTE");
        assert_eq!(imported.stream_idle_timeout_seconds, Some(75));
        assert_eq!(
            imported.upstream_retry_policy_override,
            Some(crate::settings::UpstreamRetryPolicy {
                enabled: true,
                status_codes: vec![429, 502],
                transport_errors: vec![crate::settings::UpstreamTransportRetryKind::Timeout],
                max_retries: 2,
                backoff_ms: 321,
                counts_toward_circuit_breaker: true,
            })
        );
        assert_eq!(
            super::super::queries::get_api_key_plaintext(&target_db, imported.id)
                .expect("read imported api key"),
            "SYNTHETIC_API_KEY"
        );
        assert_eq!(imported.extension_values.len(), 1);
        assert_eq!(
            imported.extension_values[0].plugin_id,
            crate::domain::provider_account_usage::ACCOUNT_USAGE_PLUGIN_ID
        );
        assert_eq!(imported.extension_values[0].values["adapterKind"], "newapi");
        assert_eq!(
            imported.extension_values[0].values["newApiQueryMode"],
            "account"
        );
        assert!(imported.newapi_account_user_id.is_none());
        assert!(!imported.newapi_account_access_token_configured);
        assert!(
            super::super::queries::default_route_list(&target_db, "codex")
                .expect("default route")
                .is_empty()
        );
    }

    #[test]
    fn import_rejects_stale_name_preview_without_writing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&dir.path().join("provider-share-stale.db"))
            .expect("init db");
        let share = minimal_share();
        let preview = preview_provider_share_v1(&db, &share).expect("preview");
        import_provider_share_v1(&db, &share, &preview.final_name, &preview.extensions)
            .expect("competing import");

        let error = import_provider_share_v1(&db, &share, &preview.final_name, &preview.extensions)
            .expect_err("stale preview");
        assert!(error.to_string().contains("PROVIDER_SHARE_PREVIEW_STALE"));
        assert_eq!(
            super::super::queries::list_by_cli(&db, "claude")
                .unwrap()
                .len(),
            1
        );
    }
}
