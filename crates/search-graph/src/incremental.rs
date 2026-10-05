//! P3-G06 with the P3-C01 registration half: guarded incremental Graph build.
//! A target first copies its READY base under a Graph build guard, verifies
//! the whole copy against the frozen base receipt, then applies one
//! closure-checked delta. Every batch locks the base `FOR SHARE` and the
//! target `FOR UPDATE` in key order, then the guard; it re-checks token,
//! fence, DB-clock expiry, the base receipt and the committed cursor, and
//! re-checks expiry immediately before commit. A cursor is a committed DB
//! position: a stale in-memory cursor is refused. An unprovable closure
//! returns `RequiresFullRebuild` and leaves the target untouched.

use std::collections::BTreeSet;

use search_application::graph_generation::{
    BuildGuardHandle, ClosureBasis, GraphBatchCursor, GraphBatchPhase, GraphBuildRef,
    GraphGenerationReceipt, GraphIncrementalDelta,
};
use search_core::id::{RelationId, ResourceId};
use search_core::projection::ProjectionGenerationKey;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::GraphError;
use crate::canonical::{GRAPH_SCHEMA_VERSION, canonical_graph_digest};
use crate::store::{PostgresGraphStore, building_for, insert_rows, load_rows, parent};

/// Stored resource columns, copied verbatim from base to target.
macro_rules! resource_table_columns {
    () => {
        "resource_id,resource_kind,resource_version_id,mapping_kind,owner_document_id, \
         folder_id,version_id,adapter_id,native_id,valid_from_nanos,valid_from_offset, \
         valid_to_nanos,valid_to_offset,freshness_anchor_nanos,freshness_anchor_offset, \
         freshness_basis,effective_from_nanos,effective_from_offset,effective_to_nanos, \
         effective_to_offset"
    };
}

/// Registers the INCREMENTAL Graph parent and its build guard inside the
/// caller's P7 registration transaction, after checking the base receipt
/// under the base row lock. The target row is new in this transaction.
#[allow(clippy::too_many_arguments)]
pub async fn register_incremental_on(
    connection: &mut PgConnection,
    base: &GraphGenerationReceipt,
    target: ProjectionGenerationKey,
    target_snapshot: &str,
    target_manifest_digest: &str,
    target_mapping_digest: &str,
    guard_token: Uuid,
    build_fence: i64,
    ttl_micros: i64,
) -> Result<BuildGuardHandle, GraphError> {
    if base.key.source_id != target.source_id || base.key == target || ttl_micros <= 0 {
        return Err(GraphError::Invalid("incremental base and target"));
    }
    let stored = parent(&mut *connection, base.key, "FOR SHARE")
        .await?
        .ok_or(GraphError::FenceLost)?;
    let count = |n: u64| i64::try_from(n).ok();
    if stored.state != "READY"
        || stored.graph_content_digest.as_deref() != Some(base.graph_content_digest.as_str())
        || stored.resource_count != count(base.resource_count)
        || stored.relation_count != count(base.relation_count)
        || stored.source_snapshot != base.source_snapshot
        || stored.source_mapping_digest != base.source_mapping_digest
        || stored.projection_manifest_digest != base.projection_manifest_digest
    {
        return Err(GraphError::Integrity("base receipt"));
    }
    sqlx::query(
        "INSERT INTO search_graph.generation (source_id,generation_id,graph_schema_version, \
         build_kind,incremental_base_generation_id,build_guard_token,build_fence, \
         source_snapshot,projection_manifest_digest,source_mapping_digest,state) \
         VALUES ($1,$2,$3,'INCREMENTAL',$4,$5,$6,$7,$8,$9,'BUILDING')",
    )
    .bind(target.source_id.as_uuid())
    .bind(target.generation_id.as_uuid())
    .bind(GRAPH_SCHEMA_VERSION)
    .bind(base.key.generation_id.as_uuid())
    .bind(guard_token)
    .bind(build_fence)
    .bind(target_snapshot)
    .bind(target_manifest_digest)
    .bind(target_mapping_digest)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "INSERT INTO search_graph.build_guard (source_id,base_generation_id, \
         target_generation_id,guard_token,fence,base_manifest_digest, \
         base_graph_content_digest,base_source_snapshot,base_source_mapping_digest, \
         base_resource_count,base_relation_count,target_manifest_digest,expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12, \
                 clock_timestamp() + ($13::bigint * interval '1 microsecond'))",
    )
    .bind(target.source_id.as_uuid())
    .bind(base.key.generation_id.as_uuid())
    .bind(target.generation_id.as_uuid())
    .bind(guard_token)
    .bind(build_fence)
    .bind(&base.projection_manifest_digest)
    .bind(&base.graph_content_digest)
    .bind(&base.source_snapshot)
    .bind(&base.source_mapping_digest)
    .bind(count(base.resource_count).ok_or(GraphError::Invalid("count"))?)
    .bind(count(base.relation_count).ok_or(GraphError::Invalid("count"))?)
    .bind(target_manifest_digest)
    .bind(ttl_micros)
    .execute(&mut *connection)
    .await?;
    Ok(BuildGuardHandle::from_identifiers(
        base.key,
        target,
        guard_token,
        build_fence,
    ))
}

