//! ホストの完全な登録集合を、共有Source台帳へ一トランザクションで反映する。

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use search_application::SearchError;
use search_application::ports::BoxFuture;
use search_application::search_core::id::SourceId;
use search_application::source_registration::{
    CompleteDesiredRegistrations, HostRegistrationSnapshotPort, RegistrationActivation,
    RegistrationNamespace, RegistrationSetDigest, SourceKind, SourceRegistration,
    SourceRegistrationLedgerPort,
};
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use crate::source_lease::PostgresSourceAdmission;

fn unavailable() -> SearchError {
    SearchError::OperationFailed("Source registration unavailable".into())
}
fn invalid() -> SearchError {
    SearchError::InvalidRequest("invalid Source registration".into())
}
#[derive(Debug)]
enum ReconcileFailure {
    Retryable,
    Search(SearchError),
}
impl From<SearchError> for ReconcileFailure {
    fn from(error: SearchError) -> Self {
        Self::Search(error)
    }
}
fn retryable_code(code: &str) -> bool {
    matches!(code, "40001" | "40P01" | "55P03")
}
fn retryable_database(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| retryable_code(&code))
}
fn statement_error(error: sqlx::Error) -> ReconcileFailure {
    if retryable_database(&error) {
        ReconcileFailure::Retryable
    } else {
        unavailable().into()
    }
}
fn commit_error(error: sqlx::Error) -> ReconcileFailure {
    if retryable_database(&error) {
        ReconcileFailure::Retryable
    } else {
        SearchError::CompletionUnknown.into()
    }
}

fn positive(value: u64) -> Result<i64, SearchError> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}
fn next(value: i64) -> Result<i64, SearchError> {
    value
        .checked_add(1)
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}
fn digest(value: RegistrationSetDigest) -> String {
    let mut result = String::with_capacity(71);
    result.push_str("sha256:");
    for byte in value.as_bytes() {
        write!(result, "{byte:02x}").expect("String write cannot fail");
    }
    result
}
fn kind(namespace: RegistrationNamespace) -> &'static str {
    match namespace {
        RegistrationNamespace::Document => "DOCUMENT",
        RegistrationNamespace::Remote => "REMOTE",
    }
}

/// 成功応答を確認できない期間は、同じ構成から発行済みのleaseも閉じる。
/// DBの所有権を代替せず、DB検証に加えるプロセス内の否定ゲートである。
#[derive(Default)]
pub(crate) struct CurrentGate(AtomicU64);
impl CurrentGate {
    const BUSY: u64 = 4;
    const READY: u64 = 3;
    fn namespace_bit(namespace: RegistrationNamespace) -> u64 {
        match namespace {
            RegistrationNamespace::Document => 1,
            RegistrationNamespace::Remote => 2,
        }
    }
    fn begin(&self) -> Result<u64, SearchError> {
        let old = self.0.load(Ordering::Acquire);
        let Some(generation) = (old & !7).checked_add(8) else {
            self.0.store(u64::MAX, Ordering::Release);
            return Err(unavailable());
        };
        // 前回の失敗・dropが残っていれば両名前空間を再照合する。
        let confirmed = if old & Self::BUSY == 0 {
            old & Self::READY
        } else {
            0
        };
        let stamp = generation | confirmed | Self::BUSY;
        self.0.store(stamp, Ordering::Release);
        Ok(stamp)
    }
    fn finish(&self, stamp: u64, namespace: RegistrationNamespace) {
        self.0.store(
            (stamp & !Self::BUSY) | Self::namespace_bit(namespace),
            Ordering::Release,
        );
    }
    fn current_stamp(&self, namespace: RegistrationNamespace) -> Option<u64> {
        let stamp = self.0.load(Ordering::Acquire);
        (stamp & Self::BUSY == 0 && stamp & Self::namespace_bit(namespace) != 0).then_some(stamp)
    }
    pub(crate) fn admission_stamp(&self) -> Option<u64> {
        let stamp = self.0.load(Ordering::Acquire);
        (stamp & 7 == Self::READY).then_some(stamp)
    }
    pub(crate) fn matches_stamp(&self, stamp: u64) -> bool {
        self.0.load(Ordering::Acquire) == stamp
    }
}

#[derive(Clone)]
pub struct PgSourceRegistrationLedger {
    pool: PgPool,
    host: Arc<dyn HostRegistrationSnapshotPort>,
    gate: Arc<CurrentGate>,
    reconcile_lock: Arc<tokio::sync::Mutex<()>>,
}

