#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod versioning_support;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use document_application::{
    BootstrapRootPolicy, DocumentAccessCheckService, InvocationKind, VerifiedActorContext,
};
use document_domain::{
    Action, DocumentId, DocumentVersionId, FolderId, LifecycleState, PolicyGrant, PolicySubject,
    PolicySubjectKind, Title,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID, migrate};
use search_application::SearchError;
use search_application::indexing_service::{
    DocumentIndexingService, DocumentSourceEvent, IndexingOutcome,
};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, HyperGraphRetrieverPort, LexicalQuery,
    LexicalRetrieverPort, ProjectionGenerationStore, SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_core::discovery::{DiscoveryNeed, DiscoveryRequest};
use search_core::evidence::EvidenceRequirement;
use search_core::graph::{GraphTraversalPlan, RelationPathPattern, TraversalBudget};
use search_core::id::{
    DiscoveryEvaluationId, NeedId, ProjectionGenerationId, ResourceId, SourceId,
};
use search_core::intent::{IntentFact, IntentFactOrigin, IntentSignature};
use search_core::profile::DiscoveryLens;
use search_core::projection::{ProjectionGenerationKey, ProjectionGenerationManifest};
use search_core::relation::RelationNamespace;
use search_core::resource::ResourceKind;
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use search_core::temporal::TemporalEvaluationContext;
use search_source_document::{
    DocumentAccessProjectionInput, DocumentCurrentAccessAdapter, DocumentIndexRuntime,
    DocumentIndexingConfig, DocumentOutboxIndexer, DocumentOutboxReader, DocumentOutboxSnapshot,
    DocumentSourceSnapshot, DsiReadState, IndexingReceipt, IndexingReceiptStore,
    MemoryDocumentIndexRuntime, PermittedDocumentMetadata, PostgresDocumentSnapshotReader,
    PublicationEndRecord, VersionSnapshotRecord, document_resource_id, folder_resource_id,
};
use search_tantivy::{LexicalBuildInput, LexicalIndexError};
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use time::{Duration, OffsetDateTime};
use tokio::sync::Notify;
use uuid::Uuid;

fn at(second: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(second).unwrap()
}

fn version(number: u128) -> DocumentVersionId {
    DocumentVersionId::from_uuid(Uuid::from_u128(number))
}

fn record(
    number: u128,
    title: &str,
    source_snapshot: &str,
    current: bool,
) -> VersionSnapshotRecord {
    VersionSnapshotRecord {
        snapshot: DocumentSourceSnapshot {
            source_snapshot: source_snapshot.into(),
            document_id: DocumentId::from_uuid(Uuid::from_u128(10)),
            document_version_id: version(number),
            current_version_id: current
                .then(|| version(number))
                .or_else(|| Some(version(2))),
            publication_end: None,
            lifecycle_state: LifecycleState::Published,
            title: Title::new(title).unwrap(),
            metadata: PermittedDocumentMetadata {
                document_type: Some("policy".into()),
                category: None,
            },
            folder_id: FolderId::from_uuid(Uuid::from_u128(30)),
            created_at: at(100),
            published_at: Some(at(200)),
            withdrawn_at: None,
            effective_from: None,
            effective_to: None,
            access: DocumentAccessProjectionInput {
                access_scope: Some("scope".into()),
            },
            dsi: None,
        },
        document_revision: 2,
        access_revision: 1,
        dsi_state: DsiReadState::UnknownMissing,
    }
}

#[derive(Clone)]
struct FakeReader(Arc<Mutex<Vec<VersionSnapshotRecord>>>);

impl FakeReader {
    fn set(&self, records: Vec<VersionSnapshotRecord>) {
        *self.0.lock().unwrap() = records;
    }
}

fn split_snapshot(records: Vec<VersionSnapshotRecord>) -> DocumentOutboxSnapshot {
    let source_snapshot = records.first().map_or_else(
        || "empty-snapshot".to_owned(),
        |r| r.snapshot.source_snapshot.clone(),
    );
    let (live, historical) = records.into_iter().partition(|r| {
        r.snapshot.lifecycle_state == LifecycleState::Published
            && r.snapshot.current_version_id == Some(r.snapshot.document_version_id)
            && r.snapshot.publication_end.is_none()
    });
    DocumentOutboxSnapshot {
        source_snapshot,
        live,
        historical,
    }
}

impl DocumentOutboxReader for FakeReader {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move { Ok(split_snapshot(self.0.lock().unwrap().clone())) })
    }
}

struct PausedReader {
    reader: FakeReader,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    pause_first: AtomicBool,
}

impl DocumentOutboxReader for PausedReader {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move {
            let captured = split_snapshot(self.reader.0.lock().unwrap().clone());
            if self.pause_first.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            Ok(captured)
        })
    }
}

struct MixedReader(DocumentOutboxSnapshot);

impl DocumentOutboxReader for MixedReader {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

#[derive(Clone, Copy, Debug)]
enum FaultPhase {
    Stage,
    Validate,
    Lexical,
    Graph,
    Publish,
    CasFalse,
    Cleanup,
}

struct FaultRuntime {
    inner: MemoryDocumentIndexRuntime,
    phase: FaultPhase,
    attempted: Arc<Mutex<Vec<ProjectionGenerationKey>>>,
}

impl DocumentIndexRuntime for FaultRuntime {
    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        DocumentIndexRuntime::pin_current(&self.inner, source_id)
    }

