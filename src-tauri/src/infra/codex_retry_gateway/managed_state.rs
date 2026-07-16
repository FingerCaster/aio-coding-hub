use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayError, CodexRetryGatewayRouteSnapshot, CodexRetryGatewayRouteTransition,
    CodexRetryGatewayTransitionStore, CODEX_RETRY_GATEWAY_REPOSITORY,
    CODEX_RETRY_GATEWAY_ROUTE_TRANSITION_SCHEMA_VERSION,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::{
    read_file_with_max_len, read_optional_file_with_max_len, write_file_atomic,
    write_file_atomic_if_changed,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::util::{
    canonicalize_path_within_root, create_or_validate_plain_directory,
    ensure_not_symlink_or_reparse, ensure_path_within_root, metadata_is_symlink_or_reparse,
    normalize_full_sha, normalized_internal_relative_path,
};

const FEATURE_ROOT_DIR_NAME: &str = "codex-retry-gateway";
const MANAGER_SCHEMA_VERSION: u32 = 1;
const SOURCE_MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANAGER_STATE_MAX_BYTES: usize = 512 * 1024;
const SOURCE_MANIFEST_MAX_BYTES: usize = 256 * 1024;
#[allow(dead_code)] // Integration-owned route transition glue persists this file outside the runtime worker.
const TRANSITION_MAX_BYTES: usize = 512 * 1024;
const TRANSITION_SNAPSHOT_MAX_BYTES: usize = 1024 * 1024;
const TRANSITION_SNAPSHOT_AGGREGATE_MAX_BYTES: usize = 8 * 1024 * 1024;
const TRANSITION_SNAPSHOT_DIR_NAME: &str = "transition-snapshots";
const TRANSITION_SNAPSHOT_STAGING_DIR_NAME: &str = "transition-snapshots.staging";

#[derive(Debug, Clone)]
pub(crate) struct CodexRetryGatewayManagerPaths {
    pub(crate) root: PathBuf,
    pub(crate) manager_path: PathBuf,
    pub(crate) sources_dir: PathBuf,
    pub(crate) downloads_dir: PathBuf,
    pub(crate) runtime_dir: PathBuf,
    pub(crate) runtime_config_dir: PathBuf,
    pub(crate) runtime_logs_dir: PathBuf,
    pub(crate) runtime_analytics_dir: PathBuf,
    pub(crate) runtime_config_path: PathBuf,
    pub(crate) runtime_state_path: PathBuf,
    pub(crate) runtime_log_path: PathBuf,
    pub(crate) runtime_pid_path: PathBuf,
    pub(crate) route_dir: PathBuf,
    #[allow(dead_code)]
    // Integration-owned route glue consumes this path outside the runtime worker.
    pub(crate) transition_path: PathBuf,
}

impl CodexRetryGatewayManagerPaths {
    pub(crate) fn from_root(root: PathBuf) -> Self {
        let runtime_dir = root.join("runtime");
        let runtime_config_dir = runtime_dir.join("config");
        let route_dir = root.join("route");
        Self {
            manager_path: root.join("manager.json"),
            sources_dir: root.join("sources"),
            downloads_dir: root.join("downloads"),
            runtime_logs_dir: runtime_dir.join("logs"),
            runtime_analytics_dir: runtime_dir.join("analytics"),
            runtime_config_path: runtime_config_dir.join("config.json"),
            runtime_state_path: runtime_dir.join("state.json"),
            runtime_log_path: runtime_dir.join("logs").join("gateway.log"),
            runtime_pid_path: runtime_dir.join("gateway.pid"),
            transition_path: route_dir.join("transition.json"),
            root,
            runtime_dir,
            runtime_config_dir,
            route_dir,
        }
    }

    pub(crate) fn from_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<Self> {
        let root = crate::app_paths::app_data_dir(app)?.join(FEATURE_ROOT_DIR_NAME);
        Ok(Self::from_root(root))
    }

    pub(crate) fn ensure_dirs(&self) -> AppResult<()> {
        create_or_validate_plain_directory(&self.root, "managed gateway root")?;
        for (dir, label) in [
            (&self.sources_dir, "managed gateway sources directory"),
            (&self.downloads_dir, "managed gateway downloads directory"),
            (&self.runtime_dir, "managed gateway runtime directory"),
            (
                &self.runtime_config_dir,
                "managed gateway runtime config directory",
            ),
            (
                &self.runtime_logs_dir,
                "managed gateway runtime logs directory",
            ),
            (
                &self.runtime_analytics_dir,
                "managed gateway runtime analytics directory",
            ),
            (&self.route_dir, "managed gateway route directory"),
        ] {
            create_or_validate_plain_directory(dir, label)?;
            ensure_path_within_root(&self.root, dir)?;
        }
        Ok(())
    }

    pub(crate) fn source_dir(&self, commit: &str) -> AppResult<PathBuf> {
        Ok(self.sources_dir.join(normalize_full_sha(commit)?))
    }

    pub(crate) fn source_manifest_path(&self, commit: &str) -> AppResult<PathBuf> {
        Ok(self.source_dir(commit)?.join("manifest.json"))
    }

    pub(crate) fn internal_path(&self, relative: &str) -> AppResult<PathBuf> {
        Ok(self.root.join(normalized_internal_relative_path(relative)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewayManagerState {
    pub schema_version: u32,
    pub repository: String,
    pub generation: u64,
    pub active_commit: Option<String>,
    pub previous_commit: Option<String>,
    pub effective_port: Option<u16>,
    pub verified_main_commit: Option<String>,
    pub process_record: Option<CodexRetryGatewayManagedProcessRecord>,
    #[serde(default)]
    pub pending_launch: Option<CodexRetryGatewayPendingLaunchRecord>,
    pub recovery_failure_count: u32,
    pub recovery_next_retry_at_ms: Option<u64>,
    pub recovery_paused: bool,
    pub last_error: Option<CodexRetryGatewayError>,
}

impl CodexRetryGatewayManagerState {
    pub(crate) fn validate(&mut self) -> AppResult<()> {
        if self.schema_version != MANAGER_SCHEMA_VERSION {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_STATE_SCHEMA_UNSUPPORTED",
                format!(
                    "unsupported codex retry gateway manager schema {}",
                    self.schema_version
                ),
            ));
        }
        if self.repository != CODEX_RETRY_GATEWAY_REPOSITORY {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_STATE_REPOSITORY_MISMATCH",
                format!(
                    "expected repository {}, got {}",
                    CODEX_RETRY_GATEWAY_REPOSITORY, self.repository
                ),
            ));
        }
        if let Some(commit) = self.active_commit.as_deref() {
            self.active_commit = Some(normalize_full_sha(commit)?);
        }
        if let Some(commit) = self.previous_commit.as_deref() {
            self.previous_commit = Some(normalize_full_sha(commit)?);
        }
        if let Some(commit) = self.verified_main_commit.as_deref() {
            self.verified_main_commit = Some(normalize_full_sha(commit)?);
        }
        if let Some(port) = self.effective_port {
            if !(1024..=65535).contains(&port) {
                return Err("SEC_INVALID_INPUT: effective port out of range".into());
            }
        }
        if let Some(record) = &mut self.process_record {
            record.validate()?;
        }
        if let Some(record) = &mut self.pending_launch {
            record.validate()?;
        }
        Ok(())
    }
}

impl Default for CodexRetryGatewayManagerState {
    fn default() -> Self {
        Self {
            schema_version: MANAGER_SCHEMA_VERSION,
            repository: CODEX_RETRY_GATEWAY_REPOSITORY.to_string(),
            generation: 0,
            active_commit: None,
            previous_commit: None,
            effective_port: None,
            verified_main_commit: None,
            process_record: None,
            pending_launch: None,
            recovery_failure_count: 0,
            recovery_next_retry_at_ms: None,
            recovery_paused: false,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewayManagedProcessRecord {
    pub pid: u32,
    pub start_identity: Option<u64>,
    pub started_at_ms: u64,
    pub node_executable: String,
    pub source_commit: String,
    pub source_dir_rel: String,
    pub config_path_rel: String,
    pub state_path_rel: String,
    pub log_path_rel: String,
    pub listener: String,
    pub upstream_base_url: String,
    pub instance_nonce: String,
    #[serde(default = "default_managed_provider_name")]
    pub provider_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewayPendingLaunchRecord {
    pub created_at_ms: u64,
    pub node_executable: String,
    pub source_commit: String,
    pub source_dir_rel: String,
    pub config_path_rel: String,
    pub state_path_rel: String,
    pub log_path_rel: String,
    pub listener: String,
    pub upstream_base_url: String,
    pub instance_nonce: String,
    #[serde(default = "default_managed_provider_name")]
    pub provider_name: String,
}

impl CodexRetryGatewayPendingLaunchRecord {
    pub(crate) fn validate(&mut self) -> AppResult<()> {
        self.source_commit = normalize_full_sha(&self.source_commit)?;
        if self.node_executable.trim().is_empty() {
            return Err("SEC_INVALID_INPUT: node executable must not be empty".into());
        }
        self.source_dir_rel = normalized_internal_relative_path(&self.source_dir_rel)?
            .display()
            .to_string();
        self.config_path_rel = normalized_internal_relative_path(&self.config_path_rel)?
            .display()
            .to_string();
        self.state_path_rel = normalized_internal_relative_path(&self.state_path_rel)?
            .display()
            .to_string();
        self.log_path_rel = normalized_internal_relative_path(&self.log_path_rel)?
            .display()
            .to_string();
        if self.listener.trim().is_empty() || self.upstream_base_url.trim().is_empty() {
            return Err(
                "SEC_INVALID_INPUT: pending launch ownership identity is incomplete".into(),
            );
        }
        if self.instance_nonce.trim().is_empty() {
            return Err(
                "SEC_INVALID_INPUT: pending launch instance nonce must not be empty".into(),
            );
        }
        crate::infra::codex_retry_gateway::config::validate_managed_provider_name(
            &self.provider_name,
        )?;
        Ok(())
    }

    pub(crate) fn into_process_record(
        self,
        pid: u32,
        start_identity: u64,
    ) -> CodexRetryGatewayManagedProcessRecord {
        CodexRetryGatewayManagedProcessRecord {
            pid,
            start_identity: Some(start_identity),
            started_at_ms: self.created_at_ms,
            node_executable: self.node_executable,
            source_commit: self.source_commit,
            source_dir_rel: self.source_dir_rel,
            config_path_rel: self.config_path_rel,
            state_path_rel: self.state_path_rel,
            log_path_rel: self.log_path_rel,
            listener: self.listener,
            upstream_base_url: self.upstream_base_url,
            instance_nonce: self.instance_nonce,
            provider_name: self.provider_name,
        }
    }
}

fn default_managed_provider_name() -> String {
    crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO.to_string()
}

impl CodexRetryGatewayManagedProcessRecord {
    pub(crate) fn validate(&mut self) -> AppResult<()> {
        self.source_commit = normalize_full_sha(&self.source_commit)?;
        if self.pid == 0 {
            return Err("SEC_INVALID_INPUT: process pid must be non-zero".into());
        }
        if self.node_executable.trim().is_empty() {
            return Err("SEC_INVALID_INPUT: node executable must not be empty".into());
        }
        self.source_dir_rel = normalized_internal_relative_path(&self.source_dir_rel)?
            .display()
            .to_string();
        self.config_path_rel = normalized_internal_relative_path(&self.config_path_rel)?
            .display()
            .to_string();
        self.state_path_rel = normalized_internal_relative_path(&self.state_path_rel)?
            .display()
            .to_string();
        self.log_path_rel = normalized_internal_relative_path(&self.log_path_rel)?
            .display()
            .to_string();
        if self.listener.trim().is_empty() || self.upstream_base_url.trim().is_empty() {
            return Err("SEC_INVALID_INPUT: process ownership identity is incomplete".into());
        }
        if self.instance_nonce.trim().is_empty() {
            return Err("SEC_INVALID_INPUT: process instance nonce must not be empty".into());
        }
        crate::infra::codex_retry_gateway::config::validate_managed_provider_name(
            &self.provider_name,
        )?;
        Ok(())
    }

    pub(crate) fn source_dir(&self, paths: &CodexRetryGatewayManagerPaths) -> AppResult<PathBuf> {
        paths.internal_path(&self.source_dir_rel)
    }

    pub(crate) fn config_path(&self, paths: &CodexRetryGatewayManagerPaths) -> AppResult<PathBuf> {
        paths.internal_path(&self.config_path_rel)
    }

    pub(crate) fn state_path(&self, paths: &CodexRetryGatewayManagerPaths) -> AppResult<PathBuf> {
        paths.internal_path(&self.state_path_rel)
    }

    pub(crate) fn log_path(&self, paths: &CodexRetryGatewayManagerPaths) -> AppResult<PathBuf> {
        paths.internal_path(&self.log_path_rel)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexRetryGatewaySourceManifest {
    pub schema_version: u32,
    pub repository: String,
    pub commit: String,
    pub verified_main_commit: String,
    pub verified_at_ms: u64,
    pub archive_sha256: String,
    pub source_sha256: String,
    pub file_count: u32,
    pub total_bytes: u64,
    pub gateway_entry_rel: String,
    pub admin_entry_rel: String,
    pub launch_ui_entry_rel: String,
}

impl CodexRetryGatewaySourceManifest {
    pub(crate) fn validate(&mut self) -> AppResult<()> {
        if self.schema_version != SOURCE_MANIFEST_SCHEMA_VERSION {
            return Err(
                "CODEX_RETRY_GATEWAY_SOURCE_SCHEMA_UNSUPPORTED: unsupported source manifest schema"
                    .into(),
            );
        }
        if self.repository != CODEX_RETRY_GATEWAY_REPOSITORY {
            return Err(
                "CODEX_RETRY_GATEWAY_SOURCE_REPOSITORY_MISMATCH: source repository mismatch".into(),
            );
        }
        self.commit = normalize_full_sha(&self.commit)?;
        self.verified_main_commit = normalize_full_sha(&self.verified_main_commit)?;
        self.gateway_entry_rel = normalized_internal_relative_path(&self.gateway_entry_rel)?
            .display()
            .to_string();
        self.admin_entry_rel = normalized_internal_relative_path(&self.admin_entry_rel)?
            .display()
            .to_string();
        self.launch_ui_entry_rel = normalized_internal_relative_path(&self.launch_ui_entry_rel)?
            .display()
            .to_string();
        Ok(())
    }
}

pub(crate) fn read_manager_state(
    paths: &CodexRetryGatewayManagerPaths,
) -> AppResult<CodexRetryGatewayManagerState> {
    let Some(bytes) =
        read_optional_file_with_max_len(&paths.manager_path, MANAGER_STATE_MAX_BYTES)?
    else {
        return Ok(CodexRetryGatewayManagerState::default());
    };
    let mut state: CodexRetryGatewayManagerState = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse {}: {err}", paths.manager_path.display()))?;
    state.validate()?;
    Ok(state)
}

pub(crate) fn write_manager_state(
    paths: &CodexRetryGatewayManagerPaths,
    state: &CodexRetryGatewayManagerState,
) -> AppResult<bool> {
    paths.ensure_dirs()?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|err| format!("failed to serialize manager state: {err}"))?;
    let mut bytes = bytes;
    bytes.push(b'\n');
    write_file_atomic_if_changed(&paths.manager_path, &bytes)
}

pub(crate) fn read_source_manifest(
    paths: &CodexRetryGatewayManagerPaths,
    commit: &str,
) -> AppResult<CodexRetryGatewaySourceManifest> {
    let requested_commit = normalize_full_sha(commit)?;
    let path = paths.source_manifest_path(&requested_commit)?;
    let bytes = crate::shared::fs::read_file_with_max_len(&path, SOURCE_MANIFEST_MAX_BYTES)?;
    let mut manifest: CodexRetryGatewaySourceManifest = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    manifest.validate()?;
    if manifest.commit != requested_commit {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_COMMIT_MISMATCH",
            format!(
                "source manifest commit {} does not match requested source directory {}",
                manifest.commit, requested_commit
            ),
        ));
    }
    ensure_path_within_root(&paths.root, &path)?;
    Ok(manifest)
}

pub(crate) fn write_source_manifest(
    path: &Path,
    manifest: &CodexRetryGatewaySourceManifest,
) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|err| format!("failed to serialize source manifest: {err}"))?;
    let mut bytes = bytes;
    bytes.push(b'\n');
    write_file_atomic(path, &bytes)
}

#[allow(dead_code)] // Integration-owned route transition glue constructs this store outside the runtime worker.
#[derive(Debug, Clone)]
pub(crate) struct FileCodexRetryGatewayTransitionStore {
    path: PathBuf,
}

fn transition_sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn validate_transition_sha256(value: &str, label: &str) -> AppResult<()> {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return Err(format!("SEC_INVALID_INPUT: {label} must use sha256:<hex>").into());
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("SEC_INVALID_INPUT: {label} must contain a 64-hex digest").into());
    }
    Ok(())
}

