//! Backend-neutral Search and Discovery application contracts.

#![forbid(unsafe_code)]

pub mod action_selection;

pub mod candidate;
pub mod context;
pub mod discovery_service;
pub mod error;
pub mod federation;
pub mod materialization;
pub mod ports;
pub mod projection;
pub mod qualification;
pub mod retrieval;
pub mod retrieval_execution;
pub mod routing;
pub mod session;
pub mod source_registry;

pub use error::SearchError;
pub use search_core;
