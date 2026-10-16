//! Durable typed n-ary Graph generations (P3). PostgreSQL is the provisional
//! backend chosen by the owner on 2026-10-05; the store stays behind the
//! search-application Graph ports so it can be re-selected before production.

#![forbid(unsafe_code)]

pub mod canonical;
pub mod incremental;
pub mod migrate;
pub mod reader;
pub mod segments;
pub mod store;

pub use canonical::{
    GRAPH_SCHEMA_VERSION, Instant, TemporalColumns, canonical_graph_digest,
    canonical_mapping_digest, canonical_relation, decode_temporal, encode_temporal,
    relation_digest,
};
pub use migrate::migrate;
pub use reader::PostgresGraphReader;
pub use store::PostgresGraphStore;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    #[error("graph record is invalid: {0}")]
    Invalid(&'static str),
    /// Stored rows disagree with each other or with their receipt.
    #[error("graph integrity check failed: {0}")]
    Integrity(&'static str),
    /// No BUILDING parent with a live guard for this reference.
    #[error("graph build fence lost")]
    FenceLost,
    #[error("graph store is unavailable")]
    Store,
    /// The incremental closure cannot be proved; build the target in full.
    #[error("graph closure requires a full rebuild")]
    RequiresFullRebuild,
}

impl From<sqlx::Error> for GraphError {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23514" | "42501") => Self::FenceLost,
            Some(_) => Self::Integrity("database constraint"),
            None => Self::Store,
        }
    }
}

impl From<GraphError> for search_application::SearchError {
    fn from(error: GraphError) -> Self {
        match error {
            GraphError::FenceLost => Self::FenceLost,
            GraphError::Store => Self::SourceUnavailable("graph store unavailable".into()),
            GraphError::Invalid(_) | GraphError::Integrity(_) => {
                Self::OperationFailed("graph generation integrity".into())
            }
            GraphError::RequiresFullRebuild => {
                Self::OperationFailed("graph closure requires a full rebuild".into())
            }
        }
    }
}
