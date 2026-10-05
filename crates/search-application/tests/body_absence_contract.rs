//! P1-A04 exact negative proof in Discovery: lexical no-hit only starts a
//! Source-owned scan, and only a bound receipt turns the Claim `Absent`.

#[path = "support/body_discovery.rs"]
mod body_discovery;

use body_discovery::*;

fn no_hit(selected: &str) -> Fixture {
    let mut fixture = Fixture::new(batch(vec![], true));
    fixture.exact_selector = Some(selector(rid(10), selected));
    fixture
}

fn exact_state(result: &DiscoveryResult) -> Vec<ClaimState> {
    result
        .evidence_set
        .iter()
        .filter(|claim| claim.claim_id == cid())
        .map(|claim| claim.state)
        .collect()
}

fn blocking(result: &DiscoveryResult, code: &str) -> bool {
    result
        .unresolved_gaps
        .iter()
        .any(|gap| gap.blocking && gap.required_fact == code)
}

#[tokio::test]
async fn exhausted_no_hit_alone_never_becomes_absent() {
    let fixture = no_hit("本文語");
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(exact_state(&result), vec![ClaimState::Unknown]);
    assert_eq!(result.evidence_sufficiency, EvidenceSufficiency::Unresolved);
    assert!(!fixture.calls().contains(&"absence-scan".to_owned()));
}

#[tokio::test]
async fn bound_source_receipt_makes_only_this_claim_absent() {
    let mut fixture = no_hit("本文語");
    fixture.absence = Some(ExactTextAbsenceOutcome::ProvenAbsent(Box::new(proof(
        rid(10),
        "本文語",
    ))));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(exact_state(&result), vec![ClaimState::Absent]);
    assert_eq!(
        result.evidence_sufficiency,
        EvidenceSufficiency::Insufficient
    );
    let claim = result
        .evidence_set
        .iter()
        .find(|claim| claim.claim_id == cid())
        .unwrap();
    assert_eq!(claim.predicate.as_deref(), Some(CONTAINS_EXACT_PREDICATE));
    assert_eq!(claim.value, Some(TypedValue::String("本文語".into())));
    assert_eq!(claim.evidence_refs.len(), 1);
}

#[tokio::test]
async fn unbound_receipt_match_or_scan_failure_stays_unknown() {
    let unbound = [
        proof(rid(11), "本文語"),
        proof(rid(10), "別の語"),
        ExactTextNegativeProof::issue(NegativeProofFields {
            generation: manifest().key(),
            bundle_digest: [9; 32],
            source_snapshot: "snap-5".into(),
            claim_id: cid(),
            parent: proof(rid(10), "本文語").parent().clone(),
            document_revision: 1,
            access_revision: 1,
            exact_text_sha256: Sha256::digest("本文語".as_bytes()).into(),
            visible_item_bindings_digest: [1; 32],
            scanned_unit_bindings_digest: [2; 32],
            visible_item_count: 1,
            scanned_unit_count: 1,
        }),
    ];
    for receipt in unbound {
        let mut fixture = no_hit("本文語");
        fixture.absence = Some(ExactTextAbsenceOutcome::ProvenAbsent(Box::new(receipt)));
        let result = fixture
            .service(config())
            .discover_with_content_scope(request(), exact_scope("本文語"))
            .await
            .unwrap();
        assert_eq!(exact_state(&result), vec![ClaimState::Unknown]);
        assert!(blocking(&result, "document.body.absence_unbound"));
    }

    // A literal the index never returned is a recall signal, not a hit or absence.
    let mut fixture = no_hit("本文語");
    fixture.absence = Some(ExactTextAbsenceOutcome::MatchFound);
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(exact_state(&result), vec![ClaimState::Unknown]);
    assert!(result.qualified_resources.is_empty());
    assert!(blocking(&result, "document.body.recall_mismatch"));

    let mut fixture = no_hit("本文語");
    fixture.absence = Some(ExactTextAbsenceOutcome::Unknown(InformationGap::new(
        "document.body.absence_coverage",
        GapReason::UnsupportedCoverage,
        true,
    )));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(exact_state(&result), vec![ClaimState::Unknown]);
    assert!(blocking(&result, "document.body.absence_coverage"));
}

#[tokio::test]
async fn mismatched_literal_or_parent_hit_never_starts_a_scan() {
    // Query 甲 against a selector for 乙.
    let mut fixture = no_hit("乙");
    fixture.absence = Some(ExactTextAbsenceOutcome::ProvenAbsent(Box::new(proof(
        rid(10),
        "乙",
    ))));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("甲"))
        .await
        .unwrap();
    assert_eq!(exact_state(&result), vec![ClaimState::Unknown]);
    assert!(!fixture.calls().contains(&"absence-scan".to_owned()));

    // The parent was hit but its span did not verify: Unknown, not a negative scan.
    let mut fixture = Fixture::new(batch(
        vec![LexicalHit {
            candidate: candidate(rid(10), "lexical"),
            unit_hit: Some(unit_hit(rid(10))),
        }],
        true,
    ));
    fixture.exact_selector = Some(selector(rid(10), "本文語"));
    fixture.absence = Some(ExactTextAbsenceOutcome::ProvenAbsent(Box::new(proof(
        rid(10),
        "本文語",
    ))));
    let result = fixture
        .service(config())
        .discover_with_content_scope(request(), exact_scope("本文語"))
        .await
        .unwrap();
    assert_eq!(exact_state(&result), vec![ClaimState::Unknown]);
    assert!(!fixture.calls().contains(&"absence-scan".to_owned()));
}
