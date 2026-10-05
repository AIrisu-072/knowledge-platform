//! P7-05: immutable file-backed lexical generations and the two-way Unit seal.

#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use search_application::body_ports::LexicalRetrievalBatch;
use search_application::ports::{LexicalQuery, LexicalRetrieverPort};
use search_application::search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_application::search_core::evidence::EvidenceRequirement;
use search_application::search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, SourceId,
};
use search_application::search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_application::search_core::observation::Coverage;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_application::search_core::resource::ResourceKind;
use search_application::search_core::source::{
    DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode,
};
use search_application::search_core::temporal::TemporalEvaluationContext;
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_runtime::full_guard::FullGuardTtl;
use search_runtime::generation_registration::{FullBuildRequest, PgGenerationRegistrar};
use search_runtime::lexical_artifact::{LexicalArtifactError, LexicalArtifactStore};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use search_source_document::{ArtifactReceipt, BodyUnitManifest};
use search_tantivy::{
    LexicalBuildInput, LexicalDocument, TantivyLexicalIndex, lexical_input_digest,
};
use sqlx::PgPool;
use time::OffsetDateTime;
use units::SNAPSHOT;
use uuid::Uuid;

fn source_id() -> SourceId {
    registration::source(7501)
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

fn manifest(generation: u128) -> ProjectionGenerationManifest {
    ProjectionGenerationManifest {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(generation)),
        projection_schema_version: "schema-1".into(),
        lens_version: 1,
        semantic_registry_version: "registry-v1".into(),
        analyzer_version: Some("tantivy-default-0.26.2".into()),
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: SNAPSHOT.into(),
        resource_count: 1,
        relation_count: None,
        coverage: Coverage::CompleteEnumeration,
        digest: format!("sha256:{}", "b".repeat(64)),
        built_at: OffsetDateTime::UNIX_EPOCH,
    }
}

fn unit_manifest(key: ProjectionGenerationKey, lines: &[&str]) -> BodyUnitManifest {
    BodyUnitManifest {
        key,
        source_snapshot: SNAPSHOT.into(),
        entries: vec![units::entry(source_id(), lines)],
    }
}

fn input(manifest: &BodyUnitManifest) -> LexicalBuildInput {
    LexicalBuildInput::new(
        source_id(),
        SNAPSHOT,
        "schema-1",
        1,
        vec![LexicalDocument {
            resource_ref: units::parent(),
            kind: ResourceKind::Document,
            canonical_name: "規程".into(),
            title: Some("規程".into()),
            aliases: vec![],
            high_signal_text: None,
            body: None,
            locator: Some("document:7402".into()),
        }],
    )
    .with_body_units(
        manifest
            .entries
            .iter()
            .flat_map(|entry| entry.units.clone())
            .collect(),
    )
}

struct Fixture {
    _guard: support::postgres::DatabaseGuard,
    pool: PgPool,
    registrar: PgGenerationRegistrar,
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn fixture() -> Fixture {
    let (guard, pool, _) = support::postgres::postgres("lexical_artifact_test").await;
    document_repository_postgres::migrate(&pool).await.unwrap();
    search_runtime::migrate(&pool).await.unwrap();
    let host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let ledger = PgSourceRegistrationLedger::new(pool.clone(), host.clone());
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
    let root = std::env::temp_dir().join(format!("search-lexical-artifact-{}", Uuid::now_v7()));
    Fixture {
        _guard: guard,
        pool,
        registrar,
        root,
    }
}

impl Fixture {
    async fn register(&self, generation: u128) {
        self.registrar
            .register_manual(
                &FullBuildRequest {
                    manifest: manifest(generation),
                    expected_snapshot: SNAPSHOT.into(),
                },
                FullGuardTtl::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap();
    }

    fn store(&self) -> LexicalArtifactStore {
        LexicalArtifactStore::new(&self.root, self.pool.clone())
    }

    /// Builds the generation into its staging directory and returns the P1
    /// lexical receipt computed from the same input.
    fn stage(&self, generation: u128, units: &BodyUnitManifest) -> ArtifactReceipt {
        let manifest = manifest(generation);
        let input = input(units);
        let logical = lexical_input_digest(&input).unwrap();
        TantivyLexicalIndex::new()
            .build_generation_at(
                manifest.clone(),
                &source(),
                input,
                &self.store().staging_dir(manifest.key()),
            )
            .unwrap();
        ArtifactReceipt {
            key: manifest.key(),
            digest: logical.digest,
            count: logical.count,
        }
    }
}

fn request() -> DiscoveryRequest {
    let now = OffsetDateTime::from_unix_timestamp(100).unwrap();
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(1)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![],
            required_claims: vec![],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(2)),
            now,
            now,
            "UTC",
        ),
        access_context: "principal".into(),
    }
}

