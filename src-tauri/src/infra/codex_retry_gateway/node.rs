use crate::infra::codex_retry_gateway::util::ensure_not_symlink_or_reparse;
use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayError, CodexRetryGatewayErrorCategory, CodexRetryGatewayNodeResolutionSource,
    CodexRetryGatewayNodeStatus,
};
use crate::shared::error::{AppError, AppResult};
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const NODE_VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const NODE_VERSION_OUTPUT_LIMIT: usize = 8 * 1024;
const NODE_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayResolvedNodeVersion {
    pub raw: String,
    pub major: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayResolvedNode {
    pub executable: PathBuf,
    pub version: CodexRetryGatewayResolvedNodeVersion,
    pub source: CodexRetryGatewayNodeResolutionSource,
}

#[derive(Debug)]
struct VersionProbeOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Clone)]
struct AutoNodeCandidate {
    executable: PathBuf,
    source: CodexRetryGatewayNodeResolutionSource,
}

pub(crate) fn resolve_node_runtime<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manual_override: Option<&str>,
) -> AppResult<CodexRetryGatewayResolvedNode> {
    if let Some(override_value) = manual_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let executable = validate_manual_override_path(override_value)?;
        let version = probe_node_executable(&executable)?;
        return Ok(CodexRetryGatewayResolvedNode {
            executable,
            version,
            source: CodexRetryGatewayNodeResolutionSource::ManualOverride,
        });
    }

    if let Some(resolved) = resolve_auto_node_candidates(auto_node_candidates(app)?)? {
        return Ok(resolved);
    }

    Err(AppError::new(
        "CODEX_RETRY_GATEWAY_NODE_MISSING",
        "failed to discover a Node.js 18+ runtime",
    ))
}

pub(crate) fn resolve_node_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manual_override: Option<&str>,
) -> CodexRetryGatewayNodeStatus {
    match resolve_node_runtime(app, manual_override) {
        Ok(node) => CodexRetryGatewayNodeStatus {
            available: true,
            executable: Some(node.executable.display().to_string()),
            version: Some(node.version.raw),
            source: node.source,
            error: None,
        },
        Err(err) => CodexRetryGatewayNodeStatus {
            available: false,
            executable: manual_override
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            version: None,
            source: if manual_override
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
            {
                CodexRetryGatewayNodeResolutionSource::ManualOverride
            } else {
                CodexRetryGatewayNodeResolutionSource::Unavailable
            },
            error: Some(public_node_error(&err)),
        },
    }
}

pub(crate) fn set_node_override<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    executable: Option<&str>,
) -> AppResult<CodexRetryGatewayNodeStatus> {
    let mut settings = crate::settings::read(app)?;
    if let Some(value) = executable.map(str::trim).filter(|value| !value.is_empty()) {
        let resolved = resolve_node_runtime(app, Some(value))?;
        settings.codex_retry_gateway_node_override = resolved.executable.display().to_string();
        crate::settings::write(app, &settings)?;
        return Ok(CodexRetryGatewayNodeStatus {
            available: true,
            executable: Some(resolved.executable.display().to_string()),
            version: Some(resolved.version.raw),
            source: CodexRetryGatewayNodeResolutionSource::ManualOverride,
            error: None,
        });
    }

    settings.codex_retry_gateway_node_override.clear();
    crate::settings::write(app, &settings)?;
    Ok(resolve_node_status(app, None))
}

pub(crate) fn public_node_error(error: &AppError) -> CodexRetryGatewayError {
    let rendered = error.to_string();
    let category = if rendered.contains("UNSUPPORTED") {
        CodexRetryGatewayErrorCategory::NodeUnsupported
    } else {
        CodexRetryGatewayErrorCategory::NodeMissing
    };
    CodexRetryGatewayError {
        code: error.code().to_string(),
        category,
        message: rendered
            .split_once(':')
            .map(|(_, message)| message.trim().to_string())
            .unwrap_or(rendered),
        retryable: false,
    }
}

fn validate_manual_override_path(raw: &str) -> AppResult<PathBuf> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_OVERRIDE_INVALID",
            "manual Node override must be an absolute path",
        ));
    }
    let metadata = ensure_not_symlink_or_reparse(&path, "manual Node override").map_err(|err| {
        AppError::new("CODEX_RETRY_GATEWAY_NODE_OVERRIDE_INVALID", err.to_string())
    })?;
    if !metadata.is_file() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_OVERRIDE_INVALID",
            "manual Node override must point to a file",
        ));
    }
    validate_real_node_executable_path(&path)
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_NODE_OVERRIDE_INVALID", err.to_string()))
}

fn resolve_auto_node_candidates(
    candidates: Vec<AutoNodeCandidate>,
) -> AppResult<Option<CodexRetryGatewayResolvedNode>> {
    resolve_auto_node_candidates_with(candidates, probe_node_executable)
}

