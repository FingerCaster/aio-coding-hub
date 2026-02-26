//! Usage: OAuth account persistence and token lifecycle helpers.

#![allow(dead_code)]

use crate::shared::error::db_err;
use crate::shared::time::now_unix_seconds;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const STATUS_ACTIVE: &str = "active";
const STATUS_QUOTA_COOLDOWN: &str = "quota_cooldown";
const STATUS_DISABLED: &str = "disabled";
const STATUS_EXPIRED: &str = "expired";
const STATUS_ERROR: &str = "error";
const MAX_LABEL_LEN: usize = 120;
const MAX_PROVIDER_TYPE_LEN: usize = 64;
const DEFAULT_REFRESH_LEAD_S: i64 = 3600;
const MAX_REFRESH_LEAD_S: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OAuthAccountStatus {
    Active,
    QuotaCooldown,
    Disabled,
    Expired,
    Error,
}

impl OAuthAccountStatus {
    pub(crate) fn parse_lossy(raw: &str) -> Self {
        match raw.trim() {
            STATUS_ACTIVE => Self::Active,
            STATUS_QUOTA_COOLDOWN => Self::QuotaCooldown,
            STATUS_DISABLED => Self::Disabled,
            STATUS_EXPIRED => Self::Expired,
            STATUS_ERROR => Self::Error,
            _ => Self::Active,
        }
    }

