// P7-02の合成ホストで永続台帳を検証する。本番の全テナント列挙と
// 起動許可は、このフィクスチャの検証範囲に含まない。
mod support;

use std::sync::Arc;

use search_application::ports::BoxFuture;
use search_application::source_registration::{
    HostRegistrationSnapshot, HostRegistrationSnapshotPort, RegistrationNamespace,
    SourceRegistrationLedgerPort, SyntheticHostRegistrationAuthority,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;
use uuid::Uuid;

#[path = "support/registration.rs"]
mod registration;
use registration::*;

#[tokio::test]
async fn remote_reconcile_never_tombstones_document() {
    let (_guard, pool, host, ledger) = fixture().await;
    let doc_id = source(1001);
    let remote_id = source(1002);
    let doc = document(doc_id, "tenant-a").await;
    let remote = remote(remote_id, "tenant-b", 1);
    let desired_doc = publish(&host, RegistrationNamespace::Document, 1, vec![doc.clone()]).await;
    let desired_remote = publish(&host, RegistrationNamespace::Remote, 1, vec![remote]).await;
    let doc_activation = ledger.reconcile(&desired_doc).await.unwrap()[&doc_id];
    ledger.reconcile(&desired_remote).await.unwrap();

    let empty_remote = publish(&host, RegistrationNamespace::Remote, 2, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT source_kind,state FROM search_source_ownership ORDER BY source_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            ("DOCUMENT".into(), "ACTIVE".into()),
            ("REMOTE".into(), "TOMBSTONED".into())
        ]
    );
    assert!(ledger.is_current(&doc, doc_activation).await.unwrap());
}

#[tokio::test]
async fn partial_tenant_map_cannot_tombstone_foreign_remote() {
    let (_guard, pool, host, ledger) = fixture().await;
    let a = remote(source(1011), "tenant-a", 1);
    let b = remote(source(1012), "tenant-b", 1);
    let initial = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![a.clone(), b.clone()],
    )
    .await;
    ledger.reconcile(&initial).await.unwrap();
    let _full = publish(&host, RegistrationNamespace::Remote, 2, vec![a.clone(), b]).await;
    let foreign_host = SyntheticHostRegistrationAuthority::new();
    let partial = publish(&foreign_host, RegistrationNamespace::Remote, 2, vec![a]).await;
    assert!(ledger.reconcile(&partial).await.is_err());
    let (active, serial): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM search_source_ownership WHERE source_kind='REMOTE' AND state='ACTIVE'),remote_deployment_revision FROM search_registration_serial",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((active, serial), (2, 1));
}

#[tokio::test]
async fn document_remote_same_source_id_is_rejected_after_tombstone() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1021);
    let doc = document(id, "tenant-a").await;
    let desired_doc = publish(&host, RegistrationNamespace::Document, 1, vec![doc]).await;
    ledger.reconcile(&desired_doc).await.unwrap();
    let empty_doc = publish(&host, RegistrationNamespace::Document, 2, vec![]).await;
    ledger.reconcile(&empty_doc).await.unwrap();
    let desired_remote = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(id, "tenant-a", 1)],
    )
    .await;
    assert!(ledger.reconcile(&desired_remote).await.is_err());
    let stored: (String, String) =
        sqlx::query_as("SELECT source_kind,state FROM search_source_ownership WHERE source_id=$1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, ("DOCUMENT".into(), "TOMBSTONED".into()));
}

#[tokio::test]
async fn partial_or_stale_remote_desired_set_is_atomic_failure() {
    let (_guard, pool, host, ledger) = fixture().await;
    let a = remote(source(1031), "tenant-a", 1);
    let b = remote(source(1032), "tenant-b", 1);
    let first = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![a.clone(), b.clone()],
    )
    .await;
    ledger.reconcile(&first).await.unwrap();
    let next = publish(&host, RegistrationNamespace::Remote, 2, vec![a.clone(), b]).await;
    assert!(ledger.reconcile(&first).await.is_err());
    let foreign = SyntheticHostRegistrationAuthority::new();
    let partial = publish(&foreign, RegistrationNamespace::Remote, 2, vec![a]).await;
    assert!(ledger.reconcile(&partial).await.is_err());
    let revision: i64 =
        sqlx::query_scalar("SELECT remote_deployment_revision FROM search_registration_serial")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(revision, 1);
    assert_eq!(next.len(), 2);
}

