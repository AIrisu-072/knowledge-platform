//! P7-10: evaluation pins bound to the actor scope.
//!
//! `pin_current` tentatively pre-checks the current lexical directory before
//! any lock, then in one short transaction locks Source → generation rows and
//! inserts a server-issued lease for the current key whose READY P7 and P3
//! rows, Source activation, ACTIVE ownership, revisions and both digests still
//! hold at that moment. The row binds the tenant, a host actor-scope
//! reference, the registration, visibility and access revisions, the
//! evaluation and a DB-clock expiry, and is immutable apart from the expiry.
//! Renewal, the before-return check and release each require the pin together
//! with the current `TrustedDiscoveryBinding` and `AuthorizedSourceScope`;
//! they re-run the P4/P5 current gates and re-match the row. A pin on an
//! older key stays valid after the pointer moves on. An actor that lost its
//! scope cannot release; only coordinator GC removes expired rows.

use std::path::PathBuf;
use std::time::Duration;

use search_application::SearchError;
use search_application::graph_generation::{GraphLeaseVerifierPort, GraphReadLease};
use search_application::ports::{AccessDecision, BoxFuture};
use search_application::scoped::{
    AccessBindingState, AccessContextAuthorityPort, AuthorizedSourceScope,
    CurrentSourceVisibilityPort, TrustedDiscoveryBinding, TrustedSearchScope,
};
use search_application::search_core::id::ProjectionGenerationId;
use search_application::search_core::projection::ProjectionGenerationKey;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::lexical_artifact::LexicalArtifactStore;

/// A bounded pin lifetime, measured on the database clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinTtl(i64);

impl PinTtl {
    pub fn new(duration: Duration) -> Option<Self> {
        let micros = i64::try_from(duration.as_micros()).ok()?;
        (micros > 0
            && duration <= Duration::from_secs(300)
            && duration == Duration::from_micros(micros as u64))
        .then_some(Self(micros))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinError {
    /// The actor, its Source scope or the pin row do not match; nothing may
    /// be returned under this pin.
    Denied,
    /// No READY current generation with its lexical directory is pinnable.
    NoCurrent,
    /// The store or an authority port failed; re-read before retrying.
    Unavailable,
}

impl From<sqlx::Error> for PinError {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("23514" | "23503" | "42501") => Self::Denied,
            _ => Self::Unavailable,
        }
    }
}

impl From<PinError> for SearchError {
    fn from(_: PinError) -> Self {
        Self::SourceUnavailable("evaluation pin unavailable".into())
    }
}

/// A committed pin. Its fields identify the row; they are never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationPin {
    pub lease: GraphReadLease,
    pub manifest_digest: String,
    pub bundle_digest: String,
}

/// Host reference of the actor scope: tenant, principal and session. The
/// host re-resolves the same reference after a restart or the pin fails.
fn actor_scope_ref(actor: &TrustedSearchScope) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"search-actor-scope-v1");
    for part in [actor.tenant().as_str(), actor.principal().as_str()] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    match actor.session() {
        Some(session) => {
            hasher.update([1]);
            hasher.update(session.as_uuid().as_bytes());
        }
        None => hasher.update([0]),
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("actor:sha256:{hex}")
}

fn db_revision(value: u64) -> Result<i64, PinError> {
    i64::try_from(value).map_err(|_| PinError::Denied)
}

/// Row match for a live pin of this actor scope, with its Source still
/// ACTIVE at the pinned revisions and both generation rows READY.
macro_rules! live_pin {
    () => {
        "l.source_id=$1 AND l.lease_id=$2 AND l.evaluation_id=$3 AND l.generation_id=$4 \
         AND l.tenant_owner_key=$5 AND l.actor_scope_ref=$6 AND l.registration_revision=$7 \
         AND l.visibility_revision=$8 AND l.access_revision=$9 AND l.activation_epoch=$10 \
         AND l.expires_at > clock_timestamp() \
         AND EXISTS (SELECT 1 FROM search_source_coordination s \
             JOIN search_source_ownership o USING (source_id) \
             WHERE s.source_id=l.source_id AND s.registration_active \
               AND s.tenant_owner_key=l.tenant_owner_key AND o.tenant_owner_key=l.tenant_owner_key \
               AND s.registration_revision=l.registration_revision \
               AND o.registration_revision=l.registration_revision \
               AND s.visibility_revision=l.visibility_revision \
               AND o.visibility_revision=l.visibility_revision \
               AND s.activation_epoch=l.activation_epoch AND o.activation_epoch=l.activation_epoch \
               AND o.state='ACTIVE') \
         AND EXISTS (SELECT 1 FROM search_generation g \
             JOIN search_graph.generation gg USING (source_id, generation_id) \
             WHERE g.source_id=l.source_id AND g.generation_id=l.generation_id \
               AND g.state='READY' AND gg.state='READY')"
    };
}

/// The checked identity of one pin call: the lease plus the current actor.
struct PinMatch {
    lease: GraphReadLease,
    tenant: String,
    actor_ref: String,
    registration_revision: i64,
    visibility_revision: i64,
    access_revision: i64,
    activation_epoch: i64,
}

