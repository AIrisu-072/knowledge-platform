//! Errors of the administrative client. Messages carry codes, SQLSTATEs and
//! the Store's own refusal codes only, never audit content.

use audit_core::OutageCode;

/// Why an administrative call did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdminError {
    /// The Store refused and recorded `audit.access.denied` (or a bounded
    /// precondition such as `already_bound` / `not_pending`).
    #[error("audit store refused the call: {code}")]
    Denied { code: String },
    /// A disclosure, state or precondition check failed (not recorded):
    /// e.g. `access_revoked`, `access_reapply_pending`, `not_in_recovery`.
    #[error("audit store rejected the call: {code}")]
    Rejected { code: String },
    /// A transient refusal the caller may retry (`intent_not_durable`).
    #[error("audit store asked to retry: {code}")]
    Retryable { code: String },
    /// The Store is in recovery mode (design §11).
    #[error("audit store requires recovery")]
    RecoveryRequired,
    /// The privilege posture check failed.
    #[error("audit store posture is invalid")]
    PostureInvalid,
    /// The database refused (e.g. 42501 for a function outside the role).
    #[error("database error {sqlstate}: {message}")]
    Database { sqlstate: String, message: String },
    /// Transport, pool or timeout failure.
    #[error("audit store unavailable: {}", code.as_str())]
    Unavailable { code: OutageCode },
    /// The Store answered with a shape this client does not understand.
    #[error("unexpected audit store response: {0}")]
    Protocol(&'static str),
}

impl AdminError {
    /// The SQLSTATE of a database refusal.
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::Database { sqlstate, .. } => Some(sqlstate),
            Self::RecoveryRequired => Some("KA001"),
            Self::PostureInvalid => Some("KA002"),
            _ => None,
        }
    }

    /// The Store's refusal code, if any.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Denied { code } | Self::Rejected { code } | Self::Retryable { code } => {
                Some(code)
            }
            _ => None,
        }
    }
}

impl From<sqlx::Error> for AdminError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(db) => {
                let sqlstate = db.code().map(|c| c.into_owned()).unwrap_or_default();
                let message = db.message();
                match sqlstate.as_str() {
                    "KA001" => Self::RecoveryRequired,
                    "KA002" => Self::PostureInvalid,
                    // Refusals raised by the Store's functions carry a bounded code.
                    "42501" | "55000" if is_code(message) => Self::Rejected {
                        code: message.to_owned(),
                    },
                    "40001" if is_code(message) => Self::Retryable {
                        code: message.to_owned(),
                    },
                    _ => Self::Database {
                        sqlstate,
                        message: message.to_owned(),
                    },
                }
            }
            _ => match crate::store::classify_sqlx_error(&error).outage_code() {
                Some(code) => Self::Unavailable { code },
                None => Self::Unavailable {
                    code: OutageCode::Other,
                },
            },
        }
    }
}

fn is_code(text: &str) -> bool {
    audit_core::port::BoundedCode::new(text).is_some()
}
