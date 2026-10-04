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
}
impl OrganizationProfile {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "sales-01" => Ok(Self::Sales),
            "office-01" => Ok(Self::Office),
            _ => Err("KP_ORGANIZATION_PROFILE must select sales-01 or office-01"),
        }
    }
    pub const fn principal(self) -> &'static str {
        match self {
            Self::Sales => "sales-01",
            Self::Office => "office-01",
        }
    }
    pub const fn default_bind(self) -> &'static str {
        match self {
            Self::Sales => "127.0.0.1:8090",
            Self::Office => "127.0.0.1:8091",
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
