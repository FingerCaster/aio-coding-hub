use crate::infra::codex_retry_gateway::config::{
    validate_managed_provider_name, DEFAULT_HEALTH_PATH, DEFAULT_LISTEN_HOST,
};
use crate::infra::codex_retry_gateway::managed_state::{
    read_manager_state, write_manager_state, CodexRetryGatewayManagedProcessRecord,
    CodexRetryGatewayManagerPaths, CodexRetryGatewayManagerState,
    CodexRetryGatewayPendingLaunchRecord,
};
use crate::infra::codex_retry_gateway::source::CodexRetryGatewayInstalledSource;
use crate::infra::codex_retry_gateway::util::{
    canonicalize_path_within_root, ensure_not_symlink_or_reparse, is_loopback_host,
    normalized_internal_relative_path, now_unix_ms, random_hex,
};
use crate::infra::codex_retry_gateway::{
    managed_gateway_config, managed_gateway_state, AioGatewayOrigin, CodexRetryGatewayError,
    CodexRetryGatewayErrorCategory, CodexRetryGatewayProcessPhase, CodexRetryGatewayProcessStatus,
    CodexRetryGatewayResolvedNode, ManagedGatewayStateInput, CODEX_RETRY_GATEWAY_DEFAULT_PORT,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::write_file_atomic;
use crate::shared::http_body::read_text_with_limit;
use reqwest::Client;
use serde_json::Value;
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const PROCESS_HEALTH_TIMEOUT: Duration = Duration::from_secs(15);
const PROCESS_STOP_TIMEOUT: Duration = Duration::from_secs(8);
const PROCESS_HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const PROCESS_HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(250);
const PROCESS_STATUS_BODY_LIMIT: usize = 512 * 1024;
const PROCESS_PORT_SEARCH_MAX: u16 = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayHealthSnapshot {
    pub(crate) listener: String,
    pub(crate) process_id: Option<u32>,
    pub(crate) upstream_base_url: Option<String>,
    pub(crate) gateway_base_url: Option<String>,
    pub(crate) config_path: Option<String>,
    pub(crate) state_path: Option<String>,
    pub(crate) state_root: Option<String>,
    pub(crate) log_path: Option<String>,
    pub(crate) instance_nonce: Option<String>,
    pub(crate) provider_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayManagedProcess {
    pub(crate) record: CodexRetryGatewayManagedProcessRecord,
    pub(crate) health: CodexRetryGatewayHealthSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayProcessReconcileResult {
    pub(crate) status: CodexRetryGatewayProcessStatus,
    pub(crate) managed: Option<CodexRetryGatewayManagedProcess>,
    pub(crate) health: Option<CodexRetryGatewayHealthSnapshot>,
    pub(crate) error: Option<CodexRetryGatewayError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedManagedRecord {
    start_identity: u64,
    node_executable: PathBuf,
    listener: String,
    source_dir: PathBuf,
    config_path: PathBuf,
    state_path: PathBuf,
    state_root: PathBuf,
    log_path: PathBuf,
}

pub(crate) async fn start_runtime_process(
    paths: &CodexRetryGatewayManagerPaths,
    source: &CodexRetryGatewayInstalledSource,
    node: &CodexRetryGatewayResolvedNode,
    aio_origin: &AioGatewayOrigin,
    provider_name: &str,
    preferred_port: u16,
    persisted_port: Option<u16>,
) -> AppResult<CodexRetryGatewayManagedProcess> {
    validate_managed_provider_name(provider_name)?;
    paths.ensure_dirs()?;
    let listen_port = choose_listen_port(persisted_port, preferred_port)?;
    let listener = format!("http://{DEFAULT_LISTEN_HOST}:{listen_port}");
    let instance_nonce = random_hex(16);
    let previous_manager = read_manager_state(paths)?;
    let node_executable = canonicalize_absolute_existing_path(
        &node.executable.display().to_string(),
        "managed Node executable",
    )?;
    let source_dir_rel = relative_to_root(&paths.root, &source.source_dir)?;
    let config_path_rel = relative_to_root(&paths.root, &paths.runtime_config_path)?;
    let state_path_rel = relative_to_root(&paths.root, &paths.runtime_state_path)?;
    let log_path_rel = relative_to_root(&paths.root, &paths.runtime_log_path)?;
    let pending_launch = CodexRetryGatewayPendingLaunchRecord {
        created_at_ms: now_unix_ms(),
        node_executable: node_executable.display().to_string(),
        source_commit: source.manifest.commit.clone(),
        source_dir_rel: source_dir_rel.clone(),
        config_path_rel: config_path_rel.clone(),
        state_path_rel: state_path_rel.clone(),
        log_path_rel: log_path_rel.clone(),
        listener: listener.clone(),
        upstream_base_url: aio_origin.url.clone(),
        instance_nonce: instance_nonce.clone(),
        provider_name: provider_name.to_string(),
    };
    let config = managed_gateway_config(listen_port, aio_origin);
    let config_bytes = json_file_bytes(&config, "managed gateway config")?;
    write_file_atomic(&paths.runtime_config_path, &config_bytes)?;
    ensure_runtime_log_file(&paths.runtime_log_path)?;
    persist_managed_gateway_state(
        paths,
        &listener,
        &aio_origin.url,
        provider_name,
        &instance_nonce,
        None,
        None,
    )?;

    let mut command = Command::new(&node_executable);
    command.arg(source.source_dir.join("gateway.mjs"));
    command.arg("--config").arg(&paths.runtime_config_path);
    command.arg("--log").arg(&paths.runtime_log_path);
    command.current_dir(&source.source_dir);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.env("PATH", runtime_path_for_executable(&node_executable)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut pending_manager = previous_manager.clone();
    pending_manager.pending_launch = Some(pending_launch.clone());
    write_manager_state(paths, &pending_manager)?;

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let error = AppError::new(
                "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED",
                format!(
                    "failed to start managed gateway with {}: {err}",
                    node_executable.display()
                ),
            );
            return Err(restore_manager_after_failed_launch(
                paths,
                &previous_manager,
                error,
            ));
        }
    };
    let pid = child.id();
    let Some(start_identity) = process_start_identity(pid) else {
        let _ = child.kill();
        let _ = child.wait();
        let error = AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED",
            format!("failed to capture a stable start identity for managed gateway process {pid}"),
        );
        return Err(restore_manager_after_failed_launch(
            paths,
            &previous_manager,
            error,
        ));
    };
    let process_record = CodexRetryGatewayManagedProcessRecord {
        pid,
        start_identity: Some(start_identity),
        started_at_ms: now_unix_ms(),
        node_executable: node_executable.display().to_string(),
        source_commit: source.manifest.commit.clone(),
        source_dir_rel,
        config_path_rel,
        state_path_rel,
        log_path_rel,
        listener: listener.clone(),
        upstream_base_url: aio_origin.url.clone(),
        instance_nonce: instance_nonce.clone(),
        provider_name: provider_name.to_string(),
    };
    let mut provisional_manager = pending_manager;
    provisional_manager.effective_port = Some(listen_port);
    provisional_manager.process_record = Some(process_record.clone());
    provisional_manager.pending_launch = None;
    if let Err(error) = write_manager_state(paths, &provisional_manager) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(restore_manager_after_failed_launch(
            paths,
            &previous_manager,
            error,
        ));
    }
    drop(child);

    let result = async {
        persist_managed_gateway_state(
            paths,
            &listener,
            &aio_origin.url,
            provider_name,
            &instance_nonce,
            Some(pid),
            Some(start_identity),
        )?;
        write_file_atomic(&paths.runtime_pid_path, format!("{pid}\n").as_bytes())?;

        let health = wait_for_healthy_listener(
            &listener,
            pid,
            start_identity,
            &aio_origin.url,
            provider_name,
            &instance_nonce,
        )
        .await?;
        let managed = CodexRetryGatewayManagedProcess {
            record: process_record,
            health,
        };
        let validated = validate_managed_record(paths, &managed.record)?;
        if !health_matches_record(&managed.health, &managed.record, &validated) {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT",
                "managed gateway health identity did not match the persisted runtime record",
            ));
        }
        Ok(managed)
    }
    .await;

    match result {
        Ok(managed) => Ok(managed),
        Err(error) => {
            let _ = terminate_process_by_identity(pid, Some(start_identity));
            let _ = std::fs::remove_file(&paths.runtime_pid_path);
            let _ = persist_managed_gateway_state(
                paths,
                &listener,
                &aio_origin.url,
                provider_name,
                &instance_nonce,
                None,
                None,
            );
            Err(restore_manager_after_failed_launch(
                paths,
                &previous_manager,
                error,
            ))
        }
    }
}

