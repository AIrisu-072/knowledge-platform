//! 登録と同じSource行を使う、DB時計による排他的な配送許可。

use std::{sync::Arc, time::Duration};

use outbox_delivery::runner::{ClaimAdmission, ClaimPermit};
use outbox_delivery::{DeliveryError, DeliveryFuture, FenceResult};
use search_application::ports::{SearchSourceLease, SourceFence};
use search_application::search_core::id::SourceId;
use sqlx::{PgPool, Row};

use crate::source_registration::CurrentGate;

#[derive(Clone)]
pub struct PostgresSourceAdmission {
    pool: PgPool,
    source: SourceId,
    ttl_micros: i64,
    gate: Arc<CurrentGate>,
}

impl PostgresSourceAdmission {
    // 公開入口はledger.source_admissionだけにし、無gateの構成を作らない。
    pub(crate) fn new(
        pool: PgPool,
        source: SourceId,
        ttl: Duration,
        gate: Arc<CurrentGate>,
    ) -> Result<Self, DeliveryError> {
        let micros = i64::try_from(ttl.as_micros()).map_err(|_| DeliveryError::InvalidConfig)?;
        if source.as_uuid().is_nil() || micros <= 0 || ttl != Duration::from_micros(micros as u64) {
            return Err(DeliveryError::InvalidConfig);
        }
        Ok(Self {
            pool,
            source,
            ttl_micros: micros,
            gate,
        })
    }
}

impl ClaimAdmission for PostgresSourceAdmission {
    type Permit = SourceLease;

    fn acquire(&self) -> DeliveryFuture<'_, Option<SourceLease>> {
        Box::pin(async move {
            let stamp = self
                .gate
                .admission_stamp()
                .ok_or(DeliveryError::StoreUnknown)?;
            let row = sqlx::query("UPDATE search_source_coordination s SET owner_token=gen_random_uuid(),fence_epoch=s.fence_epoch+1,lease_expires_at=clock_timestamp()+($2::bigint * INTERVAL '1 microsecond') WHERE s.source_id=$1 AND s.registration_active AND s.fence_epoch < 9223372036854775807 AND (s.owner_token IS NULL OR s.lease_expires_at <= clock_timestamp()) AND EXISTS (SELECT 1 FROM search_source_ownership o WHERE o.source_id=s.source_id AND o.state='ACTIVE' AND o.tenant_owner_key=s.tenant_owner_key AND o.registration_revision=s.registration_revision AND o.visibility_revision=s.visibility_revision AND o.activation_epoch=s.activation_epoch) RETURNING s.owner_token,s.fence_epoch,s.activation_epoch,s.tenant_owner_key,s.registration_revision,s.visibility_revision")
                .bind(self.source.as_uuid()).bind(self.ttl_micros).fetch_optional(&self.pool).await.map_err(|_| DeliveryError::StoreUnknown)?;
            if !self.gate.matches_stamp(stamp) {
                return Err(DeliveryError::StoreUnknown);
            }
            row.map(|row| {
                let unknown = |_| DeliveryError::StoreUnknown;
                Ok(SourceLease {
                    pool: self.pool.clone(),
                    gate: self.gate.clone(),
                    ttl_micros: self.ttl_micros,
                    fence: SourceFence {
                        source_id: self.source,
                        owner_token: row.try_get("owner_token").map_err(unknown)?,
                        epoch: row.try_get("fence_epoch").map_err(unknown)?,
                    },
                    activation: row.try_get("activation_epoch").map_err(unknown)?,
                    tenant: row.try_get("tenant_owner_key").map_err(unknown)?,
                    registration: row.try_get("registration_revision").map_err(unknown)?,
                    visibility: row.try_get("visibility_revision").map_err(unknown)?,
                })
            })
            .transpose()
        })
    }

    fn max_claims_per_permit(&self) -> u32 {
        1
    }
}

#[derive(Clone)]
pub struct SourceLease {
    pool: PgPool,
    gate: Arc<CurrentGate>,
    fence: SourceFence,
    activation: i64,
    tenant: String,
    registration: i64,
    visibility: i64,
    ttl_micros: i64,
}

