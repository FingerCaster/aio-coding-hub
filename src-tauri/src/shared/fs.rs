//! Usage: Small filesystem helpers shared across infra adapters (atomic writes, optional reads).

use std::path::Path;

const WRITE_IF_CHANGED_COMPARE_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Atomically move one path without replacing an existing destination.
///
/// Callers use this for ownership capture and recovery, where an existence
/// check followed by `rename` would reintroduce a clobber race.
pub(crate) fn rename_file_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_file_no_replace_impl(from, to)
}

#[cfg(windows)]
fn rename_file_no_replace_impl(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let from = from
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn rename_file_no_replace_impl(from: &Path, to: &Path) -> std::io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};

    renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(Into::into)
}

#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_vendor = "apple"))
))]
fn rename_file_no_replace_impl(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is unavailable on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn rename_file_no_replace_impl(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is unavailable on this platform",
    ))
}

#[cfg(not(windows))]
fn replace_file_atomic(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file_atomic(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from = from
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Check whether the given path is a symbolic link.
/// Returns an error when metadata cannot be read so callers can fail-closed.
pub(crate) fn is_symlink(path: &Path) -> crate::shared::error::AppResult<bool> {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .map_err(|e| format!("failed to read metadata {}: {e}", path.display()).into())
}

pub(crate) fn copy_dir_recursive_if_missing(
    src: &Path,
    dst: &Path,
) -> crate::shared::error::AppResult<()> {
    std::fs::create_dir_all(dst).map_err(|e| format!("failed to create {}: {e}", dst.display()))?;

    let entries =
        std::fs::read_dir(src).map_err(|e| format!("failed to read dir {}: {e}", src.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("failed to read dir entry {}: {e}", src.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            copy_dir_recursive_if_missing(&path, &dst_path)?;
            continue;
        }

        if dst_path.exists() {
            continue;
        }

        std::fs::copy(&path, &dst_path).map_err(|e| {
            format!(
                "failed to copy {} -> {}: {e}",
                path.display(),
                dst_path.display()
            )
        })?;
    }

    Ok(())
}

pub(crate) fn copy_file_if_missing(
    src: &Path,
    dst: &Path,
) -> crate::shared::error::AppResult<bool> {
    if dst.exists() {
        return Ok(false);
    }

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }

    std::fs::copy(src, dst)
        .map_err(|e| format!("failed to copy {} -> {}: {e}", src.display(), dst.display()))?;
    Ok(true)
}

/// Open a regular on-disk file without following the final path component.
/// Returns `None` only when the path is missing; other object types and open
/// failures become explicit errors so callers can fail closed.
pub(crate) fn open_regular_file_no_follow(
    path: &Path,
) -> crate::shared::error::AppResult<Option<std::fs::File>> {
    open_regular_file_no_follow_impl(path)
}

#[cfg(unix)]
fn open_regular_file_no_follow_impl(
    path: &Path,
) -> crate::shared::error::AppResult<Option<std::fs::File>> {
    use rustix::fs::{Mode, OFlags};

    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    match rustix::fs::open(path, flags, Mode::empty()) {
        Ok(fd) => {
            let file: std::fs::File = fd.into();
            let stat = rustix::fs::fstat(&file).map_err(|e| {
                format!(
                    "SEC_INVALID_INPUT: failed to inspect {}: {e}",
                    path.display()
                )
            })?;
            if rustix::fs::FileType::from_raw_mode(stat.st_mode)
                != rustix::fs::FileType::RegularFile
            {
                return Err(format!(
                    "SEC_INVALID_INPUT: {} is not a regular file",
                    path.display()
                )
                .into());
            }
            Ok(Some(file))
        }
        Err(err) if err == rustix::io::Errno::NOENT => Ok(None),
        Err(err) if err == rustix::io::Errno::LOOP => Err(format!(
            "SEC_INVALID_INPUT: {} is a symbolic link or reparse point",
            path.display()
        )
        .into()),
        Err(err) => Err(format!(
            "SEC_INVALID_INPUT: failed to open {}: {err}",
            path.display()
        )
        .into()),
    }
}

#[cfg(windows)]
fn open_regular_file_no_follow_impl(
    path: &Path,
) -> crate::shared::error::AppResult<Option<std::fs::File>> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, GetFileType, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK,
    };

    let open_result = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path);

    let file = match open_result {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "SEC_INVALID_INPUT: failed to open {}: {err}",
                path.display()
            )
            .into())
        }
    };

    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: failed to inspect {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )
        .into());
    }
    let info = unsafe { info.assume_init() };
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: {} is not a regular file",
            path.display()
        )
        .into());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "SEC_INVALID_INPUT: {} is a symbolic link or reparse point",
            path.display()
        )
        .into());
    }
    let file_type = unsafe { GetFileType(file.as_raw_handle() as _) };
    if file_type != FILE_TYPE_DISK {
        return Err(format!(
            "SEC_INVALID_INPUT: {} is not a regular disk file",
            path.display()
        )
        .into());
    }
    Ok(Some(file))
}

