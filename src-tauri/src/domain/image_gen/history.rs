//! Usage: image_gen_tasks history persistence: task files on disk (original +
//! thumbnail + reference images per task directory) and metadata rows in SQLite.
//!
//! Path safety: `read_image` only serves files whose canonical path lives inside
//! the current storage dir or a task dir recorded in the DB; task dir removal is
//! guarded by a dir-name == task-id check.

use crate::db;
use crate::shared::error::{db_err, AppResult};
use base64::Engine as _;
use rusqlite::OptionalExtension;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use super::transport::ImageGenFetchedImage;

const MAX_PERSIST_TOTAL_BYTES: usize = 96 * 1024 * 1024;
const MAX_READ_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const HISTORY_HYDRATE_PER_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const HISTORY_HYDRATE_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_HYDRATE_REFERENCES: usize = 512;
const MAX_TASK_ID_CHARS: usize = 128;
const MAX_LIST_LIMIT: u32 = 100;
const MAX_CURSOR_BYTES: usize = 512;
const CURSOR_VERSION: u8 = 1;
const MAX_TRUSTED_STORAGE_ROOTS: usize = 64;

pub(crate) const IMAGE_GEN_STORAGE_DIR_NAME: &str = "image-gen";

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskFilePayload {
    pub mime: String,
    pub data_b64: String,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskPersistPayload {
    pub id: String,
    pub adapter_id: Option<String>,
    pub prompt: String,
    pub request_json: String,
    pub status: String,
    pub error: Option<String>,
    pub usage_json: Option<String>,
    pub created_at: i64,
    pub elapsed_ms: Option<i64>,
    pub images: Vec<ImageGenTaskFilePayload>,
    /// Frontend-generated thumbnails, paired with `images` by index. Fewer
    /// thumbs than images is tolerated (missing thumb -> no thumb path).
    pub thumbs: Vec<ImageGenTaskFilePayload>,
    pub ref_images: Vec<ImageGenTaskFilePayload>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskFileRow {
    /// Opaque backend-validated reference (`task-id/filename`), never a path.
    pub path: String,
    /// Opaque backend-validated thumbnail reference.
    pub thumb_path: Option<String>,
    pub mime: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTaskRow {
    pub id: String,
    pub adapter_id: String,
    pub prompt: String,
    pub request_json: String,
    pub status: String,
    pub error: Option<String>,
    pub usage_json: Option<String>,
    pub images: Vec<ImageGenTaskFileRow>,
    pub ref_images: Vec<ImageGenTaskFileRow>,
    pub dir: String,
    pub created_at: i64,
    pub elapsed_ms: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenTasksPage {
    pub items: Vec<ImageGenTaskRow>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageGenTasksCursor {
    v: u8,
    created_at: i64,
    id: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageGenStorageView {
    pub dir: String,
    pub total_bytes: i64,
    pub task_count: u32,
}

/// On-disk file reference stored inside images_json / ref_images_json
/// (file names relative to the task dir).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredFile {
    file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thumb: Option<String>,
    mime: String,
}

/// Resolves the effective storage directory: settings override or the default
/// `<app data dir>/image-gen`.
pub(crate) fn storage_dir_from_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<PathBuf> {
    let settings = crate::settings::read(app)?;
    match settings
        .image_gen_storage_dir
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
    {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(crate::app_paths::app_data_dir(app)?.join(IMAGE_GEN_STORAGE_DIR_NAME)),
    }
}

pub(crate) fn storage_roots_from_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> AppResult<Vec<PathBuf>> {
    let settings = crate::settings::read(app)?;
    let current = storage_dir_from_settings(app)?;
    let mut roots = settings
        .image_gen_storage_roots
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    roots.push(current);
    canonical_storage_roots(&roots)
}

fn validate_task_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("SEC_INVALID_INPUT: task id is required".to_string());
    }
    if id.len() > MAX_TASK_ID_CHARS {
        return Err("SEC_INVALID_INPUT: task id is too long".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("SEC_INVALID_INPUT: task id contains invalid characters".to_string());
    }
    Ok(id)
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "bin",
    }
}

fn mime_for_ext(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

fn decode_payload_files(
    label: &str,
    files: &[ImageGenTaskFilePayload],
    total_bytes: &mut usize,
) -> Result<Vec<Vec<u8>>, String> {
    let mut decoded = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        // Cheap pre-check on the encoded length so oversized payloads are
        // rejected before allocating the decoded buffer.
        let estimated_bytes = file.data_b64.len() / 4 * 3;
        if total_bytes.saturating_add(estimated_bytes) > MAX_PERSIST_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: persist payload exceeds {MAX_PERSIST_TOTAL_BYTES} bytes limit"
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.data_b64.as_bytes())
            .map_err(|e| format!("SEC_INVALID_INPUT: {label} #{index} data_b64 is invalid: {e}"))?;
        *total_bytes = total_bytes.saturating_add(bytes.len());
        if *total_bytes > MAX_PERSIST_TOTAL_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: persist payload exceeds {MAX_PERSIST_TOTAL_BYTES} bytes limit"
            ));
        }
        decoded.push(bytes);
    }
    Ok(decoded)
}

fn write_task_file(dir: &ValidatedTaskDir, name: &str, bytes: &[u8]) -> Result<(), String> {
    #[cfg(test)]
    run_before_persist_file_create_test_hook();
    let mut file = create_validated_task_file(dir, name)?;
    file.write_all(bytes)
        .map_err(|e| format!("SYSTEM_ERROR: failed to write task file {name}: {e}"))
}

pub(crate) fn canonical_storage_root(storage_dir: &Path) -> AppResult<PathBuf> {
    validate_no_follow_components(storage_dir)?;
    let root = std::fs::canonicalize(storage_dir)
        .map_err(|_| "SEC_INVALID_INPUT: image gen storage root cannot be resolved".to_string())?;
    validate_no_follow_components(&root)?;
    let metadata = std::fs::metadata(&root)
        .map_err(|_| "SEC_INVALID_INPUT: image gen storage root cannot be inspected".to_string())?;
    if !metadata.is_dir() {
        return Err(
            "SEC_INVALID_INPUT: image gen storage root is not a directory"
                .to_string()
                .into(),
        );
    }
    Ok(root)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct FileIdentity {
    volume: u64,
    file: u64,
}

#[cfg(unix)]
fn open_trusted_dir(path: &Path) -> AppResult<std::fs::File> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "SEC_INVALID_INPUT: trusted image gen directory cannot be opened".to_string())?;
    Ok(fd.into())
}

#[cfg(unix)]
fn open_trusted_task_dir(path: &Path) -> AppResult<std::fs::File> {
    open_trusted_dir(path)
}

#[cfg(windows)]
fn open_trusted_dir(path: &Path) -> AppResult<std::fs::File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "SEC_INVALID_INPUT: trusted image gen path cannot be inspected".to_string())?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(
            "SEC_INVALID_INPUT: trusted image gen path cannot contain a reparse point"
                .to_string()
                .into(),
        );
    }
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| {
            format!("SEC_INVALID_INPUT: trusted image gen directory cannot be opened: {e}").into()
        })
}

#[cfg(windows)]
fn open_trusted_task_dir(path: &Path) -> AppResult<std::fs::File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows_sys::Win32::Foundation::GENERIC_READ;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "SEC_INVALID_INPUT: trusted image gen task cannot be inspected".to_string())?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(
            "SEC_INVALID_INPUT: trusted image gen task cannot be a reparse point"
                .to_string()
                .into(),
        );
    }
    std::fs::OpenOptions::new()
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| {
            format!("SEC_INVALID_INPUT: trusted image gen task cannot be opened: {e}").into()
        })
}

#[cfg(windows)]
fn open_identity_dir(path: &Path) -> AppResult<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| {
            format!("SEC_INVALID_INPUT: trusted image gen identity cannot be opened: {e}").into()
        })
}

#[cfg(unix)]
fn open_validated_task_handle(path: &Path) -> AppResult<std::fs::File> {
    open_trusted_dir(path)
}

#[cfg(windows)]
fn open_validated_task_handle(path: &Path) -> AppResult<std::fs::File> {
    open_identity_dir(path)
}

#[cfg(unix)]
fn file_identity_from_handle(file: &std::fs::File) -> AppResult<FileIdentity> {
    let stat = rustix::fs::fstat(file).map_err(|_| {
        "SEC_INVALID_INPUT: trusted image gen path identity cannot be read".to_string()
    })?;
    Ok(FileIdentity {
        volume: stat.st_dev as u64,
        file: stat.st_ino as u64,
    })
}

