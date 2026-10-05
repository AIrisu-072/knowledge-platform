//! P4-07: owner, lease and guarded handle.

#[path = "support/remote.rs"]
mod support;

use std::sync::Arc;
use std::time::Duration;

use search_application::ports::BoxFuture;
use search_application::remote_lease::{
    GuardedRemoteStore, LeaseClock, LeaseState, OwnerDecision, RemoteLease, RemoteOwner,
    RemoteOwnerGate, ScopedOwnerGate,
};
use search_core::source::RetentionMode;
use support::*;
use tokio::sync::Notify;

struct Always;
impl RemoteOwnerGate for Always {
    fn current<'a>(&'a self, _owner: &'a RemoteOwner) -> BoxFuture<'a, OwnerDecision> {
        Box::pin(async { Ok(OwnerDecision::Current) })
    }
}

fn lease(clock: &ManualClock, absolute: u64) -> RemoteLease {
    RemoteLease {
        absolute_deadline: clock.now() + Duration::from_secs(absolute),
        idle_timeout: None,
        provider_expiry: None,
    }
}

async fn owner(remote: &Remote) -> RemoteOwner {
    let visibility = remote.visibility();
    RemoteOwner::for_evaluation(&remote.context(&visibility).await)
}

#[tokio::test]
async fn held_handle_cannot_read_after_expiry() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let owner = owner(&remote).await;
    let clock = ManualClock::new();
    let store = Arc::new(GuardedRemoteStore::new(
        owner.clone(),
        lease(&clock, 10),
        clock.clone(),
        8,
    ));
    store.open().unwrap();
    store
        .write(&owner, "projection:doc-1", "規程".to_string(), &Always)
        .await
        .unwrap();
    let held = store.clone();
    assert_eq!(
        held.read(&owner, "projection:doc-1", &Always)
            .await
            .unwrap()
            .as_deref(),
        Some("規程")
    );
    clock.advance(Duration::from_secs(11));
    assert!(
        held.read(&owner, "projection:doc-1", &Always)
            .await
            .is_err()
    );
    assert_eq!(store.state(), LeaseState::Expired);
    // Expired is final and empty for every handle.
    assert!(
        store
            .read(&owner, "projection:doc-1", &Always)
            .await
            .is_err()
    );
    assert!(
        store
            .write(&owner, "projection:doc-2", "x".into(), &Always)
            .await
            .is_err()
    );
}

/// Blocks inside the current-access check until released.
struct Blocking {
    started: Notify,
    release: Notify,
}
impl RemoteOwnerGate for Blocking {
    fn current<'a>(&'a self, _owner: &'a RemoteOwner) -> BoxFuture<'a, OwnerDecision> {
        Box::pin(async move {
            self.started.notify_one();
            self.release.notified().await;
            Ok(OwnerDecision::Current)
        })
    }
}

#[tokio::test]
async fn async_read_finishing_after_revocation_returns_nothing() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let owner = owner(&remote).await;
    let clock = ManualClock::new();
    let store = GuardedRemoteStore::new(owner.clone(), lease(&clock, 60), clock.clone(), 8);
    store.open().unwrap();
    store
        .write(&owner, "evidence:doc-1", 7u32, &Always)
        .await
        .unwrap();
    let gate = Blocking {
        started: Notify::new(),
        release: Notify::new(),
    };
    let read = store.read(&owner, "evidence:doc-1", &gate);
    let revoke = async {
        gate.started.notified().await;
        store.revoke();
        gate.release.notify_one();
    };
    let (result, ()) = tokio::join!(read, revoke);
    assert!(
        result.is_err(),
        "a read that finishes after revocation returns nothing"
    );
    assert_eq!(store.state(), LeaseState::Revoked);
}

#[tokio::test]
async fn retention_revision_change_invalidates_all_derived_entries() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let owner = RemoteOwner::for_evaluation(&context);
    let clock = ManualClock::new();
    let store = GuardedRemoteStore::new(owner.clone(), lease(&clock, 60), clock.clone(), 16);
    store.open().unwrap();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    for key in [
        "projection:doc-1",
        "selector:doc-1",
        "evidence:doc-1",
        "graph:doc-1",
        "receipt:doc-1",
    ] {
        store
            .write(&owner, key, key.to_string(), &gate)
            .await
            .unwrap();
    }
    // The Source's policy revision moves on (the visibility grant is reissued
    // at a later revision): the owner's scope is no longer current.
    visibility
        .grant(
            remote.registration.tenant().clone(),
            remote.binding.actor().principal().clone(),
            remote.registration.source_id(),
            remote.registration.registration_revision(),
            search_application::scoped::VisibilityRevision::new(2).unwrap(),
        )
        .unwrap();
    assert!(store.read(&owner, "projection:doc-1", &gate).await.is_err());
    assert_eq!(store.state(), LeaseState::Revoked);
    // Every derived entry went together, whatever gate a later reader uses.
    for key in [
        "selector:doc-1",
        "evidence:doc-1",
        "graph:doc-1",
        "receipt:doc-1",
    ] {
        assert!(store.read(&owner, key, &Always).await.is_err());
    }
}

#[tokio::test]
async fn closed_or_revoked_lease_never_reopens() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let owner = owner(&remote).await;
    let clock = ManualClock::new();
    let closed = GuardedRemoteStore::new(owner.clone(), lease(&clock, 60), clock.clone(), 8);
    closed.open().unwrap();
    closed
        .write(&owner, "content:doc-1", vec![1u8, 2, 3], &Always)
        .await
        .unwrap();
    closed.close();
    assert_eq!(closed.state(), LeaseState::Closed);
    assert!(closed.open().is_err());
    assert!(closed.read(&owner, "content:doc-1", &Always).await.is_err());
    assert!(
        closed
            .write(&owner, "content:doc-2", vec![4u8], &Always)
            .await
            .is_err()
    );

    let revoked =
        GuardedRemoteStore::<Vec<u8>>::new(owner.clone(), lease(&clock, 60), clock.clone(), 8);
    revoked.revoke();
    assert!(revoked.open().is_err());
    assert_eq!(revoked.state(), LeaseState::Revoked);
    // Another owner can never read this owner's store.
    let other = GuardedRemoteStore::new(owner.clone(), lease(&clock, 60), clock.clone(), 8);
    other.open().unwrap();
    other
        .write(&owner, "content:doc-1", vec![9u8], &Always)
        .await
        .unwrap();
    let visibility = remote.visibility();
    let stranger =
        RemoteOwner::for_evaluation(&remote.other_principal("stranger", &visibility).await);
    assert!(
        other
            .read(&stranger, "content:doc-1", &Always)
            .await
            .is_err()
    );
}
