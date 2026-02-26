//! Usage: OAuth refresh decision and execution helpers.

#![allow(dead_code)]

use crate::domain::oauth_accounts::{self, OAuthAccountForGateway};
use crate::gateway::oauth::registry::global_registry;
use crate::gateway::oauth::token_exchange::{refresh_access_token, TokenRefreshRequest};
use crate::shared::error::AppResult;
use tauri::Emitter;
use tokio::time::Duration;

pub(crate) const MIN_REFRESH_RETRY_INTERVAL_SECS: i64 = 30;
pub(crate) const REFRESH_LINEAR_RETRY_MAX_ATTEMPTS: u32 = 3;
pub(crate) const REFRESH_LINEAR_RETRY_BASE_DELAY_SECS: u64 = 2;
const REFRESH_LOOP_INTERVAL_SECS: u64 = 60;
const REFRESH_BATCH_LIMIT: usize = 64;
const QUOTA_CLEAR_BATCH_LIMIT: usize = 128;

pub(crate) fn should_refresh_now(
    expires_at: Option<i64>,
    refresh_lead_s: i64,
    now_unix: i64,
) -> bool {
    let Some(expiry) = expires_at else {
        return false;
    };
    let lead = refresh_lead_s.max(0);
    expiry.saturating_sub(lead) <= now_unix
}

pub(crate) fn refreshed_recently(
    last_refreshed_at: Option<i64>,
    now_unix: i64,
    min_interval_secs: i64,
) -> bool {
    let Some(last) = last_refreshed_at else {
        return false;
    };
    let min_interval_secs = min_interval_secs.max(1);
    now_unix.saturating_sub(last) < min_interval_secs
}

pub(crate) async fn refresh_account_access_token(
    client: &reqwest::Client,
    account: &OAuthAccountForGateway,
) -> AppResult<crate::gateway::oauth::provider_trait::OAuthTokenSet> {
    let refresh_token = account
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "SEC_INVALID_INPUT: oauth account missing refresh_token".to_string())?;
    let token_uri = account
        .token_uri
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "SEC_INVALID_INPUT: oauth account missing token_uri".to_string())?;
    let client_id = account
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "SEC_INVALID_INPUT: oauth account missing client_id".to_string())?;

    let request = TokenRefreshRequest {
        token_uri: token_uri.to_string(),
        client_id: client_id.to_string(),
        client_secret: account.client_secret.clone(),
        refresh_token: refresh_token.to_string(),
    };

    refresh_access_token(client, &request).await
}

