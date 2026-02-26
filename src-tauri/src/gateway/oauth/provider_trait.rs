//! Usage: OAuthProvider trait definition - the core abstraction for multi-CLI OAuth.
//!
//! Each CLI (Claude, Codex, Gemini) implements this trait to customize OAuth behavior.
//! The gateway uses trait dispatch via `OAuthProviderRegistry` instead of hardcoded matches.

#![allow(dead_code)]

use crate::domain::oauth_accounts::OAuthAccountForGateway;
use crate::shared::error::AppResult;
use reqwest::header::HeaderMap;

/// Static OAuth endpoint configuration for a CLI provider.
pub(crate) struct OAuthEndpoints {
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub scopes: &'static [&'static str],
    pub redirect_host: &'static str,
    pub callback_path: &'static str,
    pub default_callback_port: u16,
}

/// Token set returned from OAuth token exchange/refresh.
#[derive(Debug, Clone)]
pub(crate) struct OAuthTokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub id_token: Option<String>,
}

/// Result from fetching usage limits for an OAuth account.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct OAuthLimitsResult {
    pub limit_5h_text: Option<String>,
    pub limit_weekly_text: Option<String>,
}

/// Each CLI implements this trait to customize OAuth behavior.
///
/// The trait is object-safe to allow dynamic dispatch via `&dyn OAuthProvider`.
/// Adding a new CLI only requires implementing this trait + registering in the registry.
pub(crate) trait OAuthProvider: Send + Sync {
    /// The CLI key (e.g., "claude", "codex", "gemini").
    fn cli_key(&self) -> &'static str;

    /// The provider type string (e.g., "claude_oauth", "codex_oauth", "gemini_oauth").
    fn provider_type(&self) -> &'static str;

    /// Static OAuth endpoint configuration.
    fn endpoints(&self) -> &OAuthEndpoints;

    /// Extra query params for the authorization URL (CLI-specific).
    fn extra_authorize_params(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }

    /// Resolve effective token from exchange response.
    /// Returns (effective_access_token, id_token_to_store).
    fn resolve_effective_token(
        &self,
        token_set: &OAuthTokenSet,
        stored_id_token: Option<&str>,
    ) -> (String, Option<String>);

    /// Inject CLI-specific upstream headers (e.g., Codex originator).
    fn inject_upstream_headers(
        &self,
        _headers: &mut HeaderMap,
        _account: &OAuthAccountForGateway,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Check if base_url needs special backend routing.
    fn is_special_backend(&self, _base_url: &str) -> bool {
        false
    }

    /// Rewrite forwarded path if needed (e.g., Codex `/v1/*` -> `/*`).
    fn rewrite_forwarded_path(&self, path: &str) -> String {
        path.to_string()
    }

    /// Rewrite request body if needed (e.g., Codex `store: false`).
    fn rewrite_request_body(&self, _path: &str, body: bytes::Bytes) -> bytes::Bytes {
        body
    }

    /// Fetch usage limits (CLI-specific API).
    fn fetch_limits(
        &self,
        client: &reqwest::Client,
        access_token: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = AppResult<OAuthLimitsResult>> + Send + '_>,
    >;
}

/// Helper to construct a redirect URI from endpoints and port.
pub(crate) fn make_redirect_uri(endpoints: &OAuthEndpoints, port: u16) -> String {
    format!(
        "http://{}:{port}{}",
        endpoints.redirect_host, endpoints.callback_path
    )
}
