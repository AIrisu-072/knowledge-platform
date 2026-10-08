//! Minimal lowercase hex helpers (no extra dependency).

/// Lowercase hex rendering.
pub fn encode(bytes: &[u8]) -> String {
    audit_core::chain::to_hex(bytes)
}

/// Decodes lowercase or uppercase hex of even length.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[high, low]| Some((nibble(high)? << 4) | nibble(low)?))
        .collect()
}

/// Decodes exactly 32 bytes of lowercase hex.
pub fn decode32(text: &str) -> Option<[u8; 32]> {
    audit_core::chain::parse_hex32(text)
}

/// Converts a database `bytea` digest into a fixed array.
pub fn digest32(bytes: &[u8]) -> Option<[u8; 32]> {
    bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_rejections() {
        let bytes = [0x00, 0x7f, 0xab, 0xff];
        assert_eq!(encode(&bytes), "007fabff");
        assert_eq!(decode("007fabff"), Some(bytes.to_vec()));
        assert_eq!(decode("007FABFF"), Some(bytes.to_vec()));
        assert_eq!(decode("abc"), None);
        assert_eq!(decode("zz"), None);
        assert_eq!(decode32(&"ab".repeat(32)), Some([0xab; 32]));
        assert_eq!(decode32(&"AB".repeat(32)), None);
        assert_eq!(digest32(&[1; 32]), Some([1; 32]));
        assert_eq!(digest32(&[1; 31]), None);
    }
}
