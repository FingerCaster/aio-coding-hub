use super::ProviderCapabilities;

#[allow(dead_code)]
pub(crate) fn is_anthropic_compatible(capabilities: ProviderCapabilities) -> bool {
    capabilities.anthropic_compatible
}