    fn begin_generation<'a>(
        &'a self,
        manifest: PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        self.attempted
            .lock()
            .unwrap()
            .push(manifest.manifest().key());
        DocumentIndexRuntime::begin_generation(&self.inner, manifest)
    }

    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        if matches!(self.phase, FaultPhase::Stage | FaultPhase::Cleanup) {
            return Box::pin(async {
                Err(SearchError::OperationFailed(
                    "injected stage failure".into(),
                ))
            });
        }
        DocumentIndexRuntime::stage_concept_registry(&self.inner, key, registry)
    }

    fn stage_resource<'a>(&'a self, resource: PersistableResourceProjection) -> BoxFuture<'a, ()> {
        DocumentIndexRuntime::stage_resource(&self.inner, resource)
    }

    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        if matches!(self.phase, FaultPhase::Validate) {
            return Box::pin(async {
                Err(SearchError::OperationFailed(
                    "injected validation failure".into(),
                ))
            });
        }
        DocumentIndexRuntime::validate_generation(&self.inner, key)
    }

    fn publish_if_current<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        expected_current: Option<ProjectionGenerationKey>,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            if matches!(self.phase, FaultPhase::Publish) {
                return Err(SearchError::OperationFailed(
                    "injected publish failure".into(),
                ));
            }
            if matches!(self.phase, FaultPhase::CasFalse) {
                return Ok(false);
            }
            DocumentIndexRuntime::publish_if_current(&self.inner, key, expected_current).await
        })
    }

    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        DocumentIndexRuntime::fail_generation(&self.inner, key)
    }

    fn discard_projection_generation<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, bool> {
        if matches!(self.phase, FaultPhase::Cleanup) {
            return Box::pin(async {
                Err(SearchError::OperationFailed(
                    "injected projection cleanup failure".into(),
                ))
            });
        }
        DocumentIndexRuntime::discard_projection_generation(&self.inner, key)
    }

    fn build_lexical_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        input: LexicalBuildInput,
    ) -> Result<(), LexicalIndexError> {
        if matches!(self.phase, FaultPhase::Lexical) {
            return Err(LexicalIndexError::AnalyzerMismatch);
        }
        DocumentIndexRuntime::build_lexical_generation(&self.inner, manifest, source, input)
    }

    fn discard_lexical_generation(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<bool, LexicalIndexError> {
        if matches!(self.phase, FaultPhase::Cleanup) {
            return Err(LexicalIndexError::LockPoisoned);
        }
        DocumentIndexRuntime::discard_lexical_generation(&self.inner, key)
    }

    fn build_graph_generation(
        &self,
        manifest: ProjectionGenerationManifest,
        source: &DiscoverableSource,
        projections: Vec<search_core::projection::CompiledResourceProjection>,
        ownership: Vec<(ResourceId, DocumentId)>,
    ) -> Result<(), SearchError> {
        if matches!(self.phase, FaultPhase::Graph) {
            return Err(SearchError::OperationFailed(
                "injected graph failure".into(),
            ));
        }
        DocumentIndexRuntime::build_graph_generation(
            &self.inner,
            manifest,
            source,
            projections,
            ownership,
        )
    }

    fn discard_graph_generation(&self, key: ProjectionGenerationKey) -> Result<bool, SearchError> {
        if matches!(self.phase, FaultPhase::Cleanup) {
            return Err(SearchError::OperationFailed(
                "injected graph cleanup failure".into(),
            ));
        }
        DocumentIndexRuntime::discard_graph_generation(&self.inner, key)
    }
}

#[derive(Default, Clone)]
struct MemoryReceipts(Arc<Mutex<BTreeMap<Uuid, IndexingReceipt>>>);

impl IndexingReceiptStore for MemoryReceipts {
    fn get<'a>(&'a self, event_id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>> {
        Box::pin(async move { Ok(self.0.lock().unwrap().get(&event_id).cloned()) })
    }

    fn put<'a>(&'a self, event_id: Uuid, receipt: IndexingReceipt) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.0.lock().unwrap().insert(event_id, receipt);
            Ok(())
        })
    }
}

#[derive(Clone)]
struct FailOnceReceipts {
    stored: MemoryReceipts,
    fail_next_put: Arc<AtomicBool>,
}

