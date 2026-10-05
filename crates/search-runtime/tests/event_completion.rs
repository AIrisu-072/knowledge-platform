//! P7-09: event-origin pointer CAS with the Search receipt and guard DELETE in
//! one commit, current reuse, and manual publication.

#[path = "support/bundle.rs"]
mod bundle;
#[path = "support/registration.rs"]
mod registration;
mod support;
#[path = "support/units.rs"]
mod units;

use bundle::*;
use search_application::indexing_service::DocumentSourceEvent;
use search_application::ports::{
    CurrentGenerationSnapshot, SearchCompletionOutcome, SearchDeliveryFence, SourceFence,
};
use search_runtime::event_completion::{EventCompletion, PgPublication};

async fn outbox_event(
    admin: &PgPool,
    number: u128,
    epoch: i64,
) -> (DocumentSourceEvent, SearchDeliveryFence) {
    let event = DocumentSourceEvent {
        event_id: Uuid::from_u128(number),
        event_type: "DocumentVersionPublished".into(),
        aggregate_id: Uuid::from_u128(number + 1),
        occurred_at: OffsetDateTime::UNIX_EPOCH,
    };
    let fence = SearchDeliveryFence {
        event_id: event.event_id,
        outbox_token: Uuid::from_u128(number + 2),
        source: SourceFence {
            source_id: source_id(),
            owner_token: Uuid::from_u128(7_999),
            epoch,
        },
    };
    sqlx::query(
        "INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,payload, \
         occurred_at,available_at,lease_token,lease_owner,lease_expires_at) \
         VALUES ($1,$2,'Document',$3,'{}',$4,clock_timestamp(),$5,$6, \
         clock_timestamp()+interval '1 hour')",
    )
    .bind(event.event_id)
    .bind(&event.event_type)
    .bind(event.aggregate_id)
    .bind(event.occurred_at)
    .bind(fence.outbox_token)
    .bind(Uuid::from_u128(number + 3))
    .execute(admin)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE search_source_coordination SET owner_token=$2, fence_epoch=$3, \
         lease_expires_at=clock_timestamp()+interval '1 hour' WHERE source_id=$1",
    )
    .bind(source_id().as_uuid())
    .bind(fence.source.owner_token)
    .bind(epoch)
    .execute(admin)
    .await
    .unwrap();
    (event, fence)
}

async fn guard_count(admin: &PgPool, key: ProjectionGenerationKey) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM search_generation_full_guard \
         WHERE source_id=$1 AND target_generation_id=$2",
    )
    .bind(key.source_id.as_uuid())
    .bind(key.generation_id.as_uuid())
    .fetch_one(admin)
    .await
    .unwrap()
}

async fn receipts(admin: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM search_index_receipts")
        .fetch_one(admin)
        .await
        .unwrap()
}

