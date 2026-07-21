//! Usage: Provider-scoped model catalog persistence, discovery, and managed alias lookup.

use crate::db;
use crate::shared::error::{db_err, AppError, AppResult};
use crate::shared::time::now_unix_seconds;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const DISCOVERY_PROTOCOL: &str = "openai_compatible";
const DISCOVERY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DISCOVERY_TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
const DISCOVERY_BODY_MAX_BYTES: usize = 2 * 1024 * 1024;
const DISCOVERY_MODEL_MAX_COUNT: usize = 2048;
/// Maximum byte length accepted for a provider-scoped upstream model ID.
///
/// Gateway wire-model and response-observation paths must preserve this whole
/// range before comparing models, otherwise a valid managed route can be
/// rejected or a suffix mismatch can be hidden.
pub(crate) const REMOTE_MODEL_ID_MAX_BYTES: usize = 256;
pub const MODEL_CONTEXT_WINDOW_MIN_TOKENS: i64 = 1_024;
pub const MODEL_CONTEXT_WINDOW_MAX_TOKENS: i64 = 10_000_000;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderModelReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
    Ultra,
}

impl ProviderModelReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Minimal => 1,
            Self::Low => 2,
            Self::Medium => 3,
            Self::High => 4,
            Self::XHigh => 5,
            Self::Max => 6,
            Self::Ultra => 7,
        }
    }
}

#[derive(Debug, Clone, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelCapabilitiesInput {
    pub supported_reasoning_efforts: Vec<ProviderModelReasoningEffort>,
    pub default_reasoning_effort: Option<ProviderModelReasoningEffort>,
    pub context_window: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderModelCapabilities {
    pub(crate) supported_reasoning_efforts: Vec<ProviderModelReasoningEffort>,
    pub(crate) default_reasoning_effort: Option<ProviderModelReasoningEffort>,
    pub(crate) context_window: Option<i64>,
}

impl ProviderModelCapabilities {
    pub(crate) fn validate(&self) -> AppResult<()> {
        let normalized = normalize_capabilities(&ProviderModelCapabilitiesInput {
            supported_reasoning_efforts: self.supported_reasoning_efforts.clone(),
            default_reasoning_effort: self.default_reasoning_effort,
            context_window: self.context_window,
        })?;
        if normalized != *self {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "provider model capabilities are not canonical",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderModelSource {
    Discovered,
    Manual,
}

impl ProviderModelSource {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "discovered" => Some(Self::Discovered),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelEntry {
    pub model_uuid: String,
    pub provider_id: i64,
    pub remote_model_id: String,
    pub source: ProviderModelSource,
    pub stale: bool,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub capabilities_configured: bool,
    pub supported_reasoning_efforts: Vec<ProviderModelReasoningEffort>,
    pub default_reasoning_effort: Option<ProviderModelReasoningEffort>,
    pub context_window: Option<i64>,
}

#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelCatalog {
    pub provider_id: i64,
    pub provider_uuid: String,
    pub protocol: String,
    pub stale: bool,
    pub last_attempt_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error_code: Option<String>,
    pub models: Vec<ProviderModelEntry>,
}

struct RawProviderModelEntry {
    model_uuid: String,
    provider_id: i64,
    remote_model_id: String,
    source: ProviderModelSource,
    stale: bool,
    last_seen_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    capabilities_configured: bool,
    supported_reasoning_efforts_json: String,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedModelBinding {
    pub(crate) model_uuid: String,
    pub(crate) provider_id: i64,
    pub(crate) provider_uuid: String,
    pub(crate) remote_model_id: String,
}

#[derive(Clone)]
struct CatalogProvider {
    provider_id: i64,
    provider_uuid: String,
    base_urls: Vec<String>,
    auth_mode: String,
    oauth_provider_type: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
struct ProviderConnectionSnapshot {
    provider_id: i64,
    provider_uuid: String,
    cli_key: String,
    base_url: String,
    base_urls_json: String,
    base_url_mode: String,
    auth_mode: String,
    api_key_plaintext: String,
    oauth_provider_type: Option<String>,
    oauth_access_token: Option<String>,
    oauth_refresh_token: Option<String>,
    oauth_id_token: Option<String>,
    oauth_token_uri: Option<String>,
    oauth_client_id: Option<String>,
    oauth_client_secret: Option<String>,
    oauth_expires_at: Option<i64>,
    oauth_refresh_lead_s: i64,
    oauth_last_refreshed_at: Option<i64>,
    source_provider_id: Option<i64>,
    bridge_type: Option<String>,
}

impl ProviderConnectionSnapshot {
    fn same_discovery_target(&self, other: &Self) -> bool {
        self.provider_id == other.provider_id
            && self.provider_uuid == other.provider_uuid
            && self.cli_key == other.cli_key
            && self.base_url == other.base_url
            && self.base_urls_json == other.base_urls_json
            && self.base_url_mode == other.base_url_mode
            && self.auth_mode == other.auth_mode
            && self.api_key_plaintext == other.api_key_plaintext
            && self.oauth_provider_type == other.oauth_provider_type
            && self.oauth_token_uri == other.oauth_token_uri
            && self.oauth_client_id == other.oauth_client_id
            && self.oauth_client_secret == other.oauth_client_secret
            && self.oauth_refresh_lead_s == other.oauth_refresh_lead_s
            && self.source_provider_id == other.source_provider_id
            && self.bridge_type == other.bridge_type
    }

    fn matches_effective_credential(&self, credential: &str) -> bool {
        let stored = if self.auth_mode == "oauth" {
            self.oauth_access_token.as_deref().unwrap_or_default()
        } else {
            self.api_key_plaintext.as_str()
        };
        stored.trim() == credential.trim()
    }
}

struct RefreshContext {
    provider: CatalogProvider,
    transport: crate::providers::ProviderTransportContext,
    snapshot: ProviderConnectionSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryErrorCode {
    Unauthorized,
    Forbidden,
    NotSupported,
    Timeout,
    Network,
    InvalidResponse,
    Empty,
    Limit,
}

enum DiscoveryAttemptError {
    Catalog(DiscoveryErrorCode),
    Internal(AppError),
}

impl DiscoveryErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotSupported => "not_supported",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::InvalidResponse => "invalid_response",
            Self::Empty => "empty",
            Self::Limit => "limit",
        }
    }
}

fn classify_credential_resolution_error(error: AppError) -> DiscoveryAttemptError {
    match error.code() {
        "AUTH_RELOGIN_REQUIRED" => DiscoveryAttemptError::Catalog(DiscoveryErrorCode::Unauthorized),
        "OAUTH_REFRESH_FAILED" => {
            let diagnostic = error.to_string().to_ascii_lowercase();
            let code = if diagnostic.contains("auth_relogin_required")
                || diagnostic.contains("invalid_grant")
                || diagnostic.contains("401 unauthorized")
                || diagnostic.contains("403 forbidden")
            {
                DiscoveryErrorCode::Unauthorized
            } else if diagnostic.contains("timed out")
                || diagnostic.contains("timeout")
                || diagnostic.contains("deadline has elapsed")
            {
                DiscoveryErrorCode::Timeout
            } else {
                DiscoveryErrorCode::Network
            };
            DiscoveryAttemptError::Catalog(code)
        }
        "DB_ERROR" => DiscoveryAttemptError::Internal(AppError::new(
            "DB_ERROR",
            "failed to load provider discovery credential",
        )),
        "DB_NOT_FOUND" | "SEC_INVALID_INPUT" | "SEC_INVALID_STATE" => {
            DiscoveryAttemptError::Internal(AppError::new(
                "PROVIDER_MODELS_INVALID_PROVIDER",
                "provider discovery credential is not configured",
            ))
        }
        _ => DiscoveryAttemptError::Internal(AppError::new(
            "PROVIDER_MODELS_DISCOVERY_INTERNAL",
            "failed to prepare provider model discovery",
        )),
    }
}

fn validate_remote_model_id(value: &str) -> Result<&str, DiscoveryErrorCode> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > REMOTE_MODEL_ID_MAX_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(DiscoveryErrorCode::InvalidResponse);
    }
    Ok(value)
}

fn validate_manual_model_id(value: &str) -> AppResult<&str> {
    validate_remote_model_id(value).map_err(|_| {
        AppError::new(
            "SEC_INVALID_INPUT",
            format!(
                "remote_model_id must be non-empty, unpadded, control-free, and at most {REMOTE_MODEL_ID_MAX_BYTES} bytes"
            ),
        )
    })
}

fn normalize_capabilities(
    input: &ProviderModelCapabilitiesInput,
) -> AppResult<ProviderModelCapabilities> {
    let mut efforts = input.supported_reasoning_efforts.clone();
    efforts.sort_by_key(|effort| effort.rank());
    if efforts.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "supported_reasoning_efforts must not contain duplicates",
        ));
    }

    match (efforts.is_empty(), input.default_reasoning_effort) {
        (true, Some(_)) => {
            return Err(AppError::new(
                "SEC_INVALID_INPUT",
                "default_reasoning_effort must be empty when reasoning is disabled",
            ));
        }
        (false, None) => {
            return Err(AppError::new(
                "SEC_INVALID_INPUT",
                "default_reasoning_effort is required when reasoning efforts are configured",
            ));
        }
        (false, Some(default)) if !efforts.contains(&default) => {
            return Err(AppError::new(
                "SEC_INVALID_INPUT",
                "default_reasoning_effort must be included in supported_reasoning_efforts",
            ));
        }
        _ => {}
    }

    if input.context_window.is_some_and(|value| {
        !(MODEL_CONTEXT_WINDOW_MIN_TOKENS..=MODEL_CONTEXT_WINDOW_MAX_TOKENS).contains(&value)
    }) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            format!(
                "context_window must be between {MODEL_CONTEXT_WINDOW_MIN_TOKENS} and {MODEL_CONTEXT_WINDOW_MAX_TOKENS} tokens"
            ),
        ));
    }

    Ok(ProviderModelCapabilities {
        supported_reasoning_efforts: efforts,
        default_reasoning_effort: input.default_reasoning_effort,
        context_window: input.context_window,
    })
}

