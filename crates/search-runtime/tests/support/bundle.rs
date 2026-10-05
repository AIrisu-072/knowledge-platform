//! Shared READY-bundle fixture for P7 runtime tests (P7-08..P7-12).
#![allow(dead_code, unused_imports)]

pub use std::collections::BTreeMap;
pub use std::path::PathBuf;
pub use std::sync::Arc;
pub use std::time::Duration;

pub use super::units::SNAPSHOT;
pub use document_domain::DocumentId;
pub use search_application::graph_generation::{
    GraphResourceRecord, GraphSourceMapping, RegisteredFullBuildHandle,
};
pub use search_application::ports::SemanticRegistrySnapshot;
pub use search_application::search_core::id::{
    ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
pub use search_application::search_core::observation::Coverage;
pub use search_application::search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
pub use search_application::search_core::relation::{
    RelationNamespace, RelationParticipant, TypedRelationInstance,
};
pub use search_application::search_core::resource::ResourceKind;
pub use search_application::search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
pub use search_application::search_core::temporal::TemporalDiscoveryProfile;
pub use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
pub use search_graph::{GRAPH_SCHEMA_VERSION, PostgresGraphStore, canonical_mapping_digest};
pub use search_projection_memory::generation_digest;
pub use search_runtime::full_guard::{FullGuardTtl, ManualBuildHandle};
pub use search_runtime::generation_registration::{FullBuildRequest, PgGenerationRegistrar};
pub use search_runtime::lexical_artifact::LexicalArtifactStore;
pub use search_runtime::payload::{PgPayloadStore, ProjectionPayloadV1, StoredBundleV1};
pub use search_runtime::ready::{ReadyCoordinator, ReadyError};
pub use search_runtime::source_registration::PgSourceRegistrationLedger;
pub use search_source_document::{
    ArtifactReceipt, BodyUnitManifest, compute_bundle_receipt, graph_receipt,
    validate_restored_manifest,
};
pub use search_tantivy::{
    LexicalBuildInput, LexicalDocument, TantivyLexicalIndex, lexical_input_digest,
};
pub use sqlx::PgPool;
pub use time::OffsetDateTime;
pub use uuid::Uuid;

pub fn source_id() -> SourceId {
    super::registration::source(7801)
}

pub fn source() -> DiscoverableSource {
    let mut source = DiscoverableSource::new(
        source_id(),
        "document",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentResource,
    );
    source.discovery_modes = vec![
        DiscoveryMode::LocalDirectory,
        DiscoveryMode::LocalContentSearch,
    ];
    source
}

pub fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

pub fn registry() -> SemanticRegistrySnapshot {
    SemanticRegistrySnapshot::new("registry-v1")
}

pub fn placement(authority: &str) -> TypedRelationInstance {
    let mut relation = TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(50)),
        RelationNamespace::Discovery,
        "document_current_placement",
        vec![
            RelationParticipant::new("document", rid(10)),
            RelationParticipant::new("folder", rid(11)),
        ],
    );
    relation.authority = Some(authority.into());
    relation
}

pub fn base_manifest(generation: u128) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-v1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: Some(GRAPH_SCHEMA_VERSION.into()),
        source_snapshot: SNAPSHOT.into(),
        resource_count: 1,
        relation_count: Some(1),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("sha256:{}", "0".repeat(64)),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

