//! Skill file system utilities for config export/import.

use crate::shared::error::{AppError, AppResult};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use super::{
    LocalSkillExport, SkillFileExport, CONFIG_SKILL_EXPORT_ENCODED_MAX_BYTES,
    CONFIG_SKILL_EXPORT_FILE_COUNT_MAX, CONFIG_SKILL_FILE_COUNT_MAX, CONFIG_SKILL_FILE_MAX_BYTES,
    CONFIG_SKILL_MD_MAX_BYTES, CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS,
    CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES, CONFIG_SKILL_TOTAL_MAX_BYTES,
    SKILL_MANAGED_MARKER_FILE, SKILL_SOURCE_MARKER_FILE,
};

const CONFIG_SKILL_FILE_BASE64_MAX_BYTES: usize = CONFIG_SKILL_FILE_MAX_BYTES.div_ceil(3) * 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SkillSourceMetadataFile {
    pub(super) source_git_url: String,
    pub(super) source_branch: String,
    pub(super) source_subdir: String,
}

pub(super) fn ssot_skills_root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> AppResult<PathBuf> {
    let cli_key =
        crate::shared::cli_key::cli_keys_with(crate::shared::cli_key::CliCapability::Skills)
            .next()
            .ok_or_else(|| "INTERNAL_ERROR: no CLI supports skills".to_string())?;
    let paths = crate::skills::paths_get(app, cli_key)?;
    Ok(PathBuf::from(paths.ssot_dir))
}

pub(super) fn cli_skills_root<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    cli_key: &str,
) -> AppResult<PathBuf> {
    let paths = crate::skills::paths_get(app, cli_key)?;
    Ok(PathBuf::from(paths.cli_dir))
}