/// Binds `$1..$10` of [`live_pin!`] for one checked pin.
macro_rules! bind_match {
    ($query:expr, $pin:expr $(,)?) => {
        $query
            .bind($pin.lease.key().source_id.as_uuid())
            .bind($pin.lease.lease_id())
            .bind($pin.lease.evaluation_id().as_uuid())
            .bind($pin.lease.key().generation_id.as_uuid())
            .bind(&$pin.tenant)
            .bind(&$pin.actor_ref)
            .bind($pin.registration_revision)
            .bind($pin.visibility_revision)
            .bind($pin.access_revision)
            .bind($pin.activation_epoch)
    };
}

/// PostgreSQL pins with the borrowed P4 actor and P5 Source gates.
pub struct PgEvaluationPins<'g> {
    pool: PgPool,
    lexical_root: PathBuf,
    authority: &'g dyn AccessContextAuthorityPort,
    visibility: &'g dyn CurrentSourceVisibilityPort,
}

impl<'g> PgEvaluationPins<'g> {
    pub fn new(
        pool: PgPool,
        lexical_root: impl Into<PathBuf>,
        authority: &'g dyn AccessContextAuthorityPort,
        visibility: &'g dyn CurrentSourceVisibilityPort,
    ) -> Self {
        Self {
            pool,
            lexical_root: lexical_root.into(),
            authority,
            visibility,
        }
    }

    /// P4/P5 gates: one live actor for both values, both still current.
    async fn gates(
        &self,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
    ) -> Result<(), PinError> {
        if binding.actor() != scope.actor() || !scope.actor().is_live() {
            return Err(PinError::Denied);
        }
        let actor = self
            .authority
            .current(binding.actor())
            .await
            .map_err(|_| PinError::Unavailable)?;
        let source = self
            .visibility
            .current(scope)
            .await
            .map_err(|_| PinError::Unavailable)?;
        if actor != AccessBindingState::Current || source != AccessDecision::Allowed {
            return Err(PinError::Denied);
        }
        Ok(())
    }

    async fn checked(
        &self,
        lease: &GraphReadLease,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
    ) -> Result<PinMatch, PinError> {
        self.gates(binding, scope).await?;
        if lease.evaluation_id() != binding.evaluation()
            || lease.key().source_id != scope.source_id()
        {
            return Err(PinError::Denied);
        }
        let actor = scope.actor();
        Ok(PinMatch {
            lease: *lease,
            tenant: actor.tenant().as_str().to_owned(),
            actor_ref: actor_scope_ref(actor),
            registration_revision: db_revision(scope.registration_revision().get())?,
            visibility_revision: db_revision(scope.visibility_revision().get())?,
            access_revision: db_revision(actor.access_revision().get())?,
            activation_epoch: db_revision(scope.registration_activation().get())?,
        })
    }

