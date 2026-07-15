use crate::infra::codex_retry_gateway::managed_state::{
    read_source_manifest, write_source_manifest, CodexRetryGatewayManagerPaths,
    CodexRetryGatewaySourceManifest,
};
use crate::infra::codex_retry_gateway::{
    CodexRetryGatewayCommitValidation, CodexRetryGatewayError, CodexRetryGatewayErrorCategory,
    CodexRetryGatewayResolvedNode, CodexRetryGatewayTrustState,
    CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT, CODEX_RETRY_GATEWAY_REPOSITORY,
};
use crate::shared::error::{AppError, AppResult};
use crate::shared::http_body::read_text_with_limit;
use reqwest::header::LOCATION;
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::util::{
    canonicalize_path_within_root, ensure_not_symlink_or_reparse, normalize_full_sha, now_unix_ms,
    random_hex,
};

const GITHUB_JSON_RESPONSE_LIMIT: usize = 512 * 1024;
const SOURCE_ZIP_MAX_BYTES: usize = 8 * 1024 * 1024;
const SOURCE_ZIP_MAX_ENTRIES: usize = 256;
const SOURCE_ZIP_MAX_EXTRACTED_BYTES: u64 = 32 * 1024 * 1024;
const SOURCE_ZIP_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const SOURCE_ZIP_MAX_COMPRESSION_RATIO: u64 = 100;
const NODE_SYNTAX_TIMEOUT: Duration = Duration::from_secs(5);
const NODE_SYNTAX_POLL_INTERVAL: Duration = Duration::from_millis(50);
const NODE_SYNTAX_OUTPUT_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewaySourceHttpConfig {
    pub(crate) api_base_url: String,
    pub(crate) download_base_url: String,
    pub(crate) allowed_hosts: Vec<String>,
}

