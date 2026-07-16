//! Usage: Read / patch Codex user-level `config.toml` ($CODEX_HOME/config.toml).

mod parsing;
mod patching;
mod types;

pub use types::{
    CodexConfigPatch, CodexConfigState, CodexConfigTomlState, CodexConfigTomlValidationError,
    CodexConfigTomlValidationResult,
};

use crate::codex_paths;
use crate::shared::fs::{
    is_symlink, read_optional_file_with_max_len, write_file_atomic_if_changed,
};
use crate::shared::time::now_unix_seconds;
use parsing::{make_state_from_bytes, validate_codex_config_toml_raw};
use patching::patch_config_toml;
use std::fs;
use std::path::{Path, PathBuf};
use types::CodexConfigStateMeta;

const CODEX_CONFIG_MAX_BYTES: usize = 1024 * 1024;

fn ensure_codex_config_len(bytes: &[u8], label: &str) -> crate::shared::error::AppResult<()> {
    if bytes.len() > CODEX_CONFIG_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} too large (max {CODEX_CONFIG_MAX_BYTES} bytes)"
        )
        .into());
    }
    Ok(())
}

fn read_optional_codex_config_file(
    path: &Path,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    read_optional_file_with_max_len(path, CODEX_CONFIG_MAX_BYTES)
}

#[derive(Debug)]
pub(crate) struct CodexCliProxyBackupSnapshot {
    manifest_path: PathBuf,
    manifest_existed: bool,
    manifest_bytes: Option<Vec<u8>>,
    backup_path: PathBuf,
    backup_existed: bool,
    backup_bytes: Option<Vec<u8>>,
}

pub(crate) fn sync_codex_cli_proxy_backup_if_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    next_bytes: &[u8],
) -> crate::shared::error::AppResult<Option<CodexCliProxyBackupSnapshot>> {
    ensure_codex_config_len(next_bytes, "codex config backup")?;
    let manifest_path = crate::app_paths::app_data_dir(app)?
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let manifest_snapshot = snapshot_optional_file(&manifest_path)?;
    let Some(backup_path) = super::cli_proxy::backup_file_path_for_enabled_manifest(
        app,
        "codex",
        "codex_config_toml",
        "config.toml",
    )
    .inspect_err(|_err| {
        let _ = restore_optional_file(&manifest_path, &manifest_snapshot);
    })?
    else {
        return Ok(None);
    };

    let backup_snapshot = match snapshot_optional_file(&backup_path) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let _ = restore_optional_file(&manifest_path, &manifest_snapshot);
            return Err(format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}").into());
        }
    };
    let snapshot = CodexCliProxyBackupSnapshot {
        manifest_path,
        manifest_existed: manifest_snapshot.0,
        manifest_bytes: manifest_snapshot.1,
        backup_path,
        backup_existed: backup_snapshot.0,
        backup_bytes: backup_snapshot.1,
    };

    if let Err(err) = write_file_atomic_if_changed(&snapshot.backup_path, next_bytes)
        .map_err(|err| format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}"))
    {
        let _ = restore_codex_cli_proxy_backup_snapshot(&snapshot);
        return Err(err.into());
    }

    Ok(Some(snapshot))
}

fn snapshot_optional_file(path: &Path) -> crate::shared::error::AppResult<(bool, Option<Vec<u8>>)> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(format!(
                    "SEC_INVALID_INPUT: backup target is not a file path={}",
                    path.display()
                )
                .into());
            }
            let bytes = fs::read(path).map_err(|err| {
                format!("failed to snapshot backup target {}: {err}", path.display())
            })?;
            Ok((true, Some(bytes)))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok((false, None)),
        Err(err) => Err(format!("failed to read backup target {}: {err}", path.display()).into()),
    }
}

fn restore_optional_file(
    path: &Path,
    snapshot: &(bool, Option<Vec<u8>>),
) -> crate::shared::error::AppResult<()> {
    match snapshot {
        (true, Some(bytes)) => {
            let _ = write_file_atomic_if_changed(path, bytes)?;
        }
        (false, _) => remove_path_if_exists(path)?,
        (true, None) => {}
    }
    Ok(())
}

