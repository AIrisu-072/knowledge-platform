//! P7-04: typed full payloads survive storage and are recomputed on restore.

#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use std::sync::Arc;
use std::time::Duration;

use search_application::ports::SemanticRegistrySnapshot;
use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_projection_memory::generation_digest;
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::generation_registration::FullBuildRequest;
use search_runtime::payload::{
    BundleError, PgPayloadStore, ProjectionPayloadV1, StoredBundleV1, validate_stored_bundle_v1,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_source_document::{
    ArtifactReceipt, BodyUnitManifest, compute_bundle_receipt, validate_restored_manifest,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use time::OffsetDateTime;
use uuid::Uuid;

use units::SNAPSHOT;

fn source() -> SourceId {
    registration::source(7401)
}

fn registry() -> SemanticRegistrySnapshot {
    SemanticRegistrySnapshot::new("registry-v1")
}

fn manifest(generation: u128) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "projection-v1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-v1".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: SNAPSHOT.into(),
        resource_count: 0,
        relation_count: None,
        coverage: Coverage::CompleteEnumeration,
        digest: generation_digest(source(), &[], &registry()).unwrap(),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

fn bundle(generation: u128, lines: &[&str]) -> StoredBundleV1 {
    let manifest = manifest(generation);
    let key = manifest.key();
    let unit_manifest = BodyUnitManifest {
        key,
        source_snapshot: SNAPSHOT.into(),
        entries: vec![units::entry(source(), lines)],
    };
    let coverage = validate_restored_manifest(&unit_manifest).unwrap();
    let artifact = |digest, count| ArtifactReceipt { key, digest, count };
    let receipt = compute_bundle_receipt(
        key,
        SNAPSHOT,
        &manifest.digest,
        &unit_manifest,
        &coverage,
        artifact([3; 32], 2),
        artifact([4; 32], 0),
    )
    .unwrap();
    StoredBundleV1 {
        manifest,
        projection: ProjectionPayloadV1 {
            resources: vec![],
            registry: registry(),
        },
        unit_manifest,
        coverage,
        receipt,
    }
}

async fn fixture() -> (
    support::postgres::DatabaseGuard,
    PgPool,
    sqlx::postgres::PgConnectOptions,
    search_runtime::generation_registration::PgGenerationRegistrar,
) {
    let (guard, pool, options) = support::postgres::postgres("bundle_durability_test").await;
    document_repository_postgres::migrate(&pool).await.unwrap();
    search_runtime::migrate(&pool).await.unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(pool.clone(), host.clone());
    let empty_remote = registration::publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let document = registration::document(source(), "tenant-a").await;
    let desired = registration::publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document.clone()],
    )
    .await;
    let activations = ledger.reconcile(&desired).await.unwrap();
    let registrar = ledger
        .generation_registrar(document, activations[&source()])
        .unwrap();
    (guard, pool, options, registrar)
}

