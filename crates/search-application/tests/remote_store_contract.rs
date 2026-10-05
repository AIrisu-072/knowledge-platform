//! P4-08: expiry cache and session owner.

#[path = "support/remote.rs"]
mod support;

use std::sync::Arc;
use std::time::Duration;

use search_application::SearchError;
use search_application::materialization::{MaterializationBudget, ResourceCostEstimate};
use search_application::ports::{
    AccessDecision, BoxFuture, CurrentAccessEvaluatorPort, CurrentSourcePolicy,
    CurrentSourcePolicyPort, MaterializationReceipt, MaterializationRequest, MaterializerPort,
};
use search_application::projection::PersistableGenerationManifest;
use search_application::remote::{RemotePage, RemoteResponseInput, RemoteResponseStatus};
use search_application::remote_cache::RemoteResultCache;
use search_application::remote_lease::{
    LeaseClock, LeaseState, RemoteLease, RemoteOwner, ScopedOwnerGate,
};
use search_application::remote_session::ScopedSessionWorkingSet;
use search_application::scoped::AccessRevision;
use search_application::session::BoundResourceKey;
use search_core::binding::{BindingMode, RepresentationBinding};
use search_core::discovery::FederatedCandidate;
use search_core::id::{
    BindingId, LogicalResourceId, ProjectionGenerationId, RepresentationId, ResourceId,
};
use search_core::materialization::{MaterializationState, ProviderContentPermission};
use search_core::observation::Coverage;
use search_core::projection::{ProjectionGenerationKey, ProjectionGenerationManifest};
use search_core::source::{DiscoverableSource, EnumerationSemantics, RetentionMode};
use support::*;
use time::OffsetDateTime;
use uuid::Uuid;

fn raw(id: &str) -> RemoteResponseInput {
    RemoteResponseInput::new(
        RemoteResponseStatus::Success,
        RemotePage::Unpaged,
        vec![hit(Some(id), Some("v1"), Some("d1"), &[])],
        None,
    )
    .unwrap()
}

#[tokio::test]
async fn cache_ttl_and_acl_revision_gate_both_reads_and_writes() {
    let remote = Remote::new(RetentionMode::CacheWithExpiry).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let clock = ManualClock::new();
    let mut cache = RemoteResultCache::new(Duration::from_secs(30), 8, clock.clone());
    let now = clock.now();
    cache
        .put(
            &context,
            &query(),
            raw("doc-1"),
            now + Duration::from_secs(10),
        )
        .unwrap();
    assert!(cache.get(&context, &query(), now).unwrap().is_some());
    // Provider TTL: gone after ten seconds.
    assert!(
        cache
            .get(&context, &query(), now + Duration::from_secs(11))
            .unwrap()
            .is_none()
    );

    // A new access revision never reads entries written under the old one.
    cache
        .put(
            &context,
            &query(),
            raw("doc-1"),
            now + Duration::from_secs(10),
        )
        .unwrap();
    remote
        .authority
        .replace_access_revision(&remote.handle, AccessRevision::new(2).unwrap())
        .unwrap();
    let renewed = remote.rebind(&remote.handle, &visibility).await;
    assert!(cache.get(&renewed, &query(), now).unwrap().is_none());
    // The registration ceiling bounds even a long provider TTL.
    cache
        .put(
            &renewed,
            &query(),
            raw("doc-2"),
            now + Duration::from_secs(3600),
        )
        .unwrap();
    assert!(
        cache
            .get(&renewed, &query(), now + Duration::from_secs(29))
            .unwrap()
            .is_some()
    );
    assert!(
        cache
            .get(&renewed, &query(), now + Duration::from_secs(31))
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn cache_cannot_cross_principal_or_promote_to_durable() {
    let remote = Remote::new(RetentionMode::CacheWithExpiry).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let clock = ManualClock::new();
    let mut cache = RemoteResultCache::new(Duration::from_secs(30), 8, clock.clone());
    let now = clock.now();
    cache
        .put(
            &context,
            &query(),
            raw("doc-1"),
            now + Duration::from_secs(10),
        )
        .unwrap();
    let bob = remote.other_principal("bob", &visibility).await;
    assert!(cache.get(&bob, &query(), now).unwrap().is_none());
    // Invalidating the owner clears its entries.
    cache.invalidate_owner(&RemoteOwner::for_evaluation(&context));
    assert!(cache.is_empty());

    // Other retention modes never use the cache.
    for retention in [RetentionMode::NoRetention, RetentionMode::SessionOnly] {
        let other = Remote::new(retention).await;
        let visibility = other.visibility();
        let context = other.context(&visibility).await;
        assert!(
            cache
                .put(
                    &context,
                    &query(),
                    raw("doc-1"),
                    now + Duration::from_secs(10)
                )
                .is_err()
        );
    }
    // A cached Source cannot become a durable generation.
    let mut source = DiscoverableSource::new(
        remote.registration.source_id(),
        "synthetic",
        EnumerationSemantics::Complete,
        RetentionMode::CacheWithExpiry,
    );
    source.resource_types = vec![search_core::resource::ResourceKind::Knowledge];
    let manifest = ProjectionGenerationManifest {
        source_id: remote.registration.source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::now_v7()),
        projection_schema_version: "remote-evaluation-v1".into(),
        lens_version: 1,
        semantic_registry_version: "remote".into(),
        analyzer_version: None,
        embedding_model_version: None,
        graph_schema_version: None,
        source_snapshot: "remote-snapshot".into(),
        resource_count: 0,
        relation_count: Some(0),
        coverage: Coverage::QueryResult,
        digest: format!("sha256:{}", "0".repeat(64)),
        built_at: OffsetDateTime::now_utc(),
    };
    assert!(PersistableGenerationManifest::try_from((manifest, &source)).is_err());
}

fn session_lease(clock: &ManualClock) -> RemoteLease {
    RemoteLease {
        absolute_deadline: clock.now() + Duration::from_secs(60),
        idle_timeout: Some(Duration::from_secs(5)),
        provider_expiry: None,
    }
}

fn binding(remote: &Remote, representation: u128) -> RepresentationBinding {
    RepresentationBinding::new(
        BindingId::from_uuid(Uuid::from_u128(representation + 1)),
        LogicalResourceId::from_uuid(Uuid::from_u128(4)),
        RepresentationId::from_uuid(Uuid::from_u128(representation)),
        remote.registration.source_id(),
        BindingMode::SessionSnapshot,
        OffsetDateTime::from_unix_timestamp(100).unwrap(),
    )
}

fn generation(remote: &Remote) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: remote.registration.source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(7)),
    }
}