impl FailOnceReceipts {
    fn new() -> Self {
        Self {
            stored: MemoryReceipts::default(),
            fail_next_put: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl IndexingReceiptStore for FailOnceReceipts {
    fn get<'a>(&'a self, event_id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>> {
        self.stored.get(event_id)
    }

    fn put<'a>(&'a self, event_id: Uuid, receipt: IndexingReceipt) -> BoxFuture<'a, ()> {
        if self.fail_next_put.swap(false, Ordering::SeqCst) {
            Box::pin(async {
                Err(SearchError::OperationFailed(
                    "injected receipt persistence failure".into(),
                ))
            })
        } else {
            self.stored.put(event_id, receipt)
        }
    }
}

fn config(analyzer_version: &str) -> DocumentIndexingConfig {
    let source_id = SourceId::from_uuid(Uuid::from_u128(70));
    let mut source = DiscoverableSource::new(
        source_id,
        "document-platform",
        EnumerationSemantics::Complete,
        RetentionMode::PersistentDiscoveryMetadata,
    );
    source.resource_types.push(ResourceKind::Knowledge);
    source.access_model = Some("document-current-check".into());
    DocumentIndexingConfig {
        source,
        lens: DiscoveryLens {
            lens_id: "document-version".into(),
            lens_version: 1,
            resource_type: ResourceKind::Knowledge,
            domain_scope: None,
            source_scope: Some(source_id),
            identity_fields: vec!["document_version_id".into()],
            high_signal_facets: vec!["document_type".into()],
            searchable_fields: vec!["title".into(), "permitted_metadata".into()],
            applicability_fields: vec![],
            temporal_fields: vec![],
            relation_fields: vec![],
            extraction_policy: None,
            projection_policy: None,
        },
        projection_schema_version: "schema-1".into(),
        analyzer_version: analyzer_version.into(),
        semantic_registry: SemanticRegistrySnapshot::new("registry-1"),
    }
}

fn event(id: u128, event_type: &str) -> DocumentSourceEvent {
    DocumentSourceEvent {
        event_id: Uuid::from_u128(id),
        event_type: event_type.into(),
        aggregate_id: Uuid::from_u128(10),
        occurred_at: at(300),
    }
}

fn service(
    reader: FakeReader,
    runtime: MemoryDocumentIndexRuntime,
) -> DocumentIndexingService<DocumentOutboxIndexer<FakeReader, MemoryReceipts>> {
    DocumentIndexingService::new(DocumentOutboxIndexer::new(
        reader,
        config("tantivy-default-0.26.2"),
        runtime,
        MemoryReceipts::default(),
    ))
}

fn request() -> DiscoveryRequest {
    let now = at(400);
    DiscoveryRequest {
        need: DiscoveryNeed {
            need_id: NeedId::from_uuid(Uuid::from_u128(80)),
            intent_signature: IntentSignature::new(IntentFact::new(
                "find".into(),
                IntentFactOrigin::Explicit,
            )),
            required_resource_types: vec![ResourceKind::Knowledge],
            required_claims: vec![],
            authority_requirements: vec![],
            freshness_requirements: vec![],
            constraints: vec![],
            completion_requirement: EvidenceRequirement::new(vec![]),
        },
        temporal_context: TemporalEvaluationContext::new(
            DiscoveryEvaluationId::from_uuid(Uuid::from_u128(81)),
            now,
            now,
            "UTC",
        ),
        access_context: "actor".into(),
    }
}

fn graph_plan() -> GraphTraversalPlan {
    let source = config("tantivy-default-0.26.2").source.source_id;
    GraphTraversalPlan {
        seed_nodes: vec![document_resource_id(
            source,
            DocumentId::from_uuid(Uuid::from_u128(10)),
        )],
        path_patterns: vec![RelationPathPattern::new(
            RelationNamespace::Discovery,
            "document_has_version",
            "document",
            "version",
        )],
        allowed_relation_types: vec!["document_has_version".into()],
        allowed_namespaces: vec![RelationNamespace::Discovery],
        authority_requirement: Some(source.as_uuid().to_string()),
        temporal_context: Some(request().temporal_context),
        access_context: "actor".into(),
        expansion_budget: TraversalBudget {
            max_hops: 1,
            max_relations: 2,
            max_branching_per_node: 2,
            max_seed_nodes: 1,
            max_paths: 2,
        },
        stop_conditions: vec![],
    }
}

#[tokio::test]
async fn reordered_and_duplicate_events_follow_current_snapshot_once() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![
        record(2, "latest title", "snapshot-2", true),
        record(1, "old title", "snapshot-2", false),
    ])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let service = service(reader, runtime.clone());
    let first = service
        .handle(event(102, "DocumentVersionPublished"))
        .await
        .unwrap();
    let key = match first {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        key
    );
    assert_eq!(
        service.handle(event(101, "DocumentCreated")).await.unwrap(),
        IndexingOutcome::Unchanged(key)
    );
    assert_eq!(
        service
            .handle(event(102, "DocumentVersionPublished"))
            .await
            .unwrap(),
        IndexingOutcome::Duplicate(key)
    );
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        key
    );
    assert_eq!(
        service
            .handle(event(103, "document.file.access_granted"))
            .await
            .unwrap(),
        IndexingOutcome::Ignored
    );
}

