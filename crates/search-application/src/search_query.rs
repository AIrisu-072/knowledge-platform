//! P5-03: the Search route over the actor-visible catalog.
//!
//! `sourceIds` only intersects the visible catalog, so an unknown, hidden or
//! foreign ID reads exactly like no match. Retrieval, hard gates, current
//! access and S1 `PriorityConcat` come from `DiscoveryService::search_visible`
//! (no second Discovery loop). A body scope uses only BodyOnly Unit hits and
//! never claims body coverage a Source did not execute. A cursor is issued
//! only for an authoritatively stamped visible set whose Sources all retain
//! results; otherwise a page that has more is `partial` with
//! `PAGINATION_UNAVAILABLE`. The result leaves only through a
//! `TransientDisclosure` final gate.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use search_core::discovery::{DiscoveryNeed, DiscoveryRequest, GapReason, InformationGap};
use search_core::evidence::EvidenceRequirement;
use search_core::id::{DiscoveryEvaluationId, NeedId, ResourceId, ResourceVersionId, SourceId};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::resource::ResourceKind;
use search_core::source::RetentionMode;
use search_core::temporal::TemporalEvaluationContext;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api_cursor::{CursorBinding, CursorHandle, PublicCursorStore};
use crate::api_scope::{ApiError, SearchOperationContext};
use crate::discovery_service::{DiscoveryService, MatchedField};
use crate::ports::LexicalQuery;
use crate::remote_disclosure::{
    Disclosable, DisclosedFields, DisclosureOwner, TransientDisclosure,
};
use crate::remote_lease::LeaseClock;
use crate::routing::RoutingConstraints;
use crate::scoped::VisibleCatalogSnapshot;

pub const MAX_QUERY_BYTES: usize = 2_048;
pub const MAX_PAGE_SIZE: usize = 100;
pub const MAX_EVALUATED_CANDIDATES: usize = 200;
pub const MAX_PUBLIC_GAPS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchCoverage {
    TitleAndPermittedMetadata,
    BodyRequired,
}

/// The closed public Search input. No actor, tenant or routing field.
#[derive(Clone, PartialEq, Eq)]
pub struct SearchInput {
    pub query: String,
    pub resource_types: Vec<ResourceKind>,
    pub source_ids: Vec<SourceId>,
    pub coverage: SearchCoverage,
    pub page_size: usize,
    pub cursor: Option<CursorHandle>,
}

impl fmt::Debug for SearchInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchInput(<redacted>)")
    }
}

impl SearchInput {
    pub fn validate(&self) -> Result<(), ApiError> {
        let unique = |count: usize, distinct: usize| count == distinct;
        if self.query.trim().is_empty()
            || self.query.len() > MAX_QUERY_BYTES
            || self.resource_types.len() > 8
            || self.source_ids.len() > 16
            || !unique(
                self.source_ids.len(),
                self.source_ids.iter().collect::<BTreeSet<_>>().len(),
            )
            || self.page_size == 0
            || self.page_size > MAX_PAGE_SIZE
        {
            return Err(ApiError::ValidationFailed);
        }
        Ok(())
    }

    /// The request a cursor continues: everything except the cursor.
    fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"search-request:v1");
        hasher.update((self.query.len() as u64).to_be_bytes());
        hasher.update(self.query.as_bytes());
        let mut types: Vec<String> = self
            .resource_types
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect();
        types.sort();
        let mut sources: Vec<_> = self
            .source_ids
            .iter()
            .map(|id| *id.as_uuid().as_bytes())
            .collect();
        sources.sort();
        hasher.update(types.join(",").as_bytes());
        for source in sources {
            hasher.update(source);
        }
        hasher.update([self.coverage as u8]);
        hasher.update((self.page_size as u64).to_be_bytes());
        hasher.finalize().into()
    }
}

/// Registered generic gap codes; no private fact or Source ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicGapCode {
    MissingFact,
    InsufficientEvidenceClass,
    Authority,
    Freshness,
    Corroboration,
    Conflict,
    Availability,
    UnsupportedCoverage,
    PaginationUnavailable,
    RequiredClaimUnresolved,
    RequiredSourceUnavailable,
    BudgetExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PublicGapView {
    pub code: PublicGapCode,
    pub blocking: bool,
}

