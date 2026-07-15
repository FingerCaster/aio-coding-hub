use crate::infra::codex_retry_gateway::config::{DEFAULT_HEALTH_PATH, DEFAULT_LISTEN_HOST};
use crate::infra::codex_retry_gateway::managed_state::{
    CodexRetryGatewayManagedProcessRecord, CodexRetryGatewayManagerPaths,
};
use crate::infra::codex_retry_gateway::source::CodexRetryGatewayInstalledSource;
use crate::infra::codex_retry_gateway::util::{
    is_loopback_host, normalized_internal_relative_path, now_unix_ms, random_hex,
};
use crate::infra::codex_retry_gateway::{
    managed_gateway_config, managed_gateway_state, AioGatewayOrigin, CodexRetryGatewayError,
    CodexRetryGatewayErrorCategory, CodexRetryGatewayProcessPhase, CodexRetryGatewayProcessStatus,
    CodexRetryGatewayResolvedNode, CODEX_RETRY_GATEWAY_DEFAULT_PORT,
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
const MANAGED_PROVIDER_NAME: &str = "aio";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayHealthSnapshot {
    pub(crate) listener: String,
    pub(crate) process_id: Option<u32>,
    pub(crate) upstream_base_url: Option<String>,
    pub(crate) gateway_base_url: Option<String>,
    pub(crate) config_path: Option<String>,
    pub(crate) state_root: Option<String>,
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
    listener: String,
    config_path: PathBuf,
    state_root: PathBuf,
}

pub(crate) async fn start_runtime_process(
    paths: &CodexRetryGatewayManagerPaths,
    source: &CodexRetryGatewayInstalledSource,
    node: &CodexRetryGatewayResolvedNode,
    aio_origin: &AioGatewayOrigin,
    preferred_port: u16,
    persisted_port: Option<u16>,
) -> AppResult<CodexRetryGatewayManagedProcess> {
    paths.ensure_dirs()?;
    let listen_port = choose_listen_port(persisted_port, preferred_port)?;
    let listener = format!("http://{DEFAULT_LISTEN_HOST}:{listen_port}");
    let instance_nonce = random_hex(16);
    let node_executable = canonicalize_absolute_existing_path(
        &node.executable.display().to_string(),
        "managed Node executable",
    )?;
    let config = managed_gateway_config(listen_port, aio_origin);
    let state = managed_gateway_state(
        &listener,
        &paths.root.display().to_string(),
        &paths.runtime_config_path.display().to_string(),
        &paths.runtime_log_path.display().to_string(),
        &paths.runtime_pid_path.display().to_string(),
        &aio_origin.url,
        MANAGED_PROVIDER_NAME,
        &instance_nonce,
    );
    let config_bytes = json_file_bytes(&config, "managed gateway config")?;
    let state_bytes = json_file_bytes(&state, "managed gateway state")?;
    write_file_atomic(&paths.runtime_config_path, &config_bytes)?;
    write_file_atomic(&paths.runtime_state_path, &state_bytes)?;

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
    let mut child = command.spawn().map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED",
            format!(
                "failed to start managed gateway with {}: {err}",
                node_executable.display()
            ),
        )
    })?;
    let pid = child.id();
    let Some(start_identity) = process_start_identity(pid) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED",
            format!("failed to capture a stable start identity for managed gateway process {pid}"),
        ));
    };
    drop(child);

    let health = wait_for_healthy_listener(
        &listener,
        pid,
        start_identity,
        &aio_origin.url,
        &instance_nonce,
    )
    .await
    .map_err(|error| {
        let _ = terminate_process_by_identity(pid, Some(start_identity));
        error
    })?;
    let managed = CodexRetryGatewayManagedProcess {
        record: CodexRetryGatewayManagedProcessRecord {
            pid,
            start_identity: Some(start_identity),
            started_at_ms: now_unix_ms(),
            node_executable: node_executable.display().to_string(),
            source_commit: source.manifest.commit.clone(),
            source_dir_rel: relative_to_root(&paths.root, &source.source_dir)?,
            config_path_rel: relative_to_root(&paths.root, &paths.runtime_config_path)?,
            state_path_rel: relative_to_root(&paths.root, &paths.runtime_state_path)?,
            log_path_rel: relative_to_root(&paths.root, &paths.runtime_log_path)?,
            listener: listener.clone(),
            upstream_base_url: aio_origin.url.clone(),
            instance_nonce,
        },
        health,
    };
    let validated = validate_managed_record(paths, &managed.record)?;
    if !health_matches_record(&managed.health, &managed.record, &validated) {
        let _ = terminate_process_by_identity(pid, Some(start_identity));
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_HEALTH_TIMEOUT",
            "managed gateway health identity did not match the persisted runtime record",
        ));
    }

    write_file_atomic(&paths.runtime_pid_path, format!("{pid}\n").as_bytes())?;
    Ok(managed)
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
    if reconciled.managed.is_none() {
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
    Ok(project_health_snapshot(listener, health, status))
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
    let body = read_text_with_limit(response, PROCESS_STATUS_BODY_LIMIT, context)
        .await
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_HEALTH_PROBE_FAILED", err))?;
    serde_json::from_str(&body)
        .map(Some)
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_HEALTH_PROBE_FAILED", err.to_string()))
}

