//! Startup checks on database sessions (design §3, §10.1).
//!
//! - `run`, `health`, `reconcile` and `replay` refuse a source session that is
//!   a superuser or a member of `audit_relay_owner` (the Store side refuses
//!   superuser and `audit_store_owner` members in `audit-store-postgres`).
//! - The relay refuses when source and Store resolve to the same database
//!   (`system_identifier` and `current_database()` both equal).
//! - Connection URLs carrying `options` are refused, and every pooled
//!   connection must report `synchronous_commit = on`.
//!
//! Errors carry codes only, never a URL or credential.

use std::fmt;
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, PgConnection, PgPool, Row};

/// Which connection a check refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Source,
    Store,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Source => "source",
            Self::Store => "store",
        })
    }
}

/// Why the relay refused to start. No variant carries a URL.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartupError {
    #[error("refusing a superuser or owner-member {0} session for this command")]
    Privileged(Side),
    #[error("source and store resolve to the same database")]
    SameDatabase,
    #[error("{side} connection URL must not carry `options`")]
    UrlOptions { side: Side },
    #[error("{side} connection URL is not a postgres URL")]
    UrlInvalid { side: Side },
    #[error("{side} session does not run with synchronous_commit = on")]
    SynchronousCommitOff { side: Side },
    #[error("audit_relay posture is invalid ({count} violations: {codes})")]
    PostureInvalid { count: usize, codes: String },
    #[error("{side} database unavailable ({code})")]
    Unavailable { side: Side, code: String },
    #[error("{side} database refused ({sqlstate})")]
    Database { side: Side, sqlstate: String },
}

impl StartupError {
    /// Maps a driver error without its message (which could echo input).
    pub fn from_sqlx(side: Side, error: &sqlx::Error) -> Self {
        match error {
            sqlx::Error::Database(db) => Self::Database {
                side,
                sqlstate: db.code().map(|c| c.into_owned()).unwrap_or_default(),
            },
            sqlx::Error::Configuration(_) => Self::UrlInvalid { side },
            sqlx::Error::PoolTimedOut => Self::Unavailable {
                side,
                code: "pool_timeout".into(),
            },
            sqlx::Error::Io(_) | sqlx::Error::Tls(_) => Self::Unavailable {
                side,
                code: "transport".into(),
            },
            sqlx::Error::Protocol(message) if message == SYNC_COMMIT_REFUSED => {
                Self::SynchronousCommitOff { side }
            }
            _ => Self::Unavailable {
                side,
                code: "unclassified".into(),
            },
        }
    }
}

const SYNC_COMMIT_REFUSED: &str = "audit relay: synchronous_commit is not on";

/// Refuses a URL whose query string sets `options` (which could, e.g., turn
/// off synchronous_commit or change search_path for the session). The same
/// check as the Store client (`audit_store_postgres::session::check_url`).
pub fn check_url(side: Side, url: &str) -> Result<(), StartupError> {
    audit_store_postgres::session::check_url(url).map_err(|error| match error {
        audit_store_postgres::SessionError::UrlOptions => StartupError::UrlOptions { side },
        _ => StartupError::UrlInvalid { side },
    })
}

/// A short redacted label for logs: scheme and database name only.
pub fn redact(url: &str) -> String {
    let database = url
        .split('?')
        .next()
        .and_then(|base| base.rsplit_once('/'))
        .map(|(_, db)| db)
        .filter(|db| !db.contains('@') && !db.contains(':'))
        .unwrap_or("?");
    format!("postgres://<redacted>/{database}")
}

/// Connects a pool after the URL check. Every new connection must report
/// `synchronous_commit = on`.
pub async fn connect(
    side: Side,
    url: &str,
    max_connections: u32,
    acquire_timeout: Duration,
) -> Result<PgPool, StartupError> {
    check_url(side, url)?;
    // One plain connection first: a refused setting is reported as such
    // (the pool would retry a failing after_connect until it times out).
    let mut first = tokio::time::timeout(acquire_timeout, PgConnection::connect(url))
        .await
        .map_err(|_| StartupError::Unavailable {
            side,
            code: "connect_timeout".into(),
        })?
        .map_err(|error| StartupError::from_sqlx(side, &error))?;
    let value: String = sqlx::query_scalar("SHOW synchronous_commit")
        .fetch_one(&mut first)
        .await
        .map_err(|error| StartupError::from_sqlx(side, &error))?;
    let _ = first.close().await;
    if value != "on" {
        return Err(StartupError::SynchronousCommitOff { side });
    }
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(acquire_timeout)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                let value: String = sqlx::query_scalar("SHOW synchronous_commit")
                    .fetch_one(&mut *conn)
                    .await?;
                if value != "on" {
                    return Err(sqlx::Error::Protocol(SYNC_COMMIT_REFUSED.into()));
                }
                Ok(())
            })
        })
        .connect(url)
        .await
        .map_err(|error| StartupError::from_sqlx(side, &error))?;
    require_synchronous_commit(side, &pool).await?;
    Ok(pool)
}

