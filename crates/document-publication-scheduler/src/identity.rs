//! Fixed requester resolution for the explicitly selected PoC runtime only.
//! The reusable scheduler executor is audit attribution, never an authenticated
//! requester, a policy subject, or an authentication provider implementation.
use document_application::{
    IdentityContextResolver, IdentityResolutionError, InvocationKind, VerifiedActorContext,
};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use time::{Duration, OffsetDateTime};

use crate::SchedulerError;

pub fn scheduler_executor() -> PrincipalRef {
    PrincipalRef::new("service", "scheduler").expect("fixed scheduler attribution is valid")
}

pub struct StaticRequesterResolver {
    _private: (),
}

impl StaticRequesterResolver {
    pub fn for_runtime_mode(mode: &str) -> Result<Self, SchedulerError> {
        if mode != "poc" {
            return Err(SchedulerError::UnsupportedRuntimeMode);
        }
        Ok(Self { _private: () })
    }
}

impl IdentityContextResolver for StaticRequesterResolver {
    async fn resolve(
        &self,
        principal: &PrincipalRef,
    ) -> Result<VerifiedActorContext, IdentityResolutionError> {
        let invalid = || IdentityResolutionError::InvalidIdentity;
        let (group, invocation) = match (principal.identity_provider(), principal.principal_id()) {
            ("poc", "poc-human") => ("poc-users", InvocationKind::HumanInteractive),
            ("poc", "poc-agent") => ("poc-agents", InvocationKind::Agent),
            _ => return Err(invalid()),
        };
        let subjects = vec![
            PolicySubject::new(
                PolicySubjectKind::Principal,
                "poc",
                principal.principal_id(),
            )
            .map_err(|_| invalid())?,
            PolicySubject::new(PolicySubjectKind::Group, "poc", group).map_err(|_| invalid())?,
        ];
        VerifiedActorContext::from_trusted_adapter(
            principal.clone(),
            subjects,
            OffsetDateTime::now_utc() + Duration::minutes(5),
            invocation,
            None,
        )
        .map_err(|_| invalid())
    }
}
