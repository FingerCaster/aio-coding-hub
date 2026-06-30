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
        runtime: &PluginRuntime,
    ) -> Result<RuntimeDispatch, GatewayPluginError> {
        match runtime {
            PluginRuntime::ExtensionHost { .. } => Ok(RuntimeDispatch::ExtensionHost),
            PluginRuntime::Native { engine } => Err(GatewayPluginError::new(
                "PLUGIN_UNSUPPORTED_RUNTIME",
                format!("unsupported native plugin runtime engine: {engine}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PluginRuntimeManager, RuntimeDispatch};
    use crate::domain::plugins::PluginRuntime;

    #[test]
    fn runtime_manager_rejects_non_extension_host_community_runtime() {
        let manager = PluginRuntimeManager::for_tests();
        let runtime = PluginRuntime::Native {
            engine: "hostPrivateRedactor".to_string(),
        };

        let err = manager
            .runtime_dispatch("example.privacy-filter", &runtime)
            .expect_err("community native runtime should be rejected");

        assert_eq!(err.code(), "PLUGIN_UNSUPPORTED_RUNTIME");
    }

    #[test]
    fn runtime_manager_rejects_non_official_native_privacy_filter() {
        let manager = PluginRuntimeManager::for_tests();
        let runtime = PluginRuntime::Native {
            engine: "hostPrivateRedactor".to_string(),
        };

        let err = manager
            .runtime_dispatch("example.privacy-filter", &runtime)
            .expect_err("native runtime should be rejected by the manager");

        assert_eq!(err.code(), "PLUGIN_UNSUPPORTED_RUNTIME");
        assert_eq!(
            err.to_string(),
            "PLUGIN_UNSUPPORTED_RUNTIME: unsupported native plugin runtime engine: hostPrivateRedactor"
        );
    }

    #[test]
    fn runtime_manager_rejects_all_native_runtimes_without_official_exceptions() {
        let manager = PluginRuntimeManager::for_tests();
        let runtime = PluginRuntime::Native {
            engine: "hostPrivateRedactor".to_string(),
        };

        let err = manager
            .runtime_dispatch("official.privacy-filter", &runtime)
            .expect_err("official plugin native runtime should be rejected");

        assert_eq!(err.code(), "PLUGIN_UNSUPPORTED_RUNTIME");
    }

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