pub(crate) fn decode_stored_capabilities(
    configured: bool,
    efforts_json: &str,
    default_effort: Option<&str>,
    context_window: Option<i64>,
) -> AppResult<ProviderModelCapabilities> {
    let effort_values = serde_json::from_str::<Vec<String>>(efforts_json).map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "provider model reasoning efforts are invalid",
        )
    })?;
    let supported_reasoning_efforts = effort_values
        .iter()
        .map(|value| {
            ProviderModelReasoningEffort::parse(value).ok_or_else(|| {
                AppError::new(
                    "DB_INVALID_DATA",
                    "provider model reasoning effort is unsupported",
                )
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let default_reasoning_effort = default_effort
        .map(|value| {
            ProviderModelReasoningEffort::parse(value).ok_or_else(|| {
                AppError::new(
                    "DB_INVALID_DATA",
                    "provider model default reasoning effort is unsupported",
                )
            })
        })
        .transpose()?;

    if !configured {
        if !supported_reasoning_efforts.is_empty()
            || default_reasoning_effort.is_some()
            || context_window.is_some()
        {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "unconfigured provider model contains capability values",
            ));
        }
        return Ok(ProviderModelCapabilities {
            supported_reasoning_efforts,
            default_reasoning_effort,
            context_window,
        });
    }

    let normalized = normalize_capabilities(&ProviderModelCapabilitiesInput {
        supported_reasoning_efforts,
        default_reasoning_effort,
        context_window,
    })
    .map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "provider model capabilities are inconsistent",
        )
    })?;
    let stored_efforts = effort_values.iter().map(String::as_str).collect::<Vec<_>>();
    let normalized_efforts = normalized
        .supported_reasoning_efforts
        .iter()
        .map(|effort| effort.as_str())
        .collect::<Vec<_>>();
    if stored_efforts != normalized_efforts {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "provider model reasoning efforts are not canonical",
        ));
    }
    Ok(normalized)
}

fn serialize_reasoning_efforts(efforts: &[ProviderModelReasoningEffort]) -> AppResult<String> {
    serde_json::to_string(
        &efforts
            .iter()
            .map(|effort| effort.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|_| AppError::new("SYSTEM_ERROR", "failed to serialize model capabilities"))
}

fn validate_expected_provider_uuid(value: &str) -> AppResult<&str> {
    if !crate::shared::uuid::is_canonical_uuid_v4(value) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider_uuid must be a canonical UUIDv4",
        ));
    }
    Ok(value)
}

fn direct_codex_provider_metadata_from_conn(
    conn: &Connection,
    provider_id: i64,
) -> AppResult<CatalogProvider> {
    if provider_id <= 0 {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "provider_id must be positive",
        ));
    }
    let provider = crate::providers::get_by_id(conn, provider_id)?;
    if provider.cli_key != "codex"
        || provider.source_provider_id.is_some()
        || provider.bridge_type.is_some()
    {
        return Err(AppError::new(
            "PROVIDER_MODELS_UNSUPPORTED_PROVIDER",
            "model catalogs are supported only for direct Codex providers",
        ));
    }
    Ok(CatalogProvider {
        provider_id,
        provider_uuid: provider.provider_uuid,
        base_urls: provider.base_urls.clone(),
        auth_mode: provider.auth_mode,
        oauth_provider_type: provider.oauth_provider_type,
    })
}

#[cfg(test)]
fn direct_codex_provider_metadata(db: &db::Db, provider_id: i64) -> AppResult<CatalogProvider> {
    let conn = db.open_connection()?;
    direct_codex_provider_metadata_from_conn(&conn, provider_id)
}

fn query_provider_connection_snapshot(
    conn: &Connection,
    provider_id: i64,
) -> rusqlite::Result<Option<ProviderConnectionSnapshot>> {
    conn.query_row(
        r#"
SELECT
  id,
  provider_uuid,
  cli_key,
  base_url,
  base_urls_json,
  base_url_mode,
  COALESCE(auth_mode, 'api_key'),
  COALESCE(api_key_plaintext, ''),
  oauth_provider_type,
  oauth_access_token,
  oauth_refresh_token,
  oauth_id_token,
  oauth_token_uri,
  oauth_client_id,
  oauth_client_secret,
  oauth_expires_at,
  COALESCE(oauth_refresh_lead_s, 3600),
  oauth_last_refreshed_at,
  source_provider_id,
  bridge_type
FROM providers
WHERE id = ?1
"#,
        params![provider_id],
        |row| {
            Ok(ProviderConnectionSnapshot {
                provider_id: row.get(0)?,
                provider_uuid: row.get(1)?,
                cli_key: row.get(2)?,
                base_url: row.get(3)?,
                base_urls_json: row.get(4)?,
                base_url_mode: row.get(5)?,
                auth_mode: row.get(6)?,
                api_key_plaintext: row.get(7)?,
                oauth_provider_type: row.get(8)?,
                oauth_access_token: row.get(9)?,
                oauth_refresh_token: row.get(10)?,
                oauth_id_token: row.get(11)?,
                oauth_token_uri: row.get(12)?,
                oauth_client_id: row.get(13)?,
                oauth_client_secret: row.get(14)?,
                oauth_expires_at: row.get(15)?,
                oauth_refresh_lead_s: row.get(16)?,
                oauth_last_refreshed_at: row.get(17)?,
                source_provider_id: row.get(18)?,
                bridge_type: row.get(19)?,
            })
        },
    )
    .optional()
}

fn discovery_transport(
    provider: &CatalogProvider,
    snapshot: &ProviderConnectionSnapshot,
) -> AppResult<crate::providers::ProviderTransportContext> {
    if provider.base_urls.is_empty() {
        return Err(AppError::new(
            "PROVIDER_MODELS_INVALID_PROVIDER",
            "provider has no Base URL",
        ));
    }
    Ok(crate::providers::ProviderTransportContext {
        provider_id: provider.provider_id,
        base_urls: provider.base_urls.clone(),
        api_key_plaintext: snapshot.api_key_plaintext.clone(),
        auth_mode: provider.auth_mode.clone(),
        oauth_provider_type: provider.oauth_provider_type.clone(),
    })
}

fn capture_refresh_context(db: &db::Db, provider_id: i64) -> AppResult<RefreshContext> {
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction()
        .map_err(|error| db_err!("failed to start provider discovery snapshot: {error}"))?;
    let provider = direct_codex_provider_metadata_from_conn(&tx, provider_id)?;
    let snapshot = query_provider_connection_snapshot(&tx, provider_id)
        .map_err(|error| db_err!("failed to read provider discovery snapshot: {error}"))?
        .ok_or_else(|| AppError::new("DB_NOT_FOUND", "provider not found"))?;
    let transport = discovery_transport(&provider, &snapshot)?;
    tx.commit()
        .map_err(|error| db_err!("failed to finish provider discovery snapshot: {error}"))?;
    Ok(RefreshContext {
        provider,
        transport,
        snapshot,
    })
}

fn direct_codex_provider_metadata_for_identity_from_conn(
    conn: &Connection,
    provider_id: i64,
    expected_provider_uuid: &str,
) -> AppResult<CatalogProvider> {
    let expected_provider_uuid = validate_expected_provider_uuid(expected_provider_uuid)?;
    let provider = match direct_codex_provider_metadata_from_conn(conn, provider_id) {
        Ok(provider) => provider,
        Err(error)
            if matches!(
                error.code(),
                "DB_NOT_FOUND" | "PROVIDER_MODELS_UNSUPPORTED_PROVIDER"
            ) =>
        {
            return Err(provider_identity_changed_error());
        }
        Err(error) => return Err(error),
    };
    if provider.provider_uuid != expected_provider_uuid {
        return Err(provider_identity_changed_error());
    }
    Ok(provider)
}

fn read_catalog_from_conn(
    conn: &Connection,
    provider_id: i64,
    expected_provider_uuid: &str,
) -> AppResult<ProviderModelCatalog> {
    let provider = direct_codex_provider_metadata_for_identity_from_conn(
        conn,
        provider_id,
        expected_provider_uuid,
    )?;
    let state = conn
        .query_row(
            r#"
SELECT protocol, stale, last_attempt_at, last_success_at, last_error_code
FROM provider_model_catalogs
WHERE provider_id = ?1
"#,
            params![provider_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| db_err!("failed to query provider model catalog: {error}"))?
        .unwrap_or_else(|| (DISCOVERY_PROTOCOL.to_string(), true, None, None, None));

    let mut statement = conn
        .prepare_cached(
            r#"
SELECT model_uuid, provider_id, remote_model_id, source, stale, last_seen_at, created_at, updated_at,
       capabilities_configured, supported_reasoning_efforts_json,
       default_reasoning_effort, context_window
FROM provider_models
WHERE provider_id = ?1
ORDER BY remote_model_id COLLATE NOCASE ASC, model_uuid ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare provider model query: {error}"))?;
    let rows = statement
        .query_map(params![provider_id], |row| {
            let source_raw = row.get::<_, String>(3)?;
            let source = ProviderModelSource::parse(&source_raw).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid provider model source",
                    )
                    .into(),
                )
            })?;
            Ok(RawProviderModelEntry {
                model_uuid: row.get(0)?,
                provider_id: row.get(1)?,
                remote_model_id: row.get(2)?,
                source,
                stale: row.get::<_, i64>(4)? != 0,
                last_seen_at: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                capabilities_configured: row.get::<_, i64>(8)? != 0,
                supported_reasoning_efforts_json: row.get(9)?,
                default_reasoning_effort: row.get(10)?,
                context_window: row.get(11)?,
            })
        })
        .map_err(|error| db_err!("failed to query provider models: {error}"))?;
    let mut models = Vec::new();
    for row in rows {
        let row = row.map_err(|error| db_err!("failed to read provider model: {error}"))?;
        let capabilities = decode_stored_capabilities(
            row.capabilities_configured,
            &row.supported_reasoning_efforts_json,
            row.default_reasoning_effort.as_deref(),
            row.context_window,
        )?;
        models.push(ProviderModelEntry {
            model_uuid: row.model_uuid,
            provider_id: row.provider_id,
            remote_model_id: row.remote_model_id,
            source: row.source,
            stale: row.stale,
            last_seen_at: row.last_seen_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            capabilities_configured: row.capabilities_configured,
            supported_reasoning_efforts: capabilities.supported_reasoning_efforts,
            default_reasoning_effort: capabilities.default_reasoning_effort,
            context_window: capabilities.context_window,
        });
    }

    Ok(ProviderModelCatalog {
        provider_id,
        provider_uuid: provider.provider_uuid,
        protocol: state.0,
        stale: state.1,
        last_attempt_at: state.2,
        last_success_at: state.3,
        last_error_code: state.4,
        models,
    })
}