impl Default for CodexRetryGatewaySourceHttpConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://api.github.com".to_string(),
            download_base_url: "https://codeload.github.com".to_string(),
            allowed_hosts: vec![
                "api.github.com".to_string(),
                "github.com".to_string(),
                "codeload.github.com".to_string(),
                "raw.githubusercontent.com".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayCommitSelection {
    pub(crate) requested_commit: String,
    pub(crate) canonical_commit: String,
    pub(crate) official_main_commit: String,
    pub(crate) summary: Option<String>,
    pub(crate) trust_state: CodexRetryGatewayTrustState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexRetryGatewayCommitCandidate {
    pub(crate) validation: CodexRetryGatewayCommitValidation,
    pub(crate) selection: Option<CodexRetryGatewayCommitSelection>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexRetryGatewayInstalledSource {
    pub(crate) source_dir: PathBuf,
    pub(crate) manifest: CodexRetryGatewaySourceManifest,
}

#[derive(Debug, Deserialize)]
struct GitHubCommitResponse {
    sha: String,
    commit: GitHubCommitMessage,
}

#[derive(Debug, Deserialize)]
struct GitHubCommitMessage {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GitHubCompareResponse {
    status: String,
}

pub(crate) async fn validate_commit_request(
    commit: &str,
    http: &CodexRetryGatewaySourceHttpConfig,
) -> CodexRetryGatewayCommitCandidate {
    match resolve_commit_candidate(commit, http).await {
        Ok(selection) => CodexRetryGatewayCommitCandidate {
            validation: CodexRetryGatewayCommitValidation {
                requested_commit: commit.trim().to_string(),
                canonical_commit: Some(selection.canonical_commit.clone()),
                official_main_commit: Some(selection.official_main_commit.clone()),
                official_main_ancestor: true,
                trust_state: Some(selection.trust_state),
                summary: selection.summary.clone(),
                error: None,
            },
            selection: Some(selection),
        },
        Err(error) => CodexRetryGatewayCommitCandidate {
            validation: CodexRetryGatewayCommitValidation {
                requested_commit: commit.trim().to_string(),
                canonical_commit: None,
                official_main_commit: None,
                official_main_ancestor: false,
                trust_state: None,
                summary: None,
                error: Some(public_source_error(&error)),
            },
            selection: None,
        },
    }
}

pub(crate) async fn resolve_commit_candidate(
    commit: &str,
    http: &CodexRetryGatewaySourceHttpConfig,
) -> AppResult<CodexRetryGatewayCommitSelection> {
    let requested_commit = normalize_full_sha(commit)?;
    resolve_commit_selection(&requested_commit, &requested_commit, true, http).await
}

pub(crate) async fn resolve_official_main_candidate(
    http: &CodexRetryGatewaySourceHttpConfig,
) -> AppResult<CodexRetryGatewayCommitSelection> {
    resolve_commit_selection("main", "main", false, http).await
}

async fn resolve_commit_selection(
    requested_commit: &str,
    commit_ref: &str,
    require_official_ancestor: bool,
    http: &CodexRetryGatewaySourceHttpConfig,
) -> AppResult<CodexRetryGatewayCommitSelection> {
    let client = build_github_client()?;
    let (canonical_commit, summary) = fetch_commit_details(&client, http, commit_ref).await?;
    let (official_main_commit, _) = fetch_commit_details(&client, http, "main").await?;
    if require_official_ancestor {
        ensure_commit_is_official_main_ancestor(
            &client,
            http,
            &canonical_commit,
            &official_main_commit,
        )
        .await?;
    }
    let trust_state = if canonical_commit == CODEX_RETRY_GATEWAY_RECOMMENDED_COMMIT {
        CodexRetryGatewayTrustState::AioReviewedRecommendation
    } else {
        CodexRetryGatewayTrustState::OfficialMainUnreviewed
    };
    Ok(CodexRetryGatewayCommitSelection {
        requested_commit: requested_commit.trim().to_string(),
        canonical_commit,
        official_main_commit,
        summary,
        trust_state,
    })
}

pub(crate) async fn install_source_commit(
    paths: &CodexRetryGatewayManagerPaths,
    selection: &CodexRetryGatewayCommitSelection,
    node: &CodexRetryGatewayResolvedNode,
    http: &CodexRetryGatewaySourceHttpConfig,
) -> AppResult<CodexRetryGatewayInstalledSource> {
    paths.ensure_dirs()?;
    if let Some(installed) = revalidate_cached_source(paths, &selection.canonical_commit)? {
        return Ok(installed);
    }

    let client = build_github_client()?;
    let zip_bytes = download_zipball(&client, http, &selection.canonical_commit).await?;
    let archive_sha256 = format!("{:x}", Sha256::digest(&zip_bytes));
    let staging_dir = paths.downloads_dir.join(format!(
        ".staging-{}-{}",
        selection.canonical_commit,
        random_hex(4)
    ));
    if staging_dir.exists() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_STAGING_CONFLICT",
            format!(
                "source staging dir already exists: {}",
                staging_dir.display()
            ),
        ));
    }
    std::fs::create_dir_all(&staging_dir).map_err(|err| {
        AppError::new("CODEX_RETRY_GATEWAY_SOURCE_STAGING_FAILED", err.to_string())
    })?;

    let install_result = (|| -> AppResult<CodexRetryGatewayInstalledSource> {
        let extracted_root = extract_source_zip(&zip_bytes, &staging_dir)?;
        validate_source_layout(&extracted_root)?;
        validate_node_syntax(node, &extracted_root)?;
        let (source_sha256, file_count, total_bytes) =
            fingerprint_extracted_source(&extracted_root)?;
        let manifest = CodexRetryGatewaySourceManifest {
            schema_version: 1,
            repository: CODEX_RETRY_GATEWAY_REPOSITORY.to_string(),
            commit: selection.canonical_commit.clone(),
            verified_main_commit: selection.official_main_commit.clone(),
            verified_at_ms: now_unix_ms(),
            archive_sha256,
            source_sha256,
            file_count,
            total_bytes,
            gateway_entry_rel: "gateway.mjs".to_string(),
            admin_entry_rel: "scripts/admin-lib.mjs".to_string(),
            launch_ui_entry_rel: "scripts/launch-ui.mjs".to_string(),
        };
        write_source_manifest(&extracted_root.join("manifest.json"), &manifest)?;
        let final_dir = paths.source_dir(&selection.canonical_commit)?;
        if final_dir.exists() {
            let _ =
                canonicalize_path_within_root(&paths.root, &final_dir, "cached source directory")?;
            std::fs::remove_dir_all(&final_dir).map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_REPAIR_FAILED",
                    format!("failed to remove invalid cached source: {err}"),
                )
            })?;
        }
        std::fs::rename(&extracted_root, &final_dir).map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_PROMOTE_FAILED",
                format!("failed to promote source to {}: {err}", final_dir.display()),
            )
        })?;
        Ok(CodexRetryGatewayInstalledSource {
            source_dir: final_dir,
            manifest,
        })
    })();

    if install_result.is_err() {
        let _ = std::fs::remove_dir_all(&staging_dir);
    } else {
        let _ = std::fs::remove_dir_all(&staging_dir);
    }
    install_result
}

