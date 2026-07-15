//! Usage: Manage local CLI proxy configuration files (infra adapter).

mod claude;
mod codex;
mod gemini;

use crate::app_paths;
use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayOperationKind, CodexRetryGatewayRouteTransition,
    CodexRetryGatewayTransitionStore, CodexRouteMode,
};
use crate::shared::fs::{
    read_file_with_max_len, read_optional_file_with_max_len, write_file_atomic,
    write_file_atomic_if_changed,
};
use crate::shared::time::now_unix_seconds;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANAGED_BY: &str = "aio-coding-hub";
pub(crate) const PLACEHOLDER_KEY: &str = "aio-coding-hub";
const CLI_PROXY_MANIFEST_MAX_BYTES: usize = 256 * 1024;
pub(super) const CLI_PROXY_FILE_MAX_BYTES: usize = 1024 * 1024;

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

// -- Public types -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CliProxyStatus {
    pub cli_key: String,
    pub enabled: bool,
    pub base_origin: Option<String>,
    pub current_gateway_origin: Option<String>,
    pub applied_to_current_gateway: Option<bool>,
    pub generation: Option<u64>,
    pub route_mode: Option<CodexRouteMode>,
    pub desired_enabled: Option<bool>,
    pub aio_origin: Option<String>,
    pub guarded_origin: Option<String>,
    pub effective_origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CliProxyResult {
    pub trace_id: String,
    pub cli_key: String,
    pub enabled: bool,
    pub ok: bool,
    pub error_code: Option<String>,
    pub message: String,
    pub base_origin: Option<String>,
}

impl CliProxyResult {
    fn success(
        trace_id: String,
        cli_key: &str,
        enabled: bool,
        message: String,
        base_origin: Option<String>,
    ) -> Self {
        Self {
            trace_id,
            cli_key: cli_key.to_string(),
            enabled,
            ok: true,
            error_code: None,
            message,
            base_origin,
        }
    }

    fn failure(
        trace_id: String,
        cli_key: &str,
        enabled: bool,
        error_code: &str,
        message: String,
        base_origin: Option<String>,
    ) -> Self {
        Self {
            trace_id,
            cli_key: cli_key.to_string(),
            enabled,
            ok: false,
            error_code: Some(error_code.to_string()),
            message,
            base_origin,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CodexExternalEnablePlan {
    pub generation: u64,
    pub canonical_config_sha256: String,
    pub live_config_sha256: String,
    pub projected_live_config_sha256: String,
    pub cli_proxy_enable_required: bool,
    pub route_change_required: bool,
    pub current_route_mode: CodexRouteMode,
    pub desired_enabled: bool,
    pub aio_origin: Option<String>,
    pub guarded_origin: Option<String>,
    pub effective_origin: Option<String>,
    pub target_guarded_origin: String,
    pub provider_sync: crate::infra::codex_retry_gateway::CodexProviderSyncPlan,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CodexRouteVerifyResult {
    pub generation: u64,
    pub cli_proxy_enabled: bool,
    pub route_mode: CodexRouteMode,
    pub desired_enabled: bool,
    pub aio_origin: Option<String>,
    pub guarded_origin: Option<String>,
    pub effective_origin: Option<String>,
    pub canonical_config_sha256: String,
    pub live_config_sha256: String,
    pub projected_live_config_sha256: String,
    pub auth_projection_managed: bool,
    pub live_matches_projection: bool,
    pub auth_matches_projection: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CodexRouteApplyResult {
    pub transition_operation_id: String,
    pub route: CodexRouteVerifyResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_sync: Option<crate::infra::codex_provider_sync::CodexProviderSyncResult>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CodexRouteReconcileResult {
    pub route: CodexRouteVerifyResult,
    pub pending_transition_reconciled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexGuardedRouteApplyRequest {
    pub expected_generation: u64,
    pub expected_canonical_sha256: String,
    pub aio_origin: String,
    pub guarded_origin: String,
    pub desired_enabled: bool,
    pub source_commit: Option<String>,
    pub process_should_run: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexDirectAioRouteApplyRequest {
    pub expected_generation: u64,
    pub expected_canonical_sha256: String,
    pub aio_origin: String,
    pub desired_enabled: bool,
    pub source_commit: Option<String>,
    pub process_should_run: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexRestoreUnproxiedRouteRequest {
    pub expected_generation: u64,
    pub expected_canonical_sha256: String,
    pub aio_origin: Option<String>,
    pub desired_enabled: bool,
    pub keep_cli_proxy_enabled: bool,
    pub source_commit: Option<String>,
    pub process_should_run: bool,
}

// -- Internal types ---------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BackupFileEntry {
    kind: String,
    path: String,
    existed: bool,
    backup_rel: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct CodexCliProxyManifestState {
    pub generation: u64,
    pub route_mode: CodexRouteMode,
    pub desired_enabled: bool,
    pub aio_origin: Option<String>,
    pub guarded_origin: Option<String>,
    pub canonical_config_sha256: Option<String>,
    pub live_config_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CliProxyManifest {
    pub schema_version: u32,
    pub managed_by: String,
    pub cli_key: String,
    pub enabled: bool,
    pub base_origin: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub files: Vec<BackupFileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<CodexCliProxyManifestState>,
}

#[derive(Debug, Clone)]
struct TargetFile {
    kind: &'static str,
    path: PathBuf,
    backup_name: &'static str,
}

#[derive(Debug, Clone)]
struct PendingBackupEntry {
    kind: String,
    path: PathBuf,
    backup_name: &'static str,
    existed: bool,
    backup_bytes: Option<Vec<u8>>,
}

pub(crate) fn codex_oauth_compatible_proxy_mode<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> bool {
    crate::settings::read(app)
        .map(|settings| settings.codex_oauth_compatible_proxy_mode)
        .unwrap_or(false)
}

fn should_skip_manifest_entry_for_current_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    kind: &str,
) -> bool {
    cli_key == "codex" && kind == "codex_auth_json" && codex_oauth_compatible_proxy_mode(app)
}

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    existed: bool,
    bytes: Option<Vec<u8>>,
}

// -- Shared helpers ---------------------------------------------------------

fn new_trace_id(prefix: &str) -> String {
    let ts = now_unix_seconds();
    let seq = TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{ts}-{seq}")
}

fn validate_cli_key(cli_key: &str) -> crate::shared::error::AppResult<()> {
    crate::shared::cli_key::validate_cli_key(cli_key)
}

fn home_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    crate::app_paths::home_dir(app)
}

fn cli_proxy_root_dir<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(app_paths::app_data_dir(app)?
        .join("cli-proxy")
        .join(cli_key))
}

fn cli_proxy_files_dir(root: &Path) -> PathBuf {
    root.join("files")
}

fn cli_proxy_safety_dir(root: &Path) -> PathBuf {
    root.join("restore-safety")
}

fn cli_proxy_manifest_path(root: &Path) -> PathBuf {
    root.join("manifest.json")
}

fn ensure_cli_proxy_bytes_len(
    bytes: &[u8],
    max_len: usize,
    label: &str,
) -> crate::shared::error::AppResult<()> {
    if bytes.len() > max_len {
        return Err(format!("SEC_INVALID_INPUT: {label} too large (max {max_len} bytes)").into());
    }
    Ok(())
}

pub(super) fn read_optional_cli_proxy_file(
    path: &Path,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    read_optional_file_with_max_len(path, CLI_PROXY_FILE_MAX_BYTES)
}

pub(super) fn read_cli_proxy_file(path: &Path) -> crate::shared::error::AppResult<Vec<u8>> {
    read_file_with_max_len(path, CLI_PROXY_FILE_MAX_BYTES)
}

pub(super) fn write_cli_proxy_file_atomic(
    path: &Path,
    bytes: &[u8],
) -> crate::shared::error::AppResult<()> {
    ensure_cli_proxy_bytes_len(
        bytes,
        CLI_PROXY_FILE_MAX_BYTES,
        &format!("CLI proxy file {}", path.display()),
    )?;
    write_file_atomic(path, bytes)
}

pub(crate) fn read_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<Option<CliProxyManifest>> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let path = cli_proxy_manifest_path(&root);
    let Some(content) = read_optional_file_with_max_len(&path, CLI_PROXY_MANIFEST_MAX_BYTES)?
    else {
        return Ok(None);
    };

    let manifest: CliProxyManifest = serde_json::from_slice(&content)
        .map_err(|e| format!("failed to parse manifest.json: {e}"))?;

    if manifest.managed_by != MANAGED_BY {
        return Err(format!(
            "manifest managed_by mismatch: expected {MANAGED_BY}, got {}",
            manifest.managed_by
        )
        .into());
    }

    Ok(Some(manifest))
}

pub(crate) fn write_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("failed to create {}: {e}", root.display()))?;
    let path = cli_proxy_manifest_path(&root);

    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| format!("failed to serialize manifest.json: {e}"))?;
    ensure_cli_proxy_bytes_len(&bytes, CLI_PROXY_MANIFEST_MAX_BYTES, "CLI proxy manifest")?;
    write_file_atomic(&path, &bytes)?;
    Ok(())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn codex_manifest_state_from_manifest(manifest: &CliProxyManifest) -> CodexCliProxyManifestState {
    manifest
        .codex
        .clone()
        .unwrap_or_else(|| CodexCliProxyManifestState {
            generation: 0,
            route_mode: if manifest.enabled {
                CodexRouteMode::DirectAio
            } else {
                CodexRouteMode::Unproxied
            },
            desired_enabled: false,
            aio_origin: manifest.base_origin.clone(),
            guarded_origin: None,
            canonical_config_sha256: None,
            live_config_sha256: None,
        })
}

fn codex_effective_origin(state: &CodexCliProxyManifestState) -> Option<String> {
    match state.route_mode {
        CodexRouteMode::Unproxied => None,
        CodexRouteMode::DirectAio => state
            .aio_origin
            .as_ref()
            .map(|origin| codex::normalize_origin(origin)),
        CodexRouteMode::Guarded => state
            .guarded_origin
            .as_ref()
            .map(|origin| codex::normalize_origin(origin)),
    }
}

pub(crate) fn project_codex_live_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    route: &CodexCliProxyManifestState,
    canonical_bytes: &[u8],
) -> crate::shared::error::AppResult<Vec<u8>> {
    codex::project_codex_config_toml(
        canonical_bytes,
        route.route_mode,
        route.aio_origin.as_deref(),
        route.guarded_origin.as_deref(),
        codex_oauth_compatible_proxy_mode(app),
        codex::CodexConfigPlatform::current(),
    )
}

pub(crate) fn set_codex_manifest_state(
    manifest: &mut CliProxyManifest,
    state: CodexCliProxyManifestState,
) {
    manifest.base_origin = state
        .aio_origin
        .clone()
        .or_else(|| manifest.base_origin.clone());
    manifest.codex = Some(state);
}

pub(crate) struct CodexCliProxyState {
    pub manifest_enabled: bool,
    pub manifest: CliProxyManifest,
    pub route: CodexCliProxyManifestState,
}

pub(crate) fn codex_cli_proxy_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Option<CodexCliProxyState>> {
    let Some(manifest) = read_manifest(app, "codex")? else {
        return Ok(None);
    };
    let route = codex_manifest_state_from_manifest(&manifest);
    Ok(Some(CodexCliProxyState {
        manifest_enabled: manifest.enabled,
        manifest,
        route,
    }))
}

pub(crate) fn current_canonical_codex_config_bytes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &CodexCliProxyState,
) -> crate::shared::error::AppResult<Vec<u8>> {
    let config_path = codex::codex_config_path(app)?;
    let live_bytes = read_optional_cli_proxy_file(&config_path)?;
    let Some(backup_path) =
        backup_file_path_for_manifest(app, &state.manifest, "codex_config_toml")?
    else {
        return Ok(live_bytes.unwrap_or_default());
    };
    let backup_bytes = read_cli_proxy_file(&backup_path)
        .map_err(|err| format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}"))?;
    codex::merge_restore_codex_config_toml_bytes(live_bytes.as_deref(), &backup_bytes)
}

pub(crate) fn current_canonical_codex_auth_bytes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &CodexCliProxyState,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    if codex_oauth_compatible_proxy_mode(app) {
        return Ok(None);
    }
    let auth_path = codex::codex_auth_path(app)?;
    let live_bytes = read_optional_cli_proxy_file(&auth_path)?;
    let Some(backup_path) = backup_file_path_for_manifest(app, &state.manifest, "codex_auth_json")?
    else {
        return Ok(live_bytes);
    };
    let backup_bytes = read_cli_proxy_file(&backup_path)
        .map_err(|err| format!("CODEX_CONFIG_BACKUP_REFRESH_FAILED: {err}"))?;
    codex::merge_restore_codex_auth_json_bytes(live_bytes.as_deref(), &backup_bytes).map(Some)
}