#[tokio::test]
async fn same_revision_same_digest_is_idempotent_across_ledger_instances() {
    let (_guard, pool, host, first_ledger) = fixture().await;
    let id = source(1041);
    let registration = remote(id, "tenant-a", 1);
    let desired = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![registration.clone()],
    )
    .await;
    let first = first_ledger.reconcile(&desired).await.unwrap();
    let second_ledger = PgSourceRegistrationLedger::new(pool.clone(), host);
    let second = second_ledger.reconcile(&desired).await.unwrap();
    assert_eq!(first, second);
    assert!(
        second_ledger
            .is_current(&registration, second[&id])
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn persisted_full_dto_controls_current_even_when_digest_is_unchanged() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1051);
    let registration = remote(id, "tenant-a", 1);
    let desired = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![registration.clone()],
    )
    .await;
    let activation = ledger.reconcile(&desired).await.unwrap()[&id];
    assert!(ledger.is_current(&registration, activation).await.unwrap());

    let restarted = PgSourceRegistrationLedger::new(pool.clone(), host);
    restarted.reconcile(&desired).await.unwrap();

    // 合成DBの破損注入で所有者トリガーだけを一時停止する。
    // 保存digestを変えず、全DTO照合を省略した実装を拒否する。
    sqlx::query("ALTER TABLE search_source_ownership DISABLE TRIGGER search_guard_owner_identity")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_ownership SET registration_dto=registration_dto || '{\"unexpected_field\":true}'::jsonb WHERE source_id=$1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE search_source_ownership ENABLE TRIGGER search_guard_owner_identity")
        .execute(&pool)
        .await
        .unwrap();

    assert!(!matches!(
        ledger.is_current(&registration, activation).await,
        Ok(true)
    ));
    assert!(!matches!(
        restarted.is_current(&registration, activation).await,
        Ok(true)
    ));
}

#[tokio::test]
async fn changed_registration_fences_lease_without_rewriting_current_pointer() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1061);
    let generation_id = Uuid::from_u128(1062);
    let old = document(id, "tenant-a").await;
    let first = publish(&host, RegistrationNamespace::Document, 1, vec![old.clone()]).await;
    let old_activation = ledger.reconcile(&first).await.unwrap()[&id];
    let manifest = format!("sha256:{}", "a".repeat(64));
    let bundle = format!("sha256:{}", "b".repeat(64));
    sqlx::query("INSERT INTO search_generation_identity (source_id,generation_id,tenant_owner_key,activation_epoch,created_at) VALUES ($1,$2,'tenant-a',1,clock_timestamp())")
        .bind(id.as_uuid())
        .bind(generation_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(r#"INSERT INTO search_generation (source_id,generation_id,activation_epoch,state,build_kind,stage_origin,source_snapshot,projection_manifest,projection_manifest_digest,projection_resource_count,bundle_version) VALUES ($1,$2,1,'BUILDING','INCREMENTAL','MANUAL','registration-test','{"dto_version":"v1"}'::jsonb,$3,0,'v1')"#)
        .bind(id.as_uuid())
        .bind(generation_id)
        .bind(&manifest)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET fence_epoch=1,current_generation_id=$2,current_manifest_digest=$3,current_bundle_digest=$4,pointer_revision=1,last_published_epoch=1 WHERE source_id=$1")
        .bind(id.as_uuid())
        .bind(generation_id)
        .bind(&manifest)
        .bind(&bundle)
        .execute(&pool)
        .await
        .unwrap();

    let changed = document_with_revision(id, "tenant-a", 2).await;
    let second = publish(
        &host,
        RegistrationNamespace::Document,
        2,
        vec![changed.clone()],
    )
    .await;
    let new_activation = ledger.reconcile(&second).await.unwrap()[&id];
    assert!(new_activation.get() > old_activation.get());
    let stored: (i64, i64, Uuid, String, String, i64) = sqlx::query_as(
        "SELECT activation_epoch,fence_epoch,current_generation_id,current_manifest_digest,current_bundle_digest,pointer_revision FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored.0, i64::try_from(new_activation.get()).unwrap());
    assert!(stored.1 > 1);
    assert_eq!(
        (stored.2, stored.3, stored.4, stored.5),
        (generation_id, manifest, bundle, 1)
    );
    assert!(!ledger.is_current(&old, old_activation).await.unwrap());
    assert!(ledger.is_current(&changed, new_activation).await.unwrap());
}

#[tokio::test]
async fn activation_overflow_aborts_whole_reconcile() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1071);
    let original = remote(id, "tenant-a", 1);
    let first = publish(&host, RegistrationNamespace::Remote, 1, vec![original]).await;
    ledger.reconcile(&first).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE search_source_ownership SET activation_epoch=9223372036854775807 WHERE source_id=$1")
        .bind(id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE search_source_coordination SET activation_epoch=9223372036854775807 WHERE source_id=$1")
        .bind(id.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let changed = remote(id, "tenant-a", 2);
    let next = publish(&host, RegistrationNamespace::Remote, 2, vec![changed]).await;
    assert!(ledger.reconcile(&next).await.is_err());
    let unchanged: (i64, i64, i64) = sqlx::query_as(
        "SELECT s.activation_epoch,s.fence_epoch,r.remote_deployment_revision FROM search_source_coordination s CROSS JOIN search_registration_serial r WHERE s.source_id=$1",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged, (i64::MAX, 0, 1));
}
struct AdvancingHost {
    first: HostRegistrationSnapshot,
    second: HostRegistrationSnapshot,
    calls: std::sync::atomic::AtomicUsize,
}

impl HostRegistrationSnapshotPort for AdvancingHost {
    fn snapshot<'a>(
        &'a self,
        _namespace: RegistrationNamespace,
    ) -> BoxFuture<'a, HostRegistrationSnapshot> {
        Box::pin(async move {
            let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if call == 0 {
                self.first.clone()
            } else {
                self.second.clone()
            })
        })
    }
}

