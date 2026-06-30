use super::ProviderCapabilities;

#[allow(dead_code)]
pub(crate) fn is_gemini_oauth(capabilities: ProviderCapabilities) -> bool {
    capabilities.gemini_oauth
}