#[derive(Debug, Clone)]
enum CodexAuthProjection {
    Unmanaged,
    Present(Vec<u8>),
    Absent,
}

fn validate_http_origin(origin: &str, label: &str) -> crate::shared::error::AppResult<String> {
    let normalized = codex::normalize_origin(origin);
    if !normalized.starts_with("http://") && !normalized.starts_with("https://") {
        return Err(
            format!("SEC_INVALID_INPUT: {label} must start with http:// or https://").into(),
        );
    }
    Ok(normalized)
}

fn should_manage_codex_route_state(state: &CodexCliProxyState) -> bool {
    state.manifest_enabled || !matches!(state.route.route_mode, CodexRouteMode::Unproxied)
}

fn reload_codex_route_state_after_rebind<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    aio_origin: Option<&str>,
) -> crate::shared::error::AppResult<Option<CodexCliProxyState>> {
    let Some(state) = codex_cli_proxy_state(app)? else {
        return Ok(None);
    };
    if !manifest_target_paths_changed(app, &state.manifest)? {
        return Ok(Some(state));
    }

    let derived_aio_origin = aio_origin
        .map(ToString::to_string)
        .or_else(|| state.route.aio_origin.clone())
        .or_else(|| state.manifest.base_origin.clone());
    let Some(aio_origin) = derived_aio_origin.as_deref() else {
        return Ok(Some(state));
    };
    let result = codex::rebind_codex_manifest_after_home_change(
        app,
        state.manifest.clone(),
        aio_origin,
        false,
        new_trace_id("codex-route-rebind"),
    )?;
    if !result.ok {
        return Err(format!(
            "{}: {}",
            result
                .error_code
                .as_deref()
                .unwrap_or("CLI_PROXY_REBIND_APPLY_FAILED"),
            result.message
        )
        .into());
    }
    codex_cli_proxy_state(app)
}

fn current_unmanaged_codex_config_bytes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<u8>> {
    read_optional_cli_proxy_file(&codex::codex_config_path(app)?)?
        .map_or_else(|| Ok(Vec::new()), Ok)
}

fn current_unmanaged_codex_auth_bytes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    if codex_oauth_compatible_proxy_mode(app) {
        return Ok(None);
    }
    read_optional_cli_proxy_file(&codex::codex_auth_path(app)?)
}

fn build_codex_auth_projection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    route_mode: CodexRouteMode,
    canonical_auth_bytes: Option<&[u8]>,
) -> crate::shared::error::AppResult<CodexAuthProjection> {
    if codex_oauth_compatible_proxy_mode(app) {
        return Ok(CodexAuthProjection::Unmanaged);
    }

    match route_mode {
        CodexRouteMode::Unproxied => Ok(match canonical_auth_bytes {
            Some(bytes) => CodexAuthProjection::Present(bytes.to_vec()),
            None => CodexAuthProjection::Absent,
        }),
        CodexRouteMode::DirectAio | CodexRouteMode::Guarded => Ok(CodexAuthProjection::Present(
            codex::build_codex_auth_json(canonical_auth_bytes.map(|bytes| bytes.to_vec()))?,
        )),
    }
}

fn write_codex_auth_projection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    projection: &CodexAuthProjection,
) -> crate::shared::error::AppResult<()> {
    let auth_path = codex::codex_auth_path(app)?;
    match projection {
        CodexAuthProjection::Unmanaged => Ok(()),
        CodexAuthProjection::Present(bytes) => write_cli_proxy_file_atomic(&auth_path, bytes),
        CodexAuthProjection::Absent => {
            if auth_path.exists() {
                std::fs::remove_file(&auth_path)
                    .map_err(|err| format!("failed to remove {}: {err}", auth_path.display()))?;
            }
            Ok(())
        }
    }
}

fn codex_auth_matches_projection<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    projection: &CodexAuthProjection,
) -> crate::shared::error::AppResult<bool> {
    let auth_path = codex::codex_auth_path(app)?;
    let current = read_optional_cli_proxy_file(&auth_path)?;
    Ok(match projection {
        CodexAuthProjection::Unmanaged => true,
        CodexAuthProjection::Present(bytes) => current.as_deref() == Some(bytes.as_slice()),
        CodexAuthProjection::Absent => current.is_none(),
    })
}

fn build_codex_route_manifest_state(
    prior_state: &CodexCliProxyManifestState,
    generation: u64,
    route_mode: CodexRouteMode,
    desired_enabled: bool,
    aio_origin: Option<String>,
    guarded_origin: Option<String>,
    canonical_config_sha256: String,
    live_config_sha256: String,
) -> CodexCliProxyManifestState {
    let mut next = prior_state.clone();
    next.generation = generation;
    next.route_mode = route_mode;
    next.desired_enabled = desired_enabled;
    next.aio_origin = aio_origin;
    next.guarded_origin = guarded_origin;
    next.canonical_config_sha256 = Some(canonical_config_sha256);
    next.live_config_sha256 = Some(live_config_sha256);
    next
}

fn codex_route_verify_from_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest_enabled: bool,
    route_state: &CodexCliProxyManifestState,
    canonical_bytes: &[u8],
) -> crate::shared::error::AppResult<CodexRouteVerifyResult> {
    let canonical_config_sha256 = sha256_hex(canonical_bytes);
    let projected_live_bytes = project_codex_live_config(app, route_state, canonical_bytes)?;
    let projected_live_config_sha256 = sha256_hex(&projected_live_bytes);
    let live_bytes =
        read_optional_cli_proxy_file(&codex::codex_config_path(app)?)?.unwrap_or_default();
    let live_config_sha256 = sha256_hex(&live_bytes);
    let canonical_auth_bytes = if let Some(state) = codex_cli_proxy_state(app)? {
        current_canonical_codex_auth_bytes(app, &state)?
    } else {
        current_unmanaged_codex_auth_bytes(app)?
    };
    let auth_projection =
        build_codex_auth_projection(app, route_state.route_mode, canonical_auth_bytes.as_deref())?;
    let auth_matches_projection = codex_auth_matches_projection(app, &auth_projection)?;

    Ok(CodexRouteVerifyResult {
        generation: route_state.generation,
        cli_proxy_enabled: manifest_enabled,
        route_mode: route_state.route_mode,
        desired_enabled: route_state.desired_enabled,
        aio_origin: route_state.aio_origin.clone(),
        guarded_origin: route_state.guarded_origin.clone(),
        effective_origin: codex_effective_origin(route_state),
        canonical_config_sha256,
        live_config_sha256,
        projected_live_config_sha256,
        auth_projection_managed: !matches!(auth_projection, CodexAuthProjection::Unmanaged),
        live_matches_projection: live_bytes == projected_live_bytes,
        auth_matches_projection,
    })
}

#[allow(dead_code)]
pub(crate) fn codex_route_transition_baseline<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<(CodexCliProxyManifestState, String, String)> {
    let Some(state) = codex_cli_proxy_state(app)? else {
        let config_path = codex::codex_config_path(app)?;
        let live_bytes = read_optional_cli_proxy_file(&config_path)?.unwrap_or_default();
        let live_hash = sha256_hex(&live_bytes);
        return Ok((
            CodexCliProxyManifestState {
                generation: 0,
                route_mode: CodexRouteMode::Unproxied,
                desired_enabled: false,
                aio_origin: None,
                guarded_origin: None,
                canonical_config_sha256: Some(live_hash.clone()),
                live_config_sha256: Some(live_hash.clone()),
            },
            live_hash.clone(),
            live_hash,
        ));
    };
    let canonical_bytes = current_canonical_codex_config_bytes(app, &state)?;
    let live_bytes =
        read_optional_cli_proxy_file(&codex::codex_config_path(app)?)?.unwrap_or_default();
    Ok((
        state.route,
        sha256_hex(&canonical_bytes),
        sha256_hex(&live_bytes),
    ))
}