pub(crate) fn restore_codex_cli_proxy_backup_snapshot(
    snapshot: &CodexCliProxyBackupSnapshot,
) -> crate::shared::error::AppResult<()> {
    restore_optional_file(
        &snapshot.backup_path,
        &(snapshot.backup_existed, snapshot.backup_bytes.clone()),
    )?;
    restore_optional_file(
        &snapshot.manifest_path,
        &(snapshot.manifest_existed, snapshot.manifest_bytes.clone()),
    )?;
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> crate::shared::error::AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)
            .map_err(|err| format!("failed to remove dir {}: {err}", path.display()).into()),
        Ok(_) => fs::remove_file(path)
            .map_err(|err| format!("failed to remove file {}: {err}", path.display()).into()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("failed to inspect path {}: {err}", path.display()).into()),
    }
}

pub(crate) fn codex_config_next_bytes(
    current: Option<Vec<u8>>,
    patch: CodexConfigPatch,
) -> crate::shared::error::AppResult<Vec<u8>> {
    patch_config_toml(current, patch)
}

pub(crate) fn codex_config_normalize_raw_toml(
    mut toml: String,
) -> crate::shared::error::AppResult<Vec<u8>> {
    ensure_codex_config_len(toml.as_bytes(), "codex config.toml")?;
    let validation = validate_codex_config_toml_raw(&toml);
    if !validation.ok {
        let err = validation.error.unwrap_or(CodexConfigTomlValidationError {
            message: "invalid TOML".to_string(),
            line: None,
            column: None,
        });

        let mut msg = format!("SEC_INVALID_INPUT: invalid config.toml: {}", err.message);
        match (err.line, err.column) {
            (Some(line), Some(column)) => msg.push_str(&format!(" (line {line}, column {column})")),
            (Some(line), None) => msg.push_str(&format!(" (line {line})")),
            _ => {}
        }
        return Err(msg.into());
    }

    if !toml.ends_with('\n') {
        toml.push('\n');
    }
    ensure_codex_config_len(toml.as_bytes(), "codex config.toml")?;
    Ok(toml.into_bytes())
}

pub(crate) fn codex_config_patch_target_provider(
    toml: &str,
) -> crate::shared::error::AppResult<String> {
    crate::infra::codex_provider_sync::codex_provider_target_from_patch_config_text(toml)
}

fn patch_requires_provider_sync(patch: &CodexConfigPatch) -> bool {
    patch.features_remote_compaction.is_some()
}

fn managed_codex_cli_proxy_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Option<crate::infra::cli_proxy::CodexCliProxyState>> {
    let Some(state) = crate::infra::cli_proxy::codex_cli_proxy_state(app)? else {
        return Ok(None);
    };
    if state.manifest_enabled
        || !matches!(
            state.route.route_mode,
            crate::infra::codex_retry_gateway::CodexRouteMode::Unproxied
        )
    {
        return Ok(Some(state));
    }
    Ok(None)
}

struct ManagedCodexProjectionRollback {
    gateway_paths: crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths,
    manifest_path: PathBuf,
    manifest_snapshot: (bool, Option<Vec<u8>>),
    manager_snapshot: (bool, Option<Vec<u8>>),
    runtime_state_snapshot: (bool, Option<Vec<u8>>),
    backup_snapshot: Option<CodexCliProxyBackupSnapshot>,
    active: bool,
}

impl ManagedCodexProjectionRollback {
    fn commit(mut self) {
        self.active = false;
    }

    fn rollback(mut self) -> crate::shared::error::AppResult<()> {
        self.rollback_inner()
    }

    fn rollback_inner(&mut self) -> crate::shared::error::AppResult<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        rollback_managed_codex_projection(
            &self.gateway_paths,
            &self.manifest_path,
            &self.manifest_snapshot,
            &self.manager_snapshot,
            &self.runtime_state_snapshot,
            self.backup_snapshot.as_ref(),
        )
    }
}