    pub(crate) fn parse_strict(raw: &str) -> Option<Self> {
        match raw.trim() {
            STATUS_ACTIVE => Some(Self::Active),
            STATUS_QUOTA_COOLDOWN => Some(Self::QuotaCooldown),
            STATUS_DISABLED => Some(Self::Disabled),
            STATUS_EXPIRED => Some(Self::Expired),
            STATUS_ERROR => Some(Self::Error),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => STATUS_ACTIVE,
            Self::QuotaCooldown => STATUS_QUOTA_COOLDOWN,
            Self::Disabled => STATUS_DISABLED,
            Self::Expired => STATUS_EXPIRED,
            Self::Error => STATUS_ERROR,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthAccount {
    pub id: i64,
    pub cli_key: String,
    pub label: String,
    pub email: Option<String>,
    pub provider_type: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub token_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub expires_at: Option<i64>,
    pub refresh_lead_s: i64,
    pub status: OAuthAccountStatus,
    pub last_error: Option<String>,
    pub last_refreshed_at: Option<i64>,
    pub quota_exceeded: bool,
    pub quota_recover_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthAccountSummary {
    pub id: i64,
    pub cli_key: String,
    pub label: String,
    pub email: Option<String>,
    pub provider_type: String,
    pub expires_at: Option<i64>,
    pub refresh_lead_s: i64,
    pub status: OAuthAccountStatus,
    pub last_error: Option<String>,
    pub last_refreshed_at: Option<i64>,
    pub quota_exceeded: bool,
    pub quota_recover_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct OAuthAccountForGateway {
    pub id: i64,
    pub cli_key: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub token_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub expires_at: Option<i64>,
    pub refresh_lead_s: i64,
    pub status: OAuthAccountStatus,
    pub quota_exceeded: bool,
    pub quota_recover_at: Option<i64>,
    pub last_refreshed_at: Option<i64>,
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn normalize_label(input: &str) -> crate::shared::error::AppResult<String> {
    let value = input.trim();
    if value.is_empty() {
        return Err("SEC_INVALID_INPUT: label is required".to_string().into());
    }
    if value.len() > MAX_LABEL_LEN {
        return Err(format!("SEC_INVALID_INPUT: label must be <= {MAX_LABEL_LEN} chars").into());
    }
    Ok(value.to_string())
}

fn normalize_provider_type(input: &str) -> crate::shared::error::AppResult<String> {
    let value = input.trim();
    if value.is_empty() {
        return Err("SEC_INVALID_INPUT: provider_type is required"
            .to_string()
            .into());
    }
    if value.len() > MAX_PROVIDER_TYPE_LEN {
        return Err(format!(
            "SEC_INVALID_INPUT: provider_type must be <= {MAX_PROVIDER_TYPE_LEN} chars"
        )
        .into());
    }
    Ok(value.to_string())
}

fn normalize_refresh_lead_seconds(value: Option<i64>) -> crate::shared::error::AppResult<i64> {
    let normalized = value.unwrap_or(DEFAULT_REFRESH_LEAD_S);
    if !(0..=MAX_REFRESH_LEAD_S).contains(&normalized) {
        return Err(format!(
            "SEC_INVALID_INPUT: refresh_lead_s must be within [0, {MAX_REFRESH_LEAD_S}]"
        )
        .into());
    }
    Ok(normalized)
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> Result<OAuthAccountSummary, rusqlite::Error> {
    let status_raw: String = row.get("status")?;
    Ok(OAuthAccountSummary {
        id: row.get("id")?,
        cli_key: row.get("cli_key")?,
        label: row.get("label")?,
        email: row.get("email")?,
        provider_type: row.get("provider_type")?,
        expires_at: row.get("expires_at")?,
        refresh_lead_s: row.get("refresh_lead_s")?,
        status: OAuthAccountStatus::parse_lossy(&status_raw),
        last_error: row.get("last_error")?,
        last_refreshed_at: row.get("last_refreshed_at")?,
        quota_exceeded: row.get::<_, i64>("quota_exceeded")? != 0,
        quota_recover_at: row.get("quota_recover_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_full(row: &rusqlite::Row<'_>) -> Result<OAuthAccount, rusqlite::Error> {
    let status_raw: String = row.get("status")?;
    Ok(OAuthAccount {
        id: row.get("id")?,
        cli_key: row.get("cli_key")?,
        label: row.get("label")?,
        email: row.get("email")?,
        provider_type: row.get("provider_type")?,
        access_token: row.get("access_token")?,
        refresh_token: row.get("refresh_token")?,
        id_token: row.get("id_token")?,
        token_uri: row.get("token_uri")?,
        client_id: row.get("client_id")?,
        client_secret: row.get("client_secret")?,
        expires_at: row.get("expires_at")?,
        refresh_lead_s: row.get("refresh_lead_s")?,
        status: OAuthAccountStatus::parse_lossy(&status_raw),
        last_error: row.get("last_error")?,
        last_refreshed_at: row.get("last_refreshed_at")?,
        quota_exceeded: row.get::<_, i64>("quota_exceeded")? != 0,
        quota_recover_at: row.get("quota_recover_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_gateway(row: &rusqlite::Row<'_>) -> Result<OAuthAccountForGateway, rusqlite::Error> {
    let status_raw: String = row.get("status")?;
    Ok(OAuthAccountForGateway {
        id: row.get("id")?,
        cli_key: row.get("cli_key")?,
        access_token: row.get("access_token")?,
        refresh_token: row.get("refresh_token")?,
        id_token: row.get("id_token")?,
        token_uri: row.get("token_uri")?,
        client_id: row.get("client_id")?,
        client_secret: row.get("client_secret")?,
        expires_at: row.get("expires_at")?,
        refresh_lead_s: row.get("refresh_lead_s")?,
        status: OAuthAccountStatus::parse_lossy(&status_raw),
        quota_exceeded: row.get::<_, i64>("quota_exceeded")? != 0,
        quota_recover_at: row.get("quota_recover_at")?,
        last_refreshed_at: row.get("last_refreshed_at")?,
    })
}

pub(crate) fn list_by_cli(
    conn: &Connection,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<OAuthAccountSummary>> {
    crate::shared::cli_key::validate_cli_key(cli_key)?;

    let mut stmt = conn
        .prepare(
            r#"
SELECT
  id,
  cli_key,
  label,
  email,
  provider_type,
  expires_at,
  refresh_lead_s,
  status,
  last_error,
  last_refreshed_at,
  quota_exceeded,
  quota_recover_at,
  created_at,
  updated_at
FROM oauth_accounts
WHERE cli_key = ?1
ORDER BY updated_at DESC, id DESC
"#,
        )
        .map_err(|e| db_err!("failed to prepare oauth account list query: {e}"))?;

    let rows = stmt
        .query_map(params![cli_key], row_to_summary)
        .map_err(|e| db_err!("failed to list oauth accounts: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read oauth account row: {e}"))?);
    }
    Ok(items)
}

pub(crate) fn get_by_id(
    conn: &Connection,
    id: i64,
) -> crate::shared::error::AppResult<OAuthAccount> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }

    conn.query_row(
        r#"
SELECT
  id,
  cli_key,
  label,
  email,
  provider_type,
  access_token,
  refresh_token,
  id_token,
  token_uri,
  client_id,
  client_secret,
  expires_at,
  refresh_lead_s,
  status,
  last_error,
  last_refreshed_at,
  quota_exceeded,
  quota_recover_at,
  created_at,
  updated_at
FROM oauth_accounts
WHERE id = ?1
"#,
        params![id],
        row_to_full,
    )
    .optional()
    .map_err(|e| db_err!("failed to query oauth account by id: {e}"))?
    .ok_or_else(|| "DB_NOT_FOUND: oauth account not found".to_string().into())
}

pub(crate) fn get_for_gateway(
    conn: &Connection,
    id: i64,
) -> crate::shared::error::AppResult<OAuthAccountForGateway> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }

    conn.query_row(
        r#"
SELECT
  id,
  cli_key,
  access_token,
  refresh_token,
  id_token,
  token_uri,
  client_id,
  client_secret,
  expires_at,
  refresh_lead_s,
  status,
  quota_exceeded,
  quota_recover_at,
  last_refreshed_at
FROM oauth_accounts
WHERE id = ?1
"#,
        params![id],
        row_to_gateway,
    )
    .optional()
    .map_err(|e| db_err!("failed to query oauth gateway account by id: {e}"))?
    .ok_or_else(|| "DB_NOT_FOUND: oauth account not found".to_string().into())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn upsert(
    conn: &Connection,
    account_id: Option<i64>,
    cli_key: &str,
    label: &str,
    email: Option<&str>,
    provider_type: &str,
    access_token: Option<&str>,
    refresh_token: Option<&str>,
    id_token: Option<&str>,
    token_uri: Option<&str>,
    client_id: Option<&str>,
    client_secret: Option<&str>,
    expires_at: Option<i64>,
    refresh_lead_s: Option<i64>,
    last_refreshed_at: Option<i64>,
    status: Option<&str>,
) -> crate::shared::error::AppResult<OAuthAccountSummary> {
    let cli_key = cli_key.trim();
    crate::shared::cli_key::validate_cli_key(cli_key)?;

    let label = normalize_label(label)?;
    let provider_type = normalize_provider_type(provider_type)?;
    let email = normalize_optional_text(email);
    let refresh_token = normalize_optional_text(refresh_token);
    let id_token = normalize_optional_text(id_token);
    let token_uri = normalize_optional_text(token_uri);
    let client_id = normalize_optional_text(client_id);
    let client_secret = normalize_optional_text(client_secret);
    let refresh_lead_s = normalize_refresh_lead_seconds(refresh_lead_s)?;
    let last_refreshed_at = match last_refreshed_at {
        Some(value) => {
            if value <= 0 {
                return Err("SEC_INVALID_INPUT: last_refreshed_at must be > 0"
                    .to_string()
                    .into());
            }
            Some(value)
        }
        None => None,
    };
    let status = match status {
        Some(raw) => Some(
            OAuthAccountStatus::parse_strict(raw)
                .ok_or_else(|| "SEC_INVALID_INPUT: invalid oauth account status".to_string())?,
        ),
        None => None,
    };

    let now = now_unix_seconds();

    match account_id {
        None => {
            let access_token = normalize_optional_text(access_token)
                .ok_or_else(|| "SEC_INVALID_INPUT: access_token is required".to_string())?;

            conn.execute(
                r#"
INSERT INTO oauth_accounts(
  cli_key,
  label,
  email,
  provider_type,
  access_token,
  refresh_token,
  id_token,
  token_uri,
  client_id,
  client_secret,
  expires_at,
  refresh_lead_s,
  status,
  last_error,
  last_refreshed_at,
  quota_exceeded,
  quota_recover_at,
  created_at,
  updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL, ?14, 0, NULL, ?15, ?15)
"#,
                params![
                    cli_key,
                    label,
                    email,
                    provider_type,
                    access_token,
                    refresh_token,
                    id_token,
                    token_uri,
                    client_id,
                    client_secret,
                    expires_at,
                    refresh_lead_s,
                    status.unwrap_or(OAuthAccountStatus::Active).as_str(),
                    last_refreshed_at,
                    now
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    crate::shared::error::AppError::new(
                        "DB_CONSTRAINT",
                        format!(
                            "oauth account already exists for cli_key={cli_key}, label={label}"
                        ),
                    )
                }
                other => db_err!("failed to insert oauth account: {other}"),
            })?;

            let id = conn.last_insert_rowid();
            get_summary_by_id(conn, id)
        }
        Some(id) => {
            if id <= 0 {
                return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
            }

            let existing: Option<OAuthAccount> = conn
                .query_row(
                    r#"
SELECT
  id,
  cli_key,
  label,
  email,
  provider_type,
  access_token,
  refresh_token,
  id_token,
  token_uri,
  client_id,
  client_secret,
  expires_at,
  refresh_lead_s,
  status,
  last_error,
  last_refreshed_at,
  quota_exceeded,
  quota_recover_at,
  created_at,
  updated_at
FROM oauth_accounts
WHERE id = ?1
"#,
                    params![id],
                    row_to_full,
                )
                .optional()
                .map_err(|e| db_err!("failed to query oauth account before update: {e}"))?;

            let Some(existing) = existing else {
                return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
            };
            if existing.cli_key != cli_key {
                return Err("SEC_INVALID_INPUT: cli_key mismatch".to_string().into());
            }

            let access_token = normalize_optional_text(access_token)
                .unwrap_or_else(|| existing.access_token.clone());

            conn.execute(
                r#"
UPDATE oauth_accounts
SET
  label = ?1,
  email = ?2,
  provider_type = ?3,
  access_token = ?4,
  refresh_token = ?5,
  id_token = ?6,
  token_uri = ?7,
  client_id = ?8,
  client_secret = ?9,
  expires_at = ?10,
  refresh_lead_s = ?11,
  status = ?12,
  last_refreshed_at = COALESCE(?13, last_refreshed_at),
  updated_at = ?14
WHERE id = ?15
"#,
                params![
                    label,
                    email,
                    provider_type,
                    access_token,
                    refresh_token.or(existing.refresh_token),
                    id_token.or(existing.id_token),
                    token_uri.or(existing.token_uri),
                    client_id.or(existing.client_id),
                    client_secret.or(existing.client_secret),
                    expires_at.or(existing.expires_at),
                    refresh_lead_s,
                    status.unwrap_or(existing.status).as_str(),
                    last_refreshed_at,
                    now,
                    id
                ],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    crate::shared::error::AppError::new(
                        "DB_CONSTRAINT",
                        format!(
                            "oauth account already exists for cli_key={cli_key}, label={label}"
                        ),
                    )
                }
                other => db_err!("failed to update oauth account: {other}"),
            })?;

            get_summary_by_id(conn, id)
        }
    }
}

pub(crate) fn delete(conn: &Connection, id: i64) -> crate::shared::error::AppResult<()> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }
    let affected = conn
        .execute("DELETE FROM oauth_accounts WHERE id = ?1", params![id])
        .map_err(|e| db_err!("failed to delete oauth account: {e}"))?;
    if affected == 0 {
        return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
    }
    Ok(())
}

pub(crate) fn update_tokens(
    conn: &Connection,
    id: i64,
    access_token: &str,
    id_token: Option<&str>,
    expires_at: Option<i64>,
    refresh_token: Option<&str>,
) -> crate::shared::error::AppResult<()> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }
    let access_token = access_token.trim();
    if access_token.is_empty() {
        return Err("SEC_INVALID_INPUT: access_token is required"
            .to_string()
            .into());
    }
    let id_token = normalize_optional_text(id_token);
    let refresh_token = normalize_optional_text(refresh_token);

    let now = now_unix_seconds();
    let affected = conn
        .execute(
            r#"
UPDATE oauth_accounts
SET
  access_token = ?1,
  expires_at = ?2,
  id_token = COALESCE(?3, id_token),
  refresh_token = COALESCE(?4, refresh_token),
  status = 'active',
  last_error = NULL,
  refresh_success_count = COALESCE(refresh_success_count, 0) + 1,
  last_refreshed_at = ?5,
  updated_at = ?5
WHERE id = ?6
"#,
            params![access_token, expires_at, id_token, refresh_token, now, id],
        )
        .map_err(|e| db_err!("failed to update oauth account tokens: {e}"))?;
    if affected == 0 {
        return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
    }
    Ok(())
}

pub(crate) fn mark_status(
    conn: &Connection,
    id: i64,
    status: OAuthAccountStatus,
    error: Option<&str>,
) -> crate::shared::error::AppResult<()> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }
    let now = now_unix_seconds();
    let affected = conn
        .execute(
            r#"
UPDATE oauth_accounts
SET
  status = ?1,
  last_error = ?2,
  updated_at = ?3
WHERE id = ?4
"#,
            params![status.as_str(), normalize_optional_text(error), now, id],
        )
        .map_err(|e| db_err!("failed to mark oauth account status: {e}"))?;
    if affected == 0 {
        return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
    }
    Ok(())
}

