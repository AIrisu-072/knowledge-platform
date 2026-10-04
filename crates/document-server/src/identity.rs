//! This PoC adapter authenticates every caller reaching a process as its fixed profile.
//! It is not a production authentication mechanism and never consumes request claims.
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_application::{Clock, IdentityResolutionError, InvocationKind, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use std::{future::Future, pin::Pin, sync::Arc};
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoCIdentityProfile {
    Human,
    Agent,
}
impl PoCIdentityProfile {
    pub const fn principal(self) -> &'static str {
        match self {
            Self::Human => "poc-human",
            Self::Agent => "poc-agent",
        }
    }
    pub const fn group(self) -> &'static str {
        match self {
            Self::Human => "poc-users",
            Self::Agent => "poc-agents",
        }
    }
    pub const fn invocation(self) -> InvocationKind {
        match self {
            Self::Human => InvocationKind::HumanInteractive,
            Self::Agent => InvocationKind::Agent,
        }
    }
}

pub(crate) struct UtcClock;
impl Clock for UtcClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

pub struct StaticPoCIdentityAdapter {
    profile: PoCIdentityProfile,
    clock: Arc<dyn Clock>,
}
impl StaticPoCIdentityAdapter {
    pub fn new(profile: PoCIdentityProfile) -> Self {
        Self {
            profile,
            clock: Arc::new(UtcClock),
        }
    }
    #[cfg(test)]
    fn with_clock(profile: PoCIdentityProfile, clock: Arc<dyn Clock>) -> Self {
        Self { profile, clock }
    }
    pub fn current_context(&self) -> Result<VerifiedActorContext, IdentityResolutionError> {
        let invalid = || IdentityResolutionError::InvalidIdentity;
        let principal =
            PrincipalRef::new("poc", self.profile.principal()).map_err(|_| invalid())?;
        let subjects = vec![
            PolicySubject::new(
                PolicySubjectKind::Principal,
                "poc",
                self.profile.principal(),
            )
            .map_err(|_| invalid())?,
            PolicySubject::new(PolicySubjectKind::Group, "poc", self.profile.group())
                .map_err(|_| invalid())?,
        ];
        // Covers the existing 120-second maximum handler budget. Refresh for each
        // request; process lifetime never fixes the context's expiration timestamp.
        VerifiedActorContext::from_trusted_adapter(
            principal,
            subjects,
            self.clock.now() + Duration::minutes(5),
            self.profile.invocation(),
            None,
        )
        .map_err(|_| invalid())
    }
}
impl IdentityAdapter for StaticPoCIdentityAdapter {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async { self.current_context() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    struct AdvancingClock(AtomicI64);
    impl document_application::Clock for AdvancingClock {
        fn now(&self) -> time::OffsetDateTime {
            time::OffsetDateTime::now_utc() + time::Duration::seconds(self.0.load(Ordering::SeqCst))
        }
    }

    #[test]
    fn profile_does_not_expire_at_startup_ttl() {
        let clock = std::sync::Arc::new(AdvancingClock(AtomicI64::new(0)));
        let adapter =
            StaticPoCIdentityAdapter::with_clock(PoCIdentityProfile::Agent, clock.clone());
        let first = adapter.current_context().unwrap();
        clock.0.store(86_400, Ordering::SeqCst);
        let second = adapter.current_context().unwrap();
        assert!(second.valid_until() > first.valid_until() + time::Duration::hours(23));
        assert_eq!(first.principal(), second.principal());
        assert_eq!(first.subjects(), second.subjects());
        assert_eq!(first.invocation_kind(), second.invocation_kind());
        assert!(second.ensure_current().is_ok());
    }
}
