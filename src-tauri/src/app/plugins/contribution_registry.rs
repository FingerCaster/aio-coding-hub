use std::collections::BTreeMap;

use serde::Serialize;

use crate::domain::plugin_contributions::is_known_ui_slot;
use crate::plugins::PluginStatus;
use crate::shared::error::AppError;
use crate::shared::error::AppResult;

use super::extension_protocol_bridge::ExtensionProtocolBridgeRegistry;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveUiContribution {
    pub plugin_id: String,
    pub contribution_id: String,
    pub provider_extension_namespace: Option<String>,
    pub slot_id: String,
    pub title: Option<String>,
    pub order: i32,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProviderContribution {
    pub plugin_id: String,
    pub provider_type: String,
    pub display_name: String,
    pub target_cli_keys: Vec<crate::domain::plugin_contributions::TargetCliKey>,
    pub extension_namespace: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProtocolContribution {
    pub plugin_id: String,
    pub protocol_id: String,
    pub direction: crate::domain::plugin_contributions::ProtocolDirection,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProtocolBridgeContribution {
    pub plugin_id: String,
    pub contribution_id: String,
    pub bridge_type: String,
    pub inbound_protocol: String,
    pub outbound_protocol: String,
    pub supports_streaming: Option<bool>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveCommandContribution {
    pub plugin_id: String,
    pub command: String,
    pub title: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveGatewayHookContribution {
    pub plugin_id: String,
    pub name: String,
    pub priority: i32,
    pub failure_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveContributionSnapshot {
    pub ui: Vec<ActiveUiContribution>,
    pub providers: Vec<ActiveProviderContribution>,
    pub protocols: Vec<ActiveProtocolContribution>,
    pub protocol_bridges: Vec<ActiveProtocolBridgeContribution>,
    pub commands: Vec<ActiveCommandContribution>,
    pub gateway_hooks: Vec<ActiveGatewayHookContribution>,
}

impl ActiveContributionSnapshot {
    pub fn from_plugin_details(plugins: &[crate::plugins::PluginDetail]) -> AppResult<Self> {
        let mut snapshot = Self::default();
        let mut command_owners = BTreeMap::<String, String>::new();

        for plugin in plugins {
            if plugin.summary.status != PluginStatus::Enabled {
                continue;
            }

            let plugin_id = plugin.manifest.id.as_str();
            let Some(contributes) = plugin.manifest.contributes.as_ref() else {
                continue;
            };
            let provider_extension_namespace_for_ui = match contributes.providers.as_slice() {
                [provider] => Some(provider.extension_namespace.clone()),
                _ => None,
            };

            for provider in &contributes.providers {
                ensure_contribution_id(&provider.provider_type, "provider")?;
                ensure_namespaced(plugin_id, &provider.provider_type, "provider")?;
                snapshot.providers.push(ActiveProviderContribution {
                    plugin_id: plugin_id.to_string(),
                    provider_type: provider.provider_type.clone(),
                    display_name: provider.display_name.clone(),
                    target_cli_keys: provider.target_cli_keys.clone(),
                    extension_namespace: provider.extension_namespace.clone(),
                });
            }

            for protocol in &contributes.protocols {
                ensure_contribution_id(&protocol.protocol_id, "protocol")?;
                snapshot.protocols.push(ActiveProtocolContribution {
                    plugin_id: plugin_id.to_string(),
                    protocol_id: protocol.protocol_id.clone(),
                    direction: protocol.direction.clone(),
                });
            }

            for bridge in &contributes.protocol_bridges {
                ensure_protocol_bridge_type(plugin_id, &bridge.bridge_type)?;
                snapshot
                    .protocol_bridges
                    .push(ActiveProtocolBridgeContribution {
                        plugin_id: plugin_id.to_string(),
                        contribution_id: ExtensionProtocolBridgeRegistry::contribution_id(
                            plugin_id,
                            &bridge.bridge_type,
                        ),
                        bridge_type: bridge.bridge_type.clone(),
                        inbound_protocol: bridge.inbound_protocol.clone(),
                        outbound_protocol: bridge.outbound_protocol.clone(),
                        supports_streaming: bridge.supports_streaming,
                    });
            }

            for command in &contributes.commands {
                ensure_contribution_id(&command.command, "command")?;
                if let Some(owner) =
                    command_owners.insert(command.command.clone(), plugin_id.to_string())
                {
                    return Err(AppError::new(
                        "PLUGIN_DUPLICATE_COMMAND",
                        format!(
                            "command {} is declared by both {} and {}",
                            command.command, owner, plugin_id
                        ),
                    ));
                }
                snapshot.commands.push(ActiveCommandContribution {
                    plugin_id: plugin_id.to_string(),
                    command: command.command.clone(),
                    title: command.title.clone(),
                    category: command.category.clone(),
                });
            }

            for hook in &contributes.gateway_hooks {
                ensure_contribution_id(&hook.name, "gateway hook")?;
                snapshot.gateway_hooks.push(ActiveGatewayHookContribution {
                    plugin_id: plugin_id.to_string(),
                    name: hook.name.clone(),
                    priority: hook.priority,
                    failure_policy: hook.failure_policy.clone(),
                    timeout_ms: hook.timeout_ms,
                });
            }

            for (slot_id, contributions) in &contributes.ui {
                if !is_known_ui_slot(slot_id) {
                    return Err(AppError::new(
                        "PLUGIN_UNKNOWN_UI_SLOT",
                        format!("unknown UI contribution slot: {slot_id}"),
                    ));
                }

                for contribution in contributions {
                    ensure_contribution_id(&contribution.id, "UI")?;
                    snapshot.ui.push(ActiveUiContribution {
                        plugin_id: plugin_id.to_string(),
                        contribution_id: contribution.id.clone(),
                        provider_extension_namespace: provider_extension_namespace_for_ui.clone(),
                        slot_id: slot_id.clone(),
                        title: contribution.title.clone(),
                        order: contribution.order.unwrap_or(0),
                        schema: serde_json::to_value(&contribution.schema).map_err(|error| {
                            AppError::new(
                                "PLUGIN_INVALID_UI_CONTRIBUTION",
                                format!("failed to serialize UI contribution schema: {error}"),
                            )
                        })?,
                    });
                }
            }
        }

        snapshot.ui.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.plugin_id.cmp(&right.plugin_id))
                .then_with(|| left.contribution_id.cmp(&right.contribution_id))
        });

        Ok(snapshot)
    }

    #[cfg(test)]
    pub fn ui_for_slot(&self, slot_id: &str) -> Vec<&ActiveUiContribution> {
        self.ui
            .iter()
            .filter(|item| item.slot_id == slot_id)
            .collect()
    }
}

fn ensure_contribution_id(value: &str, contribution_kind: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::new(
            "PLUGIN_INVALID_CONTRIBUTION_ID",
            format!("{contribution_kind} contribution id must be non-empty"),
        ));
    }
    Ok(())
}

fn ensure_namespaced(plugin_id: &str, value: &str, contribution_kind: &str) -> AppResult<()> {
    if is_plugin_namespaced(plugin_id, value) {
        return Ok(());
    }

    Err(AppError::new(
        "PLUGIN_CONTRIBUTION_NAMESPACE_MISMATCH",
        format!(
            "{contribution_kind} contribution id {value} must be namespaced by plugin {plugin_id}"
        ),
    ))
}

fn ensure_protocol_bridge_type(plugin_id: &str, bridge_type: &str) -> AppResult<()> {
    ensure_contribution_id(bridge_type, "protocol bridge")?;
    if !is_valid_protocol_bridge_type(bridge_type) {
        return Err(AppError::new(
            "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION",
            "protocol bridge bridgeType must use lower-case id segments",
        ));
    }
    if is_protocol_bridge_namespaced(plugin_id, bridge_type) {
        return Ok(());
    }

    Err(AppError::new(
        "PLUGIN_CONTRIBUTION_NAMESPACE_MISMATCH",
        format!(
            "protocol bridge contribution id {bridge_type} must be namespaced by plugin {plugin_id}"
        ),
    ))
}

fn is_valid_protocol_bridge_type(value: &str) -> bool {
    value.split(['.', '/', ':']).all(|segment| {
        let mut chars = segment.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        (first.is_ascii_lowercase() || first.is_ascii_digit())
            && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    })
}

fn is_protocol_bridge_namespaced(plugin_id: &str, value: &str) -> bool {
    if value == plugin_id {
        return true;
    }
    value
        .strip_prefix(plugin_id)
        .is_some_and(|suffix| matches!(suffix.as_bytes().first(), Some(b'.' | b'/' | b':')))
}

fn is_plugin_namespaced(plugin_id: &str, value: &str) -> bool {
    if value == plugin_id {
        return true;
    }
    value
        .strip_prefix(plugin_id)
        .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('/'))
}

