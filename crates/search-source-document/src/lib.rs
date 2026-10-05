//! Source-owned Document discovery translation. Current authorization stays in Document.

#![forbid(unsafe_code)]

mod api_read;
mod body_absence;
mod body_bundle;
mod body_evidence;
mod body_manifest;
mod coverage;
mod delivery;
mod evidence;
mod extraction;
mod graph_mapping;
mod history;
mod model;
mod outbox;
mod postgres;
mod relations;
mod translate;

pub use api_read::DocumentApiRead;
pub use body_bundle::{PublishedBody, graph_receipt, seal_lexical};
pub use body_evidence::{
    CurrentVersionReader, DocumentBodyCoverageGaps, DocumentExactTextEvidenceCatalog,
};
pub use body_manifest::{
    ArtifactReceipt, BodyCoverageArtifact, BodyCoverageItem, BodyItemEntry, BodyUnitManifest,
    GenerationBundleReceipt, LEXICAL_SCHEMA_VERSION, compute_bundle_receipt, coverage_receipt,
    profile_set_digest, projection_digest, unit_manifest_receipt, validate_manifest,
    validate_restored_manifest,
};
pub use coverage::{DocumentCoveragePreflight, DocumentCoverageRequirement};
pub use delivery::DocumentSearchDeliveryHandler;
pub use evidence::{DocumentEvidenceCatalog, DocumentEvidenceField};
pub use extraction::{
    BodyBuildError, BodyItemExtractor, BodyProfileRegistry, DocumentBodyExtractor,
    ExtractedItemResult,
};
pub use graph_mapping::{
    DocumentGenerationAccess, DocumentGraphMappingValidator, document_graph_records,
    graph_relations, validate_document_graph_mapping,
};
pub use history::DocumentHistoricalLookup;
pub use model::{
    AuthoritativeItemBinding, DocumentAccessProjectionInput, DocumentSourceSnapshot,
    DsiEvidenceRefs, PermittedDocumentMetadata, PublicationEndRecord,
};
pub use outbox::{
    DocumentGraphAccessReader, DocumentGraphActorAccess, DocumentGraphReader, DocumentIndexRuntime,
    DocumentIndexingConfig, DocumentLexicalReader, DocumentOutboxIndexer, DocumentOutboxReader,
    DocumentProjectionReader, DurableDocumentGraph, IndexingReceipt, IndexingReceiptStore,
    MemoryDocumentIndexRuntime,
};
pub use postgres::{
    DocumentCurrentAccessAdapter, DocumentOutboxSnapshot, DocumentSnapshotReader, DsiReadState,
    PostgresDocumentSnapshotReader, SnapshotReadError, VersionSnapshotRecord,
};
pub use relations::{
    DocumentRelationProjection, DocumentRelationProjector, RelationProjectionError,
    document_resource_id, folder_resource_id,
};
pub use translate::{
    DocumentIndexInputs, DocumentSourceTranslation, DocumentSourceTranslator,
    DocumentVisibilityClass, TranslationError,
};
