//! P4-06: stable identity and one sealed Source batch per evaluation.

#[path = "support/remote.rs"]
mod support;

use search_application::remote::EvaluationLeaseId;
use search_application::remote_generation::{RemoteGenerationBuilder, StageOutcome};
use search_application::remote_identity::{remote_candidate_id, remote_resource_id};
use search_core::discovery::{CandidateIdentityClass, GapReason};
use search_core::source::RetentionMode;
use support::*;

fn builder(
    remote: &Remote,
    context: search_application::remote::TrustedRemoteContext,
) -> RemoteGenerationBuilder {
    RemoteGenerationBuilder::new(context, remote.evaluation(), EvaluationLeaseId::new()).unwrap()
}

#[tokio::test]
async fn query_and_lookup_share_one_key_when_snapshot_matches() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let verifier = Verifier::shared("snapshot-1");
    let doc = hit(
        Some("doc-1"),
        Some("v1"),
        Some("d1"),
        &[("title", "規程", None)],
    );
    let searched = observe(
        &remote,
        &visibility,
        &verifier,
        &context,
        "search",
        query(),
        vec![doc.clone()],
    )
    .await;
    let looked = observe(
        &remote,
        &visibility,
        &verifier,
        &context,
        "lookup",
        lookup("doc-1"),
        vec![doc],
    )
    .await;
    let mut batch = builder(&remote, context);
    assert_eq!(batch.stage(searched).unwrap(), StageOutcome::Staged);
    assert_eq!(batch.stage(looked).unwrap(), StageOutcome::Staged);
    let generation = batch.seal().unwrap();
    let id = remote_resource_id(
        remote.registration.tenant(),
        remote.registration.source_id(),
        "synthetic",
        &native("doc-1"),
    )
    .unwrap();
    assert_eq!(
        generation.resource_ids().into_iter().collect::<Vec<_>>(),
        vec![id]
    );
    for retriever in ["search", "lookup"] {
        let candidates = generation.candidates(retriever).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].resource_ref, Some(id));
        assert_eq!(
            candidates[0].retrieval_trace_ref.as_deref(),
            Some(
                format!(
                    "{}:{}",
                    generation.key().source_id.as_uuid(),
                    generation.key().generation_id.as_uuid()
                )
                .as_str()
            )
        );
    }
    assert_eq!(generation.receipts().len(), 2);
}

#[tokio::test]
async fn conflicting_projection_aborts_source_batch() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let verifier = Verifier::shared("snapshot-1");
    // Same native ID and version: a different digest, then a different field.
    for second in [
        hit(Some("doc-1"), Some("v1"), Some("d2"), &[]),
        hit(
            Some("doc-1"),
            Some("v1"),
            Some("d1"),
            &[("title", "別の規程", None)],
        ),
    ] {
        let first = observe(
            &remote,
            &visibility,
            &verifier,
            &context,
            "search",
            query(),
            vec![hit(
                Some("doc-1"),
                Some("v1"),
                Some("d1"),
                &[("title", "規程", None)],
            )],
        )
        .await;
        let other = observe(
            &remote,
            &visibility,
            &verifier,
            &context,
            "lookup",
            lookup("doc-1"),
            vec![second],
        )
        .await;
        let mut batch = builder(&remote, context.clone());
        batch.stage(first).unwrap();
        assert!(batch.stage(other).is_err());
        // Nothing of the batch can be sealed after the conflict.
        assert!(batch.seal().is_err());
    }
}

#[tokio::test]
async fn different_snapshot_or_acl_revision_aborts_batch() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let first = observe(
        &remote,
        &visibility,
        &Verifier::shared("snapshot-1"),
        &context,
        "search",
        query(),
        vec![hit(Some("doc-1"), Some("v1"), Some("d1"), &[])],
    )
    .await;
    let other = observe(
        &remote,
        &visibility,
        &Verifier::shared("snapshot-2"),
        &context,
        "lookup",
        lookup("doc-1"),
        vec![hit(Some("doc-1"), Some("v1"), Some("d1"), &[])],
    )
    .await;
    let mut batch = builder(&remote, context.clone());
    batch.stage(first.clone()).unwrap();
    assert!(batch.stage(other).is_err());
    assert!(batch.seal().is_err());

    // A response observed under another binding (another evaluation) aborts too.
    let actor = remote.binding.actor().clone();
    let other_binding = {
        use search_application::scoped::AccessContextAuthorityPort;
        remote
            .authority
            .bind_discovery(
                &actor,
                search_core::id::DiscoveryEvaluationId::from_uuid(uuid::Uuid::now_v7()),
            )
            .await
            .unwrap()
            .unwrap()
    };
    let other_context = search_application::remote::TrustedRemoteContext::bind(
        other_binding,
        &remote.visible,
        &remote.authority,
        &visibility,
    )
    .await
    .unwrap();
    let foreign = observe(
        &remote,
        &visibility,
        &Verifier::shared("snapshot-1"),
        &other_context,
        "lookup",
        lookup("doc-1"),
        vec![hit(Some("doc-1"), Some("v1"), Some("d1"), &[])],
    )
    .await;
    let mut batch = builder(&remote, context);
    batch.stage(first).unwrap();
    assert!(batch.stage(foreign).is_err());
}

