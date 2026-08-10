//! Usage: Backend-owned desktop capability proxy commands.
//!
//! This module keeps sensitive or high-risk desktop capabilities behind one
//! handwritten IPC family so the renderer does not call plugin commands
//! directly.

use crate::shared::blocking;
use sha2::{Digest as _, Sha256};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Manager, Resource, ResourceId, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, FileAccessMode, FilePath, PickerMode};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::oneshot;

use crate::shared::ipc_confirm::RiskyIpcConfirm;

const BETA_UPDATER_ENDPOINT: &str =
    "https://raw.githubusercontent.com/FingerCaster/aio-coding-hub/release-channels/latest-beta.json";
const RELEASES_BASE_URL: &str = "https://github.com/FingerCaster/aio-coding-hub/releases";
const RELEASE_TAG_PREFIX: &str = "aio-coding-hub-v";
const OFFICIAL_UPDATER_PLATFORMS: [&str; 4] = [
    "windows-x86_64",
    "darwin-x86_64",
    "darwin-aarch64",
    "linux-x86_64",
];
const UPDATER_ERROR_BETA_FRESH_CHECK_FAILED: &str = "UPDATER_BETA_FRESH_CHECK_FAILED";
const UPDATER_ERROR_CANDIDATE_STALE: &str = "UPDATER_CANDIDATE_STALE";
const UPDATER_ERROR_CHANNEL_CHANGED: &str = "UPDATER_CHANNEL_CHANGED";
const UPDATER_ERROR_CHECK_FAILED: &str = "UPDATER_CHECK_FAILED";
const UPDATER_ERROR_CLIENT_BUILD_FAILED: &str = "UPDATER_CLIENT_BUILD_FAILED";
const UPDATER_ERROR_DOWNLOAD_FAILED: &str = "UPDATER_DOWNLOAD_FAILED";
const UPDATER_ERROR_ENDPOINT_INVALID: &str = "UPDATER_ENDPOINT_INVALID";
const UPDATER_ERROR_INSTALL_FAILED: &str = "UPDATER_INSTALL_FAILED";
const UPDATER_ERROR_MANIFEST_INVALID: &str = "UPDATER_MANIFEST_INVALID";
const UPDATER_ERROR_PLATFORM_UNSUPPORTED: &str = "UPDATER_PLATFORM_UNSUPPORTED";
const UPDATER_ERROR_RESOURCE_CLOSE_FAILED: &str = "UPDATER_RESOURCE_CLOSE_FAILED";
const UPDATER_ERROR_RESOURCE_CLOSED: &str = "UPDATER_RESOURCE_CLOSED";
const UPDATER_ERROR_RESOURCE_INVALID: &str = "UPDATER_RESOURCE_INVALID";
const UPDATER_ERROR_SETTINGS_UNAVAILABLE: &str = "UPDATER_SETTINGS_UNAVAILABLE";

fn updater_error(code: &'static str, detail: impl std::fmt::Display) -> String {
    format!("{code}: {detail}")
}

#[derive(Debug, Clone, Copy, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesktopThemeMode {
    Light,
    Dark,
    System,
}

