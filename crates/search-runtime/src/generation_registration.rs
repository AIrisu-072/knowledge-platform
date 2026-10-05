//! P7-06: trusted composition用のFULL BUILDING登録。READYや公開の証明は発行しない。

use std::sync::Arc;

use crate::source_registration::CurrentGate;
use search_application::indexing_service::{DocumentSourceEvent, validate_document_event_route};
use search_application::ports::SearchDeliveryFence;
use search_application::search_core::projection::ProjectionGenerationManifest;
use search_application::search_core::source::RetentionMode;
use search_application::source_registration::{
    RegistrationActivation, SourceKind, SourceRegistration,
};
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::full_guard::{
    EventCandidateHandle, FullGuardTtl, ManualBuildHandle, RegisteredFullBuild,
};

/// 登録時の参照binding。本文の再読・digestの再計算やREADY証明ではない。
#[derive(Clone)]
pub struct FullBuildRequest {
    pub manifest: ProjectionGenerationManifest,
    pub expected_snapshot: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationError {
    InvalidInput,
    Lost,
    Conflict,
    FenceOverflow,
    StoreUnknown,
}

#[derive(Debug)]
pub(crate) enum Failure {
    Retryable,
    Rejected(GenerationError),
}
impl From<GenerationError> for Failure {
    fn from(value: GenerationError) -> Self {
        Self::Rejected(value)
    }
}
impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.code()).as_deref() {
            Some("40001" | "40P01" | "55P03") => Self::Retryable,
            Some("23505") => GenerationError::Conflict.into(),
            _ => GenerationError::StoreUnknown.into(),
        }
    }
}

/// 信頼された構成起点が、既に登録されたSourceの正本とactivationを固定する。
/// HTTP/provider入力から構成しない。EVENTのSource経路はこの登録に固定される。
/// Graph未接続の初期経路なのでPersistentResourceだけを対象とする。
#[derive(Clone)]
pub struct PgGenerationRegistrar {
    pub(crate) pool: PgPool,
    pub(crate) registration: SourceRegistration,
    pub(crate) activation: i64,
    pub(crate) registration_dto: Value,
    pub(crate) registration_digest: String,
    pub(crate) gate: Arc<CurrentGate>,
}