pub(crate) fn revalidate_cached_source(
    paths: &CodexRetryGatewayManagerPaths,
    commit: &str,
) -> AppResult<Option<CodexRetryGatewayInstalledSource>> {
    let source_dir = paths.source_dir(commit)?;
    if !source_dir.exists() {
        return Ok(None);
    }
    let _ = canonicalize_path_within_root(&paths.root, &source_dir, "cached source directory")?;
    let manifest = read_source_manifest(paths, commit)?;
    validate_source_layout(&source_dir)?;
    let (source_sha256, file_count, total_bytes) = fingerprint_extracted_source(&source_dir)?;
    if source_sha256 != manifest.source_sha256
        || file_count != manifest.file_count
        || total_bytes != manifest.total_bytes
    {
        return Ok(None);
    }
    Ok(Some(CodexRetryGatewayInstalledSource {
        source_dir,
        manifest,
    }))
}

fn build_github_client() -> AppResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("aio-coding-hub/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_HTTP_CLIENT_FAILED", err.to_string()))
}

fn split_repository() -> (&'static str, &'static str) {
    let (owner, repo) = CODEX_RETRY_GATEWAY_REPOSITORY
        .split_once('/')
        .expect("repository constant must contain owner/repo");
    (owner, repo)
}

async fn fetch_commit_details(
    client: &Client,
    http: &CodexRetryGatewaySourceHttpConfig,
    commit: &str,
) -> AppResult<(String, Option<String>)> {
    let (owner, repo) = split_repository();
    let url = Url::parse(&format!(
        "{}/repos/{owner}/{repo}/commits/{commit}",
        http.api_base_url.trim_end_matches('/')
    ))
    .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_SOURCE_URL_INVALID", err.to_string()))?;
    let response = client.get(url).send().await.map_err(classify_http_error)?;
    let response = map_github_status(response, "commit")?;
    let body = read_text_with_limit(
        response,
        GITHUB_JSON_RESPONSE_LIMIT,
        "github commit response",
    )
    .await
    .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_SOURCE_PARSE_FAILED", err))?;
    let payload: GitHubCommitResponse = serde_json::from_str(&body).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_PARSE_FAILED",
            format!("failed to parse GitHub commit response: {err}"),
        )
    })?;
    Ok((
        normalize_full_sha(&payload.sha)?,
        payload
            .commit
            .message
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned),
    ))
}

async fn ensure_commit_is_official_main_ancestor(
    client: &Client,
    http: &CodexRetryGatewaySourceHttpConfig,
    candidate: &str,
    main: &str,
) -> AppResult<()> {
    let (owner, repo) = split_repository();
    let url = Url::parse(&format!(
        "{}/repos/{owner}/{repo}/compare/{candidate}...{main}",
        http.api_base_url.trim_end_matches('/')
    ))
    .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_SOURCE_URL_INVALID", err.to_string()))?;
    let response = client.get(url).send().await.map_err(classify_http_error)?;
    let response = map_github_status(response, "compare")?;
    let body = read_text_with_limit(
        response,
        GITHUB_JSON_RESPONSE_LIMIT,
        "github compare response",
    )
    .await
    .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_SOURCE_PARSE_FAILED", err))?;
    let payload: GitHubCompareResponse = serde_json::from_str(&body).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_PARSE_FAILED",
            format!("failed to parse GitHub compare response: {err}"),
        )
    })?;
    if payload.status != "ahead" && payload.status != "identical" {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_NOT_OFFICIAL_MAIN_ANCESTOR",
            format!("commit {candidate} is not official main or its ancestor"),
        ));
    }
    Ok(())
}