impl Drop for ManagedCodexProjectionRollback {
    fn drop(&mut self) {
        if self.active {
            if let Err(error) = self.rollback_inner() {
                tracing::error!(
                    error = %error,
                    "managed Codex projection rollback failed during drop"
                );
            }
        }
    }
}

#[must_use = "managed Codex config transactions must be explicitly committed or rolled back"]
struct LiveConfigSnapshot {
    path: PathBuf,
    contents: (bool, Option<Vec<u8>>),
}

pub(crate) struct CodexConfigMutationTransaction {
    route_transition: Option<crate::infra::cli_proxy::CodexConfigRouteTransitionGuard>,
    projection: Option<ManagedCodexProjectionRollback>,
    provider_sync: Option<crate::infra::codex_provider_sync::CodexProviderSyncRollback>,
    live_config_snapshot: Option<LiveConfigSnapshot>,
    verification_record:
        Option<crate::infra::codex_retry_gateway::CodexRetryGatewayManagedProcessRecord>,
    active: bool,
}

impl CodexConfigMutationTransaction {
    fn new(
        route_transition: Option<crate::infra::cli_proxy::CodexConfigRouteTransitionGuard>,
        projection: ManagedCodexProjectionRollback,
        provider_sync: Option<crate::infra::codex_provider_sync::CodexProviderSyncRollback>,
        live_config_snapshot: Option<LiveConfigSnapshot>,
        verification_record: Option<
            crate::infra::codex_retry_gateway::CodexRetryGatewayManagedProcessRecord,
        >,
    ) -> Self {
        Self {
            route_transition,
            projection: Some(projection),
            provider_sync,
            live_config_snapshot,
            verification_record,
            active: true,
        }
    }

    pub(crate) fn verification_record(
        &self,
    ) -> Option<&crate::infra::codex_retry_gateway::CodexRetryGatewayManagedProcessRecord> {
        self.verification_record.as_ref()
    }

    pub(crate) fn commit(mut self) -> crate::shared::error::AppResult<()> {
        self.commit_inner()
    }

    pub(crate) fn rollback(mut self) -> crate::shared::error::AppResult<()> {
        self.rollback_inner()
    }

    fn commit_inner(&mut self) -> crate::shared::error::AppResult<()> {
        if !self.active {
            return Ok(());
        }
        if let Some(route_transition) = self.route_transition.take() {
            route_transition.commit()?;
        }
        self.active = false;
        if let Some(projection) = self.projection.take() {
            projection.commit();
        }
        if let Some(provider_sync) = self.provider_sync.take() {
            provider_sync.commit();
        }
        self.live_config_snapshot = None;
        self.verification_record = None;
        Ok(())
    }

    fn rollback_inner(&mut self) -> crate::shared::error::AppResult<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let mut errors = Vec::new();
        if let Some(projection) = self.projection.take() {
            if let Err(error) = projection.rollback() {
                errors.push(error.to_string());
            }
        }
        if let Some(provider_sync) = self.provider_sync.take() {
            if let Err(error) = provider_sync.rollback() {
                errors.push(error.to_string());
            }
        } else if let Some(snapshot) = self.live_config_snapshot.take() {
            if let Err(error) = restore_optional_file(&snapshot.path, &snapshot.contents) {
                errors.push(error.to_string());
            }
        }
        if let Some(route_transition) = self.route_transition.take() {
            if let Err(error) = route_transition.rollback() {
                errors.push(error.to_string());
            }
        }
        self.verification_record = None;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "CODEX_CONFIG_MANAGED_ROLLBACK_FAILED: {}",
                errors.join("; ")
            )
            .into())
        }
    }
}

impl Drop for CodexConfigMutationTransaction {
    fn drop(&mut self) {
        if self.active {
            if let Err(error) = self.rollback_inner() {
                tracing::error!(
                    error = %error,
                    "managed Codex config transaction failed to roll back during drop"
                );
            }
        }
    }
}