#[tokio::test]
async fn host_change_before_commit_rolls_back_every_registration() {
    let (_guard, pool, host, _) = fixture().await;
    let initial = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(source(1081), "tenant-a", 1)],
    )
    .await;
    let first = host.snapshot(RegistrationNamespace::Remote).await.unwrap();
    publish(&host, RegistrationNamespace::Remote, 2, vec![]).await;
    let second = host.snapshot(RegistrationNamespace::Remote).await.unwrap();
    let changing = Arc::new(AdvancingHost {
        first,
        second,
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let ledger = PgSourceRegistrationLedger::new(pool.clone(), changing);
    assert!(ledger.reconcile(&initial).await.is_err());
    let state: (i64, Option<i64>) = sqlx::query_as("SELECT (SELECT count(*) FROM search_source_ownership),remote_deployment_revision FROM search_registration_serial")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(state, (0, None));
}

#[tokio::test]
async fn same_set_revision_conflict_and_owner_reuse_are_atomic_failures() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1091);
    let old = remote(id, "tenant-a", 1);
    let first = publish(&host, RegistrationNamespace::Remote, 1, vec![old.clone()]).await;
    let activation = ledger.reconcile(&first).await.unwrap()[&id];
    // 台帳より新しいホスト内容でも、同じ配備改訂の再解釈を許さない。
    let conflicting_host = Arc::new(SyntheticHostRegistrationAuthority::new());
    let conflict = publish(
        &conflicting_host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(id, "tenant-a", 2)],
    )
    .await;
    let other = PgSourceRegistrationLedger::new(pool.clone(), conflicting_host.clone());
    assert!(other.reconcile(&conflict).await.is_err());
    let empty = publish(&host, RegistrationNamespace::Remote, 2, vec![]).await;
    ledger.reconcile(&empty).await.unwrap();
    let changed_owner = publish(
        &host,
        RegistrationNamespace::Remote,
        3,
        vec![remote(id, "tenant-b", 2)],
    )
    .await;
    assert!(ledger.reconcile(&changed_owner).await.is_err());
    assert!(!matches!(
        ledger.is_current(&old, activation).await,
        Ok(true)
    ));
    let row: (String, String, i64) = sqlx::query_as("SELECT tenant_owner_key,state,remote_deployment_revision FROM search_source_ownership CROSS JOIN search_registration_serial WHERE source_id=$1")
        .bind(id.as_uuid()).fetch_one(&pool).await.unwrap();
    assert_eq!(row, ("tenant-a".into(), "TOMBSTONED".into(), 2));
}

#[tokio::test]
async fn concurrent_ledgers_commit_one_idempotent_activation() {
    let (_guard, pool, host, first_ledger) = fixture().await;
    let desired = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(source(1101), "tenant-a", 1)],
    )
    .await;
    let other_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let second_ledger = PgSourceRegistrationLedger::new(other_pool.clone(), host);
    let (first, second) = tokio::join!(
        first_ledger.reconcile(&desired),
        second_ledger.reconcile(&desired)
    );
    assert_eq!(first.unwrap(), second.unwrap());
    let activation: i64 =
        sqlx::query_scalar("SELECT activation_epoch FROM search_source_ownership")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(activation, 1);
    other_pool.close().await;
}

