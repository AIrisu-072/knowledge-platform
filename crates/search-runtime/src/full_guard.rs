//! 完全構築の不透明なハンドルと、上限付きのDB時計リース。

use std::time::Duration;

/// DBが正確に表現できる、120秒以下の正の完全構築リース。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullGuardTtl(i64);

impl FullGuardTtl {
    pub(crate) const fn micros(self) -> i64 {
        self.0
    }

    pub fn new(duration: Duration) -> Option<Self> {
        let micros = i64::try_from(duration.as_micros()).ok()?;
        (micros > 0
            && duration <= Duration::from_secs(120)
            && duration == Duration::from_micros(micros as u64))
        .then_some(Self(micros))
    }
}

use search_application::indexing_service::DocumentSourceEvent;
use search_application::ports::SearchDeliveryFence;
use search_application::search_core::projection::{
    ProjectionGenerationKey, ProjectionGenerationManifest,
};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::generation_registration::{
    Failure, GenerationError, PgGenerationRegistrar, finish, manifest_dto, transaction_bounds,
};

/// この値は登録commit後だけ発行される。token/fence/登録内容は外部へ公開しない。
/// MANUALはEVENTへの変換やDeserializeを持たない。
///
/// ```compile_fail
/// use search_runtime::full_guard::ManualBuildHandle;
/// let forged = ManualBuildHandle { };
/// ```
pub struct ManualBuildHandle(pub(crate) RegisteredFullBuild);
impl ManualBuildHandle {
    pub fn key(&self) -> ProjectionGenerationKey {
        self.0.manifest.key()
    }
}

/// EVENTの正本は保存済み行とlive fence。ハンドルを持つだけでは許可にならない。
///
/// ```compile_fail
/// use search_runtime::{generation_registration::PgGenerationRegistrar,
///     full_guard::{ManualBuildHandle, FullGuardTtl}};
/// async fn forge(registrar: &PgGenerationRegistrar, manual: &ManualBuildHandle, ttl: FullGuardTtl) {
///     registrar.renew_event(manual, ttl).await;
/// }
/// ```
pub struct EventCandidateHandle {
    pub(crate) build: RegisteredFullBuild,
    pub(crate) event: DocumentSourceEvent,
    pub(crate) fence: SearchDeliveryFence,
}
impl EventCandidateHandle {
    pub fn key(&self) -> ProjectionGenerationKey {
        self.build.manifest.key()
    }
}

pub(crate) struct RegisteredFullBuild {
    pub(crate) manifest: ProjectionGenerationManifest,
    pub(crate) token: Uuid,
    pub(crate) fence: i64,
    pub(crate) activation: i64,
    pub(crate) registration_digest: String,
}

impl PgGenerationRegistrar {
    pub async fn renew_manual(
        &self,
        handle: &ManualBuildHandle,
        ttl: FullGuardTtl,
    ) -> Result<(), GenerationError> {
        self.renew(&handle.0, None, ttl).await
    }
    pub async fn renew_event(
        &self,
        handle: &EventCandidateHandle,
        ttl: FullGuardTtl,
    ) -> Result<(), GenerationError> {
        self.renew(&handle.build, Some((&handle.event, handle.fence)), ttl)
            .await
    }

    async fn renew(
        &self,
        handle: &RegisteredFullBuild,
        event: Option<(&DocumentSourceEvent, SearchDeliveryFence)>,
        ttl: FullGuardTtl,
    ) -> Result<(), GenerationError> {
        let stamp = self
            .gate
            .admission_stamp()
            .ok_or(GenerationError::StoreUnknown)?;
        if handle.manifest.source_id != self.registration.source_id()
            || handle.activation != self.activation
            || handle.registration_digest != self.registration_digest
        {
            return Err(GenerationError::Lost);
        }
        for _ in 0..3 {
            let mut tx = self
                .pool
                .begin()
                .await
                .map_err(|_| GenerationError::StoreUnknown)?;
            let result=async {
                self.check_gate(stamp)?;
                transaction_bounds(&mut tx).await?;
                // 凍結順序: Source → 対象 → guard。outboxは逆順のロックを取得しない。
                self.lock_source(&mut tx,event.map(|(_,f)|f)).await?;
                self.check_guard(&mut tx,handle,event.map(|(_,f)|f)).await?;
                if let Some((event,fence))=event { self.check_event(&mut tx,event,fence).await?; }
                let updated=sqlx::query("UPDATE search_generation_full_guard SET expires_at=GREATEST(expires_at,clock_timestamp()+($5::bigint * interval '1 microsecond')) WHERE source_id=$1 AND target_generation_id=$2 AND guard_token=$3 AND build_fence=$4 AND expires_at > clock_timestamp()")
                    .bind(handle.manifest.source_id.as_uuid()).bind(handle.manifest.generation_id.as_uuid())
                    .bind(handle.token).bind(handle.fence).bind(ttl.micros()).execute(&mut *tx).await?;
                if updated.rows_affected()!=1 { return Err(GenerationError::Lost.into()); }
                self.check_source(&mut tx,event.map(|(_,f)|f)).await?;
                self.check_guard(&mut tx,handle,event.map(|(_,f)|f)).await?;
                if let Some((event,fence))=event { self.check_event(&mut tx,event,fence).await?; }
                self.check_gate(stamp)?;
                Ok(())
            }.await;
            let result = finish(tx, result).await;
            self.check_gate(stamp)
                .map_err(|_| GenerationError::StoreUnknown)?;
            match result {
                Ok(()) => return Ok(()),
                Err(Failure::Retryable) => {}
                Err(Failure::Rejected(error)) => return Err(error),
            }
        }
        Err(GenerationError::StoreUnknown)
    }