fn validate_plain_directory_tree(path: &Path, label: &str) -> AppResult<()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!(
            "failed to read {label} metadata {}: {error}",
            path.display()
        )
    })?;
    if metadata_is_symlink_or_reparse(&metadata) {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} contains a symbolic link or reparse point: {}",
            path.display()
        )
        .into());
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} contains an unsupported entry: {}",
            path.display()
        )
        .into());
    }
    for entry in std::fs::read_dir(path)
        .map_err(|error| format!("failed to read {label} {}: {error}", path.display()))?
    {
        let entry =
            entry.map_err(|error| format!("failed to read {label} {}: {error}", path.display()))?;
        validate_plain_directory_tree(&entry.path(), label)?;
    }
    Ok(())
}

fn remove_plain_directory_if_present(path: &Path, label: &str) -> AppResult<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to read {label} metadata {}: {error}",
                path.display()
            )
            .into())
        }
    };
    if metadata_is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} must be a plain directory: {}",
            path.display()
        )
        .into());
    }
    validate_plain_directory_tree(path, label)?;
    std::fs::remove_dir_all(path)
        .map_err(|error| format!("failed to remove {label} {}: {error}", path.display()).into())
}

#[allow(dead_code)] // Integration-owned route transition glue exercises these helpers outside the runtime worker.
impl FileCodexRetryGatewayTransitionStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn parent(&self) -> AppResult<&Path> {
        self.path.parent().ok_or_else(|| {
            AppError::from("SEC_INVALID_INPUT: transition path must have a parent directory")
        })
    }

    fn snapshot_root(&self) -> AppResult<PathBuf> {
        Ok(self.parent()?.join(TRANSITION_SNAPSHOT_DIR_NAME))
    }

    fn snapshot_staging_root(&self) -> AppResult<PathBuf> {
        Ok(self.parent()?.join(TRANSITION_SNAPSHOT_STAGING_DIR_NAME))
    }

    fn validate_transition(&self, transition: &CodexRetryGatewayRouteTransition) -> AppResult<()> {
        if transition.schema_version != CODEX_RETRY_GATEWAY_ROUTE_TRANSITION_SCHEMA_VERSION {
            return Err(format!(
                "CODEX_RETRY_GATEWAY_TRANSITION_SCHEMA_UNSUPPORTED: expected {}, got {}",
                CODEX_RETRY_GATEWAY_ROUTE_TRANSITION_SCHEMA_VERSION, transition.schema_version
            )
            .into());
        }
        if transition.operation_id.trim().is_empty()
            || transition.target_generation != transition.prior_generation.saturating_add(1)
        {
            return Err(
                "CODEX_RETRY_GATEWAY_TRANSITION_INVALID: invalid operation or generation".into(),
            );
        }
        for (value, label) in [
            (
                transition.prior_canonical_config_sha256.as_str(),
                "prior canonical config hash",
            ),
            (
                transition.prior_live_config_sha256.as_str(),
                "prior live config hash",
            ),
            (
                transition.canonical_config_sha256.as_str(),
                "target canonical config hash",
            ),
            (
                transition.live_config_sha256.as_str(),
                "target live config hash",
            ),
        ] {
            validate_transition_sha256(value, label)?;
        }
        if let Some(commit) = transition.source_commit.as_deref() {
            let _ = normalize_full_sha(commit)?;
        }
        if transition.snapshots.is_empty() || transition.snapshots.len() > 32 {
            return Err(
                "CODEX_RETRY_GATEWAY_TRANSITION_INVALID: route snapshots must contain 1..=32 files"
                    .into(),
            );
        }

        let mut targets = HashSet::new();
        let mut backups = HashSet::new();
        for snapshot in &transition.snapshots {
            validate_transition_sha256(&snapshot.root_path_sha256, "snapshot root hash")?;
            let target_rel = normalized_internal_relative_path(&snapshot.target_rel)?;
            if !targets.insert((snapshot.root, target_rel)) {
                return Err(
                    "CODEX_RETRY_GATEWAY_TRANSITION_INVALID: duplicate snapshot target".into(),
                );
            }
            match (
                snapshot.existed,
                snapshot.backup_rel.as_deref(),
                snapshot.backup_sha256.as_deref(),
            ) {
                (true, Some(backup_rel), Some(backup_sha256)) => {
                    let backup_path = normalized_internal_relative_path(backup_rel)?;
                    if backup_path.components().next()
                        != Some(std::path::Component::Normal(std::ffi::OsStr::new("files")))
                        || !backups.insert(backup_path)
                    {
                        return Err("CODEX_RETRY_GATEWAY_TRANSITION_INVALID: snapshot backup path is invalid or duplicated".into());
                    }
                    validate_transition_sha256(backup_sha256, "snapshot backup hash")?;
                }
                (false, None, None) => {}
                _ => {
                    return Err("CODEX_RETRY_GATEWAY_TRANSITION_INVALID: snapshot existence metadata is inconsistent".into())
                }
            }
        }
        Ok(())
    }

    fn read_transition(&self) -> AppResult<Option<CodexRetryGatewayRouteTransition>> {
        match std::fs::symlink_metadata(self.parent()?) {
            Ok(metadata) => {
                if metadata_is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(format!(
                        "SEC_INVALID_INPUT: transition parent must be a plain directory: {}",
                        self.parent()?.display()
                    )
                    .into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "failed to read transition parent metadata {}: {error}",
                    self.parent()?.display()
                )
                .into())
            }
        }
        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                if metadata_is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                    return Err(format!(
                        "SEC_INVALID_INPUT: transition journal must be a plain file: {}",
                        self.path.display()
                    )
                    .into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "failed to read transition journal metadata {}: {error}",
                    self.path.display(),
                )
                .into());
            }
        }
        let bytes = read_file_with_max_len(&self.path, TRANSITION_MAX_BYTES)?;
        let transition: CodexRetryGatewayRouteTransition = serde_json::from_slice(&bytes)
            .map_err(|err| format!("failed to parse {}: {err}", self.path.display()))?;
        self.validate_transition(&transition)?;
        let snapshot_root = self.snapshot_root()?;
        let metadata =
            ensure_not_symlink_or_reparse(&snapshot_root, "Codex route transition snapshot root")?;
        if !metadata.is_dir() {
            return Err(format!(
                "CODEX_RETRY_GATEWAY_TRANSITION_RECOVERY_FAILED: snapshot root is not a directory: {}",
                snapshot_root.display()
            )
            .into());
        }
        Ok(Some(transition))
    }

    fn cleanup_snapshot_artifacts(&self) -> AppResult<()> {
        remove_plain_directory_if_present(
            &self.snapshot_root()?,
            "Codex route transition snapshot root",
        )?;
        remove_plain_directory_if_present(
            &self.snapshot_staging_root()?,
            "Codex route transition snapshot staging root",
        )
    }
}