pub fn projection(manifest: &ProjectionGenerationManifest) -> CompiledResourceProjection {
    let resource = super::units::parent();
    CompiledResourceProjection {
        manifest: manifest.clone(),
        retention_mode: RetentionMode::PersistentResource,
        directory: DirectoryProjection {
            resource_ref: resource,
            resource_version: None,
            kind: ResourceKind::Knowledge,
            canonical_name: "規程".into(),
            title: Some("規程".into()),
            aliases: vec![],
        },
        structured: StructuredProjection {
            resource_ref: resource,
            concept_refs: vec![],
            high_signal_facets: BTreeMap::new(),
            typed_facets: BTreeMap::new(),
            assertions: vec![],
            authority_resolutions: BTreeMap::new(),
        },
        temporal: TemporalProjection {
            resource_ref: resource,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        access: AccessProjection {
            resource_ref: resource,
            access_scope: None,
            source_access_model: None,
        },
        relations: vec![placement("document-platform")],
    }
}

/// The manifest whose projection-only digest covers exactly `projection`.
pub fn manifest(generation: u128) -> ProjectionGenerationManifest {
    let mut manifest = base_manifest(generation);
    manifest.digest = generation_digest(
        source_id(),
        &[projection(&base_manifest(generation))],
        &registry(),
    )
    .unwrap();
    manifest
}

pub fn graph_records(authority: &str) -> (Vec<GraphResourceRecord>, Vec<TypedRelationInstance>) {
    let record = |resource, kind, mapping| GraphResourceRecord {
        resource_ref: resource,
        kind,
        resource_version_ref: None,
        temporal: TemporalProjection {
            resource_ref: resource,
            valid_from: None,
            valid_to: None,
            profile: TemporalDiscoveryProfile::default(),
        },
        mapping,
        attached_relations: vec![placement(authority)],
    };
    let document = Uuid::from_u128(900);
    (
        vec![
            record(
                rid(10),
                ResourceKind::Document,
                GraphSourceMapping::Document {
                    document_id: document,
                },
            ),
            record(
                rid(11),
                ResourceKind::FolderPlacement,
                GraphSourceMapping::FolderPlacement {
                    document_id: document,
                    folder_id: Uuid::from_u128(901),
                },
            ),
        ],
        vec![placement(authority)],
    )
}

pub struct Fixture {
    pub _guard: super::support::postgres::DatabaseGuard,
    pub admin: PgPool,
    pub registrar: PgGenerationRegistrar,
    pub root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub async fn fixture() -> Fixture {
    let (guard, admin, _) = super::support::postgres::postgres("ready_bundle_test").await;
    document_repository_postgres::migrate(&admin).await.unwrap();
    search_runtime::migrate(&admin).await.unwrap();
    search_graph::migrate(&admin).await.unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(admin.clone(), host.clone());
    let empty_remote =
        super::registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let document = super::registration::document(source_id(), "tenant-a").await;
    let desired = super::registration::publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document.clone()],
    )
    .await;
    let activations = ledger.reconcile(&desired).await.unwrap();
    let registrar = ledger
        .generation_registrar(document, activations[&source_id()])
        .unwrap();
    Fixture {
        _guard: guard,
        admin,
        registrar,
        root: std::env::temp_dir().join(format!("search-ready-bundle-{}", Uuid::now_v7())),
    }
}

pub struct Built {
    pub handle: ManualBuildHandle,
    pub key: ProjectionGenerationKey,
    pub expected_composite: [u8; 32],
}

pub struct BuiltEvent {
    pub handle: search_runtime::full_guard::EventCandidateHandle,
    pub key: ProjectionGenerationKey,
    pub expected_composite: [u8; 32],
}

impl Fixture {
    pub fn coordinator(&self) -> ReadyCoordinator {
        ReadyCoordinator::new(self.admin.clone(), &self.root, source())
    }

