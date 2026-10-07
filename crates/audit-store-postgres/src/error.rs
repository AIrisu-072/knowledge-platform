//! Errors of the administrative client. Messages carry codes, SQLSTATEs and
//! the Store's own refusal codes only, never audit content.

use audit_core::OutageCode;

/// Why an administrative call did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdminError {
    /// The Store refused and recorded `audit.access.denied` (or a bounded
    /// precondition such as `already_bound`).
    #[error("audit store refused the call: {code}")]
    Denied { code: String },
    /// A disclosure or precondition check failed (not recorded).
    #[error("audit store rejected the call: {code}")]
    Rejected { code: String },
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
            Self::Denied { code } | Self::Rejected { code } => Some(code),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for AdminError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(db) => {
                let sqlstate = db.code().map(|c| c.into_owned()).unwrap_or_default();
                match sqlstate.as_str() {
                    "KA001" => Self::RecoveryRequired,
                    "KA002" => Self::PostureInvalid,
                    // Disclosure refusals raised by resolve_intent/read_page carry a code.
                    "42501" if is_code(db.message()) => Self::Rejected {
                        code: db.message().to_owned(),
                    },
                    _ => Self::Database {
                        sqlstate,
                        message: db.message().to_owned(),
                    },
                }
            }
            _ => match crate::store::classify_sqlx_error(&error) {
                audit_core::StoreError::Outage { code } => Self::Unavailable { code },
                _ => Self::Unavailable {
                    code: OutageCode::Unclassified,
                },
            },
        }
    }
}

fn is_code(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