impl DesktopThemeMode {
    fn into_tauri_theme(self) -> Option<tauri::Theme> {
        match self {
            Self::Light => Some(tauri::Theme::Light),
            Self::Dark => Some(tauri::Theme::Dark),
            Self::System => None,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub(crate) struct DesktopNotificationPayload {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) sound: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DesktopNotificationPermissionState {
    Granted,
    Denied,
    Prompt,
    PromptWithRationale,
}

impl From<tauri::plugin::PermissionState> for DesktopNotificationPermissionState {
    fn from(value: tauri::plugin::PermissionState) -> Self {
        match value {
            tauri::plugin::PermissionState::Granted => Self::Granted,
            tauri::plugin::PermissionState::Denied => Self::Denied,
            tauri::plugin::PermissionState::Prompt => Self::Prompt,
            tauri::plugin::PermissionState::PromptWithRationale => Self::PromptWithRationale,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopUpdaterMetadata {
    rid: u32,
    channel: crate::settings::UpdateChannel,
    is_prerelease: bool,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    release_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdaterCandidateIdentity {
    version: String,
    target: String,
    download_url: String,
    signature_sha256: String,
}

#[derive(Clone)]
struct ChannelBoundUpdate {
    update: Update,
    channel: crate::settings::UpdateChannel,
    channel_epoch: u64,
    identity: UpdaterCandidateIdentity,
}

impl Resource for ChannelBoundUpdate {}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub(crate) enum DesktopUpdaterDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        chunk_length: usize,
    },
    Finished,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopDialogFilter {
    name: String,
    extensions: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DesktopDialogPickerMode {
    Document,
    Media,
    Image,
    Video,
}

impl From<DesktopDialogPickerMode> for PickerMode {
    fn from(value: DesktopDialogPickerMode) -> Self {
        match value {
            DesktopDialogPickerMode::Document => PickerMode::Document,
            DesktopDialogPickerMode::Media => PickerMode::Media,
            DesktopDialogPickerMode::Image => PickerMode::Image,
            DesktopDialogPickerMode::Video => PickerMode::Video,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DesktopDialogFileAccessMode {
    Copy,
    Scoped,
}

impl From<DesktopDialogFileAccessMode> for FileAccessMode {
    fn from(value: DesktopDialogFileAccessMode) -> Self {
        match value {
            DesktopDialogFileAccessMode::Copy => FileAccessMode::Copy,
            DesktopDialogFileAccessMode::Scoped => FileAccessMode::Scoped,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopDialogOpenRequest {
    title: Option<String>,
    filters: Option<Vec<DesktopDialogFilter>>,
    default_path: Option<String>,
    multiple: Option<bool>,
    directory: Option<bool>,
    recursive: Option<bool>,
    can_create_directories: Option<bool>,
    picker_mode: Option<DesktopDialogPickerMode>,
    file_access_mode: Option<DesktopDialogFileAccessMode>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopDialogSaveRequest {
    title: Option<String>,
    filters: Option<Vec<DesktopDialogFilter>>,
    default_path: Option<String>,
    can_create_directories: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopOpenUrlRequest {
    url: String,
    with: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopOpenPathRequest {
    path: String,
    with: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopRevealItemRequest {
    path: String,
}

fn trim_to_non_empty(input: &str, max_len: usize) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.chars().take(max_len).collect())
}

fn to_duration(timeout_ms: Option<u64>) -> Option<Duration> {
    timeout_ms.map(Duration::from_millis)
}

fn beta_updater_endpoint(cache_buster: i64) -> Result<tauri::Url, String> {
    let mut endpoint = tauri::Url::parse(BETA_UPDATER_ENDPOINT)
        .map_err(|error| updater_error(UPDATER_ERROR_ENDPOINT_INVALID, error))?;
    endpoint
        .query_pairs_mut()
        .append_pair("aioCheck", &cache_buster.to_string());
    Ok(endpoint)
}

fn canonical_update_channel<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<crate::settings::UpdateChannel, String> {
    crate::settings::read(app)
        .map(|settings| settings.update_channel)
        .map_err(|error| updater_error(UPDATER_ERROR_SETTINGS_UNAVAILABLE, error))
}

fn ensure_update_channel(
    expected: crate::settings::UpdateChannel,
    canonical: crate::settings::UpdateChannel,
) -> Result<(), String> {
    if expected == canonical {
        return Ok(());
    }
    Err(updater_error(
        UPDATER_ERROR_CHANNEL_CHANGED,
        format_args!("expected {expected}, canonical {canonical}"),
    ))
}

fn ensure_update_channel_state(
    expected_channel: crate::settings::UpdateChannel,
    expected_epoch: u64,
    canonical_channel: crate::settings::UpdateChannel,
    canonical_epoch: u64,
) -> Result<(), String> {
    ensure_update_channel(expected_channel, canonical_channel)?;
    if expected_epoch == canonical_epoch {
        return Ok(());
    }
    Err(updater_error(
        UPDATER_ERROR_CHANNEL_CHANGED,
        format_args!("expected epoch {expected_epoch}, canonical epoch {canonical_epoch}"),
    ))
}

fn canonical_update_channel_snapshot<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(crate::settings::UpdateChannel, u64), String> {
    let _channel_guard = crate::settings::lock_update_channel_transition();
    Ok((
        canonical_update_channel(app)?,
        crate::settings::update_channel_epoch(),
    ))
}

fn ensure_canonical_update_channel_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    expected_channel: crate::settings::UpdateChannel,
    expected_epoch: u64,
) -> Result<(), String> {
    let _channel_guard = crate::settings::lock_update_channel_transition();
    ensure_update_channel_state(
        expected_channel,
        expected_epoch,
        canonical_update_channel(app)?,
        crate::settings::update_channel_epoch(),
    )
}

fn updater_release_tag(
    channel: crate::settings::UpdateChannel,
    version: &str,
) -> Result<String, String> {
    static RELEASE_VERSION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let release_version = RELEASE_VERSION.get_or_init(|| {
        regex::Regex::new(r"^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-beta\.[1-9]\d*)?$")
            .expect("updater release version regex")
    });
    if !release_version.is_match(version)
        || (channel == crate::settings::UpdateChannel::Stable && version.contains("-beta."))
    {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            format_args!("version {version:?} is not valid for {channel}"),
        ));
    }
    Ok(format!("{RELEASE_TAG_PREFIX}{version}"))
}

fn object_has_exact_keys(
    object: &serde_json::Map<String, serde_json::Value>,
    expected: &[&str],
) -> bool {
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

fn validate_beta_updater_manifest(
    raw_json: &serde_json::Value,
    parsed_version: &str,
) -> Result<(), String> {
    let manifest = raw_json.as_object().ok_or_else(|| {
        updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "beta updater manifest must be an object",
        )
    })?;
    if !object_has_exact_keys(manifest, &["version", "notes", "pub_date", "platforms"]) {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "beta updater manifest fields do not match the release schema",
        ));
    }

    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            updater_error(
                UPDATER_ERROR_MANIFEST_INVALID,
                "beta updater manifest version must be text",
            )
        })?;
    if version != parsed_version {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "beta updater manifest version is not canonical",
        ));
    }
    let release_tag = updater_release_tag(crate::settings::UpdateChannel::Beta, version)?;

    if !manifest
        .get("notes")
        .is_some_and(serde_json::Value::is_string)
    {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "beta updater manifest notes must be text",
        ));
    }
    static UTC_TIMESTAMP: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let utc_timestamp = UTC_TIMESTAMP.get_or_init(|| {
        regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$")
            .expect("updater UTC timestamp regex")
    });
    if manifest
        .get("pub_date")
        .and_then(serde_json::Value::as_str)
        .filter(|value| utc_timestamp.is_match(value))
        .is_none()
    {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "beta updater manifest pub_date must use canonical UTC format",
        ));
    }

    let platforms = manifest
        .get("platforms")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            updater_error(
                UPDATER_ERROR_MANIFEST_INVALID,
                "beta updater manifest must use the static platforms format",
            )
        })?;
    if !object_has_exact_keys(platforms, &OFFICIAL_UPDATER_PLATFORMS) {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "beta updater manifest platform set does not match the official support matrix",
        ));
    }

    for target in OFFICIAL_UPDATER_PLATFORMS {
        let platform = platforms
            .get(target)
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                updater_error(
                    UPDATER_ERROR_MANIFEST_INVALID,
                    format_args!("beta updater platform {target:?} must be an object"),
                )
            })?;
        if !object_has_exact_keys(platform, &["signature", "url"]) {
            return Err(updater_error(
                UPDATER_ERROR_MANIFEST_INVALID,
                format_args!("beta updater platform {target:?} fields are invalid"),
            ));
        }
        if platform
            .get("signature")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .is_none()
        {
            return Err(updater_error(
                UPDATER_ERROR_MANIFEST_INVALID,
                format_args!("beta updater platform {target:?} signature is missing"),
            ));
        }
        let url = platform
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                updater_error(
                    UPDATER_ERROR_MANIFEST_INVALID,
                    format_args!("beta updater platform {target:?} URL is missing"),
                )
            })?;
        let url = tauri::Url::parse(url)
            .map_err(|error| updater_error(UPDATER_ERROR_MANIFEST_INVALID, error))?;
        validate_official_download_url(&url, &release_tag, official_updater_asset_name(target)?)?;
    }

    Ok(())
}

fn official_updater_asset_name(target: &str) -> Result<&'static str, String> {
    match target {
        "windows-x86_64" => Ok("aio-coding-hub-win64.msi"),
        "darwin-x86_64" => Ok("aio-coding-hub-macos-intel.tar.gz"),
        "darwin-aarch64" => Ok("aio-coding-hub-macos-arm.tar.gz"),
        "linux-x86_64" => Ok("aio-coding-hub-linux-amd64.AppImage"),
        _ => Err(updater_error(
            UPDATER_ERROR_PLATFORM_UNSUPPORTED,
            format_args!("target {target:?} is not in the official updater matrix"),
        )),
    }
}

fn validate_official_download_url(
    download_url: &tauri::Url,
    expected_tag: &str,
    expected_asset: &str,
) -> Result<(), String> {
    let segments = download_url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    let valid_path = segments.len() == 6
        && segments[0] == "FingerCaster"
        && segments[1] == "aio-coding-hub"
        && segments[2] == "releases"
        && segments[3] == "download"
        && segments[4] == expected_tag
        && segments[5] == expected_asset;
    if download_url.scheme() != "https"
        || download_url.host_str() != Some("github.com")
        || !download_url.username().is_empty()
        || download_url.password().is_some()
        || download_url.port().is_some()
        || download_url.query().is_some()
        || download_url.fragment().is_some()
        || !valid_path
    {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "platform URL does not match the canonical Release tag and asset",
        ));
    }
    Ok(())
}

fn updater_candidate_identity_from_parts(
    channel: crate::settings::UpdateChannel,
    version: &str,
    target: &str,
    expected_target: &str,
    download_url: &tauri::Url,
    signature: &str,
) -> Result<(UpdaterCandidateIdentity, String), String> {
    let release_tag = updater_release_tag(channel, version)?;
    if target != expected_target || signature.trim().is_empty() {
        return Err(updater_error(
            UPDATER_ERROR_MANIFEST_INVALID,
            "target must match the current platform and signature must be non-empty",
        ));
    }
    let expected_asset = official_updater_asset_name(expected_target)?;
    validate_official_download_url(download_url, &release_tag, expected_asset)?;

    let identity = UpdaterCandidateIdentity {
        version: version.to_string(),
        target: target.to_string(),
        download_url: download_url.as_str().to_string(),
        signature_sha256: format!("{:x}", Sha256::digest(signature.as_bytes())),
    };
    let release_url = format!("{RELEASES_BASE_URL}/tag/{release_tag}");
    Ok((identity, release_url))
}

