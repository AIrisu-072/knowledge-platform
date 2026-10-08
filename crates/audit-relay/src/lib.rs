//! Audit relay: delivers Document `public.audit_outbox_events` rows into the
//! Audit Store (design 2026-10-07 revision 2, §5, §6, §12; decision D4).
//!
//! - [`migrate`] installs the `audit_relay` schema on the Document database
//!   (ledger `audit_relay_sqlx_migrations`): registration trigger, staging
//!   and ledger guards, the `BEGIN ATOMIC` source digest and the definer
//!   functions owned by the NOLOGIN role `audit_relay_owner`.
//! - [`source::RelayOutboxStore`], [`handler::AuditDeliveryHandler`] and
//!   [`breaker::BreakerAdmission`] plug into the unchanged
//!   `outbox_delivery::DeliveryRunner`.
//! - [`reconcile`], [`replay`] and [`health`] implement the operator paths.
//! - [`monitor`]: the circuit-breaker state `run` reports for `health` and
//!   its bounded progress lines on stderr.
//! - [`store::RelayStore`]: the `audit_core::AuditStore` port plus the Store
//!   status and lost-range reads.
#![forbid(unsafe_code)]

pub mod breaker;
pub mod config;
pub mod handler;
pub mod health;
pub mod ledger;
pub mod monitor;
pub mod reconcile;
pub mod relay;
pub mod replay;
pub mod session;
pub mod source;
pub mod store;

use sqlx::PgPool;
use sqlx::migrate::MigrateError;

/// The relay's own migration ledger on the Document database. Document's
/// `_sqlx_migrations` is never written (design §3).
pub const MIGRATION_LEDGER: &str = "audit_relay_sqlx_migrations";

/// Capability roles, grants and login timeouts (design §10.1), applied by
/// the database owner after [`migrate`].
pub const ROLES_SQL: &str = include_str!("../sql/roles.sql");

/// The staging columns the relay digests and projects, with their types.
pub const STAGING_COLUMNS: [(&str, &str); 15] = [
    ("event_id", "uuid"),
    ("event_type", "text"),
    ("source", "text"),
    ("subject", "text"),
    ("actor_identity_provider", "text"),
    ("actor_principal_id", "text"),
    ("resource_type", "text"),
    ("resource_id", "uuid"),
    ("resource_version_id", "uuid"),
    ("result", "text"),
    ("trace_id", "text"),
    ("data", "jsonb"),
    ("occurred_at", "timestamp with time zone"),
    ("attempt_count", "integer"),
    ("delivered_at", "timestamp with time zone"),
];

/// Why the preflight refused the migration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreflightError {
    #[error("public.audit_outbox_events does not exist (run the Document migrations first)")]
    StagingMissing,
    #[error("the Document migration ledger _sqlx_migrations does not exist")]
    DocumentLedgerMissing,
    #[error("public.audit_outbox_events lacks or mistypes columns: {0:?}")]
    Columns(Vec<String>),
    #[error("database error during preflight ({0})")]
    Database(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RelayMigrateError {
    #[error(transparent)]
    Preflight(#[from] PreflightError),
    #[error("relay migration failed: {0}")]
    Migrate(#[from] MigrateError),
}

/// Checks the Document side before anything is created (design §13).
pub async fn preflight(pool: &PgPool) -> Result<(), PreflightError> {
    let database = |error: sqlx::Error| {
        PreflightError::Database(match &error {
            sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()).unwrap_or_default(),
            _ => "unavailable".to_owned(),
        })
    };
    let (staging, ledger): (bool, bool) = sqlx::query_as(
        "SELECT pg_catalog.to_regclass('public.audit_outbox_events') IS NOT NULL, \
                pg_catalog.to_regclass('public._sqlx_migrations') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .map_err(database)?;
    if !staging {
        return Err(PreflightError::StagingMissing);
    }
    if !ledger {
        return Err(PreflightError::DocumentLedgerMissing);
    }
    let present: Vec<(String, String)> = sqlx::query_as(
        "SELECT a.attname::text, pg_catalog.format_type(a.atttypid, a.atttypmod) \
         FROM pg_catalog.pg_attribute AS a \
         WHERE a.attrelid = 'public.audit_outbox_events'::regclass \
           AND a.attnum > 0 AND NOT a.attisdropped",
    )
    .fetch_all(pool)
    .await
    .map_err(database)?;
    let missing: Vec<String> = STAGING_COLUMNS
        .iter()
        .filter(|(name, kind)| !present.iter().any(|(n, k)| n == name && k == kind))
        .map(|(name, _)| (*name).to_owned())
        .collect();
    if !missing.is_empty() {
        return Err(PreflightError::Columns(missing));
    }
    Ok(())
}

/// Runs the relay migrations on the Document database after the Document
/// migrations, as a superuser (or the staging owner that may SET ROLE
/// `audit_relay_owner`). Producer INSERTs wait while it runs.
pub async fn migrate(pool: &PgPool) -> Result<(), RelayMigrateError> {
    preflight(pool).await?;
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.dangerous_set_table_name(MIGRATION_LEDGER);
    migrator.run(pool).await?;
    Ok(())
}
