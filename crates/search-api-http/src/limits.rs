//! v0 request/response bounds (spec/api/search-openapi.yaml `x-search-v0-limits`).

use axum::http::{HeaderMap, header};

pub const REQUEST_HEADER_BYTES: usize = 16 * 1024;
pub const POST_JSON_BODY_BYTES: usize = 16 * 1024;
pub const SUCCESS_BODY_BYTES: usize = 1024 * 1024;
pub const DEFAULT_PAGE_SIZE: usize = 20;

/// Bytes of all header names and values, with framing.
pub fn header_bytes(headers: &HeaderMap) -> usize {
    headers
        .iter()
        .map(|(name, value)| name.as_str().len() + value.len() + 4)
        .sum()
}

/// `application/json`, optionally with parameters (e.g. charset).
pub fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}
