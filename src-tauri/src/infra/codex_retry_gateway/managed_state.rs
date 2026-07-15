use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayError, CodexRetryGatewayRouteTransition, CodexRetryGatewayTransitionStore,
    CODEX_RETRY_GATEWAY_REPOSITORY,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::{
    read_optional_file_with_max_len, write_file_atomic, write_file_atomic_if_changed,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::util::{ensure_path_within_root, normalize_full_sha, normalized_internal_relative_path};

const FEATURE_ROOT_DIR_NAME: &str = "codex-retry-gateway";
const MANAGER_SCHEMA_VERSION: u32 = 1;
const SOURCE_MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANAGER_STATE_MAX_BYTES: usize = 512 * 1024;
const SOURCE_MANIFEST_MAX_BYTES: usize = 256 * 1024;
const TRANSITION_MAX_BYTES: usize = 256 * 1024;

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
        for dir in [
            &self.root,
            &self.sources_dir,
            &self.downloads_dir,
            &self.runtime_dir,
            &self.runtime_config_dir,
            &self.runtime_logs_dir,
            &self.runtime_analytics_dir,
            &self.route_dir,
        ] {
            std::fs::create_dir_all(dir)
                .map_err(|err| format!("failed to create {}: {err}", dir.display()))?;
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
    let path = paths.source_manifest_path(commit)?;
    let bytes = crate::shared::fs::read_file_with_max_len(&path, SOURCE_MANIFEST_MAX_BYTES)?;
    let mut manifest: CodexRetryGatewaySourceManifest = serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    manifest.validate()?;
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

#[derive(Debug, Clone)]
pub(crate) struct FileCodexRetryGatewayTransitionStore {
    path: PathBuf,
}

impl FileCodexRetryGatewayTransitionStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn read_transition(&self) -> AppResult<Option<CodexRetryGatewayRouteTransition>> {
        let Some(bytes) = read_optional_file_with_max_len(&self.path, TRANSITION_MAX_BYTES)? else {
            return Ok(None);
        };
        let transition = serde_json::from_slice(&bytes)
            .map_err(|err| format!("failed to parse {}: {err}", self.path.display()))?;
        Ok(Some(transition))
    }
}

impl CodexRetryGatewayTransitionStore for FileCodexRetryGatewayTransitionStore {
    fn load_pending(&self) -> AppResult<Option<CodexRetryGatewayRouteTransition>> {
        self.read_transition()
    }

    fn prepare(&self, transition: &CodexRetryGatewayRouteTransition) -> AppResult<()> {
        let bytes = serde_json::to_vec_pretty(transition)
            .map_err(|err| format!("failed to serialize transition: {err}"))?;
        let mut bytes = bytes;
        bytes.push(b'\n');
        write_file_atomic(&self.path, &bytes)
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
        }
        let _ = std::fs::remove_file(&self.path);
        Ok(())
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
        let mut state = CodexRetryGatewayManagerState::default();
        state.generation = 7;
        state.active_commit = Some("ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string());
        write_manager_state(&paths, &state).unwrap();
        let read_back = read_manager_state(&paths).unwrap();
        assert_eq!(read_back.generation, 7);
        assert_eq!(read_back.active_commit, state.active_commit);
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
        };
        assert!(record.validate().is_err());
    }

    #[test]
    fn transition_store_prepare_and_commit_is_bounded() {
        let dir = tempdir().unwrap();
        let store = FileCodexRetryGatewayTransitionStore::new(dir.path().join("transition.json"));
        let transition = CodexRetryGatewayRouteTransition {
            schema_version: 1,
            operation_id: "op-1".to_string(),
            operation_kind: CodexRetryGatewayOperationKind::Enable,
            prior_generation: 1,
            target_generation: 2,
            prior_mode: CodexRouteMode::Unproxied,
            target_mode: CodexRouteMode::DirectAio,
            canonical_config_sha256: "a".repeat(64),
            live_config_sha256: "b".repeat(64),
            source_commit: None,
            process_should_run: true,
        };
        store.prepare(&transition).unwrap();
        let loaded = store.load_pending().unwrap().unwrap();
        assert_eq!(loaded.operation_id, "op-1");
        store.commit("op-1", 2).unwrap();
        assert!(store.load_pending().unwrap().is_none());
    }
}
