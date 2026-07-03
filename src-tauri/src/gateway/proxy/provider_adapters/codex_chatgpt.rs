use super::ProviderCapabilities;

#[allow(dead_code)]
pub(crate) fn is_codex_chatgpt_backend(capabilities: ProviderCapabilities) -> bool {
    capabilities.codex_chatgpt_backend
}
