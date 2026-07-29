//! Usage: Canonical UUIDv4 generation and validation without an extra dependency.

use rand::RngCore as _;

pub(crate) fn new_uuid_v4() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{0:02x}{1:02x}{2:02x}{3:02x}-{4:02x}{5:02x}-{6:02x}{7:02x}-{8:02x}{9:02x}-{10:02x}{11:02x}{12:02x}{13:02x}{14:02x}{15:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

pub(crate) fn is_canonical_uuid_v4(value: &str) -> bool {
    if value.len() != 36 || value.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return false;
    }

    let bytes = value.as_bytes();
    if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
        return false;
    }
    if bytes[14] != b'4' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {
        return false;
    }

    bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_uuid_is_canonical_v4_and_unique() {
        let first = new_uuid_v4();
        let second = new_uuid_v4();
        assert!(is_canonical_uuid_v4(&first));
        assert!(is_canonical_uuid_v4(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn validator_rejects_noncanonical_or_wrong_version_values() {
        for value in [
            "",
            "550e8400-e29b-41d4-a716-446655440000 ",
            "550E8400-E29B-41D4-A716-446655440000",
            "550e8400-e29b-11d4-a716-446655440000",
            "550e8400-e29b-41d4-c716-446655440000",
            "550e8400e29b41d4a716446655440000",
            "550e8400-e29b-41d4-a716-44665544000z",
        ] {
            assert!(!is_canonical_uuid_v4(value), "accepted {value}");
        }
    }
}