#[tokio::test]
async fn cloned_runtime_keeps_unchanged_generation_retrievable_across_indexers() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "shared title",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let first = service(reader.clone(), runtime.clone());
    let second = service(reader, runtime.clone());
    let key = match first
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        second
            .handle(event(102, "DocumentVersionPublished"))
            .await
            .unwrap(),
        IndexingOutcome::Unchanged(key)
    );
    let projection_view = runtime.projection_reader();
    let lexical_view = runtime.lexical_reader();
    let generations: &dyn ProjectionGenerationStore = &projection_view;
    let lexical: &dyn LexicalRetrieverPort = &lexical_view;
    assert_eq!(
        generations
            .pin_current(key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        key
    );
    assert_eq!(
        lexical
            .retrieve(key, &request(), &LexicalQuery::new("shared", 10))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn delayed_old_snapshot_cannot_publish_after_newer_indexer_on_shared_store() {
    let runtime = MemoryDocumentIndexRuntime::new();
    let entered = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let slow_reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "old title",
        "snapshot-1",
        true,
    )])));
    let slow = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PausedReader {
            reader: slow_reader.clone(),
            entered: entered.clone(),
            resume: resume.clone(),
            pause_first: AtomicBool::new(true),
        },
        config("tantivy-default-0.26.2"),
        runtime.clone(),
        MemoryReceipts::default(),
    ));
    let slow_run =
        tokio::spawn(async move { slow.handle(event(101, "DocumentVersionPublished")).await });
    entered.notified().await;
    let fast = service(
        FakeReader(Arc::new(Mutex::new(vec![record(
            2,
            "new title",
            "snapshot-2",
            true,
        )]))),
        runtime.clone(),
    );
    let newer_key = match fast
        .handle(event(102, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    slow_reader.set(vec![record(2, "new title", "snapshot-2", true)]);
    resume.notify_one();
    assert_eq!(
        slow_run.await.unwrap().unwrap(),
        IndexingOutcome::Unchanged(newer_key)
    );
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(newer_key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        newer_key
    );
}

#[tokio::test]
async fn newer_indexer_retries_when_old_generation_wins_first_cas() {
    let runtime = MemoryDocumentIndexRuntime::new();
    let entered = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let newer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PausedReader {
            reader: FakeReader(Arc::new(Mutex::new(vec![record(
                2,
                "new title",
                "snapshot-2",
                true,
            )]))),
            entered: entered.clone(),
            resume: resume.clone(),
            pause_first: AtomicBool::new(true),
        },
        config("tantivy-default-0.26.2"),
        runtime.clone(),
        MemoryReceipts::default(),
    ));
    let newer_run =
        tokio::spawn(async move { newer.handle(event(102, "DocumentVersionPublished")).await });
    entered.notified().await;
    let older = service(
        FakeReader(Arc::new(Mutex::new(vec![record(
            1,
            "old title",
            "snapshot-1",
            true,
        )]))),
        runtime.clone(),
    );
    let older_key = match older
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    resume.notify_one();
    let newer_key = match newer_run.await.unwrap().unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_ne!(newer_key, older_key);
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(newer_key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        newer_key
    );
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(newer_key, &request(), &LexicalQuery::new("new", 10))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn mixed_authoritative_snapshots_cannot_publish_an_empty_live_generation() {
    let runtime = MemoryDocumentIndexRuntime::new();
    let source_id = config("tantivy-default-0.26.2").source.source_id;
    let service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        MixedReader(DocumentOutboxSnapshot {
            source_snapshot: "snapshot-1".into(),
            live: vec![],
            historical: vec![record(1, "later historical", "snapshot-2", false)],
        }),
        config("tantivy-default-0.26.2"),
        runtime.clone(),
        MemoryReceipts::default(),
    ));
    assert!(matches!(
        service.handle(event(103, "DocumentVersionPublished")).await,
        Err(SearchError::SourceUnavailable(_))
    ));
    assert!(
        runtime
            .projection_reader()
            .pin_current(source_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn stage_validate_and_publish_failures_leave_no_lexical_generation() {
    for phase in [
        FaultPhase::Stage,
        FaultPhase::Validate,
        FaultPhase::Lexical,
        FaultPhase::Graph,
        FaultPhase::Publish,
        FaultPhase::CasFalse,
    ] {
        let inner = MemoryDocumentIndexRuntime::new();
        let attempted = Arc::new(Mutex::new(Vec::new()));
        let receipts = MemoryReceipts::default();
        let service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
            FakeReader(Arc::new(Mutex::new(vec![record(
                1,
                "current title",
                "snapshot-1",
                true,
            )]))),
            config("tantivy-default-0.26.2"),
            FaultRuntime {
                inner: inner.clone(),
                phase,
                attempted: attempted.clone(),
            },
            receipts.clone(),
        ));
        assert!(
            matches!(
                service.handle(event(101, "DocumentVersionPublished")).await,
                Err(SearchError::OperationFailed(_))
            ),
            "{phase:?}"
        );
        let attempted = attempted.lock().unwrap().clone();
        assert!(!attempted.is_empty());
        for key in attempted {
            assert!(
                inner
                    .lexical_reader()
                    .retrieve(key, &request(), &LexicalQuery::new("current", 10))
                    .await
                    .is_err(),
                "{phase:?} left an unpublished lexical segment"
            );
            assert!(
                inner
                    .graph_reader()
                    .retrieve(key, &graph_plan())
                    .await
                    .is_err(),
                "{phase:?} left an unpublished graph segment"
            );
            assert!(
                !DocumentIndexRuntime::discard_projection_generation(&inner, key)
                    .await
                    .unwrap(),
                "{phase:?} left an unpublished projection segment"
            );
        }
        assert!(
            inner
                .projection_reader()
                .pin_current(config("tantivy-default-0.26.2").source.source_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(receipts.get(Uuid::from_u128(101)).await.unwrap().is_none());
    }
}

#[tokio::test]
async fn cleanup_attempts_all_segments_and_reports_original_and_rollback_errors() {
    let inner = MemoryDocumentIndexRuntime::new();
    let service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        FakeReader(Arc::new(Mutex::new(vec![record(
            1,
            "current title",
            "snapshot-1",
            true,
        )]))),
        config("tantivy-default-0.26.2"),
        FaultRuntime {
            inner,
            phase: FaultPhase::Cleanup,
            attempted: Arc::new(Mutex::new(Vec::new())),
        },
        MemoryReceipts::default(),
    ));
    let error = service
        .handle(event(102, "DocumentVersionPublished"))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("injected stage failure"), "{error}");
    assert!(error.contains("graph cleanup"), "{error}");
    assert!(error.contains("lexical cleanup"), "{error}");
    assert!(error.contains("projection cleanup"), "{error}");
}