fn restore_manager_after_failed_launch(
    paths: &CodexRetryGatewayManagerPaths,
    previous_manager: &CodexRetryGatewayManagerState,
    error: AppError,
) -> AppError {
    match write_manager_state(paths, previous_manager) {
        Ok(_) => error,
        Err(restore_error) => AppError::new(
            error.code(),
            format!(
                "{error}; failed to restore manager state after start failure: {restore_error}"
            ),
        ),
    }
}

pub(super) async fn reconcile_pending_runtime_launch(
    paths: &CodexRetryGatewayManagerPaths,
) -> AppResult<Option<CodexRetryGatewayManagedProcessRecord>> {
    reconcile_pending_runtime_launch_with_timeout(paths, PROCESS_HEALTH_TIMEOUT).await
}

async fn reconcile_pending_runtime_launch_with_timeout(
    paths: &CodexRetryGatewayManagerPaths,
    timeout: Duration,
) -> AppResult<Option<CodexRetryGatewayManagedProcessRecord>> {
    let mut manager = read_manager_state(paths)?;
    let Some(pending) = manager.pending_launch.clone() else {
        return Ok(None);
    };
    validate_pending_launch(paths, &pending)?;

    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(health) = probe_runtime_health(&pending.listener).await? {
            if let Some(record) = process_record_from_pending_health(paths, &pending, &health) {
                persist_managed_gateway_state(
                    paths,
                    &record.listener,
                    &record.upstream_base_url,
                    &record.provider_name,
                    &record.instance_nonce,
                    Some(record.pid),
                    record.start_identity,
                )?;
                write_file_atomic(
                    &paths.runtime_pid_path,
                    format!("{}\n", record.pid).as_bytes(),
                )?;
                manager.effective_port = listener_port(&record.listener);
                manager.process_record = Some(record.clone());
                manager.pending_launch = None;
                write_manager_state(paths, &manager)?;
                return Ok(Some(record));
            }
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(PROCESS_HEALTH_POLL_INTERVAL).await;
    }

    manager.pending_launch = None;
    write_manager_state(paths, &manager)?;
    Ok(None)
}

fn validate_pending_launch(
    paths: &CodexRetryGatewayManagerPaths,
    pending: &CodexRetryGatewayPendingLaunchRecord,
) -> AppResult<()> {
    let placeholder = pending.clone().into_process_record(1, 1);
    validate_managed_record(paths, &placeholder).map(|_| ())
}

fn process_record_from_pending_health(
    paths: &CodexRetryGatewayManagerPaths,
    pending: &CodexRetryGatewayPendingLaunchRecord,
    health: &CodexRetryGatewayHealthSnapshot,
) -> Option<CodexRetryGatewayManagedProcessRecord> {
    let pid = health.process_id.filter(|pid| *pid != 0)?;
    let start_identity = process_start_identity(pid)?;
    let record = pending.clone().into_process_record(pid, start_identity);
    let validated = validate_managed_record(paths, &record).ok()?;
    if !process_matches_identity(pid, Some(start_identity))
        || !process_executable_matches(pid, &validated.node_executable)
        || !health_matches_record(health, &record, &validated)
    {
        return None;
    }
    Some(record)
}

fn listener_port(listener: &str) -> Option<u16> {
    reqwest::Url::parse(listener).ok()?.port_or_known_default()
}

fn ensure_runtime_log_file(path: &Path) -> AppResult<()> {
    let parent = path.parent().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_STATE_WRITE_FAILED",
            format!("runtime log path has no parent: {}", path.display()),
        )
    })?;
    ensure_not_symlink_or_reparse(parent, "managed runtime log directory")
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_STATE_WRITE_FAILED", err.to_string()))?;
    if path.exists() {
        ensure_not_symlink_or_reparse(path, "managed runtime log file").map_err(|err| {
            AppError::new("CODEX_RETRY_GATEWAY_STATE_WRITE_FAILED", err.to_string())
        })?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_STATE_WRITE_FAILED",
                format!(
                    "failed to create runtime log file {}: {err}",
                    path.display()
                ),
            )
        })
}

fn persist_managed_gateway_state(
    paths: &CodexRetryGatewayManagerPaths,
    listener: &str,
    upstream_base_url: &str,
    provider_name: &str,
    instance_nonce: &str,
    process_id: Option<u32>,
    process_start_identity: Option<u64>,
) -> AppResult<()> {
    let state = managed_gateway_state(ManagedGatewayStateInput {
        gateway_base_url: listener,
        state_root: &paths.runtime_dir.display().to_string(),
        config_path: &paths.runtime_config_path.display().to_string(),
        log_path: &paths.runtime_log_path.display().to_string(),
        pid_path: &paths.runtime_pid_path.display().to_string(),
        upstream_base_url,
        provider_name,
        instance_nonce,
        process_id,
        process_start_identity,
    });
    let state_bytes = json_file_bytes(&state, "managed gateway state")?;
    write_file_atomic(&paths.runtime_state_path, &state_bytes)
}

pub(crate) fn update_managed_provider_projection(
    paths: &CodexRetryGatewayManagerPaths,
    provider_name: &str,
) -> AppResult<Option<CodexRetryGatewayManagedProcessRecord>> {
    validate_managed_provider_name(provider_name)?;
    let mut manager = read_manager_state(paths)?;
    let Some(record) = manager.process_record.as_mut() else {
        return Ok(None);
    };
    if record.provider_name == provider_name {
        return Ok(None);
    }

    persist_managed_gateway_state(
        paths,
        &record.listener,
        &record.upstream_base_url,
        provider_name,
        &record.instance_nonce,
        Some(record.pid),
        record.start_identity,
    )?;
    record.provider_name = provider_name.to_string();
    let updated_record = record.clone();
    manager.generation = manager.generation.saturating_add(1);
    write_manager_state(paths, &manager)?;
    Ok(Some(updated_record))
}

pub(crate) async fn verify_managed_provider_projection(
    paths: &CodexRetryGatewayManagerPaths,
    expected_record: &CodexRetryGatewayManagedProcessRecord,
) -> AppResult<()> {
    let manager = read_manager_state(paths)?;
    if manager.process_record.as_ref() != Some(expected_record) {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROVIDER_VERIFY_FAILED",
            "managed process record changed before provider projection verification",
        ));
    }
    let reconciled =
        reconcile_runtime_process(paths, Some(expected_record), manager.effective_port)
            .await
            .map_err(|error| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_PROVIDER_VERIFY_FAILED",
                    format!("managed provider health verification failed: {error}"),
                )
            })?;
    if reconciled.managed.is_none() {
        let detail = reconciled
            .error
            .map(|error| error.message)
            .unwrap_or_else(|| {
                "external gateway did not confirm the updated provider identity".to_string()
            });
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROVIDER_VERIFY_FAILED",
            detail,
        ));
    }
    Ok(())
}

