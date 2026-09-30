use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use document_domain::{DocumentId, DocumentVersionId, FolderId, LifecycleState, Title};
use search_application::indexing_service::{DocumentIndexingPort, IndexingOutcome};
use search_application::ports::{BoxFuture, SemanticRegistrySnapshot};
use search_application::projection::ProjectionCompiler;
use search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_core::observation::Coverage;
use search_core::predicate::TypedValue;
use search_core::profile::{DiscoveryLens, FacetState};
use search_core::projection::ProjectionGenerationManifest;
use search_core::relation::RelationNamespace;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_source_document::{
    DocumentAccessProjectionInput, DocumentIndexingConfig, DocumentOutboxIndexer,
    DocumentOutboxReader, DocumentOutboxSnapshot, DocumentRelationProjector,
    DocumentSourceSnapshot, DocumentSourceTranslator, DsiEvidenceRefs, DsiReadState,
    IndexingReceipt, IndexingReceiptStore, MemoryDocumentIndexRuntime, PermittedDocumentMetadata,
    PublicationEndRecord, RelationProjectionError, VersionSnapshotRecord, document_resource_id,
    folder_resource_id,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn at(second: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(second).unwrap()
}

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(70))
}

fn record(version: u128) -> VersionSnapshotRecord {
    let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(version));
    VersionSnapshotRecord {
        snapshot: DocumentSourceSnapshot {
            source_snapshot: "snapshot-1".into(),
            document_id: DocumentId::from_uuid(Uuid::from_u128(10)),
            document_version_id: version_id,
            current_version_id: Some(version_id),
            publication_end: None,
            lifecycle_state: LifecycleState::Published,
            title: Title::new("Policy").unwrap(),
            metadata: PermittedDocumentMetadata::default(),
            folder_id: FolderId::from_uuid(Uuid::from_u128(10)),
            created_at: at(100),
            published_at: Some(at(200)),
            withdrawn_at: None,
            effective_from: None,
            effective_to: None,
            access: DocumentAccessProjectionInput::default(),
            dsi: None,
        },
        document_revision: 4,
        access_revision: 2,
        dsi_state: DsiReadState::UnknownMissing,
    }
}

fn source_and_lens() -> (DiscoverableSource, DiscoveryLens) {
    let mut source = DiscoverableSource::new(
        source_id(),
        "document-platform",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    source.resource_types.push(ResourceKind::Knowledge);
    source.access_model = Some("document-current-check".into());
    let lens = DiscoveryLens {
        lens_id: "document-version".into(),
        lens_version: 1,
        resource_type: ResourceKind::Knowledge,
        domain_scope: None,
        source_scope: Some(source_id()),
        identity_fields: vec!["document_version_id".into()],
        high_signal_facets: vec![],
        searchable_fields: vec!["title".into()],
        applicability_fields: vec![],
        temporal_fields: vec![],
        relation_fields: vec![],
        extraction_policy: None,
        projection_policy: None,
    };
    (source, lens)
}

fn translator() -> DocumentSourceTranslator {
    let (source, lens) = source_and_lens();
    DocumentSourceTranslator::new(source, lens, "schema-1", "registry-1")
}

fn manifest() -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(71)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: "snapshot-1".into(),
        resource_count: 1,
        relation_count: Some(2),
        coverage: Coverage::CompleteEnumeration,
        digest: "synthetic".into(),
        built_at: at(300),
    }
}

