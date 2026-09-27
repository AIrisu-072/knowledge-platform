//! Task 11 RED: immutable derived-record schema constraints.

#[path = "support/semantic_inspection.rs"]
mod support;

use document_repository_postgres::migrate;
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test]
async fn migration_constrains_identity_binding_and_sha256_digest() {
    let (_container, pool) = support::postgres().await;
    migrate(&pool).await.unwrap();
    let file_id = Uuid::from_u128(11);
    support::seed_file(&pool, file_id).await;

    assert!(
        insert(
            &pool,
            Uuid::from_u128(12),
            "dsi-v0",
            &[7; 32],
            13,
            "sha256",
            &[1; 32]
        )
        .await
        .is_err(),
        "foreign key"
    );
    assert!(
        insert(&pool, file_id, "unknown", &[7; 32], 13, "sha256", &[1; 32])
            .await
            .is_err(),
        "profile"
    );
    assert!(
        insert(&pool, file_id, "dsi-v0", &[7; 31], 13, "sha256", &[1; 32])
            .await
            .is_err(),
        "raw hash length"
    );
    assert!(
        insert(&pool, file_id, "dsi-v0", &[7; 32], -1, "sha256", &[1; 32])
            .await
            .is_err(),
        "negative size"
    );
    assert!(
        insert(&pool, file_id, "dsi-v0", &[7; 32], 13, "sha512", &[1; 32])
            .await
            .is_err(),
        "fingerprint algorithm"
    );
    assert!(
        insert(&pool, file_id, "dsi-v0", &[7; 32], 13, "sha256", &[1; 31])
            .await
            .is_err(),
        "fingerprint digest length"
    );

    insert(&pool, file_id, "dsi-v0", &[7; 32], 13, "sha256", &[1; 32])
        .await
        .unwrap();
    assert!(
        insert(&pool, file_id, "dsi-v0", &[7; 32], 13, "sha256", &[1; 32])
            .await
            .is_err(),
        "one immutable key"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM document_semantic_inspections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "no failed insert leaves a partial row");
}

async fn insert(
    pool: &PgPool,
    file_id: Uuid,
    profile: &str,
    raw_hash: &[u8],
    size: i64,
    algorithm: &str,
    digest: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO document_semantic_inspections (
            file_id, inspection_profile_version, worker_protocol_version,
            observed_raw_content_hash, observed_size_bytes, detected_format,
            fingerprint_algorithm, fingerprint_digest, semantic_capabilities,
            editorial_provenance, external_dependencies, digital_signature_evidence,
            worker_build_id, adapter_id, adapter_version, parser_libraries,
            native_dependency_identity, diagnostics, inspected_at
         ) VALUES (
            $1, $2, 'dsi-worker-v0', $3, $4, 'txt', $5, $6,
            '[]'::jsonb, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb,
            'task11-test', 'text', '0', '[]'::jsonb, '[]'::jsonb,
            '[]'::jsonb, now()
         )",
    )
    .bind(file_id)
    .bind(profile)
    .bind(raw_hash)
    .bind(size)
    .bind(algorithm)
    .bind(digest)
    .execute(pool)
    .await
    .map(|_| ())
}
