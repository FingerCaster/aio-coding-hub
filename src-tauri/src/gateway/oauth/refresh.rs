//! Usage: Background token refresh loop for OAuth providers.

use super::provider_trait::OAuthTokenSet;
use super::token_exchange::{refresh_access_token, TokenRefreshRequest};
use crate::shared::time::now_unix_seconds;

const REFRESH_LINEAR_RETRY_MAX_ATTEMPTS: u32 = 3;
const REFRESH_LINEAR_RETRY_BASE_DELAY_SECS: u64 = 2;

pub(crate) fn should_refresh_now(expires_at: Option<i64>, refresh_lead_s: i64) -> bool {
    let Some(expires_at) = expires_at else {
        // Unknown expiry → assume the token needs refreshing now so we don't
        // silently serve a potentially-expired token forever.
        return true;
    };
    let now = now_unix_seconds();
    now >= (expires_at - refresh_lead_s)
}

pub(crate) async fn refresh_provider_token_with_retry(
    client: &reqwest::Client,
    token_uri: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> Result<OAuthTokenSet, String> {
    let req = TokenRefreshRequest {
        token_uri: token_uri.to_string(),
        client_id: client_id.to_string(),
        client_secret: client_secret.map(str::to_string),
        refresh_token: refresh_token.to_string(),
    };

    let mut last_err = String::new();
    for attempt in 0..REFRESH_LINEAR_RETRY_MAX_ATTEMPTS {
        match refresh_access_token(client, &req).await {
            Ok(token_set) => return Ok(token_set),
            Err(e) => {
                if e.starts_with("AUTH_RELOGIN_REQUIRED") {
                    return Err(e);
                }
                last_err = e;
                if attempt + 1 < REFRESH_LINEAR_RETRY_MAX_ATTEMPTS {
                    let delay = REFRESH_LINEAR_RETRY_BASE_DELAY_SECS * (attempt as u64 + 1);
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                }
            }
        }
    }

    Err(format!(
        "token refresh failed after {REFRESH_LINEAR_RETRY_MAX_ATTEMPTS} attempts: {last_err}"
    ))
}
