//! Canonical typed N-ary relations; participants never become independent edges.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::RelationPathPattern;
use crate::id::{RelationId, ResourceId};
use crate::predicate::TypedValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelationNamespace {
    Discovery,
    Semantic,
    Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RelationParticipant {
    pub role: String,
    pub resource_ref: ResourceId,
}

impl RelationParticipant {
    pub fn new(role: impl Into<String>, resource_ref: ResourceId) -> Self {
        Self {
            role: role.into(),
            resource_ref,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationTemporalScope {
    pub valid_from: Option<OffsetDateTime>,
    pub valid_to: Option<OffsetDateTime>,
}

impl RelationTemporalScope {
    pub fn contains(&self, target: OffsetDateTime) -> bool {
        self.valid_from.is_none_or(|from| target >= from)
            && self.valid_to.is_none_or(|to| target < to)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedRelationInstance {
    pub relation_id: RelationId,
    pub namespace: RelationNamespace,
    pub relation_type: String,
    pub participants: Vec<RelationParticipant>,
    pub qualifiers: BTreeMap<String, TypedValue>,
    pub temporal_scope: RelationTemporalScope,
    pub authority: Option<String>,
    pub provenance: Option<String>,
    pub evidence_refs: Vec<String>,
}

impl TypedRelationInstance {
    pub fn new(
        relation_id: RelationId,
        namespace: RelationNamespace,
        relation_type: impl Into<String>,
        participants: Vec<RelationParticipant>,
    ) -> Self {
        Self {
            relation_id,
            namespace,
            relation_type: relation_type.into(),
            participants,
            qualifiers: BTreeMap::new(),
            temporal_scope: RelationTemporalScope::default(),
            authority: None,
            provenance: None,
            evidence_refs: Vec::new(),
        }
    }

    pub fn same_participants_as(&self, other: &Self) -> bool {
        let mut left = self.participants.clone();
        let mut right = other.participants.clone();
        left.sort();
        right.sort();
        left == right
    }

    pub fn has_participant(&self, role: &str, resource_ref: Option<ResourceId>) -> bool {
        self.participants.iter().any(|participant| {
            participant.role == role
                && resource_ref.is_none_or(|resource| participant.resource_ref == resource)
        })
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.relation_type.is_empty() || self.participants.len() < 2 {
            return Err("relation requires a type and at least two participants");
        }
        if self
            .participants
            .iter()
            .any(|participant| participant.role.is_empty())
        {
            return Err("participant role must be nonempty");
        }
        if let (Some(from), Some(to)) =
            (self.temporal_scope.valid_from, self.temporal_scope.valid_to)
            && from >= to
        {
            return Err("relation temporal interval must be ordered");
        }
        Ok(())
    }
}

pub fn matching_relation_ids(
    relations: &[TypedRelationInstance],
    pattern: &RelationPathPattern,
) -> Vec<RelationId> {
    relations
        .iter()
        .filter(|relation| pattern.matches_relation(relation))
        .map(|relation| relation.relation_id)
        .collect()
}