pub fn get(
    db: &db::Db,
    provider_id: i64,
    expected_provider_uuid: &str,
) -> AppResult<ProviderModelCatalog> {
    let expected_provider_uuid = validate_expected_provider_uuid(expected_provider_uuid)?;
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| db_err!("failed to start provider model read transaction: {error}"))?;
    let catalog = read_catalog_from_conn(&tx, provider_id, expected_provider_uuid)?;
    tx.commit()
        .map_err(|error| db_err!("failed to finish provider model read transaction: {error}"))?;
    Ok(catalog)
}

pub fn manual_upsert(
    db: &db::Db,
    provider_id: i64,
    expected_provider_uuid: &str,
    remote_model_id: &str,
) -> AppResult<ProviderModelCatalog> {
    let expected_provider_uuid = validate_expected_provider_uuid(expected_provider_uuid)?;
    let remote_model_id = validate_manual_model_id(remote_model_id)?;
    let now = now_unix_seconds();
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| db_err!("failed to start manual provider model transaction: {error}"))?;
    direct_codex_provider_metadata_for_identity_from_conn(
        &tx,
        provider_id,
        expected_provider_uuid,
    )?;
    tx.execute(
        r#"
INSERT INTO provider_models(
  model_uuid, provider_id, remote_model_id, source, stale, last_seen_at, created_at, updated_at
) VALUES (?1, ?2, ?3, 'manual', 0, NULL, ?4, ?4)
ON CONFLICT(provider_id, remote_model_id) DO UPDATE SET
  source = 'manual',
  stale = 0,
  updated_at = excluded.updated_at
"#,
        params![
            crate::shared::uuid::new_uuid_v4(),
            provider_id,
            remote_model_id,
            now
        ],
    )
    .map_err(|error| db_err!("failed to save manual provider model: {error}"))?;
    let catalog = read_catalog_from_conn(&tx, provider_id, expected_provider_uuid)?;
    tx.commit()
        .map_err(|error| db_err!("failed to commit manual provider model: {error}"))?;
    Ok(catalog)
}

pub fn manual_delete(
    db: &db::Db,
    provider_id: i64,
    expected_provider_uuid: &str,
    model_uuid: &str,
) -> AppResult<ProviderModelCatalog> {
    let expected_provider_uuid = validate_expected_provider_uuid(expected_provider_uuid)?;
    if !crate::shared::uuid::is_canonical_uuid_v4(model_uuid) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "model_uuid must be a canonical UUIDv4",
        ));
    }
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| db_err!("failed to start manual provider model transaction: {error}"))?;
    direct_codex_provider_metadata_for_identity_from_conn(
        &tx,
        provider_id,
        expected_provider_uuid,
    )?;
    let referenced: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM codex_managed_profiles WHERE model_uuid = ?1)",
            params![model_uuid],
            |row| row.get(0),
        )
        .map_err(|error| db_err!("failed to query managed profile model reference: {error}"))?;
    if referenced {
        return Err(AppError::new(
            "PROVIDER_MODEL_MANAGED_PROFILE_REFERENCED",
            "model is referenced by a managed Codex profile",
        ));
    }
    let changed = tx
        .execute(
            "DELETE FROM provider_models WHERE provider_id = ?1 AND model_uuid = ?2 AND source = 'manual'",
            params![provider_id, model_uuid],
        )
        .map_err(|error| db_err!("failed to delete manual provider model: {error}"))?;
    if changed == 0 {
        return Err(AppError::new(
            "DB_NOT_FOUND",
            "manual provider model not found",
        ));
    }
    let catalog = read_catalog_from_conn(&tx, provider_id, expected_provider_uuid)?;
    tx.commit()
        .map_err(|error| db_err!("failed to commit manual provider model: {error}"))?;
    Ok(catalog)
}

pub fn update_capabilities<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
    provider_id: i64,
    expected_provider_uuid: &str,
    model_uuid: &str,
    input: &ProviderModelCapabilitiesInput,
) -> AppResult<ProviderModelCatalog> {
    let _guard = crate::codex_managed_profiles::lock_profile_lifecycle();
    let expected_provider_uuid = validate_expected_provider_uuid(expected_provider_uuid)?;
    if !crate::shared::uuid::is_canonical_uuid_v4(model_uuid) {
        return Err(AppError::new(
            "SEC_INVALID_INPUT",
            "model_uuid must be a canonical UUIDv4",
        ));
    }
    let capabilities = normalize_capabilities(input)?;
    let efforts_json = serialize_reasoning_efforts(&capabilities.supported_reasoning_efforts)?;

    let mut conn = db.open_connection()?;
    direct_codex_provider_metadata_for_identity_from_conn(
        &conn,
        provider_id,
        expected_provider_uuid,
    )?;
    let model_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_models WHERE provider_id = ?1 AND model_uuid = ?2)",
            params![provider_id, model_uuid],
            |row| row.get(0),
        )
        .map_err(|error| db_err!("failed to query provider model capability target: {error}"))?;
    if !model_exists {
        return Err(AppError::new("DB_NOT_FOUND", "provider model not found"));
    }

    let mut catalog_profiles = crate::codex_model_catalog::managed::load_profiles(&conn)?;
    let mut affects_active_catalog = false;
    for profile in &mut catalog_profiles {
        if profile.model_uuid == model_uuid {
            profile.set_capabilities(capabilities.clone())?;
            affects_active_catalog = true;
        }
    }
    let catalog_plan = affects_active_catalog
        .then(|| crate::codex_model_catalog::managed::prepare_for_profiles(app, &catalog_profiles))
        .transpose()?;

    let now = now_unix_seconds();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            db_err!("failed to start provider model capability transaction: {error}")
        })?;
    direct_codex_provider_metadata_for_identity_from_conn(
        &tx,
        provider_id,
        expected_provider_uuid,
    )?;
    let changed = tx
        .execute(
            r#"
UPDATE provider_models
SET capabilities_configured = 1,
    supported_reasoning_efforts_json = ?1,
    default_reasoning_effort = ?2,
    context_window = ?3,
    updated_at = ?4
WHERE provider_id = ?5 AND model_uuid = ?6
"#,
            params![
                efforts_json,
                capabilities
                    .default_reasoning_effort
                    .map(ProviderModelReasoningEffort::as_str),
                capabilities.context_window,
                now,
                provider_id,
                model_uuid,
            ],
        )
        .map_err(|error| db_err!("failed to update provider model capabilities: {error}"))?;
    if changed == 0 {
        return Err(AppError::new("DB_NOT_FOUND", "provider model not found"));
    }
    let catalog = read_catalog_from_conn(&tx, provider_id, expected_provider_uuid)?;

    let applied_catalog = match catalog_plan {
        Some(plan) => Some(plan.apply(app)?),
        None => None,
    };
    if let Err(error) = tx.commit() {
        if let Some(applied) = applied_catalog {
            applied.rollback()?;
        }
        return Err(db_err!(
            "failed to commit provider model capabilities: {error}"
        ));
    }
    Ok(catalog)
}

fn build_models_url(base_url: &str) -> Result<reqwest::Url, DiscoveryErrorCode> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| DiscoveryErrorCode::Network)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(DiscoveryErrorCode::Network);
    }
    url.set_query(None);
    url.set_fragment(None);
    let base_path = url.path().trim_end_matches('/');
    let path = if base_path.is_empty() {
        "/v1/models".to_string()
    } else {
        format!("{base_path}/models")
    };
    url.set_path(&path);
    Ok(url)
}

async fn read_discovery_body(
    mut response: reqwest::Response,
) -> Result<String, DiscoveryErrorCode> {
    if response
        .content_length()
        .is_some_and(|length| length > DISCOVERY_BODY_MAX_BYTES as u64)
    {
        return Err(DiscoveryErrorCode::Limit);
    }

    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or_default()
        .min(DISCOVERY_BODY_MAX_BYTES);
    let mut bytes = Vec::with_capacity(capacity);
    loop {
        let chunk = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                DiscoveryErrorCode::Timeout
            } else {
                DiscoveryErrorCode::Network
            }
        })?;
        let Some(chunk) = chunk else {
            break;
        };
        if chunk.len() > DISCOVERY_BODY_MAX_BYTES.saturating_sub(bytes.len()) {
            return Err(DiscoveryErrorCode::Limit);
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| DiscoveryErrorCode::InvalidResponse)
}

fn parse_openai_models(body: &str) -> Result<Vec<String>, DiscoveryErrorCode> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| DiscoveryErrorCode::InvalidResponse)?;
    let data = value
        .as_object()
        .and_then(|object| object.get("data"))
        .and_then(serde_json::Value::as_array)
        .ok_or(DiscoveryErrorCode::InvalidResponse)?;
    if data.len() > DISCOVERY_MODEL_MAX_COUNT {
        return Err(DiscoveryErrorCode::Limit);
    }

    let mut seen = HashSet::with_capacity(data.len());
    let mut models = Vec::with_capacity(data.len());
    for item in data {
        let model_id = item
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(serde_json::Value::as_str)
            .ok_or(DiscoveryErrorCode::InvalidResponse)?;
        validate_remote_model_id(model_id)?;
        if seen.insert(model_id.to_string()) {
            models.push(model_id.to_string());
        }
    }
    if models.is_empty() {
        return Err(DiscoveryErrorCode::Empty);
    }
    Ok(models)
}