#[cfg(test)]
mod contribution_registry_tests {
    use std::collections::BTreeMap;

    use crate::domain::plugin_contributions::{
        CommandContribution, HostRenderedSchema, PluginContributes, ProtocolBridgeContribution,
        ProviderContribution, TargetCliKey, UiContribution,
    };
    use crate::plugins::{
        PluginAuditLog, PluginDetail, PluginHostCompatibility, PluginInstallSource, PluginManifest,
        PluginPermissionRisk, PluginRuntime, PluginRuntimeFailure, PluginStatus, PluginSummary,
    };

    use super::ActiveContributionSnapshot;

    #[test]
    fn contribution_registry_filters_enabled_plugins_and_orders_ui_slots() {
        let enabled = plugin_detail_with_ui(
            "acme.settings",
            crate::plugins::PluginStatus::Enabled,
            "settings.sections",
            "settings-a",
            20,
        );
        let disabled = plugin_detail_with_ui(
            "acme.disabled",
            crate::plugins::PluginStatus::Disabled,
            "settings.sections",
            "settings-hidden",
            10,
        );
        let earlier = plugin_detail_with_ui(
            "acme.settings-earlier",
            crate::plugins::PluginStatus::Enabled,
            "settings.sections",
            "settings-b",
            5,
        );

        let snapshot =
            ActiveContributionSnapshot::from_plugin_details(&[enabled, disabled, earlier])
                .expect("snapshot");

        let ids: Vec<_> = snapshot
            .ui_for_slot("settings.sections")
            .iter()
            .map(|item| item.contribution_id.as_str())
            .collect();
        assert_eq!(ids, vec!["settings-b", "settings-a"]);
    }

