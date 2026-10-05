//! P4-13: the scoped Discovery entrypoint and the final disclosure gate.
//!
//! `ScopedDiscoveryService::discover` binds the request to the server-issued
//! actor and evaluation, stops before routing or any provider call when that
//! binding is not current, and evaluates only actor-visible Sources through
//! the common Discovery path. A Source whose visibility is revoked while the
//! evaluation runs is dropped with everything derived from it, and the
//! evaluation is recomputed from the remaining Sources; a revoked Required
//! Source then reads exactly like a missing one. Each evaluation's view (and
//! its remote leases) closes before the result leaves `discover`.
//!
//! The result is held by a short `TransientDisclosure` lease. The only way to
//! read it is `with_disclosure`, which rechecks the actor, Sources, items and
//! fields through `CurrentDisclosureAccessPort` and then lends a borrowed
//! `DisclosureView`. The lease closes on success, error and cancellation.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use search_core::discovery::{DiscoveryRequest, DiscoveryResult, GapReason};
use search_core::evidence::{ClaimState, EvidenceRole, EvidenceSufficiency};
use search_core::id::{ClaimId, DiscoveryEvaluationId, ResourceId, SourceId};

use crate::SearchError;
use crate::content_scope::DiscoveryScope;
use crate::discovery_service::{DiscoveryService, RemoteSourceExecution, ScopedDiscoveryExecution};
use crate::ports::{AccessDecision, BoxFuture};
use crate::remote::{RemoteSourcePort, TrustedRemoteContext};
use crate::remote_evidence::{RegisteredLineage, RemoteProvenanceLookupPort};
use crate::remote_lease::{LeaseClock, LeaseState, RemoteLease, ScopedOwnerGate};
use crate::remote_read_view::{CompositeEvaluationReadView, RemoteClaimSelectors};
use crate::routing::RoutingConstraints;
use crate::scoped::{
    AccessContextAuthorityPort, AuthorizedSourceScope, CurrentSourceVisibilityPort,
    ScopedSourceRegistryPort, TrustedDiscoveryBinding, TrustedSearchScope,
    VisibleSourceRegistration, check_actor_current, prepare_actor_visible_sources,
    verify_discovery_binding,
};
use crate::source_registration::SourceRegistration;

/// Longest disclosure lease; a result is meant to be disclosed at once.
pub const MAX_DISCLOSURE_TTL: Duration = Duration::from_secs(60);

fn disclosure_unavailable() -> SearchError {
    SearchError::SourceUnavailable("disclosure unavailable".into())
}

/// Who a transient result belongs to: the actor and every Source scope the
/// evaluation read. Only the scoped service creates one.
#[derive(Clone, PartialEq, Eq)]
pub struct DisclosureOwner {
    actor: TrustedSearchScope,
    sources: Vec<AuthorizedSourceScope>,
}

impl fmt::Debug for DisclosureOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DisclosureOwner(<scoped>)")
    }
}

impl DisclosureOwner {
    pub(crate) fn new(actor: TrustedSearchScope, sources: Vec<AuthorizedSourceScope>) -> Self {
        Self { actor, sources }
    }
    pub fn actor(&self) -> &TrustedSearchScope {
        &self.actor
    }
    pub fn sources(&self) -> &[AuthorizedSourceScope] {
        &self.sources
    }
}

/// The items and fields a disclosure is about to reveal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosedFields {
    pub resources: Vec<ResourceId>,
    /// The owning Source of each disclosed item, where the result names it;
    /// an item gate refuses an item without one.
    pub resource_sources: Vec<(ResourceId, SourceId)>,
    pub claims: Vec<ClaimId>,
}

/// The final output gate: a current actor/Source/item/field recheck right
/// before the caller sees anything.
pub trait CurrentDisclosureAccessPort: Send + Sync {
    fn authorize<'a>(
        &'a self,
        owner: &'a DisclosureOwner,
        disclosed_fields: &'a DisclosedFields,
    ) -> BoxFuture<'a, ()>;
}

/// Actor and Source gate over the trusted authority and visibility ports.
/// Item/field policy beyond Source visibility belongs to a host adapter.
pub struct ScopedDisclosureGate<'a> {
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
}

impl<'a> ScopedDisclosureGate<'a> {
    pub fn new(
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
    ) -> Self {
        Self {
            authority,
            visibility,
        }
    }
}

