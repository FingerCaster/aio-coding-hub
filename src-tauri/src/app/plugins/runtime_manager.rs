//! Usage: Runtime policy facade for plugin runtime execution.

use crate::domain::plugins::PluginRuntime;
use crate::gateway::plugins::permissions::GatewayPluginError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeDispatch {
    ExtensionHost,
}

pub(crate) struct PluginRuntimeManager;

impl PluginRuntimeManager {
    pub(crate) fn new() -> Self {
        Self
    }

    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        Self::new()
    }

    pub(crate) fn runtime_dispatch(
        &self,
        _plugin_id: &str,
        _runtime: &PluginRuntime,
    ) -> Result<RuntimeDispatch, GatewayPluginError> {
        Ok(RuntimeDispatch::ExtensionHost)
    }
}

#[cfg(test)]
mod tests {
    use super::{PluginRuntimeManager, RuntimeDispatch};
    use crate::domain::plugins::PluginRuntime;

    #[test]
    fn runtime_manager_returns_extension_host_dispatch() {
        let manager = PluginRuntimeManager::for_tests();
        let runtime = PluginRuntime::ExtensionHost {
            language: "typescript".to_string(),
        };

        assert_eq!(
            manager
                .runtime_dispatch("acme.extension", &runtime)
                .expect("extension host runtime should be recognized"),
            RuntimeDispatch::ExtensionHost
        );
    }
}