// Import rollback moves local directories as path objects; export uses
// `SkillExportRoot` below so no path returned here becomes read authority.
pub(super) fn local_skill_dirs(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut items = Vec::new();
    if !root.exists() {
        return Ok(items);
    }
    for entry in std::fs::read_dir(root)
        .map_err(|e| format!("failed to read dir {}: {e}", root.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read dir entry {}: {e}", root.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("failed to read file type {}: {e}", entry.path().display()))?;
        if file_type.is_dir()
            && entry.path().join("SKILL.md").exists()
            && !entry.path().join(SKILL_MANAGED_MARKER_FILE).exists()
        {
            items.push(entry.path());
        }
    }
    items.sort();
    Ok(items)
}

#[cfg(test)]
pub(super) fn export_skill_dir_files(
    dir: &Path,
    skip_source_marker: bool,
) -> AppResult<Vec<SkillFileExport>> {
    let mut export_budget = SkillExportBudget::default();
    let mut collector = SkillFileCollector::new(&mut export_budget);
    let mut visited_dirs = HashSet::new();
    let root = ExportDir::open_root(dir)?;
    visited_dirs.insert(root.identity()?);
    collect_skill_dir_files(
        &root,
        Path::new(""),
        &mut collector,
        &mut visited_dirs,
        skip_source_marker,
    )?;
    Ok(collector.files)
}

pub(super) struct SkillExportRoot {
    dir: ExportDir,
}

pub(super) struct CapturedSkillDir {
    dir: ExportDir,
    dir_name: String,
}

impl SkillExportRoot {
    pub(super) fn open_if_exists(path: &Path) -> AppResult<Option<Self>> {
        match std::fs::symlink_metadata(path) {
            Ok(_) => Ok(Some(Self {
                dir: ExportDir::open_root(path)?,
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "SEC_INVALID_INPUT: skill export parent root cannot be inspected: {error}"
            )
            .into()),
        }
    }

    pub(super) fn capture_named(&self, name: &str) -> AppResult<Option<CapturedSkillDir>> {
        let entries = self.entries_after_test_hook()?;
        let Some(entry) = entries
            .into_iter()
            .find(|entry| platform_path_component_eq(&entry.name.to_string_lossy(), name))
        else {
            return Ok(None);
        };
        if entry.is_link || !entry.is_directory {
            return Err(
                "SEC_INVALID_INPUT: skill export top-level entry is not a trusted directory"
                    .to_string()
                    .into(),
            );
        }
        Ok(Some(self.capture_entry(entry)?))
    }

    pub(super) fn capture_local_skills(&self) -> AppResult<Vec<CapturedSkillDir>> {
        let mut entries = self.entries_after_test_hook()?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let mut skills = Vec::new();
        for entry in entries {
            // Managed Skill links are intentionally not local export authority.
            if entry.is_link || !entry.is_directory {
                continue;
            }
            skills.push(self.capture_entry(entry)?);
        }
        Ok(skills)
    }

    fn entries_after_test_hook(&self) -> AppResult<Vec<ExportEntry>> {
        let entries = self.dir.entries()?;
        #[cfg(test)]
        run_after_skill_export_enumeration_test_hook();
        Ok(entries)
    }

    fn capture_entry(&self, entry: ExportEntry) -> AppResult<CapturedSkillDir> {
        let dir_name = entry.name.to_str().map(str::to_string).ok_or_else(|| {
            "SKILL_EXPORT_INVALID_DIR_NAME: local skill dir name invalid".to_string()
        })?;
        Ok(CapturedSkillDir {
            dir: self.dir.open_dir(&entry.name, entry.identity)?,
            dir_name,
        })
    }
}

impl CapturedSkillDir {
    pub(super) fn dir_name(&self) -> &str {
        &self.dir_name
    }

    pub(super) fn export_files(
        &self,
        skip_source_marker: bool,
        export_budget: &mut SkillExportBudget,
    ) -> AppResult<Vec<SkillFileExport>> {
        let mut collector = SkillFileCollector::new(export_budget);
        let mut visited_dirs = HashSet::new();
        visited_dirs.insert(self.dir.identity()?);
        collect_skill_dir_files(
            &self.dir,
            Path::new(""),
            &mut collector,
            &mut visited_dirs,
            skip_source_marker,
        )?;
        Ok(collector.files)
    }

    pub(super) fn export_local_files(
        &self,
        skip_source_marker: bool,
        export_budget: &mut SkillExportBudget,
    ) -> AppResult<Option<Vec<SkillFileExport>>> {
        let entries = self.dir.entries()?;
        let has_skill_md = entries
            .iter()
            .any(|entry| platform_path_component_eq(&entry.name.to_string_lossy(), "SKILL.md"));
        let has_managed_marker = entries.iter().any(|entry| {
            platform_path_component_eq(&entry.name.to_string_lossy(), SKILL_MANAGED_MARKER_FILE)
        });
        if !has_skill_md || has_managed_marker {
            return Ok(None);
        }
        let mut collector = SkillFileCollector::new(export_budget);
        let mut visited_dirs = HashSet::new();
        visited_dirs.insert(self.dir.identity()?);
        collect_skill_dir_entries(
            &self.dir,
            Path::new(""),
            entries,
            &mut collector,
            &mut visited_dirs,
            skip_source_marker,
        )?;
        Ok(Some(collector.files))
    }
}

#[derive(Debug, Default)]
pub(super) struct SkillExportBudget {
    encoded_bytes: usize,
    file_count: usize,
}

impl SkillExportBudget {
    fn reserve_file(&mut self, raw_bytes: usize) -> AppResult<()> {
        let encoded_bytes = raw_bytes
            .checked_add(2)
            .and_then(|value| value.checked_div(3))
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| {
                "SEC_INVALID_INPUT: skill export aggregate encoded payload overflow".to_string()
            })?;
        let next_encoded_bytes =
            self.encoded_bytes
                .checked_add(encoded_bytes)
                .ok_or_else(|| {
                    "SEC_INVALID_INPUT: skill export aggregate encoded payload overflow".to_string()
                })?;
        let next_file_count = self.file_count.checked_add(1).ok_or_else(|| {
            "SEC_INVALID_INPUT: skill export aggregate file count overflow".to_string()
        })?;
        if next_file_count > CONFIG_SKILL_EXPORT_FILE_COUNT_MAX {
            return Err(format!(
                "SEC_INVALID_INPUT: too many skill export aggregate files (max {CONFIG_SKILL_EXPORT_FILE_COUNT_MAX})"
            )
            .into());
        }
        if next_encoded_bytes > CONFIG_SKILL_EXPORT_ENCODED_MAX_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: skill export aggregate encoded payload too large (max {CONFIG_SKILL_EXPORT_ENCODED_MAX_BYTES} bytes)"
            )
            .into());
        }
        self.encoded_bytes = next_encoded_bytes;
        self.file_count = next_file_count;
        Ok(())
    }
}

struct SkillFileCollector<'a> {
    files: Vec<SkillFileExport>,
    total_bytes: usize,
    export_budget: &'a mut SkillExportBudget,
}

impl<'a> SkillFileCollector<'a> {
    fn new(export_budget: &'a mut SkillExportBudget) -> Self {
        Self {
            files: Vec::new(),
            total_bytes: 0,
            export_budget,
        }
    }