/// Guard row and committed cursor of one incremental target.
pub(crate) struct Gate {
    base_graph_content_digest: String,
    base_source_snapshot: String,
    resource_count: i64,
    relation_count: i64,
    copy_verified: bool,
    phase: Option<String>,
    sequence: i64,
    target_snapshot: String,
    target_mapping_digest: String,
}

/// Locks base then target in key order, then the guard, and checks every
/// binding of the handle against the rows.
pub(crate) async fn gate(
    connection: &mut PgConnection,
    handle: &BuildGuardHandle,
) -> Result<Gate, GraphError> {
    let (base, target) = (handle.base_key(), handle.target_key());
    if base.source_id != target.source_id {
        return Err(GraphError::FenceLost);
    }
    let (base_row, target_row) = if base.generation_id < target.generation_id {
        let base_row = parent(&mut *connection, base, "FOR SHARE").await?;
        (
            base_row,
            parent(&mut *connection, target, "FOR UPDATE").await?,
        )
    } else {
        let target_row = parent(&mut *connection, target, "FOR UPDATE").await?;
        (
            parent(&mut *connection, base, "FOR SHARE").await?,
            target_row,
        )
    };
    let base_row = base_row.ok_or(GraphError::FenceLost)?;
    let target_row = building_for(target_row, &GraphBuildRef::Incremental(*handle))?;
    let guard = sqlx::query(
        "SELECT guard_token, fence, base_generation_id, base_manifest_digest, \
         base_graph_content_digest, base_source_snapshot, base_source_mapping_digest, \
         base_resource_count, base_relation_count, \
         expires_at > clock_timestamp() AS live, copy_verified_at IS NOT NULL AS verified \
         FROM search_graph.build_guard WHERE source_id=$1 AND target_generation_id=$2 \
         FOR UPDATE",
    )
    .bind(target.source_id.as_uuid())
    .bind(target.generation_id.as_uuid())
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GraphError::FenceLost)?;
    let digest: String = guard.try_get("base_graph_content_digest")?;
    let resource_count: i64 = guard.try_get("base_resource_count")?;
    let relation_count: i64 = guard.try_get("base_relation_count")?;
    if guard.try_get::<Uuid, _>("guard_token")? != handle.guard_token()
        || guard.try_get::<i64, _>("fence")? != handle.build_fence()
        || guard.try_get::<Uuid, _>("base_generation_id")? != base.generation_id.as_uuid()
        || !guard.try_get::<bool, _>("live")?
    {
        return Err(GraphError::FenceLost);
    }
    if base_row.state != "READY"
        || base_row.graph_content_digest.as_deref() != Some(digest.as_str())
        || base_row.resource_count != Some(resource_count)
        || base_row.relation_count != Some(relation_count)
        || base_row.source_snapshot != guard.try_get::<String, _>("base_source_snapshot")?
        || base_row.source_mapping_digest
            != guard.try_get::<String, _>("base_source_mapping_digest")?
        || base_row.projection_manifest_digest
            != guard.try_get::<String, _>("base_manifest_digest")?
    {
        return Err(GraphError::Integrity("base receipt"));
    }
    let cursor = sqlx::query(
        "SELECT batch_phase, batch_sequence FROM search_graph.generation \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(target.source_id.as_uuid())
    .bind(target.generation_id.as_uuid())
    .fetch_one(&mut *connection)
    .await?;
    Ok(Gate {
        base_graph_content_digest: digest,
        base_source_snapshot: base_row.source_snapshot,
        resource_count,
        relation_count,
        copy_verified: guard.try_get("verified")?,
        phase: cursor.try_get("batch_phase")?,
        sequence: cursor.try_get("batch_sequence")?,
        target_snapshot: target_row.source_snapshot,
        target_mapping_digest: target_row.source_mapping_digest,
    })
}