#[cfg(not(any(unix, windows)))]
fn open_regular_file_no_follow_impl(
    path: &Path,
) -> crate::shared::error::AppResult<Option<std::fs::File>> {
    match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(file) => {
            let metadata = file.metadata().map_err(|e| {
                format!(
                    "SEC_INVALID_INPUT: failed to inspect {}: {e}",
                    path.display()
                )
            })?;
            if !metadata.is_file() {
                return Err(format!(
                    "SEC_INVALID_INPUT: {} is not a regular file",
                    path.display()
                )
                .into());
            }
            Ok(Some(file))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!(
            "SEC_INVALID_INPUT: failed to open {}: {err}",
            path.display()
        )
        .into()),
    }
}

/// Hard-bounded read from an already-opened handle.
///
/// Reads at most `max_len + 1` bytes. Capacity is capped at `max_len` so a
/// growing file cannot force a full allocation of the observed size. The path
/// is never reopened; callers retain authority via the captured handle.
pub(crate) fn read_open_file_with_max_len(
    file: &mut std::fs::File,
    max_len: usize,
) -> crate::shared::error::AppResult<Vec<u8>> {
    use std::io::Read as _;

    let metadata_len = file
        .metadata()
        .map(|meta| meta.len())
        .unwrap_or(0)
        .min(max_len as u64) as usize;
    let take_limit = max_len
        .checked_add(1)
        .ok_or_else(|| "SEC_INVALID_INPUT: file size limit overflow".to_string())?;

    let mut bytes = Vec::with_capacity(metadata_len);
    let mut limited = file.take(take_limit as u64);
    limited
        .read_to_end(&mut bytes)
        .map_err(|e| format!("failed to read open file handle: {e}"))?;
    if bytes.len() > max_len {
        return Err(format!("SEC_INVALID_INPUT: file too large (max {max_len} bytes)").into());
    }
    Ok(bytes)
}

pub(crate) fn read_optional_file_with_max_len(
    path: &Path,
    max_len: usize,
) -> crate::shared::error::AppResult<Option<Vec<u8>>> {
    let Some(mut file) = open_regular_file_no_follow(path)? else {
        return Ok(None);
    };
    let bytes = read_open_file_with_max_len(&mut file, max_len).map_err(|err| {
        let message = err.to_string();
        if message.contains("too large") {
            format!(
                "SEC_INVALID_INPUT: file {} too large (max {max_len} bytes)",
                path.display()
            )
            .into()
        } else if message.starts_with("failed to read open file handle:") {
            format!(
                "failed to read {}: {}",
                path.display(),
                message.trim_start_matches("failed to read open file handle: ")
            )
            .into()
        } else {
            err
        }
    })?;
    Ok(Some(bytes))
}