    fn push_file(&mut self, relative_path: &Path, mut source: std::fs::File) -> AppResult<()> {
        if self.files.len() >= CONFIG_SKILL_FILE_COUNT_MAX {
            return Err(format!(
                "SEC_INVALID_INPUT: too many skill files (max {CONFIG_SKILL_FILE_COUNT_MAX})"
            )
            .into());
        }

        let relative_path = relative_path_string(relative_path)?;
        let file_limit = if is_skill_md_path(Path::new(&relative_path)) {
            CONFIG_SKILL_MD_MAX_BYTES
        } else if platform_path_component_eq(&relative_path, SKILL_SOURCE_MARKER_FILE) {
            CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES
        } else {
            CONFIG_SKILL_FILE_MAX_BYTES
        };
        let metadata = source
            .metadata()
            .map_err(|e| format!("failed to inspect skill file handle: {e}"))?;
        if !metadata.is_file() || metadata.len() > file_limit as u64 {
            return Err(format!(
                "SEC_INVALID_INPUT: skill file too large (max {file_limit} bytes)"
            )
            .into());
        }
        ensure_export_file_single_link(&source)?;
        #[cfg(test)]
        run_after_skill_export_file_metadata_test_hook();
        let content = crate::shared::fs::read_open_file_with_max_len(&mut source, file_limit)
            .map_err(|e| -> AppError {
                let message = e.to_string();
                if message.contains("too large") {
                    format!("SEC_INVALID_INPUT: skill file too large (max {file_limit} bytes)")
                        .into()
                } else {
                    format!("failed to read skill file handle: {message}").into()
                }
            })?;
        let next_total = self
            .total_bytes
            .checked_add(content.len())
            .ok_or_else(|| "SEC_INVALID_INPUT: skill file payload too large".to_string())?;
        if next_total > CONFIG_SKILL_TOTAL_MAX_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: skill file payload too large (max {CONFIG_SKILL_TOTAL_MAX_BYTES} bytes)"
            )
            .into());
        }
        self.export_budget.reserve_file(content.len())?;

        self.total_bytes = next_total;
        self.files.push(SkillFileExport {
            relative_path,
            content_base64: BASE64_STANDARD.encode(content),
        });
        Ok(())
    }
}

fn collect_skill_dir_files(
    dir: &ExportDir,
    relative_root: &Path,
    files: &mut SkillFileCollector<'_>,
    visited_dirs: &mut HashSet<ExportIdentity>,
    skip_source_marker: bool,
) -> AppResult<()> {
    let mut entries = dir.entries()?;
    entries.sort_by(|left, right| left.name.cmp(&right.name));

    #[cfg(test)]
    run_after_skill_export_enumeration_test_hook();

    collect_skill_dir_entries(
        dir,
        relative_root,
        entries,
        files,
        visited_dirs,
        skip_source_marker,
    )
}

fn collect_skill_dir_entries(
    dir: &ExportDir,
    relative_root: &Path,
    entries: Vec<ExportEntry>,
    files: &mut SkillFileCollector<'_>,
    visited_dirs: &mut HashSet<ExportIdentity>,
    skip_source_marker: bool,
) -> AppResult<()> {
    for entry in entries {
        let name = entry.name;
        let name_str = name.to_string_lossy();
        if platform_path_component_eq(&name_str, SKILL_MANAGED_MARKER_FILE) {
            continue;
        }
        if skip_source_marker && platform_path_component_eq(&name_str, SKILL_SOURCE_MARKER_FILE) {
            continue;
        }

        let relative_path = relative_root.join(&name);
        if entry.is_link {
            if dir.link_targets_visited_directory(&name, visited_dirs)? {
                continue;
            }
            return Err(format!(
                "SKILL_EXPORT_BLOCKED_SYMLINK_ESCAPE: {}",
                relative_path.display()
            )
            .into());
        }

        if entry.is_directory {
            let child = dir.open_dir(&name, entry.identity)?;
            let identity = child.identity()?;
            if visited_dirs.insert(identity) {
                collect_skill_dir_files(
                    &child,
                    &relative_path,
                    files,
                    visited_dirs,
                    skip_source_marker,
                )?;
            }
            continue;
        }
        if !entry.is_file {
            return Err(format!(
                "SKILL_EXPORT_BLOCKED_SPECIAL_FILE: {}",
                relative_path.display()
            )
            .into());
        }
        let file = dir.open_file(&name, entry.identity)?;
        files.push_file(&relative_path, file)?;
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct ExportIdentity {
    volume: u64,
    file: u64,
}

#[derive(Debug)]
struct ExportEntry {
    name: std::ffi::OsString,
    identity: ExportIdentity,
    is_file: bool,
    is_directory: bool,
    is_link: bool,
}

#[derive(Debug)]
struct ExportDir {
    handle: std::fs::File,
    display_path: PathBuf,
    root_path: PathBuf,
}

impl ExportDir {
    fn open_root(path: &Path) -> AppResult<Self> {
        let handle = open_export_root(path)?;
        Ok(Self {
            handle,
            display_path: path.to_path_buf(),
            root_path: path.to_path_buf(),
        })
    }

    fn identity(&self) -> AppResult<ExportIdentity> {
        export_identity(&self.handle)
    }

    fn entries(&self) -> AppResult<Vec<ExportEntry>> {
        export_entries(&self.handle)
    }

    fn open_dir(&self, name: &std::ffi::OsStr, expected: ExportIdentity) -> AppResult<Self> {
        let handle = open_export_child(&self.handle, name, true)?;
        if export_identity(&handle)? != expected {
            return Err("SEC_INVALID_INPUT: skill export directory identity changed"
                .to_string()
                .into());
        }
        Ok(Self {
            handle,
            display_path: self.display_path.join(name),
            root_path: self.root_path.clone(),
        })
    }

    fn open_file(
        &self,
        name: &std::ffi::OsStr,
        expected: ExportIdentity,
    ) -> AppResult<std::fs::File> {
        let handle = open_export_child(&self.handle, name, false)?;
        if export_identity(&handle)? != expected {
            return Err("SEC_INVALID_INPUT: skill export file identity changed"
                .to_string()
                .into());
        }
        ensure_export_file_single_link(&handle)?;
        Ok(handle)
    }

    fn link_targets_visited_directory(
        &self,
        name: &std::ffi::OsStr,
        visited: &HashSet<ExportIdentity>,
    ) -> AppResult<bool> {
        let candidate = self.display_path.join(name);
        let canonical = candidate
            .canonicalize()
            .map_err(|e| format!("failed to resolve symlink {}: {e}", candidate.display()))?;
        if !canonical.starts_with(&self.root_path) {
            return Ok(false);
        }
        let handle = open_export_root(&canonical)?;
        Ok(handle.metadata().map(|meta| meta.is_dir()).unwrap_or(false)
            && visited.contains(&export_identity(&handle)?))
    }
}

#[cfg(unix)]
fn open_export_root(path: &Path) -> AppResult<std::fs::File> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "SEC_INVALID_INPUT: skill export directory cannot be opened".to_string())?;
    Ok(fd.into())
}

