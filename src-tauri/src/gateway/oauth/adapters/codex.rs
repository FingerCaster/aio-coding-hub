//! Usage: Codex (OpenAI ChatGPT) OAuth adapter.
//!
//! Specializations:
//! - Extra authorize params: `originator`, `codex_cli_simplified_flow`
//! - Header injection: `originator`, `chatgpt-account-id`
//! - Path rewrite: `/v1/*` -> `/*` for ChatGPT backend
//! - Body rewrite: `store: false` for ChatGPT backend
//! - Limits via `chatgpt.com/backend-api/wham/usage`

#![allow(dead_code)]

use crate::domain::oauth_accounts::OAuthAccountForGateway;
use crate::gateway::oauth::provider_trait::{
    OAuthEndpoints, OAuthLimitsResult, OAuthProvider, OAuthTokenSet,
};
use crate::shared::error::AppResult;
use serde_json::Value;

/// Codex usage/quota endpoint.
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
/// User-Agent sent with Codex quota requests (matches upstream codex_cli_rs).
const CODEX_QUOTA_USER_AGENT: &str = "codex_cli_rs/0.76.0 (Debian 13.0.0; x86_64) WindowsTerminal";

/// ChatGPT backend base URL for special routing detection.
const CHATGPT_BACKEND_URL: &str = "https://chatgpt.com";

static CODEX_ENDPOINTS: OAuthEndpoints = OAuthEndpoints {
    auth_url: "https://auth.openai.com/oauth/authorize",
    token_url: "https://auth.openai.com/oauth/token",
    client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
    client_secret: None,
    scopes: &["openid", "profile", "email", "offline_access"],
    redirect_host: "localhost",
    callback_path: "/auth/callback",
    default_callback_port: 1455,
};

pub(crate) struct CodexOAuthProvider;

impl OAuthProvider for CodexOAuthProvider {
    fn cli_key(&self) -> &'static str {
        "codex"
    }

    fn provider_type(&self) -> &'static str {
        "codex_oauth"
    }

    fn endpoints(&self) -> &OAuthEndpoints {
        &CODEX_ENDPOINTS
    }

    fn extra_authorize_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "codex_cli_rs"),
        ]
    }

    fn resolve_effective_token(
        &self,
        token_set: &OAuthTokenSet,
        stored_id_token: Option<&str>,
    ) -> (String, Option<String>) {
        // Codex uses the OAuth access_token directly.
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
        headers: &mut reqwest::header::HeaderMap,
        _account: &OAuthAccountForGateway,
    ) -> AppResult<()> {
        headers.insert(
            reqwest::header::HeaderName::from_static("originator"),
            reqwest::header::HeaderValue::from_static("codex_cli_rs"),
        );
        // TODO: extract chatgpt-account-id from id_token JWT claims if present
        Ok(())
    }

    fn is_special_backend(&self, base_url: &str) -> bool {
        base_url.starts_with(CHATGPT_BACKEND_URL)
    }

    fn rewrite_forwarded_path(&self, path: &str) -> String {
        // Strip `/v1` prefix for ChatGPT backend routing
        if let Some(stripped) = path.strip_prefix("/v1") {
            if stripped.is_empty() {
                return "/".to_string();
            }
            return stripped.to_string();
        }
        path.to_string()
    }

    fn rewrite_request_body(&self, _path: &str, body: bytes::Bytes) -> bytes::Bytes {
        // Inject `"store": false` into ChatGPT backend requests
        if let Ok(mut value) = serde_json::from_slice::<Value>(&body) {
            if let Some(obj) = value.as_object_mut() {
                obj.insert("store".to_string(), Value::Bool(false));
                if let Ok(encoded) = serde_json::to_vec(&value) {
                    return bytes::Bytes::from(encoded);
                }
            }
        }
        body
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
                .get(CODEX_USAGE_URL)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("User-Agent", CODEX_QUOTA_USER_AGENT)
                .header("Content-Type", "application/json")
                .send()
                .await
                .map_err(|e| format!("SYSTEM_ERROR: codex quota request failed: {e}"))?;

            if !response.status().is_success() {
                return Err(format!(
                    "SYSTEM_ERROR: codex quota request failed with status {}",
                    response.status()
                )
                .into());
            }

            let body: Value = response
                .json()
                .await
                .map_err(|e| format!("SYSTEM_ERROR: codex quota parse failed: {e}"))?;

            let (limit_5h_text, limit_weekly_text) = parse_codex_limits(&body);
            Ok(OAuthLimitsResult {
                limit_5h_text,
                limit_weekly_text,
            })
        })
    }
}