impl CurrentDisclosureAccessPort for ScopedDisclosureGate<'_> {
    fn authorize<'b>(
        &'b self,
        owner: &'b DisclosureOwner,
        _disclosed_fields: &'b DisclosedFields,
    ) -> BoxFuture<'b, ()> {
        Box::pin(async move {
            check_actor_current(self.authority, &owner.actor).await?;
            for scope in &owner.sources {
                if self.visibility.current(scope).await? != AccessDecision::Allowed {
                    return Err(disclosure_unavailable());
                }
            }
            // A visibility await may overlap revocation of the actor.
            check_actor_current(self.authority, &owner.actor).await
        })
    }
}

/// Borrowed, allow-listed accessors over a gated result. It cannot be
/// cloned, serialized or turned back into the result.
pub struct DisclosureView<'a> {
    result: &'a DiscoveryResult,
}

impl DisclosureView<'_> {
    pub fn evaluation_id(&self) -> DiscoveryEvaluationId {
        self.result.discovery_evaluation_id
    }
    pub fn sufficiency(&self) -> EvidenceSufficiency {
        self.result.evidence_sufficiency
    }
    pub fn qualified_resources(&self) -> impl Iterator<Item = ResourceId> + '_ {
        self.result
            .qualified_resources
            .iter()
            .map(|resource| resource.resource_ref)
    }
    pub fn claims(&self) -> impl Iterator<Item = (ClaimId, ClaimState)> + '_ {
        self.result
            .evidence_set
            .iter()
            .map(|claim| (claim.claim_id, claim.state))
    }
    pub fn evidence_roles(&self, claim_id: ClaimId) -> impl Iterator<Item = EvidenceRole> + '_ {
        self.result
            .evidence_set
            .iter()
            .filter(move |claim| claim.claim_id == claim_id)
            .flat_map(|claim| claim.evidence_refs.iter().map(|evidence| evidence.role))
    }
    pub fn gaps(&self) -> impl Iterator<Item = (&str, GapReason, bool)> + '_ {
        self.result
            .unresolved_gaps
            .iter()
            .map(|gap| (gap.required_fact.as_str(), gap.reason, gap.blocking))
    }
    /// The safe public projection, built only inside the disclosure gate.
    pub fn public(
        &self,
        trace_id: uuid::Uuid,
    ) -> crate::public_projection::DiscoveryEvaluationView {
        crate::public_projection::project_discovery(self.result, trace_id)
    }

    pub fn trace(&self) -> impl Iterator<Item = &str> + '_ {
        self.result
            .source_trace
            .iter()
            .chain(&self.result.retrieval_trace)
            .chain(&self.result.qualification_trace)
            .map(String::as_str)
    }
}

/// A short disclosure lease over one result. Reading it consumes it.
pub struct TransientDisclosure<T> {
    payload: Option<T>,
    owner: DisclosureOwner,
    deadline: Instant,
    clock: Arc<dyn LeaseClock>,
    state: LeaseState,
    evaluation_closed: bool,
}

impl<T> fmt::Debug for TransientDisclosure<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransientDisclosure(<transient>)")
    }
}

/// A result type whose disclosure the final gate can describe.
pub trait Disclosable {
    fn disclosed_fields(&self) -> DisclosedFields;
}

impl<T> TransientDisclosure<T> {
    pub(crate) fn new(
        payload: T,
        owner: DisclosureOwner,
        ttl: Duration,
        clock: Arc<dyn LeaseClock>,
        evaluation_closed: bool,
    ) -> Self {
        let deadline = clock.now() + ttl.min(MAX_DISCLOSURE_TTL);
        Self {
            payload: Some(payload),
            owner,
            deadline,
            clock,
            state: LeaseState::Open,
            evaluation_closed,
        }
    }

    /// The same one-shot, cancel-safe gate for any disclosable result; the
    /// callback sees only a shared borrow of the allow-listed view.
    pub async fn disclose_with(
        &mut self,
        gate: &dyn CurrentDisclosureAccessPort,
        inspect: impl FnOnce(&T) -> Result<(), SearchError>,
    ) -> Result<(), SearchError>
    where
        T: Disclosable,
    {
        let open = self.state() == LeaseState::Open;
        let payload = self.payload.take();
        self.state = if open {
            LeaseState::Closed
        } else {
            self.state()
        };
        let Some(payload) = payload.filter(|_| open) else {
            return Err(disclosure_unavailable());
        };
        gate.authorize(&self.owner, &payload.disclosed_fields())
            .await?;
        inspect(&payload)
    }