pub(crate) fn record_refresh_failure(
    conn: &Connection,
    id: i64,
    error: Option<&str>,
) -> crate::shared::error::AppResult<()> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }
    let now = now_unix_seconds();
    let affected = conn
        .execute(
            r#"
UPDATE oauth_accounts
SET
  last_error = ?1,
  refresh_failure_count = COALESCE(refresh_failure_count, 0) + 1,
  last_refreshed_at = ?2,
  updated_at = ?2
WHERE id = ?3
"#,
            params![normalize_optional_text(error), now, id],
        )
        .map_err(|e| db_err!("failed to record oauth refresh failure: {e}"))?;
    if affected == 0 {
        return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
    }
    Ok(())
}

pub(crate) fn mark_quota_exceeded(
    conn: &Connection,
    id: i64,
    recover_at: i64,
) -> crate::shared::error::AppResult<()> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }
    let now = now_unix_seconds();
    let affected = conn
        .execute(
            r#"
UPDATE oauth_accounts
SET
  quota_exceeded = 1,
  quota_recover_at = ?1,
  status = ?2,
  updated_at = ?3
WHERE id = ?4
"#,
            params![
                recover_at,
                OAuthAccountStatus::QuotaCooldown.as_str(),
                now,
                id
            ],
        )
        .map_err(|e| db_err!("failed to mark oauth account quota exceeded: {e}"))?;
    if affected == 0 {
        return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
    }
    Ok(())
}