fn resolve_auto_node_candidates_with<F>(
    candidates: Vec<AutoNodeCandidate>,
    mut probe: F,
) -> AppResult<Option<CodexRetryGatewayResolvedNode>>
where
    F: FnMut(&Path) -> AppResult<CodexRetryGatewayResolvedNodeVersion>,
{
    let mut last_error = None;
    for candidate in candidates {
        let executable = match validate_real_node_executable_path(&candidate.executable) {
            Ok(executable) => executable,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        match probe(&executable) {
            Ok(version) => {
                return Ok(Some(CodexRetryGatewayResolvedNode {
                    executable,
                    version,
                    source: candidate.source,
                }));
            }
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

fn validate_real_node_executable_path(path: &Path) -> AppResult<PathBuf> {
    let canonical = std::fs::canonicalize(path).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
            format!("failed to canonicalize {}: {err}", path.display()),
        )
    })?;
    let metadata = ensure_not_symlink_or_reparse(&canonical, "managed Node executable")
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED", err.to_string()))?;
    if !metadata.is_file() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
            format!("Node executable is not a file: {}", canonical.display()),
        ));
    }
    #[cfg(windows)]
    {
        let extension = canonical
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("cmd" | "bat")) {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
                format!(
                    "refusing to launch shell wrapper as managed Node runtime: {}",
                    canonical.display()
                ),
            ));
        }
    }
    Ok(canonical)
}

fn node_executable_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["node.exe", "node"]
    }
    #[cfg(not(windows))]
    {
        &["node"]
    }
}

fn is_path_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find_executable_in_dir(dir: &Path) -> Option<PathBuf> {
    node_executable_names()
        .iter()
        .map(|name| dir.join(name))
        .find(|path| is_path_executable(path))
}

fn find_codex_sibling_node<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Option<PathBuf>> {
    let codex = crate::cli_manager::resolve_executable_candidates(app, "codex")?
        .into_iter()
        .next();
    Ok(codex
        .as_deref()
        .and_then(Path::parent)
        .and_then(find_executable_in_dir))
}

fn auto_node_candidates<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Vec<AutoNodeCandidate>> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    if let Some(executable) = find_codex_sibling_node(app)? {
        push_auto_node_candidate(
            &mut candidates,
            &mut seen,
            executable,
            CodexRetryGatewayNodeResolutionSource::CodexSibling,
        );
    }

    for executable in find_aio_discovery_nodes(app)? {
        push_auto_node_candidate(
            &mut candidates,
            &mut seen,
            executable,
            CodexRetryGatewayNodeResolutionSource::AioDiscovery,
        );
    }

    for executable in find_process_path_nodes() {
        push_auto_node_candidate(
            &mut candidates,
            &mut seen,
            executable,
            CodexRetryGatewayNodeResolutionSource::ProcessPath,
        );
    }

    Ok(candidates)
}

fn push_auto_node_candidate(
    candidates: &mut Vec<AutoNodeCandidate>,
    seen: &mut HashSet<String>,
    executable: PathBuf,
    source: CodexRetryGatewayNodeResolutionSource,
) {
    let key = executable.display().to_string().to_ascii_lowercase();
    if seen.insert(key) {
        candidates.push(AutoNodeCandidate { executable, source });
    }
}

fn find_aio_discovery_nodes<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Vec<PathBuf>> {
    crate::cli_manager::resolve_executable_candidates(app, "node")
}

fn find_process_path_nodes() -> Vec<PathBuf> {
    let Some(current) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&current)
        .filter_map(|dir| find_executable_in_dir(&dir))
        .collect()
}

fn probe_node_executable(executable: &Path) -> AppResult<CodexRetryGatewayResolvedNodeVersion> {
    let mut command = Command::new(executable);
    command.arg("--version");
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.env("PATH", runtime_path_for_executable(executable)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = run_command_with_timeout(command, NODE_VERSION_TIMEOUT, executable)?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let message = if !stderr.is_empty() { stderr } else { stdout };
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
            if message.is_empty() {
                format!("failed to probe {}", executable.display())
            } else {
                message
            },
        ));
    }
    let raw = stdout
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let major = raw
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
                format!(
                    "failed to parse Node.js version from {}",
                    executable.display()
                ),
            )
        })?;
    if major < 18 {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_UNSUPPORTED",
            format!("Node.js 18+ is required, found v{raw}"),
        ));
    }
    Ok(CodexRetryGatewayResolvedNodeVersion {
        raw: format!("v{raw}"),
        major,
    })
}

