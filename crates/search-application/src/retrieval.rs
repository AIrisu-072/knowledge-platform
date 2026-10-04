//! Profile-ordered retrieval plans. This module schedules actions; it never runs ports.

use std::collections::{BTreeMap, BTreeSet};

use search_core::discovery::{GapReason, InformationGap};
use search_core::graph::GraphTraversalPlan;
use search_core::id::SourceId;
use search_core::predicate::TypedValue;
use search_core::source::{DiscoveryMode, EnumerationSemantics};

use crate::SearchError;
use crate::ports::StructuredFacetFilter;
use crate::remote_registration::RemoteRegistrationLimits;
use crate::routing::{RouteStage, RouteState, SourceRole, SourceRoute, SourceRoutePlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrieverProfile {
    Identity,
    Capability,
    Knowledge,
    EvidenceInvestigation,
    Exploratory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrieverKind {
    Directory,
    Structured,
    Lexical,
    HyperGraph,
    Vector,
    RemoteEnumeration,
    RemoteQuery,
    DirectAddress,
    LiveOnly,
}

impl RetrieverProfile {
    fn local_order(self) -> [RetrieverKind; 5] {
        use RetrieverKind::{Directory, HyperGraph, Lexical, Structured, Vector};
        match self {
            Self::Identity => [Directory, Structured, Lexical, HyperGraph, Vector],
            Self::Capability => [Structured, HyperGraph, Directory, Lexical, Vector],
            Self::Knowledge => [Structured, Lexical, Directory, HyperGraph, Vector],
            Self::EvidenceInvestigation => [HyperGraph, Structured, Lexical, Directory, Vector],
            Self::Exploratory => [Lexical, Vector, HyperGraph, Directory, Structured],
        }
    }
}

/// Runtime wiring is supplied explicitly; a trait existing in `ports.rs` does
/// not establish that its adapter is installed for this evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetrieverSupport {
    pub directory: bool,
    pub structured: bool,
    pub lexical: bool,
    pub hypergraph: bool,
    pub vector: bool,
    pub remote_enumeration: bool,
    pub remote_query: bool,
    pub direct_address: bool,
    pub live_only: bool,
}

impl RetrieverSupport {
    fn contains(self, kind: RetrieverKind) -> bool {
        match kind {
            RetrieverKind::Directory => self.directory,
            RetrieverKind::Structured => self.structured,
            RetrieverKind::Lexical => self.lexical,
            RetrieverKind::HyperGraph => self.hypergraph,
            RetrieverKind::Vector => self.vector,
            RetrieverKind::RemoteEnumeration => self.remote_enumeration,
            RetrieverKind::RemoteQuery => self.remote_query,
            RetrieverKind::DirectAddress => self.direct_address,
            RetrieverKind::LiveOnly => self.live_only,
        }
    }
}

fn invalid_remote_input() -> SearchError {
    SearchError::InvalidRequest("invalid remote retrieval input".into())
}

/// Bounded query data; facet names come from the trusted adapter allowlist.
/// The initial contract accepts scalar typed facets only (no recursive values).
/// These are canary input bounds, not production transport/SLO qualification.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteQueryInput {
    text: String,
    facets: Vec<StructuredFacetFilter>,
    window: usize,
}

impl std::fmt::Debug for RemoteQueryInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RemoteQueryInput([redacted])")
    }
}