fn parse_remaining_percent_from_window(window: &Value) -> Option<f64> {
    if !window.is_object() {
        return None;
    }
    if let Some(used) = window
        .get("used_percent")
        .and_then(Value::as_f64)
        .or_else(|| window.get("usedPercent").and_then(Value::as_f64))
    {
        let remaining = (100.0 - used).clamp(0.0, 100.0);
        return Some(remaining);
    }
    let remaining = window
        .get("remaining_count")
        .and_then(Value::as_f64)
        .or_else(|| window.get("remainingCount").and_then(Value::as_f64));
    let total = window
        .get("total_count")
        .and_then(Value::as_f64)
        .or_else(|| window.get("totalCount").and_then(Value::as_f64));
    match (remaining, total) {
        (Some(rem), Some(t)) if t > 0.0 => Some((rem / t * 100.0).clamp(0.0, 100.0)),
        _ => None,
    }
}

fn parse_codex_limits(body: &Value) -> (Option<String>, Option<String>) {
    let rate_limit = body.get("rate_limit").unwrap_or(body);
    let primary = rate_limit
        .get("primary_window")
        .or_else(|| rate_limit.get("primaryWindow"))
        .or_else(|| body.get("5_hour_window"))
        .or_else(|| body.get("fiveHourWindow"));
    let secondary = rate_limit
        .get("secondary_window")
        .or_else(|| rate_limit.get("secondaryWindow"))
        .or_else(|| body.get("weekly_window"))
        .or_else(|| body.get("weeklyWindow"));

    let limit_5h = primary
        .and_then(parse_remaining_percent_from_window)
        .map(|v| format!("{:.0}%", v.clamp(0.0, 100.0)));
    let limit_weekly = secondary
        .and_then(parse_remaining_percent_from_window)
        .map(|v| format!("{:.0}%", v.clamp(0.0, 100.0)));
    (limit_5h, limit_weekly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::oauth::registry::global_registry;

    #[test]
    fn codex_adapter_is_registered() {
        let provider = global_registry().get_by_cli_key("codex").unwrap();
        assert_eq!(provider.cli_key(), "codex");
        assert_eq!(provider.provider_type(), "codex_oauth");
    }

    #[test]
    fn codex_endpoints_are_correct() {
        let provider = global_registry().get_by_cli_key("codex").unwrap();
        let endpoints = provider.endpoints();
        assert_eq!(endpoints.default_callback_port, 1455);
        assert_eq!(endpoints.callback_path, "/auth/callback");
    }

    #[test]
    fn codex_extra_params_include_originator() {
        let provider = global_registry().get_by_cli_key("codex").unwrap();
        let params = provider.extra_authorize_params();
        assert!(params.contains(&("originator", "codex_cli_rs")));
        assert!(params.contains(&("codex_cli_simplified_flow", "true")));
    }

    #[test]
    fn codex_path_rewrite_strips_v1() {
        let provider = CodexOAuthProvider;
        assert_eq!(
            provider.rewrite_forwarded_path("/v1/chat/completions"),
            "/chat/completions"
        );
        assert_eq!(provider.rewrite_forwarded_path("/v1"), "/");
        assert_eq!(provider.rewrite_forwarded_path("/other"), "/other");
    }

    #[test]
    fn codex_body_rewrite_injects_store_false() {
        let provider = CodexOAuthProvider;
        let body = bytes::Bytes::from(r#"{"model":"gpt-4","messages":[]}"#);
        let rewritten = provider.rewrite_request_body("/chat/completions", body);
        let parsed: Value = serde_json::from_slice(&rewritten).unwrap();
        assert_eq!(parsed.get("store").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn codex_is_special_backend_chatgpt() {
        let provider = CodexOAuthProvider;
        assert!(provider.is_special_backend("https://chatgpt.com/backend-api"));
        assert!(!provider.is_special_backend("https://api.openai.com"));
    }

    #[test]
    fn parse_codex_limits_handles_rate_limit_structure() {
        let body = serde_json::json!({
            "rate_limit": {
                "primary_window": { "used_percent": 30.0 },
                "secondary_window": { "used_percent": 10.0 }
            }
        });
        let (h5, weekly) = parse_codex_limits(&body);
        assert_eq!(h5.as_deref(), Some("70%"));
        assert_eq!(weekly.as_deref(), Some("90%"));
    }
}
