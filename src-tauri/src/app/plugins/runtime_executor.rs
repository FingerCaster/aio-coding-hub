//! Usage: Runtime dispatch for gateway plugin execution.

use crate::app::plugins::official_privacy_filter_runtime::OfficialPrivacyFilterRuntime;
use crate::app::plugins::rule_runtime::RuleRuntimeGatewayPluginExecutor;
use crate::app::plugins::runtime_manager::{PluginRuntimeManager, RuntimeDispatch};
use crate::app::plugins::runtime_policy::RuntimePolicy;
use crate::domain::plugins::PluginDetail;
use crate::gateway::plugins::context::{GatewayHookResult, GatewayVisibleHookContext};
use crate::gateway::plugins::permissions::GatewayPluginError;
use crate::gateway::plugins::pipeline::{GatewayHookFuture, GatewayPluginExecutor};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeExecutionPolicy {
    pub(crate) wasm_enabled: bool,
}

#[derive(Default)]
pub(crate) struct RuntimeGatewayPluginExecutor {
    rule_runtime: RuleRuntimeGatewayPluginExecutor,
    privacy_filter_runtime: OfficialPrivacyFilterRuntime,
    policy: RuntimeExecutionPolicy,
}

impl RuntimeGatewayPluginExecutor {
    #[cfg(test)]
    pub(crate) fn for_tests(policy: RuntimeExecutionPolicy) -> Self {
        Self {
            rule_runtime: RuleRuntimeGatewayPluginExecutor::default(),
            privacy_filter_runtime: OfficialPrivacyFilterRuntime::default(),
            policy,
        }
    }

    pub(crate) fn execute_plugin_sync(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
    ) -> Result<GatewayHookResult, GatewayPluginError> {
        let manager = PluginRuntimeManager::new(RuntimePolicy {
            wasm_enabled: self.policy.wasm_enabled,
            process_enabled: false,
        });

        match manager.runtime_dispatch(&plugin.summary.plugin_id, &plugin.manifest.runtime)? {
            RuntimeDispatch::DeclarativeRules => self
                .rule_runtime
                .execute_declarative_rules_plugin(plugin, context),
            RuntimeDispatch::NativePrivacyFilter => {
                self.privacy_filter_runtime.execute_plugin(plugin, context)
            }
            RuntimeDispatch::WasmNotWired => Err(GatewayPluginError::new(
                "PLUGIN_WASM_NOT_WIRED",
                "wasm runtime policy is enabled but gateway execution is not wired in this release",
            )),
        }
    }

    pub(crate) fn retain_runtime_caches_for_plugins(&self, plugins: &[PluginDetail]) {
        self.rule_runtime.retain_runtime_caches_for_plugins(plugins);
        self.privacy_filter_runtime
            .retain_runtime_caches_for_plugins(plugins);
    }

    #[cfg(test)]
    fn privacy_filter_cache_size_for_tests(&self) -> usize {
        self.privacy_filter_runtime.cache_size_for_tests()
    }
}

impl GatewayPluginExecutor for RuntimeGatewayPluginExecutor {
    fn retain_runtime_caches_for_plugins(&self, plugins: &[PluginDetail]) {
        self.retain_runtime_caches_for_plugins(plugins);
    }

