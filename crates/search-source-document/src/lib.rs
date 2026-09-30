//! Source-owned Document discovery translation. Current authorization stays in Document.

#![forbid(unsafe_code)]

mod model;
mod postgres;
mod translate;

pub use model::{
    DocumentAccessProjectionInput, DocumentSourceSnapshot, DsiEvidenceRefs,
    PermittedDocumentMetadata, PublicationEndRecord,
};
pub use postgres::{
    DocumentCurrentAccessAdapter, DocumentSnapshotReader, DsiReadState,
    PostgresDocumentSnapshotReader, SnapshotReadError, VersionSnapshotRecord,
};
pub use translate::{
    DocumentIndexInputs, DocumentSourceTranslation, DocumentSourceTranslator,
    DocumentVisibilityClass, TranslationError,
};
