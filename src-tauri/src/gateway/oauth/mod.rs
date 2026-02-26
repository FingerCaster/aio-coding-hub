//! Usage: OAuth flow helpers for account login, token exchange, and refresh.
//!
//! Uses the Adapter Design Pattern: each CLI implements `OAuthProvider` trait,
//! registered in `OAuthProviderRegistry` for dynamic dispatch.

pub(crate) mod adapters;
pub(crate) mod callback_server;
pub(crate) mod pkce;
pub(crate) mod provider_trait;
pub(crate) mod quota_cache;
pub(crate) mod refresh;
pub(crate) mod registry;
pub(crate) mod token_exchange;
