//! P7-09: event-origin publication, current reuse and manual CAS.
//!
//! `PublishCandidate` locks outbox → Source → candidate generation → full
//! guard, re-reads the event, Source epoch, READY generation, its two digests
//! and the expected pointer from rows, and commits the pointer CAS, the Search
//! event receipt and the guard DELETE together. `ReuseCurrent` takes no
//! candidate and only records a receipt for a still-current READY bundle.
//! Manual publication starts at the Source row and never acknowledges events.
//! A lost CAS keeps the guard; the generic outbox ack stays with the delivery
//! worker in its own fenced transaction.

use search_application::ports::{
    CurrentGenerationSnapshot, SearchCompletionOutcome, SearchDeliveryFence,
};
use search_application::search_core::id::{ProjectionGenerationId, SourceId};
use search_application::search_core::projection::ProjectionGenerationKey;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::full_guard::{EventCandidateHandle, ManualBuildHandle};
use crate::ready::VerifiedBundle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionError {
    /// The database refused or failed before commit; nothing was published.
    StoreUnknown,
    /// The commit outcome is unknown; re-read current and receipts.
    CompletionUnknown,
}

impl From<sqlx::Error> for CompletionError {
    fn from(_: sqlx::Error) -> Self {
        Self::StoreUnknown
    }
}

pub enum EventCompletion<'a> {
    PublishCandidate {
        fence: SearchDeliveryFence,
        expected_current: CurrentGenerationSnapshot,
        candidate: &'a EventCandidateHandle,
        bundle: &'a VerifiedBundle,
    },
    ReuseCurrent {
        fence: SearchDeliveryFence,
        expected_current: CurrentGenerationSnapshot,
        expected_manifest_digest: String,
        expected_bundle_digest: String,
    },
}

