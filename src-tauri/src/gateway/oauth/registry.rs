//! Usage: OAuthProviderRegistry - global singleton for provider lookup via trait dispatch.
//!
//! Uses `OnceLock` for thread-safe, zero-cost-after-init access.
//! Adding a new CLI only requires registering a new adapter here.

use super::adapters::{
    claude::ClaudeOAuthProvider, codex::CodexOAuthProvider, gemini::GeminiOAuthProvider,
};
use super::provider_trait::OAuthProvider;
use std::collections::HashMap;
use std::sync::OnceLock;

pub(crate) struct OAuthProviderRegistry {
    by_cli_key: HashMap<&'static str, Box<dyn OAuthProvider>>,
    by_provider_type: HashMap<&'static str, &'static str>, // provider_type -> cli_key
}

impl OAuthProviderRegistry {
    fn new() -> Self {
        let mut by_cli_key: HashMap<&'static str, Box<dyn OAuthProvider>> = HashMap::new();
        let mut by_provider_type: HashMap<&'static str, &'static str> = HashMap::new();

        let providers: Vec<Box<dyn OAuthProvider>> = vec![
            Box::new(ClaudeOAuthProvider),
            Box::new(CodexOAuthProvider),
            Box::new(GeminiOAuthProvider),
        ];

        for provider in providers {
            let cli_key = provider.cli_key();
            let provider_type = provider.provider_type();
            by_provider_type.insert(provider_type, cli_key);
            by_cli_key.insert(cli_key, provider);
        }

        Self {
            by_cli_key,
            by_provider_type,
        }
    }

    /// Look up a provider by cli_key (e.g., "claude").
    pub(crate) fn get_by_cli_key(&self, cli_key: &str) -> Option<&dyn OAuthProvider> {
        self.by_cli_key.get(cli_key).map(|p| p.as_ref())
    }

    /// Look up a provider by provider_type (e.g., "claude_oauth").
    pub(crate) fn get_by_provider_type(&self, provider_type: &str) -> Option<&dyn OAuthProvider> {
        let cli_key = self.by_provider_type.get(provider_type)?;
        self.get_by_cli_key(cli_key)
    }

    /// Get all registered CLI keys.
    #[allow(dead_code)]
    pub(crate) fn cli_keys(&self) -> Vec<&str> {
        self.by_cli_key.keys().copied().collect()
    }
}

/// Global singleton accessor.
pub(crate) fn global_registry() -> &'static OAuthProviderRegistry {
    static REGISTRY: OnceLock<OAuthProviderRegistry> = OnceLock::new();
    REGISTRY.get_or_init(OAuthProviderRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_three_providers() {
        let reg = global_registry();
        assert!(reg.get_by_cli_key("claude").is_some());
        assert!(reg.get_by_cli_key("codex").is_some());
        assert!(reg.get_by_cli_key("gemini").is_some());
        assert!(reg.get_by_cli_key("unknown").is_none());
    }

    #[test]
    fn registry_lookup_by_provider_type() {
        let reg = global_registry();
        assert!(reg.get_by_provider_type("claude_oauth").is_some());
        assert!(reg.get_by_provider_type("codex_oauth").is_some());
        assert!(reg.get_by_provider_type("gemini_oauth").is_some());
        assert!(reg.get_by_provider_type("unknown_oauth").is_none());
    }

    #[test]
    fn provider_type_maps_to_correct_cli_key() {
        let reg = global_registry();
        let claude = reg.get_by_provider_type("claude_oauth").unwrap();
        assert_eq!(claude.cli_key(), "claude");
        let codex = reg.get_by_provider_type("codex_oauth").unwrap();
        assert_eq!(codex.cli_key(), "codex");
        let gemini = reg.get_by_provider_type("gemini_oauth").unwrap();
        assert_eq!(gemini.cli_key(), "gemini");
    }
}
