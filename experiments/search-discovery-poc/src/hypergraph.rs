//! Pure-Rust semantic oracle for typed n-ary relation traversal.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    pub role: String,
    pub resource_id: String,
}

impl Participant {
    pub fn new(role: impl Into<String>, resource_id: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            resource_id: resource_id.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub id: String,
    pub namespace: String,
    pub relation_type: String,
    pub participants: Vec<Participant>,
}

#[derive(Debug, Clone)]
pub struct TraversalStep {
    pub namespace: String,
    pub relation_type: String,
    pub from_role: String,
    pub to_role: String,
    pub required_participants: Vec<Participant>,
}

#[derive(Debug, Clone)]
pub struct TraversalQuery {
    pub seed_resource_id: String,
    pub steps: Vec<TraversalStep>,
    pub max_paths: usize,
    pub max_branching_per_node: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathEvidence {
    pub target_resource_id: String,
    pub resource_path: Vec<String>,
    pub relation_ids: Vec<String>,
    pub participants: Vec<Vec<Participant>>,
}

#[derive(Debug, Clone)]
pub struct IncidenceIndex {
    relations: Vec<Relation>,
    resource: BTreeMap<String, Vec<usize>>,
    resource_role: BTreeMap<(String, String), Vec<usize>>,
    relation_type: BTreeMap<String, Vec<usize>>,
    resource_type: BTreeMap<(String, String), Vec<usize>>,
    type_role: BTreeMap<(String, String), Vec<usize>>,
}

fn insert<K: Ord>(index: &mut BTreeMap<K, Vec<usize>>, key: K, relation_position: usize) {
    let positions = index.entry(key).or_default();
    if positions.last() != Some(&relation_position) {
        positions.push(relation_position);
    }
}

pub(crate) fn validate_relations(relations: &[Relation]) -> Result<(), &'static str> {
    let mut ids = BTreeSet::new();
    for relation in relations {
        if relation.id.is_empty()
            || relation.namespace.is_empty()
            || relation.relation_type.is_empty()
            || relation.participants.len() < 2
            || !ids.insert(&relation.id)
        {
            return Err("relation identity, namespace, type and participant count must be valid");
        }
        let mut participants = BTreeSet::new();
        for participant in &relation.participants {
            if participant.role.is_empty() || participant.resource_id.is_empty() {
                return Err("participant role and resource must be nonempty");
            }
            if !participants.insert((&participant.role, &participant.resource_id)) {
                return Err("duplicate relation participant");
            }
        }
    }
    Ok(())
}

impl IncidenceIndex {
    pub fn build(relations: Vec<Relation>) -> Result<Self, &'static str> {
        validate_relations(&relations)?;
        let mut result = Self {
            relations,
            resource: BTreeMap::new(),
            resource_role: BTreeMap::new(),
            relation_type: BTreeMap::new(),
            resource_type: BTreeMap::new(),
            type_role: BTreeMap::new(),
        };
        for (position, relation) in result.relations.iter().enumerate() {
            insert(
                &mut result.relation_type,
                relation.relation_type.clone(),
                position,
            );
            for participant in &relation.participants {
                insert(
                    &mut result.resource,
                    participant.resource_id.clone(),
                    position,
                );
                insert(
                    &mut result.resource_role,
                    (participant.resource_id.clone(), participant.role.clone()),
                    position,
                );
                insert(
                    &mut result.resource_type,
                    (
                        participant.resource_id.clone(),
                        relation.relation_type.clone(),
                    ),
                    position,
                );
                insert(
                    &mut result.type_role,
                    (relation.relation_type.clone(), participant.role.clone()),
                    position,
                );
            }
        }
        Ok(result)
    }

