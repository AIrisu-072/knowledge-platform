//! P5-02: the request scope shared by the four Search API routes.
//!
//! Every route authenticates one server-issued handle into a current actor,
//! then reads one complete actor-visible Document/Remote catalog through the
//! shared P4 helper (`check_actor_current` → union enumeration → registration,
//! activation and current visibility → `check_actor_current`). A Source that
//! is individually denied, unknown or racing a revision is simply absent; an
//! actor, registry, ledger or visibility failure, a duplicate or a foreign
//! entry discards the whole snapshot as one generic dependency failure.
//! Only Discovery binds an evaluation; the other routes never mint one.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::scoped::{
    AccessBindingState, AccessContextAuthorityPort, AccessContextHandle, ScopedSourceRegistryPort,
    TrustedSearchScope, VisibleCatalogSnapshot, check_actor_current, prepare_actor_visible_sources,
};

/// The registry outcome classes a Search route can end in. The HTTP adapter
/// maps each to its one fixed Problem; nothing here carries request data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    MalformedRequest,
    AuthenticationRequired,
    Forbidden,
    ResourceNotFound,
    CursorStale,
    ValidationFailed,
    IdentityUnavailable,
    DependencyUnavailable,
    ServiceUnavailable,
    UpstreamTimeout,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for ApiError {}

/// Cooperative cancellation shared by a route and its ports.
#[derive(Debug, Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// One authenticated request: the current actor, its operation deadline and
/// cancellation. No tenant, principal or grant comes from the request.
#[derive(Clone)]
pub struct SearchOperationContext {
    actor: TrustedSearchScope,
    operation_deadline: Instant,
    cancellation: Cancellation,
}

impl fmt::Debug for SearchOperationContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchOperationContext(<actor-scoped>)")
    }
}

impl SearchOperationContext {
    /// Resolves a verified opaque handle. Unknown, expired or revoked is
    /// `AuthenticationRequired`; an authority failure is `IdentityUnavailable`.
    pub async fn authenticate(
        authority: &dyn AccessContextAuthorityPort,
        handle: &AccessContextHandle,
        operation_deadline: Instant,
    ) -> Result<Self, ApiError> {
        let actor = authority
            .resolve(handle)
            .await
            .map_err(|_| ApiError::IdentityUnavailable)?
            .ok_or(ApiError::AuthenticationRequired)?;
        match authority.current(&actor).await {
            Ok(AccessBindingState::Current) if actor.is_live() => {}
            Ok(_) => return Err(ApiError::AuthenticationRequired),
            Err(_) => return Err(ApiError::IdentityUnavailable),
        }
        Ok(Self {
            actor,
            operation_deadline,
            cancellation: Cancellation::default(),
        })
    }

    pub fn actor(&self) -> &TrustedSearchScope {
        &self.actor
    }

    pub const fn operation_deadline(&self) -> Instant {
        self.operation_deadline
    }

    pub fn cancellation(&self) -> &Cancellation {
        &self.cancellation
    }

    /// A deadline or cancellation ends the operation as a generic 503.
    pub fn check_live(&self) -> Result<(), ApiError> {
        if self.cancellation.is_cancelled() || Instant::now() >= self.operation_deadline {
            return Err(ApiError::ServiceUnavailable);
        }
        Ok(())
    }
}

/// The complete actor-visible catalog for any of the four routes.
pub async fn prepare_api_visible_sources(
    authority: &dyn AccessContextAuthorityPort,
    registry: &dyn ScopedSourceRegistryPort,
    context: &SearchOperationContext,
) -> Result<VisibleCatalogSnapshot, ApiError> {
    context.check_live()?;
    // An actor revoked before routing stops every Source read; an authority
    // that cannot answer is an identity outage, not a credential failure.
    let actor = context.actor();
    match authority.resolve(actor.access_handle()).await {
        Ok(Some(resolved)) if &resolved == actor => {}
        Ok(_) => return Err(ApiError::AuthenticationRequired),
        Err(_) => return Err(ApiError::IdentityUnavailable),
    }
    match authority.current(actor).await {
        Ok(AccessBindingState::Current) if actor.is_live() => {}
        Ok(_) => return Err(ApiError::AuthenticationRequired),
        Err(_) => return Err(ApiError::IdentityUnavailable),
    }
    let snapshot = prepare_actor_visible_sources(authority, registry, context.actor())
        .await
        .map_err(|_| ApiError::DependencyUnavailable)?;
    context.check_live()?;
    Ok(snapshot)
}
