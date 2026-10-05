//! Hard-gated candidate federation with routed retriever-order priority concat.

use std::collections::{BTreeMap, BTreeSet};

use search_core::applicability::ApplicabilityState;
use search_core::discovery::{
    CandidateIdentityClass, GapReason, InformationGap, RejectedCandidate,
};
use search_core::id::{LogicalResourceId, ProjectionGenerationId, ResourceId, SourceId};
use search_core::identity::{IdentityEvidence, IdentityState, resolve_identity};

use crate::candidate::{
    CandidateFederationResult, CandidateHardGates, FusedCandidate, PendingCandidateGroup,
    PendingCandidateHit, RejectedCandidateHit, RepresentationHit, RetrieverHitTrace,
    RetrieverRankList,
};

/// The initial S1 policy. Other rank strategies can be added without changing
/// the input or grouped output contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FusionStrategy {
    PriorityConcat,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FederationError {
    #[error(
        "retriever {retriever_id} hit Source {hit_source:?} differs from list Source {list_source:?}"
    )]
    HitSourceMismatch {
        retriever_id: String,
        list_source: SourceId,
        hit_source: SourceId,
    },
    #[error("Source {source_id:?} mixes projection generations {first:?} and {second:?}")]
    MixedSourceGenerations {
        source_id: SourceId,
        first: ProjectionGenerationId,
        second: ProjectionGenerationId,
    },
}

impl FusionStrategy {
    fn fuse(
        self,
        eligible_lists: Vec<Vec<RepresentationHit>>,
        associations: &BTreeMap<LocalIdentity, Association>,
    ) -> Vec<FusedCandidate> {
        match self {
            Self::PriorityConcat => priority_concat(eligible_lists, associations),
        }
    }
}

pub struct CandidateFederator;

impl CandidateFederator {
    pub fn merge(
        lists: &[RetrieverRankList],
        strategy: FusionStrategy,
    ) -> Result<CandidateFederationResult, FederationError> {
        validate_lists(lists)?;
        let mut eligible_lists = Vec::with_capacity(lists.len());
        let mut pending = Vec::new();
        let mut rejected = Vec::new();

        for list in lists {
            let mut eligible = Vec::new();
            for (index, input) in list.hits.iter().enumerate() {
                let state = gate_state(&input.hard_gates);
                let trace = RetrieverHitTrace {
                    retriever_id: list.retriever_id.clone(),
                    generation: list.generation,
                    rank: index + 1,
                    raw_score: input.raw_score,
                    evidence_refs: input.evidence_refs.clone(),
                };
                let hit = RepresentationHit {
                    candidate: input.candidate.clone(),
                    hard_gates: input.hard_gates.clone(),
                    identity_evidence: input.identity_evidence.clone(),
                    trace,
                };
                match state {
                    ApplicabilityState::Applicable => eligible.push(hit),
                    ApplicabilityState::Unresolved => pending.push(PendingCandidateHit {
                        gaps: gate_gaps(&hit.hard_gates),
                        reason_trace: gate_reasons(&hit.hard_gates),
                        hit,
                    }),
                    ApplicabilityState::Excluded | ApplicabilityState::Invalid => {
                        rejected.push(RejectedCandidateHit {
                            rejection: RejectedCandidate {
                                candidate_id: hit.candidate.candidate_id.clone(),
                                state,
                                reason_trace: gate_reasons(&hit.hard_gates),
                            },
                            hit,
                        });
                    }
                }
            }
            eligible_lists.push(eligible);
        }

        let associations = resolved_associations(
            eligible_lists
                .iter()
                .flatten()
                .chain(pending.iter().map(|item: &PendingCandidateHit| &item.hit))
                .chain(rejected.iter().map(|item: &RejectedCandidateHit| &item.hit)),
        );
        let ranked = strategy.fuse(eligible_lists, &associations);
        Ok(CandidateFederationResult {
            ranked,
            pending: group_pending(pending, &associations),
            rejected,
        })
    }
}