#[cfg(unix)]
fn export_identity(file: &std::fs::File) -> AppResult<ExportIdentity> {
    let stat = rustix::fs::fstat(file)
        .map_err(|_| "SEC_INVALID_INPUT: skill export identity cannot be read".to_string())?;
    Ok(ExportIdentity {
        volume: stat.st_dev as u64,
        file: stat.st_ino as u64,
    })
}

#[cfg(unix)]
fn export_entries(dir: &std::fs::File) -> AppResult<Vec<ExportEntry>> {
    use std::os::unix::ffi::OsStrExt as _;
    let mut entries = rustix::fs::Dir::read_from(dir)
        .map_err(|e| format!("failed to enumerate skill directory handle: {e}"))?;
    let mut output = Vec::new();
    while let Some(entry) = entries.next() {
        let entry = entry.map_err(|e| format!("failed to read skill directory entry: {e}"))?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let stat = rustix::fs::statat(dir, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|e| format!("failed to inspect skill directory entry: {e}"))?;
        let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode);
        output.push(ExportEntry {
            name: std::ffi::OsStr::from_bytes(name.to_bytes()).to_os_string(),
            identity: ExportIdentity {
                volume: stat.st_dev as u64,
                file: stat.st_ino as u64,
            },
            is_file: file_type == rustix::fs::FileType::RegularFile,
            is_directory: file_type == rustix::fs::FileType::Directory,
            is_link: file_type == rustix::fs::FileType::Symlink,
        });
    }
    Ok(output)
}

#[cfg(unix)]
fn open_export_child(
    parent: &std::fs::File,
    name: &std::ffi::OsStr,
    directory: bool,
) -> AppResult<std::fs::File> {
    let mut flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC
        | rustix::fs::OFlags::NONBLOCK;
    if directory {
        flags |= rustix::fs::OFlags::DIRECTORY;
    }
    let fd = rustix::fs::openat(parent, name, flags, rustix::fs::Mode::empty())
        .map_err(|_| "SEC_INVALID_INPUT: skill export entry cannot be opened".to_string())?;
    Ok(fd.into())
}

#[cfg(unix)]
fn ensure_export_file_single_link(file: &std::fs::File) -> AppResult<()> {
    let stat = rustix::fs::fstat(file)
        .map_err(|_| "SEC_INVALID_INPUT: skill export file cannot be inspected".to_string())?;
    if stat.st_nlink != 1 {
        return Err("SEC_INVALID_INPUT: skill export hard links are not allowed"
            .to_string()
            .into());
    }
    Ok(())
}

#[cfg(windows)]
fn open_export_root(path: &Path) -> AppResult<std::fs::File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| "SEC_INVALID_INPUT: skill export root cannot be inspected".to_string())?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(
            "SEC_INVALID_INPUT: skill export root cannot be a reparse point"
                .to_string()
                .into(),
        );
    }
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| format!("SEC_INVALID_INPUT: skill export root cannot be opened: {e}"))?;
    ensure_windows_export_directory_handle(&handle)?;
    Ok(handle)
}