pub(crate) async fn reconcile_runtime_process(
    paths: &CodexRetryGatewayManagerPaths,
    record: Option<&CodexRetryGatewayManagedProcessRecord>,
    effective_port: Option<u16>,
) -> AppResult<CodexRetryGatewayProcessReconcileResult> {
    let Some(record) = record else {
        if let Some(port) = effective_port {
            let listener = format!("http://{DEFAULT_LISTEN_HOST}:{port}");
            if let Some(health) = probe_runtime_health(&listener).await? {
                return Ok(CodexRetryGatewayProcessReconcileResult {
                    status: CodexRetryGatewayProcessStatus {
                        phase: CodexRetryGatewayProcessPhase::OwnershipMismatch,
                        owned: false,
                        healthy: true,
                        process_id: health.process_id,
                        listener: Some(listener),
                    },
                    managed: None,
                    health: Some(health),
                    error: Some(public_process_error(
                        "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
                        CodexRetryGatewayErrorCategory::OwnershipMismatch,
                        "gateway listener is occupied by an unmanaged process",
                        false,
                    )),
                });
            }
            if !is_loopback_port_available(DEFAULT_LISTEN_HOST, port) {
                return Ok(CodexRetryGatewayProcessReconcileResult {
                    status: CodexRetryGatewayProcessStatus {
                        phase: CodexRetryGatewayProcessPhase::OwnershipMismatch,
                        owned: false,
                        healthy: false,
                        process_id: None,
                        listener: Some(listener),
                    },
                    managed: None,
                    health: None,
                    error: Some(public_process_error(
                        "CODEX_RETRY_GATEWAY_PORT_CONFLICT",
                        CodexRetryGatewayErrorCategory::PortConflict,
                        "gateway port is occupied by an unmanaged listener",
                        true,
                    )),
                });
            }
        }
        return Ok(CodexRetryGatewayProcessReconcileResult {
            status: CodexRetryGatewayProcessStatus::default(),
            managed: None,
            health: None,
            error: None,
        });
    };

    let health = probe_runtime_health(&record.listener).await?;
    let validated_record = validate_managed_record(paths, record).ok();
    let pid_matches = validated_record.as_ref().is_some_and(|validated| {
        process_matches_identity(record.pid, Some(validated.start_identity))
            && process_executable_matches(record.pid, &validated.node_executable)
    });

    match (pid_matches, health) {
        (true, Some(health))
            if validated_record
                .as_ref()
                .is_some_and(|validated| health_matches_record(&health, record, validated)) =>
        {
            Ok(CodexRetryGatewayProcessReconcileResult {
                status: CodexRetryGatewayProcessStatus {
                    phase: CodexRetryGatewayProcessPhase::Healthy,
                    owned: true,
                    healthy: true,
                    process_id: Some(record.pid),
                    listener: Some(record.listener.clone()),
                },
                managed: Some(CodexRetryGatewayManagedProcess {
                    record: record.clone(),
                    health: health.clone(),
                }),
                health: Some(health),
                error: None,
            })
        }
        (true, Some(health)) => Ok(CodexRetryGatewayProcessReconcileResult {
            status: CodexRetryGatewayProcessStatus {
                phase: CodexRetryGatewayProcessPhase::OwnershipMismatch,
                owned: false,
                healthy: true,
                process_id: Some(record.pid),
                listener: Some(record.listener.clone()),
            },
            managed: None,
            health: Some(health),
            error: Some(public_process_error(
                "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
                CodexRetryGatewayErrorCategory::OwnershipMismatch,
                "gateway process identity no longer matches the managed record",
                false,
            )),
        }),
        (true, None) => Ok(CodexRetryGatewayProcessReconcileResult {
            status: CodexRetryGatewayProcessStatus {
                phase: CodexRetryGatewayProcessPhase::Unhealthy,
                owned: true,
                healthy: false,
                process_id: Some(record.pid),
                listener: Some(record.listener.clone()),
            },
            managed: None,
            health: None,
            error: Some(public_process_error(
                "CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT",
                CodexRetryGatewayErrorCategory::HealthTimeout,
                "gateway process is running but failed health validation",
                true,
            )),
        }),
        (false, Some(health)) => Ok(CodexRetryGatewayProcessReconcileResult {
            status: CodexRetryGatewayProcessStatus {
                phase: CodexRetryGatewayProcessPhase::OwnershipMismatch,
                owned: false,
                healthy: true,
                process_id: health.process_id,
                listener: Some(record.listener.clone()),
            },
            managed: None,
            health: Some(health),
            error: Some(public_process_error(
                "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
                CodexRetryGatewayErrorCategory::OwnershipMismatch,
                "gateway listener is responding but the owned process identity is gone",
                false,
            )),
        }),
        (false, None) => Ok(CodexRetryGatewayProcessReconcileResult {
            status: CodexRetryGatewayProcessStatus {
                phase: CodexRetryGatewayProcessPhase::Stopped,
                owned: false,
                healthy: false,
                process_id: None,
                listener: Some(record.listener.clone()),
            },
            managed: None,
            health: None,
            error: None,
        }),
    }
}

pub(crate) async fn stop_runtime_process(
    paths: &CodexRetryGatewayManagerPaths,
    record: &CodexRetryGatewayManagedProcessRecord,
) -> AppResult<bool> {
    let reconciled = reconcile_runtime_process(paths, Some(record), None).await?;
    if reconciled.managed.is_none() && !reconciled.status.owned {
        return Ok(false);
    }
    terminate_process_by_identity(record.pid, record.start_identity)?;
    let deadline = tokio::time::Instant::now() + PROCESS_STOP_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if !process_matches_identity(record.pid, record.start_identity)
            && probe_runtime_health(&record.listener).await?.is_none()
        {
            let _ = std::fs::remove_file(&paths.runtime_pid_path);
            return Ok(true);
        }
        tokio::time::sleep(PROCESS_HEALTH_POLL_INTERVAL).await;
    }
    Err(AppError::new(
        "CODEX_RETRY_GATEWAY_PROCESS_STOP_TIMEOUT",
        format!(
            "managed gateway process {} did not exit within {}ms",
            record.pid,
            PROCESS_STOP_TIMEOUT.as_millis()
        ),
    ))
}

pub(crate) async fn probe_runtime_health(
    listener: &str,
) -> AppResult<Option<CodexRetryGatewayHealthSnapshot>> {
    let client = build_process_client()?;
    let health = fetch_gateway_json(
        &client,
        &format!("{listener}{DEFAULT_HEALTH_PATH}"),
        "gateway health",
    )
    .await?;
    let status = fetch_gateway_json(
        &client,
        &format!("{listener}/__codex_retry_gateway/api/status"),
        "gateway status",
    )
    .await?;
    Ok(project_health_snapshot(health, status))
}

fn build_process_client() -> AppResult<Client> {
    Client::builder()
        .no_proxy()
        .timeout(PROCESS_HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_HTTP_CLIENT_FAILED", err.to_string()))
}

async fn fetch_gateway_json(client: &Client, url: &str, context: &str) -> AppResult<Option<Value>> {
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(err) if err.is_connect() || err.is_timeout() => return Ok(None),
        Err(err) => {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_HEALTH_PROBE_FAILED",
                format!("{context} request failed: {err}"),
            ))
        }
    };
    if !response.status().is_success() {
        return Ok(None);
    }
    let body = match read_text_with_limit(response, PROCESS_STATUS_BODY_LIMIT, context).await {
        Ok(body) => body,
        Err(_) => return Ok(None),
    };
    Ok(serde_json::from_str(&body).ok())
}