pub(crate) fn reject_stale_codex_route_state(
    expected_generation: u64,
    expected_canonical_sha256: &str,
    current_generation: u64,
    current_canonical_sha256: &str,
) -> crate::shared::error::AppResult<()> {
    if expected_generation != current_generation {
        return Err(format!(
            "CLI_PROXY_STALE_ROUTE_GENERATION: expected generation {}, got {}",
            expected_generation, current_generation
        )
        .into());
    }
    if expected_canonical_sha256 != current_canonical_sha256 {
        return Err(format!(
            "CLI_PROXY_STALE_ROUTE_HASH: expected canonical hash {}, got {}",
            expected_canonical_sha256, current_canonical_sha256
        )
        .into());
    }
    Ok(())
}

pub(crate) fn prepare_codex_route_transition<S: CodexRetryGatewayTransitionStore>(
    store: &S,
    operation_kind: CodexRetryGatewayOperationKind,
    prior_state: &CodexCliProxyManifestState,
    target_mode: CodexRouteMode,
    canonical_config_sha256: String,
    live_config_sha256: String,
    source_commit: Option<String>,
    process_should_run: bool,
) -> crate::shared::error::AppResult<CodexRetryGatewayRouteTransition> {
    let transition = CodexRetryGatewayRouteTransition {
        schema_version: 1,
        operation_id: new_trace_id("codex-route"),
        operation_kind,
        prior_generation: prior_state.generation,
        target_generation: prior_state.generation.saturating_add(1),
        prior_mode: prior_state.route_mode,
        target_mode,
        canonical_config_sha256,
        live_config_sha256,
        source_commit,
        process_should_run,
    };
    store.prepare(&transition)?;
    Ok(transition)
}

pub(crate) fn commit_codex_route_transition<S: CodexRetryGatewayTransitionStore>(
    store: &S,
    transition: &CodexRetryGatewayRouteTransition,
) -> crate::shared::error::AppResult<()> {
    store.commit(&transition.operation_id, transition.target_generation)
}

pub(crate) fn clear_codex_route_transition<S: CodexRetryGatewayTransitionStore>(
    store: &S,
    transition: &CodexRetryGatewayRouteTransition,
) -> crate::shared::error::AppResult<()> {
    store.clear(&transition.operation_id)
}

pub(crate) fn reconcile_codex_route_transition<S: CodexRetryGatewayTransitionStore>(
    store: &S,
    current_generation: u64,
    current_route_mode: CodexRouteMode,
    current_live_config_sha256: &str,
) -> crate::shared::error::AppResult<Option<bool>> {
    let Some(transition) = store.load_pending()? else {
        return Ok(None);
    };
    if transition.target_generation == current_generation
        && transition.target_mode == current_route_mode
        && transition.live_config_sha256 == current_live_config_sha256
    {
        store.commit(&transition.operation_id, transition.target_generation)?;
        return Ok(Some(true));
    }
    store.clear(&transition.operation_id)?;
    Ok(Some(false))
}

fn update_codex_manifest_state_for_route<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &mut CliProxyManifest,
    route_mode: CodexRouteMode,
    desired_enabled: bool,
) -> crate::shared::error::AppResult<()> {
    if manifest.cli_key != "codex" {
        return Ok(());
    }

    let config_path = codex::codex_config_path(app)?;
    let live_bytes = read_optional_cli_proxy_file(&config_path)?.unwrap_or_default();
    let canonical_bytes = if manifest.enabled {
        if let Some(backup_path) =
            backup_file_path_for_manifest(app, manifest, "codex_config_toml")?
        {
            let backup_bytes = read_cli_proxy_file(&backup_path)?;
            codex::merge_restore_codex_config_toml_bytes(Some(&live_bytes), &backup_bytes)?
        } else {
            live_bytes.clone()
        }
    } else {
        live_bytes.clone()
    };

    let mut state = codex_manifest_state_from_manifest(manifest);
    let next_canonical_hash = sha256_hex(&canonical_bytes);
    let next_live_hash = sha256_hex(&live_bytes);
    let changed = state.route_mode != route_mode
        || state.desired_enabled != desired_enabled
        || state.aio_origin != manifest.base_origin
        || state.canonical_config_sha256.as_deref() != Some(next_canonical_hash.as_str())
        || state.live_config_sha256.as_deref() != Some(next_live_hash.as_str());
    if changed {
        state.generation = state.generation.saturating_add(1);
    }
    state.route_mode = route_mode;
    state.desired_enabled = desired_enabled;
    state.aio_origin = manifest.base_origin.clone();
    state.canonical_config_sha256 = Some(next_canonical_hash);
    state.live_config_sha256 = Some(next_live_hash);
    set_codex_manifest_state(manifest, state);
    Ok(())
}

fn codex_route_manifest_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<PathBuf> {
    Ok(cli_proxy_manifest_path(&cli_proxy_root_dir(app, "codex")?))
}

fn codex_route_provider_sync_trigger(
    operation_kind: CodexRetryGatewayOperationKind,
) -> &'static str {
    match operation_kind {
        CodexRetryGatewayOperationKind::Enable => "route_enable",
        CodexRetryGatewayOperationKind::DisableGateway => "route_direct_aio",
        CodexRetryGatewayOperationKind::DisableCliProxy => "route_restore_unproxied",
        CodexRetryGatewayOperationKind::Update => "route_update",
        CodexRetryGatewayOperationKind::Recover => "route_recover",
        CodexRetryGatewayOperationKind::Uninstall => "route_uninstall",
        CodexRetryGatewayOperationKind::Startup => "route_startup",
        CodexRetryGatewayOperationKind::Shutdown => "route_shutdown",
        CodexRetryGatewayOperationKind::ProviderModeChange => "route_provider_mode_change",
        CodexRetryGatewayOperationKind::ExternalRestore => "route_external_restore",
    }
}

fn codex_route_context<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    aio_origin: Option<&str>,
) -> crate::shared::error::AppResult<(
    Option<CodexCliProxyState>,
    CodexCliProxyManifestState,
    Vec<u8>,
    Option<Vec<u8>>,
    String,
)> {
    let state = reload_codex_route_state_after_rebind(app, aio_origin)?;
    let baseline = if let Some(state) = state.as_ref() {
        state.route.clone()
    } else {
        CodexCliProxyManifestState {
            generation: 0,
            route_mode: CodexRouteMode::Unproxied,
            desired_enabled: false,
            aio_origin: aio_origin.map(ToString::to_string),
            guarded_origin: None,
            canonical_config_sha256: None,
            live_config_sha256: None,
        }
    };
    let canonical_config = if let Some(state) = state.as_ref() {
        current_canonical_codex_config_bytes(app, state)?
    } else {
        current_unmanaged_codex_config_bytes(app)?
    };
    let canonical_auth = if let Some(state) = state.as_ref() {
        current_canonical_codex_auth_bytes(app, state)?
    } else {
        current_unmanaged_codex_auth_bytes(app)?
    };
    let canonical_sha256 = sha256_hex(&canonical_config);
    Ok((
        state,
        baseline,
        canonical_config,
        canonical_auth,
        canonical_sha256,
    ))
}

fn restore_codex_route_snapshots(
    target_snapshots: &[FileSnapshot],
    backup_snapshots: &[FileSnapshot],
    manifest_snapshot: &FileSnapshot,
) -> crate::shared::error::AppResult<()> {
    restore_file_snapshots(target_snapshots)?;
    restore_file_snapshots(backup_snapshots)?;
    restore_file_snapshots(std::slice::from_ref(manifest_snapshot))?;
    Ok(())
}

fn codex_route_write_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    mut manifest: CliProxyManifest,
    manifest_enabled: bool,
    route_state: CodexCliProxyManifestState,
) -> crate::shared::error::AppResult<()> {
    manifest.enabled = manifest_enabled;
    manifest.updated_at = now_unix_seconds();
    set_codex_manifest_state(&mut manifest, route_state);
    write_manifest(app, "codex", &manifest)
}

struct CodexRouteWritePlan {
    manifest: CliProxyManifest,
    route_state: CodexCliProxyManifestState,
    manifest_enabled: bool,
    live_bytes: Vec<u8>,
    auth_projection: CodexAuthProjection,
    provider_sync_plan: crate::infra::codex_retry_gateway::CodexProviderSyncPlan,
}

fn codex_route_provider_sync_plan(
    current_live_text: &str,
    target_live_text: &str,
    route_mode: CodexRouteMode,
) -> crate::shared::error::AppResult<crate::infra::codex_retry_gateway::CodexProviderSyncPlan> {
    match route_mode {
        CodexRouteMode::Unproxied => {
            let target_provider =
                crate::infra::codex_provider_sync::codex_provider_identity_from_config_text(
                    target_live_text,
                )?;
            crate::infra::codex_provider_sync::codex_provider_sync_plan_for_trusted_target(
                current_live_text,
                &target_provider,
            )
        }
        CodexRouteMode::DirectAio | CodexRouteMode::Guarded => {
            crate::infra::codex_provider_sync::codex_provider_sync_plan_for_config_text(
                current_live_text,
                target_live_text,
            )
        }
    }
}

