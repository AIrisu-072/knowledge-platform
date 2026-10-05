//! P7-04: typed full payloads survive storage and are recomputed on restore.

#[path = "support/registration.rs"]
mod registration;
mod support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use search_application::ports::SemanticRegistrySnapshot;
use search_application::search_core::id::{ProjectionGenerationId, ResourceId, SourceId};
use search_application::search_core::knowledge_unit::{
    BudgetKey, ContentPartRef, ExtractionProfileDefinitionV1, ExtractionProfileId, FormatId,
    FormatSettings, KnowledgeUnit, NativeLocator, RawBinding, ResourceVersionRef, UnitId, UnitKind,
    UnitProvenance, text_sha256,
};
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_extraction_core::{BodyCoverage, ItemOperationState};
use search_projection_memory::generation_digest;
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::generation_registration::FullBuildRequest;
use search_runtime::payload::{
    BundleError, PgPayloadStore, ProjectionPayloadV1, StoredBundleV1, validate_stored_bundle_v1,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_source_document::{
    ArtifactReceipt, BodyItemEntry, BodyUnitManifest, compute_bundle_receipt,
    validate_restored_manifest,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use time::OffsetDateTime;
use uuid::Uuid;

const SNAPSHOT: &str = "synthetic-snapshot-v1";

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

fn profile() -> ExtractionProfileId {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 1024);
    limits.insert(BudgetKey::Units, 16);
    limits.insert(BudgetKey::UnitUtf8Bytes, 1024);
    limits.insert(BudgetKey::WorkerOutputBytes, 65_536);
    ExtractionProfileId::for_definition(&ExtractionProfileDefinitionV1 {
        format: FormatId::Text,
        parser_name: "search-extraction-worker".into(),
        parser_version: "1".into(),
        parser_build_sha256: [1; 32],
        native_binary_sha256: None,
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: FormatSettings::Text {
            charset: "utf-8".into(),
        },
        limits,
    })
    .unwrap()
}

/// One Supported text item of one Version with the given line Units.
fn entry(lines: &[&str]) -> BodyItemEntry {
    let version_id = Uuid::from_u128(7_402);
    let version = ResourceVersionRef {
        source_id: source(),
        resource_id: ResourceId::from_uuid(version_id),
        source_native_version: version_id.to_string(),
    };
    let part = ContentPartRef {
        source_native_part_id: Uuid::from_u128(7_403).to_string(),
        logical_path: "本文/primary".into(),
        ordinal: 0,
    };
    let raw = RawBinding {
        sha256: [9; 32],
        size_bytes: 64,
        media_type: "text/plain".into(),
    };
    let units = lines
        .iter()
        .enumerate()
        .map(|(ordinal, text)| {
            let ordinal = u32::try_from(ordinal).unwrap();
            let locator = NativeLocator::Text {
                line_start: ordinal,
                line_end: ordinal + 1,
            };
            KnowledgeUnit {
                unit_id: UnitId::derive(&version, &part, &profile(), &locator, ordinal).unwrap(),
                version: version.clone(),
                part: part.clone(),
                parent_unit_id: None,
                ordinal,
                kind: UnitKind::PlainText,
                text: (*text).into(),
                locator,
                text_sha256: text_sha256(text),
                provenance: UnitProvenance {
                    source_snapshot: SNAPSHOT.into(),
                    authoritative_representation_ref: Uuid::from_u128(7_404).to_string(),
                    raw: raw.clone(),
                    detected_format: FormatId::Text,
                    archive_inner_format: None,
                    profile: profile(),
                    parser_build_id: "search-extraction-worker-test".into(),
                },
            }
        })
        .collect();
    BodyItemEntry {
        version,
        part,
        authoritative_representation_ref: Uuid::from_u128(7_404).to_string(),
        raw,
        detected_format: Some(FormatId::Text),
        profile: Some(profile()),
        parser_build_id: "search-extraction-worker-test".into(),
        archive_plan: None,
        operation: ItemOperationState::Completed,
        coverage: Some(BodyCoverage::Supported),
        units,
    }
}

fn bundle(generation: u128, lines: &[&str]) -> StoredBundleV1 {
    let manifest = manifest(generation);
    let key = manifest.key();
    let unit_manifest = BodyUnitManifest {
        key,
        source_snapshot: SNAPSHOT.into(),
        entries: vec![entry(lines)],
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

    // Unit text changed behind an unchanged digest column.
    sqlx::query(
        "UPDATE search_generation_payload \
         SET payload = jsonb_set(payload, '{body,entries,0,units,0,text}', '\"大阪の本文\"') \
         WHERE source_id=$1 AND generation_id=$2 AND kind='unit_manifest'",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(restore().await, Err(BundleError::Digest));

    // A key without a registered BUILDING parent and live guard cannot be written.
    let unregistered = bundle(7_431, &["東京の本文"]);
    assert_eq!(store.store(&unregistered).await, Err(BundleError::Rejected));
    // A second write of the same kinds conflicts instead of replacing.
    assert_eq!(store.store(&stored).await, Err(BundleError::Rejected));
}
