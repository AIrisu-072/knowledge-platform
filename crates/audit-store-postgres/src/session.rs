//! Startup checks on the database session (design §7.3, §10.1).
//!
//! - Every command except `migrate`, `bootstrap-admin`, `bind`, `unbind` and
//!   `register-source-service` refuses a session that is a superuser or a
//!   member of `audit_store_owner` (those owner-only commands require an
//!   owner member instead).
//! - A connection URL carrying `options` (or any `options[<setting>]` key,
//!   which the driver also turns into startup options) is refused, as is a
//!   non-empty `PGOPTIONS` environment variable (they could, e.g., turn off
//!   synchronous_commit, zero the role timeouts or change search_path), and
//!   the session must report `SHOW synchronous_commit = on` after
//!   connecting.
//!
//! Errors carry codes only, never a URL or credential.

use sqlx::{PgPool, Row};

/// What the session's login role can do beyond its capability roles.
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

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("refusing a superuser or audit_store_owner session for this command")]
    Privileged,
    #[error("this command requires a member of audit_store_owner")]
    NotOwnerMember,
    #[error("connection URL must not carry `options`")]
    UrlOptions,
    #[error("PGOPTIONS must not be set (it sets session options on every connection)")]
    EnvironmentOptions,
    #[error("connection URL is not a postgres URL")]
    UrlInvalid,
    #[error("session does not run with synchronous_commit = on")]
    SynchronousCommitOff,
    #[error("cannot inspect the database session: {0}")]
    Database(#[from] sqlx::Error),
}

/// Refuses a URL that is not `postgres[ql]://` or whose query string sets
/// `options` or an `options[<setting>]` key (any case, percent-encoded or
/// not).
pub fn check_url(url: &str) -> Result<(), SessionError> {
    if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
        return Err(SessionError::UrlInvalid);
    }
    let Some((_, query)) = url.split_once('?') else {
        return Ok(());
    };
    let query = query.split('#').next().unwrap_or_default();
    for pair in query.split('&') {
        let key = pair.split('=').next().unwrap_or_default();
        let decoded = percent_decode(key).ok_or(SessionError::UrlInvalid)?;
        if decoded
            .trim()
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("options"))
        {
            return Err(SessionError::UrlOptions);
        }
    }
    Ok(())
}

/// Refuses a non-empty `PGOPTIONS` value: the driver applies it as startup
/// options to every connection.
pub fn check_pgoptions(value: Option<&std::ffi::OsStr>) -> Result<(), SessionError> {
    match value {
        Some(value) if !value.is_empty() => Err(SessionError::EnvironmentOptions),
        _ => Ok(()),
    }
}

/// [`check_pgoptions`] on this process's environment.
pub fn check_environment() -> Result<(), SessionError> {
    check_pgoptions(std::env::var_os("PGOPTIONS").as_deref())
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = text.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// `SHOW synchronous_commit` must be `on`.
pub async fn require_synchronous_commit(pool: &PgPool) -> Result<(), SessionError> {
    let value: String = sqlx::query_scalar("SHOW synchronous_commit")
        .fetch_one(pool)
        .await?;
    if value != "on" {
        return Err(SessionError::SynchronousCommitOff);
    }
    Ok(())
}

/// Inspects `session_user` (never `current_user`).
pub async fn session_privilege(pool: &PgPool) -> Result<SessionPrivilege, sqlx::Error> {
    let row = sqlx::query(
        "SELECT r.rolsuper AS superuser, \
                CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_roles \
                                  WHERE rolname = 'audit_store_owner') \
                     THEN pg_catalog.pg_has_role(session_user, 'audit_store_owner', 'MEMBER') \
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

/// Refuses superuser and owner-member sessions.
pub async fn refuse_privileged(pool: &PgPool) -> Result<(), SessionError> {
    if session_privilege(pool).await?.is_privileged() {
        return Err(SessionError::Privileged);
    }
    Ok(())
}

/// Requires an owner-member session (bootstrap, bind, unbind, source service).
pub async fn require_owner_member(pool: &PgPool) -> Result<(), SessionError> {
    if !session_privilege(pool).await?.owner_member {
        return Err(SessionError::NotOwnerMember);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_in_the_url_are_refused() {
        assert!(check_url("postgres://u:p@h:5432/db?sslmode=disable").is_ok());
        assert!(check_url("postgresql://u:p@h/db").is_ok());
        for bad in [
            "postgres://u:p@h/db?options=-c%20synchronous_commit%3Doff",
            "postgres://u:p@h/db?sslmode=disable&OPTIONS=x",
            "postgres://u:p@h/db?%6Fptions=x",
            "postgresql://u:p@h/db?options",
            "postgres://u:p@h/db?options[statement_timeout]=0",
            "postgres://u:p@h/db?sslmode=disable&Options%5Block_timeout%5D=0",
            "postgres://u:p@h/db?%6Fptions[search_path]=x",
        ] {
            assert!(
                matches!(check_url(bad), Err(SessionError::UrlOptions)),
                "{bad}"
            );
        }
        assert!(check_pgoptions(None).is_ok());
        assert!(check_pgoptions(Some(std::ffi::OsStr::new(""))).is_ok());
        assert!(matches!(
            check_pgoptions(Some(std::ffi::OsStr::new("-c statement_timeout=0"))),
            Err(SessionError::EnvironmentOptions)
        ));
        assert!(matches!(
            check_url("mysql://x"),
            Err(SessionError::UrlInvalid)
        ));
        assert!(matches!(
            check_url("postgres://h/db?%zz=1"),
            Err(SessionError::UrlInvalid)
        ));
    }
}