fn codex_route_provider_sync_transaction<R: tauri::Runtime, T, F>(
    app: &tauri::AppHandle<R>,
    route_mode: CodexRouteMode,
    context: crate::infra::codex_provider_sync::CodexProviderSyncContext,
    after_apply: F,
) -> crate::shared::error::AppResult<(
    crate::infra::codex_provider_sync::CodexProviderSyncResult,
    T,
)>
where
    F: FnOnce(
        &crate::infra::codex_provider_sync::CodexProviderSyncResult,
    ) -> crate::shared::error::AppResult<T>,
{
    match route_mode {
        CodexRouteMode::Unproxied => {
            crate::infra::codex_provider_sync::codex_provider_sync_transaction_for_trusted_target(
                app,
                context,
                after_apply,
            )
        }
        CodexRouteMode::DirectAio | CodexRouteMode::Guarded => {
            crate::infra::codex_provider_sync::codex_provider_sync_transaction(
                app,
                context,
                after_apply,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_codex_route_change<R: tauri::Runtime, S: CodexRetryGatewayTransitionStore>(
    app: &tauri::AppHandle<R>,
    store: &S,
    existing_manifest: Option<CliProxyManifest>,
    refresh_backups: bool,
    prior_state: CodexCliProxyManifestState,
    canonical_bytes: Vec<u8>,
    canonical_auth_bytes: Option<Vec<u8>>,
    manifest_enabled: bool,
    route_mode: CodexRouteMode,
    desired_enabled: bool,
    aio_origin: Option<String>,
    guarded_origin: Option<String>,
    operation_kind: CodexRetryGatewayOperationKind,
    source_commit: Option<String>,
    process_should_run: bool,
) -> crate::shared::error::AppResult<CodexRouteApplyResult> {
    let canonical_sha256 = sha256_hex(&canonical_bytes);
    let projection_route_state = build_codex_route_manifest_state(
        &prior_state,
        prior_state.generation.saturating_add(1),
        route_mode,
        desired_enabled,
        aio_origin.clone(),
        guarded_origin.clone(),
        canonical_sha256.clone(),
        String::new(),
    );
    let live_bytes = project_codex_live_config(app, &projection_route_state, &canonical_bytes)?;
    let live_sha256 = sha256_hex(&live_bytes);
    let route_state = build_codex_route_manifest_state(
        &prior_state,
        prior_state.generation.saturating_add(1),
        route_mode,
        desired_enabled,
        aio_origin.clone(),
        guarded_origin.clone(),
        canonical_sha256.clone(),
        live_sha256.clone(),
    );
    let auth_projection =
        build_codex_auth_projection(app, route_mode, canonical_auth_bytes.as_deref())?;

    let current_live_text = String::from_utf8(
        read_optional_cli_proxy_file(&codex::codex_config_path(app)?)?.unwrap_or_default(),
    )
    .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let target_live_text = String::from_utf8(live_bytes.clone())
        .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let provider_sync_plan =
        codex_route_provider_sync_plan(&current_live_text, &target_live_text, route_mode)?;
    let transition = prepare_codex_route_transition(
        store,
        operation_kind,
        &prior_state,
        route_mode,
        canonical_sha256.clone(),
        live_sha256.clone(),
        source_commit,
        process_should_run,
    )?;

    let captured = capture_current_target_state(app, "codex")?;
    let target_snapshots = snapshot_target_files(&captured)?;
    let backup_snapshots = snapshot_backup_files(app, "codex", &captured)?;
    let manifest_snapshot = snapshot_file(&codex_route_manifest_path(app)?)?;
    let manifest = match (refresh_backups, existing_manifest) {
        (true, existing_manifest) => {
            let aio_origin = aio_origin.as_deref().ok_or_else(|| {
                crate::shared::error::AppError::from(
                    "CLI_PROXY_INVALID_ROUTE: route mutation requires aio_origin",
                )
            })?;
            backup_for_enable(app, "codex", aio_origin, existing_manifest)?
        }
        (false, Some(mut manifest)) => {
            ensure_manifest_has_current_targets(app, "codex", &mut manifest)?;
            manifest
        }
        (false, None) => {
            return Err("CLI_PROXY_NO_BACKUP: missing codex route manifest".into());
        }
    };
    let write_plan = CodexRouteWritePlan {
        manifest,
        route_state,
        manifest_enabled,
        live_bytes,
        auth_projection,
        provider_sync_plan: provider_sync_plan.clone(),
    };

    let result = (|| -> crate::shared::error::AppResult<CodexRouteApplyResult> {
        let transition_operation_id = transition.operation_id.clone();
        if write_plan.provider_sync_plan.change_required {
            let target_provider = write_plan.provider_sync_plan.target_provider.clone();
            let trigger = codex_route_provider_sync_trigger(operation_kind).to_string();
            let (provider_sync, route) = codex_route_provider_sync_transaction(
                app,
                route_mode,
                crate::infra::codex_provider_sync::CodexProviderSyncContext {
                    trigger,
                    target_provider,
                    config_bytes: Some(write_plan.live_bytes.clone()),
                },
                |_| {
                    write_codex_auth_projection(app, &write_plan.auth_projection)?;
                    codex_route_write_manifest(
                        app,
                        write_plan.manifest.clone(),
                        write_plan.manifest_enabled,
                        write_plan.route_state.clone(),
                    )?;
                    let verified = verify_route(app)?;
                    if verified.generation != write_plan.route_state.generation
                        || verified.route_mode != write_plan.route_state.route_mode
                        || verified.desired_enabled != write_plan.route_state.desired_enabled
                        || verified.aio_origin != write_plan.route_state.aio_origin
                        || verified.guarded_origin != write_plan.route_state.guarded_origin
                        || !verified.live_matches_projection
                        || !verified.auth_matches_projection
                    {
                        return Err(
                            "CLI_PROXY_ROUTE_VERIFY_FAILED: live Codex route did not match projection"
                                .into(),
                        );
                    }
                    commit_codex_route_transition(store, &transition)?;
                    Ok(verified)
                },
            )?;
            return Ok(CodexRouteApplyResult {
                transition_operation_id,
                route,
                provider_sync: Some(provider_sync),
            });
        }

        let config_path = codex::codex_config_path(app)?;
        let _ = write_file_atomic_if_changed(&config_path, &write_plan.live_bytes)?;
        write_codex_auth_projection(app, &write_plan.auth_projection)?;
        codex_route_write_manifest(
            app,
            write_plan.manifest.clone(),
            write_plan.manifest_enabled,
            write_plan.route_state.clone(),
        )?;
        let route = verify_route(app)?;
        if route.generation != write_plan.route_state.generation
            || route.route_mode != write_plan.route_state.route_mode
            || route.desired_enabled != write_plan.route_state.desired_enabled
            || route.aio_origin != write_plan.route_state.aio_origin
            || route.guarded_origin != write_plan.route_state.guarded_origin
            || !route.live_matches_projection
            || !route.auth_matches_projection
        {
            return Err(
                "CLI_PROXY_ROUTE_VERIFY_FAILED: live Codex route did not match projection".into(),
            );
        }
        commit_codex_route_transition(store, &transition)?;
        Ok(CodexRouteApplyResult {
            transition_operation_id,
            route,
            provider_sync: None,
        })
    })();

    match result {
        Ok(result) => Ok(result),
        Err(err) => {
            let restore_result = restore_codex_route_snapshots(
                &target_snapshots,
                &backup_snapshots,
                &manifest_snapshot,
            );
            let clear_result = clear_codex_route_transition(store, &transition);
            match (restore_result, clear_result) {
                (Ok(()), Ok(())) => Err(err),
                (Err(restore_err), Ok(())) => Err(format!(
                    "CLI_PROXY_ROUTE_ROLLBACK_FAILED: failed to restore route state after {err}; rollback error: {restore_err}"
                )
                .into()),
                (Ok(()), Err(clear_err)) => Err(format!(
                    "CLI_PROXY_ROUTE_ROLLBACK_FAILED: failed to clear pending transition after {err}; clear error: {clear_err}"
                )
                .into()),
                (Err(restore_err), Err(clear_err)) => Err(format!(
                    "CLI_PROXY_ROUTE_ROLLBACK_FAILED: failed to restore route state after {err}; rollback error: {restore_err}; clear error: {clear_err}"
                )
                .into()),
            }
        }
    }
}

pub(crate) fn plan_external_enable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    aio_origin: &str,
    guarded_origin: &str,
) -> crate::shared::error::AppResult<CodexExternalEnablePlan> {
    let aio_origin = validate_http_origin(aio_origin, "aio_origin")?;
    let guarded_origin = validate_http_origin(guarded_origin, "guarded_origin")?;
    if aio_origin == guarded_origin {
        return Err(
            "CLI_PROXY_INVALID_ROUTE: guarded route origin must differ from aio_origin".into(),
        );
    }

    let (state, current_route_state, canonical_bytes, _, canonical_sha256) =
        codex_route_context(app, Some(&aio_origin))?;
    let projected_route_state = build_codex_route_manifest_state(
        &current_route_state,
        current_route_state.generation.saturating_add(1),
        CodexRouteMode::Guarded,
        true,
        Some(aio_origin.clone()),
        Some(guarded_origin.clone()),
        canonical_sha256.clone(),
        String::new(),
    );
    let projected_live_bytes =
        project_codex_live_config(app, &projected_route_state, &canonical_bytes)?;
    let projected_live_config_sha256 = sha256_hex(&projected_live_bytes);
    let current_live_text = String::from_utf8(
        read_optional_cli_proxy_file(&codex::codex_config_path(app)?)?.unwrap_or_default(),
    )
    .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let target_live_text = String::from_utf8(projected_live_bytes)
        .map_err(|_| "SEC_INVALID_INPUT: codex config.toml must be valid UTF-8".to_string())?;
    let provider_sync =
        crate::infra::codex_provider_sync::codex_provider_sync_plan_for_config_text(
            &current_live_text,
            &target_live_text,
        )?;
    let verification = verify_route(app)?;

    Ok(CodexExternalEnablePlan {
        generation: current_route_state.generation,
        canonical_config_sha256: canonical_sha256,
        live_config_sha256: verification.live_config_sha256,
        projected_live_config_sha256,
        cli_proxy_enable_required: !state.as_ref().is_some_and(|state| state.manifest_enabled),
        route_change_required: current_route_state.route_mode != CodexRouteMode::Guarded
            || current_route_state.desired_enabled != true
            || current_route_state.aio_origin.as_deref() != Some(aio_origin.as_str())
            || current_route_state.guarded_origin.as_deref() != Some(guarded_origin.as_str())
            || !verification.live_matches_projection
            || !verification.auth_matches_projection,
        current_route_mode: current_route_state.route_mode,
        desired_enabled: current_route_state.desired_enabled,
        aio_origin: current_route_state.aio_origin.clone(),
        guarded_origin: current_route_state.guarded_origin.clone(),
        effective_origin: codex_effective_origin(&current_route_state),
        target_guarded_origin: guarded_origin,
        provider_sync,
    })
}

pub(crate) fn apply_guarded_route<R: tauri::Runtime, S: CodexRetryGatewayTransitionStore>(
    app: &tauri::AppHandle<R>,
    store: &S,
    request: CodexGuardedRouteApplyRequest,
) -> crate::shared::error::AppResult<CodexRouteApplyResult> {
    let aio_origin = validate_http_origin(&request.aio_origin, "aio_origin")?;
    let guarded_origin = validate_http_origin(&request.guarded_origin, "guarded_origin")?;
    if aio_origin == guarded_origin {
        return Err(
            "CLI_PROXY_INVALID_ROUTE: guarded route origin must differ from aio_origin".into(),
        );
    }

    let (state, prior_state, canonical_bytes, canonical_auth_bytes, canonical_sha256) =
        codex_route_context(app, Some(&aio_origin))?;
    reject_stale_codex_route_state(
        request.expected_generation,
        &request.expected_canonical_sha256,
        prior_state.generation,
        &canonical_sha256,
    )?;

    apply_codex_route_change(
        app,
        store,
        state.as_ref().map(|state| state.manifest.clone()),
        !state.as_ref().is_some_and(|state| state.manifest_enabled),
        prior_state,
        canonical_bytes,
        canonical_auth_bytes,
        true,
        CodexRouteMode::Guarded,
        request.desired_enabled,
        Some(aio_origin),
        Some(guarded_origin),
        CodexRetryGatewayOperationKind::Enable,
        request.source_commit,
        request.process_should_run,
    )
}

pub(crate) fn apply_direct_aio_route<R: tauri::Runtime, S: CodexRetryGatewayTransitionStore>(
    app: &tauri::AppHandle<R>,
    store: &S,
    request: CodexDirectAioRouteApplyRequest,
) -> crate::shared::error::AppResult<CodexRouteApplyResult> {
    let aio_origin = validate_http_origin(&request.aio_origin, "aio_origin")?;
    let (state, prior_state, canonical_bytes, canonical_auth_bytes, canonical_sha256) =
        codex_route_context(app, Some(&aio_origin))?;
    reject_stale_codex_route_state(
        request.expected_generation,
        &request.expected_canonical_sha256,
        prior_state.generation,
        &canonical_sha256,
    )?;

    apply_codex_route_change(
        app,
        store,
        state.as_ref().map(|state| state.manifest.clone()),
        !state.as_ref().is_some_and(|state| state.manifest_enabled),
        prior_state,
        canonical_bytes,
        canonical_auth_bytes,
        true,
        CodexRouteMode::DirectAio,
        request.desired_enabled,
        Some(aio_origin),
        state
            .as_ref()
            .and_then(|state| state.route.guarded_origin.clone()),
        CodexRetryGatewayOperationKind::DisableGateway,
        request.source_commit,
        request.process_should_run,
    )
}

pub(crate) fn restore_unproxied_route<R: tauri::Runtime, S: CodexRetryGatewayTransitionStore>(
    app: &tauri::AppHandle<R>,
    store: &S,
    request: CodexRestoreUnproxiedRouteRequest,
) -> crate::shared::error::AppResult<CodexRouteApplyResult> {
    let aio_origin = request
        .aio_origin
        .as_deref()
        .map(|origin| validate_http_origin(origin, "aio_origin"))
        .transpose()?;
    let (state, prior_state, canonical_bytes, canonical_auth_bytes, canonical_sha256) =
        codex_route_context(app, aio_origin.as_deref())?;
    reject_stale_codex_route_state(
        request.expected_generation,
        &request.expected_canonical_sha256,
        prior_state.generation,
        &canonical_sha256,
    )?;

    if state.is_none() && !request.keep_cli_proxy_enabled {
        return Ok(CodexRouteApplyResult {
            transition_operation_id: new_trace_id("codex-route-noop"),
            route: verify_route(app)?,
            provider_sync: None,
        });
    }

    apply_codex_route_change(
        app,
        store,
        state.as_ref().map(|state| state.manifest.clone()),
        request.keep_cli_proxy_enabled
            && !state.as_ref().is_some_and(|state| state.manifest_enabled),
        prior_state,
        canonical_bytes,
        canonical_auth_bytes,
        request.keep_cli_proxy_enabled,
        CodexRouteMode::Unproxied,
        request.desired_enabled,
        aio_origin.or_else(|| {
            state
                .as_ref()
                .and_then(|state| state.route.aio_origin.clone())
        }),
        state
            .as_ref()
            .and_then(|state| state.route.guarded_origin.clone()),
        CodexRetryGatewayOperationKind::DisableCliProxy,
        request.source_commit,
        request.process_should_run,
    )
}

pub(crate) fn verify_route<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<CodexRouteVerifyResult> {
    let (state, route_state, canonical_bytes, _, _) = codex_route_context(app, None)?;
    let manifest_enabled = state.as_ref().is_some_and(|state| state.manifest_enabled);
    let route_state = if state.as_ref().is_some_and(should_manage_codex_route_state) {
        route_state
    } else {
        CodexCliProxyManifestState {
            generation: route_state.generation,
            route_mode: CodexRouteMode::Unproxied,
            desired_enabled: route_state.desired_enabled,
            aio_origin: route_state.aio_origin,
            guarded_origin: route_state.guarded_origin,
            canonical_config_sha256: route_state.canonical_config_sha256,
            live_config_sha256: route_state.live_config_sha256,
        }
    };
    codex_route_verify_from_state(app, manifest_enabled, &route_state, &canonical_bytes)
}

pub(crate) fn reconcile_pending_route<R: tauri::Runtime, S: CodexRetryGatewayTransitionStore>(
    app: &tauri::AppHandle<R>,
    store: &S,
) -> crate::shared::error::AppResult<CodexRouteReconcileResult> {
    let route = verify_route(app)?;
    let pending_transition_reconciled =
        if route.live_matches_projection && route.auth_matches_projection {
            reconcile_codex_route_transition(
                store,
                route.generation,
                route.route_mode,
                &route.live_config_sha256,
            )?
        } else if let Some(transition) = store.load_pending()? {
            store.clear(&transition.operation_id)?;
            Some(false)
        } else {
            None
        };

    Ok(CodexRouteReconcileResult {
        route,
        pending_transition_reconciled,
    })
}

// -- Dispatch: target_files -------------------------------------------------

fn target_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<TargetFile>> {
    validate_cli_key(cli_key)?;

    match cli_key {
        "claude" => Ok(vec![TargetFile {
            kind: "claude_settings_json",
            path: claude::claude_settings_path(app)?,
            backup_name: "settings.json",
        }]),
        "codex" => {
            let mut files = vec![TargetFile {
                kind: "codex_config_toml",
                path: codex::codex_config_path(app)?,
                backup_name: "config.toml",
            }];
            if !codex_oauth_compatible_proxy_mode(app) {
                files.push(TargetFile {
                    kind: "codex_auth_json",
                    path: codex::codex_auth_path(app)?,
                    backup_name: "auth.json",
                });
            }
            Ok(files)
        }
        "gemini" => Ok(vec![TargetFile {
            kind: "gemini_env",
            path: gemini::gemini_env_path(app)?,
            backup_name: ".env",
        }]),
        _ => Err(format!("SEC_INVALID_INPUT: unknown cli_key={cli_key}").into()),
    }
}

// -- Dispatch: is_proxy_config_applied --------------------------------------

fn is_proxy_config_applied<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    base_origin: &str,
) -> bool {
    match cli_key {
        "claude" => claude::is_proxy_config_applied(app, base_origin),
        "codex" => codex::is_proxy_config_applied(app, base_origin),
        "gemini" => gemini::is_proxy_config_applied(app, base_origin),
        _ => false,
    }
}

// -- Dispatch: apply_proxy_config -------------------------------------------

fn apply_proxy_config<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    base_origin: &str,
) -> crate::shared::error::AppResult<()> {
    validate_cli_key(cli_key)?;

    let targets = target_files(app, cli_key)?;
    let mut prepared_writes: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(targets.len());

    for t in targets {
        let current = read_optional_cli_proxy_file(&t.path)?;

        let bytes = match cli_key {
            "claude" => {
                match claude::build_claude_settings_json(
                    current.clone(),
                    &format!("{base_origin}/claude"),
                ) {
                    Ok(b) => b,
                    Err(err) => {
                        // Preserve the original file — never clobber user data on parse failure.
                        if let Some(original_bytes) = current.as_ref() {
                            let backup_path = t.path.with_extension("json.invalid-backup");
                            let _ = write_cli_proxy_file_atomic(&backup_path, original_bytes);
                            tracing::warn!(
                                "cli_proxy: preserved invalid config as {}",
                                backup_path.display()
                            );
                        }
                        return Err(err);
                    }
                }
            }
            "codex" => {
                if t.kind == "codex_config_toml" {
                    let build_result = if codex_oauth_compatible_proxy_mode(app) {
                        codex::build_codex_config_toml_oauth_compatible(
                            current.clone(),
                            &format!("{base_origin}/v1"),
                            codex::CodexConfigPlatform::current(),
                        )
                    } else {
                        codex::build_codex_config_toml(
                            current.clone(),
                            &format!("{base_origin}/v1"),
                            codex::CodexConfigPlatform::current(),
                        )
                    };
                    match build_result {
                        Ok(b) => b,
                        Err(err) => {
                            if let Some(original_bytes) = current.as_ref() {
                                let backup_path = t.path.with_extension("toml.invalid-backup");
                                let _ = write_cli_proxy_file_atomic(&backup_path, original_bytes);
                            }
                            return Err(err);
                        }
                    }
                } else {
                    match codex::build_codex_auth_json(current.clone()) {
                        Ok(b) => b,
                        Err(err) => {
                            if let Some(original_bytes) = current.as_ref() {
                                let backup_path = t.path.with_extension("json.invalid-backup");
                                let _ = write_cli_proxy_file_atomic(&backup_path, original_bytes);
                            }
                            return Err(err);
                        }
                    }
                }
            }
            "gemini" => gemini::build_gemini_env(current, &format!("{base_origin}/gemini"))?,
            _ => return Err(format!("SEC_INVALID_INPUT: unknown cli_key={cli_key}").into()),
        };

        prepared_writes.push((t.path, bytes));
    }

    for (path, bytes) in prepared_writes {
        ensure_cli_proxy_bytes_len(
            &bytes,
            CLI_PROXY_FILE_MAX_BYTES,
            &format!("CLI proxy file {}", path.display()),
        )?;
        let _ = write_file_atomic_if_changed(&path, &bytes)?;
    }

    Ok(())
}

// -- Dispatch: restore_from_manifest ----------------------------------------

fn restore_from_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let cli_key = manifest.cli_key.as_str();
    validate_cli_key(cli_key)?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    let safety_dir = cli_proxy_safety_dir(&root);
    std::fs::create_dir_all(&safety_dir)
        .map_err(|e| format!("failed to create {}: {e}", safety_dir.display()))?;

    let ts = now_unix_seconds();

    for entry in &manifest.files {
        if should_skip_manifest_entry_for_current_settings(app, cli_key, &entry.kind) {
            continue;
        }

        let target_path = PathBuf::from(&entry.path);
        if entry.existed {
            let Some(rel) = entry.backup_rel.as_ref() else {
                return Err(format!("missing backup_rel for {}", entry.kind).into());
            };
            let backup_path = safe_backup_path(&files_dir, rel)?;

            // Use merge-restore for known file kinds to preserve user changes
            // made while the proxy was enabled.
            match entry.kind.as_str() {
                "claude_settings_json" => {
                    claude::merge_restore_claude_settings_json(&target_path, &backup_path)?;
                    continue;
                }
                "codex_auth_json" => {
                    codex::merge_restore_codex_auth_json(&target_path, &backup_path)?;
                    continue;
                }
                "codex_config_toml" => {
                    codex::merge_restore_codex_config_toml(&target_path, &backup_path)?;
                    continue;
                }
                "gemini_env" => {
                    gemini::merge_restore_gemini_env(&target_path, &backup_path)?;
                    continue;
                }
                _ => {}
            }

            // Fallback: full restore for unknown file kinds
            let bytes = read_cli_proxy_file(&backup_path)?;
            write_cli_proxy_file_atomic(&target_path, &bytes)?;
            continue;
        }

        if !target_path.exists() {
            continue;
        }

        // If the file did not exist before enabling proxy, restore to "absent".
        // Safety copy current content before removal.
        if target_path.exists() {
            let bytes = read_cli_proxy_file(&target_path)?;
            let safe_name = format!("{ts}_{}_before_remove", entry.kind);
            let safe_path = safety_dir.join(safe_name);
            write_cli_proxy_file_atomic(&safe_path, &bytes)?;
        }

        std::fs::remove_file(&target_path)
            .map_err(|e| format!("failed to remove {}: {e}", target_path.display()))?;
    }

    Ok(())
}

