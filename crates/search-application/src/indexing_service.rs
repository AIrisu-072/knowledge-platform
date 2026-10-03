//! Transport-neutral Document invalidation handling. The Source remains authoritative.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use search_core::projection::ProjectionGenerationKey;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::SearchError;
use crate::ports::{BoxFuture, FencedDocumentIndexingPort, SearchDeliveryFence};

/// The generic outbox delivery worker owns its delivery state. This value only
/// asks Search to reconcile its derived index with the current Document Source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSourceEvent {
    pub event_id: Uuid,
    pub event_type: String,
    /// The outbox aggregate may be a Document, Folder, or AccessPolicy.
    pub aggregate_id: Uuid,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexingOutcome {
    Published(ProjectionGenerationKey),
    Unchanged(ProjectionGenerationKey),
    Duplicate(ProjectionGenerationKey),
    Ignored,
}

pub trait DocumentIndexingPort: Send + Sync {
    fn refresh<'a>(&'a self, event: DocumentSourceEvent) -> BoxFuture<'a, IndexingOutcome>;
    fn rebuild<'a>(&'a self) -> BoxFuture<'a, IndexingOutcome>;
}

pub struct DocumentIndexingService<P> {
    indexer: P,
}

impl<P> DocumentIndexingService<P> {
    pub fn new(indexer: P) -> Self {
        Self { indexer }
    }
}

impl<P: DocumentIndexingPort> DocumentIndexingService<P> {
    pub async fn handle(&self, event: DocumentSourceEvent) -> Result<IndexingOutcome, SearchError> {
        if !relevant_event_type(&event.event_type) {
            if event.event_type.starts_with("Document")
                || event.event_type.starts_with("Folder")
                || event.event_type.starts_with("AccessPolicy")
            {
                return Err(SearchError::InvalidRequest(format!(
                    "unsupported Document domain event type: {}",
                    event.event_type
                )));
            }
            return Ok(IndexingOutcome::Ignored);
        }
        self.indexer.refresh(event).await
    }

    /// An independent reconciliation path recovers missed events or a lost index.
    pub async fn rebuild(&self) -> Result<IndexingOutcome, SearchError> {
        self.indexer.rebuild().await
    }
}

impl<P: FencedDocumentIndexingPort> DocumentIndexingService<P> {
    /// The bridge validates the actual aggregate type before constructing the
    /// event; this service validates the event kind and dispatch identity.
    pub async fn handle_delivery(
        &self,
        event: DocumentSourceEvent,
        fence: SearchDeliveryFence,
        cancel: Arc<AtomicBool>,
    ) -> Result<IndexingOutcome, SearchError> {
        if !relevant_event_type(&event.event_type) {
            return Err(SearchError::InvalidRequest(
                "unsupported Document domain event type".into(),
            ));
        }
        if event.event_id != fence.event_id {
            return Err(SearchError::InvalidRequest(
                "Document delivery event and fence IDs differ".into(),
            ));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(SearchError::FenceLost);
        }
        self.indexer.refresh_fenced(event, fence, cancel).await
    }
}

/// Validate the envelope route before the bridge constructs a Source event.
/// Payload is never used as the indexing Source.
pub fn validate_document_event_route(
    event_type: &str,
    aggregate_type: &str,
) -> Result<(), SearchError> {
    let valid = match event_type {
        "AccessPolicyChanged" => matches!(aggregate_type, "Document" | "Folder" | "AccessPolicy"),
        _ if is_document_event_type(event_type) => aggregate_type == "Document",
        _ if is_folder_event_type(event_type) => aggregate_type == "Folder",
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(SearchError::InvalidRequest(
            "invalid Document outbox event route".into(),
        ))
    }
}

fn relevant_event_type(event_type: &str) -> bool {
    is_document_event_type(event_type)
        || is_folder_event_type(event_type)
        || event_type == "AccessPolicyChanged"
}

fn is_document_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "DocumentCreated"
            | "DocumentVersionCreated"
            | "DocumentVersionUpdated"
            | "DocumentVersionRebased"
            | "DocumentVersionPublished"
            | "DocumentVersionWithdrawn"
            | "DocumentVersionPublicationScheduled"
            | "DocumentVersionPublicationCancelled"
            | "DocumentVersionPublicationTerminal"
            | "DocumentPublicationEnded"
            | "DocumentMetadataChanged"
            | "DocumentMoved"
    )
}

fn is_folder_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "FolderCreated" | "FolderRenamed" | "FolderMoved"
    )
}