#[cfg(windows)]
fn ensure_windows_export_directory_handle(file: &std::fs::File) -> AppResult<()> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
        return Err(
            "SEC_INVALID_INPUT: skill export root handle cannot be inspected"
                .to_string()
                .into(),
        );
    }
    let attributes = unsafe { info.assume_init() }.dwFileAttributes;
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(
            "SEC_INVALID_INPUT: skill export root handle is not a trusted directory"
                .to_string()
                .into(),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn export_identity(file: &std::fs::File) -> AppResult<ExportIdentity> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
        return Err("SEC_INVALID_INPUT: skill export identity cannot be read"
            .to_string()
            .into());
    }
    let info = unsafe { info.assume_init() };
    Ok(ExportIdentity {
        volume: u64::from(info.dwVolumeSerialNumber),
        file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

#[cfg(windows)]
fn export_entries(dir: &std::fs::File) -> AppResult<Vec<ExportEntry>> {
    use std::os::windows::ffi::OsStringExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES};
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdBothDirectoryInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO,
    };
    let volume = export_identity(dir)?.volume;
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
            return Err(
                format!("failed to enumerate skill directory handle: os error {error}").into(),
            );
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
            let len = info.FileNameLength as usize / std::mem::size_of::<u16>();
            let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), len) };
            if name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] {
                let attributes = info.FileAttributes;
                output.push(ExportEntry {
                    name: std::ffi::OsString::from_wide(name),
                    identity: ExportIdentity {
                        volume,
                        file: info.FileId as u64,
                    },
                    is_file: attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                        == 0,
                    is_directory: attributes & FILE_ATTRIBUTE_DIRECTORY != 0
                        && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
                    is_link: attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
                });
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset = offset
                .checked_add(info.NextEntryOffset as usize)
                .ok_or_else(|| {
                    "SEC_INVALID_INPUT: invalid skill directory entry offset".to_string()
                })?;
        }
    }
    Ok(output)
}