// -- Shared backup / snapshot helpers ---------------------------------------

pub fn backup_file_path_for_enabled_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    kind: &str,
    backup_name: &str,
) -> crate::shared::error::AppResult<Option<PathBuf>> {
    validate_cli_key(cli_key)?;

    let Some(mut manifest) = read_manifest(app, cli_key)? else {
        return Ok(None);
    };
    if !manifest.enabled {
        return Ok(None);
    }

    let target = target_files(app, cli_key)?
        .into_iter()
        .find(|t| t.kind == kind)
        .ok_or_else(|| {
            format!("SEC_INVALID_INPUT: unknown cli backup kind={kind} for cli_key={cli_key}")
        })?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let mut changed = false;
    let target_path = target.path.to_string_lossy().to_string();

    let backup_rel = if let Some(entry) = manifest.files.iter_mut().find(|entry| entry.kind == kind)
    {
        if entry.path != target_path {
            entry.path = target_path.clone();
            changed = true;
        }
        if !entry.existed {
            entry.existed = true;
            changed = true;
        }
        if entry.backup_rel.is_none() {
            entry.backup_rel = Some(backup_name.to_string());
            changed = true;
        }
        entry.backup_rel.clone()
    } else {
        let backup_rel = Some(backup_name.to_string());
        manifest.files.push(BackupFileEntry {
            kind: kind.to_string(),
            path: target_path,
            existed: true,
            backup_rel: backup_rel.clone(),
        });
        changed = true;
        backup_rel
    };

    if changed {
        manifest.updated_at = now_unix_seconds();
        write_manifest(app, cli_key, &manifest)?;
    }

    backup_rel
        .map(|rel| safe_backup_path(&files_dir, &rel))
        .transpose()
}

