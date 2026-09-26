//! Pi/OMP public model declarations and generated-entry ownership.
mod catalog;
mod generation;
mod import;
mod manifests;
mod metadata;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use catalog::seed_native_gateway_for_test;
pub(crate) use catalog::{
    candidate_eligible, catalog, import_confirm, models_get, models_set, validate_publication,
};
pub(crate) use generation::{generate_entries, GatewayCatalogGroup, GeneratedEntry};
pub(crate) use import::{
    preview_import, static_url as validate_static_base_url, validate_explicit_api_key,
    GatewayImportConfirmInput, GatewayImportPreview,
};
pub(crate) use manifests::{
    delete_owned_manifest, list_manifests, manifest_summaries, put_manifest,
};
pub(crate) use manifests::{managed_native_keys, Manifest, ManifestPayload, ManifestVersion};
#[cfg(test)]
pub(crate) use metadata::ModelInput;
pub(crate) use metadata::{
    intersect as intersect_models, validate_client as validate_native_client,
};
pub(crate) use metadata::{NativeModelSpec, ThinkingSpec};

use crate::shared::gateway_protocol::GatewayProtocol;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayModelsSnapshot {
    pub provider_id: i64,
    pub provider_uuid: String,
    pub revision: String,
    pub models: Vec<NativeModelSpec>,
    pub stale: bool,
}

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayManifestSummary {
    pub protocol: GatewayProtocol,
    pub native_key: String,
    pub generation: i64,
    pub state: String,
    pub stale: bool,
    pub modified: bool,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCatalogPreview {
    pub target_id: String,
    pub revision: String,
    pub catalog_revision: String,
    pub listener_ready: bool,
    pub groups: Vec<GatewayCatalogGroup>,
    pub entries: Vec<GeneratedEntry>,
    pub manifests: Vec<GatewayManifestSummary>,
}
#[derive(Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayLifecycleInput {
    pub target_id: String,
    pub expected_revision: String,
    pub catalog_revision: String,
}
#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayMutationResult {
    pub target_id: String,
    pub revision: String,
    pub changed: bool,
    pub backup_path: Option<String>,
    pub manifests: Vec<GatewayManifestSummary>,
}

pub(super) fn db_error(_: impl std::fmt::Display) -> crate::shared::error::AppError {
    crate::shared::error::AppError::new(
        "NATIVE_GATEWAY_DB_ERROR",
        "Cannot persist native gateway state",
    )
}
pub(crate) fn hash(value: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_ref()))
}

#[cfg(test)]
use manifests::delete_manifest;