#[test]
fn stable_ids_and_canonical_order_bind_source_type_roles_and_participants() {
    let projector = DocumentRelationProjector::new(source_id());
    let first = projector.project(&record(20)).unwrap();
    let repeat = projector.project(&record(20)).unwrap();
    assert_eq!(first, repeat);
    assert_eq!(first.relations.len(), 2);
    assert!(first.relations[0].relation_id < first.relations[1].relation_id);
    assert_ne!(
        first.relations[0].relation_id,
        first.relations[1].relation_id
    );
    let next_version = projector.project(&record(21)).unwrap();
    for kind in ["document_has_version", "document_current_placement"] {
        let id_for = |projection: &search_source_document::DocumentRelationProjection| {
            projection
                .relations
                .iter()
                .find(|relation| relation.relation_type == kind)
                .unwrap()
                .relation_id
        };
        assert_ne!(id_for(&first), id_for(&next_version));
    }
    let other_source = DocumentRelationProjector::new(SourceId::from_uuid(Uuid::from_u128(71)));
    assert_ne!(
        first.relations,
        other_source.project(&record(20)).unwrap().relations
    );
    let raw = Uuid::from_u128(10);
    assert_ne!(
        document_resource_id(source_id(), DocumentId::from_uuid(raw)),
        folder_resource_id(source_id(), FolderId::from_uuid(raw))
    );
    assert_ne!(
        document_resource_id(source_id(), DocumentId::from_uuid(raw)),
        ResourceId::from_uuid(raw)
    );
}

#[test]
fn version_id_colliding_with_document_resource_id_fails_closed() {
    let projector = DocumentRelationProjector::new(source_id());
    let mut item = record(20);
    let version_id = DocumentVersionId::from_uuid(
        document_resource_id(source_id(), item.snapshot.document_id).as_uuid(),
    );
    item.snapshot.document_version_id = version_id;
    item.snapshot.current_version_id = Some(version_id);
    assert_eq!(
        projector.project(&item).unwrap_err(),
        RelationProjectionError::ResourceIdentityCollision
    );
}

#[test]
fn version_id_colliding_with_current_folder_resource_id_fails_closed() {
    let projector = DocumentRelationProjector::new(source_id());
    let mut item = record(20);
    let version_id = DocumentVersionId::from_uuid(
        folder_resource_id(source_id(), item.snapshot.folder_id).as_uuid(),
    );
    item.snapshot.document_version_id = version_id;
    item.snapshot.current_version_id = Some(version_id);
    assert_eq!(
        projector.project(&item).unwrap_err(),
        RelationProjectionError::ResourceIdentityCollision
    );
}

#[test]
fn binary_membership_and_current_ternary_placement_keep_exact_roles_and_provenance() {
    let record = record(20);
    let projected = DocumentRelationProjector::new(source_id())
        .project(&record)
        .unwrap();
    let membership = projected
        .relations
        .iter()
        .find(|relation| relation.relation_type == "document_has_version")
        .unwrap();
    assert_eq!(membership.namespace, RelationNamespace::Discovery);
    assert_eq!(
        membership
            .participants
            .iter()
            .map(|p| p.role.as_str())
            .collect::<Vec<_>>(),
        vec!["document", "version"]
    );
    assert_eq!(
        membership.participants[1].resource_ref,
        ResourceId::from_uuid(Uuid::from_u128(20))
    );
    let placement = projected
        .relations
        .iter()
        .find(|relation| relation.relation_type == "document_current_placement")
        .unwrap();
    assert_eq!(
        placement
            .participants
            .iter()
            .map(|p| p.role.as_str())
            .collect::<Vec<_>>(),
        vec!["current_version", "document", "folder"]
    );
    assert_eq!(
        placement.participants[0].resource_ref,
        ResourceId::from_uuid(Uuid::from_u128(20))
    );
    assert_eq!(
        placement.participants[1].resource_ref,
        document_resource_id(source_id(), record.snapshot.document_id)
    );
    assert_eq!(
        placement.participants[2].resource_ref,
        folder_resource_id(source_id(), record.snapshot.folder_id)
    );
    let authority = source_id().as_uuid().to_string();
    let provenance = record.snapshot.document_id.as_uuid().to_string();
    for relation in &projected.relations {
        assert_eq!(relation.authority.as_deref(), Some(authority.as_str()));
        assert_eq!(relation.provenance.as_deref(), Some(provenance.as_str()));
        assert!(relation.evidence_refs.is_empty());
        assert_eq!(
            relation.qualifiers.get("document_revision"),
            Some(&TypedValue::Integer(4))
        );
    }
}