    /// Tentative check, before any lock, that the key's sealed lexical
    /// directory is where its row says. READY already proved its content.
    async fn lexical_present(&self, key: ProjectionGenerationKey) -> Result<bool, PinError> {
        let relpath: Option<String> = sqlx::query_scalar(
            "SELECT index_relpath FROM search_lexical_artifact \
             WHERE source_id=$1 AND generation_id=$2",
        )
        .bind(key.source_id.as_uuid())
        .bind(key.generation_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        let expected = LexicalArtifactStore::relpath(key);
        Ok(relpath.as_deref() == Some(expected.as_str())
            && self.lexical_root.join(&expected).is_dir())
    }

    /// Pins the generation that is current right now for this actor scope.
    pub async fn pin_current(
        &self,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
        ttl: PinTtl,
    ) -> Result<EvaluationPin, PinError> {
        self.gates(binding, scope).await?;
        let source = scope.source_id();
        let actor = scope.actor();
        let tenant = actor.tenant().as_str();
        let registration_revision = db_revision(scope.registration_revision().get())?;
        let visibility_revision = db_revision(scope.visibility_revision().get())?;
        let activation_epoch = db_revision(scope.registration_activation().get())?;
        for _ in 0..3 {
            let current: Option<Option<Uuid>> = sqlx::query_scalar(
                "SELECT current_generation_id FROM search_source_coordination \
                 WHERE source_id=$1",
            )
            .bind(source.as_uuid())
            .fetch_optional(&self.pool)
            .await?;
            let Some(Some(generation)) = current else {
                return Err(PinError::NoCurrent);
            };
            let key = ProjectionGenerationKey {
                source_id: source,
                generation_id: ProjectionGenerationId::from_uuid(generation),
            };
            if !self.lexical_present(key).await? {
                return Err(PinError::NoCurrent);
            }

            let mut tx = self.pool.begin().await?;
            let row = sqlx::query(
                "SELECT current_generation_id, current_manifest_digest, current_bundle_digest, \
                 tenant_owner_key, registration_revision, visibility_revision, activation_epoch, \
                 registration_active FROM search_source_coordination WHERE source_id=$1 FOR SHARE",
            )
            .bind(source.as_uuid())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(PinError::NoCurrent)?;
            if row.try_get::<Option<Uuid>, _>("current_generation_id")? != Some(generation) {
                continue; // The pointer moved after the pre-check.
            }
            if row.try_get::<String, _>("tenant_owner_key")? != tenant
                || row.try_get::<i64, _>("registration_revision")? != registration_revision
                || row.try_get::<i64, _>("visibility_revision")? != visibility_revision
                || row.try_get::<i64, _>("activation_epoch")? != activation_epoch
                || !row.try_get::<bool, _>("registration_active")?
            {
                return Err(PinError::Denied);
            }
            let manifest_digest: Option<String> = row.try_get("current_manifest_digest")?;
            let bundle_digest: Option<String> = row.try_get("current_bundle_digest")?;
            let (Some(manifest_digest), Some(bundle_digest)) = (manifest_digest, bundle_digest)
            else {
                return Err(PinError::NoCurrent);
            };
            let ready: Option<i32> = sqlx::query_scalar(
                "SELECT 1 FROM search_generation g \
                 JOIN search_graph.generation gg USING (source_id, generation_id) \
                 WHERE g.source_id=$1 AND g.generation_id=$2 AND g.state='READY' \
                   AND g.activation_epoch=$3 AND gg.state='READY' FOR SHARE OF g, gg",
            )
            .bind(source.as_uuid())
            .bind(generation)
            .bind(activation_epoch)
            .fetch_optional(&mut *tx)
            .await?;
            if ready.is_none() {
                return Err(PinError::NoCurrent);
            }
            let lease_id: Uuid = sqlx::query_scalar(
                "INSERT INTO search_evaluation_lease (source_id, lease_id, evaluation_id, \
                 generation_id, activation_epoch, tenant_owner_key, actor_scope_ref, \
                 registration_revision, visibility_revision, access_revision, manifest_digest, \
                 bundle_digest, expires_at) VALUES ($1, gen_random_uuid(), $2, $3, $4, $5, $6, \
                 $7, $8, $9, $10, $11, clock_timestamp() + ($12::bigint * INTERVAL '1 microsecond')) \
                 RETURNING lease_id",
            )
            .bind(source.as_uuid())
            .bind(binding.evaluation().as_uuid())
            .bind(generation)
            .bind(activation_epoch)
            .bind(tenant)
            .bind(actor_scope_ref(actor))
            .bind(registration_revision)
            .bind(visibility_revision)
            .bind(db_revision(actor.access_revision().get())?)
            .bind(&manifest_digest)
            .bind(&bundle_digest)
            .bind(ttl.0)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(EvaluationPin {
                lease: GraphReadLease::from_identifiers(key, binding.evaluation(), lease_id),
                manifest_digest,
                bundle_digest,
            });
        }
        Err(PinError::Unavailable)
    }

    /// Extends a live pin of the same actor scope on the DB clock.
    pub async fn renew(
        &self,
        lease: &GraphReadLease,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
        ttl: PinTtl,
    ) -> Result<(), PinError> {
        let pin = self.checked(lease, binding, scope).await?;
        let renewed: Option<Uuid> = bind_match!(
            sqlx::query(concat!(
                "UPDATE search_evaluation_lease l SET expires_at = clock_timestamp() + \
                 ($11::bigint * INTERVAL '1 microsecond') WHERE ",
                live_pin!(),
                " RETURNING l.lease_id"
            )),
            &pin,
        )
        .bind(ttl.0)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| row.try_get("lease_id"))
        .transpose()?;
        renewed.map(|_| ()).ok_or(PinError::Denied)
    }

    /// Checks, in a new DB-clock transaction, that results may still be
    /// returned under this pin.
    pub async fn verify_pin_before_return(
        &self,
        lease: &GraphReadLease,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
    ) -> Result<(), PinError> {
        let pin = self.checked(lease, binding, scope).await?;
        let live = bind_match!(
            sqlx::query(concat!(
                "SELECT l.lease_id FROM search_evaluation_lease l WHERE ",
                live_pin!()
            )),
            &pin,
        )
        .fetch_optional(&self.pool)
        .await?;
        live.map(|_| ()).ok_or(PinError::Denied)
    }

    /// Releases a live pin; an actor that lost its scope cannot release.
    pub async fn release(
        &self,
        lease: &GraphReadLease,
        binding: &TrustedDiscoveryBinding,
        scope: &AuthorizedSourceScope,
    ) -> Result<(), PinError> {
        let pin = self.checked(lease, binding, scope).await?;
        let released = bind_match!(
            sqlx::query(concat!(
                "DELETE FROM search_evaluation_lease l WHERE ",
                live_pin!(),
                " RETURNING l.lease_id"
            )),
            &pin,
        )
        .fetch_optional(&self.pool)
        .await?;
        released.map(|_| ()).ok_or(PinError::Denied)
    }
}

impl GraphLeaseVerifierPort for PgEvaluationPins<'_> {
    fn verify<'a>(
        &'a self,
        lease: &'a GraphReadLease,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move { Ok(self.verify_pin_before_return(lease, binding, scope).await?) })
    }
}
