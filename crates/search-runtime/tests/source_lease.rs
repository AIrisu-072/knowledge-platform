#[path = "support/registration.rs"]
mod registration;
mod support;

use std::time::Duration;

use outbox_delivery::model::{DeliveryError, FenceResult};
use outbox_delivery::runner::{ClaimAdmission, ClaimPermit};
use search_application::ports::SearchSourceLease;
use search_application::source_registration::SyntheticHostRegistrationAuthority;
use search_application::source_registration::{
    RegistrationNamespace, SourceRegistrationLedgerPort,
};
use search_runtime::source_registration::PgSourceRegistrationLedger;

use registration::*;

#[tokio::test]
async fn two_four_eight_connections_have_one_source_owner() {
    let (_guard, pool, host, ledger) = fixture().await;
    let empty_remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let id = source(2001);
    let desired = publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document(id, "tenant-a").await],
    )
    .await;
    ledger.reconcile(&desired).await.unwrap();
    let mut last_epoch = 0;
    for count in [2, 4, 8] {
        let mut tasks = tokio::task::JoinSet::new();
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(count));
        let mut pools = Vec::new();
        for _ in 0..count {
            let independent = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect_with((*pool.connect_options()).clone())
                .await
                .unwrap();
            pools.push(independent.clone());
            let peer = PgSourceRegistrationLedger::new(independent, host.clone());
            peer.reconcile(&desired).await.unwrap();
            peer.reconcile(&empty_remote).await.unwrap();
            let admission = peer.source_admission(id, Duration::from_secs(30)).unwrap();
            assert_eq!(admission.max_claims_per_permit(), 1);
            let barrier = barrier.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                admission.acquire().await.unwrap()
            });
        }
        let mut winners = Vec::new();
        while let Some(result) = tasks.join_next().await {
            if let Some(lease) = result.unwrap() {
                winners.push(lease);
            }
        }
        assert_eq!(winners.len(), 1);
        let winner = winners.pop().unwrap();
        assert!(winner.fence().epoch > last_epoch);
        last_epoch = winner.fence().epoch;
        assert_eq!(winner.preflight().await.unwrap(), FenceResult::Updated);
        assert_eq!(winner.release().await.unwrap(), FenceResult::Updated);
        for independent in pools {
            independent.close().await;
        }
    }
}

#[tokio::test]
async fn lost_owner_cannot_renew_release_or_recover_a_new_owner() {
    let (_guard, pool, host, ledger) = fixture().await;
    let empty_remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let id = source(2011);
    let desired = publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document(id, "tenant-a").await],
    )
    .await;
    ledger.reconcile(&desired).await.unwrap();
    let admission = ledger
        .source_admission(id, Duration::from_secs(30))
        .unwrap();
    let old = admission.acquire().await.unwrap().unwrap();
    sqlx::query("UPDATE search_source_coordination SET lease_expires_at=clock_timestamp() WHERE source_id=$1")
        .bind(id.as_uuid()).execute(&pool).await.unwrap();
    assert_eq!(old.preflight().await.unwrap(), FenceResult::Lost);
    assert_eq!(old.renew().await.unwrap(), FenceResult::Lost);
    assert_eq!(old.release().await.unwrap(), FenceResult::Lost);
    let new = admission.acquire().await.unwrap().unwrap();
    assert!(new.fence().epoch > old.fence().epoch);
    assert_ne!(new.fence().owner_token, old.fence().owner_token);
    assert_eq!(old.renew().await.unwrap(), FenceResult::Lost);
    assert_eq!(old.release().await.unwrap(), FenceResult::Lost);
    assert_eq!(new.preflight().await.unwrap(), FenceResult::Updated);
}

#[tokio::test]
async fn registration_change_invalidates_existing_lease_and_tombstone_refuses_acquire() {
    let (_guard, _pool, host, ledger) = fixture().await;
    let empty_remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let id = source(2021);
    let first = publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document(id, "tenant-a").await],
    )
    .await;
    ledger.reconcile(&first).await.unwrap();
    let admission = ledger
        .source_admission(id, Duration::from_secs(30))
        .unwrap();
    let lease = admission.acquire().await.unwrap().unwrap();
    let changed = publish(
        &host,
        RegistrationNamespace::Document,
        2,
        vec![document_with_revision(id, "tenant-a", 2).await],
    )
    .await;
    ledger.reconcile(&changed).await.unwrap();
    assert_eq!(lease.preflight().await.unwrap(), FenceResult::Lost);
    assert_eq!(lease.renew().await.unwrap(), FenceResult::Lost);
    assert_eq!(lease.release().await.unwrap(), FenceResult::Lost);
    let current = admission.acquire().await.unwrap().unwrap();
    assert!(current.fence().epoch > lease.fence().epoch);
    let empty = publish(&host, RegistrationNamespace::Document, 3, vec![]).await;
    ledger.reconcile(&empty).await.unwrap();
    assert!(admission.acquire().await.unwrap().is_none());
}