/// The DB clock re-check immediately before commit.
async fn still_live(
    connection: &mut PgConnection,
    handle: &BuildGuardHandle,
) -> Result<(), GraphError> {
    let live: Option<bool> = sqlx::query_scalar(
        "SELECT expires_at > clock_timestamp() FROM search_graph.build_guard \
         WHERE source_id=$1 AND target_generation_id=$2 AND guard_token=$3 AND fence=$4",
    )
    .bind(handle.target_key().source_id.as_uuid())
    .bind(handle.target_key().generation_id.as_uuid())
    .bind(handle.guard_token())
    .bind(handle.build_fence())
    .fetch_optional(&mut *connection)
    .await?;
    if live != Some(true) {
        return Err(GraphError::FenceLost);
    }
    Ok(())
}

async fn set_cursor(
    connection: &mut PgConnection,
    target: ProjectionGenerationKey,
    phase: &str,
    sequence: i64,
) -> Result<(), GraphError> {
    sqlx::query(
        "UPDATE search_graph.generation SET batch_phase=$3, batch_sequence=$4 \
         WHERE source_id=$1 AND generation_id=$2",
    )
    .bind(target.source_id.as_uuid())
    .bind(target.generation_id.as_uuid())
    .bind(phase)
    .bind(sequence)
    .execute(connection)
    .await?;
    Ok(())
}

/// The delta's own consistency and its closure proof against the copied base.
async fn closure(
    connection: &mut PgConnection,
    target: ProjectionGenerationKey,
    gate: &Gate,
    delta: &GraphIncrementalDelta,
) -> Result<(BTreeSet<ResourceId>, BTreeSet<RelationId>), GraphError> {
    let proof = &delta.proof;
    let changed_stream = matches!(&proof.basis, ClosureBasis::AuthoritativeChangeStream { cursor } if cursor.trim().is_empty());
    if proof.base_snapshot != gate.base_source_snapshot
        || proof.target_snapshot != gate.target_snapshot
        || changed_stream
    {
        return Err(GraphError::RequiresFullRebuild);
    }
    let affected: BTreeSet<ResourceId> = delta
        .changed_resources
        .iter()
        .map(|record| record.resource_ref)
        .chain(delta.retired_resources.iter().copied())
        .collect();
    let replacement: BTreeSet<RelationId> = delta
        .replacement_relations
        .iter()
        .map(|relation| relation.relation_id)
        .collect();
    let changed: BTreeSet<RelationId> = delta.changed_relation_ids.iter().copied().collect();
    let retired: BTreeSet<RelationId> = delta.retired_relation_ids.iter().copied().collect();
    if affected.len() != delta.changed_resources.len() + delta.retired_resources.len()
        || replacement.len() != delta.replacement_relations.len()
        || proof
            .affected_resources
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            != affected
        || proof
            .new_relation_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            != replacement
        || !changed.is_subset(&replacement)
        || !retired.is_disjoint(&replacement)
    {
        return Err(GraphError::RequiresFullRebuild);
    }
    let incident: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT relation_id FROM search_graph.participant \
         WHERE source_id=$1 AND generation_id=$2 AND resource_id=ANY($3)",
    )
    .bind(target.source_id.as_uuid())
    .bind(target.generation_id.as_uuid())
    .bind(affected.iter().map(|id| id.as_uuid()).collect::<Vec<_>>())
    .fetch_all(&mut *connection)
    .await?;
    let old: BTreeSet<RelationId> = proof.old_relation_ids.iter().copied().collect();
    let required: BTreeSet<RelationId> = incident
        .into_iter()
        .map(RelationId::from_uuid)
        .chain(changed.iter().copied())
        .chain(retired.iter().copied())
        .collect();
    if !required.is_subset(&old) {
        return Err(GraphError::RequiresFullRebuild);
    }
    // Every old relation and every replacement ID present in the copy goes.
    Ok((affected, old.union(&replacement).copied().collect()))
}