#[cfg(windows)]
fn open_export_child(
    parent: &std::fs::File,
    name: &std::ffi::OsStr,
    directory: bool,
) -> AppResult<std::fs::File> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        NtCreateFile, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Foundation::{GENERIC_READ, HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    let mut name = name.encode_wide().collect::<Vec<_>>();
    let byte_len = u16::try_from(name.len() * std::mem::size_of::<u16>())
        .map_err(|_| "SEC_INVALID_INPUT: skill export entry name is too long".to_string())?;
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
            GENERIC_READ
                | FILE_READ_ATTRIBUTES
                | SYNCHRONIZE
                | if directory { FILE_LIST_DIRECTORY } else { 0 },
            &attributes,
            io_status.as_mut_ptr(),
            std::ptr::null(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT
                | FILE_SYNCHRONOUS_IO_NONALERT
                | if directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
                },
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle.is_null() {
        return Err(format!("SEC_INVALID_INPUT: skill export entry cannot be opened relative to root handle: ntstatus {status:#x}").into());
    }
    Ok(unsafe { std::fs::File::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn ensure_export_file_single_link(file: &std::fs::File) -> AppResult<()> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0
        || unsafe { info.assume_init() }.nNumberOfLinks != 1
    {
        return Err("SEC_INVALID_INPUT: skill export hard links are not allowed"
            .to_string()
            .into());
    }
    Ok(())
}

#[cfg(test)]
type AfterSkillExportEnumerationHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
type AfterSkillExportFileMetadataHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
thread_local! {
    static AFTER_SKILL_EXPORT_ENUMERATION_TEST_HOOK: std::cell::RefCell<Option<AfterSkillExportEnumerationHook>> = const { std::cell::RefCell::new(None) };
    static AFTER_SKILL_EXPORT_FILE_METADATA_TEST_HOOK: std::cell::RefCell<Option<AfterSkillExportFileMetadataHook>> = const { std::cell::RefCell::new(None) };
    static WRITE_PREPARED_SKILL_FILES_FAILPOINT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(super) fn set_after_skill_export_enumeration_test_hook(hook: AfterSkillExportEnumerationHook) {
    AFTER_SKILL_EXPORT_ENUMERATION_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
pub(super) fn set_after_skill_export_file_metadata_test_hook(
    hook: AfterSkillExportFileMetadataHook,
) {
    AFTER_SKILL_EXPORT_FILE_METADATA_TEST_HOOK.with(|current| current.replace(Some(hook)));
}

#[cfg(test)]
fn run_after_skill_export_enumeration_test_hook() {
    let hook = AFTER_SKILL_EXPORT_ENUMERATION_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
fn run_after_skill_export_file_metadata_test_hook() {
    let hook =
        AFTER_SKILL_EXPORT_FILE_METADATA_TEST_HOOK.with(|current| current.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
pub(super) fn set_write_prepared_skill_files_failpoint(after_files: Option<usize>) {
    WRITE_PREPARED_SKILL_FILES_FAILPOINT.with(|current| current.set(after_files));
}

#[cfg(test)]
fn run_write_prepared_skill_files_failpoint() -> AppResult<()> {
    let should_fail = WRITE_PREPARED_SKILL_FILES_FAILPOINT.with(|current| match current.get() {
        Some(remaining) if remaining <= 1 => {
            current.set(None);
            true
        }
        Some(remaining) => {
            current.set(Some(remaining - 1));
            false
        }
        None => false,
    });
    if should_fail {
        return Err("SYSTEM_ERROR: injected mid-write failure".into());
    }
    Ok(())
}

fn relative_path_string(path: &Path) -> AppResult<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| "SEC_INVALID_INPUT: invalid utf-8 skill path".to_string())?
                    .to_string(),
            ),
            _ => {
                return Err(format!(
                    "SEC_INVALID_INPUT: invalid skill relative path component in {}",
                    path.display()
                )
                .into())
            }
        }
    }

    if parts.is_empty() {
        return Err("SEC_INVALID_INPUT: empty skill relative path"
            .to_string()
            .into());
    }

    let relative_path = parts.join("/");
    if relative_path.chars().count() > CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS {
        return Err(format!(
            "SEC_INVALID_INPUT: skill relative path too long (max {CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS} chars)"
        )
        .into());
    }
    Ok(relative_path)
}

#[cfg(test)]
pub(super) fn write_skill_files_to_dir(
    dir: &Path,
    files: &[SkillFileExport],
    source_metadata: Option<&SkillSourceMetadataFile>,
) -> AppResult<()> {
    let prepared = prepare_skill_files_for_write(files, source_metadata)?;
    write_prepared_skill_files_to_dir(dir, prepared)
}

#[derive(Debug)]
pub(super) struct PreparedSkillFiles {
    decoded_files: Vec<(PathBuf, Vec<u8>)>,
    source_metadata_bytes: Option<Vec<u8>>,
}

pub(super) fn prepare_skill_files_for_write(
    files: &[SkillFileExport],
    source_metadata: Option<&SkillSourceMetadataFile>,
) -> AppResult<PreparedSkillFiles> {
    let decoded_files = decode_skill_files_for_write(files)?;
    let source_metadata_bytes = source_metadata
        .map(|metadata| -> AppResult<Vec<u8>> {
            let bytes = serde_json::to_vec_pretty(metadata)
                .map_err(|e| format!("SYSTEM_ERROR: failed to serialize source metadata: {e}"))?;
            if bytes.len() > CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES {
                return Err(format!(
                    "SEC_INVALID_INPUT: skill source metadata too large (max {CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES} bytes)"
                )
                .into());
            }
            Ok(bytes)
        })
        .transpose()?;

    Ok(PreparedSkillFiles {
        decoded_files,
        source_metadata_bytes,
    })
}

pub(super) fn write_prepared_skill_files_to_dir(
    dir: &Path,
    prepared: PreparedSkillFiles,
) -> AppResult<()> {
    match std::fs::create_dir(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(format!("SKILL_IMPORT_DIR_ALREADY_EXISTS: {}", dir.display()).into())
        }
        Err(error) => return Err(format!("failed to create {}: {error}", dir.display()).into()),
    }

    write_prepared_skill_files_into_existing_dir(dir, prepared)
}

pub(super) fn write_prepared_skill_files_into_existing_dir(
    dir: &Path,
    prepared: PreparedSkillFiles,
) -> AppResult<()> {
    if !dir.is_dir() {
        return Err(format!("SKILL_IMPORT_DIR_NOT_DIRECTORY: {}", dir.display()).into());
    }

    for (relative_path, bytes) in prepared.decoded_files {
        let target = dir.join(&relative_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        crate::shared::fs::write_file_atomic(&target, &bytes)?;
        #[cfg(test)]
        run_write_prepared_skill_files_failpoint()?;
    }

    let managed_marker = dir.join(SKILL_MANAGED_MARKER_FILE);
    if managed_marker.exists() {
        std::fs::remove_file(&managed_marker)
            .map_err(|e| format!("failed to remove {}: {e}", managed_marker.display()))?;
    }

    let source_marker = dir.join(SKILL_SOURCE_MARKER_FILE);
    if let Some(bytes) = prepared.source_metadata_bytes {
        crate::shared::fs::write_file_atomic(&source_marker, &bytes)?;
    } else if source_marker.exists() {
        std::fs::remove_file(&source_marker)
            .map_err(|e| format!("failed to remove {}: {e}", source_marker.display()))?;
    }

    Ok(())
}

fn decode_skill_files_for_write(files: &[SkillFileExport]) -> AppResult<Vec<(PathBuf, Vec<u8>)>> {
    if files.len() > CONFIG_SKILL_FILE_COUNT_MAX {
        return Err(format!(
            "SEC_INVALID_INPUT: too many skill files (max {CONFIG_SKILL_FILE_COUNT_MAX})"
        )
        .into());
    }

    let reserved_paths = [
        normalized_skill_path(Path::new(SKILL_MANAGED_MARKER_FILE))?,
        normalized_skill_path(Path::new(SKILL_SOURCE_MARKER_FILE))?,
    ];
    let mut seen_paths: Vec<(Vec<String>, bool)> = reserved_paths
        .into_iter()
        .map(|path| (path, true))
        .collect();
    let mut total_bytes = 0_usize;
    let mut decoded_files = Vec::with_capacity(files.len());
    for file in files {
        let relative_path = validate_skill_file_relative_path(&file.relative_path)?;
        let path_key = normalized_skill_path(&relative_path)?;
        if let Some((_, reserved)) = seen_paths.iter().find(|(other, _)| other == &path_key) {
            if *reserved {
                return Err(format!(
                    "SEC_INVALID_INPUT: reserved skill marker path {}",
                    file.relative_path
                )
                .into());
            }
            return Err(format!(
                "SEC_INVALID_INPUT: duplicate skill file path {}",
                file.relative_path
            )
            .into());
        }

        if let Some((_, reserved)) = seen_paths.iter().find(|(other, _)| {
            path_components_start_with(other, &path_key)
                || path_components_start_with(&path_key, other)
        }) {
            if *reserved {
                return Err(format!(
                    "SEC_INVALID_INPUT: reserved skill marker path conflicts with {}",
                    file.relative_path
                )
                .into());
            }
            return Err(format!(
                "SEC_INVALID_INPUT: conflicting skill file paths involving {}",
                file.relative_path
            )
            .into());
        }
        seen_paths.push((path_key, false));

        if file.content_base64.len() > CONFIG_SKILL_FILE_BASE64_MAX_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: skill file {} too large (max {CONFIG_SKILL_FILE_MAX_BYTES} bytes)",
                file.relative_path
            )
            .into());
        }

        let bytes = BASE64_STANDARD
            .decode(file.content_base64.as_bytes())
            .map_err(|e| {
                format!(
                    "SEC_INVALID_INPUT: invalid base64 for {}: {e}",
                    file.relative_path
                )
            })?;
        if bytes.len() > CONFIG_SKILL_FILE_MAX_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: skill file {} too large (max {CONFIG_SKILL_FILE_MAX_BYTES} bytes)",
                file.relative_path
            )
            .into());
        }
        if is_skill_md_path(&relative_path) && bytes.len() > CONFIG_SKILL_MD_MAX_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: SKILL.md too large (max {CONFIG_SKILL_MD_MAX_BYTES} bytes)"
            )
            .into());
        }
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| "SEC_INVALID_INPUT: skill file payload too large".to_string())?;
        if total_bytes > CONFIG_SKILL_TOTAL_MAX_BYTES {
            return Err(format!(
                "SEC_INVALID_INPUT: skill file payload too large (max {CONFIG_SKILL_TOTAL_MAX_BYTES} bytes)"
            )
            .into());
        }
        decoded_files.push((relative_path, bytes));
    }

    Ok(decoded_files)
}

