//! P5-04: the safe public Discovery projection.

#[path = "support/body_discovery.rs"]
mod body_discovery;

use body_discovery::*;
use search_application::public_projection::{
    CompletenessReason, DiscoveryEvaluationView, PublicClaimState, PublicEvidenceValue,
    PublicSufficiency, project_discovery,
};
use search_application::search_query::PublicGapCode;
use search_core::applicability::ApplicabilityState;
use search_core::discovery::{QualifiedResource, RejectedCandidate};
use search_core::evidence::{Claim, EvidenceReference};
use uuid::Uuid;

fn project(result: &DiscoveryResult) -> DiscoveryEvaluationView {
    project_discovery(result, Uuid::new_v4())
}

fn qualified(resource: u128, source: Option<SourceId>) -> QualifiedResource {
    QualifiedResource {
        resource_ref: rid(resource),
        source_ref: source,
        usage_profile_ref: None,
        applicability: ApplicabilityState::Applicable,
        matched_conditions: vec!["SCOPE_MATCH".into(), "private free text".into()],
        resolved_discriminators: vec![],
        remaining_nonblocking_unknowns: vec!["internal".into()],
        contrast_resolution: Some("private".into()),
        evidence_refs: vec!["ev-1".into()],
        qualification_trace: vec!["qualified:private-candidate-id".into()],
    }
}

fn supported(value: TypedValue) -> Claim {
    let mut claim = Claim::new(cid(), ClaimState::Supported);
    claim.value = Some(value);
    claim.evidence_refs = vec![EvidenceReference {
        source_ref: sid(),
        evidence_ref: Some("ev-1".into()),
        upstream_origin: "private-lineage-label".into(),
        role: EvidenceRole::Primary,
        citation_chain: vec!["private://citation/locator".into()],
        content_digest: Some("private-digest".into()),
        is_summary: false,
    }];
    claim
}

/// An engine result after the final gate: the revoked n-ary path item is
/// already gone; only its private traces would remain to leak.
fn gated_result() -> DiscoveryResult {
    let request = request();
    DiscoveryResult {
        discovery_evaluation_id: request.temporal_context.evaluation_id,
        need: request.need,
        qualified_resources: vec![qualified(10, Some(sid())), qualified(11, None)],
        evidence_set: vec![supported(TypedValue::String("規程".into()))],
        evidence_sufficiency: EvidenceSufficiency::Sufficient,
        unresolved_gaps: vec![],
        rejected_candidates: vec![RejectedCandidate {
            candidate_id: "private-rejected-id".into(),
            state: ApplicabilityState::Excluded,
            reason_trace: vec!["private reason".into()],
        }],
        source_trace: vec!["source:private-source:Required:Planned".into()],
        retrieval_trace: vec![
            "graph_path:private-candidate:[{\"participants\":[]}]".into(),
            "candidate:retriever:1:private-candidate-id".into(),
        ],
        qualification_trace: vec!["qualified:private-candidate-id".into()],
    }
}

#[tokio::test]
async fn nary_relation_any_participant_revoked_removes_derived_item_evidence_rank_count_trace() {
    let view = project(&gated_result());
    // Only an item with a currently visible Source is projected, counted once.
    assert_eq!(view.qualified.len(), 1);
    assert_eq!(view.qualified[0].resource_id, rid(10));
    assert_eq!(view.qualified[0].source_id, sid());
    let counted: u32 = view
        .trace
        .iter()
        .filter(|entry| entry.visible_source_id.is_some())
        .filter_map(|entry| entry.count)
        .sum();
    assert_eq!(counted, 1);
    // No Graph path, candidate or source trace string has a public channel.
    let printed = format!("{view:?}");
    for private in [
        "graph_path",
        "participants",
        "private-candidate",
        "private-source",
        "private-rejected-id",
    ] {
        assert!(!printed.contains(private), "{private}");
    }
    assert_eq!(view.rejected_reason_codes, vec!["HARD_GATE_REJECTED"]);
}

