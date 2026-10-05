//! P3-G05: Source-owned mapping commitment and Document current access for
//! durable Graph rows. A Document or folder-placement node has exactly one
//! owning Document in the same snapshot and a ResourceId recomputed from it;
//! a Knowledge node is its own Version. Access re-checks the stored row's
//! structure and then the current Document state through the Document
//! authorization service on every call. There is no RAM owner map and no
//! caller-supplied owner.

use std::collections::{BTreeMap, BTreeSet};

use document_domain::{DocumentId, FolderId};
use search_application::SearchError;
use search_application::graph_generation::{
    GenerationScopedGraphAccessPort, GraphResourceRecord, GraphSourceMapping,
    GraphSourceMappingReceipt, GraphSourceMappingValidatorPort,
};
use search_application::ports::{AccessDecision, BoxFuture};
use search_application::scoped::{AuthorizedSourceScope, TrustedDiscoveryBinding};
use search_core::id::{RelationId, ResourceId, SourceId};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_core::relation::TypedRelationInstance;
use search_core::resource::ResourceKind;
use search_graph::{PostgresGraphStore, canonical_mapping_digest};

use crate::postgres::{DocumentCurrentAccessAdapter, DocumentOutboxSnapshot};
use crate::relations::{document_resource_id, folder_resource_id};

fn mapping_error(reason: &str) -> SearchError {
    SearchError::OperationFailed(format!("Document graph mapping: {reason}"))
}

fn exactly_one<T: Ord>(owners: BTreeSet<T>) -> Result<T, SearchError> {
    let mut owners = owners.into_iter();
    match (owners.next(), owners.next()) {
        (Some(owner), None) => Ok(owner),
        _ => Err(mapping_error(
            "node needs exactly one owner in the snapshot",
        )),
    }
}

/// Every relation of the generation once, in relation-ID order.
pub fn graph_relations(records: &[GraphResourceRecord]) -> Vec<TypedRelationInstance> {
    let mut relations: BTreeMap<RelationId, TypedRelationInstance> = BTreeMap::new();
    for record in records {
        for relation in &record.attached_relations {
            relations
                .entry(relation.relation_id)
                .or_insert_with(|| relation.clone());
        }
    }
    relations.into_values().collect()
}

/// Graph rows of one Document generation, bound to the snapshot that also
/// produced its projections, and their mapping commitment digest.
pub fn document_graph_records(
    source: SourceId,
    snapshot: &DocumentOutboxSnapshot,
    projections: &[CompiledResourceProjection],
) -> Result<(Vec<GraphResourceRecord>, String), SearchError> {
    let mut relations: BTreeMap<RelationId, TypedRelationInstance> = BTreeMap::new();
    for relation in projections.iter().flat_map(|p| p.relations.iter()) {
        if let Some(existing) = relations.insert(relation.relation_id, relation.clone())
            && existing != *relation
        {
            return Err(mapping_error("one relation ID has two definitions"));
        }
    }
    let present: BTreeSet<ResourceId> = projections
        .iter()
        .map(|projection| projection.directory.resource_ref)
        .collect();
    if present.len() != projections.len() {
        return Err(mapping_error("duplicate Resource"));
    }
    if relations
        .values()
        .flat_map(|relation| relation.participants.iter())
        .any(|participant| !present.contains(&participant.resource_ref))
    {
        return Err(mapping_error(
            "relation participant is not a generation Resource",
        ));
    }

    let mut records = Vec::with_capacity(projections.len());
    for projection in projections {
        let id = projection.directory.resource_ref;
        let mapping = match projection.directory.kind {
            ResourceKind::Knowledge => {
                if projection.directory.resource_version.map(|v| v.as_uuid()) != Some(id.as_uuid())
                {
                    return Err(mapping_error("Knowledge node is not its own Version"));
                }
                let document_id = exactly_one(
                    snapshot
                        .live
                        .iter()
                        .filter(|record| {
                            record.snapshot.document_version_id.as_uuid() == id.as_uuid()
                        })
                        .map(|record| record.snapshot.document_id.as_uuid())
                        .collect(),
                )?;
                GraphSourceMapping::Version {
                    document_id,
                    version_id: id.as_uuid(),
                }
            }
            ResourceKind::Document => GraphSourceMapping::Document {
                document_id: exactly_one(
                    snapshot
                        .live
                        .iter()
                        .map(|record| record.snapshot.document_id)
                        .filter(|document| document_resource_id(source, *document) == id)
                        .map(|document| document.as_uuid())
                        .collect(),
                )?,
            },
            ResourceKind::FolderPlacement => {
                let (document_id, folder_id) = exactly_one(
                    snapshot
                        .live
                        .iter()
                        .map(|record| (record.snapshot.document_id, record.snapshot.folder_id))
                        .filter(|(document, folder)| {
                            folder_resource_id(source, *document, *folder) == id
                        })
                        .map(|(document, folder)| (document.as_uuid(), folder.as_uuid()))
                        .collect(),
                )?;
                GraphSourceMapping::FolderPlacement {
                    document_id,
                    folder_id,
                }
            }
            _ => return Err(mapping_error("Resource kind has no Document mapping")),
        };
        let attached_relations = relations
            .values()
            .filter(|relation| relation.participants.iter().any(|p| p.resource_ref == id))
            .cloned()
            .collect();
        records.push(GraphResourceRecord {
            resource_ref: id,
            kind: projection.directory.kind,
            resource_version_ref: projection.directory.resource_version,
            temporal: projection.temporal.clone(),
            mapping,
            attached_relations,
        });
    }
    let digest = canonical_mapping_digest(source, &snapshot.source_snapshot, &records)
        .map_err(|_| mapping_error("mapping digest"))?;
    Ok((records, digest))
}