pub(crate) struct CodexConfigMutationStage {
    pub(crate) state: CodexConfigState,
    pub(crate) transaction: Option<CodexConfigMutationTransaction>,
}

fn write_managed_codex_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &crate::infra::cli_proxy::CodexCliProxyState,
    canonical_bytes: &[u8],
    live_bytes: &[u8],
) -> crate::shared::error::AppResult<(
    ManagedCodexProjectionRollback,
    Option<crate::infra::codex_retry_gateway::CodexRetryGatewayManagedProcessRecord>,
)> {
    let live_text = String::from_utf8(live_bytes.to_vec())
        .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let provider_name =
        crate::infra::codex_provider_sync::codex_provider_target_from_config_text(&live_text)?;
    let gateway_paths =
        crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths::from_app(app)?;
    let manifest_path = crate::app_paths::app_data_dir(app)?
        .join("cli-proxy")
        .join("codex")
        .join("manifest.json");
    let manifest_snapshot = snapshot_optional_file(&manifest_path)?;
    let manager_snapshot = snapshot_optional_file(&gateway_paths.manager_path)?;
    let runtime_state_snapshot = snapshot_optional_file(&gateway_paths.runtime_state_path)?;
    let backup_snapshot = sync_codex_cli_proxy_backup_if_enabled(app, canonical_bytes)?;
    let rollback = ManagedCodexProjectionRollback {
        gateway_paths: gateway_paths.clone(),
        manifest_path,
        manifest_snapshot,
        manager_snapshot,
        runtime_state_snapshot,
        backup_snapshot,
        active: true,
    };
    let mut manifest = state.manifest.clone();
    let mut route = state.route.clone();
    let next_canonical_hash = crate::infra::cli_proxy::sha256_hex(canonical_bytes);
    let next_live_hash = crate::infra::cli_proxy::sha256_hex(live_bytes);
    let changed = route.canonical_config_sha256.as_deref() != Some(next_canonical_hash.as_str())
        || route.live_config_sha256.as_deref() != Some(next_live_hash.as_str());
    if changed {
        route.generation = route.generation.saturating_add(1);
    }
    route.canonical_config_sha256 = Some(next_canonical_hash);
    route.live_config_sha256 = Some(next_live_hash);
    crate::infra::cli_proxy::set_codex_manifest_state(&mut manifest, route);
    manifest.updated_at = now_unix_seconds();
    let verification_record =
        match crate::infra::codex_retry_gateway::update_managed_provider_projection(
            &gateway_paths,
            &provider_name,
        ) {
            Ok(record) => record,
            Err(err) => {
                return match rollback.rollback() {
                    Ok(()) => Err(err),
                    Err(rollback_err) => Err(format!(
                        "CODEX_RETRY_GATEWAY_PROVIDER_ROLLBACK_FAILED: {err}; rollback error: {rollback_err}"
                    )
                    .into()),
                };
            }
        };
    if let Err(err) = crate::infra::cli_proxy::write_manifest(app, "codex", &manifest) {
        return match rollback.rollback() {
            Ok(()) => Err(err),
            Err(rollback_err) => Err(format!(
                "CODEX_RETRY_GATEWAY_PROVIDER_ROLLBACK_FAILED: {err}; rollback error: {rollback_err}"
            )
            .into()),
        };
    }
    Ok((rollback, verification_record))
}

fn rollback_prepared_route_transition<T>(
    route_transition: Option<crate::infra::cli_proxy::CodexConfigRouteTransitionGuard>,
    cause: crate::shared::error::AppError,
) -> crate::shared::error::AppResult<T> {
    let Some(route_transition) = route_transition else {
        return Err(cause);
    };
    match route_transition.rollback() {
        Ok(()) => Err(cause),
        Err(rollback_error) => Err(format!(
            "CODEX_CONFIG_MANAGED_ROLLBACK_FAILED: {cause}; persistent route rollback error: {rollback_error}"
        )
        .into()),
    }
}