#[tokio::test]
async fn finalized_directory_reopens_seals_and_serves_queries() {
    let fixture = fixture().await;
    fixture.register(7_510).await;
    let key = manifest(7_510).key();
    let units = unit_manifest(key, &["東京の本文", "大阪の補足"]);
    let receipt = fixture.stage(7_510, &units);
    let store = fixture.store();
    let sealed = store
        .finalize(
            &manifest(7_510),
            &source(),
            &store.staging_dir(key),
            &receipt,
            &units,
        )
        .await
        .unwrap();
    assert_eq!(sealed.index_relpath, LexicalArtifactStore::relpath(key));
    assert_eq!(sealed.unit_seal_count, 2);
    assert_eq!(sealed.searchable_doc_count, 3);
    assert!(!store.staging_dir(key).exists());

    // A fresh process view: reopen from disk, compare with the saved row.
    let reopened = store
        .reopen_and_validate(&manifest(7_510), &source(), &units)
        .await
        .unwrap();
    assert_eq!(reopened, sealed);
    let index = TantivyLexicalIndex::new();
    index
        .load_generation_at(&manifest(7_510), &source(), &store.final_dir(key))
        .unwrap();
    let batch: LexicalRetrievalBatch = index
        .retrieve_body(key, &request(), &LexicalQuery::body_only("東京の本文", 10))
        .await
        .unwrap();
    assert_eq!(batch.hits.len(), 1);
    assert!(batch.hits[0].unit_hit.is_some());
}

#[tokio::test]
async fn self_reported_receipt_missing_units_and_corrupt_files_fail_closed() {
    let fixture = fixture().await;
    let store = fixture.store();

    // A builder's self-reported digest that the reopened index does not produce.
    fixture.register(7_520).await;
    let key = manifest(7_520).key();
    let units = unit_manifest(key, &["東京の本文"]);
    let mut forged = fixture.stage(7_520, &units);
    forged.digest = [0; 32];
    assert_eq!(
        store
            .finalize(
                &manifest(7_520),
                &source(),
                &store.staging_dir(key),
                &forged,
                &units
            )
            .await,
        Err(LexicalArtifactError::Receipt)
    );

    // An index whose Unit documents differ from the manifest never seals.
    fixture.register(7_521).await;
    let key = manifest(7_521).key();
    let indexed = unit_manifest(key, &["東京の本文"]);
    let receipt = fixture.stage(7_521, &indexed);
    let claimed = unit_manifest(key, &["東京の本文", "大阪の補足"]);
    assert_eq!(
        store
            .finalize(
                &manifest(7_521),
                &source(),
                &store.staging_dir(key),
                &receipt,
                &claimed
            )
            .await,
        Err(LexicalArtifactError::Seal)
    );

    // Only the trusted staging path for the key is accepted.
    fixture.register(7_522).await;
    let key = manifest(7_522).key();
    let units = unit_manifest(key, &["東京の本文"]);
    let receipt = fixture.stage(7_522, &units);
    let elsewhere = fixture.root.join("elsewhere");
    assert_eq!(
        store
            .finalize(&manifest(7_522), &source(), &elsewhere, &receipt, &units)
            .await,
        Err(LexicalArtifactError::Path)
    );
    let sealed = store
        .finalize(
            &manifest(7_522),
            &source(),
            &store.staging_dir(key),
            &receipt,
            &units,
        )
        .await
        .unwrap();
    assert_eq!(sealed.unit_seal_count, 1);

    // Any byte change on disk after finalization is detected on reopen.
    let sidecar = store.final_dir(key).join("lexical-input.json");
    let mut bytes = std::fs::read(&sidecar).unwrap();
    bytes.push(b' ');
    std::fs::write(&sidecar, bytes).unwrap();
    assert_eq!(
        store
            .reopen_and_validate(&manifest(7_522), &source(), &units)
            .await,
        Err(LexicalArtifactError::Drift)
    );

    // Without a registered BUILDING parent and live guard no row is recorded.
    let key = manifest(7_523).key();
    let units = unit_manifest(key, &["東京の本文"]);
    let receipt = fixture.stage(7_523, &units);
    assert_eq!(
        store
            .finalize(
                &manifest(7_523),
                &source(),
                &store.staging_dir(key),
                &receipt,
                &units
            )
            .await,
        Err(LexicalArtifactError::Rejected)
    );
}