#[cfg(windows)]
fn file_identity_from_handle(file: &std::fs::File) -> AppResult<FileIdentity> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) };
    if ok == 0 {
        return Err(
            "SEC_INVALID_INPUT: trusted image gen path identity cannot be read"
                .to_string()
                .into(),
        );
    }
    let info = unsafe { info.assume_init() };
    Ok(FileIdentity {
        volume: u64::from(info.dwVolumeSerialNumber),
        file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

fn file_identity(path: &Path) -> AppResult<FileIdentity> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "SEC_INVALID_INPUT: trusted image gen path cannot be inspected".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(
            "SEC_INVALID_INPUT: trusted image gen path cannot contain a link"
                .to_string()
                .into(),
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(
                "SEC_INVALID_INPUT: trusted image gen path cannot contain a reparse point"
                    .to_string()
                    .into(),
            );
        }
    }
    if metadata.is_dir() {
        #[cfg(unix)]
        let handle = open_trusted_dir(path)?;
        #[cfg(windows)]
        let handle = open_identity_dir(path)?;
        return file_identity_from_handle(&handle);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| "SEC_INVALID_INPUT: trusted image gen file cannot be opened".to_string())?;
    file_identity_from_handle(&file)
}

fn validate_no_follow_components(path: &Path) -> AppResult<()> {
    if !path.is_absolute() {
        return Err("SEC_INVALID_INPUT: trusted image gen path must be absolute"
            .to_string()
            .into());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                current.push(component.as_os_str());
            }
            std::path::Component::Normal(part) => {
                current.push(part);
                file_identity(&current)?;
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(
                    "SEC_INVALID_INPUT: trusted image gen path cannot contain traversal"
                        .to_string()
                        .into(),
                );
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct ValidatedTaskDir {
    path: PathBuf,
    root: PathBuf,
    root_identity: FileIdentity,
    task_identity: FileIdentity,
    root_handle: std::fs::File,
    task_handle: std::fs::File,
}

type ReferencedTaskPaths = Vec<(String, PathBuf, FileIdentity)>;
type ValidatedRawTask = (ImageGenTaskRow, ReferencedTaskPaths, ValidatedTaskDir);

impl ValidatedTaskDir {
    fn revalidate(&self) -> AppResult<()> {
        validate_no_follow_components(&self.root)?;
        validate_no_follow_components(&self.path)?;
        if file_identity(&self.root)? != self.root_identity
            || file_identity(&self.path)? != self.task_identity
            || file_identity_from_handle(&self.root_handle)? != self.root_identity
            || file_identity_from_handle(&self.task_handle)? != self.task_identity
            || self.path.parent() != Some(self.root.as_path())
        {
            return Err("SEC_INVALID_INPUT: image gen storage path identity changed"
                .to_string()
                .into());
        }
        Ok(())
    }
}

pub(crate) fn canonical_storage_roots(storage_roots: &[PathBuf]) -> AppResult<Vec<PathBuf>> {
    if storage_roots.len() > MAX_TRUSTED_STORAGE_ROOTS {
        return Err("SEC_INVALID_INPUT: too many image gen storage roots"
            .to_string()
            .into());
    }
    let mut canonical = Vec::with_capacity(storage_roots.len());
    for root in storage_roots {
        if !root.is_absolute() {
            return Err("SEC_INVALID_INPUT: image gen storage root must be absolute"
                .to_string()
                .into());
        }
        if !root.exists() {
            continue;
        }
        let root = canonical_storage_root(root)?;
        if !canonical.contains(&root) {
            canonical.push(root);
        }
    }
    if canonical.is_empty() {
        return Err("SEC_INVALID_INPUT: no trusted image gen storage roots"
            .to_string()
            .into());
    }
    Ok(canonical)
}

fn validate_task_dir(
    storage_roots: &[PathBuf],
    dir: &str,
    id: &str,
) -> AppResult<ValidatedTaskDir> {
    let id = validate_task_id(id)?;
    let original = PathBuf::from(dir);
    validate_no_follow_components(&original)?;
    let candidate = std::fs::canonicalize(&original)
        .map_err(|_| "SEC_INVALID_INPUT: image gen task dir cannot be resolved".to_string())?;
    validate_no_follow_components(&candidate)?;
    let root = storage_roots
        .iter()
        .find(|root| candidate.parent() == Some(root.as_path()))
        .cloned()
        .ok_or_else(|| {
            crate::shared::error::AppError::from(
                "SEC_INVALID_INPUT: image gen task dir is outside the trusted storage root"
                    .to_string(),
            )
        })?;
    if candidate.file_name().and_then(|value| value.to_str()) != Some(id) || !candidate.is_dir() {
        return Err(
            "SEC_INVALID_INPUT: image gen task dir is outside the trusted storage root"
                .to_string()
                .into(),
        );
    }
    let validated = ValidatedTaskDir {
        root_handle: open_trusted_dir(&root)?,
        task_handle: open_validated_task_handle(&candidate)?,
        root_identity: file_identity(&root)?,
        task_identity: file_identity(&candidate)?,
        root,
        path: candidate,
    };
    validated.revalidate()?;
    Ok(validated)
}

fn remove_validated_task_dir(validated: &ValidatedTaskDir) -> AppResult<()> {
    validated.revalidate()?;
    #[cfg(test)]
    run_before_quarantine_test_hook();
    let delete_handle = acquire_delete_task_handle(validated)?;
    let quarantine_name = unique_quarantine_name(&validated.root)?;
    rename_task_to_quarantine(validated, &delete_handle, &quarantine_name)?;
    let quarantine = validated.root.join(&quarantine_name);
    validate_no_follow_components(&quarantine)?;
    if file_identity(&quarantine)? != validated.task_identity {
        return Err("SEC_INVALID_INPUT: image gen quarantine identity mismatch"
            .to_string()
            .into());
    }
    #[cfg(test)]
    run_after_quarantine_validation_test_hook();
    if file_identity(&quarantine)? != validated.task_identity
        || file_identity_from_handle(&delete_handle)? != validated.task_identity
    {
        return Err("SEC_INVALID_INPUT: image gen quarantine identity changed"
            .to_string()
            .into());
    }
    remove_quarantined_task(validated, &delete_handle, &quarantine_name, &quarantine)
}

#[cfg(unix)]
fn acquire_delete_task_handle(validated: &ValidatedTaskDir) -> AppResult<std::fs::File> {
    validated
        .task_handle
        .try_clone()
        .map_err(|e| format!("SYSTEM_ERROR: failed to clone task handle: {e}").into())
}

#[cfg(windows)]
fn acquire_delete_task_handle(validated: &ValidatedTaskDir) -> AppResult<std::fs::File> {
    let handle = open_trusted_task_dir(&validated.path)?;
    if file_identity_from_handle(&handle)? != validated.task_identity {
        return Err("SEC_INVALID_INPUT: image gen task identity changed"
            .to_string()
            .into());
    }
    Ok(handle)
}

#[cfg(test)]
type BeforeQuarantineHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
thread_local! {
    static BEFORE_QUARANTINE_TEST_HOOK: std::cell::RefCell<Option<BeforeQuarantineHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_before_quarantine_test_hook(hook: BeforeQuarantineHook) {
    BEFORE_QUARANTINE_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_before_quarantine_test_hook() {
    let hook = BEFORE_QUARANTINE_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
type AfterQuarantineValidationHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
thread_local! {
    static AFTER_QUARANTINE_VALIDATION_TEST_HOOK: std::cell::RefCell<Option<AfterQuarantineValidationHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_after_quarantine_validation_test_hook(hook: AfterQuarantineValidationHook) {
    AFTER_QUARANTINE_VALIDATION_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_after_quarantine_validation_test_hook() {
    let hook = AFTER_QUARANTINE_VALIDATION_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn unique_quarantine_name(root: &Path) -> AppResult<String> {
    use rand::RngCore as _;
    for _ in 0..32 {
        let mut random = [0_u8; 16];
        rand::thread_rng().fill_bytes(&mut random);
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let name = format!(".aio-quarantine-{suffix}");
        if std::fs::symlink_metadata(root.join(&name)).is_err() {
            return Ok(name);
        }
    }
    Err("SYSTEM_ERROR: failed to allocate image gen quarantine name"
        .to_string()
        .into())
}

#[cfg(unix)]
fn rename_task_to_quarantine(
    validated: &ValidatedTaskDir,
    _delete_handle: &std::fs::File,
    quarantine: &str,
) -> AppResult<()> {
    let id = validated
        .path
        .file_name()
        .ok_or_else(|| "SEC_INVALID_INPUT: invalid image gen task dir".to_string())?;
    rustix::fs::renameat_with(
        &validated.root_handle,
        id,
        &validated.root_handle,
        quarantine,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| format!("SYSTEM_ERROR: failed to quarantine task dir: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn rename_task_to_quarantine(
    validated: &ValidatedTaskDir,
    delete_handle: &std::fs::File,
    quarantine: &str,
) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
    };

    if file_identity_from_handle(delete_handle)? != validated.task_identity {
        return Err("SEC_INVALID_INPUT: image gen task identity changed"
            .to_string()
            .into());
    }
    let quarantine_path = validated.root.join(quarantine);
    let name = quarantine_path
        .as_os_str()
        .encode_wide()
        .collect::<Vec<_>>();
    let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let byte_len = header + name.len() * std::mem::size_of::<u16>();
    let mut buffer = vec![0_u64; byte_len.div_ceil(std::mem::size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = 0;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = (name.len() * std::mem::size_of::<u16>()) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr().cast::<u8>(),
            (*info).FileName.as_mut_ptr().cast::<u8>(),
            name.len() * std::mem::size_of::<u16>(),
        );
    }
    let ok = unsafe {
        SetFileInformationByHandle(
            delete_handle.as_raw_handle() as _,
            FileRenameInfo,
            buffer.as_ptr().cast(),
            byte_len as u32,
        )
    };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        return Err(
            format!("SYSTEM_ERROR: failed to quarantine task dir: os error {error}").into(),
        );
    }
    Ok(())
}

#[cfg(unix)]
fn remove_quarantined_task(
    validated: &ValidatedTaskDir,
    delete_handle: &std::fs::File,
    quarantine_name: &str,
    _quarantine_path: &Path,
) -> AppResult<()> {
    remove_dir_contents_at(delete_handle)?;
    rustix::fs::unlinkat(
        &validated.root_handle,
        quarantine_name,
        rustix::fs::AtFlags::REMOVEDIR,
    )
    .map_err(|e| format!("SYSTEM_ERROR: failed to remove quarantined task dir: {e}"))?;
    Ok(())
}

#[cfg(unix)]
fn remove_dir_contents_at(dir: &std::fs::File) -> AppResult<()> {
    let mut entries = rustix::fs::Dir::read_from(dir)
        .map_err(|e| format!("SYSTEM_ERROR: failed to read quarantined task dir: {e}"))?;
    while let Some(entry) = entries.next() {
        let entry = entry
            .map_err(|e| format!("SYSTEM_ERROR: failed to read quarantined task entry: {e}"))?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let stat = rustix::fs::statat(dir, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|e| format!("SYSTEM_ERROR: failed to inspect quarantined task entry: {e}"))?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Directory {
            let child = rustix::fs::openat(
                dir,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|e| format!("SYSTEM_ERROR: failed to open quarantined task entry: {e}"))?;
            let child: std::fs::File = child.into();
            remove_dir_contents_at(&child)?;
            rustix::fs::unlinkat(dir, name, rustix::fs::AtFlags::REMOVEDIR).map_err(|e| {
                format!("SYSTEM_ERROR: failed to remove quarantined directory: {e}")
            })?;
        } else {
            rustix::fs::unlinkat(dir, name, rustix::fs::AtFlags::empty())
                .map_err(|e| format!("SYSTEM_ERROR: failed to remove quarantined file: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn remove_quarantined_task(
    _validated: &ValidatedTaskDir,
    delete_handle: &std::fs::File,
    _quarantine_name: &str,
    _quarantine_path: &Path,
) -> AppResult<()> {
    remove_windows_dir_contents(delete_handle)?;
    delete_windows_handle(delete_handle)
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsDirectoryEntry {
    name: Vec<u16>,
    is_directory: bool,
    file_id: u64,
}

#[cfg(windows)]
fn windows_directory_entries(dir: &std::fs::File) -> AppResult<Vec<WindowsDirectoryEntry>> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES};
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdBothDirectoryInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ID_BOTH_DIR_INFO,
    };

    let mut entries = Vec::new();
    loop {
        let mut buffer = vec![0_u64; (64 * 1024) / std::mem::size_of::<u64>()];
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle() as _,
                FileIdBothDirectoryInfo,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * std::mem::size_of::<u64>()) as u32,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(format!(
                "SYSTEM_ERROR: failed to enumerate quarantined task handle: os error {error}"
            )
            .into());
        }

        let mut offset = 0usize;
        loop {
            let info = unsafe {
                &*buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>()
            };
            let name_len = info.FileNameLength as usize / std::mem::size_of::<u16>();
            let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
            if name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] {
                entries.push(WindowsDirectoryEntry {
                    name: name.to_vec(),
                    is_directory: info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
                    file_id: info.FileId as u64,
                });
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset = offset
                .checked_add(info.NextEntryOffset as usize)
                .ok_or_else(|| "SYSTEM_ERROR: invalid quarantined task entry offset".to_string())?;
            if offset >= buffer.len() * std::mem::size_of::<u64>() {
                return Err("SYSTEM_ERROR: invalid quarantined task entry buffer"
                    .to_string()
                    .into());
            }
        }
    }
    Ok(entries)
}

#[cfg(windows)]
fn open_windows_child_no_follow(
    parent: &std::fs::File,
    name: &mut [u16],
    is_directory: bool,
) -> AppResult<std::fs::File> {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        NtCreateFile, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let byte_len = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "SEC_INVALID_INPUT: quarantined task entry name is too long".to_string())?;
    let unicode = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle() as _,
        ObjectName: &unicode,
        Attributes: 0,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut io_status = std::mem::MaybeUninit::<IO_STATUS_BLOCK>::zeroed();
    let mut handle: HANDLE = std::ptr::null_mut();
    let desired_access = DELETE
        | FILE_READ_ATTRIBUTES
        | SYNCHRONIZE
        | if is_directory { FILE_LIST_DIRECTORY } else { 0 };
    let create_options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if is_directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &attributes,
            io_status.as_mut_ptr(),
            std::ptr::null(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            create_options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle.is_null() {
        return Err(format!(
            "SYSTEM_ERROR: failed to open quarantined task entry relative to trusted handle: ntstatus {status:#x}"
        )
        .into());
    }
    Ok(unsafe { std::fs::File::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn open_windows_file_relative(
    parent: &std::fs::File,
    name: &str,
    create: bool,
) -> AppResult<std::fs::File> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        NtCreateFile, FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let mut name = std::ffi::OsStr::new(name).encode_wide().collect::<Vec<_>>();
    let byte_len = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| "SEC_INVALID_INPUT: stored image filename is too long".to_string())?;
    let unicode = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle() as _,
        ObjectName: &unicode,
        Attributes: 0,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut io_status = std::mem::MaybeUninit::<IO_STATUS_BLOCK>::zeroed();
    let mut handle: HANDLE = std::ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            if create {
                GENERIC_WRITE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
            } else {
                GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE
            },
            &attributes,
            io_status.as_mut_ptr(),
            std::ptr::null(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            if create { FILE_CREATE } else { FILE_OPEN },
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle.is_null() {
        return Err(format!(
            "SEC_INVALID_INPUT: stored image file cannot be {} relative to its trusted task handle: ntstatus {status:#x}",
            if create { "created" } else { "opened" }
        )
        .into());
    }
    Ok(unsafe { std::fs::File::from_raw_handle(handle as _) })
}

#[cfg(unix)]
fn create_validated_task_file(
    validated: &ValidatedTaskDir,
    filename: &str,
) -> Result<std::fs::File, String> {
    let fd = rustix::fs::openat(
        &validated.task_handle,
        filename,
        rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .map_err(|e| format!("SYSTEM_ERROR: failed to create task file {filename}: {e}"))?;
    Ok(fd.into())
}

#[cfg(windows)]
fn create_validated_task_file(
    validated: &ValidatedTaskDir,
    filename: &str,
) -> Result<std::fs::File, String> {
    open_windows_file_relative(&validated.task_handle, filename, true).map_err(|e| e.to_string())
}

#[cfg(test)]
type BeforePersistFileCreateHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
thread_local! {
    static BEFORE_PERSIST_FILE_CREATE_TEST_HOOK: std::cell::RefCell<Option<BeforePersistFileCreateHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_before_persist_file_create_test_hook(hook: BeforePersistFileCreateHook) {
    BEFORE_PERSIST_FILE_CREATE_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_before_persist_file_create_test_hook() {
    let hook = BEFORE_PERSIST_FILE_CREATE_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(windows)]
fn windows_handle_attributes(file: &std::fs::File) -> AppResult<u32> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) };
    if ok == 0 {
        return Err(
            "SYSTEM_ERROR: failed to inspect quarantined task entry handle"
                .to_string()
                .into(),
        );
    }
    Ok(unsafe { info.assume_init() }.dwFileAttributes)
}

#[cfg(windows)]
fn remove_windows_dir_contents(dir: &std::fs::File) -> AppResult<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let entries = windows_directory_entries(dir)?;
    #[cfg(test)]
    run_after_windows_directory_enumeration_test_hook();
    for mut entry in entries {
        let child = open_windows_child_no_follow(dir, &mut entry.name, entry.is_directory)?;
        // A same-name object can replace an enumerated child before relative open.
        // Bind recursion/deletion to the exact enumerated file ID, not its name.
        if file_identity_from_handle(&child)?.file != entry.file_id {
            return Err("SEC_INVALID_INPUT: quarantined task entry identity changed"
                .to_string()
                .into());
        }
        let attributes = windows_handle_attributes(&child)?;
        if attributes & FILE_ATTRIBUTE_DIRECTORY != 0
            && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
        {
            remove_windows_dir_contents(&child)?;
        }
        delete_windows_handle(&child)?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
type AfterWindowsDirectoryEnumerationHook = Box<dyn FnOnce() + Send>;

#[cfg(all(test, windows))]
thread_local! {
    static AFTER_WINDOWS_DIRECTORY_ENUMERATION_TEST_HOOK: std::cell::RefCell<Option<AfterWindowsDirectoryEnumerationHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, windows))]
pub(super) fn set_after_windows_directory_enumeration_test_hook(
    hook: AfterWindowsDirectoryEnumerationHook,
) {
    AFTER_WINDOWS_DIRECTORY_ENUMERATION_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(all(test, windows))]
fn run_after_windows_directory_enumeration_test_hook() {
    let hook =
        AFTER_WINDOWS_DIRECTORY_ENUMERATION_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(windows)]
fn delete_windows_handle(file: &std::fs::File) -> AppResult<()> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfoEx, SetFileInformationByHandle, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO_EX,
    };

    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as _,
            FileDispositionInfoEx,
            (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "SYSTEM_ERROR: failed to delete quarantined task handle: os error {error}"
        )
        .into());
    }
    Ok(())
}

/// Writes task files to `{storage_dir}/{id}` then upserts the DB row.
/// Files are written first; if the DB write fails the task dir is removed
/// so no orphan directory survives a partial persist.
pub(crate) fn task_persist(
    db: &db::Db,
    storage_dir: &Path,
    payload: ImageGenTaskPersistPayload,
) -> AppResult<ImageGenTaskRow> {
    let id = validate_task_id(&payload.id)?.to_string();
    let adapter_id = payload
        .adapter_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("gpt-image")
        .to_string();
    if payload.status != "done" && payload.status != "error" {
        return Err("SEC_INVALID_INPUT: status must be 'done' or 'error'"
            .to_string()
            .into());
    }
    if payload.thumbs.len() > payload.images.len() {
        return Err("SEC_INVALID_INPUT: more thumbs than images"
            .to_string()
            .into());
    }

    let mut total_bytes = 0usize;
    let images = decode_payload_files("image", &payload.images, &mut total_bytes)?;
    let thumbs = decode_payload_files("thumb", &payload.thumbs, &mut total_bytes)?;
    let ref_images = decode_payload_files("ref image", &payload.ref_images, &mut total_bytes)?;

    ensure_writable_dir(storage_dir)?;
    let storage_root = canonical_storage_root(storage_dir)?;
    let task_dir = storage_root.join(&id);
    {
        let conn = db.open_connection()?;
        let already_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM image_gen_tasks WHERE id = ?1)",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .map_err(|e| db_err!("failed to check image gen task id: {e}"))?;
        if already_exists {
            return Err("SEC_INVALID_INPUT: image gen task id already exists"
                .to_string()
                .into());
        }
    }
    std::fs::create_dir(&task_dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            "SEC_INVALID_INPUT: image gen task directory already exists".to_string()
        } else {
            format!("SYSTEM_ERROR: failed to create task dir: {e}")
        }
    })?;
    let owned_task_dir = match validate_task_dir(
        std::slice::from_ref(&storage_root),
        &task_dir.to_string_lossy(),
        &id,
    ) {
        Ok(validated) => validated,
        Err(error) => {
            let _ = std::fs::remove_dir(&task_dir);
            return Err(error);
        }
    };

    let write_insert_and_validate = || -> AppResult<ImageGenTaskRow> {
        let mut stored_images = Vec::with_capacity(images.len());
        for (index, bytes) in images.iter().enumerate() {
            let mime = &payload.images[index].mime;
            let file = format!("image-{}.{}", index + 1, ext_for_mime(mime));
            write_task_file(&owned_task_dir, &file, bytes)?;
            let thumb = match thumbs.get(index) {
                Some(thumb_bytes) => {
                    let thumb_mime = &payload.thumbs[index].mime;
                    let thumb_file = format!("thumb-{}.{}", index + 1, ext_for_mime(thumb_mime));
                    write_task_file(&owned_task_dir, &thumb_file, thumb_bytes)?;
                    Some(thumb_file)
                }
                None => None,
            };
            stored_images.push(StoredFile {
                file,
                thumb,
                mime: mime.clone(),
            });
        }

        let mut stored_refs = Vec::with_capacity(ref_images.len());
        for (index, bytes) in ref_images.iter().enumerate() {
            let mime = &payload.ref_images[index].mime;
            let file = format!("ref-{}.{}", index + 1, ext_for_mime(mime));
            write_task_file(&owned_task_dir, &file, bytes)?;
            stored_refs.push(StoredFile {
                file,
                thumb: None,
                mime: mime.clone(),
            });
        }

        let images_json = serde_json::to_string(&stored_images)
            .map_err(|e| format!("SYSTEM_ERROR: failed to serialize images_json: {e}"))?;
        let ref_images_json = serde_json::to_string(&stored_refs)
            .map_err(|e| format!("SYSTEM_ERROR: failed to serialize ref_images_json: {e}"))?;

        #[cfg(test)]
        maybe_inject_persist_failure(PersistFailurePoint::BeforeInsert, &task_dir)?;

        let mut conn = db.open_connection()?;
        let transaction = conn
            .transaction()
            .map_err(|e| db_err!("failed to begin image gen task transaction: {e}"))?;
        transaction
            .execute(
                r#"
INSERT INTO image_gen_tasks(
  id, adapter_id, prompt, request_json, status, error, usage_json,
  images_json, ref_images_json, dir, created_at, elapsed_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
"#,
                rusqlite::params![
                    id,
                    adapter_id,
                    payload.prompt,
                    payload.request_json,
                    payload.status,
                    payload.error,
                    payload.usage_json,
                    images_json,
                    ref_images_json,
                    task_dir.to_string_lossy().to_string(),
                    payload.created_at,
                    payload.elapsed_ms,
                ],
            )
            .map_err(|e| db_err!("failed to upsert image gen task: {e}"))?;
        #[cfg(test)]
        maybe_inject_persist_failure(PersistFailurePoint::PostInsertValidation, &task_dir)?;
        let raw = transaction
            .query_row(
                &format!("SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks WHERE id = ?1"),
                rusqlite::params![id],
                row_to_raw_task,
            )
            .optional()
            .map_err(|e| db_err!("failed to read persisted image gen task: {e}"))?
            .ok_or_else(|| {
                crate::shared::error::AppError::from(
                    "DB_ERROR: persisted image gen task not found".to_string(),
                )
            })?;
        let row = validate_raw_task(std::slice::from_ref(&storage_root), raw)?.0;
        transaction
            .commit()
            .map_err(|e| db_err!("failed to commit image gen task: {e}"))?;
        Ok(row)
    };

    match write_insert_and_validate() {
        Ok(row) => Ok(row),
        Err(error) => {
            let _ = remove_validated_task_dir(&owned_task_dir);
            Err(error)
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PersistFailurePoint {
    BeforeInsert,
    PostInsertValidation,
}

#[cfg(test)]
thread_local! {
    static PERSIST_FAILURE_POINT: std::cell::Cell<Option<PersistFailurePoint>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn set_persist_failure_point(point: PersistFailurePoint) {
    PERSIST_FAILURE_POINT.set(Some(point));
}

#[cfg(test)]
fn maybe_inject_persist_failure(point: PersistFailurePoint, task_dir: &Path) -> AppResult<()> {
    let should_inject = PERSIST_FAILURE_POINT.with(|current| {
        if current.get() == Some(point) {
            current.set(None);
            true
        } else {
            false
        }
    });
    if !should_inject {
        return Ok(());
    }
    if point == PersistFailurePoint::PostInsertValidation {
        std::fs::remove_file(task_dir.join("image-1.png"))
            .map_err(|e| format!("SYSTEM_ERROR: failed to inject post-insert validation: {e}"))?;
        return Ok(());
    }
    Err("DB_ERROR: injected before-insert failure"
        .to_string()
        .into())
}

struct RawTaskRow {
    id: String,
    adapter_id: String,
    prompt: String,
    request_json: String,
    status: String,
    error: Option<String>,
    usage_json: Option<String>,
    images_json: String,
    ref_images_json: String,
    dir: String,
    created_at: i64,
    elapsed_ms: Option<i64>,
}

fn row_to_raw_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawTaskRow> {
    Ok(RawTaskRow {
        id: row.get("id")?,
        adapter_id: row.get("adapter_id")?,
        prompt: row.get("prompt")?,
        request_json: row.get("request_json")?,
        status: row.get("status")?,
        error: row.get("error")?,
        usage_json: row.get("usage_json")?,
        images_json: row.get("images_json")?,
        ref_images_json: row.get("ref_images_json")?,
        dir: row.get("dir")?,
        created_at: row.get("created_at")?,
        elapsed_ms: row.get("elapsed_ms")?,
    })
}

fn validate_stored_file(
    task_dir: &Path,
    task_id: &str,
    stored: StoredFile,
    referenced_paths: &mut ReferencedTaskPaths,
) -> AppResult<ImageGenTaskFileRow> {
    let file = safe_stored_file_name(&stored.file)
        .ok_or_else(|| "SEC_INVALID_INPUT: unsafe stored image filename".to_string())?;
    let (path, identity) = validate_stored_file_path(task_dir, file)?;
    let reference = format!("{task_id}/{file}");
    referenced_paths.push((reference.clone(), path, identity));

    let thumb_path = if let Some(thumb) = stored.thumb {
        let thumb = safe_stored_file_name(&thumb)
            .ok_or_else(|| "SEC_INVALID_INPUT: unsafe stored thumbnail filename".to_string())?;
        let (path, identity) = validate_stored_file_path(task_dir, thumb)?;
        let reference = format!("{task_id}/{thumb}");
        referenced_paths.push((reference.clone(), path, identity));
        Some(reference)
    } else {
        None
    };
    Ok(ImageGenTaskFileRow {
        path: reference,
        thumb_path,
        mime: stored.mime,
    })
}

fn validate_stored_file_path(
    task_dir: &Path,
    filename: &str,
) -> AppResult<(PathBuf, FileIdentity)> {
    let candidate = task_dir.join(filename);
    validate_no_follow_components(&candidate)?;
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be resolved".to_string())?;
    validate_no_follow_components(&canonical)?;
    if canonical.parent() != Some(task_dir) || !canonical.is_file() {
        return Err(
            "SEC_INVALID_INPUT: stored image file is outside its task directory"
                .to_string()
                .into(),
        );
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .open(&canonical)
        .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be opened".to_string())?;
    ensure_single_link_file(&file)?;
    let identity = file_identity_from_handle(&file)?;
    Ok((canonical, identity))
}

fn validate_raw_task(storage_roots: &[PathBuf], raw: RawTaskRow) -> AppResult<ValidatedRawTask> {
    let id = validate_task_id(&raw.id)?.to_string();
    let task_dir = validate_task_dir(storage_roots, &raw.dir, &id)?;
    let images = serde_json::from_str::<Vec<StoredFile>>(&raw.images_json)
        .map_err(|_| "SEC_INVALID_INPUT: invalid stored image metadata".to_string())?;
    let ref_images = serde_json::from_str::<Vec<StoredFile>>(&raw.ref_images_json)
        .map_err(|_| "SEC_INVALID_INPUT: invalid stored reference metadata".to_string())?;
    let mut referenced_paths = Vec::new();
    let images = images
        .into_iter()
        .map(|stored| validate_stored_file(&task_dir.path, &id, stored, &mut referenced_paths))
        .collect::<AppResult<Vec<_>>>()?;
    let ref_images = ref_images
        .into_iter()
        .map(|stored| validate_stored_file(&task_dir.path, &id, stored, &mut referenced_paths))
        .collect::<AppResult<Vec<_>>>()?;
    Ok((
        ImageGenTaskRow {
            id: id.clone(),
            adapter_id: raw.adapter_id,
            prompt: raw.prompt,
            request_json: raw.request_json,
            status: raw.status,
            error: raw.error,
            usage_json: raw.usage_json,
            images,
            ref_images,
            dir: id,
            created_at: raw.created_at,
            elapsed_ms: raw.elapsed_ms,
        },
        referenced_paths,
        task_dir,
    ))
}

const TASK_SELECT_COLUMNS: &str = "id, adapter_id, prompt, request_json, status, error, usage_json, images_json, ref_images_json, dir, created_at, elapsed_ms";

fn decode_tasks_cursor(cursor: Option<&str>) -> AppResult<Option<ImageGenTasksCursor>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let cursor = cursor.trim();
    if cursor.is_empty() || cursor.len() > MAX_CURSOR_BYTES {
        return Err("SEC_INVALID_INPUT: invalid image gen tasks cursor"
            .to_string()
            .into());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor.as_bytes())
        .map_err(|_| "SEC_INVALID_INPUT: invalid image gen tasks cursor".to_string())?;
    if bytes.len() > MAX_CURSOR_BYTES {
        return Err("SEC_INVALID_INPUT: invalid image gen tasks cursor"
            .to_string()
            .into());
    }
    let decoded: ImageGenTasksCursor = serde_json::from_slice(&bytes)
        .map_err(|_| "SEC_INVALID_INPUT: invalid image gen tasks cursor".to_string())?;
    if decoded.v != CURSOR_VERSION {
        return Err(
            "SEC_INVALID_INPUT: unsupported image gen tasks cursor version"
                .to_string()
                .into(),
        );
    }
    validate_task_id(&decoded.id)?;
    Ok(Some(decoded))
}

fn encode_tasks_cursor(row: &ImageGenTaskRow) -> AppResult<String> {
    let bytes = serde_json::to_vec(&ImageGenTasksCursor {
        v: CURSOR_VERSION,
        created_at: row.created_at,
        id: row.id.clone(),
    })
    .map_err(|e| format!("SYSTEM_ERROR: failed to encode image gen tasks cursor: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Newest-first page. `before_created_at = None` starts from the newest row.
#[cfg(test)]
pub(crate) fn tasks_list(
    db: &db::Db,
    storage_root: &Path,
    before_created_at: Option<i64>,
    limit: u32,
) -> AppResult<Vec<ImageGenTaskRow>> {
    tasks_list_with_roots(db, &[storage_root.to_path_buf()], before_created_at, limit)
}

#[cfg(test)]
pub(crate) fn tasks_list_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    before_created_at: Option<i64>,
    limit: u32,
) -> AppResult<Vec<ImageGenTaskRow>> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare(&format!(
            r#"
SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks
WHERE (?1 IS NULL OR created_at < ?1)
ORDER BY created_at DESC, id DESC
LIMIT ?2
"#
        ))
        .map_err(|e| db_err!("failed to prepare image gen tasks query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![before_created_at, limit], row_to_raw_task)
        .map_err(|e| db_err!("failed to query image gen tasks: {e}"))?;

    let mut tasks = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| db_err!("failed to read image gen task row: {e}"))?;
        tasks.push(validate_raw_task(&storage_roots, raw)?.0);
    }
    Ok(tasks)
}

pub(crate) fn tasks_page_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    cursor: Option<&str>,
    limit: u32,
) -> AppResult<ImageGenTasksPage> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let cursor = decode_tasks_cursor(cursor)?;
    let before_created_at = cursor.as_ref().map(|cursor| cursor.created_at);
    let before_id = cursor.as_ref().map(|cursor| cursor.id.as_str());
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let query_limit = i64::from(limit) + 1;
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare(&format!(
            r#"
SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks
WHERE (
  ?1 IS NULL
  OR created_at < ?1
  OR (created_at = ?1 AND id < ?2)
)
ORDER BY created_at DESC, id DESC
LIMIT ?3
"#
        ))
        .map_err(|e| db_err!("failed to prepare image gen tasks page query: {e}"))?;
    let rows = stmt
        .query_map(
            rusqlite::params![before_created_at, before_id, query_limit],
            row_to_raw_task,
        )
        .map_err(|e| db_err!("failed to query image gen tasks page: {e}"))?;

    let mut items = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| db_err!("failed to read image gen task page row: {e}"))?;
        items.push(validate_raw_task(&storage_roots, raw)?.0);
    }
    let has_more = items.len() > limit as usize;
    if has_more {
        items.pop();
    }
    let next_cursor = if has_more {
        items.last().map(encode_tasks_cursor).transpose()?
    } else {
        None
    };
    Ok(ImageGenTasksPage { items, next_cursor })
}

/// Deletes the DB row and the task directory. Missing row / missing dir are
/// both tolerated (idempotent delete).
#[cfg(test)]
pub(crate) fn task_delete(db: &db::Db, storage_root: &Path, id: &str) -> AppResult<()> {
    task_delete_with_roots(db, &[storage_root.to_path_buf()], id)
}

pub(crate) fn task_delete_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    id: &str,
) -> AppResult<()> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let id = validate_task_id(id)?;
    let conn = db.open_connection()?;
    let dir: Option<String> = conn
        .query_row(
            "SELECT dir FROM image_gen_tasks WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| db_err!("failed to query image gen task dir: {e}"))?;

    let Some(dir) = dir else {
        return Ok(());
    };

    let validated = validate_task_dir(&storage_roots, &dir, id)?;
    remove_validated_task_dir(&validated)?;
    conn.execute(
        "DELETE FROM image_gen_tasks WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| db_err!("failed to delete image gen task: {e}"))?;
    Ok(())
}

/// Deletes every task row and its directory. Returns the number of deleted rows.
#[cfg(test)]
pub(crate) fn tasks_clear(db: &db::Db, storage_root: &Path) -> AppResult<u32> {
    tasks_clear_with_roots(db, &[storage_root.to_path_buf()])
}

pub(crate) fn tasks_clear_with_roots(db: &db::Db, storage_roots: &[PathBuf]) -> AppResult<u32> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        "SELECT id, dir FROM image_gen_tasks",
        rusqlite::params![],
    )?;

    let validated = pairs
        .iter()
        .map(|(id, dir)| validate_task_dir(&storage_roots, dir, id))
        .collect::<AppResult<Vec<_>>>()?;
    for path in &validated {
        remove_validated_task_dir(path)?;
    }
    conn.execute("DELETE FROM image_gen_tasks", [])
        .map_err(|e| db_err!("failed to clear image gen tasks: {e}"))?;
    Ok(pairs.len() as u32)
}

/// Keeps the most recent `keep_count` tasks and deletes the rest (rows + dirs).
/// Returns the number of deleted tasks.
#[cfg(test)]
pub(crate) fn storage_cleanup(db: &db::Db, storage_root: &Path, keep_count: u32) -> AppResult<u32> {
    storage_cleanup_with_roots(db, &[storage_root.to_path_buf()], keep_count)
}

pub(crate) fn storage_cleanup_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    keep_count: u32,
) -> AppResult<u32> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        r#"
SELECT id, dir FROM image_gen_tasks
ORDER BY created_at DESC, id DESC
LIMIT -1 OFFSET ?1
"#,
        rusqlite::params![keep_count],
    )?;

    let validated = pairs
        .iter()
        .map(|(id, dir)| validate_task_dir(&storage_roots, dir, id))
        .collect::<AppResult<Vec<_>>>()?;
    for ((id, _), path) in pairs.iter().zip(validated.iter()) {
        remove_validated_task_dir(path)?;
        conn.execute(
            "DELETE FROM image_gen_tasks WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| db_err!("failed to delete image gen task during cleanup: {e}"))?;
    }
    Ok(pairs.len() as u32)
}

