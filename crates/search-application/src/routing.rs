//! Deterministic Source routing from trusted constraints and registry capabilities.

use std::collections::{BTreeMap, BTreeSet};

use search_core::discovery::{DiscoveryNeed, GapReason, InformationGap};
use search_core::id::{ClaimId, SourceId};
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRole {
    Required,
    Preferred,
    Expansion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStage {
    Initial,
    Expansion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteIssue {
    MissingRegistryEntry,
    ConflictingRegistryEntries,
    ResourceTypesUnknown,
    ResourceTypeMismatch,
    NoDiscoveryMode,
    RuntimePortUnavailable,
}

/// `Planned` says a registered mode has runtime support, not that retrieval ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteState {
    Planned,
    Unresolved(RouteIssue),
    Unsupported(RouteIssue),
}

/// Required/Preferred IDs are trusted caller input. A `DiscoveryRequest` does
/// not carry these IDs or an initial fan-out budget.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoutingConstraints {
    pub required_source_ids: Vec<SourceId>,
    pub preferred_source_ids: Vec<SourceId>,
    /// Limits only optional initial routes. Default zero starts none; Required
    /// routes are never capped.
    pub max_initial_optional_sources: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoute {
    pub source_id: SourceId,
    pub role: SourceRole,
    pub stage: RouteStage,
    pub discovery_mode: Option<DiscoveryMode>,
    pub discovery_modes: Vec<DiscoveryMode>,
    pub enumeration_semantics: Option<EnumerationSemantics>,
    pub state: RouteState,
    pub required_claims: Vec<ClaimId>,
    pub authority_requirements: Vec<String>,
    pub freshness_requirements: Vec<String>,
    /// Registry metadata cannot prove authority or freshness for a candidate.
    pub unresolved_gaps: Vec<InformationGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoutePlan {
    /// Required IDs, explicit Preferred IDs, then other registry IDs in stable order.
    pub routes: Vec<SourceRoute>,
}

pub struct SourceRouter;

impl SourceRouter {
    pub fn plan(
        need: &DiscoveryNeed,
        registry_sources: &[DiscoverableSource],
        constraints: &RoutingConstraints,
    ) -> SourceRoutePlan {
        Self::plan_with_runtime_modes(
            need,
            registry_sources,
            constraints,
            &[
                DiscoveryMode::LocalDirectory,
                DiscoveryMode::LocalContentSearch,
            ],
        )
    }

    /// Explicit wiring for this evaluation. Remote callers first obtain both
    /// sources and constraints from `VisibleRouting::prepare`; advertised modes
    /// alone do not establish a runtime port or current visibility.
    pub fn plan_with_runtime_modes(
        need: &DiscoveryNeed,
        registry_sources: &[DiscoverableSource],
        constraints: &RoutingConstraints,
        runtime_modes: &[DiscoveryMode],
    ) -> SourceRoutePlan {
        let mut sources: BTreeMap<SourceId, Option<&DiscoverableSource>> = BTreeMap::new();
        for source in registry_sources {
            // Any collision invalidates all metadata for that ID, regardless
            // of input order or the number of colliding entries.
            sources
                .entry(source.source_id)
                .and_modify(|stored| *stored = None)
                .or_insert(Some(source));
        }

        let required: BTreeSet<_> = constraints.required_source_ids.iter().copied().collect();
        let preferred: BTreeSet<_> = constraints.preferred_source_ids.iter().copied().collect();
        let ordered = constraints
            .required_source_ids
            .iter()
            .chain(&constraints.preferred_source_ids)
            .copied()
            .chain(sources.keys().copied());
        let mut seen = BTreeSet::new();
        let mut optional_initial = 0;
        let mut routes = Vec::new();

        for source_id in ordered {
            if !seen.insert(source_id) {
                continue;
            }
            let conflict = sources.get(&source_id).is_some_and(Option::is_none);
            let source = sources.get(&source_id).copied().flatten();
            let explicit = required.contains(&source_id) || preferred.contains(&source_id);
            let type_match = source.is_some_and(|source| {
                need.required_resource_types.is_empty()
                    || source
                        .resource_types
                        .iter()
                        .any(|kind| need.required_resource_types.contains(kind))
            });
            let types_unknown = source.is_some_and(|source| source.resource_types.is_empty())
                && !need.required_resource_types.is_empty();
            if !explicit && !type_match && !types_unknown && !conflict {
                continue;
            }

            let role = if required.contains(&source_id) {
                SourceRole::Required
            } else if preferred.contains(&source_id) || type_match {
                SourceRole::Preferred
            } else {
                SourceRole::Expansion
            };
            let discovery_modes = source.map_or_else(Vec::new, |source| {
                // Normalize registry order so equivalent capability sets route identically.
                [
                    DiscoveryMode::LocalDirectory,
                    DiscoveryMode::LocalContentSearch,
                    DiscoveryMode::RemoteEnumeration,
                    DiscoveryMode::RemoteQuery,
                    DiscoveryMode::DirectAddress,
                    DiscoveryMode::LiveOnly,
                ]
                .into_iter()
                .filter(|mode| source.supports(*mode))
                .collect()
            });
            let discovery_mode = discovery_modes.first().copied();
            let state = if conflict {
                RouteState::Unresolved(RouteIssue::ConflictingRegistryEntries)
            } else if source.is_none() {
                RouteState::Unresolved(RouteIssue::MissingRegistryEntry)
            } else if !type_match && !types_unknown {
                RouteState::Unresolved(RouteIssue::ResourceTypeMismatch)
            } else if types_unknown {
                RouteState::Unresolved(RouteIssue::ResourceTypesUnknown)
            } else if discovery_mode.is_none() {
                RouteState::Unresolved(RouteIssue::NoDiscoveryMode)
            } else if !discovery_modes
                .iter()
                .any(|mode| runtime_modes.contains(mode))
            {
                RouteState::Unsupported(RouteIssue::RuntimePortUnavailable)
            } else {
                RouteState::Planned
            };
            let stage = if role == SourceRole::Required {
                RouteStage::Initial
            } else if role == SourceRole::Preferred
                && state == RouteState::Planned
                && optional_initial < constraints.max_initial_optional_sources
            {
                optional_initial += 1;
                RouteStage::Initial
            } else {
                RouteStage::Expansion
            };

            let mut unresolved_gaps = Vec::new();
            if let RouteState::Unresolved(issue) | RouteState::Unsupported(issue) = state {
                unresolved_gaps.push(InformationGap::new(
                    format!("source:{source_id:?}:{issue:?}"),
                    match issue {
                        RouteIssue::MissingRegistryEntry
                        | RouteIssue::ConflictingRegistryEntries
                        | RouteIssue::NoDiscoveryMode
                        | RouteIssue::RuntimePortUnavailable => GapReason::Availability,
                        RouteIssue::ResourceTypesUnknown | RouteIssue::ResourceTypeMismatch => {
                            GapReason::UnsupportedCoverage
                        }
                    },
                    role == SourceRole::Required,
                ));
            }
            for requirement in &need.authority_requirements {
                unresolved_gaps.push(InformationGap::new(
                    format!("authority:{requirement}"),
                    GapReason::Authority,
                    true,
                ));
            }
            for requirement in &need.freshness_requirements {
                unresolved_gaps.push(InformationGap::new(
                    format!("freshness:{requirement}"),
                    GapReason::Freshness,
                    true,
                ));
            }

            routes.push(SourceRoute {
                source_id,
                role,
                stage,
                discovery_mode,
                discovery_modes,
                enumeration_semantics: source.map(|source| source.enumeration_semantics),
                state,
                required_claims: need.required_claims.clone(),
                authority_requirements: need.authority_requirements.clone(),
                freshness_requirements: need.freshness_requirements.clone(),
                unresolved_gaps,
            });
        }

        SourceRoutePlan { routes }
    }
}