impl PgSourceRegistrationLedger {
    /// hostは信頼された構成起点が固定する。本番の全テナント列挙はR01P/R02が検証する。
    pub fn new(pool: PgPool, host: Arc<dyn HostRegistrationSnapshotPort>) -> Self {
        Self {
            pool,
            host,
            gate: Arc::default(),
            reconcile_lock: Arc::default(),
        }
    }

    /// 登録応答不明・キャンセル時に、登録と同じ否定ゲートを適用する。
    pub fn source_admission(
        &self,
        source: SourceId,
        ttl: Duration,
    ) -> Result<PostgresSourceAdmission, outbox_delivery::DeliveryError> {
        PostgresSourceAdmission::new(self.pool.clone(), source, ttl, self.gate.clone())
    }

    /// 世代の登録/guard更新にも、Source登録と同じ否定ゲートを必ず適用する。
    pub fn generation_registrar(
        &self,
        registration: SourceRegistration,
        activation: RegistrationActivation,
    ) -> Result<
        crate::generation_registration::PgGenerationRegistrar,
        crate::generation_registration::GenerationError,
    > {
        crate::generation_registration::PgGenerationRegistrar::new(
            self.pool.clone(),
            registration,
            activation,
            self.gate.clone(),
        )
    }

    async fn matches_host(
        &self,
        desired: &CompleteDesiredRegistrations,
    ) -> Result<(), SearchError> {
        if CompleteDesiredRegistrations::capture(self.host.as_ref(), desired.namespace()).await?
            != *desired
        {
            return Err(invalid());
        }
        Ok(())
    }

