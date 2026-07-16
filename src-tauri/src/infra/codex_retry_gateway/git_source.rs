use crate::infra::codex_retry_gateway::managed_state::CodexRetryGatewayManagerPaths;
use crate::infra::codex_retry_gateway::CODEX_RETRY_GATEWAY_REPOSITORY;
use crate::shared::error::{AppError, AppResult};
use crate::shared::fs::write_file_atomic;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::util::{ensure_not_symlink_or_reparse, normalize_full_sha};

const OFFICIAL_MAIN_REF: &str = "refs/remotes/aio-official/main";
const GIT_TIMEOUT: Duration = Duration::from_secs(90);
const GIT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const GIT_OUTPUT_LIMIT: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalGitCommitSelection {
    pub(crate) canonical_commit: String,
    pub(crate) official_main_commit: String,
    pub(crate) summary: Option<String>,
}

pub(crate) enum LocalGitResult<T> {
    Unavailable,
    Ready(T),
}

pub(crate) async fn resolve_commit(
    paths: &CodexRetryGatewayManagerPaths,
    commit: &str,
    require_official_ancestor: bool,
) -> AppResult<LocalGitResult<LocalGitCommitSelection>> {
    let paths = paths.clone();
    let commit = commit.to_string();
    tokio::task::spawn_blocking(move || {
        if !local_git_available()? {
            return Ok(LocalGitResult::Unavailable);
        }
        let cache = refresh_official_main_cache(&paths)?;
        resolve_from_cache(&cache, &commit, require_official_ancestor).map(LocalGitResult::Ready)
    })
    .await
    .map_err(|error| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
            format!("local Git task failed: {error}"),
        )
    })?
}

pub(crate) async fn official_commit_distance(
    paths: &CodexRetryGatewayManagerPaths,
    candidate: &str,
    main: &str,
) -> AppResult<LocalGitResult<Option<u32>>> {
    let paths = paths.clone();
    let candidate = normalize_full_sha(candidate)?;
    let main = normalize_full_sha(main)?;
    tokio::task::spawn_blocking(move || {
        if !local_git_available()? {
            return Ok(LocalGitResult::Unavailable);
        }
        let cache = official_cache_path(&paths);
        ensure_cache_is_bare(&cache)?;
        ensure_ancestor(&cache, &candidate, &main)?;
        let output = run_git(
            Some(&cache),
            ["rev-list", "--count", &format!("{candidate}..{main}")],
        )?;
        require_git_success(output, "count official main commits")?
            .trim()
            .parse::<u32>()
            .map(Some)
            .map(LocalGitResult::Ready)
            .map_err(|error| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
                    format!("failed to parse local Git commit distance: {error}"),
                )
            })
    })
    .await
    .map_err(|error| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
            format!("local Git task failed: {error}"),
        )
    })?
}

fn local_git_available() -> AppResult<bool> {
    match run_git(None, ["--version"]) {
        Ok(output) => Ok(output.status.success()),
        Err(error) if error.code() == "CODEX_RETRY_GATEWAY_SOURCE_GIT_UNAVAILABLE" => Ok(false),
        Err(error) => Err(error),
    }
}

fn official_cache_path(paths: &CodexRetryGatewayManagerPaths) -> PathBuf {
    paths.downloads_dir.join("official-main.git")
}

fn refresh_official_main_cache(paths: &CodexRetryGatewayManagerPaths) -> AppResult<PathBuf> {
    paths.ensure_dirs()?;
    let cache = official_cache_path(paths);
    if !cache.exists() {
        let output = run_git(None, ["init", "--bare", path_text(&cache)?])?;
        require_git_success(output, "initialize official source cache")?;
    }
    ensure_cache_is_bare(&cache)?;

    let official_url = format!("https://github.com/{CODEX_RETRY_GATEWAY_REPOSITORY}.git");
    let refspec = format!("+refs/heads/main:{OFFICIAL_MAIN_REF}");
    let output = run_git(
        Some(&cache),
        [
            "fetch",
            "--no-tags",
            "--prune",
            "--force",
            &official_url,
            &refspec,
        ],
    )?;
    require_git_success(output, "fetch official gateway main")?;
    Ok(cache)
}

