//! Maintain the complete Codex model catalog used by AIO-managed profiles.

use super::protocol;
use crate::shared::error::{db_err, AppError, AppResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const GENERATED_CATALOG_FILE_NAME: &str = "managed-model-catalog.json";
const GENERATED_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const USER_CATALOG_MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_BASE_MODEL_COUNT: usize = 1_000;
const MAX_MANAGED_PROFILE_COUNT: usize = 256;
const OWNER_METADATA_KEY: &str = "_aio_managed_model_catalog";
const LEGACY_OWNER_SCHEMA_VERSION: u64 = 1;
const LEGACY_POLICY_OWNER_SCHEMA_VERSION: u64 = 2;
const OWNER_SCHEMA_VERSION: u64 = 3;
const MANAGED_BY: &str = "aio-coding-hub";
const MODEL_CONTEXT_PROJECTION_VERSION: u64 = 1;

// Owner schema v2 encoded the retired GPT-5.6-only policy. These constants are
// validation-only so an existing generated catalog can be recovered safely.
const LEGACY_GPT56_372K_CONTEXT_TOKENS: u64 = 372_000;
const LEGACY_GPT56_372K_POLICY_VERSION: u64 = 1;
const LEGACY_GPT56_372K_MODEL_SLUGS: [&str; 3] = ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedCatalogProfile {
    pub(crate) profile_name_key: String,
    pub(crate) model_uuid: String,
    pub(crate) provider_name: String,
    pub(crate) remote_model_id: String,
    pub(crate) capabilities: crate::provider_models::ProviderModelCapabilities,
}

struct RawManagedCatalogProfile {
    profile_name_key: String,
    model_uuid: String,
    provider_name: String,
    remote_model_id: String,
    capabilities_configured: bool,
    supported_reasoning_efforts_json: String,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ManagedCatalogPolicy {
    pub(crate) model_context_rules: Vec<crate::settings::CodexModelContextRule>,
}

impl ManagedCatalogPolicy {
    pub(crate) fn from_settings(settings: &crate::settings::AppSettings) -> AppResult<Self> {
        Self::from_rules(settings.codex_model_context_rules.clone())
    }

    pub(crate) fn from_rules(
        model_context_rules: Vec<crate::settings::CodexModelContextRule>,
    ) -> AppResult<Self> {
        Self {
            model_context_rules,
        }
        .canonicalized()
    }

    fn canonicalized(mut self) -> AppResult<Self> {
        crate::settings::normalize_codex_model_context_rules_for_write(
            &mut self.model_context_rules,
        )?;
        Ok(self)
    }

    fn enabled_rules(&self) -> impl Iterator<Item = &crate::settings::CodexModelContextRule> {
        self.model_context_rules.iter().filter(|rule| rule.enabled)
    }

    fn has_enabled_rules(&self) -> bool {
        self.enabled_rules().next().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnabledModelContextRule {
    model_id: String,
    context_window: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelContextPolicyProjection {
    rule_set_sha256: String,
    enabled_rules: Vec<EnabledModelContextRule>,
    enabled_rules_sha256: String,
}

struct CatalogGenerationContext<'a> {
    profile_set_sha256: &'a str,
    base_source_fingerprint: &'a str,
    policy: &'a ManagedCatalogPolicy,
    policy_projection: &'a ModelContextPolicyProjection,
    original_catalog_path: Option<&'a Path>,
    codex_home_key: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexBaseCatalogRead {
    pub(crate) bytes: Vec<u8>,
}

impl ManagedCatalogProfile {
    pub(crate) fn new(
        profile_name_key: impl Into<String>,
        model_uuid: impl Into<String>,
        provider_name: impl Into<String>,
        remote_model_id: impl Into<String>,
        capabilities: crate::provider_models::ProviderModelCapabilities,
    ) -> AppResult<Self> {
        let profile = Self {
            profile_name_key: profile_name_key.into(),
            model_uuid: model_uuid.into(),
            provider_name: provider_name.into(),
            remote_model_id: remote_model_id.into(),
            capabilities,
        };
        validate_profile(&profile)?;
        Ok(profile)
    }

    fn alias(&self) -> String {
        format!("aio/{}", self.profile_name_key)
    }

    pub(crate) fn set_capabilities(
        &mut self,
        capabilities: crate::provider_models::ProviderModelCapabilities,
    ) -> AppResult<()> {
        capabilities.validate().map_err(|_| {
            AppError::new(
                "DB_INVALID_DATA",
                "managed Codex model capabilities are invalid",
            )
        })?;
        self.capabilities = capabilities;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug)]
struct PreparedCatalogChange {
    ownership: CatalogOwnershipContext,
    ownership_after: CatalogOwnershipContext,
    codex_home_key: String,
    base_source_guard: Option<BaseCatalogGuard>,
    baseline_backup: Option<AppliedFileChange>,
    config_before: FileSnapshot,
    config_after: Vec<u8>,
    expected_catalog_binding: Option<PathBuf>,
    generated_before: FileSnapshot,
    generated_after: Option<Vec<u8>>,
    expected_owner: Option<OwnedCatalogMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CatalogOwnershipContext {
    ProxyApplied(crate::cli_proxy::CodexProxyBaseline),
    ProxyRestoredDirect(crate::cli_proxy::CodexProxyBaseline),
    Direct { config_path: PathBuf },
}

#[derive(Debug)]
struct PreparedCatalogBaseline {
    catalog_path: Option<PathBuf>,
    backup_change: Option<AppliedFileChange>,
}

struct PreparedCatalogContext {
    ownership: CatalogOwnershipContext,
    generated_path: PathBuf,
    generated_before: FileSnapshot,
    existing_metadata: Option<OwnedCatalogMetadata>,
    baseline_backup: Option<AppliedFileChange>,
    config_before: FileSnapshot,
    current_config_bytes: Vec<u8>,
    original_catalog_path: Option<PathBuf>,
    codex_home_key: String,
    legacy_catalog_ownership: LegacyCatalogOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyCatalogOwnership {
    NotLegacy,
    ActiveV2,
    Inactive,
}

#[derive(Debug)]
pub(crate) struct ManagedCatalogPlan {
    change: PreparedCatalogChange,
}

#[derive(Debug, Clone)]
struct AppliedFileChange {
    before: FileSnapshot,
    after: Option<Vec<u8>>,
}

#[derive(Debug)]
pub(crate) struct AppliedManagedCatalog {
    config: Option<AppliedFileChange>,
    generated: Option<AppliedFileChange>,
    baseline_backup: Option<AppliedFileChange>,
    direction: CatalogApplyDirection,
}

impl ManagedCatalogPlan {
    pub(crate) fn apply<R: tauri::Runtime>(
        self,
        app: &tauri::AppHandle<R>,
    ) -> AppResult<AppliedManagedCatalog> {
        let PreparedCatalogChange {
            ownership,
            ownership_after,
            codex_home_key,
            base_source_guard,
            baseline_backup,
            config_before,
            config_after,
            expected_catalog_binding,
            generated_before,
            generated_after,
            expected_owner,
        } = self.change;

        if current_codex_home_key(app)? != codex_home_key {
            return Err(AppError::new(
                "CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT",
                "the Codex home changed while preparing the managed model catalog",
            ));
        }
        if managed_catalog_path(app)? != generated_before.path {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
                "the managed Codex catalog path changed while preparing the catalog",
            ));
        }
        if catalog_ownership_context(app)? != ownership {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
                "Codex catalog ownership changed while preparing the managed model catalog",
            ));
        }
        if let Some(guard) = base_source_guard.as_ref() {
            guard.ensure_unchanged(app)?;
        }

        ensure_snapshot_unchanged(&config_before, crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES)?;
        ensure_snapshot_unchanged(&generated_before, GENERATED_CATALOG_MAX_BYTES)?;

        let baseline_expected = baseline_backup.as_ref().map(expected_snapshot_after_change);
        let config_expected = FileSnapshot {
            path: config_before.path.clone(),
            bytes: Some(config_after.clone()),
        };
        let generated_expected = FileSnapshot {
            path: generated_before.path.clone(),
            bytes: generated_after.clone(),
        };
        let applied = apply_prepared_catalog_files(
            baseline_backup,
            config_before,
            config_after,
            generated_before,
            generated_after,
        )?;

        confirm_applied_catalog(
            applied,
            baseline_expected.as_ref(),
            &config_expected,
            &generated_expected,
            || {
                if current_codex_home_key(app)? != codex_home_key {
                    return Err(AppError::new(
                        "CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT",
                        "the Codex home changed while confirming the managed model catalog",
                    ));
                }
                if !catalog_owner_matches_after_apply(
                    &catalog_ownership_context(app)?,
                    &ownership_after,
                ) {
                    return Err(AppError::new(
                        "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
                        "Codex catalog ownership changed while confirming the managed model catalog",
                    ));
                }
                if let Some(guard) = base_source_guard.as_ref() {
                    guard.ensure_unchanged(app)?;
                }
                confirm_catalog_binding(&config_expected, expected_catalog_binding.as_deref())?;
                confirm_generated_owner(
                    &generated_expected,
                    expected_owner.as_ref(),
                    &codex_home_key,
                )
            },
        )
    }
}

fn catalog_owner_matches_after_apply(
    actual: &CatalogOwnershipContext,
    expected: &CatalogOwnershipContext,
) -> bool {
    match (actual, expected) {
        (
            CatalogOwnershipContext::ProxyApplied(actual)
            | CatalogOwnershipContext::ProxyRestoredDirect(actual),
            CatalogOwnershipContext::ProxyApplied(expected)
            | CatalogOwnershipContext::ProxyRestoredDirect(expected),
        ) => actual == expected,
        (
            CatalogOwnershipContext::Direct {
                config_path: actual,
            },
            CatalogOwnershipContext::Direct {
                config_path: expected,
            },
        ) => actual == expected,
        _ => false,
    }
}

fn expected_snapshot_after_change(change: &AppliedFileChange) -> FileSnapshot {
    FileSnapshot {
        path: change.before.path.clone(),
        bytes: change.after.clone(),
    }
}

fn apply_prepared_catalog_files(
    baseline_backup: Option<AppliedFileChange>,
    config_before: FileSnapshot,
    config_after: Vec<u8>,
    generated_before: FileSnapshot,
    generated_after: Option<Vec<u8>>,
) -> AppResult<AppliedManagedCatalog> {
    let direction = if generated_after.is_some() {
        CatalogApplyDirection::ActivateOrRefresh
    } else {
        CatalogApplyDirection::Deactivate
    };
    let mut applied = AppliedManagedCatalog {
        config: None,
        generated: None,
        baseline_backup: None,
        direction,
    };

    if let Some(planned) = baseline_backup.as_ref() {
        ensure_snapshot_unchanged(&planned.before, crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES)?;
        if planned.before.bytes != planned.after {
            let after = planned.after.as_deref().ok_or_else(|| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                    "the repaired Codex proxy baseline cannot be empty",
                )
            })?;
            record_catalog_apply_stage(CatalogApplyStage::Baseline);
            injected_catalog_apply_failure(CatalogApplyStage::Baseline)?;
            crate::cli_proxy::write_cli_proxy_file_atomic(&planned.before.path, after).map_err(
                |error| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CONFIG_WRITE_FAILED",
                        format!("failed to repair Codex proxy baseline: {error}"),
                    )
                },
            )?;
            applied.baseline_backup = Some(planned.clone());
        }
    }

    let result = match direction {
        CatalogApplyDirection::ActivateOrRefresh => {
            apply_generated_catalog_change(&generated_before, generated_after.as_deref())
                .map(|change| applied.generated = change)
                .and_then(|()| {
                    apply_catalog_config_change(&config_before, &config_after)
                        .map(|change| applied.config = change)
                })
        }
        CatalogApplyDirection::Deactivate => {
            apply_catalog_config_change(&config_before, &config_after)
                .map(|change| applied.config = change)
                .and_then(|()| {
                    apply_generated_catalog_change(&generated_before, generated_after.as_deref())
                        .map(|change| applied.generated = change)
                })
        }
    };
    match result {
        Ok(()) => Ok(applied),
        Err(error) => Err(rollback_catalog_apply_failure(error, applied)),
    }
}

fn confirm_applied_catalog<F>(
    applied: AppliedManagedCatalog,
    baseline_expected: Option<&FileSnapshot>,
    config_expected: &FileSnapshot,
    generated_expected: &FileSnapshot,
    confirm_context: F,
) -> AppResult<AppliedManagedCatalog>
where
    F: FnOnce() -> AppResult<()>,
{
    let confirmation = (|| {
        injected_catalog_post_apply_mutation(
            baseline_expected,
            config_expected,
            generated_expected,
        )?;
        if let Some(expected) = baseline_expected {
            ensure_confirmed_snapshot(
                expected,
                crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
                "Codex proxy baseline",
            )?;
        }
        ensure_confirmed_snapshot(
            generated_expected,
            GENERATED_CATALOG_MAX_BYTES,
            "AIO-managed Codex catalog",
        )?;
        ensure_confirmed_snapshot(
            config_expected,
            crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
            "Codex config",
        )?;
        confirm_context()
    })();
    match confirmation {
        Ok(()) => Ok(applied),
        Err(error) => Err(rollback_catalog_apply_failure(error, applied)),
    }
}

fn ensure_confirmed_snapshot(
    expected: &FileSnapshot,
    max_len: usize,
    target: &str,
) -> AppResult<()> {
    if read_snapshot(&expected.path, max_len)? != expected.bytes {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            format!("{target} changed while confirming the managed model catalog"),
        ));
    }
    Ok(())
}

fn confirm_catalog_binding(
    config_expected: &FileSnapshot,
    expected_catalog_binding: Option<&Path>,
) -> AppResult<()> {
    let bytes = read_snapshot(
        &config_expected.path,
        crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
    )?
    .ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "Codex config disappeared while confirming the managed model catalog",
        )
    })?;
    let confirmed = parse_catalog_path(Some(&bytes), "confirmed")?;
    let matches = match (confirmed.as_deref(), expected_catalog_binding) {
        (Some(actual), Some(expected)) => catalog_paths_match(actual, expected),
        (None, None) => true,
        _ => false,
    };
    if !matches {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "model_catalog_json did not match the confirmed managed catalog binding",
        ));
    }
    Ok(())
}

fn confirm_generated_owner(
    generated_expected: &FileSnapshot,
    expected_owner: Option<&OwnedCatalogMetadata>,
    codex_home_key: &str,
) -> AppResult<()> {
    let Some(expected_owner) = expected_owner else {
        return Ok(());
    };
    let bytes = read_snapshot(&generated_expected.path, GENERATED_CATALOG_MAX_BYTES)?
        .ok_or_else(modified_catalog_error)?;
    let confirmed_owner = validate_owned_catalog(&bytes)?;
    if &confirmed_owner != expected_owner
        || confirmed_owner.codex_home_key.as_deref() != Some(codex_home_key)
    {
        return Err(modified_catalog_error());
    }
    Ok(())
}

fn apply_generated_catalog_change(
    generated_before: &FileSnapshot,
    generated_after: Option<&[u8]>,
) -> AppResult<Option<AppliedFileChange>> {
    if generated_before.bytes.as_deref() == generated_after {
        return Ok(None);
    }
    record_catalog_apply_stage(CatalogApplyStage::Generated);
    injected_catalog_apply_failure(CatalogApplyStage::Generated)?;
    apply_generated_catalog_state(&generated_before.path, generated_after)?;
    Ok(Some(AppliedFileChange {
        before: generated_before.clone(),
        after: generated_after.map(<[u8]>::to_vec),
    }))
}

