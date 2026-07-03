//! Usage: Derives runtime access for Extension Host plugin contributions.

use crate::domain::plugins::{gateway_hook_effective_permissions, PluginDetail};
use crate::gateway::plugins::context::GatewayPluginHookName;

pub(crate) fn effective_hook_permissions(
    plugin: &PluginDetail,
    hook_name: GatewayPluginHookName,
) -> Vec<String> {
    gateway_hook_effective_permissions(&plugin.manifest, hook_name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin_contributions::PluginContributes;
    use crate::domain::plugins::{
        PluginDetail, PluginHook, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginStatus, PluginSummary,
    };
    use std::collections::BTreeMap;

    fn extension_host_plugin(hooks: Vec<PluginHook>, capabilities: Vec<&str>) -> PluginDetail {
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: "community.prompt-helper".to_string(),
                name: "Prompt Helper".to_string(),
                current_version: Some("1.0.0".to_string()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::Low,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: PluginManifest {
                id: "community.prompt-helper".to_string(),
                name: "Prompt Helper".to_string(),
                version: "1.0.0".to_string(),
                api_version: "1.0.0".to_string(),
                runtime: PluginRuntime::ExtensionHost {
                    language: "typescript".to_string(),
                },
                hooks: vec![],
                permissions: vec![],
                main: Some("dist/extension.js".to_string()),
                activation_events: vec![],
                contributes: Some(PluginContributes {
                    providers: vec![],
                    protocols: vec![],
                    protocol_bridges: vec![],
                    commands: vec![],
                    gateway_hooks: hooks,
                    ui: BTreeMap::new(),
                }),
                capabilities: capabilities.into_iter().map(str::to_string).collect(),
                host_compatibility: PluginHostCompatibility {
                    app: ">=0.60.0 <1.0.0".to_string(),
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
            installed_dir: None,
            config: serde_json::json!({}),
            granted_permissions: vec![],
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
            rollback_versions: vec![],
        }
    }

    #[test]
    fn extension_host_hook_access_is_derived_from_contribution_descriptor() {
        let plugin = extension_host_plugin(
            vec![PluginHook {
                name: "gateway.request.afterBodyRead".to_string(),
                priority: 10,
                failure_policy: Some("fail-open".to_string()),
                timeout_ms: None,
            }],
            vec!["gateway.hooks"],
        );

        let permissions =
            effective_hook_permissions(&plugin, GatewayPluginHookName::RequestAfterBodyRead);

        assert!(permissions.contains(&"request.body.read".to_string()));
        assert!(permissions.contains(&"request.body.write".to_string()));
    }

    #[test]
    fn extension_host_hook_access_requires_matching_capability_and_contribution() {
        let missing_capability = extension_host_plugin(
            vec![PluginHook {
                name: "gateway.request.afterBodyRead".to_string(),
                priority: 10,
                failure_policy: Some("fail-open".to_string()),
                timeout_ms: None,
            }],
            vec![],
        );
        assert!(effective_hook_permissions(
            &missing_capability,
            GatewayPluginHookName::RequestAfterBodyRead
        )
        .is_empty());

        let missing_contribution = extension_host_plugin(vec![], vec!["gateway.hooks"]);
        assert!(effective_hook_permissions(
            &missing_contribution,
            GatewayPluginHookName::RequestAfterBodyRead
        )
        .is_empty());
    }
}
