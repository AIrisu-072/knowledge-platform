//! Backend-neutral Search and Discovery application contracts.

#![forbid(unsafe_code)]

pub mod error;
pub mod ports;
pub mod qualification;
pub mod source_registry;

pub use error::SearchError;
pub use search_core;
