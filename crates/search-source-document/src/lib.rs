//! Source-owned Document discovery translation. Current authorization stays in Document.

#![forbid(unsafe_code)]

mod model;
mod outbox;
mod postgres;
mod relations;
mod translate;

pub use model::{
    DocumentAccessProjectionInput, DocumentSourceSnapshot, DsiEvidenceRefs,
    PermittedDocumentMetadata, PublicationEndRecord,
};
pub use outbox::{
    DocumentIndexRuntime, DocumentIndexingConfig, DocumentLexicalReader, DocumentOutboxIndexer,
    DocumentOutboxReader, DocumentProjectionReader, IndexingReceipt, IndexingReceiptStore,
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