/// `SHOW synchronous_commit` must be `on`.
pub async fn require_synchronous_commit(side: Side, pool: &PgPool) -> Result<(), StartupError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| StartupError::from_sqlx(side, &error))?;
    conn.ping()
        .await
        .map_err(|error| StartupError::from_sqlx(side, &error))?;
    let value: String = sqlx::query_scalar("SHOW synchronous_commit")
        .fetch_one(&mut *conn)
        .await
        .map_err(|error| StartupError::from_sqlx(side, &error))?;
    if value != "on" {
        return Err(StartupError::SynchronousCommitOff { side });
    }
    Ok(())
}

/// What the source login can do beyond its capability roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPrivilege {
    pub superuser: bool,
    pub owner_member: bool,
}

impl SessionPrivilege {
    pub const fn is_privileged(self) -> bool {
        self.superuser || self.owner_member
    }
}

/// Inspects `session_user` on the source database.
pub async fn source_privilege(pool: &PgPool) -> Result<SessionPrivilege, sqlx::Error> {
    let row = sqlx::query(
        "SELECT r.rolsuper AS superuser, \
                CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_roles \
                                  WHERE rolname = 'audit_relay_owner') \
                     THEN pg_catalog.pg_has_role(session_user, 'audit_relay_owner', 'MEMBER') \
                     ELSE FALSE END AS owner_member \
         FROM pg_catalog.pg_roles AS r WHERE r.rolname = session_user",
    )
    .fetch_one(pool)
    .await?;
    Ok(SessionPrivilege {
        superuser: row.try_get("superuser")?,
        owner_member: row.try_get("owner_member")?,
    })
}

/// Refuses superuser and `audit_relay_owner`-member source sessions.
pub async fn refuse_privileged_source(pool: &PgPool) -> Result<(), StartupError> {
    let privilege = source_privilege(pool)
        .await
        .map_err(|error| StartupError::from_sqlx(Side::Source, &error))?;
    if privilege.is_privileged() {
        return Err(StartupError::Privileged(Side::Source));
    }
    Ok(())
}

/// `(system_identifier, current_database())` of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseIdentity {
    pub system_identifier: String,
    pub database: String,
}

pub async fn database_identity(pool: &PgPool) -> Result<DatabaseIdentity, sqlx::Error> {
    let row = sqlx::query(
        "SELECT (SELECT c.system_identifier::text FROM pg_catalog.pg_control_system() AS c) \
                    AS system_identifier, \
                pg_catalog.current_database()::text AS database",
    )
    .fetch_one(pool)
    .await?;
    Ok(DatabaseIdentity {
        system_identifier: row.try_get("system_identifier")?,
        database: row.try_get("database")?,
    })
}

/// Refuses when both sessions resolve to the same database (design §3).
pub async fn refuse_same_database(source: &PgPool, store: &PgPool) -> Result<(), StartupError> {
    let a = database_identity(source)
        .await
        .map_err(|error| StartupError::from_sqlx(Side::Source, &error))?;
    let b = database_identity(store)
        .await
        .map_err(|error| StartupError::from_sqlx(Side::Store, &error))?;
    if a == b {
        return Err(StartupError::SameDatabase);
    }
    Ok(())
}

/// One `audit_relay.posture_check()` row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PostureViolation {
    pub violation: String,
    pub object: String,
}

pub async fn posture(pool: &PgPool) -> Result<Vec<PostureViolation>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT violation, object FROM audit_relay.posture_check() ORDER BY 1, 2")
            .fetch_all(pool)
            .await?;
    rows.iter()
        .map(|row| {
            Ok(PostureViolation {
                violation: row.try_get("violation")?,
                object: row.try_get("object")?,
            })
        })
        .collect()
}

/// Refuses while `audit_relay.posture_check()` reports any violation.
pub async fn require_posture(pool: &PgPool) -> Result<(), StartupError> {
    let violations = posture(pool)
        .await
        .map_err(|error| StartupError::from_sqlx(Side::Source, &error))?;
    if violations.is_empty() {
        return Ok(());
    }
    let mut codes: Vec<&str> = violations.iter().map(|v| v.violation.as_str()).collect();
    codes.sort_unstable();
    codes.dedup();
    Err(StartupError::PostureInvalid {
        count: violations.len(),
        codes: codes.join(","),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_in_the_url_are_refused() {
        let ok = "postgres://u:p@h:5432/db?sslmode=disable";
        assert_eq!(check_url(Side::Source, ok), Ok(()));
        for bad in [
            "postgres://u:p@h/db?options=-c%20synchronous_commit%3Doff",
            "postgres://u:p@h/db?sslmode=disable&OPTIONS=x",
            "postgres://u:p@h/db?%6Fptions=x",
            "postgresql://u:p@h/db?options",
        ] {
            assert_eq!(
                check_url(Side::Store, bad),
                Err(StartupError::UrlOptions { side: Side::Store }),
                "{bad}"
            );
        }
        assert_eq!(
            check_url(Side::Source, "mysql://x"),
            Err(StartupError::UrlInvalid { side: Side::Source })
        );
    }

    #[test]
    fn redaction_keeps_only_the_database_name() {
        let label = redact("postgres://user:secret@host:5432/document?sslmode=require");
        assert_eq!(label, "postgres://<redacted>/document");
        assert!(!label.contains("secret") && !label.contains("host"));
        assert_eq!(
            redact("postgres://user:secret@host"),
            "postgres://<redacted>/?"
        );
    }
}
