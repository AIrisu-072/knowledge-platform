//! Profile-ordered retrieval plans. This module schedules actions; it never runs ports.

use std::collections::BTreeMap;

use search_core::discovery::{GapReason, InformationGap};
use search_core::graph::GraphTraversalPlan;
use search_core::id::SourceId;
use search_core::source::{DiscoveryMode, EnumerationSemantics};

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
}

impl RetrieverSupport {
    fn contains(self, kind: RetrieverKind) -> bool {
        match kind {
            RetrieverKind::Directory => self.directory,
            RetrieverKind::Structured => self.structured,
            RetrieverKind::Lexical => self.lexical,
            RetrieverKind::HyperGraph => self.hypergraph,
            RetrieverKind::Vector => self.vector,
            RetrieverKind::RemoteEnumeration
            | RetrieverKind::RemoteQuery
            | RetrieverKind::DirectAddress
            | RetrieverKind::LiveOnly => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetrievalInputs {
    pub lexical_query: Option<String>,
    /// A graph plan is source-local and must pass the core traversal validator.
    pub graph_plans: BTreeMap<SourceId, GraphTraversalPlan>,
    pub vector_query_available: bool,
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
                    push_action(
                        &mut plan,
                        route.source_id,
                        kind,
                        RouteStage::Expansion,
                        ActionState::Unsupported(ActionIssue::NoExecutionPort),
                    );
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
