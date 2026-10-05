//! Disposable PostgreSQL incidence-table candidate for comparison with the oracle.

use std::collections::BTreeSet;
use std::error::Error;
use std::time::Instant;

use tokio_postgres::Client;

use crate::hypergraph::{Participant, PathEvidence, Relation, TraversalQuery, validate_relations};

pub struct PostgresIncidence {
    pub load_ms: f64,
    pub table_bytes: i64,
    pub relation_count: usize,
    pub participant_rows: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct QueryExpansion {
    pub returned_rows: usize,
    pub expanded_relations: usize,
    pub expanded_nodes: usize,
}

impl PostgresIncidence {
    pub async fn load(client: &mut Client, relations: &[Relation]) -> Result<Self, Box<dyn Error>> {
        let started = Instant::now();
        validate_relations(relations)?;
        client
            .batch_execute(
                "DROP TABLE IF EXISTS search_participants;
                 DROP TABLE IF EXISTS search_relations;
                 CREATE TABLE search_relations (
                     id text PRIMARY KEY,
                     namespace text NOT NULL,
                     relation_type text NOT NULL
                 );
                 CREATE TABLE search_participants (
                     relation_id text NOT NULL REFERENCES search_relations(id),
                     ordinal integer NOT NULL,
                     role text NOT NULL,
                     resource_id text NOT NULL
                 );",
            )
            .await?;
        let ids = relations
            .iter()
            .map(|relation| relation.id.clone())
            .collect::<Vec<_>>();
        let namespaces = relations
            .iter()
            .map(|relation| relation.namespace.clone())
            .collect::<Vec<_>>();
        let types = relations
            .iter()
            .map(|relation| relation.relation_type.clone())
            .collect::<Vec<_>>();
        client
            .execute(
                "INSERT INTO search_relations(id, namespace, relation_type)
                 SELECT * FROM unnest($1::text[], $2::text[], $3::text[])",
                &[&ids, &namespaces, &types],
            )
            .await?;
        let mut relation_ids = Vec::new();
        let mut ordinals = Vec::new();
        let mut roles = Vec::new();
        let mut resources = Vec::new();
        for relation in relations {
            for (ordinal, participant) in relation.participants.iter().enumerate() {
                relation_ids.push(relation.id.clone());
                ordinals.push(i32::try_from(ordinal)?);
                roles.push(participant.role.clone());
                resources.push(participant.resource_id.clone());
            }
        }
        client
            .execute(
                "INSERT INTO search_participants(relation_id, ordinal, role, resource_id)
                 SELECT * FROM unnest($1::text[], $2::integer[], $3::text[], $4::text[])",
                &[&relation_ids, &ordinals, &roles, &resources],
            )
            .await?;
        client
            .batch_execute(
                "CREATE INDEX search_participants_resource_role
                   ON search_participants(resource_id, role, relation_id);
                 CREATE INDEX search_participants_relation_role
                   ON search_participants(relation_id, role, resource_id);
                 CREATE INDEX search_relations_type
                   ON search_relations(namespace, relation_type, id);
                 ANALYZE search_relations;
                 ANALYZE search_participants;",
            )
            .await?;
        let load_ms = started.elapsed().as_secs_f64() * 1000.0;
        let table_bytes: i64 = client
            .query_one(
                "SELECT pg_total_relation_size('search_relations')
                      + pg_total_relation_size('search_participants')",
                &[],
            )
            .await?
            .get(0);
        Ok(Self {
            load_ms,
            table_bytes,
            relation_count: relations.len(),
            participant_rows: resources.len(),
        })
    }

    pub async fn traverse(
        &self,
        client: &Client,
        query: &TraversalQuery,
    ) -> Result<Vec<PathEvidence>, Box<dyn Error>> {
        Ok(self.traverse_measured(client, query).await?.0)
    }