#[tokio::test]
async fn graph_build_failure_preserves_old_pointer_lexical_and_graph() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "old title",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let old_key = match service(reader.clone(), runtime.clone())
        .handle(event(501, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    reader.set(vec![record(1, "new title", "snapshot-2", true)]);
    let attempted = Arc::new(Mutex::new(Vec::new()));
    let failing = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        reader,
        config("tantivy-default-0.26.2"),
        FaultRuntime {
            inner: runtime.clone(),
            phase: FaultPhase::Graph,
            attempted: attempted.clone(),
        },
        MemoryReceipts::default(),
    ));
    assert!(matches!(
        failing.handle(event(502, "DocumentVersionPublished")).await,
        Err(SearchError::OperationFailed(_))
    ));
    let unpublished = *attempted.lock().unwrap().last().unwrap();
    assert_ne!(unpublished, old_key);
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(old_key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        old_key,
    );
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(old_key, &request(), &LexicalQuery::new("old", 10))
            .await
            .unwrap()
            .len(),
        1,
    );
    assert!(
        runtime
            .graph_reader()
            .retrieve(old_key, &graph_plan())
            .await
            .is_ok()
    );
    assert!(
        runtime
            .graph_reader()
            .retrieve(unpublished, &graph_plan())
            .await
            .is_err()
    );
    assert!(
        runtime
            .lexical_reader()
            .retrieve(unpublished, &request(), &LexicalQuery::new("new", 10))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn receipt_failure_after_publish_preserves_generation_and_retry_records_receipt() {
    let fixture = versioning_support::fixture().await;
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap();
    let actor = VerifiedActorContext::from_trusted_adapter(
        versioning_support::actor(),
        vec![subject.clone()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        fixture.pool.clone(),
        versioning_support::actor(),
    ));
    repository
        .initialize_root_policy(
            &actor,
            vec![PolicyGrant::new(subject, [Action::Read]).unwrap()],
        )
        .await
        .unwrap();
    let source_id = config("tantivy-default-0.26.2").source.source_id;
    let access = Arc::new(DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(repository),
        actor,
        "trusted-session".into(),
    ));
    let runtime = MemoryDocumentIndexRuntime::with_current_access(access);
    let receipts = FailOnceReceipts::new();
    let service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        config("tantivy-default-0.26.2"),
        runtime.clone(),
        receipts.clone(),
    ));
    let event = event(405, "DocumentVersionPublished");
    let error = service.handle(event.clone()).await.unwrap_err();
    assert!(matches!(
        error,
        SearchError::OperationFailed(message) if message == "injected receipt persistence failure"
    ));
    assert!(receipts.get(event.event_id).await.unwrap().is_none());

    let published = runtime
        .projection_reader()
        .pin_current(source_id)
        .await
        .unwrap()
        .expect("publish must remain current after receipt failure");
    let key = published.key();
    let document = document_resource_id(source_id, fixture.document_id);
    let snapshot = PostgresDocumentSnapshotReader::new(fixture.pool.clone())
        .enumerate_outbox_snapshot()
        .await
        .unwrap();
    let folder = folder_resource_id(
        source_id,
        fixture.document_id,
        snapshot.live[0].snapshot.folder_id,
    );
    for resource in [document, folder] {
        assert!(
            runtime
                .projection_reader()
                .resource_at(key, resource)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            runtime
                .graph_access_reader()
                .evaluate(resource, "trusted-session")
                .await
                .unwrap(),
            AccessDecision::Allowed
        );
    }
    let lexical = runtime
        .lexical_reader()
        .retrieve(key, &request(), &LexicalQuery::new("Base", 10))
        .await
        .unwrap();
    assert_eq!(lexical.len(), 1);
    let mut plan = graph_plan();
    plan.seed_nodes = vec![document];
    plan.access_context = "trusted-session".into();
    let graph = runtime.graph_reader().retrieve(key, &plan).await.unwrap();
    assert!(graph.hits.iter().any(|hit| {
        hit.candidate.resource_ref == Some(ResourceId::from_uuid(fixture.base_id.as_uuid()))
    }));

    assert_eq!(
        service.handle(event.clone()).await.unwrap(),
        IndexingOutcome::Unchanged(key),
        "retry should save the receipt against the published generation"
    );
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        key
    );
    assert_eq!(
        receipts.get(event.event_id).await.unwrap(),
        Some(IndexingReceipt {
            digest: published.digest,
            generation: key,
        })
    );
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(key, &request(), &LexicalQuery::new("Base", 10))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        runtime.graph_reader().retrieve(key, &plan).await.unwrap(),
        graph
    );
    assert_eq!(
        runtime
            .graph_access_reader()
            .evaluate(folder, "trusted-session")
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
}