    /// Registers a MANUAL target with its Graph parent and stages every artifact.
    pub async fn build(&self, generation: u128, graph_authority: &str) -> Built {
        let manifest = manifest(generation);
        let (graph_resources, _) = graph_records(graph_authority);
        let handle = self
            .registrar
            .register_manual_with_graph(
                &FullBuildRequest {
                    manifest: manifest.clone(),
                    expected_snapshot: SNAPSHOT.into(),
                },
                &canonical_mapping_digest(source_id(), SNAPSHOT, &graph_resources).unwrap(),
                FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap();
        let graph_target = handle.graph_target().unwrap();
        let expected_composite = self.stage(generation, graph_authority, graph_target).await;
        Built {
            key: handle.key(),
            handle,
            expected_composite,
        }
    }

    /// Registers an EVENT target for `fence` with its Graph parent and stages it.
    pub async fn build_event(
        &self,
        generation: u128,
        graph_authority: &str,
        event: &search_application::indexing_service::DocumentSourceEvent,
        fence: search_application::ports::SearchDeliveryFence,
    ) -> BuiltEvent {
        let manifest = manifest(generation);
        let (graph_resources, _) = graph_records(graph_authority);
        let handle = self
            .registrar
            .register_event_with_graph(
                event,
                fence,
                &FullBuildRequest {
                    manifest: manifest.clone(),
                    expected_snapshot: SNAPSHOT.into(),
                },
                &canonical_mapping_digest(source_id(), SNAPSHOT, &graph_resources).unwrap(),
                FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap();
        let graph_target = handle.graph_target().unwrap();
        let expected_composite = self.stage(generation, graph_authority, graph_target).await;
        BuiltEvent {
            key: handle.key(),
            handle,
            expected_composite,
        }
    }

    /// Stages the lexical artifact, payload rows and Graph rows of a target.
    pub async fn stage(
        &self,
        generation: u128,
        graph_authority: &str,
        graph_target: RegisteredFullBuildHandle,
    ) -> [u8; 32] {
        let manifest = manifest(generation);
        let key = manifest.key();
        let (graph_resources, graph_relations) = graph_records(graph_authority);

        // Lexical artifact: build, finalize and seal against the Unit manifest.
        let unit_manifest = BodyUnitManifest {
            key,
            source_snapshot: SNAPSHOT.into(),
            entries: vec![super::units::entry(source_id(), &["東京の本文"])],
        };
        let input = LexicalBuildInput::new(
            source_id(),
            SNAPSHOT,
            "schema-1",
            1,
            vec![LexicalDocument {
                resource_ref: super::units::parent(),
                kind: ResourceKind::Knowledge,
                canonical_name: "規程".into(),
                title: Some("規程".into()),
                aliases: vec![],
                high_signal_text: None,
                body: None,
                locator: None,
            }],
        )
        .with_body_units(unit_manifest.entries[0].units.clone());
        let logical = lexical_input_digest(&input).unwrap();
        let lexical = ArtifactReceipt {
            key,
            digest: logical.digest,
            count: logical.count,
        };
        let store = LexicalArtifactStore::new(&self.root, self.admin.clone());
        TantivyLexicalIndex::new()
            .build_generation_at(manifest.clone(), &source(), input, &store.staging_dir(key))
            .unwrap();
        store
            .finalize(
                &manifest,
                &source(),
                &store.staging_dir(key),
                &lexical,
                &unit_manifest,
            )
            .await
            .unwrap();

        // Payload rows with the receipt computed from real artifact receipts.
        let projections = vec![projection(&base_manifest(generation))];
        let owners: Vec<(ResourceId, DocumentId)> = vec![
            (rid(10), DocumentId::from_uuid(Uuid::from_u128(900))),
            (rid(11), DocumentId::from_uuid(Uuid::from_u128(900))),
        ];
        let graph = graph_receipt(key, &projections, &owners).unwrap();
        let coverage = validate_restored_manifest(&unit_manifest).unwrap();
        let receipt = compute_bundle_receipt(
            key,
            SNAPSHOT,
            &manifest.digest,
            &unit_manifest,
            &coverage,
            lexical,
            graph,
        )
        .unwrap();
        PgPayloadStore::new(self.admin.clone())
            .store(&StoredBundleV1 {
                manifest: manifest.clone(),
                projection: ProjectionPayloadV1 {
                    resources: projections,
                    registry: registry(),
                },
                unit_manifest,
                coverage,
                receipt: receipt.clone(),
            })
            .await
            .unwrap();

        // Graph rows under the same target.
        PostgresGraphStore::new(self.admin.clone())
            .stage_full(&graph_target, &graph_resources, &graph_relations)
            .await
            .unwrap();
        receipt.composite_digest
    }

    pub async fn states(&self, key: ProjectionGenerationKey) -> (String, String, i64) {
        sqlx::query_as(
            "SELECT g.state, gg.state, (SELECT count(*) FROM search_generation_receipt r \
             WHERE r.source_id=g.source_id AND r.generation_id=g.generation_id) \
             FROM search_generation g JOIN search_graph.generation gg \
             USING (source_id, generation_id) WHERE g.source_id=$1 AND g.generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_one(&self.admin)
        .await
        .unwrap()
    }
}