fn build_discovery_client() -> Result<reqwest::Client, DiscoveryErrorCode> {
    let user_agent = format!(
        "aio-coding-hub-model-discovery/{}",
        env!("CARGO_PKG_VERSION")
    );
    crate::gateway::http_client::build_client_with_current_proxy(
        &user_agent,
        DISCOVERY_CONNECT_TIMEOUT,
        DISCOVERY_TOTAL_TIMEOUT,
        reqwest::redirect::Policy::none(),
    )
    .map_err(|_| DiscoveryErrorCode::Network)
}

async fn discover_from_provider_with_client(
    client: &reqwest::Client,
    provider: &CatalogProvider,
    effective_credential: &str,
) -> Result<Vec<String>, DiscoveryErrorCode> {
    let mut last_error = DiscoveryErrorCode::Network;
    let mut seen_urls = HashSet::new();
    for base_url in &provider.base_urls {
        let url = match build_models_url(base_url) {
            Ok(url) => url,
            Err(error) => {
                last_error = error;
                continue;
            }
        };
        if !seen_urls.insert(url.as_str().to_string()) {
            continue;
        }
        let response = match client
            .get(url)
            .bearer_auth(effective_credential)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                last_error = if error.is_timeout() {
                    DiscoveryErrorCode::Timeout
                } else {
                    DiscoveryErrorCode::Network
                };
                continue;
            }
        };
        last_error = match response.status().as_u16() {
            200..=299 => match read_discovery_body(response).await {
                Ok(body) => match parse_openai_models(&body) {
                    Ok(models) => return Ok(models),
                    Err(error) => error,
                },
                Err(error) => error,
            },
            401 => DiscoveryErrorCode::Unauthorized,
            403 => DiscoveryErrorCode::Forbidden,
            404 | 405 => DiscoveryErrorCode::NotSupported,
            _ => DiscoveryErrorCode::Network,
        };
    }
    Err(last_error)
}

fn refresh_locks() -> &'static Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn refresh_lock(provider_uuid: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = refresh_locks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(provider_uuid.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn commit_if_snapshot_current<T, F>(
    db: &db::Db,
    snapshot: &ProviderConnectionSnapshot,
    apply: F,
) -> AppResult<Option<T>>
where
    F: FnOnce(&rusqlite::Transaction<'_>) -> AppResult<T>,
{
    let mut conn = db.open_connection()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| db_err!("failed to start provider model refresh transaction: {error}"))?;
    let current = query_provider_connection_snapshot(&tx, snapshot.provider_id)
        .map_err(|error| db_err!("failed to verify provider discovery snapshot: {error}"))?;
    if current.as_ref() != Some(snapshot) {
        return Ok(None);
    }
    let output = apply(&tx)?;
    tx.commit()
        .map_err(|error| db_err!("failed to commit provider model refresh: {error}"))?;
    Ok(Some(output))
}

fn apply_refresh_failure(
    tx: &rusqlite::Transaction<'_>,
    snapshot: &ProviderConnectionSnapshot,
    now: i64,
    error: DiscoveryErrorCode,
) -> AppResult<()> {
    tx.execute(
        r#"
INSERT INTO provider_model_catalogs(
  provider_id, protocol, stale, last_attempt_at, last_success_at, last_error_code
) VALUES (?1, ?2, 1, ?3, NULL, ?4)
ON CONFLICT(provider_id) DO UPDATE SET
  stale = 1,
  last_attempt_at = excluded.last_attempt_at,
  last_error_code = excluded.last_error_code
"#,
        params![
            snapshot.provider_id,
            DISCOVERY_PROTOCOL,
            now,
            error.as_str()
        ],
    )
    .map_err(|db_error| db_err!("failed to record provider model refresh failure: {db_error}"))?;
    Ok(())
}

#[cfg(test)]
fn record_refresh_failure(
    db: &db::Db,
    snapshot: &ProviderConnectionSnapshot,
    now: i64,
    error: DiscoveryErrorCode,
) -> AppResult<bool> {
    commit_if_snapshot_current(db, snapshot, |tx| {
        apply_refresh_failure(tx, snapshot, now, error)
    })
    .map(|result| result.is_some())
}

fn apply_refresh_success(
    tx: &rusqlite::Transaction<'_>,
    snapshot: &ProviderConnectionSnapshot,
    now: i64,
    remote_model_ids: &[String],
) -> AppResult<()> {
    tx.execute(
        "UPDATE provider_models SET stale = 1, updated_at = ?1 WHERE provider_id = ?2 AND source = 'discovered'",
        params![now, snapshot.provider_id],
    )
    .map_err(|error| db_err!("failed to mark previous discovered models stale: {error}"))?;
    for remote_model_id in remote_model_ids {
        tx.execute(
            r#"
INSERT INTO provider_models(
  model_uuid, provider_id, remote_model_id, source, stale, last_seen_at, created_at, updated_at
) VALUES (?1, ?2, ?3, 'discovered', 0, ?4, ?4, ?4)
ON CONFLICT(provider_id, remote_model_id) DO UPDATE SET
  stale = 0,
  last_seen_at = excluded.last_seen_at,
  updated_at = excluded.updated_at
"#,
            params![
                crate::shared::uuid::new_uuid_v4(),
                snapshot.provider_id,
                remote_model_id,
                now
            ],
        )
        .map_err(|error| db_err!("failed to save discovered provider model: {error}"))?;
    }
    tx.execute(
        r#"
INSERT INTO provider_model_catalogs(
  provider_id, protocol, stale, last_attempt_at, last_success_at, last_error_code
) VALUES (?1, ?2, 0, ?3, ?3, NULL)
ON CONFLICT(provider_id) DO UPDATE SET
  protocol = excluded.protocol,
  stale = 0,
  last_attempt_at = excluded.last_attempt_at,
  last_success_at = excluded.last_success_at,
  last_error_code = NULL
"#,
        params![snapshot.provider_id, DISCOVERY_PROTOCOL, now],
    )
    .map_err(|error| db_err!("failed to update provider model catalog: {error}"))?;
    Ok(())
}

#[cfg(test)]
fn record_refresh_success(
    db: &db::Db,
    snapshot: &ProviderConnectionSnapshot,
    now: i64,
    remote_model_ids: &[String],
) -> AppResult<bool> {
    commit_if_snapshot_current(db, snapshot, |tx| {
        apply_refresh_success(tx, snapshot, now, remote_model_ids)
    })
    .map(|result| result.is_some())
}

fn provider_identity_changed_error() -> AppError {
    AppError::new(
        "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED",
        "provider identity changed during model catalog operation",
    )
}

fn finish_refresh_attempt(
    db: &db::Db,
    provider_id: i64,
    snapshot: &ProviderConnectionSnapshot,
    attempt_at: i64,
    result: Result<Vec<String>, DiscoveryErrorCode>,
) -> AppResult<ProviderModelCatalog> {
    commit_if_snapshot_current(db, snapshot, |tx| {
        match result {
            Ok(models) => apply_refresh_success(tx, snapshot, attempt_at, &models)?,
            Err(error) => apply_refresh_failure(tx, snapshot, attempt_at, error)?,
        }
        read_catalog_from_conn(tx, provider_id, &snapshot.provider_uuid)
    })?
    .ok_or_else(provider_identity_changed_error)
}

pub async fn refresh(
    db: &db::Db,
    provider_id: i64,
    expected_provider_uuid: &str,
) -> AppResult<ProviderModelCatalog> {
    let expected_provider_uuid = validate_expected_provider_uuid(expected_provider_uuid)?;
    let lock = refresh_lock(expected_provider_uuid);
    let _guard = lock.clone().lock_owned().await;
    let initial_context = capture_refresh_context(db, provider_id)?;
    if initial_context.provider.provider_uuid != expected_provider_uuid {
        return Err(provider_identity_changed_error());
    }

    let attempt_at = now_unix_seconds();
    let deadline = tokio::time::Instant::now() + DISCOVERY_TOTAL_TIMEOUT;
    let discovery_client = match build_discovery_client() {
        Ok(client) => client,
        Err(error) => {
            return finish_refresh_attempt(
                db,
                provider_id,
                &initial_context.snapshot,
                attempt_at,
                Err(error),
            );
        }
    };
    let effective_credential = match tokio::time::timeout_at(
        deadline,
        crate::providers::resolve_effective_transport_credential(
            db,
            &discovery_client,
            "codex",
            &initial_context.transport,
        ),
    )
    .await
    {
        Ok(Ok(credential)) => credential,
        Ok(Err(error)) => match classify_credential_resolution_error(error) {
            DiscoveryAttemptError::Catalog(error) => {
                return finish_refresh_attempt(
                    db,
                    provider_id,
                    &initial_context.snapshot,
                    attempt_at,
                    Err(error),
                );
            }
            DiscoveryAttemptError::Internal(error) => return Err(error),
        },
        Err(_) => {
            return finish_refresh_attempt(
                db,
                provider_id,
                &initial_context.snapshot,
                attempt_at,
                Err(DiscoveryErrorCode::Timeout),
            );
        }
    };

    let request_context = capture_refresh_context(db, provider_id)?;
    if request_context.provider.provider_uuid != expected_provider_uuid {
        return Err(provider_identity_changed_error());
    }
    if !initial_context
        .snapshot
        .same_discovery_target(&request_context.snapshot)
        || !request_context
            .snapshot
            .matches_effective_credential(&effective_credential)
    {
        return Err(AppError::new(
            "PROVIDER_MODELS_CONNECTION_CHANGED",
            "provider connection changed before model discovery",
        ));
    }

    let discovery = tokio::time::timeout_at(
        deadline,
        discover_from_provider_with_client(
            &discovery_client,
            &request_context.provider,
            effective_credential.trim(),
        ),
    )
    .await;

    let result = match discovery {
        Ok(result) => result,
        Err(_) => Err(DiscoveryErrorCode::Timeout),
    };
    finish_refresh_attempt(
        db,
        provider_id,
        &request_context.snapshot,
        attempt_at,
        result,
    )
}

