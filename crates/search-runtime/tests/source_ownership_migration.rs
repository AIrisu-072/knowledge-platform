mod support;

use sqlx::{PgPool, migrate::Migrator, postgres::PgPoolOptions};
use support::postgres::DatabaseGuard;
use uuid::Uuid;

async fn postgres() -> (DatabaseGuard, PgPool) {
    let (guard, pool, _) = support::postgres::postgres("source_ownership_migration_test").await;
    (guard, pool)
}

async fn migrate_search_through(pool: &PgPool, version: i64) {
    let migrations = Migrator::new(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap();
    let selected = migrations
        .migrations
        .iter()
        .filter(|migration| migration.version <= version)
        .cloned()
        .collect();
    let mut migrator = Migrator::with_migrations(selected);
    migrator.dangerous_set_table_name("search_runtime_sqlx_migrations");
    migrator.run(pool).await.unwrap();
}

async fn migrate_only_search_0001(pool: &PgPool) {
    migrate_search_through(pool, 1).await;
}

async fn register_source(pool: &PgPool, source_id: Uuid, kind: &str, tenant: &str) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,$2,$3,1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$4,clock_timestamp(),clock_timestamp())",
    )
    .bind(source_id)
    .bind(tenant)
    .bind(kind)
    .bind(format!("sha256:{}", "a".repeat(64)))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO search_source_coordination (source_id,tenant_owner_key,registration_revision,visibility_revision,activation_epoch,registration_active) VALUES ($1,$2,1,1,1,true)",
    )
    .bind(source_id)
    .bind(tenant)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

fn assert_pg_error(error: sqlx::Error, expected_code: &str) {
    let sqlx::Error::Database(error) = error else {
        panic!("expected PostgreSQL database error, got {error}");
    };
    assert_eq!(error.code().as_deref(), Some(expected_code));
}

