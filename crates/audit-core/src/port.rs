//! The Audit Store port used by the relay. Runtime-neutral: no async runtime
//! types appear here, only boxed `Send` futures.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::envelope::AuditEnvelope;

/// A boxed `Send` future borrowed for `'a`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An append-only, idempotent Audit Store.
pub trait AuditStore: Send + Sync {
    /// Ingests one validated envelope. Re-ingesting the same event id with the
    /// same source commitment converges to a `Duplicate*` outcome.
    fn ingest<'a>(
        &'a self,
        envelope: &'a AuditEnvelope,
    ) -> BoxFuture<'a, Result<IngestReceipt, StoreError>>;

    /// Checks availability and writability without storing anything.
    fn probe(&self) -> BoxFuture<'_, Result<StoreStatus, StoreError>>;
}

/// How the Store settled an ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IngestOutcome {
    Stored,
    Duplicate,
    DuplicateExpired,
    DuplicateReprojected,
}

impl IngestOutcome {
    pub const ALL: [Self; 4] = [
        Self::Stored,
        Self::Duplicate,
        Self::DuplicateExpired,
        Self::DuplicateReprojected,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Duplicate => "duplicate",
            Self::DuplicateExpired => "duplicate_expired",
            Self::DuplicateReprojected => "duplicate_reprojected",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == value)
    }
}

/// Store receipt. For duplicates, `seq`, `envelope_digest` and
/// `adapter_version` describe the originally stored event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestReceipt {
    pub seq: i64,
    pub envelope_digest: [u8; 32],
    pub outcome: IngestOutcome,
    pub adapter_version: i32,
}

/// Content-free Store status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStatus {
    pub head_seq: i64,
    pub recovery_epoch: i64,
    pub writable: bool,
}

/// External-failure classes (design §6.3). An outage returns the attempt and
/// holds delivery instead of consuming the retry budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutageCode {
    Transport,
    Timeout,
    Connection,
    Resources,
    ReadOnly,
    Shutdown,
    Serialization,
    Deadlock,
    LockUnavailable,
    RecoveryRequired,
    Regressed,
    /// The Store's privilege posture check failed (design §7.3, §11).
    PostureInvalid,
    /// `(source, type, adapter_version)` is not registered in the Store: the
    /// catalog and the Store migration disagree (design §6.3, §7.2).
    VersionSkew,
    /// Any other failure that is not a structured Store verdict (design §6.3:
    /// everything that is not an explicit verdict holds delivery).
    Unclassified,
}

impl OutageCode {
    pub const ALL: [Self; 14] = [
        Self::Transport,
        Self::Timeout,
        Self::Connection,
        Self::Resources,
        Self::ReadOnly,
        Self::Shutdown,
        Self::Serialization,
        Self::Deadlock,
        Self::LockUnavailable,
        Self::RecoveryRequired,
        Self::Regressed,
        Self::PostureInvalid,
        Self::VersionSkew,
        Self::Unclassified,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "store_transport",
            Self::Timeout => "store_timeout",
            Self::Connection => "store_connection",
            Self::Resources => "store_resources",
            Self::ReadOnly => "store_read_only",
            Self::Shutdown => "store_shutdown",
            Self::Serialization => "store_serialization",
            Self::Deadlock => "store_deadlock",
            Self::LockUnavailable => "store_lock_unavailable",
            Self::RecoveryRequired => "store_recovery_required",
            Self::Regressed => "store_regressed",
            Self::PostureInvalid => "store_posture_invalid",
            Self::VersionSkew => "store_unregistered_type",
            Self::Unclassified => "store_unclassified",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == value)
    }
}

/// Classifies a PostgreSQL SQLSTATE into a specific outage class, or `None`
/// when no specific class applies. A Store adapter maps `None` to
/// [`OutageCode::Unclassified`] unless the error is a structured verdict:
/// verdicts are returned as result rows, never as SQL errors.
pub fn classify_sqlstate(sqlstate: &str) -> Option<OutageCode> {
    let bytes = sqlstate.as_bytes();
    if bytes.len() != 5
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
    {
        return None;
    }
    match sqlstate {
        "25006" => Some(OutageCode::ReadOnly),
        "57P01" | "57P02" | "57P03" => Some(OutageCode::Shutdown),
        "57014" => Some(OutageCode::Timeout),
        "40001" => Some(OutageCode::Serialization),
        "40P01" => Some(OutageCode::Deadlock),
        "55P03" => Some(OutageCode::LockUnavailable),
        _ if sqlstate.starts_with("08") => Some(OutageCode::Connection),
        _ if sqlstate.starts_with("53") => Some(OutageCode::Resources),
        _ => None,
    }
}

