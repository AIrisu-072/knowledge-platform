mod support;

use sqlx::PgPool;
use support::postgres::DatabaseGuard;
use uuid::Uuid;

async fn postgres() -> (DatabaseGuard, PgPool) {
    let (guard, pool, _) = support::postgres::postgres("coordination_migration_test").await;
    (guard, pool)
}

async fn migrate_both(pool: &PgPool) {
    document_repository_postgres::migrate(pool).await.unwrap();
    search_runtime::migrate(pool).await.unwrap();
}

async fn register_source(pool: &PgPool, source_id: Uuid) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,'test-tenant','DOCUMENT',1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$2,clock_timestamp(),clock_timestamp())",
    )
    .bind(source_id)
    .bind(format!("sha256:{}", "e".repeat(64)))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO search_source_coordination (source_id,tenant_owner_key,registration_revision,visibility_revision,activation_epoch,registration_active) VALUES ($1,'test-tenant',1,1,1,true)",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn register_generation(pool: &PgPool, source_id: Uuid, generation_id: Uuid) {
    let digest = format!("sha256:{}", "a".repeat(64));
    sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'test-tenant',1,clock_timestamp())")
        .bind(source_id)
        .bind(generation_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO search_generation (source_id,generation_id,activation_epoch,state,build_kind,stage_origin,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version) VALUES ($1,$2,1,'BUILDING','INCREMENTAL','MANUAL','coordination-test','{"dto_version":"v1"}'::jsonb,$3,0,'v1')"#)
        .bind(source_id)
        .bind(generation_id)
        .bind(digest)
        .execute(pool)
        .await
        .unwrap();
}

fn assert_database_error(error: sqlx::Error, code: &str, constraint: &str) {
    let sqlx::Error::Database(error) = error else {
        panic!("expected PostgreSQL constraint error");
    };
    assert_eq!(error.code().as_deref(), Some(code));
    assert_eq!(error.constraint(), Some(constraint));
}