    pub fn state(&self) -> LeaseState {
        if self.state == LeaseState::Open && self.clock.now() >= self.deadline {
            LeaseState::Expired
        } else {
            self.state
        }
    }

    /// Whether every remote lease of the evaluation that produced this
    /// result was closed before the result left the service.
    pub const fn evaluation_closed(&self) -> bool {
        self.evaluation_closed
    }
}

impl TransientDisclosure<DiscoveryResult> {
    /// Rechecks the owner and the disclosed items/fields, then lends the
    /// view to `inspect`. The lease is closed before the gate is awaited, so
    /// success, error and cancellation all leave nothing behind.
    pub async fn with_disclosure(
        &mut self,
        gate: &dyn CurrentDisclosureAccessPort,
        inspect: impl FnOnce(DisclosureView<'_>) -> Result<(), SearchError>,
    ) -> Result<(), SearchError> {
        let open = self.state() == LeaseState::Open;
        let payload = self.payload.take();
        self.state = if open {
            LeaseState::Closed
        } else {
            self.state()
        };
        let Some(result) = payload.filter(|_| open) else {
            return Err(disclosure_unavailable());
        };
        let fields = DisclosedFields {
            resources: result
                .qualified_resources
                .iter()
                .map(|resource| resource.resource_ref)
                .collect(),
            resource_sources: result
                .qualified_resources
                .iter()
                .filter_map(|resource| {
                    resource
                        .source_ref
                        .map(|source| (resource.resource_ref, source))
                })
                .collect(),
            // An unbound or unevaluated Claim carries only the caller's own
            // ID; every Claim with a value or evidence is rechecked.
            claims: result
                .evidence_set
                .iter()
                .filter(|claim| claim.value.is_some() || !claim.evidence_refs.is_empty())
                .map(|claim| claim.claim_id)
                .collect(),
        };
        gate.authorize(&self.owner, &fields).await?;
        inspect(DisclosureView { result: &result })
    }
}

/// Host wiring of one remote Source: its checked adapter, server-owned
/// lineage and provenance lookup, and the evaluation lease bounds.
#[derive(Clone)]
pub struct RemoteWiring<'a> {
    pub port: &'a dyn RemoteSourcePort,
    pub lineage: RegisteredLineage,
    pub provenance: &'a dyn RemoteProvenanceLookupPort,
    pub evaluation_ttl: Duration,
    pub idle_timeout: Option<Duration>,
}

pub struct ScopedDiscoveryService<'a> {
    service: &'a DiscoveryService<'a>,
    authority: &'a dyn AccessContextAuthorityPort,
    visibility: &'a dyn CurrentSourceVisibilityPort,
    registry: &'a dyn ScopedSourceRegistryPort,
    remote_selectors: &'a RemoteClaimSelectors,
    remote: BTreeMap<SourceId, RemoteWiring<'a>>,
    clock: Arc<dyn LeaseClock>,
    disclosure_ttl: Duration,
}

impl fmt::Debug for ScopedDiscoveryService<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ScopedDiscoveryService(<wired>)")
    }
}

