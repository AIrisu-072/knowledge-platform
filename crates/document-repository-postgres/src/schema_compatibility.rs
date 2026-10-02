use sqlx::PgPool;
use thiserror::Error;

use crate::MIGRATOR;

/// Safe diagnostic categories without database identifiers or connection details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SchemaCompatibilityError {
    #[error("document schema is not initialized")]
    NotInitialized,
    #[error("document schema is missing a required migration")]
    MissingMigration,
    #[error("document schema contains an unsupported migration")]
    UnexpectedMigration,
    #[error("document schema contains an unsuccessful migration")]
    FailedMigration,
    #[error("document schema migration checksum does not match")]
    ChecksumMismatch,
    #[error("document schema compatibility could not be checked")]
    Unavailable,
}

/// Validate the exact successfully applied migration set without changing state.
///
/// This deliberately does not use `Migrator::run`, which creates migration history
/// and applies pending migrations. Migration remains an explicit operator action.
pub async fn check_schema_compatibility(pool: &PgPool) -> Result<(), SchemaCompatibilityError> {
    let applied: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .map_err(|error| {
                if error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref()
                    == Some("42P01")
                {
                    SchemaCompatibilityError::NotInitialized
                } else {
                    SchemaCompatibilityError::Unavailable
                }
            })?;

    let expected: Vec<_> = MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .collect();
    for (version, success, checksum) in &applied {
        let migration = expected
            .iter()
            .find(|migration| migration.version == *version)
            .ok_or(SchemaCompatibilityError::UnexpectedMigration)?;
        if !success {
            return Err(SchemaCompatibilityError::FailedMigration);
        }
        if checksum.as_slice() != migration.checksum.as_ref() {
            return Err(SchemaCompatibilityError::ChecksumMismatch);
        }
    }
    if expected.len() != applied.len()
        || expected.iter().any(|migration| {
            !applied
                .iter()
                .any(|(version, _, _)| *version == migration.version)
        })
    {
        return Err(SchemaCompatibilityError::MissingMigration);
    }
    Ok(())
}