    #[test]
    fn contribution_registry_rejects_unknown_slots() {
        let plugin = plugin_detail_with_raw_ui_slot("acme.bad", "settings.unknown");
        let err = ActiveContributionSnapshot::from_plugin_details(&[plugin]).unwrap_err();
        assert_eq!(err.code(), "PLUGIN_UNKNOWN_UI_SLOT");
    }

    #[test]
    fn contribution_registry_rejects_duplicate_command_ids() {
        let first = plugin_detail_with_command("acme.one", "acme.command.open");
        let second = plugin_detail_with_command("acme.two", "acme.command.open");

        let err = ActiveContributionSnapshot::from_plugin_details(&[first, second]).unwrap_err();

        assert_eq!(err.code(), "PLUGIN_DUPLICATE_COMMAND");
    }

    #[test]
    fn contribution_registry_rejects_provider_namespace_mismatch() {
        let plugin = plugin_detail_with_provider("acme.provider", "other.provider");

        let err = ActiveContributionSnapshot::from_plugin_details(&[plugin]).unwrap_err();

        assert_eq!(err.code(), "PLUGIN_CONTRIBUTION_NAMESPACE_MISMATCH");
    }

    #[test]
    fn contribution_registry_rejects_protocol_bridge_namespace_mismatch() {
        let plugin = plugin_detail_with_protocol_bridge("acme.bridge", "other.bridge");

        let err = ActiveContributionSnapshot::from_plugin_details(&[plugin]).unwrap_err();

        assert_eq!(err.code(), "PLUGIN_CONTRIBUTION_NAMESPACE_MISMATCH");
    }

    #[test]
    fn contribution_registry_rejects_invalid_protocol_bridge_type() {
        let plugin = plugin_detail_with_protocol_bridge("acme.bridge", "acme.bridge.OpenAI");

        let err = ActiveContributionSnapshot::from_plugin_details(&[plugin]).unwrap_err();

        assert_eq!(err.code(), "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION");
    }