pub(crate) fn backup_file_path_for_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
    kind: &str,
) -> crate::shared::error::AppResult<Option<PathBuf>> {
    let cli_key = manifest.cli_key.as_str();
    validate_cli_key(cli_key)?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let Some(entry) = manifest.files.iter().find(|entry| entry.kind == kind) else {
        return Ok(None);
    };
    if !entry.existed {
        return Ok(None);
    }
    entry
        .backup_rel
        .as_deref()
        .map(|rel| safe_backup_path(&files_dir, rel))
        .transpose()
}

fn safe_backup_path(files_dir: &Path, rel: &str) -> crate::shared::error::AppResult<PathBuf> {
    let rel_path = Path::new(rel);
    if rel.trim().is_empty()
        || rel_path.is_absolute()
        || rel_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("SEC_INVALID_INPUT: invalid CLI proxy backup_rel={rel}").into());
    }

    let mut path = files_dir.to_path_buf();
    for component in rel_path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(format!("SEC_INVALID_INPUT: invalid CLI proxy backup_rel={rel}").into());
        };
        path.push(part);
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "SEC_INVALID_INPUT: refusing to use symlink CLI proxy backup path={}",
                    path.display()
                )
                .into());
            }
        }
    }

    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "SEC_INVALID_INPUT: refusing to use symlink CLI proxy backup path={}",
                path.display()
            )
            .into());
        }
    }
    Ok(path)
}

fn backup_for_enable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    base_origin: &str,
    existing: Option<CliProxyManifest>,
) -> crate::shared::error::AppResult<CliProxyManifest> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    let now = now_unix_seconds();
    let targets = target_files(app, cli_key)?;

    let mut entries = Vec::with_capacity(targets.len());
    for t in targets {
        let read_bytes = read_optional_cli_proxy_file(&t.path)?;
        let existed = read_bytes.is_some();
        let backup_rel = if let Some(bytes) = read_bytes {
            let backup_path = files_dir.join(t.backup_name);
            write_cli_proxy_file_atomic(&backup_path, &bytes)?;
            Some(t.backup_name.to_string())
        } else {
            None
        };

        entries.push(BackupFileEntry {
            kind: t.kind.to_string(),
            path: t.path.to_string_lossy().to_string(),
            existed,
            backup_rel,
        });
    }

    let created_at = existing.as_ref().map(|m| m.created_at).unwrap_or(now);

    Ok(CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: cli_key.to_string(),
        enabled: true,
        base_origin: Some(base_origin.to_string()),
        created_at,
        updated_at: now,
        files: entries,
        codex: existing
            .as_ref()
            .and_then(|manifest| manifest.codex.clone()),
    })
}

fn ensure_manifest_has_current_targets<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    manifest: &mut CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let targets = target_files(app, cli_key)?;
    if targets
        .iter()
        .all(|target| manifest.files.iter().any(|entry| entry.kind == target.kind))
    {
        return Ok(());
    }

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    for target in targets {
        if manifest.files.iter().any(|entry| entry.kind == target.kind) {
            continue;
        }

        let read_bytes = read_optional_cli_proxy_file(&target.path)?;
        let existed = read_bytes.is_some();
        let backup_rel = if let Some(bytes) = read_bytes {
            let backup_path = files_dir.join(target.backup_name);
            write_cli_proxy_file_atomic(&backup_path, &bytes)?;
            Some(target.backup_name.to_string())
        } else {
            None
        };

        manifest.files.push(BackupFileEntry {
            kind: target.kind.to_string(),
            path: target.path.to_string_lossy().to_string(),
            existed,
            backup_rel,
        });
    }

    Ok(())
}

fn capture_current_target_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<Vec<PendingBackupEntry>> {
    let targets = target_files(app, cli_key)?;
    let mut captured = Vec::with_capacity(targets.len());

    for target in targets {
        let backup_bytes = read_optional_cli_proxy_file(&target.path)?;

        captured.push(PendingBackupEntry {
            kind: target.kind.to_string(),
            path: target.path,
            backup_name: target.backup_name,
            existed: backup_bytes.is_some(),
            backup_bytes,
        });
    }

    Ok(captured)
}

fn manifest_target_paths_changed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<bool> {
    let targets = target_files(app, manifest.cli_key.as_str())?;
    for target in targets {
        let Some(entry) = manifest
            .files
            .iter()
            .find(|entry| entry.kind == target.kind)
        else {
            continue;
        };
        if Path::new(&entry.path) != target.path {
            return Ok(true);
        }
    }

    Ok(false)
}

fn write_captured_backups<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    captured: &[PendingBackupEntry],
) -> crate::shared::error::AppResult<()> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    std::fs::create_dir_all(&files_dir)
        .map_err(|e| format!("failed to create {}: {e}", files_dir.display()))?;

    for entry in captured {
        if let Some(bytes) = entry.backup_bytes.as_ref() {
            let backup_path = files_dir.join(entry.backup_name);
            write_cli_proxy_file_atomic(&backup_path, bytes)?;
        }
    }

    Ok(())
}

fn snapshot_file(path: &Path) -> crate::shared::error::AppResult<FileSnapshot> {
    let bytes = read_optional_cli_proxy_file(path)?;

    Ok(FileSnapshot {
        path: path.to_path_buf(),
        existed: bytes.is_some(),
        bytes,
    })
}

fn restore_file_snapshots(snapshots: &[FileSnapshot]) -> crate::shared::error::AppResult<()> {
    for snapshot in snapshots {
        if let Some(bytes) = snapshot.bytes.as_ref() {
            if let Some(parent) = snapshot.path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
            }
            write_cli_proxy_file_atomic(&snapshot.path, bytes)?;
            continue;
        }

        if snapshot.existed {
            return Err(format!(
                "snapshot for {} marked existed but no bytes captured",
                snapshot.path.display()
            )
            .into());
        }

        if snapshot.path.exists() {
            std::fs::remove_file(&snapshot.path)
                .map_err(|e| format!("failed to remove {}: {e}", snapshot.path.display()))?;
        }
    }

    Ok(())
}

