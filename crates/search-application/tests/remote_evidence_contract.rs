//! P4-10: verified evidence and canonical lineage.

#[path = "support/remote.rs"]
mod support;

use std::collections::BTreeMap;

use search_application::ports::{
    AssertionStorePort, BoxFuture, ClaimSelector, ClaimSelectorPort, EvidenceResolverPort,
    ResolvedAssertionEvidence, assemble_resource_claims, assess_claim_evidence,
};
use search_application::remote::{PinnedRemoteTarget, TrustedRemoteContext};
use search_application::remote_evidence::{
    RegisteredLineage, RemoteProvenanceLookupPort, UntrustedEvidenceHint, VerifiedProvenance,
    VerifiedSourceProvenance, resolved_evidence, verify_provenance,
};
use search_application::scoped::AuthorizedSourceScope;
use search_core::assertion::{Assertion, AssertionOrigin};
use search_core::evidence::{ClaimState, EvidenceRequirement, EvidenceRole, EvidenceSufficiency};
use search_core::id::{ClaimId, ProjectionGenerationId, ResourceId};
use search_core::predicate::TypedValue;
use search_core::projection::ProjectionGenerationKey;
use search_core::source::RetentionMode;
use support::*;
use time::OffsetDateTime;
use uuid::Uuid;

/// The fixed Source's provenance protocol, keyed by evidence ref.
struct Lookup(BTreeMap<String, VerifiedSourceProvenance>);
impl RemoteProvenanceLookupPort for Lookup {
    fn lookup<'a>(
        &'a self,
        _scope: &'a AuthorizedSourceScope,
        _target: &'a PinnedRemoteTarget,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<VerifiedSourceProvenance>> {
        Box::pin(async move { Ok(self.0.get(evidence_ref).cloned()) })
    }
}

fn record(label: &str, direct: bool, version: &str) -> VerifiedSourceProvenance {
    VerifiedSourceProvenance {
        direct,
        summary: false,
        version: Some(version.into()),
        digest: Some("d1".into()),
        lineage_label: label.into(),
        predicate: "catalog.title".into(),
        citation_chain: vec![],
    }
}

async fn pinned(remote: &Remote) -> (TrustedRemoteContext, PinnedRemoteTarget) {
    let visibility = remote.visibility();
    let context = remote.context(&visibility).await;
    let response = observe(
        remote,
        &visibility,
        &Verifier::shared("snapshot-1"),
        &context,
        "lookup",
        lookup("doc-1"),
        vec![hit(Some("doc-1"), Some("v1"), Some("d1"), &[])],
    )
    .await;
    let target = PinnedRemoteTarget::from_response(&context, &response, &native("doc-1")).unwrap();
    (context, target)
}

fn key(remote: &Remote) -> ProjectionGenerationKey {
    ProjectionGenerationKey {
        source_id: remote.registration.source_id(),
        generation_id: ProjectionGenerationId::from_uuid(Uuid::from_u128(11)),
    }
}

const RESOURCE: u128 = 21;
const CLAIM: u128 = 31;

/// Selector, assertion and resolver over the verified records.
struct Claims {
    key: ProjectionGenerationKey,
    refs: Vec<String>,
    resolved: BTreeMap<String, ResolvedAssertionEvidence>,
}
impl ClaimSelectorPort for Claims {
    fn selector_for<'a>(
        &'a self,
        _g: ProjectionGenerationKey,
        claim: ClaimId,
    ) -> BoxFuture<'a, Option<ClaimSelector>> {
        Box::pin(async move {
            Ok(Some(ClaimSelector {
                claim_id: claim,
                subject_ref: "doc-1".into(),
                predicate: "catalog.title".into(),
                expected_value: None,
            }))
        })
    }
}
impl AssertionStorePort for Claims {
    fn assertions_for<'a>(
        &'a self,
        _g: ProjectionGenerationKey,
        _r: ResourceId,
        predicate: &'a str,
    ) -> BoxFuture<'a, Vec<Assertion>> {
        Box::pin(async move {
            let mut assertion = Assertion::new(
                "doc-1",
                predicate,
                TypedValue::String("規程".into()),
                "remote",
                AssertionOrigin::Observed,
                "catalog",
                OffsetDateTime::now_utc(),
            );
            assertion.evidence_refs = self.refs.clone();
            Ok(vec![assertion])
        })
    }
}
impl EvidenceResolverPort for Claims {
    fn resolve<'a>(
        &'a self,
        _g: ProjectionGenerationKey,
        _r: ResourceId,
        evidence_ref: &'a str,
    ) -> BoxFuture<'a, Option<ResolvedAssertionEvidence>> {
        Box::pin(async move { Ok(self.resolved.get(evidence_ref).cloned()) })
    }
}