async fn download_zipball(
    client: &Client,
    http: &CodexRetryGatewaySourceHttpConfig,
    commit: &str,
) -> AppResult<Vec<u8>> {
    let (owner, repo) = split_repository();
    let mut url = Url::parse(&format!(
        "{}/{owner}/{repo}/zip/{commit}",
        http.download_base_url.trim_end_matches('/')
    ))
    .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_SOURCE_URL_INVALID", err.to_string()))?;
    for _ in 0..3 {
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(classify_http_error)?;
        if response.status().is_redirection() {
            let location = response.headers().get(LOCATION).ok_or_else(|| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_REDIRECT_INVALID",
                    "redirect response missing Location header",
                )
            })?;
            let next = url
                .join(location.to_str().unwrap_or_default())
                .map_err(|err| {
                    AppError::new(
                        "CODEX_RETRY_GATEWAY_SOURCE_REDIRECT_INVALID",
                        format!("invalid redirect location: {err}"),
                    )
                })?;
            validate_allowed_host(&next, &http.allowed_hosts)?;
            url = next;
            continue;
        }
        let response = map_github_status(response, "zipball")?;
        return read_body_with_limit(response, SOURCE_ZIP_MAX_BYTES).await;
    }
    Err(AppError::new(
        "CODEX_RETRY_GATEWAY_SOURCE_REDIRECT_INVALID",
        "too many redirect hops while downloading source zip",
    ))
}

fn validate_allowed_host(url: &Url, allowed_hosts: &[String]) -> AppResult<()> {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&host))
    {
        return Ok(());
    }
    Err(AppError::new(
        "CODEX_RETRY_GATEWAY_SOURCE_REDIRECT_INVALID",
        format!("redirect host {host} is not allowlisted"),
    ))
}

async fn read_body_with_limit(mut response: reqwest::Response, limit: usize) -> AppResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
            format!("source archive exceeds {limit} bytes"),
        ));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default()
            .min(limit),
    );
    while let Some(chunk) = response.chunk().await.map_err(classify_http_error)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
                format!("source archive exceeds {limit} bytes"),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn map_github_status(response: reqwest::Response, kind: &str) -> AppResult<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let code = github_status_error_code(status);
    Err(AppError::new(
        code,
        format!("GitHub {kind} request failed with status {status}"),
    ))
}

fn github_status_error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::NOT_FOUND => "CODEX_RETRY_GATEWAY_SOURCE_NOT_FOUND",
        StatusCode::FORBIDDEN => "CODEX_RETRY_GATEWAY_SOURCE_FORBIDDEN",
        StatusCode::TOO_MANY_REQUESTS => "CODEX_RETRY_GATEWAY_SOURCE_RATE_LIMITED",
        _ if status.is_server_error() => "CODEX_RETRY_GATEWAY_SOURCE_SERVER_ERROR",
        _ => "CODEX_RETRY_GATEWAY_SOURCE_HTTP_ERROR",
    }
}

fn classify_http_error(error: reqwest::Error) -> AppError {
    let code = if error.is_timeout() || error.is_connect() {
        "CODEX_RETRY_GATEWAY_SOURCE_TRANSIENT"
    } else {
        "CODEX_RETRY_GATEWAY_SOURCE_HTTP_ERROR"
    };
    AppError::new(code, error.to_string())
}

