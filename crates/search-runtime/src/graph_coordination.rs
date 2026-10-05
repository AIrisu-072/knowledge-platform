//! P3-C01: incremental Graph build registration on the one Source row.
//!
//! Registration locks the Source row, checks the READY base under its row
//! lock, increments the Source `build_fence_seq` shared with FULL builds, and
//! inserts the P7 INCREMENTAL target, its Graph parent and the build guard in
//! one commit, before any copy. Renewal follows Source → generation → guard
//! and never revives an expired guard. Since B6 (migration 0006) the target
//! is a self-contained P7 bundle: its payload and lexical children are
//! written under this Graph build guard. An EVENT-origin target binds its
//! outbox event and Source epoch exactly like a FULL event candidate.

use search_application::graph_generation::{BuildGuardHandle, GraphGenerationReceipt};
use search_application::indexing_service::DocumentSourceEvent;
use search_application::ports::SearchDeliveryFence;
use search_application::source_registration::SourceKind;
use uuid::Uuid;

use crate::full_guard::FullGuardTtl;
use crate::generation_registration::{
    Failure, FullBuildRequest, GenerationError, PgGenerationRegistrar, finish, graph_failure,
    manifest_dto, transaction_bounds, valid_digest,
};

impl PgGenerationRegistrar {
    /// Registers an INCREMENTAL target copying from a READY base receipt.
    pub async fn register_incremental(
        &self,
        base: &GraphGenerationReceipt,
        request: &FullBuildRequest,
        target_mapping_digest: &str,
        ttl: FullGuardTtl,
    ) -> Result<BuildGuardHandle, GenerationError> {
        self.register_incremental_inner(base, request, None, target_mapping_digest, ttl)
            .await
    }

    /// B6: an INCREMENTAL target for one fenced outbox delivery.
    pub async fn register_incremental_event(
        &self,
        event: &DocumentSourceEvent,
        fence: SearchDeliveryFence,
        base: &GraphGenerationReceipt,
        request: &FullBuildRequest,
        target_mapping_digest: &str,
        ttl: FullGuardTtl,
    ) -> Result<BuildGuardHandle, GenerationError> {
        if self.registration.kind() != SourceKind::Document
            || event.event_id != fence.event_id
            || fence.event_id.is_nil()
            || fence.outbox_token.is_nil()
            || fence.source.owner_token.is_nil()
            || fence.source.epoch <= 0
            || fence.source.source_id != self.registration.source_id()
        {
            return Err(GenerationError::InvalidInput);
        }
        self.register_incremental_inner(
            base,
            request,
            Some((event, fence)),
            target_mapping_digest,
            ttl,
        )
        .await
    }

    async fn register_incremental_inner(
        &self,
        base: &GraphGenerationReceipt,
        request: &FullBuildRequest,
        event: Option<(&DocumentSourceEvent, SearchDeliveryFence)>,
        target_mapping_digest: &str,
        ttl: FullGuardTtl,
    ) -> Result<BuildGuardHandle, GenerationError> {
        self.validate_request(request)?;
        if !valid_digest(target_mapping_digest)
            || base.key.source_id != request.manifest.source_id
            || base.key == request.manifest.key()
        {
            return Err(GenerationError::InvalidInput);
        }
        let stamp = self
            .gate
            .admission_stamp()
            .ok_or(GenerationError::StoreUnknown)?;
        let token = Uuid::new_v4();
        for _ in 0..3 {
            let result = self
                .register_incremental_once(
                    base,
                    request,
                    event,
                    target_mapping_digest,
                    ttl,
                    token,
                    stamp,
                )
                .await;
            self.check_gate(stamp)
                .map_err(|_| GenerationError::StoreUnknown)?;
            match result {
                Ok(handle) => return Ok(handle),
                Err(Failure::Retryable) => {}
                Err(Failure::Rejected(error)) => return Err(error),
            }
        }
        Err(GenerationError::StoreUnknown)
    }