fn restore_backups_exactly_from_manifest<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &CliProxyManifest,
) -> crate::shared::error::AppResult<()> {
    let cli_key = manifest.cli_key.as_str();
    validate_cli_key(cli_key)?;

    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);

    for entry in &manifest.files {
        if should_skip_manifest_entry_for_current_settings(app, cli_key, &entry.kind) {
            continue;
        }

        let target_path = PathBuf::from(&entry.path);
        if entry.existed {
            let Some(rel) = entry.backup_rel.as_ref() else {
                return Err(format!("missing backup_rel for {}", entry.kind).into());
            };
            let backup_path = safe_backup_path(&files_dir, rel)?;
            let bytes = read_cli_proxy_file(&backup_path)?;
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
            }
            write_cli_proxy_file_atomic(&target_path, &bytes)?;
            continue;
        }

        if target_path.exists() {
            std::fs::remove_file(&target_path)
                .map_err(|e| format!("failed to remove {}: {e}", target_path.display()))?;
        }
    }

    Ok(())
}

fn snapshot_backup_files<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    captured: &[PendingBackupEntry],
) -> crate::shared::error::AppResult<Vec<FileSnapshot>> {
    let root = cli_proxy_root_dir(app, cli_key)?;
    let files_dir = cli_proxy_files_dir(&root);
    captured
        .iter()
        .map(|entry| snapshot_file(&files_dir.join(entry.backup_name)))
        .collect()
}

fn snapshot_target_files(
    captured: &[PendingBackupEntry],
) -> crate::shared::error::AppResult<Vec<FileSnapshot>> {
    captured
        .iter()
        .map(|entry| {
            Ok(FileSnapshot {
                path: entry.path.clone(),
                existed: entry.existed,
                bytes: entry.backup_bytes.clone(),
            })
        })
        .collect()
}

fn build_manifest_from_captured(
    existing: &CliProxyManifest,
    base_origin: &str,
    captured: Vec<PendingBackupEntry>,
) -> CliProxyManifest {
    let now = now_unix_seconds();
    let files = captured
        .into_iter()
        .map(|entry| BackupFileEntry {
            kind: entry.kind,
            path: entry.path.to_string_lossy().to_string(),
            existed: entry.existed,
            backup_rel: entry.existed.then(|| entry.backup_name.to_string()),
        })
        .collect();

    CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: existing.cli_key.clone(),
        enabled: existing.enabled,
        base_origin: Some(base_origin.to_string()),
        created_at: existing.created_at,
        updated_at: now,
        files,
        codex: existing.codex.clone(),
    }
}

fn build_manifest_with_current_target_paths<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    existing: &CliProxyManifest,
    base_origin: &str,
) -> crate::shared::error::AppResult<CliProxyManifest> {
    let now = now_unix_seconds();
    let files = target_files(app, existing.cli_key.as_str())?
        .into_iter()
        .map(|target| {
            let existing_entry = existing
                .files
                .iter()
                .find(|entry| entry.kind == target.kind)
                .ok_or_else(|| format!("missing manifest entry for {}", target.kind))?;

            Ok(BackupFileEntry {
                kind: existing_entry.kind.clone(),
                path: target.path.to_string_lossy().to_string(),
                existed: existing_entry.existed,
                backup_rel: existing_entry.backup_rel.clone(),
            })
        })
        .collect::<crate::shared::error::AppResult<Vec<_>>>()?;

    Ok(CliProxyManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        managed_by: MANAGED_BY.to_string(),
        cli_key: existing.cli_key.clone(),
        enabled: existing.enabled,
        base_origin: Some(base_origin.to_string()),
        created_at: existing.created_at,
        updated_at: now,
        files,
        codex: existing.codex.clone(),
    })
}

// -- Public API -------------------------------------------------------------

pub fn status_all<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    current_base_origin: Option<&str>,
) -> crate::shared::error::AppResult<Vec<CliProxyStatus>> {
    let mut out = Vec::new();
    for cli_key in crate::shared::cli_key::SUPPORTED_CLI_KEYS {
        let manifest = read_manifest(app, cli_key)?;
        let enabled = manifest.as_ref().map(|m| m.enabled).unwrap_or(false);
        let manifest_base_origin = manifest.as_ref().and_then(|m| m.base_origin.clone());
        let (
            applied_to_current_gateway,
            generation,
            route_mode,
            desired_enabled,
            aio_origin,
            guarded_origin,
            effective_origin,
            current_gateway_origin,
        ) = if cli_key == "codex" {
            if let Some(manifest) = manifest.as_ref() {
                let route = codex_manifest_state_from_manifest(manifest);
                let aio_origin = route
                    .aio_origin
                    .clone()
                    .or_else(|| manifest.base_origin.clone());
                let guarded_origin = route.guarded_origin.clone();
                let effective_origin = codex_effective_origin(&route);
                let current_gateway_origin = match route.route_mode {
                    CodexRouteMode::Unproxied => current_base_origin.map(codex::normalize_origin),
                    CodexRouteMode::DirectAio => current_base_origin.map(codex::normalize_origin),
                    CodexRouteMode::Guarded => effective_origin.clone(),
                };
                let applied = match route.route_mode {
                    CodexRouteMode::Unproxied => {
                        if enabled && current_base_origin.is_some() {
                            Some(false)
                        } else {
                            None
                        }
                    }
                    CodexRouteMode::DirectAio | CodexRouteMode::Guarded => current_gateway_origin
                        .as_deref()
                        .map(|origin| is_proxy_config_applied(app, cli_key, origin)),
                };
                (
                    applied,
                    Some(route.generation),
                    Some(route.route_mode),
                    Some(route.desired_enabled),
                    aio_origin,
                    guarded_origin,
                    effective_origin,
                    current_gateway_origin,
                )
            } else {
                (
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    current_base_origin.map(str::to_string),
                )
            }
        } else {
            (
                if enabled {
                    current_base_origin
                        .map(|base_origin| is_proxy_config_applied(app, cli_key, base_origin))
                } else {
                    None
                },
                None,
                None,
                None,
                None,
                None,
                None,
                current_base_origin.map(str::to_string),
            )
        };
        out.push(CliProxyStatus {
            cli_key: cli_key.to_string(),
            enabled,
            base_origin: manifest_base_origin,
            current_gateway_origin,
            applied_to_current_gateway,
            generation,
            route_mode,
            desired_enabled,
            aio_origin,
            guarded_origin,
            effective_origin,
        });
    }
    Ok(out)
}

pub fn is_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> crate::shared::error::AppResult<bool> {
    validate_cli_key(cli_key)?;
    let Some(manifest) = read_manifest(app, cli_key)? else {
        return Ok(false);
    };
    Ok(manifest.enabled)
}

pub fn set_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
    enabled: bool,
    base_origin: &str,
) -> crate::shared::error::AppResult<CliProxyResult> {
    validate_cli_key(cli_key)?;
    if !base_origin.starts_with("http://") && !base_origin.starts_with("https://") {
        return Err("SEC_INVALID_INPUT: base_origin must start with http:// or https://".into());
    }

    let trace_id = new_trace_id("cli-proxy");
    let existing = read_manifest(app, cli_key)?;

    if enabled {
        let should_backup = existing.as_ref().map(|m| !m.enabled).unwrap_or(true);
        let origin = Some(base_origin.to_string());
        let mut manifest = match if should_backup {
            backup_for_enable(app, cli_key, base_origin, existing.clone())
        } else {
            Ok(existing.unwrap())
        } {
            Ok(m) => m,
            Err(err) => {
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_BACKUP_FAILED",
                    err.to_string(),
                    origin,
                ));
            }
        };

        // Persist snapshot before applying changes to ensure we can restore on failure.
        if should_backup {
            manifest.enabled = false;
            manifest.base_origin = Some(base_origin.to_string());
            manifest.updated_at = now_unix_seconds();
            if let Err(err) = write_manifest(app, cli_key, &manifest) {
                return Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_MANIFEST_WRITE_FAILED",
                    err.to_string(),
                    origin,
                ));
            }
        } else if let Err(err) = ensure_manifest_has_current_targets(app, cli_key, &mut manifest) {
            return Ok(CliProxyResult::failure(
                trace_id,
                cli_key,
                true,
                "CLI_PROXY_BACKUP_FAILED",
                err.to_string(),
                origin,
            ));
        }

        return match apply_proxy_config(app, cli_key, base_origin) {
            Ok(()) => {
                manifest.enabled = true;
                manifest.base_origin = Some(base_origin.to_string());
                manifest.updated_at = now_unix_seconds();
                if cli_key == "codex" {
                    let desired_enabled =
                        codex_manifest_state_from_manifest(&manifest).desired_enabled;
                    if let Err(err) = update_codex_manifest_state_for_route(
                        app,
                        &mut manifest,
                        CodexRouteMode::DirectAio,
                        desired_enabled,
                    ) {
                        return Ok(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_MANIFEST_WRITE_FAILED",
                            err.to_string(),
                            origin,
                        ));
                    }
                }
                if let Err(err) = write_manifest(app, cli_key, &manifest) {
                    return Ok(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        true,
                        "CLI_PROXY_MANIFEST_WRITE_FAILED",
                        err.to_string(),
                        origin,
                    ));
                }

                Ok(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "已开启代理：已备份直连配置并写入网关地址".to_string(),
                    origin,
                ))
            }
            Err(err) => {
                let is_parse_error = err.to_string().contains("CLI_PROXY_INVALID_");

                // Only rollback if we actually wrote proxy config (not on parse
                // failure where the file was never modified). On parse failure
                // the invalid file is already preserved as .invalid-backup by
                // apply_proxy_config, so restoring would clobber user changes.
                if should_backup && !is_parse_error {
                    let _ = restore_from_manifest(app, &manifest);
                    manifest.enabled = false;
                    manifest.updated_at = now_unix_seconds();
                    let _ = write_manifest(app, cli_key, &manifest);
                }

                Ok(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_ENABLE_FAILED",
                    err.to_string(),
                    origin,
                ))
            }
        };
    }

    let Some(mut manifest) = existing else {
        return Ok(CliProxyResult::failure(
            trace_id,
            cli_key,
            false,
            "CLI_PROXY_NO_BACKUP",
            "未找到备份，无法自动恢复；请手动处理".to_string(),
            Some(base_origin.to_string()),
        ));
    };

    match restore_from_manifest(app, &manifest) {
        Ok(()) => {
            manifest.enabled = false;
            manifest.updated_at = now_unix_seconds();
            if cli_key == "codex" {
                if let Err(err) = update_codex_manifest_state_for_route(
                    app,
                    &mut manifest,
                    CodexRouteMode::Unproxied,
                    false,
                ) {
                    return Ok(CliProxyResult::failure(
                        trace_id,
                        cli_key,
                        false,
                        "CLI_PROXY_DISABLE_FAILED",
                        err.to_string(),
                        manifest.base_origin.clone(),
                    ));
                }
            }
            let _ = write_manifest(app, cli_key, &manifest);

            Ok(CliProxyResult::success(
                trace_id,
                cli_key,
                false,
                "已关闭代理：已恢复备份直连配置".to_string(),
                manifest.base_origin.clone(),
            ))
        }
        Err(err) => Ok(CliProxyResult::failure(
            trace_id,
            cli_key,
            manifest.enabled,
            "CLI_PROXY_DISABLE_FAILED",
            err.to_string(),
            manifest.base_origin.clone(),
        )),
    }
}

