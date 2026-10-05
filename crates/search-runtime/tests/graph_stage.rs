//! P7-07 Graph parent registration in the P7 commit, and P3-G04 full stage,
//! staged validation and READY recovery on real PostgreSQL.

#[path = "support/registration.rs"]
mod registration;
mod support;

use std::sync::Arc;
use std::time::Duration;

use search_application::graph_generation::{
    GraphBuildRef, GraphResourceRecord, GraphSourceMapping, RegisteredFullBuildHandle,
};
use search_application::search_core::id::{
    ProjectionGenerationId, RelationId, ResourceId, SourceId,
};
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest, TemporalProjection,
};
use search_application::search_core::relation::{
    RelationNamespace, RelationParticipant, TypedRelationInstance,
};
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::temporal::TemporalDiscoveryProfile;
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_graph::{
    GRAPH_SCHEMA_VERSION, GraphError, PostgresGraphStore, canonical_graph_digest,
    canonical_mapping_digest,
};
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::generation_registration::{
    FullBuildRequest, GenerationError, PgGenerationRegistrar,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

const SNAPSHOT: &str = "synthetic-snapshot-v1";

fn source_id() -> SourceId {
    registration::source(7701)
}

fn manifest(generation: u128) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "projection-v1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-v1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: Some(GRAPH_SCHEMA_VERSION.into()),
        source_snapshot: SNAPSHOT.into(),
        resource_count: 2,
        relation_count: Some(1),
        coverage: Coverage::CompleteEnumeration,
        digest: format!("sha256:{}", "a".repeat(64)),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

fn rid(n: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(n))
}

fn placement() -> TypedRelationInstance {
    let mut relation = TypedRelationInstance::new(
        RelationId::from_uuid(Uuid::from_u128(50)),
        RelationNamespace::Discovery,
        "document_current_placement",
        vec![
            RelationParticipant::new("document", rid(10)),
            RelationParticipant::new("folder", rid(11)),
        ],
    );
    relation.authority = Some("document-platform".into());
    relation.evidence_refs = vec!["placement:1".into()];
    relation
}

fn record(
    resource: ResourceId,
    kind: ResourceKind,
    mapping: GraphSourceMapping,
) -> GraphResourceRecord {
    GraphResourceRecord {
        resource_ref: resource,
        kind,
        resource_version_ref: None,
        temporal: TemporalProjection {
            resource_ref: resource,
            valid_from: Some(
                OffsetDateTime::from_unix_timestamp_nanos(1_700_000_000_000_000_007).unwrap(),
            ),
            valid_to: None,
            profile: TemporalDiscoveryProfile {
                freshness_anchor_at: None,
                freshness_basis: Some(String::new()),
                effective_from: None,
                effective_to: None,
            },
        },
        mapping,
        attached_relations: vec![placement()],
    }
}

fn graph() -> (Vec<GraphResourceRecord>, Vec<TypedRelationInstance>) {
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
        vec![placement()],
    )
}

fn mapping_digest(resources: &[GraphResourceRecord]) -> String {
    canonical_mapping_digest(source_id(), SNAPSHOT, resources).unwrap()
}

struct Fixture {
    _guard: support::postgres::DatabaseGuard,
    admin: PgPool,
    registrar: PgGenerationRegistrar,
}

async fn fixture() -> Fixture {
    let (guard, admin, _) = support::postgres::postgres("graph_stage_test").await;
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
    }
}