    #[allow(clippy::too_many_arguments)]
    async fn register_incremental_once(
        &self,
        base: &GraphGenerationReceipt,
        request: &FullBuildRequest,
        event: Option<(&DocumentSourceEvent, SearchDeliveryFence)>,
        target_mapping_digest: &str,
        ttl: FullGuardTtl,
        token: Uuid,
        stamp: u64,
    ) -> Result<BuildGuardHandle, Failure> {
        let mut tx = self.pool.begin().await?;
        let result = async {
            self.check_gate(stamp)?;
            transaction_bounds(&mut tx).await?;
            if let Some((event, fence)) = event {
                // EVENT locks outbox → Source, as a FULL event candidate does.
                sqlx::query("SELECT event_id FROM outbox_events WHERE event_id=$1 FOR UPDATE")
                    .bind(fence.event_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or(GenerationError::Lost)?;
                self.check_event(&mut tx, event, fence).await?;
            }
            self.lock_source(&mut tx, event.map(|(_, fence)| fence))
                .await?;
            let base_ready: Option<String> = sqlx::query_scalar(
                "SELECT state FROM search_generation WHERE source_id=$1 AND generation_id=$2 \
                 FOR SHARE",
            )
            .bind(base.key.source_id.as_uuid())
            .bind(base.key.generation_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
            // The Graph READY receipt is checked under the Graph base lock
            // below; here the P7 row must still exist and not be retiring.
            if !matches!(base_ready.as_deref(), Some("READY" | "BUILDING")) {
                // A retired base: build the target in full instead.
                return Err(GenerationError::Lost.into());
            }
            let fence: i64 = sqlx::query_scalar(
                "UPDATE search_source_coordination SET build_fence_seq=build_fence_seq+1 \
                 WHERE source_id=$1 AND build_fence_seq < 9223372036854775807 \
                 RETURNING build_fence_seq",
            )
            .bind(self.registration.source_id().as_uuid())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(GenerationError::FenceOverflow)?;
            let manifest = &request.manifest;
            sqlx::query(
                "INSERT INTO search_generation_identity(source_id,generation_id, \
                 tenant_owner_key,activation_epoch,created_at) \
                 VALUES($1,$2,$3,$4,clock_timestamp())",
            )
            .bind(manifest.source_id.as_uuid())
            .bind(manifest.generation_id.as_uuid())
            .bind(self.registration.tenant().as_str())
            .bind(self.activation)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO search_generation(source_id,generation_id,activation_epoch,state, \
                 build_kind,stage_origin,stage_event_id,stage_source_epoch,source_snapshot, \
                 projection_manifest,projection_manifest_digest,projection_resource_count, \
                 bundle_version) \
                 VALUES($1,$2,$3,'BUILDING','INCREMENTAL',$8,$9,$10,$4,$5,$6,$7,'v1')",
            )
            .bind(manifest.source_id.as_uuid())
            .bind(manifest.generation_id.as_uuid())
            .bind(self.activation)
            .bind(&manifest.source_snapshot)
            .bind(manifest_dto(manifest))
            .bind(&manifest.digest)
            .bind(manifest.resource_count as i64)
            .bind(if event.is_some() { "EVENT" } else { "MANUAL" })
            .bind(event.map(|(_, fence)| fence.event_id))
            .bind(event.map(|(_, fence)| fence.source.epoch))
            .execute(&mut *tx)
            .await?;
            let handle = search_graph::incremental::register_incremental_on(
                &mut tx,
                base,
                manifest.key(),
                &manifest.source_snapshot,
                &manifest.digest,
                target_mapping_digest,
                token,
                fence,
                ttl.micros(),
            )
            .await
            .map_err(graph_failure)?;
            self.check_source(&mut tx, event.map(|(_, fence)| fence))
                .await?;
            if let Some((event, fence)) = event {
                self.check_event(&mut tx, event, fence).await?;
            }
            self.check_gate(stamp)?;
            Ok(handle)
        }
        .await;
        finish(tx, result).await
    }

    /// Extends a live build guard on the DB clock; an expired one stays dead.
    pub async fn renew_build_guard(
        &self,
        handle: &BuildGuardHandle,
        ttl: FullGuardTtl,
    ) -> Result<(), GenerationError> {
        let target = handle.target_key();
        if target.source_id != self.registration.source_id() {
            return Err(GenerationError::InvalidInput);
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(Failure::from)
            .map_err(rejected)?;
        let result = async {
            transaction_bounds(&mut tx).await?;
            self.lock_source(&mut tx, None).await?;
            sqlx::query(
                "SELECT 1 FROM search_graph.generation WHERE source_id=$1 AND generation_id=$2 \
                 FOR UPDATE",
            )
            .bind(target.source_id.as_uuid())
            .bind(target.generation_id.as_uuid())
            .execute(&mut *tx)
            .await?;
            let renewed = sqlx::query(
                "UPDATE search_graph.build_guard SET expires_at = GREATEST(expires_at, \
                 clock_timestamp() + ($6::bigint * interval '1 microsecond')) \
                 WHERE source_id=$1 AND target_generation_id=$2 AND base_generation_id=$3 \
                   AND guard_token=$4 AND fence=$5 AND expires_at > clock_timestamp()",
            )
            .bind(target.source_id.as_uuid())
            .bind(target.generation_id.as_uuid())
            .bind(handle.base_key().generation_id.as_uuid())
            .bind(handle.guard_token())
            .bind(handle.build_fence())
            .bind(ttl.micros())
            .execute(&mut *tx)
            .await?;
            if renewed.rows_affected() != 1 {
                return Err(GenerationError::Lost.into());
            }
            Ok(())
        }
        .await;
        finish(tx, result).await.map_err(rejected)
    }
}

fn rejected(failure: Failure) -> GenerationError {
    match failure {
        Failure::Rejected(error) => error,
        Failure::Retryable => GenerationError::StoreUnknown,
    }
}