fn validate_lists(lists: &[RetrieverRankList]) -> Result<(), FederationError> {
    let mut generations = BTreeMap::new();
    for list in lists {
        let source_id = list.generation.source_id;
        if let Some(first) = generations.insert(source_id, list.generation.generation_id)
            && first != list.generation.generation_id
        {
            return Err(FederationError::MixedSourceGenerations {
                source_id,
                first,
                second: list.generation.generation_id,
            });
        }
        for hit in &list.hits {
            if hit.candidate.source_ref != source_id {
                return Err(FederationError::HitSourceMismatch {
                    retriever_id: list.retriever_id.clone(),
                    list_source: source_id,
                    hit_source: hit.candidate.source_ref,
                });
            }
        }
    }
    Ok(())
}

fn gate_state(gates: &CandidateHardGates) -> ApplicabilityState {
    let states = [
        gates.applicability.state,
        gates.structured.state,
        gates.access.state,
        gates.temporal.state,
    ];
    if states.contains(&ApplicabilityState::Invalid) {
        ApplicabilityState::Invalid
    } else if states.contains(&ApplicabilityState::Excluded) {
        ApplicabilityState::Excluded
    } else if states.contains(&ApplicabilityState::Unresolved) {
        ApplicabilityState::Unresolved
    } else {
        ApplicabilityState::Applicable
    }
}

fn gate_gaps(gates: &CandidateHardGates) -> Vec<InformationGap> {
    let mut gaps = Vec::new();
    for (state, supplied, required_fact, reason) in [
        (
            gates.applicability.state,
            &gates.applicability.gaps,
            "applicability",
            GapReason::MissingFact,
        ),
        (
            gates.structured.state,
            &gates.structured.gaps,
            "structured_hard_gate",
            GapReason::MissingFact,
        ),
        (
            gates.access.state,
            &gates.access.gaps,
            "current_access",
            GapReason::Availability,
        ),
        (
            gates.temporal.state,
            &gates.temporal.gaps,
            "temporal_hard_gate",
            GapReason::MissingFact,
        ),
    ] {
        gaps.extend(supplied.iter().cloned().map(|mut gap| {
            if state == ApplicabilityState::Unresolved {
                gap.blocking = true;
            }
            gap
        }));
        if state == ApplicabilityState::Unresolved && supplied.is_empty() {
            gaps.push(InformationGap::new(required_fact, reason, true));
        }
    }
    gaps
}