/// Maps internal gaps to generic codes, deduplicated and bounded.
pub fn public_gaps(gaps: &[InformationGap]) -> Vec<PublicGapView> {
    let mut out = BTreeSet::new();
    for gap in gaps {
        let fact = gap.required_fact.as_str();
        let code = if fact == "required_source_unavailable" {
            PublicGapCode::RequiredSourceUnavailable
        } else if fact.starts_with("claim:") {
            PublicGapCode::RequiredClaimUnresolved
        } else if fact == "max_discovery_actions_reached" {
            PublicGapCode::BudgetExhausted
        } else {
            match gap.reason {
                GapReason::MissingFact => PublicGapCode::MissingFact,
                GapReason::InsufficientEvidenceClass => PublicGapCode::InsufficientEvidenceClass,
                GapReason::Authority => PublicGapCode::Authority,
                GapReason::Freshness => PublicGapCode::Freshness,
                GapReason::Corroboration => PublicGapCode::Corroboration,
                GapReason::Conflict => PublicGapCode::Conflict,
                GapReason::Availability => PublicGapCode::Availability,
                GapReason::UnsupportedCoverage => PublicGapCode::UnsupportedCoverage,
            }
        };
        out.insert(PublicGapView {
            code,
            blocking: gap.blocking,
        });
    }
    out.into_iter().take(MAX_PUBLIC_GAPS).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchItemView {
    pub resource_id: ResourceId,
    pub source_id: SourceId,
    pub resource_type: Option<ResourceKind>,
    pub resource_version: Option<ResourceVersionId>,
    pub title: Option<String>,
    /// One-based S1 rank after the final gate.
    pub rank: usize,
    pub matched: Vec<MatchedField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCoverageView {
    pub source_id: SourceId,
    pub body: bool,
}

/// The allow-listed Search result: borrowed accessors only.
pub struct SearchResultView {
    items: Vec<SearchItemView>,
    next_cursor: Option<CursorHandle>,
    partial: bool,
    coverage: Vec<SearchCoverageView>,
    gaps: Vec<PublicGapView>,
}

impl fmt::Debug for SearchResultView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchResultView(<transient>)")
    }
}

impl SearchResultView {
    pub fn items(&self) -> &[SearchItemView] {
        &self.items
    }
    pub fn next_cursor(&self) -> Option<CursorHandle> {
        self.next_cursor
    }
    pub fn partial(&self) -> bool {
        self.partial
    }
    pub fn coverage(&self) -> &[SearchCoverageView] {
        &self.coverage
    }
    pub fn gaps(&self) -> &[PublicGapView] {
        &self.gaps
    }
}

impl Disclosable for SearchResultView {
    fn disclosed_fields(&self) -> DisclosedFields {
        DisclosedFields {
            resources: self.items.iter().map(|item| item.resource_id).collect(),
            claims: vec![],
        }
    }
}

pub struct SearchQueryService<'a> {
    discovery: &'a DiscoveryService<'a>,
    cursors: &'a PublicCursorStore,
    clock: Arc<dyn LeaseClock>,
    disclosure_ttl: Duration,
}

impl fmt::Debug for SearchQueryService<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SearchQueryService(<wired>)")
    }
}

impl<'a> SearchQueryService<'a> {
    pub fn new(
        discovery: &'a DiscoveryService<'a>,
        cursors: &'a PublicCursorStore,
        clock: Arc<dyn LeaseClock>,
        disclosure_ttl: Duration,
    ) -> Self {
        Self {
            discovery,
            cursors,
            clock,
            disclosure_ttl,
        }
    }

