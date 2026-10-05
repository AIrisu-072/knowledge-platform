//! P1-A03 exact positive evidence: only a Source-verified span bound to the
//! required Claim, parent and literal becomes a Primary `Extracted` Claim.

#[path = "support/body_discovery.rs"]
mod body_discovery;

use body_discovery::*;
use search_application::ports::assemble_verified_unit_text_claim;

fn hit_fixture() -> Fixture {
    Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(10), "lexical"),
            unit_hit: Some(unit_hit(rid(10))),
        }],
        true,
    ))
}

fn exact_claim(result: &DiscoveryResult) -> Vec<(ClaimState, Option<String>)> {
    result
        .evidence_set
        .iter()
        .filter(|claim| claim.claim_id == cid())
        .map(|claim| (claim.state, claim.predicate.clone()))
        .collect()
}

#[tokio::test]
async fn verified_span_supports_only_the_bound_exact_claim() {
    let mut fixture = hit_fixture();
    fixture.exact_selector = Some(selector(rid(10), "本文語"));
    fixture.exact_verified = Some(verified(rid(10), "本文語"));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Sufficient);
    let claim = result
        .evidence_set
        .iter()
        .find(|claim| claim.claim_id == cid())
        .unwrap();
    assert_eq!(claim.state, ClaimState::Supported);
    assert_eq!(claim.predicate.as_deref(), Some(CONTAINS_EXACT_PREDICATE));
    assert_eq!(claim.value, Some(TypedValue::String("本文語".into())));
    assert_eq!(claim.evidence_refs.len(), 1);
    assert_eq!(claim.evidence_refs[0].role, EvidenceRole::Primary);
    assert!(!claim.evidence_refs[0].is_summary);
    // The stored-Assertion path never ran for the exact-text Claim.
    assert!(fixture.calls().contains(&"exact-resolve".to_owned()));
    assert_eq!(result.qualified_resources.len(), 1);
}

#[tokio::test]
async fn unbound_or_unverified_exact_claim_stays_unknown() {
    let cases: Vec<(
        Option<ExactTextSelector>,
        Option<VerifiedExtractedTextEvidence>,
    )> = vec![
        // Selector for another parent.
        (
            Some(selector(rid(11), "本文語")),
            Some(verified(rid(10), "本文語")),
        ),
        // Selector literal differs from the request literal.
        (
            Some(selector(rid(10), "別の語")),
            Some(verified(rid(10), "別の語")),
        ),
        // Predicate outside the allowlist.
        (
            Some(ExactTextSelector {
                predicate: "document.title".into(),
                ..selector(rid(10), "本文語")
            }),
            Some(verified(rid(10), "本文語")),
        ),
        // No selector, or the Source could not verify the hit.
        (None, Some(verified(rid(10), "本文語"))),
        (Some(selector(rid(10), "本文語")), None),
        // A Declared Assertion is not extracted body evidence.
        (Some(selector(rid(10), "本文語")), {
            let mut declared = verified(rid(10), "本文語");
            declared.assertion.origin = AssertionOrigin::Declared;
            Some(declared)
        }),
        // A span that is not exactly the literal.
        (Some(selector(rid(10), "本文語")), {
            let mut short = verified(rid(10), "本文語");
            short.matched_span = TextSpan::new("本文語", 0, 6).unwrap();
            Some(short)
        }),
    ];
    for (index, (selector, verified)) in cases.into_iter().enumerate() {
        let mut fixture = hit_fixture();
        fixture.exact_selector = selector;
        fixture.exact_verified = verified;
        let result = fixture
            .service(config())
            .discover_with_content_scope(request(), exact_scope("本文語"))
            .await
            .unwrap();
        assert_eq!(
            exact_claim(&result),
            vec![(ClaimState::Unknown, None)],
            "case {index}"
        );
        assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
        assert!(
            result.unresolved_gaps.iter().any(
                |gap| gap.blocking && gap.required_fact == format!("claim:{}", cid().as_uuid())
            ),
            "case {index}"
        );
    }

    // A selector for a Claim the request does not require is refused outright.
    let fixture = hit_fixture();
    let scope = DiscoveryScope::BodyRequired(BodySearchSpec {
        query: LexicalQuery::body_only("本文語", 10),
        exact_text_claim: Some(ClaimId::from_uuid(Uuid::from_u128(99))),
    });
    assert!(matches!(
        fixture
            .service(config())
            .discover_with_content_scope(request(), scope)
            .await,
        Err(SearchError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn partial_positive_keeps_the_claim_and_a_blocking_coverage_gap() {
    let mut fixture = hit_fixture();
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
    assert_eq!(
        exact_claim(&result),
        vec![(ClaimState::Supported, Some(CONTAINS_EXACT_PREDICATE.into()))]
    );
    assert_eq!(result.qualified_resources.len(), 1);
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(result.unresolved_gaps.iter().any(|gap| gap.blocking
        && gap.reason == GapReason::UnsupportedCoverage
        && gap.required_fact.ends_with(":partial")));

    // Without a coverage port the corpus is never reported complete.
    let mut fixture = hit_fixture();
    fixture.exact_selector = Some(selector(rid(10), "本文語"));
    fixture.exact_verified = Some(verified(rid(10), "本文語"));
    fixture.coverage = None;
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(
        result
            .unresolved_gaps
            .iter()
            .any(|gap| gap.blocking && gap.required_fact == "document.body.coverage_unverified")
    );
}

#[test]
fn assembler_rejects_every_unbound_field() {
    let good = verified(rid(10), "本文語");
    let bound = selector(rid(10), "本文語");
    assert_eq!(
        assemble_verified_unit_text_claim(cid(), &bound, &good).state,
        ClaimState::Supported
    );
    let other = ClaimId::from_uuid(Uuid::from_u128(77));
    assert_eq!(
        assemble_verified_unit_text_claim(other, &bound, &good).state,
        ClaimState::Unknown
    );
    let mut summary = good.clone();
    summary.resolved.is_summary = true;
    let mut corroborating = good.clone();
    corroborating.resolved.role = EvidenceRole::Corroborating;
    let mut wrong_parent = good.clone();
    wrong_parent.resolved.resource_id = rid(11);
    let mut other_ref = good.clone();
    other_ref.assertion.evidence_refs = vec!["other".into()];
    let mut no_digest = good.clone();
    no_digest.resolved.content_digest = None;
    let mut other_value = good.clone();
    other_value.assertion.value = TypedValue::String("別".into());
    for broken in [
        summary,
        corroborating,
        wrong_parent,
        other_ref,
        no_digest,
        other_value,
    ] {
        assert_eq!(
            assemble_verified_unit_text_claim(cid(), &bound, &broken).state,
            ClaimState::Unknown
        );
    }
    let unnormalized = selector(rid(10), "本文\r\n語");
    assert_eq!(
        assemble_verified_unit_text_claim(cid(), &unnormalized, &good).state,
        ClaimState::Unknown
    );
}
