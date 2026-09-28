#![forbid(unsafe_code)]

//! PostgreSQL authoritative persistence adapter for documents.

mod access_control;
mod access_policy;
mod error;
mod mapping;
mod publication_end;
mod publish;
mod publish_rows;
mod repository;
mod rows;
mod schedule;
mod semantic_inspection;
mod semantic_inspection_rows;
mod targeted_events;
mod versioning;
mod versioning_mutation;
mod versioning_rows;
mod withdrawal;

use sqlx::{PgPool, migrate::MigrateError};
use uuid::Uuid;

pub use repository::PostgresDocumentRepository;

pub const SYSTEM_ROOT_FOLDER_ID: Uuid = Uuid::from_u128(0x00000000000070008000000000000001);

pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
