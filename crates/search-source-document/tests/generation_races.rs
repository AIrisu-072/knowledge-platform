#[path = "support/document_discovery.rs"]
mod discovery_support;
#[path = "../../document-repository-postgres/tests/support/versioning.rs"]
mod versioning_support;

use std::collections::BTreeSet;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use document_application::{
    BootstrapRootPolicy, CreateDocumentCommand, CreateVersionCommand, DocumentAccessCheckService,
    DocumentPublicationEndService, DocumentService, EndDocumentPublicationCommand, InvocationKind,
    PublicationEndOperationId, PublishDocumentCommand, PublishOperationId, VerifiedActorContext,
    WithdrawVersionCommand,
};
use document_domain::{
    Action, DocumentVersionId, LifecycleState, MediaType, Metadata, PolicyGrant, PolicySubject,
    PolicySubjectKind,
};
use document_repository_postgres::{PostgresDocumentRepository, SYSTEM_ROOT_FOLDER_ID};
use search_application::SearchError;
use search_application::discovery_service::{DiscoveryPorts, DiscoveryService};
use search_application::indexing_service::{DocumentIndexingService, IndexingOutcome};
use search_application::ports::{
    BoxFuture, ConceptRegistryPort, CurrentCandidateAccessEvaluatorPort, LexicalQuery,
    LexicalRetrieverPort, ProjectionGenerationStore, SemanticRegistrySnapshot,
};
use search_application::projection::{
    PersistableGenerationManifest, PersistableResourceProjection,
};
use search_application::retrieval_execution::RetrievalExecutionPorts;
use search_application::source_registry::InMemorySourceRegistry;
use search_core::discovery::{DiscoveryRequest, FederatedCandidate};
use search_core::id::{ResourceId, SourceId};
use search_core::predicate::{ConceptResolver, TruthValue};
use search_core::projection::{
    CompiledResourceProjection, ProjectionGenerationKey, ProjectionGenerationManifest,
};
use search_source_document::{
    DocumentCurrentAccessAdapter, DocumentLexicalReader, DocumentOutboxIndexer,
    DocumentOutboxReader, DocumentOutboxSnapshot, DocumentProjectionReader,
    MemoryDocumentIndexRuntime, PostgresDocumentSnapshotReader,
};
use time::{Duration, OffsetDateTime};
use tokio::sync::Notify;
use uuid::Uuid;

struct PauseBeforeAuthoritativeRead {
    reader: PostgresDocumentSnapshotReader,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    pause_once: AtomicBool,
    observed: Arc<Mutex<Option<DocumentOutboxSnapshot>>>,
}

impl DocumentOutboxReader for PauseBeforeAuthoritativeRead {
    fn enumerate_snapshot<'a>(&'a self) -> BoxFuture<'a, DocumentOutboxSnapshot> {
        Box::pin(async move {
            if self.pause_once.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            let snapshot = self
                .reader
                .enumerate_outbox_snapshot()
                .await
                .map_err(|error| SearchError::SourceUnavailable(error.to_string()))?;
            *self.observed.lock().unwrap() = Some(snapshot.clone());
            Ok(snapshot)
        })
    }
}

struct PauseBeforeLexicalRead {
    inner: DocumentLexicalReader,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    pause_once: AtomicBool,
    keys: Arc<Mutex<Vec<ProjectionGenerationKey>>>,
}

impl LexicalRetrieverPort for PauseBeforeLexicalRead {
    fn retrieve<'a>(
        &'a self,
        generation: ProjectionGenerationKey,
        request: &'a DiscoveryRequest,
        query: &'a LexicalQuery,
    ) -> BoxFuture<'a, Vec<FederatedCandidate>> {
        Box::pin(async move {
            if self.pause_once.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            self.keys.lock().unwrap().push(generation);
            self.inner.retrieve(generation, request, query).await
        })
    }
}

