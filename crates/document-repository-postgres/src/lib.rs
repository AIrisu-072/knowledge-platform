#![forbid(unsafe_code)]

//! PostgreSQL authoritative persistence adapter for documents.

mod access_control;
mod access_policy;
mod authorized_repository;
mod document_management;
mod error;
mod folder_management;
mod folder_preflight;
mod mapping;
mod publication_end;
mod publish;
mod publish_rows;
mod read_state;
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

pub use folder_preflight::{
    FolderPreflightCategory, FolderPreflightFinding, FolderPreflightReport, preflight_folder_names,
};
pub use repository::PostgresDocumentRepository;

pub const SYSTEM_ROOT_FOLDER_ID: Uuid = Uuid::from_u128(0x00000000000070008000000000000001);

pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    let folders_exist: bool =
        sqlx::query_scalar("SELECT to_regclass('public.folders') IS NOT NULL")
            .fetch_one(pool)
            .await
            .map_err(MigrateError::Execute)?;
    if folders_exist {
        let report = preflight_folder_names(pool).await.map_err(|error| {
            MigrateError::Execute(sqlx::Error::Protocol(format!(
                "folder preflight could not run: {error}"
            )))
        })?;
        if !report.is_clean() {
            return Err(MigrateError::Execute(sqlx::Error::Protocol(format!(
                "folder preflight rejected migration ({} findings; first 32 IDs/categories: {:?})",
                report.findings.len(),
                report.findings.iter().take(32).collect::<Vec<_>>()
            ))));
        }
    }
    sqlx::migrate!("./migrations").run(pool).await
}
