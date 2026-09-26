//! Private staging files and atomic replacement for native secret-bearing data.
use crate::shared::error::{AppError, AppResult};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

fn io_error() -> AppError {
    AppError::new("NATIVE_WRITE_FAILED", "Cannot safely persist the native model document; original data or its private backup is retained")
}

pub(crate) fn create_directory(path: &Path) -> AppResult<()> {
    if path.is_dir() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|_| io_error())
}

fn create_private(path: &Path) -> AppResult<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // GENERIC_WRITE | READ_CONTROL | WRITE_DAC: restrict before any bytes.
        options.access_mode(0x40000000 | 0x00020000 | 0x00040000);
    }
    let file = options.open(path).map_err(|_| io_error())?;
    #[cfg(windows)]
    if restrict_windows_file(&file).is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(io_error());
    }
    Ok(file)
}

#[cfg(windows)]
fn restrict_windows_file(file: &File) -> AppResult<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        SetKernelObjectSecurity, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    // Protected ACL: only the file owner and SYSTEM can read/write.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io_error());
        }
        let result = SetKernelObjectSecurity(
            file.as_raw_handle(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        LocalFree(descriptor);
        if result == 0 {
            return Err(io_error());
        }
    }
    Ok(())
}

pub(crate) fn backup(path: &Path, bytes: &[u8]) -> AppResult<PathBuf> {
    let parent = path.parent().ok_or_else(io_error)?;
    let directory = parent.join(".aio-native-backups");
    if let Ok(meta) = std::fs::symlink_metadata(&directory) {
        if super::targets::is_link(&meta) || !meta.is_dir() {
            return Err(io_error());
        }
    }
    create_directory(&directory)?;
    // Never reuse an externally supplied, potentially world-readable backup.
    let destination = directory.join(format!(
        "{}-{}.original",
        super::document::digest_bytes(bytes),
        crate::shared::uuid::new_uuid_v4()
    ));
    atomic_write(&destination, bytes, false, || Ok(()))?;
    Ok(destination)
}

pub(crate) fn atomic_write(
    path: &Path,
    bytes: &[u8],
    replace_existing: bool,
    before_replace: impl FnOnce() -> AppResult<()>,
) -> AppResult<()> {
    let parent = path.parent().ok_or_else(io_error)?;
    create_directory(parent)?;
    let temporary = parent.join(format!(
        ".aio-native-{}.tmp",
        crate::shared::uuid::new_uuid_v4()
    ));
    let mut file = create_private(&temporary)?;
    let staged = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| io_error());
    drop(file); // Windows rename must not retain an open writer.
    let result = staged.and_then(|_| before_replace()).and_then(|_| {
        if replace_existing {
            replace(&temporary, path).map_err(|_| io_error())
        } else {
            crate::shared::fs::rename_file_no_replace(&temporary, path).map_err(|_| io_error())
        }
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