fn apply_catalog_config_change(
    config_before: &FileSnapshot,
    config_after: &[u8],
) -> AppResult<Option<AppliedFileChange>> {
    if config_before.bytes.as_deref() == Some(config_after) {
        return Ok(None);
    }
    record_catalog_apply_stage(CatalogApplyStage::Config);
    injected_catalog_apply_failure(CatalogApplyStage::Config)?;
    crate::cli_proxy::write_cli_proxy_file_atomic(&config_before.path, config_after).map_err(
        |error| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_WRITE_FAILED",
                format!("failed to update Codex config.toml: {error}"),
            )
        },
    )?;
    Ok(Some(AppliedFileChange {
        before: config_before.clone(),
        after: Some(config_after.to_vec()),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogApplyDirection {
    ActivateOrRefresh,
    Deactivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogApplyStage {
    Baseline,
    Generated,
    Config,
}

#[cfg(not(test))]
fn record_catalog_apply_stage(_stage: CatalogApplyStage) {}

#[cfg(not(test))]
fn record_catalog_rollback_stage(_stage: CatalogApplyStage) {}

#[cfg(not(test))]
fn injected_catalog_apply_failure(_stage: CatalogApplyStage) -> AppResult<()> {
    Ok(())
}

#[cfg(not(test))]
fn injected_catalog_post_apply_mutation(
    _baseline_expected: Option<&FileSnapshot>,
    _config_expected: &FileSnapshot,
    _generated_expected: &FileSnapshot,
) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
thread_local! {
    static CATALOG_APPLY_FAILURE: std::cell::Cell<Option<CatalogApplyStage>> = const {
        std::cell::Cell::new(None)
    };
    static CATALOG_APPLY_TRACE: std::cell::RefCell<Vec<CatalogApplyStage>> = const {
        std::cell::RefCell::new(Vec::new())
    };
    static CATALOG_ROLLBACK_TRACE: std::cell::RefCell<Vec<CatalogApplyStage>> = const {
        std::cell::RefCell::new(Vec::new())
    };
    static CATALOG_POST_APPLY_MUTATION: std::cell::Cell<Option<CatalogApplyStage>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
fn record_catalog_apply_stage(stage: CatalogApplyStage) {
    CATALOG_APPLY_TRACE.with(|trace| trace.borrow_mut().push(stage));
}

#[cfg(test)]
fn record_catalog_rollback_stage(stage: CatalogApplyStage) {
    CATALOG_ROLLBACK_TRACE.with(|trace| trace.borrow_mut().push(stage));
}

#[cfg(test)]
fn injected_catalog_apply_failure(stage: CatalogApplyStage) -> AppResult<()> {
    let should_fail = CATALOG_APPLY_FAILURE.with(|failure| {
        if failure.get() == Some(stage) {
            failure.set(None);
            true
        } else {
            false
        }
    });
    if should_fail {
        Err(AppError::new(
            "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED",
            format!("injected {stage:?} catalog write failure"),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn injected_catalog_post_apply_mutation(
    baseline_expected: Option<&FileSnapshot>,
    config_expected: &FileSnapshot,
    generated_expected: &FileSnapshot,
) -> AppResult<()> {
    let Some(stage) = CATALOG_POST_APPLY_MUTATION.with(|mutation| mutation.take()) else {
        return Ok(());
    };
    let (path, bytes): (&Path, &[u8]) = match stage {
        CatalogApplyStage::Baseline => (
            &baseline_expected
                .ok_or_else(|| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED",
                        "the injected baseline confirmation target is missing",
                    )
                })?
                .path,
            b"external-baseline-change",
        ),
        CatalogApplyStage::Generated => (
            &generated_expected.path,
            br#"{"models":[],"external":true}"#,
        ),
        CatalogApplyStage::Config => (
            &config_expected.path,
            b"model = \"external-confirmation-change\"\n",
        ),
    };
    std::fs::write(path, bytes).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED",
            "failed to inject a post-apply catalog mutation",
        )
    })
}

impl AppliedManagedCatalog {
    pub(crate) fn rollback(self) -> AppResult<()> {
        let mut errors = Vec::new();
        match self.direction {
            CatalogApplyDirection::ActivateOrRefresh => {
                rollback_catalog_change(
                    self.config.as_ref(),
                    crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
                    Some(CatalogApplyStage::Config),
                    &mut errors,
                );
                rollback_catalog_change(
                    self.generated.as_ref(),
                    GENERATED_CATALOG_MAX_BYTES,
                    Some(CatalogApplyStage::Generated),
                    &mut errors,
                );
            }
            CatalogApplyDirection::Deactivate => {
                rollback_catalog_change(
                    self.generated.as_ref(),
                    GENERATED_CATALOG_MAX_BYTES,
                    Some(CatalogApplyStage::Generated),
                    &mut errors,
                );
                rollback_catalog_change(
                    self.config.as_ref(),
                    crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
                    Some(CatalogApplyStage::Config),
                    &mut errors,
                );
            }
        }
        rollback_catalog_change(
            self.baseline_backup.as_ref(),
            crate::cli_proxy::CLI_PROXY_FILE_MAX_BYTES,
            Some(CatalogApplyStage::Baseline),
            &mut errors,
        );
        if !errors.is_empty() {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                format!(
                    "failed to roll back managed catalog transaction: {}",
                    errors.join("; ")
                ),
            ));
        }
        Ok(())
    }
}

fn rollback_catalog_change(
    change: Option<&AppliedFileChange>,
    max_len: usize,
    stage: Option<CatalogApplyStage>,
    errors: &mut Vec<String>,
) {
    let Some(change) = change else {
        return;
    };
    if let Some(stage) = stage {
        record_catalog_rollback_stage(stage);
    }
    if let Err(error) = rollback_file_change(change, max_len) {
        errors.push(error.to_string());
    }
}

fn rollback_catalog_apply_failure(original: AppError, applied: AppliedManagedCatalog) -> AppError {
    match applied.rollback() {
        Ok(()) => original,
        Err(rollback_error) => AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            format!(
                "managed catalog update failed ({original}); rollback also failed: {rollback_error}"
            ),
        ),
    }
}

enum BaseCatalogSource {
    User {
        path: PathBuf,
        bytes: Vec<u8>,
        fingerprint: String,
    },
    Bundled {
        launch: crate::cli_manager::CodexLaunchSpec,
        descriptor: BundledCatalogDescriptor,
        fingerprint: String,
    },
}

#[derive(Debug, Clone)]
enum BaseCatalogGuard {
    User {
        path: PathBuf,
        bytes: Vec<u8>,
        fingerprint: String,
    },
    Bundled {
        descriptor: BundledCatalogDescriptor,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BundledCatalogDescriptor {
    executable: PathBuf,
    runtime_path: OsString,
    version: Option<String>,
    executable_len: u64,
    executable_modified_nanos: u128,
}

impl BaseCatalogSource {
    fn fingerprint(&self) -> &str {
        match self {
            Self::User { fingerprint, .. } | Self::Bundled { fingerprint, .. } => fingerprint,
        }
    }

    fn guard(&self) -> BaseCatalogGuard {
        match self {
            Self::User {
                path,
                bytes,
                fingerprint,
            } => BaseCatalogGuard::User {
                path: path.clone(),
                bytes: bytes.clone(),
                fingerprint: fingerprint.clone(),
            },
            Self::Bundled { descriptor, .. } => BaseCatalogGuard::Bundled {
                descriptor: descriptor.clone(),
            },
        }
    }

    fn load<R: tauri::Runtime>(self, app: &tauri::AppHandle<R>) -> AppResult<Vec<u8>> {
        match self {
            Self::User { bytes, .. } => Ok(bytes),
            Self::Bundled { launch, .. } => {
                let codex_home = crate::codex_paths::codex_home_dir(app)?;
                protocol::fetch_bundled_catalog(&launch, &codex_home).map_err(|error| {
                    let (code, message) = match error {
                        protocol::ProtocolError::Timeout => (
                            "CODEX_MANAGED_MODEL_BUNDLED_TIMEOUT",
                            "Codex debug models --bundled timed out",
                        ),
                        protocol::ProtocolError::Spawn => (
                            "CODEX_MANAGED_MODEL_BUNDLED_UNAVAILABLE",
                            "failed to run Codex debug models --bundled",
                        ),
                        protocol::ProtocolError::Malformed | protocol::ProtocolError::JsonRpc => (
                            "CODEX_MANAGED_MODEL_BUNDLED_INVALID",
                            "Codex debug models --bundled returned an invalid catalog",
                        ),
                    };
                    AppError::new(code, message)
                })
            }
        }
    }
}

impl BaseCatalogGuard {
    fn ensure_unchanged<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) -> AppResult<()> {
        match self {
            Self::User {
                path,
                bytes,
                fingerprint,
            } => ensure_user_base_catalog_unchanged(path, bytes, fingerprint),
            Self::Bundled { descriptor } => {
                let current_launch = crate::cli_manager::codex_launch_spec(app)
                    .map_err(|_| base_catalog_drift_error())?
                    .ok_or_else(base_catalog_drift_error)?;
                let current_descriptor = bundled_catalog_descriptor(&current_launch)
                    .map_err(|_| base_catalog_drift_error())?;
                if &current_descriptor != descriptor {
                    return Err(base_catalog_drift_error());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnedCatalogMetadata {
    schema_version: u64,
    profile_set_sha256: String,
    base_source_fingerprint: String,
    projection_sha256: Option<String>,
    model_context_rule_set_sha256: Option<String>,
    model_context_enabled_rules: Vec<EnabledModelContextRule>,
    model_context_enabled_rules_sha256: Option<String>,
    codex_home_key: Option<String>,
    original_catalog_path: Option<PathBuf>,
}

impl OwnedCatalogMetadata {
    fn is_legacy_v1(&self) -> bool {
        self.schema_version == LEGACY_OWNER_SCHEMA_VERSION
    }

    fn is_current(&self) -> bool {
        self.schema_version == OWNER_SCHEMA_VERSION
    }

    fn is_legacy_v2(&self) -> bool {
        self.schema_version == LEGACY_POLICY_OWNER_SCHEMA_VERSION
    }
}

#[derive(Clone, Copy)]
enum CatalogReconcileIntent<'a> {
    Background,
    ProposedConfigSave {
        previous_config: Option<&'a [u8]>,
        proposed_config: &'a [u8],
    },
}

pub(crate) fn load_profiles(conn: &Connection) -> AppResult<Vec<ManagedCatalogProfile>> {
    let mut statement = conn
        .prepare_cached(
            r#"
SELECT profile.profile_name_key, profile.model_uuid, provider.name, model.remote_model_id,
       model.capabilities_configured, model.supported_reasoning_efforts_json,
       model.default_reasoning_effort, model.context_window
FROM codex_managed_profiles profile
JOIN provider_models model ON model.model_uuid = profile.model_uuid
JOIN providers provider ON provider.id = model.provider_id
ORDER BY profile.profile_name_key ASC
"#,
        )
        .map_err(|error| db_err!("failed to prepare managed model catalog query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(RawManagedCatalogProfile {
                profile_name_key: row.get(0)?,
                model_uuid: row.get(1)?,
                provider_name: row.get(2)?,
                remote_model_id: row.get(3)?,
                capabilities_configured: row.get::<_, i64>(4)? != 0,
                supported_reasoning_efforts_json: row.get(5)?,
                default_reasoning_effort: row.get(6)?,
                context_window: row.get(7)?,
            })
        })
        .map_err(|error| db_err!("failed to query managed model catalog profiles: {error}"))?;
    let raw_profiles = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| db_err!("failed to read managed model catalog profile: {error}"))?;
    let mut profiles = Vec::with_capacity(raw_profiles.len());
    for raw in raw_profiles {
        if !raw.capabilities_configured {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "managed Codex profile references an unconfigured model",
            ));
        }
        let capabilities = crate::provider_models::decode_stored_capabilities(
            true,
            &raw.supported_reasoning_efforts_json,
            raw.default_reasoning_effort.as_deref(),
            raw.context_window,
        )?;
        profiles.push(ManagedCatalogProfile {
            profile_name_key: raw.profile_name_key,
            model_uuid: raw.model_uuid,
            provider_name: raw.provider_name,
            remote_model_id: raw.remote_model_id,
            capabilities,
        });
    }
    validate_profiles(&profiles)?;
    Ok(profiles)
}

pub(crate) fn prepare_for_profiles<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
) -> AppResult<ManagedCatalogPlan> {
    let settings = crate::settings::read(app)?;
    prepare_for_profiles_with_policy(
        app,
        profiles,
        ManagedCatalogPolicy::from_settings(&settings)?,
    )
}

pub(crate) fn prepare_for_profiles_with_policy<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
    policy: ManagedCatalogPolicy,
) -> AppResult<ManagedCatalogPlan> {
    prepare_for_profiles_with_policy_and_intent(
        app,
        profiles,
        policy,
        CatalogReconcileIntent::Background,
    )
}

fn prepare_for_profiles_with_policy_and_intent<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
    policy: ManagedCatalogPolicy,
    intent: CatalogReconcileIntent<'_>,
) -> AppResult<ManagedCatalogPlan> {
    validate_profiles(profiles)?;
    let policy = policy.canonicalized()?;
    let PreparedCatalogContext {
        ownership,
        generated_path,
        generated_before,
        existing_metadata,
        baseline_backup,
        config_before,
        current_config_bytes,
        original_catalog_path,
        codex_home_key,
        legacy_catalog_ownership,
    } = prepare_catalog_context(app, intent)?;
    let current_config = current_config_bytes.as_slice();

    let needs_generated_catalog = !profiles.is_empty() || policy.has_enabled_rules();
    if needs_generated_catalog && legacy_catalog_ownership == LegacyCatalogOwnership::Inactive {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "an inactive legacy AIO Codex catalog cannot be claimed by the current Codex home",
        ));
    }
    let (generated_after, base_source_guard) = if !needs_generated_catalog {
        let generated_after = (legacy_catalog_ownership == LegacyCatalogOwnership::Inactive)
            .then(|| generated_before.bytes.clone())
            .flatten();
        (generated_after, None)
    } else {
        let profile_set_sha256 = profile_set_sha256(profiles)?;
        let policy_projection = model_context_policy_projection(&policy)?;
        let source = base_catalog_source(app, original_catalog_path.as_deref())?;
        let source_guard = source.guard();
        let expected_projection_sha256 = projection_sha256(
            &profile_set_sha256,
            source.fingerprint(),
            &policy_projection,
            original_catalog_path.as_deref(),
            &codex_home_key,
        )?;
        if existing_metadata.as_ref().is_some_and(|metadata| {
            metadata.is_current()
                && metadata.profile_set_sha256 == profile_set_sha256
                && metadata.base_source_fingerprint == source.fingerprint()
                && metadata.projection_sha256.as_deref()
                    == Some(expected_projection_sha256.as_str())
                && metadata.model_context_rule_set_sha256.as_deref()
                    == Some(policy_projection.rule_set_sha256.as_str())
                && metadata.model_context_enabled_rules == policy_projection.enabled_rules
                && metadata.model_context_enabled_rules_sha256.as_deref()
                    == Some(policy_projection.enabled_rules_sha256.as_str())
                && metadata.codex_home_key.as_deref() == Some(codex_home_key.as_str())
                && metadata.original_catalog_path == original_catalog_path
        }) {
            (generated_before.bytes.clone(), Some(source_guard))
        } else {
            let source_fingerprint = source.fingerprint().to_string();
            let base_bytes = source.load(app)?;
            (
                Some(generate_catalog(
                    &base_bytes,
                    profiles,
                    &CatalogGenerationContext {
                        profile_set_sha256: &profile_set_sha256,
                        base_source_fingerprint: &source_fingerprint,
                        policy: &policy,
                        policy_projection: &policy_projection,
                        original_catalog_path: original_catalog_path.as_deref(),
                        codex_home_key: &codex_home_key,
                    },
                )?),
                Some(source_guard),
            )
        }
    };

    let desired_catalog_path = needs_generated_catalog.then_some(generated_path.as_path());
    let expected_catalog_binding = desired_catalog_path
        .map(Path::to_path_buf)
        .or_else(|| original_catalog_path.clone());
    let config_after = patch_model_catalog_config_with_original_path(
        current_config,
        original_catalog_path.as_deref(),
        desired_catalog_path,
    )?;
    let expected_owner = if needs_generated_catalog {
        let metadata = generated_after
            .as_deref()
            .ok_or_else(|| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                    "the active AIO Codex catalog has no expected owner metadata",
                )
            })
            .and_then(validate_owned_catalog)?;
        if !metadata.is_current() {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "the active AIO Codex catalog owner metadata was not upgraded",
            ));
        }
        Some(metadata)
    } else {
        None
    };
    let ownership_after =
        catalog_ownership_after_baseline_change(&ownership, baseline_backup.as_ref())?;

    Ok(ManagedCatalogPlan {
        change: PreparedCatalogChange {
            ownership,
            ownership_after,
            codex_home_key,
            base_source_guard,
            baseline_backup,
            config_before,
            config_after,
            expected_catalog_binding,
            generated_before,
            generated_after,
            expected_owner,
        },
    })
}

fn prepare_catalog_context<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    intent: CatalogReconcileIntent<'_>,
) -> AppResult<PreparedCatalogContext> {
    let ownership = catalog_ownership_context(app)?;
    let codex_home_key = current_codex_home_key(app)?;
    let generated_path = managed_catalog_path(app)?;
    let generated_before = snapshot_generated_file(&generated_path)?;
    let existing_metadata = generated_before
        .bytes
        .as_deref()
        .map(validate_owned_catalog)
        .transpose()?;
    validate_catalog_home_identity(existing_metadata.as_ref(), &codex_home_key)?;

    let (baseline_backup, config_before, current_config_bytes, original_catalog_path) =
        match &ownership {
            CatalogOwnershipContext::ProxyApplied(baseline)
            | CatalogOwnershipContext::ProxyRestoredDirect(baseline) => {
                let prepared_baseline = prepare_catalog_baseline(
                    baseline,
                    &generated_path,
                    existing_metadata.as_ref(),
                )?;
                let config_before = snapshot_cli_proxy_file(&baseline.config_path)?;
                validate_config_save_snapshot(intent, &config_before)?;
                let current = match intent {
                    CatalogReconcileIntent::ProposedConfigSave {
                        proposed_config, ..
                    } => proposed_config.to_vec(),
                    CatalogReconcileIntent::Background => match &ownership {
                        CatalogOwnershipContext::ProxyApplied(_) => {
                            config_before.bytes.clone().ok_or_else(|| {
                                AppError::new(
                                    "CODEX_MANAGED_MODEL_CONFIG_MISSING",
                                    "Codex config.toml disappeared while the CLI proxy was enabled",
                                )
                            })?
                        }
                        CatalogOwnershipContext::ProxyRestoredDirect(_) => {
                            config_before.bytes.clone().unwrap_or_default()
                        }
                        CatalogOwnershipContext::Direct { .. } => unreachable!(),
                    },
                };
                (
                    prepared_baseline.backup_change,
                    config_before,
                    current,
                    prepared_baseline.catalog_path,
                )
            }
            CatalogOwnershipContext::Direct { config_path } => {
                let config_before = snapshot_cli_proxy_file(config_path)?;
                validate_config_save_snapshot(intent, &config_before)?;
                let current = match intent {
                    CatalogReconcileIntent::Background => {
                        config_before.bytes.clone().unwrap_or_default()
                    }
                    CatalogReconcileIntent::ProposedConfigSave {
                        proposed_config, ..
                    } => proposed_config.to_vec(),
                };
                let current_catalog_path = parse_catalog_path(Some(&current), "current")?;
                let original = match intent {
                    CatalogReconcileIntent::Background => direct_original_catalog_path(
                        current_catalog_path.as_deref(),
                        &generated_path,
                        existing_metadata.as_ref(),
                    )?,
                    CatalogReconcileIntent::ProposedConfigSave {
                        previous_config,
                        proposed_config,
                    } => explicit_save_original_catalog_path(
                        previous_config,
                        proposed_config,
                        &generated_path,
                        existing_metadata.as_ref(),
                    )?,
                };
                (None, config_before, current, original)
            }
        };
    let legacy_catalog_ownership = classify_legacy_catalog_ownership(
        existing_metadata.as_ref(),
        &ownership,
        config_before.bytes.as_deref(),
        &generated_path,
    )?;
    validate_current_catalog_binding(
        &current_config_bytes,
        original_catalog_path.as_deref(),
        &generated_path,
    )?;

    Ok(PreparedCatalogContext {
        ownership,
        generated_path,
        generated_before,
        existing_metadata,
        baseline_backup,
        config_before,
        current_config_bytes,
        original_catalog_path,
        codex_home_key,
        legacy_catalog_ownership,
    })
}

pub(crate) fn read_original_base_catalog_locked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CodexBaseCatalogRead> {
    let context = prepare_catalog_context(app, CatalogReconcileIntent::Background)?;
    let source = base_catalog_source(app, context.original_catalog_path.as_deref())?;
    let bytes = source.load(app)?;
    Ok(CodexBaseCatalogRead { bytes })
}

fn validate_config_save_snapshot(
    intent: CatalogReconcileIntent<'_>,
    config_before: &FileSnapshot,
) -> AppResult<()> {
    let CatalogReconcileIntent::ProposedConfigSave {
        previous_config, ..
    } = intent
    else {
        return Ok(());
    };
    if config_before.bytes.as_deref() != previous_config {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "Codex config.toml changed while preparing the explicit config save",
        ));
    }
    Ok(())
}