pub(crate) fn clear_quota(conn: &Connection, id: i64) -> crate::shared::error::AppResult<bool> {
    if id <= 0 {
        return Err(format!("SEC_INVALID_INPUT: invalid oauth account id={id}").into());
    }
    let now = now_unix_seconds();
    let affected = conn
        .execute(
            r#"
UPDATE oauth_accounts
SET
  quota_exceeded = 0,
  quota_recover_at = NULL,
  status = CASE
    WHEN status = ?2 THEN ?3
    ELSE status
  END,
  updated_at = ?1
WHERE id = ?4
  AND quota_exceeded = 1
  AND (quota_recover_at IS NULL OR quota_recover_at <= ?1)
"#,
            params![
                now,
                OAuthAccountStatus::QuotaCooldown.as_str(),
                OAuthAccountStatus::Active.as_str(),
                id
            ],
        )
        .map_err(|e| db_err!("failed to clear oauth account quota: {e}"))?;
    if affected == 0 {
        let exists = conn
            .query_row(
                "SELECT id FROM oauth_accounts WHERE id = ?1",
                params![id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|e| db_err!("failed to query oauth account existence: {e}"))?;
        if exists.is_none() {
            return Err("DB_NOT_FOUND: oauth account not found".to_string().into());
        }
        return Ok(false);
    }
    Ok(true)
}

#[allow(dead_code)]
pub(crate) fn list_active_for_cli(
    conn: &Connection,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<OAuthAccountForGateway>> {
    crate::shared::cli_key::validate_cli_key(cli_key)?;
    let mut stmt = conn
        .prepare(
            r#"
SELECT
  id,
  cli_key,
  access_token,
  refresh_token,
  id_token,
  token_uri,
  client_id,
  client_secret,
  expires_at,
  refresh_lead_s,
  status,
  quota_exceeded,
  quota_recover_at,
  last_refreshed_at
FROM oauth_accounts
WHERE cli_key = ?1
  AND status = 'active'
  AND quota_exceeded = 0
ORDER BY updated_at DESC, id DESC
"#,
        )
        .map_err(|e| db_err!("failed to prepare oauth active list query: {e}"))?;
    let rows = stmt
        .query_map(params![cli_key], row_to_gateway)
        .map_err(|e| db_err!("failed to list active oauth accounts: {e}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read oauth active row: {e}"))?);
    }
    Ok(items)
}

