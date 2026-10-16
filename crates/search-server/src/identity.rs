//! PoC identity: every caller presenting this process's token is its fixed
//! profile, the same `poc` principal and group the Document PoC runtime uses
//! (`document-server`'s `StaticPoCIdentityAdapter`). It is not a production
//! authentication mechanism and never consumes request claims.

use std::path::Path;
use std::time::{Duration, Instant};

use document_application::InvocationKind;
use search_api_http::auth::{CredentialError, CredentialFuture, SearchCredentialVerifierPort};
use search_application::ports::BoxFuture;
use search_application::scoped::{
    AccessRevision, CheckedAuthorityAdapter, PrincipalRef, RegistrationRevision, TenantId,
    TrustedSearchScope, VerifiedActorDescriptor, VerifiedActorResolverPort, VerifiedSourceGrant,
    VerifiedSourceVisibilityPort, VisibilityRevision,
};
use search_application::search_core::id::{SessionId, SourceId};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PocProfile {
    Human,
    Agent,
}

impl PocProfile {
    pub const fn principal(self) -> &'static str {
        match self {
            Self::Human => "poc-human",
            Self::Agent => "poc-agent",
        }
    }

    pub const fn group(self) -> &'static str {
        match self {
            Self::Human => "poc-users",
            Self::Agent => "poc-agents",
        }
    }

    pub const fn invocation(self) -> InvocationKind {
        match self {
            Self::Human => InvocationKind::HumanInteractive,
            Self::Agent => InvocationKind::Agent,
        }
    }
}

/// The shortest token accepted from the token file.
pub const MIN_TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    Unreadable,
    TooShort,
    NotPrintable,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unreadable => "KP_SEARCH_TOKEN_FILE cannot be read",
            Self::TooShort => "the search token must be at least 32 bytes",
            Self::NotPrintable => "the search token must be printable ASCII without spaces",
        })
    }
}

/// The process's shared Bearer token. It is never printed.
pub struct Token(Vec<u8>);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl Token {
    pub fn read(path: &Path) -> Result<Self, TokenError> {
        let raw = std::fs::read(path).map_err(|_| TokenError::Unreadable)?;
        Self::parse(&raw)
    }

    /// The file's content without surrounding whitespace (a trailing newline).
    pub fn parse(raw: &[u8]) -> Result<Self, TokenError> {
        let start = raw
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(raw.len());
        let end = raw
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .map_or(start, |at| at + 1);
        let token = &raw[start..end];
        if token.len() < MIN_TOKEN_BYTES {
            return Err(TokenError::TooShort);
        }
        if !token.iter().all(|b| b.is_ascii_graphic()) {
            return Err(TokenError::NotPrintable);
        }
        Ok(Self(token.to_vec()))
    }

    /// Compares in time that depends only on the lengths.
    pub fn matches(&self, presented: &str) -> bool {
        let presented = presented.as_bytes();
        presented.len() == self.0.len()
            && presented
                .iter()
                .zip(&self.0)
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
}

/// A session window: every resolution inside it describes the same session,
/// and the server never outlives its sessions however long it runs.
pub const SESSION_WINDOW: Duration = Duration::from_secs(12 * 60 * 60);
/// A session stays valid for one window after the window in which it began.
pub const SESSION_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// The fixed-profile actor of this process, for one Document Source.
pub struct PocIdentity {
    profile: PocProfile,
    token: Token,
    tenant: TenantId,
    source: SourceId,
    registration_revision: u64,
    visibility_revision: u64,
    started: Instant,
    session_seed: u128,
}

impl PocIdentity {
    pub fn new(
        profile: PocProfile,
        token: Token,
        tenant: TenantId,
        source: SourceId,
        registration_revision: u64,
        visibility_revision: u64,
    ) -> Self {
        Self {
            profile,
            token,
            tenant,
            source,
            registration_revision,
            visibility_revision,
            started: Instant::now(),
            session_seed: Uuid::new_v4().as_u128(),
        }
    }

    pub const fn profile(&self) -> PocProfile {
        self.profile
    }