impl PostgresGraphStore {
    /// Copies the next `limit` base rows (Resources, then relations with
    /// their participants, in key order) after the committed cursor.
    pub async fn copy_batch(
        &self,
        handle: &BuildGuardHandle,
        expected: &GraphBatchCursor,
        limit: u32,
    ) -> Result<GraphBatchCursor, GraphError> {
        let target = handle.target_key();
        if expected.target_key != target || expected.phase != GraphBatchPhase::Copy || limit == 0 {
            return Err(GraphError::Invalid("copy cursor"));
        }
        let mut tx = self.pool().begin().await?;
        let gate = gate(&mut tx, handle).await?;
        let committed = match gate.phase.as_deref() {
            None if gate.sequence == 0 => 0,
            Some("COPY") => gate.sequence,
            _ => return Err(GraphError::FenceLost),
        };
        if gate.copy_verified || u64::try_from(committed).ok() != Some(expected.committed_sequence)
        {
            return Err(GraphError::FenceLost);
        }
        let total = gate.resource_count + gate.relation_count;
        let end = committed.saturating_add(i64::from(limit)).min(total);
        let source = target.source_id.as_uuid();
        let base = handle.base_key().generation_id.as_uuid();
        let target_id = target.generation_id.as_uuid();
        if committed < gate.resource_count {
            sqlx::query(concat!(
                "INSERT INTO search_graph.resource (source_id,generation_id,",
                resource_table_columns!(),
                ") SELECT source_id,$3,",
                resource_table_columns!(),
                " FROM search_graph.resource WHERE source_id=$1 AND generation_id=$2 \
                 ORDER BY resource_id OFFSET $4 LIMIT $5"
            ))
            .bind(source)
            .bind(base)
            .bind(target_id)
            .bind(committed)
            .bind(end.min(gate.resource_count) - committed)
            .execute(&mut *tx)
            .await?;
        }
        if end > gate.resource_count {
            let from = committed.max(gate.resource_count) - gate.resource_count;
            let copied: Vec<Uuid> = sqlx::query_scalar(
                "INSERT INTO search_graph.relation (source_id,generation_id,relation_id, \
                 namespace,relation_type,payload,canonical_digest) \
                 SELECT source_id,$3,relation_id,namespace,relation_type,payload, \
                 canonical_digest FROM search_graph.relation \
                 WHERE source_id=$1 AND generation_id=$2 \
                 ORDER BY relation_id OFFSET $4 LIMIT $5 RETURNING relation_id",
            )
            .bind(source)
            .bind(base)
            .bind(target_id)
            .bind(from)
            .bind(end - gate.resource_count - from)
            .fetch_all(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO search_graph.participant (source_id,generation_id,relation_id, \
                 ordinal,role,resource_id) SELECT source_id,$3,relation_id,ordinal,role, \
                 resource_id FROM search_graph.participant \
                 WHERE source_id=$1 AND generation_id=$2 AND relation_id=ANY($4)",
            )
            .bind(source)
            .bind(base)
            .bind(target_id)
            .bind(&copied)
            .execute(&mut *tx)
            .await?;
        }
        set_cursor(&mut tx, target, "COPY", end).await?;
        still_live(&mut tx, handle).await?;
        tx.commit().await?;
        Ok(GraphBatchCursor {
            target_key: target,
            phase: GraphBatchPhase::Copy,
            committed_sequence: u64::try_from(end).map_err(|_| GraphError::Invalid("cursor"))?,
        })
    }

