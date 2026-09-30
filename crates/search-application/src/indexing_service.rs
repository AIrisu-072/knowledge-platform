//! Transport-neutral Document invalidation handling. The Source remains authoritative.

use search_core::projection::ProjectionGenerationKey;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::SearchError;
use crate::ports::BoxFuture;

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

impl<P: DocumentIndexingPort> DocumentIndexingService<P> {
    pub fn new(indexer: P) -> Self {
        Self { indexer }
    }

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

fn relevant_event_type(event_type: &str) -> bool {
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
            | "FolderCreated"
            | "FolderRenamed"
            | "FolderMoved"
            | "AccessPolicyChanged"
    )
}
