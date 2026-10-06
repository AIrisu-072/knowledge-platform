//! P6-I04: composition of the Search outbox worker.
//!
//! One static Document route: the generic outbox runner claims a row only
//! under the distributed Source lease, the bridge delivers it to the fenced
//! Document indexer on the durable P7 runtime, and the indexer completes it
//! only through the atomic P7 port. The types admit no in-process memory
//! runtime. Search processes one row at a time. Delivery state (ack, retry,
//! dead letter) stays with the generic runner and its own role.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use outbox_delivery::postgres::PostgresOutboxStore;
use outbox_delivery::runner::DeliveryRunner;
use outbox_delivery::{DeliveryConfig, DeliveryError, DeliveryPolicy};
use search_application::SearchError;
use search_application::indexing_service::DocumentIndexingService;
use search_application::ports::{BoxFuture, SemanticRegistrySnapshot};
use search_application::search_core::profile::DiscoveryLens;
use search_application::search_core::source::DiscoverableSource;
use search_application::source_registration::{RegistrationActivation, SourceRegistration};
use search_source_document::{
    BodyItemExtractor, DocumentIndexingConfig, DocumentOutboxIndexer,
    DocumentSearchDeliveryHandler, IndexingReceipt, IndexingReceiptStore,
    PostgresDocumentSnapshotReader,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::document_runtime::PgDocumentIndexRuntime;
use crate::event_completion::PgPublication;
use crate::full_guard::FullGuardTtl;
use crate::source_lease::PostgresSourceAdmission;
use crate::source_registration::PgSourceRegistrationLedger;

/// The fenced path never writes legacy receipts; any attempt is an error.
pub struct NoLegacyReceipts;

impl IndexingReceiptStore for NoLegacyReceipts {
    fn get<'a>(&'a self, _event_id: Uuid) -> BoxFuture<'a, Option<IndexingReceipt>> {
        Box::pin(async { Ok(None) })
    }

    fn put<'a>(&'a self, _event_id: Uuid, _receipt: IndexingReceipt) -> BoxFuture<'a, ()> {
        Box::pin(async {
            Err(SearchError::OperationFailed(
                "the Search worker records receipts only through the P7 port".into(),
            ))
        })
    }
}

pub type SearchIndexer =
    DocumentOutboxIndexer<PostgresDocumentSnapshotReader, NoLegacyReceipts, PgDocumentIndexRuntime>;

pub type SearchWorkerRunner = DeliveryRunner<
    PostgresOutboxStore,
    DocumentSearchDeliveryHandler<SearchIndexer>,
    PostgresSourceAdmission,
>;

/// Trusted host composition input. Nothing here comes from a request.
pub struct SearchWorkerConfig {
    pub registration: SourceRegistration,
    pub activation: RegistrationActivation,
    pub source: DiscoverableSource,
    pub lens: DiscoveryLens,
    pub projection_schema_version: String,
    pub analyzer_version: String,
    pub semantic_registry: SemanticRegistrySnapshot,
    pub lexical_root: PathBuf,
    pub delivery: DeliveryConfig,
    pub policy: DeliveryPolicy,
    pub source_lease: Duration,
    pub guard_ttl: FullGuardTtl,
}

#[derive(Debug)]
pub enum WorkerError {
    InvalidConfig(&'static str),
    Delivery(DeliveryError),
}

/// Builds the runner. `delivery_pool` serves the generic outbox role;
/// `search_pool` reads the Source and completes Search work.
pub fn compose(
    delivery_pool: PgPool,
    search_pool: PgPool,
    ledger: &PgSourceRegistrationLedger,
    extractor: Arc<dyn BodyItemExtractor>,
    config: SearchWorkerConfig,
) -> Result<SearchWorkerRunner, WorkerError> {
    let source_id = config.registration.source_id();
    if config.source.source_id != source_id {
        return Err(WorkerError::InvalidConfig(
            "Source descriptor and registration differ",
        ));
    }
    let indexer = indexer(search_pool, ledger, extractor, &config)?;
    let handler =
        DocumentSearchDeliveryHandler::new(DocumentIndexingService::new(indexer), source_id);
    let admission = ledger
        .source_admission(source_id, config.source_lease)
        .map_err(WorkerError::Delivery)?;
    // Search work is serialized per Source: one claim, one in flight.
    let delivery = DeliveryConfig {
        batch_size: 1,
        max_in_flight: 1,
        ..config.delivery
    };
    DeliveryRunner::new(
        Arc::new(PostgresOutboxStore::new(delivery_pool, config.policy)),
        Arc::new(handler),
        Arc::new(admission),
        delivery,
    )
    .map_err(WorkerError::Delivery)
}

/// The fenced durable indexer both the delivery runner and a manual rebuild
/// run on.
fn indexer(
    search_pool: PgPool,
    ledger: &PgSourceRegistrationLedger,
    extractor: Arc<dyn BodyItemExtractor>,
    config: &SearchWorkerConfig,
) -> Result<SearchIndexer, WorkerError> {
    let registrar = ledger
        .generation_registrar(config.registration.clone(), config.activation)
        .map_err(|_| WorkerError::InvalidConfig("generation registrar"))?;
    let runtime = PgDocumentIndexRuntime::new(
        search_pool.clone(),
        &config.lexical_root,
        config.source.clone(),
        registrar,
        config.guard_ttl,
    );
    Ok(DocumentOutboxIndexer::new(
        PostgresDocumentSnapshotReader::new(search_pool.clone()),
        DocumentIndexingConfig {
            source: config.source.clone(),
            lens: config.lens.clone(),
            projection_schema_version: config.projection_schema_version.clone(),
            analyzer_version: config.analyzer_version.clone(),
            semantic_registry: config.semantic_registry.clone(),
        },
        runtime,
        NoLegacyReceipts,
    )
    .with_body_extractor(extractor)
    .with_fenced_completion(Arc::new(PgPublication::new(search_pool))))
}

/// A2: the operator's manual retry. A full rebuild of the Source from its
/// current Document snapshot, outside the outbox, so it has no attempt limit
/// (automatic redelivery stops at the delivery policy's `max_attempts`) and
/// supersedes any event that limit dead-lettered. It publishes only through
/// the MANUAL CAS on the Source row, never by acknowledging events.
pub fn compose_rebuild(
    search_pool: PgPool,
    ledger: &PgSourceRegistrationLedger,
    extractor: Arc<dyn BodyItemExtractor>,
    config: SearchWorkerConfig,
) -> Result<DocumentIndexingService<SearchIndexer>, WorkerError> {
    if config.source.source_id != config.registration.source_id() {
        return Err(WorkerError::InvalidConfig(
            "Source descriptor and registration differ",
        ));
    }
    Ok(DocumentIndexingService::new(indexer(
        search_pool,
        ledger,
        extractor,
        &config,
    )?))
}