fn query_id_dir_pairs(
    conn: &rusqlite::Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| db_err!("failed to prepare image gen task id/dir query: {e}"))?;
    let rows = stmt
        .query_map(params, |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| db_err!("failed to query image gen task id/dir pairs: {e}"))?;
    let mut pairs = Vec::new();
    for row in rows {
        pairs.push(row.map_err(|e| db_err!("failed to read image gen task id/dir row: {e}"))?);
    }
    Ok(pairs)
}

fn safe_stored_file_name(value: &str) -> Option<&str> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
        || path.file_name().and_then(|name| name.to_str()) != Some(value)
        || path.components().count() != 1
    {
        return None;
    }
    Some(value)
}

/// Reads a stored image back as base64. Security boundary: the canonicalized
/// path must live strictly inside the current storage dir or a DB-recorded task
/// dir (canonicalization also rejects `..` traversal and symlink escapes).
#[cfg(test)]
pub(crate) fn read_image(
    db: &db::Db,
    storage_dir: &Path,
    reference: &str,
) -> AppResult<ImageGenFetchedImage> {
    read_image_with_roots(db, &[storage_dir.to_path_buf()], reference)
}

pub(crate) fn read_image_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    reference: &str,
) -> AppResult<ImageGenFetchedImage> {
    let mut images = read_images_with_budget_with_roots(
        db,
        storage_roots,
        std::slice::from_ref(&reference.to_string()),
        MAX_READ_IMAGE_BYTES,
        MAX_READ_IMAGE_BYTES,
    )?;
    images.pop().ok_or_else(|| {
        "SYSTEM_ERROR: image read returned no result"
            .to_string()
            .into()
    })
}