/// The ResourceId and kind of a stored row are recomputable from its owner.
/// A generic `Registered` mapping is never a Document node.
fn structurally_valid(source: SourceId, record: &GraphResourceRecord) -> bool {
    match &record.mapping {
        GraphSourceMapping::Document { document_id } => {
            record.kind == ResourceKind::Document
                && record.resource_version_ref.is_none()
                && record.resource_ref
                    == document_resource_id(source, DocumentId::from_uuid(*document_id))
        }
        GraphSourceMapping::FolderPlacement {
            document_id,
            folder_id,
        } => {
            record.kind == ResourceKind::FolderPlacement
                && record.resource_version_ref.is_none()
                && record.resource_ref
                    == folder_resource_id(
                        source,
                        DocumentId::from_uuid(*document_id),
                        FolderId::from_uuid(*folder_id),
                    )
        }
        GraphSourceMapping::Version { version_id, .. } => {
            record.kind == ResourceKind::Knowledge
                && record.resource_ref.as_uuid() == *version_id
                && record.resource_version_ref.map(|v| v.as_uuid()) == Some(*version_id)
        }
        GraphSourceMapping::Registered { .. } => false,
    }
}

/// Stage and recovery check of stored rows against the committed digest.
pub fn validate_document_graph_mapping(
    source: SourceId,
    snapshot_id: &str,
    records: &[GraphResourceRecord],
    expected_digest: &str,
) -> Result<(), SearchError> {
    if !records
        .iter()
        .all(|record| structurally_valid(source, record))
    {
        return Err(mapping_error("stored node does not match its owner"));
    }
    let digest = canonical_mapping_digest(source, snapshot_id, records)
        .map_err(|_| mapping_error("mapping digest"))?;
    if digest != expected_digest {
        return Err(mapping_error("mapping commitment differs"));
    }
    Ok(())
}

/// The registered mapping validator of one Document Source.
pub struct DocumentGraphMappingValidator {
    source: SourceId,
}

impl DocumentGraphMappingValidator {
    pub const fn new(source: SourceId) -> Self {
        Self { source }
    }
}