fn updater_candidate_identity(
    channel: crate::settings::UpdateChannel,
    update: &Update,
) -> Result<(UpdaterCandidateIdentity, String), String> {
    let expected_target = tauri_plugin_updater::target().ok_or_else(|| {
        updater_error(
            UPDATER_ERROR_PLATFORM_UNSUPPORTED,
            "cannot determine the current updater target",
        )
    })?;
    updater_candidate_identity_from_parts(
        channel,
        &update.version,
        &update.target,
        &expected_target,
        &update.download_url,
        &update.signature,
    )
}

fn ensure_fresh_beta_candidate(
    expected: &UpdaterCandidateIdentity,
    fresh: &UpdaterCandidateIdentity,
) -> Result<(), String> {
    if expected == fresh {
        return Ok(());
    }
    Err(updater_error(
        UPDATER_ERROR_CANDIDATE_STALE,
        "beta pointer no longer selects this candidate",
    ))
}

fn take_typed_resource<R, T>(app: &tauri::AppHandle<R>, rid: ResourceId) -> Result<Arc<T>, String>
where
    R: tauri::Runtime,
    T: Resource,
{
    app.resources_table().take::<T>(rid).map_err(|_| {
        updater_error(
            UPDATER_ERROR_RESOURCE_CLOSED,
            format_args!("updater resource {rid} is unavailable"),
        )
    })
}

fn discard_typed_resource<R, T>(app: &tauri::AppHandle<R>, rid: ResourceId) -> Result<bool, String>
where
    R: tauri::Runtime,
    T: Resource,
{
    let mut resources = app.resources_table();
    if !resources.has(rid) {
        return Ok(false);
    }
    resources.get::<T>(rid).map_err(|_| {
        updater_error(
            UPDATER_ERROR_RESOURCE_INVALID,
            format_args!("resource {rid} is not an updater resource"),
        )
    })?;
    resources
        .close(rid)
        .map_err(|error| updater_error(UPDATER_ERROR_RESOURCE_CLOSE_FAILED, error))?;
    Ok(true)
}

fn discard_typed_resources_where<R, T, F>(app: &tauri::AppHandle<R>, predicate: F) -> usize
where
    R: tauri::Runtime,
    T: Resource,
    F: Fn(&T) -> bool,
{
    let mut resources = app.resources_table();
    let matching = resources
        .names()
        .filter_map(|(rid, _)| {
            resources
                .get::<T>(rid)
                .ok()
                .filter(|resource| predicate(resource))
                .map(|_| rid)
        })
        .collect::<Vec<_>>();
    for rid in &matching {
        let _ = resources.close(*rid);
    }
    matching.len()
}

fn discard_stale_typed_updater_resources<R, T, F>(
    app: &tauri::AppHandle<R>,
    resource_state: F,
) -> usize
where
    R: tauri::Runtime,
    T: Resource,
    F: Fn(&T) -> (crate::settings::UpdateChannel, u64),
{
    let _channel_guard = crate::settings::lock_update_channel_transition();
    let canonical_epoch = crate::settings::update_channel_epoch();
    let canonical_channel = match canonical_update_channel(app) {
        Ok(channel) => channel,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "discarding all updater resources because canonical channel is unavailable"
            );
            return discard_typed_resources_where::<_, T, _>(app, |_| true);
        }
    };
    let discarded = discard_typed_resources_where::<_, T, _>(app, |resource| {
        let (resource_channel, resource_epoch) = resource_state(resource);
        resource_channel != canonical_channel || resource_epoch != canonical_epoch
    });
    tracing::info!(
        canonical_channel = %canonical_channel,
        canonical_epoch,
        discarded_resources = discarded,
        "stale updater resources reconciled"
    );
    discarded
}

pub(crate) fn discard_stale_updater_resources<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> usize {
    discard_stale_typed_updater_resources::<_, ChannelBoundUpdate, _>(app, |resource| {
        (resource.channel, resource.channel_epoch)
    })
}

fn simplify_path(path: PathBuf) -> PathBuf {
    path.components().collect()
}

fn normalize_existing_path(path: PathBuf) -> PathBuf {
    if path.exists() {
        return std::fs::canonicalize(&path)
            .map(simplify_path)
            .unwrap_or_else(|_| simplify_path(path));
    }

    simplify_path(path)
}

fn sanitize_dialog_filters(
    filters: Option<Vec<DesktopDialogFilter>>,
) -> Result<Vec<DesktopDialogFilter>, String> {
    let Some(filters) = filters else {
        return Ok(Vec::new());
    };

    let mut sanitized = Vec::new();
    for filter in filters {
        let name = trim_to_non_empty(&filter.name, 128).ok_or_else(|| {
            "DESKTOP_DIALOG_INVALID_FILTER_NAME: filter name cannot be empty".to_string()
        })?;
        let extensions = filter
            .extensions
            .into_iter()
            .filter_map(|item| trim_to_non_empty(&item, 64))
            .map(|item| item.trim_start_matches('.').to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if extensions.is_empty() {
            return Err(
                "DESKTOP_DIALOG_INVALID_FILTER: filter extensions cannot be empty".to_string(),
            );
        }
        sanitized.push(DesktopDialogFilter { name, extensions });
    }

    Ok(sanitized)
}

fn apply_dialog_default_path<R: tauri::Runtime>(
    mut dialog: tauri_plugin_dialog::FileDialogBuilder<R>,
    default_path: Option<String>,
) -> Result<tauri_plugin_dialog::FileDialogBuilder<R>, String> {
    let Some(default_path) = default_path else {
        return Ok(dialog);
    };

    let default_path = trim_to_non_empty(&default_path, 4_096).ok_or_else(|| {
        "DESKTOP_DIALOG_INVALID_DEFAULT_PATH: defaultPath cannot be empty".to_string()
    })?;
    let path = simplify_path(PathBuf::from(default_path));
    if path.is_file() || !path.exists() {
        if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) {
            if parent.components().count() > 0 {
                dialog = dialog.set_directory(parent);
            }
            dialog = dialog.set_file_name(file_name.to_string_lossy());
            return Ok(dialog);
        }
    }

    Ok(dialog.set_directory(path))
}

fn build_open_dialog(
    window: &WebviewWindow,
    request: DesktopDialogOpenRequest,
) -> Result<tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>, String> {
    let mut dialog = window.dialog().file();
    #[cfg(any(windows, target_os = "macos"))]
    {
        dialog = dialog.set_parent(window);
    }

    if let Some(title) = request
        .title
        .and_then(|value| trim_to_non_empty(&value, 256))
    {
        dialog = dialog.set_title(title);
    }

    dialog = apply_dialog_default_path(dialog, request.default_path)?;

    if let Some(can_create_directories) = request.can_create_directories {
        dialog = dialog.set_can_create_directories(can_create_directories);
    }
    if let Some(picker_mode) = request.picker_mode {
        dialog = dialog.set_picker_mode(picker_mode.into());
    }
    if let Some(file_access_mode) = request.file_access_mode {
        dialog = dialog.set_file_access_mode(file_access_mode.into());
    }

    for filter in sanitize_dialog_filters(request.filters)? {
        let extensions = filter
            .extensions
            .iter()
            .map(|item| item.as_str())
            .collect::<Vec<_>>();
        dialog = dialog.add_filter(filter.name, &extensions);
    }

    let _ = request.recursive;
    Ok(dialog)
}