    #[test]
    fn contribution_registry_indexes_namespaced_protocol_bridge() {
        let plugin = plugin_detail_with_protocol_bridge("acme.bridge", "acme.bridge.openai-gemini");

        let snapshot =
            ActiveContributionSnapshot::from_plugin_details(&[plugin]).expect("snapshot");

        assert_eq!(snapshot.protocol_bridges.len(), 1);
        let bridge = &snapshot.protocol_bridges[0];
        assert_eq!(bridge.plugin_id, "acme.bridge");
        assert_eq!(bridge.contribution_id, "acme.bridge.openai-gemini");
        assert_eq!(bridge.bridge_type, "acme.bridge.openai-gemini");
        assert_eq!(bridge.inbound_protocol, "claude");
        assert_eq!(bridge.outbound_protocol, "codex");
        assert_eq!(bridge.supports_streaming, Some(true));
    }

    #[test]
    fn contribution_registry_rejects_empty_contribution_id() {
        let plugin = plugin_detail_with_raw_ui_slot_and_id("acme.empty", "settings.sections", " ");

        let err = ActiveContributionSnapshot::from_plugin_details(&[plugin]).unwrap_err();

        assert_eq!(err.code(), "PLUGIN_INVALID_CONTRIBUTION_ID");
    }

    #[test]
    fn contribution_registry_sets_provider_extension_namespace_for_single_provider_ui() {
        let plugin = plugin_detail_with_provider_and_ui(
            "acme.provider",
            vec![provider_contribution("acme.provider.codex", "shared")],
            "providers.editor.sections",
            ui_contribution("routing-panel", 0),
        );

        let snapshot =
            ActiveContributionSnapshot::from_plugin_details(&[plugin]).expect("snapshot");

        let ui = snapshot.ui_for_slot("providers.editor.sections");
        assert_eq!(
            ui[0].provider_extension_namespace.as_deref(),
            Some("shared")
        );
    }

    #[test]
    fn contribution_registry_omits_provider_extension_namespace_without_exactly_one_provider() {
        let no_provider =
            plugin_detail_with_raw_ui_slot("acme.no-provider", "providers.editor.sections");
        let two_providers = plugin_detail_with_provider_and_ui(
            "acme.multi-provider",
            vec![
                provider_contribution("acme.multi-provider.codex", "codex"),
                provider_contribution("acme.multi-provider.gemini", "gemini"),
            ],
            "providers.editor.sections",
            ui_contribution("routing-panel", 0),
        );

        let snapshot =
            ActiveContributionSnapshot::from_plugin_details(&[no_provider, two_providers])
                .expect("snapshot");

        let namespaces: Vec<_> = snapshot
            .ui_for_slot("providers.editor.sections")
            .iter()
            .map(|item| item.provider_extension_namespace.as_deref())
            .collect();
        assert_eq!(namespaces, vec![None, None]);
    }

    fn plugin_detail_with_ui(
        plugin_id: &str,
        status: PluginStatus,
        slot_id: &str,
        contribution_id: &str,
        order: i32,
    ) -> PluginDetail {
        plugin_detail_with_ui_contribution(
            plugin_id,
            status,
            slot_id,
            ui_contribution(contribution_id, order),
        )
    }

    fn plugin_detail_with_raw_ui_slot(plugin_id: &str, slot_id: &str) -> PluginDetail {
        plugin_detail_with_raw_ui_slot_and_id(plugin_id, slot_id, "settings-a")
    }

    fn plugin_detail_with_raw_ui_slot_and_id(
        plugin_id: &str,
        slot_id: &str,
        contribution_id: &str,
    ) -> PluginDetail {
        plugin_detail_with_ui_contribution(
            plugin_id,
            PluginStatus::Enabled,
            slot_id,
            ui_contribution(contribution_id, 0),
        )
    }

    fn plugin_detail_with_command(plugin_id: &str, command: &str) -> PluginDetail {
        let mut detail = plugin_detail(plugin_id, PluginStatus::Enabled);
        detail.manifest.contributes = Some(PluginContributes {
            commands: vec![CommandContribution {
                command: command.to_string(),
                title: "Open".to_string(),
                category: None,
            }],
            ..empty_contributes()
        });
        detail
    }