pub(crate) async fn refresh_account_access_token_with_linear_retry(
    client: &reqwest::Client,
    account: &OAuthAccountForGateway,
    max_attempts: u32,
) -> AppResult<crate::gateway::oauth::provider_trait::OAuthTokenSet> {
    let max_attempts = max_attempts.max(1);
    for attempt in 1..=max_attempts {
        match refresh_account_access_token(client, account).await {
            Ok(tokens) => return Ok(tokens),
            Err(err) if attempt < max_attempts => {
                let delay_secs = REFRESH_LINEAR_RETRY_BASE_DELAY_SECS * attempt as u64;
                tracing::warn!(
                    oauth_account_id = account.id,
                    cli_key = %account.cli_key,
                    attempt = attempt,
                    max_attempts = max_attempts,
                    delay_secs = delay_secs,
                    "oauth refresh failed; retrying: {}",
                    err
                );
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("max_attempts is always >= 1, so the loop always returns")
}

fn collect_refresh_work(
    db: &crate::db::Db,
    now_unix: i64,
) -> AppResult<(Vec<OAuthAccountForGateway>, Vec<i64>)> {
    let conn = db.open_connection()?;
    let accounts = oauth_accounts::list_needing_refresh(&conn, now_unix, REFRESH_BATCH_LIMIT)?;
    let expired_quotas =
        oauth_accounts::list_expired_quotas(&conn, now_unix, QUOTA_CLEAR_BATCH_LIMIT)?;
    Ok((accounts, expired_quotas))
}

fn clear_expired_quotas(
    app: &tauri::AppHandle,
    db: &crate::db::Db,
    expired_quotas: Vec<i64>,
) -> AppResult<()> {
    if expired_quotas.is_empty() {
        return Ok(());
    }

    let conn = db.open_connection()?;
    let mut changed = false;
    for account_id in expired_quotas {
        if !oauth_accounts::clear_quota(&conn, account_id)? {
            continue;
        }
        changed = true;
        let _ = app.emit(
            "oauth-account-quota",
            serde_json::json!({
                "id": account_id,
                "exceeded": false,
                "recover_at": serde_json::Value::Null
            }),
        );
    }
    if changed {
        crate::gateway::oauth::quota_cache::invalidate_all();
    }
    Ok(())
}

async fn refresh_due_accounts(
    app: &tauri::AppHandle,
    db: &crate::db::Db,
    client: &reqwest::Client,
    now_unix: i64,
    accounts: Vec<OAuthAccountForGateway>,
) -> AppResult<()> {
    let registry = global_registry();

    for account in accounts {
        if refreshed_recently(
            account.last_refreshed_at,
            now_unix,
            MIN_REFRESH_RETRY_INTERVAL_SECS,
        ) {
            continue;
        }

        match refresh_account_access_token_with_linear_retry(
            client,
            &account,
            REFRESH_LINEAR_RETRY_MAX_ATTEMPTS,
        )
        .await
        {
            Ok(tokens) => {
                // Use the adapter's resolve_effective_token instead of hardcoded logic
                let (effective_access_token, id_token_to_store) =
                    if let Some(provider) = registry.get_by_cli_key(&account.cli_key) {
                        provider.resolve_effective_token(&tokens, account.id_token.as_deref())
                    } else {
                        // Fallback for unknown providers
                        (tokens.access_token.clone(), tokens.id_token.clone())
                    };

                let expires_at = tokens.expires_at.or(account.expires_at);
                let conn = db.open_connection()?;
                oauth_accounts::update_tokens(
                    &conn,
                    account.id,
                    effective_access_token.as_str(),
                    id_token_to_store.as_deref(),
                    expires_at,
                    tokens.refresh_token.as_deref(),
                )?;
                let _ = app.emit(
                    "oauth-account-refreshed",
                    serde_json::json!({
                        "id": account.id,
                        "expires_at": expires_at
                    }),
                );
            }
            Err(err) => {
                let err_text = err.to_string();
                let conn = db.open_connection()?;
                let _ = oauth_accounts::record_refresh_failure(&conn, account.id, Some(&err_text));
                let _ = app.emit(
                    "oauth-account-error",
                    serde_json::json!({
                        "id": account.id,
                        "error": err_text,
                        "status": "unchanged"
                    }),
                );
            }
        }
    }
    Ok(())
}

pub(crate) async fn run_background_refresh_loop(
    app: tauri::AppHandle,
    db: crate::db::Db,
    client: reqwest::Client,
) {
    loop {
        let now_unix = crate::shared::time::now_unix_seconds();
        match collect_refresh_work(&db, now_unix) {
            Ok((accounts, expired_quotas)) => {
                if let Err(err) = clear_expired_quotas(&app, &db, expired_quotas) {
                    tracing::warn!("oauth quota-clear tick failed: {}", err);
                }
                if let Err(err) = refresh_due_accounts(&app, &db, &client, now_unix, accounts).await
                {
                    tracing::warn!("oauth refresh tick failed: {}", err);
                }
            }
            Err(err) => {
                tracing::warn!("oauth refresh tick query failed: {}", err);
            }
        }

        tokio::time::sleep(Duration::from_secs(REFRESH_LOOP_INTERVAL_SECS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refresh_now_handles_unknown_expiry() {
        assert!(!should_refresh_now(None, 3600, 1000));
    }

    #[test]
    fn should_refresh_now_respects_refresh_lead_window() {
        assert!(!should_refresh_now(Some(2000), 300, 1600));
        assert!(should_refresh_now(Some(2000), 300, 1700));
        assert!(should_refresh_now(Some(2000), 300, 2200));
    }

    #[test]
    fn refreshed_recently_guards_duplicate_refresh() {
        assert!(!refreshed_recently(
            None,
            2000,
            MIN_REFRESH_RETRY_INTERVAL_SECS
        ));
        assert!(refreshed_recently(
            Some(1980),
            2000,
            MIN_REFRESH_RETRY_INTERVAL_SECS
        ));
        assert!(!refreshed_recently(
            Some(1800),
            2000,
            MIN_REFRESH_RETRY_INTERVAL_SECS
        ));
    }
}