fn resource() -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(9))
}

#[tokio::test]
async fn session_owner_idle_absolute_and_close_expire_all_handles() {
    let remote = Remote::with_session(RetentionMode::SessionOnly).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let owner = RemoteOwner::for_session(&context).unwrap();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let key = BoundResourceKey::new(remote.registration.source_id(), resource());

    // Idle expiry.
    let clock = ManualClock::new();
    let mut set =
        ScopedSessionWorkingSet::open(&context, session_lease(&clock), clock.clone()).unwrap();
    set.bind(
        &owner,
        &gate,
        generation(&remote),
        resource(),
        "candidate",
        binding(&remote, 5),
    )
    .await
    .unwrap();
    assert!(set.read(&owner, &gate, key).await.unwrap().is_some());
    clock.advance(Duration::from_secs(6));
    assert!(set.read(&owner, &gate, key).await.is_err());
    assert_eq!(set.state(), LeaseState::Expired);

    // Absolute expiry despite activity.
    let clock = ManualClock::new();
    let mut set =
        ScopedSessionWorkingSet::open(&context, session_lease(&clock), clock.clone()).unwrap();
    set.bind(
        &owner,
        &gate,
        generation(&remote),
        resource(),
        "candidate",
        binding(&remote, 5),
    )
    .await
    .unwrap();
    for _ in 0..16 {
        clock.advance(Duration::from_secs(4));
        let _ = set.read(&owner, &gate, key).await;
    }
    assert!(set.read(&owner, &gate, key).await.is_err());
    assert_eq!(set.state(), LeaseState::Expired);

    // Close ends the session for every later call.
    let clock = ManualClock::new();
    let mut set =
        ScopedSessionWorkingSet::open(&context, session_lease(&clock), clock.clone()).unwrap();
    set.bind(
        &owner,
        &gate,
        generation(&remote),
        resource(),
        "candidate",
        binding(&remote, 5),
    )
    .await
    .unwrap();
    set.close();
    assert_eq!(set.state(), LeaseState::Closed);
    assert!(set.read(&owner, &gate, key).await.is_err());
}

