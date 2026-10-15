//! Fixed synthetic profiles chosen only by the composition root at startup.
use document_api_http::identity::{IdentityAdapter, IdentityRequestContext};
use document_application::{IdentityResolutionError, InvocationKind, VerifiedActorContext};
use document_domain::{PolicySubject, PolicySubjectKind, PrincipalRef};
use std::{future::Future, pin::Pin};
use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizationProfile {
    Sales,
    Office,
    Review,
    Approver,
    MultiRole,
    Delegate,
}
impl OrganizationProfile {
    /// Closed allowlist (Domain §16). One process serves exactly one profile.
    pub const ALL: [Self; 6] = [
        Self::Sales,
        Self::Office,
        Self::Review,
        Self::Approver,
        Self::MultiRole,
        Self::Delegate,
    ];
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        Self::ALL
            .into_iter()
            .find(|profile| profile.principal() == value)
            .ok_or("KP_ORGANIZATION_PROFILE must select one fixed synthetic profile")
    }
    pub const fn principal(self) -> &'static str {
        match self {
            Self::Sales => "sales-01",
            Self::Office => "office-01",
            Self::Review => "review-01",
            Self::Approver => "approver-01",
            Self::MultiRole => "multi-role-01",
            Self::Delegate => "delegate-01",
        }
    }
    pub const fn default_bind(self) -> &'static str {
        match self {
            Self::Sales => "127.0.0.1:8090",
            Self::Office => "127.0.0.1:8091",
            Self::Review => "127.0.0.1:8092",
            Self::Approver => "127.0.0.1:8093",
            Self::MultiRole => "127.0.0.1:8094",
            Self::Delegate => "127.0.0.1:8095",
        }
    }
}

pub struct SyntheticIdentityAdapter {
    profile: OrganizationProfile,
}
impl SyntheticIdentityAdapter {
    pub fn new(profile: OrganizationProfile) -> Self {
        Self { profile }
    }
    pub fn current_context(&self) -> Result<VerifiedActorContext, IdentityResolutionError> {
        let invalid = |_| IdentityResolutionError::InvalidIdentity;
        let principal = PrincipalRef::new("organization-synthetic", self.profile.principal())
            .map_err(invalid)?;
        let subject = PolicySubject::new(
            PolicySubjectKind::Principal,
            "organization-synthetic",
            self.profile.principal(),
        )
        .map_err(invalid)?;
        VerifiedActorContext::from_trusted_adapter(
            principal,
            vec![subject],
            OffsetDateTime::now_utc() + Duration::minutes(5),
            InvocationKind::HumanInteractive,
            None,
        )
        .map_err(|_| IdentityResolutionError::InvalidIdentity)
    }
}
impl IdentityAdapter for SyntheticIdentityAdapter {
    fn resolve<'a>(
        &'a self,
        _request: &'a IdentityRequestContext,
    ) -> Pin<
        Box<dyn Future<Output = Result<VerifiedActorContext, IdentityResolutionError>> + Send + 'a>,
    > {
        Box::pin(async { self.current_context() })
    }
}