fn extract_source_zip(zip_bytes: &[u8], staging_dir: &Path) -> AppResult<PathBuf> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
            format!("failed to open source archive: {err}"),
        )
    })?;
    if archive.len() > SOURCE_ZIP_MAX_ENTRIES {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
            format!("source archive has too many entries: {}", archive.len()),
        ));
    }

    let mut root_name: Option<String> = None;
    let mut extracted_bytes = 0_u64;
    let mut seen_paths = HashSet::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                format!("failed to read source archive entry: {err}"),
            )
        })?;
        if let Some(mode) = file.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                    "source archive contains an unsupported symbolic link entry",
                ));
            }
        }
        let normalized = normalize_zip_entry(file.name())?;
        if normalized.components().count() < 2 {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                "source archive must contain a single top-level directory",
            ));
        }
        let mut components = normalized.components();
        let current_root = components
            .next()
            .and_then(|component| match component {
                Component::Normal(value) => value.to_str().map(ToOwned::to_owned),
                _ => None,
            })
            .ok_or_else(|| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                    "source archive contains an invalid top-level path",
                )
            })?;
        match &root_name {
            Some(existing) if existing != &current_root => {
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                    "source archive contains multiple top-level directories",
                ));
            }
            None => root_name = Some(current_root),
            _ => {}
        }
        let relative = components.as_path();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if !seen_paths.insert(archive_duplicate_key(relative)) {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                format!("duplicate source archive path {}", relative.display()),
            ));
        }
        if !file.is_dir() {
            if archive_entry_exceeds_compression_ratio(file.size(), file.compressed_size()) {
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_COMPRESSION_RATIO",
                    format!(
                        "source archive entry {} exceeds the maximum compression ratio",
                        relative.display()
                    ),
                ));
            }
            extracted_bytes = extracted_bytes.checked_add(file.size()).ok_or_else(|| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                    "source archive extracted size overflowed",
                )
            })?;
            if file.size() > SOURCE_ZIP_MAX_FILE_BYTES
                || extracted_bytes > SOURCE_ZIP_MAX_EXTRACTED_BYTES
            {
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
                    "source archive extracted content exceeds limits",
                ));
            }
        }
    }

    let root = staging_dir.join(root_name.unwrap_or_else(|| "source".to_string()));
    std::fs::create_dir_all(&root).map_err(|err| {
        AppError::new("CODEX_RETRY_GATEWAY_SOURCE_EXTRACT_FAILED", err.to_string())
    })?;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                format!("failed to read source archive entry: {err}"),
            )
        })?;
        let normalized = normalize_zip_entry(file.name())?;
        let mut components = normalized.components();
        let _ = components.next();
        let relative = components.as_path();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output_path = root.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path).map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_EXTRACT_FAILED",
                    format!("failed to create {}: {err}", output_path.display()),
                )
            })?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_EXTRACT_FAILED",
                    format!("failed to create {}: {err}", parent.display()),
                )
            })?;
        }
        let mut output = std::fs::File::create(&output_path).map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_EXTRACT_FAILED",
                format!("failed to create {}: {err}", output_path.display()),
            )
        })?;
        let remaining_limit = SOURCE_ZIP_MAX_FILE_BYTES.min(SOURCE_ZIP_MAX_EXTRACTED_BYTES);
        let copied = {
            let mut limited = file.by_ref().take(remaining_limit + 1);
            std::io::copy(&mut limited, &mut output)
        }
        .map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_EXTRACT_FAILED",
                format!("failed to write {}: {err}", output_path.display()),
            )
        })?;
        if copied > remaining_limit {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
                "source archive extracted content exceeds limits",
            ));
        }
    }

    Ok(root)
}

fn archive_entry_exceeds_compression_ratio(uncompressed: u64, compressed: u64) -> bool {
    uncompressed > 0
        && (compressed == 0
            || uncompressed > compressed.saturating_mul(SOURCE_ZIP_MAX_COMPRESSION_RATIO))
}

fn archive_duplicate_key(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        return PathBuf::from(path.to_string_lossy().to_lowercase());
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

fn normalize_zip_entry(name: &str) -> AppResult<PathBuf> {
    let replaced = name.replace('\\', "/");
    let path = Path::new(replaced.trim());
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
            "source archive contains an invalid absolute path",
        ));
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => clean.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID",
                    "source archive contains a path traversal entry",
                ));
            }
        }
    }
    Ok(clean)
}

fn validate_source_layout(root: &Path) -> AppResult<()> {
    let _ = ensure_not_symlink_or_reparse(root, "source root").map_err(|err| {
        AppError::new("CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID", err.to_string())
    })?;
    for relative in [
        "gateway.mjs",
        "scripts/admin-lib.mjs",
        "scripts/launch-ui.mjs",
    ] {
        let path = root.join(relative);
        let metadata =
            ensure_not_symlink_or_reparse(&path, "required source file").map_err(|err| {
                AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                    format!("required source file missing {}: {err}", path.display()),
                )
            })?;
        if !metadata.is_file() {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                format!("required source path is not a file: {}", path.display()),
            ));
        }
    }
    Ok(())
}