#[tokio::test]
async fn ownership_migration_preserves_independent_ledgers_and_namespaces() {
    let (_container, pool) = postgres().await;
    document_repository_postgres::migrate(&pool).await.unwrap();
    let domain_before: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        domain_before
            .iter()
            .map(|(version, _)| *version)
            .collect::<Vec<_>>(),
        (1..=11).collect::<Vec<_>>()
    );
    migrate_only_search_0001(&pool).await;
    let first_search: (i64, Vec<u8>) = sqlx::query_as(
        "SELECT version,checksum FROM search_runtime_sqlx_migrations WHERE version=1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    search_runtime::migrate(&pool).await.unwrap();
    let search_after: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version,checksum FROM search_runtime_sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        search_after
            .iter()
            .map(|(version, _)| *version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(search_after[0], first_search);
    assert!(
        search_after
            .iter()
            .all(|(_, checksum)| !checksum.is_empty())
    );
    let domain_after: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(domain_after, domain_before);

    let serial: (Option<i64>, Option<String>, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT document_deployment_revision,document_desired_set_digest,remote_deployment_revision,remote_desired_set_digest FROM search_registration_serial WHERE singleton=true",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(serial, (None, None, None, None));
    let document_digest = format!("sha256:{}", "b".repeat(64));
    let remote_digest = format!("sha256:{}", "c".repeat(64));
    sqlx::query("UPDATE search_registration_serial SET document_deployment_revision=1,document_desired_set_digest=$1 WHERE singleton=true")
        .bind(&document_digest)
        .execute(&pool)
        .await
        .unwrap();
    let half: (i64, String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT document_deployment_revision,document_desired_set_digest,remote_deployment_revision,remote_desired_set_digest FROM search_registration_serial",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(half, (1, document_digest.clone(), None, None));
    sqlx::query("UPDATE search_registration_serial SET remote_deployment_revision=2,remote_desired_set_digest=$1 WHERE singleton=true")
        .bind(&remote_digest)
        .execute(&pool)
        .await
        .unwrap();
    let complete: (i64, String, i64, String) = sqlx::query_as(
        "SELECT document_deployment_revision,document_desired_set_digest,remote_deployment_revision,remote_desired_set_digest FROM search_registration_serial",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(complete, (1, document_digest, 2, remote_digest));
    for statement in [
        "UPDATE search_registration_serial SET document_desired_set_digest=NULL",
        "UPDATE search_registration_serial SET document_deployment_revision=0",
        "UPDATE search_registration_serial SET remote_desired_set_digest='sha256:BAD'",
        "UPDATE search_registration_serial SET remote_deployment_revision=NULL",
    ] {
        assert_pg_error(
            sqlx::query(statement).execute(&pool).await.unwrap_err(),
            "23514",
        );
    }
    assert_pg_error(
        sqlx::query("UPDATE search_registration_serial SET remote_deployment_revision=1")
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );

    search_runtime::migrate(&pool).await.unwrap();
    let search_again: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version,checksum FROM search_runtime_sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(search_again, search_after);
}

#[tokio::test]
async fn ownership_shape_and_identity_remain_guarded_after_tombstone() {
    let (_container, pool) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(101);
    register_source(&pool, source_id, "DOCUMENT", "tenant-one").await;

    for (index, tenant) in ["", " tenant", "tenant ", "has\ncontrol", &"é".repeat(129)]
        .into_iter()
        .enumerate()
    {
        let error = sqlx::query(
            "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,$2,'REMOTE',1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$3,clock_timestamp(),clock_timestamp())",
        )
        .bind(Uuid::from_u128(102 + index as u128))
        .bind(tenant)
        .bind(format!("sha256:{}", "a".repeat(64)))
        .execute(&pool)
        .await
        .unwrap_err();
        assert_pg_error(error, "23514");
    }
    for (index, (kind, dto, digest)) in [
        (
            "OTHER",
            "{\"dto_version\":\"v1\"}",
            format!("sha256:{}", "a".repeat(64)),
        ),
        (
            "REMOTE",
            "{\"dto_version\":\"v2\"}",
            format!("sha256:{}", "a".repeat(64)),
        ),
        ("REMOTE", "[]", format!("sha256:{}", "a".repeat(64))),
        (
            "REMOTE",
            "{\"dto_version\":\"v1\"}",
            format!("sha256:{}", "A".repeat(64)),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let error = sqlx::query(
            "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,'tenant-one',$2,1,1,1,'ACTIVE',$3::jsonb,$4,clock_timestamp(),clock_timestamp())",
        )
        .bind(Uuid::from_u128(107 + index as u128))
        .bind(kind)
        .bind(dto)
        .bind(digest)
        .execute(&pool)
        .await
        .unwrap_err();
        assert_pg_error(error, "23514");
    }
    let too_large = format!(
        "{{\"dto_version\":\"v1\",\"content\":\"{}\"}}",
        "x".repeat(65_536)
    );
    let error = sqlx::query(
        "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,'tenant-one','REMOTE',1,1,1,'ACTIVE',$2::jsonb,$3,clock_timestamp(),clock_timestamp())",
    )
    .bind(Uuid::from_u128(111))
    .bind(too_large)
    .bind(format!("sha256:{}", "a".repeat(64)))
    .execute(&pool)
    .await
    .unwrap_err();
    assert_pg_error(error, "23514");

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE search_source_ownership SET state='TOMBSTONED',activation_epoch=2 WHERE source_id=$1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET registration_active=false,activation_epoch=2 WHERE source_id=$1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    for statement in [
        "UPDATE search_source_ownership SET tenant_owner_key='other' WHERE source_id=$1",
        "UPDATE search_source_ownership SET source_kind='REMOTE' WHERE source_id=$1",
        "UPDATE search_source_ownership SET source_id='00000000-0000-0000-0000-000000000009' WHERE source_id=$1",
        "DELETE FROM search_source_ownership WHERE source_id=$1",
    ] {
        assert_pg_error(
            sqlx::query(statement)
                .bind(source_id)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
    let persisted: (String, String, String, bool) = sqlx::query_as(
        "SELECT o.tenant_owner_key,o.source_kind,o.state,s.registration_active FROM search_source_ownership o JOIN search_source_coordination s USING(source_id) WHERE o.source_id=$1",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "tenant-one".into(),
            "DOCUMENT".into(),
            "TOMBSTONED".into(),
            false
        )
    );
}

#[tokio::test]
async fn ownership_and_source_binding_require_one_transaction() {
    let (_container, pool) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(201);
    let orphan_id = Uuid::from_u128(202);
    assert_pg_error(
        sqlx::query("INSERT INTO search_source_coordination (source_id) VALUES ($1)")
            .bind(orphan_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );
    assert_pg_error(
        sqlx::query(
            "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,'tenant-two','REMOTE',1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$2,clock_timestamp(),clock_timestamp())",
        )
        .bind(orphan_id)
        .bind(format!("sha256:{}", "a".repeat(64)))
        .execute(&pool)
        .await
        .unwrap_err(),
        "23503",
    );
    register_source(&pool, source_id, "REMOTE", "tenant-two").await;

    let owner_only = sqlx::query(
        "UPDATE search_source_ownership SET registration_revision=2 WHERE source_id=$1",
    )
    .bind(source_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_pg_error(owner_only, "23514");
    let source_only = sqlx::query(
        "UPDATE search_source_coordination SET registration_active=false WHERE source_id=$1",
    )
    .bind(source_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_pg_error(source_only, "23514");

    let mut no_activation = pool.begin().await.unwrap();
    assert_pg_error(
        sqlx::query(
            "UPDATE search_source_ownership SET registration_revision=2 WHERE source_id=$1",
        )
        .bind(source_id)
        .execute(&mut *no_activation)
        .await
        .unwrap_err(),
        "23514",
    );
    no_activation.rollback().await.unwrap();

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE search_source_ownership SET registration_revision=2,visibility_revision=2,activation_epoch=2,registration_digest=$2 WHERE source_id=$1")
        .bind(source_id)
        .bind(format!("sha256:{}", "b".repeat(64)))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET registration_revision=2,visibility_revision=2,activation_epoch=2 WHERE source_id=$1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let pair: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT o.registration_revision,s.registration_revision,o.visibility_revision,s.visibility_revision,o.activation_epoch,s.activation_epoch FROM search_source_ownership o JOIN search_source_coordination s USING(source_id) WHERE o.source_id=$1",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pair, (2, 2, 2, 2, 2, 2));
}

#[tokio::test]
async fn new_receipts_require_an_explicit_bundle_version() {
    let (_container, pool) = postgres().await;
    search_runtime::migrate(&pool).await.unwrap();
    let source_id = Uuid::from_u128(250);
    let generation_id = Uuid::from_u128(251);
    register_source(&pool, source_id, "DOCUMENT", "tenant-one").await;
    let digest = format!("sha256:{}", "c".repeat(64));
    let insert = "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,$4,1,clock_timestamp(),$5)";
    for (event_id, version) in [(252, None), (253, Some("v2"))] {
        assert_pg_error(
            sqlx::query(insert)
                .bind(source_id)
                .bind(Uuid::from_u128(event_id))
                .bind(generation_id)
                .bind(&digest)
                .bind(version)
                .execute(&pool)
                .await
                .unwrap_err(),
            "23514",
        );
    }
    let event_id = Uuid::from_u128(254);
    sqlx::query(insert)
        .bind(source_id)
        .bind(event_id)
        .bind(generation_id)
        .bind(&digest)
        .bind(Some("v1"))
        .execute(&pool)
        .await
        .unwrap();
    assert_pg_error(
        sqlx::query("UPDATE search_index_receipts SET bundle_version='v2' WHERE source_id=$1 AND event_id=$2")
            .bind(source_id)
            .bind(event_id)
            .execute(&pool)
            .await
            .unwrap_err(),
        "23514",
    );
}

#[tokio::test]
async fn unproven_legacy_current_and_receipt_fail_atomically() {
    let (_container, pool) = postgres().await;
    document_repository_postgres::migrate(&pool).await.unwrap();
    migrate_only_search_0001(&pool).await;
    let source_id = Uuid::from_u128(301);
    let generation_id = Uuid::from_u128(302);
    let event_id = Uuid::from_u128(303);
    let digest = format!("sha256:{}", "d".repeat(64));
    sqlx::query("INSERT INTO search_source_coordination (source_id,fence_epoch,current_generation_id,current_manifest_digest,current_bundle_digest,pointer_revision,last_published_epoch) VALUES ($1,1,$2,$3,$3,1,1)")
        .bind(source_id)
        .bind(generation_id)
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at) VALUES ($1,$2,$3,$4,$4,1,clock_timestamp())")
        .bind(source_id)
        .bind(event_id)
        .bind(generation_id)
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();

    assert!(search_runtime::migrate(&pool).await.is_err());
    let pointer: (Uuid, String, String, i64) = sqlx::query_as("SELECT current_generation_id,current_manifest_digest,current_bundle_digest,pointer_revision FROM search_source_coordination WHERE source_id=$1")
        .bind(source_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pointer, (generation_id, digest.clone(), digest.clone(), 1));
    let receipt: (Uuid, String) = sqlx::query_as(
        "SELECT generation_id,bundle_digest FROM search_index_receipts WHERE source_id=$1 AND event_id=$2",
    )
    .bind(source_id)
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipt, (generation_id, digest));
    let ledger: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM search_runtime_sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ledger, vec![1]);
    let ownership_exists: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.search_source_ownership')::text")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ownership_exists, None);
}

#[tokio::test]
async fn explicit_legacy_proof_requires_every_generation_key() {
    let (_container, pool, options) =
        support::postgres::postgres("source_ownership_migration_test").await;
    migrate_only_search_0001(&pool).await;
    let source_id = Uuid::from_u128(401);
    let current_generation = Uuid::from_u128(402);
    let historical_generation = Uuid::from_u128(403);
    let event_id = Uuid::from_u128(404);
    let digest = format!("sha256:{}", "f".repeat(64));
    sqlx::query("INSERT INTO search_source_coordination (source_id,fence_epoch,current_generation_id,current_manifest_digest,current_bundle_digest,pointer_revision,last_published_epoch) VALUES ($1,1,$2,$3,$3,1,1)")
        .bind(source_id)
        .bind(current_generation)
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at) VALUES ($1,$2,$3,$4,$4,1,clock_timestamp())")
        .bind(source_id)
        .bind(event_id)
        .bind(historical_generation)
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();

    // These tables are supplied by the trusted host from its durable authority
    // before running 0002. SQL validates exact Source and generation key coverage.
    sqlx::query(
        "CREATE TABLE search_legacy_source_backfill_proof (source_id uuid PRIMARY KEY, tenant_owner_key text NOT NULL, source_kind text NOT NULL, registration_revision bigint NOT NULL, visibility_revision bigint NOT NULL, activation_epoch bigint NOT NULL, state text NOT NULL, registration_dto jsonb NOT NULL, registration_digest text NOT NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE search_legacy_generation_backfill_proof (source_id uuid NOT NULL, generation_id uuid NOT NULL, tenant_owner_key text NOT NULL, source_kind text NOT NULL, registration_revision bigint NOT NULL, activation_epoch bigint NOT NULL, PRIMARY KEY (source_id,generation_id))",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO search_legacy_source_backfill_proof VALUES ($1,'tenant-legacy','DOCUMENT',1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$2)")
        .bind(source_id)
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_legacy_generation_backfill_proof VALUES ($1,$2,'wrong-tenant','DOCUMENT',1,1)")
        .bind(source_id)
        .bind(current_generation)
        .execute(&pool)
        .await
        .unwrap();

    assert!(search_runtime::migrate(&pool).await.is_err());
    // SQLx 0.9's migrator returns before unlocking a session advisory lock on
    // failure. Close the failed pool and re-read from a fresh session.
    pool.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options.clone())
        .await
        .unwrap();
    let pointer_after_failure: Uuid = sqlx::query_scalar(
        "SELECT current_generation_id FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pointer_after_failure, current_generation);
    let ledger_after_failure: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM search_runtime_sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ledger_after_failure, vec![1]);

    sqlx::query("INSERT INTO search_legacy_generation_backfill_proof VALUES ($1,$2,'tenant-legacy','DOCUMENT',1,1)")
        .bind(source_id)
        .bind(historical_generation)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        search_runtime::migrate(&pool).await.is_err(),
        "wrong tenant proof must fail"
    );
    pool.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options.clone())
        .await
        .unwrap();
    sqlx::query("UPDATE search_legacy_generation_backfill_proof SET tenant_owner_key='tenant-legacy' WHERE source_id=$1 AND generation_id=$2")
        .bind(source_id)
        .bind(current_generation)
        .execute(&pool)
        .await
        .unwrap();
    let generation_error = search_runtime::migrate(&pool).await.unwrap_err();
    pool.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    assert!(
        format!("{generation_error:?}").contains("legacy current generation requires"),
        "0003 must reject an unproven current rather than fail for another reason: {generation_error:?}"
    );
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM search_runtime_sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(applied, vec![1, 2]);
    let bound: (String, String, i64, bool, Uuid) = sqlx::query_as(
        "SELECT o.tenant_owner_key,o.source_kind,o.activation_epoch,s.registration_active,s.current_generation_id FROM search_source_ownership o JOIN search_source_coordination s USING(source_id) WHERE o.source_id=$1",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        bound,
        (
            "tenant-legacy".into(),
            "DOCUMENT".into(),
            1,
            true,
            current_generation
        )
    );
    let version: Option<String> = sqlx::query_scalar(
        "SELECT bundle_version FROM search_index_receipts WHERE source_id=$1 AND event_id=$2",
    )
    .bind(source_id)
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        version, None,
        "historical receipt version cannot be guessed from digest"
    );
}