pub(crate) fn resolve_managed_model_alias(
    db: &db::Db,
    canonical: &str,
) -> AppResult<Option<ManagedModelBinding>> {
    let Some(alias_key) = canonical.strip_prefix("aio/") else {
        return Ok(None);
    };
    if alias_key.is_empty() || canonical.len() != "aio/".len() + alias_key.len() {
        return Ok(None);
    }

    let conn = db.open_connection()?;
    if crate::shared::uuid::is_canonical_uuid_v4(alias_key) && alias_key.len() == 36 {
        return conn
            .query_row(
                r#"
SELECT model.model_uuid, model.provider_id, provider.provider_uuid, model.remote_model_id
FROM provider_models model
JOIN providers provider ON provider.id = model.provider_id
WHERE model.model_uuid = ?1
  AND provider.cli_key = 'codex'
  AND provider.source_provider_id IS NULL
  AND provider.bridge_type IS NULL
"#,
                params![alias_key],
                read_managed_model_binding,
            )
            .optional()
            .map_err(|error| db_err!("failed to resolve managed model UUID alias: {error}"));
    }
    if !crate::codex_managed_profiles::is_valid_profile_name_key(alias_key) {
        return Ok(None);
    }

    conn.query_row(
        r#"
SELECT model.model_uuid, model.provider_id, provider.provider_uuid, model.remote_model_id
FROM codex_managed_profiles profile
JOIN provider_models model ON model.model_uuid = profile.model_uuid
JOIN providers provider ON provider.id = model.provider_id
WHERE profile.profile_name_key = ?1
  AND provider.cli_key = 'codex'
  AND provider.source_provider_id IS NULL
  AND provider.bridge_type IS NULL
"#,
        params![alias_key],
        read_managed_model_binding,
    )
    .optional()
    .map_err(|error| db_err!("failed to resolve managed model profile alias: {error}"))
}