impl GraphSourceMappingValidatorPort for DocumentGraphMappingValidator {
    fn validate_authoritative<'a>(
        &'a self,
        manifest: &'a ProjectionGenerationManifest,
        records: &'a [GraphResourceRecord],
    ) -> BoxFuture<'a, GraphSourceMappingReceipt> {
        Box::pin(async move {
            if manifest.source_id != self.source {
                return Err(mapping_error("manifest belongs to another Source"));
            }
            let mapping_digest =
                canonical_mapping_digest(self.source, &manifest.source_snapshot, records)
                    .map_err(|_| mapping_error("mapping digest"))?;
            validate_document_graph_mapping(
                self.source,
                &manifest.source_snapshot,
                records,
                &mapping_digest,
            )?;
            Ok(GraphSourceMappingReceipt {
                key: manifest.key(),
                source_snapshot: manifest.source_snapshot.clone(),
                mapping_digest,
            })
        })
    }
}

/// Current Document access for nodes of a stored Graph generation. Holds the
/// actor-bound Document adapter and the access context it was issued for.
pub struct DocumentGenerationAccess<'a> {
    graph: PostgresGraphStore,
    access: &'a DocumentCurrentAccessAdapter,
    access_context: String,
}

impl<'a> DocumentGenerationAccess<'a> {
    pub fn new(
        graph: PostgresGraphStore,
        access: &'a DocumentCurrentAccessAdapter,
        access_context: impl Into<String>,
    ) -> Self {
        Self {
            graph,
            access,
            access_context: access_context.into(),
        }
    }

    /// Decides one stored row: its own Source, its recomputed structure, and
    /// then the current Document Read, publication and Version state.
    pub async fn evaluate_stored(
        &self,
        key: ProjectionGenerationKey,
        stored: &GraphResourceRecord,
        access_context: &str,
    ) -> Result<AccessDecision, SearchError> {
        let source = self.access.source_id();
        if key.source_id != source || !structurally_valid(source, stored) {
            return Ok(AccessDecision::Denied);
        }
        match &stored.mapping {
            GraphSourceMapping::Document { document_id }
            | GraphSourceMapping::FolderPlacement { document_id, .. } => {
                self.access
                    .evaluate_owned_document(DocumentId::from_uuid(*document_id), access_context)
                    .await
            }
            GraphSourceMapping::Version {
                document_id,
                version_id,
            } => {
                self.access
                    .evaluate_version(
                        ResourceId::from_uuid(*version_id),
                        DocumentId::from_uuid(*document_id),
                        access_context,
                    )
                    .await
            }
            GraphSourceMapping::Registered { .. } => Ok(AccessDecision::Denied),
        }
    }
}

impl GenerationScopedGraphAccessPort for DocumentGenerationAccess<'_> {
    fn evaluate<'b>(
        &'b self,
        key: &'b ProjectionGenerationKey,
        resource_ref: ResourceId,
        binding: &'b TrustedDiscoveryBinding,
        scope: &'b AuthorizedSourceScope,
    ) -> BoxFuture<'b, AccessDecision> {
        Box::pin(async move {
            if scope.source_id() != key.source_id || binding.actor() != scope.actor() {
                return Ok(AccessDecision::Denied);
            }
            let Some(stored) = self.graph.ready_resource(*key, resource_ref).await? else {
                return Ok(AccessDecision::Denied);
            };
            self.evaluate_stored(*key, &stored, &self.access_context)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn registered_mapping_is_never_a_document_node() {
        let source = SourceId::from_uuid(Uuid::from_u128(1));
        let record = GraphResourceRecord {
            resource_ref: ResourceId::from_uuid(Uuid::from_u128(2)),
            kind: ResourceKind::Document,
            resource_version_ref: None,
            temporal: search_core::projection::TemporalProjection {
                resource_ref: ResourceId::from_uuid(Uuid::from_u128(2)),
                valid_from: None,
                valid_to: None,
                profile: Default::default(),
            },
            mapping: GraphSourceMapping::Registered {
                adapter_id: "a".into(),
                native_id: "n".into(),
            },
            attached_relations: vec![],
        };
        assert!(!structurally_valid(source, &record));
    }
}
