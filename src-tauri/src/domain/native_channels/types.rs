use crate::domain::native_gateway::{GatewayCatalogGroup, GeneratedEntry, NativeModelSpec};
use crate::shared::gateway_protocol::GatewayProtocol;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum SourceChannel {
    Claude,
    Codex,
    Grok,
    Gemini,
}

impl SourceChannel {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Grok, Self::Gemini];
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Gemini => "gemini",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Grok => "Grok",
            Self::Gemini => "Gemini",
        }
    }
    pub(crate) fn protocols(self) -> &'static [GatewayProtocol] {
        use GatewayProtocol::*;
        match self {
            Self::Claude => &[AnthropicMessages],
            Self::Codex => &[OpenaiResponses],
            Self::Grok => &[OpenaiCompletions, OpenaiResponses],
            Self::Gemini => &[GoogleGenerativeAi],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelProvider {
    pub provider_id: i64,
    pub provider_uuid: String,
    pub name: String,
    pub auth_mode: String,
    pub blocked_reason: Option<String>,
    pub model_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSource {
    pub source_channel: SourceChannel,
    pub protocol: GatewayProtocol,
    pub providers: Vec<ChannelProvider>,
    pub models: Vec<NativeModelSpec>,
    pub blocked_reason: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelBindingSummary {
    pub binding_id: String,
    pub source_channel: SourceChannel,
    pub protocol: GatewayProtocol,
    pub native_key: String,
    pub models: Vec<NativeModelSpec>,
    pub state: String,
    pub modified: bool,
    pub stale: bool,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCatalog {
    pub target_id: String,
    pub revision: String,
    pub catalog_revision: String,
    pub listener_ready: bool,
    pub sources: Vec<ChannelSource>,
    pub bindings: Vec<ChannelBindingSummary>,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelSelection {
    pub source_channel: SourceChannel,
    pub protocol: GatewayProtocol,
    pub model_ids: Vec<String>,
}
#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelLifecycleInput {
    pub target_id: String,
    pub expected_revision: String,
    pub catalog_revision: String,
    pub selections: Vec<ChannelSelection>,
    /// Explicit removal IDs only; absent selections do not remove other bindings.
    pub remove_binding_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPreview {
    pub target_id: String,
    pub revision: String,
    pub catalog_revision: String,
    pub entries: Vec<GeneratedEntry>,
    pub removed_native_keys: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMutationResult {
    pub target_id: String,
    pub revision: String,
    pub changed: bool,
    pub backup_path: Option<String>,
    pub bindings: Vec<ChannelBindingSummary>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelIdentity {
    pub binding_id: String,
    pub source_channel: SourceChannel,
}

pub(crate) fn selected_group(
    source: &ChannelSource,
    ids: &[String],
) -> crate::shared::error::AppResult<GatewayCatalogGroup> {
    if ids.is_empty() || ids.len() > 512 {
        return Err(super::error(
            "NATIVE_CHANNEL_INVALID_SELECTION",
            "Select explicit compatible models",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let models = ids
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(super::error(
                    "NATIVE_CHANNEL_INVALID_SELECTION",
                    "Duplicate selected model",
                ));
            }
            source
                .models
                .iter()
                .find(|model| &model.request_model_id == id)
                .cloned()
                .ok_or_else(|| {
                    super::error(
                        "NATIVE_CHANNEL_MODEL_UNAVAILABLE",
                        "Selected model no longer has compatible source candidates",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GatewayCatalogGroup {
        protocol: source.protocol,
        models,
    })
}
