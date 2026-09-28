use document_domain::PrincipalRef;

use crate::{
    IdentityContextResolver, IdentityResolutionError, InvocationKind, VerifiedActorContext,
};

/// Resolve the original requester outside any database lock. The scheduler
/// identity is recorded separately and is never added to the requester's
/// policy subjects.
pub async fn authorize_scheduled_publish<R: IdentityContextResolver>(
    resolver: &R,
    requester: &PrincipalRef,
    executor: &PrincipalRef,
) -> Result<VerifiedActorContext, IdentityResolutionError> {
    let resolved = resolver.resolve(requester).await?;
    if resolved.principal() != requester || resolved.ensure_current().is_err() {
        return Err(IdentityResolutionError::InvalidIdentity);
    }
    VerifiedActorContext::from_trusted_adapter(
        requester.clone(),
        resolved.subjects().to_vec(),
        resolved.valid_until(),
        InvocationKind::Service,
        Some(executor.clone()),
    )
    .map_err(|_| IdentityResolutionError::InvalidIdentity)
}