fn normalized_skill_path(path: &Path) -> AppResult<Vec<String>> {
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part
                .to_str()
                .map(normalize_platform_path_component)
                .ok_or_else(|| {
                    "SEC_INVALID_INPUT: invalid utf-8 skill path"
                        .to_string()
                        .into()
                }),
            _ => Err("SEC_INVALID_INPUT: invalid skill path component"
                .to_string()
                .into()),
        })
        .collect()
}

fn path_components_start_with(path: &[String], prefix: &[String]) -> bool {
    path.len() > prefix.len() && path.starts_with(prefix)
}

fn is_skill_md_path(path: &Path) -> bool {
    normalized_skill_path(path)
        .is_ok_and(|path| path == vec![normalize_platform_path_component("SKILL.md")])
}

pub(super) fn platform_path_component_eq(left: &str, right: &str) -> bool {
    normalize_platform_path_component(left) == normalize_platform_path_component(right)
}

#[cfg(windows)]
fn normalize_platform_path_component(value: &str) -> String {
    value.to_lowercase()
}

#[cfg(not(windows))]
fn normalize_platform_path_component(value: &str) -> String {
    value.to_string()
}

fn validate_skill_file_relative_path(relative_path: &str) -> AppResult<PathBuf> {
    if relative_path.chars().count() > CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS {
        return Err(format!(
            "SEC_INVALID_INPUT: skill relative path too long (max {CONFIG_SKILL_RELATIVE_PATH_MAX_CHARS} chars)"
        )
        .into());
    }

    if relative_path.contains('\\') || relative_path.contains(':') {
        return Err(format!(
            "SEC_INVALID_INPUT: invalid skill relative path {}",
            relative_path
        )
        .into());
    }
    let path = Path::new(relative_path);
    if path.as_os_str().is_empty() {
        return Err("SEC_INVALID_INPUT: empty skill relative path"
            .to_string()
            .into());
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            _ => {
                return Err(format!(
                    "SEC_INVALID_INPUT: invalid skill relative path {}",
                    relative_path
                )
                .into())
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err("SEC_INVALID_INPUT: empty skill relative path"
            .to_string()
            .into());
    }

    Ok(normalized)
}

pub(super) fn validate_local_dir_name(dir_name: &str) -> AppResult<String> {
    let trimmed = dir_name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(format!("SEC_INVALID_INPUT: invalid local skill dir_name={dir_name}").into());
    }
    Ok(trimmed.to_string())
}