/// A bounded machine code (`[a-z0-9_]{1,64}`) returned by the Store's own
/// validation. It cannot carry payload text.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundedCode(String);

impl BoundedCode {
    pub fn new(code: &str) -> Option<Self> {
        let ok = !code.is_empty()
            && code.len() <= 64
            && code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        ok.then(|| Self(code.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BoundedCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for BoundedCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Store failures, split into outages (hold), unknown outcomes (retry and
/// reconcile) and terminal verdicts (quarantine).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("audit store outage: {}", code.as_str())]
    Outage { code: OutageCode },
    #[error("audit store outcome unknown")]
    Unknown,
    #[error("audit store conflict")]
    Conflict,
    #[error("audit store rejected the envelope: {code}")]
    Rejected { code: BoundedCode },
    #[error("audit store requires recovery")]
    RecoveryRequired,
    #[error("audit store regressed behind acknowledged receipts")]
    Regressed,
}

impl StoreError {
    /// Whether the relay should return the attempt and hold delivery.
    pub const fn is_outage(&self) -> bool {
        self.outage_code().is_some()
    }

    /// The outage class, if this error is an outage.
    pub const fn outage_code(&self) -> Option<OutageCode> {
        match self {
            Self::Outage { code } => Some(*code),
            Self::RecoveryRequired => Some(OutageCode::RecoveryRequired),
            Self::Regressed => Some(OutageCode::Regressed),
            Self::Unknown | Self::Conflict | Self::Rejected { .. } => None,
        }
    }

    /// Whether the failure is a definite verdict that quarantines the event.
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Conflict | Self::Rejected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlstate_classification_follows_design_6_3() {
        for (state, code) in [
            ("08006", OutageCode::Connection),
            ("08001", OutageCode::Connection),
            ("53300", OutageCode::Resources),
            ("53100", OutageCode::Resources),
            ("25006", OutageCode::ReadOnly),
            ("57P01", OutageCode::Shutdown),
            ("57P02", OutageCode::Shutdown),
            ("57P03", OutageCode::Shutdown),
            ("57014", OutageCode::Timeout),
            ("40001", OutageCode::Serialization),
            ("40P01", OutageCode::Deadlock),
            ("55P03", OutageCode::LockUnavailable),
        ] {
            assert_eq!(classify_sqlstate(state), Some(code), "{state}");
        }
        for state in [
            "23505", "22023", "42501", "P0001", "55000", "57P04", "", "0800", "08x01",
        ] {
            assert_eq!(classify_sqlstate(state), None, "{state}");
        }
    }

    #[test]
    fn outage_classification() {
        assert!(
            StoreError::Outage {
                code: OutageCode::Transport
            }
            .is_outage()
        );
        assert!(StoreError::RecoveryRequired.is_outage());
        assert!(StoreError::Regressed.is_outage());
        assert_eq!(
            StoreError::Regressed.outage_code(),
            Some(OutageCode::Regressed)
        );
        assert!(!StoreError::Unknown.is_outage());
        assert!(!StoreError::Unknown.is_terminal());
        assert!(!StoreError::Conflict.is_outage());
        assert!(StoreError::Conflict.is_terminal());
        let rejected = StoreError::Rejected {
            code: BoundedCode::new("invalid_envelope").expect("bounded"),
        };
        assert!(rejected.is_terminal() && !rejected.is_outage());
    }

    #[test]
    fn bounded_code_refuses_text() {
        assert!(BoundedCode::new("conflict_v1").is_some());
        assert!(BoundedCode::new("").is_none());
        assert!(BoundedCode::new("has space").is_none());
        assert!(BoundedCode::new("Upper").is_none());
        assert!(BoundedCode::new(&"a".repeat(65)).is_none());
    }

    #[test]
    fn names_round_trip() {
        for outcome in IngestOutcome::ALL {
            assert_eq!(IngestOutcome::parse(outcome.as_str()), Some(outcome));
        }
        for code in OutageCode::ALL {
            assert_eq!(OutageCode::parse(code.as_str()), Some(code));
        }
    }
}