impl<'a> ScopedDiscoveryService<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service: &'a DiscoveryService<'a>,
        authority: &'a dyn AccessContextAuthorityPort,
        visibility: &'a dyn CurrentSourceVisibilityPort,
        registry: &'a dyn ScopedSourceRegistryPort,
        remote_selectors: &'a RemoteClaimSelectors,
        remote: Vec<(SourceId, RemoteWiring<'a>)>,
        clock: Arc<dyn LeaseClock>,
        disclosure_ttl: Duration,
    ) -> Result<Self, SearchError> {
        let mut wired = BTreeMap::new();
        for (source, wiring) in remote {
            if wiring.evaluation_ttl.is_zero() || wired.insert(source, wiring).is_some() {
                return Err(SearchError::InvalidRequest(
                    "remote wiring is invalid".into(),
                ));
            }
        }
        if disclosure_ttl.is_zero() || disclosure_ttl > MAX_DISCLOSURE_TTL {
            return Err(SearchError::InvalidRequest(
                "disclosure lease is out of bounds".into(),
            ));
        }
        Ok(Self {
            service,
            authority,
            visibility,
            registry,
            remote_selectors,
            remote: wired,
            clock,
            disclosure_ttl,
        })
    }

    /// The public request cannot choose its actor or evaluation: both are
    /// taken from the server-issued binding and verified before any read.
    pub async fn discover(
        &self,
        binding: &TrustedDiscoveryBinding,
        request: DiscoveryRequest,
        trusted_routing: RoutingConstraints,
    ) -> Result<TransientDisclosure<DiscoveryResult>, SearchError> {
        self.discover_with_scope(binding, request, trusted_routing, DiscoveryScope::Normal)
            .await
    }

    /// The same entrypoint for an explicit content scope (e.g. a body scope
    /// whose query was validated by the trusted route).
    pub async fn discover_with_scope(
        &self,
        binding: &TrustedDiscoveryBinding,
        mut request: DiscoveryRequest,
        trusted_routing: RoutingConstraints,
        content_scope: DiscoveryScope,
    ) -> Result<TransientDisclosure<DiscoveryResult>, SearchError> {
        request.access_context = binding.actor().access_handle().to_opaque_string();
        request.temporal_context.evaluation_id = binding.evaluation();
        verify_discovery_binding(
            self.authority,
            binding,
            &request.access_context,
            binding.evaluation(),
        )
        .await?;
        let snapshot =
            prepare_actor_visible_sources(self.authority, self.registry, binding.actor()).await?;
        let mut visible = snapshot.entries().to_vec();
        // Each round drops at least one revoked Source, so this is bounded.
        for _ in 0..=visible.len() {
            let (outcome, evaluation_closed) = self
                .evaluate(
                    binding,
                    &request,
                    &visible,
                    &trusted_routing,
                    &content_scope,
                )
                .await;
            let mut revoked = Vec::new();
            for entry in &visible {
                if self.visibility.current(entry.scope()).await? != AccessDecision::Allowed {
                    revoked.push(entry.scope().source_id());
                }
            }
            check_actor_current(self.authority, binding.actor()).await?;
            if revoked.is_empty() {
                return Ok(TransientDisclosure {
                    payload: Some(outcome?),
                    owner: DisclosureOwner {
                        actor: binding.actor().clone(),
                        sources: visible.iter().map(|entry| entry.scope().clone()).collect(),
                    },
                    deadline: self.clock.now() + self.disclosure_ttl,
                    clock: self.clock.clone(),
                    state: LeaseState::Open,
                    evaluation_closed,
                });
            }
            // Everything derived from a revoked Source is discarded with
            // this evaluation and recomputed from the remaining Sources.
            visible.retain(|entry| !revoked.contains(&entry.scope().source_id()));
        }
        Err(SearchError::SourceUnavailable(
            "visible Sources changed during Discovery".into(),
        ))
    }

    async fn evaluate(
        &self,
        binding: &TrustedDiscoveryBinding,
        request: &DiscoveryRequest,
        visible: &[VisibleSourceRegistration],
        routing: &RoutingConstraints,
        content_scope: &DiscoveryScope,
    ) -> (Result<DiscoveryResult, SearchError>, bool) {
        let durable = self.service.durable_read_ports();
        let gate = ScopedOwnerGate::new(self.authority, self.visibility);
        let view = CompositeEvaluationReadView::new(
            durable.generations,
            durable.concepts,
            durable.selectors,
            durable.assertions,
            durable.evidence,
            self.remote_selectors,
            &gate,
            self.clock.clone(),
        );
        let mut remote = Vec::new();
        for entry in visible {
            let Some(wiring) = self.remote.get(&entry.scope().source_id()) else {
                continue;
            };
            if !matches!(entry.registration(), SourceRegistration::Remote(_)) {
                continue;
            }
            // A Source revoked before its bind is caught by the caller's
            // visibility recheck and recomputed without it.
            let Ok(context) =
                TrustedRemoteContext::bind(binding.clone(), entry, self.authority, self.visibility)
                    .await
            else {
                continue;
            };
            remote.push(RemoteSourceExecution {
                context,
                port: wiring.port,
                lineage: wiring.lineage.clone(),
                provenance: wiring.provenance,
                lease: RemoteLease {
                    absolute_deadline: self.clock.now() + wiring.evaluation_ttl,
                    idle_timeout: wiring.idle_timeout,
                    provider_expiry: None,
                },
            });
        }
        let outcome = self
            .service
            .discover_scoped(
                request.clone(),
                ScopedDiscoveryExecution {
                    content_scope: content_scope.clone(),
                    binding,
                    visible,
                    routing: routing.clone(),
                    remote,
                    view: &view,
                },
            )
            .await;
        view.close();
        (outcome, view.is_closed())
    }
}