async fn wait_for_healthy_listener(
    listener: &str,
    pid: u32,
    start_identity: u64,
    upstream_base_url: &str,
    provider_name: &str,
    instance_nonce: &str,
) -> AppResult<CodexRetryGatewayHealthSnapshot> {
    let deadline = tokio::time::Instant::now() + PROCESS_HEALTH_TIMEOUT;
    loop {
        if !process_matches_identity(pid, Some(start_identity)) {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED",
                "managed gateway process exited before becoming healthy",
            ));
        }
        if let Some(health) = probe_runtime_health(listener).await? {
            if health.process_id == Some(pid)
                && health.listener.trim() == listener
                && health.gateway_base_url.as_deref().map(str::trim) == Some(listener)
                && health
                    .upstream_base_url
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| value == upstream_base_url)
                && health
                    .instance_nonce
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| value == instance_nonce)
                && health.provider_name.as_deref().map(str::trim) == Some(provider_name)
            {
                return Ok(health);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT",
                format!(
                    "managed gateway did not become healthy within {}ms",
                    PROCESS_HEALTH_TIMEOUT.as_millis()
                ),
            ));
        }
        tokio::time::sleep(PROCESS_HEALTH_POLL_INTERVAL).await;
    }
}

fn health_matches_record(
    health: &CodexRetryGatewayHealthSnapshot,
    record: &CodexRetryGatewayManagedProcessRecord,
    validated: &ValidatedManagedRecord,
) -> bool {
    if health.process_id != Some(record.pid) {
        return false;
    }
    if health.upstream_base_url.as_deref().map(str::trim) != Some(record.upstream_base_url.as_str())
    {
        return false;
    }
    if health.instance_nonce.as_deref().map(str::trim) != Some(record.instance_nonce.as_str()) {
        return false;
    }
    if health.listener.trim() != validated.listener {
        return false;
    }
    if health.gateway_base_url.as_deref().map(str::trim) != Some(validated.listener.as_str()) {
        return false;
    }
    if health.provider_name.as_deref().map(str::trim) != Some(record.provider_name.as_str()) {
        return false;
    }
    if !health
        .config_path
        .as_deref()
        .is_some_and(|path| reported_path_matches(path, &validated.config_path))
    {
        return false;
    }
    if !health
        .state_root
        .as_deref()
        .is_some_and(|path| reported_path_matches(path, &validated.state_root))
    {
        return false;
    }
    if !health
        .state_path
        .as_deref()
        .is_some_and(|path| reported_path_matches(path, &validated.state_path))
    {
        return false;
    }
    if !health
        .log_path
        .as_deref()
        .is_some_and(|path| reported_path_matches(path, &validated.log_path))
    {
        return false;
    }
    true
}

fn choose_listen_port(persisted_port: Option<u16>, preferred_port: u16) -> AppResult<u16> {
    let mut candidates = Vec::new();
    if let Some(port) = persisted_port.filter(|value| *value >= 1024) {
        candidates.push(port);
    }
    if preferred_port >= 1024 && !candidates.contains(&preferred_port) {
        candidates.push(preferred_port);
    }
    if !candidates.contains(&CODEX_RETRY_GATEWAY_DEFAULT_PORT) {
        candidates.push(CODEX_RETRY_GATEWAY_DEFAULT_PORT);
    }
    for port in candidates {
        if is_loopback_port_available(DEFAULT_LISTEN_HOST, port) {
            return Ok(port);
        }
    }
    for offset in 1..=PROCESS_PORT_SEARCH_MAX {
        let port = CODEX_RETRY_GATEWAY_DEFAULT_PORT.saturating_add(offset);
        if port >= 1024 && is_loopback_port_available(DEFAULT_LISTEN_HOST, port) {
            return Ok(port);
        }
    }
    bind_random_loopback_port(DEFAULT_LISTEN_HOST)
}

fn bind_random_loopback_port(host: &str) -> AppResult<u16> {
    TcpListener::bind((host, 0))
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_PORT_CONFLICT",
                format!("failed to find an available managed gateway port: {err}"),
            )
        })
}

fn is_loopback_port_available(host: &str, port: u16) -> bool {
    if !is_loopback_host(host) {
        return false;
    }
    TcpListener::bind((host, port)).is_ok()
}

fn runtime_path_for_executable(executable: &Path) -> AppResult<OsString> {
    let mut paths = Vec::new();
    if let Some(parent) = executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        paths.push(parent.to_path_buf());
    }
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    std::env::join_paths(paths)
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_NODE_PATH_INVALID", err.to_string()))
}

fn json_file_bytes(value: &Value, label: &str) -> AppResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_STATE_WRITE_FAILED",
            format!("{label} serialize failed: {err}"),
        )
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn relative_to_root(root: &Path, path: &Path) -> AppResult<String> {
    path.strip_prefix(root)
        .map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_STATE_WRITE_FAILED",
                format!(
                    "failed to make {} relative to {}: {err}",
                    path.display(),
                    root.display()
                ),
            )
        })
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

fn public_process_error(
    code: &str,
    category: CodexRetryGatewayErrorCategory,
    message: &str,
    retryable: bool,
) -> CodexRetryGatewayError {
    CodexRetryGatewayError {
        code: code.to_string(),
        category,
        message: message.to_string(),
        retryable,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OfficialHealthProjection {
    listener: String,
    upstream_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OfficialStatusProjection {
    listener: String,
    process_id: Option<u32>,
    upstream_base_url: Option<String>,
    gateway_base_url: Option<String>,
    config_path: Option<String>,
    state_path: Option<String>,
    state_root: Option<String>,
    log_path: Option<String>,
    instance_nonce: Option<String>,
    provider_name: Option<String>,
}

fn object_string(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(|candidate| candidate.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn object_u32(map: &serde_json::Map<String, Value>, key: &str) -> Option<u32> {
    map.get(key)
        .and_then(|candidate| candidate.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

fn normalize_reported_listener(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = if trimmed.contains("://") {
        normalize_listener(trimmed).ok()?
    } else {
        normalize_listener(&format!("http://{trimmed}")).ok()?
    };
    Some(normalized)
}

fn parse_official_health_payload(value: &Value) -> Option<OfficialHealthProjection> {
    let object = value.as_object()?;
    if object.get("ok").and_then(|candidate| candidate.as_bool()) != Some(true) {
        return None;
    }
    Some(OfficialHealthProjection {
        listener: normalize_reported_listener(&object_string(object, "listen")?)?,
        upstream_base_url: object_string(object, "upstream_base_url")?,
    })
}

fn parse_official_status_payload(value: &Value) -> Option<OfficialStatusProjection> {
    let object = value.as_object()?;
    if object.get("ok").and_then(|candidate| candidate.as_bool()) != Some(true) {
        return None;
    }
    let state = object.get("state")?.as_object()?;
    let paths = object.get("paths")?.as_object()?;
    Some(OfficialStatusProjection {
        listener: normalize_reported_listener(&object_string(object, "listen")?)?,
        process_id: object_u32(state, "process_id"),
        upstream_base_url: object_string(state, "original_base_url"),
        gateway_base_url: object_string(state, "gateway_base_url"),
        config_path: object_string(paths, "config_path"),
        state_path: object_string(paths, "state_path"),
        state_root: object_string(paths, "state_root"),
        log_path: object_string(paths, "log_path"),
        instance_nonce: object_string(state, "aio_instance_nonce"),
        provider_name: object_string(state, "provider_name"),
    })
}

fn project_health_snapshot(
    health: Option<Value>,
    status: Option<Value>,
) -> Option<CodexRetryGatewayHealthSnapshot> {
    let health = parse_official_health_payload(&health?)?;
    let status = parse_official_status_payload(&status?)?;
    if health.listener != status.listener {
        return None;
    }
    if status
        .upstream_base_url
        .as_deref()
        .is_some_and(|value| value != health.upstream_base_url)
    {
        return None;
    }

    Some(CodexRetryGatewayHealthSnapshot {
        listener: health.listener,
        process_id: status.process_id,
        upstream_base_url: Some(health.upstream_base_url),
        gateway_base_url: status.gateway_base_url,
        config_path: status.config_path,
        state_path: status.state_path,
        state_root: status.state_root,
        log_path: status.log_path,
        instance_nonce: status.instance_nonce,
        provider_name: status.provider_name,
    })
}

fn process_matches_identity(pid: u32, expected_start_identity: Option<u64>) -> bool {
    if pid == 0 {
        return false;
    }
    let Some(expected_start_identity) = expected_start_identity else {
        return false;
    };
    let Some(current) = process_start_identity(pid) else {
        return false;
    };
    expected_start_identity == current
}

fn process_executable_matches(pid: u32, expected: &Path) -> bool {
    let Some(actual) = process_executable_path(pid) else {
        return false;
    };
    let Ok(actual) = std::fs::canonicalize(actual) else {
        return false;
    };
    let Ok(expected) = std::fs::canonicalize(expected) else {
        return false;
    };
    actual == expected
}

#[cfg(windows)]
fn process_executable_path(pid: u32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0_u16; 32_768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(handle);
        if ok == 0 || length == 0 {
            return None;
        }
        Some(PathBuf::from(std::ffi::OsString::from_wide(
            &buffer[..length as usize],
        )))
    }
}

#[cfg(target_os = "linux")]
fn process_executable_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_executable_path(pid: u32) -> Option<PathBuf> {
    let output = Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(not(any(windows, unix)))]
fn process_executable_path(_pid: u32) -> Option<PathBuf> {
    None
}

fn terminate_process_by_identity(pid: u32, expected_start_identity: Option<u64>) -> AppResult<()> {
    if !process_matches_identity(pid, expected_start_identity) {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!("refusing to terminate unmanaged process {pid}"),
        ));
    }
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_PROCESS_STOP_FAILED",
                    format!("failed to invoke taskkill for {pid}: {err}"),
                )
            })?;
        if status.success() {
            return Ok(());
        }
        Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_STOP_FAILED",
            format!("taskkill failed for process {pid} with status {status}"),
        ))
    }
    #[cfg(not(windows))]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_PROCESS_STOP_FAILED",
                    format!("failed to invoke kill for {pid}: {err}"),
                )
            })?;
        if status.success() {
            return Ok(());
        }
        Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_STOP_FAILED",
            format!("kill failed for process {pid} with status {status}"),
        ))
    }
}