fn build_save_dialog(
    window: &WebviewWindow,
    request: DesktopDialogSaveRequest,
) -> Result<tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>, String> {
    let mut dialog = window.dialog().file();
    #[cfg(any(windows, target_os = "macos"))]
    {
        dialog = dialog.set_parent(window);
    }

    if let Some(title) = request
        .title
        .and_then(|value| trim_to_non_empty(&value, 256))
    {
        dialog = dialog.set_title(title);
    }

    dialog = apply_dialog_default_path(dialog, request.default_path)?;

    if let Some(can_create_directories) = request.can_create_directories {
        dialog = dialog.set_can_create_directories(can_create_directories);
    }

    for filter in sanitize_dialog_filters(request.filters)? {
        let extensions = filter
            .extensions
            .iter()
            .map(|item| item.as_str())
            .collect::<Vec<_>>();
        dialog = dialog.add_filter(filter.name, &extensions);
    }

    Ok(dialog)
}

fn file_path_to_string(path: FilePath) -> String {
    path.to_string()
}

fn sanitize_optional_program(input: Option<String>) -> Option<String> {
    input.and_then(|value| trim_to_non_empty(&value, 256))
}

fn sanitize_url(input: String) -> Result<String, String> {
    let url = trim_to_non_empty(&input, 2_048)
        .ok_or_else(|| "DESKTOP_OPEN_URL_EMPTY: url cannot be empty".to_string())?;
    let parsed = tauri::Url::parse(&url)
        .map_err(|error| format!("DESKTOP_OPEN_URL_INVALID: invalid url: {error}"))?;
    match parsed.scheme() {
        "http" | "https" | "mailto" | "tel" => Ok(url),
        scheme => Err(format!(
            "DESKTOP_OPEN_URL_SCHEME_DENIED: unsupported url scheme: {scheme}"
        )),
    }
}

fn sanitize_open_path(input: String) -> Result<PathBuf, String> {
    let path = trim_to_non_empty(&input, 4_096)
        .ok_or_else(|| "DESKTOP_OPEN_PATH_EMPTY: path cannot be empty".to_string())?;
    Ok(normalize_existing_path(PathBuf::from(path)))
}

fn path_is_within_root(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn push_desktop_open_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    let root = normalize_existing_path(root);
    if roots.iter().any(|existing| existing == &root) {
        return;
    }
    roots.push(root);
}

fn desktop_open_allowed_roots<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<PathBuf>, String> {
    let home_dir = crate::infra::app_paths::home_dir(app).map_err(|error| error.to_string())?;
    let app_data_dir =
        crate::infra::app_paths::app_data_dir(app).map_err(|error| error.to_string())?;
    let user_default_codex_home_dir = crate::infra::codex_paths::codex_home_dir_user_default(app)
        .map_err(|error| error.to_string())?;
    let follow_codex_home_dir =
        crate::infra::codex_paths::codex_home_dir_follow_env_or_default(app)
            .map_err(|error| error.to_string())?;
    let effective_codex_home_dir =
        crate::infra::codex_paths::codex_home_dir(app).map_err(|error| error.to_string())?;
    let configured_codex_home_dir = crate::infra::codex_paths::configured_codex_home_dir(app);
    let grok_home_dir =
        crate::infra::grok_config::grok_home_dir(app).map_err(|error| error.to_string())?;

    let mut roots = Vec::new();
    push_desktop_open_root(&mut roots, app_data_dir);
    push_desktop_open_root(&mut roots, home_dir.join(".claude"));
    push_desktop_open_root(&mut roots, home_dir.join(".gemini"));
    push_desktop_open_root(&mut roots, user_default_codex_home_dir);
    push_desktop_open_root(&mut roots, follow_codex_home_dir);
    push_desktop_open_root(&mut roots, effective_codex_home_dir);
    push_desktop_open_root(&mut roots, grok_home_dir);
    if let Some(configured_codex_home_dir) = configured_codex_home_dir {
        push_desktop_open_root(&mut roots, configured_codex_home_dir);
    }

    Ok(roots)
}

