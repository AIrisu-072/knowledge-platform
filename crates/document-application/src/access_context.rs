use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use thiserror::Error;
use time::OffsetDateTime;

use crate::ApplicationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationKind {
    HumanInteractive,
    Agent,
    Service,
}

impl InvocationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HumanInteractive => "human_interactive",
            Self::Agent => "agent",
            Self::Service => "service",
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum IdentityResolutionError {
    #[error("identity provider is temporarily unavailable")]
    Unavailable,
    #[error("identity is invalid or no longer verifiable")]
    InvalidIdentity,
}

/// Constructed only at a trusted identity-adapter assembly boundary. This value
/// carries verified claims; its constructor does not authenticate a caller.
#[derive(Debug, Clone)]
pub struct VerifiedActorContext {
    principal: PrincipalRef,
    subjects: Vec<PolicySubject>,
    valid_until: OffsetDateTime,
    invocation_kind: InvocationKind,
    service_executor: Option<PrincipalRef>,
}

impl VerifiedActorContext {
    pub fn from_trusted_adapter(
        principal: PrincipalRef,
        subjects: Vec<PolicySubject>,
        valid_until: OffsetDateTime,
        invocation_kind: InvocationKind,
        service_executor: Option<PrincipalRef>,
    ) -> Result<Self, ApplicationError> {
        if valid_until <= OffsetDateTime::now_utc() {
            return Err(ApplicationError::Validation(
                "identity context expired".into(),
            ));
        }
        if !subjects.iter().any(|subject| {
            subject.kind() == PolicySubjectKind::Principal
                && subject.identity_provider() == principal.identity_provider()
                && subject.subject_id() == principal.principal_id()
        }) {
            return Err(ApplicationError::Validation(
                "verified subjects must include the actor principal".into(),
            ));
        }
        if service_executor.is_some() && invocation_kind != InvocationKind::Service {
            return Err(ApplicationError::Validation(
                "service executor requires service invocation".into(),
            ));
        }
        Ok(Self {
            principal,
            subjects,
            valid_until,
            invocation_kind,
            service_executor,
        })
    }

    pub fn ensure_current(&self) -> Result<(), ApplicationError> {
        if self.valid_until <= OffsetDateTime::now_utc() {
            return Err(ApplicationError::Validation(
                "identity context expired".into(),
            ));
        }
        Ok(())
    }

    pub const fn principal(&self) -> &PrincipalRef {
        &self.principal
    }

    pub fn subjects(&self) -> &[PolicySubject] {
        &self.subjects
    }

    pub const fn valid_until(&self) -> OffsetDateTime {
        self.valid_until
    }

    pub const fn invocation_kind(&self) -> InvocationKind {
        self.invocation_kind
    }

    pub const fn service_executor(&self) -> Option<&PrincipalRef> {
        self.service_executor.as_ref()
    }
}