impl RemoteQueryInput {
    pub fn new(
        text: impl Into<String>,
        facets: Vec<StructuredFacetFilter>,
        window: usize,
        allowed_facets: &[&str],
    ) -> Result<Self, SearchError> {
        let text = text.into();
        let limits = RemoteRegistrationLimits::synthetic_canary();
        if text.len() > 4096
            || (text.trim().is_empty() && facets.is_empty())
            || facets.len() > 16
            || window == 0
            || window > limits.max_hits_per_page
        {
            return Err(invalid_remote_input());
        }
        let mut names = BTreeSet::new();
        for facet in &facets {
            if facet.facet.trim().is_empty()
                || facet.facet.len() > 128
                || !allowed_facets.contains(&facet.facet.as_str())
                || !names.insert(&facet.facet)
                || !bounded_scalar(&facet.expected)
            {
                return Err(invalid_remote_input());
            }
        }
        // Account for escaped UTF-8 input as well as raw string bounds. The
        // eventual adapter must independently bound its complete wire request.
        let encoded_facets: Vec<_> = facets
            .iter()
            .map(|facet| (&facet.facet, &facet.expected))
            .collect();
        let encoded = serde_json::to_vec(&(&text, encoded_facets, window))
            .map_err(|_| invalid_remote_input())?;
        if encoded.len() > limits.max_request_bytes {
            return Err(invalid_remote_input());
        }
        Ok(Self {
            text,
            facets,
            window,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn facets(&self) -> &[StructuredFacetFilter] {
        &self.facets
    }

    pub const fn window(&self) -> usize {
        self.window
    }
}

fn bounded_scalar(value: &TypedValue) -> bool {
    match value {
        TypedValue::String(value) | TypedValue::ConceptRef(value) => value.len() <= 1024,
        TypedValue::Money(value) => value.currency.len() <= 1024,
        TypedValue::Quantity(value) => value.unit.len() <= 1024,
        TypedValue::List(_) | TypedValue::Set(_) => false,
        TypedValue::Bool(_)
        | TypedValue::Integer(_)
        | TypedValue::Decimal(_)
        | TypedValue::Date(_)
        | TypedValue::DateTime(_)
        | TypedValue::Duration(_)
        | TypedValue::ResourceRef(_) => true,
    }
}

/// Source-native identity. It is never an endpoint or a fetch destination.
#[derive(Clone, PartialEq, Eq)]
pub struct OpaqueNativeId(String);

impl std::fmt::Debug for OpaqueNativeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OpaqueNativeId([redacted])")
    }
}

impl OpaqueNativeId {
    pub fn new(value: impl Into<String>) -> Result<Self, SearchError> {
        let value = value.into();
        let has_scheme = value
            .trim_start()
            .split_once(':')
            .is_some_and(|(scheme, _)| {
                scheme
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic)
                    && scheme.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-')
                    })
            });
        if value.trim().is_empty()
            || value.len() > RemoteRegistrationLimits::synthetic_canary().max_native_id_bytes
            || value.chars().any(char::is_control)
            || value.contains("://")
            || value.trim_start().starts_with("//")
            || has_scheme
        {
            return Err(invalid_remote_input());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exactly one bounded query or direct lookup, with no reusable collection.
#[derive(Clone, PartialEq, Eq)]
pub struct LiveInput {
    query: Option<RemoteQueryInput>,
    native_id: Option<OpaqueNativeId>,
}

impl std::fmt::Debug for LiveInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LiveInput([redacted])")
    }
}

impl LiveInput {
    pub fn query(query: RemoteQueryInput) -> Self {
        Self {
            query: Some(query),
            native_id: None,
        }
    }

    pub fn lookup(native_id: OpaqueNativeId) -> Self {
        Self {
            query: None,
            native_id: Some(native_id),
        }
    }

    pub fn query_input(&self) -> Option<&RemoteQueryInput> {
        self.query.as_ref()
    }