pub(crate) fn read_file_with_max_len(
    path: &Path,
    max_len: usize,
) -> crate::shared::error::AppResult<Vec<u8>> {
    read_optional_file_with_max_len(path, max_len)?.ok_or_else(|| {
        crate::shared::error::AppError::from(format!(
            "failed to read {}: not found",
            path.display()
        ))
    })
}

fn write_file_atomic_with_replacer(
    path: &Path,
    bytes: &[u8],
    replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> crate::shared::error::AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
    }

    let (tmp_path, mut temp_file) = create_unique_atomic_temp(path)?;
    use std::io::Write as _;
    if let Err(error) = temp_file
        .write_all(bytes)
        .and_then(|()| temp_file.sync_all())
    {
        drop(temp_file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to write temp file {}: {error}", tmp_path.display()).into());
    }
    drop(temp_file);

    if let Err(error) = replace(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to finalize file {}: {error}", path.display()).into());
    }

    Ok(())
}

fn create_unique_atomic_temp(
    target: &Path,
) -> crate::shared::error::AppResult<(std::path::PathBuf, std::fs::File)> {
    use rand::RngCore as _;

    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    for _ in 0..32 {
        let random = rand::thread_rng().next_u64();
        let temp_path = parent.join(format!(
            ".aio-atomic-{}-{random:016x}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to create atomic temp file {}: {error}",
                    temp_path.display()
                )
                .into())
            }
        }
    }
    Err(format!(
        "failed to allocate atomic temp file for {}",
        target.display()
    )
    .into())
}

pub(crate) fn write_file_atomic(path: &Path, bytes: &[u8]) -> crate::shared::error::AppResult<()> {
    write_file_atomic_with_replacer(path, bytes, replace_file_atomic)
}

pub(crate) fn write_file_atomic_create_new(
    path: &Path,
    bytes: &[u8],
) -> crate::shared::error::AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create dir {}: {error}", parent.display()))?;
    }

    let (tmp_path, mut temp_file) = create_unique_atomic_temp(path)?;
    use std::io::Write as _;
    if let Err(error) = temp_file
        .write_all(bytes)
        .and_then(|()| temp_file.sync_all())
    {
        drop(temp_file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to write temp file {}: {error}", tmp_path.display()).into());
    }
    drop(temp_file);

    let activation = rename_file_no_replace(&tmp_path, path);
    match activation {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(crate::shared::error::AppError::new(
                "FS_ALREADY_EXISTS",
                format!("refusing to overwrite existing file {}", path.display()),
            ))
        }
        Err(error) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(format!("failed to activate file {}: {error}", path.display()).into())
        }
    }
}

