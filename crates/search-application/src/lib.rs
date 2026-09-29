//! Backend-neutral Search and Discovery application contracts.

#![forbid(unsafe_code)]

pub mod candidate;
pub mod error;
pub mod federation;
pub mod materialization;
pub mod ports;
pub mod projection;
pub mod qualification;
pub mod retrieval;
pub mod routing;
pub mod source_registry;

pub use error::SearchError;
pub use search_core;