fn ensure_cache_is_bare(cache: &Path) -> AppResult<()> {
    let metadata = ensure_not_symlink_or_reparse(cache, "official Git cache").map_err(|error| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_CACHE_INVALID",
            error.to_string(),
        )
    })?;
    if !metadata.is_dir() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_CACHE_INVALID",
            format!("official Git cache is not a directory: {}", cache.display()),
        ));
    }
    reject_commit_graph_overrides(cache)?;
    write_file_atomic(
        &cache.join("config"),
        b"[core]\n\trepositoryformatversion = 0\n\tbare = true\n\tlogallrefupdates = false\n",
    )
    .map_err(|error| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_CACHE_INVALID",
            format!("failed to reset official Git cache config: {error}"),
        )
    })?;
    let output = run_git(Some(cache), ["rev-parse", "--is-bare-repository"])?;
    let value = require_git_success(output, "validate official source cache")?;
    if value.trim() != "true" {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_CACHE_INVALID",
            "official Git cache is not a bare repository",
        ));
    }
    Ok(())
}

fn reject_commit_graph_overrides(cache: &Path) -> AppResult<()> {
    for relative in [
        "info/grafts",
        "objects/info/alternates",
        "shallow",
        "refs/replace",
    ] {
        let path = cache.join(relative);
        if path.exists() {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_GIT_CACHE_INVALID",
                format!("official Git cache contains forbidden graph override: {relative}"),
            ));
        }
    }
    Ok(())
}

fn resolve_from_cache(
    cache: &Path,
    commit: &str,
    require_official_ancestor: bool,
) -> AppResult<LocalGitCommitSelection> {
    let official_main_commit = rev_parse_commit(cache, OFFICIAL_MAIN_REF)?;
    let canonical_commit = if commit == "main" {
        official_main_commit.clone()
    } else {
        rev_parse_commit(cache, &normalize_full_sha(commit)?)?
    };
    if require_official_ancestor {
        ensure_ancestor(cache, &canonical_commit, &official_main_commit)?;
    }
    let output = run_git(
        Some(cache),
        ["show", "-s", "--format=%s", &canonical_commit],
    )?;
    let summary = require_git_success(output, "read gateway commit summary")?
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned);
    Ok(LocalGitCommitSelection {
        canonical_commit,
        official_main_commit,
        summary,
    })
}

fn rev_parse_commit(cache: &Path, reference: &str) -> AppResult<String> {
    let expression = format!("{reference}^{{commit}}");
    let output = run_git(Some(cache), ["rev-parse", "--verify", &expression])?;
    if !output.status.success() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_NOT_FOUND",
            format!("commit {reference} was not found in official main history"),
        ));
    }
    normalize_full_sha(output.stdout.trim())
}

fn ensure_ancestor(cache: &Path, candidate: &str, main: &str) -> AppResult<()> {
    let output = run_git(
        Some(cache),
        ["merge-base", "--is-ancestor", candidate, main],
    )?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_NOT_OFFICIAL_MAIN_ANCESTOR",
            format!("commit {candidate} is not official main or its ancestor"),
        )),
        _ => Err(git_failure("verify official main ancestry", &output)),
    }
}

fn path_text(path: &Path) -> AppResult<&str> {
    path.to_str().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
            format!("Git cache path is not valid Unicode: {}", path.display()),
        )
    })
}

fn secure_git_command() -> Command {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg(format!("core.hooksPath={}", null_device()))
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

fn run_git<I, S>(git_dir: Option<&Path>, args: I) -> AppResult<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut command = secure_git_command();
    if let Some(git_dir) = git_dir {
        command.arg("--git-dir").arg(git_dir);
    }
    command.args(args);
    let mut child = command.spawn().map_err(|error| {
        AppError::new(
            if error.kind() == std::io::ErrorKind::NotFound {
                "CODEX_RETRY_GATEWAY_SOURCE_GIT_UNAVAILABLE"
            } else {
                "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED"
            },
            format!("failed to start local Git: {error}"),
        )
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
            "local Git stdout pipe was not available",
        )
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
            "local Git stderr pipe was not available",
        )
    })?;
    let stdout_thread = std::thread::spawn(move || read_limited(&mut stdout));
    let stderr_thread = std::thread::spawn(move || read_limited(&mut stderr));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(GitOutput {
                    status,
                    stdout: String::from_utf8_lossy(&stdout_thread.join().unwrap_or_default())
                        .into_owned(),
                    stderr: String::from_utf8_lossy(&stderr_thread.join().unwrap_or_default())
                        .into_owned(),
                });
            }
            Ok(None) if started.elapsed() < GIT_TIMEOUT => {
                std::thread::sleep(GIT_POLL_INTERVAL);
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_GIT_TIMEOUT",
                    "local Git timed out while synchronizing the official gateway repository",
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
                    format!("failed to wait for local Git: {error}"),
                ));
            }
        }
    }
}