    /// Before any delta: the whole copied target must equal the base receipt.
    pub async fn verify_copy(&self, handle: &BuildGuardHandle) -> Result<(), GraphError> {
        let target = handle.target_key();
        let mut tx = self.pool().begin().await?;
        let gate = gate(&mut tx, handle).await?;
        let total = gate.resource_count + gate.relation_count;
        let complete = match gate.phase.as_deref() {
            None => total == 0 && gate.sequence == 0,
            Some("COPY") => gate.sequence == total,
            _ => false,
        };
        if gate.copy_verified || !complete {
            return Err(GraphError::FenceLost);
        }
        let (resources, relations) = load_rows(&mut tx, target).await?;
        let digest = canonical_graph_digest(
            target.source_id,
            GRAPH_SCHEMA_VERSION,
            &resources,
            &relations,
        )
        .map_err(|_| GraphError::Integrity("copied content"))?;
        if digest != gate.base_graph_content_digest
            || i64::try_from(resources.len()).ok() != Some(gate.resource_count)
            || i64::try_from(relations.len()).ok() != Some(gate.relation_count)
        {
            return Err(GraphError::Integrity("copy differs from base"));
        }
        sqlx::query(
            "UPDATE search_graph.build_guard SET copy_verified_at = clock_timestamp() \
             WHERE source_id=$1 AND target_generation_id=$2",
        )
        .bind(target.source_id.as_uuid())
        .bind(target.generation_id.as_uuid())
        .execute(&mut *tx)
        .await?;
        set_cursor(&mut tx, target, "DELTA", 0).await?;
        still_live(&mut tx, handle).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Applies one whole delta after a verified copy: every affected
    /// participant row, then relation row, then Resource row goes, and the
    /// changed Resources and all replacement relations are written.
    pub async fn apply_delta_batch(
        &self,
        handle: &BuildGuardHandle,
        delta: &GraphIncrementalDelta,
        expected: &GraphBatchCursor,
        limit: u32,
    ) -> Result<GraphBatchCursor, GraphError> {
        let target = handle.target_key();
        let size = delta.changed_resources.len()
            + delta.retired_resources.len()
            + delta.changed_relation_ids.len()
            + delta.retired_relation_ids.len()
            + delta.replacement_relations.len();
        if expected.target_key != target
            || expected.phase != GraphBatchPhase::Delta
            || expected.committed_sequence != 0
            || size > usize::try_from(limit).unwrap_or(usize::MAX)
        {
            return Err(GraphError::Invalid("delta cursor or batch limit"));
        }
        for relation in &delta.replacement_relations {
            relation
                .validate()
                .map_err(|_| GraphError::Invalid("relation"))?;
        }
        let mut tx = self.pool().begin().await?;
        let gate = gate(&mut tx, handle).await?;
        if !gate.copy_verified || gate.phase.as_deref() != Some("DELTA") || gate.sequence != 0 {
            return Err(GraphError::FenceLost);
        }
        if delta.target_source_mapping_digest != gate.target_mapping_digest {
            return Err(GraphError::Integrity("target mapping commitment"));
        }
        let (affected, removed) = closure(&mut tx, target, &gate, delta).await?;
        let source = target.source_id.as_uuid();
        let target_id = target.generation_id.as_uuid();
        let removed: Vec<Uuid> = removed.iter().map(|id| id.as_uuid()).collect();
        let affected: Vec<Uuid> = affected.iter().map(|id| id.as_uuid()).collect();
        for statement in [
            "DELETE FROM search_graph.participant \
             WHERE source_id=$1 AND generation_id=$2 AND relation_id=ANY($3)",
            "DELETE FROM search_graph.relation \
             WHERE source_id=$1 AND generation_id=$2 AND relation_id=ANY($3)",
        ] {
            sqlx::query(statement)
                .bind(source)
                .bind(target_id)
                .bind(&removed)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(
            "DELETE FROM search_graph.resource \
             WHERE source_id=$1 AND generation_id=$2 AND resource_id=ANY($3)",
        )
        .bind(source)
        .bind(target_id)
        .bind(&affected)
        .execute(&mut *tx)
        .await
        .map_err(|error| match GraphError::from(error) {
            // A surviving relation still names a removed Resource.
            GraphError::Integrity(_) => GraphError::RequiresFullRebuild,
            other => other,
        })?;
        insert_rows(
            &mut tx,
            target,
            &delta.changed_resources,
            &delta.replacement_relations,
        )
        .await?;
        set_cursor(&mut tx, target, "DELTA", 1).await?;
        still_live(&mut tx, handle).await?;
        tx.commit().await?;
        Ok(GraphBatchCursor {
            target_key: target,
            phase: GraphBatchPhase::Delta,
            committed_sequence: 1,
        })
    }
}

/// READY validation of an incremental target additionally needs a live guard,
/// a verified copy and the applied delta.
pub(crate) async fn ready_gate(
    connection: &mut PgConnection,
    handle: &BuildGuardHandle,
) -> Result<(), GraphError> {
    let gate = gate(connection, handle).await?;
    if !gate.copy_verified || gate.phase.as_deref() != Some("DELTA") || gate.sequence != 1 {
        return Err(GraphError::FenceLost);
    }
    Ok(())
}