#[tokio::test]
async fn p1_archive_mixed_text_csv_part_verified_no_scalar_rejection() {
    // A verified CSV leaf of an archive yields plain text: kept as a value.
    let mut result = gated_result();
    result.evidence_set = vec![supported(TypedValue::String("列A,列B\n1,2".into()))];
    let view = project(&result);
    assert_eq!(
        view.evidence[0].value,
        Some(PublicEvidenceValue::Text("列A,列B\n1,2".into()))
    );
    // A non-scalar value is omitted, but the Supported evidence is not rejected.
    result.evidence_set = vec![supported(TypedValue::List(vec![
        TypedValue::String("a".into()),
        TypedValue::String("b".into()),
    ]))];
    let view = project(&result);
    assert_eq!(view.evidence.len(), 1);
    assert_eq!(view.evidence[0].state, PublicClaimState::Supported);
    assert_eq!(view.evidence[0].value, None);
    // Overlong text is never truncated into a different value.
    result.evidence_set = vec![supported(TypedValue::String("字".repeat(321)))];
    assert_eq!(project(&result).evidence[0].value, None);
}

#[tokio::test]
async fn partial_positive_keeps_blocking_gap() {
    let mut fixture = Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(10), "lexical"),
            unit_hit: Some(unit_hit(rid(10))),
        }],
        true,
    ));
    fixture.exact_selector = Some(selector(rid(10), "本文語"));
    fixture.exact_verified = Some(verified(rid(10), "本文語"));
    fixture.coverage = Some(vec![InformationGap::new(
        format!("document.body.coverage:{}:0:partial", rid(10).as_uuid()),
        GapReason::UnsupportedCoverage,
        true,
    )]);
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    let view = project(&result);
    assert_eq!(view.sufficiency, PublicSufficiency::Unresolved);
    assert!(
        view.evidence
            .iter()
            .any(|evidence| evidence.state == PublicClaimState::Supported)
    );
    assert!(
        view.gaps
            .iter()
            .any(|gap| gap.code == PublicGapCode::UnsupportedCoverage && gap.blocking)
    );
    assert!(
        view.completeness_reasons
            .contains(&CompletenessReason::BodyCoverageIncomplete)
    );
}

#[tokio::test]
async fn negative_requires_exact_text_proof() {
    let no_hit = || {
        let mut fixture = Fixture::new(batch(vec![], true));
        fixture.exact_selector = Some(selector(rid(10), "本文語"));
        fixture
    };
    // An exhausted no-hit alone stays Unknown.
    let fixture = no_hit();
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    let states: Vec<_> = project(&result)
        .evidence
        .iter()
        .filter(|evidence| evidence.claim_id == cid())
        .map(|evidence| evidence.state)
        .collect();
    assert_eq!(states, vec![PublicClaimState::Unknown]);
    // Only a bound Source receipt makes it Absent.
    let mut fixture = no_hit();
    fixture.absence = Some(ExactTextAbsenceOutcome::ProvenAbsent(Box::new(proof(
        rid(10),
        "本文語",
    ))));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    let view = project(&result);
    assert!(
        view.evidence.iter().any(
            |evidence| evidence.claim_id == cid() && evidence.state == PublicClaimState::Absent
        )
    );
    assert_ne!(view.sufficiency, PublicSufficiency::Sufficient);
}

#[tokio::test]
async fn private_fields_never_serialize() {
    let view = project(&gated_result());
    let printed = format!("{view:?}");
    for private in [
        "private free text",
        "internal",
        "private-lineage-label",
        "private://citation/locator",
        "private-digest",
        "private reason",
    ] {
        assert!(!printed.contains(private), "{private}");
    }
    assert_eq!(
        view.qualified[0].matched_condition_codes,
        vec!["SCOPE_MATCH".to_owned()]
    );
    // Evidence IDs are canonical and shared between the item and its Claim.
    assert_eq!(view.qualified[0].evidence_ids, vec![view.evidence[0].id]);
    assert_eq!(view.evidence[0].id.get_version_num(), 8);
}