#[test]
fn only_current_published_version_has_current_folder_placement() {
    let projector = DocumentRelationProjector::new(source_id());
    let mut historical = record(20);
    historical.snapshot.current_version_id =
        Some(DocumentVersionId::from_uuid(Uuid::from_u128(21)));
    let mut withdrawn = record(20);
    withdrawn.snapshot.current_version_id = None;
    withdrawn.snapshot.lifecycle_state = LifecycleState::Withdrawn;
    withdrawn.snapshot.withdrawn_at = Some(at(300));
    let mut working = record(20);
    working.snapshot.current_version_id = None;
    working.snapshot.lifecycle_state = LifecycleState::Working;
    working.snapshot.published_at = None;
    let mut ended = record(20);
    ended.snapshot.current_version_id = None;
    ended.snapshot.publication_end = Some(PublicationEndRecord {
        operation_id: Uuid::from_u128(50),
        ended_at: at(400),
    });
    for item in [historical, withdrawn, working, ended] {
        let projection = projector.project(&item).unwrap();
        assert_eq!(projection.relations.len(), 1);
        assert_eq!(
            projection.relations[0].relation_type,
            "document_has_version"
        );
    }
}

#[test]
fn folder_change_rebuilds_current_placement_without_claiming_old_history() {
    let projector = DocumentRelationProjector::new(source_id());
    let old = projector.project(&record(20)).unwrap();
    let mut moved = record(20);
    moved.snapshot.folder_id = FolderId::from_uuid(Uuid::from_u128(31));
    moved.document_revision += 1;
    let new = projector.project(&moved).unwrap();
    let old_current = old
        .relations
        .iter()
        .find(|r| r.relation_type == "document_current_placement")
        .unwrap();
    let new_current = new
        .relations
        .iter()
        .find(|r| r.relation_type == "document_current_placement")
        .unwrap();
    assert_ne!(old_current.relation_id, new_current.relation_id);
    assert_eq!(
        new_current.participants[2].resource_ref,
        folder_resource_id(source_id(), moved.snapshot.folder_id)
    );
    assert!(new.relations.iter().all(|relation| {
        !relation
            .participants
            .iter()
            .any(|p| p.resource_ref == old_current.participants[2].resource_ref)
    }));
    let mut history = moved;
    history.snapshot.current_version_id = Some(DocumentVersionId::from_uuid(Uuid::from_u128(21)));
    assert_eq!(projector.project(&history).unwrap().relations.len(), 1);
}

#[test]
fn role_swap_missing_extra_and_cross_document_composite_are_rejected() {
    let projector = DocumentRelationProjector::new(source_id());
    let item = record(20);
    let membership = projector
        .project(&item)
        .unwrap()
        .relations
        .into_iter()
        .find(|r| r.relation_type == "document_has_version")
        .unwrap();
    assert!(projector.validate_relation(&item, &membership).is_ok());
    let mut swapped = membership.clone();
    swapped.participants[0].role = "version".into();
    swapped.participants[1].role = "document".into();
    swapped.participants.sort();
    assert!(projector.validate_relation(&item, &swapped).is_err());
    let mut missing = membership.clone();
    missing.participants.pop();
    assert!(projector.validate_relation(&item, &missing).is_err());
    let mut extra = membership.clone();
    extra.participants.push(membership.participants[0].clone());
    assert!(projector.validate_relation(&item, &extra).is_err());
    let mut other = record(20);
    other.snapshot.document_id = DocumentId::from_uuid(Uuid::from_u128(11));
    let false_composite = projector
        .project(&other)
        .unwrap()
        .relations
        .into_iter()
        .find(|r| r.relation_type == "document_has_version")
        .unwrap();
    assert!(
        projector
            .validate_relation(&item, &false_composite)
            .is_err()
    );

    let placement = projector
        .project(&item)
        .unwrap()
        .relations
        .into_iter()
        .find(|r| r.relation_type == "document_current_placement")
        .unwrap();
    let mut foreign_document = placement.clone();
    foreign_document.participants[1].resource_ref =
        document_resource_id(source_id(), other.snapshot.document_id);
    assert!(
        projector
            .validate_relation(&item, &foreign_document)
            .is_err()
    );
    let mut foreign_folder = placement;
    foreign_folder.participants[2].resource_ref =
        folder_resource_id(source_id(), FolderId::from_uuid(Uuid::from_u128(31)));
    assert!(projector.validate_relation(&item, &foreign_folder).is_err());
}