async fn verify_all(
    remote: &Remote,
    lineage: &RegisteredLineage,
    lookup: &Lookup,
    hints: &[UntrustedEvidenceHint],
) -> Vec<Option<VerifiedProvenance>> {
    let (context, target) = pinned(remote).await;
    let mut out = Vec::new();
    for hint in hints {
        out.push(
            verify_provenance(
                &remote.registration,
                lineage,
                context.source_scope(),
                &target,
                hint,
                lookup,
            )
            .await
            .unwrap(),
        );
    }
    out
}

/// Claim and sufficiency through the existing resolver functions.
async fn sufficiency(
    remote: &Remote,
    verified: &[(String, Option<VerifiedProvenance>)],
    minimum: usize,
) -> (ClaimState, EvidenceSufficiency) {
    let key = key(remote);
    let resource = ResourceId::from_uuid(Uuid::from_u128(RESOURCE));
    let claims = Claims {
        key,
        refs: verified.iter().map(|(r, _)| r.clone()).collect(),
        resolved: verified
            .iter()
            .filter_map(|(r, v)| {
                v.as_ref()
                    .map(|v| (r.clone(), resolved_evidence(key, resource, v)))
            })
            .collect(),
    };
    let claim_id = ClaimId::from_uuid(Uuid::from_u128(CLAIM));
    let mut requirement = EvidenceRequirement::new(vec![claim_id]);
    requirement.minimum_independent_sources = minimum;
    let assembled = assemble_resource_claims(
        claims.key,
        resource,
        &requirement,
        &claims,
        &claims,
        &claims,
    )
    .await
    .unwrap();
    let state = assembled[0].state;
    (
        state,
        assess_claim_evidence(&requirement, &assembled).unwrap(),
    )
}

fn hint(evidence: &str, role: &str, origin: &str, quoted: bool) -> UntrustedEvidenceHint {
    UntrustedEvidenceHint::new(evidence, Some(role.into()), Some(origin.into()), quoted).unwrap()
}

#[tokio::test]
async fn provider_labels_cannot_create_two_direct_origins() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let lineage = RegisteredLineage::new(&remote.registration, vec![]).unwrap();
    // The provider claims two primary origins; the Source verifies one lineage.
    let lookup = Lookup(BTreeMap::from([
        ("ev-a".to_string(), record("catalog", true, "v1")),
        ("ev-b".to_string(), record("catalog", true, "v1")),
    ]));
    let verified = verify_all(
        &remote,
        &lineage,
        &lookup,
        &[
            hint("ev-a", "primary", "origin-a", false),
            hint("ev-b", "authoritative", "origin-b", false),
        ],
    )
    .await;
    let origins: Vec<_> = verified
        .iter()
        .flatten()
        .map(|v| v.upstream_origin().to_owned())
        .collect();
    assert_eq!(
        origins,
        vec![
            "synthetic-catalog".to_string(),
            "synthetic-catalog".to_string()
        ]
    );
    let (state, sufficiency) = sufficiency(
        &remote,
        &[
            ("ev-a".into(), verified[0].clone()),
            ("ev-b".into(), verified[1].clone()),
        ],
        2,
    )
    .await;
    assert_eq!(state, ClaimState::Supported);
    assert_ne!(
        sufficiency,
        EvidenceSufficiency::Sufficient,
        "one upstream counts once"
    );
}