#[tokio::test]
async fn cas_failure_does_not_rebind_published_auxiliary_to_an_unpublished_version() {
    let fixture = versioning_support::fixture().await;
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap();
    let actor = VerifiedActorContext::from_trusted_adapter(
        versioning_support::actor(),
        vec![subject.clone()],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap();
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        fixture.pool.clone(),
        versioning_support::actor(),
    ));
    repository
        .initialize_root_policy(
            &actor,
            vec![PolicyGrant::new(subject, [Action::Read]).unwrap()],
        )
        .await
        .unwrap();
    let source_id = config("tantivy-default-0.26.2").source.source_id;
    let access = Arc::new(DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(repository),
        actor,
        "trusted-session".into(),
    ));
    let runtime = MemoryDocumentIndexRuntime::with_current_access(access);
    let first = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        config("tantivy-default-0.26.2"),
        runtime.clone(),
        MemoryReceipts::default(),
    ));
    let old_key = match first
        .handle(event(401, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let auxiliary = document_resource_id(source_id, fixture.document_id);
    assert_eq!(
        runtime
            .graph_access_reader()
            .evaluate(auxiliary, "trusted-session")
            .await
            .unwrap(),
        AccessDecision::Allowed
    );
    let real_snapshot = PostgresDocumentSnapshotReader::new(fixture.pool.clone())
        .enumerate_outbox_snapshot()
        .await
        .unwrap();
    let old_folder = folder_resource_id(
        source_id,
        fixture.document_id,
        real_snapshot.live[0].snapshot.folder_id,
    );
    let mut unpublished = real_snapshot.live[0].clone();
    let unpublished_folder = FolderId::from_uuid(Uuid::now_v7());
    unpublished.snapshot.folder_id = unpublished_folder;
    let unpublished_folder_resource =
        folder_resource_id(source_id, fixture.document_id, unpublished_folder);
    let phantom = DocumentVersionId::from_uuid(Uuid::now_v7());
    unpublished.snapshot.document_version_id = phantom;
    unpublished.snapshot.current_version_id = Some(phantom);
    unpublished.snapshot.title = Title::new("Unpublished phantom").unwrap();
    unpublished.snapshot.source_snapshot = "unpublished-snapshot".into();
    for (number, phase) in [FaultPhase::Publish, FaultPhase::CasFalse]
        .into_iter()
        .enumerate()
    {
        let attempted = Arc::new(Mutex::new(Vec::new()));
        let losing = DocumentIndexingService::new(DocumentOutboxIndexer::new(
            FakeReader(Arc::new(Mutex::new(vec![unpublished.clone()]))),
            config("tantivy-default-0.26.2"),
            FaultRuntime {
                inner: runtime.clone(),
                phase,
                attempted: attempted.clone(),
            },
            MemoryReceipts::default(),
        ));
        assert!(matches!(
            losing
                .handle(event(402 + number as u128, "DocumentVersionPublished"))
                .await,
            Err(SearchError::OperationFailed(_))
        ));
        let attempted = attempted.lock().unwrap().clone();
        assert_eq!(
            attempted.len(),
            if matches!(phase, FaultPhase::CasFalse) {
                3
            } else {
                1
            }
        );
        assert_eq!(
            runtime
                .projection_reader()
                .pin_current(source_id)
                .await
                .unwrap()
                .unwrap()
                .key(),
            old_key
        );
        assert_eq!(
            runtime
                .graph_access_reader()
                .evaluate(auxiliary, "trusted-session")
                .await
                .unwrap(),
            AccessDecision::Allowed,
            "an unpublished Version must not replace the Document ownership check"
        );
        assert_eq!(
            runtime
                .graph_access_reader()
                .evaluate(old_folder, "trusted-session")
                .await
                .unwrap(),
            AccessDecision::Allowed,
        );
        assert_eq!(
            runtime
                .graph_access_reader()
                .evaluate(unpublished_folder_resource, "trusted-session")
                .await
                .unwrap(),
            AccessDecision::Denied,
            "a discarded graph generation must release its unique FolderPlacement owner"
        );
        assert_eq!(
            runtime
                .lexical_reader()
                .retrieve(old_key, &request(), &LexicalQuery::new("Base", 10))
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            runtime
                .graph_reader()
                .retrieve(old_key, &graph_plan())
                .await
                .is_ok()
        );
        for unpublished_key in attempted {
            assert_ne!(unpublished_key, old_key);
            assert!(
                runtime
                    .graph_reader()
                    .retrieve(unpublished_key, &graph_plan())
                    .await
                    .is_err()
            );
            assert!(
                runtime
                    .lexical_reader()
                    .retrieve(
                        unpublished_key,
                        &request(),
                        &LexicalQuery::new("Unpublished", 10),
                    )
                    .await
                    .is_err()
            );
            assert!(
                !DocumentIndexRuntime::discard_projection_generation(&runtime, unpublished_key)
                    .await
                    .unwrap(),
                "the CAS loser must not retain a projection generation"
            );
        }
    }
}

#[tokio::test]
async fn graph_build_rejects_missing_duplicate_and_non_auxiliary_owners() {
    let record = record(1, "current title", "snapshot-1", true);
    let document = record.snapshot.document_id;
    let source = config("tantivy-default-0.26.2").source;
    let version = ResourceId::from_uuid(record.snapshot.document_version_id.as_uuid());
    let document_node = document_resource_id(source.source_id, document);
    let folder_node = folder_resource_id(source.source_id, document, record.snapshot.folder_id);
    let runtime = MemoryDocumentIndexRuntime::new();
    let old_key = match service(
        FakeReader(Arc::new(Mutex::new(vec![record]))),
        runtime.clone(),
    )
    .rebuild()
    .await
    .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let old_manifest = runtime
        .projection_reader()
        .pin_current(source.source_id)
        .await
        .unwrap()
        .unwrap();
    let mut projections = Vec::new();
    for id in [version, document_node, folder_node] {
        projections.push(
            runtime
                .projection_reader()
                .resource_at(old_key, id)
                .await
                .unwrap()
                .unwrap(),
        );
    }
    for (number, owners) in [
        vec![],
        vec![(document_node, document)],
        vec![
            (document_node, document),
            (folder_node, document),
            (version, document),
        ],
        vec![
            (document_node, document),
            (folder_node, document),
            (folder_node, document),
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let mut manifest = old_manifest.clone();
        manifest.generation_id =
            ProjectionGenerationId::from_uuid(Uuid::from_u128(500 + number as u128));
        let mut rebased = projections.clone();
        for projection in &mut rebased {
            projection.manifest = manifest.clone();
        }
        assert!(
            DocumentIndexRuntime::build_graph_generation(
                &runtime,
                manifest.clone(),
                &source,
                rebased,
                owners,
            )
            .is_err(),
            "invalid owner set {number} was accepted"
        );
        assert!(
            runtime
                .graph_reader()
                .retrieve(manifest.key(), &graph_plan())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn document_folder_and_access_policy_domain_events_all_trigger_reconciliation() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "current",
        "snapshot-1",
        true,
    )])));
    let service = service(reader, MemoryDocumentIndexRuntime::new());
    let first = service.handle(event(100, "DocumentCreated")).await.unwrap();
    let key = match first {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    for (offset, event_type) in [
        "DocumentVersionCreated",
        "DocumentVersionUpdated",
        "DocumentVersionRebased",
        "DocumentVersionPublished",
        "DocumentVersionWithdrawn",
        "DocumentVersionPublicationScheduled",
        "DocumentVersionPublicationCancelled",
        "DocumentVersionPublicationTerminal",
        "DocumentPublicationEnded",
        "DocumentMetadataChanged",
        "DocumentMoved",
        "FolderCreated",
        "FolderRenamed",
        "FolderMoved",
        "AccessPolicyChanged",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            service
                .handle(event(101 + offset as u128, event_type))
                .await
                .unwrap(),
            IndexingOutcome::Unchanged(key),
            "{event_type} was ignored"
        );
    }
}