fn rollback_managed_codex_projection(
    gateway_paths: &crate::infra::codex_retry_gateway::CodexRetryGatewayManagerPaths,
    manifest_path: &Path,
    manifest_snapshot: &(bool, Option<Vec<u8>>),
    manager_snapshot: &(bool, Option<Vec<u8>>),
    runtime_state_snapshot: &(bool, Option<Vec<u8>>),
    backup_snapshot: Option<&CodexCliProxyBackupSnapshot>,
) -> crate::shared::error::AppResult<()> {
    let mut errors = Vec::new();
    if let Err(error) = restore_optional_file(manifest_path, manifest_snapshot) {
        errors.push(error.to_string());
    }
    if let Err(error) = restore_optional_file(&gateway_paths.manager_path, manager_snapshot) {
        errors.push(error.to_string());
    }
    if let Err(error) =
        restore_optional_file(&gateway_paths.runtime_state_path, runtime_state_snapshot)
    {
        errors.push(error.to_string());
    }
    if let Some(snapshot) = backup_snapshot {
        if let Err(error) = restore_codex_cli_proxy_backup_snapshot(snapshot) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "CODEX_CONFIG_MANAGED_PROJECTION_ROLLBACK_FAILED: {}",
            errors.join("; ")
        )
        .into())
    }
}

fn apply_managed_codex_config_bytes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &crate::infra::cli_proxy::CodexCliProxyState,
    canonical_bytes: Vec<u8>,
    trigger: &str,
) -> crate::shared::error::AppResult<CodexConfigMutationTransaction> {
    ensure_codex_config_len(&canonical_bytes, "codex config canonical bytes")?;
    let config_path = codex_paths::codex_config_toml_path(app)?;
    let live_bytes =
        crate::infra::cli_proxy::project_codex_live_config(app, &state.route, &canonical_bytes)?;
    ensure_codex_config_len(&live_bytes, "codex config live bytes")?;

    let current_live = read_optional_codex_config_file(&config_path)?;
    let current_live_text = optional_config_bytes_to_utf8(current_live.clone())?;
    let projected_live_text = String::from_utf8(live_bytes.clone())
        .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let current_provider =
        crate::infra::codex_provider_sync::codex_provider_target_from_current_config_text(
            &current_live_text,
        )?;
    let target_provider =
        crate::infra::codex_provider_sync::codex_provider_target_from_config_text(
            &projected_live_text,
        )
        .ok();
    let prior_canonical_bytes =
        crate::infra::cli_proxy::current_canonical_codex_config_bytes(app, state)?;
    let mut route_transition =
        crate::infra::cli_proxy::prepare_managed_codex_config_route_transition(
            app,
            state,
            &prior_canonical_bytes,
            current_live.as_deref().unwrap_or_default(),
            &canonical_bytes,
            &live_bytes,
        )?;

    if let Some(target_provider) = target_provider {
        if current_provider != target_provider {
            let sync_result =
                crate::infra::codex_provider_sync::codex_provider_sync_transaction_reversible(
                    app,
                    crate::infra::codex_provider_sync::CodexProviderSyncContext {
                        trigger: trigger.to_string(),
                        target_provider,
                        config_bytes: Some(live_bytes.clone()),
                    },
                    |_| write_managed_codex_manifest(app, state, &canonical_bytes, &live_bytes),
                );
            return match sync_result {
                Ok((_, (projection, verification_record), provider_sync)) => {
                    Ok(CodexConfigMutationTransaction::new(
                        route_transition.take(),
                        projection,
                        provider_sync,
                        None,
                        verification_record,
                    ))
                }
                Err(error) => rollback_prepared_route_transition(route_transition.take(), error),
            };
        }
    }

    let live_snapshot = snapshot_optional_file(&config_path)?;
    if let Err(err) = write_file_atomic_if_changed(&config_path, &live_bytes) {
        let cause = match restore_optional_file(&config_path, &live_snapshot) {
            Ok(()) => err,
            Err(restore_error) => format!(
                "CODEX_CONFIG_MANAGED_ROLLBACK_FAILED: {err}; live config rollback error: {restore_error}"
            )
            .into(),
        };
        return rollback_prepared_route_transition(route_transition.take(), cause);
    }
    match write_managed_codex_manifest(app, state, &canonical_bytes, &live_bytes) {
        Ok((projection, verification_record)) => Ok(CodexConfigMutationTransaction::new(
            route_transition.take(),
            projection,
            None,
            Some(LiveConfigSnapshot {
                path: config_path,
                contents: live_snapshot,
            }),
            verification_record,
        )),
        Err(err) => {
            let cause = match restore_optional_file(&config_path, &live_snapshot) {
                Ok(()) => err,
                Err(restore_error) => format!(
                    "CODEX_CONFIG_MANAGED_ROLLBACK_FAILED: {err}; live config rollback error: {restore_error}"
                )
                .into(),
            };
            rollback_prepared_route_transition(route_transition.take(), cause)
        }
    }
}

