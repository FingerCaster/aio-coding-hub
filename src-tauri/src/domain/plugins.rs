use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::plugin_contributions::{
    is_known_capability, is_known_ui_slot, HostRenderedField, HostRenderedSchema,
    PluginContributes, UiContribution,
};

pub type PluginId = String;

const SUPPORTED_PLUGIN_API_MAJOR: u64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginManifest {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub runtime: PluginRuntime,
    #[serde(default)]
    pub hooks: Vec<PluginHook>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    #[serde(rename = "activationEvents")]
    #[serde(default)]
    pub activation_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contributes: Option<PluginContributes>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(rename = "hostCompatibility")]
    pub host_compatibility: PluginHostCompatibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(rename = "configSchema")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    #[serde(rename = "configVersion")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PluginRuntime {
    ExtensionHost { language: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct PluginHook {
    pub name: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(rename = "failurePolicy")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_policy: Option<String>,
    #[serde(rename = "timeoutMs")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct PluginHostCompatibility {
    pub app: String,
    #[serde(rename = "pluginApi")]
    pub plugin_api: String,
    #[serde(default)]
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Available,
    Installed,
    Enabled,
    Disabled,
    UpdateAvailable,
    Incompatible,
    Quarantined,
    Uninstalled,
}

impl PluginStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Installed => "installed",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::UpdateAvailable => "update_available",
            Self::Incompatible => "incompatible",
            Self::Quarantined => "quarantined",
            Self::Uninstalled => "uninstalled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "available" => Some(Self::Available),
            "installed" => Some(Self::Installed),
            "enabled" => Some(Self::Enabled),
            "disabled" => Some(Self::Disabled),
            "update_available" => Some(Self::UpdateAvailable),
            "incompatible" => Some(Self::Incompatible),
            "quarantined" => Some(Self::Quarantined),
            "uninstalled" => Some(Self::Uninstalled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginInstallSource {
    Local,
    Market,
    GithubRelease,
    Offline,
    Official,
}

impl PluginInstallSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Market => "market",
            Self::GithubRelease => "github_release",
            Self::Offline => "offline",
            Self::Official => "official",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "local" => Some(Self::Local),
            "market" => Some(Self::Market),
            "github_release" => Some(Self::GithubRelease),
            "offline" => Some(Self::Offline),
            "official" => Some(Self::Official),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermissionRisk {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct PluginValidationError {
    pub code: String,
    pub message: String,
}

impl PluginValidationError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginSummary {
    pub id: i64,
    pub plugin_id: String,
    pub name: String,
    pub current_version: Option<String>,
    pub status: PluginStatus,
    pub runtime: String,
    pub permission_risk: PluginPermissionRisk,
    pub update_available: bool,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginDetail {
    pub summary: PluginSummary,
    pub manifest: PluginManifest,
    pub install_source: PluginInstallSource,
    pub installed_dir: Option<String>,
    pub config: serde_json::Value,
    pub granted_permissions: Vec<String>,
    pub pending_permissions: Vec<String>,
    pub audit_logs: Vec<PluginAuditLog>,
    pub runtime_failures: Vec<PluginRuntimeFailure>,
    pub rollback_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginAuditLog {
    pub id: i64,
    pub plugin_id: Option<String>,
    pub trace_id: Option<String>,
    pub event_type: String,
    pub risk_level: String,
    pub message: String,
    pub details: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginRuntimeFailure {
    pub id: i64,
    pub plugin_id: String,
    pub hook_name: Option<String>,
    pub failure_kind: String,
    pub message: String,
    pub trace_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
pub struct PluginHookExecutionReport {
    pub id: i64,
    pub plugin_id: String,
    pub trace_id: Option<String>,
    pub hook_name: String,
    pub runtime_kind: String,
    pub status: String,
    pub started_at_ms: i64,
    pub duration_ms: i64,
    pub failure_kind: Option<String>,
    pub error_code: Option<String>,
    pub failure_policy: Option<String>,
    pub circuit_state: Option<String>,
    pub context_budget: serde_json::Value,
    pub output_budget: serde_json::Value,
    pub mutation_summary: serde_json::Value,
    pub replayable: bool,
    pub replay_export_reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginExtensionExecutionReport {
    pub id: i64,
    pub plugin_id: String,
    pub contribution_type: String,
    pub contribution_id: String,
    pub command_or_hook: Option<String>,
    pub trace_id: Option<String>,
    pub status: String,
    pub started_at_ms: i64,
    pub duration_ms: i64,
    pub failure_kind: Option<String>,
    pub error_code: Option<String>,
    pub input_budget: serde_json::Value,
    pub output_budget: serde_json::Value,
    pub mutation_summary: serde_json::Value,
    pub replayable: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginReplayFixture {
    pub schema_version: u32,
    pub trace_id: String,
    pub source: PluginReplayFixtureSource,
    pub hook_name: String,
    pub plugin_id: Option<String>,
    pub request: PluginReplayFixtureRequest,
    pub response: PluginReplayFixtureResponse,
    pub log: PluginReplayFixtureLog,
    pub attempts: Vec<PluginReplayFixtureAttempt>,
    pub runtime_reports: Vec<PluginHookExecutionReport>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginReplayFixtureSource {
    pub app_version: String,
    pub trace_id: String,
    pub exported_at_ms: i64,
    pub request_log_id: i64,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginReplayFixtureRequest {
    pub cli_key: String,
    pub session_id: Option<String>,
    pub method: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub provider: Option<String>,
    pub provider_source: Option<String>,
    pub model: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub body: Option<serde_json::Value>,
    pub normalized_messages: Vec<serde_json::Value>,
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginReplayFixtureResponse {
    pub status: Option<i64>,
    pub error_code: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub body: Option<serde_json::Value>,
    pub chunks: Vec<serde_json::Value>,
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginReplayFixtureLog {
    pub body: Option<serde_json::Value>,
    pub meta: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginReplayFixtureAttempt {
    pub id: i64,
    pub trace_id: String,
    pub cli_key: String,
    pub attempt_index: i64,
    pub provider_id: i64,
    pub provider_name: String,
    pub base_url: String,
    pub outcome: String,
    pub status: Option<i64>,
    pub attempt_started_ms: i64,
    pub attempt_duration_ms: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLifecycleNotice {
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeLifecycleSummary {
    pub kind: String,
    pub label: String,
    pub supported: bool,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookLifecycleSummary {
    pub name: String,
    pub priority: i32,
    pub failure_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionLifecycleSummary {
    pub permission: String,
    pub risk: PluginPermissionRisk,
    pub granted: bool,
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCompatibilitySummary {
    pub compatible: bool,
    pub host_version: String,
    pub app_range: String,
    pub plugin_api_range: String,
    pub platforms: Vec<String>,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginTrustSummary {
    pub checksum: String,
    pub expected_checksum: Option<String>,
    pub checksum_verified: bool,
    pub signature_verified: bool,
    pub unsigned: bool,
    pub developer_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributionImpact {
    pub providers: Vec<PluginContributionImpactItem>,
    pub protocols: Vec<PluginContributionImpactItem>,
    pub protocol_bridges: Vec<PluginContributionImpactItem>,
    pub ui_slots: Vec<PluginUiSlotImpact>,
    pub commands: Vec<PluginCommandImpact>,
    pub gateway: Vec<PluginContributionImpactItem>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributionImpactItem {
    pub id: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginUiSlotImpact {
    pub slot_id: String,
    pub contribution_id: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandImpact {
    pub command: String,
    pub title: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallPreview {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub source: PluginInstallSource,
    pub description: Option<String>,
    pub author: Option<serde_json::Value>,
    pub homepage: Option<String>,
    pub repository: Option<serde_json::Value>,
    pub license: Option<String>,
    pub category: Option<String>,
    pub runtime: PluginRuntimeLifecycleSummary,
    pub hooks: Vec<PluginHookLifecycleSummary>,
    pub permissions: Vec<PluginPermissionLifecycleSummary>,
    pub contribution_impact: PluginContributionImpact,
    pub compatibility: PluginCompatibilitySummary,
    pub trust: PluginTrustSummary,
    pub existing_status: Option<PluginStatus>,
    pub existing_version: Option<String>,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
    pub warnings: Vec<PluginLifecycleNotice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLifecycleChange {
    pub name: String,
    pub change: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionLifecycleChange {
    pub permission: String,
    pub risk: PluginPermissionRisk,
    pub change: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributionChange {
    pub kind: String,
    pub name: String,
    pub label: Option<String>,
    pub change: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateDiff {
    pub plugin_id: String,
    pub from_version: String,
    pub to_version: String,
    pub version_direction: String,
    pub runtime_change: Option<PluginLifecycleChange>,
    pub hook_changes: Vec<PluginLifecycleChange>,
    pub permission_changes: Vec<PluginPermissionLifecycleChange>,
    pub contribution_changes: Vec<PluginContributionChange>,
    pub config_version_change: Option<String>,
    pub compatibility: PluginCompatibilitySummary,
    pub trust: PluginTrustSummary,
    pub rollback_available: bool,
    pub blocking_reasons: Vec<PluginLifecycleNotice>,
    pub warnings: Vec<PluginLifecycleNotice>,
}

impl From<PluginValidationError> for crate::shared::error::AppError {
    fn from(value: PluginValidationError) -> Self {
        crate::shared::error::AppError::new(value.code, value.message)
    }
}

pub fn validate_manifest(
    manifest: &PluginManifest,
    host_version: &str,
) -> Result<(), PluginValidationError> {
    validate_manifest_identity(manifest)?;
    match &manifest.runtime {
        PluginRuntime::ExtensionHost { language } => {
            validate_extension_host_manifest(manifest, language)?;
        }
    }
    validate_config_schema(manifest.config_schema.as_ref())?;
    validate_host_compatibility(&manifest.host_compatibility, host_version)?;
    Ok(())
}

pub fn validate_manifest_for_official_plugin(
    manifest: &PluginManifest,
    host_version: &str,
) -> Result<(), PluginValidationError> {
    validate_manifest_identity(manifest)?;
    match &manifest.runtime {
        PluginRuntime::ExtensionHost { language } => {
            validate_extension_host_manifest(manifest, language)?;
        }
    }
    validate_config_schema(manifest.config_schema.as_ref())?;
    validate_host_compatibility(&manifest.host_compatibility, host_version)?;
    Ok(())
}

fn validate_manifest_identity(manifest: &PluginManifest) -> Result<(), PluginValidationError> {
    validate_plugin_id(&manifest.id)?;
    validate_semver(&manifest.version, "PLUGIN_INVALID_VERSION")?;
    validate_manifest_api_version(&manifest.api_version)
}

pub fn permission_risk(permission: &str) -> Option<PluginPermissionRisk> {
    match permission {
        "request.meta.read" => Some(PluginPermissionRisk::Low),
        "request.header.read" => Some(PluginPermissionRisk::Medium),
        "request.header.readSensitive" => Some(PluginPermissionRisk::High),
        "request.header.write" => Some(PluginPermissionRisk::High),
        "request.body.read" => Some(PluginPermissionRisk::High),
        "request.body.write" => Some(PluginPermissionRisk::High),
        "response.header.read" => Some(PluginPermissionRisk::Low),
        "response.header.write" => Some(PluginPermissionRisk::Medium),
        "response.body.read" => Some(PluginPermissionRisk::High),
        "response.body.write" => Some(PluginPermissionRisk::High),
        "stream.inspect" => Some(PluginPermissionRisk::High),
        "stream.modify" => Some(PluginPermissionRisk::High),
        "log.redact" => Some(PluginPermissionRisk::Medium),
        "plugin.storage" => Some(PluginPermissionRisk::Medium),
        "network.fetch" => Some(PluginPermissionRisk::High),
        "file.read" => Some(PluginPermissionRisk::High),
        "file.write" => Some(PluginPermissionRisk::High),
        "secret.read" => Some(PluginPermissionRisk::Critical),
        _ => None,
    }
}

pub fn manifest_effective_permissions(manifest: &PluginManifest) -> Vec<String> {
    extension_host_effective_permissions(manifest)
}

pub fn gateway_hook_effective_permissions(
    manifest: &PluginManifest,
    hook_name: &str,
) -> Vec<String> {
    extension_host_hook_permissions(manifest, hook_name)
}

pub fn manifest_permission_risk(manifest: &PluginManifest) -> PluginPermissionRisk {
    manifest_effective_permissions(manifest)
        .iter()
        .filter_map(|permission| permission_risk(permission))
        .max_by_key(|risk| match risk {
            PluginPermissionRisk::Low => 0,
            PluginPermissionRisk::Medium => 1,
            PluginPermissionRisk::High => 2,
            PluginPermissionRisk::Critical => 3,
        })
        .unwrap_or(PluginPermissionRisk::Low)
}

fn extension_host_effective_permissions(manifest: &PluginManifest) -> Vec<String> {
    if !manifest
        .capabilities
        .iter()
        .any(|capability| capability == "gateway.hooks")
    {
        return Vec::new();
    }
    let Some(contributes) = manifest.contributes.as_ref() else {
        return Vec::new();
    };

    let mut permissions = contributes
        .gateway_hooks
        .iter()
        .flat_map(|hook| extension_host_hook_permissions(manifest, &hook.name))
        .collect::<Vec<_>>();
    permissions.sort();
    permissions.dedup();
    permissions
}

fn extension_host_hook_permissions(manifest: &PluginManifest, hook_name: &str) -> Vec<String> {
    if !manifest
        .capabilities
        .iter()
        .any(|capability| capability == "gateway.hooks")
    {
        return Vec::new();
    }
    let Some(contributes) = manifest.contributes.as_ref() else {
        return Vec::new();
    };
    if !contributes
        .gateway_hooks
        .iter()
        .any(|hook| hook.name == hook_name)
    {
        return Vec::new();
    }
    let Some(contract) = crate::gateway::plugins::contract::hook_contract(hook_name) else {
        return Vec::new();
    };

    let mut permissions = contract
        .read_permissions
        .iter()
        .chain(contract.write_permissions.iter())
        .map(|permission| (*permission).to_string())
        .collect::<Vec<_>>();
    permissions.sort();
    permissions.dedup();
    permissions
}

pub fn is_known_hook(hook: &str) -> bool {
    is_active_gateway_hook(hook) || is_reserved_gateway_hook(hook)
}

pub fn is_active_gateway_hook(hook: &str) -> bool {
    crate::gateway::plugins::contract::is_active_hook(hook)
}

pub fn is_reserved_gateway_hook(hook: &str) -> bool {
    crate::gateway::plugins::contract::is_reserved_hook(hook)
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), PluginValidationError> {
    if crate::app_paths::plugin_id_path_segment(plugin_id).is_err() || !plugin_id.contains('.') {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_ID",
            "plugin id must match publisher.plugin-name",
        ));
    }
    Ok(())
}

fn validate_semver(version: &str, code: &str) -> Result<(), PluginValidationError> {
    parse_semver(version)
        .map(|_| ())
        .ok_or_else(|| PluginValidationError::new(code, format!("invalid SemVer: {version}")))
}

fn validate_manifest_api_version(api_version: &str) -> Result<(), PluginValidationError> {
    validate_semver(api_version, "PLUGIN_INVALID_API_VERSION")?;
    let Some((major, _, _)) = parse_semver(api_version) else {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_API_VERSION",
            format!("invalid plugin apiVersion: {api_version}"),
        ));
    };
    if major != SUPPORTED_PLUGIN_API_MAJOR {
        return Err(PluginValidationError::new(
            "PLUGIN_INCOMPATIBLE_API",
            format!(
                "plugin apiVersion {api_version} is not supported; supported major is {}",
                SUPPORTED_PLUGIN_API_MAJOR
            ),
        ));
    }
    Ok(())
}

fn validate_extension_host_manifest(
    manifest: &PluginManifest,
    language: &str,
) -> Result<(), PluginValidationError> {
    if manifest
        .main
        .as_deref()
        .is_none_or(|main| main.trim().is_empty())
    {
        return Err(PluginValidationError::new(
            "PLUGIN_MISSING_MAIN",
            "extensionHost runtime requires main",
        ));
    }
    if language != "typescript" {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_RUNTIME",
            "extensionHost language must be typescript",
        ));
    }
    if !manifest.hooks.is_empty() {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_MANIFEST",
            "extensionHost manifests must not declare top-level hooks",
        ));
    }
    if !manifest.permissions.is_empty() {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_MANIFEST",
            "extensionHost manifests must not declare top-level permissions",
        ));
    }
    validate_activation_events(&manifest.activation_events)?;
    validate_contributes(manifest.id.as_str(), manifest.contributes.as_ref())?;
    validate_capabilities(&manifest.capabilities)?;
    validate_capability_dependencies(manifest.contributes.as_ref(), &manifest.capabilities)?;
    Ok(())
}

fn validate_activation_events(activation_events: &[String]) -> Result<(), PluginValidationError> {
    for event in activation_events {
        if event == "onStartup" {
            continue;
        }
        let has_known_prefix = [
            "onCommand:",
            "onProviderEditor:",
            "onProtocolBridge:",
            "onGatewayHook:",
        ]
        .iter()
        .any(|prefix| {
            event
                .strip_prefix(prefix)
                .is_some_and(|value| !value.trim().is_empty())
        });
        if !has_known_prefix {
            return Err(PluginValidationError::new(
                "PLUGIN_INVALID_ACTIVATION_EVENT",
                format!("invalid activation event: {event}"),
            ));
        }
    }
    Ok(())
}

fn validate_contributes(
    plugin_id: &str,
    contributes: Option<&PluginContributes>,
) -> Result<(), PluginValidationError> {
    let Some(contributes) = contributes else {
        return Ok(());
    };

    for provider in &contributes.providers {
        if is_blank(&provider.provider_type)
            || is_blank(&provider.display_name)
            || is_blank(&provider.extension_namespace)
            || provider.target_cli_keys.is_empty()
        {
            return Err(PluginValidationError::new(
                "PLUGIN_INVALID_PROVIDER_CONTRIBUTION",
                "provider contribution requires providerType, displayName, extensionNamespace, and targetCliKeys",
            ));
        }
    }

    for protocol in &contributes.protocols {
        if is_blank(&protocol.protocol_id) {
            return Err(PluginValidationError::new(
                "PLUGIN_INVALID_PROTOCOL_CONTRIBUTION",
                "protocol contribution requires protocolId",
            ));
        }
    }

    for bridge in &contributes.protocol_bridges {
        if is_blank(&bridge.bridge_type)
            || is_blank(&bridge.inbound_protocol)
            || is_blank(&bridge.outbound_protocol)
        {
            return Err(PluginValidationError::new(
                "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION",
                "protocol bridge contribution requires bridgeType, inboundProtocol, and outboundProtocol",
            ));
        }
        if !is_namespaced_contribution_id(plugin_id, &bridge.bridge_type) {
            return Err(PluginValidationError::new(
                "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION",
                "protocol bridge bridgeType must be lower-case and namespaced by plugin id",
            ));
        }
    }

    for command in &contributes.commands {
        if is_blank(&command.command) || is_blank(&command.title) {
            return Err(PluginValidationError::new(
                "PLUGIN_INVALID_COMMAND_CONTRIBUTION",
                "command contribution requires command and title",
            ));
        }
    }

    for hook in &contributes.gateway_hooks {
        validate_hook(hook)?;
    }

    for (slot, ui_contributions) in &contributes.ui {
        if !is_known_ui_slot(slot) {
            return Err(PluginValidationError::new(
                "PLUGIN_UNKNOWN_UI_SLOT",
                format!("unknown UI contribution slot: {slot}"),
            ));
        }
        for contribution in ui_contributions {
            validate_ui_contribution(contribution)?;
        }
    }
    Ok(())
}

fn validate_ui_contribution(
    contribution: &super::plugin_contributions::UiContribution,
) -> Result<(), PluginValidationError> {
    if is_blank(&contribution.id) {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_UI_CONTRIBUTION",
            "UI contribution requires id",
        ));
    }

    match &contribution.schema {
        HostRenderedSchema::Section { fields } | HostRenderedSchema::Panel { fields } => {
            for field in fields {
                validate_host_rendered_field(field)?;
            }
        }
        HostRenderedSchema::Badge { label, .. } => {
            if is_blank(label) {
                return Err(PluginValidationError::new(
                    "PLUGIN_INVALID_UI_CONTRIBUTION",
                    "badge schema requires label",
                ));
            }
        }
    }
    Ok(())
}

fn validate_host_rendered_field(field: &HostRenderedField) -> Result<(), PluginValidationError> {
    match field {
        HostRenderedField::Text { key, label, .. }
        | HostRenderedField::Password { key, label, .. }
        | HostRenderedField::Number { key, label, .. }
        | HostRenderedField::Boolean { key, label }
        | HostRenderedField::Textarea { key, label, .. }
        | HostRenderedField::Info { key, label, .. } => validate_ui_field_key_label(key, label),
        HostRenderedField::Button {
            key,
            label,
            command,
        } => {
            validate_ui_field_key_label(key, label)?;
            if is_blank(command) {
                return Err(PluginValidationError::new(
                    "PLUGIN_INVALID_UI_CONTRIBUTION",
                    "button field requires command",
                ));
            }
            Ok(())
        }
        HostRenderedField::Select {
            key,
            label,
            options,
        } => {
            validate_ui_field_key_label(key, label)?;
            if options.is_empty()
                || options
                    .iter()
                    .any(|option| is_blank(&option.value) || is_blank(&option.label))
            {
                return Err(PluginValidationError::new(
                    "PLUGIN_INVALID_UI_CONTRIBUTION",
                    "select field requires options",
                ));
            }
            Ok(())
        }
    }
}

fn validate_ui_field_key_label(key: &str, label: &str) -> Result<(), PluginValidationError> {
    if is_blank(key) || is_blank(label) {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_UI_CONTRIBUTION",
            "UI field requires key and label",
        ));
    }
    Ok(())
}

fn validate_capabilities(capabilities: &[String]) -> Result<(), PluginValidationError> {
    for capability in capabilities {
        if !is_known_capability(capability) {
            return Err(PluginValidationError::new(
                "PLUGIN_UNKNOWN_CAPABILITY",
                format!("unknown capability: {capability}"),
            ));
        }
    }
    Ok(())
}

fn validate_capability_dependencies(
    contributes: Option<&PluginContributes>,
    capabilities: &[String],
) -> Result<(), PluginValidationError> {
    let Some(contributes) = contributes else {
        return Ok(());
    };

    if !contributes.commands.is_empty() {
        require_capability(capabilities, "commands.execute", "commands contribution")?;
    }
    if !contributes.providers.is_empty() {
        require_capability(
            capabilities,
            "provider.extensionValues",
            "provider contribution",
        )?;
    }
    if !contributes.gateway_hooks.is_empty() {
        require_capability(capabilities, "gateway.hooks", "gatewayHooks contribution")?;
    }
    if !contributes.protocol_bridges.is_empty() {
        require_capability(
            capabilities,
            "protocol.bridge",
            "protocolBridges contribution",
        )?;
    }
    for slot in ["providers.editor.sections", "providers.editor.fields"] {
        if contributes
            .ui
            .get(slot)
            .is_some_and(|items| !items.is_empty())
        {
            require_capability(
                capabilities,
                "provider.extensionValues",
                &format!("{slot} UI contribution"),
            )?;
        }
    }
    if ui_has_button_field(&contributes.ui) {
        require_capability(capabilities, "commands.execute", "UI command field")?;
    }

    Ok(())
}

fn has_capability(capabilities: &[String], capability: &str) -> bool {
    capabilities.iter().any(|item| item == capability)
}

fn require_capability(
    capabilities: &[String],
    capability: &str,
    reason: &str,
) -> Result<(), PluginValidationError> {
    if has_capability(capabilities, capability) {
        return Ok(());
    }
    Err(PluginValidationError::new(
        "PLUGIN_MISSING_CAPABILITY",
        format!("{reason} requires {capability}"),
    ))
}

fn ui_has_button_field(ui: &BTreeMap<String, Vec<UiContribution>>) -> bool {
    ui.values().any(|contributions| {
        contributions
            .iter()
            .any(|contribution| schema_has_button_field(&contribution.schema))
    })
}

fn schema_has_button_field(schema: &HostRenderedSchema) -> bool {
    match schema {
        HostRenderedSchema::Section { fields } | HostRenderedSchema::Panel { fields } => {
            fields.iter().any(is_button_field)
        }
        HostRenderedSchema::Badge { .. } => false,
    }
}

fn is_button_field(field: &HostRenderedField) -> bool {
    matches!(field, HostRenderedField::Button { .. })
}

fn is_blank(value: &str) -> bool {
    value.trim().is_empty()
}

fn is_namespaced_contribution_id(plugin_id: &str, value: &str) -> bool {
    if !is_valid_contribution_id(value) {
        return false;
    }
    if value == plugin_id {
        return true;
    }
    let Some(suffix) = value.strip_prefix(plugin_id) else {
        return false;
    };
    suffix.len() > 1 && matches!(suffix.as_bytes().first(), Some(b'.' | b'/' | b':'))
}

fn is_valid_contribution_id(value: &str) -> bool {
    value.split(['.', '/', ':']).all(|segment| {
        let mut chars = segment.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        (first.is_ascii_lowercase() || first.is_ascii_digit())
            && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    })
}

fn validate_hook(hook: &PluginHook) -> Result<(), PluginValidationError> {
    validate_hook_name(&hook.name)?;
    if hook.timeout_ms == Some(0) {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_HOOK_TIMEOUT",
            "hook timeoutMs must be greater than zero",
        ));
    }
    Ok(())
}

fn validate_hook_name(hook_name: &str) -> Result<(), PluginValidationError> {
    if is_reserved_gateway_hook(hook_name) {
        return Err(PluginValidationError::new(
            "PLUGIN_RESERVED_HOOK",
            format!(
                "hook is reserved for a future host integration and is not active in plugin API v1: {}",
                hook_name
            ),
        ));
    }
    if !is_known_hook(hook_name) {
        return Err(PluginValidationError::new(
            "PLUGIN_UNKNOWN_HOOK",
            format!("unknown hook: {}", hook_name),
        ));
    }
    Ok(())
}

fn validate_config_schema(schema: Option<&serde_json::Value>) -> Result<(), PluginValidationError> {
    let Some(schema) = schema else {
        return Ok(());
    };
    validate_schema_node(schema)
}

fn validate_schema_node(schema: &serde_json::Value) -> Result<(), PluginValidationError> {
    let Some(schema_type) = schema.get("type").and_then(|value| value.as_str()) else {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_CONFIG_SCHEMA",
            "config schema node must declare type",
        ));
    };

    match schema_type {
        "string" | "number" | "integer" | "boolean" | "password" => Ok(()),
        "array" => {
            if let Some(items) = schema.get("items") {
                validate_schema_node(items)?;
            }
            Ok(())
        }
        "object" => {
            if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
                for property in properties.values() {
                    validate_schema_node(property)?;
                }
            }
            Ok(())
        }
        _ => Err(PluginValidationError::new(
            "PLUGIN_INVALID_CONFIG_SCHEMA",
            format!("unsupported config schema type: {schema_type}"),
        )),
    }
}

fn validate_host_compatibility(
    compatibility: &PluginHostCompatibility,
    host_version: &str,
) -> Result<(), PluginValidationError> {
    let Some(host) = parse_semver(host_version) else {
        return Err(PluginValidationError::new(
            "PLUGIN_INVALID_HOST_VERSION",
            format!("invalid host version: {host_version}"),
        ));
    };

    if !matches_version_range(&compatibility.app, host) {
        return Err(PluginValidationError::new(
            "PLUGIN_INCOMPATIBLE_HOST",
            format!(
                "host version {host_version} does not satisfy {}",
                compatibility.app
            ),
        ));
    }

    let plugin_api = compatibility.plugin_api.trim();
    let api_supported = if let Some(required) = plugin_api.strip_prefix('^') {
        parse_semver(required).is_some_and(|(major, _, _)| major == SUPPORTED_PLUGIN_API_MAJOR)
    } else {
        parse_semver(plugin_api).is_some_and(|(major, _, _)| major == SUPPORTED_PLUGIN_API_MAJOR)
    };
    if !api_supported {
        return Err(PluginValidationError::new(
            "PLUGIN_INCOMPATIBLE_API",
            format!("plugin API range is not supported: {plugin_api}"),
        ));
    }

    Ok(())
}

fn matches_version_range(range: &str, version: (u64, u64, u64)) -> bool {
    let mut saw_constraint = false;
    for part in range.split_whitespace() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        saw_constraint = true;
        if let Some(raw) = part.strip_prefix(">=") {
            let Some(bound) = parse_semver(raw) else {
                return false;
            };
            if version < bound {
                return false;
            }
        } else if let Some(raw) = part.strip_prefix('<') {
            let Some(bound) = parse_semver(raw) else {
                return false;
            };
            if version >= bound {
                return false;
            }
        } else if let Some(raw) = part.strip_prefix('^') {
            let Some(bound) = parse_semver(raw) else {
                return false;
            };
            if version < bound || version.0 != bound.0 {
                return false;
            }
        } else {
            let Some(bound) = parse_semver(part) else {
                return false;
            };
            if version != bound {
                return false;
            }
        }
    }
    saw_constraint
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let core = version
        .trim()
        .split_once('-')
        .map_or(version.trim(), |(core, _)| core);
    let mut parts = core.split('.');
    let major = parse_semver_number(parts.next()?)?;
    let minor = parse_semver_number(parts.next()?)?;
    let patch = parse_semver_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn parse_semver_number(value: &str) -> Option<u64> {
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return None;
    }
    value.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> serde_json::Value {
        valid_extension_host_manifest()
    }

    fn valid_extension_host_manifest() -> serde_json::Value {
        serde_json::json!({
            "id": "acme.extension",
            "name": "Extension",
            "version": "1.0.0",
            "apiVersion": "1.0.0",
            "main": "dist/extension.js",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "activationEvents": ["onStartup"],
            "capabilities": [],
            "hostCompatibility": {
                "app": ">=0.62.0 <1.0.0",
                "pluginApi": "^1.0.0"
            }
        })
    }

    fn assert_manifest_validation_error(raw: serde_json::Value, expected_code: &str) {
        let err = manifest_validation_error(raw);
        assert_eq!(err.code, expected_code);
    }

    fn manifest_validation_error(raw: serde_json::Value) -> PluginValidationError {
        let manifest: PluginManifest = serde_json::from_value(raw).unwrap();
        validate_manifest(&manifest, "0.62.0").unwrap_err()
    }

    fn assert_unsupported_or_unknown_runtime(raw: serde_json::Value) {
        match serde_json::from_value::<PluginManifest>(raw) {
            Ok(manifest) => {
                let err = validate_manifest(&manifest, "0.62.0").unwrap_err();
                assert_eq!(err.code, "PLUGIN_UNSUPPORTED_RUNTIME");
            }
            Err(err) => assert!(err.to_string().contains("unknown variant")),
        }
    }

    fn hook(name: &str) -> PluginHook {
        PluginHook {
            name: name.to_string(),
            priority: 100,
            failure_policy: Some("fail-open".to_string()),
            timeout_ms: None,
        }
    }

    fn plugin_manifest_with_contributes(contributes: serde_json::Value) -> PluginManifest {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = contributes;
        serde_json::from_value(raw).unwrap()
    }

    #[test]
    fn manifest_json_deserializes_and_validates() {
        let manifest: PluginManifest =
            serde_json::from_value(valid_extension_host_manifest()).unwrap();
        validate_manifest(&manifest, "0.62.0").unwrap();
        assert_eq!(manifest.id.as_str(), "acme.extension");
    }

    #[test]
    fn unknown_runtime_is_unsupported() {
        let mut raw = valid_manifest();
        raw["runtime"] = serde_json::json!({
            "kind": "legacyRules",
            "rules": ["rules/main.json"]
        });

        assert_unsupported_or_unknown_runtime(raw);
    }

    #[test]
    fn wasm_runtime_is_unsupported() {
        let mut raw = valid_manifest();
        raw["runtime"] = serde_json::json!({
            "kind": "wasm",
            "abiVersion": "1.0.0"
        });

        assert_unsupported_or_unknown_runtime(raw);
    }

    #[test]
    fn third_party_native_runtime_is_unsupported() {
        let mut raw = valid_manifest();
        raw["id"] = serde_json::json!("community.privacy-filter");
        raw["runtime"] = serde_json::json!({
            "kind": "native",
            "engine": "hostPrivateRedactor"
        });

        assert_unsupported_or_unknown_runtime(raw);
    }

    #[test]
    fn native_runtime_is_not_a_public_manifest_runtime() {
        let mut raw = valid_manifest();
        raw["runtime"] = serde_json::json!({
            "kind": "native",
            "engine": "hostPrivateRedactor"
        });

        let manifest = serde_json::from_value::<PluginManifest>(raw);
        assert!(manifest
            .expect_err("native must not deserialize as a public PluginRuntime")
            .to_string()
            .contains("unknown variant"),);
    }

    #[test]
    fn extension_host_rejects_top_level_hooks() {
        let mut raw = valid_extension_host_manifest();
        raw["hooks"] = serde_json::json!([
            {
                "name": "gateway.request.afterBodyRead",
                "priority": 100,
                "failurePolicy": "fail-open"
            }
        ]);

        assert_manifest_validation_error(raw, "PLUGIN_INVALID_MANIFEST");
    }

    #[test]
    fn extension_host_rejects_top_level_permissions() {
        let mut raw = valid_extension_host_manifest();
        raw["permissions"] = serde_json::json!(["request.body.read"]);

        assert_manifest_validation_error(raw, "PLUGIN_INVALID_MANIFEST");
    }

    #[test]
    fn extension_host_rejects_unknown_contribution_fields() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "legacyRules": [{
                "id": "acme.extension.gateway-rule",
                "rules": ["request.path == '/v1/chat/completions'"],
                "hooks": ["gateway.request.afterBodyRead"]
            }]
        });

        let err = serde_json::from_value::<PluginManifest>(raw).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn extension_host_rejects_unknown_contribution_fields_when_empty_or_non_array() {
        for legacy_rules in [
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!("bad"),
            serde_json::json!(null),
        ] {
            let mut raw = valid_extension_host_manifest();
            raw["contributes"] = serde_json::json!({ "legacyRules": legacy_rules });

            let err = serde_json::from_value::<PluginManifest>(raw).unwrap_err();
            assert!(err.to_string().contains("unknown field"));
        }
    }

    #[test]
    fn extension_host_commands_require_execute_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "commands": [{
                "command": "acme.extension.refresh",
                "title": "Refresh"
            }]
        });

        assert_manifest_validation_error(raw, "PLUGIN_MISSING_CAPABILITY");
    }

    #[test]
    fn extension_host_gateway_hooks_require_gateway_hooks_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "gatewayHooks": [{
                "name": "gateway.request.afterBodyRead",
                "priority": 100,
                "failurePolicy": "fail-open"
            }]
        });

        assert_manifest_validation_error(raw, "PLUGIN_MISSING_CAPABILITY");
    }

    #[test]
    fn extension_host_protocol_bridges_require_protocol_bridge_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "protocolBridges": [{
                "bridgeType": "acme.extension.bridge",
                "inboundProtocol": "claude",
                "outboundProtocol": "codex"
            }]
        });

        assert_manifest_validation_error(raw, "PLUGIN_MISSING_CAPABILITY");
    }

    #[test]
    fn extension_host_rejects_non_namespaced_protocol_bridge_type() {
        let mut raw = valid_extension_host_manifest();
        raw["capabilities"] = serde_json::json!(["protocol.bridge"]);
        raw["contributes"] = serde_json::json!({
            "protocolBridges": [{
                "bridgeType": "openai-gemini",
                "inboundProtocol": "openai.chat",
                "outboundProtocol": "gemini.generateContent"
            }]
        });

        let err = manifest_validation_error(raw);
        assert_eq!(err.code, "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION");
        assert_eq!(
            err.message,
            "protocol bridge bridgeType must be lower-case and namespaced by plugin id"
        );
    }

    #[test]
    fn extension_host_rejects_invalid_protocol_bridge_type() {
        let mut raw = valid_extension_host_manifest();
        raw["capabilities"] = serde_json::json!(["protocol.bridge"]);
        raw["contributes"] = serde_json::json!({
            "protocolBridges": [{
                "bridgeType": "acme.extension.OpenAI",
                "inboundProtocol": "openai.chat",
                "outboundProtocol": "gemini.generateContent"
            }]
        });

        let err = manifest_validation_error(raw);
        assert_eq!(err.code, "PLUGIN_INVALID_PROTOCOL_BRIDGE_CONTRIBUTION");
        assert_eq!(
            err.message,
            "protocol bridge bridgeType must be lower-case and namespaced by plugin id"
        );
    }

    #[test]
    fn extension_host_accepts_valid_namespaced_protocol_bridge_type() {
        let mut raw = valid_extension_host_manifest();
        raw["capabilities"] = serde_json::json!(["protocol.bridge"]);
        raw["contributes"] = serde_json::json!({
            "protocolBridges": [{
                "bridgeType": "acme.extension.openai-gemini",
                "inboundProtocol": "openai.chat",
                "outboundProtocol": "gemini.generateContent"
            }]
        });
        let manifest: PluginManifest = serde_json::from_value(raw).unwrap();

        validate_manifest(&manifest, "0.62.0").unwrap();
    }

    #[test]
    fn extension_host_providers_require_extension_values_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "providers": [{
                "providerType": "acme.extension.openrouter",
                "displayName": "OpenRouter",
                "targetCliKeys": ["codex"],
                "extensionNamespace": "openrouter"
            }]
        });

        assert_manifest_validation_error(raw, "PLUGIN_MISSING_CAPABILITY");
    }

    #[test]
    fn extension_host_provider_editor_sections_require_extension_values_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "ui": {
                "providers.editor.sections": [{
                    "id": "openrouter-routing",
                    "schema": {
                        "type": "section",
                        "fields": [{ "type": "text", "key": "route", "label": "Route" }]
                    }
                }]
            }
        });

        assert_manifest_validation_error(raw, "PLUGIN_MISSING_CAPABILITY");
    }

    #[test]
    fn extension_host_provider_editor_fields_require_extension_values_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "ui": {
                "providers.editor.fields": [{
                    "id": "openrouter-api-key",
                    "schema": {
                        "type": "section",
                        "fields": [{ "type": "password", "key": "apiKey", "label": "API key" }]
                    }
                }]
            }
        });

        let err = manifest_validation_error(raw);
        assert_eq!(err.code, "PLUGIN_MISSING_CAPABILITY");
        assert!(err
            .message
            .contains("providers.editor.fields UI contribution requires provider.extensionValues"));
    }

    #[test]
    fn extension_host_button_fields_require_execute_capability() {
        let mut raw = valid_extension_host_manifest();
        raw["contributes"] = serde_json::json!({
            "ui": {
                "settings.sections": [{
                    "id": "refresh-settings",
                    "schema": {
                        "type": "section",
                        "fields": [{
                            "type": "button",
                            "key": "refresh",
                            "label": "Refresh",
                            "command": "acme.extension.refresh"
                        }]
                    }
                }]
            }
        });

        assert_manifest_validation_error(raw, "PLUGIN_MISSING_CAPABILITY");
    }

    #[test]
    fn validates_extension_host_provider_manifest() {
        let manifest = serde_json::json!({
            "id": "acme.openrouter",
            "name": "OpenRouter Provider",
            "version": "0.1.0",
            "apiVersion": "1.0.0",
            "main": "dist/extension.js",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "activationEvents": ["onStartup", "onProviderEditor:openrouter"],
            "contributes": {
                "providers": [{
                    "providerType": "openrouter",
                    "displayName": "OpenRouter",
                    "targetCliKeys": ["claude", "codex"],
                    "extensionNamespace": "openrouter"
                }],
                "ui": {
                    "providers.editor.sections": [{
                        "id": "openrouter-routing",
                        "title": "OpenRouter 路由",
                        "order": 100,
                        "schema": {
                            "type": "section",
                            "fields": [{ "type": "text", "key": "route", "label": "Route" }]
                        }
                    }]
                },
                "commands": [{
                    "command": "acme.openrouter.refreshModels",
                    "title": "刷新 OpenRouter 模型"
                }]
            },
            "capabilities": ["provider.extensionValues", "commands.execute"],
            "hostCompatibility": { "app": ">=0.62.0 <1.0.0", "pluginApi": "^1.0.0" }
        });
        let manifest: PluginManifest = serde_json::from_value(manifest).unwrap();

        validate_manifest(&manifest, "0.62.0").unwrap();
    }

    #[test]
    fn extension_host_manifest_rejects_unknown_slot() {
        let manifest = serde_json::json!({
            "id": "acme.bad-slot",
            "name": "Bad Slot",
            "version": "0.1.0",
            "apiVersion": "1.0.0",
            "main": "dist/extension.js",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "activationEvents": ["onStartup"],
            "contributes": { "ui": { "providers.editor.unknown": [] } },
            "capabilities": [],
            "hostCompatibility": { "app": ">=0.62.0 <1.0.0", "pluginApi": "^1.0.0" }
        });
        let manifest: PluginManifest = serde_json::from_value(manifest).unwrap();

        let err = validate_manifest(&manifest, "0.62.0").unwrap_err();
        assert_eq!(err.code, "PLUGIN_UNKNOWN_UI_SLOT");
    }

    #[test]
    fn extension_host_manifest_rejects_invalid_provider_contribution() {
        let manifest = serde_json::json!({
            "id": "acme.bad-provider",
            "name": "Bad Provider",
            "version": "0.1.0",
            "apiVersion": "1.0.0",
            "main": "dist/extension.js",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "activationEvents": ["onStartup"],
            "contributes": {
                "providers": [{
                    "providerType": "",
                    "displayName": "OpenRouter",
                    "targetCliKeys": ["claude"],
                    "extensionNamespace": "openrouter"
                }]
            },
            "capabilities": [],
            "hostCompatibility": { "app": ">=0.62.0 <1.0.0", "pluginApi": "^1.0.0" }
        });
        let manifest: PluginManifest = serde_json::from_value(manifest).unwrap();

        let err = validate_manifest(&manifest, "0.62.0").unwrap_err();
        assert_eq!(err.code, "PLUGIN_INVALID_PROVIDER_CONTRIBUTION");
    }

    #[test]
    fn manifest_rejects_unknown_hook() {
        let err = validate_hook(&hook("gateway.request.missing")).unwrap_err();
        assert_eq!(err.code, "PLUGIN_UNKNOWN_HOOK");
    }

    #[test]
    fn validate_manifest_rejects_reserved_hook_until_it_is_wired() {
        let err = validate_hook(&hook("gateway.request.received")).unwrap_err();
        assert_eq!(err.code, "PLUGIN_RESERVED_HOOK");
        assert!(err.message.contains("gateway.request.received"));
    }

    #[test]
    fn manifest_rejects_zero_hook_timeout() {
        let mut hook = hook("gateway.request.afterBodyRead");
        hook.timeout_ms = Some(0);

        let err = validate_hook(&hook).unwrap_err();

        assert_eq!(err.code, "PLUGIN_INVALID_HOOK_TIMEOUT");
    }

    #[test]
    fn extension_host_effective_permissions_are_derived_from_gateway_contributions() {
        let mut manifest = plugin_manifest_with_contributes(serde_json::json!({
            "gatewayHooks": [
                { "name": "gateway.request.afterBodyRead", "priority": 10 },
                { "name": "log.beforePersist", "priority": 20 }
            ]
        }));
        manifest.capabilities = vec!["gateway.hooks".to_string()];

        let permissions = manifest_effective_permissions(&manifest);

        for permission in [
            "request.meta.read",
            "request.header.read",
            "request.header.readSensitive",
            "request.body.read",
            "request.header.write",
            "request.body.write",
            "log.redact",
        ] {
            assert!(
                permissions.contains(&permission.to_string()),
                "missing derived permission {permission}"
            );
        }
        assert_eq!(
            manifest_permission_risk(&manifest),
            PluginPermissionRisk::High
        );
    }

    #[test]
    fn extension_host_effective_permissions_require_gateway_capability() {
        let manifest = plugin_manifest_with_contributes(serde_json::json!({
            "gatewayHooks": [
                { "name": "gateway.request.afterBodyRead", "priority": 10 }
            ]
        }));

        assert!(manifest_effective_permissions(&manifest).is_empty());
        assert!(
            gateway_hook_effective_permissions(&manifest, "gateway.request.afterBodyRead")
                .is_empty()
        );
    }

    #[test]
    fn manifest_rejects_incompatible_host() {
        let mut raw = valid_extension_host_manifest();
        raw["hostCompatibility"]["app"] = serde_json::json!(">=9.0.0");
        let manifest: PluginManifest = serde_json::from_value(raw).unwrap();
        let err = validate_manifest(&manifest, "0.62.0").unwrap_err();
        assert_eq!(err.code, "PLUGIN_INCOMPATIBLE_HOST");
    }

    #[test]
    fn validate_manifest_rejects_future_api_version_major_even_when_compat_range_mentions_v1() {
        let mut raw = valid_extension_host_manifest();
        raw["apiVersion"] = serde_json::json!("2.0.0");
        raw["hostCompatibility"]["pluginApi"] = serde_json::json!("^1.0.0");
        let manifest: PluginManifest = serde_json::from_value(raw).unwrap();

        let err = validate_manifest(&manifest, "0.62.0").unwrap_err();

        assert_eq!(err.code, "PLUGIN_INCOMPATIBLE_API");
        assert!(err.message.contains("apiVersion"));
        assert!(err.message.contains("2.0.0"));
    }

    #[test]
    fn manifest_rejects_invalid_runtime() {
        let mut raw = valid_manifest();
        raw["runtime"] = serde_json::json!({ "kind": "node" });
        let err = serde_json::from_value::<PluginManifest>(raw).unwrap_err();
        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn official_privacy_filter_extension_host_manifest_uses_normal_validation() {
        let manifest: PluginManifest = serde_json::from_value(serde_json::json!({
            "id": "official.privacy-filter",
            "name": "Privacy Filter",
            "version": "1.0.0",
            "apiVersion": "1.0.0",
            "runtime": { "kind": "extensionHost", "language": "typescript" },
            "main": "dist/extension.js",
            "capabilities": ["gateway.hooks", "privacy.redact"],
            "contributes": {
                "gatewayHooks": [
                    { "name": "gateway.request.afterBodyRead", "priority": 5, "failurePolicy": "fail-closed" },
                    { "name": "gateway.request.beforeSend", "priority": 5, "failurePolicy": "fail-closed" },
                    { "name": "log.beforePersist", "priority": 1, "failurePolicy": "fail-closed" }
                ]
            },
            "hostCompatibility": { "app": ">=0.60.0 <1.0.0", "pluginApi": "^1.0.0" }
        }))
        .unwrap();

        validate_manifest(&manifest, "0.62.0")
            .expect("official privacy filter should be a normal extension host manifest");
        validate_manifest_for_official_plugin(&manifest, "0.62.0")
            .expect("official validation should also accept normal extension host manifest");
    }

    #[test]
    fn official_privacy_filter_native_runtime_is_not_a_public_manifest_runtime() {
        let mut official = valid_manifest();
        official["id"] = serde_json::json!("official.privacy-filter");
        official["runtime"] = serde_json::json!({
            "kind": "native",
            "engine": "hostPrivateRedactor"
        });

        assert_unsupported_or_unknown_runtime(official);
    }
}