#[tokio::test]
async fn unknown_document_domain_events_fail_instead_of_being_acknowledged_as_ignored() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "current",
        "snapshot-1",
        true,
    )])));
    let service = service(reader, MemoryDocumentIndexRuntime::new());
    for event_type in ["DocumentVersionPublised", "DocumentRetentionChanged"] {
        assert!(matches!(
            service.handle(event(120, event_type)).await,
            Err(SearchError::InvalidRequest(_))
        ));
    }
    assert_eq!(
        service
            .handle(event(121, "OtherSystemHeartbeat"))
            .await
            .unwrap(),
        IndexingOutcome::Ignored
    );
}

#[tokio::test]
async fn publication_end_removes_current_version_from_live_generation() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "current",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let service = service(reader.clone(), runtime.clone());
    service
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap();
    let mut ended = record(1, "current", "snapshot-2", false);
    ended.snapshot.current_version_id = None;
    ended.snapshot.publication_end = Some(PublicationEndRecord {
        operation_id: Uuid::from_u128(201),
        ended_at: at(300),
    });
    reader.set(vec![ended]);
    let key = match service
        .handle(event(102, "DocumentPublicationEnded"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert!(
        runtime
            .projection_reader()
            .resource_at(key, ResourceId::from_uuid(version(1).as_uuid()))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn access_policy_change_refreshes_derived_scope_without_using_an_event_payload() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "current",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let service = service(reader.clone(), runtime.clone());
    let first = service
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap();
    let old_key = match first {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let mut changed = record(1, "current", "snapshot-2", true);
    changed.snapshot.access.access_scope = Some("new-revision".into());
    changed.access_revision = 2;
    reader.set(vec![changed]);
    let new_key = match service
        .handle(event(102, "AccessPolicyChanged"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_ne!(new_key, old_key);
    assert_eq!(
        runtime
            .projection_reader()
            .resource_at(new_key, ResourceId::from_uuid(version(1).as_uuid()))
            .await
            .unwrap()
            .unwrap()
            .access
            .access_scope
            .as_deref(),
        Some("new-revision")
    );
}

#[tokio::test]
async fn lens_revision_rebuilds_even_when_document_projection_digest_is_unchanged() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "current",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let first_service = service(reader.clone(), runtime.clone());
    let old_key = match first_service
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    let mut next_config = config("tantivy-default-0.26.2");
    next_config.lens.lens_version = 2;
    let updated_service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        reader,
        next_config,
        runtime.clone(),
        MemoryReceipts::default(),
    ));
    let new_key = match updated_service
        .handle(event(102, "DocumentMetadataChanged"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_ne!(new_key, old_key);
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(new_key.source_id)
            .await
            .unwrap()
            .unwrap()
            .lens_version,
        2
    );
}

#[tokio::test]
async fn full_rebuild_recovers_a_lost_event_and_replaces_old_lexical_generation() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "old title",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let service = service(reader.clone(), runtime.clone());
    let first = service
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap();
    let old_key = match first {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    reader.set(vec![
        record(2, "new title", "snapshot-2", true),
        record(1, "old title", "snapshot-2", false),
    ]);
    let rebuilt = service.rebuild().await.unwrap();
    let new_key = match rebuilt {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_ne!(new_key, old_key);
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(new_key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        new_key
    );
    assert_eq!(
        runtime
            .projection_reader()
            .resource_at(new_key, ResourceId::from_uuid(version(2).as_uuid()))
            .await
            .unwrap()
            .unwrap()
            .directory
            .title
            .as_deref(),
        Some("new title")
    );
    assert!(
        runtime
            .projection_reader()
            .resource_at(new_key, ResourceId::from_uuid(version(1).as_uuid()))
            .await
            .unwrap()
            .is_none(),
        "an old Version must not remain in the Live generation"
    );
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(new_key, &request(), &LexicalQuery::new("new", 10))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(new_key, &request(), &LexicalQuery::new("old", 10))
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(old_key, &request(), &LexicalQuery::new("old", 10))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn full_rebuild_restores_a_lost_lexical_index_even_when_projection_digest_matches() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "authoritative title",
        "snapshot-1",
        true,
    )])));
    let runtime = MemoryDocumentIndexRuntime::new();
    let first_service = service(reader.clone(), runtime.clone());
    let old_key = match first_service
        .handle(event(101, "DocumentVersionPublished"))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert!(runtime.discard_lexical_generation(old_key).unwrap());
    let restore_service = service(reader, runtime.clone());
    let new_key = match restore_service.rebuild().await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("{other:?}"),
    };
    assert_ne!(new_key, old_key);
    assert_eq!(
        runtime
            .lexical_reader()
            .retrieve(new_key, &request(), &LexicalQuery::new("authoritative", 10))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        runtime
            .projection_reader()
            .pin_current(new_key.source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        new_key
    );
}