#[tokio::test]
async fn authoritative_requires_predicate_grant_and_matching_version() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let lineage = RegisteredLineage::new(&remote.registration, vec![]).unwrap();
    let mut ungranted = record("catalog", true, "v1");
    ungranted.predicate = "catalog.owner".into();
    let lookup = Lookup(BTreeMap::from([
        ("granted".to_string(), record("catalog", true, "v1")),
        ("ungranted".to_string(), ungranted),
        ("stale".to_string(), record("catalog", true, "v0")),
    ]));
    let verified = verify_all(
        &remote,
        &lineage,
        &lookup,
        &[
            hint("granted", "primary", "catalog", false),
            hint("ungranted", "authoritative", "catalog", false),
            hint("stale", "authoritative", "catalog", false),
        ],
    )
    .await;
    assert_eq!(
        verified[0].as_ref().unwrap().assertion_origin(),
        AssertionOrigin::Authoritative
    );
    assert_eq!(
        verified[1].as_ref().unwrap().assertion_origin(),
        AssertionOrigin::Observed
    );
    // A record for another version of the Resource is not this Resource's evidence.
    assert!(verified[2].is_none());
}

#[tokio::test]
async fn unknown_lineage_keeps_claim_unknown() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let lineage = RegisteredLineage::new(&remote.registration, vec![]).unwrap();
    let lookup = Lookup(BTreeMap::new());
    let verified = verify_all(
        &remote,
        &lineage,
        &lookup,
        &[hint("ev-x", "primary", "anywhere", false)],
    )
    .await;
    assert!(verified[0].is_none());
    let (state, sufficiency) = sufficiency(&remote, &[("ev-x".into(), None)], 1).await;
    assert_eq!(state, ClaimState::Unknown);
    assert_ne!(sufficiency, EvidenceSufficiency::Sufficient);
}

#[tokio::test]
async fn two_registered_independent_groups_can_satisfy_threshold() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let lineage = RegisteredLineage::new(
        &remote.registration,
        vec![
            ("registry-a".into(), "group-a".into()),
            ("registry-b".into(), "group-b".into()),
        ],
    )
    .unwrap();
    let lookup = Lookup(BTreeMap::from([
        ("ev-a".to_string(), record("registry-a", true, "v1")),
        ("ev-b".to_string(), record("registry-b", true, "v1")),
    ]));
    let verified = verify_all(
        &remote,
        &lineage,
        &lookup,
        &[
            hint("ev-a", "primary", "x", false),
            hint("ev-b", "corroborating", "x", false),
        ],
    )
    .await;
    assert_eq!(
        verified[1].as_ref().unwrap().role(),
        EvidenceRole::Corroborating
    );
    let (state, sufficiency) = sufficiency(
        &remote,
        &[
            ("ev-a".into(), verified[0].clone()),
            ("ev-b".into(), verified[1].clone()),
        ],
        2,
    )
    .await;
    assert_eq!(state, ClaimState::Supported);
    assert_eq!(sufficiency, EvidenceSufficiency::Sufficient);
}

#[tokio::test]
async fn quoted_or_summary_claim_is_not_direct() {
    let remote = Remote::new(RetentionMode::NoRetention).await;
    let lineage = RegisteredLineage::new(&remote.registration, vec![]).unwrap();
    let mut summary = record("catalog", true, "v1");
    summary.summary = true;
    let lookup = Lookup(BTreeMap::from([
        ("quoted".to_string(), record("catalog", true, "v1")),
        ("summary".to_string(), summary),
        ("derived".to_string(), record("catalog", false, "v1")),
    ]));
    let verified = verify_all(
        &remote,
        &lineage,
        &lookup,
        &[
            hint("quoted", "primary", "catalog", true),
            hint("summary", "primary", "catalog", false),
            hint("derived", "primary", "catalog", false),
        ],
    )
    .await;
    for v in verified.iter().flatten() {
        assert_eq!(v.role(), EvidenceRole::Contextual);
        assert!(v.is_summary());
    }
    let refs: Vec<_> = ["quoted", "summary", "derived"]
        .iter()
        .map(|r| r.to_string())
        .zip(verified)
        .collect();
    let (state, _) = sufficiency(&remote, &refs, 1).await;
    assert_eq!(state, ClaimState::Unknown);
}