#[tokio::test]
async fn stale_registration_and_unversioned_reactivation_are_refused() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1111);
    let old = remote(id, "tenant-a", 2);
    let first = publish(&host, RegistrationNamespace::Remote, 1, vec![old.clone()]).await;
    ledger.reconcile(&first).await.unwrap();
    let stale = publish(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![remote(id, "tenant-a", 1)],
    )
    .await;
    assert!(ledger.reconcile(&stale).await.is_err());
    let empty = publish(&host, RegistrationNamespace::Remote, 3, vec![]).await;
    ledger.reconcile(&empty).await.unwrap();
    let resurrect = publish(&host, RegistrationNamespace::Remote, 4, vec![old]).await;
    assert!(ledger.reconcile(&resurrect).await.is_err());
    let row: (String, i64) = sqlx::query_as("SELECT state,remote_deployment_revision FROM search_source_ownership CROSS JOIN search_registration_serial")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(row, ("TOMBSTONED".into(), 3));
}

#[tokio::test]
async fn revision_outside_bigint_is_rejected_without_a_partial_insert() {
    let (_guard, pool, host, ledger) = fixture().await;
    let desired = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(source(1121), "tenant-a", u64::MAX)],
    )
    .await;
    assert!(ledger.reconcile(&desired).await.is_err());
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM search_source_coordination")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[tokio::test]
async fn cancellation_while_commit_waits_keeps_catalog_and_existing_lease_closed() {
    use outbox_delivery::{
        DeliveryError,
        runner::{ClaimAdmission, ClaimPermit},
    };
    use std::time::Duration;
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1131);
    let old = document(id, "tenant-a").await;
    let initial = publish(&host, RegistrationNamespace::Document, 1, vec![old.clone()]).await;
    let old_activation = ledger.reconcile(&initial).await.unwrap()[&id];
    let remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&remote).await.unwrap();
    let admission = ledger
        .source_admission(id, Duration::from_secs(30))
        .unwrap();
    let lease = admission.acquire().await.unwrap().unwrap();

    // 合成DB内の遅延トリガーを、別接続のadvisory lockでcommit中だけ止める。
    sqlx::raw_sql("CREATE FUNCTION registration_commit_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(1131); RETURN NULL; END $$; CREATE CONSTRAINT TRIGGER registration_commit_barrier AFTER UPDATE ON search_registration_serial DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION registration_commit_barrier();")
        .execute(&pool).await.unwrap();
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(1131)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let changed = document_with_revision(id, "tenant-a", 2).await;
    let desired = publish(
        &host,
        RegistrationNamespace::Document,
        2,
        vec![changed.clone()],
    )
    .await;
    let task_ledger = ledger.clone();
    let task_desired = desired.clone();
    let task = tokio::spawn(async move { task_ledger.reconcile(&task_desired).await });
    let reached_commit = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype='advisory' AND database=(SELECT oid FROM pg_database WHERE datname=current_database()) AND objid=1131 AND NOT granted)")
                .fetch_one(&pool).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await;
    task.abort();
    let stopped = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("cancelled reconcile must stop");
    if reached_commit.is_err() {
        tokio::time::timeout(Duration::from_secs(5), blocker.rollback())
            .await
            .expect("barrier rollback must stop")
            .unwrap();
        panic!("commit must reach the deferred barrier");
    }
    assert!(stopped.unwrap_err().is_cancelled());
    assert!(matches!(
        admission.acquire().await,
        Err(DeliveryError::StoreUnknown)
    ));
    assert!(matches!(
        lease.renew().await,
        Err(DeliveryError::StoreUnknown)
    ));
    assert!(!matches!(
        ledger.is_current(&old, old_activation).await,
        Ok(true)
    ));
    tokio::time::timeout(Duration::from_secs(5), blocker.rollback())
        .await
        .expect("barrier rollback must stop")
        .unwrap();
    // commit済み/未commitの推定をせず、同じhostの全状態を再照合して解除する。
    let activation = tokio::time::timeout(Duration::from_secs(10), ledger.reconcile(&desired))
        .await
        .expect("registration recovery must finish")
        .unwrap()[&id];
    assert!(matches!(
        admission.acquire().await,
        Err(DeliveryError::StoreUnknown)
    ));
    tokio::time::timeout(Duration::from_secs(10), ledger.reconcile(&remote))
        .await
        .expect("namespace recovery must finish")
        .unwrap();
    assert!(ledger.is_current(&changed, activation).await.unwrap());
    assert!(admission.acquire().await.unwrap().is_some());
}