fn direct_original_catalog_path(
    current_catalog_path: Option<&Path>,
    generated_path: &Path,
    existing_metadata: Option<&OwnedCatalogMetadata>,
) -> AppResult<Option<PathBuf>> {
    let current_is_generated =
        current_catalog_path.is_some_and(|path| catalog_paths_match(path, generated_path));
    let Some(metadata) = existing_metadata else {
        if current_is_generated {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "the active AIO Codex catalog has no recoverable ownership metadata",
            ));
        }
        return Ok(current_catalog_path.map(Path::to_path_buf));
    };
    if metadata.is_legacy_v1() {
        if current_is_generated {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "a legacy AIO Codex catalog cannot recover its original direct-mode binding",
            ));
        }
        // Older releases could leave an inactive v1 generated file behind after
        // restoring direct config. Its metadata lacks an original binding, but
        // the unbound current config is already the authoritative base.
        return Ok(current_catalog_path.map(Path::to_path_buf));
    }
    if metadata.is_legacy_v2() {
        return if current_is_generated {
            Ok(metadata.original_catalog_path.clone())
        } else {
            Ok(current_catalog_path.map(Path::to_path_buf))
        };
    }
    if current_is_generated || current_catalog_path == metadata.original_catalog_path.as_deref() {
        return Ok(metadata.original_catalog_path.clone());
    }
    Err(AppError::new(
        "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
        "model_catalog_json changed outside AIO while the managed catalog was active",
    ))
}

fn classify_legacy_catalog_ownership(
    metadata: Option<&OwnedCatalogMetadata>,
    ownership: &CatalogOwnershipContext,
    live_config: Option<&[u8]>,
    generated_path: &Path,
) -> AppResult<LegacyCatalogOwnership> {
    let Some(metadata) = metadata else {
        return Ok(LegacyCatalogOwnership::NotLegacy);
    };
    if metadata.is_current() {
        return Ok(LegacyCatalogOwnership::NotLegacy);
    }

    let live_binding = parse_catalog_path(live_config, "current")?;
    let live_is_generated = live_binding
        .as_deref()
        .is_some_and(|path| catalog_paths_match(path, generated_path));
    let baseline_is_generated = match ownership {
        CatalogOwnershipContext::ProxyApplied(baseline)
        | CatalogOwnershipContext::ProxyRestoredDirect(baseline) => {
            parse_catalog_path(baseline.config_bytes.as_deref(), "proxy baseline")?
                .as_deref()
                .is_some_and(|path| catalog_paths_match(path, generated_path))
        }
        CatalogOwnershipContext::Direct { .. } => false,
    };
    let active = live_is_generated || baseline_is_generated;
    if metadata.is_legacy_v1() {
        if active {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "a legacy AIO Codex catalog cannot prove its original binding",
            ));
        }
        return Ok(LegacyCatalogOwnership::Inactive);
    }
    if metadata.is_legacy_v2() && active {
        return Ok(LegacyCatalogOwnership::ActiveV2);
    }
    Ok(LegacyCatalogOwnership::Inactive)
}

fn catalog_ownership_after_baseline_change(
    ownership: &CatalogOwnershipContext,
    baseline_change: Option<&AppliedFileChange>,
) -> AppResult<CatalogOwnershipContext> {
    let mut expected = ownership.clone();
    let Some(change) = baseline_change else {
        return Ok(expected);
    };
    let baseline = match &mut expected {
        CatalogOwnershipContext::ProxyApplied(baseline)
        | CatalogOwnershipContext::ProxyRestoredDirect(baseline) => baseline,
        CatalogOwnershipContext::Direct { .. } => {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "a direct Codex catalog plan cannot repair a proxy baseline",
            ));
        }
    };
    if baseline.config_backup_path.as_deref() != Some(change.before.path.as_path()) {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the Codex proxy baseline repair target changed unexpectedly",
        ));
    }
    baseline.config_bytes = Some(change.after.clone().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the repaired Codex proxy baseline cannot be empty",
        )
    })?);
    Ok(expected)
}

fn explicit_save_original_catalog_path(
    previous_config: Option<&[u8]>,
    committed_config: &[u8],
    generated_path: &Path,
    existing_metadata: Option<&OwnedCatalogMetadata>,
) -> AppResult<Option<PathBuf>> {
    let previous_catalog_path = parse_catalog_path(previous_config, "previous")?;
    let previous_original = direct_original_catalog_path(
        previous_catalog_path.as_deref(),
        generated_path,
        existing_metadata,
    )?;
    let committed_catalog_path = parse_catalog_path(Some(committed_config), "committed")?;
    if committed_catalog_path
        .as_deref()
        .is_some_and(|path| catalog_paths_match(path, generated_path))
    {
        if existing_metadata.is_none() {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "an explicit Codex config save referenced an unowned AIO catalog",
            ));
        }
        return Ok(previous_original);
    }
    Ok(committed_catalog_path)
}

fn catalog_ownership_context<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<CatalogOwnershipContext> {
    if let Some(baseline) = crate::cli_proxy::codex_enabled_proxy_baseline(app)? {
        if crate::cli_proxy::codex_proxy_config_is_applied(app, &baseline.base_origin) {
            return Ok(CatalogOwnershipContext::ProxyApplied(baseline));
        }
        return Ok(CatalogOwnershipContext::ProxyRestoredDirect(baseline));
    }
    Ok(CatalogOwnershipContext::Direct {
        config_path: crate::codex_paths::codex_config_toml_path(app)?,
    })
}

pub(crate) fn sync_current_locked<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<()> {
    #[cfg(test)]
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
    let db = crate::db::init(app)?;
    let conn = db.open_connection()?;
    let profiles = load_profiles(&conn)?;
    let _applied = prepare_for_profiles(app, &profiles)?.apply(app)?;
    Ok(())
}

pub(crate) fn sync_current_after_config_save_locked<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    previous_config: Option<&[u8]>,
    proposed_config: &[u8],
) -> AppResult<AppliedManagedCatalog> {
    #[cfg(test)]
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
    let db = crate::db::init(app)?;
    let conn = db.open_connection()?;
    let profiles = load_profiles(&conn)?;
    let settings = crate::settings::read(app)?;
    let policy = ManagedCatalogPolicy::from_settings(&settings)?;
    prepare_for_profiles_with_policy_and_intent(
        app,
        &profiles,
        policy,
        CatalogReconcileIntent::ProposedConfigSave {
            previous_config,
            proposed_config,
        },
    )?
    .apply(app)
}

pub(crate) fn preserve_active_binding<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    baseline: Option<&[u8]>,
    current: Option<&[u8]>,
    projected: &[u8],
) -> AppResult<Vec<u8>> {
    let generated = managed_catalog_path(app)?;
    let original = parse_original_catalog_path(baseline)?;
    let current_path = parse_catalog_path(current, "current")?;
    validate_current_catalog_binding(current.unwrap_or_default(), original.as_deref(), &generated)?;
    let generated_path =
        (current_path.as_deref() == Some(generated.as_path())).then_some(generated.as_path());
    patch_model_catalog_config(projected, baseline, generated_path)
}

#[cfg(test)]
thread_local! {
    static OAUTH_SYNC_CATALOG_INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_sync_current_invocations_for_test() {
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn sync_current_invocations_for_test() -> usize {
    OAUTH_SYNC_CATALOG_INVOCATIONS.with(std::cell::Cell::get)
}

fn managed_catalog_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<PathBuf> {
    managed_catalog_path_from_root(&crate::app_paths::app_data_dir(app)?)
}

fn managed_catalog_path_from_root(app_data_dir: &Path) -> AppResult<PathBuf> {
    let parent = app_data_dir.join("cli-proxy").join("codex");
    Ok(resolve_directory_path_without_creation(&parent)?.join(GENERATED_CATALOG_FILE_NAME))
}

fn resolve_directory_path_without_creation(path: &Path) -> AppResult<PathBuf> {
    let mut cursor = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => {
                let mut resolved = std::fs::canonicalize(cursor).map_err(|_| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                        "failed to resolve the managed Codex catalog directory",
                    )
                })?;
                if !std::fs::metadata(&resolved)
                    .map(|metadata| metadata.is_dir())
                    .unwrap_or(false)
                {
                    return Err(AppError::new(
                        "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                        "the managed Codex catalog ancestor is not a directory",
                    ));
                }
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let component = cursor.file_name().ok_or_else(|| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                        "the managed Codex catalog directory has no resolvable ancestor",
                    )
                })?;
                missing.push(component.to_os_string());
                cursor = cursor.parent().ok_or_else(|| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                        "the managed Codex catalog directory has no resolvable ancestor",
                    )
                })?;
            }
            Err(_) => {
                return Err(AppError::new(
                    "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                    "failed to inspect the managed Codex catalog directory",
                ));
            }
        }
    }
}

fn current_codex_home_key<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<String> {
    let home = crate::codex_paths::codex_home_dir(app)?;
    normalized_codex_home_key(&home)
}

fn normalized_codex_home_key(home: &Path) -> AppResult<String> {
    if !home.is_absolute() {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "the resolved Codex home must be absolute",
        ));
    }
    let resolved = canonicalize_allow_missing(home);
    let key = resolved
        .to_str()
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                "the resolved Codex home must be valid UTF-8",
            )
        })?
        .to_string();
    #[cfg(windows)]
    let key = {
        let mut key = key;
        key = key.replace('\\', "/");
        key.make_ascii_lowercase();
        key
    };
    Ok(key)
}

fn canonicalize_allow_missing(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }

    let mut cursor = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(cursor) {
            for component in missing.iter().rev() {
                resolved.push(component);
            }
            return resolved;
        }

        let Some(file_name) = cursor.file_name() else {
            return path.to_path_buf();
        };
        missing.push(file_name.to_os_string());

        let Some(parent) = cursor.parent() else {
            return path.to_path_buf();
        };
        if parent == cursor {
            return path.to_path_buf();
        }
        cursor = parent;
    }
}

fn validate_catalog_home_identity(
    metadata: Option<&OwnedCatalogMetadata>,
    current_codex_home_key: &str,
) -> AppResult<()> {
    let Some(metadata) = metadata.filter(|metadata| metadata.is_current()) else {
        return Ok(());
    };
    if metadata.codex_home_key.as_deref() != Some(current_codex_home_key) {
        return Err(AppError::new(
            "CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT",
            "the Codex home changed while an AIO-managed catalog was owned by another home",
        ));
    }
    Ok(())
}

fn ensure_managed_catalog_parent(path: &Path) -> AppResult<()> {
    let parent = path.parent().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
            "the managed Codex catalog path has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
            "failed to create the managed Codex catalog directory",
        )
    })
}

fn validate_profile(profile: &ManagedCatalogProfile) -> AppResult<()> {
    let key = profile.profile_name_key.as_bytes();
    let valid_key = !key.is_empty()
        && key.len() <= 64
        && key[0].is_ascii_lowercase_or_digit()
        && key
            .iter()
            .all(|byte| byte.is_ascii_lowercase_or_digit() || matches!(byte, b'_' | b'-'));
    if !valid_key
        || crate::shared::uuid::is_canonical_uuid_v4(&profile.profile_name_key)
        || !crate::shared::uuid::is_canonical_uuid_v4(&profile.model_uuid)
    {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile identity is invalid",
        ));
    }
    if profile.remote_model_id.trim().is_empty() || profile.remote_model_id.len() > 256 {
        return Err(AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile remote model is invalid",
        ));
    }
    profile.capabilities.validate().map_err(|_| {
        AppError::new(
            "DB_INVALID_DATA",
            "managed Codex profile capabilities are invalid",
        )
    })?;
    Ok(())
}

trait AsciiLowercaseOrDigit {
    fn is_ascii_lowercase_or_digit(&self) -> bool;
}

impl AsciiLowercaseOrDigit for u8 {
    fn is_ascii_lowercase_or_digit(&self) -> bool {
        self.is_ascii_lowercase() || self.is_ascii_digit()
    }
}

fn validate_profiles(profiles: &[ManagedCatalogProfile]) -> AppResult<()> {
    if profiles.len() > MAX_MANAGED_PROFILE_COUNT {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_PROFILE_LIMIT",
            "too many managed Codex profiles to build a bounded model catalog",
        ));
    }
    let mut aliases = HashSet::with_capacity(profiles.len());
    for profile in profiles {
        validate_profile(profile)?;
        if !aliases.insert(profile.alias()) {
            return Err(AppError::new(
                "DB_INVALID_DATA",
                "managed Codex profile aliases are not unique",
            ));
        }
    }
    Ok(())
}

fn snapshot_cli_proxy_file(path: &Path) -> AppResult<FileSnapshot> {
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        bytes: crate::cli_proxy::read_optional_cli_proxy_file(path)?,
    })
}

fn snapshot_generated_file(path: &Path) -> AppResult<FileSnapshot> {
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        bytes: crate::shared::fs::read_optional_file_with_max_len(
            path,
            GENERATED_CATALOG_MAX_BYTES,
        )?,
    })
}

fn read_snapshot(path: &Path, max_len: usize) -> AppResult<Option<Vec<u8>>> {
    crate::shared::fs::read_optional_file_with_max_len(path, max_len)
}

fn ensure_snapshot_unchanged(snapshot: &FileSnapshot, max_len: usize) -> AppResult<()> {
    if read_snapshot(&snapshot.path, max_len)? != snapshot.bytes {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "a managed Codex catalog target changed while preparing the catalog",
        ));
    }
    Ok(())
}

fn rollback_file_change(change: &AppliedFileChange, max_len: usize) -> AppResult<()> {
    let current = read_snapshot(&change.before.path, max_len)?;
    if current != change.after {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "a managed Codex catalog target changed after update; refusing to overwrite it",
        ));
    }
    match change.before.bytes.as_deref() {
        Some(bytes) => crate::shared::fs::write_file_atomic(&change.before.path, bytes),
        None => match std::fs::remove_file(&change.before.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AppError::new(
                "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
                "failed to remove a managed Codex catalog rollback target",
            )),
        },
    }
}

fn apply_generated_catalog_state(path: &Path, bytes: Option<&[u8]>) -> AppResult<()> {
    match bytes {
        Some(bytes) => {
            ensure_managed_catalog_parent(path)?;
            crate::shared::fs::write_file_atomic(path, bytes).map_err(|_| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                    "failed to write the AIO-managed Codex model catalog",
                )
            })
        }
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AppError::new(
                "CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED",
                "failed to remove the inactive AIO-managed Codex model catalog",
            )),
        },
    }
}

fn parse_original_catalog_path(config: Option<&[u8]>) -> AppResult<Option<PathBuf>> {
    parse_catalog_path(config, "original")
}

fn prepare_catalog_baseline(
    baseline: &crate::cli_proxy::CodexProxyBaseline,
    generated_path: &Path,
    existing_metadata: Option<&OwnedCatalogMetadata>,
) -> AppResult<PreparedCatalogBaseline> {
    let configured_catalog_path = parse_original_catalog_path(baseline.config_bytes.as_deref())?;
    if !configured_catalog_path
        .as_deref()
        .is_some_and(|path| catalog_paths_match(path, generated_path))
    {
        if let Some(path) = configured_catalog_path.as_deref() {
            reject_generated_path_as_base(path, generated_path)?;
        }
        return Ok(PreparedCatalogBaseline {
            catalog_path: configured_catalog_path,
            backup_change: None,
        });
    }

    let backup_path = baseline.config_backup_path.as_ref().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the polluted Codex proxy baseline has no recoverable backup path",
        )
    })?;
    let before = snapshot_cli_proxy_file(backup_path)?;
    if before.bytes != baseline.config_bytes {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "the Codex proxy baseline changed while preparing catalog repair",
        ));
    }
    let current = before.bytes.as_deref().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the polluted Codex proxy baseline disappeared",
        )
    })?;
    let metadata = existing_metadata.ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "the active AIO Codex catalog has no recoverable ownership metadata",
        )
    })?;
    if metadata.is_legacy_v1() {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED",
            "a legacy AIO Codex catalog cannot recover its original proxy baseline binding",
        ));
    }
    let recovered_catalog_path = metadata.original_catalog_path.clone();
    let repaired = patch_model_catalog_config_with_original_path(
        current,
        recovered_catalog_path.as_deref(),
        None,
    )?;
    Ok(PreparedCatalogBaseline {
        catalog_path: recovered_catalog_path,
        backup_change: Some(AppliedFileChange {
            before,
            after: Some(repaired),
        }),
    })
}

fn parse_catalog_path(config: Option<&[u8]>, source: &str) -> AppResult<Option<PathBuf>> {
    let Some(config) = config else {
        return Ok(None);
    };
    let text = std::str::from_utf8(config).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("the {source} Codex config.toml is not UTF-8"),
        )
    })?;
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("the {source} Codex config.toml is invalid TOML"),
        )
    })?;
    let Some(item) = document.get("model_catalog_json") else {
        return Ok(None);
    };
    let value = item.as_str().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("model_catalog_json in the {source} Codex config must be a string"),
        )
    })?;
    let path = PathBuf::from(value);
    if value.is_empty() || !path.is_absolute() {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            format!("model_catalog_json in the {source} Codex config must be an absolute path"),
        ));
    }
    Ok(Some(path))
}

fn validate_current_catalog_binding(
    current: &[u8],
    original: Option<&Path>,
    generated: &Path,
) -> AppResult<()> {
    let current_path = parse_catalog_path(Some(current), "current")?;
    let matches_original = current_path.as_deref() == original;
    let matches_generated = current_path.as_deref() == Some(generated);
    if !matches_original && !matches_generated {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_DRIFT",
            "model_catalog_json changed outside AIO while the managed catalog was active",
        ));
    }
    Ok(())
}

fn patch_model_catalog_config(
    current: &[u8],
    original: Option<&[u8]>,
    generated_path: Option<&Path>,
) -> AppResult<Vec<u8>> {
    let original_path = parse_original_catalog_path(original)?;
    patch_model_catalog_config_with_original_path(current, original_path.as_deref(), generated_path)
}