#[tokio::test]
async fn no_retention_never_enters_session() {
    let clock = ManualClock::new();
    for retention in [
        RetentionMode::NoRetention,
        RetentionMode::CacheWithExpiry,
        RetentionMode::PersistentDiscoveryMetadata,
    ] {
        let remote = Remote::with_session(retention).await;
        let visibility = remote.visibility();
        let context = remote.context(&visibility).await;
        assert!(
            ScopedSessionWorkingSet::open(&context, session_lease(&clock), clock.clone()).is_err()
        );
    }
    // A session-only Source without a trusted session has no session owner.
    let remote = Remote::new(RetentionMode::SessionOnly).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    assert!(ScopedSessionWorkingSet::open(&context, session_lease(&clock), clock.clone()).is_err());
}

struct Unreachable;
impl MaterializerPort for Unreachable {
    fn materialize<'a>(
        &'a self,
        _request: &'a MaterializationRequest,
    ) -> BoxFuture<'a, MaterializationReceipt> {
        panic!("a revoked session must not reach the Source")
    }
}
impl CurrentAccessEvaluatorPort for Unreachable {
    fn evaluate<'a>(
        &'a self,
        _resource: ResourceId,
        _context: &'a str,
    ) -> BoxFuture<'a, AccessDecision> {
        panic!("a revoked session must not reach the Source")
    }
}
impl CurrentSourcePolicyPort for Unreachable {
    fn for_candidate<'a>(
        &'a self,
        _candidate: &'a FederatedCandidate,
        _context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        panic!("a revoked session must not reach the Source")
    }
    fn for_resource<'a>(
        &'a self,
        _source: search_core::id::SourceId,
        _resource: ResourceId,
        _binding: &'a RepresentationBinding,
        _context: &'a str,
    ) -> BoxFuture<'a, Option<CurrentSourcePolicy>> {
        panic!("a revoked session must not reach the Source")
    }
}

#[tokio::test]
async fn materialization_rechecks_access_budget_and_version_digest() {
    let remote = Remote::with_session(RetentionMode::SessionOnly).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let owner = RemoteOwner::for_session(&context).unwrap();
    let gate = ScopedOwnerGate::new(&remote.authority, &visibility);
    let clock = ManualClock::new();
    let mut set =
        ScopedSessionWorkingSet::open(&context, session_lease(&clock), clock.clone()).unwrap();
    let bound = binding(&remote, 5);
    set.bind(
        &owner,
        &gate,
        generation(&remote),
        resource(),
        "candidate",
        bound.clone(),
    )
    .await
    .unwrap();
    // Another representation for the same Resource (a changed live
    // version/digest) is a new discovery, never a silent rebind.
    assert!(
        set.bind(
            &owner,
            &gate,
            generation(&remote),
            resource(),
            "candidate",
            binding(&remote, 6)
        )
        .await
        .is_err()
    );

    // Access revoked: the Source is never called and the session ends.
    visibility
        .revoke(
            remote.binding.actor().principal(),
            remote.registration.source_id(),
        )
        .unwrap();
    let request = MaterializationRequest {
        resource_ref: resource(),
        binding: bound,
        current_state: MaterializationState::ReferenceOnly,
        requested_state: MaterializationState::Fragment,
        access_context: remote.handle.to_opaque_string(),
        access: AccessDecision::Allowed,
        provider_permission: ProviderContentPermission::Fragment,
        retention_mode: RetentionMode::SessionOnly,
        estimate: ResourceCostEstimate {
            content_bytes: Some(12),
            latency_ms: Some(20),
            remote_calls: Some(1),
            monetary_cost_minor_units: Some(0),
            currency: Some("USD".into()),
        },
        budget: MaterializationBudget {
            max_content_bytes: 100,
            max_latency_ms: 100,
            max_remote_calls: 1,
            max_monetary_cost_minor_units: 10,
            currency: "USD".into(),
            direct_full_max_bytes: 16,
        },
        allow_direct_full: false,
    };
    let refused: Result<MaterializationState, SearchError> = set
        .materialize(
            &owner,
            &gate,
            &Unreachable,
            &Unreachable,
            &Unreachable,
            &request,
        )
        .await;
    assert!(refused.is_err());
    assert_eq!(set.state(), LeaseState::Revoked);
    let _ = Arc::new(());
}