async fn wait_for_healthy_listener(
    listener: &str,
    pid: u32,
    start_identity: u64,
    upstream_base_url: &str,
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
    if health.provider_name.as_deref().map(str::trim) != Some(MANAGED_PROVIDER_NAME) {
        return false;
    }
    if !health
        .config_path
        .as_deref()
        .is_some_and(|path| reported_path_matches(path, &validated.config_path))
    {
        return false;
    }
    health
        .state_root
        .as_deref()
        .is_some_and(|path| reported_path_matches(path, &validated.state_root))
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

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(string) = map.get(*key).and_then(|candidate| candidate.as_str()) {
                    let trimmed = string.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for nested in map.values() {
                if let Some(found) = find_string(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| find_string(item, keys)),
        _ => None,
    }
}

fn find_u32(value: &Value, keys: &[&str]) -> Option<u32> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(found) = map
                    .get(*key)
                    .and_then(|candidate| candidate.as_u64())
                    .and_then(|value| u32::try_from(value).ok())
                {
                    return Some(found);
                }
            }
            for nested in map.values() {
                if let Some(found) = find_u32(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| find_u32(item, keys)),
        _ => None,
    }
}

fn project_health_snapshot(
    listener: &str,
    health: Option<Value>,
    status: Option<Value>,
) -> Option<CodexRetryGatewayHealthSnapshot> {
    let primary = health.as_ref().or(status.as_ref())?;
    let process_id = find_u32(primary, &["process_id", "pid"]).or_else(|| {
        status
            .as_ref()
            .and_then(|value| find_u32(value, &["process_id", "pid"]))
    });
    let upstream_base_url = find_string(primary, &["upstream_base_url", "original_base_url"])
        .or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["upstream_base_url", "original_base_url"]))
        });
    let gateway_base_url = find_string(primary, &["gateway_base_url", "base_url"])
        .or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["gateway_base_url", "base_url"]))
        })
        .or_else(|| Some(listener.to_string()));
    let config_path = find_string(primary, &["gateway_config_path", "config_path"]).or_else(|| {
        status
            .as_ref()
            .and_then(|value| find_string(value, &["gateway_config_path", "config_path"]))
    });
    let state_root = find_string(primary, &["state_root"]).or_else(|| {
        status
            .as_ref()
            .and_then(|value| find_string(value, &["state_root"]))
    });
    let instance_nonce =
        find_string(primary, &["aio_instance_nonce", "instance_nonce"]).or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["aio_instance_nonce", "instance_nonce"]))
        });
    let provider_name = find_string(primary, &["provider_name"]).or_else(|| {
        status
            .as_ref()
            .and_then(|value| find_string(value, &["provider_name"]))
    });
    let listener = gateway_base_url.unwrap_or_else(|| listener.to_string());

    Some(CodexRetryGatewayHealthSnapshot {
        listener: listener.clone(),
        process_id,
        upstream_base_url,
        gateway_base_url: Some(listener),
        config_path,
        state_root,
        instance_nonce,
        provider_name,
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
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_STOP_FAILED",
            format!("taskkill failed for process {pid} with status {status}"),
        ));
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
    let _ = canonicalize_existing_path(&normalized.source_dir(paths)?, "managed source directory")?;
    let config_path = validate_managed_runtime_path(
        &normalized.config_path_rel,
        &relative_to_root(&paths.root, &paths.runtime_config_path)?,
        &normalized.config_path(paths)?,
        "managed runtime config",
    )?;
    let _ = validate_managed_runtime_path(
        &normalized.state_path_rel,
        &relative_to_root(&paths.root, &paths.runtime_state_path)?,
        &normalized.state_path(paths)?,
        "managed runtime state",
    )?;
    validate_exact_record_path(
        &normalized.log_path_rel,
        &relative_to_root(&paths.root, &paths.runtime_log_path)?,
        "managed runtime log",
    )?;
    if normalized.log_path(paths)? != paths.runtime_log_path {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_OWNERSHIP_MISMATCH",
            format!(
                "managed runtime log path {} no longer matches {}",
                normalized.log_path(paths)?.display(),
                paths.runtime_log_path.display()
            ),
        ));
    }
    let state_root = canonicalize_existing_path(&paths.root, "managed runtime root")?;
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
        listener,
        config_path,
        state_root,
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
    recorded_relative: &str,
    expected_relative: &str,
    recorded_absolute: &Path,
    label: &str,
) -> AppResult<PathBuf> {
    validate_exact_record_path(recorded_relative, expected_relative, label)?;
    canonicalize_existing_path(recorded_absolute, label)
}

fn canonicalize_existing_path(path: &Path, label: &str) -> AppResult<PathBuf> {
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
    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};

    struct ManagedFixture {
        _dir: TempDir,
        paths: CodexRetryGatewayManagerPaths,
        record: CodexRetryGatewayManagedProcessRecord,
        validated: ValidatedManagedRecord,
        health: CodexRetryGatewayHealthSnapshot,
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
            state_root: Some(
                std::fs::canonicalize(&paths.root)
                    .expect("root canonical")
                    .display()
                    .to_string(),
            ),
            instance_nonce: Some(record.instance_nonce.clone()),
            provider_name: Some(MANAGED_PROVIDER_NAME.to_string()),
        };
        ManagedFixture {
            _dir: dir,
            paths,
            record,
            validated,
            health,
        }
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
        state_mismatch.state_root = Some(fixture.paths.runtime_dir.display().to_string());
        assert!(!health_matches_record(
            &state_mismatch,
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
            "http://127.0.0.1:4610",
            None,
            Some(serde_json::json!({
                "process_id": 7,
                "original_base_url": "http://127.0.0.1:37123/v1",
                "gateway_base_url": "http://127.0.0.1:4999",
                "gateway_config_path": "C:/managed/config.json",
                "state_root": "C:/managed",
                "aio_instance_nonce": "deadbeef",
                "provider_name": "aio"
            })),
        )
        .expect("snapshot");
        assert_eq!(snapshot.listener, "http://127.0.0.1:4999");
        assert_eq!(
            snapshot.gateway_base_url,
            Some("http://127.0.0.1:4999".to_string())
        );
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
}
