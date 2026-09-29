//! Bounded traversal requests and relation-preserving path evidence.

use serde::{Deserialize, Serialize};

use crate::id::{RelationId, ResourceId};
use crate::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use crate::temporal::TemporalEvaluationContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationPathPattern {
    pub namespace: RelationNamespace,
    pub relation_type: String,
    pub from_role: String,
    pub to_role: String,
    pub from_resource: Option<ResourceId>,
    pub to_resource: Option<ResourceId>,
    #[serde(default)]
    pub required_participants: Vec<RelationParticipant>,
}

impl RelationPathPattern {
    pub fn new(
        namespace: RelationNamespace,
        relation_type: impl Into<String>,
        from_role: impl Into<String>,
        to_role: impl Into<String>,
    ) -> Self {
        Self {
            namespace,
            relation_type: relation_type.into(),
            from_role: from_role.into(),
            to_role: to_role.into(),
            from_resource: None,
            to_resource: None,
            required_participants: Vec::new(),
        }
    }

    pub fn with_endpoints(mut self, from_resource: ResourceId, to_resource: ResourceId) -> Self {
        self.from_resource = Some(from_resource);
        self.to_resource = Some(to_resource);
        self
    }

    pub fn with_participant(mut self, role: impl Into<String>, resource_ref: ResourceId) -> Self {
        self.required_participants
            .push(RelationParticipant::new(role, resource_ref));
        self
    }

    pub fn matches_relation(&self, relation: &TypedRelationInstance) -> bool {
        relation.namespace == self.namespace
            && relation.relation_type == self.relation_type
            && relation.has_participant(&self.from_role, self.from_resource)
            && relation.has_participant(&self.to_role, self.to_resource)
            && self.required_participants.iter().all(|participant| {
                relation.has_participant(&participant.role, Some(participant.resource_ref))
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalBudget {
    pub max_hops: usize,
    pub max_relations: usize,
    pub max_branching_per_node: usize,
    pub max_seed_nodes: usize,
    pub max_paths: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphTraversalPlan {
    pub seed_nodes: Vec<ResourceId>,
    pub path_patterns: Vec<RelationPathPattern>,
    pub allowed_relation_types: Vec<String>,
    pub allowed_namespaces: Vec<RelationNamespace>,
    pub authority_requirement: Option<String>,
    pub temporal_context: Option<TemporalEvaluationContext>,
    pub access_context: String,
    pub expansion_budget: TraversalBudget,
    pub stop_conditions: Vec<String>,
}

impl GraphTraversalPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.seed_nodes.is_empty()
            || self.path_patterns.is_empty()
            || self.allowed_relation_types.is_empty()
            || self.allowed_namespaces.is_empty()
        {
            return Err("traversal requires seed and relation constraints");
        }
        if self.access_context.is_empty() || self.temporal_context.is_none() {
            return Err("traversal requires access and temporal context");
        }
        if self.expansion_budget.max_hops == 0
            || self.expansion_budget.max_relations == 0
            || self.expansion_budget.max_branching_per_node == 0
            || self.expansion_budget.max_seed_nodes == 0
            || self.expansion_budget.max_paths == 0
        {
            return Err("traversal requires finite positive budgets");
        }
        if self.seed_nodes.len() > self.expansion_budget.max_seed_nodes {
            return Err("graph seed budget exceeded");
        }
        if !self.stop_conditions.is_empty() {
            return Err("graph stop conditions are not supported");
        }
        Ok(())
    }

    pub fn allows(&self, relation: &TypedRelationInstance) -> bool {
        if self.validate().is_err() || relation.validate().is_err() {
            return false;
        }
        self.allowed_namespaces.contains(&relation.namespace)
            && self
                .allowed_relation_types
                .contains(&relation.relation_type)
            && self
                .path_patterns
                .iter()
                .any(|pattern| pattern.matches_relation(relation))
            && self
                .authority_requirement
                .as_ref()
                .is_none_or(|required| relation.authority.as_ref() == Some(required))
            && self
                .temporal_context
                .as_ref()
                .is_some_and(|context| relation.temporal_scope.contains(context.temporal_target))
    }
}

/// One relation occurrence in an ordered path. Participant sets remain tied to
/// their relation ID, so paths cannot manufacture a composite n-ary relation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphPathStepEvidence {
    pub relation_id: RelationId,
    pub namespace: RelationNamespace,
    pub relation_type: String,
    pub from_role: String,
    pub from_resource: ResourceId,
    pub to_role: String,
    pub to_resource: ResourceId,
    pub participants: Vec<RelationParticipant>,
    pub evidence_refs: Vec<String>,
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GraphPathEvidence {
    pub resource_path: Vec<ResourceId>,
    pub steps: Vec<GraphPathStepEvidence>,
}
