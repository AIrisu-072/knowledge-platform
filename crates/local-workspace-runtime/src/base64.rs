//! Standard padded Base64 for the JSON IPC wire (RFC 4648 section 4).

use crate::error::{RuntimeError, RuntimeErrorCode, RuntimeErrorReason, RuntimeResult};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Strict decode: canonical padding only, no whitespace, bounded output.
pub fn decode(text: &str, max_bytes: usize) -> RuntimeResult<Vec<u8>> {
    let invalid = || RuntimeError::with(RuntimeErrorCode::InvalidLocator, RuntimeErrorReason::Io);
    let input = text.as_bytes();
    if !input.len().is_multiple_of(4) {
        return Err(invalid());
    }
    if input.len() / 4 * 3 > max_bytes + 2 {
        return Err(RuntimeError::with(
            RuntimeErrorCode::Limit,
            RuntimeErrorReason::TooLarge,
        ));
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let chunks = input.len() / 4;
    for (index, chunk) in input.chunks(4).enumerate() {
        let last = index + 1 == chunks;
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err(invalid());
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = (n << 6) | value(c).ok_or_else(invalid)?;
        }
        n <<= 6 * pad as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        // Reject non-canonical trailing bits.
        if (pad == 1 && bytes[2] != 0) || (pad == 2 && (bytes[1] != 0 || bytes[2] != 0)) {
            return Err(invalid());
        }
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    if out.len() > max_bytes {
        return Err(RuntimeError::with(
            RuntimeErrorCode::Limit,
            RuntimeErrorReason::TooLarge,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rfc4648_vectors() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded, 16).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all), 256).unwrap(), all);
    }

    #[test]
    fn rejects_non_canonical_and_oversized_input() {
        for bad in ["Zg=", "Zh==", "Z===", "Zm9v\n", "Zg==Zg==", "Zm9-", "Zm 9"] {
            assert!(decode(bad, 16).is_err(), "{bad:?}");
        }
        assert_eq!(
            decode("Zm9vYmFy", 5).unwrap_err().code,
            RuntimeErrorCode::Limit
        );
    }
}
