use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::plugins::PluginHook;

pub(crate) const ACTIVE_UI_SLOTS: &[&str] = &[
    "app.sidebar.items",
    "home.overview.cards",
    "providers.editor.sections",
    "providers.editor.fields",
    "providers.card.badges",
    "providers.card.actions",
    "settings.sections",
    "logs.detail.tabs",
    "logs.detail.actions",
    "usage.panels",
    "plugins.detail.panels",
];

const ACTIVE_CAPABILITIES: &[&str] = &[
    "commands.execute",
    "storage.plugin",
    "diagnostics.read",
    "provider.extensionValues",
    "provider.requestPreparation",
    "provider.modelDiscovery",
    "provider.healthCheck",
    "protocol.bridge",
    "gateway.hooks",
    "privacy.redact",
];

pub fn is_known_ui_slot(slot: &str) -> bool {
    ACTIVE_UI_SLOTS.contains(&slot)
}

pub fn is_known_capability(capability: &str) -> bool {
    ACTIVE_CAPABILITIES.contains(&capability)
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginContributes {
    #[serde(default)]
    pub providers: Vec<ProviderContribution>,
    #[serde(default)]
    pub protocols: Vec<ProtocolContribution>,
    #[serde(rename = "protocolBridges")]
    #[serde(default)]
    pub protocol_bridges: Vec<ProtocolBridgeContribution>,
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    #[serde(rename = "gatewayHooks")]
    #[serde(default)]
    pub gateway_hooks: Vec<PluginHook>,
    #[serde(default)]
    pub ui: BTreeMap<String, Vec<UiContribution>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderContribution {
    #[serde(rename = "providerType")]
    pub provider_type: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "targetCliKeys")]
    pub target_cli_keys: Vec<TargetCliKey>,
    #[serde(rename = "extensionNamespace")]
    pub extension_namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetCliKey {
    Claude,
    Codex,
    Gemini,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolContribution {
    #[serde(rename = "protocolId")]
    pub protocol_id: String,
    pub direction: ProtocolDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolDirection {
    Inbound,
    Outbound,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolBridgeContribution {
    #[serde(rename = "bridgeType")]
    pub bridge_type: String,
    #[serde(rename = "inboundProtocol")]
    pub inbound_protocol: String,
    #[serde(rename = "outboundProtocol")]
    pub outbound_protocol: String,
    #[serde(rename = "supportsStreaming")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_streaming: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandContribution {
    pub command: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiContribution {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    pub schema: HostRenderedSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostRenderedSchema {
    Section {
        fields: Vec<HostRenderedField>,
    },
    Panel {
        fields: Vec<HostRenderedField>,
    },
    Badge {
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<HostRenderedBadgeTone>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HostRenderedBadgeTone {
    Neutral,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostRenderedField {
    Text {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
    },
    Password {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
    },
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
    },
    Boolean {
        key: String,
        label: String,
    },
    Select {
        key: String,
        label: String,
        options: Vec<HostRenderedSelectOption>,
    },
    Textarea {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u32>,
    },
    Info {
        key: String,
        label: String,
        value: String,
    },
    Button {
        key: String,
        label: String,
        command: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostRenderedSelectOption {
    pub value: String,
    pub label: String,
}