pub(super) fn validate_installed_skill_key(skill_key: &str) -> AppResult<String> {
    let trimmed = skill_key.trim();
    let path = Path::new(trimmed);
    let mut components = path.components();
    let is_single_normal =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains(':')
        || !is_single_normal
    {
        return Err(format!("SEC_INVALID_INPUT: invalid installed skill_key={skill_key}").into());
    }
    Ok(trimmed.to_string())
}

pub(super) fn build_local_skill_source_metadata(
    local_skill: &LocalSkillExport,
) -> AppResult<Option<SkillSourceMetadataFile>> {
    match (
        local_skill.source_git_url.as_deref().map(str::trim),
        local_skill.source_branch.as_deref().map(str::trim),
        local_skill.source_subdir.as_deref().map(str::trim),
    ) {
        (None, None, None) => Ok(None),
        (Some(git_url), Some(branch), Some(source_subdir))
            if !git_url.is_empty() && !branch.is_empty() && !source_subdir.is_empty() =>
        {
            Ok(Some(SkillSourceMetadataFile {
                source_git_url: git_url.to_string(),
                source_branch: branch.to_string(),
                source_subdir: source_subdir.to_string(),
            }))
        }
        _ => Err(format!(
            "SEC_INVALID_INPUT: incomplete local skill source metadata for cli_key={}, dir_name={}",
            local_skill.cli_key, local_skill.dir_name
        )
        .into()),
    }
}

pub(super) fn read_local_skill_source_metadata_bytes(
    bytes: Option<&[u8]>,
) -> AppResult<Option<SkillSourceMetadataFile>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if bytes.len() > CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: skill source metadata too large (max {CONFIG_SKILL_SOURCE_METADATA_MAX_BYTES} bytes)"
        )
        .into());
    }
    let metadata = serde_json::from_slice::<SkillSourceMetadataFile>(bytes)
        .map_err(|e| format!("failed to parse {SKILL_SOURCE_MARKER_FILE}: {e}"))?;
    Ok(Some(metadata))
}

pub(super) fn parse_skill_md_metadata_bytes(bytes: Vec<u8>) -> AppResult<(String, String)> {
    if bytes.len() > CONFIG_SKILL_MD_MAX_BYTES {
        return Err(format!(
            "SEC_INVALID_INPUT: skill file too large (max {CONFIG_SKILL_MD_MAX_BYTES} bytes)"
        )
        .into());
    }
    let text = String::from_utf8(bytes)
        .map_err(|e| format!("SEC_INVALID_INPUT: invalid UTF-8 in SKILL.md: {e}"))?;
    let text = text.trim_start();
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return Err("SEC_INVALID_INPUT: SKILL.md is empty".to_string().into());
    };
    if first.trim() != "---" {
        return Err("SEC_INVALID_INPUT: SKILL.md front matter is required"
            .to_string()
            .into());
    }

    let mut front_matter = HashMap::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string();
        front_matter.insert(key.trim().to_string(), value);
    }

    let name = front_matter.get("name").cloned().unwrap_or_default();
    let description = front_matter.get("description").cloned().unwrap_or_default();
    if name.trim().is_empty() {
        return Err("SEC_INVALID_INPUT: SKILL.md missing 'name'"
            .to_string()
            .into());
    }
    Ok((name.trim().to_string(), description.trim().to_string()))
}

pub(super) fn remove_dir_if_exists(path: &Path) -> AppResult<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)
            .map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
    }
    Ok(())
}