pub(crate) fn list_needing_refresh(
    conn: &Connection,
    now_unix: i64,
    limit: usize,
) -> crate::shared::error::AppResult<Vec<OAuthAccountForGateway>> {
    let limit = limit.max(1) as i64;
    let mut stmt = conn
        .prepare(
            r#"
SELECT
  id,
  cli_key,
  access_token,
  refresh_token,
  id_token,
  token_uri,
  client_id,
  client_secret,
  expires_at,
  refresh_lead_s,
  status,
  quota_exceeded,
  quota_recover_at,
  last_refreshed_at
FROM oauth_accounts
WHERE status IN ('active', 'error')
  AND quota_exceeded = 0
  AND refresh_token IS NOT NULL
  AND token_uri IS NOT NULL
  AND expires_at IS NOT NULL
  AND (expires_at - refresh_lead_s) <= ?1
ORDER BY expires_at ASC, id ASC
LIMIT ?2
"#,
        )
        .map_err(|e| db_err!("failed to prepare oauth refresh list query: {e}"))?;
    let rows = stmt
        .query_map(params![now_unix, limit], row_to_gateway)
        .map_err(|e| db_err!("failed to list oauth accounts needing refresh: {e}"))?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| db_err!("failed to read oauth refresh row: {e}"))?);
    }
    Ok(items)
}