fn patch_model_catalog_config_with_original_path(
    current: &[u8],
    original_path: Option<&Path>,
    generated_path: Option<&Path>,
) -> AppResult<Vec<u8>> {
    let current = std::str::from_utf8(current).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "current Codex config.toml is not UTF-8",
        )
    })?;
    let mut document = current.parse::<toml_edit::DocumentMut>().map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "current Codex config.toml is invalid TOML",
        )
    })?;
    if document
        .get("model_catalog_json")
        .is_some_and(|item| item.as_str().is_none())
    {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "current model_catalog_json must be a string",
        ));
    }

    let desired = match generated_path {
        Some(path) => Some(
            path.to_str()
                .ok_or_else(|| {
                    AppError::new(
                        "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                        "managed model catalog path must be valid UTF-8",
                    )
                })?
                .to_string(),
        ),
        None => original_path.and_then(|path| path.to_str().map(str::to_string)),
    };
    match desired.as_deref() {
        Some(path) => document["model_catalog_json"] = toml_edit::value(path),
        None => {
            document.remove("model_catalog_json");
        }
    }
    let output = document.to_string().into_bytes();
    let reparsed = std::str::from_utf8(&output)
        .ok()
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                "generated Codex config.toml failed validation",
            )
        })?;
    if reparsed
        .get("model_catalog_json")
        .and_then(toml_edit::Item::as_str)
        != desired.as_deref()
    {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CONFIG_INVALID",
            "generated model_catalog_json failed round-trip validation",
        ));
    }
    Ok(output)
}

fn reject_generated_path_as_base(base: &Path, generated: &Path) -> AppResult<()> {
    if catalog_paths_match(base, generated) {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the AIO-generated catalog cannot be used as its own base catalog",
        ));
    }
    Ok(())
}

fn catalog_paths_match(base: &Path, generated: &Path) -> bool {
    if base == generated {
        true
    } else {
        match (
            std::fs::canonicalize(base),
            std::fs::canonicalize(generated),
        ) {
            (Ok(base), Ok(generated)) => base == generated,
            _ => false,
        }
    }
}

fn base_catalog_source<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    original_catalog_path: Option<&Path>,
) -> AppResult<BaseCatalogSource> {
    if let Some(path) = original_catalog_path {
        let bytes = crate::shared::fs::read_file_with_max_len(path, USER_CATALOG_MAX_BYTES)
            .map_err(|_| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_BASE_CATALOG_UNAVAILABLE",
                    "failed to read the user-configured Codex model catalog",
                )
            })?;
        let fingerprint = user_catalog_fingerprint(path, &bytes);
        return Ok(BaseCatalogSource::User {
            path: path.to_path_buf(),
            bytes,
            fingerprint,
        });
    }

    let launch = crate::cli_manager::codex_launch_spec(app)?.ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CLI_NOT_FOUND",
            "Codex CLI was not found",
        )
    })?;
    let descriptor = bundled_catalog_descriptor(&launch)?;
    let fingerprint = bundled_catalog_fingerprint(&descriptor);
    Ok(BaseCatalogSource::Bundled {
        launch,
        descriptor,
        fingerprint,
    })
}

fn user_catalog_fingerprint(path: &Path, bytes: &[u8]) -> String {
    sha256_hex(
        &[
            b"user\0".as_slice(),
            path.to_string_lossy().as_bytes(),
            b"\0",
            sha256_hex(bytes).as_bytes(),
        ]
        .concat(),
    )
}

fn ensure_user_base_catalog_unchanged(
    path: &Path,
    expected_bytes: &[u8],
    expected_fingerprint: &str,
) -> AppResult<()> {
    let current = crate::shared::fs::read_file_with_max_len(path, USER_CATALOG_MAX_BYTES)
        .map_err(|_| base_catalog_drift_error())?;
    if current != expected_bytes || user_catalog_fingerprint(path, &current) != expected_fingerprint
    {
        return Err(base_catalog_drift_error());
    }
    Ok(())
}

fn bundled_catalog_descriptor(
    launch: &crate::cli_manager::CodexLaunchSpec,
) -> AppResult<BundledCatalogDescriptor> {
    let metadata = std::fs::metadata(&launch.executable).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_CLI_NOT_FOUND",
            "the resolved Codex executable is unavailable",
        )
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    Ok(BundledCatalogDescriptor {
        executable: launch.executable.clone(),
        runtime_path: launch.runtime_path.clone(),
        version: launch.version.clone(),
        executable_len: metadata.len(),
        executable_modified_nanos: modified,
    })
}

fn bundled_catalog_fingerprint(descriptor: &BundledCatalogDescriptor) -> String {
    let payload = format!(
        "bundled\0{}\0{}\0{}\0{}\0{}",
        descriptor.executable.to_string_lossy(),
        descriptor.runtime_path.to_string_lossy(),
        descriptor.version.as_deref().unwrap_or(""),
        descriptor.executable_len,
        descriptor.executable_modified_nanos,
    );
    sha256_hex(payload.as_bytes())
}

fn base_catalog_drift_error() -> AppError {
    AppError::new(
        "CODEX_MANAGED_MODEL_BASE_CATALOG_DRIFT",
        "the Codex base model catalog changed while preparing the managed catalog",
    )
}

fn profile_set_sha256(profiles: &[ManagedCatalogProfile]) -> AppResult<String> {
    let payload = profiles
        .iter()
        .map(|profile| {
            json!({
                "alias": profile.alias(),
                "model_uuid": profile.model_uuid,
                "provider_name": profile.provider_name,
                "remote_model_id": profile.remote_model_id,
                "supported_reasoning_efforts": profile.capabilities.supported_reasoning_efforts
                    .iter()
                    .map(|effort| effort.as_str())
                    .collect::<Vec<_>>(),
                "default_reasoning_effort": profile.capabilities.default_reasoning_effort
                    .map(crate::provider_models::ProviderModelReasoningEffort::as_str),
                "context_window": profile.capabilities.context_window,
            })
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&payload).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to serialize the managed Codex profile set",
        )
    })?;
    Ok(sha256_hex(&bytes))
}

fn model_context_policy_projection(
    policy: &ManagedCatalogPolicy,
) -> AppResult<ModelContextPolicyProjection> {
    let rule_set_bytes = serde_json::to_vec(&policy.model_context_rules).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to serialize the Codex model context rule set",
        )
    })?;
    let enabled_rules = policy
        .enabled_rules()
        .map(|rule| EnabledModelContextRule {
            model_id: rule.model_id.clone(),
            context_window: rule.context_window,
        })
        .collect::<Vec<_>>();
    let enabled_rule_bytes = serde_json::to_vec(&enabled_rules).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to serialize the enabled Codex model context rules",
        )
    })?;
    Ok(ModelContextPolicyProjection {
        rule_set_sha256: sha256_hex(&rule_set_bytes),
        enabled_rules_sha256: sha256_hex(&enabled_rule_bytes),
        enabled_rules,
    })
}

fn projection_value(
    profile_set_sha256: &str,
    base_source_fingerprint: &str,
    policy: &ModelContextPolicyProjection,
    original_catalog_path: Option<&Path>,
    codex_home_key: &str,
) -> AppResult<Value> {
    let original_catalog_path = catalog_path_string(original_catalog_path)?;
    Ok(json!({
        "schema_version": OWNER_SCHEMA_VERSION,
        "projection_algorithm_version": MODEL_CONTEXT_PROJECTION_VERSION,
        "profile_set_sha256": profile_set_sha256,
        "base_source_fingerprint": base_source_fingerprint,
        "original_catalog_path": original_catalog_path,
        "codex_home_key": codex_home_key,
        "model_context_rule_set_sha256": policy.rule_set_sha256,
        "model_context_enabled_rules": policy.enabled_rules,
        "model_context_enabled_rules_sha256": policy.enabled_rules_sha256,
    }))
}

fn projection_sha256(
    profile_set_sha256: &str,
    base_source_fingerprint: &str,
    policy: &ModelContextPolicyProjection,
    original_catalog_path: Option<&Path>,
    codex_home_key: &str,
) -> AppResult<String> {
    let bytes = serde_json::to_vec(&projection_value(
        profile_set_sha256,
        base_source_fingerprint,
        policy,
        original_catalog_path,
        codex_home_key,
    )?)
    .map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to hash the managed Codex catalog projection",
        )
    })?;
    Ok(sha256_hex(&bytes))
}

fn legacy_v2_projection_sha256(
    profile_set_sha256: &str,
    base_source_fingerprint: &str,
    gpt56_372k_context_enabled: bool,
    original_catalog_path: Option<&Path>,
) -> AppResult<String> {
    let original_catalog_path = catalog_path_string(original_catalog_path)?;
    let bytes = serde_json::to_vec(&json!({
        "schema_version": LEGACY_POLICY_OWNER_SCHEMA_VERSION,
        "profile_set_sha256": profile_set_sha256,
        "base_source_fingerprint": base_source_fingerprint,
        "original_catalog_path": original_catalog_path,
        "gpt56_372k_policy_version": LEGACY_GPT56_372K_POLICY_VERSION,
        "gpt56_372k_context_enabled": gpt56_372k_context_enabled,
        "gpt56_372k_context_tokens": LEGACY_GPT56_372K_CONTEXT_TOKENS,
        "gpt56_372k_model_slugs": LEGACY_GPT56_372K_MODEL_SLUGS,
    }))
    .map_err(|_| modified_catalog_error())?;
    Ok(sha256_hex(&bytes))
}

fn catalog_path_string(path: Option<&Path>) -> AppResult<Option<String>> {
    path.map(|path| {
        path.to_str().map(str::to_string).ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_CONFIG_INVALID",
                "model_catalog_json path must be valid UTF-8",
            )
        })
    })
    .transpose()
}

fn generate_catalog(
    base_bytes: &[u8],
    profiles: &[ManagedCatalogProfile],
    context: &CatalogGenerationContext<'_>,
) -> AppResult<Vec<u8>> {
    let profile_set_sha256 = context.profile_set_sha256;
    let base_source_fingerprint = context.base_source_fingerprint;
    let policy = context.policy;
    let policy_projection = context.policy_projection;
    let original_catalog_path = context.original_catalog_path;
    let codex_home_key = context.codex_home_key;
    if model_context_policy_projection(policy)? != *policy_projection {
        return Err(AppError::new(
            "SYSTEM_ERROR",
            "the managed Codex catalog policy projection is inconsistent",
        ));
    }
    let mut root: Value = serde_json::from_slice(base_bytes).map_err(|_| {
        AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog is not valid JSON",
        )
    })?;
    let object = root.as_object_mut().ok_or_else(|| {
        AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog root must be an object",
        )
    })?;
    if object.contains_key(OWNER_METADATA_KEY) {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog contains reserved AIO metadata",
        ));
    }
    let models = object
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "the base Codex model catalog must contain a models array",
            )
        })?;
    if models.is_empty() || models.len() > MAX_BASE_MODEL_COUNT {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
            "the base Codex model catalog has an invalid model count",
        ));
    }

    let mut slugs = HashSet::with_capacity(models.len() + profiles.len());
    let mut slug_indices = HashMap::with_capacity(models.len());
    let enabled_targets = policy
        .enabled_rules()
        .map(|rule| (rule.model_id.as_str(), rule.context_window))
        .collect::<HashMap<_, _>>();
    let mut template = None;
    for (index, model) in models.iter().enumerate() {
        let model_object = model.as_object().ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "every base Codex model must be an object",
            )
        })?;
        let slug = model_object
            .get("slug")
            .and_then(Value::as_str)
            .filter(|slug| !slug.is_empty() && slug.len() <= 256)
            .ok_or_else(|| {
                AppError::new(
                    "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                    "every base Codex model must have a valid slug",
                )
            })?;
        if !slugs.insert(slug.to_string()) {
            if enabled_targets.contains_key(slug) {
                return Err(AppError::new(
                    "CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID",
                    format!("Codex model {slug} appears more than once in the base catalog"),
                ));
            }
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "the base Codex model catalog contains duplicate slugs",
            ));
        }
        slug_indices.insert(slug.to_string(), index);
        if template.is_none()
            && model_object.get("visibility").and_then(Value::as_str) == Some("list")
        {
            template = Some(model_object.clone());
        }
    }
    let template = if profiles.is_empty() {
        None
    } else {
        Some(template.ok_or_else(|| {
            AppError::new(
                "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID",
                "the base Codex model catalog has no visible template model",
            )
        })?)
    };

    let mut context_updates = Vec::with_capacity(enabled_targets.len());
    for rule in policy.enabled_rules() {
        let index = slug_indices.get(&rule.model_id).copied().ok_or_else(|| {
            AppError::new(
                "CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING",
                format!(
                    "Codex model {} is missing from the base catalog",
                    rule.model_id
                ),
            )
        })?;
        let model = models[index]
            .as_object()
            .expect("base model objects were validated");
        if model
            .get("context_window")
            .and_then(Value::as_u64)
            .is_none()
            || model
                .get("max_context_window")
                .and_then(Value::as_u64)
                .is_none()
        {
            return Err(AppError::new(
                "CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID",
                format!(
                    "Codex model {} has invalid context window fields",
                    rule.model_id
                ),
            ));
        }
        context_updates.push((index, rule.context_window));
    }
    for (index, context_window) in context_updates {
        let model = models[index]
            .as_object_mut()
            .expect("base model objects were validated");
        model.insert("context_window".to_string(), json!(context_window));
        model.insert("max_context_window".to_string(), json!(context_window));
    }

    for (index, profile) in profiles.iter().enumerate() {
        let alias = profile.alias();
        if !slugs.insert(alias.clone()) {
            return Err(AppError::new(
                "CODEX_MANAGED_MODEL_ALIAS_CONFLICT",
                format!("the base Codex model catalog already contains {alias}"),
            ));
        }
        models.push(Value::Object(build_managed_model(
            template.as_ref().expect("profiles require a template"),
            profile,
            index,
        )));
    }

    let base_catalog_sha256 = sha256_hex(base_bytes);
    let aliases = profiles
        .iter()
        .map(ManagedCatalogProfile::alias)
        .collect::<Vec<_>>();
    let projection_sha256 = projection_sha256(
        profile_set_sha256,
        base_source_fingerprint,
        policy_projection,
        original_catalog_path,
        codex_home_key,
    )?;
    let original_catalog_path = catalog_path_string(original_catalog_path)?;
    let mut payload_root = root.clone();
    payload_root
        .as_object_mut()
        .expect("validated catalog object")
        .remove(OWNER_METADATA_KEY);
    let payload_sha256 = sha256_hex(
        &serde_json::to_vec(&json!({
            "catalog": payload_root,
            "profile_set_sha256": profile_set_sha256,
            "base_catalog_sha256": base_catalog_sha256,
            "base_source_fingerprint": base_source_fingerprint,
            "managed_aliases": aliases,
            "projection_sha256": projection_sha256,
            "projection_algorithm_version": MODEL_CONTEXT_PROJECTION_VERSION,
            "model_context_rule_set_sha256": policy_projection.rule_set_sha256,
            "model_context_enabled_rules": policy_projection.enabled_rules,
            "model_context_enabled_rules_sha256": policy_projection.enabled_rules_sha256,
            "codex_home_key": codex_home_key,
            "original_catalog_path": original_catalog_path,
        }))
        .map_err(|_| {
            AppError::new(
                "SYSTEM_ERROR",
                "failed to hash the managed Codex model catalog",
            )
        })?,
    );
    root.as_object_mut()
        .expect("validated catalog object")
        .insert(
            OWNER_METADATA_KEY.to_string(),
            json!({
                "schema_version": OWNER_SCHEMA_VERSION,
                "managed_by": MANAGED_BY,
                "payload_sha256": payload_sha256,
                "profile_set_sha256": profile_set_sha256,
                "base_catalog_sha256": base_catalog_sha256,
                "base_source_fingerprint": base_source_fingerprint,
                "managed_aliases": aliases,
                "projection_sha256": projection_sha256,
                "projection_algorithm_version": MODEL_CONTEXT_PROJECTION_VERSION,
                "model_context_rule_set_sha256": policy_projection.rule_set_sha256,
                "model_context_enabled_rules": policy_projection.enabled_rules,
                "model_context_enabled_rules_sha256": policy_projection.enabled_rules_sha256,
                "codex_home_key": codex_home_key,
                "original_catalog_path": original_catalog_path,
            }),
        );
    let mut output = serde_json::to_vec_pretty(&root).map_err(|_| {
        AppError::new(
            "SYSTEM_ERROR",
            "failed to serialize the managed Codex model catalog",
        )
    })?;
    output.push(b'\n');
    if output.len() > GENERATED_CATALOG_MAX_BYTES {
        return Err(AppError::new(
            "CODEX_MANAGED_MODEL_CATALOG_LIMIT",
            "the generated Codex model catalog exceeds the size limit",
        ));
    }
    Ok(output)
}

fn build_managed_model(
    template: &Map<String, Value>,
    profile: &ManagedCatalogProfile,
    index: usize,
) -> Map<String, Value> {
    let alias = profile.alias();
    let mut model = template.clone();
    model.insert("slug".to_string(), json!(alias));
    model.insert(
        "display_name".to_string(),
        json!(format!("AIO / {}", profile.profile_name_key)),
    );
    model.insert(
        "description".to_string(),
        json!(bounded_description(profile)),
    );
    let reasoning_levels = profile
        .capabilities
        .supported_reasoning_efforts
        .iter()
        .map(|effort| {
            json!({
                "effort": effort.as_str(),
                "description": reasoning_effort_description(*effort),
            })
        })
        .collect::<Vec<_>>();
    model.insert(
        "default_reasoning_level".to_string(),
        profile
            .capabilities
            .default_reasoning_effort
            .map(|effort| json!(effort.as_str()))
            .unwrap_or(Value::Null),
    );
    model.insert(
        "supported_reasoning_levels".to_string(),
        json!(reasoning_levels),
    );
    model.insert("visibility".to_string(), json!("list"));
    model.insert("supported_in_api".to_string(), json!(true));
    model.insert(
        "priority".to_string(),
        json!(10_000_i64.saturating_add(index as i64)),
    );
    model.insert("additional_speed_tiers".to_string(), json!([]));
    model.insert("service_tiers".to_string(), json!([]));
    model.insert("default_service_tier".to_string(), Value::Null);
    model.insert("availability_nux".to_string(), Value::Null);
    model.insert("upgrade".to_string(), Value::Null);
    model.insert("model_messages".to_string(), Value::Null);
    model.insert(
        "include_skills_usage_instructions".to_string(),
        json!(false),
    );
    model.insert(
        "supports_reasoning_summaries".to_string(),
        json!(!profile.capabilities.supported_reasoning_efforts.is_empty()),
    );
    model.insert("default_reasoning_summary".to_string(), json!("none"));
    model.insert("support_verbosity".to_string(), json!(false));
    model.insert("default_verbosity".to_string(), Value::Null);
    model.insert("apply_patch_tool_type".to_string(), Value::Null);
    model.insert("web_search_tool_type".to_string(), json!("text"));
    model.insert("supports_parallel_tool_calls".to_string(), json!(false));
    model.insert("supports_image_detail_original".to_string(), json!(false));
    let context_window = profile
        .capabilities
        .context_window
        .map(Value::from)
        .unwrap_or(Value::Null);
    model.insert("context_window".to_string(), context_window.clone());
    model.insert("max_context_window".to_string(), context_window);
    model.insert("auto_compact_token_limit".to_string(), Value::Null);
    model.insert("comp_hash".to_string(), Value::Null);
    model.insert("effective_context_window_percent".to_string(), json!(95));
    model.insert("experimental_supported_tools".to_string(), json!([]));
    model.insert("input_modalities".to_string(), json!(["text"]));
    model.insert("supports_search_tool".to_string(), json!(false));
    model.insert("use_responses_lite".to_string(), json!(false));
    model.insert("auto_review_model_override".to_string(), Value::Null);
    model.insert("tool_mode".to_string(), Value::Null);
    model.insert("multi_agent_version".to_string(), Value::Null);
    model
}