impl PgGenerationRegistrar {
    pub(crate) fn new(
        pool: PgPool,
        registration: SourceRegistration,
        activation: RegistrationActivation,
        gate: Arc<CurrentGate>,
    ) -> Result<Self, GenerationError> {
        if registration.discoverable_source().retention_mode != RetentionMode::PersistentResource {
            return Err(GenerationError::InvalidInput);
        }
        let activation =
            i64::try_from(activation.get()).map_err(|_| GenerationError::InvalidInput)?;
        let registration_dto = registration
            .persistence_dto_v1()
            .map_err(|_| GenerationError::InvalidInput)?;
        let digest = registration
            .persistence_digest_v1()
            .map_err(|_| GenerationError::InvalidInput)?;
        let registration_digest = format!(
            "sha256:{}",
            digest
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        Ok(Self {
            pool,
            registration,
            activation,
            registration_dto,
            registration_digest,
            gate,
        })
    }

    pub async fn register_manual(
        &self,
        request: &FullBuildRequest,
        ttl: FullGuardTtl,
    ) -> Result<ManualBuildHandle, GenerationError> {
        self.register(request, None, ttl)
            .await
            .map(ManualBuildHandle)
    }

    pub async fn register_event(
        &self,
        event: &DocumentSourceEvent,
        fence: SearchDeliveryFence,
        request: &FullBuildRequest,
        ttl: FullGuardTtl,
    ) -> Result<EventCandidateHandle, GenerationError> {
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
        let build = self.register(request, Some((event, fence)), ttl).await?;
        Ok(EventCandidateHandle {
            build,
            event: event.clone(),
            fence,
        })
    }

    async fn register(
        &self,
        request: &FullBuildRequest,
        event: Option<(&DocumentSourceEvent, SearchDeliveryFence)>,
        ttl: FullGuardTtl,
    ) -> Result<RegisteredFullBuild, GenerationError> {
        self.validate_request(request)?;
        let stamp = self
            .gate
            .admission_stamp()
            .ok_or(GenerationError::StoreUnknown)?;
        let token = Uuid::new_v4();
        for _ in 0..3 {
            let result = self.register_once(request, event, ttl, token, stamp).await;
            self.check_gate(stamp)
                .map_err(|_| GenerationError::StoreUnknown)?;
            match result {
                Ok(value) => return Ok(value),
                Err(Failure::Retryable) => {}
                Err(Failure::Rejected(error)) => return Err(error),
            }
        }
        Err(GenerationError::StoreUnknown)
    }

    fn validate_request(&self, request: &FullBuildRequest) -> Result<(), GenerationError> {
        let manifest = &request.manifest;
        let bounded = |value: &str| !value.is_empty() && value.len() <= 1024;
        if manifest.source_id != self.registration.source_id()
            || manifest.source_id.as_uuid().is_nil()
            || manifest.generation_id.as_uuid().is_nil()
            || !bounded(&manifest.source_snapshot)
            || manifest.source_snapshot != request.expected_snapshot
            || !bounded(&manifest.projection_schema_version)
            || !bounded(&manifest.semantic_registry_version)
            || manifest.lens_version == 0
            || manifest.resource_count > i64::MAX as u64
            || manifest.relation_count.is_some_and(|n| n > i64::MAX as u64)
            || !valid_digest(&manifest.digest)
            || [
                &manifest.analyzer_version,
                &manifest.embedding_model_version,
                &manifest.graph_schema_version,
            ]
            .iter()
            .any(|v| v.as_ref().is_some_and(|v| !bounded(v)))
        {
            return Err(GenerationError::InvalidInput);
        }
        Ok(())
    }

    async fn register_once(
        &self,
        request: &FullBuildRequest,
        event: Option<(&DocumentSourceEvent, SearchDeliveryFence)>,
        ttl: FullGuardTtl,
        token: Uuid,
        stamp: u64,
    ) -> Result<RegisteredFullBuild, Failure> {
        let mut tx = self.pool.begin().await?;
        let result = async {
            self.check_gate(stamp)?;
            transaction_bounds(&mut tx).await?;
            if let Some((event, fence)) = event {
                // EVENTは必ずoutbox → Source。期限判定はロック取得後にも実行する。
                sqlx::query("SELECT event_id FROM outbox_events WHERE event_id=$1 FOR UPDATE")
                    .bind(fence.event_id).fetch_optional(&mut *tx).await?
                    .ok_or(GenerationError::Lost)?;
                self.check_event(&mut tx,event,fence).await?;
            }
            self.lock_source(&mut tx,event.map(|(_,f)|f)).await?;
            let fence: i64 = sqlx::query_scalar("UPDATE search_source_coordination SET build_fence_seq=build_fence_seq+1 WHERE source_id=$1 AND build_fence_seq < 9223372036854775807 RETURNING build_fence_seq")
                .bind(self.registration.source_id().as_uuid()).fetch_optional(&mut *tx).await?
                .ok_or(GenerationError::FenceOverflow)?;
            let manifest = &request.manifest;
            sqlx::query("INSERT INTO search_generation_identity(source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES($1,$2,$3,$4,clock_timestamp())")
                .bind(manifest.source_id.as_uuid()).bind(manifest.generation_id.as_uuid())
                .bind(self.registration.tenant().as_str()).bind(self.activation).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO search_generation(source_id,generation_id,activation_epoch,state,build_kind,stage_origin,stage_event_id,stage_source_epoch,full_guard_token,full_build_fence,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version) VALUES($1,$2,$3,'BUILDING','FULL',$4,$5,$6,$7,$8,$9,$10,$11,$12,'v1')")
                .bind(manifest.source_id.as_uuid()).bind(manifest.generation_id.as_uuid()).bind(self.activation)
                .bind(if event.is_some() {"EVENT"} else {"MANUAL"})
                .bind(event.map(|(_,f)|f.event_id)).bind(event.map(|(_,f)|f.source.epoch))
                .bind(token).bind(fence).bind(&manifest.source_snapshot).bind(manifest_dto(manifest))
                .bind(&manifest.digest).bind(manifest.resource_count as i64).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO search_generation_full_guard(source_id,target_generation_id,guard_token,build_fence,expires_at) VALUES($1,$2,$3,$4,clock_timestamp()+($5::bigint * interval '1 microsecond'))")
                .bind(manifest.source_id.as_uuid()).bind(manifest.generation_id.as_uuid()).bind(token)
                .bind(fence).bind(ttl.micros()).execute(&mut *tx).await?;
            let handle = RegisteredFullBuild { manifest: manifest.clone(), token, fence,
                activation: self.activation, registration_digest: self.registration_digest.clone() };
            self.check_source(&mut tx,event.map(|(_,f)|f)).await?;
            if let Some((event,fence))=event { self.check_event(&mut tx,event,fence).await?; }
            self.check_guard(&mut tx,&handle,event.map(|(_,f)|f)).await?;
            self.check_gate(stamp)?;
            Ok(handle)
        }.await;
        finish(tx, result).await
    }

    pub(crate) fn check_gate(&self, stamp: u64) -> Result<(), Failure> {
        if self.gate.matches_stamp(stamp) {
            Ok(())
        } else {
            Err(GenerationError::StoreUnknown.into())
        }
    }

    pub(crate) async fn lock_source(
        &self,
        connection: &mut PgConnection,
        event: Option<SearchDeliveryFence>,
    ) -> Result<(), Failure> {
        sqlx::query(
            "SELECT source_id FROM search_source_coordination WHERE source_id=$1 FOR UPDATE",
        )
        .bind(self.registration.source_id().as_uuid())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(GenerationError::Lost)?;
        self.check_source(connection, event).await
    }

    pub(crate) async fn check_source(
        &self,
        connection: &mut PgConnection,
        event: Option<SearchDeliveryFence>,
    ) -> Result<(), Failure> {
        let row = sqlx::query("SELECT s.owner_token,s.fence_epoch,s.lease_expires_at > clock_timestamp() AS live FROM search_source_coordination s JOIN search_source_ownership o USING(source_id) WHERE s.source_id=$1 AND s.registration_active AND o.state='ACTIVE' AND s.tenant_owner_key=$2 AND o.tenant_owner_key=s.tenant_owner_key AND s.activation_epoch=$3 AND o.activation_epoch=s.activation_epoch AND s.registration_revision=$4 AND o.registration_revision=s.registration_revision AND s.visibility_revision=$5 AND o.visibility_revision=s.visibility_revision AND o.source_kind=$6 AND o.registration_dto=$7 AND o.registration_digest=$8")
            .bind(self.registration.source_id().as_uuid()).bind(self.registration.tenant().as_str())
            .bind(self.activation).bind(i64::try_from(self.registration.registration_revision().get()).map_err(|_|GenerationError::InvalidInput)?)
            .bind(i64::try_from(self.registration.visibility_revision().get()).map_err(|_|GenerationError::InvalidInput)?)
            .bind(match self.registration.kind(){SourceKind::Document=>"DOCUMENT",SourceKind::Remote=>"REMOTE"})
            .bind(&self.registration_dto).bind(&self.registration_digest).fetch_optional(connection).await?
            .ok_or(GenerationError::Lost)?;
        if let Some(fence) = event
            && (fence.source.source_id != self.registration.source_id()
                || row.try_get::<Option<Uuid>, _>("owner_token")? != Some(fence.source.owner_token)
                || row.try_get::<i64, _>("fence_epoch")? != fence.source.epoch
                || row.try_get::<Option<bool>, _>("live")? != Some(true))
        {
            return Err(GenerationError::Lost.into());
        }
        Ok(())
    }

    pub(crate) async fn check_event(
        &self,
        connection: &mut PgConnection,
        event: &DocumentSourceEvent,
        fence: SearchDeliveryFence,
    ) -> Result<(), Failure> {
        let row = sqlx::query("SELECT event_type,aggregate_type,aggregate_id,occurred_at FROM outbox_events WHERE event_id=$1 AND lease_token=$2 AND lease_expires_at > clock_timestamp() AND delivered_at IS NULL AND dead_lettered_at IS NULL")
            .bind(fence.event_id).bind(fence.outbox_token).fetch_optional(connection).await?
            .ok_or(GenerationError::Lost)?;
        let event_type: String = row.try_get("event_type")?;
        let aggregate_type: String = row.try_get("aggregate_type")?;
        if event.event_id != fence.event_id
            || event.event_type != event_type
            || event.aggregate_id != row.try_get::<Uuid, _>("aggregate_id")?
            || event.occurred_at != row.try_get::<time::OffsetDateTime, _>("occurred_at")?
            || validate_document_event_route(&event_type, &aggregate_type).is_err()
        {
            return Err(GenerationError::Lost.into());
        }
        Ok(())
    }
}

pub(crate) fn manifest_dto(manifest: &ProjectionGenerationManifest) -> Value {
    serde_json::json!({"dto_version":"v1", "manifest":manifest})
}
fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f'))
}
pub(crate) async fn transaction_bounds(connection: &mut PgConnection) -> Result<(), Failure> {
    sqlx::query("SET LOCAL lock_timeout='2s'")
        .execute(&mut *connection)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='5s'")
        .execute(connection)
        .await?;
    Ok(())
}
pub(crate) async fn finish<T>(
    tx: sqlx::Transaction<'_, sqlx::Postgres>,
    result: Result<T, Failure>,
) -> Result<T, Failure> {
    match result {
        Ok(value) => {
            // DBが中止を証明した場合だけ再試行する。COMMIT応答不明は再試行しない。
            tx.commit().await.map_err(|error| {
                if matches!(
                    error.as_database_error().and_then(|e| e.code()).as_deref(),
                    Some("40001" | "40P01" | "55P03")
                ) {
                    Failure::Retryable
                } else {
                    Failure::Rejected(GenerationError::StoreUnknown)
                }
            })?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback()
                .await
                .map_err(|_| Failure::Rejected(GenerationError::StoreUnknown))?;
            Err(error)
        }
    }
}
