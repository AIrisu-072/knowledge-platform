//! Backend-neutral Search and Discovery application contracts.

#![forbid(unsafe_code)]

pub mod action_selection;
pub mod body_ports;

pub mod candidate;
pub mod content_scope;
pub mod context;
pub mod discovery_service;
pub mod error;
pub mod federation;
pub mod graph_generation;
pub mod indexing_service;
pub mod materialization;
pub mod ports;
pub mod projection;
pub mod qualification;
pub mod remote;
pub mod remote_cache;
pub mod remote_generation;
pub mod remote_identity;
pub mod remote_lease;
pub mod remote_observation;
pub mod remote_registration;
pub mod remote_session;
pub mod retrieval;
pub mod retrieval_execution;
pub mod routing;
pub mod scoped;
pub mod session;
pub mod source_registration;
pub mod source_registry;
pub mod visible_routing;

pub use error::SearchError;
pub use search_core;
