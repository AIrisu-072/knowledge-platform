//! P7-12: startup verification and restart recovery.
//!
//! Startup checks the required schema objects, lets GC collect only expired
//! guards, targets and pins on the DB clock (a live guard is never removed),
//! and then re-verifies the current key from every stored artifact: payload
//! DTOs and composite digest, the lexical directory, and the Graph rows with
//! their mapping. A key that fails is reported unusable and is never served;
//! the next build uses a new key from the Source. Nothing here moves a pin to
//! another key or restores RAM-only Remote state.

use std::path::PathBuf;

use search_application::search_core::id::ProjectionGenerationId;
use search_application::search_core::projection::ProjectionGenerationKey;
use search_application::search_core::source::DiscoverableSource;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::gc::{CleanupReport, GcError, PgGenerationGc};
use crate::ready::{ReadyCoordinator, ReadyError, VerifiedBundle};

/// Schema objects the runtime needs before it may serve or claim work.
const REQUIRED: [&str; 16] = [
    "public.search_source_coordination",
    "public.search_source_ownership",
    "public.search_generation",
    "public.search_generation_full_guard",
    "public.search_generation_payload",
    "public.search_generation_receipt",
    "public.search_lexical_artifact",
    "public.search_evaluation_lease",
    "public.search_evaluation_lease_by_generation",
    "search_graph.generation",
    "search_graph.resource",
    "search_graph.relation",
    "search_graph.participant",
    "search_graph.build_guard",
    "search_graph.graph_participant_incidence",
    "search_graph.graph_build_guard_by_base",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    /// A required table, index or function is missing: stay unready.
    Schema(&'static str),
    Store,
}

impl From<sqlx::Error> for RecoveryError {
    fn from(_: sqlx::Error) -> Self {
        Self::Store
    }
}

impl From<GcError> for RecoveryError {
    fn from(_: GcError) -> Self {
        Self::Store
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentState {
    None,
    Verified(Box<VerifiedBundle>),
    /// The current key failed re-verification and must not be served.
    Unusable(ProjectionGenerationKey, ReadyError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupReport {
    pub cleanup: CleanupReport,
    pub current: CurrentState,
}

pub struct PgStartupRecovery {
    pool: PgPool,
    lexical_root: PathBuf,
    source: DiscoverableSource,
}

impl PgStartupRecovery {
    pub fn new(pool: PgPool, lexical_root: impl Into<PathBuf>, source: DiscoverableSource) -> Self {
        Self {
            pool,
            lexical_root: lexical_root.into(),
            source,
        }
    }

    pub async fn check_schema(&self) -> Result<(), RecoveryError> {
        for object in REQUIRED {
            let present: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
                .bind(object)
                .fetch_one(&self.pool)
                .await?;
            if !present {
                return Err(RecoveryError::Schema(object));
            }
        }
        let lock: bool = sqlx::query_scalar(
            "SELECT to_regprocedure('public.search_gc_lock_source(uuid)') IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await?;
        if !lock {
            return Err(RecoveryError::Schema("public.search_gc_lock_source(uuid)"));
        }
        Ok(())
    }

    /// Re-verifies the Source's current key from every stored artifact and
    /// the pointer's two digests.
    pub async fn verify_current(&self) -> Result<CurrentState, RecoveryError> {
        let row = sqlx::query(
            "SELECT current_generation_id, current_manifest_digest, current_bundle_digest \
             FROM search_source_coordination WHERE source_id=$1",
        )
        .bind(self.source.source_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(CurrentState::None);
        };
        let Some(generation) = row.try_get::<Option<Uuid>, _>("current_generation_id")? else {
            return Ok(CurrentState::None);
        };
        let key = ProjectionGenerationKey {
            source_id: self.source.source_id,
            generation_id: ProjectionGenerationId::from_uuid(generation),
        };
        let coordinator =
            ReadyCoordinator::new(self.pool.clone(), &self.lexical_root, self.source.clone());
        let bundle = match coordinator.reverify(key).await {
            Ok(bundle) => bundle,
            Err(error) => return Ok(CurrentState::Unusable(key, error)),
        };
        let hex = |digest: &[u8; 32]| -> String {
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("sha256:{hex}")
        };
        let manifest: Option<String> = row.try_get("current_manifest_digest")?;
        let bundle_digest: Option<String> = row.try_get("current_bundle_digest")?;
        if manifest.as_deref() != Some(bundle.graph().projection_manifest_digest.as_str())
            || bundle_digest != Some(hex(&bundle.receipt().composite_digest))
        {
            return Ok(CurrentState::Unusable(
                key,
                ReadyError::Payload(crate::payload::BundleError::Digest),
            ));
        }
        Ok(CurrentState::Verified(Box::new(bundle)))
    }

    /// Schema → expired cleanup → current re-verification, in that order.
    pub async fn startup(&self, cleanup_limit: u32) -> Result<StartupReport, RecoveryError> {
        self.check_schema().await?;
        let cleanup = PgGenerationGc::new(self.pool.clone(), &self.lexical_root)
            .cleanup_expired(cleanup_limit)
            .await?;
        let current = self.verify_current().await?;
        Ok(StartupReport { cleanup, current })
    }
}
