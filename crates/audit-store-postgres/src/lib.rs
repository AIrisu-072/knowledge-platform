//! PostgreSQL Audit Store (design 2026-10-07 revision 2, §7–§11).
//!
//! The Store lives in its own database and `audit_store` schema with the
//! migration ledger `audit_store_sqlx_migrations`. Every write and read goes
//! through SECURITY DEFINER SQL functions owned by the NOLOGIN, non-superuser
//! role `audit_store_owner`; this crate is the client side:
//!
//! - [`PostgresAuditStore`] implements the relay's [`audit_core::AuditStore`]
//!   port (idempotent ingest, admission probe).
//! - [`admin::AuditAdmin`] wraps the investigation, verification, retention,
//!   access and recovery functions used by `audit-admin`.
//! - [`files`] writes export, manifest and checkpoint files (mode 0600).

pub mod admin;
pub mod error;
pub mod files;
pub mod hex;
pub mod session;
pub mod store;

pub use error::AdminError;
pub use session::{SessionError, SessionPrivilege};
pub use store::{PostgresAuditStore, classify_sqlx_error};

use sqlx::PgPool;
use sqlx::migrate::MigrateError;

/// Name of the Store's own migration ledger. The default `_sqlx_migrations`
/// table is never touched.
pub const MIGRATION_LEDGER: &str = "audit_store_sqlx_migrations";

/// Capability role template (design §10.1), applied by the database owner.
pub const ROLES_SQL: &str = include_str!("../sql/roles.sql");

/// Idempotent database privileges and login-role timeouts (design §11),
/// re-applied after every restore and every new login role.
pub const PRIVILEGES_SQL: &str = include_str!("../sql/privileges.sql");

/// Runs the Store migrations with a superuser (migrator) connection.
pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(MIGRATION_LEDGER);
    migrator.run(pool).await
}
