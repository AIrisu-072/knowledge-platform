use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use search_application::SearchError;
use search_application::indexing_service::{
    DocumentIndexingService, DocumentSourceEvent, IndexingOutcome, validate_document_event_route,
};
use search_application::ports::{
    BoxFuture, CompleteEventRequest, CompletionMode, CurrentGenerationSnapshot,
    FencedDocumentIndexingPort, SearchCompletionOutcome, SearchDeliveryFence,
    SearchEventCompletionPort, SearchSourceLease, SourceFence, SourceRegistryPort,
};
use search_application::source_registry::InMemorySourceRegistry;
use search_core::id::{ProjectionGenerationId, SourceId};
use search_core::projection::ProjectionGenerationKey;
use search_core::source::{DiscoverableSource, DiscoveryMode, EnumerationSemantics, RetentionMode};
use time::OffsetDateTime;
use uuid::Uuid;

fn source_id() -> SourceId {
    SourceId::from_uuid(Uuid::from_u128(1))
}

#[tokio::test]
async fn registry_returns_capabilities_without_source_body() {
    let mut registry = InMemorySourceRegistry::default();
    let mut source = DiscoverableSource::new(
        source_id(),
        "remote-policy",
        EnumerationSemantics::QueryOnly,
        RetentionMode::NoRetention,
    );
    source.discovery_modes.push(DiscoveryMode::RemoteQuery);
    registry.insert(source);
    let resolved = registry.get_source(source_id()).await.unwrap().unwrap();
    assert!(resolved.supports(DiscoveryMode::RemoteQuery));
    assert_eq!(resolved.retention_mode, RetentionMode::NoRetention);
}

#[test]
fn route_matrix_rejects_unknown_and_wrong_aggregate() {
    let document_events = [
        "DocumentCreated",
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
    ];
    let folder_events = ["FolderCreated", "FolderRenamed", "FolderMoved"];

    for event_type in document_events {
        assert!(validate_document_event_route(event_type, "Document").is_ok());
        for wrong_aggregate in ["Folder", "AccessPolicy", "Unknown"] {
            assert!(matches!(
                validate_document_event_route(event_type, wrong_aggregate),
                Err(SearchError::InvalidRequest(_))
            ));
        }
    }
    for event_type in folder_events {
        assert!(validate_document_event_route(event_type, "Folder").is_ok());
        for wrong_aggregate in ["Document", "AccessPolicy", "Unknown"] {
            assert!(matches!(
                validate_document_event_route(event_type, wrong_aggregate),
                Err(SearchError::InvalidRequest(_))
            ));
        }
    }
    for aggregate_type in ["Document", "Folder", "AccessPolicy"] {
        assert!(validate_document_event_route("AccessPolicyChanged", aggregate_type).is_ok());
    }
    for (event_type, aggregate_type) in [
        ("AccessPolicyChanged", "Unknown"),
        ("DocumentFutureEvent", "Document"),
        ("FolderFutureEvent", "Folder"),
        ("AccessPolicyFutureEvent", "AccessPolicy"),
        ("UnrelatedEvent", "Document"),
    ] {
        assert!(matches!(
            validate_document_event_route(event_type, aggregate_type),
            Err(SearchError::InvalidRequest(_))
        ));
    }
}

fn generation_key() -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(2)),
    }
}

fn source_event(event_type: &str) -> DocumentSourceEvent {
    DocumentSourceEvent {
        event_id: Uuid::from_u128(3),
        event_type: event_type.into(),
        aggregate_id: Uuid::from_u128(4),
        occurred_at: OffsetDateTime::UNIX_EPOCH,
    }
}

fn delivery_fence() -> SearchDeliveryFence {
    SearchDeliveryFence {
        event_id: Uuid::from_u128(3),
        outbox_token: Uuid::from_u128(5),
        source: SourceFence {
            source_id: source_id(),
            owner_token: Uuid::from_u128(6),
            epoch: 7,
        },
    }
}

struct TestLease(SourceFence);

impl SearchSourceLease for TestLease {
    fn fence(&self) -> SourceFence {
        self.0
    }
}

