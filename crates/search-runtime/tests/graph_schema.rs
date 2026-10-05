//! P3-G03: the search_graph schema, its real roles and the DB mutation fence.
//! Lives here because every Graph parent binds a P7 target row by FK.

#[path = "support/registration.rs"]
mod registration;
mod support;

use std::sync::Arc;
use std::time::Duration;

use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::generation_registration::{FullBuildRequest, PgGenerationRegistrar};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use time::OffsetDateTime;
use uuid::Uuid;

const SNAPSHOT: &str = "synthetic-snapshot-v1";

fn source_id() -> SourceId {
    registration::source(7601)
}

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
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
        graph_schema_version: Some("search-graph-v1".into()),
        source_snapshot: SNAPSHOT.into(),
        resource_count: 0,
        relation_count: Some(0),
        coverage: Coverage::CompleteEnumeration,
        digest: digest('a'),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

async fn login_pool(admin: &PgPool, options: &PgConnectOptions, group: &str) -> PgPool {
    let login = format!("p3_test_{}", Uuid::new_v4().simple());
    for statement in [
        format!("CREATE ROLE {login} LOGIN PASSWORD 'p3-disposable-fixture'"),
        format!("GRANT {group} TO {login}"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
            .execute(admin)
            .await
            .unwrap();
    }
    PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            options
                .clone()
                .username(&login)
                .password("p3-disposable-fixture"),
        )
        .await
        .unwrap()
}

struct Fixture {
    _guard: support::postgres::DatabaseGuard,
    admin: PgPool,
    options: PgConnectOptions,
    registrar: PgGenerationRegistrar,
}

async fn fixture() -> Fixture {
    let (guard, admin, options) = support::postgres::postgres("graph_schema_test").await;
    document_repository_postgres::migrate(&admin).await.unwrap();
    search_runtime::migrate(&admin).await.unwrap();
    search_graph::migrate(&admin).await.unwrap();
    for roles in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/sql/roles.sql"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../search-graph/sql/roles.sql"),
    ] {
        let text = std::fs::read_to_string(roles).unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(text.as_str()))
            .execute(&admin)
            .await
            .unwrap();
    }
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
        options,
        registrar,
    }
}