async fn register(
    registrar: &search_runtime::generation_registration::PgGenerationRegistrar,
    generation: u128,
) -> ProjectionGenerationKey {
    registrar
        .register_manual(
            &FullBuildRequest {
                manifest: manifest(generation),
                expected_snapshot: SNAPSHOT.into(),
            },
            FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap()
        .key()
}

#[test]
fn body_only_change_keeps_projection_digest_and_changes_composite() {
    let first = bundle(7_410, &["東京の本文"]);
    let second = bundle(7_410, &["大阪の本文"]);
    let first = validate_stored_bundle_v1(&first).unwrap();
    let second = validate_stored_bundle_v1(&second).unwrap();
    assert_eq!(
        first.receipt.projection_digest,
        second.receipt.projection_digest
    );
    assert_ne!(first.receipt.unit_manifest, second.receipt.unit_manifest);
    assert_ne!(
        first.receipt.composite_digest,
        second.receipt.composite_digest
    );
}

#[test]
fn pure_validation_rejects_forged_digests_and_bindings() {
    let mut forged = bundle(7_411, &["東京の本文"]);
    forged.receipt.composite_digest = [0; 32];
    assert_eq!(validate_stored_bundle_v1(&forged), Err(BundleError::Digest));
    let mut swapped = bundle(7_411, &["東京の本文"]);
    swapped.unit_manifest.entries[0].units[0].text = "大阪の本文".into();
    assert_eq!(
        validate_stored_bundle_v1(&swapped),
        Err(BundleError::Digest)
    );
    let mut moved = bundle(7_411, &["東京の本文"]);
    moved.coverage.key.generation_id = ProjectionGenerationId::from_uuid(Uuid::from_u128(1));
    assert_eq!(validate_stored_bundle_v1(&moved), Err(BundleError::Binding));
    let mut registry_drift = bundle(7_411, &["東京の本文"]);
    registry_drift.projection.registry.version = "registry-v2".into();
    assert_eq!(
        validate_stored_bundle_v1(&registry_drift),
        Err(BundleError::Binding)
    );
}

#[tokio::test]
async fn stored_bundle_restores_on_a_new_connection_with_the_same_digests() {
    let (_guard, pool, options, registrar) = fixture().await;
    let key = register(&registrar, 7_420).await;
    let stored = bundle(7_420, &["東京の本文", "大阪の補足"]);
    assert_eq!(stored.manifest.key(), key);
    PgPayloadStore::new(pool.clone())
        .store(&stored)
        .await
        .unwrap();

    // A separate pool restores from JSONB; digests are recomputed, not copied.
    let other = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let restored = PgPayloadStore::new(other)
        .load(&stored.manifest, &stored.receipt)
        .await
        .unwrap();
    assert_eq!(restored, stored);
    assert_eq!(
        validate_stored_bundle_v1(&restored)
            .unwrap()
            .receipt
            .composite_digest,
        stored.receipt.composite_digest
    );

    // T12: the same checks without holding the Units, from cold and from the
    // per-process summaries.
    for _ in 0..2 {
        let summary = PgPayloadStore::new(pool.clone())
            .restore_without_units(&stored.manifest)
            .await
            .unwrap();
        assert_eq!(summary.projection, stored.projection);
        assert_eq!(summary.coverage, stored.coverage);
    }
}

/// A payload larger than one JSONB value is stored as ordered text chunks
/// and restored to the same digests; a missing chunk fails closed.
#[tokio::test]
async fn chunked_payload_restores_and_a_missing_chunk_fails_closed() {
    let (_guard, pool, _options, registrar) = fixture().await;
    let key = register(&registrar, 7_425).await;
    let stored = bundle(7_425, &["東京の本文", "大阪の補足"]);
    PgPayloadStore::new(pool.clone())
        .with_chunk_bytes(64)
        .store(&stored)
        .await
        .unwrap();
    let chunks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM search_generation_payload \
         WHERE source_id=$1 AND generation_id=$2 AND kind='unit_manifest'",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(chunks > 2, "{chunks}");
    let store = PgPayloadStore::new(pool.clone());
    assert_eq!(
        store.load(&stored.manifest, &stored.receipt).await.unwrap(),
        stored
    );
    // A gap in the chunk sequence (here: chunk 1 renumbered) fails closed.
    sqlx::query(
        "UPDATE search_generation_payload SET chunk=1000 \
         WHERE source_id=$1 AND generation_id=$2 AND kind='unit_manifest' AND chunk=1",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        store.load(&stored.manifest, &stored.receipt).await,
        Err(BundleError::Shape)
    );
}

#[tokio::test]
async fn tampered_unknown_or_unguarded_payload_fails_closed() {
    let (_guard, pool, _options, registrar) = fixture().await;
    let key = register(&registrar, 7_430).await;
    let stored = bundle(7_430, &["東京の本文"]);
    let store = PgPayloadStore::new(pool.clone());
    store.store(&stored).await.unwrap();
    let restore = || store.load(&stored.manifest, &stored.receipt);

    // An unknown field anywhere in the DTO is refused.
    sqlx::query(
        "UPDATE search_generation_payload SET payload = jsonb_set(payload, '{body,extra}', '1') \
         WHERE source_id=$1 AND generation_id=$2 AND kind='body_coverage'",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(restore().await, Err(BundleError::Shape));
    sqlx::query(
        "UPDATE search_generation_payload SET payload = payload #- '{body,extra}' \
         WHERE source_id=$1 AND generation_id=$2 AND kind='body_coverage'",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert!(restore().await.is_ok());

    // A segment row is immutable; an update fails even for its owner.
    assert!(
        sqlx::query(
            "UPDATE search_unit_segment \
         SET payload = jsonb_set(payload, '{body,units,0,text}', '\"大阪の本文\"') \
         WHERE segment_digest = (SELECT segment_digest FROM search_generation_segment \
         WHERE source_id=$1 AND generation_id=$2 AND ordinal=0)",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .execute(&pool)
        .await
        .is_err()
    );
    // Unit text changed below the triggers is found when a process first reads it.
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE search_unit_segment \
         SET payload = jsonb_set(payload, '{body,units,0,text}', '\"大阪の本文\"') \
         WHERE segment_digest = (SELECT segment_digest FROM search_generation_segment \
         WHERE source_id=$1 AND generation_id=$2 AND ordinal=0)",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    search_runtime::payload::forget_verified_segments();
    assert_eq!(restore().await, Err(BundleError::Digest));
    assert_eq!(
        store.restore_without_units(&stored.manifest).await,
        Err(BundleError::Digest)
    );

    // A key without a registered BUILDING parent and live guard cannot be written.
    let unregistered = bundle(7_431, &["東京の本文"]);
    assert_eq!(store.store(&unregistered).await, Err(BundleError::Rejected));
    // A second write of the same kinds conflicts instead of replacing.
    assert_eq!(store.store(&stored).await, Err(BundleError::Rejected));
}

async fn segment_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM search_unit_segment")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// An unchanged item is stored once and listed by every generation that has
/// it; GC deletes only the segments no generation lists.
#[tokio::test]
async fn unchanged_items_share_one_segment_and_gc_keeps_listed_segments() {
    let (_guard, pool, options, registrar) = fixture().await;
    let store = PgPayloadStore::new(pool.clone());
    register(&registrar, 7_440).await;
    let first = bundle(7_440, &["東京の本文", "共有の補足"]);
    store.store(&first).await.unwrap();
    assert_eq!(segment_rows(&pool).await, 1);

    register(&registrar, 7_441).await;
    let same = bundle(7_441, &["東京の本文", "共有の補足"]);
    store.store(&same).await.unwrap();
    assert_eq!(segment_rows(&pool).await, 1);

    let changed_key = register(&registrar, 7_442).await;
    let changed = bundle(7_442, &["大阪の本文", "共有の補足"]);
    store.store(&changed).await.unwrap();
    assert_eq!(segment_rows(&pool).await, 2);

    // Every generation restores its own snapshot binding from shared rows.
    search_runtime::payload::forget_verified_segments();
    let other = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let reader = PgPayloadStore::new(other);
    assert_eq!(
        reader.load(&same.manifest, &same.receipt).await.unwrap(),
        same
    );
    assert_eq!(
        reader
            .load(&changed.manifest, &changed.receipt)
            .await
            .unwrap(),
        changed
    );

    // The changed generation's list goes away; its own segment is swept and
    // the shared one stays because the first two generations still list it.
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM search_generation_segment WHERE source_id=$1 AND generation_id=$2")
        .bind(changed_key.source_id.as_uuid())
        .bind(changed_key.generation_id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let root = std::env::temp_dir().join(format!("segments-gc-{}", Uuid::new_v4()));
    let gc = search_runtime::gc::PgGenerationGc::new(pool.clone(), &root);
    assert_eq!(gc.sweep_unreferenced_segments().await.unwrap(), 1);
    assert_eq!(segment_rows(&pool).await, 1);
    assert_eq!(gc.sweep_unreferenced_segments().await.unwrap(), 0);
    assert_eq!(
        reader.load(&first.manifest, &first.receipt).await.unwrap(),
        first
    );
}
