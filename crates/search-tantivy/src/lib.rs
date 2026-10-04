//! Source-local, rebuildable Tantivy lexical index.

mod index;
mod query;
mod schema;

pub use index::{
    LexicalBuildInput, LexicalDocument, LexicalIndexError, SourceSuppliedBody, TantivyLexicalIndex,
};