fn reasoning_effort_description(
    effort: crate::provider_models::ProviderModelReasoningEffort,
) -> &'static str {
    use crate::provider_models::ProviderModelReasoningEffort as Effort;
    match effort {
        Effort::None => "No additional reasoning",
        Effort::Minimal => "Minimal reasoning",
        Effort::Low => "Light reasoning",
        Effort::Medium => "Balanced reasoning",
        Effort::High => "Deep reasoning",
        Effort::XHigh => "Very deep reasoning",
        Effort::Max => "Maximum reasoning",
        Effort::Ultra => "Ultra reasoning",
    }
}

fn bounded_description(profile: &ManagedCatalogProfile) -> String {
    let raw = format!(
        "AIO managed route · {} · {}",
        profile.provider_name, profile.remote_model_id
    );
    raw.chars().take(512).collect()
}

fn validate_owned_catalog(bytes: &[u8]) -> AppResult<OwnedCatalogMetadata> {
    let root: Value = serde_json::from_slice(bytes).map_err(|_| modified_catalog_error())?;
    let object = root.as_object().ok_or_else(modified_catalog_error)?;
    let metadata = object
        .get(OWNER_METADATA_KEY)
        .and_then(Value::as_object)
        .ok_or_else(modified_catalog_error)?;
    let schema_version = metadata
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(modified_catalog_error)?;
    if !matches!(
        schema_version,
        LEGACY_OWNER_SCHEMA_VERSION | LEGACY_POLICY_OWNER_SCHEMA_VERSION | OWNER_SCHEMA_VERSION
    ) || metadata.get("managed_by").and_then(Value::as_str) != Some(MANAGED_BY)
    {
        return Err(modified_catalog_error());
    }
    let payload_sha256 = required_metadata_string(metadata, "payload_sha256")?;
    let profile_set_sha256 = required_metadata_string(metadata, "profile_set_sha256")?;
    let base_catalog_sha256 = required_metadata_string(metadata, "base_catalog_sha256")?;
    let base_source_fingerprint = required_metadata_string(metadata, "base_source_fingerprint")?;
    let aliases = metadata
        .get("managed_aliases")
        .and_then(Value::as_array)
        .ok_or_else(modified_catalog_error)?;
    if aliases.len() > MAX_MANAGED_PROFILE_COUNT
        || aliases.iter().any(|alias| {
            alias
                .as_str()
                .is_none_or(|value| value.is_empty() || value.len() > 320)
        })
    {
        return Err(modified_catalog_error());
    }

    let mut payload_root = root.clone();
    payload_root
        .as_object_mut()
        .ok_or_else(modified_catalog_error)?
        .remove(OWNER_METADATA_KEY);

    if schema_version == LEGACY_OWNER_SCHEMA_VERSION {
        const LEGACY_V1_METADATA_KEYS: [&str; 7] = [
            "schema_version",
            "managed_by",
            "payload_sha256",
            "profile_set_sha256",
            "base_catalog_sha256",
            "base_source_fingerprint",
            "managed_aliases",
        ];
        validate_exact_metadata_keys(metadata, &LEGACY_V1_METADATA_KEYS)?;
        let expected = sha256_hex(
            &serde_json::to_vec(&json!({
                "catalog": payload_root,
                "profile_set_sha256": profile_set_sha256,
                "base_catalog_sha256": base_catalog_sha256,
                "base_source_fingerprint": base_source_fingerprint,
                "managed_aliases": aliases,
            }))
            .map_err(|_| modified_catalog_error())?,
        );
        if payload_sha256 != expected {
            return Err(modified_catalog_error());
        }
        return Ok(OwnedCatalogMetadata {
            schema_version,
            profile_set_sha256: profile_set_sha256.to_string(),
            base_source_fingerprint: base_source_fingerprint.to_string(),
            projection_sha256: None,
            model_context_rule_set_sha256: None,
            model_context_enabled_rules: Vec::new(),
            model_context_enabled_rules_sha256: None,
            codex_home_key: None,
            original_catalog_path: None,
        });
    }

    if schema_version == LEGACY_POLICY_OWNER_SCHEMA_VERSION {
        const LEGACY_V2_METADATA_KEYS: [&str; 13] = [
            "schema_version",
            "managed_by",
            "payload_sha256",
            "profile_set_sha256",
            "base_catalog_sha256",
            "base_source_fingerprint",
            "managed_aliases",
            "projection_sha256",
            "gpt56_372k_policy_version",
            "gpt56_372k_context_enabled",
            "gpt56_372k_context_tokens",
            "gpt56_372k_model_slugs",
            "original_catalog_path",
        ];
        validate_exact_metadata_keys(metadata, &LEGACY_V2_METADATA_KEYS)?;
        return validate_legacy_v2_owned_catalog(
            metadata,
            payload_root,
            payload_sha256,
            profile_set_sha256,
            base_catalog_sha256,
            base_source_fingerprint,
            aliases,
        );
    }

    const CURRENT_METADATA_KEYS: [&str; 14] = [
        "schema_version",
        "managed_by",
        "payload_sha256",
        "profile_set_sha256",
        "base_catalog_sha256",
        "base_source_fingerprint",
        "managed_aliases",
        "projection_sha256",
        "projection_algorithm_version",
        "model_context_rule_set_sha256",
        "model_context_enabled_rules",
        "model_context_enabled_rules_sha256",
        "codex_home_key",
        "original_catalog_path",
    ];
    if metadata.len() != CURRENT_METADATA_KEYS.len()
        || metadata
            .keys()
            .any(|key| !CURRENT_METADATA_KEYS.contains(&key.as_str()))
        || metadata
            .get("projection_algorithm_version")
            .and_then(Value::as_u64)
            != Some(MODEL_CONTEXT_PROJECTION_VERSION)
    {
        return Err(modified_catalog_error());
    }

    let projection_sha256_value = required_metadata_string(metadata, "projection_sha256")?;
    let rule_set_sha256 = required_metadata_string(metadata, "model_context_rule_set_sha256")?;
    let enabled_rules_sha256 =
        required_metadata_string(metadata, "model_context_enabled_rules_sha256")?;
    let enabled_rules = parse_owned_enabled_rules(metadata)?;
    let expected_enabled_rules_sha256 =
        sha256_hex(&serde_json::to_vec(&enabled_rules).map_err(|_| modified_catalog_error())?);
    if enabled_rules_sha256 != expected_enabled_rules_sha256 {
        return Err(modified_catalog_error());
    }
    let codex_home_key = metadata
        .get("codex_home_key")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 4_096 && !value.chars().any(char::is_control)
        })
        .ok_or_else(modified_catalog_error)?;
    #[cfg(windows)]
    if codex_home_key.contains('\\') || codex_home_key.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(modified_catalog_error());
    }
    let original_catalog_path = metadata_original_catalog_path(metadata)?;
    let policy_projection = ModelContextPolicyProjection {
        rule_set_sha256: rule_set_sha256.to_string(),
        enabled_rules: enabled_rules.clone(),
        enabled_rules_sha256: enabled_rules_sha256.to_string(),
    };
    let expected_projection_sha256 = projection_sha256(
        profile_set_sha256,
        base_source_fingerprint,
        &policy_projection,
        original_catalog_path.as_deref(),
        codex_home_key,
    )
    .map_err(|_| modified_catalog_error())?;
    if projection_sha256_value != expected_projection_sha256 {
        return Err(modified_catalog_error());
    }
    let expected_payload_sha256 = sha256_hex(
        &serde_json::to_vec(&json!({
            "catalog": payload_root,
            "profile_set_sha256": profile_set_sha256,
            "base_catalog_sha256": base_catalog_sha256,
            "base_source_fingerprint": base_source_fingerprint,
            "managed_aliases": aliases,
            "projection_sha256": projection_sha256_value,
            "projection_algorithm_version": MODEL_CONTEXT_PROJECTION_VERSION,
            "model_context_rule_set_sha256": rule_set_sha256,
            "model_context_enabled_rules": enabled_rules,
            "model_context_enabled_rules_sha256": enabled_rules_sha256,
            "codex_home_key": codex_home_key,
            "original_catalog_path": original_catalog_path.as_ref().and_then(|path| path.to_str()),
        }))
        .map_err(|_| modified_catalog_error())?,
    );
    if payload_sha256 != expected_payload_sha256 {
        return Err(modified_catalog_error());
    }
    Ok(OwnedCatalogMetadata {
        schema_version,
        profile_set_sha256: profile_set_sha256.to_string(),
        base_source_fingerprint: base_source_fingerprint.to_string(),
        projection_sha256: Some(projection_sha256_value.to_string()),
        model_context_rule_set_sha256: Some(rule_set_sha256.to_string()),
        model_context_enabled_rules: policy_projection.enabled_rules,
        model_context_enabled_rules_sha256: Some(enabled_rules_sha256.to_string()),
        codex_home_key: Some(codex_home_key.to_string()),
        original_catalog_path,
    })
}

fn validate_exact_metadata_keys(metadata: &Map<String, Value>, allowed: &[&str]) -> AppResult<()> {
    if metadata.len() != allowed.len()
        || metadata.keys().any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(modified_catalog_error());
    }
    Ok(())
}

fn validate_legacy_v2_owned_catalog(
    metadata: &Map<String, Value>,
    payload_root: Value,
    payload_sha256: &str,
    profile_set_sha256: &str,
    base_catalog_sha256: &str,
    base_source_fingerprint: &str,
    aliases: &[Value],
) -> AppResult<OwnedCatalogMetadata> {
    let projection_sha256_value = required_metadata_string(metadata, "projection_sha256")?;
    if metadata
        .get("gpt56_372k_policy_version")
        .and_then(Value::as_u64)
        != Some(LEGACY_GPT56_372K_POLICY_VERSION)
        || metadata
            .get("gpt56_372k_context_tokens")
            .and_then(Value::as_u64)
            != Some(LEGACY_GPT56_372K_CONTEXT_TOKENS)
        || metadata.get("gpt56_372k_model_slugs") != Some(&json!(LEGACY_GPT56_372K_MODEL_SLUGS))
    {
        return Err(modified_catalog_error());
    }
    let legacy_enabled = metadata
        .get("gpt56_372k_context_enabled")
        .and_then(Value::as_bool)
        .ok_or_else(modified_catalog_error)?;
    let original_catalog_path = metadata_original_catalog_path(metadata)?;
    let expected_projection_sha256 = legacy_v2_projection_sha256(
        profile_set_sha256,
        base_source_fingerprint,
        legacy_enabled,
        original_catalog_path.as_deref(),
    )
    .map_err(|_| modified_catalog_error())?;
    if projection_sha256_value != expected_projection_sha256 {
        return Err(modified_catalog_error());
    }
    let expected_payload_sha256 = sha256_hex(
        &serde_json::to_vec(&json!({
            "catalog": payload_root,
            "profile_set_sha256": profile_set_sha256,
            "base_catalog_sha256": base_catalog_sha256,
            "base_source_fingerprint": base_source_fingerprint,
            "managed_aliases": aliases,
            "projection_sha256": projection_sha256_value,
            "gpt56_372k_policy_version": LEGACY_GPT56_372K_POLICY_VERSION,
            "gpt56_372k_context_enabled": legacy_enabled,
            "gpt56_372k_context_tokens": LEGACY_GPT56_372K_CONTEXT_TOKENS,
            "gpt56_372k_model_slugs": LEGACY_GPT56_372K_MODEL_SLUGS,
            "original_catalog_path": original_catalog_path.as_ref().and_then(|path| path.to_str()),
        }))
        .map_err(|_| modified_catalog_error())?,
    );
    if payload_sha256 != expected_payload_sha256 {
        return Err(modified_catalog_error());
    }
    Ok(OwnedCatalogMetadata {
        schema_version: LEGACY_POLICY_OWNER_SCHEMA_VERSION,
        profile_set_sha256: profile_set_sha256.to_string(),
        base_source_fingerprint: base_source_fingerprint.to_string(),
        projection_sha256: Some(projection_sha256_value.to_string()),
        model_context_rule_set_sha256: None,
        model_context_enabled_rules: Vec::new(),
        model_context_enabled_rules_sha256: None,
        codex_home_key: None,
        original_catalog_path,
    })
}

fn parse_owned_enabled_rules(
    metadata: &Map<String, Value>,
) -> AppResult<Vec<EnabledModelContextRule>> {
    let rules = serde_json::from_value::<Vec<EnabledModelContextRule>>(
        metadata
            .get("model_context_enabled_rules")
            .cloned()
            .ok_or_else(modified_catalog_error)?,
    )
    .map_err(|_| modified_catalog_error())?;
    let mut canonical = rules
        .iter()
        .map(|rule| crate::settings::CodexModelContextRule {
            model_id: rule.model_id.clone(),
            context_window: rule.context_window,
            enabled: true,
        })
        .collect::<Vec<_>>();
    let changed = crate::settings::normalize_codex_model_context_rules_for_write(&mut canonical)
        .map_err(|_| modified_catalog_error())?;
    if changed
        || canonical.len() != rules.len()
        || canonical.iter().zip(&rules).any(|(canonical, rule)| {
            canonical.model_id != rule.model_id
                || canonical.context_window != rule.context_window
                || !canonical.enabled
        })
    {
        return Err(modified_catalog_error());
    }
    Ok(rules)
}

fn metadata_original_catalog_path(metadata: &Map<String, Value>) -> AppResult<Option<PathBuf>> {
    match metadata.get("original_catalog_path") {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let path = PathBuf::from(value);
            if value.is_empty() || !path.is_absolute() {
                return Err(modified_catalog_error());
            }
            Ok(Some(path))
        }
        _ => Err(modified_catalog_error()),
    }
}

fn required_metadata_string<'a>(metadata: &'a Map<String, Value>, key: &str) -> AppResult<&'a str> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
        .ok_or_else(modified_catalog_error)
}