pub fn startup_repair_incomplete_enable<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<CliProxyResult>> {
    let mut out = Vec::new();

    for cli_key in crate::shared::cli_key::SUPPORTED_CLI_KEYS {
        let Some(mut manifest) = read_manifest(app, cli_key)? else {
            continue;
        };
        if manifest.enabled {
            continue;
        }

        let Some(base_origin) = manifest.base_origin.clone() else {
            continue;
        };

        if !is_proxy_config_applied(app, cli_key, &base_origin) {
            continue;
        }

        let trace_id = new_trace_id("cli-proxy-startup-repair");

        manifest.enabled = true;
        manifest.updated_at = now_unix_seconds();
        if cli_key == "codex" {
            let desired_enabled = codex_manifest_state_from_manifest(&manifest).desired_enabled;
            if let Err(err) = update_codex_manifest_state_for_route(
                app,
                &mut manifest,
                CodexRouteMode::DirectAio,
                desired_enabled,
            ) {
                out.push(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    false,
                    "CLI_PROXY_STARTUP_REPAIR_FAILED",
                    err.to_string(),
                    Some(base_origin),
                ));
                continue;
            }
        }
        match write_manifest(app, cli_key, &manifest) {
            Ok(()) => out.push(CliProxyResult::success(
                trace_id,
                cli_key,
                true,
                "启动自愈：已修复异常中断导致的启用状态不一致".to_string(),
                Some(base_origin),
            )),
            Err(err) => out.push(CliProxyResult::failure(
                trace_id,
                cli_key,
                false,
                "CLI_PROXY_STARTUP_REPAIR_FAILED",
                err.to_string(),
                Some(base_origin),
            )),
        }
    }

    Ok(out)
}

pub fn sync_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> crate::shared::error::AppResult<Vec<CliProxyResult>> {
    if !base_origin.starts_with("http://") && !base_origin.starts_with("https://") {
        return Err("SEC_INVALID_INPUT: base_origin must start with http:// or https://".into());
    }

    let mut out = Vec::new();
    for cli_key in crate::shared::cli_key::SUPPORTED_CLI_KEYS {
        let Some(mut manifest) = read_manifest(app, cli_key)? else {
            continue;
        };
        if !manifest.enabled {
            continue;
        }

        let trace_id = new_trace_id("cli-proxy-sync");
        let needs_target_rebind =
            cli_key == "codex" && manifest_target_paths_changed(app, &manifest)?;
        let codex_route_state =
            (cli_key == "codex").then(|| codex_manifest_state_from_manifest(&manifest));

        if needs_target_rebind {
            out.push(codex::rebind_codex_manifest_after_home_change(
                app,
                manifest,
                base_origin,
                apply_live,
                trace_id,
            )?);
            continue;
        }

        if let Some(route_state) = codex_route_state.as_ref() {
            if route_state.route_mode == CodexRouteMode::Guarded {
                if manifest.base_origin.as_deref() != Some(base_origin) {
                    manifest.base_origin = Some(base_origin.to_string());
                    manifest.updated_at = now_unix_seconds();
                    if let Err(err) = update_codex_manifest_state_for_route(
                        app,
                        &mut manifest,
                        CodexRouteMode::Guarded,
                        route_state.desired_enabled,
                    ) {
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_SYNC_FAILED",
                            err.to_string(),
                            Some(base_origin.to_string()),
                        ));
                        continue;
                    }
                    write_manifest(app, cli_key, &manifest)?;
                }
                out.push(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    if apply_live {
                        "已更新 AIO 上游基线，当前仍由外部网关守护".to_string()
                    } else {
                        "已更新 AIO 上游基线，外部守护路由待协调确认".to_string()
                    },
                    Some(base_origin.to_string()),
                ));
                continue;
            }
        }

        if !apply_live {
            if manifest.base_origin.as_deref() != Some(base_origin) {
                manifest.base_origin = Some(base_origin.to_string());
                manifest.updated_at = now_unix_seconds();
                if cli_key == "codex" {
                    let route_state = codex_manifest_state_from_manifest(&manifest);
                    if let Err(err) = update_codex_manifest_state_for_route(
                        app,
                        &mut manifest,
                        route_state.route_mode,
                        route_state.desired_enabled,
                    ) {
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_SYNC_FAILED",
                            err.to_string(),
                            Some(base_origin.to_string()),
                        ));
                        continue;
                    }
                }
                write_manifest(app, cli_key, &manifest)?;
            }
            out.push(CliProxyResult::success(
                trace_id,
                cli_key,
                true,
                "已更新代理目标端口，待网关启动后接管".to_string(),
                Some(base_origin.to_string()),
            ));
            continue;
        }

        if manifest.base_origin.as_deref() == Some(base_origin)
            && is_proxy_config_applied(app, cli_key, base_origin)
        {
            out.push(CliProxyResult::success(
                trace_id,
                cli_key,
                true,
                "已是最新，无需同步".to_string(),
                Some(base_origin.to_string()),
            ));
            continue;
        }

        if let Err(err) = ensure_manifest_has_current_targets(app, cli_key, &mut manifest) {
            out.push(CliProxyResult::failure(
                trace_id,
                cli_key,
                true,
                "CLI_PROXY_BACKUP_FAILED",
                err.to_string(),
                Some(base_origin.to_string()),
            ));
            continue;
        }

        match apply_proxy_config(app, cli_key, base_origin) {
            Ok(()) => {
                manifest.base_origin = Some(base_origin.to_string());
                manifest.updated_at = now_unix_seconds();
                if cli_key == "codex" {
                    let desired_enabled =
                        codex_manifest_state_from_manifest(&manifest).desired_enabled;
                    if let Err(err) = update_codex_manifest_state_for_route(
                        app,
                        &mut manifest,
                        CodexRouteMode::DirectAio,
                        desired_enabled,
                    ) {
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_SYNC_FAILED",
                            err.to_string(),
                            Some(base_origin.to_string()),
                        ));
                        continue;
                    }
                }
                write_manifest(app, cli_key, &manifest)?;
                out.push(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "已同步代理配置到新端口".to_string(),
                    Some(base_origin.to_string()),
                ));
            }
            Err(err) => {
                out.push(CliProxyResult::failure(
                    trace_id,
                    cli_key,
                    true,
                    "CLI_PROXY_SYNC_FAILED",
                    err.to_string(),
                    Some(base_origin.to_string()),
                ));
            }
        }
    }
    Ok(out)
}

pub fn rebind_codex_home_after_change<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> crate::shared::error::AppResult<CliProxyResult> {
    codex::rebind_codex_home_after_change(app, base_origin, apply_live)
}

pub fn restore_enabled_keep_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> crate::shared::error::AppResult<Vec<CliProxyResult>> {
    let mut out = Vec::new();
    for cli_key in crate::shared::cli_key::SUPPORTED_CLI_KEYS {
        let Some(mut manifest) = read_manifest(app, cli_key)? else {
            continue;
        };
        if !manifest.enabled {
            continue;
        }

        let trace_id = new_trace_id("cli-proxy-restore");

        match restore_from_manifest(app, &manifest) {
            Ok(()) => {
                if cli_key == "codex" {
                    let desired_enabled =
                        codex_manifest_state_from_manifest(&manifest).desired_enabled;
                    if let Err(err) = update_codex_manifest_state_for_route(
                        app,
                        &mut manifest,
                        CodexRouteMode::Unproxied,
                        desired_enabled,
                    ) {
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_RESTORE_FAILED",
                            err.to_string(),
                            manifest.base_origin.clone(),
                        ));
                        continue;
                    }
                    manifest.updated_at = now_unix_seconds();
                    if let Err(err) = write_manifest(app, cli_key, &manifest) {
                        out.push(CliProxyResult::failure(
                            trace_id,
                            cli_key,
                            true,
                            "CLI_PROXY_RESTORE_FAILED",
                            err.to_string(),
                            manifest.base_origin.clone(),
                        ));
                        continue;
                    }
                }
                out.push(CliProxyResult::success(
                    trace_id,
                    cli_key,
                    true,
                    "已恢复备份直连配置（保留启用状态）".to_string(),
                    manifest.base_origin.clone(),
                ))
            }
            Err(err) => out.push(CliProxyResult::failure(
                trace_id,
                cli_key,
                true,
                "CLI_PROXY_RESTORE_FAILED",
                err.to_string(),
                manifest.base_origin.clone(),
            )),
        }
    }
    Ok(out)
}

// Re-export submodule items for tests (tests use `super::*`).
#[cfg(test)]
use claude::{build_claude_settings_json, merge_restore_claude_settings_json};
#[cfg(test)]
use codex::{
    build_codex_auth_json, build_codex_config_toml, build_codex_config_toml_oauth_compatible,
    codex_auth_path, codex_config_path, merge_restore_codex_auth_json,
    merge_restore_codex_config_toml, CodexConfigPlatform,
};
#[cfg(test)]
use gemini::merge_restore_gemini_env;

#[cfg(test)]
mod tests;
