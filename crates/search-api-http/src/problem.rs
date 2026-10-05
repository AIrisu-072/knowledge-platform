//! The one Search Problem writer (spec/errors/search-api-error-registry.yaml).
//!
//! RFC 9457 `about:blank`, fixed reason title and fixed detail per code, the
//! declared `code` extension and `trace_id`, `instance` omitted, and every
//! Problem carries `Cache-Control: private, no-store` and
//! `X-Content-Type-Options: nosniff`. A 401 carries only the validated
//! challenge of the wired scheme.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use search_application::api_scope::ApiError;
use serde::Serialize;
use uuid::Uuid;

use crate::auth::ValidatedChallenge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemCode {
    MalformedRequest,
    AuthenticationRequired,
    Forbidden,
    ResourceNotFound,
    CursorStale,
    PayloadTooLarge,
    UnsupportedMediaType,
    ValidationFailed,
    RateLimited,
    RequestHeadersTooLarge,
    IdentityUnavailable,
    DependencyUnavailable,
    ServiceUnavailable,
    UpstreamTimeout,
}

impl ProblemCode {
    pub const ALL: [Self; 14] = [
        Self::MalformedRequest,
        Self::AuthenticationRequired,
        Self::Forbidden,
        Self::ResourceNotFound,
        Self::CursorStale,
        Self::PayloadTooLarge,
        Self::UnsupportedMediaType,
        Self::ValidationFailed,
        Self::RateLimited,
        Self::RequestHeadersTooLarge,
        Self::IdentityUnavailable,
        Self::DependencyUnavailable,
        Self::ServiceUnavailable,
        Self::UpstreamTimeout,
    ];

    /// `(status, code, title, detail)` exactly as registered.
    pub const fn registry(self) -> (u16, &'static str, &'static str, &'static str) {
        match self {
            Self::MalformedRequest => (
                400,
                "MALFORMED_REQUEST",
                "Bad Request",
                "The request could not be parsed.",
            ),
            Self::AuthenticationRequired => (
                401,
                "AUTHENTICATION_REQUIRED",
                "Unauthorized",
                "Authentication is required.",
            ),
            Self::Forbidden => (
                403,
                "FORBIDDEN",
                "Forbidden",
                "The operation is not permitted.",
            ),
            Self::ResourceNotFound => (
                404,
                "RESOURCE_NOT_FOUND",
                "Not Found",
                "The resource was not found.",
            ),
            Self::CursorStale => (
                409,
                "CURSOR_STALE",
                "Conflict",
                "The cursor is unavailable.",
            ),
            Self::PayloadTooLarge => (
                413,
                "PAYLOAD_TOO_LARGE",
                "Content Too Large",
                "The request content is too large.",
            ),
            Self::UnsupportedMediaType => (
                415,
                "UNSUPPORTED_MEDIA_TYPE",
                "Unsupported Media Type",
                "The request media type is unsupported.",
            ),
            Self::ValidationFailed => (
                422,
                "VALIDATION_FAILED",
                "Unprocessable Content",
                "The request fields are invalid.",
            ),
            Self::RateLimited => (
                429,
                "RATE_LIMITED",
                "Too Many Requests",
                "The request rate limit was reached.",
            ),
            Self::RequestHeadersTooLarge => (
                431,
                "REQUEST_HEADERS_TOO_LARGE",
                "Request Header Fields Too Large",
                "The request headers are too large.",
            ),
            Self::IdentityUnavailable => (
                503,
                "IDENTITY_UNAVAILABLE",
                "Service Unavailable",
                "A required service is unavailable.",
            ),
            Self::DependencyUnavailable => (
                503,
                "DEPENDENCY_UNAVAILABLE",
                "Service Unavailable",
                "A required service is unavailable.",
            ),
            Self::ServiceUnavailable => (
                503,
                "SERVICE_UNAVAILABLE",
                "Service Unavailable",
                "The service could not complete the operation.",
            ),
            Self::UpstreamTimeout => (
                504,
                "UPSTREAM_TIMEOUT",
                "Gateway Timeout",
                "An upstream service did not respond in time.",
            ),
        }
    }
}

impl From<ApiError> for ProblemCode {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::MalformedRequest => Self::MalformedRequest,
            ApiError::AuthenticationRequired => Self::AuthenticationRequired,
            ApiError::Forbidden => Self::Forbidden,
            ApiError::ResourceNotFound => Self::ResourceNotFound,
            ApiError::CursorStale => Self::CursorStale,
            ApiError::ValidationFailed => Self::ValidationFailed,
            ApiError::IdentityUnavailable => Self::IdentityUnavailable,
            ApiError::DependencyUnavailable => Self::DependencyUnavailable,
            ApiError::ServiceUnavailable => Self::ServiceUnavailable,
            ApiError::UpstreamTimeout => Self::UpstreamTimeout,
        }
    }
}

/// A public request field pointer and fixed code (422 only).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FieldError {
    pub pointer: &'static str,
    pub code: &'static str,
}

#[derive(Serialize)]
struct ProblemBody<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    detail: &'static str,
    code: &'static str,
    trace_id: String,
    #[serde(skip_serializing_if = "<[FieldError]>::is_empty")]
    errors: &'a [FieldError],
}

pub const PRIVATE_NO_STORE: &str = "private, no-store";
pub const NO_SNIFF: &str = "nosniff";

pub fn problem(
    code: ProblemCode,
    trace_id: Uuid,
    challenge: Option<&ValidatedChallenge>,
    errors: &[FieldError],
) -> Response {
    let (status, code_text, title, detail) = code.registry();
    let errors = if code == ProblemCode::ValidationFailed {
        &errors[..errors.len().min(16)]
    } else {
        &[]
    };
    let body = serde_json::to_vec(&ProblemBody {
        kind: "about:blank",
        title,
        status,
        detail,
        code: code_text,
        trace_id: trace_id.hyphenated().to_string(),
        errors,
    })
    .unwrap_or_default();
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE))
        .header(header::CONTENT_TYPE, "application/problem+json")
        .header(header::CACHE_CONTROL, PRIVATE_NO_STORE)
        .header(header::X_CONTENT_TYPE_OPTIONS, NO_SNIFF);
    if code == ProblemCode::AuthenticationRequired
        && let Some(challenge) = challenge
        && let Ok(value) = HeaderValue::from_str(challenge.as_str())
    {
        builder = builder.header(header::WWW_AUTHENTICATE, value);
    }
    builder.body(Body::from(body)).unwrap_or_else(|_| {
        let mut fallback = Response::new(Body::empty());
        *fallback.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
        fallback
    })
}
