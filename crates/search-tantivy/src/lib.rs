//! Source-local, rebuildable Tantivy lexical index.

mod body;
mod index;
mod query;
mod schema;

pub use body::{
    BODY_LEXICAL_SCHEMA_VERSION, IndexedUnitDoc, LexicalInputDigest, lexical_input_digest,
};
#[cfg(feature = "fault-injection")]
pub use index::UnitIndexFault;
pub use index::{
    LexicalBuildInput, LexicalDocument, LexicalIndexError, SourceSuppliedBody, TantivyLexicalIndex,
};