pub(crate) fn list_expired_quotas(
    conn: &Connection,
    now_unix: i64,
    limit: usize,
) -> crate::shared::error::AppResult<Vec<i64>> {
    let limit = limit.max(1) as i64;
    let mut stmt = conn
        .prepare(
            r#"
SELECT id
FROM oauth_accounts
WHERE quota_exceeded = 1
  AND quota_recover_at IS NOT NULL
  AND quota_recover_at <= ?1
ORDER BY quota_recover_at ASC, id ASC
LIMIT ?2
"#,
        )
        .map_err(|e| db_err!("failed to prepare expired quota list query: {e}"))?;
    let rows = stmt
        .query_map(params![now_unix, limit], |row| row.get::<_, i64>(0))
        .map_err(|e| db_err!("failed to query expired quota rows: {e}"))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|e| db_err!("failed to read expired quota row: {e}"))?);
    }
    Ok(ids)
}

pub(crate) fn list_quota_exceeded_account_ids_for_cli(
    conn: &Connection,
    cli_key: &str,
    now_unix: i64,
) -> crate::shared::error::AppResult<std::collections::HashSet<i64>> {
    crate::shared::cli_key::validate_cli_key(cli_key)?;
    let mut stmt = conn
        .prepare(
            r#"
SELECT id
FROM oauth_accounts
WHERE cli_key = ?1
  AND quota_exceeded = 1
  AND (
    quota_recover_at IS NULL
    OR quota_recover_at > ?2
  )
"#,
        )
        .map_err(|e| db_err!("failed to prepare oauth exceeded list query: {e}"))?;
    let rows = stmt
        .query_map(params![cli_key, now_unix], |row| row.get::<_, i64>(0))
        .map_err(|e| db_err!("failed to query oauth exceeded ids: {e}"))?;
    let mut ids = std::collections::HashSet::new();
    for row in rows {
        ids.insert(row.map_err(|e| db_err!("failed to read oauth exceeded row: {e}"))?);
    }
    Ok(ids)
}