#[cfg(windows)]
fn process_start_identity(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_QUERY_INFORMATION,
            0,
            pid,
        );
        if handle.is_null() {
            return None;
        }
        let mut created = std::mem::zeroed();
        let mut exited = std::mem::zeroed();
        let mut kernel = std::mem::zeroed();
        let mut user = std::mem::zeroed();
        let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }
}

#[cfg(target_os = "linux")]
fn process_start_identity(pid: u32) -> Option<u64> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|value| parse_linux_proc_start_identity(&value))
        .or_else(|| unix_ps_start_identity(pid))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_start_identity(pid: u32) -> Option<u64> {
    unix_ps_start_identity(pid)
}

#[cfg(not(any(windows, unix)))]
fn process_start_identity(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
pub(crate) fn process_start_identity_for_tests(pid: u32) -> Option<u64> {
    process_start_identity(pid)
}

#[cfg(unix)]
fn unix_ps_start_identity(pid: u32) -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_unix_ps_start_identity(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(any(test, target_os = "linux"))]
fn parse_linux_proc_start_identity(stat: &str) -> Option<u64> {
    let (_, tail) = stat.rsplit_once(')')?;
    tail.split_whitespace().nth(19)?.parse::<u64>().ok()
}

#[cfg(any(test, unix))]
fn parse_unix_ps_start_identity(output: &str) -> Option<u64> {
    let fields = output.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 5 {
        return None;
    }
    let month = match fields[1] {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let day = fields[2]
        .parse::<u64>()
        .ok()
        .filter(|value| (1..=31).contains(value))?;
    let mut time_parts = fields[3].split(':');
    let hour = time_parts
        .next()?
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= 23)?;
    let minute = time_parts
        .next()?
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= 59)?;
    let second = time_parts
        .next()?
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= 59)?;
    if time_parts.next().is_some() {
        return None;
    }
    let year = fields[4]
        .parse::<u64>()
        .ok()
        .filter(|value| *value >= 1970)?;
    Some((((((year * 100) + month) * 100 + day) * 100 + hour) * 100 + minute) * 100 + second)
}

fn validate_managed_record(
    paths: &CodexRetryGatewayManagerPaths,
    record: &CodexRetryGatewayManagedProcessRecord,
) -> AppResult<ValidatedManagedRecord> {
    let mut normalized = record.clone();
    normalized.validate()?;
    let start_identity = normalized.start_identity.ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            "managed process record is missing a start identity",
        )
    })?;
    let canonical_node = canonicalize_absolute_existing_path(
        &normalized.node_executable,
        "managed Node executable",
    )?;
    if normalized.node_executable != canonical_node.display().to_string() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!(
                "managed Node executable must be stored canonically, got {}",
                normalized.node_executable
            ),
        ));
    }
    validate_exact_record_path(
        &normalized.source_dir_rel,
        &relative_to_root(&paths.root, &paths.source_dir(&normalized.source_commit)?)?,
        "managed source directory",
    )?;
    let source_dir = canonicalize_path_within_root(
        &paths.root,
        &normalized.source_dir(paths)?,
        "managed source directory",
    )
    .map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            err.to_string(),
        )
    })?;
    let config_path = validate_managed_runtime_path(
        &paths.root,
        &normalized.config_path_rel,
        &relative_to_root(&paths.root, &paths.runtime_config_path)?,
        &normalized.config_path(paths)?,
        "managed runtime config",
    )?;
    let state_path = validate_managed_runtime_path(
        &paths.root,
        &normalized.state_path_rel,
        &relative_to_root(&paths.root, &paths.runtime_state_path)?,
        &normalized.state_path(paths)?,
        "managed runtime state",
    )?;
    let log_path = validate_managed_runtime_path(
        &paths.root,
        &normalized.log_path_rel,
        &relative_to_root(&paths.root, &paths.runtime_log_path)?,
        &normalized.log_path(paths)?,
        "managed runtime log",
    )?;
    let state_root =
        canonicalize_path_within_root(&paths.root, &paths.runtime_dir, "managed runtime root")
            .map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
                    err.to_string(),
                )
            })?;
    let listener = normalize_listener(&normalized.listener)?;
    if normalized.listener != listener {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!(
                "managed listener must be stored canonically, got {}",
                normalized.listener
            ),
        ));
    }
    Ok(ValidatedManagedRecord {
        start_identity,
        node_executable: canonical_node,
        listener,
        source_dir,
        config_path,
        state_path,
        state_root,
        log_path,
    })
}

fn validate_exact_record_path(recorded: &str, expected: &str, label: &str) -> AppResult<()> {
    let recorded = normalized_internal_relative_path(recorded)?;
    let expected = normalized_internal_relative_path(expected)?;
    if recorded == expected {
        return Ok(());
    }
    Err(AppError::new(
        "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
        format!(
            "{label} {} no longer matches {}",
            recorded.display(),
            expected.display()
        ),
    ))
}

fn validate_managed_runtime_path(
    root: &Path,
    recorded_relative: &str,
    expected_relative: &str,
    recorded_absolute: &Path,
    label: &str,
) -> AppResult<PathBuf> {
    validate_exact_record_path(recorded_relative, expected_relative, label)?;
    canonicalize_path_within_root(root, recorded_absolute, label).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            err.to_string(),
        )
    })
}

fn canonicalize_existing_path(path: &Path, label: &str) -> AppResult<PathBuf> {
    ensure_not_symlink_or_reparse(path, label).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            err.to_string(),
        )
    })?;
    std::fs::canonicalize(path).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!("failed to canonicalize {label} {}: {err}", path.display()),
        )
    })
}