#[tokio::test]
async fn coordination_schema_rejects_half_lease_and_duplicate_receipt() {
    let (_container, pool) = postgres().await;
    migrate_both(&pool).await;
    let source_id = Uuid::from_u128(1);
    let event_id = Uuid::from_u128(2);
    let generation_id = Uuid::from_u128(3);
    let owner_token = Uuid::from_u128(4);
    let manifest_digest = format!("sha256:{}", "a".repeat(64));
    let bundle_digest = format!("sha256:{}", "b".repeat(64));
    register_source(&pool, source_id).await;
    register_generation(&pool, source_id, generation_id).await;

    let error =
        sqlx::query("UPDATE search_source_coordination SET owner_token=$2 WHERE source_id=$1")
            .bind(source_id)
            .bind(owner_token)
            .execute(&pool)
            .await
            .unwrap_err();
    assert_database_error(error, "23514", "ck_search_source_lease_complete");
    let error = sqlx::query(
        "UPDATE search_source_coordination SET lease_expires_at=clock_timestamp() + interval '1 minute' WHERE source_id=$1",
    )
    .bind(source_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_source_lease_complete");

    sqlx::query(
        "UPDATE search_source_coordination SET owner_token=$2, lease_expires_at=clock_timestamp() + interval '1 minute', fence_epoch=1 WHERE source_id=$1",
    )
    .bind(source_id)
    .bind(owner_token)
    .execute(&pool)
    .await
    .unwrap();
    let error = sqlx::query(
        "UPDATE search_source_coordination SET current_generation_id=$2 WHERE source_id=$1",
    )
    .bind(source_id)
    .bind(generation_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_source_current_complete");

    sqlx::query(
        "UPDATE search_source_coordination SET current_generation_id=$2, current_manifest_digest=$3, current_bundle_digest=$4, pointer_revision=1, last_published_epoch=1 WHERE source_id=$1",
    )
    .bind(source_id)
    .bind(generation_id)
    .bind(&manifest_digest)
    .bind(&bundle_digest)
    .execute(&pool)
    .await
    .unwrap();

    for statement in [
        "UPDATE search_source_coordination SET fence_epoch=-1 WHERE source_id=$1",
        "UPDATE search_source_coordination SET pointer_revision=-1 WHERE source_id=$1",
        "UPDATE search_source_coordination SET last_published_epoch=-1 WHERE source_id=$1",
        "UPDATE search_source_coordination SET build_fence_seq=-1 WHERE source_id=$1",
        "UPDATE search_source_coordination SET last_published_epoch=2 WHERE source_id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(source_id)
            .execute(&pool)
            .await
            .unwrap_err();
        let sqlx::Error::Database(error) = error else {
            panic!("expected PostgreSQL CHECK error");
        };
        assert_eq!(error.code().as_deref(), Some("23514"));
    }
    let error = sqlx::query(
        "UPDATE search_source_coordination SET current_manifest_digest='SHA256:bad' WHERE source_id=$1",
    )
    .bind(source_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_source_current_manifest_digest");
    let error = sqlx::query(
        "UPDATE search_source_coordination SET current_bundle_digest='sha256:BAD' WHERE source_id=$1",
    )
    .bind(source_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_source_current_bundle_digest");

    sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,$5,1,clock_timestamp(),'v1')",
    )
    .bind(source_id)
    .bind(event_id)
    .bind(generation_id)
    .bind(&manifest_digest)
    .bind(&bundle_digest)
    .execute(&pool)
    .await
    .unwrap();
    let error = sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,$5,2,clock_timestamp(),'v1')",
    )
    .bind(source_id)
    .bind(event_id)
    .bind(generation_id)
    .bind(&manifest_digest)
    .bind(&bundle_digest)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23505", "search_index_receipts_pkey");
    let error = sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,$5,0,clock_timestamp(),'v1')",
    )
    .bind(source_id)
    .bind(Uuid::from_u128(5))
    .bind(generation_id)
    .bind(&manifest_digest)
    .bind(&bundle_digest)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_receipt_positive_epoch");
    let error = sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,'sha256:BAD',$4,1,clock_timestamp(),'v1')",
    )
    .bind(source_id)
    .bind(Uuid::from_u128(6))
    .bind(generation_id)
    .bind(&bundle_digest)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_receipt_digest");
    let error = sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,'sha256:BAD',1,clock_timestamp(),'v1')",
    )
    .bind(source_id)
    .bind(Uuid::from_u128(7))
    .bind(generation_id)
    .bind(&manifest_digest)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_database_error(error, "23514", "ck_search_receipt_bundle_digest");
}