pub(crate) fn read_images_with_budget_with_roots(
    db: &db::Db,
    storage_roots: &[PathBuf],
    references: &[String],
    per_image_max: u64,
    aggregate_max: u64,
) -> AppResult<Vec<ImageGenFetchedImage>> {
    if references.len() > MAX_HYDRATE_REFERENCES || per_image_max == 0 || aggregate_max == 0 {
        return Err("SEC_INVALID_INPUT: invalid image history hydration budget"
            .to_string()
            .into());
    }
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let mut used = 0_u64;
    let mut output = Vec::with_capacity(references.len());
    for reference in references {
        let (mime, mut file, len) = open_image_reference(db, &storage_roots, reference)?;
        let next = used.checked_add(len).ok_or_else(|| {
            crate::shared::error::AppError::from(
                "SEC_INVALID_INPUT: image history hydration budget exceeded".to_string(),
            )
        })?;
        if len > per_image_max || next > aggregate_max {
            return Err("SEC_INVALID_INPUT: image history hydration budget exceeded"
                .to_string()
                .into());
        }
        used = next;
        #[cfg(test)]
        run_before_history_file_read_test_hook();
        let mut bytes = Vec::with_capacity(len as usize);
        std::io::Read::take(&mut file, len.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| format!("SYSTEM_ERROR: failed to read image file: {e}"))?;
        if bytes.len() as u64 > len {
            return Err(
                "SEC_INVALID_INPUT: stored image file size changed during read"
                    .to_string()
                    .into(),
            );
        }
        output.push(ImageGenFetchedImage {
            mime,
            data_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
    }
    Ok(output)
}

fn open_image_reference(
    db: &db::Db,
    storage_roots: &[PathBuf],
    reference: &str,
) -> AppResult<(String, std::fs::File, u64)> {
    let reference = reference.trim();
    let (task_id, filename) = reference
        .split_once('/')
        .ok_or_else(|| "SEC_INVALID_INPUT: invalid image reference".to_string())?;
    validate_task_id(task_id)?;
    safe_stored_file_name(filename)
        .ok_or_else(|| "SEC_INVALID_INPUT: invalid image reference".to_string())?;
    let conn = db.open_connection()?;
    let raw = conn
        .query_row(
            &format!("SELECT {TASK_SELECT_COLUMNS} FROM image_gen_tasks WHERE id = ?1"),
            rusqlite::params![task_id],
            row_to_raw_task,
        )
        .optional()
        .map_err(|e| db_err!("failed to query image gen task: {e}"))?
        .ok_or_else(|| "SEC_INVALID_INPUT: image reference task was not found".to_string())?;
    let (_, referenced_paths, validated_task) = validate_raw_task(storage_roots, raw)?;
    let expected_identity = referenced_paths
        .into_iter()
        .find(|(candidate, _, _)| candidate == reference)
        .map(|(_, _, identity)| identity)
        .ok_or_else(|| {
            "SEC_INVALID_INPUT: image reference is not present in trusted metadata".to_string()
        })?;
    #[cfg(test)]
    run_before_history_read_open_test_hook();
    let file = open_validated_task_file(&validated_task, filename, expected_identity)?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("SYSTEM_ERROR: failed to stat image file: {e}"))?;
    if !metadata.is_file() {
        return Err("SEC_INVALID_INPUT: image path is not a file"
            .to_string()
            .into());
    }
    Ok((
        mime_for_ext(Path::new(filename)).to_string(),
        file,
        metadata.len(),
    ))
}

#[cfg(unix)]
fn open_validated_task_file(
    validated: &ValidatedTaskDir,
    filename: &str,
    expected_identity: FileIdentity,
) -> AppResult<std::fs::File> {
    validated.revalidate()?;
    let fd = rustix::fs::openat(
        &validated.task_handle,
        filename,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be opened".to_string())?;
    let file: std::fs::File = fd.into();
    ensure_single_link_file(&file)?;
    if file_identity_from_handle(&file)? != expected_identity {
        return Err("SEC_INVALID_INPUT: stored image file identity changed"
            .to_string()
            .into());
    }
    Ok(file)
}

#[cfg(windows)]
fn open_validated_task_file(
    validated: &ValidatedTaskDir,
    filename: &str,
    expected_identity: FileIdentity,
) -> AppResult<std::fs::File> {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let file = open_windows_file_relative(&validated.task_handle, filename, false)?;
    let metadata = file
        .metadata()
        .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be inspected".to_string())?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_file() {
        return Err(
            "SEC_INVALID_INPUT: stored image file cannot be a reparse point"
                .to_string()
                .into(),
        );
    }
    ensure_single_link_file(&file)?;
    if file_identity_from_handle(&file)? != expected_identity {
        return Err("SEC_INVALID_INPUT: stored image file identity changed"
            .to_string()
            .into());
    }
    Ok(file)
}

#[cfg(unix)]
fn ensure_single_link_file(file: &std::fs::File) -> AppResult<()> {
    let stat = rustix::fs::fstat(file)
        .map_err(|_| "SEC_INVALID_INPUT: stored image file cannot be inspected".to_string())?;
    if stat.st_nlink != 1 {
        return Err("SEC_INVALID_INPUT: stored image file cannot be a hard link"
            .to_string()
            .into());
    }
    Ok(())
}

#[cfg(windows)]
fn ensure_single_link_file(file: &std::fs::File) -> AppResult<()> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) };
    if ok == 0 || unsafe { info.assume_init() }.nNumberOfLinks != 1 {
        return Err("SEC_INVALID_INPUT: stored image file cannot be a hard link"
            .to_string()
            .into());
    }
    Ok(())
}