fn read_managed_model_binding(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedModelBinding> {
    Ok(ManagedModelBinding {
        model_uuid: row.get(0)?,
        provider_id: row.get(1)?,
        provider_uuid: row.get(2)?,
        remote_model_id: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::MutexGuard;
    use tauri::Manager as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct ModelTestApp {
        _lock: MutexGuard<'static, ()>,
        previous_home: Option<OsString>,
        previous_dotdir: Option<OsString>,
        _home: tempfile::TempDir,
        _app: tauri::App<tauri::test::MockRuntime>,
        db: db::Db,
    }

    impl ModelTestApp {
        fn new() -> Self {
            let lock = crate::test_support::test_env_lock();
            let home = tempfile::tempdir().expect("tempdir");
            let previous_home = std::env::var_os("AIO_CODING_HUB_HOME_DIR");
            let previous_dotdir = std::env::var_os("AIO_CODING_HUB_DOTDIR_NAME");
            std::env::set_var("AIO_CODING_HUB_HOME_DIR", home.path());
            std::env::set_var(
                "AIO_CODING_HUB_DOTDIR_NAME",
                ".aio-coding-hub-provider-model-test",
            );
            crate::test_support::clear_settings_cache();
            let app = tauri::test::mock_app();
            app.manage(crate::resident::ResidentState::default());
            let db = crate::db::init(app.handle()).expect("init db");
            Self {
                _lock: lock,
                previous_home,
                previous_dotdir,
                _home: home,
                _app: app,
                db,
            }
        }

        fn seed_provider(&self) -> i64 {
            let conn = self.db.open_connection().expect("open db");
            conn.execute(
                r#"
INSERT INTO providers(
  provider_uuid, cli_key, name, base_url, api_key_plaintext, created_at, updated_at
) VALUES (?1, 'codex', 'Model Test', 'https://example.invalid/v1', 'key', 1, 1)
"#,
                params![crate::shared::uuid::new_uuid_v4()],
            )
            .expect("insert provider");
            conn.last_insert_rowid()
        }

        fn handle(&self) -> tauri::AppHandle<tauri::test::MockRuntime> {
            self._app.handle().clone()
        }
    }

    impl Drop for ModelTestApp {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(value) => std::env::set_var("AIO_CODING_HUB_HOME_DIR", value),
                None => std::env::remove_var("AIO_CODING_HUB_HOME_DIR"),
            }
            match self.previous_dotdir.take() {
                Some(value) => std::env::set_var("AIO_CODING_HUB_DOTDIR_NAME", value),
                None => std::env::remove_var("AIO_CODING_HUB_DOTDIR_NAME"),
            }
            crate::test_support::clear_settings_cache();
        }
    }

    async fn spawn_raw_response_server(
        raw_response: &'static [u8],
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(raw_response)
                .await
                .expect("write response");
        });
        (address, server)
    }

    async fn response_from_raw(raw_response: &'static [u8]) -> reqwest::Response {
        let (address, server) = spawn_raw_response_server(raw_response).await;
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client")
            .get(format!("http://{address}"))
            .send()
            .await
            .expect("send request");
        server.await.expect("server task");
        response
    }

    #[test]
    fn models_url_treats_configured_path_as_api_base() {
        assert_eq!(
            build_models_url("https://example.invalid/v1")
                .expect("v1")
                .as_str(),
            "https://example.invalid/v1/models"
        );
        assert_eq!(
            build_models_url("https://example.invalid/v1beta/openai/")
                .expect("gemini compatible")
                .as_str(),
            "https://example.invalid/v1beta/openai/models"
        );
        assert_eq!(
            build_models_url("https://example.invalid")
                .expect("origin")
                .as_str(),
            "https://example.invalid/v1/models"
        );
    }

    #[test]
    fn models_url_rejects_userinfo() {
        assert_eq!(
            build_models_url("https://user:password@example.invalid/v1"),
            Err(DiscoveryErrorCode::Network)
        );
        assert_eq!(
            build_models_url("https://user@example.invalid/v1"),
            Err(DiscoveryErrorCode::Network)
        );
    }

    #[test]
    fn credential_errors_are_classified_without_exposing_diagnostics() {
        assert!(matches!(
            classify_credential_resolution_error(AppError::new(
                "OAUTH_REFRESH_FAILED",
                "token refresh request timed out for https://secret.invalid"
            )),
            DiscoveryAttemptError::Catalog(DiscoveryErrorCode::Timeout)
        ));
        assert!(matches!(
            classify_credential_resolution_error(AppError::new(
                "OAUTH_REFRESH_FAILED",
                "token endpoint error (401 Unauthorized): invalid_grant"
            )),
            DiscoveryAttemptError::Catalog(DiscoveryErrorCode::Unauthorized)
        ));
        assert!(matches!(
            classify_credential_resolution_error(AppError::new(
                "OAUTH_REFRESH_FAILED",
                "connection reset while using token SECRET_TOKEN"
            )),
            DiscoveryAttemptError::Catalog(DiscoveryErrorCode::Network)
        ));

        let internal = classify_credential_resolution_error(AppError::new(
            "DB_ERROR",
            "failed while reading SECRET_TOKEN",
        ));
        let DiscoveryAttemptError::Internal(error) = internal else {
            panic!("database failures must remain internal");
        };
        assert_eq!(error.code(), "DB_ERROR");
        assert!(!error.to_string().contains("SECRET_TOKEN"));

        let invalid = classify_credential_resolution_error(AppError::new(
            "SEC_INVALID_INPUT",
            "provider api_key is SECRET_TOKEN",
        ));
        let DiscoveryAttemptError::Internal(error) = invalid else {
            panic!("configuration failures must remain internal");
        };
        assert_eq!(error.code(), "PROVIDER_MODELS_INVALID_PROVIDER");
        assert!(!error.to_string().contains("SECRET_TOKEN"));
    }

    #[tokio::test]
    async fn discovery_body_distinguishes_limit_from_network_failure() {
        let oversized = response_from_raw(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2097153\r\nConnection: close\r\n\r\n",
        )
        .await;
        assert_eq!(
            read_discovery_body(oversized).await,
            Err(DiscoveryErrorCode::Limit)
        );

        let interrupted = response_from_raw(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabc",
        )
        .await;
        assert_eq!(
            read_discovery_body(interrupted).await,
            Err(DiscoveryErrorCode::Network)
        );

        let invalid_utf8 = response_from_raw(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"\xff\"}]}",
        )
        .await;
        assert_eq!(
            read_discovery_body(invalid_utf8).await,
            Err(DiscoveryErrorCode::InvalidResponse)
        );
    }

    #[tokio::test]
    async fn discovery_tries_next_base_url_after_malformed_success_response() {
        let (malformed_address, malformed_server) = spawn_raw_response_server(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"data\":\"not-an-array\"}",
        )
        .await;
        let (valid_address, valid_server) = spawn_raw_response_server(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"second-base-model\"}]}",
        )
        .await;
        let provider = CatalogProvider {
            provider_id: 1,
            provider_uuid: crate::shared::uuid::new_uuid_v4(),
            base_urls: vec![
                format!("http://{malformed_address}/v1"),
                format!("http://{valid_address}/v1"),
            ],
            auth_mode: "api_key".to_string(),
            oauth_provider_type: None,
        };
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(DISCOVERY_CONNECT_TIMEOUT)
            .timeout(DISCOVERY_TOTAL_TIMEOUT)
            .build()
            .expect("test client");

        let models = discover_from_provider_with_client(&client, &provider, "test-key")
            .await
            .expect("second Base URL should succeed");
        malformed_server.await.expect("malformed server task");
        valid_server.await.expect("valid server task");
        assert_eq!(models, vec!["second-base-model"]);
    }

    #[tokio::test]
    async fn oauth_discovery_does_not_replay_credentials_across_redirects() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let provider_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read provider")
            .provider_uuid;

        let sink_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind redirect sink");
        let sink_address = sink_listener.local_addr().expect("redirect sink address");
        let sink_request_seen = Arc::new(AtomicBool::new(false));
        let sink_request_seen_by_server = sink_request_seen.clone();
        let sink_server = tokio::spawn(async move {
            let Ok((mut stream, _)) = sink_listener.accept().await else {
                return;
            };
            sink_request_seen_by_server.store(true, Ordering::SeqCst);
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"error\":\"invalid_grant\",\"error_description\":\"refresh_token invalid\"}",
                )
                .await
                .expect("write redirect sink response");
        });

        let redirect_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind token endpoint");
        let redirect_address = redirect_listener
            .local_addr()
            .expect("token endpoint address");
        let redirect_body =
            r#"{"error":"invalid_grant","error_description":"refresh_token invalid"}"#;
        let redirect_response = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{sink_address}/stolen\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{redirect_body}",
            redirect_body.len()
        );
        let redirect_server = tokio::spawn(async move {
            let (mut stream, _) = redirect_listener
                .accept()
                .await
                .expect("accept token request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(redirect_response.as_bytes())
                .await
                .expect("write token redirect");
        });

        crate::providers::update_oauth_tokens(
            &test_app.db,
            provider_id,
            "oauth",
            "codex_oauth",
            "expired-access-token",
            Some("SYNTHETIC_SECRET_REFRESH_TOKEN"),
            None,
            &format!("http://{redirect_address}/oauth/token"),
            "client-id",
            Some("SYNTHETIC_SECRET_CLIENT_SECRET"),
            Some(now_unix_seconds().saturating_sub(1)),
            None,
        )
        .expect("seed expired OAuth credential");

        let catalog = refresh(&test_app.db, provider_id, &provider_uuid)
            .await
            .expect("redirect rejection is a catalog refresh failure");
        redirect_server.await.expect("token endpoint task");
        sink_server.abort();
        let _ = sink_server.await;

        assert_eq!(catalog.last_error_code.as_deref(), Some("unauthorized"));
        assert!(
            !sink_request_seen.load(Ordering::SeqCst),
            "OAuth credentials were replayed to the redirect target"
        );
    }

    #[test]
    fn openai_parser_is_strict_bounded_and_deduplicated() {
        let models = parse_openai_models(
            r#"{"object":"list","data":[{"id":"grok-4.5","owned_by":"xai"},{"id":"grok-4.5"},{"id":"gpt-5"}],"extra":true}"#,
        )
        .expect("parse");
        assert_eq!(models, vec!["grok-4.5", "gpt-5"]);
        assert_eq!(
            parse_openai_models(r#"{"data":[]}"#),
            Err(DiscoveryErrorCode::Empty)
        );
        assert_eq!(
            parse_openai_models(r#"{"data":[{"name":"missing-id"}]}"#),
            Err(DiscoveryErrorCode::InvalidResponse)
        );
    }

    #[test]
    fn model_capabilities_require_explicit_consistent_configuration() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let provider_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read provider")
            .provider_uuid;
        let catalog = manual_upsert(&test_app.db, provider_id, &provider_uuid, "grok-4.5")
            .expect("create model");
        let model = &catalog.models[0];
        assert!(!model.capabilities_configured);
        assert!(model.supported_reasoning_efforts.is_empty());
        assert_eq!(model.default_reasoning_effort, None);
        assert_eq!(model.context_window, None);

        let invalid_default = update_capabilities(
            &test_app.handle(),
            &test_app.db,
            provider_id,
            &provider_uuid,
            &model.model_uuid,
            &ProviderModelCapabilitiesInput {
                supported_reasoning_efforts: vec![ProviderModelReasoningEffort::Low],
                default_reasoning_effort: Some(ProviderModelReasoningEffort::High),
                context_window: Some(128_000),
            },
        )
        .expect_err("default outside supported set must fail");
        assert_eq!(invalid_default.code(), "SEC_INVALID_INPUT");

        let duplicate = update_capabilities(
            &test_app.handle(),
            &test_app.db,
            provider_id,
            &provider_uuid,
            &model.model_uuid,
            &ProviderModelCapabilitiesInput {
                supported_reasoning_efforts: vec![
                    ProviderModelReasoningEffort::Low,
                    ProviderModelReasoningEffort::Low,
                ],
                default_reasoning_effort: Some(ProviderModelReasoningEffort::Low),
                context_window: None,
            },
        )
        .expect_err("duplicate efforts must fail");
        assert_eq!(duplicate.code(), "SEC_INVALID_INPUT");

        let invalid_context = update_capabilities(
            &test_app.handle(),
            &test_app.db,
            provider_id,
            &provider_uuid,
            &model.model_uuid,
            &ProviderModelCapabilitiesInput {
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
                context_window: Some(MODEL_CONTEXT_WINDOW_MIN_TOKENS - 1),
            },
        )
        .expect_err("tiny context must fail");
        assert_eq!(invalid_context.code(), "SEC_INVALID_INPUT");

        let updated = update_capabilities(
            &test_app.handle(),
            &test_app.db,
            provider_id,
            &provider_uuid,
            &model.model_uuid,
            &ProviderModelCapabilitiesInput {
                supported_reasoning_efforts: vec![
                    ProviderModelReasoningEffort::Max,
                    ProviderModelReasoningEffort::Minimal,
                ],
                default_reasoning_effort: Some(ProviderModelReasoningEffort::Max),
                context_window: Some(1_000_000),
            },
        )
        .expect("configure model");
        let model = &updated.models[0];
        assert!(model.capabilities_configured);
        assert_eq!(
            model.supported_reasoning_efforts,
            vec![
                ProviderModelReasoningEffort::Minimal,
                ProviderModelReasoningEffort::Max,
            ]
        );
        assert_eq!(
            model.default_reasoning_effort,
            Some(ProviderModelReasoningEffort::Max)
        );
        assert_eq!(model.context_window, Some(1_000_000));

        let model_uuid = model.model_uuid.clone();
        let manual_result = manual_upsert(&test_app.db, provider_id, &provider_uuid, "grok-4.5")
            .expect("repeat manual upsert");
        let snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture refresh after capability update")
            .snapshot;
        assert!(record_refresh_success(
            &test_app.db,
            &snapshot,
            2_000_000,
            &["grok-4.5".to_string()]
        )
        .expect("repeat discovery refresh"));
        let refresh_result = get(&test_app.db, provider_id, &provider_uuid)
            .expect("read model after repeat refresh");
        for catalog in [manual_result, refresh_result] {
            let persisted = &catalog.models[0];
            assert!(persisted.capabilities_configured);
            assert_eq!(
                persisted.supported_reasoning_efforts,
                vec![
                    ProviderModelReasoningEffort::Minimal,
                    ProviderModelReasoningEffort::Max,
                ]
            );
            assert_eq!(
                persisted.default_reasoning_effort,
                Some(ProviderModelReasoningEffort::Max)
            );
            assert_eq!(persisted.context_window, Some(1_000_000));
        }

        let no_reasoning = update_capabilities(
            &test_app.handle(),
            &test_app.db,
            provider_id,
            &provider_uuid,
            &model_uuid,
            &ProviderModelCapabilitiesInput {
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
                context_window: None,
            },
        )
        .expect("explicit no-reasoning configuration");
        assert!(no_reasoning.models[0].capabilities_configured);
        assert!(no_reasoning.models[0]
            .supported_reasoning_efforts
            .is_empty());
        assert_eq!(no_reasoning.models[0].default_reasoning_effort, None);
        assert_eq!(no_reasoning.models[0].context_window, None);
    }

    #[test]
    fn refresh_failure_keeps_previous_models_and_marks_catalog_stale() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture snapshot")
            .snapshot;
        assert!(
            record_refresh_success(&test_app.db, &snapshot, 10, &["grok-4.5".to_string()])
                .expect("record success")
        );
        record_refresh_failure(
            &test_app.db,
            &snapshot,
            20,
            DiscoveryErrorCode::Unauthorized,
        )
        .expect("record failure");

        let catalog =
            get(&test_app.db, provider_id, &snapshot.provider_uuid).expect("read catalog");
        assert!(catalog.stale);
        assert_eq!(catalog.last_attempt_at, Some(20));
        assert_eq!(catalog.last_success_at, Some(10));
        assert_eq!(catalog.last_error_code.as_deref(), Some("unauthorized"));
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].remote_model_id, "grok-4.5");
    }

    #[test]
    fn stale_refresh_snapshot_cannot_overwrite_provider_edit() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture snapshot")
            .snapshot;
        assert!(
            record_refresh_success(&test_app.db, &snapshot, 10, &["old-model".to_string()])
                .expect("record initial success")
        );

        let mut conn = test_app.db.open_connection().expect("open db");
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("start edit transaction");
        tx.execute(
            "UPDATE providers SET base_url = ?1, base_urls_json = ?2, updated_at = 20 WHERE id = ?3",
            params![
                "https://changed.invalid/v1",
                r#"["https://changed.invalid/v1"]"#,
                provider_id
            ],
        )
        .expect("edit provider connection");
        tx.execute(
            "UPDATE provider_model_catalogs SET stale = 1 WHERE provider_id = ?1",
            params![provider_id],
        )
        .expect("mark catalog stale");
        tx.execute(
            "UPDATE provider_models SET stale = 1, updated_at = 20 WHERE provider_id = ?1 AND source = 'discovered'",
            params![provider_id],
        )
        .expect("mark models stale");
        tx.commit().expect("commit provider edit");

        assert!(!record_refresh_failure(
            &test_app.db,
            &snapshot,
            30,
            DiscoveryErrorCode::Unauthorized,
        )
        .expect("reject stale failure"));
        assert!(
            !record_refresh_success(&test_app.db, &snapshot, 40, &["new-model".to_string()],)
                .expect("reject stale success")
        );

        let catalog =
            get(&test_app.db, provider_id, &snapshot.provider_uuid).expect("read catalog");
        assert!(catalog.stale);
        assert_eq!(catalog.last_attempt_at, Some(10));
        assert_eq!(catalog.last_success_at, Some(10));
        assert_eq!(catalog.last_error_code, None);
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].remote_model_id, "old-model");
        assert!(catalog.models[0].stale);
    }

    #[test]
    fn oauth_refresh_uses_post_resolution_snapshot_for_commit() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        crate::providers::update_oauth_tokens(
            &test_app.db,
            provider_id,
            "oauth",
            "codex",
            "old-access",
            Some("old-refresh"),
            None,
            "https://auth.example.invalid/token",
            "client-id",
            Some("client-secret"),
            Some(10),
            None,
        )
        .expect("seed OAuth credential");
        let initial_snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture initial OAuth snapshot")
            .snapshot;
        assert!(record_refresh_success(
            &test_app.db,
            &initial_snapshot,
            10,
            &["old-model".to_string()]
        )
        .expect("record initial success"));

        crate::providers::update_oauth_tokens(
            &test_app.db,
            provider_id,
            "oauth",
            "codex",
            "new-access",
            Some("new-refresh"),
            None,
            "https://auth.example.invalid/token",
            "client-id",
            Some("client-secret"),
            Some(1000),
            None,
        )
        .expect("replace OAuth credential");

        let resolved_snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture resolved OAuth snapshot")
            .snapshot;
        assert!(initial_snapshot.same_discovery_target(&resolved_snapshot));
        assert!(!resolved_snapshot.matches_effective_credential("old-access"));
        assert!(resolved_snapshot.matches_effective_credential("new-access"));
        assert!(!record_refresh_success(
            &test_app.db,
            &initial_snapshot,
            30,
            &["stale-result".to_string()],
        )
        .expect("reject pre-resolution snapshot"));

        let stale_catalog = get(&test_app.db, provider_id, &resolved_snapshot.provider_uuid)
            .expect("read stale catalog");
        assert!(stale_catalog.stale);
        assert_eq!(stale_catalog.last_attempt_at, Some(10));
        assert_eq!(stale_catalog.models.len(), 1);
        assert_eq!(stale_catalog.models[0].remote_model_id, "old-model");

        assert!(record_refresh_success(
            &test_app.db,
            &resolved_snapshot,
            40,
            &["resolved-model".to_string()],
        )
        .expect("accept post-resolution snapshot"));
        let refreshed_catalog = get(&test_app.db, provider_id, &resolved_snapshot.provider_uuid)
            .expect("read refreshed catalog");
        assert!(!refreshed_catalog.stale);
        assert_eq!(refreshed_catalog.last_attempt_at, Some(40));
        assert!(refreshed_catalog
            .models
            .iter()
            .any(|model| model.remote_model_id == "resolved-model" && !model.stale));

        let auto_refreshed = crate::providers::update_oauth_tokens_if_last_refreshed_matches(
            &test_app.db,
            provider_id,
            "oauth",
            "codex",
            "auto-access",
            Some("auto-refresh"),
            None,
            "https://auth.example.invalid/token",
            "client-id",
            Some("client-secret"),
            Some(2000),
            None,
            resolved_snapshot.oauth_last_refreshed_at,
        )
        .expect("auto-refresh OAuth credential");
        assert!(auto_refreshed);
        let after_auto_refresh = get(&test_app.db, provider_id, &resolved_snapshot.provider_uuid)
            .expect("read auto-refreshed catalog");
        assert!(!after_auto_refresh.stale);
        assert_eq!(after_auto_refresh.last_attempt_at, Some(40));
        let auto_snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture auto-refreshed OAuth snapshot")
            .snapshot;
        assert!(resolved_snapshot.same_discovery_target(&auto_snapshot));
        assert!(auto_snapshot.matches_effective_credential("auto-access"));
    }

    #[test]
    fn post_network_outcomes_never_return_a_replacement_provider_catalog() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture initial snapshot")
            .snapshot;
        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
            .expect("delete initial provider");
        conn.execute(
            r#"
INSERT INTO providers(
  id, provider_uuid, cli_key, name, base_url, base_urls_json,
  api_key_plaintext, created_at, updated_at
) VALUES (?1, ?2, 'codex', 'Replacement', 'https://replacement.invalid/v1',
          '["https://replacement.invalid/v1"]', 'replacement-key', 2, 2)
"#,
            params![provider_id, replacement_uuid],
        )
        .expect("insert replacement provider");
        conn.execute(
            r#"
INSERT INTO provider_model_catalogs(
  provider_id, protocol, stale, last_attempt_at, last_success_at, last_error_code
) VALUES (?1, ?2, 1, NULL, NULL, NULL)
"#,
            params![provider_id, DISCOVERY_PROTOCOL],
        )
        .expect("insert replacement catalog");

        let outcomes = [
            Ok(vec!["stale-success".to_string()]),
            Err(DiscoveryErrorCode::Unauthorized),
            Err(DiscoveryErrorCode::Timeout),
        ];
        for (offset, outcome) in outcomes.into_iter().enumerate() {
            let error = finish_refresh_attempt(
                &test_app.db,
                provider_id,
                &snapshot,
                30 + offset as i64,
                outcome,
            )
            .expect_err("stale attempt must not return replacement catalog");
            assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");
        }

        let replacement_catalog =
            get(&test_app.db, provider_id, &replacement_uuid).expect("read replacement catalog");
        assert_eq!(replacement_catalog.provider_uuid, replacement_uuid);
        assert!(replacement_catalog.stale);
        assert_eq!(replacement_catalog.last_attempt_at, None);
        assert_eq!(replacement_catalog.last_success_at, None);
        assert_eq!(replacement_catalog.last_error_code, None);
        assert!(replacement_catalog.models.is_empty());
    }

    #[test]
    fn catalog_projection_rejects_a_reused_provider_id_for_expected_uuid() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let expected_provider_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read initial provider")
            .provider_uuid;
        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
            .expect("delete initial provider");
        conn.execute(
            r#"
INSERT INTO providers(
  id, provider_uuid, cli_key, name, base_url, base_urls_json,
  api_key_plaintext, created_at, updated_at
) VALUES (?1, ?2, 'codex', 'Replacement', 'https://replacement.invalid/v1',
          '["https://replacement.invalid/v1"]', 'replacement-key', 2, 2)
"#,
            params![provider_id, replacement_uuid],
        )
        .expect("insert replacement provider");
        drop(conn);

        let error = get(&test_app.db, provider_id, &expected_provider_uuid)
            .expect_err("stale expected UUID must not project replacement catalog");
        assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");
        let error = manual_upsert(
            &test_app.db,
            provider_id,
            &expected_provider_uuid,
            "stale-write",
        )
        .expect_err("stale expected UUID must not mutate replacement catalog");
        assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");
        let replacement_catalog =
            get(&test_app.db, provider_id, &replacement_uuid).expect("read replacement catalog");
        assert!(replacement_catalog.models.is_empty());
    }

    #[test]
    fn manual_catalog_mutations_roll_back_if_identity_changes_before_projection() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let expected_provider_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read initial provider")
            .provider_uuid;
        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute_batch(&format!(
            r#"
CREATE TRIGGER replace_provider_during_manual_insert
BEFORE INSERT ON provider_models
WHEN NEW.provider_id = {provider_id} AND NEW.source = 'manual'
BEGIN
  DELETE FROM providers WHERE id = NEW.provider_id;
  INSERT INTO providers(
    id, provider_uuid, cli_key, name, base_url, base_urls_json,
    api_key_plaintext, created_at, updated_at
  ) VALUES (
    NEW.provider_id, '{replacement_uuid}', 'codex', 'Replacement',
    'https://replacement.invalid/v1', '["https://replacement.invalid/v1"]',
    'replacement-key', 2, 2
  );
END;
"#
        ))
        .expect("install insert replacement trigger");
        drop(conn);

        let error = manual_upsert(
            &test_app.db,
            provider_id,
            &expected_provider_uuid,
            "grok-4.5",
        )
        .expect_err("manual insert must reject replacement identity");
        assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");
        let conn = test_app.db.open_connection().expect("open db after insert");
        conn.execute_batch("DROP TRIGGER replace_provider_during_manual_insert;")
            .expect("drop insert trigger");
        drop(conn);
        let catalog =
            get(&test_app.db, provider_id, &expected_provider_uuid).expect("read original catalog");
        assert_eq!(catalog.provider_uuid, expected_provider_uuid);
        assert!(catalog.models.is_empty());

        let catalog = manual_upsert(
            &test_app.db,
            provider_id,
            &expected_provider_uuid,
            "grok-4.5",
        )
        .expect("seed manual model");
        let model_uuid = catalog.models[0].model_uuid.clone();
        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute_batch(&format!(
            r#"
CREATE TRIGGER replace_provider_during_manual_delete
AFTER DELETE ON provider_models
WHEN OLD.model_uuid = '{model_uuid}'
BEGIN
  DELETE FROM providers WHERE id = OLD.provider_id;
  INSERT INTO providers(
    id, provider_uuid, cli_key, name, base_url, base_urls_json,
    api_key_plaintext, created_at, updated_at
  ) VALUES (
    OLD.provider_id, '{replacement_uuid}', 'codex', 'Replacement',
    'https://replacement.invalid/v1', '["https://replacement.invalid/v1"]',
    'replacement-key', 2, 2
  );
END;
"#
        ))
        .expect("install delete replacement trigger");
        drop(conn);

        let error = manual_delete(
            &test_app.db,
            provider_id,
            &expected_provider_uuid,
            &model_uuid,
        )
        .expect_err("manual delete must reject replacement identity");
        assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");
        let catalog = get(&test_app.db, provider_id, &expected_provider_uuid)
            .expect("read rolled back catalog");
        assert_eq!(catalog.provider_uuid, expected_provider_uuid);
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].model_uuid, model_uuid);
    }

    #[test]
    fn refresh_finish_rolls_back_if_identity_changes_after_apply() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let snapshot = capture_refresh_context(&test_app.db, provider_id)
            .expect("capture initial snapshot")
            .snapshot;
        assert!(
            record_refresh_success(&test_app.db, &snapshot, 10, &["old-model".to_string()],)
                .expect("record initial catalog")
        );

        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute_batch(&format!(
            r#"
CREATE TRIGGER replace_provider_after_refresh_apply
AFTER UPDATE OF last_attempt_at ON provider_model_catalogs
WHEN OLD.provider_id = {provider_id}
BEGIN
  DELETE FROM providers WHERE id = OLD.provider_id;
  INSERT INTO providers(
    id, provider_uuid, cli_key, name, base_url, base_urls_json,
    api_key_plaintext, created_at, updated_at
  ) VALUES (
    OLD.provider_id, '{replacement_uuid}', 'codex', 'Replacement',
    'https://replacement.invalid/v1', '["https://replacement.invalid/v1"]',
    'replacement-key', 2, 2
  );
END;
"#
        ))
        .expect("install refresh replacement trigger");
        drop(conn);

        for result in [
            Err(DiscoveryErrorCode::Unauthorized),
            Ok(vec!["new-model".to_string()]),
        ] {
            let error = finish_refresh_attempt(&test_app.db, provider_id, &snapshot, 20, result)
                .expect_err("refresh finish must reject replacement identity");
            assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");

            let catalog = get(&test_app.db, provider_id, &snapshot.provider_uuid)
                .expect("read rolled back catalog");
            assert_eq!(catalog.provider_uuid, snapshot.provider_uuid);
            assert_eq!(catalog.last_attempt_at, Some(10));
            assert_eq!(catalog.last_success_at, Some(10));
            assert_eq!(catalog.last_error_code, None);
            assert_eq!(catalog.models.len(), 1);
            assert_eq!(catalog.models[0].remote_model_id, "old-model");
            assert!(!catalog.models[0].stale);
        }
    }

    #[test]
    fn catalog_operations_work_with_a_single_connection_pool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("provider_models_single_connection.db");
        let db = crate::db::init_for_tests(&db_path).expect("init single-connection db");
        let conn = db.open_connection().expect("open db");
        let provider_uuid = crate::shared::uuid::new_uuid_v4();
        conn.execute(
            r#"
INSERT INTO providers(
  provider_uuid, cli_key, name, base_url, api_key_plaintext, created_at, updated_at
) VALUES (?1, 'codex', 'Single Connection', 'https://example.invalid/v1', 'key', 1, 1)
"#,
            params![provider_uuid],
        )
        .expect("insert provider");
        let provider_id = conn.last_insert_rowid();
        drop(conn);

        let empty = get(&db, provider_id, &provider_uuid).expect("read empty catalog");
        assert_eq!(empty.provider_uuid, provider_uuid);
        let created = manual_upsert(&db, provider_id, &provider_uuid, "grok-4.5")
            .expect("insert manual model");
        let model_uuid = created.models[0].model_uuid.clone();
        let deleted = manual_delete(&db, provider_id, &provider_uuid, &model_uuid)
            .expect("delete manual model");
        assert!(deleted.models.is_empty());

        let snapshot = capture_refresh_context(&db, provider_id)
            .expect("capture refresh snapshot")
            .snapshot;
        let refreshed = finish_refresh_attempt(
            &db,
            provider_id,
            &snapshot,
            20,
            Ok(vec!["discovered-model".to_string()]),
        )
        .expect("finish refresh");
        assert_eq!(refreshed.provider_uuid, provider_uuid);
        assert_eq!(refreshed.models.len(), 1);
        assert_eq!(refreshed.models[0].remote_model_id, "discovered-model");
    }

    #[test]
    fn managed_profile_alias_and_legacy_uuid_alias_resolve_same_binding() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let provider_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read provider")
            .provider_uuid;
        let catalog = manual_upsert(&test_app.db, provider_id, &provider_uuid, "grok-4.5")
            .expect("create model");
        let model_uuid = catalog.models[0].model_uuid.clone();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute(
            r#"
INSERT INTO codex_managed_profiles(
  profile_uuid, profile_name, profile_name_key, model_uuid,
  codex_home_path, content_sha256, created_at, updated_at
) VALUES (?1, 'Grok', 'grok', ?2, 'C:\codex', ?3, 1, 1)
"#,
            params![
                crate::shared::uuid::new_uuid_v4(),
                model_uuid,
                "a".repeat(64)
            ],
        )
        .expect("insert profile");

        let readable = resolve_managed_model_alias(&test_app.db, "aio/grok")
            .expect("resolve readable alias")
            .expect("readable binding");
        let legacy = resolve_managed_model_alias(&test_app.db, &format!("aio/{model_uuid}"))
            .expect("resolve legacy alias")
            .expect("legacy binding");
        assert_eq!(readable, legacy);
        assert_eq!(readable.provider_id, provider_id);
        assert_eq!(readable.remote_model_id, "grok-4.5");
        assert!(resolve_managed_model_alias(&test_app.db, "aio/Grok")
            .expect("reject non-canonical profile key")
            .is_none());
    }

    #[test]
    fn managed_binding_identity_rejects_a_reused_provider_id() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let initial_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read initial provider")
            .provider_uuid;
        let catalog = manual_upsert(&test_app.db, provider_id, &initial_uuid, "grok-4.5")
            .expect("create managed model binding");
        let model_uuid = catalog.models[0].model_uuid.clone();
        let binding = resolve_managed_model_alias(&test_app.db, &format!("aio/{model_uuid}"))
            .expect("resolve alias")
            .expect("binding exists");
        assert_eq!(binding.provider_id, provider_id);
        assert_eq!(binding.provider_uuid, initial_uuid);

        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let conn = test_app.db.open_connection().expect("open db");
        conn.execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
            .expect("delete initial provider");
        conn.execute(
            r#"
INSERT INTO providers(
  id, provider_uuid, cli_key, name, base_url, base_urls_json,
  api_key_plaintext, enabled, created_at, updated_at
) VALUES (?1, ?2, 'codex', 'Replacement', 'https://replacement.invalid/v1',
          '["https://replacement.invalid/v1"]', 'replacement-key', 1, 2, 2)
"#,
            params![provider_id, replacement_uuid],
        )
        .expect("insert replacement provider with reused id");

        assert!(
            crate::providers::get_enabled_direct_codex_for_gateway_by_identity(
                &test_app.db,
                binding.provider_id,
                &binding.provider_uuid,
            )
            .expect("load by stable identity")
            .is_none()
        );
    }

    #[tokio::test]
    async fn refresh_does_not_follow_a_reused_provider_id() {
        let test_app = ModelTestApp::new();
        let provider_id = test_app.seed_provider();
        let initial_uuid = direct_codex_provider_metadata(&test_app.db, provider_id)
            .expect("read initial provider")
            .provider_uuid;
        let refresh_mutex = refresh_lock(&initial_uuid);
        let held_guard = refresh_mutex.clone().lock_owned().await;
        let initial_strong_count = Arc::strong_count(&refresh_mutex);

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind replacement provider server");
        let replacement_url = format!(
            "http://{}/v1",
            listener.local_addr().expect("replacement server address")
        );
        let request_seen = Arc::new(AtomicBool::new(false));
        let server_request_seen = request_seen.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            server_request_seen.store(true, Ordering::SeqCst);
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"replacement-model\"}]}",
                )
                .await
                .expect("write replacement response");
        });

        let refresh_db = test_app.db.clone();
        let refresh_expected_uuid = initial_uuid.clone();
        let refresh_task =
            tokio::spawn(
                async move { refresh(&refresh_db, provider_id, &refresh_expected_uuid).await },
            );
        tokio::time::timeout(Duration::from_secs(2), async {
            while Arc::strong_count(&refresh_mutex) <= initial_strong_count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("refresh should wait on initial provider UUID lock");

        let replacement_uuid = crate::shared::uuid::new_uuid_v4();
        let replacement_urls_json =
            serde_json::to_string(&vec![replacement_url.clone()]).expect("serialize URLs");
        let mut conn = test_app.db.open_connection().expect("open db");
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("start provider replacement");
        tx.execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
            .expect("delete initial provider");
        tx.execute(
            r#"
INSERT INTO providers(
  id, provider_uuid, cli_key, name, base_url, base_urls_json,
  api_key_plaintext, created_at, updated_at
) VALUES (?1, ?2, 'codex', 'Replacement', ?3, ?4, 'replacement-key', 2, 2)
"#,
            params![
                provider_id,
                replacement_uuid,
                replacement_url,
                replacement_urls_json
            ],
        )
        .expect("insert replacement provider");
        tx.execute(
            r#"
INSERT INTO provider_model_catalogs(
  provider_id, protocol, stale, last_attempt_at, last_success_at, last_error_code
) VALUES (?1, ?2, 1, NULL, NULL, NULL)
"#,
            params![provider_id, DISCOVERY_PROTOCOL],
        )
        .expect("insert replacement catalog");
        tx.commit().expect("commit provider replacement");

        drop(held_guard);
        let error = tokio::time::timeout(Duration::from_secs(3), refresh_task)
            .await
            .expect("refresh should stop before network")
            .expect("refresh task")
            .expect_err("reused provider identity must fail");
        server.abort();
        let _ = server.await;

        assert_eq!(error.code(), "PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED");
        assert!(!request_seen.load(Ordering::SeqCst));
        let catalog =
            get(&test_app.db, provider_id, &replacement_uuid).expect("read replacement catalog");
        assert_eq!(catalog.provider_uuid, replacement_uuid);
        assert!(catalog.stale);
        assert_eq!(catalog.last_attempt_at, None);
        assert_eq!(catalog.last_success_at, None);
        assert_eq!(catalog.last_error_code, None);
        assert!(catalog.models.is_empty());
    }
}