    async fn reconcile_inner(
        &self,
        desired: &CompleteDesiredRegistrations,
    ) -> Result<BTreeMap<SourceId, RegistrationActivation>, ReconcileFailure> {
        self.matches_host(desired).await?;
        let revision = positive(desired.deployment_revision().get())?;
        let set_digest = digest(desired.set_digest());
        let prepared: BTreeMap<_, _> = desired
            .registrations()
            .iter()
            .map(|(id, registration)| Prepared::new(registration).map(|value| (*id, value)))
            .collect::<Result<_, _>>()?;
        let mut tx = self.pool.begin().await.map_err(statement_error)?;
        let serial = sqlx::query("SELECT document_deployment_revision,document_desired_set_digest,remote_deployment_revision,remote_desired_set_digest FROM search_registration_serial WHERE singleton=TRUE FOR UPDATE")
            .fetch_one(&mut *tx).await.map_err(statement_error)?;
        let (revision_column, digest_column) = match desired.namespace() {
            RegistrationNamespace::Document => (
                "document_deployment_revision",
                "document_desired_set_digest",
            ),
            RegistrationNamespace::Remote => {
                ("remote_deployment_revision", "remote_desired_set_digest")
            }
        };
        let old_revision: Option<i64> = serial.try_get(revision_column).map_err(statement_error)?;
        let old_digest: Option<String> = serial.try_get(digest_column).map_err(statement_error)?;
        if old_revision.is_some_and(|prior| {
            revision < prior || (revision == prior && old_digest.as_deref() != Some(&set_digest))
        }) || old_revision.is_none() != old_digest.is_none()
        {
            return Err(invalid().into());
        }
        let idempotent = old_revision == Some(revision);
        let ids: Vec<_> = prepared.keys().map(|id| id.as_uuid()).collect();
        // 直列化行 → UUID昇順のSource行 → UUID昇順の所有権行、の順序を固定する。
        let sources = sqlx::query("SELECT source_id,fence_epoch,tenant_owner_key,registration_revision,visibility_revision,activation_epoch,registration_active FROM search_source_coordination WHERE source_id=ANY($1) OR source_id IN (SELECT source_id FROM search_source_ownership WHERE source_kind=$2) ORDER BY source_id FOR UPDATE")
            .bind(&ids).bind(kind(desired.namespace())).fetch_all(&mut *tx).await.map_err(statement_error)?;
        let source_rows = sources
            .into_iter()
            .map(|row| SourceRow::read(&row).map(|value| (value.id, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        // 新規Sourceも、所有権行をロックする前に同じトランザクションへ準備する。
        for (id, candidate) in &prepared {
            if source_rows.contains_key(id) {
                continue;
            }
            if idempotent {
                return Err(invalid().into());
            }
            sqlx::query("INSERT INTO search_source_coordination (source_id,tenant_owner_key,registration_revision,visibility_revision,activation_epoch,registration_active) VALUES ($1,$2,$3,$4,1,TRUE)")
                .bind(id.as_uuid()).bind(&candidate.tenant).bind(candidate.registration).bind(candidate.visibility)
                .execute(&mut *tx).await.map_err(statement_error)?;
        }
        let owners = sqlx::query("SELECT source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest FROM search_source_ownership WHERE source_id=ANY($1) OR source_kind=$2 ORDER BY source_id FOR UPDATE")
            .bind(&ids).bind(kind(desired.namespace())).fetch_all(&mut *tx).await.map_err(statement_error)?;
        let owner_rows = owners
            .into_iter()
            .map(|row| OwnerRow::read(&row).map(|value| (value.id, value)))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        if source_rows.len() != owner_rows.len() {
            return Err(unavailable().into());
        }
        for (id, owner) in &owner_rows {
            if !source_rows
                .get(id)
                .is_some_and(|source| source.matches(owner))
            {
                return Err(unavailable().into());
            }
        }
        let mut activations = BTreeMap::new();
        for (id, owner) in &owner_rows {
            let source = &source_rows[id];
            if let Some(candidate) = prepared.get(id) {
                candidate.validate_previous(owner, &desired.registrations()[id])?;
                if idempotent && (!owner.active || !candidate.matches(owner)) {
                    return Err(invalid().into());
                }
                let changed = !owner.active || !candidate.matches(owner);
                let activation = if changed {
                    next(owner.activation)?
                } else {
                    owner.activation
                };
                if changed {
                    let fence = next(source.fence)?;
                    update_registration(&mut tx, *id, candidate, activation, fence).await?;
                }
                activations.insert(
                    *id,
                    RegistrationActivation::from_persisted(activation as u64)?,
                );
            } else if owner.kind == kind(desired.namespace()) && owner.active {
                if idempotent {
                    return Err(invalid().into());
                }
                let activation = next(owner.activation)?;
                let fence = next(source.fence)?;
                sqlx::query("UPDATE search_source_ownership SET state='TOMBSTONED',activation_epoch=$2,updated_at=clock_timestamp() WHERE source_id=$1")
                    .bind(id.as_uuid()).bind(activation).execute(&mut *tx).await.map_err(statement_error)?;
                sqlx::query("UPDATE search_source_coordination SET registration_active=FALSE,activation_epoch=$2,fence_epoch=$3,owner_token=NULL,lease_expires_at=NULL WHERE source_id=$1")
                    .bind(id.as_uuid()).bind(activation).bind(fence).execute(&mut *tx).await.map_err(statement_error)?;
            }
        }
        for (id, candidate) in &prepared {
            if owner_rows.contains_key(id) {
                continue;
            }
            if idempotent {
                return Err(invalid().into());
            }
            sqlx::query("INSERT INTO search_source_ownership (source_id,tenant_owner_key,source_kind,registration_revision,visibility_revision,activation_epoch,state,registration_dto,registration_digest,created_at,updated_at) VALUES ($1,$2,$3,$4,$5,1,'ACTIVE',$6,$7,clock_timestamp(),clock_timestamp())")
                .bind(id.as_uuid()).bind(&candidate.tenant).bind(candidate.kind).bind(candidate.registration).bind(candidate.visibility).bind(&candidate.dto).bind(&candidate.digest)
                .execute(&mut *tx).await.map_err(statement_error)?;
            activations.insert(*id, RegistrationActivation::from_persisted(1)?);
        }
        let update_serial = match desired.namespace() {
            RegistrationNamespace::Document => {
                "UPDATE search_registration_serial SET document_deployment_revision=$1,document_desired_set_digest=$2 WHERE singleton=TRUE"
            }
            RegistrationNamespace::Remote => {
                "UPDATE search_registration_serial SET remote_deployment_revision=$1,remote_desired_set_digest=$2 WHERE singleton=TRUE"
            }
        };
        sqlx::query(update_serial)
            .bind(revision)
            .bind(&set_digest)
            .execute(&mut *tx)
            .await
            .map_err(statement_error)?;
        self.matches_host(desired).await?;
        tx.commit().await.map_err(commit_error)?;
        Ok(activations)
    }
}

impl SourceRegistrationLedgerPort for PgSourceRegistrationLedger {
    fn reconcile<'a>(
        &'a self,
        desired: &'a CompleteDesiredRegistrations,
    ) -> BoxFuture<'a, BTreeMap<SourceId, RegistrationActivation>> {
        Box::pin(async move {
            let _lock = self.reconcile_lock.lock().await;
            // 成功時だけ開く。futureのdropも含め、どの中断経路でも閉じたまま残る。
            let stamp = self.gate.begin()?;
            // 確認済みのトランザクション中断だけを同じ期待集合で最大3回試す。
            for attempt in 0..3 {
                match self.reconcile_inner(desired).await {
                    Ok(result) => {
                        self.gate.finish(stamp, desired.namespace());
                        return Ok(result);
                    }
                    Err(ReconcileFailure::Retryable) if attempt < 2 => {
                        tokio::task::yield_now().await
                    }
                    Err(ReconcileFailure::Retryable) => return Err(unavailable()),
                    Err(ReconcileFailure::Search(error)) => return Err(error),
                }
            }
            Err(unavailable())
        })
    }

    fn is_current<'a>(
        &'a self,
        registration: &'a SourceRegistration,
        activation: RegistrationActivation,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let stamp = self
                .gate
                .current_stamp(match registration.kind() {
                    SourceKind::Document => RegistrationNamespace::Document,
                    SourceKind::Remote => RegistrationNamespace::Remote,
                })
                .ok_or_else(unavailable)?;
            let expected = Prepared::new(registration)?;
            let activation = positive(activation.get())?;
            let row = sqlx::query("SELECT o.source_id,o.tenant_owner_key,o.source_kind,o.registration_revision,o.visibility_revision,o.activation_epoch,o.state,o.registration_dto,o.registration_digest FROM search_source_ownership o JOIN search_source_coordination s USING (source_id) WHERE o.source_id=$1 AND o.state='ACTIVE' AND s.registration_active AND s.tenant_owner_key=o.tenant_owner_key AND s.registration_revision=o.registration_revision AND s.visibility_revision=o.visibility_revision AND s.activation_epoch=o.activation_epoch")
                .bind(registration.source_id().as_uuid()).fetch_optional(&self.pool).await.map_err(|_| unavailable())?;
            if !self.gate.matches_stamp(stamp) {
                return Err(unavailable());
            }
            let Some(row) = row else {
                return Ok(false);
            };
            let owner = OwnerRow::read(&row)?;
            Ok(owner.activation == activation && expected.matches(&owner))
        })
    }
}

