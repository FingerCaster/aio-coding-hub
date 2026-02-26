//! Usage: Gemini (Google) OAuth adapter.
//!
//! Specializations:
//! - Extra authorize params: `access_type=offline`, `prompt=consent`
//! - Token validation: `ya29.*` prefix warning
//! - No limits support (requires project context)

use crate::domain::oauth_accounts::OAuthAccountForGateway;
use crate::gateway::oauth::provider_trait::{
    OAuthEndpoints, OAuthLimitsResult, OAuthProvider, OAuthTokenSet,
};
use crate::shared::error::AppResult;

const GEMINI_DEFAULT_CLIENT_ID: &str = concat!(
    "681255809395",
    "-oo8ft2oprdrnp9e3aqf6av3hmdib135j",
    ".apps.googleusercontent.com"
);
const GEMINI_DEFAULT_CLIENT_SECRET: &str = concat!("GOCSPX-", "4uHgMPm-1o7Sk-geV6Cu5clXFsxl");

const fn pick_non_empty_str(value: Option<&'static str>, fallback: &'static str) -> &'static str {
    match value {
        Some(v) if !v.is_empty() => v,
        _ => fallback,
    }
}

const fn pick_non_empty_opt_str(
    value: Option<&'static str>,
    fallback: &'static str,
) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => Some(fallback),
    }
}

const GEMINI_CLIENT_ID: &str = pick_non_empty_str(
    option_env!("AIO_GEMINI_OAUTH_CLIENT_ID"),
    GEMINI_DEFAULT_CLIENT_ID,
);
const GEMINI_CLIENT_SECRET: Option<&str> = pick_non_empty_opt_str(
    option_env!("AIO_GEMINI_OAUTH_CLIENT_SECRET"),
    GEMINI_DEFAULT_CLIENT_SECRET,
);

static GEMINI_ENDPOINTS: OAuthEndpoints = OAuthEndpoints {
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    client_id: GEMINI_CLIENT_ID,
    client_secret: GEMINI_CLIENT_SECRET,
    scopes: &[
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
    ],
    redirect_host: "127.0.0.1",
    callback_path: "/oauth2callback",
    default_callback_port: 8085,
};

pub(crate) struct GeminiOAuthProvider;

impl OAuthProvider for GeminiOAuthProvider {
    fn cli_key(&self) -> &'static str {
        "gemini"
    }

    fn provider_type(&self) -> &'static str {
        "gemini_oauth"
    }

    fn endpoints(&self) -> &OAuthEndpoints {
        &GEMINI_ENDPOINTS
    }

    fn extra_authorize_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![("access_type", "offline"), ("prompt", "consent")]
    }

    fn resolve_effective_token(
        &self,
        token_set: &OAuthTokenSet,
        stored_id_token: Option<&str>,
    ) -> (String, Option<String>) {
        // Validate the access_token format
        if !token_set.access_token.trim().starts_with("ya29.") {
            tracing::warn!(
                "gemini oauth access_token does not match expected ya29.* format; exchange response may be invalid"
            );
        }

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
        // Gemini does not need special headers
        Ok(())
    }

    fn fetch_limits(
        &self,
        _client: &reqwest::Client,
        _access_token: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = AppResult<OAuthLimitsResult>> + Send + '_>,
    > {
        Box::pin(async {
            Err("SYSTEM_ERROR: Gemini limit fetch requires project context; not supported from account-only fetch"
                .to_string()
                .into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::oauth::registry::global_registry;

    #[test]
    fn gemini_adapter_is_registered() {
        let provider = global_registry().get_by_cli_key("gemini").unwrap();
        assert_eq!(provider.cli_key(), "gemini");
        assert_eq!(provider.provider_type(), "gemini_oauth");
    }

    #[test]
    fn gemini_endpoints_are_correct() {
        let provider = global_registry().get_by_cli_key("gemini").unwrap();
        let endpoints = provider.endpoints();
        assert_eq!(endpoints.default_callback_port, 8085);
        assert_eq!(endpoints.callback_path, "/oauth2callback");
        assert_eq!(endpoints.redirect_host, "127.0.0.1");
    }

    #[test]
    fn gemini_extra_params_include_offline_access() {
        let provider = global_registry().get_by_cli_key("gemini").unwrap();
        let params = provider.extra_authorize_params();
        assert!(params.contains(&("access_type", "offline")));
        assert!(params.contains(&("prompt", "consent")));
    }

    #[test]
    fn gemini_credentials_are_not_empty() {
        let provider = global_registry().get_by_cli_key("gemini").unwrap();
        let endpoints = provider.endpoints();
        assert!(!endpoints.client_id.trim().is_empty());
        assert!(endpoints
            .client_secret
            .map(str::trim)
            .is_some_and(|v| !v.is_empty()));
    }

    #[test]
    fn gemini_oauth_uses_build_env_credentials() {
        let provider = global_registry().get_by_cli_key("gemini").unwrap();
        let endpoints = provider.endpoints();
        let expected_client_id = match option_env!("AIO_GEMINI_OAUTH_CLIENT_ID") {
            Some(value) if !value.is_empty() => value,
            _ => GEMINI_DEFAULT_CLIENT_ID,
        };
        assert_eq!(endpoints.client_id, expected_client_id);
    }
}