#[cfg(test)]
type BeforeHistoryReadOpenHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
thread_local! {
    static BEFORE_HISTORY_READ_OPEN_TEST_HOOK: std::cell::RefCell<Option<BeforeHistoryReadOpenHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_before_history_read_open_test_hook(hook: BeforeHistoryReadOpenHook) {
    BEFORE_HISTORY_READ_OPEN_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_before_history_read_open_test_hook() {
    let hook = BEFORE_HISTORY_READ_OPEN_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
type BeforeHistoryFileReadHook = Box<dyn FnMut() + Send>;

#[cfg(test)]
thread_local! {
    static BEFORE_HISTORY_FILE_READ_TEST_HOOK: std::cell::RefCell<Option<BeforeHistoryFileReadHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_before_history_file_read_test_hook(hook: BeforeHistoryFileReadHook) {
    BEFORE_HISTORY_FILE_READ_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_before_history_file_read_test_hook() {
    BEFORE_HISTORY_FILE_READ_TEST_HOOK.with(|current| {
        if let Some(hook) = current.borrow_mut().as_mut() {
            hook();
        }
    });
}

/// Storage usage view. Size is computed by walking DB-recorded task dirs.
// ponytail: orphan dirs (files written but row never landed) are not counted
// and cannot be reclaimed by cleanup; add an orphan scan if this ever matters.
#[cfg(test)]
pub(crate) fn storage_stats(db: &db::Db, storage_dir: &Path) -> AppResult<ImageGenStorageView> {
    storage_stats_with_roots(db, storage_dir, &[storage_dir.to_path_buf()])
}

pub(crate) fn storage_stats_with_roots(
    db: &db::Db,
    current_storage_dir: &Path,
    storage_roots: &[PathBuf],
) -> AppResult<ImageGenStorageView> {
    let storage_roots = canonical_storage_roots(storage_roots)?;
    let conn = db.open_connection()?;
    let task_count: u32 = conn
        .query_row("SELECT COUNT(*) FROM image_gen_tasks", [], |row| row.get(0))
        .map_err(|e| db_err!("failed to count image gen tasks: {e}"))?;
    drop(conn);

    let mut total_bytes: u64 = 0;
    #[cfg(test)]
    let mut total_entries: u64 = take_storage_stats_entry_count_seed();
    #[cfg(not(test))]
    let mut total_entries: u64 = 0;
    let mut visited = std::collections::HashSet::new();
    let max_entries = storage_stats_max_entries();
    #[cfg(test)]
    let byte_bias = take_storage_stats_byte_bias();
    #[cfg(not(test))]
    let byte_bias = 0;
    let conn = db.open_connection()?;
    let pairs = query_id_dir_pairs(
        &conn,
        "SELECT id, dir FROM image_gen_tasks",
        rusqlite::params![],
    )?;
    for (id, dir) in pairs {
        let dir = validate_task_dir(&storage_roots, &dir, &id)?;
        dir.revalidate()?;
        let task_bytes = dir_size_bytes_from_handle(
            &dir.task_handle,
            dir.task_identity,
            &mut visited,
            &mut total_entries,
            max_entries,
            byte_bias,
        )?;
        total_bytes = total_bytes.checked_add(task_bytes).ok_or_else(|| {
            "SEC_INVALID_INPUT: image gen storage size exceeds representable range".to_string()
        })?;
        if total_bytes > i64::MAX as u64 {
            return Err(
                "SEC_INVALID_INPUT: image gen storage size exceeds representable range"
                    .to_string()
                    .into(),
            );
        }
    }

    Ok(ImageGenStorageView {
        dir: current_storage_dir.to_string_lossy().to_string(),
        total_bytes: total_bytes as i64,
        task_count,
    })
}

const STORAGE_STATS_MAX_DEPTH: usize = 64;
const STORAGE_STATS_MAX_ENTRIES: u64 = 100_000;

#[cfg(test)]
thread_local! {
    static STORAGE_STATS_ENTRY_COUNT_SEED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Seed the real production entry counter for a one-shot boundary test. The
/// walker still enumerates and charges a real filesystem entry, which becomes
/// the 100001st entry without creating a large test tree.
#[cfg(test)]
pub(crate) fn set_storage_stats_entry_count_seed_for_test(seed: u64) {
    STORAGE_STATS_ENTRY_COUNT_SEED.with(|current| current.set(seed));
}

#[cfg(test)]
fn take_storage_stats_entry_count_seed() -> u64 {
    STORAGE_STATS_ENTRY_COUNT_SEED.with(|current| current.replace(0))
}

// Optional one-shot additive byte bias applied after each regular-file size is
// read from handle metadata. Used to force checked i64::MAX+1 fail-closed without
// allocating huge files.
#[cfg(test)]
thread_local! {
    static STORAGE_STATS_BYTE_BIAS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn set_storage_stats_byte_bias_for_test(bias: u64) {
    STORAGE_STATS_BYTE_BIAS.with(|current| current.set(bias));
}

#[cfg(test)]
fn take_storage_stats_byte_bias() -> u64 {
    STORAGE_STATS_BYTE_BIAS.with(|current| current.replace(0))
}

/// Fired after a stats entry is enumerated but before relative open, so tests
/// can replace the entry with a different identity (symlink/junction/file swap).
#[cfg(test)]
type BeforeStatsOpenHook = Box<dyn FnOnce(&str) + Send>;

#[cfg(test)]
thread_local! {
    static BEFORE_STATS_OPEN_TEST_HOOK: std::cell::RefCell<Option<BeforeStatsOpenHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_before_stats_open_test_hook(hook: BeforeStatsOpenHook) {
    BEFORE_STATS_OPEN_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_before_stats_open_test_hook(entry_name: &str) {
    let hook = BEFORE_STATS_OPEN_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook(entry_name);
    }
}

fn storage_stats_max_entries() -> u64 {
    STORAGE_STATS_MAX_ENTRIES
}

fn dir_size_bytes_from_handle(
    task_handle: &std::fs::File,
    task_identity: FileIdentity,
    visited: &mut std::collections::HashSet<FileIdentity>,
    total_entries: &mut u64,
    max_entries: u64,
    byte_bias: u64,
) -> AppResult<u64> {
    if !visited.insert(task_identity) {
        return Err("SEC_INVALID_INPUT: image gen storage directory identity cycle".into());
    }
    let mut stack = vec![(
        task_handle
            .try_clone()
            .map_err(|e| format!("SYSTEM_ERROR: failed to clone image gen task handle: {e}"))?,
        0usize,
    )];
    let mut total: u64 = 0;

    while let Some((dir_handle, depth)) = stack.pop() {
        if depth > STORAGE_STATS_MAX_DEPTH {
            return Err(format!(
                "SEC_INVALID_INPUT: image gen storage directory exceeds max depth {STORAGE_STATS_MAX_DEPTH}"
            )
            .into());
        }
        // Enumeration shares and enforces the global entry budget incrementally;
        // never collect an unbounded Vec first.
        let entries = list_stats_entries(&dir_handle, total_entries, max_entries)?;
        for entry in entries {
            match entry.kind {
                StatsEntryKind::Directory => {
                    #[cfg(test)]
                    run_before_stats_open_test_hook(&stats_entry_name(&entry));
                    let child = open_stats_child(&dir_handle, &entry, true)?;
                    let child_identity = file_identity_from_handle(&child)?;
                    if child_identity != entry.identity {
                        return Err(
                            "SEC_INVALID_INPUT: image gen storage directory identity changed"
                                .to_string()
                                .into(),
                        );
                    }
                    if !visited.insert(child_identity) {
                        return Err(
                            "SEC_INVALID_INPUT: image gen storage directory identity cycle"
                                .to_string()
                                .into(),
                        );
                    }
                    stack.push((child, depth + 1));
                }
                StatsEntryKind::File => {
                    #[cfg(test)]
                    run_before_stats_open_test_hook(&stats_entry_name(&entry));
                    let child = open_stats_child(&dir_handle, &entry, false)?;
                    let child_identity = file_identity_from_handle(&child)?;
                    if child_identity != entry.identity {
                        return Err("SEC_INVALID_INPUT: image gen storage file identity changed"
                            .to_string()
                            .into());
                    }
                    ensure_single_link_file(&child)?;
                    let metadata = child.metadata().map_err(|e| {
                        format!("SYSTEM_ERROR: failed to inspect image gen storage file: {e}")
                    })?;
                    if !metadata.is_file() {
                        return Err(
                            "SEC_INVALID_INPUT: image gen storage entry is not a regular file"
                                .to_string()
                                .into(),
                        );
                    }
                    let charged_size = metadata.len().checked_add(byte_bias).ok_or_else(|| {
                        "SEC_INVALID_INPUT: image gen storage size exceeds representable range"
                            .to_string()
                    })?;
                    total = total.checked_add(charged_size).ok_or_else(|| {
                        "SEC_INVALID_INPUT: image gen storage size exceeds representable range"
                            .to_string()
                    })?;
                    if total > i64::MAX as u64 {
                        return Err(
                            "SEC_INVALID_INPUT: image gen storage size exceeds representable range"
                                .to_string()
                                .into(),
                        );
                    }
                }
                StatsEntryKind::Rejected(reason) => {
                    return Err(format!("SEC_INVALID_INPUT: {reason}").into());
                }
            }
        }
    }

    Ok(total)
}

#[derive(Debug, Clone, Copy)]
enum StatsEntryKind {
    File,
    Directory,
    Rejected(&'static str),
}

#[derive(Debug)]
struct StatsEntry {
    #[cfg(unix)]
    name: std::ffi::OsString,
    #[cfg(windows)]
    name_utf16: Vec<u16>,
    identity: FileIdentity,
    kind: StatsEntryKind,
}

fn charge_stats_entry(total_entries: &mut u64, max_entries: u64) -> AppResult<()> {
    *total_entries = total_entries
        .checked_add(1)
        .ok_or_else(|| "SEC_INVALID_INPUT: image gen storage entry count overflow".to_string())?;
    if *total_entries > max_entries {
        return Err(format!(
            "SEC_INVALID_INPUT: image gen storage exceeds max entries {max_entries}"
        )
        .into());
    }
    Ok(())
}

#[cfg(unix)]
fn list_stats_entries(
    dir: &std::fs::File,
    total_entries: &mut u64,
    max_entries: u64,
) -> AppResult<Vec<StatsEntry>> {
    use std::os::unix::ffi::OsStrExt as _;
    let mut entries = rustix::fs::Dir::read_from(dir).map_err(|e| {
        format!("SYSTEM_ERROR: failed to enumerate image gen storage directory: {e}")
    })?;
    let mut output = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|e| {
            format!("SYSTEM_ERROR: failed to read image gen storage directory entry: {e}")
        })?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        charge_stats_entry(total_entries, max_entries)?;
        let stat = rustix::fs::statat(dir, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|e| format!("SYSTEM_ERROR: failed to inspect image gen storage entry: {e}"))?;
        let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode);
        let kind = match file_type {
            rustix::fs::FileType::RegularFile => StatsEntryKind::File,
            rustix::fs::FileType::Directory => StatsEntryKind::Directory,
            rustix::fs::FileType::Symlink => {
                StatsEntryKind::Rejected("image gen storage symlink is not allowed")
            }
            _ => StatsEntryKind::Rejected("image gen storage special file is not allowed"),
        };
        output.push(StatsEntry {
            name: std::ffi::OsStr::from_bytes(name.to_bytes()).to_os_string(),
            identity: FileIdentity {
                volume: stat.st_dev as u64,
                file: stat.st_ino as u64,
            },
            kind,
        });
    }
    Ok(output)
}

#[cfg(unix)]
fn open_stats_child(
    parent: &std::fs::File,
    entry: &StatsEntry,
    directory: bool,
) -> AppResult<std::fs::File> {
    // NONBLOCK prevents permanent hang if a regular-file entry is replaced by a
    // FIFO/socket between enumeration and open.
    let mut flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC
        | rustix::fs::OFlags::NONBLOCK;
    if directory {
        flags |= rustix::fs::OFlags::DIRECTORY;
    }
    let fd = rustix::fs::openat(
        parent,
        entry.name.as_os_str(),
        flags,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "SEC_INVALID_INPUT: image gen storage entry cannot be opened".to_string())?;
    let file: std::fs::File = fd.into();
    if !directory {
        let stat = rustix::fs::fstat(&file).map_err(|_| {
            "SEC_INVALID_INPUT: image gen storage entry cannot be inspected".to_string()
        })?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
            return Err(
                "SEC_INVALID_INPUT: image gen storage entry is not a regular file"
                    .to_string()
                    .into(),
            );
        }
    }
    Ok(file)
}

#[cfg(test)]
fn stats_entry_name(entry: &StatsEntry) -> String {
    #[cfg(unix)]
    {
        entry.name.to_string_lossy().into_owned()
    }
    #[cfg(windows)]
    {
        String::from_utf16_lossy(&entry.name_utf16)
    }
}

#[cfg(windows)]
fn list_stats_entries(
    dir: &std::fs::File,
    total_entries: &mut u64,
    max_entries: u64,
) -> AppResult<Vec<StatsEntry>> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES};
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdBothDirectoryInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ID_BOTH_DIR_INFO,
    };

    let volume = {
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
        if unsafe { GetFileInformationByHandle(dir.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
            return Err(
                "SYSTEM_ERROR: failed to inspect image gen storage directory handle".into(),
            );
        }
        unsafe { info.assume_init() }.dwVolumeSerialNumber
    };

    let mut output = Vec::new();
    loop {
        let mut buffer = vec![0_u64; (64 * 1024) / std::mem::size_of::<u64>()];
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle() as _,
                FileIdBothDirectoryInfo,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * std::mem::size_of::<u64>()) as u32,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(format!(
                "SYSTEM_ERROR: failed to enumerate image gen storage handle: os error {error}"
            )
            .into());
        }

        let mut offset = 0usize;
        loop {
            let info = unsafe {
                &*buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>()
            };
            let name_len = info.FileNameLength as usize / std::mem::size_of::<u16>();
            let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), name_len) };
            if name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] {
                charge_stats_entry(total_entries, max_entries)?;
                let attributes = info.FileAttributes;
                let kind = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    StatsEntryKind::Rejected("image gen storage reparse point is not allowed")
                } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                    StatsEntryKind::Directory
                } else {
                    StatsEntryKind::File
                };
                output.push(StatsEntry {
                    name_utf16: name.to_vec(),
                    identity: FileIdentity {
                        volume: u64::from(volume),
                        file: info.FileId as u64,
                    },
                    kind,
                });
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset = offset
                .checked_add(info.NextEntryOffset as usize)
                .ok_or_else(|| {
                    "SYSTEM_ERROR: invalid image gen storage entry offset".to_string()
                })?;
            if offset >= buffer.len() * std::mem::size_of::<u64>() {
                return Err("SYSTEM_ERROR: invalid image gen storage entry buffer"
                    .to_string()
                    .into());
            }
        }
    }
    Ok(output)
}

#[cfg(windows)]
fn open_stats_child(
    parent: &std::fs::File,
    entry: &StatsEntry,
    directory: bool,
) -> AppResult<std::fs::File> {
    let mut name = entry.name_utf16.clone();
    open_windows_child_no_follow(parent, &mut name, directory).map_err(|err| {
        format!("SEC_INVALID_INPUT: image gen storage entry cannot be opened: {err}").into()
    })
}

/// Ensures the directory exists and is writable (probe file round-trip).
pub(crate) fn ensure_writable_dir(dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("SEC_INVALID_INPUT: storage dir cannot be created: {e}"))?;
    let probe = dir.join(".aio-write-probe");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| format!("SEC_INVALID_INPUT: storage dir is not writable: {e}"))?;
    let write_result = file.write_all(b"probe");
    drop(file);
    let _ = std::fs::remove_file(&probe);
    write_result.map_err(|e| format!("SEC_INVALID_INPUT: storage dir is not writable: {e}"))?;
    Ok(())
}