fn sha256_text(digest: &[u8; 32]) -> String {
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

struct SourceRow {
    current: CurrentGenerationSnapshot,
    last_published_epoch: i64,
}

async fn lock_source(
    connection: &mut PgConnection,
    source: SourceId,
    fence: Option<SearchDeliveryFence>,
) -> Result<Option<SourceRow>, CompletionError> {
    let Some(row) = sqlx::query(
        "SELECT current_generation_id, current_manifest_digest, current_bundle_digest, \
         pointer_revision, last_published_epoch, owner_token, fence_epoch, \
         lease_expires_at > clock_timestamp() AS live, registration_active \
         FROM search_source_coordination WHERE source_id=$1 FOR UPDATE",
    )
    .bind(source.as_uuid())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    if !row.try_get::<bool, _>("registration_active")? {
        return Ok(None);
    }
    if let Some(fence) = fence
        && (row.try_get::<Option<Uuid>, _>("owner_token")? != Some(fence.source.owner_token)
            || row.try_get::<i64, _>("fence_epoch")? != fence.source.epoch
            || row.try_get::<Option<bool>, _>("live")? != Some(true))
    {
        return Ok(None);
    }
    Ok(Some(SourceRow {
        current: CurrentGenerationSnapshot {
            key: row
                .try_get::<Option<Uuid>, _>("current_generation_id")?
                .map(|generation| ProjectionGenerationKey {
                    source_id: source,
                    generation_id: ProjectionGenerationId::from_uuid(generation),
                }),
            manifest_digest: row.try_get("current_manifest_digest")?,
            bundle_digest: row.try_get("current_bundle_digest")?,
            pointer_revision: row.try_get("pointer_revision")?,
        },
        last_published_epoch: row.try_get("last_published_epoch")?,
    }))
}

/// The live, unexpired outbox lease of the event. Returns false otherwise.
async fn lock_outbox(
    connection: &mut PgConnection,
    fence: SearchDeliveryFence,
) -> Result<bool, CompletionError> {
    Ok(sqlx::query(
        "SELECT event_id FROM outbox_events WHERE event_id=$1 AND lease_token=$2 \
         AND lease_expires_at > clock_timestamp() AND delivered_at IS NULL \
         AND dead_lettered_at IS NULL FOR UPDATE",
    )
    .bind(fence.event_id)
    .bind(fence.outbox_token)
    .fetch_optional(connection)
    .await?
    .is_some())
}

async fn existing_receipt(
    connection: &mut PgConnection,
    fence: SearchDeliveryFence,
) -> Result<Option<Uuid>, CompletionError> {
    Ok(sqlx::query_scalar(
        "SELECT generation_id FROM search_index_receipts WHERE source_id=$1 AND event_id=$2",
    )
    .bind(fence.source.source_id.as_uuid())
    .bind(fence.event_id)
    .fetch_optional(connection)
    .await?)
}

/// The READY generation, its stored receipt and its Graph are exactly the
/// expected digests. Origin, event and token checks are the caller's.
async fn ready_bundle_matches(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    manifest_digest: &str,
    bundle_digest: &str,
) -> Result<bool, CompletionError> {
    let row = sqlx::query(
        "SELECT g.state, g.projection_manifest_digest, r.composite_digest, gg.state AS graph_state \
         FROM search_generation g \
         JOIN search_generation_receipt r USING (source_id, generation_id) \
         JOIN search_graph.generation gg USING (source_id, generation_id) \
         WHERE g.source_id=$1 AND g.generation_id=$2 FOR UPDATE OF g",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    Ok(row.try_get::<String, _>("state")? == "READY"
        && row.try_get::<String, _>("graph_state")? == "READY"
        && row.try_get::<String, _>("projection_manifest_digest")? == manifest_digest
        && row.try_get::<String, _>("composite_digest")? == bundle_digest)
}

/// The candidate's stored origin, event, epoch and guard binding.
async fn candidate_binding(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    token: Uuid,
    fence_seq: i64,
    event: Option<SearchDeliveryFence>,
) -> Result<bool, CompletionError> {
    let Some(row) = sqlx::query(
        "SELECT stage_origin, stage_event_id, stage_source_epoch, full_guard_token, \
         full_build_fence FROM search_generation WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(false);
    };
    let origin_ok = match event {
        Some(fence) => {
            row.try_get::<String, _>("stage_origin")? == "EVENT"
                && row.try_get::<Option<Uuid>, _>("stage_event_id")? == Some(fence.event_id)
                && row.try_get::<Option<i64>, _>("stage_source_epoch")? == Some(fence.source.epoch)
        }
        None => row.try_get::<String, _>("stage_origin")? == "MANUAL",
    };
    let guard_live: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM search_generation_full_guard WHERE source_id=$1 \
         AND target_generation_id=$2 AND guard_token=$3 AND build_fence=$4 \
         AND expires_at > clock_timestamp())",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(token)
    .bind(fence_seq)
    .fetch_one(&mut *connection)
    .await?;
    Ok(origin_ok
        && row.try_get::<Option<Uuid>, _>("full_guard_token")? == Some(token)
        && row.try_get::<Option<i64>, _>("full_build_fence")? == Some(fence_seq)
        && guard_live)
}

async fn swap_pointer(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    expected_revision: i64,
    manifest_digest: &str,
    bundle_digest: &str,
    published_epoch: Option<i64>,
) -> Result<bool, CompletionError> {
    let updated = sqlx::query(
        "UPDATE search_source_coordination SET current_generation_id=$2, \
         current_manifest_digest=$3, current_bundle_digest=$4, \
         pointer_revision=pointer_revision+1, \
         last_published_epoch=COALESCE($6, last_published_epoch) \
         WHERE source_id=$1 AND pointer_revision=$5",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(manifest_digest)
    .bind(bundle_digest)
    .bind(expected_revision)
    .bind(published_epoch)
    .execute(connection)
    .await?;
    Ok(updated.rows_affected() == 1)
}

async fn delete_guard(
    connection: &mut PgConnection,
    key: ProjectionGenerationKey,
    token: Uuid,
) -> Result<(), CompletionError> {
    sqlx::query(
        "DELETE FROM search_generation_full_guard WHERE source_id=$1 \
         AND target_generation_id=$2 AND guard_token=$3",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .bind(token)
    .execute(connection)
    .await?;
    Ok(())
}

async fn record_receipt(
    connection: &mut PgConnection,
    fence: SearchDeliveryFence,
    key: ProjectionGenerationKey,
    manifest_digest: &str,
    bundle_digest: &str,
) -> Result<(), CompletionError> {
    sqlx::query(
        "INSERT INTO search_index_receipts (source_id,event_id,generation_id,digest, \
         bundle_digest,fence_epoch,recorded_at,bundle_version) \
         VALUES ($1,$2,$3,$4,$5,$6,clock_timestamp(),'v1')",
    )
    .bind(key.source_id.as_uuid())
    .bind(fence.event_id)
    .bind(key.generation_id.as_uuid())
    .bind(manifest_digest)
    .bind(bundle_digest)
    .bind(fence.source.epoch)
    .execute(connection)
    .await?;
    Ok(())
}

async fn commit(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    outcome: SearchCompletionOutcome,
) -> Result<SearchCompletionOutcome, CompletionError> {
    tx.commit()
        .await
        .map_err(|_| CompletionError::CompletionUnknown)?;
    Ok(outcome)
}

/// Pointer CAS and Search event receipts on the shared Source row.
#[derive(Clone)]
pub struct PgPublication {
    pool: PgPool,
}

impl PgPublication {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn current(
        &self,
        source: SourceId,
    ) -> Result<CurrentGenerationSnapshot, CompletionError> {
        let mut tx = self.pool.begin().await?;
        let row = lock_source(&mut tx, source, None)
            .await?
            .ok_or(CompletionError::StoreUnknown)?;
        tx.rollback().await?;
        Ok(row.current)
    }

    pub async fn complete_event(
        &self,
        request: EventCompletion<'_>,
    ) -> Result<SearchCompletionOutcome, CompletionError> {
        let fence = match &request {
            EventCompletion::PublishCandidate { fence, .. }
            | EventCompletion::ReuseCurrent { fence, .. } => *fence,
        };
        let mut tx = self.pool.begin().await?;
        if !lock_outbox(&mut tx, fence).await? {
            return Ok(SearchCompletionOutcome::Lost);
        }
        if let Some(generation) = existing_receipt(&mut tx, fence).await? {
            let key = ProjectionGenerationKey {
                source_id: fence.source.source_id,
                generation_id: ProjectionGenerationId::from_uuid(generation),
            };
            return Ok(SearchCompletionOutcome::Duplicate(key));
        }
        let Some(source) = lock_source(&mut tx, fence.source.source_id, Some(fence)).await? else {
            return Ok(SearchCompletionOutcome::Lost);
        };
        if source.last_published_epoch > fence.source.epoch {
            return Ok(SearchCompletionOutcome::Lost);
        }
        match request {
            EventCompletion::PublishCandidate {
                expected_current,
                candidate,
                bundle,
                ..
            } => {
                if source.current != expected_current {
                    // A lost CAS keeps the guard for an explicit abort or retry.
                    return Ok(SearchCompletionOutcome::Retry);
                }
                let key = candidate.key();
                let target = candidate
                    .graph_target()
                    .ok_or(CompletionError::StoreUnknown)?;
                let manifest_digest = sha256_text(&bundle.receipt().projection_digest);
                let bundle_digest = sha256_text(&bundle.receipt().composite_digest);
                if bundle.key() != key
                    || key.source_id != fence.source.source_id
                    || !ready_bundle_matches(&mut tx, key, &manifest_digest, &bundle_digest).await?
                    || !candidate_binding(
                        &mut tx,
                        key,
                        target.guard_token(),
                        target.build_fence(),
                        Some(fence),
                    )
                    .await?
                {
                    return Ok(SearchCompletionOutcome::Lost);
                }
                if !swap_pointer(
                    &mut tx,
                    key,
                    expected_current.pointer_revision,
                    &manifest_digest,
                    &bundle_digest,
                    Some(fence.source.epoch),
                )
                .await?
                {
                    return Ok(SearchCompletionOutcome::Retry);
                }
                record_receipt(&mut tx, fence, key, &manifest_digest, &bundle_digest).await?;
                delete_guard(&mut tx, key, target.guard_token()).await?;
                commit(tx, SearchCompletionOutcome::Published(key)).await
            }
            EventCompletion::ReuseCurrent {
                expected_current,
                expected_manifest_digest,
                expected_bundle_digest,
                ..
            } => {
                let Some(key) = source.current.key else {
                    return Ok(SearchCompletionOutcome::Retry);
                };
                if source.current != expected_current
                    || source.current.manifest_digest.as_deref() != Some(&expected_manifest_digest)
                    || source.current.bundle_digest.as_deref() != Some(&expected_bundle_digest)
                    || !ready_bundle_matches(
                        &mut tx,
                        key,
                        &expected_manifest_digest,
                        &expected_bundle_digest,
                    )
                    .await?
                {
                    return Ok(SearchCompletionOutcome::Retry);
                }
                record_receipt(
                    &mut tx,
                    fence,
                    key,
                    &expected_manifest_digest,
                    &expected_bundle_digest,
                )
                .await?;
                commit(tx, SearchCompletionOutcome::Unchanged(key)).await
            }
        }
    }

    /// Manual publication: Source → generation → guard, CAS and guard DELETE.
    /// No event is acknowledged or recorded.
    pub async fn publish_manual(
        &self,
        handle: &ManualBuildHandle,
        bundle: &VerifiedBundle,
        expected_current: &CurrentGenerationSnapshot,
    ) -> Result<SearchCompletionOutcome, CompletionError> {
        let key = handle.key();
        let target = handle.graph_target().ok_or(CompletionError::StoreUnknown)?;
        let manifest_digest = sha256_text(&bundle.receipt().projection_digest);
        let bundle_digest = sha256_text(&bundle.receipt().composite_digest);
        let mut tx = self.pool.begin().await?;
        let Some(source) = lock_source(&mut tx, key.source_id, None).await? else {
            return Ok(SearchCompletionOutcome::Lost);
        };
        if source.current != *expected_current {
            return Ok(SearchCompletionOutcome::Retry);
        }
        if bundle.key() != key
            || !ready_bundle_matches(&mut tx, key, &manifest_digest, &bundle_digest).await?
            || !candidate_binding(
                &mut tx,
                key,
                target.guard_token(),
                target.build_fence(),
                None,
            )
            .await?
        {
            return Ok(SearchCompletionOutcome::Lost);
        }
        if !swap_pointer(
            &mut tx,
            key,
            expected_current.pointer_revision,
            &manifest_digest,
            &bundle_digest,
            None,
        )
        .await?
        {
            return Ok(SearchCompletionOutcome::Retry);
        }
        delete_guard(&mut tx, key, target.guard_token()).await?;
        commit(tx, SearchCompletionOutcome::Published(key)).await
    }
}