#[tokio::test]
async fn search_index_failure_never_records_a_receipt_or_changes_document_state() {
    let reader = FakeReader(Arc::new(Mutex::new(vec![record(
        1,
        "authoritative",
        "snapshot-1",
        true,
    )])));
    let authoritative_before = reader.0.lock().unwrap().clone();
    let receipts = MemoryReceipts::default();
    let runtime = MemoryDocumentIndexRuntime::new();
    let source_id = config("wrong-analyzer").source.source_id;
    let service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        reader.clone(),
        config("wrong-analyzer"),
        runtime.clone(),
        receipts.clone(),
    ));
    assert!(matches!(
        service.handle(event(101, "DocumentVersionPublished")).await,
        Err(SearchError::OperationFailed(_))
    ));
    assert_eq!(*reader.0.lock().unwrap(), authoritative_before);
    assert!(
        runtime
            .projection_reader()
            .pin_current(source_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(receipts.get(Uuid::from_u128(101)).await.unwrap(), None);
}

#[tokio::test]
async fn failed_indexing_leaves_committed_document_and_generic_outbox_untouched() {
    let container = GenericImage::new("postgres", "18.6-bookworm")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "search_outbox_test")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432.tcp()).await.unwrap();
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/search_outbox_test"
        ))
        .await
        .unwrap();
    migrate(&pool).await.unwrap();
    let document_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let event_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO documents (document_id,folder_id,current_version_id,revision,metadata,created_at) \
         VALUES ($1,$2,NULL,1,'{}'::jsonb,now())",
    )
    .bind(document_id)
    .bind(SYSTEM_ROOT_FOLDER_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO document_versions (document_version_id,document_id,version_no,lifecycle_state, \
         title,published_at,created_by_identity_provider,created_by_principal_id,metadata,created_at) \
         VALUES ($1,$2,1,'PUBLISHED','Authoritative title',now(),'test','actor','{}'::jsonb,now())",
    )
    .bind(version_id)
    .bind(document_id)
    .execute(&pool)
    .await
    .unwrap();
    let reader = PostgresDocumentSnapshotReader::new(pool.clone());
    let before = reader.enumerate_outbox_snapshot().await.unwrap();
    assert!(before.live.is_empty());
    assert_eq!(before.historical.len(), 1);
    assert_eq!(
        before.historical[0].snapshot.source_snapshot,
        before.source_snapshot
    );
    let mut old_read = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *old_read)
        .await
        .unwrap();
    let old_current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id=$1")
            .bind(document_id)
            .fetch_one(&mut *old_read)
            .await
            .unwrap();
    assert_eq!(old_current, None);
    sqlx::query("UPDATE documents SET current_version_id=$1 WHERE document_id=$2")
        .bind(version_id)
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
    let still_old: Option<Uuid> =
        sqlx::query_scalar("SELECT current_version_id FROM documents WHERE document_id=$1")
            .bind(document_id)
            .fetch_one(&mut *old_read)
            .await
            .unwrap();
    assert_eq!(still_old, None, "one read transaction must stay at S1");
    old_read.rollback().await.unwrap();
    let after = reader.enumerate_outbox_snapshot().await.unwrap();
    assert_eq!(after.live.len(), 1);
    assert!(after.historical.is_empty());
    assert_eq!(
        after.live[0].snapshot.source_snapshot,
        after.source_snapshot
    );
    assert_ne!(before.source_snapshot, after.source_snapshot);
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload,occurred_at,available_at) \
         VALUES ($1,'DocumentVersionPublished','Document',$2,'{}'::jsonb,now(),now())",
    )
    .bind(event_id)
    .bind(document_id)
    .execute(&pool)
    .await
    .unwrap();
    let receipts = MemoryReceipts::default();
    let runtime = MemoryDocumentIndexRuntime::new();
    let service = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(pool.clone()),
        config("wrong-analyzer"),
        runtime.clone(),
        receipts.clone(),
    ));
    let result = service
        .handle(DocumentSourceEvent {
            event_id,
            event_type: "DocumentVersionPublished".into(),
            aggregate_id: document_id,
            occurred_at: at(300),
        })
        .await;
    assert!(matches!(result, Err(SearchError::OperationFailed(_))));
    let document: (Option<Uuid>, i64) =
        sqlx::query_as("SELECT current_version_id,revision FROM documents WHERE document_id=$1")
            .bind(document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let outbox: (i32, Option<OffsetDateTime>) =
        sqlx::query_as("SELECT attempt_count,delivered_at FROM outbox_events WHERE event_id=$1")
            .bind(event_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(document, (Some(version_id), 1));
    assert_eq!(outbox, (0, None));
    assert!(receipts.get(event_id).await.unwrap().is_none());
    assert!(
        runtime
            .projection_reader()
            .pin_current(config("wrong-analyzer").source.source_id)
            .await
            .unwrap()
            .is_none()
    );
}