    fn plugin_detail_with_provider(plugin_id: &str, provider_type: &str) -> PluginDetail {
        let mut detail = plugin_detail(plugin_id, PluginStatus::Enabled);
        detail.manifest.contributes = Some(PluginContributes {
            providers: vec![provider_contribution(provider_type, plugin_id)],
            ..empty_contributes()
        });
        detail
    }

    fn plugin_detail_with_provider_and_ui(
        plugin_id: &str,
        providers: Vec<ProviderContribution>,
        slot_id: &str,
        contribution: UiContribution,
    ) -> PluginDetail {
        let mut detail = plugin_detail(plugin_id, PluginStatus::Enabled);
        detail.manifest.contributes = Some(PluginContributes {
            providers,
            ui: BTreeMap::from([(slot_id.to_string(), vec![contribution])]),
            ..empty_contributes()
        });
        detail
    }

    fn provider_contribution(
        provider_type: &str,
        extension_namespace: &str,
    ) -> ProviderContribution {
        ProviderContribution {
            provider_type: provider_type.to_string(),
            display_name: "Provider".to_string(),
            target_cli_keys: vec![TargetCliKey::Codex],
            extension_namespace: extension_namespace.to_string(),
        }
    }

    fn plugin_detail_with_protocol_bridge(plugin_id: &str, bridge_type: &str) -> PluginDetail {
        let mut detail = plugin_detail(plugin_id, PluginStatus::Enabled);
        detail.manifest.contributes = Some(PluginContributes {
            protocol_bridges: vec![ProtocolBridgeContribution {
                bridge_type: bridge_type.to_string(),
                inbound_protocol: "claude".to_string(),
                outbound_protocol: "codex".to_string(),
                supports_streaming: Some(true),
            }],
            ..empty_contributes()
        });
        detail
    }

    fn plugin_detail_with_ui_contribution(
        plugin_id: &str,
        status: PluginStatus,
        slot_id: &str,
        contribution: UiContribution,
    ) -> PluginDetail {
        let mut detail = plugin_detail(plugin_id, status);
        detail.manifest.contributes = Some(PluginContributes {
            ui: BTreeMap::from([(slot_id.to_string(), vec![contribution])]),
            ..empty_contributes()
        });
        detail
    }

    fn ui_contribution(contribution_id: &str, order: i32) -> UiContribution {
        UiContribution {
            id: contribution_id.to_string(),
            title: Some("Settings".to_string()),
            order: Some(order),
            schema: HostRenderedSchema::Panel { fields: Vec::new() },
            when: None,
        }
    }

    fn empty_contributes() -> PluginContributes {
        PluginContributes {
            providers: Vec::new(),
            protocols: Vec::new(),
            protocol_bridges: Vec::new(),
            commands: Vec::new(),
            gateway_hooks: Vec::new(),
            ui: BTreeMap::new(),
        }
    }

    fn plugin_detail(plugin_id: &str, status: PluginStatus) -> PluginDetail {
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: plugin_id.to_string(),
                name: "Plugin".to_string(),
                current_version: Some("1.0.0".to_string()),
                status,
                runtime: "extensionHost".to_string(),
                permission_risk: PluginPermissionRisk::Low,
                update_available: false,
                last_error: None,
                created_at: 10,
                updated_at: 20,
            },
            manifest: PluginManifest {
                id: plugin_id.to_string(),
                name: "Plugin".to_string(),
                version: "1.0.0".to_string(),
                api_version: "1.0.0".to_string(),
                runtime: PluginRuntime::ExtensionHost {
                    language: "javascript".to_string(),
                },
                hooks: Vec::new(),
                permissions: Vec::new(),
                main: None,
                activation_events: Vec::new(),
                contributes: None,
                capabilities: Vec::new(),
                host_compatibility: PluginHostCompatibility {
                    app: ">=0.56.0 <1.0.0".to_string(),
                    plugin_api: "^1.0.0".to_string(),
                    platforms: Vec::new(),
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
            granted_permissions: Vec::new(),
            pending_permissions: Vec::new(),
            audit_logs: Vec::<PluginAuditLog>::new(),
            runtime_failures: Vec::<PluginRuntimeFailure>::new(),
            rollback_versions: Vec::new(),
        }
    }
}