#[tokio::test]
async fn migration_keeps_pointer_and_receipt_same_database() {
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
        (1..=12).collect::<Vec<_>>()
    );

    search_runtime::migrate(&pool).await.unwrap();
    let search_before: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version,checksum FROM search_runtime_sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        search_before
            .iter()
            .map(|(version, _)| *version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert!(
        search_before
            .iter()
            .all(|(_, checksum)| !checksum.is_empty())
    );

    let event_id = Uuid::from_u128(10);
    let source_id = Uuid::from_u128(11);
    let generation_id = Uuid::from_u128(12);
    let manifest_digest = format!("sha256:{}", "c".repeat(64));
    let bundle_digest = format!("sha256:{}", "d".repeat(64));
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at) VALUES ($1,'DocumentRegistered','Document',$2,'{}'::jsonb,clock_timestamp(),clock_timestamp())",
    )
    .bind(event_id)
    .bind(Uuid::from_u128(13))
    .execute(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,'test-tenant','DOCUMENT',1,1,1,'ACTIVE','{\"dto_version\":\"v1\"}'::jsonb,$2,clock_timestamp(),clock_timestamp())",
    )
    .bind(source_id)
    .bind(format!("sha256:{}", "e".repeat(64)))
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO search_source_coordination (source_id,tenant_owner_key,registration_revision,visibility_revision,activation_epoch,registration_active,fence_epoch) VALUES ($1,'test-tenant',1,1,1,true,1)",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'test-tenant',1,clock_timestamp())")
        .bind(source_id)
        .bind(generation_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO search_generation (source_id,generation_id,activation_epoch,state,build_kind,stage_origin,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version) VALUES ($1,$2,1,'BUILDING','INCREMENTAL','MANUAL','coordination-test','{"dto_version":"v1"}'::jsonb,$3,0,'v1')"#)
        .bind(source_id)
        .bind(generation_id)
        .bind(&manifest_digest)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$4,pointer_revision=1,last_published_epoch=1 WHERE source_id=$1")
        .bind(source_id)
        .bind(generation_id)
        .bind(&manifest_digest)
        .bind(&bundle_digest)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest,bundle_digest,fence_epoch,recorded_at,bundle_version) VALUES ($1,$2,$3,$4,$5,1,clock_timestamp(),'v1')",
    )
    .bind(source_id)
    .bind(event_id)
    .bind(generation_id)
    .bind(&manifest_digest)
    .bind(&bundle_digest)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let paired: bool = sqlx::query_scalar(
        "SELECT s.current_generation_id=r.generation_id AND s.current_manifest_digest=r.digest AND s.current_bundle_digest=r.bundle_digest FROM outbox_events o JOIN search_index_receipts r ON r.event_id=o.event_id JOIN search_source_coordination s ON s.source_id=r.source_id WHERE o.event_id=$1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(paired);

    search_runtime::migrate(&pool).await.unwrap();
    let search_after: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT version,checksum FROM search_runtime_sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let domain_after: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(search_after, search_before);
    assert_eq!(domain_after, domain_before);
}

#[tokio::test]
async fn source_row_is_preseeded_before_lease() {
    let (_container, pool) = postgres().await;
    migrate_both(&pool).await;
    let source_id = Uuid::from_u128(20);
    let owner_token = Uuid::from_u128(21);
    let absent_epoch: Option<i64> = sqlx::query_scalar(
        "UPDATE search_source_coordination SET fence_epoch=fence_epoch+1,owner_token=$2,lease_expires_at=clock_timestamp()+interval '1 minute' WHERE source_id=$1 AND owner_token IS NULL AND fence_epoch<9223372036854775807 RETURNING fence_epoch",
    )
    .bind(source_id)
    .bind(owner_token)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(absent_epoch, None, "missing Source cannot be claimed");
    let row_count: i64 = sqlx::query_scalar("SELECT count(*) FROM search_source_coordination")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row_count, 0, "claim must not implicitly create a Source");

    register_source(&pool, source_id).await;
    let initial: (i64, i64, i64, i64, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT fence_epoch,pointer_revision,last_published_epoch,build_fence_seq,owner_token,current_generation_id FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(source_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(initial, (0, 0, 0, 0, None, None));
    let epoch: Option<i64> = sqlx::query_scalar(
        "UPDATE search_source_coordination SET fence_epoch=fence_epoch+1,owner_token=$2,lease_expires_at=clock_timestamp()+interval '1 minute' WHERE source_id=$1 AND owner_token IS NULL AND fence_epoch<9223372036854775807 RETURNING fence_epoch",
    )
    .bind(source_id)
    .bind(owner_token)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(epoch, Some(1));

    sqlx::query(
        "UPDATE search_source_coordination SET owner_token=NULL,lease_expires_at=NULL,fence_epoch=9223372036854775807 WHERE source_id=$1",
    )
    .bind(source_id)
    .execute(&pool)
    .await
    .unwrap();
    let overflow_claim: Option<i64> = sqlx::query_scalar(
        "UPDATE search_source_coordination SET fence_epoch=fence_epoch+1,owner_token=$2,lease_expires_at=clock_timestamp()+interval '1 minute' WHERE source_id=$1 AND owner_token IS NULL AND fence_epoch<9223372036854775807 RETURNING fence_epoch",
    )
    .bind(source_id)
    .bind(owner_token)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(overflow_claim, None, "exhausted epoch cannot be claimed");
}
