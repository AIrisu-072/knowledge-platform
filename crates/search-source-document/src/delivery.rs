//! P6-S05: the Search bridge from the generic outbox runner to the fenced
//! Document indexer. Only a validated event and aggregate route becomes a
//! `DocumentSourceEvent`; the payload is never forwarded. A committed
//! `Published`, `Unchanged` or `Duplicate` completion is `Applied`; an
//! unsupported or misrouted event is terminal; every failure, cancellation or
//! unknown commit is retryable and leaves the row to the runner, which alone
//! settles outbox state.

use outbox_delivery::runner::{ClaimPermit, DeliveryContext, DeliveryHandler};
use outbox_delivery::{DeliveryDecision, DeliveryEnvelope, ErrorCode, HandlerFuture};
use search_application::SearchError;
use search_application::indexing_service::{
    DocumentIndexingService, DocumentSourceEvent, IndexingOutcome, validate_document_event_route,
};
use search_application::ports::{
    FencedDocumentIndexingPort, SearchDeliveryFence, SearchSourceLease,
};
use search_core::id::SourceId;

pub struct DocumentSearchDeliveryHandler<I> {
    indexer: DocumentIndexingService<I>,
    source_id: SourceId,
}

impl<I> DocumentSearchDeliveryHandler<I> {
    pub fn new(indexer: DocumentIndexingService<I>, source_id: SourceId) -> Self {
        Self { indexer, source_id }
    }
}

/// Known event types with a wrong aggregate are invalid envelopes; anything
/// else that fails routing is an unsupported event.
fn route_failure(event_type: &str) -> ErrorCode {
    let known = ["Document", "Folder", "AccessPolicy"]
        .iter()
        .any(|aggregate| validate_document_event_route(event_type, aggregate).is_ok());
    if known {
        ErrorCode::InvalidEnvelope
    } else {
        ErrorCode::UnsupportedEvent
    }
}

pub(crate) fn decision(result: Result<IndexingOutcome, SearchError>) -> DeliveryDecision {
    match result {
        Ok(
            IndexingOutcome::Published(_)
            | IndexingOutcome::Unchanged(_)
            | IndexingOutcome::Duplicate(_),
        ) => DeliveryDecision::Applied,
        Ok(IndexingOutcome::Ignored) | Err(SearchError::InvalidRequest(_)) => {
            DeliveryDecision::Terminal(ErrorCode::UnsupportedEvent)
        }
        Err(SearchError::CompletionUnknown) => {
            DeliveryDecision::Retryable(ErrorCode::DeliveryUnknown)
        }
        Err(SearchError::SourceUnavailable(_) | SearchError::FenceLost) => {
            DeliveryDecision::Retryable(ErrorCode::SourceUnavailable)
        }
        Err(SearchError::OperationFailed(_)) => {
            DeliveryDecision::Retryable(ErrorCode::IndexingFailed)
        }
    }
}

impl<I, P> DeliveryHandler<P> for DocumentSearchDeliveryHandler<I>
where
    I: FencedDocumentIndexingPort,
    P: ClaimPermit + SearchSourceLease + 'static,
{
    fn deliver(
        &self,
        envelope: DeliveryEnvelope,
        context: DeliveryContext,
        permit: P,
    ) -> HandlerFuture<'_, DeliveryDecision> {
        Box::pin(async move {
            if validate_document_event_route(&envelope.event_type, &envelope.aggregate_type)
                .is_err()
            {
                return DeliveryDecision::Terminal(route_failure(&envelope.event_type));
            }
            let source = permit.fence();
            if source.source_id != self.source_id {
                return DeliveryDecision::Retryable(ErrorCode::SourceUnavailable);
            }
            let fence = SearchDeliveryFence {
                event_id: envelope.event_id,
                outbox_token: context.outbox_token,
                source,
            };
            let event = DocumentSourceEvent {
                event_id: envelope.event_id,
                event_type: envelope.event_type,
                aggregate_id: envelope.aggregate_id,
                occurred_at: envelope.occurred_at,
            };
            decision(
                self.indexer
                    .handle_delivery(event, fence, context.cancel.clone())
                    .await,
            )
        })
    }
}