fn gate_reasons(gates: &CandidateHardGates) -> Vec<String> {
    [
        (
            "applicability",
            gates.applicability.state,
            &gates.applicability.reasons,
        ),
        (
            "structured",
            gates.structured.state,
            &gates.structured.reasons,
        ),
        ("access", gates.access.state, &gates.access.reasons),
        ("temporal", gates.temporal.state, &gates.temporal.reasons),
    ]
    .into_iter()
    .filter(|(_, state, _)| *state != ApplicabilityState::Applicable)
    .flat_map(|(name, state, reasons)| {
        if reasons.is_empty() {
            vec![format!("{name}: {state:?}")]
        } else {
            reasons
                .iter()
                .map(|reason| format!("{name}: {reason}"))
                .collect()
        }
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum LocalIdentity {
    Resource(SourceId, ResourceId),
    RemoteStableReference(SourceId, String),
}

#[derive(Debug, Clone, Copy)]
enum Association {
    Unique(LogicalResourceId),
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Logical(LogicalResourceId),
    Local(LocalIdentity),
    Unique(usize),
}

fn local_identity(hit: &RepresentationHit) -> Option<LocalIdentity> {
    let candidate = &hit.candidate;
    if let Some(resource_ref) = candidate.resource_ref {
        Some(LocalIdentity::Resource(candidate.source_ref, resource_ref))
    } else if candidate.identity_class == CandidateIdentityClass::RemoteStableReference
        && !candidate.candidate_id.is_empty()
    {
        Some(LocalIdentity::RemoteStableReference(
            candidate.source_ref,
            candidate.candidate_id.clone(),
        ))
    } else {
        None
    }
}

fn resolved_logical(hit: &RepresentationHit) -> Option<LogicalResourceId> {
    let logical = hit.candidate.logical_resource_ref?;
    (resolve_identity(&hit.identity_evidence) == IdentityState::Resolved).then_some(logical)
}

fn priority_concat(
    eligible_lists: Vec<Vec<RepresentationHit>>,
    associations: &BTreeMap<LocalIdentity, Association>,
) -> Vec<FusedCandidate> {
    let eligible = eligible_lists.into_iter().flatten().collect::<Vec<_>>();
    let mut ranked: Vec<FusedCandidate> = Vec::new();
    let mut positions: BTreeMap<GroupKey, usize> = BTreeMap::new();
    for (index, hit) in eligible.into_iter().enumerate() {
        let key = group_key(&hit, associations, index);
        if let Some(position) = positions.get(&key).copied() {
            ranked[position].hits.push(hit);
        } else {
            positions.insert(key.clone(), ranked.len());
            ranked.push(FusedCandidate {
                logical_resource_ref: match key {
                    GroupKey::Logical(logical) => Some(logical),
                    GroupKey::Local(_) | GroupKey::Unique(_) => None,
                },
                hits: vec![hit],
            });
        }
    }
    ranked
}

fn group_pending(
    pending: Vec<PendingCandidateHit>,
    associations: &BTreeMap<LocalIdentity, Association>,
) -> Vec<PendingCandidateGroup> {
    let mut groups: Vec<PendingCandidateGroup> = Vec::new();
    let mut positions: BTreeMap<GroupKey, usize> = BTreeMap::new();
    for (index, item) in pending.into_iter().enumerate() {
        let key = group_key(&item.hit, associations, index);
        if let Some(position) = positions.get(&key).copied() {
            groups[position].hits.push(item);
        } else {
            positions.insert(key.clone(), groups.len());
            groups.push(PendingCandidateGroup {
                logical_resource_ref: match key {
                    GroupKey::Logical(logical) => Some(logical),
                    GroupKey::Local(_) | GroupKey::Unique(_) => None,
                },
                hits: vec![item],
            });
        }
    }
    groups
}

fn resolved_associations<'a>(
    hits: impl IntoIterator<Item = &'a RepresentationHit>,
) -> BTreeMap<LocalIdentity, Association> {
    let mut claims: BTreeMap<LocalIdentity, BTreeMap<LogicalResourceId, Vec<IdentityEvidence>>> =
        BTreeMap::new();
    let mut unclaimed_conflicts = BTreeSet::new();
    for hit in hits {
        let Some(local) = local_identity(hit) else {
            continue;
        };
        if let Some(logical) = hit.candidate.logical_resource_ref {
            claims
                .entry(local)
                .or_default()
                .entry(logical)
                .or_default()
                .extend(hit.identity_evidence.iter().cloned());
        } else if resolve_identity(&hit.identity_evidence) == IdentityState::Conflict {
            unclaimed_conflicts.insert(local);
        }
    }

    let mut associations = BTreeMap::new();
    for (local, logical_claims) in claims {
        let mut resolved = None;
        let mut conflict = false;
        for (logical, evidence) in logical_claims {
            match resolve_identity(&evidence) {
                IdentityState::Conflict => {
                    conflict = true;
                    break;
                }
                IdentityState::Resolved => {
                    if resolved.replace(logical).is_some() {
                        conflict = true;
                        break;
                    }
                }
                IdentityState::Provisional | IdentityState::Unresolved => {}
            }
        }
        if conflict {
            associations.insert(local, Association::Conflict);
        } else if let Some(logical) = resolved {
            associations.insert(local, Association::Unique(logical));
        }
    }
    for local in unclaimed_conflicts {
        associations.insert(local, Association::Conflict);
    }
    associations
}

fn group_key(
    hit: &RepresentationHit,
    associations: &BTreeMap<LocalIdentity, Association>,
    index: usize,
) -> GroupKey {
    let local = local_identity(hit);
    match local.as_ref().and_then(|key| associations.get(key)) {
        Some(Association::Unique(logical)) => GroupKey::Logical(*logical),
        Some(Association::Conflict) => GroupKey::Local(local.expect("matched local identity")),
        None => match (resolved_logical(hit), local) {
            (Some(logical), _) => GroupKey::Logical(logical),
            (None, Some(local)) => GroupKey::Local(local),
            (None, None) => GroupKey::Unique(index),
        },
    }
}