#[derive(Clone)]
struct TracedProjection {
    inner: DocumentProjectionReader,
    keys: Arc<Mutex<Vec<(&'static str, ProjectionGenerationKey)>>>,
}

impl ProjectionGenerationStore for TracedProjection {
    fn begin_generation<'a>(
        &'a self,
        manifest: PersistableGenerationManifest,
    ) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::begin_generation(&self.inner, manifest)
    }

    fn begin_incremental_generation<'a>(
        &'a self,
        manifest: PersistableGenerationManifest,
        base: ProjectionGenerationKey,
        retired: BTreeSet<ResourceId>,
    ) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::begin_incremental_generation(
            &self.inner,
            manifest,
            base,
            retired,
        )
    }

    fn stage_resource<'a>(
        &'a self,
        projection: PersistableResourceProjection,
    ) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::stage_resource(&self.inner, projection)
    }

    fn stage_concept_registry<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        registry: SemanticRegistrySnapshot,
    ) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::stage_concept_registry(&self.inner, key, registry)
    }

    fn validate_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::validate_generation(&self.inner, key)
    }

    fn publish_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::publish_generation(&self.inner, key)
    }

    fn fail_generation<'a>(&'a self, key: ProjectionGenerationKey) -> BoxFuture<'a, ()> {
        ProjectionGenerationStore::fail_generation(&self.inner, key)
    }

    fn pin_current<'a>(
        &'a self,
        source_id: SourceId,
    ) -> BoxFuture<'a, Option<ProjectionGenerationManifest>> {
        Box::pin(async move {
            let manifest = ProjectionGenerationStore::pin_current(&self.inner, source_id).await?;
            if let Some(ref manifest) = manifest {
                self.keys.lock().unwrap().push(("pin", manifest.key()));
            }
            Ok(manifest)
        })
    }

    fn resource_at<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        resource_id: ResourceId,
    ) -> BoxFuture<'a, Option<CompiledResourceProjection>> {
        self.keys.lock().unwrap().push(("detail", key));
        ProjectionGenerationStore::resource_at(&self.inner, key, resource_id)
    }
}

impl ConceptRegistryPort for TracedProjection {
    fn pin_view<'a>(
        &'a self,
        key: ProjectionGenerationKey,
    ) -> BoxFuture<'a, Arc<dyn ConceptResolver + Send + Sync>> {
        self.keys.lock().unwrap().push(("concept", key));
        ConceptRegistryPort::pin_view(&self.inner, key)
    }

    fn same_concept<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        left: &'a str,
        right: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        self.keys.lock().unwrap().push(("same_concept", key));
        ConceptRegistryPort::same_concept(&self.inner, key, left, right)
    }

    fn is_a<'a>(
        &'a self,
        key: ProjectionGenerationKey,
        child: &'a str,
        parent: &'a str,
    ) -> BoxFuture<'a, TruthValue> {
        self.keys.lock().unwrap().push(("is_a", key));
        ConceptRegistryPort::is_a(&self.inner, key, child, parent)
    }
}

fn actor() -> VerifiedActorContext {
    let principal = versioning_support::actor();
    let subject = PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap();
    VerifiedActorContext::from_trusted_adapter(
        principal,
        vec![subject],
        OffsetDateTime::now_utc() + Duration::hours(1),
        InvocationKind::HumanInteractive,
        None,
    )
    .unwrap()
}

async fn allow_read(fixture: &versioning_support::Fixture) -> Arc<PostgresDocumentRepository> {
    let repository = Arc::new(PostgresDocumentRepository::new_with_bootstrap_actor(
        fixture.pool.clone(),
        versioning_support::actor(),
    ));
    repository
        .initialize_root_policy(
            &actor(),
            vec![
                PolicyGrant::new(
                    PolicySubject::new(PolicySubjectKind::Principal, "test-idp", "editor").unwrap(),
                    [Action::Read],
                )
                .unwrap(),
            ],
        )
        .await
        .unwrap();
    repository
}

