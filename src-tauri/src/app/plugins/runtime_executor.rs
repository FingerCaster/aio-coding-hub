//! Usage: Runtime dispatch for gateway plugin execution.

use crate::app::plugins::extension_host_registry::{
    ExtensionHostInstanceLifecycleRegistry, ExtensionHostInstanceRegistry,
};
use crate::app::plugins::privacy_redaction_service::PrivacyRedactionService;
use crate::app::plugins::runtime_lifecycle::RuntimeLifecycleRegistry;
use crate::app::plugins::runtime_manager::{PluginRuntimeManager, RuntimeDispatch};
use crate::db;
use crate::domain::plugins::PluginDetail;
#[cfg(test)]
use crate::gateway::plugins::context::GatewayHookResult;
use crate::gateway::plugins::context::GatewayVisibleHookContext;
use crate::gateway::plugins::permissions::GatewayPluginError;
use crate::gateway::plugins::pipeline::{GatewayHookFuture, GatewayPluginExecutor};
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct RuntimeGatewayPluginExecutor {
    lifecycle: RuntimeLifecycleRegistry,
    extension_host_registry: Option<Arc<ExtensionHostInstanceRegistry>>,
}

impl RuntimeGatewayPluginExecutor {
    pub(crate) fn new() -> Self {
        Self::with_extension_host_registry(None, Arc::new(PrivacyRedactionService::default()))
    }

    pub(crate) fn with_db(db: db::Db) -> Self {
        let privacy_redaction = Arc::new(PrivacyRedactionService::default());
        Self::with_extension_host_registry(
            Some(Arc::new(
                ExtensionHostInstanceRegistry::new_with_privacy_redaction(
                    db,
                    privacy_redaction.clone(),
                ),
            )),
            privacy_redaction,
        )
    }

    fn with_extension_host_registry(
        extension_host_registry: Option<Arc<ExtensionHostInstanceRegistry>>,
        privacy_redaction: Arc<PrivacyRedactionService>,
    ) -> Self {
        let lifecycle = RuntimeLifecycleRegistry::default();
        lifecycle.register_cache(privacy_redaction.clone());
        if let Some(registry) = extension_host_registry.clone() {
            lifecycle.register_instance_registry(Arc::new(
                ExtensionHostInstanceLifecycleRegistry::new(registry),
            ));
        }
        Self {
            lifecycle,
            extension_host_registry,
        }
    }

    #[cfg(test)]
    fn for_tests_with_extension_host_registry(
        extension_host_registry: Arc<ExtensionHostInstanceRegistry>,
    ) -> Self {
        Self::with_extension_host_registry(
            Some(extension_host_registry),
            Arc::new(PrivacyRedactionService::default()),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_tests() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = crate::db::init_for_tests(&temp.path().join("runtime-executor.db"))
            .expect("init test db");
        Self::with_db(db)
    }

    #[cfg(test)]
    pub(crate) fn execute_plugin_sync(
        &self,
        plugin: &PluginDetail,
        _context: GatewayVisibleHookContext,
    ) -> Result<GatewayHookResult, GatewayPluginError> {
        let manager = PluginRuntimeManager::new();

        match manager.runtime_dispatch(&plugin.summary.plugin_id, &plugin.manifest.runtime)? {
            RuntimeDispatch::ExtensionHost => {
                ensure_gateway_hooks_capability(plugin)?;
                Err(GatewayPluginError::new(
                    "PLUGIN_EXTENSION_HOST_GATEWAY_ASYNC_REQUIRED",
                    "extension host gateway hook execution requires async dispatch",
                ))
            }
        }
    }

    fn execute_plugin(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
        hook_timeout: Duration,
    ) -> GatewayHookFuture {
        let manager = PluginRuntimeManager::new();
        match manager.runtime_dispatch(&plugin.summary.plugin_id, &plugin.manifest.runtime) {
            Ok(RuntimeDispatch::ExtensionHost) => {
                if let Err(err) = ensure_gateway_hooks_capability(plugin) {
                    return Box::pin(async move { Err(err) });
                }
                let Some(registry) = self.extension_host_registry.clone() else {
                    return Box::pin(async {
                        Err(GatewayPluginError::new(
                            "PLUGIN_EXTENSION_HOST_GATEWAY_NOT_CONFIGURED",
                            "extension host gateway hook registry is not configured",
                        ))
                    });
                };
                let detail = plugin.clone();
                let hook = context.hook_name.clone();
                Box::pin(async move {
                    registry
                        .execute_gateway_hook(detail, &hook, context, hook_timeout)
                        .await
                })
            }
            Err(err) => Box::pin(async move { Err(err) }),
        }
    }

    pub(crate) fn retain_runtime_caches_for_plugins(&self, plugins: &[PluginDetail]) {
        self.lifecycle.retain_for_plugins(plugins);
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn dispose_runtime_caches_for_tests(&self) {
        self.lifecycle.dispose_all();
    }

    #[cfg(test)]
    pub(crate) async fn dispose_extension_host_instances_for_tests(&self) {
        if let Some(registry) = self.extension_host_registry.as_ref() {
            registry.dispose_all().await;
        }
    }
}

impl Default for RuntimeGatewayPluginExecutor {
    fn default() -> Self {
        Self::new()
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
        hook_timeout: Duration,
    ) -> GatewayHookFuture {
        self.execute_plugin(plugin, context, hook_timeout)
    }

    fn execute_response_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
        hook_timeout: Duration,
    ) -> GatewayHookFuture {
        self.execute_plugin(plugin, context, hook_timeout)
    }

    fn execute_stream_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
        hook_timeout: Duration,
    ) -> GatewayHookFuture {
        self.execute_plugin(plugin, context, hook_timeout)
    }

