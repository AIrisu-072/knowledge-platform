//! Rebuildable, immutable source-local typed n-ary incidence graph.

#![forbid(unsafe_code)]

mod index;
mod traversal;

pub use index::{GraphIndexError, MemoryGraphRetriever};
