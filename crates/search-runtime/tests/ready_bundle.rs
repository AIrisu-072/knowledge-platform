//! P7-08: one READY commit from stored payloads, the sealed lexical directory
//! and the PostgreSQL Graph rows; any drift leaves nothing READY.

#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use document_domain::DocumentId;
use search_application::graph_generation::{
    GraphResourceRecord, GraphSourceMapping, RegisteredFullBuildHandle,
};
use search_application::ports::SemanticRegistrySnapshot;
use search_application::search_core::id::{
    ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::{
    AccessProjection, CompiledResourceProjection, DirectoryProjection, ProjectionGenerationKey,
    ProjectionGenerationManifest, StructuredProjection, TemporalProjection,
};
use search_application::search_core::relation::{
    RelationNamespace, RelationParticipant, TypedRelationInstance,
};
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
use search_application::search_core::temporal::TemporalDiscoveryProfile;
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_graph::{GRAPH_SCHEMA_VERSION, PostgresGraphStore, canonical_mapping_digest};
use search_projection_memory::generation_digest;
use search_runtime::full_guard::{FullGuardTtl, ManualBuildHandle};
use search_runtime::generation_registration::{FullBuildRequest, PgGenerationRegistrar};
use search_runtime::lexical_artifact::LexicalArtifactStore;
use search_runtime::payload::{PgPayloadStore, ProjectionPayloadV1, StoredBundleV1};
use search_runtime::ready::{ReadyCoordinator, ReadyError};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_source_document::{
    ArtifactReceipt, BodyUnitManifest, compute_bundle_receipt, graph_receipt,
    validate_restored_manifest,
};
use search_tantivy::{
    LexicalBuildInput, LexicalDocument, TantivyLexicalIndex, lexical_input_digest,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use units::SNAPSHOT;
use uuid::Uuid;

fn source_id() -> SourceId {
    registration::source(7801)
}

fn source() -> DiscoverableSource {
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

fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

fn registry() -> SemanticRegistrySnapshot {
    SemanticRegistrySnapshot::new("registry-v1")
}

fn placement(authority: &str) -> TypedRelationInstance {
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

fn base_manifest(generation: u128) -> ProjectionGenerationManifest {
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

fn projection(manifest: &ProjectionGenerationManifest) -> CompiledResourceProjection {
    let resource = units::parent();
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
fn manifest(generation: u128) -> ProjectionGenerationManifest {
    let mut manifest = base_manifest(generation);
    manifest.digest = generation_digest(
        source_id(),
        &[projection(&base_manifest(generation))],
        &registry(),
    )
    .unwrap();
    manifest
}

fn graph_records(authority: &str) -> (Vec<GraphResourceRecord>, Vec<TypedRelationInstance>) {
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

struct Fixture {
    _guard: support::postgres::DatabaseGuard,
    admin: PgPool,
    registrar: PgGenerationRegistrar,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn fixture() -> Fixture {
    let (guard, admin, _) = support::postgres::postgres("ready_bundle_test").await;
    document_repository_postgres::migrate(&admin).await.unwrap();
    search_runtime::migrate(&admin).await.unwrap();
    search_graph::migrate(&admin).await.unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(admin.clone(), host.clone());
    let empty_remote = registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let document = registration::document(source_id(), "tenant-a").await;
    let desired = registration::publish(
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

struct Built {
    handle: ManualBuildHandle,
    key: ProjectionGenerationKey,
    expected_composite: [u8; 32],
}

impl Fixture {
    fn coordinator(&self) -> ReadyCoordinator {
        ReadyCoordinator::new(self.admin.clone(), &self.root, source())
    }

    /// Registers the target with its Graph parent and stages every artifact.
    async fn build(&self, generation: u128, graph_authority: &str) -> Built {
        let manifest = manifest(generation);
        let key = manifest.key();
        let (graph_resources, graph_relations) = graph_records(graph_authority);
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
        let graph_target: RegisteredFullBuildHandle = handle.graph_target().unwrap();

        // Lexical artifact: build, finalize and seal against the Unit manifest.
        let unit_manifest = BodyUnitManifest {
            key,
            source_snapshot: SNAPSHOT.into(),
            entries: vec![units::entry(source_id(), &["東京の本文"])],
        };
        let input = LexicalBuildInput::new(
            source_id(),
            SNAPSHOT,
            "schema-1",
            1,
            vec![LexicalDocument {
                resource_ref: units::parent(),
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
        Built {
            handle,
            key,
            expected_composite: receipt.composite_digest,
        }
    }

    async fn states(&self, key: ProjectionGenerationKey) -> (String, String, i64) {
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

#[tokio::test]
async fn real_payload_lexical_and_graph_become_ready_in_one_commit() {
    let fixture = fixture().await;
    let built = fixture.build(7_810, "document-platform").await;
    let verified = fixture
        .coordinator()
        .ready_manual(&built.handle)
        .await
        .unwrap();
    assert_eq!(verified.key(), built.key);
    assert_eq!(
        verified.receipt().composite_digest,
        built.expected_composite
    );
    assert_eq!(verified.graph().relation_count, 1);
    assert_eq!(
        fixture.states(built.key).await,
        ("READY".into(), "READY".into(), 1)
    );
    let (composite, backend): (String, String) = sqlx::query_as(
        "SELECT composite_digest, graph_backend FROM search_generation_receipt \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(built.key.source_id.as_uuid())
    .bind(built.key.generation_id.as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    let hex: String = built
        .expected_composite
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        (composite, backend),
        (format!("sha256:{hex}"), "postgresql".into())
    );
    // READY never moves the current pointer.
    let current: Option<Uuid> = sqlx::query_scalar(
        "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id().as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(current, None);
    // A second READY attempt is fenced.
    assert_eq!(
        fixture.coordinator().ready_manual(&built.handle).await,
        Err(ReadyError::Fence)
    );
}

#[tokio::test]
async fn graph_lexical_guard_or_payload_drift_leaves_nothing_ready() {
    let fixture = fixture().await;

    // Graph rows that differ from the projection's typed relations.
    let diverged = fixture.build(7_820, "another-authority").await;
    assert_eq!(
        fixture.coordinator().ready_manual(&diverged.handle).await,
        Err(ReadyError::Mapping)
    );
    assert_eq!(
        fixture.states(diverged.key).await,
        ("BUILDING".into(), "BUILDING".into(), 0)
    );

    // The finalized lexical directory disappeared.
    let missing = fixture.build(7_821, "document-platform").await;
    std::fs::remove_dir_all(
        LexicalArtifactStore::new(&fixture.root, fixture.admin.clone()).final_dir(missing.key),
    )
    .unwrap();
    assert!(matches!(
        fixture.coordinator().ready_manual(&missing.handle).await,
        Err(ReadyError::Lexical(_))
    ));
    assert_eq!(fixture.states(missing.key).await.0, "BUILDING");

    // The full guard expired before READY.
    let expired = fixture.build(7_822, "document-platform").await;
    sqlx::query(
        "UPDATE public.search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(expired.key.source_id.as_uuid())
    .bind(expired.key.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        fixture.coordinator().ready_manual(&expired.handle).await,
        Err(ReadyError::Fence)
    );
    assert_eq!(fixture.states(expired.key).await.0, "BUILDING");

    // A stored Unit text changed behind its digest column.
    let tampered = fixture.build(7_823, "document-platform").await;
    sqlx::query(
        "UPDATE search_generation_payload \
         SET payload = jsonb_set(payload, '{body,entries,0,units,0,text}', '\"大阪の本文\"') \
         WHERE source_id=$1 AND generation_id=$2 AND kind='unit_manifest'",
    )
    .bind(tampered.key.source_id.as_uuid())
    .bind(tampered.key.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert!(matches!(
        fixture.coordinator().ready_manual(&tampered.handle).await,
        Err(ReadyError::Payload(_))
    ));
    assert_eq!(fixture.states(tampered.key).await.0, "BUILDING");
}