struct Prepared {
    tenant: String,
    kind: &'static str,
    registration: i64,
    visibility: i64,
    dto: Value,
    digest: String,
}
impl Prepared {
    fn new(value: &SourceRegistration) -> Result<Self, SearchError> {
        let dto = value.persistence_dto_v1()?;
        // PostgreSQLのJSONB文字列表現にも上限があるため、最終的にはDB制約も検査する。
        if serde_json::to_vec(&dto).map_err(|_| invalid())?.len() > 65_536 {
            return Err(invalid());
        }
        Ok(Self {
            tenant: value.tenant().as_str().into(),
            kind: match value.kind() {
                SourceKind::Document => "DOCUMENT",
                SourceKind::Remote => "REMOTE",
            },
            registration: positive(value.registration_revision().get())?,
            visibility: positive(value.visibility_revision().get())?,
            dto,
            digest: digest(value.persistence_digest_v1()?),
        })
    }
    fn matches(&self, owner: &OwnerRow) -> bool {
        self.tenant == owner.tenant
            && self.kind == owner.kind
            && self.registration == owner.registration
            && self.visibility == owner.visibility
            && self.dto == owner.dto
            && self.digest == owner.digest
    }
    fn validate_previous(
        &self,
        owner: &OwnerRow,
        registration: &SourceRegistration,
    ) -> Result<(), SearchError> {
        if self.tenant != owner.tenant
            || self.kind != owner.kind
            || self.registration < owner.registration
            || self.visibility < owner.visibility
            || (self.registration == owner.registration
                && !registration.matches_persisted_definition_v1(&owner.dto)?)
            || (!owner.active
                && self.registration == owner.registration
                && self.visibility == owner.visibility)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
struct OwnerRow {
    id: SourceId,
    tenant: String,
    kind: String,
    registration: i64,
    visibility: i64,
    activation: i64,
    active: bool,
    dto: Value,
    digest: String,
}
impl OwnerRow {
    fn read(row: &PgRow) -> Result<Self, SearchError> {
        let get_error = |_| unavailable();
        let state: String = row.try_get("state").map_err(get_error)?;
        let result = Self {
            id: SourceId::from_uuid(row.try_get::<Uuid, _>("source_id").map_err(get_error)?),
            tenant: row.try_get("tenant_owner_key").map_err(get_error)?,
            kind: row.try_get("source_kind").map_err(get_error)?,
            registration: row.try_get("registration_revision").map_err(get_error)?,
            visibility: row.try_get("visibility_revision").map_err(get_error)?,
            activation: row.try_get("activation_epoch").map_err(get_error)?,
            active: state == "ACTIVE",
            dto: row.try_get("registration_dto").map_err(get_error)?,
            digest: row.try_get("registration_digest").map_err(get_error)?,
        };
        if !matches!(state.as_str(), "ACTIVE" | "TOMBSTONED")
            || result.registration <= 0
            || result.visibility <= 0
            || result.activation <= 0
        {
            return Err(unavailable());
        }
        Ok(result)
    }
}
struct SourceRow {
    id: SourceId,
    fence: i64,
    tenant: Option<String>,
    registration: Option<i64>,
    visibility: Option<i64>,
    activation: Option<i64>,
    active: bool,
}
impl SourceRow {
    fn read(row: &PgRow) -> Result<Self, SearchError> {
        let get_error = |_| unavailable();
        Ok(Self {
            id: SourceId::from_uuid(row.try_get::<Uuid, _>("source_id").map_err(get_error)?),
            fence: row.try_get("fence_epoch").map_err(get_error)?,
            tenant: row.try_get("tenant_owner_key").map_err(get_error)?,
            registration: row.try_get("registration_revision").map_err(get_error)?,
            visibility: row.try_get("visibility_revision").map_err(get_error)?,
            activation: row.try_get("activation_epoch").map_err(get_error)?,
            active: row.try_get("registration_active").map_err(get_error)?,
        })
    }
    fn matches(&self, owner: &OwnerRow) -> bool {
        self.fence >= 0
            && self.tenant.as_deref() == Some(owner.tenant.as_str())
            && self.registration == Some(owner.registration)
            && self.visibility == Some(owner.visibility)
            && self.activation == Some(owner.activation)
            && self.active == owner.active
    }
}
async fn update_registration(
    conn: &mut PgConnection,
    id: SourceId,
    candidate: &Prepared,
    activation: i64,
    fence: i64,
) -> Result<(), ReconcileFailure> {
    sqlx::query("UPDATE search_source_ownership SET registration_revision=$2,visibility_revision=$3,activation_epoch=$4,state='ACTIVE',registration_dto=$5,registration_digest=$6,updated_at=clock_timestamp() WHERE source_id=$1")
        .bind(id.as_uuid()).bind(candidate.registration).bind(candidate.visibility).bind(activation).bind(&candidate.dto).bind(&candidate.digest)
        .execute(&mut *conn).await.map_err(statement_error)?;
    sqlx::query("UPDATE search_source_coordination SET registration_revision=$2,visibility_revision=$3,activation_epoch=$4,registration_active=TRUE,fence_epoch=$5,owner_token=NULL,lease_expires_at=NULL WHERE source_id=$1")
        .bind(id.as_uuid()).bind(candidate.registration).bind(candidate.visibility).bind(activation).bind(fence)
        .execute(&mut *conn).await.map_err(statement_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use outbox_delivery::{DeliveryError, runner::ClaimAdmission};
    use search_application::source_registration::{
        HostRegistrationSnapshot, SyntheticHostRegistrationAuthority,
    };
    use tokio::sync::Notify;

    #[test]
    fn retries_only_database_proven_transaction_abort_codes() {
        for code in ["40001", "40P01", "55P03"] {
            assert!(retryable_code(code));
        }
        for code in ["08006", "08007", "57014", "23514", "XX000"] {
            assert!(!retryable_code(code));
        }
    }

    #[test]
    fn gate_preserves_other_namespace_only_after_confirmed_success() {
        let gate = CurrentGate::default();
        let doc = gate.begin().unwrap();
        gate.finish(doc, RegistrationNamespace::Document);
        assert!(gate.admission_stamp().is_none());
        let remote = gate.begin().unwrap();
        gate.finish(remote, RegistrationNamespace::Remote);
        let old = gate.admission_stamp().unwrap();
        let refresh = gate.begin().unwrap();
        assert!(gate.admission_stamp().is_none());
        gate.finish(refresh, RegistrationNamespace::Document);
        assert!(gate.admission_stamp().is_some());
        assert!(!gate.matches_stamp(old));
    }

    #[test]
    fn exhausted_gate_stamp_cannot_reopen_or_repeat_an_old_stamp() {
        let gate = CurrentGate(AtomicU64::new(u64::MAX - 7));
        assert!(gate.begin().is_err());
        assert!(gate.begin().is_err());
        assert!(gate.admission_stamp().is_none());
    }

    #[test]
    fn unknown_or_cancelled_write_requires_both_namespace_rechecks() {
        let gate = CurrentGate::default();
        for namespace in [
            RegistrationNamespace::Document,
            RegistrationNamespace::Remote,
        ] {
            let stamp = gate.begin().unwrap();
            gate.finish(stamp, namespace);
        }
        let _cancelled = gate.begin().unwrap();
        assert!(gate.admission_stamp().is_none());
        let doc = gate.begin().unwrap();
        gate.finish(doc, RegistrationNamespace::Document);
        assert!(gate.admission_stamp().is_none());
        assert!(gate.current_stamp(RegistrationNamespace::Remote).is_none());
        let remote = gate.begin().unwrap();
        gate.finish(remote, RegistrationNamespace::Remote);
        assert!(gate.admission_stamp().is_some());
    }

    struct PendingHost(Arc<Notify>);
    impl HostRegistrationSnapshotPort for PendingHost {
        fn snapshot<'a>(
            &'a self,
            _: RegistrationNamespace,
        ) -> BoxFuture<'a, HostRegistrationSnapshot> {
            Box::pin(async move {
                self.0.notify_one();
                std::future::pending().await
            })
        }
    }

    #[tokio::test]
    async fn dropping_reconcile_future_keeps_shared_admission_closed_without_io() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost/unused")
            .unwrap();
        let authority = SyntheticHostRegistrationAuthority::new();
        authority
            .publish(
                RegistrationNamespace::Document,
                search_application::source_registration::RegistrationSetRevision::new(1).unwrap(),
                vec![],
            )
            .unwrap();
        let desired =
            CompleteDesiredRegistrations::capture(&authority, RegistrationNamespace::Document)
                .await
                .unwrap();
        let entered = Arc::new(Notify::new());
        let ledger = PgSourceRegistrationLedger::new(pool, Arc::new(PendingHost(entered.clone())));
        for namespace in [
            RegistrationNamespace::Document,
            RegistrationNamespace::Remote,
        ] {
            let stamp = ledger.gate.begin().unwrap();
            ledger.gate.finish(stamp, namespace);
        }
        assert!(ledger.gate.admission_stamp().is_some());
        let admission = ledger
            .source_admission(
                SourceId::from_uuid(Uuid::from_u128(1)),
                Duration::from_secs(1),
            )
            .unwrap();
        let task_ledger = ledger.clone();
        let task = tokio::spawn(async move { task_ledger.reconcile(&desired).await });
        entered.notified().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        // この検査を先に置き、ゲート欠陥の負の対照でも通信を試みない。
        assert!(ledger.gate.admission_stamp().is_none());
        assert!(matches!(
            admission.acquire().await,
            Err(DeliveryError::StoreUnknown)
        ));
    }

    #[tokio::test]
    async fn confirmed_gate_with_closed_pool_returns_store_unknown() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost/unused")
            .unwrap();
        pool.close().await;
        let ledger = PgSourceRegistrationLedger::new(
            pool,
            Arc::new(SyntheticHostRegistrationAuthority::new()),
        );
        for namespace in [
            RegistrationNamespace::Document,
            RegistrationNamespace::Remote,
        ] {
            let stamp = ledger.gate.begin().unwrap();
            ledger.gate.finish(stamp, namespace);
        }
        let admission = ledger
            .source_admission(
                SourceId::from_uuid(Uuid::from_u128(1)),
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(matches!(
            admission.acquire().await,
            Err(DeliveryError::StoreUnknown)
        ));
    }
}
