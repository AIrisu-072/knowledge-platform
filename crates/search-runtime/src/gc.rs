//! P7-11 with P3-C02/C03: guarded GC and FK-safe cleanup.
//!
//! Every call locks the Source row (through a lock-only definer function),
//! then the P7 and Graph generation rows. It refuses the current key, a key
//! with a live pin, a live full guard, a live Graph build guard on the target,
//! or any Graph build guard that uses the key as its base. Otherwise, in one
//! transaction: DELETING → guard DELETE → lease DELETE → Graph participant,
//! relation and resource → P7 children → Graph and P7 parents. The permanent
//! identity, guard issuance and historical Search receipts stay, so an expired
//! target never receives a guard again. Guard rows are not locked: guards are
//! never renewed. A lease can be renewed across its expiry while GC runs, so
//! the lease DELETE re-checks expiry against the row version it locks, and a
//! lease that survives it rolls the whole collection back as
//! `Protected(Pinned)`. A new lease needs a current key.
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
    /// The holder of a live Graph build guard gives up its incremental target.
    AbortIncremental { base: Uuid, token: Uuid, fence: i64 },
}

/// One child or parent delete of the binding order.
const DELETE_ORDER: [&str; 12] = [
    "DELETE FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2",
    "DELETE FROM search_graph.build_guard WHERE source_id=$1 AND target_generation_id=$2",
    "DELETE FROM search_evaluation_lease WHERE source_id=$1 AND generation_id=$2 \
     AND expires_at <= clock_timestamp()",
    "DELETE FROM search_graph.participant WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_graph.relation WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_generation_payload WHERE source_id=$1 AND generation_id=$2",
    "DELETE FROM search_generation_segment WHERE source_id=$1 AND generation_id=$2",
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

    /// Retires READY generations of `source_id` older than the current one and
    /// the `keep_previous` newest before it. A pinned generation is skipped
    /// and retried at the next publication. Returns the number deleted.
    pub async fn retire_superseded(
        &self,
        source_id: SourceId,
        keep_previous: usize,
    ) -> Result<u64, GcError> {
        let rows: Vec<Uuid> = sqlx::query_scalar(
            "SELECT g.generation_id FROM search_generation g \
             JOIN search_source_coordination c ON c.source_id = g.source_id \
             WHERE g.source_id = $1 AND g.state = 'READY' \
             AND g.generation_id IS DISTINCT FROM c.current_generation_id \
             ORDER BY g.ready_at DESC NULLS LAST OFFSET $2",
        )
        .bind(source_id.as_uuid())
        .bind(i64::try_from(keep_previous).map_err(|_| GcError::Store)?)
        .fetch_all(&self.pool)
        .await?;
        let mut deleted = 0;
        for generation in rows {
            let key = ProjectionGenerationKey {
                source_id,
                generation_id: ProjectionGenerationId::from_uuid(generation),
            };
            if self.retire_unpinned(key).await? == GcOutcome::Deleted {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Deletes Unit segments that no generation lists any more. Returns the
    /// number of deleted segments.
    pub async fn sweep_unreferenced_segments(&self) -> Result<u64, GcError> {
        let deleted = sqlx::query(
            "DELETE FROM search_unit_segment s WHERE NOT EXISTS \
             (SELECT 1 FROM search_generation_segment g WHERE g.segment_digest = s.segment_digest)",
        )
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
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

    /// The holder of a live Graph build guard abandons its incremental target;
    /// the base becomes collectable in the same commit.
    pub async fn abort_incremental(
        &self,
        handle: &search_application::graph_generation::BuildGuardHandle,
    ) -> Result<GcOutcome, GcError> {
        self.collect(
            handle.target_key(),
            Mode::AbortIncremental {
                base: handle.base_key().generation_id.as_uuid(),
                token: handle.guard_token(),
                fence: handle.build_fence(),
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
             WHERE expires_at <= clock_timestamp() ORDER BY expires_at LIMIT $1) \
             AND expires_at <= clock_timestamp()",
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
            Mode::Abort { .. } | Mode::AbortIncremental { .. } => state != "DELETING",
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
            Mode::AbortIncremental { .. } => guard.is_some(),
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
             EXISTS (SELECT 1 FROM search_graph.build_guard \
                 WHERE source_id=$1 AND target_generation_id=$2 \
                   AND base_generation_id=$3 AND guard_token=$4 AND fence=$5 \
                   AND expires_at > clock_timestamp()) AS own, \
             EXISTS (SELECT 1 FROM search_evaluation_lease \
                 WHERE source_id=$1 AND generation_id=$2 \
                   AND expires_at > clock_timestamp()) AS pinned",
        )
        .bind(source)
        .bind(generation)
        .bind(match mode {
            Mode::AbortIncremental { base, .. } => Some(base),
            _ => None,
        })
        .bind(match mode {
            Mode::AbortIncremental { token, .. } => Some(token),
            _ => None,
        })
        .bind(match mode {
            Mode::AbortIncremental { fence, .. } => Some(fence),
            _ => None,
        })
        .fetch_one(&mut *connection)
        .await?;
        if graph.try_get::<bool, _>("base")? {
            return Ok(Some(GcOutcome::Protected(Protection::BaseOfBuild)));
        }
        let own = graph.try_get::<bool, _>("own")?;
        let guarded = match mode {
            Mode::AbortIncremental { .. } => !own,
            _ => graph.try_get::<bool, _>("target")?,
        };
        if guarded {
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
            // A renew that read its lease as live before expiry can commit
            // after the pinned check; the expiry-checked DELETE then keeps
            // the renewed row, and its presence rolls everything back.
            if statement.starts_with("DELETE FROM search_evaluation_lease") {
                let renewed: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM search_evaluation_lease \
                     WHERE source_id=$1 AND generation_id=$2)",
                )
                .bind(source)
                .bind(generation)
                .fetch_one(&mut *tx)
                .await?;
                if renewed {
                    return Ok(GcOutcome::Protected(Protection::Pinned));
                }
            }
        }
        tx.commit().await.map_err(|_| GcError::CompletionUnknown)?;
        remove_dir(&self.lexical.final_dir(key));
        remove_dir(&self.lexical.staging_dir(key));
        // Best effort: a segment a concurrent build has just listed keeps its
        // row (the foreign key refuses the delete) and is swept later.
        let _ = self.sweep_unreferenced_segments().await;
        Ok(GcOutcome::Deleted)
    }
}