#[derive(Clone, Copy)]
enum Transition {
    PublishReplacement,
    Withdraw,
    PublicationEnd,
}

async fn apply_transition(
    fixture: &versioning_support::Fixture,
    transition: Transition,
) -> Option<DocumentVersionId> {
    match transition {
        Transition::PublishReplacement => {
            let replacement = DocumentVersionId::from_uuid(Uuid::now_v7());
            let service = fixture.service();
            service
                .create_version(
                    CreateVersionCommand::new(
                        versioning_support::operation_id(50),
                        fixture.document_id,
                        replacement,
                        1,
                        versioning_support::actor(),
                    )
                    .unwrap(),
                    fixture.prepare("Base Replacement", 2).await,
                )
                .await
                .unwrap();
            service
                .publish_document(
                    PublishDocumentCommand::new(
                        PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                        fixture.document_id,
                        replacement,
                        2,
                        versioning_support::actor(),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            Some(replacement)
        }
        Transition::Withdraw => {
            fixture
                .service()
                .withdraw_version(
                    WithdrawVersionCommand::new(
                        versioning_support::operation_id(51),
                        fixture.document_id,
                        fixture.base_id,
                        1,
                        versioning_support::actor(),
                        "withdraw current version",
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            None
        }
        Transition::PublicationEnd => {
            DocumentPublicationEndService::new(
                Arc::new(versioning_support::TestIds),
                fixture.clock.clone(),
                fixture.repository.clone(),
            )
            .end_document_publication(
                EndDocumentPublicationCommand::new(
                    PublicationEndOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                    fixture.document_id,
                    1,
                    fixture.base_id,
                    versioning_support::actor(),
                    "publication period ended".into(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
            None
        }
    }
}

#[tokio::test]
async fn discovery_keeps_one_runtime_generation_when_new_document_publishes_mid_evaluation() {
    let fixture = versioning_support::fixture().await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let access_repository = allow_read(&fixture).await;
    let runtime = MemoryDocumentIndexRuntime::new();
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        discovery_support::Receipts::default(),
    ));
    let n = match indexer.rebuild().await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected first generation: {other:?}"),
    };
    let first_resource = ResourceId::from_uuid(fixture.base_id.as_uuid());
    let entered = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let lexical_keys = Arc::new(Mutex::new(Vec::new()));
    let projection_keys = Arc::new(Mutex::new(Vec::new()));
    let paused_lexical = PauseBeforeLexicalRead {
        inner: runtime.lexical_reader(),
        entered: entered.clone(),
        resume: resume.clone(),
        pause_once: AtomicBool::new(true),
        keys: lexical_keys.clone(),
    };
    let traced = TracedProjection {
        inner: runtime.projection_reader(),
        keys: projection_keys.clone(),
    };
    let first_access = DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(access_repository.clone()),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let first = tokio::spawn(async move {
        let mut sources = InMemorySourceRegistry::default();
        sources.insert(discovery_support::source(source_id));
        let no_evidence = discovery_support::NoEvidence;
        let service = DiscoveryService::new(
            discovery_support::discovery_config(source_id, "Base"),
            DiscoveryPorts {
                sources: &sources,
                generations: &traced,
                concepts: &traced,
                retrieval: RetrievalExecutionPorts {
                    directory: None,
                    structured: None,
                    lexical: Some(&paused_lexical),
                    hypergraph: None,
                    graph_resource_access: None,
                    access: &first_access,
                },
                selectors: &no_evidence,
                assertions: &no_evidence,
                evidence: &no_evidence,
                probe: None,
                probe_catalog: None,
                source_policy: None,
            },
        )
        .unwrap();
        service
            .discover(discovery_support::request())
            .await
            .unwrap()
    });
    entered.notified().await;
    assert!(
        projection_keys
            .lock()
            .unwrap()
            .iter()
            .any(|(kind, key)| *kind == "pin" && *key == n)
    );
    assert!(
        projection_keys
            .lock()
            .unwrap()
            .iter()
            .any(|(kind, key)| *kind == "concept" && *key == n)
    );
    assert!(lexical_keys.lock().unwrap().is_empty());

    let document_service = DocumentService::new(
        Arc::new(versioning_support::TestIds),
        fixture.clock.clone(),
        fixture.storage.clone(),
        fixture.repository.clone(),
    );
    let added = document_service
        .create_document(CreateDocumentCommand {
            folder_id: document_domain::FolderId::from_uuid(SYSTEM_ROOT_FOLDER_ID),
            title: "Base Additional".into(),
            document_metadata: Metadata::default(),
            version_metadata: Metadata::default(),
            principal: versioning_support::actor(),
            original_filename: "additional.txt".into(),
            media_type: MediaType::new("text/plain").unwrap(),
            content: Box::pin(Cursor::new(vec![7_u8; 3])),
        })
        .await
        .unwrap();
    document_service
        .publish_document(
            PublishDocumentCommand::new(
                PublishOperationId::try_from_uuid(Uuid::now_v7()).unwrap(),
                added.document_id(),
                added.document_version_id(),
                0,
                versioning_support::actor(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let second_resource = ResourceId::from_uuid(added.document_version_id().as_uuid());
    let n_plus_one = match indexer
        .handle(discovery_support::event(
            "DocumentVersionPublished",
            added.document_id().as_uuid(),
        ))
        .await
        .unwrap()
    {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected replacement generation: {other:?}"),
    };
    assert_ne!(n, n_plus_one);
    resume.notify_one();
    let first_result = first.await.unwrap();
    assert_eq!(*lexical_keys.lock().unwrap(), vec![n]);
    let observed = projection_keys.lock().unwrap().clone();
    assert!(observed.iter().any(|(kind, _)| *kind == "detail"));
    assert!(observed.iter().all(|(_, key)| *key == n), "{observed:?}");
    assert_eq!(
        first_result.qualified_resources.len(),
        1,
        "{first_result:?}"
    );
    assert_eq!(
        first_result.qualified_resources[0].resource_ref,
        first_resource
    );
    assert!(
        !serde_json::to_string(&first_result)
            .unwrap()
            .contains(&second_resource.as_uuid().to_string())
    );

    let projection = runtime.projection_reader();
    let lexical = runtime.lexical_reader();
    let access = DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(access_repository),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let mut sources = InMemorySourceRegistry::default();
    sources.insert(discovery_support::source(source_id));
    let no_evidence = discovery_support::NoEvidence;
    let service = DiscoveryService::new(
        discovery_support::discovery_config(source_id, "Base"),
        DiscoveryPorts {
            sources: &sources,
            generations: &projection,
            concepts: &projection,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: Some(&lexical),
                hypergraph: None,
                graph_resource_access: None,
                access: &access,
            },
            selectors: &no_evidence,
            assertions: &no_evidence,
            evidence: &no_evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    assert_eq!(
        projection
            .pin_current(source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        n_plus_one
    );
    let next_result = service
        .discover(discovery_support::request())
        .await
        .unwrap();
    let found: BTreeSet<_> = next_result
        .qualified_resources
        .iter()
        .map(|item| item.resource_ref)
        .collect();
    assert_eq!(
        found,
        BTreeSet::from([first_resource, second_resource]),
        "{next_result:?}"
    );
}

#[tokio::test]
async fn older_event_reloads_post_commit_publish_withdraw_and_explicit_t10() {
    for transition in [
        Transition::PublishReplacement,
        Transition::Withdraw,
        Transition::PublicationEnd,
    ] {
        let fixture = versioning_support::fixture().await;
        let source_id = SourceId::from_uuid(Uuid::now_v7());
        let runtime = MemoryDocumentIndexRuntime::new();
        let receipts = discovery_support::Receipts::default();
        let baseline = DocumentIndexingService::new(DocumentOutboxIndexer::new(
            PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
            discovery_support::index_config(source_id),
            runtime.clone(),
            receipts.clone(),
        ));
        let old_key = match baseline.rebuild().await.unwrap() {
            IndexingOutcome::Published(key) => key,
            other => panic!("expected baseline generation: {other:?}"),
        };

        let entered = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        let observed = Arc::new(Mutex::new(None));
        let paused_reader = PauseBeforeAuthoritativeRead {
            reader: PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
            entered: entered.clone(),
            resume: resume.clone(),
            pause_once: AtomicBool::new(true),
            observed: observed.clone(),
        };
        let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
            paused_reader,
            discovery_support::index_config(source_id),
            runtime.clone(),
            receipts.clone(),
        ));
        // The trigger is deliberately older than the transaction below. The
        // reader pauses before opening its real PostgreSQL snapshot.
        let mut event =
            discovery_support::event("DocumentVersionPublished", fixture.document_id.as_uuid());
        event.occurred_at = OffsetDateTime::UNIX_EPOCH;
        let event_id = event.event_id;
        let pending = tokio::spawn(async move { indexer.handle(event).await });
        entered.notified().await;
        assert!(observed.lock().unwrap().is_none());
        let replacement = apply_transition(&fixture, transition).await;
        resume.notify_one();
        let next_key = match pending.await.unwrap().unwrap() {
            IndexingOutcome::Published(key) => key,
            other => panic!("expected post-commit generation: {other:?}"),
        };
        assert_ne!(next_key, old_key);

        let captured = observed.lock().unwrap().clone().unwrap();
        let reader = runtime.projection_reader();
        let manifest = reader.pin_current(source_id).await.unwrap().unwrap();
        assert_eq!(manifest.key(), next_key);
        assert_eq!(manifest.source_snapshot, captured.source_snapshot);
        assert_eq!(manifest.resource_count, captured.live.len() as u64);
        assert_eq!(
            receipts
                .0
                .lock()
                .unwrap()
                .get(&event_id)
                .unwrap()
                .generation,
            next_key
        );
        let lexical = runtime.lexical_reader();
        let matches = lexical
            .retrieve(
                next_key,
                &discovery_support::request(),
                &LexicalQuery::new("Base", 10),
            )
            .await
            .unwrap();
        match transition {
            Transition::PublishReplacement => {
                let replacement = replacement.unwrap();
                assert_eq!(captured.live.len(), 1);
                assert_eq!(captured.live[0].snapshot.document_version_id, replacement);
                assert_eq!(captured.historical.len(), 1);
                assert!(captured.historical[0].snapshot.publication_end.is_none());
                assert_eq!(matches.len(), 1);
                assert_eq!(
                    matches[0].resource_ref,
                    Some(ResourceId::from_uuid(replacement.as_uuid()))
                );
                assert!(
                    reader
                        .resource_at(next_key, ResourceId::from_uuid(fixture.base_id.as_uuid()))
                        .await
                        .unwrap()
                        .is_none()
                );
            }
            Transition::Withdraw => {
                assert!(captured.live.is_empty());
                assert_eq!(captured.historical.len(), 1);
                assert_eq!(
                    captured.historical[0].snapshot.lifecycle_state,
                    LifecycleState::Withdrawn
                );
                assert!(captured.historical[0].snapshot.publication_end.is_none());
                assert!(matches.is_empty());
            }
            Transition::PublicationEnd => {
                assert!(captured.live.is_empty());
                assert_eq!(captured.historical.len(), 1);
                assert_eq!(
                    captured.historical[0].snapshot.lifecycle_state,
                    LifecycleState::Published
                );
                assert!(captured.historical[0].snapshot.publication_end.is_some());
                assert!(matches.is_empty());
            }
        }
    }
}

#[tokio::test]
async fn source_outage_fails_without_receipt_or_replacing_stored_generation() {
    let fixture = versioning_support::fixture().await;
    let source_id = SourceId::from_uuid(Uuid::now_v7());
    let access_repository = allow_read(&fixture).await;
    let access = DocumentCurrentAccessAdapter::new(
        source_id,
        fixture.pool.clone(),
        DocumentAccessCheckService::new(access_repository),
        actor(),
        discovery_support::ACCESS_CONTEXT.into(),
    );
    let runtime = MemoryDocumentIndexRuntime::new();
    let receipts = discovery_support::Receipts::default();
    let baseline = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        receipts.clone(),
    ));
    let baseline_event =
        discovery_support::event("DocumentVersionPublished", fixture.document_id.as_uuid());
    let old_key = match baseline.handle(baseline_event.clone()).await.unwrap() {
        IndexingOutcome::Published(key) => key,
        other => panic!("expected baseline generation: {other:?}"),
    };
    let receipts_before_outage = receipts.0.lock().unwrap().clone();
    assert_eq!(receipts_before_outage.len(), 1);
    assert_eq!(
        receipts_before_outage
            .get(&baseline_event.event_id)
            .unwrap()
            .generation,
        old_key
    );
    let resource = ResourceId::from_uuid(fixture.base_id.as_uuid());
    let lexical = runtime.lexical_reader();
    let request = discovery_support::request();
    let old_hits = lexical
        .retrieve(old_key, &request, &LexicalQuery::new("Base", 10))
        .await
        .unwrap();
    assert_eq!(old_hits.len(), 1);

    // Closing the reader pool makes the real PostgreSQL adapter return an
    // error. This is not an empty authoritative enumeration.
    fixture.pool.close().await;
    let indexer = DocumentIndexingService::new(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(fixture.pool.clone()),
        discovery_support::index_config(source_id),
        runtime.clone(),
        receipts.clone(),
    ));
    let failed_event =
        discovery_support::event("DocumentVersionPublished", fixture.document_id.as_uuid());
    assert!(matches!(
        indexer.handle(failed_event.clone()).await,
        Err(SearchError::SourceUnavailable(_))
    ));
    assert!(matches!(
        indexer.rebuild().await,
        Err(SearchError::SourceUnavailable(_))
    ));
    assert_eq!(*receipts.0.lock().unwrap(), receipts_before_outage);
    let projection = runtime.projection_reader();
    assert_eq!(
        projection
            .pin_current(source_id)
            .await
            .unwrap()
            .unwrap()
            .key(),
        old_key
    );
    assert!(
        projection
            .resource_at(old_key, resource)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        lexical
            .retrieve(old_key, &request, &LexicalQuery::new("Base", 10))
            .await
            .unwrap()
            .len(),
        1
    );
    // A retained segment is still rebuildable index data. When the authoritative
    // DB itself is down, call-time access cannot authorize its stale candidate.
    assert!(matches!(
        access
            .evaluate(&old_hits[0], discovery_support::ACCESS_CONTEXT)
            .await,
        Err(SearchError::SourceUnavailable(_))
    ));
    let mut sources = InMemorySourceRegistry::default();
    sources.insert(discovery_support::source(source_id));
    let no_evidence = discovery_support::NoEvidence;
    let service = DiscoveryService::new(
        discovery_support::discovery_config(source_id, "Base"),
        DiscoveryPorts {
            sources: &sources,
            generations: &projection,
            concepts: &projection,
            retrieval: RetrievalExecutionPorts {
                directory: None,
                structured: None,
                lexical: Some(&lexical),
                hypergraph: None,
                graph_resource_access: None,
                access: &access,
            },
            selectors: &no_evidence,
            assertions: &no_evidence,
            evidence: &no_evidence,
            probe: None,
            probe_catalog: None,
            source_policy: None,
        },
    )
    .unwrap();
    let result = service.discover(request).await.unwrap();
    assert!(result.qualified_resources.is_empty());
    assert!(result.rejected_candidates.is_empty());
    assert!(result.evidence_set.is_empty());
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains(&resource.as_uuid().to_string())
    );
}