fn read_limited<R: Read>(reader: &mut R) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let Ok(read) = reader.read(&mut chunk) else {
            break;
        };
        if read == 0 {
            break;
        }
        if bytes.len() < GIT_OUTPUT_LIMIT {
            let keep = read.min(GIT_OUTPUT_LIMIT - bytes.len());
            bytes.extend_from_slice(&chunk[..keep]);
        }
    }
    bytes
}

fn require_git_success(output: GitOutput, action: &str) -> AppResult<String> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(git_failure(action, &output))
    }
}

fn git_failure(action: &str, output: &GitOutput) -> AppError {
    let detail = output.stderr.trim();
    AppError::new(
        "CODEX_RETRY_GATEWAY_SOURCE_GIT_FAILED",
        if detail.is_empty() {
            format!("local Git failed to {action} with status {}", output.status)
        } else {
            format!("local Git failed to {action}: {detail}")
        },
    )
}

struct GitOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_cache_accepts_main_ancestors_and_rejects_side_branches() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let root = tempdir().unwrap();
        let work = root.path().join("work");
        test_git(None, ["init", "-b", "main", path_text(&work).unwrap()]);
        test_git(
            Some(&work),
            [
                "-c",
                "user.name=AIO Test",
                "-c",
                "user.email=aio-test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "root commit",
            ],
        );
        let ancestor = test_git(Some(&work), ["rev-parse", "HEAD"]);

        test_git(Some(&work), ["checkout", "-b", "side"]);
        test_git(
            Some(&work),
            [
                "-c",
                "user.name=AIO Test",
                "-c",
                "user.email=aio-test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "side commit",
            ],
        );
        let side = test_git(Some(&work), ["rev-parse", "HEAD"]);
        test_git(Some(&work), ["checkout", "main"]);
        test_git(
            Some(&work),
            [
                "-c",
                "user.name=AIO Test",
                "-c",
                "user.email=aio-test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "main commit",
            ],
        );
        let main = test_git(Some(&work), ["rev-parse", "HEAD"]);

        let cache = root.path().join("official.git");
        test_git(
            None,
            [
                "clone",
                "--bare",
                path_text(&work).unwrap(),
                path_text(&cache).unwrap(),
            ],
        );
        test_git(Some(&cache), ["update-ref", OFFICIAL_MAIN_REF, main.trim()]);

        let selection = resolve_from_cache(&cache, ancestor.trim(), true).unwrap();
        assert_eq!(selection.canonical_commit, ancestor.trim());
        assert_eq!(selection.official_main_commit, main.trim());
        assert_eq!(selection.summary.as_deref(), Some("root commit"));

        let error = resolve_from_cache(&cache, side.trim(), true).unwrap_err();
        assert_eq!(
            error.code(),
            "CODEX_RETRY_GATEWAY_SOURCE_NOT_OFFICIAL_MAIN_ANCESTOR"
        );

        std::fs::create_dir_all(cache.join("refs").join("replace")).unwrap();
        let error = ensure_cache_is_bare(&cache).unwrap_err();
        assert_eq!(error.code(), "CODEX_RETRY_GATEWAY_SOURCE_GIT_CACHE_INVALID");
    }

    fn test_git<I, S>(work_tree: Option<&Path>, args: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new("git");
        if let Some(work_tree) = work_tree {
            command.arg("-C").arg(work_tree);
        }
        let output = command.args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}