fn validate_node_syntax(node: &CodexRetryGatewayResolvedNode, root: &Path) -> AppResult<()> {
    for relative in [
        "gateway.mjs",
        "scripts/admin-lib.mjs",
        "scripts/launch-ui.mjs",
    ] {
        let path = root.join(relative);
        let mut command = Command::new(&node.executable);
        command.arg("--check").arg(&path);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.env("PATH", syntax_runtime_path(&node.executable)?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let output = run_with_timeout(command, NODE_SYNTAX_TIMEOUT, &path)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = if !stderr.is_empty() { stderr } else { stdout };
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                if message.is_empty() {
                    format!("Node syntax check failed for {}", path.display())
                } else {
                    message
                },
            ));
        }
    }
    Ok(())
}

fn syntax_runtime_path(executable: &Path) -> AppResult<std::ffi::OsString> {
    let mut paths = Vec::new();
    if let Some(parent) = executable
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
    {
        paths.push(parent.to_path_buf());
    }
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    std::env::join_paths(paths)
        .map_err(|err| AppError::new("CODEX_RETRY_GATEWAY_NODE_PATH_INVALID", err.to_string()))
}

fn run_with_timeout(
    mut command: Command,
    timeout: Duration,
    path: &Path,
) -> AppResult<ProcessOutput> {
    let mut child = command.spawn().map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
            format!(
                "failed to start Node syntax check for {}: {err}",
                path.display()
            ),
        )
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
            "syntax check stdout pipe was not available",
        )
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
            "syntax check stderr pipe was not available",
        )
    })?;
    let started = Instant::now();
    let stdout_thread = std::thread::spawn(move || read_stream_with_limit(&mut stdout));
    let stderr_thread = std::thread::spawn(move || read_stream_with_limit(&mut stderr));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_thread.join().unwrap_or_default();
                let stderr = stderr_thread.join().unwrap_or_default();
                return Ok(ProcessOutput {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::new(
                        "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                        format!("Node syntax check timed out for {}", path.display()),
                    ));
                }
                std::thread::sleep(NODE_SYNTAX_POLL_INTERVAL);
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::new(
                    "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                    format!("failed to wait for Node syntax check: {err}"),
                ));
            }
        }
    }
}

#[derive(Debug)]
struct ProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub(crate) fn public_source_error(error: &AppError) -> CodexRetryGatewayError {
    let rendered = error.to_string();
    let category = match error.code() {
        value if value.contains("ARCHIVE") || value.contains("LAYOUT") => {
            CodexRetryGatewayErrorCategory::SourceArchive
        }
        _ => CodexRetryGatewayErrorCategory::SourceResolution,
    };
    CodexRetryGatewayError {
        code: error.code().to_string(),
        category,
        message: rendered
            .split_once(':')
            .map(|(_, message)| message.trim().to_string())
            .unwrap_or(rendered),
        retryable: matches!(
            error.code(),
            "CODEX_RETRY_GATEWAY_SOURCE_TRANSIENT"
                | "CODEX_RETRY_GATEWAY_SOURCE_RATE_LIMITED"
                | "CODEX_RETRY_GATEWAY_SOURCE_SERVER_ERROR"
                | "CODEX_RETRY_GATEWAY_SOURCE_HTTP_ERROR"
        ),
    }
}

fn read_stream_with_limit<R: Read>(reader: &mut R) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(read) => read,
            Err(_) => return bytes,
        };
        if read == 0 {
            return bytes;
        }
        let remaining = NODE_SYNTAX_OUTPUT_LIMIT.saturating_sub(bytes.len());
        if remaining == 0 {
            return bytes;
        }
        let keep = remaining.min(read);
        bytes.extend_from_slice(&chunk[..keep]);
    }
}

fn fingerprint_extracted_source(root: &Path) -> AppResult<(String, u32, u64)> {
    let mut files = Vec::new();
    collect_source_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    let mut total_bytes = 0_u64;
    for (relative, path) in &files {
        let metadata =
            ensure_not_symlink_or_reparse(path, "cached source file").map_err(|err| {
                AppError::new("CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID", err.to_string())
            })?;
        if !metadata.is_file() {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                format!("cached source path is not a file: {}", path.display()),
            ));
        }
        if metadata.len() > SOURCE_ZIP_MAX_FILE_BYTES {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
                format!("cached source file exceeds limit: {}", path.display()),
            ));
        }
        let bytes = std::fs::read(path).map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                format!("failed to read {}: {err}", path.display()),
            )
        })?;
        if bytes.len() as u64 > SOURCE_ZIP_MAX_FILE_BYTES {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
                format!("cached source file exceeds limit: {}", path.display()),
            ));
        }
        total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        if total_bytes > SOURCE_ZIP_MAX_EXTRACTED_BYTES {
            return Err(AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_TOO_LARGE",
                "cached source extracted content exceeds limits",
            ));
        }
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(&bytes);
    }
    Ok((
        format!("{:x}", hasher.finalize()),
        files.len() as u32,
        total_bytes,
    ))
}

