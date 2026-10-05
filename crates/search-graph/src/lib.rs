//! Durable typed n-ary Graph generations (P3). PostgreSQL is the provisional
//! backend chosen by the owner on 2026-10-05; the store stays behind the
//! search-application Graph ports so it can be re-selected before production.

#![forbid(unsafe_code)]

pub mod canonical;
pub mod migrate;

pub use canonical::{
    GRAPH_SCHEMA_VERSION, Instant, TemporalColumns, canonical_graph_digest, canonical_relation,
    decode_temporal, encode_temporal,
};
pub use migrate::migrate;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    #[error("graph record is invalid: {0}")]
    Invalid(&'static str),
}