    pub async fn traverse_measured(
        &self,
        client: &Client,
        query: &TraversalQuery,
    ) -> Result<(Vec<PathEvidence>, QueryExpansion), Box<dyn Error>> {
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
            return Err("traversal requires seed, typed steps and finite positive budgets".into());
        }
        let mut expansion = QueryExpansion {
            returned_rows: 0,
            expanded_relations: 0,
            expanded_nodes: 0,
        };
        let mut relation_ids = BTreeSet::new();
        let mut paths = vec![PathEvidence {
            target_resource_id: query.seed_resource_id.clone(),
            resource_path: vec![query.seed_resource_id.clone()],
            relation_ids: Vec::new(),
            participants: Vec::new(),
        }];
        for step in &query.steps {
            let mut next = Vec::new();
            for path in &paths {
                expansion.expanded_nodes += 1;
                let required_roles = step
                    .required_participants
                    .iter()
                    .map(|participant| participant.role.clone())
                    .collect::<Vec<_>>();
                let required_resources = step
                    .required_participants
                    .iter()
                    .map(|participant| participant.resource_id.clone())
                    .collect::<Vec<_>>();
                // Fetch at most one row beyond the tighter remaining budget. The
                // extra row is enough to distinguish an exhausted path budget.
                let remaining_paths = query.max_paths.saturating_sub(next.len());
                let row_limit = i64::try_from(
                    query
                        .max_branching_per_node
                        .min(remaining_paths)
                        .saturating_add(1),
                )?;
                let rows = client
                    .query(
                        "SELECT relation.id, target.resource_id,
                                participants.roles, participants.resources
                           FROM search_relations relation
                           JOIN search_participants source ON source.relation_id = relation.id
                           JOIN search_participants target ON target.relation_id = relation.id
                           JOIN LATERAL (
                               SELECT array_agg(role ORDER BY ordinal) AS roles,
                                      array_agg(resource_id ORDER BY ordinal) AS resources
                                 FROM search_participants all_part
                                WHERE all_part.relation_id = relation.id
                           ) participants ON true
                          WHERE source.resource_id = $1 AND source.role = $2
                            AND target.role = $3
                            AND relation.namespace = $4 AND relation.relation_type = $5
                            AND NOT EXISTS (
                                SELECT 1 FROM unnest($6::text[], $7::text[]) required(role, resource_id)
                                 WHERE NOT EXISTS (
                                    SELECT 1 FROM search_participants actual
                                     WHERE actual.relation_id = relation.id
                                       AND actual.role = required.role
                                       AND actual.resource_id = required.resource_id
                                 )
                            )
                          ORDER BY relation.id, target.resource_id
                          LIMIT $8",
                        &[
                            &path.target_resource_id,
                            &step.from_role,
                            &step.to_role,
                            &step.namespace,
                            &step.relation_type,
                            &required_roles,
                            &required_resources,
                            &row_limit,
                        ],
                    )
                    .await?;
                if rows.len() > query.max_branching_per_node {
                    return Err("traversal branching budget exceeded".into());
                }
                if rows.len() > remaining_paths {
                    return Err("traversal path budget exceeded".into());
                }
                expansion.returned_rows += rows.len();
                for row in rows {
                    let relation_id: String = row.get(0);
                    let target_resource_id: String = row.get(1);
                    relation_ids.insert(relation_id.clone());
                    let roles: Vec<String> = row.get(2);
                    let resources: Vec<String> = row.get(3);
                    let participants = roles
                        .into_iter()
                        .zip(resources)
                        .map(|(role, resource_id)| Participant::new(role, resource_id))
                        .collect::<Vec<_>>();
                    let mut extended = path.clone();
                    extended.target_resource_id = target_resource_id.clone();
                    extended.resource_path.push(target_resource_id);
                    extended.relation_ids.push(relation_id);
                    extended.participants.push(participants);
                    next.push(extended);
                }
            }
            next.sort_by(|left, right| {
                left.relation_ids
                    .cmp(&right.relation_ids)
                    .then(left.target_resource_id.cmp(&right.target_resource_id))
            });
            paths = next;
        }
        expansion.expanded_relations = relation_ids.len();
        Ok((paths, expansion))
    }
}
