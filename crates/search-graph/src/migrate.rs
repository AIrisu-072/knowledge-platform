//! Graph migrations with their own ledger, applied after Search runtime 0003.

use sqlx::PgPool;
use sqlx::migrate::MigrateError;

const GRAPH_MIGRATION_LEDGER: &str = "search_graph_sqlx_migrations";

pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    // Fixed before the first Graph migration; the Domain and Search runtime
    // ledgers in the same database are independent of this one.
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(GRAPH_MIGRATION_LEDGER);
    migrator.run(pool).await
}