fn ensure_desktop_open_path_allowed<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &Path,
) -> Result<(), String> {
    let normalized_path = normalize_existing_path(path.to_path_buf());
    let allowed = desktop_open_allowed_roots(app)?;
    if allowed
        .iter()
        .any(|root| path_is_within_root(&normalized_path, root))
    {
        return Ok(());
    }

    Err(format!(
        "DESKTOP_OPEN_PATH_DENIED: path is outside allowed desktop roots: {}",
        normalized_path.display()
    ))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_clipboard_write_text(
    app: tauri::AppHandle,
    text: String,
) -> Result<bool, String> {
    let text = trim_to_non_empty(&text, 1_000_000)
        .ok_or_else(|| "CLIPBOARD_EMPTY_TEXT: text cannot be empty".to_string())?;

    app.clipboard()
        .write_text(Cow::Owned(text))
        .map_err(|error| format!("failed to write clipboard text: {error}"))?;

    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn desktop_dialog_open(
    window: WebviewWindow,
    options: DesktopDialogOpenRequest,
) -> Result<Option<Vec<String>>, String> {
    let multiple = options.multiple.unwrap_or(false);
    let directory = options.directory.unwrap_or(false);
    let dialog = build_open_dialog(&window, options)?;
    let (tx, rx) = oneshot::channel();

    match (directory, multiple) {
        (true, true) => {
            dialog.pick_folders(move |selection| {
                let _ = tx.send(selection.map(|paths| {
                    paths
                        .into_iter()
                        .map(file_path_to_string)
                        .collect::<Vec<_>>()
                }));
            });
        }
        (true, false) => {
            dialog.pick_folder(move |selection| {
                let _ = tx.send(selection.map(|path| vec![file_path_to_string(path)]));
            });
        }
        (false, true) => {
            dialog.pick_files(move |selection| {
                let _ = tx.send(selection.map(|paths| {
                    paths
                        .into_iter()
                        .map(file_path_to_string)
                        .collect::<Vec<_>>()
                }));
            });
        }
        (false, false) => {
            dialog.pick_file(move |selection| {
                let _ = tx.send(selection.map(|path| vec![file_path_to_string(path)]));
            });
        }
    }

    rx.await
        .map_err(|_| "DESKTOP_DIALOG_OPEN_CANCELLED: dialog response channel dropped".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn desktop_dialog_save(
    window: WebviewWindow,
    options: DesktopDialogSaveRequest,
) -> Result<Option<String>, String> {
    let dialog = build_save_dialog(&window, options)?;
    let (tx, rx) = oneshot::channel();

    dialog.save_file(move |selection| {
        let _ = tx.send(selection.map(file_path_to_string));
    });

    rx.await
        .map_err(|_| "DESKTOP_DIALOG_SAVE_CANCELLED: dialog response channel dropped".to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_window_set_theme(
    window: WebviewWindow,
    theme: DesktopThemeMode,
) -> Result<bool, String> {
    window
        .set_theme(theme.into_tauri_theme())
        .map_err(|error| format!("failed to set desktop window theme: {error}"))?;

    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn desktop_opener_open_url(
    app: tauri::AppHandle,
    input: DesktopOpenUrlRequest,
) -> Result<bool, String> {
    let url = sanitize_url(input.url)?;
    let with = sanitize_optional_program(input.with);

    blocking::run("desktop_opener_open_url", move || {
        let with = with.as_deref();
        app.opener()
            .open_url(url, with)
            .map_err(|error| format!("failed to open desktop url: {error}"))?;
        Ok::<bool, crate::shared::error::AppError>(true)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn desktop_opener_open_path(
    app: tauri::AppHandle,
    input: DesktopOpenPathRequest,
) -> Result<bool, String> {
    let path = sanitize_open_path(input.path)?;
    ensure_desktop_open_path_allowed(&app, &path)?;
    let with = sanitize_optional_program(input.with);
    let path_string = path.display().to_string();

    blocking::run("desktop_opener_open_path", move || {
        let with = with.as_deref();
        app.opener()
            .open_path(path_string, with)
            .map_err(|error| format!("failed to open desktop path: {error}"))?;
        Ok::<bool, crate::shared::error::AppError>(true)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn desktop_opener_reveal_item_in_dir(
    app: tauri::AppHandle,
    input: DesktopRevealItemRequest,
) -> Result<bool, String> {
    let path = sanitize_open_path(input.path)?;
    ensure_desktop_open_path_allowed(&app, &path)?;

    blocking::run("desktop_opener_reveal_item_in_dir", move || {
        app.opener()
            .reveal_item_in_dir(path)
            .map_err(|error| format!("failed to reveal desktop item: {error}"))?;
        Ok::<bool, crate::shared::error::AppError>(true)
    })
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_notification_is_permission_granted(
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let granted = matches!(
        app.notification()
            .permission_state()
            .map_err(|error| format!("failed to read notification permission: {error}"))?,
        tauri_plugin_notification::PermissionState::Granted
    );

    Ok(granted)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_notification_request_permission(
    app: tauri::AppHandle,
) -> Result<DesktopNotificationPermissionState, String> {
    let permission = app
        .notification()
        .request_permission()
        .map_err(|error| format!("failed to request notification permission: {error}"))?;

    Ok(permission.into())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_notification_notify(
    app: tauri::AppHandle,
    options: DesktopNotificationPayload,
) -> Result<bool, String> {
    let title = trim_to_non_empty(&options.title, 256)
        .ok_or_else(|| "NOTICE_INVALID_TITLE: title cannot be empty".to_string())?;
    let body = trim_to_non_empty(&options.body, 4_096)
        .ok_or_else(|| "NOTICE_INVALID_BODY: body cannot be empty".to_string())?;
    let sound = options
        .sound
        .as_deref()
        .and_then(|value| trim_to_non_empty(value, 128));

    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(sound) = sound {
        builder = builder.sound(sound);
    }

    builder
        .show()
        .map_err(|error| format!("failed to show desktop notification: {error}"))?;

    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_notification_play_sound() -> Result<bool, String> {
    crate::app::notification_sound::play_notification_sound()?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn desktop_updater_check(
    app: tauri::AppHandle,
    expected_channel: crate::settings::UpdateChannel,
    timeout: Option<u64>,
) -> Result<Option<DesktopUpdaterMetadata>, String> {
    let (canonical_channel, channel_epoch) = canonical_update_channel_snapshot(&app)?;
    ensure_update_channel(expected_channel, canonical_channel)?;

    let update = fetch_updater_candidate(&app, canonical_channel, timeout).await?;
    let prepared = update
        .map(|update| {
            let (identity, release_url) = updater_candidate_identity(canonical_channel, &update)?;
            let metadata = DesktopUpdaterMetadata {
                rid: 0,
                channel: canonical_channel,
                is_prerelease: update.version.contains('-'),
                current_version: update.current_version.clone(),
                version: update.version.clone(),
                date: update.date.map(|value| value.to_string()),
                body: update.body.clone(),
                release_url,
            };
            Ok::<_, String>((update, identity, metadata))
        })
        .transpose()?;

    let _channel_guard = crate::settings::lock_update_channel_transition();
    ensure_update_channel_state(
        canonical_channel,
        channel_epoch,
        canonical_update_channel(&app)?,
        crate::settings::update_channel_epoch(),
    )?;
    if let Some((update, identity, metadata)) = prepared {
        let rid = app.resources_table().add(ChannelBoundUpdate {
            update,
            channel: canonical_channel,
            channel_epoch,
            identity,
        });

        return Ok(Some(DesktopUpdaterMetadata { rid, ..metadata }));
    }

    Ok(None)
}

async fn fetch_updater_candidate<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    channel: crate::settings::UpdateChannel,
    timeout: Option<u64>,
) -> Result<Option<Update>, String> {
    let mut builder = app.updater_builder();
    if channel == crate::settings::UpdateChannel::Beta {
        builder = builder
            .endpoints(vec![beta_updater_endpoint(
                crate::shared::time::now_unix_millis(),
            )?])
            .map_err(|error| updater_error(UPDATER_ERROR_ENDPOINT_INVALID, error))?;
    }
    if let Some(timeout) = to_duration(timeout) {
        builder = builder.timeout(timeout);
    }

    let updater = builder
        .build()
        .map_err(|error| updater_error(UPDATER_ERROR_CLIENT_BUILD_FAILED, error))?;
    let update = updater
        .check()
        .await
        .map_err(|error| updater_error(UPDATER_ERROR_CHECK_FAILED, error))?;
    if channel == crate::settings::UpdateChannel::Beta {
        if let Some(update) = &update {
            validate_beta_updater_manifest(&update.raw_json, &update.version)?;
        }
    }
    Ok(update)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn desktop_updater_discard(
    app: tauri::AppHandle,
    rid: ResourceId,
) -> Result<bool, String> {
    discard_typed_resource::<_, ChannelBoundUpdate>(&app, rid)
}

#[tauri::command]
pub(crate) async fn desktop_updater_download_and_install(
    app: tauri::AppHandle,
    rid: ResourceId,
    on_event: Channel<DesktopUpdaterDownloadEvent>,
    timeout: Option<u64>,
    confirm: Option<RiskyIpcConfirm>,
) -> Result<bool, String> {
    // Taking the resource first makes install a one-shot operation. Confirmation,
    // channel validation, fresh-check, download, and install errors all leave the
    // rid closed instead of retaining a stale or cross-channel candidate.
    let bound = take_typed_resource::<_, ChannelBoundUpdate>(&app, rid)?;
    RiskyIpcConfirm::require(
        confirm,
        "desktop_updater_download_and_install",
        format!("updater:{rid}"),
    )?;

    ensure_canonical_update_channel_state(&app, bound.channel, bound.channel_epoch)?;
    if bound.channel == crate::settings::UpdateChannel::Beta {
        let fresh = fetch_updater_candidate(&app, bound.channel, timeout)
            .await
            .map_err(|error| updater_error(UPDATER_ERROR_BETA_FRESH_CHECK_FAILED, error))?
            .ok_or_else(|| {
                updater_error(
                    UPDATER_ERROR_CANDIDATE_STALE,
                    "beta pointer no longer offers an update",
                )
            })?;
        let (fresh_identity, _) = updater_candidate_identity(bound.channel, &fresh)
            .map_err(|error| updater_error(UPDATER_ERROR_BETA_FRESH_CHECK_FAILED, error))?;
        ensure_fresh_beta_candidate(&bound.identity, &fresh_identity)?;
        ensure_canonical_update_channel_state(&app, bound.channel, bound.channel_epoch)?;
    }

    let mut update = bound.update.clone();
    update.timeout = to_duration(timeout);

    let mut first_chunk = true;
    let bytes = update
        .download(
            |chunk_length, content_length| {
                if first_chunk {
                    first_chunk = false;
                    let _ = on_event.send(DesktopUpdaterDownloadEvent::Started { content_length });
                }
                let _ = on_event.send(DesktopUpdaterDownloadEvent::Progress { chunk_length });
            },
            || {
                let _ = on_event.send(DesktopUpdaterDownloadEvent::Finished);
            },
        )
        .await
        .map_err(|error| updater_error(UPDATER_ERROR_DOWNLOAD_FAILED, error))?;

    // The transition guard closes the final check-to-install window. Dedicated
    // channel changes and portable imports cannot commit until install returns.
    let _channel_guard = crate::settings::lock_update_channel_transition();
    ensure_update_channel_state(
        bound.channel,
        bound.channel_epoch,
        canonical_update_channel(&app)?,
        crate::settings::update_channel_epoch(),
    )?;
    update
        .install(bytes)
        .map_err(|error| updater_error(UPDATER_ERROR_INSTALL_FAILED, error))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        beta_updater_endpoint, desktop_open_allowed_roots, discard_stale_typed_updater_resources,
        discard_typed_resource, discard_typed_resources_where, ensure_desktop_open_path_allowed,
        ensure_fresh_beta_candidate, ensure_update_channel, ensure_update_channel_state,
        normalize_existing_path, take_typed_resource, updater_candidate_identity_from_parts,
        validate_beta_updater_manifest, BETA_UPDATER_ENDPOINT, RELEASES_BASE_URL,
        UPDATER_ERROR_CANDIDATE_STALE, UPDATER_ERROR_CHANNEL_CHANGED,
        UPDATER_ERROR_MANIFEST_INVALID, UPDATER_ERROR_RESOURCE_CLOSED,
        UPDATER_ERROR_RESOURCE_INVALID,
    };
    use crate::infra::settings::{self, AppSettings, CodexHomeMode};
    use crate::test_support::{clear_settings_cache, test_env_lock};
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tauri::Manager;

    static TEST_ENV_SEQ: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug)]
    struct MockUpdaterResource {
        channel: settings::UpdateChannel,
        channel_epoch: u64,
    }

    impl tauri::Resource for MockUpdaterResource {}

    #[derive(Debug)]
    struct OtherResource;

    impl tauri::Resource for OtherResource {}

    #[derive(Default)]
    struct EnvRestore {
        saved: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvRestore {
        fn save_once(&mut self, key: &'static str) {
            if self.saved.iter().any(|(saved_key, _)| *saved_key == key) {
                return;
            }
            self.saved.push((key, std::env::var_os(key)));
        }

        fn set_var(&mut self, key: &'static str, value: impl Into<OsString>) {
            self.save_once(key);
            std::env::set_var(key, value.into());
        }

        fn remove_var(&mut self, key: &'static str) {
            self.save_once(key);
            std::env::remove_var(key);
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..).rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    struct DesktopCommandTestApp {
        #[allow(dead_code)]
        env_restore: EnvRestore,
        #[allow(dead_code)]
        env_lock: std::sync::MutexGuard<'static, ()>,
        #[allow(dead_code)]
        home_dir: tempfile::TempDir,
        app: tauri::App<tauri::test::MockRuntime>,
    }

    impl DesktopCommandTestApp {
        fn new() -> Self {
            let env_lock = test_env_lock();
            let home_dir = tempfile::tempdir().expect("tempdir");
            let seq = TEST_ENV_SEQ.fetch_add(1, Ordering::Relaxed);
            let mut env_restore = EnvRestore::default();
            env_restore.set_var(
                "AIO_CODING_HUB_HOME_DIR",
                home_dir.path().as_os_str().to_os_string(),
            );
            env_restore.set_var(
                "AIO_CODING_HUB_DOTDIR_NAME",
                format!(".aio-coding-hub-desktop-test-{seq}"),
            );
            env_restore.remove_var("AIO_CODING_HUB_TEST_HOME");
            clear_settings_cache();

            Self {
                env_lock,
                env_restore,
                home_dir,
                app: tauri::test::mock_app(),
            }
        }

        fn handle(&self) -> tauri::AppHandle<tauri::test::MockRuntime> {
            self.app.handle().clone()
        }
    }

    fn write_custom_codex_home<R: tauri::Runtime>(app: &tauri::AppHandle<R>, custom_home: &Path) {
        let settings = AppSettings {
            codex_home_mode: CodexHomeMode::Custom,
            codex_home_override: custom_home.display().to_string(),
            ..AppSettings::default()
        };
        settings::write(app, &settings).expect("write settings");
    }

    fn assert_updater_error_code(error: &str, expected: &str) {
        assert_eq!(
            error.split_once(':').map_or(error, |(code, _)| code),
            expected
        );
    }

    fn update_channel_confirm() -> crate::shared::ipc_confirm::RiskyIpcConfirm {
        crate::shared::ipc_confirm::RiskyIpcConfirm {
            confirm: crate::shared::ipc_confirm::IpcConfirm {
                action: crate::app::settings_service::UPDATE_CHANNEL_CONFIRM_ACTION.to_string(),
                resource: crate::app::settings_service::UPDATE_CHANNEL_BETA_CONFIRM_RESOURCE
                    .to_string(),
                nonce: "betaDesktopCommandNonce123".to_string(),
                issued_at_ms: crate::shared::time::now_unix_millis(),
                ttl_ms: 60_000,
            },
        }
    }

    fn valid_beta_manifest(version: &str) -> serde_json::Value {
        let tag = format!("aio-coding-hub-v{version}");
        serde_json::json!({
            "version": version,
            "notes": "verified release notes",
            "pub_date": "2026-08-10T12:34:56.789Z",
            "platforms": {
                "windows-x86_64": {
                    "url": format!("https://github.com/FingerCaster/aio-coding-hub/releases/download/{tag}/aio-coding-hub-win64.msi"),
                    "signature": "windows-signature"
                },
                "darwin-x86_64": {
                    "url": format!("https://github.com/FingerCaster/aio-coding-hub/releases/download/{tag}/aio-coding-hub-macos-intel.tar.gz"),
                    "signature": "macos-intel-signature"
                },
                "darwin-aarch64": {
                    "url": format!("https://github.com/FingerCaster/aio-coding-hub/releases/download/{tag}/aio-coding-hub-macos-arm.tar.gz"),
                    "signature": "macos-arm-signature"
                },
                "linux-x86_64": {
                    "url": format!("https://github.com/FingerCaster/aio-coding-hub/releases/download/{tag}/aio-coding-hub-linux-amd64.AppImage"),
                    "signature": "linux-signature"
                }
            }
        })
    }

    #[test]
    fn beta_endpoint_is_fixed_and_adds_only_a_controlled_cache_buster() {
        let endpoint = beta_updater_endpoint(123_456).expect("beta endpoint");
        assert_eq!(
            format!(
                "{}://{}{}",
                endpoint.scheme(),
                endpoint.host_str().expect("host"),
                endpoint.path()
            ),
            BETA_UPDATER_ENDPOINT
        );
        assert_eq!(endpoint.query(), Some("aioCheck=123456"));
    }

    #[test]
    fn beta_manifest_contract_accepts_only_the_exact_four_platform_static_schema() {
        validate_beta_updater_manifest(&valid_beta_manifest("0.60.41-beta.2"), "0.60.41-beta.2")
            .expect("canonical beta manifest");
        validate_beta_updater_manifest(&valid_beta_manifest("0.60.41"), "0.60.41")
            .expect("beta channel may select a stable release");

        let dynamic = serde_json::json!({
            "version": "0.60.41-beta.2",
            "notes": "notes",
            "pub_date": "2026-08-10T12:34:56Z",
            "url": "https://example.invalid/update",
            "signature": "signature"
        });
        let error = validate_beta_updater_manifest(&dynamic, "0.60.41-beta.2").unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);

        let mut partial = valid_beta_manifest("0.60.41-beta.2");
        partial["platforms"]
            .as_object_mut()
            .expect("platforms")
            .remove("linux-x86_64");
        let error = validate_beta_updater_manifest(&partial, "0.60.41-beta.2").unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);

        let mut extra = valid_beta_manifest("0.60.41-beta.2");
        extra["platforms"]
            .as_object_mut()
            .expect("platforms")
            .insert(
                "windows-aarch64".to_string(),
                serde_json::json!({ "url": "https://example.invalid/update", "signature": "x" }),
            );
        let error = validate_beta_updater_manifest(&extra, "0.60.41-beta.2").unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);
    }

    #[test]
    fn beta_manifest_contract_rejects_v_prefixed_and_noncanonical_release_assets() {
        let mut prefixed = valid_beta_manifest("0.60.41-beta.2");
        prefixed["version"] = serde_json::json!("v0.60.41-beta.2");
        let error = validate_beta_updater_manifest(&prefixed, "0.60.41-beta.2").unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);

        let mut wrong_asset = valid_beta_manifest("0.60.41-beta.2");
        wrong_asset["platforms"]["darwin-aarch64"]["url"] = serde_json::json!(
            "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41-beta.2/aio-coding-hub-macos-intel.tar.gz"
        );
        let error = validate_beta_updater_manifest(&wrong_asset, "0.60.41-beta.2").unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);
    }

    #[test]
    fn updater_candidate_contract_binds_channel_release_url_and_official_asset() {
        let beta_url = tauri::Url::parse(
            "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41-beta.2/aio-coding-hub-win64.msi",
        )
        .unwrap();
        let (identity, release_url) = updater_candidate_identity_from_parts(
            settings::UpdateChannel::Beta,
            "0.60.41-beta.2",
            "windows-x86_64",
            "windows-x86_64",
            &beta_url,
            "signed-candidate",
        )
        .expect("valid beta candidate");
        assert_eq!(identity.version, "0.60.41-beta.2");
        assert_eq!(
            release_url,
            format!("{RELEASES_BASE_URL}/tag/aio-coding-hub-v0.60.41-beta.2")
        );

        let stable_on_beta = tauri::Url::parse(
            "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41/aio-coding-hub-win64.msi",
        )
        .unwrap();
        assert!(updater_candidate_identity_from_parts(
            settings::UpdateChannel::Beta,
            "0.60.41",
            "windows-x86_64",
            "windows-x86_64",
            &stable_on_beta,
            "signed-stable",
        )
        .is_ok());

        let error = updater_candidate_identity_from_parts(
            settings::UpdateChannel::Stable,
            "0.60.41-beta.2",
            "windows-x86_64",
            "windows-x86_64",
            &beta_url,
            "signed-candidate",
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);

        let arbitrary = tauri::Url::parse("https://example.invalid/update.exe").unwrap();
        let error = updater_candidate_identity_from_parts(
            settings::UpdateChannel::Beta,
            "0.60.41-beta.2",
            "windows-x86_64",
            "windows-x86_64",
            &arbitrary,
            "signed-candidate",
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);

        let wrong_asset = tauri::Url::parse(
            "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41-beta.2/aio-coding-hub.exe",
        )
        .unwrap();
        let error = updater_candidate_identity_from_parts(
            settings::UpdateChannel::Beta,
            "0.60.41-beta.2",
            "windows-x86_64",
            "windows-x86_64",
            &wrong_asset,
            "signed-candidate",
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);

        let error = updater_candidate_identity_from_parts(
            settings::UpdateChannel::Beta,
            "0.60.41-beta.2",
            "darwin-aarch64",
            "windows-x86_64",
            &beta_url,
            "signed-candidate",
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);
    }

    #[test]
    fn updater_candidate_contract_rejects_leading_zero_versions() {
        let url = tauri::Url::parse(
            "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v1.2.3/aio-coding-hub-win64.msi",
        )
        .unwrap();
        for version in ["01.2.3", "1.02.3", "1.2.03", "1.2.3-beta.01"] {
            let error = updater_candidate_identity_from_parts(
                settings::UpdateChannel::Beta,
                version,
                "windows-x86_64",
                "windows-x86_64",
                &url,
                "signed-candidate",
            )
            .unwrap_err();
            assert_updater_error_code(&error, UPDATER_ERROR_MANIFEST_INVALID);
        }
    }

    #[test]
    fn updater_candidate_contract_pins_every_official_platform_asset() {
        for (target, asset) in [
            ("windows-x86_64", "aio-coding-hub-win64.msi"),
            ("darwin-x86_64", "aio-coding-hub-macos-intel.tar.gz"),
            ("darwin-aarch64", "aio-coding-hub-macos-arm.tar.gz"),
            ("linux-x86_64", "aio-coding-hub-linux-amd64.AppImage"),
        ] {
            let url = tauri::Url::parse(&format!(
                "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v1.2.3/{asset}"
            ))
            .unwrap();
            updater_candidate_identity_from_parts(
                settings::UpdateChannel::Stable,
                "1.2.3",
                target,
                target,
                &url,
                "signed-candidate",
            )
            .expect("official platform asset");
        }
    }

    #[test]
    fn beta_fresh_check_rejects_every_candidate_identity_change() {
        let url = tauri::Url::parse(
            "https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41-beta.2/aio-coding-hub-win64.msi",
        )
        .unwrap();
        let (expected, _) = updater_candidate_identity_from_parts(
            settings::UpdateChannel::Beta,
            "0.60.41-beta.2",
            "windows-x86_64",
            "windows-x86_64",
            &url,
            "signature-a",
        )
        .unwrap();
        assert!(ensure_fresh_beta_candidate(&expected, &expected).is_ok());

        for changed in [
            {
                let mut value = expected.clone();
                value.version = "0.60.41-beta.3".to_string();
                value
            },
            {
                let mut value = expected.clone();
                value.target = "windows-aarch64-nsis".to_string();
                value
            },
            {
                let mut value = expected.clone();
                value.download_url.push_str(".changed");
                value
            },
            {
                let mut value = expected.clone();
                value.signature_sha256 = "changed".to_string();
                value
            },
        ] {
            let error = ensure_fresh_beta_candidate(&expected, &changed).unwrap_err();
            assert_updater_error_code(&error, UPDATER_ERROR_CANDIDATE_STALE);
        }
    }

    #[test]
    fn updater_channel_mismatch_is_typed_and_never_coerces_to_beta() {
        assert!(ensure_update_channel(
            settings::UpdateChannel::Stable,
            settings::UpdateChannel::Stable
        )
        .is_ok());
        let error = ensure_update_channel(
            settings::UpdateChannel::Beta,
            settings::UpdateChannel::Stable,
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_CHANNEL_CHANGED);
    }

    #[test]
    fn updater_switch_during_fresh_check_is_rejected_even_after_switching_back() {
        let error = ensure_update_channel_state(
            settings::UpdateChannel::Beta,
            10,
            settings::UpdateChannel::Beta,
            12,
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_CHANNEL_CHANGED);
    }

    #[test]
    fn updater_switch_during_download_is_rejected_before_install() {
        let error = ensure_update_channel_state(
            settings::UpdateChannel::Beta,
            10,
            settings::UpdateChannel::Stable,
            11,
        )
        .unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_CHANNEL_CHANGED);
    }

    #[test]
    fn updater_switch_during_install_waits_for_the_install_gate() {
        let test_app = DesktopCommandTestApp::new();
        let handle = test_app.handle();
        settings::write(
            &handle,
            &AppSettings {
                update_channel: settings::UpdateChannel::Beta,
                ..AppSettings::default()
            },
        )
        .expect("seed beta channel");

        let install_guard = settings::lock_update_channel_transition();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).expect("signal switch start");
            let result = crate::app::settings_service::settings_update_channel_set_sync(
                &handle,
                settings::UpdateChannel::Stable,
                None,
            );
            result_tx.send(result).expect("send switch result");
        });

        started_rx.recv().expect("switch worker started");
        assert!(result_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());
        drop(install_guard);

        let view = result_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("switch completes after install gate")
            .expect("switch succeeds");
        assert_eq!(view.update_channel, settings::UpdateChannel::Stable);
        worker.join().expect("switch worker");
    }

    #[test]
    fn updater_resource_take_and_discard_are_typed_and_idempotent() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let (beta_rid, stable_rid, other_rid, take_rid) = {
            let mut resources = handle.resources_table();
            (
                resources.add(MockUpdaterResource {
                    channel: settings::UpdateChannel::Beta,
                    channel_epoch: 1,
                }),
                resources.add(MockUpdaterResource {
                    channel: settings::UpdateChannel::Stable,
                    channel_epoch: 1,
                }),
                resources.add(OtherResource),
                resources.add(MockUpdaterResource {
                    channel: settings::UpdateChannel::Beta,
                    channel_epoch: 1,
                }),
            )
        };

        assert_eq!(
            discard_typed_resources_where::<_, MockUpdaterResource, _>(&handle, |resource| {
                resource.channel == settings::UpdateChannel::Beta
            }),
            2
        );
        assert!(!handle.resources_table().has(beta_rid));
        assert!(!handle.resources_table().has(take_rid));
        assert!(handle.resources_table().has(stable_rid));

        assert!(discard_typed_resource::<_, MockUpdaterResource>(&handle, stable_rid).unwrap());
        assert!(!discard_typed_resource::<_, MockUpdaterResource>(&handle, stable_rid).unwrap());
        let error =
            discard_typed_resource::<_, MockUpdaterResource>(&handle, other_rid).unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_RESOURCE_INVALID);
        assert!(handle.resources_table().has(other_rid));

        let fresh_take_rid = handle.resources_table().add(MockUpdaterResource {
            channel: settings::UpdateChannel::Stable,
            channel_epoch: 1,
        });
        let _taken = take_typed_resource::<_, MockUpdaterResource>(&handle, fresh_take_rid)
            .expect("take updater resource");
        assert!(!handle.resources_table().has(fresh_take_rid));
        let error =
            take_typed_resource::<_, MockUpdaterResource>(&handle, fresh_take_rid).unwrap_err();
        assert_updater_error_code(&error, UPDATER_ERROR_RESOURCE_CLOSED);
    }

    #[test]
    fn delayed_transition_cleanup_preserves_a_new_current_epoch_resource() {
        let test_app = DesktopCommandTestApp::new();
        let handle = test_app.handle();
        let initial_epoch = settings::update_channel_epoch();
        let old_stable_rid = handle.resources_table().add(MockUpdaterResource {
            channel: settings::UpdateChannel::Stable,
            channel_epoch: initial_epoch,
        });

        crate::app::settings_service::settings_update_channel_set_sync(
            &handle,
            settings::UpdateChannel::Beta,
            Some(update_channel_confirm()),
        )
        .expect("first transition enters beta");
        let beta_epoch = settings::update_channel_epoch();
        let beta_rid = handle.resources_table().add(MockUpdaterResource {
            channel: settings::UpdateChannel::Beta,
            channel_epoch: beta_epoch,
        });

        // Model the interleaving that used to be unsafe: transition A has
        // committed but delayed its cleanup while transition B switches back
        // and a check publishes a valid resource for B's epoch.
        let worker_handle = handle.clone();
        let worker = std::thread::spawn(move || {
            crate::app::settings_service::settings_update_channel_set_sync(
                &worker_handle,
                settings::UpdateChannel::Stable,
                None,
            )
            .expect("second transition returns to stable");
            let _guard = settings::lock_update_channel_transition();
            let current = settings::read(&worker_handle).expect("current settings");
            let current_epoch = settings::update_channel_epoch();
            assert_eq!(current.update_channel, settings::UpdateChannel::Stable);
            let rid = worker_handle.resources_table().add(MockUpdaterResource {
                channel: current.update_channel,
                channel_epoch: current_epoch,
            });
            (rid, current_epoch)
        });
        let (current_stable_rid, current_epoch) = worker.join().expect("transition worker");
        assert!(current_epoch > beta_epoch);

        let discarded = discard_stale_typed_updater_resources::<_, MockUpdaterResource, _>(
            &handle,
            |resource| (resource.channel, resource.channel_epoch),
        );

        assert_eq!(discarded, 2);
        assert!(!handle.resources_table().has(old_stable_rid));
        assert!(!handle.resources_table().has(beta_rid));
        assert!(
            handle.resources_table().has(current_stable_rid),
            "delayed cleanup must not close the valid rid created after a newer transition"
        );
    }

    #[test]
    fn desktop_open_allowed_roots_include_custom_codex_home() {
        let test_app = DesktopCommandTestApp::new();
        let app_handle = test_app.handle();
        let custom_home = test_app.home_dir.path().join("custom-codex-home");
        write_custom_codex_home(&app_handle, &custom_home);

        let allowed_roots = desktop_open_allowed_roots(&app_handle).expect("allowed roots");

        assert!(allowed_roots.contains(&normalize_existing_path(custom_home)));
    }

    #[test]
    fn desktop_open_path_allows_paths_under_custom_codex_home() {
        let test_app = DesktopCommandTestApp::new();
        let app_handle = test_app.handle();
        let custom_home = test_app.home_dir.path().join("custom-codex-home");
        write_custom_codex_home(&app_handle, &custom_home);

        let config_path = custom_home.join("config.toml");

        assert!(ensure_desktop_open_path_allowed(&app_handle, &config_path).is_ok());
    }

    #[test]
    fn desktop_open_allowed_roots_include_only_effective_grok_home() {
        let mut test_app = DesktopCommandTestApp::new();
        let app_handle = test_app.handle();
        let default_home = test_app.home_dir.path().join(".grok");
        let custom_home = test_app.home_dir.path().join("custom-grok-home");
        test_app
            .env_restore
            .set_var("GROK_HOME", custom_home.as_os_str().to_os_string());

        let allowed_roots = desktop_open_allowed_roots(&app_handle).expect("allowed roots");

        assert!(allowed_roots.contains(&normalize_existing_path(custom_home)));
        assert!(!allowed_roots.contains(&normalize_existing_path(default_home)));
    }

    #[test]
    fn desktop_open_path_allows_paths_under_effective_grok_home() {
        let mut test_app = DesktopCommandTestApp::new();
        let app_handle = test_app.handle();
        let custom_home = test_app.home_dir.path().join("custom-grok-home");
        test_app
            .env_restore
            .set_var("GROK_HOME", custom_home.as_os_str().to_os_string());

        let config_path = custom_home.join("config.toml");

        assert!(ensure_desktop_open_path_allowed(&app_handle, &config_path).is_ok());
    }
}