impl CodexRetryGatewayTransitionStore for FileCodexRetryGatewayTransitionStore {
    fn load_pending(&self) -> AppResult<Option<CodexRetryGatewayRouteTransition>> {
        self.read_transition()
    }

    fn prepare(
        &self,
        transition: &CodexRetryGatewayRouteTransition,
        snapshot_bytes: &[Option<Vec<u8>>],
    ) -> AppResult<()> {
        self.validate_transition(transition)?;
        if transition.snapshots.len() != snapshot_bytes.len() {
            return Err(
                "CODEX_RETRY_GATEWAY_TRANSITION_INVALID: snapshot metadata and bytes count differ"
                    .into(),
            );
        }
        if self.read_transition()?.is_some() {
            return Err(
                "CODEX_RETRY_GATEWAY_TRANSITION_RECOVERY_REQUIRED: pending route transition exists"
                    .into(),
            );
        }
        create_or_validate_plain_directory(
            self.parent()?,
            "Codex route transition parent directory",
        )?;
        self.cleanup_snapshot_artifacts()?;

        let staging_root = self.snapshot_staging_root()?;
        std::fs::create_dir(&staging_root).map_err(|error| {
            format!(
                "failed to create Codex route transition snapshot staging root {}: {error}",
                staging_root.display()
            )
        })?;
        let files_root = staging_root.join("files");
        std::fs::create_dir(&files_root).map_err(|error| {
            format!(
                "failed to create Codex route transition snapshot files root {}: {error}",
                files_root.display()
            )
        })?;

        let staged = (|| -> AppResult<()> {
            let mut aggregate_bytes = 0usize;
            for (snapshot, bytes) in transition.snapshots.iter().zip(snapshot_bytes) {
                match (snapshot.existed, bytes.as_deref()) {
                    (true, Some(bytes)) => {
                        if bytes.len() > TRANSITION_SNAPSHOT_MAX_BYTES {
                            return Err(format!(
                                "SEC_INVALID_INPUT: route snapshot {} exceeds {} bytes",
                                snapshot.target_rel, TRANSITION_SNAPSHOT_MAX_BYTES
                            )
                            .into());
                        }
                        aggregate_bytes = aggregate_bytes.saturating_add(bytes.len());
                        if aggregate_bytes > TRANSITION_SNAPSHOT_AGGREGATE_MAX_BYTES {
                            return Err("SEC_INVALID_INPUT: route snapshot aggregate exceeds 8 MiB".into());
                        }
                        if snapshot.backup_sha256.as_deref()
                            != Some(transition_sha256(bytes).as_str())
                        {
                            return Err("CODEX_RETRY_GATEWAY_TRANSITION_INVALID: snapshot bytes do not match declared hash".into());
                        }
                        let backup_rel = snapshot.backup_rel.as_deref().ok_or_else(|| {
                            AppError::from("CODEX_RETRY_GATEWAY_TRANSITION_INVALID: snapshot backup path missing")
                        })?;
                        let backup_path = staging_root
                            .join(normalized_internal_relative_path(backup_rel)?);
                        write_file_atomic(&backup_path, bytes)?;
                    }
                    (false, None) => {}
                    _ => {
                        return Err("CODEX_RETRY_GATEWAY_TRANSITION_INVALID: snapshot bytes contradict existence metadata".into())
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = staged {
            let _ = remove_plain_directory_if_present(
                &staging_root,
                "Codex route transition snapshot staging root",
            );
            return Err(error);
        }

        let snapshot_root = self.snapshot_root()?;
        if let Err(error) = std::fs::rename(&staging_root, &snapshot_root) {
            let _ = remove_plain_directory_if_present(
                &staging_root,
                "Codex route transition snapshot staging root",
            );
            return Err(format!(
                "failed to publish Codex route transition snapshots {}: {error}",
                snapshot_root.display()
            )
            .into());
        }
        let bytes = serde_json::to_vec_pretty(transition)
            .map_err(|err| format!("failed to serialize transition: {err}"))?;
        let mut bytes = bytes;
        bytes.push(b'\n');
        if bytes.len() > TRANSITION_MAX_BYTES {
            let _ = remove_plain_directory_if_present(
                &snapshot_root,
                "Codex route transition snapshot root",
            );
            return Err("SEC_INVALID_INPUT: route transition journal exceeds 512 KiB".into());
        }
        if let Err(error) = write_file_atomic(&self.path, &bytes) {
            let _ = remove_plain_directory_if_present(
                &snapshot_root,
                "Codex route transition snapshot root",
            );
            return Err(error);
        }
        Ok(())
    }

    fn read_snapshot(
        &self,
        transition: &CodexRetryGatewayRouteTransition,
        snapshot: &CodexRetryGatewayRouteSnapshot,
    ) -> AppResult<Vec<u8>> {
        let existing = self.read_transition()?.ok_or_else(|| {
            AppError::from("CODEX_RETRY_GATEWAY_TRANSITION_MISSING: pending transition not found")
        })?;
        if existing.operation_id != transition.operation_id
            || !existing
                .snapshots
                .iter()
                .any(|candidate| candidate == snapshot)
            || !snapshot.existed
        {
            return Err("CODEX_RETRY_GATEWAY_TRANSITION_MISMATCH: snapshot read did not match pending transition".into());
        }
        let backup_rel = snapshot.backup_rel.as_deref().ok_or_else(|| {
            AppError::from(
                "CODEX_RETRY_GATEWAY_TRANSITION_INVALID: existing snapshot has no backup path",
            )
        })?;
        let snapshot_root = self.snapshot_root()?;
        let path = snapshot_root.join(normalized_internal_relative_path(backup_rel)?);
        let metadata = ensure_not_symlink_or_reparse(&path, "Codex route transition snapshot")?;
        if !metadata.is_file() {
            return Err(format!(
                "CODEX_RETRY_GATEWAY_TRANSITION_RECOVERY_FAILED: snapshot is not a file: {}",
                path.display()
            )
            .into());
        }
        let _ = canonicalize_path_within_root(
            &snapshot_root,
            &path,
            "Codex route transition snapshot",
        )?;
        let bytes = read_file_with_max_len(&path, TRANSITION_SNAPSHOT_MAX_BYTES)?;
        if snapshot.backup_sha256.as_deref() != Some(transition_sha256(&bytes).as_str()) {
            return Err(
                "CODEX_RETRY_GATEWAY_TRANSITION_RECOVERY_FAILED: snapshot hash mismatch".into(),
            );
        }
        Ok(bytes)
    }

    fn commit(&self, operation_id: &str, generation: u64) -> AppResult<()> {
        let Some(existing) = self.read_transition()? else {
            return Err(
                "CODEX_RETRY_GATEWAY_TRANSITION_MISSING: pending transition not found".into(),
            );
        };
        if existing.operation_id != operation_id || existing.target_generation != generation {
            return Err("CODEX_RETRY_GATEWAY_TRANSITION_MISMATCH: pending transition did not match commit request".into());
        }
        self.clear(operation_id)
    }

    fn clear(&self, operation_id: &str) -> AppResult<()> {
        if let Some(existing) = self.read_transition()? {
            if existing.operation_id != operation_id {
                return Err("CODEX_RETRY_GATEWAY_TRANSITION_MISMATCH: pending transition did not match clear request".into());
            }
            std::fs::remove_file(&self.path).map_err(|error| {
                format!(
                    "failed to remove Codex route transition journal {}: {error}",
                    self.path.display()
                )
            })?;
        }
        self.cleanup_snapshot_artifacts()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::codex_retry_gateway::{CodexRetryGatewayOperationKind, CodexRouteMode};
    use tempfile::tempdir;

    #[test]
    fn manager_state_defaults_to_repository_and_schema() {
        let state = CodexRetryGatewayManagerState::default();
        assert_eq!(state.schema_version, MANAGER_SCHEMA_VERSION);
        assert_eq!(state.repository, CODEX_RETRY_GATEWAY_REPOSITORY);
    }

    #[test]
    fn read_manager_state_returns_default_when_missing() {
        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        let state = read_manager_state(&paths).unwrap();
        assert_eq!(state, CodexRetryGatewayManagerState::default());
    }

    #[test]
    fn write_and_read_manager_state_round_trips() {
        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        let state = CodexRetryGatewayManagerState {
            generation: 7,
            active_commit: Some("ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string()),
            ..Default::default()
        };
        write_manager_state(&paths, &state).unwrap();
        let read_back = read_manager_state(&paths).unwrap();
        assert_eq!(read_back.generation, 7);
        assert_eq!(read_back.active_commit, state.active_commit);
    }

    #[test]
    fn manager_state_round_trips_pending_launch_intent() {
        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        let state = CodexRetryGatewayManagerState {
            pending_launch: Some(CodexRetryGatewayPendingLaunchRecord {
                created_at_ms: 7,
                node_executable: "C:\\node.exe".to_string(),
                source_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
                source_dir_rel: "sources/ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
                config_path_rel: "runtime/config/config.json".to_string(),
                state_path_rel: "runtime/state.json".to_string(),
                log_path_rel: "runtime/logs/gateway.log".to_string(),
                listener: "http://127.0.0.1:4610".to_string(),
                upstream_base_url: "http://127.0.0.1:37123/v1".to_string(),
                instance_nonce: "deadbeef".to_string(),
                provider_name: default_managed_provider_name(),
            }),
            ..Default::default()
        };

        write_manager_state(&paths, &state).unwrap();
        let read_back = read_manager_state(&paths).unwrap();
        let mut expected = state.pending_launch;
        expected.as_mut().unwrap().validate().unwrap();
        assert_eq!(read_back.pending_launch, expected);
    }

    #[test]
    fn legacy_manager_state_defaults_pending_launch_to_none() {
        let state: CodexRetryGatewayManagerState = serde_json::from_value(serde_json::json!({
            "schema_version": MANAGER_SCHEMA_VERSION,
            "repository": CODEX_RETRY_GATEWAY_REPOSITORY,
            "generation": 0,
            "active_commit": null,
            "previous_commit": null,
            "effective_port": null,
            "verified_main_commit": null,
            "process_record": null,
            "recovery_failure_count": 0,
            "recovery_next_retry_at_ms": null,
            "recovery_paused": false,
            "last_error": null
        }))
        .expect("legacy manager state");

        assert!(state.pending_launch.is_none());
    }

    #[cfg(any(unix, windows))]
    fn create_directory_redirect(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).expect("create directory symlink");
        #[cfg(windows)]
        junction::create(target, link).expect("create directory junction");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn ensure_dirs_rejects_redirected_managed_root() {
        let dir = tempdir().unwrap();
        let outside = dir.path().join("outside");
        let root = dir.path().join("gateway");
        std::fs::create_dir(&outside).unwrap();
        create_directory_redirect(&outside, &root);

        let paths = CodexRetryGatewayManagerPaths::from_root(root);
        let error = paths
            .ensure_dirs()
            .expect_err("redirected managed root must fail closed");
        assert!(error.to_string().contains("reparse point"), "{error}");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn ensure_dirs_rejects_redirected_managed_child() {
        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        paths.ensure_dirs().unwrap();
        std::fs::remove_dir(&paths.runtime_logs_dir).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        create_directory_redirect(&outside, &paths.runtime_logs_dir);

        let error = paths
            .ensure_dirs()
            .expect_err("redirected managed child must fail closed");
        assert!(error.to_string().contains("reparse point"), "{error}");
    }

    #[test]
    fn process_record_rejects_unsafe_relative_paths() {
        let mut record = CodexRetryGatewayManagedProcessRecord {
            pid: 1,
            start_identity: None,
            started_at_ms: 1,
            node_executable: "C:\\node.exe".to_string(),
            source_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            source_dir_rel: "../evil".to_string(),
            config_path_rel: "runtime/config/config.json".to_string(),
            state_path_rel: "runtime/state.json".to_string(),
            log_path_rel: "runtime/logs/gateway.log".to_string(),
            listener: "http://127.0.0.1:4610".to_string(),
            upstream_base_url: "http://127.0.0.1:37123/v1".to_string(),
            instance_nonce: "deadbeef".to_string(),
            provider_name: default_managed_provider_name(),
        };
        assert!(record.validate().is_err());
    }

    #[test]
    fn legacy_process_record_defaults_provider_name_to_aio() {
        let record: CodexRetryGatewayManagedProcessRecord =
            serde_json::from_value(serde_json::json!({
                "pid": 1,
                "start_identity": 2,
                "started_at_ms": 3,
                "node_executable": "C:\\node.exe",
                "source_commit": "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2",
                "source_dir_rel": "sources/ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2",
                "config_path_rel": "runtime/config/config.json",
                "state_path_rel": "runtime/state.json",
                "log_path_rel": "runtime/logs/gateway.log",
                "listener": "http://127.0.0.1:4610",
                "upstream_base_url": "http://127.0.0.1:37123/v1",
                "instance_nonce": "deadbeef"
            }))
            .expect("legacy process record");

        assert_eq!(record.provider_name, default_managed_provider_name());
    }

    #[test]
    fn transition_store_prepare_and_commit_is_bounded() {
        let dir = tempdir().unwrap();
        let store = FileCodexRetryGatewayTransitionStore::new(dir.path().join("transition.json"));
        let snapshot = CodexRetryGatewayRouteSnapshot {
            root: crate::infra::codex_retry_gateway::CodexRetryGatewayRouteSnapshotRoot::CodexHome,
            root_path_sha256: format!("sha256:{}", "c".repeat(64)),
            target_rel: "config.toml".to_string(),
            existed: true,
            backup_rel: Some("files/00000000.bin".to_string()),
            backup_sha256: Some(transition_sha256(b"before")),
        };
        let transition = CodexRetryGatewayRouteTransition {
            schema_version: CODEX_RETRY_GATEWAY_ROUTE_TRANSITION_SCHEMA_VERSION,
            operation_id: "op-1".to_string(),
            operation_kind: CodexRetryGatewayOperationKind::Enable,
            prior_generation: 1,
            target_generation: 2,
            prior_mode: CodexRouteMode::Unproxied,
            target_mode: CodexRouteMode::DirectAio,
            prior_canonical_config_sha256: format!("sha256:{}", "a".repeat(64)),
            prior_live_config_sha256: format!("sha256:{}", "b".repeat(64)),
            canonical_config_sha256: format!("sha256:{}", "d".repeat(64)),
            live_config_sha256: format!("sha256:{}", "e".repeat(64)),
            source_commit: None,
            process_should_run: true,
            snapshots: vec![snapshot.clone()],
        };
        store
            .prepare(&transition, &[Some(b"before".to_vec())])
            .unwrap();
        let loaded = store.load_pending().unwrap().unwrap();
        assert_eq!(loaded.operation_id, "op-1");
        assert_eq!(store.read_snapshot(&loaded, &snapshot).unwrap(), b"before");
        store.commit("op-1", 2).unwrap();
        assert!(store.load_pending().unwrap().is_none());
        assert!(!dir.path().join(TRANSITION_SNAPSHOT_DIR_NAME).exists());
    }

    #[test]
    fn source_manifest_commit_must_match_requested_directory() {
        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        let requested = "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2";
        let other = "0123456789abcdef0123456789abcdef01234567";
        let source_dir = paths.source_dir(requested).unwrap();
        std::fs::create_dir_all(&source_dir).unwrap();
        let manifest = CodexRetryGatewaySourceManifest {
            schema_version: SOURCE_MANIFEST_SCHEMA_VERSION,
            repository: CODEX_RETRY_GATEWAY_REPOSITORY.to_string(),
            commit: other.to_string(),
            verified_main_commit: requested.to_string(),
            verified_at_ms: 1,
            archive_sha256: "a".repeat(64),
            source_sha256: "b".repeat(64),
            file_count: 0,
            total_bytes: 0,
            gateway_entry_rel: "gateway.mjs".to_string(),
            admin_entry_rel: "scripts/admin-lib.mjs".to_string(),
            launch_ui_entry_rel: "scripts/launch-ui.mjs".to_string(),
        };
        write_source_manifest(&source_dir.join("manifest.json"), &manifest).unwrap();

        let error = read_source_manifest(&paths, requested).unwrap_err();
        assert_eq!(error.code(), "CODEX_RETRY_GATEWAY_SOURCE_COMMIT_MISMATCH");
    }
}
