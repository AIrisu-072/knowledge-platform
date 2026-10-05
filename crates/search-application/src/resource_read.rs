//! P5-05: reading one current, visible, durable Resource.
//!
//! The locator looks a Resource ID up only inside the actor's visible Source
//! scopes (never a bare global lookup) and only for the Source-owned current
//! durable identity: no native, ephemeral or historical fallback. Everything
//! that could distinguish "exists but not for you" — unknown, foreign,
//! hidden, old, terminated, revoked, ambiguous (two Sources), or a target
//! error before existence is disclosed — is the same `ResourceNotFound`.
//! The Source rechecks current Read right before the detail is returned, and
//! a cancelled or late operation drops the detail.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use search_core::id::{ResourceId, ResourceVersionId, SourceId};
use search_core::resource::ResourceKind;

use crate::api_scope::{ApiError, SearchOperationContext};
use crate::ports::BoxFuture;
use crate::remote_disclosure::{
    Disclosable, DisclosedFields, DisclosureOwner, TransientDisclosure,
};
use crate::remote_lease::LeaseClock;
use crate::scoped::{AuthorizedSourceScope, TrustedSearchScope, VisibleCatalogSnapshot};

/// Per-item body coverage of the current Resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCoverage {
    TitleAndPermittedMetadata,
    BodySupported,
    BodyPartial,
    BodyUnsupported,
    BodyUnknown,
}

/// A Resource located inside one visible Source scope. The Source-owned
/// locator stays private to the Source adapter.
#[derive(Clone, PartialEq, Eq)]
pub struct VisibleResourceBinding {
    scope: AuthorizedSourceScope,
    resource_id: ResourceId,
    locator: String,
}

impl fmt::Debug for VisibleResourceBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VisibleResourceBinding(<source-owned>)")
    }
}

impl VisibleResourceBinding {
    /// For the owning Source adapter, after a bounded in-scope lookup.
    pub fn new(scope: AuthorizedSourceScope, resource_id: ResourceId, locator: String) -> Self {
        Self {
            scope,
            resource_id,
            locator,
        }
    }
    pub fn scope(&self) -> &AuthorizedSourceScope {
        &self.scope
    }
    pub const fn resource_id(&self) -> ResourceId {
        self.resource_id
    }
    pub fn locator(&self) -> &str {
        &self.locator
    }
}

/// What a Source returns for its current Resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub resource_id: ResourceId,
    pub source_id: SourceId,
    pub resource_type: ResourceKind,
    pub resource_version: Option<ResourceVersionId>,
    pub title: Option<String>,
    pub coverage: ResourceCoverage,
}

pub trait ResourceLocatorPort: Send + Sync {
    /// Bounded lookup inside the given visible scopes only. Every scope that
    /// holds the current durable Resource answers; more than one is ambiguous.
    fn resolve_visible<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        visible: &'a [AuthorizedSourceScope],
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Vec<VisibleResourceBinding>>;
}

pub trait CurrentResourceReadPort: Send + Sync {
    /// The current Source-owned state, or `None` when it is no longer current
    /// or readable by the actor.
    fn read_current<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        binding: &'a VisibleResourceBinding,
    ) -> BoxFuture<'a, Option<ResourceSnapshot>>;
}

/// The allow-listed detail view.
pub struct ResourceView {
    snapshot: ResourceSnapshot,
}

impl fmt::Debug for ResourceView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResourceView(<transient>)")
    }
}

impl ResourceView {
    pub fn snapshot(&self) -> &ResourceSnapshot {
        &self.snapshot
    }
}

impl Disclosable for ResourceView {
    fn disclosed_fields(&self) -> DisclosedFields {
        DisclosedFields {
            resources: vec![self.snapshot.resource_id],
            resource_sources: vec![(self.snapshot.resource_id, self.snapshot.source_id)],
            claims: vec![],
        }
    }
}

pub struct ResourceReadService<'a> {
    locator: &'a dyn ResourceLocatorPort,
    reader: &'a dyn CurrentResourceReadPort,
    clock: Arc<dyn LeaseClock>,
    disclosure_ttl: Duration,
}

impl fmt::Debug for ResourceReadService<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResourceReadService(<wired>)")
    }
}

impl<'a> ResourceReadService<'a> {
    pub fn new(
        locator: &'a dyn ResourceLocatorPort,
        reader: &'a dyn CurrentResourceReadPort,
        clock: Arc<dyn LeaseClock>,
        disclosure_ttl: Duration,
    ) -> Self {
        Self {
            locator,
            reader,
            clock,
            disclosure_ttl,
        }
    }

    pub async fn read(
        &self,
        context: &SearchOperationContext,
        snapshot: &VisibleCatalogSnapshot,
        resource_id: ResourceId,
    ) -> Result<TransientDisclosure<ResourceView>, ApiError> {
        context.check_live()?;
        let actor = context.actor();
        let scopes: Vec<AuthorizedSourceScope> = snapshot
            .entries()
            .iter()
            .map(|entry| entry.scope().clone())
            .collect();
        // Any error before existence is disclosed reads like absence.
        let mut bindings = self
            .locator
            .resolve_visible(actor, &scopes, resource_id)
            .await
            .map_err(|_| ApiError::ResourceNotFound)?;
        if bindings.len() != 1 {
            return Err(ApiError::ResourceNotFound);
        }
        let binding = bindings.remove(0);
        if !scopes.contains(binding.scope()) || binding.resource_id() != resource_id {
            return Err(ApiError::ResourceNotFound);
        }
        let current = self
            .reader
            .read_current(actor, &binding)
            .await
            .map_err(|_| ApiError::ResourceNotFound)?
            .filter(|current| {
                current.resource_id == resource_id
                    && current.source_id == binding.scope().source_id()
            })
            .ok_or(ApiError::ResourceNotFound)?;
        // A cancelled or late request never returns the detail it read.
        context.check_live()?;
        Ok(TransientDisclosure::new(
            ResourceView { snapshot: current },
            DisclosureOwner::new(actor.clone(), vec![binding.scope().clone()]),
            self.disclosure_ttl,
            self.clock.clone(),
            true,
        ))
    }
}