impl Fixture {
    /// A P7 FULL target with its live guard and the bound Graph parent.
    async fn graph_target(&self, generation: u128) -> ProjectionGenerationKey {
        let key = self
            .registrar
            .register_manual(
                &FullBuildRequest {
                    manifest: manifest(generation),
                    expected_snapshot: SNAPSHOT.into(),
                },
                FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap()
            .key();
        sqlx::query(
            "INSERT INTO search_graph.generation (source_id,generation_id,graph_schema_version, \
             build_kind,full_guard_token,full_build_fence,source_snapshot, \
             projection_manifest_digest,source_mapping_digest,state) \
             SELECT source_id,generation_id,'search-graph-v1','FULL',full_guard_token, \
             full_build_fence,source_snapshot,projection_manifest_digest,$3,'BUILDING' \
             FROM public.search_generation WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(digest('d'))
        .execute(&self.admin)
        .await
        .unwrap();
        key
    }

    async fn login(&self, group: &str) -> PgPool {
        login_pool(&self.admin, &self.options, group).await
    }
}

fn code(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> Option<String> {
    result.err().and_then(|error| {
        error
            .as_database_error()
            .and_then(|e| e.code())
            .map(Into::into)
    })
}

async fn insert_resource(
    pool: &PgPool,
    key: ProjectionGenerationKey,
    resource: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO search_graph.resource (source_id,generation_id,resource_id,resource_kind, \
         mapping_kind,owner_document_id) VALUES ($1,$2,$3,'DOCUMENT','DOCUMENT',$4)",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(resource)
    .bind(Uuid::from_u128(9_000))
    .execute(pool)
    .await
}

async fn insert_relation(
    pool: &PgPool,
    key: ProjectionGenerationKey,
    relation: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO search_graph.relation (source_id,generation_id,relation_id,namespace, \
         relation_type,payload,canonical_digest) \
         VALUES ($1,$2,$3,'DISCOVERY','placement','{\"dto_version\":\"v1\"}'::jsonb,$4)",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(relation)
    .bind(digest('e'))
    .execute(pool)
    .await
}

async fn settle_ready(pool: &PgPool, key: ProjectionGenerationKey) -> Option<String> {
    code(
        sqlx::query(
            "UPDATE search_graph.generation SET state='READY', graph_content_digest=$3, \
             resource_count=1, relation_count=0, ready_at=clock_timestamp() \
             WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .bind(digest('c'))
        .execute(pool)
        .await,
    )
}

#[tokio::test]
async fn schema_rejects_cross_generation_participant_and_invalid_temporal_pair() {
    let fixture = fixture().await;
    let builder = fixture.login("search_builder").await;
    let a = fixture.graph_target(7_610).await;
    let b = fixture.graph_target(7_611).await;
    insert_resource(&builder, a, Uuid::from_u128(1))
        .await
        .unwrap();
    insert_resource(&builder, b, Uuid::from_u128(2))
        .await
        .unwrap();
    insert_relation(&builder, a, Uuid::from_u128(10))
        .await
        .unwrap();
    // A participant may only name a Resource of the same generation.
    let cross = sqlx::query(
        "INSERT INTO search_graph.participant (source_id,generation_id,relation_id,ordinal, \
         role,resource_id) VALUES ($1,$2,$3,0,'document',$4)",
    )
    .bind(a.source_id.as_uuid())
    .bind(a.generation_id.as_uuid())
    .bind(Uuid::from_u128(10))
    .bind(Uuid::from_u128(2))
    .execute(&builder)
    .await;
    assert_eq!(code(cross).as_deref(), Some("23503"));
    // Half-set instant pairs and empty intervals are refused.
    for (nanos_from, offset_from, nanos_to) in [
        (Some(1_i64), None, None),
        (Some(5_i64), Some(0), Some(5_i64)),
    ] {
        let invalid = sqlx::query(
            "INSERT INTO search_graph.resource (source_id,generation_id,resource_id, \
             resource_kind,mapping_kind,owner_document_id,valid_from_nanos,valid_from_offset, \
             valid_to_nanos,valid_to_offset) \
             VALUES ($1,$2,$3,'DOCUMENT','DOCUMENT',$4,$5,$6,$7,$8)",
        )
        .bind(a.source_id.as_uuid())
        .bind(a.generation_id.as_uuid())
        .bind(Uuid::from_u128(3))
        .bind(Uuid::from_u128(9_000))
        .bind(nanos_from)
        .bind(offset_from)
        .bind(nanos_to)
        .bind(nanos_to.map(|_| 0_i32))
        .execute(&builder)
        .await;
        assert_eq!(code(invalid).as_deref(), Some("23514"));
    }
}

#[tokio::test]
async fn direct_ready_child_insert_update_delete_is_denied() {
    let fixture = fixture().await;
    let builder = fixture.login("search_builder").await;
    let coordinator = fixture.login("search_coordinator").await;
    let key = fixture.graph_target(7_620).await;
    insert_resource(&builder, key, Uuid::from_u128(1))
        .await
        .unwrap();
    assert_eq!(settle_ready(&coordinator, key).await, None);
    assert_eq!(
        code(insert_resource(&builder, key, Uuid::from_u128(2)).await).as_deref(),
        Some("23514")
    );
    for statement in [
        "UPDATE search_graph.resource SET owner_document_id = gen_random_uuid() \
         WHERE source_id=$1 AND generation_id=$2",
        "DELETE FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2",
    ] {
        // The admin bypasses grants but not the fence trigger.
        let denied = sqlx::query(sqlx::AssertSqlSafe(statement))
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .execute(&fixture.admin)
            .await;
        assert_eq!(code(denied).as_deref(), Some("23514"), "{statement}");
    }
}

#[tokio::test]
async fn builder_cannot_delete_ready_or_edit_pointer() {
    let fixture = fixture().await;
    let builder = fixture.login("search_builder").await;
    let reader = fixture.login("search_reader").await;
    let key = fixture.graph_target(7_630).await;
    let attempts = [
        (
            &builder,
            "DELETE FROM search_graph.generation WHERE source_id=$1 AND generation_id=$2",
        ),
        (
            &builder,
            "UPDATE search_graph.generation SET state='READY' \
             WHERE source_id=$1 AND generation_id=$2",
        ),
        (
            &builder,
            "UPDATE public.search_source_coordination SET pointer_revision = pointer_revision + 1 \
             WHERE source_id=$1 AND $2::uuid IS NOT NULL",
        ),
        (
            &builder,
            "INSERT INTO search_graph.build_guard (source_id,base_generation_id, \
             target_generation_id,guard_token,fence,base_manifest_digest, \
             base_graph_content_digest,base_source_snapshot,base_source_mapping_digest, \
             base_resource_count,base_relation_count,target_manifest_digest,expires_at) \
             SELECT $1,$2,gen_random_uuid(),gen_random_uuid(),1,'x','x','x','x',0,0,'x',now()",
        ),
        (
            &reader,
            "DELETE FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2",
        ),
    ];
    for (pool, statement) in attempts {
        let denied = sqlx::query(sqlx::AssertSqlSafe(statement))
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .execute(pool)
            .await;
        assert_eq!(code(denied).as_deref(), Some("42501"), "{statement}");
    }
}

#[tokio::test]
async fn state_transition_and_guard_binding_are_fenced() {
    let fixture = fixture().await;
    let builder = fixture.login("search_builder").await;
    let coordinator = fixture.login("search_coordinator").await;
    let key = fixture.graph_target(7_640).await;
    assert_eq!(settle_ready(&coordinator, key).await, None);
    // READY never returns to BUILDING, and its binding never changes.
    for statement in [
        "UPDATE search_graph.generation SET state='BUILDING', ready_at=NULL \
         WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_graph.generation SET source_snapshot='other' \
         WHERE source_id=$1 AND generation_id=$2",
        "UPDATE search_graph.generation SET graph_content_digest=source_mapping_digest \
         WHERE source_id=$1 AND generation_id=$2",
    ] {
        let denied = sqlx::query(sqlx::AssertSqlSafe(statement))
            .bind(key.source_id.as_uuid())
            .bind(key.generation_id.as_uuid())
            .execute(&fixture.admin)
            .await;
        assert_eq!(code(denied).as_deref(), Some("23514"), "{statement}");
    }

    // An expired full guard stops child writes and READY.
    let late = fixture.graph_target(7_641).await;
    sqlx::query(
        "UPDATE public.search_generation_full_guard SET expires_at = clock_timestamp() \
         - interval '1 second' WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(late.source_id.as_uuid())
    .bind(late.generation_id.as_uuid())
    .execute(&fixture.admin)
    .await
    .unwrap();
    assert_eq!(
        code(insert_resource(&builder, late, Uuid::from_u128(5)).await).as_deref(),
        Some("23514")
    );
    assert_eq!(
        settle_ready(&coordinator, late).await.as_deref(),
        Some("23514")
    );

    // A Graph parent can only bind its own BUILDING P7 target.
    let foreign = sqlx::query(
        "INSERT INTO search_graph.generation (source_id,generation_id,graph_schema_version, \
         build_kind,full_guard_token,full_build_fence,source_snapshot, \
         projection_manifest_digest,source_mapping_digest,state) \
         SELECT source_id,generation_id,'search-graph-v1','FULL',full_guard_token, \
         full_build_fence,'another-snapshot',projection_manifest_digest,$3,'BUILDING' \
         FROM public.search_generation WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(digest('d'))
    .execute(&fixture.admin)
    .await;
    assert!(code(foreign).is_some());
}