#[tokio::test]
async fn source_epoch_overflow_and_missing_source_refuse_acquire() {
    let (_guard, pool, host, ledger) = fixture().await;
    let empty_remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&empty_remote).await.unwrap();
    let id = source(2031);
    let admission = ledger
        .source_admission(id, Duration::from_secs(30))
        .unwrap();
    let empty_document = publish(&host, RegistrationNamespace::Document, 1, vec![]).await;
    ledger.reconcile(&empty_document).await.unwrap();
    assert!(admission.acquire().await.unwrap().is_none());
    let desired = publish(
        &host,
        RegistrationNamespace::Document,
        2,
        vec![document(id, "tenant-a").await],
    )
    .await;
    ledger.reconcile(&desired).await.unwrap();
    sqlx::query(
        "UPDATE search_source_coordination SET fence_epoch=9223372036854775807 WHERE source_id=$1",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .unwrap();
    assert!(admission.acquire().await.unwrap().is_none());
    let state: (i64, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT fence_epoch,owner_token FROM search_source_coordination WHERE source_id=$1",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, (i64::MAX, None));
}

#[tokio::test]
async fn unreconciled_closed_pool_is_unknown_never_a_permit() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://localhost/unused")
        .unwrap();
    pool.close().await;
    let ledger = PgSourceRegistrationLedger::new(
        pool,
        std::sync::Arc::new(SyntheticHostRegistrationAuthority::new()),
    );
    let admission = ledger
        .source_admission(source(2041), Duration::from_secs(30))
        .unwrap();
    assert!(matches!(
        admission.acquire().await,
        Err(DeliveryError::StoreUnknown)
    ));
}

#[tokio::test]
async fn zero_oversized_and_submicrosecond_ttl_are_invalid() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://localhost/unused")
        .unwrap();
    let ledger = PgSourceRegistrationLedger::new(
        pool,
        std::sync::Arc::new(SyntheticHostRegistrationAuthority::new()),
    );
    for ttl in [Duration::ZERO, Duration::from_nanos(1), Duration::MAX] {
        assert!(matches!(
            ledger.source_admission(source(2051), ttl),
            Err(DeliveryError::InvalidConfig)
        ));
    }
}

#[derive(Default)]
struct ClaimCounter(std::sync::atomic::AtomicUsize);
impl outbox_delivery::OutboxStore for ClaimCounter {
    fn verify_policy(&self) -> outbox_delivery::DeliveryFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn claim(
        &self,
        _: uuid::Uuid,
        _: u32,
        _: Duration,
    ) -> outbox_delivery::DeliveryFuture<'_, Vec<outbox_delivery::ClaimedEvent>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok(vec![]) })
    }
    fn renew(
        &self,
        _: uuid::Uuid,
        _: uuid::Uuid,
        _: Duration,
    ) -> outbox_delivery::DeliveryFuture<'_, FenceResult> {
        unreachable!("no claim is permitted")
    }
    fn settle_success(
        &self,
        _: uuid::Uuid,
        _: uuid::Uuid,
    ) -> outbox_delivery::DeliveryFuture<'_, FenceResult> {
        unreachable!("no claim is permitted")
    }
    fn settle_failure(
        &self,
        _: uuid::Uuid,
        _: uuid::Uuid,
        _: outbox_delivery::ErrorCode,
        _: bool,
        _: Duration,
    ) -> outbox_delivery::DeliveryFuture<'_, FenceResult> {
        unreachable!("no claim is permitted")
    }
    fn reap_exhausted(&self, _: u32) -> outbox_delivery::DeliveryFuture<'_, u64> {
        Box::pin(async { Ok(0) })
    }
}
struct UnexpectedHandler;
impl outbox_delivery::runner::DeliveryHandler<search_runtime::source_lease::SourceLease>
    for UnexpectedHandler
{
    fn deliver(
        &self,
        _: outbox_delivery::DeliveryEnvelope,
        _: outbox_delivery::runner::DeliveryContext,
        _: search_runtime::source_lease::SourceLease,
    ) -> outbox_delivery::HandlerFuture<'_, outbox_delivery::DeliveryDecision> {
        unreachable!("no claim is permitted")
    }
}

#[tokio::test]
async fn source_acquire_commit_error_never_calls_outbox_claim() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(2061);
    let remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&remote).await.unwrap();
    let document = publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document(id, "tenant-a").await],
    )
    .await;
    ledger.reconcile(&document).await.unwrap();
    // RETURNINGの行を受け取っていても、暗黙commitで失敗すればpermitを返さない。
    sqlx::raw_sql("CREATE FUNCTION source_lease_commit_error() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic commit response error' USING ERRCODE='08007'; END $$; CREATE CONSTRAINT TRIGGER source_lease_commit_error AFTER UPDATE ON search_source_coordination DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION source_lease_commit_error();")
        .execute(&pool).await.unwrap();
    let admission = ledger
        .source_admission(id, Duration::from_secs(30))
        .unwrap();
    let counter = std::sync::Arc::new(ClaimCounter::default());
    let runner = outbox_delivery::runner::DeliveryRunner::new(
        counter.clone(),
        std::sync::Arc::new(UnexpectedHandler),
        std::sync::Arc::new(admission),
        outbox_delivery::DeliveryConfig::default(),
    )
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), runner.run_cycle())
        .await
        .unwrap();
    assert_eq!(result, Err(DeliveryError::StoreUnknown));
    assert_eq!(counter.0.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn source_renew_commit_error_never_reports_updated() {
    let (_guard, pool, host, ledger) = fixture().await;
    let id = source(2071);
    let remote = publish(&host, RegistrationNamespace::Remote, 1, vec![]).await;
    ledger.reconcile(&remote).await.unwrap();
    let document = publish(
        &host,
        RegistrationNamespace::Document,
        1,
        vec![document(id, "tenant-a").await],
    )
    .await;
    ledger.reconcile(&document).await.unwrap();
    let admission = ledger
        .source_admission(id, Duration::from_secs(30))
        .unwrap();
    let lease = admission.acquire().await.unwrap().unwrap();
    sqlx::raw_sql("CREATE FUNCTION source_lease_commit_error() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic commit response error' USING ERRCODE='08007'; END $$; CREATE CONSTRAINT TRIGGER source_lease_commit_error AFTER UPDATE ON search_source_coordination DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION source_lease_commit_error();")
        .execute(&pool).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), lease.renew())
            .await
            .unwrap(),
        Err(DeliveryError::StoreUnknown)
    );
    assert!(admission.acquire().await.unwrap().is_none());
}
