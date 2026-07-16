use rand::RngCore;
use std::fs::Metadata;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub(crate) fn now_rfc3339() -> String {
    let dt: chrono::DateTime<chrono::Utc> = SystemTime::now().into();
    dt.to_rfc3339()
}

pub(crate) fn random_hex(bytes_len: usize) -> String {
    let mut bytes = vec![0_u8; bytes_len];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub(crate) fn normalize_full_sha(value: &str) -> crate::shared::error::AppResult<String> {
    let sha = value.trim().to_ascii_lowercase();
    if sha.len() != 40
        || !sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("SEC_INVALID_INPUT: commit must be a canonical 40-hex SHA".into());
    }
    Ok(sha)
}

pub(crate) fn normalized_internal_relative_path(
    value: &str,
) -> crate::shared::error::AppResult<PathBuf> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(normalized.trim());
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("SEC_INVALID_INPUT: invalid managed relative path".into());
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => clean.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("SEC_INVALID_INPUT: invalid managed relative path".into());
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("SEC_INVALID_INPUT: invalid managed relative path".into());
    }
    Ok(clean)
}

pub(crate) fn ensure_path_within_root(
    root: &Path,
    candidate: &Path,
) -> crate::shared::error::AppResult<()> {
    let root = std::fs::canonicalize(root)
        .map_err(|err| format!("failed to canonicalize {}: {err}", root.display()))?;
    let candidate = std::fs::canonicalize(candidate)
        .map_err(|err| format!("failed to canonicalize {}: {err}", candidate.display()))?;
    if !candidate.starts_with(&root) {
        return Err("SEC_INVALID_INPUT: path escapes managed root".into());
    }
    Ok(())
}

pub(crate) fn metadata_is_symlink_or_reparse(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

pub(crate) fn symlink_metadata_fail_closed(
    path: &Path,
    label: &str,
) -> crate::shared::error::AppResult<Metadata> {
    std::fs::symlink_metadata(path)
        .map_err(|err| format!("failed to read {label} metadata {}: {err}", path.display()).into())
}

pub(crate) fn ensure_not_symlink_or_reparse(
    path: &Path,
    label: &str,
) -> crate::shared::error::AppResult<Metadata> {
    let metadata = symlink_metadata_fail_closed(path, label)?;
    if metadata_is_symlink_or_reparse(&metadata) {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} must not be a symbolic link or reparse point: {}",
            path.display()
        )
        .into());
    }
    Ok(metadata)
}

pub(crate) fn create_or_validate_plain_directory(
    path: &Path,
    label: &str,
) -> crate::shared::error::AppResult<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata_is_symlink_or_reparse(&metadata) {
                return Err(format!(
                    "SEC_INVALID_INPUT: {label} must not be a symbolic link or reparse point: {}",
                    path.display()
                )
                .into());
            }
            if !metadata.is_dir() {
                return Err(format!(
                    "SEC_INVALID_INPUT: {label} must be a directory: {}",
                    path.display()
                )
                .into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path)
                .map_err(|error| format!("failed to create {label} {}: {error}", path.display()))?;
            let metadata = ensure_not_symlink_or_reparse(path, label)?;
            if !metadata.is_dir() {
                return Err(format!(
                    "SEC_INVALID_INPUT: {label} must be a directory: {}",
                    path.display()
                )
                .into());
            }
            Ok(())
        }
        Err(error) => Err(format!(
            "failed to read {label} metadata {}: {error}",
            path.display()
        )
        .into()),
    }
}

pub(crate) fn canonicalize_path_within_root(
    root: &Path,
    candidate: &Path,
    label: &str,
) -> crate::shared::error::AppResult<PathBuf> {
    let _ = ensure_not_symlink_or_reparse(candidate, label)?;
    let root = std::fs::canonicalize(root)
        .map_err(|err| format!("failed to canonicalize {}: {err}", root.display()))?;
    let candidate = std::fs::canonicalize(candidate)
        .map_err(|err| format!("failed to canonicalize {}: {err}", candidate.display()))?;
    if !candidate.starts_with(&root) {
        return Err(format!(
            "SEC_INVALID_INPUT: {label} escapes managed root {}",
            candidate.display()
        )
        .into());
    }
    Ok(candidate)
}

pub(crate) fn is_loopback_host(host: &str) -> bool {
    let trimmed = host.trim().trim_matches(['[', ']']);
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    trimmed
        .parse::<IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

pub(crate) fn strip_trailing_v1(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/v1")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalize_full_sha_requires_canonical_40_hex() {
        assert_eq!(
            normalize_full_sha("EF7FC5A0F9DA125B91431CD99BCF6FD9387A53B2").unwrap(),
            "ef7fc5a0f9da125b91431cd99bcf6fd9387a53b2"
        );
        assert!(normalize_full_sha("1234").is_err());
        assert!(normalize_full_sha("g123456789012345678901234567890123456789").is_err());
    }

    #[test]
    fn normalized_internal_relative_path_rejects_escape() {
        assert!(normalized_internal_relative_path("../evil").is_err());
        assert!(normalized_internal_relative_path("C:/evil").is_err());
        assert_eq!(
            normalized_internal_relative_path("runtime/config/config.json").unwrap(),
            PathBuf::from("runtime").join("config").join("config.json")
        );
    }

    #[test]
    fn is_loopback_host_accepts_localhost_and_ips() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("0.0.0.0"));
    }

    #[test]
    fn strip_trailing_v1_removes_single_suffix() {
        assert_eq!(
            strip_trailing_v1("http://127.0.0.1:37123/v1"),
            "http://127.0.0.1:37123"
        );
        assert_eq!(
            strip_trailing_v1("http://127.0.0.1:37123"),
            "http://127.0.0.1:37123"
        );
    }

    #[test]
    fn canonicalize_path_within_root_rejects_escape() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("root");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("value.txt");
        std::fs::write(&file, "ok").unwrap();
        let escaped = dir.path().join("outside.txt");
        std::fs::write(&escaped, "no").unwrap();
        assert!(canonicalize_path_within_root(&root, &file, "managed file").is_ok());
        assert!(canonicalize_path_within_root(&root, &escaped, "managed file").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_not_symlink_or_reparse_rejects_symlink() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        std::fs::write(&target, "ok").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(ensure_not_symlink_or_reparse(&link, "managed file").is_err());
    }
}