    fn ids(&self, positions: Option<&Vec<usize>>) -> Vec<String> {
        let mut ids = positions
            .into_iter()
            .flatten()
            .map(|position| self.relations[*position].id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub fn by_resource(&self, resource_id: &str) -> Vec<String> {
        self.ids(self.resource.get(resource_id))
    }

    pub fn by_resource_role(&self, resource_id: &str, role: &str) -> Vec<String> {
        self.ids(self.resource_role.get(&(resource_id.into(), role.into())))
    }

    pub fn by_relation_type(&self, relation_type: &str) -> Vec<String> {
        self.ids(self.relation_type.get(relation_type))
    }

    pub fn by_resource_type(&self, resource_id: &str, relation_type: &str) -> Vec<String> {
        self.ids(
            self.resource_type
                .get(&(resource_id.into(), relation_type.into())),
        )
    }

    pub fn by_type_role(&self, relation_type: &str, role: &str) -> Vec<String> {
        self.ids(self.type_role.get(&(relation_type.into(), role.into())))
    }

    /// Lower-bound heap estimate; BTreeMap node allocations and allocator metadata are excluded.
    pub fn estimated_bytes(&self) -> usize {
        let relation_payload = self.relations.capacity() * std::mem::size_of::<Relation>()
            + self
                .relations
                .iter()
                .map(|relation| {
                    relation.id.capacity()
                        + relation.namespace.capacity()
                        + relation.relation_type.capacity()
                        + relation.participants.capacity() * std::mem::size_of::<Participant>()
                        + relation
                            .participants
                            .iter()
                            .map(|participant| {
                                participant.role.capacity() + participant.resource_id.capacity()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>();
        let index_values = self
            .resource
            .values()
            .chain(self.resource_role.values())
            .chain(self.relation_type.values())
            .chain(self.resource_type.values())
            .chain(self.type_role.values())
            .map(|positions| positions.capacity() * std::mem::size_of::<usize>())
            .sum::<usize>();
        let index_keys = self.resource.keys().map(String::capacity).sum::<usize>()
            + self
                .relation_type
                .keys()
                .map(String::capacity)
                .sum::<usize>()
            + self
                .resource_role
                .keys()
                .chain(self.resource_type.keys())
                .chain(self.type_role.keys())
                .map(|(left, right)| left.capacity() + right.capacity())
                .sum::<usize>();
        std::mem::size_of::<Self>() + relation_payload + index_keys + index_values
    }

    pub fn traverse(&self, query: &TraversalQuery) -> Result<Vec<PathEvidence>, &'static str> {
        if query.seed_resource_id.is_empty()
            || query.steps.is_empty()
            || query.max_paths == 0
            || query.max_branching_per_node == 0
            || query.steps.iter().any(|step| {
                step.namespace.is_empty()
                    || step.relation_type.is_empty()
                    || step.from_role.is_empty()
                    || step.to_role.is_empty()
            })
        {
            return Err("traversal requires typed steps and finite positive budgets");
        }
        let mut paths = vec![PathEvidence {
            target_resource_id: query.seed_resource_id.clone(),
            resource_path: vec![query.seed_resource_id.clone()],
            relation_ids: Vec::new(),
            participants: Vec::new(),
        }];
        for step in &query.steps {
            let mut next = Vec::new();
            for path in &paths {
                let mut branches = Vec::new();
                let key = (path.target_resource_id.clone(), step.from_role.clone());
                for position in self.resource_role.get(&key).into_iter().flatten() {
                    let relation = &self.relations[*position];
                    if relation.namespace != step.namespace
                        || relation.relation_type != step.relation_type
                        || !step
                            .required_participants
                            .iter()
                            .all(|required| relation.participants.contains(required))
                    {
                        continue;
                    }
                    for target in relation
                        .participants
                        .iter()
                        .filter(|participant| participant.role == step.to_role)
                    {
                        let mut extended = path.clone();
                        extended.target_resource_id = target.resource_id.clone();
                        extended.resource_path.push(target.resource_id.clone());
                        extended.relation_ids.push(relation.id.clone());
                        extended.participants.push(relation.participants.clone());
                        branches.push(extended);
                        if branches.len() > query.max_branching_per_node {
                            return Err("traversal branching budget exceeded");
                        }
                        if next.len().saturating_add(branches.len()) > query.max_paths {
                            return Err("traversal path budget exceeded");
                        }
                    }
                }
                next.extend(branches);
            }
            next.sort_by(|left, right| {
                left.relation_ids
                    .cmp(&right.relation_ids)
                    .then(left.target_resource_id.cmp(&right.target_resource_id))
            });
            paths = next;
        }
        Ok(paths)
    }
}