fn get_summary_by_id(
    conn: &Connection,
    id: i64,
) -> crate::shared::error::AppResult<OAuthAccountSummary> {
    conn.query_row(
        r#"
SELECT
  id,
  cli_key,
  label,
  email,
  provider_type,
  expires_at,
  refresh_lead_s,
  status,
  last_error,
  last_refreshed_at,
  quota_exceeded,
  quota_recover_at,
  created_at,
  updated_at
FROM oauth_accounts
WHERE id = ?1
"#,
        params![id],
        row_to_summary,
    )
    .optional()
    .map_err(|e| db_err!("failed to query oauth account summary: {e}"))?
    .ok_or_else(|| "DB_NOT_FOUND: oauth account not found".to_string().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        conn.execute_batch(
            r#"
CREATE TABLE oauth_accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  cli_key TEXT NOT NULL,
  label TEXT NOT NULL,
  email TEXT,
  provider_type TEXT NOT NULL,
  access_token TEXT NOT NULL,
  refresh_token TEXT,
  id_token TEXT,
  token_uri TEXT,
  client_id TEXT,
  client_secret TEXT,
  expires_at INTEGER,
  refresh_lead_s INTEGER NOT NULL DEFAULT 3600,
  status TEXT NOT NULL DEFAULT 'active',
  last_error TEXT,
  refresh_success_count INTEGER NOT NULL DEFAULT 0,
  refresh_failure_count INTEGER NOT NULL DEFAULT 0,
  last_refreshed_at INTEGER,
  quota_exceeded INTEGER NOT NULL DEFAULT 0,
  quota_recover_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(cli_key, label)
);
"#,
        )
        .expect("create oauth_accounts table");
        conn
    }

    #[test]
    fn upsert_create_and_list_summary_excludes_tokens() {
        let conn = setup_conn();
        let created = upsert(
            &conn,
            None,
            "claude",
            "Work",
            Some("work@example.com"),
            "claude_oauth",
            Some("token-1"),
            Some("refresh-1"),
            Some("id-1"),
            Some("https://token.example.com"),
            Some("client-a"),
            Some("secret-a"),
            Some(1_800_000_000),
            Some(1200),
            None,
            Some("active"),
        )
        .expect("create oauth account");

        assert_eq!(created.label, "Work");
        assert_eq!(created.status, OAuthAccountStatus::Active);
        assert!(!created.quota_exceeded);

        let listed = list_by_cli(&conn, "claude").expect("list oauth accounts");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, "Work");

        let full = get_by_id(&conn, created.id).expect("read full oauth account");
        assert_eq!(full.access_token, "token-1");
        assert_eq!(full.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(full.id_token.as_deref(), Some("id-1"));
    }

    #[test]
    fn update_tokens_marks_active_and_refresh_time() {
        let conn = setup_conn();
        let created = upsert(
            &conn,
            None,
            "codex",
            "Personal",
            None,
            "codex_oauth",
            Some("token-old"),
            Some("refresh-1"),
            Some("id-old"),
            Some("https://token.example.com"),
            None,
            None,
            Some(100),
            None,
            None,
            Some("error"),
        )
        .expect("create oauth account");

        mark_status(&conn, created.id, OAuthAccountStatus::Error, Some("boom"))
            .expect("mark error first");
        update_tokens(
            &conn,
            created.id,
            "token-new",
            Some("id-new"),
            Some(2_000_000_000),
            None,
        )
        .expect("update token");

        let updated = get_by_id(&conn, created.id).expect("read oauth account");
        assert_eq!(updated.access_token, "token-new");
        assert_eq!(updated.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(updated.id_token.as_deref(), Some("id-new"));
        assert_eq!(updated.expires_at, Some(2_000_000_000));
        assert_eq!(updated.status, OAuthAccountStatus::Active);
        assert!(updated.last_error.is_none());
        assert!(updated.last_refreshed_at.is_some());
    }

    #[test]
    fn quota_mark_clear_and_active_filter() {
        let conn = setup_conn();
        let a = upsert(
            &conn,
            None,
            "gemini",
            "G1",
            None,
            "gemini_oauth",
            Some("token-1"),
            Some("refresh-1"),
            None,
            Some("https://oauth2.googleapis.com/token"),
            Some("client-a"),
            None,
            Some(500),
            Some(60),
            None,
            Some("active"),
        )
        .expect("create account A");

        mark_quota_exceeded(&conn, a.id, 999).expect("mark quota exceeded");
        let quota_marked = get_by_id(&conn, a.id).expect("read quota-marked account");
        assert_eq!(quota_marked.status, OAuthAccountStatus::QuotaCooldown);

        clear_quota(&conn, a.id).expect("clear quota");
        let quota_cleared = get_by_id(&conn, a.id).expect("read quota-cleared account");
        assert_eq!(quota_cleared.status, OAuthAccountStatus::Active);
    }
}
