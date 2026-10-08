//! Closed failure provenance only; no source identifiers or error payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Identity {
    Requester,
    Provider,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Identity,
    Revision,
    Files,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    IdentityUnavailable,
    Timeout,
    SourceMismatch,
    Forbidden,
    NotFound,
    Stale,
    Validation,
    Conflict,
    BusinessRule,
    RepositoryUnavailable,
    StorageUnavailable,
    Integrity,
    CommitUnknown,
    Internal,
    OtherApplication,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Boundary {
    pub identity: Identity,
    pub phase: Phase,
    pub failure: Option<Failure>,
}
impl Default for Boundary {
    fn default() -> Self {
        Self {
            identity: Identity::Requester,
            phase: Phase::Identity,
            failure: None,
        }
    }
}
impl Boundary {
    pub fn json(self, elapsed_ms: u128) -> Option<String> {
        let failure = match self.failure? {
            Failure::IdentityUnavailable => "identity_unavailable",
            Failure::Timeout => "timeout",
            Failure::SourceMismatch => "source_mismatch",
            Failure::Forbidden => "forbidden",
            Failure::NotFound => "not_found",
            Failure::Stale => "stale",
            Failure::Validation => "validation",
            Failure::Conflict => "conflict",
            Failure::BusinessRule => "business_rule",
            Failure::RepositoryUnavailable => "repository_unavailable",
            Failure::StorageUnavailable => "storage_unavailable",
            Failure::Integrity => "integrity",
            Failure::CommitUnknown => "commit_unknown",
            Failure::Internal => "internal",
            Failure::OtherApplication => "other_application",
        };
        let identity = match self.identity {
            Identity::Requester => "requester",
            Identity::Provider => "provider",
        };
        let phase = match self.phase {
            Phase::Identity => "identity",
            Phase::Revision => "revision",
            Phase::Files => "files",
        };
        Some(format!(
            "{{\"identity\":\"{identity}\",\"phase\":\"{phase}\",\"failure\":\"{failure}\",\"elapsed_ms\":{}}}",
            elapsed_ms.min(u32::MAX as u128)
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_boundary_is_silent_and_failed_boundary_is_closed() {
        assert_eq!(Boundary::default().json(5), None);
        let b = Boundary {
            identity: Identity::Provider,
            phase: Phase::Revision,
            failure: Some(Failure::RepositoryUnavailable),
        };
        assert_eq!(
            b.json(11).unwrap(),
            r#"{"identity":"provider","phase":"revision","failure":"repository_unavailable","elapsed_ms":11}"#
        );
    }
    #[test]
    fn elapsed_is_bounded_without_wrapping() {
        let b = Boundary {
            failure: Some(Failure::Internal),
            ..Boundary::default()
        };
        assert!(
            b.json(u128::MAX)
                .unwrap()
                .contains("\"elapsed_ms\":4294967295")
        );
    }
}

/// A per-call trace shared by the sequential reads; locks never span an await.
pub(super) struct Trace(std::sync::Mutex<Boundary>);
impl Trace {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(Boundary::default()))
    }
    pub fn get(&self) -> Boundary {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }
    pub fn set(&self, value: Boundary) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) = value;
    }
}