    fn execute_request_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
    ) -> GatewayHookFuture {
        let result = self.execute_plugin_sync(plugin, context);
        Box::pin(async move { result })
    }

    fn execute_response_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
    ) -> GatewayHookFuture {
        let result = self.execute_plugin_sync(plugin, context);
        Box::pin(async move { result })
    }

    fn execute_stream_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
    ) -> GatewayHookFuture {
        let result = self.execute_plugin_sync(plugin, context);
        Box::pin(async move { result })
    }

    fn execute_log_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
    ) -> GatewayHookFuture {
        let result = self.execute_plugin_sync(plugin, context);
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugins::{
        PluginDetail, PluginHook, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginStatus, PluginSummary,
    };
    use crate::gateway::plugins::context::{
        GatewayHookAction, GatewayVisibleHookContext, GatewayVisibleLogContext,
        GatewayVisibleRequestContext, GatewayVisibleResponseContext, GatewayVisibleStreamContext,
    };
    use serde_json::json;

    #[test]
    fn runtime_executor_returns_clear_error_for_policy_disabled_wasm() {
        let executor = RuntimeGatewayPluginExecutor::for_tests(RuntimeExecutionPolicy {
            wasm_enabled: false,
        });
        let plugin = wasm_plugin_detail("example.wasm");
        let context = hook_context("gateway.request.afterBodyRead", "trace-1");

        let err = executor
            .execute_plugin_sync(&plugin, context)
            .expect_err("wasm disabled");

        assert_eq!(err.code(), "PLUGIN_RUNTIME_DISABLED");
        assert!(err.to_string().contains("wasm"));
    }

    #[test]
    fn runtime_executor_delegates_declarative_rules_to_rule_runtime() {
        let dir = tempfile::tempdir().expect("temp plugin dir");
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).expect("rules dir");
        std::fs::write(
            rules_dir.join("main.json"),
            json!({
                "rules": [{
                    "id": "no-op-warn",
                    "hook": "gateway.request.afterBodyRead",
                    "target": { "field": "request.body" },
                    "match": { "regex": "not-present" },
                    "action": { "kind": "warn", "message": "not used" }
                }]
            })
            .to_string(),
        )
        .expect("rule file");
        let plugin = rule_plugin_detail("example.rules", dir.path().to_string_lossy().to_string());
        let context = hook_context("gateway.request.afterBodyRead", "trace-2");

        let result = executor()
            .execute_plugin_sync(&plugin, context)
            .expect("rule runtime executes");

        assert_eq!(result.action, GatewayHookAction::Continue);
    }

    #[test]
    fn runtime_executor_rejects_non_official_privacy_filter_native_runtime() {
        let executor = RuntimeGatewayPluginExecutor::for_tests(RuntimeExecutionPolicy {
            wasm_enabled: false,
        });
        let plugin = plugin_detail(
            "example.privacy-filter",
            PluginRuntime::Native {
                engine: "privacyFilter".to_string(),
            },
            "native:privacyFilter".to_string(),
            None,
        );
        let context = hook_context("gateway.request.afterBodyRead", "trace-native");

        let err = executor
            .execute_plugin_sync(&plugin, context)
            .expect_err("non-official native privacy filter should be rejected");

        assert_eq!(err.code(), "PLUGIN_UNSUPPORTED_RUNTIME");
        assert_eq!(
            err.to_string(),
            "PLUGIN_UNSUPPORTED_RUNTIME: unsupported native plugin runtime engine: privacyFilter"
        );
    }

    #[test]
    fn runtime_executor_retain_prunes_official_privacy_filter_runtime_cache() {
        let executor = executor();
        let plugin = official_privacy_filter_plugin_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        }));
        let context = hook_context("log.beforePersist", "trace-privacy");

        executor
            .execute_plugin_sync(&plugin, context)
            .expect("official privacy filter runtime executes");
        assert_eq!(executor.privacy_filter_cache_size_for_tests(), 1);

        executor.retain_runtime_caches_for_plugins(&[]);

        assert_eq!(executor.privacy_filter_cache_size_for_tests(), 0);
    }

    fn executor() -> RuntimeGatewayPluginExecutor {
        RuntimeGatewayPluginExecutor::for_tests(RuntimeExecutionPolicy {
            wasm_enabled: false,
        })
    }

    fn hook_context(hook_name: &str, trace_id: &str) -> GatewayVisibleHookContext {
        GatewayVisibleHookContext {
            hook_name: hook_name.to_string(),
            trace_id: trace_id.to_string(),
            request: GatewayVisibleRequestContext {
                body: Some(
                    json!({ "messages": [{ "role": "user", "content": "hello" }] }).to_string(),
                ),
                ..GatewayVisibleRequestContext::default()
            },
            response: GatewayVisibleResponseContext::default(),
            stream: GatewayVisibleStreamContext::default(),
            log: GatewayVisibleLogContext::default(),
        }
    }

    fn wasm_plugin_detail(plugin_id: &str) -> PluginDetail {
        plugin_detail(
            plugin_id,
            PluginRuntime::Wasm {
                abi_version: "1.0.0".to_string(),
                memory_limit_bytes: Some(16 * 1024 * 1024),
            },
            "wasm".to_string(),
            None,
        )
    }

    fn rule_plugin_detail(plugin_id: &str, installed_dir: String) -> PluginDetail {
        plugin_detail(
            plugin_id,
            PluginRuntime::DeclarativeRules {
                rules: vec!["rules/main.json".to_string()],
            },
            "declarativeRules".to_string(),
            Some(installed_dir),
        )
    }

    fn official_privacy_filter_plugin_detail(config: serde_json::Value) -> PluginDetail {
        let fixture = crate::app::plugins::official::official_plugin("official.privacy-filter")
            .expect("official privacy filter fixture");
        let permissions = fixture.manifest.permissions.clone();
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: fixture.manifest.id.clone(),
                name: fixture.manifest.name.clone(),
                current_version: Some(fixture.manifest.version.clone()),
                status: PluginStatus::Enabled,
                runtime: "native:privacyFilter".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: fixture.manifest,
            install_source: PluginInstallSource::Official,
            installed_dir: Some(fixture.root_dir.to_string_lossy().to_string()),
            config,
            granted_permissions: permissions,
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
        }
    }

    fn plugin_detail(
        plugin_id: &str,
        runtime: PluginRuntime,
        runtime_summary: String,
        installed_dir: Option<String>,
    ) -> PluginDetail {
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: plugin_id.to_string(),
                name: plugin_id.to_string(),
                current_version: Some("1.0.0".to_string()),
                status: PluginStatus::Enabled,
                runtime: runtime_summary,
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: PluginManifest {
                id: plugin_id.to_string(),
                name: plugin_id.to_string(),
                version: "1.0.0".to_string(),
                api_version: "1.0.0".to_string(),
                runtime,
                hooks: vec![PluginHook {
                    name: "gateway.request.afterBodyRead".to_string(),
                    priority: 10,
                    failure_policy: Some("fail-open".to_string()),
                }],
                permissions: vec![
                    "request.body.read".to_string(),
                    "request.body.write".to_string(),
                ],
                host_compatibility: PluginHostCompatibility {
                    app: ">=0.56.0 <1.0.0".to_string(),
                    plugin_api: "^1.0.0".to_string(),
                    platforms: vec![],
                },
                entry: None,
                config_schema: None,
                config_version: None,
                description: None,
                author: None,
                homepage: None,
                repository: None,
                license: None,
                checksum: None,
                signature: None,
                category: None,
            },
            install_source: PluginInstallSource::Local,
            installed_dir,
            config: json!({}),
            granted_permissions: vec![
                "request.body.read".to_string(),
                "request.body.write".to_string(),
            ],
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
        }
    }
}