fn collect_source_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> AppResult<()> {
    let _ =
        canonicalize_path_within_root(root, current, "cached source directory").map_err(|err| {
            AppError::new("CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID", err.to_string())
        })?;
    let entries = std::fs::read_dir(current).map_err(|err| {
        AppError::new(
            "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
            format!("failed to read {}: {err}", current.display()),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            AppError::new(
                "CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID",
                format!("failed to read dir entry in {}: {err}", current.display()),
            )
        })?;
        let path = entry.path();
        if path.file_name().and_then(|value| value.to_str()) == Some("manifest.json") {
            continue;
        }
        let metadata =
            ensure_not_symlink_or_reparse(&path, "cached source entry").map_err(|err| {
                AppError::new("CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID", err.to_string())
            })?;
        let _ =
            canonicalize_path_within_root(root, &path, "cached source entry").map_err(|err| {
                AppError::new("CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID", err.to_string())
            })?;
        if metadata.is_dir() {
            collect_source_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| {
                    AppError::new("CODEX_RETRY_GATEWAY_SOURCE_LAYOUT_INVALID", err.to_string())
                })?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Path as AxumPath;
    use axum::routing::get;
    use axum::{Json, Router};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn normalize_zip_entry_rejects_traversal() {
        assert!(normalize_zip_entry("../evil").is_err());
        assert!(normalize_zip_entry("/absolute").is_err());
        assert_eq!(
            normalize_zip_entry("root/scripts/admin-lib.mjs").unwrap(),
            PathBuf::from("root").join("scripts").join("admin-lib.mjs")
        );
    }

    #[test]
    fn github_statuses_keep_actionable_failure_classes() {
        assert_eq!(
            github_status_error_code(StatusCode::NOT_FOUND),
            "CODEX_RETRY_GATEWAY_SOURCE_NOT_FOUND"
        );
        assert_eq!(
            github_status_error_code(StatusCode::FORBIDDEN),
            "CODEX_RETRY_GATEWAY_SOURCE_FORBIDDEN"
        );
        assert_eq!(
            github_status_error_code(StatusCode::TOO_MANY_REQUESTS),
            "CODEX_RETRY_GATEWAY_SOURCE_RATE_LIMITED"
        );
        assert_eq!(
            github_status_error_code(StatusCode::BAD_GATEWAY),
            "CODEX_RETRY_GATEWAY_SOURCE_SERVER_ERROR"
        );
    }

    #[test]
    fn source_archive_rejects_excessive_compression_ratio() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::FileOptions::<()>::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("root/gateway.mjs", options).unwrap();
            writer.write_all(&vec![b'x'; 1024 * 1024]).unwrap();
            writer.finish().unwrap();
        }
        let staging = tempdir().unwrap();
        let error = extract_source_zip(cursor.get_ref(), staging.path()).unwrap_err();
        assert_eq!(
            error.code(),
            "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_COMPRESSION_RATIO"
        );
    }

    #[cfg(windows)]
    #[test]
    fn source_archive_rejects_case_insensitive_duplicate_paths() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::FileOptions::<()>::default();
            writer
                .start_file("root/scripts/Admin.mjs", options)
                .unwrap();
            writer.write_all(b"first").unwrap();
            writer
                .start_file("root/scripts/admin.mjs", options)
                .unwrap();
            writer.write_all(b"second").unwrap();
            writer.finish().unwrap();
        }
        let staging = tempdir().unwrap();
        let error = extract_source_zip(cursor.get_ref(), staging.path()).unwrap_err();
        assert_eq!(error.code(), "CODEX_RETRY_GATEWAY_SOURCE_ARCHIVE_INVALID");
        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn validate_source_layout_requires_core_runtime_files() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        std::fs::write(dir.path().join("gateway.mjs"), "export {}").unwrap();
        std::fs::write(
            dir.path().join("scripts").join("admin-lib.mjs"),
            "export {}",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("scripts").join("launch-ui.mjs"),
            "export {}",
        )
        .unwrap();
        validate_source_layout(dir.path()).unwrap();
    }

    #[test]
    fn fingerprint_extracted_source_is_stable() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        std::fs::write(dir.path().join("gateway.mjs"), "gateway").unwrap();
        std::fs::write(dir.path().join("scripts").join("admin-lib.mjs"), "admin").unwrap();
        let (fingerprint, file_count, total_bytes) =
            fingerprint_extracted_source(dir.path()).unwrap();
        assert_eq!(file_count, 2);
        assert_eq!(total_bytes, 12);
        assert_eq!(fingerprint.len(), 64);
    }

    #[test]
    fn revalidate_cached_source_returns_none_when_fingerprint_changes() {
        let dir = tempdir().unwrap();
        let paths = CodexRetryGatewayManagerPaths::from_root(dir.path().join("gateway"));
        paths.ensure_dirs().unwrap();
        let source_dir = paths
            .source_dir("ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2")
            .unwrap();
        std::fs::create_dir_all(source_dir.join("scripts")).unwrap();
        std::fs::write(source_dir.join("gateway.mjs"), "gateway").unwrap();
        std::fs::write(source_dir.join("scripts").join("admin-lib.mjs"), "admin").unwrap();
        std::fs::write(source_dir.join("scripts").join("launch-ui.mjs"), "ui").unwrap();
        let manifest = CodexRetryGatewaySourceManifest {
            schema_version: 1,
            repository: CODEX_RETRY_GATEWAY_REPOSITORY.to_string(),
            commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            verified_main_commit: "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2".to_string(),
            verified_at_ms: 1,
            archive_sha256: "a".repeat(64),
            source_sha256: "b".repeat(64),
            file_count: 3,
            total_bytes: 0,
            gateway_entry_rel: "gateway.mjs".to_string(),
            admin_entry_rel: "scripts/admin-lib.mjs".to_string(),
            launch_ui_entry_rel: "scripts/launch-ui.mjs".to_string(),
        };
        write_source_manifest(&source_dir.join("manifest.json"), &manifest).unwrap();
        assert!(revalidate_cached_source(&paths, &manifest.commit)
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn resolve_official_main_candidate_accepts_main_ref() {
        const MAIN_SHA: &str = "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2";

        async fn commit_handler(
            AxumPath((_owner, _repo, reference)): AxumPath<(String, String, String)>,
        ) -> Json<serde_json::Value> {
            let sha = if reference == "main" {
                MAIN_SHA.to_string()
            } else {
                reference
            };
            Json(serde_json::json!({
                "sha": sha,
                "commit": {
                    "message": "official main"
                }
            }))
        }

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tauri::async_runtime::spawn(async move {
            let router = Router::new().route(
                "/repos/:owner/:repo/commits/:reference",
                get(commit_handler),
            );
            let _ = axum::serve(listener, router).await;
        });

        let selection = resolve_official_main_candidate(&CodexRetryGatewaySourceHttpConfig {
            api_base_url: format!("http://127.0.0.1:{port}"),
            download_base_url: "http://127.0.0.1:1".to_string(),
            allowed_hosts: vec!["127.0.0.1".to_string()],
        })
        .await
        .unwrap();
        assert_eq!(selection.requested_commit, "main");
        assert_eq!(selection.canonical_commit, MAIN_SHA);
        assert_eq!(selection.official_main_commit, MAIN_SHA);
        server.abort();
    }

    #[cfg(windows)]
    #[test]
    fn fingerprint_extracted_source_rejects_junction_escape() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("source");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("gateway.mjs"), "gateway").unwrap();
        std::fs::write(root.join("scripts").join("admin-lib.mjs"), "admin").unwrap();
        std::fs::write(root.join("scripts").join("launch-ui.mjs"), "ui").unwrap();
        std::fs::write(outside.join("payload.txt"), "escape").unwrap();
        junction::create(&outside, root.join("escaped")).unwrap();

        let err = fingerprint_extracted_source(&root).expect_err("junction escape must fail");
        assert!(err.to_string().contains("reparse point"));
    }
}
