pub(crate) mod claude;
pub(crate) mod codex_chatgpt;
pub(crate) mod gemini_oauth;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProviderCapabilities {
    pub(crate) anthropic_compatible: bool,
    pub(crate) openai_responses_compatible: bool,
    pub(crate) codex_chatgpt_backend: bool,
    pub(crate) gemini_oauth: bool,
    pub(crate) cx2cc_bridge: bool,
    pub(crate) service_tier_adjustment: bool,
    pub(crate) stream_idle_timeout_override: bool,
}

#[cfg(test)]
mod tests {
    use super::ProviderCapabilities;

    #[test]
    fn registry_identifies_cx2cc_bridge_capability() {
        let capabilities = ProviderCapabilities {
            cx2cc_bridge: true,
            ..ProviderCapabilities::default()
        };

        assert!(capabilities.cx2cc_bridge);
    }

    #[test]
    fn registry_default_capabilities_are_plain_provider() {
        let capabilities = ProviderCapabilities::default();

        assert_eq!(
            capabilities,
            ProviderCapabilities {
                anthropic_compatible: false,
                openai_responses_compatible: false,
                codex_chatgpt_backend: false,
                gemini_oauth: false,
                cx2cc_bridge: false,
                service_tier_adjustment: false,
                stream_idle_timeout_override: false,
            }
        );
        assert!(!capabilities.cx2cc_bridge);
    }
}
