//! Native CLI membership and edit contracts; independent of gateway protocols.

mod validation;
pub(crate) use validation::{apply_field_patches, validate_native_key, validate_provider};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum NativeClient {
    Pi,
    Omp,
}

impl NativeClient {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Omp => "omp",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum NativeTargetMode {
    #[default]
    Default,
    Custom,
    Profile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTargetSelection {
    pub client: NativeClient,
    #[serde(default)]
    pub mode: NativeTargetMode,
    pub agent_dir: Option<String>,
    pub profile: Option<String>,
}

impl NativeTargetSelection {
    pub fn default_for(client: NativeClient) -> Self {
        Self {
            client,
            mode: NativeTargetMode::Default,
            agent_dir: None,
            profile: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NativeFormat {
    Jsonc,
    Yaml,
    LegacyJson,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeTarget {
    pub target_id: String,
    pub client: NativeClient,
    pub environment: String,
    pub profile: Option<String>,
    pub agent_dir: String,
    pub models_path: String,
    pub source: String,
    pub format: NativeFormat,
    pub selected: bool,
    pub writable: bool,
    pub issue: Option<String>,
    pub shadowed_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NativeParseStatus {
    Ready,
    Missing,
    ReadOnly,
    Invalid,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum NativeProviderState {
    Present,
    Archived,
    Unknown,
}

/// Safe list projection. Never include credentials, headers, URLs or raw nodes.
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeProviderSummary {
    pub profile_uuid: Option<String>,
    pub native_key: String,
    pub display_name: String,
    pub state: NativeProviderState,
    pub managed: bool,
    pub node_digest: Option<String>,
    pub profile_revision: Option<String>,
    pub api: Option<String>,
    pub model_count: u32,
    pub api_key_configured: bool,
}

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeProvidersList {
    pub target: NativeTarget,
    pub revision: Option<String>,
    pub parse_status: NativeParseStatus,
    pub issue: Option<String>,
    pub providers: Vec<NativeProviderSummary>,
}

/// Secret-bearing: returned only by the explicit edit command. No Debug.
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeProviderEdit {
    pub target: NativeTarget,
    pub revision: String,
    pub provider: NativeProviderSummary,
    pub node: Value,
}

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeFieldPatch {
    pub path: Vec<String>,
    pub value: Option<Value>,
}

#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeProviderSaveInput {
    pub target_id: String,
    pub native_key: String,
    pub display_name: String,
    pub expected_revision: String,
    pub expected_node_digest: Option<String>,
    pub expected_profile_revision: Option<String>,
    pub node: Option<Value>,
    #[serde(default)]
    pub patch: Vec<NativeFieldPatch>,
    pub apply: bool,
}

#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeProviderActionInput {
    pub target_id: String,
    pub native_key: String,
    pub expected_revision: String,
    pub expected_node_digest: Option<String>,
    pub expected_profile_revision: Option<String>,
}

#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeProviderDeleteInput {
    pub target_id: String,
    pub native_key: String,
    pub expected_revision: String,
    pub expected_node_digest: Option<String>,
    pub expected_profile_revision: Option<String>,
    pub remove_from_native: bool,
}

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeMutationResult {
    pub target_id: String,
    pub revision: String,
    pub changed: bool,
    pub backup_path: Option<String>,
    pub provider: Option<NativeProviderSummary>,
}
