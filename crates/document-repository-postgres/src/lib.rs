#![forbid(unsafe_code)]

//! PostgreSQL authoritative persistence adapter for documents.

mod error;
mod mapping;
mod publish;
mod publish_rows;
mod repository;
mod rows;
mod semantic_inspection;
mod semantic_inspection_rows;

use sqlx::{PgPool, migrate::MigrateError};
use uuid::Uuid;

pub use repository::PostgresDocumentRepository;

pub const SYSTEM_ROOT_FOLDER_ID: Uuid = Uuid::from_u128(0x00000000000070008000000000000001);

pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
