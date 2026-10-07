//! Closed rejection codes. A [`Rejection`] carries only a code and, at most, a
//! static location name taken from the catalog or the envelope layout. It never
//! carries payload values, so logging it cannot leak audit content.

use std::fmt;

/// Stable quarantine/rejection codes shared by the relay, the Store and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RejectionCode {
    EnvelopeTooLarge,
    InvalidJson,
    DuplicateKey,
    InvalidEnvelope,
    UnknownEventType,
    ControlTypeForbidden,
    InvalidSource,
    InvalidSubject,
    InvalidResource,
    /// A client-chosen identifier (uuid-kind field, `resource.id` or
    /// `resource.version_id`) is the nil UUID. The producer accepts it, so
    /// this is a producer-reachable quarantine (handoff to Document).
    NilClientId,
    InvalidResult,
    InvalidActor,
    InvalidServiceExecutor,
    UnknownField,
    MissingField,
    InvalidField,
    InvalidCorrelation,
    InvalidSourceCorrelation,
    InvalidReason,
    ReasonNotString,
    ActorMismatch,
    SourceRowTooLarge,
    SourceDigestMismatch,
    InvalidProvenance,
    InvalidExtensions,
}

impl RejectionCode {
    /// Every code, in declaration order.
    pub const ALL: [Self; 25] = [
        Self::EnvelopeTooLarge,
        Self::InvalidJson,
        Self::DuplicateKey,
        Self::InvalidEnvelope,
        Self::UnknownEventType,
        Self::ControlTypeForbidden,
        Self::InvalidSource,
        Self::InvalidSubject,
        Self::InvalidResource,
        Self::NilClientId,
        Self::InvalidResult,
        Self::InvalidActor,
        Self::InvalidServiceExecutor,
        Self::UnknownField,
        Self::MissingField,
        Self::InvalidField,
        Self::InvalidCorrelation,
        Self::InvalidSourceCorrelation,
        Self::InvalidReason,
        Self::ReasonNotString,
        Self::ActorMismatch,
        Self::SourceRowTooLarge,
        Self::SourceDigestMismatch,
        Self::InvalidProvenance,
        Self::InvalidExtensions,
    ];

    /// Stable snake_case wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnvelopeTooLarge => "envelope_too_large",
            Self::InvalidJson => "invalid_json",
            Self::DuplicateKey => "duplicate_key",
            Self::InvalidEnvelope => "invalid_envelope",
            Self::UnknownEventType => "unknown_event_type",
            Self::ControlTypeForbidden => "control_type_forbidden",
            Self::InvalidSource => "invalid_source",
            Self::InvalidSubject => "invalid_subject",
            Self::InvalidResource => "invalid_resource",
            Self::NilClientId => "nil_client_id",
            Self::InvalidResult => "invalid_result",
            Self::InvalidActor => "invalid_actor",
            Self::InvalidServiceExecutor => "invalid_service_executor",
            Self::UnknownField => "unknown_field",
            Self::MissingField => "missing_field",
            Self::InvalidField => "invalid_field",
            Self::InvalidCorrelation => "invalid_correlation",
            Self::InvalidSourceCorrelation => "invalid_source_correlation",
            Self::InvalidReason => "invalid_reason",
            Self::ReasonNotString => "reason_not_string",
            Self::ActorMismatch => "actor_mismatch",
            Self::SourceRowTooLarge => "source_row_too_large",
            Self::SourceDigestMismatch => "source_digest_mismatch",
            Self::InvalidProvenance => "invalid_provenance",
            Self::InvalidExtensions => "invalid_extensions",
        }
    }

    /// Parses a wire name produced by [`Self::as_str`].
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == value)
    }
}

impl fmt::Display for RejectionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validation failure. `field` is a static location: a catalog field name of
/// the embedded catalog or a fixed envelope path such as `data.actor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rejection {
    pub code: RejectionCode,
    pub field: Option<&'static str>,
}

impl Rejection {
    pub const fn new(code: RejectionCode) -> Self {
        Self { code, field: None }
    }

    pub const fn at(code: RejectionCode, field: &'static str) -> Self {
        Self {
            code,
            field: Some(field),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.field {
            Some(field) => write!(f, "{} ({field})", self.code),
            None => f.write_str(self.code.as_str()),
        }
    }
}

impl std::error::Error for Rejection {}

impl From<RejectionCode> for Rejection {
    fn from(code: RejectionCode) -> Self {
        Self::new(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_snake_case_and_round_trip() {
        let mut seen = std::collections::BTreeSet::new();
        for code in RejectionCode::ALL {
            let name = code.as_str();
            assert!(seen.insert(name), "duplicate code {name}");
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{name}"
            );
            assert_eq!(RejectionCode::parse(name), Some(code));
        }
        assert_eq!(RejectionCode::parse("nope"), None);
    }

    #[test]
    fn display_names_only_code_and_static_field() {
        let rejection = Rejection::at(RejectionCode::InvalidField, "versionNo");
        assert_eq!(rejection.to_string(), "invalid_field (versionNo)");
        assert_eq!(
            Rejection::new(RejectionCode::UnknownField).to_string(),
            "unknown_field"
        );
    }
}