#[tokio::test]
async fn visibility_only_change_advances_activation_but_definition_needs_revision() {
    use search_application::{
        remote_registration::RemoteSourceRegistration, source_registration::SourceRegistration,
    };
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1141);
    let old = remote(id, "tenant-a", 1);
    let initial = publish(&host, RegistrationNamespace::Remote, 1, vec![old.clone()]).await;
    let old_activation = ledger.reconcile(&initial).await.unwrap()[&id];
    let mut config = remote_config(id, "tenant-a", 1);
    config.visibility_revision = visibility(2);
    let changed = SourceRegistration::Remote(
        RemoteSourceRegistration::from_server_config(config.clone()).unwrap(),
    );
    let next = publish(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![changed.clone()],
    )
    .await;
    let activation = ledger.reconcile(&next).await.unwrap()[&id];
    assert!(activation.get() > old_activation.get());
    assert!(!ledger.is_current(&old, old_activation).await.unwrap());
    assert!(ledger.is_current(&changed, activation).await.unwrap());
    config.provider_kind = "provider-b".into();
    let unversioned =
        SourceRegistration::Remote(RemoteSourceRegistration::from_server_config(config).unwrap());
    let invalid = publish(&host, RegistrationNamespace::Remote, 3, vec![unversioned]).await;
    assert!(ledger.reconcile(&invalid).await.is_err());
    let stored: (i64,i64,i64) = sqlx::query_as("SELECT registration_revision,visibility_revision,activation_epoch FROM search_source_ownership")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(stored, (1, 2, activation.get() as i64));
}

#[tokio::test]
async fn retryable_transaction_abort_rechecks_whole_set_without_partial_activation() {
    let (_guard, pool, host, ledger) = fixture().await;
    sqlx::raw_sql("CREATE SEQUENCE registration_retry_attempt; CREATE FUNCTION registration_retry_twice() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF nextval('registration_retry_attempt') < 3 THEN RAISE EXCEPTION 'synthetic serialization failure' USING ERRCODE='40001'; END IF; RETURN NEW; END $$; CREATE TRIGGER registration_retry_twice BEFORE UPDATE ON search_registration_serial FOR EACH ROW EXECUTE FUNCTION registration_retry_twice();")
        .execute(&pool).await.unwrap();
    let id = source(1151);
    let desired = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(id, "tenant-a", 1)],
    )
    .await;
    let activation = ledger.reconcile(&desired).await.unwrap()[&id];
    assert_eq!(activation.get(), 1);
    let state: (i64,i64) = sqlx::query_as("SELECT (SELECT last_value FROM registration_retry_attempt),(SELECT count(*) FROM search_source_ownership)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(state, (3, 1));
}

#[tokio::test]
async fn repeated_transaction_abort_exhausts_bounded_retry_and_stays_closed() {
    let (_guard, pool, host, ledger) = fixture().await;
    sqlx::raw_sql("CREATE SEQUENCE registration_retry_attempt; CREATE FUNCTION registration_retry_always() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM nextval('registration_retry_attempt'); RAISE EXCEPTION 'synthetic deadlock' USING ERRCODE='40P01'; END $$; CREATE TRIGGER registration_retry_always BEFORE UPDATE ON search_registration_serial FOR EACH ROW EXECUTE FUNCTION registration_retry_always();")
        .execute(&pool).await.unwrap();
    let id = source(1161);
    let desired = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(id, "tenant-a", 1)],
    )
    .await;
    assert!(ledger.reconcile(&desired).await.is_err());
    let state: (i64,i64) = sqlx::query_as("SELECT (SELECT last_value FROM registration_retry_attempt),(SELECT count(*) FROM search_source_coordination)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(state, (3, 0));
}

#[tokio::test]
async fn fence_overflow_alone_aborts_whole_reconcile() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(1171);
    let initial = publish(
        &host,
        RegistrationNamespace::Remote,
        1,
        vec![remote(id, "tenant-a", 1)],
    )
    .await;
    ledger.reconcile(&initial).await.unwrap();
    sqlx::query(
        "UPDATE search_source_coordination SET fence_epoch=9223372036854775807 WHERE source_id=$1",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    let changed = publish(
        &host,
        RegistrationNamespace::Remote,
        2,
        vec![remote(id, "tenant-a", 2)],
    )
    .await;
    assert!(ledger.reconcile(&changed).await.is_err());
    let unchanged: (i64,i64,i64) = sqlx::query_as("SELECT s.activation_epoch,s.fence_epoch,r.remote_deployment_revision FROM search_source_coordination s CROSS JOIN search_registration_serial r WHERE s.source_id=$1")
        .bind(id.as_uuid()).fetch_one(&pool).await.unwrap();
    assert_eq!(unchanged, (1, i64::MAX, 1));
}
