//! Source-local, rebuildable Tantivy lexical index.

mod analyzer;
mod body;
mod index;
mod persist;
mod query;
mod schema;

pub use analyzer::{CJK_BIGRAM_ANALYZER_VERSION, LEGACY_ANALYZER_VERSION};
pub use body::{
    BODY_LEXICAL_SCHEMA_VERSION, IndexedUnitDoc, LexicalInputDigest, lexical_input_digest,
};
#[cfg(feature = "fault-injection")]
pub use index::UnitIndexFault;
pub use index::{
    LexicalBuildInput, LexicalDocument, LexicalIndexError, SourceSuppliedBody, TantivyLexicalIndex,
};
pub use persist::PersistedLexical;
