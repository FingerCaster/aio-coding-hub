use super::ProviderCapabilities;

pub(crate) fn capabilities_for_provider(
    provider: &crate::providers::ProviderForGateway,
) -> ProviderCapabilities {
    ProviderCapabilities {
        cx2cc_bridge: provider.is_cx2cc_bridge(),
        ..ProviderCapabilities::default()
    }
}

pub(crate) fn is_count_tokens_intercept_supported(
    is_claude_count_tokens: bool,
    capabilities: ProviderCapabilities,
) -> bool {
    is_claude_count_tokens && capabilities.supports_count_tokens_local_intercept()
}