impl Fixture {
    async fn register(&self, generation: u128, mapping: &str) -> RegisteredFullBuildHandle {
        self.registrar
            .register_manual_with_graph(
                &FullBuildRequest {
                    manifest: manifest(generation),
                    expected_snapshot: SNAPSHOT.into(),
                },
                mapping,
                FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap()
            .graph_target()
            .expect("Graph parent registered in the same commit")
    }

    async fn counts(&self) -> (i64, i64, i64, i64, i64) {
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM search_generation_identity), \
             (SELECT count(*) FROM search_generation), \
             (SELECT count(*) FROM search_generation_full_guard), \
             (SELECT count(*) FROM search_graph.generation), \
             (SELECT count(*) FROM search_graph.resource)",
        )
        .fetch_one(&self.admin)
        .await
        .unwrap()
    }

    async fn settle_ready(
        &self,
        key: ProjectionGenerationKey,
        digest: &str,
        resources: i64,
        relations: i64,
    ) {
        sqlx::query(
            "UPDATE search_graph.generation SET state='READY', graph_content_digest=$3, \
             resource_count=$4, relation_count=$5, ready_at=clock_timestamp() \
             WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(digest)
        .bind(resources)
        .bind(relations)
        .execute(&self.admin)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn full_registration_is_one_commit_with_guard_and_graph_key() {
    let fixture = fixture().await;
    let (resources, _) = graph();
    let target = fixture.register(7_710, &mapping_digest(&resources)).await;
    assert_eq!(target.key(), manifest(7_710).key());
    assert_eq!(fixture.counts().await, (1, 1, 1, 1, 0));
    let (token, fence): (Uuid, i64) = sqlx::query_as(
        "SELECT full_guard_token, full_build_fence FROM search_graph.generation \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(target.key().source_id.as_uuid())
    .bind(target.key().generation_id.as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!((token, fence), (target.guard_token(), target.build_fence()));

    // A failing Graph insert rolls back the identity, target and guard too.
    sqlx::raw_sql(
        "CREATE FUNCTION fault_graph_insert() RETURNS TRIGGER LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'injected Graph insert fault' USING ERRCODE = '23514'; END; $$; \
         CREATE TRIGGER fault_graph_insert BEFORE INSERT ON search_graph.generation \
         FOR EACH ROW EXECUTE FUNCTION fault_graph_insert();",
    )
    .execute(&fixture.admin)
    .await
    .unwrap();
    let failed = fixture
        .registrar
        .register_manual_with_graph(
            &FullBuildRequest {
                manifest: manifest(7_711),
                expected_snapshot: SNAPSHOT.into(),
            },
            &mapping_digest(&resources),
            FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
        )
        .await;
    assert!(matches!(failed, Err(GenerationError::Lost)));
    assert_eq!(fixture.counts().await, (1, 1, 1, 1, 0));
    let fence_seq: i64 = sqlx::query_scalar(
        "SELECT build_fence_seq FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id().as_uuid())
    .fetch_one(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(fence_seq, 1);
}

#[tokio::test]
async fn full_stage_is_lossless_and_ready_is_immutable() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let (resources, relations) = graph();
    let target = fixture.register(7_720, &mapping_digest(&resources)).await;
    store
        .stage_full(&target, &resources, &relations)
        .await
        .unwrap();
    let report = store.validate(&GraphBuildRef::Full(target)).await.unwrap();
    let expected =
        canonical_graph_digest(source_id(), GRAPH_SCHEMA_VERSION, &resources, &relations).unwrap();
    assert_eq!(report.graph_content_digest, expected);
    assert_eq!((report.resource_count, report.relation_count), (2, 1));
    assert_eq!(report.source_mapping_digest, mapping_digest(&resources));

    fixture
        .settle_ready(target.key(), &report.graph_content_digest, 2, 1)
        .await;
    let receipt = store
        .recover(target.key(), &manifest(7_720).digest)
        .await
        .unwrap();
    assert_eq!(receipt.graph_content_digest, expected);
    // A late stage after READY is fenced; the receipt is unchanged.
    assert_eq!(
        store.stage_full(&target, &resources, &relations).await,
        Err(GraphError::FenceLost)
    );
    // The wrong expected manifest never recovers a READY generation.
    assert!(
        store
            .recover(target.key(), &format!("sha256:{}", "b".repeat(64)))
            .await
            .is_err()
    );

    // Corrupting one stored relation payload breaks recovery.
    sqlx::query("ALTER TABLE search_graph.relation DISABLE TRIGGER graph_guard_relation")
        .execute(&fixture.admin)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE search_graph.relation SET payload = jsonb_set(payload, '{relation,authority}', \
         '\"forged\"') WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(target.key().source_id.as_uuid())
    .bind(target.key().generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert!(matches!(
        store.recover(target.key(), &manifest(7_720).digest).await,
        Err(GraphError::Integrity(_))
    ));
}

#[tokio::test]
async fn forged_reference_mismatched_attachment_or_mapping_writes_nothing() {
    let fixture = fixture().await;
    let store = PostgresGraphStore::new(fixture.admin.clone());
    let (resources, relations) = graph();
    let target = fixture.register(7_730, &mapping_digest(&resources)).await;

    // A public reference with a guessed token and fence is not a grant.
    let forged = RegisteredFullBuildHandle::from_identifiers(target.key(), Uuid::from_u128(1), 1);
    assert_eq!(
        store.stage_full(&forged, &resources, &relations).await,
        Err(GraphError::FenceLost)
    );
    // Attachments must equal the one canonical relation.
    let mut diverged = resources.clone();
    diverged[1].attached_relations[0].authority = Some("other".into());
    assert!(matches!(
        store.stage_full(&target, &diverged, &relations).await,
        Err(GraphError::Invalid(_))
    ));
    // Records whose Source mapping differs from the registered commitment.
    let mut remapped = resources.clone();
    remapped[0].mapping = GraphSourceMapping::Document {
        document_id: Uuid::from_u128(999),
    };
    assert!(matches!(
        store.stage_full(&target, &remapped, &relations).await,
        Err(GraphError::Integrity(_))
    ));
    // A participant Resource outside the generation violates the FK; rollback.
    let mut orphan = placement();
    orphan
        .participants
        .push(RelationParticipant::new("reviewer", rid(77)));
    let mut with_orphan = resources.clone();
    for record in &mut with_orphan {
        record.attached_relations = vec![orphan.clone()];
    }
    assert!(
        store
            .stage_full(&target, &with_orphan, &[orphan])
            .await
            .is_err()
    );
    assert_eq!(fixture.counts().await.4, 0);

    // After the full guard expires no child can be written.
    sqlx::query(
        "UPDATE public.search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(target.key().source_id.as_uuid())
    .bind(target.key().generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        store.stage_full(&target, &resources, &relations).await,
        Err(GraphError::FenceLost)
    );
    assert_eq!(fixture.counts().await.4, 0);
}
