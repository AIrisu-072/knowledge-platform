//! P5-04: the actor-visible Claim catalog.
//!
//! A public Discover request names required Claims only by ID. The server
//! binds each ID to its server-owned definition only when the definition
//! belongs to the actor's tenant and to a Source in the actor's current
//! visible set. An unknown, foreign, hidden or withdrawn Claim stays in the
//! request unbound: it can never be satisfied and becomes the same generic
//! unresolved gap. A bound selector is rechecked against the pinned
//! generation's Source and the definition's current revision every time it
//! is read; no selector value ever leaves the server.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::RwLock;

use search_core::id::{ClaimId, SourceId};
use search_core::predicate::TypedValue;
use search_core::projection::ProjectionGenerationKey;

use crate::SearchError;
use crate::ports::{BoxFuture, ClaimSelector, ClaimSelectorPort};
use crate::remote_read_view::RemoteClaimSelectors;
use crate::scoped::{AuthorizedSourceScope, TenantId, TrustedSearchScope};

/// One server-owned Claim definition.
#[derive(Clone, PartialEq, Eq)]
pub struct ClaimDefinition {
    pub claim_id: ClaimId,
    pub tenant: TenantId,
    pub source_id: SourceId,
    pub subject_ref: String,
    pub predicate: String,
    pub expected_value: Option<TypedValue>,
    pub revision: u64,
}

impl fmt::Debug for ClaimDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClaimDefinition(<server-owned>)")
    }
}

/// A Claim bound to one visible Source scope of the actor, at one revision.
#[derive(Clone, PartialEq, Eq)]
pub struct VisibleClaimBinding {
    claim_id: ClaimId,
    source: AuthorizedSourceScope,
    revision: u64,
    predicate: String,
    expected_value: Option<TypedValue>,
}

impl fmt::Debug for VisibleClaimBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VisibleClaimBinding(<server-owned>)")
    }
}

impl VisibleClaimBinding {
    pub const fn claim_id(&self) -> ClaimId {
        self.claim_id
    }
    pub fn source(&self) -> &AuthorizedSourceScope {
        &self.source
    }
}

pub trait ActorVisibleClaimCatalogPort: Send + Sync {
    fn bind<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        visible: &'a [AuthorizedSourceScope],
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<VisibleClaimBinding>>;

    fn selector_for_visible<'a>(
        &'a self,
        binding: &'a VisibleClaimBinding,
        pinned_generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Option<ClaimSelector>>;
}

/// In-process server-owned definitions (a host adapter may persist them).
#[derive(Default)]
pub struct InMemoryClaimCatalog {
    definitions: RwLock<BTreeMap<ClaimId, ClaimDefinition>>,
}

impl fmt::Debug for InMemoryClaimCatalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InMemoryClaimCatalog(<server-owned>)")
    }
}

impl InMemoryClaimCatalog {
    pub fn new(definitions: Vec<ClaimDefinition>) -> Self {
        Self {
            definitions: RwLock::new(
                definitions
                    .into_iter()
                    .map(|definition| (definition.claim_id, definition))
                    .collect(),
            ),
        }
    }

    /// Replaces a definition; a bound older revision stops resolving.
    pub fn upsert(&self, definition: ClaimDefinition) -> Result<(), SearchError> {
        self.definitions
            .write()
            .map_err(|_| unavailable())?
            .insert(definition.claim_id, definition);
        Ok(())
    }

    pub fn withdraw(&self, claim_id: ClaimId) -> Result<(), SearchError> {
        self.definitions
            .write()
            .map_err(|_| unavailable())?
            .remove(&claim_id);
        Ok(())
    }
}

fn unavailable() -> SearchError {
    SearchError::SourceUnavailable("claim catalog unavailable".into())
}

impl ActorVisibleClaimCatalogPort for InMemoryClaimCatalog {
    fn bind<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        visible: &'a [AuthorizedSourceScope],
        claim_id: ClaimId,
    ) -> BoxFuture<'a, Option<VisibleClaimBinding>> {
        Box::pin(async move {
            let definitions = self.definitions.read().map_err(|_| unavailable())?;
            let Some(definition) = definitions.get(&claim_id) else {
                return Ok(None);
            };
            if &definition.tenant != actor.tenant() {
                return Ok(None);
            }
            Ok(visible
                .iter()
                .find(|scope| scope.actor() == actor && scope.source_id() == definition.source_id)
                .map(|scope| VisibleClaimBinding {
                    claim_id,
                    source: scope.clone(),
                    revision: definition.revision,
                    predicate: definition.predicate.clone(),
                    expected_value: definition.expected_value.clone(),
                }))
        })
    }

    fn selector_for_visible<'a>(
        &'a self,
        binding: &'a VisibleClaimBinding,
        pinned_generation: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async move {
            let definitions = self.definitions.read().map_err(|_| unavailable())?;
            Ok(definitions
                .get(&binding.claim_id)
                .filter(|definition| {
                    definition.revision == binding.revision
                        && definition.source_id == binding.source.source_id()
                        && pinned_generation.source_id == definition.source_id
                        && definition.predicate == binding.predicate
                        && definition.expected_value == binding.expected_value
                })
                .map(|definition| ClaimSelector {
                    claim_id: definition.claim_id,
                    subject_ref: definition.subject_ref.clone(),
                    predicate: definition.predicate.clone(),
                    expected_value: definition.expected_value.clone(),
                }))
        })
    }
}

/// The selector port of one Discover evaluation: only the request's bound
/// Claims, each rechecked for the generation it is read against.
pub struct VisibleClaimSelectors<'a> {
    catalog: &'a dyn ActorVisibleClaimCatalogPort,
    bindings: BTreeMap<ClaimId, VisibleClaimBinding>,
}

impl fmt::Debug for VisibleClaimSelectors<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VisibleClaimSelectors(<request-owned>)")
    }
}

impl<'a> VisibleClaimSelectors<'a> {
    /// Binds every requested Claim it can; the rest stay unbound.
    pub async fn bind_all(
        catalog: &'a dyn ActorVisibleClaimCatalogPort,
        actor: &TrustedSearchScope,
        visible: &[AuthorizedSourceScope],
        claims: &[ClaimId],
    ) -> Result<Self, SearchError> {
        let mut bindings = BTreeMap::new();
        for claim in claims {
            if let Some(binding) = catalog.bind(actor, visible, *claim).await? {
                bindings.insert(*claim, binding);
            }
        }
        Ok(Self { catalog, bindings })
    }

    pub fn bound(&self) -> impl Iterator<Item = &VisibleClaimBinding> {
        self.bindings.values()
    }

    /// Server-owned selectors for the bound Claims of remote Sources, each
    /// answering only for its own Source.
    pub fn remote_selectors(&self, remote_sources: &[SourceId]) -> RemoteClaimSelectors {
        RemoteClaimSelectors::for_sources(
            self.bindings
                .values()
                .filter(|binding| remote_sources.contains(&binding.source.source_id()))
                .map(|binding| {
                    (
                        binding.claim_id,
                        binding.source.source_id(),
                        binding.predicate.clone(),
                        binding.expected_value.clone(),
                    )
                })
                .collect(),
        )
    }
}

impl ClaimSelectorPort for VisibleClaimSelectors<'_> {
    fn selector_for<'b>(
        &'b self,
        generation: ProjectionGenerationKey,
        claim_id: ClaimId,
    ) -> BoxFuture<'b, Option<ClaimSelector>> {
        Box::pin(async move {
            match self.bindings.get(&claim_id) {
                Some(binding) => self.catalog.selector_for_visible(binding, generation).await,
                None => Ok(None),
            }
        })
    }
}