fn run_command_with_timeout(
    mut command: Command,
    timeout: Duration,
    executable: &Path,
) -> AppResult<VersionProbeOutput> {
    let mut child = command.spawn().map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
            format!("failed to start Node probe {}: {err}", executable.display()),
        )
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    if let Some(mut reader) = stdout {
        thread::spawn(move || {
            let _ = stdout_tx.send(read_limited_stream(&mut reader));
        });
    }
    if let Some(mut reader) = stderr {
        thread::spawn(move || {
            let _ = stderr_tx.send(read_limited_stream(&mut reader));
        });
    }

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::new(
                        "CODEX_RETRY_GATEWAY_NODE_PROBE_TIMEOUT",
                        format!(
                            "Node.js version probe timed out after {}ms",
                            timeout.as_millis()
                        ),
                    ));
                }
                thread::sleep(NODE_POLL_INTERVAL);
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_NODE_PROBE_FAILED",
                    format!("failed to wait for Node probe: {err}"),
                ));
            }
        }
    };

    let (stdout, _stdout_truncated) = stdout_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_else(|_| (Vec::new(), false));
    let (stderr, _stderr_truncated) = stderr_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap_or_else(|_| (Vec::new(), false));
    Ok(VersionProbeOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_limited_stream<R: Read>(reader: &mut R) -> (Vec<u8>, bool) {
    let mut bytes = Vec::with_capacity(NODE_VERSION_OUTPUT_LIMIT.min(1024));
    let mut buf = [0_u8; 1024];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut buf) {
            Ok(read) => read,
            Err(_) => return (bytes, truncated),
        };
        if read == 0 {
            return (bytes, truncated);
        }
        let remaining = NODE_VERSION_OUTPUT_LIMIT.saturating_sub(bytes.len());
        if remaining == 0 {
            truncated = true;
            continue;
        }
        let keep = remaining.min(read);
        bytes.extend_from_slice(&buf[..keep]);
        if keep < read {
            truncated = true;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validate_manual_override_requires_absolute_regular_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("node.exe");
        std::fs::write(&file, b"fake").unwrap();
        assert!(validate_manual_override_path(file.to_str().unwrap()).is_ok());
        assert!(validate_manual_override_path("relative-node").is_err());
        assert!(validate_manual_override_path(dir.path().to_str().unwrap()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn automatic_discovery_accepts_symlink_to_regular_node_binary() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempdir().unwrap();
        let target = dir.path().join("node-real");
        let link = dir.path().join("node");
        std::fs::write(&target, b"node").unwrap();
        let mut permissions = std::fs::metadata(&target).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&target, permissions).unwrap();
        symlink(&target, &link).unwrap();

        assert_eq!(
            validate_real_node_executable_path(&link).unwrap(),
            std::fs::canonicalize(&target).unwrap()
        );
        assert!(validate_manual_override_path(link.to_str().unwrap()).is_err());
    }

    #[test]
    fn read_limited_stream_keeps_prefix_and_marks_truncation() {
        let mut cursor = std::io::Cursor::new(vec![b'x'; NODE_VERSION_OUTPUT_LIMIT + 32]);
        let (bytes, truncated) = read_limited_stream(&mut cursor);
        assert_eq!(bytes.len(), NODE_VERSION_OUTPUT_LIMIT);
        assert!(truncated);
    }

    #[test]
    fn public_node_error_maps_supported_categories() {
        let error = AppError::new(
            "CODEX_RETRY_GATEWAY_NODE_UNSUPPORTED",
            "Node.js 18+ is required",
        );
        let public = public_node_error(&error);
        assert_eq!(
            public.category,
            CodexRetryGatewayErrorCategory::NodeUnsupported
        );
        assert!(!public.retryable);
    }

    #[test]
    fn resolve_auto_node_candidates_skips_invalid_probe_and_continues() {
        let dir = tempdir().unwrap();
        let first = dir.path().join("node-16.exe");
        let second = dir.path().join("node-20.exe");
        std::fs::write(&first, b"node").unwrap();
        std::fs::write(&second, b"node").unwrap();
        let resolved = resolve_auto_node_candidates_with(
            vec![
                AutoNodeCandidate {
                    executable: first.clone(),
                    source: CodexRetryGatewayNodeResolutionSource::AioDiscovery,
                },
                AutoNodeCandidate {
                    executable: second.clone(),
                    source: CodexRetryGatewayNodeResolutionSource::ProcessPath,
                },
            ],
            |path| {
                if path.file_name().and_then(|value| value.to_str()) == Some("node-16.exe") {
                    Err(AppError::new(
                        "CODEX_RETRY_GATEWAY_NODE_UNSUPPORTED",
                        "Node.js 18+ is required, found v16.0.0",
                    ))
                } else {
                    Ok(CodexRetryGatewayResolvedNodeVersion {
                        raw: "v20.0.0".to_string(),
                        major: 20,
                    })
                }
            },
        )
        .expect("candidate resolution");
        let resolved = resolved.expect("resolved node");
        assert_eq!(resolved.executable, std::fs::canonicalize(&second).unwrap());
        assert_eq!(
            resolved.source,
            CodexRetryGatewayNodeResolutionSource::ProcessPath
        );
    }

    #[cfg(windows)]
    #[test]
    fn find_executable_in_dir_rejects_cmd_wrapper_and_prefers_real_exe() {
        let dir = tempdir().unwrap();
        let wrapper = dir.path().join("node.cmd");
        let exe = dir.path().join("node.exe");
        std::fs::write(&wrapper, "@echo off\r\necho v20.0.0\r\n").unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        assert_eq!(find_executable_in_dir(dir.path()), Some(exe));
        assert!(validate_real_node_executable_path(&wrapper).is_err());
    }
}