#[tokio::test]
async fn event_candidate_publishes_once_with_receipt_and_guard_delete() {
    let fixture = fixture().await;
    let publication = PgPublication::new(fixture.admin.clone());
    let (event, fence) = outbox_event(&fixture.admin, 8_100, 1).await;
    let built = fixture
        .build_event(7_910, "document-platform", &event, fence)
        .await;
    let verified = fixture
        .coordinator()
        .ready_event(&built.handle)
        .await
        .unwrap();
    let before = publication.current(source_id()).await.unwrap();
    assert_eq!(before.key, None);
    let outcome = publication
        .complete_event(EventCompletion::PublishCandidate {
            fence,
            expected_current: before.clone(),
            candidate: &built.handle,
            bundle: &verified,
        })
        .await
        .unwrap();
    assert_eq!(outcome, SearchCompletionOutcome::Published(built.key));
    let after = publication.current(source_id()).await.unwrap();
    assert_eq!(after.key, Some(built.key));
    assert_eq!(after.pointer_revision, before.pointer_revision + 1);
    assert_eq!(guard_count(&fixture.admin, built.key).await, 0);
    assert_eq!(receipts(&fixture.admin).await, 1);
    // Redelivery of the same event never publishes twice.
    let replay = publication
        .complete_event(EventCompletion::PublishCandidate {
            fence,
            expected_current: after.clone(),
            candidate: &built.handle,
            bundle: &verified,
        })
        .await
        .unwrap();
    assert_eq!(replay, SearchCompletionOutcome::Duplicate(built.key));
    assert_eq!(publication.current(source_id()).await.unwrap(), after);

    // A later event whose Source still has this bundle reuses it.
    let (_, next) = outbox_event(&fixture.admin, 8_200, 2).await;
    let reused = publication
        .complete_event(EventCompletion::ReuseCurrent {
            fence: next,
            expected_current: after.clone(),
            expected_manifest_digest: after.manifest_digest.clone().unwrap(),
            expected_bundle_digest: after.bundle_digest.clone().unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(reused, SearchCompletionOutcome::Unchanged(built.key));
    assert_eq!(receipts(&fixture.admin).await, 2);
}

#[tokio::test]
async fn wrong_event_old_epoch_or_lost_cas_cannot_publish() {
    let fixture = fixture().await;
    let publication = PgPublication::new(fixture.admin.clone());
    let (event_a, fence_a) = outbox_event(&fixture.admin, 8_300, 1).await;
    let built = fixture
        .build_event(7_920, "document-platform", &event_a, fence_a)
        .await;
    let verified = fixture
        .coordinator()
        .ready_event(&built.handle)
        .await
        .unwrap();
    let current = publication.current(source_id()).await.unwrap();

    // The candidate belongs to event A; event B's fence cannot publish it.
    let (_, fence_b) = outbox_event(&fixture.admin, 8_400, 1).await;
    let wrong = publication
        .complete_event(EventCompletion::PublishCandidate {
            fence: fence_b,
            expected_current: current.clone(),
            candidate: &built.handle,
            bundle: &verified,
        })
        .await
        .unwrap();
    assert_eq!(wrong, SearchCompletionOutcome::Lost);

    // A stale expected pointer is a lost CAS: no change and the guard stays.
    let stale = CurrentGenerationSnapshot {
        pointer_revision: current.pointer_revision + 5,
        ..current.clone()
    };
    let lost = publication
        .complete_event(EventCompletion::PublishCandidate {
            fence: fence_a,
            expected_current: stale,
            candidate: &built.handle,
            bundle: &verified,
        })
        .await
        .unwrap();
    assert_eq!(lost, SearchCompletionOutcome::Retry);
    assert_eq!(guard_count(&fixture.admin, built.key).await, 1);

    // After the Source epoch advances, the old fence is no longer live.
    sqlx::query("UPDATE search_source_coordination SET fence_epoch = 3 WHERE source_id=$1")
        .bind(source_id().as_uuid())
        .execute(&fixture.admin)
        .await
        .unwrap();
    let old_epoch = publication
        .complete_event(EventCompletion::PublishCandidate {
            fence: fence_a,
            expected_current: current.clone(),
            candidate: &built.handle,
            bundle: &verified,
        })
        .await
        .unwrap();
    assert_eq!(old_epoch, SearchCompletionOutcome::Lost);
    assert_eq!(publication.current(source_id()).await.unwrap(), current);
    assert_eq!(receipts(&fixture.admin).await, 0);

    // Reuse requires a current READY bundle with the expected digests.
    let (_, fence_c) = outbox_event(&fixture.admin, 8_500, 4).await;
    let no_current = publication
        .complete_event(EventCompletion::ReuseCurrent {
            fence: fence_c,
            expected_current: current.clone(),
            expected_manifest_digest: format!("sha256:{}", "a".repeat(64)),
            expected_bundle_digest: format!("sha256:{}", "b".repeat(64)),
        })
        .await
        .unwrap();
    assert_eq!(no_current, SearchCompletionOutcome::Retry);
}

#[tokio::test]
async fn manual_publication_swaps_pointer_without_any_event_receipt() {
    let fixture = fixture().await;
    let publication = PgPublication::new(fixture.admin.clone());
    let built = fixture.build(7_930, "document-platform").await;
    let verified = fixture
        .coordinator()
        .ready_manual(&built.handle)
        .await
        .unwrap();
    let before = publication.current(source_id()).await.unwrap();
    let outcome = publication
        .publish_manual(&built.handle, &verified, &before)
        .await
        .unwrap();
    assert_eq!(outcome, SearchCompletionOutcome::Published(built.key));
    let after = publication.current(source_id()).await.unwrap();
    assert_eq!(after.key, Some(built.key));
    assert_eq!(receipts(&fixture.admin).await, 0);
    assert_eq!(guard_count(&fixture.admin, built.key).await, 0);
    // The same stale expectation now loses the CAS.
    assert_eq!(
        publication
            .publish_manual(&built.handle, &verified, &before)
            .await
            .unwrap(),
        SearchCompletionOutcome::Retry
    );
}
