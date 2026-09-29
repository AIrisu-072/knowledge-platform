//! Rebuildable, in-process source-local projection generations.

#![forbid(unsafe_code)]

mod retrieval;
mod store;

pub use store::{MemoryProjectionStore, generation_digest};