    /// The start and session ID of the window holding `now`.
    pub fn session_at(&self, now: Instant) -> (Instant, SessionId) {
        let elapsed = now.saturating_duration_since(self.started);
        let windows = elapsed.as_secs() / SESSION_WINDOW.as_secs();
        let start = self.started + Duration::from_secs(windows * SESSION_WINDOW.as_secs());
        let id = Uuid::from_u128(self.session_seed ^ u128::from(windows));
        (start, SessionId::from_uuid(id))
    }
}

impl VerifiedActorResolverPort for PocIdentity {
    fn resolve_verified<'a>(
        &'a self,
        raw_handle: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedActorDescriptor>> {
        Box::pin(async move {
            if !self.token.matches(raw_handle) {
                return Ok(None);
            }
            let (start, session) = self.session_at(Instant::now());
            Ok(Some(VerifiedActorDescriptor::new(
                self.tenant.clone(),
                PrincipalRef::new(self.profile.principal().to_owned())?,
                Some(session),
                AccessRevision::new(1)?,
                start,
                start + SESSION_LIFETIME,
            )?))
        })
    }
}

impl VerifiedSourceVisibilityPort for PocIdentity {
    fn grant_for<'a>(
        &'a self,
        actor: &'a TrustedSearchScope,
        source: SourceId,
    ) -> BoxFuture<'a, Option<VerifiedSourceGrant>> {
        Box::pin(async move {
            // The profile is granted its one Source; Document ACLs still decide
            // every Resource.
            let granted =
                source == self.source && actor.principal().as_str() == self.profile.principal();
            if !granted {
                return Ok(None);
            }
            Ok(Some(VerifiedSourceGrant::new(
                self.tenant.clone(),
                source,
                RegistrationRevision::new(self.registration_revision)?,
                VisibilityRevision::new(self.visibility_revision)?,
            )))
        })
    }
}

/// Bearer verification: the token becomes the verified session handle.
pub struct Credentials(pub Arc<PocIdentity>);

impl SearchCredentialVerifierPort for Credentials {
    fn verify<'a>(&'a self, token: &'a str) -> CredentialFuture<'a> {
        Box::pin(async move {
            if !self.0.token.matches(token) {
                return Ok(None);
            }
            let scope = CheckedAuthorityAdapter::new(&*self.0)
                .authenticate_handle(token)
                .await
                .map_err(|_| CredentialError::Unavailable)?;
            Ok(scope.map(|scope| scope.access_handle().clone()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document_server::identity::PoCIdentityProfile;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn identity(profile: PocProfile) -> PocIdentity {
        PocIdentity::new(
            profile,
            Token::parse(TOKEN.as_bytes()).unwrap(),
            TenantId::new("poc".to_owned()).unwrap(),
            SourceId::from_uuid(Uuid::nil()),
            1,
            1,
        )
    }

    #[test]
    fn profiles_match_the_document_poc_runtime() {
        for (search, document) in [
            (PocProfile::Human, PoCIdentityProfile::Human),
            (PocProfile::Agent, PoCIdentityProfile::Agent),
        ] {
            assert_eq!(search.principal(), document.principal());
            assert_eq!(search.group(), document.group());
            assert_eq!(search.invocation(), document.invocation());
        }
    }

    #[test]
    fn token_file_content_is_trimmed_and_checked() {
        let token = Token::parse(format!("  {TOKEN}\n").as_bytes()).unwrap();
        assert!(token.matches(TOKEN));
        assert!(!token.matches(&TOKEN[1..]));
        assert!(!token.matches(&format!("{TOKEN}x")));
        assert!(!token.matches(&TOKEN.replace('0', "1")));
        assert_eq!(Token::parse(b"short\n").unwrap_err(), TokenError::TooShort);
        assert_eq!(
            Token::parse(format!("{TOKEN} {TOKEN}").as_bytes()).unwrap_err(),
            TokenError::NotPrintable
        );
        assert_eq!(format!("{token:?}"), "Token(<redacted>)");
    }

    #[test]
    fn sessions_hold_within_a_window_and_renew_after_it() {
        let identity = identity(PocProfile::Human);
        let started = identity.started;
        let (a_start, a) = identity.session_at(started + Duration::from_secs(10));
        let (b_start, b) = identity.session_at(started + SESSION_WINDOW - Duration::from_secs(1));
        let (c_start, c) = identity.session_at(started + SESSION_WINDOW + Duration::from_secs(1));
        assert_eq!((a_start, a), (b_start, b));
        assert_ne!(a, c);
        assert_eq!(c_start, started + SESSION_WINDOW);
        // A session begun at the end of a window is still valid for a full window.
        assert!(SESSION_LIFETIME >= SESSION_WINDOW * 2);
    }

    #[tokio::test]
    async fn only_the_configured_token_resolves_to_the_fixed_profile() {
        let identity = Arc::new(identity(PocProfile::Agent));
        let credentials = Credentials(identity.clone());
        assert!(
            credentials
                .verify("wrong-token-wrong-token-wrong-tok")
                .await
                .unwrap()
                .is_none()
        );
        assert!(credentials.verify(TOKEN).await.unwrap().is_some());
        assert!(identity.resolve_verified("nope").await.unwrap().is_none());
        let scope = CheckedAuthorityAdapter::new(&*identity)
            .authenticate_handle(TOKEN)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(scope.principal().as_str(), "poc-agent");
        // The profile sees its one Source and no other.
        let own = SourceId::from_uuid(Uuid::nil());
        let other = SourceId::from_uuid(Uuid::from_u128(1));
        assert!(identity.grant_for(&scope, own).await.unwrap().is_some());
        assert!(identity.grant_for(&scope, other).await.unwrap().is_none());
    }
}
