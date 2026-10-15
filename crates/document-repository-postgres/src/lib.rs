#![forbid(unsafe_code)]

//! PostgreSQL authoritative persistence adapter for documents.

mod access_control;
mod access_policy;
mod action_capability;
mod authorized_repository;
mod create_outcome;
mod current_read_state;
mod document_diff_access;
mod document_diff_cache;
mod document_diff_snapshot;
mod document_history;
mod document_management;
mod document_query;
mod document_revision;
mod document_revision_read;
mod edit_manifest;
mod error;
mod file_access;
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
mod schema_compatibility;
mod semantic_inspection;
mod semantic_inspection_rows;
mod targeted_events;
mod versioning;
mod versioning_mutation;
mod versioning_rows;
mod withdrawal;

use sqlx::{
    PgPool,
    migrate::{MigrateError, Migrator},
};
use uuid::Uuid;

pub use folder_preflight::{
    FolderPreflightCategory, FolderPreflightFinding, FolderPreflightReport, preflight_folder_names,
};
pub use repository::PostgresDocumentRepository;
pub use schema_compatibility::{SchemaCompatibilityError, check_schema_compatibility};

pub const SYSTEM_ROOT_FOLDER_ID: Uuid = Uuid::from_u128(0x00000000000070008000000000000001);

// The explicit migration command and read-only runtime compatibility check must
// always use the same packaged migration identities and checksums.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

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
    MIGRATOR.run(pool).await
}