#[test]
fn unknown_dsi_states_emit_no_dsi_relations_and_remain_unknown_in_projection() {
    let projector = DocumentRelationProjector::new(source_id());
    for state in [DsiReadState::UnknownMissing, DsiReadState::UnknownInvalid] {
        let mut item = record(20);
        item.dsi_state = state;
        let result = projector.project(&item).unwrap();
        assert_eq!(result.dsi_read_state, state);
        assert_eq!(result.dsi_dependency_state, FacetState::Unknown);
        assert!(
            result
                .relations
                .iter()
                .all(|r| !r.relation_type.contains("dsi"))
        );
        let translation = translator().translate_record(item).unwrap();
        let inputs = translation.live_inputs().unwrap();
        assert_eq!(
            inputs
                .projection
                .typed_facets
                .get("document.dsi_external_dependencies"),
            Some(&FacetState::Unknown)
        );
    }
    let mut verified = record(20);
    verified.dsi_state = DsiReadState::Verified;
    verified.snapshot.dsi = Some(DsiEvidenceRefs {
        capability_refs: vec!["dsi-capability:embedded-link".into()],
        evidence_refs: vec!["dsi:opaque".into()],
    });
    let result = projector.project(&verified).unwrap();
    assert_eq!(result.dsi_dependency_state, FacetState::Unknown);
    assert_eq!(result.relations.len(), 2);
}

#[test]
fn translator_sets_exact_relation_ids_before_compilation() {
    let translation = translator().translate_record(record(20)).unwrap();
    let input = &translation.live_inputs().unwrap().projection;
    assert_eq!(input.resource.relation_ids.len(), 2);
    assert_eq!(
        input.resource.relation_ids,
        input
            .relations
            .iter()
            .map(|r| r.relation_id)
            .collect::<Vec<_>>()
    );
    let compiled = ProjectionCompiler::compile_resource(&manifest(), input).unwrap();
    assert_eq!(compiled.relations, input.relations);
}

#[derive(Clone)]
struct OneSnapshot(VersionSnapshotRecord);

impl DocumentOutboxReader for OneSnapshot {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move {
            Ok(DocumentOutboxSnapshot {
                source_snapshot: self.0.snapshot.source_snapshot.clone(),
                live: vec![self.0.clone()],
                historical: vec![],
            })
        })
    }
}

#[derive(Default)]
struct Receipts(Arc<Mutex<BTreeMap<Uuid, IndexingReceipt>>>);

impl IndexingReceiptStore for Receipts {
    fn get<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().get(&id).cloned()) })
    }

    fn put<'a>(&'a self, id: Uuid, receipt: IndexingReceipt) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().insert(id, receipt);
            Ok(())
        })
    }
}

#[tokio::test]
async fn live_outbox_generation_publishes_relation_count_and_content() {
    let (source, lens) = source_and_lens();
    let config = DocumentIndexingConfig {
        source,
        lens,
        projection_schema_version: "schema-1".into(),
        analyzer_version: "tantivy-default-0.26.2".into(),
        semantic_registry: SemanticRegistrySnapshot::new("registry-1"),
    };
    let runtime = MemoryDocumentIndexRuntime::new();
    let indexer = DocumentOutboxIndexer::new(
        OneSnapshot(record(20)),
        config,
        runtime.clone(),
        Receipts::default(),
    );
    let outcome = indexer.rebuild().await.unwrap();
    let IndexingOutcome::Published(key) = outcome else {
        panic!("expected published generation")
    };
    let reader = runtime.projection_reader();
    let manifest = reader.pin_current(source_id()).await.unwrap().unwrap();
    assert_eq!(manifest.key(), key);
    assert_eq!(manifest.relation_count, Some(2));
    let projection = reader
        .resource_at(key, ResourceId::from_uuid(Uuid::from_u128(20)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(projection.relations.len(), 2);
    assert_eq!(
        projection
            .relations
            .iter()
            .map(|r| r.relation_type.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["document_has_version", "document_current_placement"])
    );
}