#[derive(Default, Clone)]
struct RecordingFencedIndexer {
    calls: Arc<Mutex<Vec<(DocumentSourceEvent, SearchDeliveryFence, bool)>>>,
}

impl FencedDocumentIndexingPort for RecordingFencedIndexer {
    fn refresh_fenced<'a>(
        &'a self,
        event: DocumentSourceEvent,
        fence: SearchDeliveryFence,
        cancel: Arc<AtomicBool>,
    ) -> BoxFuture<'a, IndexingOutcome> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((event, fence, cancel.load(Ordering::Acquire)));
            Ok(IndexingOutcome::Published(generation_key()))
        })
    }
}

struct RecordingCompletion {
    requests: Mutex<Vec<CompleteEventRequest>>,
}

impl SearchEventCompletionPort for RecordingCompletion {
    fn current_snapshot<'a>(&'a self, id: SourceId) -> BoxFuture<'a, CurrentGenerationSnapshot> {
        Box::pin(async move {
            assert_eq!(id, source_id());
            Ok(CurrentGenerationSnapshot {
                key: None,
                manifest_digest: None,
                bundle_digest: None,
                pointer_revision: 0,
            })
        })
    }

    fn complete_event_if_current<'a>(
        &'a self,
        request: CompleteEventRequest,
    ) -> BoxFuture<'a, SearchCompletionOutcome> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            Ok(SearchCompletionOutcome::Published(generation_key()))
        })
    }
}

#[tokio::test]
async fn fenced_port_preserves_search_only_types() {
    let source_fence = TestLease(delivery_fence().source).fence();
    assert_eq!(source_fence.source_id, source_id());
    let completion = RecordingCompletion {
        requests: Mutex::new(Vec::new()),
    };
    let snapshot = completion.current_snapshot(source_id()).await.unwrap();
    let request = CompleteEventRequest {
        fence: delivery_fence(),
        expected_current: snapshot,
        candidate: generation_key(),
        manifest_digest: "manifest-v1".into(),
        bundle_digest: "bundle-v1".into(),
        mode: CompletionMode::PublishCandidate,
    };
    assert_eq!(
        completion.complete_event_if_current(request).await.unwrap(),
        SearchCompletionOutcome::Published(generation_key())
    );
    {
        let requests = completion.requests.lock().unwrap();
        assert_eq!(requests[0].manifest_digest, "manifest-v1");
        assert_eq!(requests[0].bundle_digest, "bundle-v1");
    }

    let indexer = RecordingFencedIndexer::default();
    let service = DocumentIndexingService::new(indexer.clone());
    let event = source_event("DocumentVersionPublished");
    let fence = delivery_fence();
    let cancel = Arc::new(AtomicBool::new(false));
    assert_eq!(
        service
            .handle_delivery(event.clone(), fence, Arc::clone(&cancel))
            .await
            .unwrap(),
        IndexingOutcome::Published(generation_key())
    );
    let calls = indexer.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), &[(event, fence, false)]);
}

#[tokio::test]
async fn fenced_delivery_rejects_invalid_or_cancelled_dispatch_before_indexing() {
    let indexer = RecordingFencedIndexer::default();
    let service = DocumentIndexingService::new(indexer.clone());
    let fence = delivery_fence();
    let active = Arc::new(AtomicBool::new(false));

    for event_type in ["DocumentFutureEvent", "UnrelatedEvent"] {
        assert!(matches!(
            service
                .handle_delivery(source_event(event_type), fence, Arc::clone(&active))
                .await,
            Err(SearchError::InvalidRequest(_))
        ));
    }
    let mut wrong_event = source_event("DocumentCreated");
    wrong_event.event_id = Uuid::from_u128(99);
    assert!(matches!(
        service
            .handle_delivery(wrong_event, fence, Arc::clone(&active))
            .await,
        Err(SearchError::InvalidRequest(_))
    ));

    let cancelled = Arc::new(AtomicBool::new(true));
    assert!(matches!(
        service
            .handle_delivery(source_event("FolderMoved"), fence, cancelled)
            .await,
        Err(SearchError::FenceLost)
    ));
    assert!(indexer.calls.lock().unwrap().is_empty());
}
