//! Versioned, transport-independent Audit contract. No business event or SQL types.
mod canonical;
mod catalog;
mod event;
mod json;
mod legacy;
mod schema;
pub use schema::schema_validate;

pub use canonical::{canonical_bytes, event_digest};
pub use event::{AuditEnvelope, ValidationError};
pub use legacy::LegacyAuditRow;