pub(crate) fn write_file_atomic_if_changed(
    path: &Path,
    bytes: &[u8],
) -> crate::shared::error::AppResult<bool> {
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.len() == bytes.len() as u64 && bytes.len() <= WRITE_IF_CHANGED_COMPARE_MAX_BYTES
        {
            if let Ok(existing) = std::fs::read(path) {
                if existing == bytes {
                    return Ok(false);
                }
            }
        }
    }
    write_file_atomic(path, bytes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TMP_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn unique_tmp_dir() -> std::path::PathBuf {
        let seq = TMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "aio_coding_hub_fs_test_{nanos}_{}_{}",
            std::process::id(),
            seq
        ));
        std::fs::create_dir_all(&dir).expect("create tmp dir");
        dir
    }

    #[test]
    fn unique_tmp_dir_is_unique_across_calls() {
        let a = unique_tmp_dir();
        let b = unique_tmp_dir();
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn read_optional_file_with_max_len_rejects_oversized_files() {
        let dir = unique_tmp_dir();
        let path = dir.join("large.txt");
        std::fs::write(&path, b"hello").expect("write large");

        let err = read_optional_file_with_max_len(&path, 4).expect_err("oversized file fails");

        assert!(err.to_string().contains("too large"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_open_file_with_max_len_accepts_exact_limit_and_arbitrary_bytes() {
        let dir = unique_tmp_dir();
        let path = dir.join("exact.bin");
        let payload = b"\x00secret\xff\n\r\x7f";
        std::fs::write(&path, payload).expect("write exact");

        let mut file = open_regular_file_no_follow(&path)
            .expect("open")
            .expect("exists");
        let got = read_open_file_with_max_len(&mut file, payload.len()).expect("read exact");
        assert_eq!(got, payload);

        let path_over = dir.join("over.bin");
        std::fs::write(&path_over, b"12345").expect("write over");
        let mut over_file = open_regular_file_no_follow(&path_over)
            .expect("open over")
            .expect("exists");
        let err = read_open_file_with_max_len(&mut over_file, 4).expect_err("limit+1 rejects");
        assert!(err.to_string().contains("too large"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_open_file_with_max_len_rejects_growth_after_metadata() {
        let dir = unique_tmp_dir();
        let path = dir.join("grow.bin");
        std::fs::write(&path, b"abcd").expect("write initial");

        let mut file = open_regular_file_no_follow(&path)
            .expect("open")
            .expect("exists");
        // Append after open/metadata would have observed the short size.
        {
            use std::io::Write as _;
            let mut appender = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("append open");
            appender.write_all(b"EFGHIJKLMNOP").expect("append");
        }
        let err = read_open_file_with_max_len(&mut file, 4).expect_err("growth rejects");
        assert!(err.to_string().contains("too large"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_file_with_max_len_uses_captured_handle_after_path_replace() {
        let dir = unique_tmp_dir();
        let path = dir.join("target.bin");
        let replacement = dir.join("replacement.bin");
        std::fs::write(&path, b"original-bytes").expect("write original");
        std::fs::write(&replacement, b"replacement-bytes").expect("write replacement");

        let mut file = open_regular_file_no_follow(&path)
            .expect("open original")
            .expect("exists");
        std::fs::rename(&replacement, &path).expect("replace path");
        let got = read_open_file_with_max_len(&mut file, 64).expect("read captured handle");
        assert_eq!(got, b"original-bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn open_regular_file_no_follow_rejects_symlink_and_directory() {
        let dir = unique_tmp_dir();
        let target = dir.join("target.txt");
        let link = dir.join("link.txt");
        let nested = dir.join("nested");
        std::fs::write(&target, b"target").expect("write target");
        std::fs::create_dir_all(&nested).expect("create nested");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");

        let link_err = open_regular_file_no_follow(&link).expect_err("symlink rejected");
        assert!(link_err.to_string().contains("SEC_INVALID_INPUT"));

        let dir_err = open_regular_file_no_follow(&nested).expect_err("directory rejected");
        assert!(dir_err.to_string().contains("SEC_INVALID_INPUT"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_file_with_max_len_missing_is_error() {
        let dir = unique_tmp_dir();
        let path = dir.join("missing.txt");

        let err = read_file_with_max_len(&path, 4).expect_err("missing file fails");

        assert!(err.to_string().contains("not found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_atomic_creates_parent_and_writes_bytes() {
        let dir = unique_tmp_dir();
        let path = dir.join("a").join("b").join("file.txt");
        write_file_atomic(&path, b"hello").expect("write_file_atomic");
        let got = read_file_with_max_len(&path, 16).expect("read_file_with_max_len");
        assert_eq!(got, b"hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_atomic_preserves_existing_target_when_replace_fails() {
        let dir = unique_tmp_dir();
        let path = dir.join("config.toml");
        std::fs::write(&path, b"original").expect("write original");

        let error = write_file_atomic_with_replacer(&path, b"replacement", |_, _| {
            Err(std::io::Error::other("injected replace failure"))
        })
        .expect_err("replace must fail");

        assert!(error.to_string().contains("injected replace failure"));
        assert_eq!(std::fs::read(&path).expect("read original"), b"original");
        assert!(std::fs::read_dir(&dir)
            .expect("read temp dir")
            .all(|entry| !entry
                .expect("temp entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".aio-atomic-")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_atomic_if_changed_is_false_when_unchanged() {
        let dir = unique_tmp_dir();
        let path = dir.join("file.txt");
        assert!(write_file_atomic_if_changed(&path, b"v1").expect("write"));
        assert!(!write_file_atomic_if_changed(&path, b"v1").expect("write"));
        assert!(write_file_atomic_if_changed(&path, b"v2").expect("write"));
        let got = read_file_with_max_len(&path, 16).expect("read_file_with_max_len");
        assert_eq!(got, b"v2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_atomic_create_new_never_overwrites_existing_target() {
        let dir = unique_tmp_dir();
        let path = dir.join("profile.config.toml");
        write_file_atomic_create_new(&path, b"first").expect("create");
        let error = write_file_atomic_create_new(&path, b"second").expect_err("must not replace");
        assert!(error.to_string().contains("FS_ALREADY_EXISTS"));
        assert_eq!(std::fs::read(&path).expect("read"), b"first");
        assert!(std::fs::read_dir(&dir)
            .expect("read temp dir")
            .all(|entry| !entry
                .expect("temp entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".aio-atomic-")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_file_no_replace_moves_once_and_never_clobbers() {
        let dir = unique_tmp_dir();
        let source = dir.join("source.txt");
        let target = dir.join("target.txt");
        std::fs::write(&source, b"source").expect("write source");
        std::fs::write(&target, b"target").expect("write target");

        let error = rename_file_no_replace(&source, &target).expect_err("target exists");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&source).expect("source preserved"), b"source");
        assert_eq!(std::fs::read(&target).expect("target preserved"), b"target");

        std::fs::remove_file(&target).expect("remove target");
        rename_file_no_replace(&source, &target).expect("move without replacement");
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).expect("moved target"), b"source");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_file_atomic_if_changed_skips_compare_for_oversized_inputs() {
        let dir = unique_tmp_dir();
        let path = dir.join("large.txt");
        let bytes = vec![b'x'; WRITE_IF_CHANGED_COMPARE_MAX_BYTES + 1];
        write_file_atomic(&path, &bytes).expect("write initial large file");

        assert!(
            write_file_atomic_if_changed(&path, &bytes).expect("rewrite oversized file"),
            "oversized inputs should skip full-file equality compare"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_file_if_missing_copies_once() {
        let dir = unique_tmp_dir();
        let src = dir.join("src.txt");
        let dst = dir.join("nested").join("dst.txt");

        std::fs::write(&src, "content").expect("write src");
        assert!(copy_file_if_missing(&src, &dst).expect("copy"));
        assert_eq!(std::fs::read_to_string(&dst).expect("read dst"), "content");

        assert!(!copy_file_if_missing(&src, &dst).expect("copy"));
        assert_eq!(std::fs::read_to_string(&dst).expect("read dst"), "content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_dir_recursive_if_missing_skips_existing_files() {
        let dir = unique_tmp_dir();
        let src_dir = dir.join("src");
        let dst_dir = dir.join("dst");

        std::fs::create_dir_all(src_dir.join("sub")).expect("create src dir");
        std::fs::write(src_dir.join("a.txt"), "src-a").expect("write");
        std::fs::write(src_dir.join("sub").join("b.txt"), "src-b").expect("write");

        std::fs::create_dir_all(&dst_dir).expect("create dst dir");
        std::fs::write(dst_dir.join("a.txt"), "dst-a").expect("write dst override");

        copy_dir_recursive_if_missing(&src_dir, &dst_dir).expect("copy dir");
        assert_eq!(
            std::fs::read_to_string(dst_dir.join("a.txt")).expect("read"),
            "dst-a"
        );
        assert_eq!(
            std::fs::read_to_string(dst_dir.join("sub").join("b.txt")).expect("read"),
            "src-b"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