fn canonicalize_absolute_existing_path(raw: &str, label: &str) -> AppResult<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!("{label} must not be empty"),
        ));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!("{label} must be an absolute path"),
        ));
    }
    canonicalize_existing_path(path, label)
}

fn normalize_listener(listener: &str) -> AppResult<String> {
    let url = reqwest::Url::parse(listener.trim()).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!("managed listener is invalid: {err}"),
        )
    })?;
    if url.scheme() != "http" {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            "managed listener must use http",
        ));
    }
    if url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            "managed listener must not contain a path, query, or fragment",
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            "managed listener is missing a host",
        )
    })?;
    if !is_loopback_host(host) {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!("managed listener host {host} is not loopback"),
        ));
    }
    let port = url.port_or_known_default().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            "managed listener is missing a port",
        )
    })?;
    Ok(format!("http://{host}:{port}"))
}

fn reported_path_matches(raw: &str, expected: &Path) -> bool {
    canonicalize_absolute_existing_path(raw, "reported managed path")
        .map(|path| path == expected)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::codex_retry_gateway::managed_state::CodexRetryGatewaySourceManifest;
    use crate::infra::codex_retry_gateway::{
        CodexRetryGatewayInstalledSource, CodexRetryGatewayNodeResolutionSource,
        CodexRetryGatewayResolvedNode, CodexRetryGatewayResolvedNodeVersion,
    };
    use axum::routing::get;
    use axum::{Json, Router};
    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};

    struct ManagedFixture {
        _dir: TempDir,
        paths: CodexRetryGatewayManagerPaths,
        record: CodexRetryGatewayManagedProcessRecord,
        validated: ValidatedManagedRecord,
        health: CodexRetryGatewayHealthSnapshot,
    }

    fn pending_launch_from_record(
        record: &CodexRetryGatewayManagedProcessRecord,
    ) -> CodexRetryGatewayPendingLaunchRecord {
        CodexRetryGatewayPendingLaunchRecord {
            created_at_ms: record.started_at_ms,
            node_executable: record.node_executable.clone(),
            source_commit: record.source_commit.clone(),
            source_dir_rel: record.source_dir_rel.clone(),
            config_path_rel: record.config_path_rel.clone(),
            state_path_rel: record.state_path_rel.clone(),
            log_path_rel: record.log_path_rel.clone(),
            listener: record.listener.clone(),
            upstream_base_url: record.upstream_base_url.clone(),
            instance_nonce: record.instance_nonce.clone(),
            provider_name: record.provider_name.clone(),
        }
    }

    async fn spawn_pending_health_server(
        paths: &CodexRetryGatewayManagerPaths,
        process_id: u32,
        instance_nonce: &str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind((DEFAULT_LISTEN_HOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let listener_url = format!("http://{DEFAULT_LISTEN_HOST}:{port}");
        let health_body = serde_json::json!({
            "ok": true,
            "listen": format!("{DEFAULT_LISTEN_HOST}:{port}"),
            "upstream_base_url": "http://127.0.0.1:37123/v1",
            "ui_path": "/__codex_retry_gateway/ui"
        });
        let status_body = serde_json::json!({
            "ok": true,
            "listen": format!("{DEFAULT_LISTEN_HOST}:{port}"),
            "state": {
                "process_id": process_id,
                "original_base_url": "http://127.0.0.1:37123/v1",
                "gateway_base_url": listener_url.clone(),
                "aio_instance_nonce": instance_nonce,
                "provider_name": crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO
            },
            "paths": {
                "config_path": std::fs::canonicalize(&paths.runtime_config_path).unwrap(),
                "state_path": std::fs::canonicalize(&paths.runtime_state_path).unwrap(),
                "state_root": std::fs::canonicalize(&paths.runtime_dir).unwrap(),
                "log_path": std::fs::canonicalize(&paths.runtime_log_path).unwrap()
            }
        });
        let server = tokio::spawn(async move {
            let router = Router::new()
                .route(
                    DEFAULT_HEALTH_PATH,
                    get(move || {
                        let body = health_body.clone();
                        async move { Json(body) }
                    }),
                )
                .route(
                    "/__codex_retry_gateway/api/status",
                    get(move || {
                        let body = status_body.clone();
                        async move { Json(body) }
                    }),
                );
            let _ = axum::serve(listener, router).await;
        });
        (listener_url, server)
    }

    fn node_fixture_name() -> &'static str {
        #[cfg(windows)]
        {
            "node.exe"
        }
        #[cfg(not(windows))]
        {
            "node"
        }
    }

    fn managed_fixture() -> ManagedFixture {
        let dir = tempdir().expect("tempdir");
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        paths.ensure_dirs().expect("dirs");
        let commit = "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2";
        let source_dir = paths.source_dir(commit).expect("source dir");
        std::fs::create_dir_all(&source_dir).expect("source dir create");
        std::fs::write(&paths.runtime_config_path, b"{}").expect("config");
        std::fs::write(&paths.runtime_state_path, b"{}").expect("state");
        std::fs::write(&paths.runtime_log_path, b"").expect("log");
        let node_executable = dir.path().join(node_fixture_name());
        std::fs::write(&node_executable, b"node").expect("node");
        let listener = format!(
            "http://{DEFAULT_LISTEN_HOST}:{}",
            bind_random_loopback_port(DEFAULT_LISTEN_HOST).expect("port")
        );
        let record = CodexRetryGatewayManagedProcessRecord {
            pid: 7,
            start_identity: Some(42),
            started_at_ms: 1,
            node_executable: std::fs::canonicalize(&node_executable)
                .expect("node canonical")
                .display()
                .to_string(),
            source_commit: commit.to_string(),
            source_dir_rel: relative_to_root(&paths.root, &source_dir).expect("source rel"),
            config_path_rel: relative_to_root(&paths.root, &paths.runtime_config_path)
                .expect("config rel"),
            state_path_rel: relative_to_root(&paths.root, &paths.runtime_state_path)
                .expect("state rel"),
            log_path_rel: relative_to_root(&paths.root, &paths.runtime_log_path).expect("log rel"),
            listener: listener.clone(),
            upstream_base_url: "http://127.0.0.1:37123/v1".to_string(),
            instance_nonce: "deadbeef".to_string(),
            provider_name: crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO
                .to_string(),
        };
        let validated = validate_managed_record(&paths, &record).expect("validated");
        let health = CodexRetryGatewayHealthSnapshot {
            listener: listener.clone(),
            process_id: Some(record.pid),
            upstream_base_url: Some(record.upstream_base_url.clone()),
            gateway_base_url: Some(listener),
            config_path: Some(
                std::fs::canonicalize(&paths.runtime_config_path)
                    .expect("config canonical")
                    .display()
                    .to_string(),
            ),
            state_path: Some(
                std::fs::canonicalize(&paths.runtime_state_path)
                    .expect("state canonical")
                    .display()
                    .to_string(),
            ),
            state_root: Some(
                std::fs::canonicalize(&paths.runtime_dir)
                    .expect("runtime canonical")
                    .display()
                    .to_string(),
            ),
            log_path: Some(
                std::fs::canonicalize(&paths.runtime_log_path)
                    .expect("log canonical")
                    .display()
                    .to_string(),
            ),
            instance_nonce: Some(record.instance_nonce.clone()),
            provider_name: Some(
                crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO.to_string(),
            ),
        };
        ManagedFixture {
            _dir: dir,
            paths,
            record,
            validated,
            health,
        }
    }

    #[tokio::test]
    async fn pending_launch_without_health_is_cleared_and_prior_record_is_preserved() {
        let fixture = managed_fixture();
        let manager = CodexRetryGatewayManagerState {
            process_record: Some(fixture.record.clone()),
            pending_launch: Some(pending_launch_from_record(&fixture.record)),
            ..Default::default()
        };
        write_manager_state(&fixture.paths, &manager).unwrap();
        let prior_record = read_manager_state(&fixture.paths).unwrap().process_record;

        let recovered =
            reconcile_pending_runtime_launch_with_timeout(&fixture.paths, Duration::ZERO)
                .await
                .unwrap();

        assert!(recovered.is_none());
        let persisted = read_manager_state(&fixture.paths).unwrap();
        assert!(persisted.pending_launch.is_none());
        assert_eq!(persisted.process_record, prior_record);
    }

    #[tokio::test]
    async fn pending_launch_is_promoted_only_from_complete_owned_health() {
        let fixture = managed_fixture();
        let process_id = std::process::id();
        let expected_start_identity =
            process_start_identity(process_id).expect("current process start identity");
        let (listener, server) =
            spawn_pending_health_server(&fixture.paths, process_id, "deadbeef").await;
        let mut pending = pending_launch_from_record(&fixture.record);
        pending.listener = listener.clone();
        pending.node_executable = std::fs::canonicalize(std::env::current_exe().unwrap())
            .unwrap()
            .display()
            .to_string();
        let manager = CodexRetryGatewayManagerState {
            pending_launch: Some(pending),
            ..Default::default()
        };
        write_manager_state(&fixture.paths, &manager).unwrap();

        let recovered =
            reconcile_pending_runtime_launch_with_timeout(&fixture.paths, Duration::from_secs(1))
                .await
                .unwrap()
                .expect("matching pending launch must be promoted");
        server.abort();

        assert_eq!(recovered.pid, process_id);
        assert_eq!(recovered.start_identity, Some(expected_start_identity));
        assert_eq!(recovered.listener, listener);
        let persisted = read_manager_state(&fixture.paths).unwrap();
        assert!(persisted.pending_launch.is_none());
        assert_eq!(persisted.process_record.as_ref(), Some(&recovered));
        assert_eq!(persisted.effective_port, listener_port(&listener));
    }

    #[tokio::test]
    async fn pending_launch_with_mismatched_nonce_is_not_adopted_or_terminated() {
        let fixture = managed_fixture();
        let process_id = std::process::id();
        let start_identity =
            process_start_identity(process_id).expect("current process start identity");
        let (listener, server) =
            spawn_pending_health_server(&fixture.paths, process_id, "foreign-nonce").await;
        let mut pending = pending_launch_from_record(&fixture.record);
        pending.listener = listener;
        pending.node_executable = std::fs::canonicalize(std::env::current_exe().unwrap())
            .unwrap()
            .display()
            .to_string();
        let manager = CodexRetryGatewayManagerState {
            pending_launch: Some(pending),
            ..Default::default()
        };
        write_manager_state(&fixture.paths, &manager).unwrap();

        let recovered = reconcile_pending_runtime_launch_with_timeout(
            &fixture.paths,
            Duration::from_millis(20),
        )
        .await
        .unwrap();
        server.abort();

        assert!(recovered.is_none());
        assert!(process_matches_identity(process_id, Some(start_identity)));
        let persisted = read_manager_state(&fixture.paths).unwrap();
        assert!(persisted.pending_launch.is_none());
        assert!(persisted.process_record.is_none());
    }

    #[tokio::test]
    async fn pending_launch_spawn_failure_restores_the_exact_prior_manager() {
        let fixture = managed_fixture();
        let prior = CodexRetryGatewayManagerState {
            generation: 17,
            active_commit: Some(fixture.record.source_commit.clone()),
            ..Default::default()
        };
        write_manager_state(&fixture.paths, &prior).unwrap();
        let prior = read_manager_state(&fixture.paths).unwrap();
        let source = CodexRetryGatewayInstalledSource {
            source_dir: fixture.record.source_dir(&fixture.paths).unwrap(),
            manifest: CodexRetryGatewaySourceManifest {
                schema_version: 1,
                repository: crate::infra::codex_retry_gateway::CODEX_RETRY_GATEWAY_REPOSITORY
                    .to_string(),
                commit: fixture.record.source_commit.clone(),
                verified_main_commit: fixture.record.source_commit.clone(),
                verified_at_ms: 1,
                archive_sha256: "0".repeat(64),
                source_sha256: "0".repeat(64),
                file_count: 0,
                total_bytes: 0,
                gateway_entry_rel: "gateway.mjs".to_string(),
                admin_entry_rel: "scripts/admin-lib.mjs".to_string(),
                launch_ui_entry_rel: "scripts/launch-ui.mjs".to_string(),
            },
        };
        let node = CodexRetryGatewayResolvedNode {
            executable: PathBuf::from(&fixture.record.node_executable),
            version: CodexRetryGatewayResolvedNodeVersion {
                raw: "v20.0.0".to_string(),
                major: 20,
            },
            source: CodexRetryGatewayNodeResolutionSource::ProcessPath,
        };

        let error = start_runtime_process(
            &fixture.paths,
            &source,
            &node,
            &AioGatewayOrigin {
                url: fixture.record.upstream_base_url.clone(),
            },
            &fixture.record.provider_name,
            CODEX_RETRY_GATEWAY_DEFAULT_PORT,
            None,
        )
        .await
        .expect_err("non-executable Node fixture must fail at spawn");

        assert_eq!(error.code(), "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED");
        assert_eq!(read_manager_state(&fixture.paths).unwrap(), prior);
    }

    #[test]
    fn choose_listen_port_prefers_explicit_candidates() {
        let port = choose_listen_port(Some(4621), 4622).expect("choose port");
        assert!(port >= 1024);
    }

    #[test]
    fn relative_to_root_normalizes_windows_style_separators() {
        let root = PathBuf::from("D:/gateway");
        let path = root.join("runtime").join("config").join("config.json");
        assert_eq!(
            relative_to_root(&root, &path).expect("relative"),
            "runtime/config/config.json"
        );
    }

    #[test]
    fn health_match_rejects_missing_required_fields() {
        let fixture = managed_fixture();
        assert!(health_matches_record(
            &fixture.health,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_pid = fixture.health.clone();
        missing_pid.process_id = None;
        assert!(!health_matches_record(
            &missing_pid,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_upstream = fixture.health.clone();
        missing_upstream.upstream_base_url = None;
        assert!(!health_matches_record(
            &missing_upstream,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_config = fixture.health.clone();
        missing_config.config_path = None;
        assert!(!health_matches_record(
            &missing_config,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_state = fixture.health.clone();
        missing_state.state_root = None;
        assert!(!health_matches_record(
            &missing_state,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_nonce = fixture.health.clone();
        missing_nonce.instance_nonce = None;
        assert!(!health_matches_record(
            &missing_nonce,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_provider = fixture.health.clone();
        missing_provider.provider_name = None;
        assert!(!health_matches_record(
            &missing_provider,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_state_path = fixture.health.clone();
        missing_state_path.state_path = None;
        assert!(!health_matches_record(
            &missing_state_path,
            &fixture.record,
            &fixture.validated
        ));

        let mut missing_log_path = fixture.health.clone();
        missing_log_path.log_path = None;
        assert!(!health_matches_record(
            &missing_log_path,
            &fixture.record,
            &fixture.validated
        ));
    }

    #[test]
    fn health_match_rejects_listener_config_state_and_provider_mismatch() {
        let fixture = managed_fixture();

        let mut listener_mismatch = fixture.health.clone();
        listener_mismatch.listener = "http://127.0.0.1:9999".to_string();
        assert!(!health_matches_record(
            &listener_mismatch,
            &fixture.record,
            &fixture.validated
        ));

        let mut gateway_mismatch = fixture.health.clone();
        gateway_mismatch.gateway_base_url = Some("http://127.0.0.1:9999".to_string());
        assert!(!health_matches_record(
            &gateway_mismatch,
            &fixture.record,
            &fixture.validated
        ));

        let mut config_mismatch = fixture.health.clone();
        config_mismatch.config_path = Some(
            fixture
                .paths
                .runtime_config_dir
                .join("other.json")
                .display()
                .to_string(),
        );
        assert!(!health_matches_record(
            &config_mismatch,
            &fixture.record,
            &fixture.validated
        ));

        let mut state_mismatch = fixture.health.clone();
        state_mismatch.state_root = Some(
            fixture
                .paths
                .runtime_dir
                .join("other-root")
                .display()
                .to_string(),
        );
        assert!(!health_matches_record(
            &state_mismatch,
            &fixture.record,
            &fixture.validated
        ));

        let mut state_path_mismatch = fixture.health.clone();
        state_path_mismatch.state_path = Some(
            fixture
                .paths
                .runtime_dir
                .join("other-state.json")
                .display()
                .to_string(),
        );
        assert!(!health_matches_record(
            &state_path_mismatch,
            &fixture.record,
            &fixture.validated
        ));

        let mut log_path_mismatch = fixture.health.clone();
        log_path_mismatch.log_path = Some(
            fixture
                .paths
                .runtime_logs_dir
                .join("other.log")
                .display()
                .to_string(),
        );
        assert!(!health_matches_record(
            &log_path_mismatch,
            &fixture.record,
            &fixture.validated
        ));

        let mut provider_mismatch = fixture.health.clone();
        provider_mismatch.provider_name = Some("other".to_string());
        assert!(!health_matches_record(
            &provider_mismatch,
            &fixture.record,
            &fixture.validated
        ));
    }

    #[test]
    fn validate_managed_record_rejects_missing_start_identity() {
        let fixture = managed_fixture();
        let mut record = fixture.record.clone();
        record.start_identity = None;
        assert!(validate_managed_record(&fixture.paths, &record).is_err());
    }

    #[test]
    fn stop_runtime_process_returns_false_for_mismatched_record() {
        let fixture = managed_fixture();
        let mut record = fixture.record.clone();
        record.config_path_rel = "runtime/config/other.json".to_string();
        let stopped = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(stop_runtime_process(&fixture.paths, &record))
            .expect("stop");
        assert!(!stopped);
    }

    #[test]
    fn project_health_snapshot_preserves_reported_gateway_base_url() {
        let snapshot = project_health_snapshot(
            Some(serde_json::json!({
                "ok": true,
                "listen": "127.0.0.1:4610",
                "upstream_base_url": "http://127.0.0.1:37123/v1",
                "ui_path": "/__codex_retry_gateway/ui"
            })),
            Some(serde_json::json!({
                "ok": true,
                "listen": "127.0.0.1:4610",
                "state": {
                    "process_id": 7,
                    "original_base_url": "http://127.0.0.1:37123/v1",
                    "gateway_base_url": "http://127.0.0.1:4999",
                    "aio_instance_nonce": "deadbeef",
                    "provider_name": "aio"
                },
                "paths": {
                    "config_path": "C:/managed/config.json",
                    "state_path": "C:/managed/state.json",
                    "state_root": "C:/managed/runtime",
                    "log_path": "C:/managed/gateway.log"
                }
            })),
        )
        .expect("snapshot");
        assert_eq!(snapshot.listener, "http://127.0.0.1:4610");
        assert_eq!(
            snapshot.gateway_base_url,
            Some("http://127.0.0.1:4999".to_string())
        );
    }

    #[tokio::test]
    async fn malformed_success_payload_is_treated_as_unhealthy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot-json",
                )
                .await
                .unwrap();
        });

        let client = build_process_client().unwrap();
        let result = fetch_gateway_json(
            &client,
            &format!("http://{address}/health"),
            "gateway health",
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn parse_linux_proc_start_identity_reads_field_22_after_command_name() {
        assert_eq!(
            parse_linux_proc_start_identity(
                "123 (gateway worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654"
            ),
            Some(987654)
        );
    }

    #[test]
    fn parse_unix_ps_start_identity_normalizes_space_padded_day() {
        assert_eq!(
            parse_unix_ps_start_identity("Wed Jul  5 12:34:56 2026"),
            Some(20260705123456)
        );
        assert!(parse_unix_ps_start_identity("bad output").is_none());
    }

    #[test]
    fn process_matches_identity_requires_expected_start_identity() {
        assert!(!process_matches_identity(1234, None));
    }

    #[test]
    fn process_executable_match_uses_the_live_process_image() {
        let current = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert!(process_executable_matches(std::process::id(), &current));
        assert!(!process_executable_matches(
            std::process::id(),
            &current.with_file_name("not-the-current-process")
        ));
    }

    #[cfg(windows)]
    fn discover_smoke_node() -> PathBuf {
        let output = Command::new("where")
            .arg("node")
            .output()
            .expect("where node");
        assert!(output.status.success(), "where node must succeed for smoke");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
            .expect("node path")
    }

    #[cfg(not(windows))]
    fn discover_smoke_node() -> PathBuf {
        let output = Command::new("which")
            .arg("node")
            .output()
            .expect("which node");
        assert!(output.status.success(), "which node must succeed for smoke");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
            .expect("node path")
    }

    #[tokio::test]
    #[ignore = "manual smoke against the official read-only checkout"]
    async fn official_gateway_smoke_start_reconcile_stop() {
        let official_root = std::env::var("AIO_GATEWAY_OFFICIAL_CHECKOUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"D:\UGit\codex-retry-gateway"));
        assert!(
            official_root.join("gateway.mjs").exists(),
            "official checkout must contain gateway.mjs"
        );

        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        let managed_source_dir = paths
            .source_dir("ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2")
            .expect("managed source dir");
        crate::shared::fs::copy_dir_recursive_if_missing(&official_root, &managed_source_dir)
            .expect("copy official source into managed root");
        let node = CodexRetryGatewayResolvedNode {
            executable: discover_smoke_node(),
            version: CodexRetryGatewayResolvedNodeVersion {
                raw: "v20.0.0".to_string(),
                major: 20,
            },
            source: CodexRetryGatewayNodeResolutionSource::ProcessPath,
        };
        let source = CodexRetryGatewayInstalledSource {
            source_dir: managed_source_dir,
            manifest: CodexRetryGatewaySourceManifest {
                schema_version: 1,
                repository: crate::infra::codex_retry_gateway::CODEX_RETRY_GATEWAY_REPOSITORY
                    .to_string(),
                commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
                verified_main_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
                verified_at_ms: 1,
                archive_sha256: "0".repeat(64),
                source_sha256: "0".repeat(64),
                file_count: 0,
                total_bytes: 0,
                gateway_entry_rel: "gateway.mjs".to_string(),
                admin_entry_rel: "scripts/admin-lib.mjs".to_string(),
                launch_ui_entry_rel: "scripts/launch-ui.mjs".to_string(),
            },
        };
        let aio_origin = AioGatewayOrigin {
            url: "http://127.0.0.1:37123/v1".to_string(),
        };

        let process = start_runtime_process(
            &paths,
            &source,
            &node,
            &aio_origin,
            crate::infra::codex_retry_gateway::config::MANAGED_PROVIDER_AIO,
            CODEX_RETRY_GATEWAY_DEFAULT_PORT,
            None,
        )
        .await
        .expect("start runtime process");
        let reconciled = reconcile_runtime_process(&paths, Some(&process.record), None)
            .await
            .expect("reconcile runtime process");
        assert!(
            reconciled.managed.is_some(),
            "managed process must reconcile"
        );
        assert!(
            stop_runtime_process(&paths, &process.record)
                .await
                .expect("stop runtime process"),
            "managed process must stop cleanly"
        );
    }
}
