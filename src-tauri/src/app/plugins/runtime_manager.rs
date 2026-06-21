//! Usage: Runtime policy facade for plugin runtime execution.

use crate::app::plugins::runtime_policy::RuntimePolicy;
use crate::domain::plugins::PluginRuntime;
use crate::gateway::plugins::permissions::GatewayPluginError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeDispatch {
    DeclarativeRules,
    NativePrivacyFilter,
    WasmNotWired,
}

pub(crate) struct PluginRuntimeManager {
    policy: RuntimePolicy,
}

impl PluginRuntimeManager {
    pub(crate) fn new(policy: RuntimePolicy) -> Self {
        Self { policy }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(policy: RuntimePolicy) -> Self {
        Self::new(policy)
    }

    pub(crate) fn validate_runtime_policy(
        &self,
        runtime: &PluginRuntime,
    ) -> Result<(), GatewayPluginError> {
        if matches!(runtime, PluginRuntime::Wasm { .. }) && !self.policy.wasm_enabled {
            return Err(GatewayPluginError::new(
                "PLUGIN_RUNTIME_DISABLED",
                "wasm runtime execution is disabled by host policy",
            ));
        }
        Ok(())
    }

    pub(crate) fn runtime_dispatch(
        &self,
        runtime: &PluginRuntime,
    ) -> Result<RuntimeDispatch, GatewayPluginError> {
        self.validate_runtime_policy(runtime)?;
        // Reserved for future process runtime policy. Plugin API v1 has no
        // process runtime variant, so this flag is intentionally not decisive yet.
        let _process_enabled = self.policy.process_enabled;

        match runtime {
            PluginRuntime::DeclarativeRules { .. } => Ok(RuntimeDispatch::DeclarativeRules),
            PluginRuntime::Native { engine } if engine == "privacyFilter" => {
                Ok(RuntimeDispatch::NativePrivacyFilter)
            }
            PluginRuntime::Native { engine } => Err(GatewayPluginError::new(
                "PLUGIN_UNSUPPORTED_RUNTIME",
                format!("unsupported native plugin runtime engine: {engine}"),
            )),
            PluginRuntime::Wasm { .. } => Ok(RuntimeDispatch::WasmNotWired),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PluginRuntimeManager, RuntimeDispatch};
    use crate::app::plugins::runtime_policy::RuntimePolicy;
    use crate::domain::plugins::PluginRuntime;

    #[test]
    fn runtime_manager_rejects_wasm_when_policy_disabled() {
        let manager = PluginRuntimeManager::for_tests(RuntimePolicy::default());
        let runtime = PluginRuntime::Wasm {
            abi_version: "1.0.0".to_string(),
            memory_limit_bytes: Some(16 * 1024 * 1024),
        };

        let err = manager
            .validate_runtime_policy(&runtime)
            .expect_err("wasm should be disabled by default policy");

        assert_eq!(err.code(), "PLUGIN_RUNTIME_DISABLED");
        assert_eq!(
            err.to_string(),
            "PLUGIN_RUNTIME_DISABLED: wasm runtime execution is disabled by host policy"
        );
    }

    #[test]
    fn runtime_manager_allows_declarative_rules_policy() {
        let manager = PluginRuntimeManager::for_tests(RuntimePolicy::default());
        let runtime = PluginRuntime::DeclarativeRules {
            rules: vec!["rules/main.json".to_string()],
        };

        manager
            .validate_runtime_policy(&runtime)
            .expect("declarative rules should be allowed by host policy");
        assert_eq!(
            manager
                .runtime_dispatch(&runtime)
                .expect("declarative rules should resolve"),
            RuntimeDispatch::DeclarativeRules
        );
    }

    #[test]
    fn runtime_manager_returns_wasm_not_wired_decision_when_policy_enabled() {
        let manager = PluginRuntimeManager::for_tests(RuntimePolicy {
            wasm_enabled: true,
            process_enabled: false,
        });
        let runtime = PluginRuntime::Wasm {
            abi_version: "1.0.0".to_string(),
            memory_limit_bytes: Some(16 * 1024 * 1024),
        };

        assert_eq!(
            manager
                .runtime_dispatch(&runtime)
                .expect("enabled wasm policy should reach dispatch decision"),
            RuntimeDispatch::WasmNotWired
        );
    }
}
