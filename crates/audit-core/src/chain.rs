//! Write-time hash chain (decision D3). The Store computes the same values in
//! SQL; the vectors in the tests and in `spec/telemetry/README.md` pin both.
//!
//! - `envelope_digest = sha256(envelope text bytes)` where the text is the
//!   PostgreSQL `jsonb::text` rendering (`kp-audit-jsonb-sha256-v1`).
//! - `chain(seq) = sha256("kp-audit-chain-v1" || prev_chain || int8be(seq) ||
//!   uuid bytes(event_id) || envelope_digest)`.
//! - `GENESIS = sha256("kp-audit-chain-genesis-v1")` is `prev_chain` of seq 1.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Preimage of [`GENESIS`].
pub const GENESIS_PREIMAGE: &[u8] = b"kp-audit-chain-genesis-v1";
/// Domain separator prefixed to every chain step.
pub const CHAIN_DOMAIN: &[u8] = b"kp-audit-chain-v1";
/// Name of the envelope digest algorithm recorded by the Store.
pub const DIGEST_ALGORITHM: &str = "kp-audit-jsonb-sha256-v1";

/// `sha256("kp-audit-chain-genesis-v1")` =
/// `9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344`.
pub const GENESIS: [u8; 32] = [
    0x9a, 0xe4, 0xe1, 0xd7, 0x94, 0x2c, 0xe3, 0x18, 0x77, 0x0d, 0xe8, 0x97, 0xb2, 0xa3, 0x36, 0xed,
    0xc2, 0xa9, 0xe7, 0x00, 0xf8, 0x73, 0x3f, 0x65, 0x8d, 0xcd, 0xc2, 0xe5, 0x63, 0xaf, 0xf3, 0x44,
];

/// SHA-256 of the exact envelope text bytes.
pub fn envelope_digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

/// The next chain value.
pub fn chain_next(
    prev: &[u8; 32],
    seq: i64,
    event_id: Uuid,
    envelope_digest: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(CHAIN_DOMAIN);
    hasher.update(prev);
    hasher.update(seq.to_be_bytes());
    hasher.update(event_id.as_bytes());
    hasher.update(envelope_digest);
    hasher.finalize().into()
}

/// Lowercase hex rendering.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Parses exactly 64 lowercase hex characters.
pub fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    if !crate::kinds::is_hex_digest(text) {
        return None;
    }
    let nibble = |b: u8| {
        if b.is_ascii_digit() {
            b - b'0'
        } else {
            b - b'a' + 10
        }
    };
    let bytes = text.as_bytes();
    let mut out = [0_u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = (nibble(bytes[2 * i]) << 4) | nibble(bytes[2 * i + 1]);
    }
    Some(out)
}
