//! P7-11 with P3-C02/C03: guarded GC and FK-safe cleanup.
//!
//! Every call locks the Source row (through a lock-only definer function),
//! then the P7 and Graph generation rows. It refuses the current key, a key
//! with a live pin, a live full guard, a live Graph build guard on the target,
//! or any Graph build guard that uses the key as its base. Otherwise, in one
//! transaction: DELETING → guard DELETE → lease DELETE → Graph participant,
//! relation and resource → P7 children → Graph and P7 parents. The permanent
//! identity, guard issuance and historical Search receipts stay, so an expired
//! target never receives a guard again. Guard and lease rows are not locked:
//! renewal never revives an expired row and a new lease needs a current key.
//! Lexical directories are removed only after commit, idempotently.

use std::path::Path;

use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::projection::ProjectionGenerationKey;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::full_guard::{EventCandidateHandle, ManualBuildHandle};
use crate::lexical_artifact::LexicalArtifactStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    Current,
    Pinned,
    Guarded,
    /// A Graph build guard still copies from this key.
    BaseOfBuild,
    /// The generation is not in a state this operation removes.
    State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcOutcome {
    Deleted,
    Protected(Protection),
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcError {
    /// Refused or failed before commit; nothing changed.
    Store,
    /// The commit outcome is unknown; re-read before retrying.
    CompletionUnknown,
}

impl From<sqlx::Error> for GcError {
    fn from(_: sqlx::Error) -> Self {
        Self::Store
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupReport {
    pub generations: u64,
    pub leases: u64,
}

#[derive(Clone, Copy)]
enum Mode {
    /// A READY key that was published earlier and is no longer current.
    Retire,
    /// A never-published target whose full guard is gone or expired.
    Discard,
    /// The guard holder gives up its own live target.
    Abort { token: Uuid, fence: i64 },
}

/// One child or parent delete of the binding order.
const DELETE_ORDER: [&str; 11] = [
    "DELETE FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2",
    "DELETE FROM search_graph.build_guard WHERE source_id=$1 AND target_generation_id=$2",
    "DELETE FROM search_evaluation_lease WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_graph.participant WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_graph.relation WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_generation_payload WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_generation_receipt WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_lexical_artifact WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_graph.generation WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_generation WHERE source_id=$1 AND generation_id=$2",
];

fn remove_dir(path: &Path) {
    // Idempotent: a missing directory is already collected; anything left
    // over is an orphan for `sweep_orphan_dirs`.
    let _ = std::fs::remove_dir_all(path);
}

/// GC over the PostgreSQL rows and the lexical directories.
pub struct PgGenerationGc {
    pool: PgPool,
    lexical: LexicalArtifactStore,
}

impl PgGenerationGc {
    pub fn new(pool: PgPool, lexical_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            lexical: LexicalArtifactStore::new(lexical_root.into(), pool.clone()),
            pool,
        }
    }

    /// Removes a READY key that is not current and has no live pin.
    pub async fn retire_unpinned(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<GcOutcome, GcError> {
        self.collect(key, Mode::Retire).await
    }

    /// Removes a BUILDING or FAILED target whose full guard is not live.
    pub async fn discard_unpublished(
        &self,
        key: ProjectionGenerationKey,
    ) -> Result<GcOutcome, GcError> {
        self.collect(key, Mode::Discard).await
    }

    /// The holder of a live full guard abandons its target, e.g. after a lost CAS.
    pub async fn abort_manual(&self, handle: &ManualBuildHandle) -> Result<GcOutcome, GcError> {
        let build = &handle.0;
        self.collect(
            build.manifest.key(),
            Mode::Abort {
                token: build.token,
                fence: build.fence,
            },
        )
        .await
    }

    pub async fn abort_event(&self, handle: &EventCandidateHandle) -> Result<GcOutcome, GcError> {
        let build = &handle.build;
        self.collect(
            build.manifest.key(),
            Mode::Abort {
                token: build.token,
                fence: build.fence,
            },
        )
        .await
    }

    /// Discards never-published targets with no live full guard and removes
    /// expired pins, each target in its own transaction.
    pub async fn cleanup_expired(&self, limit: u32) -> Result<CleanupReport, GcError> {
        let targets = sqlx::query(
            "SELECT g.source_id, g.generation_id, g.state FROM search_generation g \
             JOIN search_source_coordination s USING (source_id) \
             WHERE s.current_generation_id IS DISTINCT FROM g.generation_id \
               AND (g.state IN ('BUILDING','FAILED') OR (g.state = 'READY' AND EXISTS ( \
                   SELECT 1 FROM search_generation_full_guard f \
                   WHERE f.source_id = g.source_id AND f.target_generation_id = g.generation_id))) \
               AND NOT EXISTS (SELECT 1 FROM search_generation_full_guard f \
                   WHERE f.source_id = g.source_id AND f.target_generation_id = g.generation_id \
                     AND f.expires_at > clock_timestamp()) \
             ORDER BY g.source_id, g.generation_id LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        let mut report = CleanupReport::default();
        for row in targets {
            let key = ProjectionGenerationKey {
                source_id: SourceId::from_uuid(row.try_get("source_id")?),
                generation_id: ProjectionGenerationId::from_uuid(row.try_get("generation_id")?),
            };
            // A READY row that still holds its full guard was never published.
            let mode = if row.try_get::<String, _>("state")? == "READY" {
                Mode::Retire
            } else {
                Mode::Discard
            };
            if self.collect(key, mode).await? == GcOutcome::Deleted {
                report.generations += 1;
            }
        }
        let leases = sqlx::query(
            "DELETE FROM search_evaluation_lease WHERE (source_id, lease_id) IN ( \
             SELECT source_id, lease_id FROM search_evaluation_lease \
             WHERE expires_at <= clock_timestamp() ORDER BY expires_at LIMIT $1)",
        )
        .bind(i64::from(limit))
        .execute(&self.pool)
        .await?;
        report.leases = leases.rows_affected();
        Ok(report)
    }

    /// Removes lexical directories of a Source whose generation row is gone.
    pub async fn sweep_orphan_dirs(&self, source: SourceId) -> Result<u64, GcError> {
        let parent = self
            .lexical
            .final_dir(ProjectionGenerationKey {
                source_id: source,
                generation_id: ProjectionGenerationId::from_uuid(Uuid::nil()),
            })
            .parent()
            .map(Path::to_path_buf)
            .ok_or(GcError::Store)?;
        let Ok(entries) = std::fs::read_dir(&parent) else {
            return Ok(0);
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let Some(generation) = entry
                .file_name()
                .to_str()
                .and_then(|name| Uuid::parse_str(name).ok())
            else {
                continue;
            };
            let exists: Option<i32> = sqlx::query_scalar(
                "SELECT 1 FROM search_generation WHERE source_id=$1 AND generation_id=$2",
            )
            .bind(source.as_uuid())
            .bind(generation)
            .fetch_optional(&self.pool)
            .await?;
            if exists.is_none() {
                remove_dir(&entry.path());
                removed += 1;
            }
        }
        Ok(removed)
    }

    async fn protection(
        connection: &mut PgConnection,
        key: ProjectionGenerationKey,
        mode: Mode,
    ) -> Result<Option<GcOutcome>, GcError> {
        let source = key.source_id.as_uuid();
        let generation = key.generation_id.as_uuid();
        let current: Option<Option<Uuid>> =
            sqlx::query_scalar("SELECT current_generation_id FROM search_gc_lock_source($1)")
                .bind(source)
                .fetch_optional(&mut *connection)
                .await?;
        let Some(current) = current else {
            return Ok(Some(GcOutcome::Missing));
        };
        if current == Some(generation) {
            return Ok(Some(GcOutcome::Protected(Protection::Current)));
        }
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM search_generation WHERE source_id=$1 AND generation_id=$2 \
             FOR UPDATE",
        )
        .bind(source)
        .bind(generation)
        .fetch_optional(&mut *connection)
        .await?;
        let Some(state) = state else {
            return Ok(Some(GcOutcome::Missing));
        };
        sqlx::query(
            "SELECT 1 FROM search_graph.generation WHERE source_id=$1 AND generation_id=$2 \
             FOR UPDATE",
        )
        .bind(source)
        .bind(generation)
        .execute(&mut *connection)
        .await?;
        let state_ok = match mode {
            Mode::Retire => state == "READY",
            Mode::Discard => state == "BUILDING" || state == "FAILED",
            Mode::Abort { .. } => state != "DELETING",
        };
        if !state_ok {
            return Ok(Some(GcOutcome::Protected(Protection::State)));
        }
        let guard = sqlx::query(
            "SELECT guard_token, build_fence, expires_at > clock_timestamp() AS live \
             FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2",
        )
        .bind(source)
        .bind(generation)
        .fetch_optional(&mut *connection)
        .await?;
        let guard = guard
            .map(|row| -> Result<(Uuid, i64, bool), sqlx::Error> {
                Ok((
                    row.try_get("guard_token")?,
                    row.try_get("build_fence")?,
                    row.try_get("live")?,
                ))
            })
            .transpose()?;
        let guarded = match mode {
            Mode::Abort { token, fence } => guard != Some((token, fence, true)),
            Mode::Retire | Mode::Discard => guard.is_some_and(|(_, _, live)| live),
        };
        if guarded {
            return Ok(Some(GcOutcome::Protected(Protection::Guarded)));
        }
        let graph = sqlx::query(
            "SELECT EXISTS (SELECT 1 FROM search_graph.build_guard \
                 WHERE source_id=$1 AND base_generation_id=$2) AS base, \
             EXISTS (SELECT 1 FROM search_graph.build_guard \
                 WHERE source_id=$1 AND target_generation_id=$2 \
                   AND expires_at > clock_timestamp()) AS target, \
             EXISTS (SELECT 1 FROM search_evaluation_lease \
                 WHERE source_id=$1 AND generation_id=$2 \
                   AND expires_at > clock_timestamp()) AS pinned",
        )
        .bind(source)
        .bind(generation)
        .fetch_one(&mut *connection)
        .await?;
        if graph.try_get::<bool, _>("base")? {
            return Ok(Some(GcOutcome::Protected(Protection::BaseOfBuild)));
        }
        if graph.try_get::<bool, _>("target")? {
            return Ok(Some(GcOutcome::Protected(Protection::Guarded)));
        }
        if graph.try_get::<bool, _>("pinned")? {
            return Ok(Some(GcOutcome::Protected(Protection::Pinned)));
        }
        Ok(None)
    }

    async fn collect(
        &self,
        key: ProjectionGenerationKey,
        mode: Mode,
    ) -> Result<GcOutcome, GcError> {
        let mut tx = self.pool.begin().await?;
        if let Some(outcome) = Self::protection(&mut tx, key, mode).await? {
            return Ok(outcome);
        }
        let source = key.source_id.as_uuid();
        let generation = key.generation_id.as_uuid();
        for statement in [
            "UPDATE search_generation SET state='DELETING' WHERE source_id=$1 AND generation_id=$2",
            "UPDATE search_graph.generation SET state='DELETING' \
             WHERE source_id=$1 AND generation_id=$2",
        ] {
            sqlx::query(statement)
                .bind(source)
                .bind(generation)
                .execute(&mut *tx)
                .await?;
        }
        for statement in DELETE_ORDER {
            sqlx::query(statement)
                .bind(source)
                .bind(generation)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await.map_err(|_| GcError::CompletionUnknown)?;
        remove_dir(&self.lexical.final_dir(key));
        remove_dir(&self.lexical.staging_dir(key));
        Ok(GcOutcome::Deleted)
    }
}
