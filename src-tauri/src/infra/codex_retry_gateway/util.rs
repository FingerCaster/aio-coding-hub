use rand::RngCore;
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
}