fn optional_config_bytes_to_utf8(
    bytes: Option<Vec<u8>>,
) -> crate::shared::error::AppResult<String> {
    match bytes {
        Some(bytes) => String::from_utf8(bytes).map_err(|_| {
            "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8"
                .to_string()
                .into()
        }),
        None => Ok(String::new()),
    }
}

#[cfg(windows)]
fn normalize_path_for_prefix_match(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

#[cfg(windows)]
fn path_is_under_allowed_root(dir: &Path, allowed_root: &Path) -> bool {
    let dir_s = normalize_path_for_prefix_match(dir);
    let root_s = normalize_path_for_prefix_match(allowed_root);
    dir_s == root_s || dir_s.starts_with(&(root_s + "/"))
}

#[cfg(not(windows))]
fn path_is_under_allowed_root(dir: &Path, allowed_root: &Path) -> bool {
    dir.starts_with(allowed_root)
}

pub fn codex_config_get<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<CodexConfigState> {
    let path = codex_paths::codex_config_toml_path(app)?;
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    let user_default_path = codex_paths::codex_home_dir_user_default(app)?.join("config.toml");
    let user_default_dir = user_default_path
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let follow_path = codex_paths::codex_home_dir_follow_env_or_default(app)?.join("config.toml");
    let follow_dir = follow_path.parent().unwrap_or(Path::new("")).to_path_buf();
    let bytes = if let Some(state) = managed_codex_cli_proxy_state(app)? {
        Some(crate::infra::cli_proxy::current_canonical_codex_config_bytes(app, &state)?)
    } else {
        read_optional_codex_config_file(&path)?
    };

    let can_open_config_dir = crate::app_paths::home_dir(app)
        .ok()
        .map(|home| {
            let allowed_root = home.join(".codex");
            path_is_under_allowed_root(&dir, &allowed_root)
                || follow_dir == dir
                || codex_paths::configured_codex_home_dir(app)
                    .as_ref()
                    .is_some_and(|configured_dir| configured_dir == &dir)
        })
        .unwrap_or(false);

    make_state_from_bytes(
        CodexConfigStateMeta {
            config_dir: dir.to_string_lossy().to_string(),
            config_path: path.to_string_lossy().to_string(),
            user_home_default_dir: user_default_dir.to_string_lossy().to_string(),
            user_home_default_path: user_default_path.to_string_lossy().to_string(),
            follow_codex_home_dir: follow_dir.to_string_lossy().to_string(),
            follow_codex_home_path: follow_path.to_string_lossy().to_string(),
            can_open_config_dir,
        },
        bytes,
    )
}

pub fn codex_config_toml_get_raw<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<CodexConfigTomlState> {
    let path = codex_paths::codex_config_toml_path(app)?;
    let bytes = if let Some(state) = managed_codex_cli_proxy_state(app)? {
        Some(crate::infra::cli_proxy::current_canonical_codex_config_bytes(app, &state)?)
    } else {
        read_optional_codex_config_file(&path)?
    };
    let exists = bytes.as_ref().is_some_and(|bytes| !bytes.is_empty());

    let toml = match bytes {
        Some(bytes) => String::from_utf8(bytes)
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?,
        None => String::new(),
    };

    Ok(CodexConfigTomlState {
        config_path: path.to_string_lossy().to_string(),
        exists,
        toml,
    })
}

pub fn codex_config_toml_validate_raw(
    toml: String,
) -> crate::shared::error::AppResult<CodexConfigTomlValidationResult> {
    Ok(validate_codex_config_toml_raw(&toml))
}

pub(crate) fn codex_config_toml_set_raw_staged<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    toml: String,
) -> crate::shared::error::AppResult<CodexConfigMutationStage> {
    let path = codex_paths::codex_config_toml_path(app)?;
    if path.exists() && is_symlink(&path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            path.display()
        )
        .into());
    }

    let bytes = codex_config_normalize_raw_toml(toml)?;
    let transaction = if let Some(state) = managed_codex_cli_proxy_state(app)? {
        Some(apply_managed_codex_config_bytes(
            app,
            &state,
            bytes,
            "codex_config_toml_set_raw",
        )?)
    } else if let Err(err) = write_file_atomic_if_changed(&path, &bytes) {
        return Err(err);
    } else {
        None
    };
    Ok(CodexConfigMutationStage {
        state: codex_config_get(app)?,
        transaction,
    })
}

