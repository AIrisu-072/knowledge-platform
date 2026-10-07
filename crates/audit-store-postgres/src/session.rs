//! Startup checks on the database session (design §10.1): every command
//! except `migrate`, `bootstrap-admin`, `bind` and `unbind` refuses a session
//! that is a superuser or a member of `audit_store_owner`.

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
    #[error("cannot inspect the database session: {0}")]
    Database(#[from] sqlx::Error),
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

/// Requires an owner-member session (bootstrap, bind, unbind).
pub async fn require_owner_member(pool: &PgPool) -> Result<(), SessionError> {
    if !session_privilege(pool).await?.owner_member {
        return Err(SessionError::NotOwnerMember);
    }
    Ok(())
}