impl SearchSourceLease for SourceLease {
    fn fence(&self) -> SourceFence {
        self.fence
    }
}

impl SourceLease {
    async fn check_or_update(
        &self,
        operation: LeaseOperation,
    ) -> Result<FenceResult, DeliveryError> {
        let stamp = self
            .gate
            .admission_stamp()
            .ok_or(DeliveryError::StoreUnknown)?;
        // すべての操作で同じ所有者・epoch・有効期限・登録の結び付けを確認する。
        let statement = match operation {
            LeaseOperation::Check => {
                "SELECT s.source_id FROM search_source_coordination s WHERE s.source_id=$1 AND s.owner_token=$2 AND s.fence_epoch=$3 AND s.activation_epoch=$4 AND s.tenant_owner_key=$5 AND s.registration_revision=$6 AND s.visibility_revision=$7 AND s.registration_active AND s.lease_expires_at > clock_timestamp() AND EXISTS (SELECT 1 FROM search_source_ownership o WHERE o.source_id=s.source_id AND o.state='ACTIVE' AND o.tenant_owner_key=s.tenant_owner_key AND o.registration_revision=s.registration_revision AND o.visibility_revision=s.visibility_revision AND o.activation_epoch=s.activation_epoch) AND $8::bigint > 0"
            }
            LeaseOperation::Renew => {
                "UPDATE search_source_coordination s SET lease_expires_at=clock_timestamp()+($8::bigint * INTERVAL '1 microsecond') WHERE s.source_id=$1 AND s.owner_token=$2 AND s.fence_epoch=$3 AND s.activation_epoch=$4 AND s.tenant_owner_key=$5 AND s.registration_revision=$6 AND s.visibility_revision=$7 AND s.registration_active AND s.lease_expires_at > clock_timestamp() AND EXISTS (SELECT 1 FROM search_source_ownership o WHERE o.source_id=s.source_id AND o.state='ACTIVE' AND o.tenant_owner_key=s.tenant_owner_key AND o.registration_revision=s.registration_revision AND o.visibility_revision=s.visibility_revision AND o.activation_epoch=s.activation_epoch) RETURNING s.source_id"
            }
            LeaseOperation::Release => {
                "UPDATE search_source_coordination s SET owner_token=NULL,lease_expires_at=NULL WHERE s.source_id=$1 AND s.owner_token=$2 AND s.fence_epoch=$3 AND s.activation_epoch=$4 AND s.tenant_owner_key=$5 AND s.registration_revision=$6 AND s.visibility_revision=$7 AND s.registration_active AND s.lease_expires_at > clock_timestamp() AND EXISTS (SELECT 1 FROM search_source_ownership o WHERE o.source_id=s.source_id AND o.state='ACTIVE' AND o.tenant_owner_key=s.tenant_owner_key AND o.registration_revision=s.registration_revision AND o.visibility_revision=s.visibility_revision AND o.activation_epoch=s.activation_epoch) AND $8::bigint > 0 RETURNING s.source_id"
            }
        };
        let row = sqlx::query(statement)
            .bind(self.fence.source_id.as_uuid())
            .bind(self.fence.owner_token)
            .bind(self.fence.epoch)
            .bind(self.activation)
            .bind(&self.tenant)
            .bind(self.registration)
            .bind(self.visibility)
            .bind(self.ttl_micros)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| DeliveryError::StoreUnknown)?;
        if !self.gate.matches_stamp(stamp) {
            return Err(DeliveryError::StoreUnknown);
        }
        Ok(if row.is_some() {
            FenceResult::Updated
        } else {
            FenceResult::Lost
        })
    }
}

enum LeaseOperation {
    Check,
    Renew,
    Release,
}
impl ClaimPermit for SourceLease {
    fn preflight(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.check_or_update(LeaseOperation::Check))
    }
    fn renew(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.check_or_update(LeaseOperation::Renew))
    }
    fn release(&self) -> DeliveryFuture<'_, FenceResult> {
        Box::pin(self.check_or_update(LeaseOperation::Release))
    }
}