    pub fn native_id(&self) -> Option<&OpaqueNativeId> {
        self.native_id.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetrievalInputs {
    pub lexical_query: Option<String>,
    /// A graph plan is source-local and must pass the core traversal validator.
    pub graph_plans: BTreeMap<SourceId, GraphTraversalPlan>,
    pub vector_query_available: bool,
    /// Remote inputs are explicit and source-local; local query text is never
    /// implicitly transmitted to a remote provider.
    pub remote_queries: BTreeMap<SourceId, RemoteQueryInput>,
    pub native_ids: BTreeMap<SourceId, OpaqueNativeId>,
    pub live_inputs: BTreeMap<SourceId, LiveInput>,
    /// Trusted fan-out limit. The default of zero schedules no initial calls.
    pub max_initial_retrievers_per_source: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionIssue {
    RouteUnresolved,
    AdapterUnavailable,
    MissingLexicalQuery,
    MissingGraphPlan,
    MissingVectorQuery,
    MissingRemoteQuery,
    MissingNativeId,
    MissingLiveInput,
    NoExecutionPort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    /// Eligible for a later executor; not an execution result.
    Planned,
    Unresolved(ActionIssue),
    Unsupported(ActionIssue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorExecution {
    /// The current retrieval ports return whole lists without a cursor/offset.
    PlanningOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrieverCursorState {
    pub requested_limit: Option<usize>,
    pub execution: CursorExecution,
    pub reuses_prior_results: bool,
}

impl RetrieverCursorState {
    pub const fn initial() -> Self {
        Self {
            requested_limit: None,
            execution: CursorExecution::PlanningOnly,
            reuses_prior_results: false,
        }
    }

    /// Records only a desired larger lexical window. A cursor-aware port must
    /// exist before an executor can claim an incremental fetch was performed.
    pub fn plan_window_expansion(self, requested_limit: usize) -> Self {
        Self {
            requested_limit: Some(self.requested_limit.unwrap_or(0).max(requested_limit)),
            execution: CursorExecution::PlanningOnly,
            reuses_prior_results: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalAction {
    pub source_id: SourceId,
    /// Stable identifier to use in routed `RetrieverRankList` inputs for S1.
    pub retriever_id: String,
    pub retriever: RetrieverKind,
    pub stage: RouteStage,
    pub state: ActionState,
    pub cursor: RetrieverCursorState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalPlan {
    pub initial_actions: Vec<RetrievalAction>,
    pub expansion_actions: Vec<RetrievalAction>,
    /// Explicit route/retriever priority order; only planned initial actions.
    pub s1_retriever_order: Vec<String>,
    pub blocking_gaps: Vec<InformationGap>,
}

pub struct RetrieverPlanner;

impl RetrieverPlanner {
    pub fn plan(
        profile: RetrieverProfile,
        routes: &SourceRoutePlan,
        support: &RetrieverSupport,
        inputs: &RetrievalInputs,
    ) -> RetrievalPlan {
        let mut plan = RetrievalPlan {
            initial_actions: vec![],
            expansion_actions: vec![],
            s1_retriever_order: vec![],
            blocking_gaps: vec![],
        };
        for route in &routes.routes {
            plan.blocking_gaps.extend(
                route
                    .unresolved_gaps
                    .iter()
                    .filter(|gap| gap.blocking)
                    .cloned(),
            );
            let mut initial_for_source = 0;
            for kind in profile.local_order() {
                if !source_supports_kind(route, kind) {
                    continue;
                }
                let state = local_action_state(route, kind, *support, inputs);
                let stage = if route.stage == RouteStage::Initial
                    && state == ActionState::Planned
                    && initial_for_source < inputs.max_initial_retrievers_per_source
                {
                    initial_for_source += 1;
                    RouteStage::Initial
                } else {
                    RouteStage::Expansion
                };
                push_action(&mut plan, route.source_id, kind, stage, state);
            }
            for (mode, kind) in [
                (
                    DiscoveryMode::RemoteEnumeration,
                    RetrieverKind::RemoteEnumeration,
                ),
                (DiscoveryMode::RemoteQuery, RetrieverKind::RemoteQuery),
                (DiscoveryMode::DirectAddress, RetrieverKind::DirectAddress),
                (DiscoveryMode::LiveOnly, RetrieverKind::LiveOnly),
            ] {
                if route.discovery_modes.contains(&mode) {
                    let state = remote_action_state(route, kind, *support, inputs);
                    let stage = if route.stage == RouteStage::Initial
                        && state == ActionState::Planned
                        && initial_for_source < inputs.max_initial_retrievers_per_source
                    {
                        initial_for_source += 1;
                        RouteStage::Initial
                    } else {
                        RouteStage::Expansion
                    };
                    push_action(&mut plan, route.source_id, kind, stage, state);
                }
            }
            if route.role == SourceRole::Required && initial_for_source == 0 {
                plan.blocking_gaps.push(InformationGap::new(
                    format!("source:{:?}:no_planned_initial_retriever", route.source_id),
                    GapReason::Availability,
                    true,
                ));
            }
        }
        if plan.initial_actions.is_empty() {
            plan.blocking_gaps.push(InformationGap::new(
                "initial_retrieval_unplanned",
                GapReason::Availability,
                true,
            ));
        }
        plan
    }
}

fn source_supports_kind(route: &SourceRoute, kind: RetrieverKind) -> bool {
    match kind {
        RetrieverKind::Directory | RetrieverKind::Structured | RetrieverKind::HyperGraph => route
            .discovery_modes
            .contains(&DiscoveryMode::LocalDirectory),
        RetrieverKind::Lexical | RetrieverKind::Vector => route
            .discovery_modes
            .contains(&DiscoveryMode::LocalContentSearch),
        RetrieverKind::RemoteEnumeration
        | RetrieverKind::RemoteQuery
        | RetrieverKind::DirectAddress
        | RetrieverKind::LiveOnly => false,
    }
}

fn local_action_state(
    route: &SourceRoute,
    kind: RetrieverKind,
    support: RetrieverSupport,
    inputs: &RetrievalInputs,
) -> ActionState {
    if route.state != RouteState::Planned {
        return ActionState::Unresolved(ActionIssue::RouteUnresolved);
    }
    if !support.contains(kind) {
        return ActionState::Unsupported(ActionIssue::AdapterUnavailable);
    }
    match kind {
        RetrieverKind::Lexical
            if inputs
                .lexical_query
                .as_ref()
                .is_none_or(|query| query.trim().is_empty()) =>
        {
            ActionState::Unresolved(ActionIssue::MissingLexicalQuery)
        }
        RetrieverKind::HyperGraph
            if inputs
                .graph_plans
                .get(&route.source_id)
                .is_none_or(|plan| plan.validate().is_err()) =>
        {
            ActionState::Unresolved(ActionIssue::MissingGraphPlan)
        }
        RetrieverKind::Vector if !inputs.vector_query_available => {
            ActionState::Unresolved(ActionIssue::MissingVectorQuery)
        }
        _ => ActionState::Planned,
    }
}

fn remote_action_state(
    route: &SourceRoute,
    kind: RetrieverKind,
    support: RetrieverSupport,
    inputs: &RetrievalInputs,
) -> ActionState {
    if !support.contains(kind) {
        return ActionState::Unsupported(ActionIssue::NoExecutionPort);
    }
    if route.state != RouteState::Planned {
        return ActionState::Unresolved(ActionIssue::RouteUnresolved);
    }
    match kind {
        RetrieverKind::RemoteQuery if !inputs.remote_queries.contains_key(&route.source_id) => {
            ActionState::Unresolved(ActionIssue::MissingRemoteQuery)
        }
        RetrieverKind::DirectAddress if !inputs.native_ids.contains_key(&route.source_id) => {
            ActionState::Unresolved(ActionIssue::MissingNativeId)
        }
        RetrieverKind::LiveOnly if !inputs.live_inputs.contains_key(&route.source_id) => {
            ActionState::Unresolved(ActionIssue::MissingLiveInput)
        }
        _ => ActionState::Planned,
    }
}

fn push_action(
    plan: &mut RetrievalPlan,
    source_id: SourceId,
    retriever: RetrieverKind,
    stage: RouteStage,
    state: ActionState,
) {
    let retriever_id = format!("{}:{retriever:?}", source_id.as_uuid());
    let action = RetrievalAction {
        source_id,
        retriever_id: retriever_id.clone(),
        retriever,
        stage,
        state,
        cursor: RetrieverCursorState::initial(),
    };
    if stage == RouteStage::Initial {
        plan.s1_retriever_order.push(retriever_id);
        plan.initial_actions.push(action);
    } else {
        plan.expansion_actions.push(action);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalObservation {
    QueryMiss,
    CompleteEnumerationMiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceAssessment {
    AbsentByCompleteEnumeration,
    Unresolved,
}

/// Query and probe misses remain unresolved. Only an explicitly complete
/// enumeration from a ready directory route can support source-local absence.
pub fn assess_presence(
    route: &SourceRoute,
    observation: RetrievalObservation,
) -> PresenceAssessment {
    if observation == RetrievalObservation::CompleteEnumerationMiss
        && route.state == RouteState::Planned
        && route.enumeration_semantics == Some(EnumerationSemantics::Complete)
        && route
            .discovery_modes
            .contains(&DiscoveryMode::LocalDirectory)
    {
        PresenceAssessment::AbsentByCompleteEnumeration
    } else {
        PresenceAssessment::Unresolved
    }
}
