//! Document-owned relation projection. Only identities in the authoritative
//! Version snapshot are eligible; DSI dependency identities are not read yet.

use std::collections::BTreeMap;

use document_domain::{DocumentId, FolderId, LifecycleState};
use search_core::id::{RelationId, ResourceId, SourceId};
use search_core::predicate::TypedValue;
use search_core::profile::FacetState;
use search_core::relation::{RelationNamespace, RelationParticipant, TypedRelationInstance};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::postgres::{DsiReadState, VersionSnapshotRecord};

const DOCUMENT_HAS_VERSION: &str = "document_has_version";
const DOCUMENT_CURRENT_PLACEMENT: &str = "document_current_placement";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRelationProjection {
    pub relations: Vec<TypedRelationInstance>,
    /// DSI read quality is retained separately from the still-unknown
    /// external dependency relation capability.
    pub dsi_read_state: DsiReadState,
    pub dsi_dependency_state: FacetState<TypedValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationProjectionError {
    #[error("DSI state contradicts the Document Source snapshot")]
    InconsistentDsiState,
    #[error("Document relation participants have colliding ResourceIds")]
    ResourceIdentityCollision,
    #[error("Document relation does not match its authoritative snapshot")]
    InvalidRelation,
}

pub struct DocumentRelationProjector {
    source_id: SourceId,
}

impl DocumentRelationProjector {
    pub const fn new(source_id: SourceId) -> Self {
        Self { source_id }
    }

    pub fn project(
        &self,
        record: &VersionSnapshotRecord,
    ) -> Result<DocumentRelationProjection, RelationProjectionError> {
        match (record.dsi_state, record.snapshot.dsi.is_some()) {
            (DsiReadState::Verified, true)
            | (DsiReadState::UnknownMissing | DsiReadState::UnknownInvalid, false) => {}
            _ => return Err(RelationProjectionError::InconsistentDsiState),
        }

        let snapshot = &record.snapshot;
        let document = document_resource_id(self.source_id, snapshot.document_id);
        // Preserve the established Version ResourceId mapping from D1.
        let version = ResourceId::from_uuid(snapshot.document_version_id.as_uuid());
        if document == version {
            return Err(RelationProjectionError::ResourceIdentityCollision);
        }
        let mut relations = vec![self.relation(
            record,
            DOCUMENT_HAS_VERSION,
            vec![
                RelationParticipant::new("document", document),
                RelationParticipant::new("version", version),
            ],
        )];

        if snapshot.lifecycle_state == LifecycleState::Published
            && snapshot.current_version_id == Some(snapshot.document_version_id)
            && snapshot.publication_end.is_none()
            && snapshot.published_at.is_some()
            && snapshot.withdrawn_at.is_none()
        {
            let folder = folder_resource_id(self.source_id, snapshot.folder_id);
            if folder == document || folder == version {
                return Err(RelationProjectionError::ResourceIdentityCollision);
            }
            relations.push(self.relation(
                record,
                DOCUMENT_CURRENT_PLACEMENT,
                vec![
                    RelationParticipant::new("document", document),
                    RelationParticipant::new("current_version", version),
                    RelationParticipant::new("folder", folder),
                ],
            ));
        }
        relations.sort_by_key(|relation| relation.relation_id);
        Ok(DocumentRelationProjection {
            relations,
            dsi_read_state: record.dsi_state,
            // Even verified DSI evidence does not provide an authoritative
            // external dependency identity in the D3 snapshot.
            dsi_dependency_state: FacetState::Unknown,
        })
    }

    /// Snapshot-bound validation rejects role swaps and composites formed
    /// from participants belonging to different Documents or versions.
    pub fn validate_relation(
        &self,
        record: &VersionSnapshotRecord,
        relation: &TypedRelationInstance,
    ) -> Result<(), RelationProjectionError> {
        relation
            .validate()
            .map_err(|_| RelationProjectionError::InvalidRelation)?;
        let expected = self.project(record)?;
        if expected.relations.iter().any(|item| item == relation) {
            Ok(())
        } else {
            Err(RelationProjectionError::InvalidRelation)
        }
    }

    fn relation(
        &self,
        record: &VersionSnapshotRecord,
        relation_type: &str,
        mut participants: Vec<RelationParticipant>,
    ) -> TypedRelationInstance {
        participants.sort();
        let relation_id = relation_id(self.source_id, relation_type, &participants);
        let mut relation = TypedRelationInstance::new(
            relation_id,
            RelationNamespace::Discovery,
            relation_type,
            participants,
        );
        relation.qualifiers = BTreeMap::from([(
            "document_revision".into(),
            TypedValue::Integer(i128::from(record.document_revision)),
        )]);
        // Both fields are direct Source values, not synthetic evidence locators.
        relation.authority = Some(self.source_id.as_uuid().to_string());
        relation.provenance = Some(record.snapshot.document_id.as_uuid().to_string());
        relation
    }
}

pub fn document_resource_id(source_id: SourceId, id: DocumentId) -> ResourceId {
    namespaced_resource_id(source_id, b"document", id.as_uuid())
}

pub fn folder_resource_id(source_id: SourceId, id: FolderId) -> ResourceId {
    namespaced_resource_id(source_id, b"folder", id.as_uuid())
}

fn namespaced_resource_id(source_id: SourceId, kind: &[u8], native_id: Uuid) -> ResourceId {
    let mut hasher = Sha256::new();
    frame(&mut hasher, b"search-source-document:resource:v1");
    frame(&mut hasher, source_id.as_uuid().as_bytes());
    frame(&mut hasher, kind);
    frame(&mut hasher, native_id.as_bytes());
    ResourceId::from_uuid(uuid_from_digest(hasher))
}

fn relation_id(
    source_id: SourceId,
    relation_type: &str,
    participants: &[RelationParticipant],
) -> RelationId {
    let mut hasher = Sha256::new();
    frame(&mut hasher, b"search-source-document:relation:v1");
    frame(&mut hasher, source_id.as_uuid().as_bytes());
    frame(&mut hasher, relation_type.as_bytes());
    for participant in participants {
        frame(&mut hasher, participant.role.as_bytes());
        frame(&mut hasher, participant.resource_ref.as_uuid().as_bytes());
    }
    RelationId::from_uuid(uuid_from_digest(hasher))
}

fn frame(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn uuid_from_digest(hasher: Sha256) -> Uuid {
    let digest = hasher.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}
