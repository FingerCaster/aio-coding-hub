//! Usage: Security-related helpers (token masking, constant-time comparison).

/// Mask a token for safe logging: show first 6 + last 4 chars, hide middle with
/// a fixed 8 asterisks (does NOT reveal actual token length).
///
/// Uses char-based indexing to avoid panics on (hypothetical) non-ASCII tokens.
#[cfg(test)]
pub(crate) fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 10 {
        // Too short to show any real characters — return fixed-length mask.
        return "**********".to_string();
    }
    let prefix: String = chars[..6].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    // Fixed 8 asterisks regardless of token length to avoid revealing length.
    format!("{prefix}********…{suffix}")
}

/// Constant-time byte comparison to prevent timing attacks.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_short_token() {
        assert_eq!(mask_token("abc"), "**********");
    }

    #[test]
    fn mask_long_token() {
        let masked = mask_token("sk-ant-1234567890abcdef");
        assert!(masked.starts_with("sk-ant"));
        assert!(masked.ends_with("cdef"));
    }

    #[test]
    fn mask_long_token_has_fixed_length() {
        let a = mask_token("sk-ant-1234567890abcdef");
        let b = mask_token("sk-ant-1234567890abcdef1234567890");
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hi", b"hello"));
    }
}