    pub(crate) async fn check_guard(
        &self,
        connection: &mut PgConnection,
        handle: &RegisteredFullBuild,
        event: Option<SearchDeliveryFence>,
    ) -> Result<(), Failure> {
        let row=sqlx::query("SELECT activation_epoch,state,build_kind,stage_origin,stage_event_id,stage_source_epoch,full_guard_token,full_build_fence,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version FROM search_generation WHERE source_id=$1 AND generation_id=$2 FOR UPDATE")
            .bind(handle.manifest.source_id.as_uuid()).bind(handle.manifest.generation_id.as_uuid())
            .fetch_optional(&mut *connection).await?.ok_or(GenerationError::Lost)?;
        let manifest = &handle.manifest;
        if row.try_get::<i64, _>("activation_epoch")? != handle.activation
            || row.try_get::<String, _>("state")? != "BUILDING"
            || row.try_get::<String, _>("build_kind")? != "FULL"
            || row.try_get::<String, _>("stage_origin")?
                != if event.is_some() { "EVENT" } else { "MANUAL" }
            || row.try_get::<Option<Uuid>, _>("stage_event_id")? != event.map(|f| f.event_id)
            || row.try_get::<Option<i64>, _>("stage_source_epoch")? != event.map(|f| f.source.epoch)
            || row.try_get::<Option<Uuid>, _>("full_guard_token")? != Some(handle.token)
            || row.try_get::<Option<i64>, _>("full_build_fence")? != Some(handle.fence)
            || row.try_get::<String, _>("source_snapshot")? != manifest.source_snapshot
            || row.try_get::<serde_json::Value, _>("projection_manifest")? != manifest_dto(manifest)
            || row.try_get::<String, _>("projection_manifest_digest")? != manifest.digest
            || row.try_get::<i64, _>("projection_resource_count")? != manifest.resource_count as i64
            || row.try_get::<String, _>("bundle_version")? != "v1"
        {
            return Err(GenerationError::Lost.into());
        }
        sqlx::query("SELECT guard_token FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2 FOR UPDATE")
            .bind(manifest.source_id.as_uuid()).bind(manifest.generation_id.as_uuid())
            .fetch_optional(&mut *connection).await?.ok_or(GenerationError::Lost)?;
        // SELECTの待機前に評価したclockに依存せず、guardロック後に再評価する。
        let live: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM search_generation_full_guard WHERE source_id=$1 AND target_generation_id=$2 AND guard_token=$3 AND build_fence=$4 AND expires_at > clock_timestamp())")
            .bind(manifest.source_id.as_uuid()).bind(manifest.generation_id.as_uuid())
            .bind(handle.token).bind(handle.fence).fetch_one(connection).await?;
        if !live {
            return Err(GenerationError::Lost.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_microseconds_through_bound_are_accepted() {
        for value in [
            Duration::from_micros(1),
            Duration::from_secs(30),
            Duration::from_secs(120),
        ] {
            assert!(FullGuardTtl::new(value).is_some());
        }
    }

    #[test]
    fn zero_submicrosecond_rounding_and_excess_are_rejected() {
        for value in [
            Duration::ZERO,
            Duration::from_nanos(1),
            Duration::from_nanos(1001),
            Duration::from_secs(121),
            Duration::MAX,
        ] {
            assert!(FullGuardTtl::new(value).is_none());
        }
    }
}