    pub async fn search(
        &self,
        context: &SearchOperationContext,
        snapshot: &VisibleCatalogSnapshot,
        input: SearchInput,
    ) -> Result<TransientDisclosure<SearchResultView>, ApiError> {
        input.validate()?;
        context.check_live()?;
        let visible: Vec<_> = snapshot
            .entries()
            .iter()
            .filter(|entry| {
                input.source_ids.is_empty() || input.source_ids.contains(&entry.scope().source_id())
            })
            .cloned()
            .collect();
        let targets: Vec<SourceId> = visible
            .iter()
            .map(|entry| entry.scope().source_id())
            .collect();
        let mut types: Vec<ResourceKind> = if input.resource_types.is_empty() {
            visible
                .iter()
                .flat_map(|entry| entry.discoverable_source().resource_types)
                .collect()
        } else {
            input.resource_types.clone()
        };
        types.sort_by_key(|kind| format!("{kind:?}"));
        types.dedup();
        let actor = context.actor();
        let now = OffsetDateTime::now_utc();
        let request = DiscoveryRequest {
            need: DiscoveryNeed {
                need_id: NeedId::from_uuid(Uuid::now_v7()),
                intent_signature: IntentSignature::new(IntentFact::new(
                    input.query.clone(),
                    IntentFactOrigin::Explicit,
                )),
                required_resource_types: types.clone(),
                required_claims: vec![],
                authority_requirements: vec![],
                freshness_requirements: vec![],
                constraints: vec![],
                completion_requirement: EvidenceRequirement::new(vec![]),
            },
            // Search binds no Discovery evaluation.
            temporal_context: TemporalEvaluationContext::new(
                DiscoveryEvaluationId::from_uuid(Uuid::nil()),
                now,
                now,
                "UTC",
            ),
            access_context: actor.access_handle().to_opaque_string(),
        };
        let query = match input.coverage {
            SearchCoverage::TitleAndPermittedMetadata => {
                LexicalQuery::new(input.query.clone(), MAX_EVALUATED_CANDIDATES)
            }
            SearchCoverage::BodyRequired => {
                LexicalQuery::body_only(input.query.clone(), MAX_EVALUATED_CANDIDATES)
            }
        };
        let routing = RoutingConstraints {
            required_source_ids: vec![],
            preferred_source_ids: targets.clone(),
            max_initial_optional_sources: targets.len(),
        };
        let outcome = if visible.is_empty() {
            None
        } else {
            Some(
                self.discovery
                    .search_visible(
                        actor,
                        &visible,
                        &routing,
                        request,
                        query,
                        MAX_EVALUATED_CANDIDATES,
                    )
                    .await
                    .map_err(|_| ApiError::DependencyUnavailable)?,
            )
        };
        context.check_live()?;
        let (hits, pins, mut gaps, body_sources, bounded) = match outcome {
            Some(outcome) => (
                outcome.hits,
                outcome.pins,
                outcome.gaps,
                outcome.body_sources,
                outcome.bounded,
            ),
            None => Default::default(),
        };
        let ranked: Vec<SearchItemView> = hits
            .into_iter()
            .filter(|hit| {
                input.resource_types.is_empty()
                    || hit
                        .resource_kind
                        .is_some_and(|kind| input.resource_types.contains(&kind))
            })
            .enumerate()
            .map(|(index, hit)| SearchItemView {
                resource_id: hit.resource_id,
                source_id: hit.source_id,
                resource_type: hit.resource_kind,
                resource_version: hit.resource_version,
                title: hit.title,
                rank: index + 1,
                matched: hit.matched,
            })
            .collect();
        // A continuation is bound to everything that produced this ranking.
        let continuable = snapshot.continuation_stamp().is_some()
            && visible.iter().all(|entry| {
                entry.discoverable_source().retention_mode != RetentionMode::NoRetention
            });
        let binding = snapshot
            .continuation_stamp()
            .map(|stamp| CursorBinding::new(actor, input.digest(), stamp.clone(), pins.clone()));
        let start = match (&input.cursor, &binding) {
            (None, _) => 0,
            (Some(handle), Some(binding)) if continuable => {
                self.cursors.consume(handle, binding)?
            }
            (Some(_), _) => return Err(ApiError::CursorStale),
        };
        let end = (start + input.page_size).min(ranked.len());
        let more = end < ranked.len();
        let next_cursor = match (&binding, more && continuable) {
            (Some(binding), true) => Some(self.cursors.issue(binding.clone(), end)?),
            _ => None,
        };
        if more && next_cursor.is_none() {
            gaps.push(InformationGap::new(
                "pagination_unavailable",
                GapReason::UnsupportedCoverage,
                false,
            ));
        }
        let mut public = public_gaps(&gaps);
        if more && next_cursor.is_none() {
            public.retain(|gap| gap.code != PublicGapCode::UnsupportedCoverage || gap.blocking);
            public.push(PublicGapView {
                code: PublicGapCode::PaginationUnavailable,
                blocking: false,
            });
        }
        let partial = bounded
            || (more && next_cursor.is_none())
            || gaps.iter().any(|gap| gap.reason == GapReason::Availability);
        let coverage = targets
            .iter()
            .map(|source| SearchCoverageView {
                source_id: *source,
                body: input.coverage == SearchCoverage::BodyRequired
                    && body_sources.contains(source),
            })
            .collect();
        let view = SearchResultView {
            items: ranked[start.min(ranked.len())..end].to_vec(),
            next_cursor,
            partial,
            coverage,
            gaps: public,
        };
        Ok(TransientDisclosure::new(
            view,
            DisclosureOwner::new(
                actor.clone(),
                visible.iter().map(|entry| entry.scope().clone()).collect(),
            ),
            self.disclosure_ttl,
            self.clock.clone(),
            true,
        ))
    }
}
