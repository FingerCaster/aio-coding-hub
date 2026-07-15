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
use std::path::Path;
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
    let config = managed_gateway_config(listen_port, aio_origin);
    let state = managed_gateway_state(
        &listener,
        &paths.root.display().to_string(),
        &paths.runtime_config_path.display().to_string(),
        &paths.runtime_log_path.display().to_string(),
        &paths.runtime_pid_path.display().to_string(),
        &aio_origin.url,
        "aio",
        &instance_nonce,
    );
    let config_bytes = json_file_bytes(&config, "managed gateway config")?;
    let state_bytes = json_file_bytes(&state, "managed gateway state")?;
    write_file_atomic(&paths.runtime_config_path, &config_bytes)?;
    write_file_atomic(&paths.runtime_state_path, &state_bytes)?;

    let mut command = Command::new(&node.executable);
    command.arg(source.source_dir.join("gateway.mjs"));
    command.arg("--config").arg(&paths.runtime_config_path);
    command.arg("--log").arg(&paths.runtime_log_path);
    command.current_dir(&source.source_dir);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.env("PATH", runtime_path_for_executable(&node.executable)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = command.spawn().map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_PROCESS_START_FAILED",
            format!(
                "failed to start managed gateway with {}: {err}",
                node.executable.display()
            ),
        )
    })?;
    let pid = child.id();
    let start_identity = process_start_identity(pid);
    drop(child);

    let health = wait_for_healthy_listener(&listener, pid, &aio_origin.url, &instance_nonce)
        .await
        .map_err(|error| {
            let _ = terminate_process_by_identity(pid, start_identity);
            error
        })?;

    write_file_atomic(&paths.runtime_pid_path, format!("{pid}\n").as_bytes())?;

    Ok(CodexRetryGatewayManagedProcess {
        record: CodexRetryGatewayManagedProcessRecord {
            pid,
            start_identity,
            started_at_ms: now_unix_ms(),
            node_executable: node.executable.display().to_string(),
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
    })
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
    let pid_matches = process_matches_identity(record.pid, record.start_identity);
    let record_valid = record.clone().validate().is_ok()
        && normalized_internal_relative_path(&record.config_path_rel).is_ok()
        && paths.internal_path(&record.config_path_rel).is_ok();

    match (pid_matches, health) {
        (true, Some(health)) if record_valid && health_matches_record(&health, record) => {
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
    let Some(primary) = health.or(status.clone()) else {
        return Ok(None);
    };

    let process_id = find_u32(&primary, &["process_id", "pid"]).or_else(|| {
        status
            .as_ref()
            .and_then(|value| find_u32(value, &["process_id", "pid"]))
    });
    let upstream_base_url = find_string(&primary, &["upstream_base_url", "original_base_url"])
        .or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["upstream_base_url", "original_base_url"]))
        });
    let gateway_base_url = find_string(&primary, &["gateway_base_url", "base_url"])
        .or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["gateway_base_url", "base_url"]))
        })
        .or_else(|| Some(listener.to_string()));
    let config_path =
        find_string(&primary, &["gateway_config_path", "config_path"]).or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["gateway_config_path", "config_path"]))
        });
    let state_root = find_string(&primary, &["state_root"]);
    let instance_nonce =
        find_string(&primary, &["aio_instance_nonce", "instance_nonce"]).or_else(|| {
            status
                .as_ref()
                .and_then(|value| find_string(value, &["aio_instance_nonce", "instance_nonce"]))
        });
    let provider_name = find_string(&primary, &["provider_name"]);

    Ok(Some(CodexRetryGatewayHealthSnapshot {
        listener: gateway_base_url.unwrap_or_else(|| listener.to_string()),
        process_id,
        upstream_base_url,
        gateway_base_url: Some(listener.to_string()),
        config_path,
        state_root,
        instance_nonce,
        provider_name,
    }))
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
    upstream_base_url: &str,
    instance_nonce: &str,
) -> AppResult<CodexRetryGatewayHealthSnapshot> {
    let deadline = tokio::time::Instant::now() + PROCESS_HEALTH_TIMEOUT;
    loop {
        if !process_matches_identity(pid, process_start_identity(pid)) {
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
) -> bool {
    if let Some(process_id) = health.process_id {
        if process_id != record.pid {
            return false;
        }
    }
    if let Some(upstream) = health.upstream_base_url.as_deref() {
        if upstream.trim() != record.upstream_base_url {
            return false;
        }
    }
    if let Some(nonce) = health.instance_nonce.as_deref() {
        if nonce.trim() != record.instance_nonce {
            return false;
        }
    }
    if !is_loopback_listener(&record.listener) {
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

fn is_loopback_listener(listener: &str) -> bool {
    reqwest::Url::parse(listener)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .map(|host| is_loopback_host(&host))
        .unwrap_or(false)
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

fn process_matches_identity(pid: u32, expected_start_identity: Option<u64>) -> bool {
    if pid == 0 {
        return false;
    }
    let Some(current) = process_start_identity(pid) else {
        return false;
    };
    expected_start_identity
        .map(|expected| expected == current)
        .unwrap_or(true)
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

#[cfg(not(windows))]
fn process_start_identity(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn health_match_requires_pid_upstream_and_nonce_when_present() {
        let record = CodexRetryGatewayManagedProcessRecord {
            pid: 7,
            start_identity: None,
            started_at_ms: 1,
            node_executable: "node".to_string(),
            source_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            source_dir_rel: "sources/ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            config_path_rel: "runtime/config/config.json".to_string(),
            state_path_rel: "runtime/state.json".to_string(),
            log_path_rel: "runtime/logs/gateway.log".to_string(),
            listener: "http://127.0.0.1:4610".to_string(),
            upstream_base_url: "http://127.0.0.1:37123/v1".to_string(),
            instance_nonce: "abcd".to_string(),
        };
        let health = CodexRetryGatewayHealthSnapshot {
            listener: "http://127.0.0.1:4610".to_string(),
            process_id: Some(7),
            upstream_base_url: Some("http://127.0.0.1:37123/v1".to_string()),
            gateway_base_url: Some("http://127.0.0.1:4610".to_string()),
            config_path: None,
            state_root: None,
            instance_nonce: Some("abcd".to_string()),
            provider_name: Some("aio".to_string()),
        };
        assert!(health_matches_record(&health, &record));
    }
}
