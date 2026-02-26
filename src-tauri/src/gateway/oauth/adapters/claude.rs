//! Usage: Claude OAuth adapter.
//!
//! Specializations:
//! - Extra authorize param: `code=true`
//! - Limits via `api.anthropic.com/api/oauth/usage`

use crate::domain::oauth_accounts::OAuthAccountForGateway;
use crate::gateway::oauth::provider_trait::{
    OAuthEndpoints, OAuthLimitsResult, OAuthProvider, OAuthTokenSet,
};
use crate::shared::error::AppResult;

/// Claude OAuth usage endpoint.
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

static CLAUDE_ENDPOINTS: OAuthEndpoints = OAuthEndpoints {
    auth_url: "https://claude.ai/oauth/authorize",
    token_url: "https://api.anthropic.com/v1/oauth/token",
    // Public client identifier used by Claude Code desktop OAuth login.
    client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    client_secret: None,
    scopes: &["org:create_api_key", "user:profile", "user:inference"],
    redirect_host: "localhost",
    callback_path: "/callback",
    default_callback_port: 54545,
};

pub(crate) struct ClaudeOAuthProvider;

impl OAuthProvider for ClaudeOAuthProvider {
    fn cli_key(&self) -> &'static str {
        "claude"
    }

    fn provider_type(&self) -> &'static str {
        "claude_oauth"
    }

    fn endpoints(&self) -> &OAuthEndpoints {
        &CLAUDE_ENDPOINTS
    }

    fn extra_authorize_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![("code", "true")]
    }

    fn resolve_effective_token(
        &self,
        token_set: &OAuthTokenSet,
        stored_id_token: Option<&str>,
    ) -> (String, Option<String>) {
        let id_token_to_store = token_set.id_token.clone().or_else(|| {
            stored_id_token
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        });
        (token_set.access_token.clone(), id_token_to_store)
    }

    fn inject_upstream_headers(
        &self,
        _headers: &mut reqwest::header::HeaderMap,
        _account: &OAuthAccountForGateway,
    ) -> AppResult<()> {
        // Claude does not need special headers beyond standard Bearer auth
        Ok(())
    }

    fn fetch_limits(
        &self,
        client: &reqwest::Client,
        access_token: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = AppResult<OAuthLimitsResult>> + Send + '_>,
    > {
        let client = client.clone();
        let access_token = access_token.to_string();
        Box::pin(async move {
            let response = client
                .get(CLAUDE_USAGE_URL)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("anthropic-beta", "oauth-2025-04-20")
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|e| format!("SYSTEM_ERROR: claude quota request failed: {e}"))?;

            if !response.status().is_success() {
                return Err(format!(
                    "SYSTEM_ERROR: claude quota request failed with status {}",
                    response.status()
                )
                .into());
            }

            let body: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("SYSTEM_ERROR: claude quota parse failed: {e}"))?;

            let (limit_5h_text, limit_weekly_text) = parse_claude_limits(&body);
            Ok(OAuthLimitsResult {
                limit_5h_text,
                limit_weekly_text,
            })
        })
    }
}

fn extract_utilization(window: &serde_json::Value) -> Option<f64> {
    window
        .get("utilization")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| {
            window
                .get("utilization")
                .and_then(serde_json::Value::as_str)?
                .parse::<f64>()
                .ok()
        })
}

fn parse_claude_limits(body: &serde_json::Value) -> (Option<String>, Option<String>) {
    let limit_5h = body
        .get("five_hour")
        .and_then(extract_utilization)
        .map(|used| format!("{:.0}%", (100.0 - used).clamp(0.0, 100.0)));
    let limit_weekly = body
        .get("seven_day")
        .and_then(extract_utilization)
        .map(|used| format!("{:.0}%", (100.0 - used).clamp(0.0, 100.0)));
    (limit_5h, limit_weekly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::oauth::registry::global_registry;

    #[test]
    fn claude_adapter_is_registered() {
        let provider = global_registry().get_by_cli_key("claude").unwrap();
        assert_eq!(provider.cli_key(), "claude");
        assert_eq!(provider.provider_type(), "claude_oauth");
    }

    #[test]
    fn claude_endpoints_are_correct() {
        let provider = global_registry().get_by_cli_key("claude").unwrap();
        let endpoints = provider.endpoints();
        assert_eq!(endpoints.default_callback_port, 54545);
        assert_eq!(endpoints.callback_path, "/callback");
    }

    #[test]
    fn claude_extra_params_includes_code_true() {
        let provider = global_registry().get_by_cli_key("claude").unwrap();
        let params = provider.extra_authorize_params();
        assert!(params.contains(&("code", "true")));
    }

    #[test]
    fn claude_resolve_token_preserves_stored_id_token() {
        let provider = ClaudeOAuthProvider;
        let token_set = OAuthTokenSet {
            access_token: "access-123".to_string(),
            refresh_token: None,
            expires_at: None,
            id_token: None,
        };
        let (effective, id_token) =
            provider.resolve_effective_token(&token_set, Some("stored-id-token"));
        assert_eq!(effective, "access-123");
        assert_eq!(id_token.as_deref(), Some("stored-id-token"));
    }

    #[test]
    fn parse_claude_limits_extracts_utilization() {
        let body = serde_json::json!({
            "five_hour": { "utilization": 30.0 },
            "seven_day": { "utilization": 50.0 }
        });
        let (h5, weekly) = parse_claude_limits(&body);
        assert_eq!(h5.as_deref(), Some("70%"));
        assert_eq!(weekly.as_deref(), Some("50%"));
    }
}
