#![forbid(unsafe_code)]

//! PostgreSQL authoritative persistence adapter for documents.

use sqlx::{PgPool, migrate::MigrateError};
use uuid::Uuid;

pub const SYSTEM_ROOT_FOLDER_ID: Uuid = Uuid::from_u128(0x00000000000070008000000000000001);

pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