#[tokio::test]
async fn unproven_second_live_action_yields_explicit_gap() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let first = observe(
        &remote,
        &visibility,
        &Verifier::single("response-1"),
        &context,
        "live-1",
        query(),
        vec![hit(Some("doc-1"), Some("v1"), Some("d1"), &[])],
    )
    .await;
    let second = observe(
        &remote,
        &visibility,
        &Verifier::single("response-2"),
        &context,
        "live-2",
        lookup("doc-2"),
        vec![hit(Some("doc-2"), Some("v1"), Some("d2"), &[])],
    )
    .await;
    let mut batch = builder(&remote, context);
    batch.stage(first).unwrap();
    match batch.stage(second).unwrap() {
        StageOutcome::Gap(gap) => {
            assert!(gap.required_fact.ends_with("remote_snapshot_incompatible"));
            assert_eq!(gap.reason, GapReason::UnsupportedCoverage);
        }
        other => panic!("expected an explicit gap: {other:?}"),
    }
    let generation = batch.seal().unwrap();
    // Only the first action's Resource is in the generation.
    assert_eq!(generation.resource_ids().len(), 1);
    assert!(generation.candidates("live-2").is_none());
    assert_eq!(generation.gaps().len(), 1);
}

#[tokio::test]
async fn idless_hit_yields_ephemeral_gap_without_federation() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let response = observe(
        &remote,
        &visibility,
        &Verifier::shared("snapshot-1"),
        &context,
        "search",
        query(),
        vec![
            hit(None, None, None, &[("title", "無名", None)]),
            hit(Some("doc-1"), Some("v1"), None, &[]),
        ],
    )
    .await;
    let mut batch = builder(&remote, context);
    batch.stage(response).unwrap();
    let generation = batch.seal().unwrap();
    let candidates = generation.candidates("search").unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.resource_ref.is_some())
    );
    assert!(generation.gaps().iter().any(|gap| {
        gap.required_fact
            .ends_with("ephemeral_identity_not_qualifiable")
            && gap.reason == GapReason::UnsupportedCoverage
    }));
}

#[tokio::test]
async fn provider_candidate_id_and_locator_cannot_choose_identity() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    // A native ID shaped like a candidate ID or a URL is only an opaque ID.
    let response = observe(
        &remote,
        &visibility,
        &Verifier::shared("snapshot-1"),
        &context,
        "search",
        query(),
        vec![hit(
            Some("forged-candidate-id"),
            Some("v1"),
            Some("d1"),
            &[("locator", "elsewhere", None)],
        )],
    )
    .await;
    let mut batch = builder(&remote, context);
    batch.stage(response).unwrap();
    let generation = batch.seal().unwrap();
    let candidate = &generation.candidates("search").unwrap()[0];
    let id = remote_resource_id(
        remote.registration.tenant(),
        remote.registration.source_id(),
        "synthetic",
        &native("forged-candidate-id"),
    )
    .unwrap();
    assert_eq!(candidate.resource_ref, Some(id));
    assert_eq!(
        candidate.candidate_id,
        remote_candidate_id(remote.registration.source_id(), id)
    );
    assert_eq!(candidate.source_ref, remote.registration.source_id());
    assert_eq!(
        candidate.identity_class,
        CandidateIdentityClass::RemoteStableReference
    );
    assert_eq!(candidate.locator, None);
    // The identity is tenant-scoped: another tenant derives another ResourceId.
    let other_tenant = search_application::scoped::TenantId::new("tenant-b").unwrap();
    assert_ne!(
        remote_resource_id(
            &other_tenant,
            remote.registration.source_id(),
            "synthetic",
            &native("forged-candidate-id")
        )
        .unwrap(),
        id
    );
}