pub fn codex_config_toml_set_raw<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    toml: String,
) -> crate::shared::error::AppResult<CodexConfigState> {
    finish_synchronous_config_mutation(codex_config_toml_set_raw_staged(app, toml)?)
}

pub(crate) fn codex_config_set_staged<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    patch: CodexConfigPatch,
) -> crate::shared::error::AppResult<CodexConfigMutationStage> {
    let path = codex_paths::codex_config_toml_path(app)?;
    if path.exists() && is_symlink(&path)? {
        return Err(format!(
            "SEC_INVALID_INPUT: refusing to modify symlink path={}",
            path.display()
        )
        .into());
    }

    let managed_state = managed_codex_cli_proxy_state(app)?;
    let current = if let Some(state) = managed_state.as_ref() {
        Some(crate::infra::cli_proxy::current_canonical_codex_config_bytes(app, state)?)
    } else {
        read_optional_codex_config_file(&path)?
    };
    let requires_provider_sync = patch_requires_provider_sync(&patch);
    let next = codex_config_next_bytes(current, patch)?;
    ensure_codex_config_len(&next, "codex config.toml")?;
    let transaction = if let Some(state) = managed_state.as_ref() {
        Some(apply_managed_codex_config_bytes(
            app,
            state,
            next,
            "codex_config_set",
        )?)
    } else if requires_provider_sync {
        let next_text = String::from_utf8(next.clone())
            .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
        let target_provider = codex_config_patch_target_provider(&next_text)?;
        crate::infra::codex_provider_sync::codex_provider_sync(
            app,
            crate::infra::codex_provider_sync::CodexProviderSyncContext {
                trigger: "codex_config_set".to_string(),
                target_provider,
                config_bytes: Some(next),
            },
        )?;
        None
    } else if let Err(err) = write_file_atomic_if_changed(&path, &next) {
        return Err(err);
    } else {
        None
    };

    Ok(CodexConfigMutationStage {
        state: codex_config_get(app)?,
        transaction,
    })
}

pub fn codex_config_set<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    patch: CodexConfigPatch,
) -> crate::shared::error::AppResult<CodexConfigState> {
    finish_synchronous_config_mutation(codex_config_set_staged(app, patch)?)
}

fn finish_synchronous_config_mutation(
    mut staged: CodexConfigMutationStage,
) -> crate::shared::error::AppResult<CodexConfigState> {
    if let Some(transaction) = staged.transaction.take() {
        if transaction.verification_record().is_some() {
            transaction.rollback()?;
            return Err(crate::shared::error::AppError::new(
                "CODEX_CONFIG_MANAGED_ASYNC_VERIFY_REQUIRED",
                "managed provider changes require the async Codex config coordinator",
            ));
        }
        transaction.commit()?;
    }
    Ok(staged.state)
}

#[cfg(test)]
mod tests;