    fn execute_log_hook(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
        hook_timeout: Duration,
    ) -> GatewayHookFuture {
        self.execute_plugin(plugin, context, hook_timeout)
    }
}

fn ensure_gateway_hooks_capability(plugin: &PluginDetail) -> Result<(), GatewayPluginError> {
    if plugin
        .manifest
        .capabilities
        .iter()
        .any(|capability| capability == "gateway.hooks")
    {
        Ok(())
    } else {
        Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            "extension host gateway hooks require gateway.hooks capability",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin_contributions::PluginContributes;
    use crate::domain::plugins::{
        PluginDetail, PluginHook, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginStatus, PluginSummary,
    };
    use crate::gateway::plugins::context::{
        GatewayHookResult, GatewayPluginHookName, GatewayRequestHookInput,
        GatewayVisibleHookContext, GatewayVisibleLogContext, GatewayVisibleRequestContext,
        GatewayVisibleResponseContext, GatewayVisibleStreamContext,
    };
    use crate::gateway::plugins::pipeline::{GatewayPluginPipeline, GatewayPluginPipelineConfig};
    use axum::body::Bytes;
    use axum::http::{HeaderMap, Method};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn runtime_executor_rejects_extension_host_gateway_hook_without_capability() {
        let mut plugin = extension_host_plugin_detail("example.extension");
        plugin.manifest.capabilities.clear();
        let context = hook_context("gateway.request.afterBodyRead", "trace-extension");

        let err = executor()
            .execute_plugin_sync(&plugin, context)
            .expect_err("extension host gateway hooks require gateway.hooks capability");

        assert_eq!(err.code(), "PLUGIN_PERMISSION_DENIED");
    }

    #[tokio::test]
    async fn runtime_executor_extension_host_request_continue_maps_to_unchanged_result() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.request.afterBodyRead",
            r#"{ action: "continue" }"#,
        );
        let plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        let context = hook_context("gateway.request.afterBodyRead", "trace-extension");

        let result = executor()
            .execute_request_hook(&plugin, context, test_hook_timeout())
            .await
            .expect("extension host gateway hook executes");

        assert_eq!(result, GatewayHookResult::continue_unchanged());
    }

    #[tokio::test]
    async fn runtime_executor_extension_host_request_replace_maps_request_body() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.request.afterBodyRead",
            r#"{ action: "replace", requestBody: "{\"messages\":[]}" }"#,
        );
        let plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        let context = hook_context("gateway.request.afterBodyRead", "trace-extension");

        let result = executor()
            .execute_request_hook(&plugin, context, test_hook_timeout())
            .await
            .expect("extension host gateway hook executes");

        assert_eq!(result.request_body.as_deref(), Some(r#"{"messages":[]}"#));
    }

    #[tokio::test]
    async fn runtime_executor_extension_host_request_hooks_receive_large_bodies_without_truncation()
    {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.request.afterBodyRead",
            r#"(() => {
                const body = arguments[0].context.request.body;
                if (arguments[0].context.request.body_truncated) {
                  return { action: "block", reason: "body was truncated" };
                }
                return {
                  action: "replace",
                  requestBody: body.replace("13344441520", "[电话]")
                };
            })()"#,
        );
        let plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        let body = json!({
            "messages": [{
                "role": "user",
                "content": format!(
                    "{} 你知道 13344441520 是哪里的手机号嘛",
                    "x".repeat(300 * 1024)
                )
            }]
        })
        .to_string();
        let pipeline = GatewayPluginPipeline::for_tests(
            vec![plugin],
            Arc::new(RuntimeGatewayPluginExecutor::for_tests()),
            GatewayPluginPipelineConfig::default(),
        );

        let output = pipeline
            .run_request_hook(GatewayRequestHookInput {
                hook_name: GatewayPluginHookName::RequestAfterBodyRead,
                trace_id: "trace-extension-large-body".to_string(),
                cli_key: "codex".to_string(),
                method: Method::POST,
                path: "/v1/responses".to_string(),
                query: None,
                headers: HeaderMap::new(),
                body: Bytes::from(body),
                requested_model: None,
            })
            .await
            .expect("large extension host request body should be available to plugins");
        let redacted = String::from_utf8(output.body.to_vec()).expect("utf8 body");

        assert!(redacted.contains("[电话]"));
        assert!(!redacted.contains("13344441520"));
        assert!(output.blocked.is_none());
    }

    #[tokio::test]
    async fn runtime_executor_extension_host_response_warn_maps_reason() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.response.after",
            r#"{ action: "warn", message: "response looked risky" }"#,
        );
        let mut plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        plugin
            .manifest
            .contributes
            .as_mut()
            .expect("contributes")
            .gateway_hooks[0]
            .name = "gateway.response.after".to_string();
        let context = hook_context("gateway.response.after", "trace-extension");

        let result = executor()
            .execute_response_hook(&plugin, context, test_hook_timeout())
            .await
            .expect("extension host gateway hook executes");

        assert_eq!(result.reason.as_deref(), Some("response looked risky"));
        assert_eq!(result.request_body, None);
        assert_eq!(result.response_body, None);
    }

    #[tokio::test]
    async fn runtime_executor_extension_host_unsupported_action_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.request.afterBodyRead",
            r#"{ action: "teleport" }"#,
        );
        let plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        let context = hook_context("gateway.request.afterBodyRead", "trace-extension");

        let err = executor()
            .execute_request_hook(&plugin, context, test_hook_timeout())
            .await
            .expect_err("unsupported gateway hook action should be rejected");

        assert_eq!(err.code(), "PLUGIN_EXTENSION_HOST_INVALID_OUTPUT");
    }

    #[tokio::test]
    async fn runtime_executor_extension_host_pipeline_timeout_kills_warm_instance() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.request.afterBodyRead",
            r#"(() => {
                globalThis.__gatewayHookCalls = (globalThis.__gatewayHookCalls || 0) + 1;
                if (globalThis.__gatewayHookCalls === 2) {
                    while (true) {}
                }
                return { action: "continue" };
            })()"#,
        );
        let plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        let pipeline = GatewayPluginPipeline::for_tests(
            vec![plugin],
            Arc::new(RuntimeGatewayPluginExecutor::for_tests()),
            GatewayPluginPipelineConfig::default(),
        );
        let input = || request_input_for_pipeline();

        pipeline
            .run_request_hook(input())
            .await
            .expect("first extension host hook should warm an instance");
        let timed_out = pipeline
            .run_request_hook(input())
            .await
            .expect("extension host timeout should fail open through pipeline");

        assert_eq!(timed_out.body.as_ref(), b"hello");
        assert_eq!(timed_out.execution_reports.len(), 1);
        assert_eq!(
            timed_out.execution_reports[0].error_code.as_deref(),
            Some("PLUGIN_EXTENSION_HOST_TIMEOUT")
        );

        let recovered = pipeline
            .run_request_hook(input())
            .await
            .expect("timeout should kill the warm instance so a fresh instance can run");
        assert_eq!(recovered.body.as_ref(), b"hello");
        assert_eq!(recovered.execution_reports[0].status, "completed");
    }

    #[tokio::test]
    async fn runtime_executor_retain_prunes_extension_host_gateway_instances() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gateway_extension_plugin(
            temp.path(),
            "gateway.request.afterBodyRead",
            r#"{ action: "continue" }"#,
        );
        let plugin = extension_host_plugin_detail_with_root("example.extension", temp.path());
        let registry = Arc::new(ExtensionHostInstanceRegistry::new_real_for_tests());
        let executor =
            RuntimeGatewayPluginExecutor::for_tests_with_extension_host_registry(registry.clone());
        let context = hook_context("gateway.request.afterBodyRead", "trace-extension");

        executor
            .execute_request_hook(&plugin, context, test_hook_timeout())
            .await
            .expect("extension host gateway hook warms an instance");
        assert_eq!(registry.instance_count_for_tests().await, 1);

        executor.retain_runtime_caches_for_plugins(&[]);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if registry.instance_count_for_tests().await == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("retain should dispose inactive extension host gateway instances");
    }

    fn executor() -> RuntimeGatewayPluginExecutor {
        RuntimeGatewayPluginExecutor::for_tests()
    }

    fn test_hook_timeout() -> Duration {
        Duration::from_secs(5)
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

    fn request_input_for_pipeline() -> GatewayRequestHookInput {
        GatewayRequestHookInput {
            hook_name: GatewayPluginHookName::RequestAfterBodyRead,
            trace_id: "trace-extension-pipeline".to_string(),
            cli_key: "codex".to_string(),
            method: Method::POST,
            path: "/v1/responses".to_string(),
            query: None,
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"hello"),
            requested_model: None,
        }
    }

    fn extension_host_plugin_detail(plugin_id: &str) -> PluginDetail {
        plugin_detail(
            plugin_id,
            PluginRuntime::ExtensionHost {
                language: "typescript".to_string(),
            },
            "extensionHost".to_string(),
            None,
        )
    }

    fn extension_host_plugin_detail_with_root(plugin_id: &str, root: &Path) -> PluginDetail {
        plugin_detail(
            plugin_id,
            PluginRuntime::ExtensionHost {
                language: "typescript".to_string(),
            },
            "extensionHost".to_string(),
            Some(root.to_string_lossy().to_string()),
        )
    }

    fn write_gateway_extension_plugin(root: &Path, hook_name: &str, result_source: &str) {
        std::fs::create_dir_all(root.join("dist")).expect("create dist");
        let manifest = json!({
            "id": "example.extension",
            "name": "Example Extension",
            "version": "1.0.0",
            "apiVersion": "1.0.0",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "main": "dist/index.js",
            "contributes": {
                "gatewayHooks": [{ "name": hook_name, "priority": 10, "failurePolicy": "fail-open" }]
            },
            "capabilities": ["gateway.hooks"],
            "hostCompatibility": { "app": ">=0.56.0 <1.0.0", "pluginApi": "^1.0.0" }
        });
        std::fs::write(
            root.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .expect("write plugin manifest");
        std::fs::write(
            root.join("dist/index.js"),
            format!(
                r#"
                module.exports.activate = function(api) {{
                  api.gateway.registerHook("{hook_name}", function() {{
                    return {result_source};
                  }});
                }};
                "#
            ),
        )
        .expect("write extension");
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
                hooks: vec![],
                permissions: vec![],
                main: Some("dist/index.js".to_string()),
                activation_events: vec![],
                contributes: Some(PluginContributes {
                    providers: vec![],
                    protocols: vec![],
                    protocol_bridges: vec![],
                    commands: vec![],
                    gateway_hooks: vec![PluginHook {
                        name: "gateway.request.afterBodyRead".to_string(),
                        priority: 10,
                        failure_policy: Some("fail-open".to_string()),
                        timeout_ms: None,
                    }],
                    ui: BTreeMap::new(),
                }),
                capabilities: vec!["gateway.hooks".to_string()],
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
            rollback_versions: vec![],
        }
    }
}