fn modified_catalog_error() -> AppError {
    AppError::new(
        "CODEX_MANAGED_MODEL_CATALOG_MODIFIED",
        "the AIO-managed Codex model catalog was modified externally",
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};

    const TEST_CODEX_HOME_KEY: &str = "/test/codex-home";

    struct EnvRestore {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            crate::settings::clear_cache();
            match self.previous.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn context_rule(
        model_id: &str,
        context_window: i64,
        enabled: bool,
    ) -> crate::settings::CodexModelContextRule {
        crate::settings::CodexModelContextRule {
            model_id: model_id.to_string(),
            context_window,
            enabled,
        }
    }

    fn policy(rules: Vec<crate::settings::CodexModelContextRule>) -> ManagedCatalogPolicy {
        ManagedCatalogPolicy::from_rules(rules).expect("valid test policy")
    }

    fn empty_policy() -> ManagedCatalogPolicy {
        ManagedCatalogPolicy::default()
    }

    fn generate_test_catalog(
        base_bytes: &[u8],
        profiles: &[ManagedCatalogProfile],
        profile_set_sha256: &str,
        base_source_fingerprint: &str,
        policy: &ManagedCatalogPolicy,
        original_catalog_path: Option<&Path>,
    ) -> AppResult<Vec<u8>> {
        let policy_projection = model_context_policy_projection(policy)?;
        generate_catalog(
            base_bytes,
            profiles,
            &CatalogGenerationContext {
                profile_set_sha256,
                base_source_fingerprint,
                policy,
                policy_projection: &policy_projection,
                original_catalog_path,
                codex_home_key: TEST_CODEX_HOME_KEY,
            },
        )
    }

    fn base_catalog() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "future_top_level": {"kept": true},
            "models": [{
                "slug": "gpt-base",
                "display_name": "GPT Base",
                "description": "base",
                "default_reasoning_level": "high",
                "supported_reasoning_levels": [{"effort": "high", "description": "deep"}],
                "shell_type": "shell_command",
                "visibility": "list",
                "supported_in_api": true,
                "priority": 1,
                "additional_speed_tiers": ["fast"],
                "service_tiers": [{"id": "priority", "name": "Fast", "description": "fast"}],
                "availability_nux": {"message": "new"},
                "upgrade": {"model": "next", "migration_markdown": "move"},
                "base_instructions": "base instructions",
                "model_messages": {"instructions_template": "large"},
                "supports_reasoning_summaries": true,
                "default_reasoning_summary": "auto",
                "support_verbosity": true,
                "default_verbosity": "high",
                "apply_patch_tool_type": "freeform",
                "web_search_tool_type": "text_and_image",
                "truncation_policy": {"mode": "tokens", "limit": 10000},
                "supports_parallel_tool_calls": true,
                "context_window": 272000,
                "max_context_window": 272000,
                "comp_hash": "hash",
                "effective_context_window_percent": 95,
                "experimental_supported_tools": ["future"],
                "input_modalities": ["text", "image"],
                "supports_search_tool": true,
                "use_responses_lite": true,
                "tool_mode": "code_mode_only",
                "multi_agent_version": "v2",
                "future_required_field": {"kept": true}
            }]
        }))
        .expect("base catalog")
    }

    fn rule_base_catalog() -> Vec<u8> {
        let target_models = [
            ("model-alpha", 120_000_u64),
            ("Model-Beta", 272_000_u64),
            ("model-gamma", 640_000_u64),
        ];
        let mut models = target_models
            .iter()
            .enumerate()
            .map(|(index, (slug, context_window))| {
                json!({
                    "slug": slug,
                    "display_name": slug,
                    "description": "base model",
                    "visibility": if index == 0 { "list" } else { "hide" },
                    "context_window": context_window,
                    "max_context_window": context_window,
                    "auto_compact_token_limit": null,
                    "effective_context_window_percent": 95,
                    "future_model_field": {"kept": slug},
                })
            })
            .collect::<Vec<_>>();
        models.extend([
            json!({
                "slug": "model-untouched",
                "visibility": "hide",
                "context_window": 280000,
                "max_context_window": 280000,
                "future_model_field": {"kept": true},
            }),
            json!({
                "slug": "gpt-base",
                "visibility": "hide",
                "context_window": 272000,
                "max_context_window": 272000,
            }),
            json!({
                "slug": "aio/existing",
                "visibility": "hide",
                "context_window": 512000,
                "max_context_window": 512000,
                "aio_capability": {"kept": true},
            }),
        ]);
        serde_json::to_vec(&json!({
            "future_top_level": {"kept": true},
            "models": models,
        }))
        .expect("rule base catalog")
    }

    fn model_by_slug<'a>(root: &'a Value, slug: &str) -> &'a Value {
        root["models"]
            .as_array()
            .expect("models")
            .iter()
            .find(|model| model["slug"].as_str() == Some(slug))
            .expect("model slug")
    }

    fn config_with_catalog(path: Option<&Path>) -> Vec<u8> {
        let mut document = toml_edit::DocumentMut::new();
        document["model"] = toml_edit::value("gpt-5.6-sol");
        if let Some(path) = path {
            document["model_catalog_json"] = toml_edit::value(path.to_str().expect("UTF-8 path"));
        }
        document.to_string().into_bytes()
    }

    fn profile() -> ManagedCatalogProfile {
        ManagedCatalogProfile::new(
            "grok",
            "11111111-1111-4111-8111-111111111111",
            "xAI",
            "grok-4.5",
            crate::provider_models::ProviderModelCapabilities {
                supported_reasoning_efforts: vec![
                    crate::provider_models::ProviderModelReasoningEffort::Low,
                    crate::provider_models::ProviderModelReasoningEffort::Medium,
                    crate::provider_models::ProviderModelReasoningEffort::High,
                ],
                default_reasoning_effort: Some(
                    crate::provider_models::ProviderModelReasoningEffort::Medium,
                ),
                context_window: Some(128_000),
            },
        )
        .expect("profile")
    }

    fn legacy_v1_catalog() -> Vec<u8> {
        let current = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            None,
        )
        .expect("generate current fixture source");
        let mut root: Value = serde_json::from_slice(&current).expect("json");
        let metadata = root[OWNER_METADATA_KEY].clone();
        root.as_object_mut()
            .expect("catalog object")
            .remove(OWNER_METADATA_KEY);
        let payload_sha256 = sha256_hex(
            &serde_json::to_vec(&json!({
                "catalog": root,
                "profile_set_sha256": metadata["profile_set_sha256"],
                "base_catalog_sha256": metadata["base_catalog_sha256"],
                "base_source_fingerprint": metadata["base_source_fingerprint"],
                "managed_aliases": metadata["managed_aliases"],
            }))
            .expect("legacy payload"),
        );
        root.as_object_mut().expect("catalog object").insert(
            OWNER_METADATA_KEY.to_string(),
            json!({
                "schema_version": LEGACY_OWNER_SCHEMA_VERSION,
                "managed_by": MANAGED_BY,
                "payload_sha256": payload_sha256,
                "profile_set_sha256": metadata["profile_set_sha256"],
                "base_catalog_sha256": metadata["base_catalog_sha256"],
                "base_source_fingerprint": metadata["base_source_fingerprint"],
                "managed_aliases": metadata["managed_aliases"],
            }),
        );
        let mut output = serde_json::to_vec_pretty(&root).expect("legacy catalog");
        output.push(b'\n');
        output
    }

    fn legacy_v2_catalog(original_catalog_path: Option<&Path>) -> Vec<u8> {
        let current = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            original_catalog_path,
        )
        .expect("generate current fixture source");
        let mut root: Value = serde_json::from_slice(&current).expect("json");
        let metadata = root[OWNER_METADATA_KEY].clone();
        root.as_object_mut()
            .expect("catalog object")
            .remove(OWNER_METADATA_KEY);
        let profile_set_sha256 = metadata["profile_set_sha256"]
            .as_str()
            .expect("profile hash");
        let base_source_fingerprint = metadata["base_source_fingerprint"]
            .as_str()
            .expect("source fingerprint");
        let original_catalog_path_string =
            catalog_path_string(original_catalog_path).expect("catalog path");
        let projection_sha256 = legacy_v2_projection_sha256(
            profile_set_sha256,
            base_source_fingerprint,
            false,
            original_catalog_path,
        )
        .expect("legacy projection hash");
        let payload_sha256 = sha256_hex(
            &serde_json::to_vec(&json!({
                "catalog": root,
                "profile_set_sha256": profile_set_sha256,
                "base_catalog_sha256": metadata["base_catalog_sha256"],
                "base_source_fingerprint": base_source_fingerprint,
                "managed_aliases": metadata["managed_aliases"],
                "projection_sha256": projection_sha256,
                "gpt56_372k_policy_version": LEGACY_GPT56_372K_POLICY_VERSION,
                "gpt56_372k_context_enabled": false,
                "gpt56_372k_context_tokens": LEGACY_GPT56_372K_CONTEXT_TOKENS,
                "gpt56_372k_model_slugs": LEGACY_GPT56_372K_MODEL_SLUGS,
                "original_catalog_path": original_catalog_path_string,
            }))
            .expect("legacy payload"),
        );
        root.as_object_mut().expect("catalog object").insert(
            OWNER_METADATA_KEY.to_string(),
            json!({
                "schema_version": LEGACY_POLICY_OWNER_SCHEMA_VERSION,
                "managed_by": MANAGED_BY,
                "payload_sha256": payload_sha256,
                "profile_set_sha256": profile_set_sha256,
                "base_catalog_sha256": metadata["base_catalog_sha256"],
                "base_source_fingerprint": base_source_fingerprint,
                "managed_aliases": metadata["managed_aliases"],
                "projection_sha256": projection_sha256,
                "gpt56_372k_policy_version": LEGACY_GPT56_372K_POLICY_VERSION,
                "gpt56_372k_context_enabled": false,
                "gpt56_372k_context_tokens": LEGACY_GPT56_372K_CONTEXT_TOKENS,
                "gpt56_372k_model_slugs": LEGACY_GPT56_372K_MODEL_SLUGS,
                "original_catalog_path": original_catalog_path_string,
            }),
        );
        let mut output = serde_json::to_vec_pretty(&root).expect("legacy v2 catalog");
        output.push(b'\n');
        output
    }

    #[test]
    fn generated_catalog_preserves_base_and_sets_managed_reasoning_capabilities() {
        let output = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            None,
        )
        .expect("generate");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(root["future_top_level"]["kept"], json!(true));
        assert_eq!(
            root["models"][0]["future_required_field"]["kept"],
            json!(true)
        );
        let managed = &root["models"][1];
        assert_eq!(managed["slug"], json!("aio/grok"));
        assert_eq!(managed["visibility"], json!("list"));
        assert_eq!(
            managed["supported_reasoning_levels"],
            json!([
                {
                    "effort": "low",
                    "description": "Light reasoning"
                },
                {
                    "effort": "medium",
                    "description": "Balanced reasoning"
                },
                {
                    "effort": "high",
                    "description": "Deep reasoning"
                }
            ])
        );
        assert_eq!(managed["default_reasoning_level"], json!("medium"));
        assert_eq!(managed["supports_reasoning_summaries"], json!(true));
        assert_eq!(managed["context_window"], json!(128_000));
        assert_eq!(managed["max_context_window"], json!(128_000));
        assert_eq!(managed["auto_compact_token_limit"], Value::Null);
        assert_eq!(managed["additional_speed_tiers"], json!([]));
        assert_eq!(managed["service_tiers"], json!([]));
        assert_eq!(managed["supports_parallel_tool_calls"], json!(false));
        assert_eq!(managed["supports_search_tool"], json!(false));
        assert_eq!(managed["input_modalities"], json!(["text"]));
        assert_eq!(managed["future_required_field"]["kept"], json!(true));
        validate_owned_catalog(&output).expect("owned");
    }

    #[test]
    fn model_context_rules_rewrite_exact_targets_and_preserve_unowned_fields() {
        let original_catalog = std::env::temp_dir().join("codex-user-models.json");
        let rules = policy(vec![
            context_rule("model-gamma", 640_000, true),
            context_rule("missing-disabled", 200_000, false),
            context_rule("model-alpha", 80_000, true),
            context_rule("Model-Beta", 372_000, true),
        ]);
        let output = generate_test_catalog(
            &rule_base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &rules,
            Some(&original_catalog),
        )
        .expect("generate rule catalog");
        let root: Value = serde_json::from_slice(&output).expect("json");

        for (slug, expected) in [
            ("model-alpha", 80_000),
            ("Model-Beta", 372_000),
            ("model-gamma", 640_000),
        ] {
            let model = model_by_slug(&root, slug);
            assert_eq!(model["context_window"], json!(expected));
            assert_eq!(model["max_context_window"], json!(expected));
            assert_eq!(model["effective_context_window_percent"], json!(95));
            assert_eq!(model["auto_compact_token_limit"], Value::Null);
            assert_eq!(model["future_model_field"]["kept"], json!(slug));
        }
        assert_eq!(
            model_by_slug(&root, "model-untouched")["context_window"],
            json!(280_000)
        );
        assert_eq!(
            model_by_slug(&root, "aio/existing")["context_window"],
            json!(512_000)
        );
        assert_eq!(
            model_by_slug(&root, "aio/existing")["aio_capability"]["kept"],
            json!(true)
        );
        assert_eq!(
            model_by_slug(&root, "aio/grok")["context_window"],
            json!(128_000)
        );
        assert_eq!(root["future_top_level"]["kept"], json!(true));

        let metadata = &root[OWNER_METADATA_KEY];
        assert_eq!(metadata["schema_version"], json!(OWNER_SCHEMA_VERSION));
        assert_eq!(
            metadata["projection_algorithm_version"],
            json!(MODEL_CONTEXT_PROJECTION_VERSION)
        );
        assert_eq!(
            metadata["model_context_enabled_rules"],
            json!([
                {"model_id": "Model-Beta", "context_window": 372000},
                {"model_id": "model-alpha", "context_window": 80000},
                {"model_id": "model-gamma", "context_window": 640000},
            ])
        );
        assert_eq!(metadata["codex_home_key"], json!(TEST_CODEX_HOME_KEY));
        assert!(metadata.get("gpt56_372k_context_enabled").is_none());
        assert_eq!(
            metadata["original_catalog_path"],
            json!(original_catalog.to_str().expect("UTF-8 path"))
        );
        validate_owned_catalog(&output).expect("owned catalog");
    }

    #[test]
    fn model_context_rule_bounds_are_projected_exactly() {
        let rules = policy(vec![
            context_rule("model-alpha", 1_024, true),
            context_rule("Model-Beta", 10_000_000, true),
        ]);
        let output = generate_test_catalog(
            &rule_base_catalog(),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &rules,
            None,
        )
        .expect("generate boundary rules");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            model_by_slug(&root, "model-alpha")["context_window"],
            json!(1_024)
        );
        assert_eq!(
            model_by_slug(&root, "Model-Beta")["max_context_window"],
            json!(10_000_000)
        );
    }

    #[test]
    fn managed_policy_revalidates_static_rule_contract_and_count_limit() {
        let error = ManagedCatalogPolicy::from_rules(vec![context_rule("   ", 1_024, true)])
            .expect_err("empty model ID");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_INVALID");

        let error = ManagedCatalogPolicy::from_rules(vec![context_rule("bad\nid", 1_024, true)])
            .expect_err("control character");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_INVALID");

        let error =
            ManagedCatalogPolicy::from_rules(vec![context_rule(&"é".repeat(129), 1_024, true)])
                .expect_err("model ID over 256 UTF-8 bytes");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_INVALID");

        let error =
            ManagedCatalogPolicy::from_rules(vec![context_rule("aio/reserved", 1_024, false)])
                .expect_err("reserved target");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_INVALID");

        let error = ManagedCatalogPolicy::from_rules(vec![
            context_rule("model-alpha", 1_024, true),
            context_rule(" model-alpha ", 2_048, false),
        ])
        .expect_err("normalized duplicate");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_DUPLICATE");

        let error =
            ManagedCatalogPolicy::from_rules(vec![context_rule("model-alpha", 1_023, true)])
                .expect_err("context below lower bound");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_INVALID");

        let mut rules = (0..128)
            .rev()
            .map(|index| context_rule(&format!("model-{index:03}"), 1_024, false))
            .collect::<Vec<_>>();
        let policy = ManagedCatalogPolicy::from_rules(rules.clone()).expect("128 rules");
        assert_eq!(policy.model_context_rules.len(), 128);
        assert_eq!(policy.model_context_rules[0].model_id, "model-000");
        rules.push(context_rule("model-128", 1_024, false));
        let error = ManagedCatalogPolicy::from_rules(rules).expect_err("129 rules");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_LIMIT");
    }

    #[test]
    fn catalog_projects_the_maximum_128_enabled_rules_atomically() {
        let models = (0..128)
            .map(|index| {
                json!({
                    "slug": format!("model-{index:03}"),
                    "visibility": "hide",
                    "context_window": 272000,
                    "max_context_window": 272000,
                })
            })
            .collect::<Vec<_>>();
        let base = serde_json::to_vec(&json!({"models": models})).expect("base");
        let rules = policy(
            (0..128)
                .rev()
                .map(|index| {
                    context_rule(&format!("model-{index:03}"), 1_024 + i64::from(index), true)
                })
                .collect(),
        );
        let output = generate_test_catalog(
            &base,
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &rules,
            None,
        )
        .expect("generate maximum rule set");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            model_by_slug(&root, "model-000")["context_window"],
            json!(1_024)
        );
        assert_eq!(
            model_by_slug(&root, "model-127")["max_context_window"],
            json!(1_151)
        );
    }

    #[test]
    fn model_context_rules_fail_closed_for_missing_duplicate_or_invalid_targets() {
        let target_policy = policy(vec![context_rule("model-alpha", 372_000, true)]);
        let mut missing: Value = serde_json::from_slice(&rule_base_catalog()).expect("json");
        missing["models"]
            .as_array_mut()
            .expect("models")
            .retain(|model| model["slug"].as_str() != Some("model-alpha"));
        let error = generate_test_catalog(
            &serde_json::to_vec(&missing).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &target_policy,
            None,
        )
        .expect_err("missing target must fail");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING");

        let mut duplicate: Value = serde_json::from_slice(&rule_base_catalog()).expect("json");
        let duplicate_model = model_by_slug(&duplicate, "model-alpha").clone();
        duplicate["models"]
            .as_array_mut()
            .expect("models")
            .push(duplicate_model);
        let error = generate_test_catalog(
            &serde_json::to_vec(&duplicate).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &target_policy,
            None,
        )
        .expect_err("duplicate target must fail");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID");

        let mut invalid: Value = serde_json::from_slice(&rule_base_catalog()).expect("json");
        let invalid_model = invalid["models"]
            .as_array_mut()
            .expect("models")
            .iter_mut()
            .find(|model| model["slug"].as_str() == Some("model-alpha"))
            .expect("target");
        invalid_model["max_context_window"] = json!("272000");
        let error = generate_test_catalog(
            &serde_json::to_vec(&invalid).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &target_policy,
            None,
        )
        .expect_err("invalid target field must fail");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID");

        let case_mismatch = policy(vec![context_rule("Model-alpha", 372_000, true)]);
        let error = generate_test_catalog(
            &rule_base_catalog(),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &case_mismatch,
            None,
        )
        .expect_err("matching must remain case-sensitive");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING");
    }

    #[test]
    fn model_context_rules_keep_non_target_duplicates_as_base_catalog_errors() {
        let mut duplicate: Value = serde_json::from_slice(&rule_base_catalog()).expect("json");
        let duplicate_model = model_by_slug(&duplicate, "gpt-base").clone();
        duplicate["models"]
            .as_array_mut()
            .expect("models")
            .push(duplicate_model);

        let error = generate_test_catalog(
            &serde_json::to_vec(&duplicate).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &policy(vec![context_rule("model-alpha", 372_000, true)]),
            None,
        )
        .expect_err("non-target duplicate must fail");

        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID");
    }

    #[test]
    fn policy_only_catalog_does_not_require_a_managed_profile_template() {
        let mut base: Value = serde_json::from_slice(&rule_base_catalog()).expect("json");
        for model in base["models"].as_array_mut().expect("models") {
            model["visibility"] = json!("hide");
        }
        let output = generate_test_catalog(
            &serde_json::to_vec(&base).expect("serialize"),
            &[],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &policy(vec![context_rule("model-alpha", 372_000, true)]),
            None,
        )
        .expect("policy-only catalog");
        let root: Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            model_by_slug(&root, "model-alpha")["context_window"],
            json!(372_000)
        );
    }

    #[test]
    fn v3_projection_is_order_stable_and_detects_owner_metadata_tampering() {
        let base = rule_base_catalog();
        let profile_sha = profile_set_sha256(&[profile()]).expect("profile hash");
        let first_policy = policy(vec![
            context_rule("model-alpha", 372_000, true),
            context_rule("missing-disabled", 200_000, false),
            context_rule("Model-Beta", 500_000, true),
        ]);
        let reordered_policy = policy(vec![
            context_rule("Model-Beta", 500_000, true),
            context_rule("missing-disabled", 200_000, false),
            context_rule("model-alpha", 372_000, true),
        ]);
        assert_eq!(first_policy, reordered_policy);
        let first = generate_test_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            &first_policy,
            None,
        )
        .expect("first generation");
        let second = generate_test_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            &reordered_policy,
            None,
        )
        .expect("second generation");
        assert_eq!(first, second);

        let projection_of = |bytes: &[u8]| {
            let root: Value = serde_json::from_slice(bytes).expect("json");
            root[OWNER_METADATA_KEY]["projection_sha256"]
                .as_str()
                .expect("projection hash")
                .to_string()
        };
        let first_projection = projection_of(&first);
        let changed_disabled = generate_test_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            &policy(vec![
                context_rule("model-alpha", 372_000, true),
                context_rule("missing-disabled", 201_000, false),
                context_rule("Model-Beta", 500_000, true),
            ]),
            None,
        )
        .expect("changed disabled rule projection");
        assert_ne!(first_projection, projection_of(&changed_disabled));
        let different_base = generate_test_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "c".repeat(64).as_str(),
            &first_policy,
            None,
        )
        .expect("different base projection");
        assert_ne!(first_projection, projection_of(&different_base));
        let original_path = std::env::temp_dir().join("projection-user-catalog.json");
        let different_binding = generate_test_catalog(
            &base,
            &[profile()],
            &profile_sha,
            "b".repeat(64).as_str(),
            &first_policy,
            Some(&original_path),
        )
        .expect("different binding projection");
        assert_ne!(first_projection, projection_of(&different_binding));
        let mut changed_profile = profile();
        changed_profile.capabilities.context_window = Some(256_000);
        let changed_profile_sha = profile_set_sha256(std::slice::from_ref(&changed_profile))
            .expect("changed profile hash");
        let different_profile = generate_test_catalog(
            &base,
            &[changed_profile],
            &changed_profile_sha,
            "b".repeat(64).as_str(),
            &first_policy,
            None,
        )
        .expect("different profile projection");
        assert_ne!(first_projection, projection_of(&different_profile));

        let mut root: Value = serde_json::from_slice(&first).expect("json");
        root[OWNER_METADATA_KEY]["model_context_enabled_rules"][0]["context_window"] =
            json!(380_928);
        let tampered = serde_json::to_vec(&root).expect("serialize");
        assert_eq!(
            validate_owned_catalog(&tampered)
                .expect_err("policy metadata drift")
                .code(),
            "CODEX_MANAGED_MODEL_CATALOG_MODIFIED"
        );

        let mut root: Value = serde_json::from_slice(&first).expect("json");
        root[OWNER_METADATA_KEY]["unexpected_owner_field"] = json!(true);
        let tampered = serde_json::to_vec(&root).expect("serialize");
        assert_eq!(
            validate_owned_catalog(&tampered)
                .expect_err("unknown owner metadata must fail closed")
                .code(),
            "CODEX_MANAGED_MODEL_CATALOG_MODIFIED"
        );
    }

    #[test]
    fn legacy_owner_metadata_rejects_unhashed_unknown_fields() {
        for legacy in [legacy_v1_catalog(), legacy_v2_catalog(None)] {
            let mut root: Value = serde_json::from_slice(&legacy).expect("legacy json");
            root[OWNER_METADATA_KEY]["unhashed_external_field"] = json!(true);
            let tampered = serde_json::to_vec(&root).expect("serialize tampered legacy owner");
            assert_eq!(
                validate_owned_catalog(&tampered)
                    .expect_err("unknown legacy owner metadata must fail closed")
                    .code(),
                "CODEX_MANAGED_MODEL_CATALOG_MODIFIED"
            );
        }
    }

    #[test]
    fn generated_catalog_supports_explicit_no_reasoning_and_unknown_context() {
        let mut profile = profile();
        profile
            .set_capabilities(crate::provider_models::ProviderModelCapabilities {
                supported_reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
                context_window: None,
            })
            .expect("no reasoning capabilities");
        let output = generate_test_catalog(
            &base_catalog(),
            &[profile],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            None,
        )
        .expect("generate");
        let root: Value = serde_json::from_slice(&output).expect("json");
        let managed = &root["models"][1];
        assert_eq!(managed["supported_reasoning_levels"], json!([]));
        assert_eq!(managed["default_reasoning_level"], Value::Null);
        assert_eq!(managed["supports_reasoning_summaries"], json!(false));
        assert_eq!(managed["context_window"], Value::Null);
        assert_eq!(managed["max_context_window"], Value::Null);
    }

    #[test]
    fn ownership_hash_detects_external_model_changes() {
        let mut output = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            None,
        )
        .expect("generate");
        let mut root: Value = serde_json::from_slice(&output).expect("json");
        root["models"][1]["description"] = json!("externally changed");
        output = serde_json::to_vec(&root).expect("serialize");
        assert_eq!(
            validate_owned_catalog(&output)
                .expect_err("modified")
                .code(),
            "CODEX_MANAGED_MODEL_CATALOG_MODIFIED"
        );
    }

    #[test]
    fn user_base_catalog_guard_detects_prepare_apply_byte_drift() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("user-models.json");
        let prepared = b"first-catalog".to_vec();
        std::fs::write(&path, &prepared).expect("write prepared catalog");
        let fingerprint = user_catalog_fingerprint(&path, &prepared);

        ensure_user_base_catalog_unchanged(&path, &prepared, &fingerprint)
            .expect("unchanged source");

        let changed = b"other-catalog";
        assert_eq!(changed.len(), prepared.len());
        std::fs::write(&path, changed).expect("rewrite source with same byte length");
        let error = ensure_user_base_catalog_unchanged(&path, &prepared, &fingerprint)
            .expect_err("changed source must invalidate the prepared plan");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_BASE_CATALOG_DRIFT");
    }

    #[test]
    fn managed_catalog_path_computation_has_no_directory_side_effect() {
        let temp = tempfile::tempdir().expect("tempdir");
        let app_data = temp.path().join("missing-app-data");
        let expected = std::fs::canonicalize(temp.path())
            .expect("canonical temp directory")
            .join("missing-app-data")
            .join("cli-proxy")
            .join("codex")
            .join(GENERATED_CATALOG_FILE_NAME);

        assert_eq!(
            managed_catalog_path_from_root(&app_data).expect("managed path"),
            expected
        );
        assert!(!app_data.exists());
    }

    #[cfg(unix)]
    #[test]
    fn catalog_apply_rejects_managed_parent_symlink_drift() {
        use std::os::unix::fs::symlink;

        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("home");
        let outside = tempfile::tempdir().expect("outside");
        let _home_restore = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", home.path());
        let _dotdir_restore = EnvRestore::set(
            "AIO_CODING_HUB_DOTDIR_NAME",
            ".aio-managed-parent-drift-test",
        );
        crate::settings::clear_cache();

        let codex_home = home.path().join(".codex");
        std::fs::create_dir_all(&codex_home).expect("Codex home");
        let base_path = codex_home.join("user-models.json");
        std::fs::write(&base_path, rule_base_catalog()).expect("base catalog");
        std::fs::write(
            codex_home.join("config.toml"),
            config_with_catalog(Some(&base_path)),
        )
        .expect("Codex config");

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let plan = prepare_for_profiles_with_policy(
            &handle,
            &[],
            policy(vec![context_rule("model-alpha", 372_000, true)]),
        )
        .expect("prepare catalog plan");
        let app_data = crate::app_paths::app_data_dir(&handle).expect("app data");
        let cli_proxy = app_data.join("cli-proxy");
        assert!(!cli_proxy.exists());
        symlink(outside.path(), &cli_proxy).expect("replace managed parent with symlink");

        let error = plan
            .apply(&handle)
            .expect_err("managed parent drift must fail before writes");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_CONFIG_DRIFT");
        assert!(!outside.path().join("codex").exists());
    }

    #[test]
    fn catalog_apply_rechecks_codex_home_identity_before_writing() {
        let _env_lock = crate::test_support::test_env_lock();
        let first_home = tempfile::tempdir().expect("first home");
        let second_home = tempfile::tempdir().expect("second home");
        let _home_restore = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", first_home.path());
        let _dotdir_restore =
            EnvRestore::set("AIO_CODING_HUB_DOTDIR_NAME", ".aio-managed-home-drift-test");
        crate::settings::clear_cache();
        std::fs::create_dir_all(first_home.path().join(".codex")).expect("first Codex home");
        std::fs::write(
            first_home.path().join(".codex").join("config.toml"),
            b"model = \"gpt-base\"\n",
        )
        .expect("first config");

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let plan = prepare_for_profiles_with_policy(&handle, &[], empty_policy())
            .expect("prepare first-home plan");

        let _second_home_restore = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", second_home.path());
        crate::settings::clear_cache();
        std::fs::create_dir_all(second_home.path().join(".codex")).expect("second Codex home");
        std::fs::write(
            second_home.path().join(".codex").join("config.toml"),
            b"model = \"gpt-base\"\n",
        )
        .expect("second config");

        let error = plan
            .apply(&handle)
            .expect_err("home drift must fail before catalog writes");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT");
        assert!(!second_home
            .path()
            .join(".aio-managed-home-drift-test")
            .join("cli-proxy")
            .join("codex")
            .exists());
    }

    #[test]
    fn v3_owner_binds_to_one_normalized_codex_home() {
        let output = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            None,
        )
        .expect("generate v3 catalog");
        let metadata = validate_owned_catalog(&output).expect("metadata");
        validate_catalog_home_identity(Some(&metadata), TEST_CODEX_HOME_KEY)
            .expect("same home identity");
        let error = validate_catalog_home_identity(Some(&metadata), "/other/codex-home")
            .expect_err("different home must fail closed");
        assert_eq!(error.code(), "CODEX_MODEL_CONTEXT_RULES_HOME_DRIFT");

        let legacy = validate_owned_catalog(&legacy_v2_catalog(None)).expect("legacy v2");
        validate_catalog_home_identity(Some(&legacy), "/other/codex-home")
            .expect("legacy home ownership is classified from current live bindings");
    }

    #[test]
    fn home_identity_is_stable_when_the_home_is_created_after_prepare() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("missing").join("codex-home");
        let before = normalized_codex_home_key(&home).expect("missing-home key");

        std::fs::create_dir_all(&home).expect("create Codex home");

        assert_eq!(
            normalized_codex_home_key(&home).expect("created-home key"),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_home_identity_preserves_literal_backslashes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let literal_backslash = temp.path().join("a\\b");
        let nested = temp.path().join("a").join("b");
        std::fs::create_dir_all(&literal_backslash).expect("create literal-backslash home");
        std::fs::create_dir_all(&nested).expect("create nested home");

        assert_ne!(
            normalized_codex_home_key(&literal_backslash).expect("literal-backslash key"),
            normalized_codex_home_key(&nested).expect("nested key")
        );

        let literal_key =
            normalized_codex_home_key(&literal_backslash).expect("literal-backslash key");
        let policy = empty_policy();
        let policy_projection =
            model_context_policy_projection(&policy).expect("policy projection");
        let output = generate_catalog(
            &base_catalog(),
            &[profile()],
            &CatalogGenerationContext {
                profile_set_sha256: &"a".repeat(64),
                base_source_fingerprint: &"b".repeat(64),
                policy: &policy,
                policy_projection: &policy_projection,
                original_catalog_path: None,
                codex_home_key: &literal_key,
            },
        )
        .expect("generate catalog for literal-backslash home");
        let metadata =
            validate_owned_catalog(&output).expect("validate catalog for literal-backslash home");
        assert_eq!(
            metadata.codex_home_key.as_deref(),
            Some(literal_key.as_str())
        );
    }

    #[test]
    fn bundled_base_catalog_descriptor_tracks_launch_and_executable_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable = temp.path().join("codex-test.exe");
        std::fs::write(&executable, b"first").expect("write executable");
        let launch = crate::cli_manager::CodexLaunchSpec {
            executable: executable.clone(),
            runtime_path: OsString::from("runtime-a"),
            version: Some("codex-cli 0.147.0".to_string()),
        };
        let prepared = bundled_catalog_descriptor(&launch).expect("prepared descriptor");

        std::fs::write(&executable, b"second-longer").expect("replace executable");
        let replaced = bundled_catalog_descriptor(&launch).expect("replaced descriptor");
        assert_ne!(prepared, replaced);
        assert_ne!(
            bundled_catalog_fingerprint(&prepared),
            bundled_catalog_fingerprint(&replaced)
        );

        let mut changed_launch = launch;
        changed_launch.runtime_path = OsString::from("runtime-b");
        changed_launch.version = Some("codex-cli 0.147.1".to_string());
        let changed_launch =
            bundled_catalog_descriptor(&changed_launch).expect("launch descriptor");
        assert_ne!(replaced, changed_launch);
    }

    #[test]
    fn legacy_v1_catalog_cannot_recover_an_active_proxy_binding() {
        let legacy = legacy_v1_catalog();
        let legacy_metadata = validate_owned_catalog(&legacy).expect("valid legacy catalog");
        assert!(legacy_metadata.is_legacy_v1());
        assert!(legacy_metadata.projection_sha256.is_none());

        let temp = tempfile::tempdir().expect("tempdir");
        let generated_path = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let backup_path = temp.path().join("config.toml.backup");
        let baseline_bytes = config_with_catalog(Some(&generated_path));
        std::fs::write(&backup_path, &baseline_bytes).expect("write baseline");
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: temp.path().join("config.toml"),
            config_backup_path: Some(backup_path),
            config_bytes: Some(baseline_bytes),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };
        let error = prepare_catalog_baseline(&baseline, &generated_path, Some(&legacy_metadata))
            .expect_err("v1 has no recoverable original binding");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
    }

    #[test]
    fn direct_mode_rejects_an_orphaned_legacy_v1_catalog() {
        let metadata = validate_owned_catalog(&legacy_v1_catalog()).expect("legacy metadata");
        let generated_path = std::env::temp_dir().join(GENERATED_CATALOG_FILE_NAME);
        let error = direct_original_catalog_path(
            Some(generated_path.as_path()),
            &generated_path,
            Some(&metadata),
        )
        .expect_err("legacy direct ownership is ambiguous");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
    }

    #[test]
    fn direct_mode_ignores_an_inactive_legacy_v1_catalog() {
        let metadata = validate_owned_catalog(&legacy_v1_catalog()).expect("legacy metadata");
        let generated_path = std::env::temp_dir().join(GENERATED_CATALOG_FILE_NAME);
        let user_catalog_path = std::env::temp_dir().join("legacy-user-catalog.json");

        assert_eq!(
            direct_original_catalog_path(
                Some(user_catalog_path.as_path()),
                &generated_path,
                Some(&metadata),
            )
            .expect("inactive legacy output must not block a restored direct binding"),
            Some(user_catalog_path),
        );
        assert_eq!(
            direct_original_catalog_path(None, &generated_path, Some(&metadata))
                .expect("inactive legacy output must not block the bundled binding"),
            None,
        );
    }

    #[test]
    fn inactive_legacy_catalog_is_preserved_and_cannot_be_claimed() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        let _home_restore = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", home.path());
        let _dotdir_restore = EnvRestore::set(
            "AIO_CODING_HUB_DOTDIR_NAME",
            ".aio-managed-legacy-inactive-test",
        );
        crate::settings::clear_cache();

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let codex_home = home.path().join(".codex");
        std::fs::create_dir_all(&codex_home).expect("create Codex home");
        let config_path = codex_home.join("config.toml");
        let config_bytes = b"model = \"gpt-base\"\n".to_vec();
        std::fs::write(&config_path, &config_bytes).expect("write config");

        let generated_path = managed_catalog_path(&handle).expect("managed path");
        std::fs::create_dir_all(generated_path.parent().expect("managed parent"))
            .expect("create managed parent");
        let legacy_bytes = legacy_v2_catalog(None);
        std::fs::write(&generated_path, &legacy_bytes).expect("write inactive legacy catalog");

        let applied = prepare_for_profiles_with_policy(&handle, &[], empty_policy())
            .expect("prepare disabled policy")
            .apply(&handle)
            .expect("preserve inactive legacy catalog");
        assert_eq!(
            std::fs::read(&generated_path).expect("read preserved legacy catalog"),
            legacy_bytes
        );
        assert_eq!(
            std::fs::read(&config_path).expect("read unchanged config"),
            config_bytes
        );
        drop(applied);

        let error = prepare_for_profiles_with_policy(
            &handle,
            std::slice::from_ref(&profile()),
            empty_policy(),
        )
        .expect_err("the current home cannot claim an inactive legacy catalog");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
        assert_eq!(
            std::fs::read(&generated_path).expect("read unclaimed legacy catalog"),
            legacy_bytes
        );
        assert_eq!(
            std::fs::read(config_path).expect("read config after rejected claim"),
            config_bytes
        );
    }

    #[test]
    fn active_legacy_v2_binding_is_upgraded_to_the_current_home_owner() {
        let _env_lock = crate::test_support::test_env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        let _home_restore = EnvRestore::set("AIO_CODING_HUB_TEST_HOME", home.path());
        let _dotdir_restore = EnvRestore::set(
            "AIO_CODING_HUB_DOTDIR_NAME",
            ".aio-managed-legacy-active-test",
        );
        crate::settings::clear_cache();

        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let codex_home = home.path().join(".codex");
        std::fs::create_dir_all(&codex_home).expect("create Codex home");
        let base_path = codex_home.join("base-models.json");
        std::fs::write(&base_path, base_catalog()).expect("write base catalog");

        let generated_path = managed_catalog_path(&handle).expect("managed path");
        std::fs::create_dir_all(generated_path.parent().expect("managed parent"))
            .expect("create managed parent");
        std::fs::write(&generated_path, legacy_v2_catalog(Some(&base_path)))
            .expect("write active legacy catalog");
        let config_path = codex_home.join("config.toml");
        std::fs::write(&config_path, config_with_catalog(Some(&generated_path)))
            .expect("write active config");

        let _applied = prepare_for_profiles_with_policy(
            &handle,
            std::slice::from_ref(&profile()),
            empty_policy(),
        )
        .expect("prepare v2 upgrade")
        .apply(&handle)
        .expect("apply v2 upgrade");

        let generated = std::fs::read(&generated_path).expect("read upgraded catalog");
        let metadata = validate_owned_catalog(&generated).expect("validate v3 owner");
        let current_home_key = current_codex_home_key(&handle).expect("current home key");
        assert!(metadata.is_current());
        assert_eq!(
            metadata.codex_home_key.as_deref(),
            Some(current_home_key.as_str())
        );
        assert_eq!(
            parse_catalog_path(
                Some(&std::fs::read(config_path).expect("read active config")),
                "confirmed",
            )
            .expect("parse active config")
            .as_deref(),
            Some(generated_path.as_path())
        );
    }

    #[test]
    fn direct_mode_uses_active_v2_binding_and_ignores_inactive_legacy_metadata() {
        let original_path = std::env::temp_dir().join("original-catalog.json");
        let generated_path = std::env::temp_dir().join(GENERATED_CATALOG_FILE_NAME);
        let output = legacy_v2_catalog(Some(&original_path));
        let metadata = validate_owned_catalog(&output).expect("metadata");
        assert_eq!(metadata.schema_version, LEGACY_POLICY_OWNER_SCHEMA_VERSION);
        assert_eq!(
            direct_original_catalog_path(Some(&generated_path), &generated_path, Some(&metadata))
                .expect("active generated binding"),
            Some(original_path.clone())
        );
        assert_eq!(
            direct_original_catalog_path(Some(&original_path), &generated_path, Some(&metadata))
                .expect("known restored binding"),
            Some(original_path)
        );

        let external_path = std::env::temp_dir().join("external-catalog.json");
        assert_eq!(
            direct_original_catalog_path(Some(&external_path), &generated_path, Some(&metadata),)
                .expect("inactive legacy metadata must not own the current binding"),
            Some(external_path)
        );
    }

    #[test]
    fn explicit_direct_config_save_updates_original_binding_but_preserves_owned_binding() {
        let temp = tempfile::tempdir().expect("tempdir");
        let original_path = temp.path().join("original-catalog.json");
        let next_path = temp.path().join("next-catalog.json");
        let generated_path = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let output = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            Some(&original_path),
        )
        .expect("generate v3 catalog");
        let metadata = validate_owned_catalog(&output).expect("metadata");
        let previous = config_with_catalog(Some(&generated_path));
        let committed = config_with_catalog(Some(&next_path));

        assert_eq!(
            explicit_save_original_catalog_path(
                Some(&previous),
                &committed,
                &generated_path,
                Some(&metadata),
            )
            .expect("accept explicit user catalog"),
            Some(next_path)
        );
        assert_eq!(
            explicit_save_original_catalog_path(
                Some(&previous),
                &config_with_catalog(None),
                &generated_path,
                Some(&metadata),
            )
            .expect("accept bundled catalog intent"),
            None
        );
        assert_eq!(
            explicit_save_original_catalog_path(
                Some(&previous),
                &config_with_catalog(Some(&generated_path)),
                &generated_path,
                Some(&metadata),
            )
            .expect("preserve owned binding on unrelated save"),
            Some(original_path)
        );
    }

    #[test]
    fn explicit_direct_config_save_cannot_adopt_unowned_or_legacy_generated_catalog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let generated_path = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let ordinary = config_with_catalog(Some(&temp.path().join("user-catalog.json")));
        let committed = config_with_catalog(Some(&generated_path));
        let error =
            explicit_save_original_catalog_path(Some(&ordinary), &committed, &generated_path, None)
                .expect_err("unowned generated path");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");

        let legacy = validate_owned_catalog(&legacy_v1_catalog()).expect("legacy metadata");
        let previous = config_with_catalog(Some(&generated_path));
        let next = config_with_catalog(Some(&temp.path().join("next-catalog.json")));
        let error = explicit_save_original_catalog_path(
            Some(&previous),
            &next,
            &generated_path,
            Some(&legacy),
        )
        .expect_err("legacy direct save remains ambiguous");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
    }

    #[test]
    fn base_catalog_alias_conflicts_fail_closed() {
        let mut base: Value = serde_json::from_slice(&base_catalog()).expect("json");
        base["models"][0]["slug"] = json!("aio/grok");
        let error = generate_test_catalog(
            &serde_json::to_vec(&base).expect("serialize"),
            &[profile()],
            "a".repeat(64).as_str(),
            "b".repeat(64).as_str(),
            &empty_policy(),
            None,
        )
        .expect_err("conflict");
        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_ALIAS_CONFLICT");
    }

    #[test]
    fn config_patch_round_trips_native_paths_and_restores_original_value() {
        let current = br#"model_provider = "aio"
[model_providers.aio]
base_url = "http://127.0.0.1:37123/v1"
"#;
        let original_path = std::env::temp_dir()
            .join("Codex Catalogs")
            .join("custom.json");
        let mut original = toml_edit::DocumentMut::new();
        original["model_catalog_json"] = toml_edit::value(
            original_path
                .to_str()
                .expect("temporary path must be UTF-8"),
        );
        let original = original.to_string().into_bytes();
        let generated = std::env::temp_dir()
            .join("AIO Data")
            .join("managed-model-catalog.json");
        let patched = patch_model_catalog_config(
            current,
            Some(original.as_slice()),
            Some(generated.as_path()),
        )
        .expect("patch generated");
        let parsed = std::str::from_utf8(&patched)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(parsed["model_catalog_json"].as_str(), generated.to_str());

        let restored = patch_model_catalog_config(&patched, Some(original.as_slice()), None)
            .expect("restore original");
        let parsed = std::str::from_utf8(&restored)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(
            parsed["model_catalog_json"].as_str(),
            original_path.to_str()
        );
    }

    fn catalog_transaction_fixture(
        dir: &Path,
    ) -> (
        Option<AppliedFileChange>,
        FileSnapshot,
        Vec<u8>,
        FileSnapshot,
        Option<Vec<u8>>,
    ) {
        let baseline_path = dir.join("config.toml.backup");
        let config_path = dir.join("config.toml");
        let generated_path = dir.join(GENERATED_CATALOG_FILE_NAME);
        let baseline_before = b"baseline-before".to_vec();
        let config_before = b"config-before".to_vec();
        let generated_before = b"generated-before".to_vec();
        std::fs::write(&baseline_path, &baseline_before).expect("write baseline fixture");
        std::fs::write(&config_path, &config_before).expect("write config fixture");
        std::fs::write(&generated_path, &generated_before).expect("write catalog fixture");

        (
            Some(AppliedFileChange {
                before: FileSnapshot {
                    path: baseline_path,
                    bytes: Some(baseline_before),
                },
                after: Some(b"baseline-after".to_vec()),
            }),
            FileSnapshot {
                path: config_path,
                bytes: Some(config_before),
            },
            b"config-after".to_vec(),
            FileSnapshot {
                path: generated_path,
                bytes: Some(generated_before),
            },
            Some(b"generated-after".to_vec()),
        )
    }

    fn reset_catalog_transaction_test_state() {
        CATALOG_APPLY_FAILURE.with(|failure| failure.set(None));
        CATALOG_POST_APPLY_MUTATION.with(|mutation| mutation.set(None));
        CATALOG_APPLY_TRACE.with(|trace| trace.borrow_mut().clear());
        CATALOG_ROLLBACK_TRACE.with(|trace| trace.borrow_mut().clear());
    }

    fn catalog_apply_trace() -> Vec<CatalogApplyStage> {
        CATALOG_APPLY_TRACE.with(|trace| trace.borrow().clone())
    }

    fn catalog_rollback_trace() -> Vec<CatalogApplyStage> {
        CATALOG_ROLLBACK_TRACE.with(|trace| trace.borrow().clone())
    }

    #[test]
    fn catalog_file_transaction_applies_and_rolls_back_all_files() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, config_after, generated, generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();

        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            config_after.clone(),
            generated,
            generated_after.clone(),
        )
        .expect("apply catalog transaction");

        assert_eq!(std::fs::read(&baseline_path).unwrap(), b"baseline-after");
        assert_eq!(std::fs::read(&config_path).unwrap(), config_after);
        assert_eq!(
            std::fs::read(&generated_path).unwrap(),
            generated_after.unwrap()
        );
        assert_eq!(
            catalog_apply_trace(),
            vec![
                CatalogApplyStage::Baseline,
                CatalogApplyStage::Generated,
                CatalogApplyStage::Config,
            ]
        );

        applied.rollback().expect("roll back catalog transaction");
        assert_eq!(
            catalog_rollback_trace(),
            vec![
                CatalogApplyStage::Config,
                CatalogApplyStage::Generated,
                CatalogApplyStage::Baseline,
            ]
        );
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
        assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
        assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
    }

    #[test]
    fn catalog_file_transaction_rolls_back_prior_stages_after_write_failures() {
        for failure_stage in [
            CatalogApplyStage::Baseline,
            CatalogApplyStage::Generated,
            CatalogApplyStage::Config,
        ] {
            reset_catalog_transaction_test_state();
            let temp = tempfile::tempdir().expect("tempdir");
            let (baseline, config, config_after, generated, generated_after) =
                catalog_transaction_fixture(temp.path());
            let baseline_path = baseline.as_ref().unwrap().before.path.clone();
            let config_path = config.path.clone();
            let generated_path = generated.path.clone();
            CATALOG_APPLY_FAILURE.with(|failure| failure.set(Some(failure_stage)));

            let error = apply_prepared_catalog_files(
                baseline,
                config,
                config_after,
                generated,
                generated_after,
            )
            .expect_err("injected catalog write should fail");

            assert_eq!(error.code(), "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED");
            assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
            assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
            assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
        }
    }

    #[test]
    fn catalog_post_apply_confirmation_rejects_owner_or_binding_tampering() {
        for mutation_stage in [CatalogApplyStage::Generated, CatalogApplyStage::Config] {
            reset_catalog_transaction_test_state();
            let temp = tempfile::tempdir().expect("tempdir");
            let (baseline, config, config_after, generated, generated_after) =
                catalog_transaction_fixture(temp.path());
            let baseline_path = baseline.as_ref().unwrap().before.path.clone();
            let config_path = config.path.clone();
            let generated_path = generated.path.clone();
            let baseline_expected = baseline
                .as_ref()
                .map(expected_snapshot_after_change)
                .expect("baseline expected");
            let config_expected = FileSnapshot {
                path: config_path.clone(),
                bytes: Some(config_after.clone()),
            };
            let generated_expected = FileSnapshot {
                path: generated_path.clone(),
                bytes: generated_after.clone(),
            };
            let applied = apply_prepared_catalog_files(
                baseline,
                config,
                config_after,
                generated,
                generated_after,
            )
            .expect("apply catalog transaction");
            CATALOG_POST_APPLY_MUTATION.with(|mutation| mutation.set(Some(mutation_stage)));

            let error = confirm_applied_catalog(
                applied,
                Some(&baseline_expected),
                &config_expected,
                &generated_expected,
                || Ok(()),
            )
            .expect_err("post-apply mutation must fail confirmation");

            assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
            assert_eq!(
                std::fs::read(&baseline_path).expect("read restored baseline"),
                b"baseline-before"
            );
            match mutation_stage {
                CatalogApplyStage::Generated => {
                    assert_eq!(
                        std::fs::read(&config_path).expect("read restored config"),
                        b"config-before"
                    );
                    assert_ne!(
                        std::fs::read(&generated_path).expect("read external catalog winner"),
                        b"generated-before"
                    );
                }
                CatalogApplyStage::Config => {
                    assert_ne!(
                        std::fs::read(&config_path).expect("read external config winner"),
                        b"config-before"
                    );
                    assert_eq!(
                        std::fs::read(&generated_path).expect("read restored catalog"),
                        b"generated-before"
                    );
                }
                CatalogApplyStage::Baseline => unreachable!(),
            }
        }
    }

    #[test]
    fn catalog_rollback_reports_recovery_required_and_continues_other_files() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, config_after, generated, generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();
        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            config_after,
            generated,
            generated_after,
        )
        .expect("apply catalog transaction");
        std::fs::write(&config_path, b"external-change").expect("create rollback drift");

        let error = applied
            .rollback()
            .expect_err("config drift must block rollback");

        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
        assert_eq!(std::fs::read(config_path).unwrap(), b"external-change");
        assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
    }

    #[test]
    fn catalog_deactivation_restores_binding_before_delete_and_rolls_back_in_reverse() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, _config_after, generated, _generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();
        let restored_config = b"config-original-binding".to_vec();

        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            restored_config.clone(),
            generated,
            None,
        )
        .expect("deactivate catalog");

        assert_eq!(std::fs::read(&config_path).unwrap(), restored_config);
        assert!(!generated_path.exists());
        assert_eq!(
            catalog_apply_trace(),
            vec![
                CatalogApplyStage::Baseline,
                CatalogApplyStage::Config,
                CatalogApplyStage::Generated,
            ]
        );

        applied.rollback().expect("roll back deactivation");
        assert_eq!(
            catalog_rollback_trace(),
            vec![
                CatalogApplyStage::Generated,
                CatalogApplyStage::Config,
                CatalogApplyStage::Baseline,
            ]
        );
        assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
        assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
    }

    #[test]
    fn catalog_deactivation_failure_restores_every_committed_prior_stage() {
        for failure_stage in [
            CatalogApplyStage::Baseline,
            CatalogApplyStage::Config,
            CatalogApplyStage::Generated,
        ] {
            reset_catalog_transaction_test_state();
            let temp = tempfile::tempdir().expect("tempdir");
            let (baseline, config, _config_after, generated, _generated_after) =
                catalog_transaction_fixture(temp.path());
            let baseline_path = baseline.as_ref().unwrap().before.path.clone();
            let config_path = config.path.clone();
            let generated_path = generated.path.clone();
            CATALOG_APPLY_FAILURE.with(|failure| failure.set(Some(failure_stage)));

            let error = apply_prepared_catalog_files(
                baseline,
                config,
                b"config-original-binding".to_vec(),
                generated,
                None,
            )
            .expect_err("injected deactivation failure");

            assert_eq!(error.code(), "CODEX_MANAGED_MODEL_TEST_WRITE_FAILED");
            assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
            assert_eq!(std::fs::read(generated_path).unwrap(), b"generated-before");
            assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
            let expected_trace = match failure_stage {
                CatalogApplyStage::Baseline => vec![CatalogApplyStage::Baseline],
                CatalogApplyStage::Config => {
                    vec![CatalogApplyStage::Baseline, CatalogApplyStage::Config]
                }
                CatalogApplyStage::Generated => {
                    vec![
                        CatalogApplyStage::Baseline,
                        CatalogApplyStage::Config,
                        CatalogApplyStage::Generated,
                    ]
                }
            };
            assert_eq!(catalog_apply_trace(), expected_trace);
        }
    }

    #[test]
    fn catalog_deactivation_rollback_restores_config_even_if_generated_drifted() {
        reset_catalog_transaction_test_state();
        let temp = tempfile::tempdir().expect("tempdir");
        let (baseline, config, _config_after, generated, _generated_after) =
            catalog_transaction_fixture(temp.path());
        let baseline_path = baseline.as_ref().unwrap().before.path.clone();
        let config_path = config.path.clone();
        let generated_path = generated.path.clone();
        let applied = apply_prepared_catalog_files(
            baseline,
            config,
            b"config-original-binding".to_vec(),
            generated,
            None,
        )
        .expect("deactivate catalog");
        std::fs::write(&generated_path, b"external-generated").expect("external drift");

        let error = applied.rollback().expect_err("generated rollback drift");

        assert_eq!(error.code(), "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED");
        assert_eq!(std::fs::read(config_path).unwrap(), b"config-before");
        assert_eq!(
            std::fs::read(generated_path).unwrap(),
            b"external-generated"
        );
        assert_eq!(std::fs::read(baseline_path).unwrap(), b"baseline-before");
        assert_eq!(
            catalog_rollback_trace(),
            vec![
                CatalogApplyStage::Generated,
                CatalogApplyStage::Config,
                CatalogApplyStage::Baseline,
            ]
        );
    }

    #[test]
    fn generated_catalog_binding_in_proxy_backup_is_prepared_for_repair() {
        let dir = std::env::temp_dir().join(format!(
            "aio-catalog-baseline-repair-{}",
            crate::shared::uuid::new_uuid_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let generated = dir.join(GENERATED_CATALOG_FILE_NAME);
        let generated_bytes = generate_test_catalog(
            &base_catalog(),
            &[profile()],
            &"a".repeat(64),
            &"b".repeat(64),
            &empty_policy(),
            None,
        )
        .expect("generate owned catalog");
        std::fs::write(&generated, &generated_bytes).expect("write owned catalog");
        let metadata = validate_owned_catalog(&generated_bytes).expect("owned metadata");
        let backup_path = dir.join("config.toml.backup");
        let mut document = toml_edit::DocumentMut::new();
        document["model"] = toml_edit::value("gpt-5");
        document["model_catalog_json"] = toml_edit::value(generated.to_string_lossy().to_string());
        let polluted = document.to_string().into_bytes();
        std::fs::write(&backup_path, &polluted).expect("write polluted backup");
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: dir.join("config.toml"),
            config_backup_path: Some(backup_path),
            config_bytes: Some(polluted),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };

        assert_eq!(
            prepare_catalog_baseline(&baseline, &generated, None)
                .expect_err("an unowned active generated binding must fail closed")
                .code(),
            "CODEX_MANAGED_MODEL_RECOVERY_REQUIRED"
        );
        let prepared = prepare_catalog_baseline(&baseline, &generated, Some(&metadata))
            .expect("prepare repair");

        assert!(prepared.catalog_path.is_none());
        assert!(prepared.backup_change.is_some());
        let repaired = std::str::from_utf8(
            prepared
                .backup_change
                .as_ref()
                .and_then(|change| change.after.as_deref())
                .expect("repaired baseline bytes"),
        )
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
        assert!(repaired.get("model_catalog_json").is_none());
        assert_eq!(repaired["model"].as_str(), Some("gpt-5"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn v2_generated_binding_in_proxy_backup_recovers_original_user_catalog() {
        let temp = tempfile::tempdir().expect("tempdir");
        let generated = temp.path().join(GENERATED_CATALOG_FILE_NAME);
        let original_catalog = temp.path().join("user-catalog.json");
        let generated_bytes = legacy_v2_catalog(Some(&original_catalog));
        std::fs::write(&generated, &generated_bytes).expect("write generated catalog");
        let metadata = validate_owned_catalog(&generated_bytes).expect("v2 metadata");

        let backup_path = temp.path().join("config.toml.backup");
        let polluted = config_with_catalog(Some(&generated));
        std::fs::write(&backup_path, &polluted).expect("write proxy backup");
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: temp.path().join("config.toml"),
            config_backup_path: Some(backup_path),
            config_bytes: Some(polluted),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };

        let prepared = prepare_catalog_baseline(&baseline, &generated, Some(&metadata))
            .expect("recover original binding");

        assert_eq!(
            prepared.catalog_path.as_deref(),
            Some(original_catalog.as_path())
        );
        let repaired = prepared
            .backup_change
            .as_ref()
            .and_then(|change| change.after.as_deref())
            .expect("repaired baseline bytes");
        assert_eq!(
            parse_original_catalog_path(Some(repaired)).expect("repaired catalog path"),
            Some(original_catalog)
        );
    }

    #[test]
    fn user_catalog_binding_is_not_prepared_for_repair() {
        let dir = std::env::temp_dir().join(format!(
            "aio-user-catalog-baseline-{}",
            crate::shared::uuid::new_uuid_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let generated = dir.join(GENERATED_CATALOG_FILE_NAME);
        let user_catalog = dir.join("user-catalog.json");
        let mut document = toml_edit::DocumentMut::new();
        document["model_catalog_json"] =
            toml_edit::value(user_catalog.to_string_lossy().to_string());
        let original = document.to_string().into_bytes();
        let baseline = crate::cli_proxy::CodexProxyBaseline {
            config_path: dir.join("config.toml"),
            config_backup_path: Some(dir.join("config.toml.backup")),
            config_bytes: Some(original.clone()),
            base_origin: "http://127.0.0.1:37123".to_string(),
        };

        let prepared =
            prepare_catalog_baseline(&baseline, &generated, None).expect("keep user path");

        assert_eq!(prepared.catalog_path, Some(user_catalog));
        assert!(prepared.backup_change.is_none());
        assert_eq!(baseline.config_bytes, Some(original));
        let _ = std::fs::remove_dir_all(dir);
    }
}
