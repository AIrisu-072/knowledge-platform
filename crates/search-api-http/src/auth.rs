//! The v0 Search authentication binding: HTTP Bearer.
//!
//! A request credential is verified by the host's `SearchCredentialVerifierPort`
//! into an opaque server-issued session handle; only that handle reaches the
//! application's authority. The 401 challenge is the wired scheme's fixed
//! `Bearer realm="search"`; an empty, invalid or unwired challenge refuses
//! startup.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::http::{HeaderMap, header};
use search_application::scoped::AccessContextHandle;

use crate::router::SearchOperation;

/// The only challenge this binding accepts.
pub const BEARER_CHALLENGE: &str = "Bearer realm=\"search\"";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    /// The verifier or its identity provider could not answer.
    Unavailable,
}

pub type CredentialFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<AccessContextHandle>, CredentialError>> + Send + 'a>>;

/// Host credential verification: a Bearer token becomes a verified opaque
/// session handle, or nothing. It never returns request-supplied identity.
pub trait SearchCredentialVerifierPort: Send + Sync {
    fn verify<'a>(&'a self, token: &'a str) -> CredentialFuture<'a>;
}

/// A challenge value proven to be the fixed v0 Bearer challenge.
#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedChallenge(String);

impl fmt::Debug for ValidatedChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidatedChallenge")
    }
}

impl ValidatedChallenge {
    pub fn parse(value: &str) -> Result<Self, AuthConfigurationError> {
        if value != BEARER_CHALLENGE {
            return Err(AuthConfigurationError::InvalidChallenge);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthConfigurationError {
    EmptyChallenge,
    InvalidChallenge,
    InvalidScheme,
}

/// Supplies the challenges of each operation; validated at startup.
pub trait SearchAuthChallengePort: Send + Sync {
    fn challenges(&self, operation: SearchOperation) -> Vec<String>;
}

/// The fixed v0 challenge for every operation.
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticBearerChallenge;

impl SearchAuthChallengePort for StaticBearerChallenge {
    fn challenges(&self, _operation: SearchOperation) -> Vec<String> {
        vec![BEARER_CHALLENGE.to_owned()]
    }
}

/// The OpenAPI security scheme and its challenge source, as one binding.
#[derive(Clone)]
pub struct SearchAuthSchemeBinding {
    pub scheme_id: String,
    pub security_scheme: String,
    pub challenge_port: Arc<dyn SearchAuthChallengePort>,
}

impl fmt::Debug for SearchAuthSchemeBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchAuthSchemeBinding(<wired>)")
    }
}

impl SearchAuthSchemeBinding {
    pub fn bearer(challenge_port: Arc<dyn SearchAuthChallengePort>) -> Self {
        Self {
            scheme_id: "SearchBearer".into(),
            security_scheme: "bearer".into(),
            challenge_port,
        }
    }

    /// The single validated challenge, identical for all four operations.
    pub fn validate(&self) -> Result<ValidatedChallenge, AuthConfigurationError> {
        if self.scheme_id != "SearchBearer" || self.security_scheme != "bearer" {
            return Err(AuthConfigurationError::InvalidScheme);
        }
        let mut validated: Option<ValidatedChallenge> = None;
        for operation in SearchOperation::ALL {
            let challenges = self.challenge_port.challenges(operation);
            if challenges.is_empty() {
                return Err(AuthConfigurationError::EmptyChallenge);
            }
            for challenge in challenges {
                let parsed = ValidatedChallenge::parse(&challenge)?;
                if validated.as_ref().is_some_and(|known| known != &parsed) {
                    return Err(AuthConfigurationError::InvalidChallenge);
                }
                validated = Some(parsed);
            }
        }
        validated.ok_or(AuthConfigurationError::EmptyChallenge)
    }
}

/// The token of exactly one well-formed `Authorization: Bearer` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || token.len() > 4_096
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return None;
    }
    Some(token)
}
