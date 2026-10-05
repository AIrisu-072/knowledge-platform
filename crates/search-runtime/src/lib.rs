#![forbid(unsafe_code)]

pub mod event_completion;
pub mod full_guard;
pub mod gc;
pub mod generation_registration;
pub mod graph_coordination;
pub mod lexical_artifact;
pub mod payload;
pub mod pin;
pub mod ready;
pub mod source_lease;
pub mod source_registration;

use sqlx::{PgPool, migrate::MigrateError};

const SEARCH_MIGRATION_LEDGER: &str = "search_runtime_sqlx_migrations";

pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    // This ledger name is fixed before the first Search migration. Domain
    // migrations retain SQLx's default _sqlx_migrations ledger in this DB.
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(SEARCH_MIGRATION_LEDGER);
    migrator.run(pool).await
}
