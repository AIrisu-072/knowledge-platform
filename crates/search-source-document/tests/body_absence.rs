//! P1-A04 finite Source-owned exact negative proof over the pinned manifest.

#[path = "support/body_claims.rs"]
mod body_claims;
#[path = "support/body_index.rs"]
mod body_index;
#[path = "support/body.rs"]
mod body_support;

use std::time::{Duration, Instant};

use body_claims::*;
use search_application::body_ports::{
    ExactScanBudget, ExactTextAbsenceOutcome, PinnedBodyBundle, SourceExactTextAbsencePort,
};

fn text(value: &str) -> (Vec<u8>, &'static str) {
    (value.as_bytes().to_vec(), "text/plain")
}

async fn pinned(published: &Published) -> PinnedBodyBundle {
    published
        .catalog(access(AccessDecision::Allowed, &[]))
        .pin_body(published.key)
        .await
        .unwrap()
        .expect("published body bundle")
}

async fn verify(
    published: &Published,
    decision: Arc<ScriptedAccess>,
    literal: &str,
    pinned: &PinnedBodyBundle,
    budget: ExactScanBudget,
) -> ExactTextAbsenceOutcome {
    let bound = selector(literal);
    published
        .catalog_with(decision, std::slice::from_ref(&bound))
        .verify_absence(&request(), pinned, &bound, budget)
        .await
        .unwrap()
}

fn budget() -> ExactScanBudget {
    ExactScanBudget::initial(Instant::now())
}

fn gap_code(outcome: &ExactTextAbsenceOutcome) -> Option<&str> {
    match outcome {
        ExactTextAbsenceOutcome::Unknown(gap) => {
            assert!(gap.blocking);
            Some(gap.required_fact.as_str())
        }
        _ => None,
    }
}

#[tokio::test]
async fn complete_supported_scan_issues_a_bound_receipt() {
    let published = publish_items(vec![text("大阪の本文\n"), text("東\n京\n")]).await;
    let pin = pinned(&published).await;
    let outcome = verify(
        &published,
        access(AccessDecision::Allowed, &[]),
        "東京",
        &pin,
        budget(),
    )
    .await;
    let ExactTextAbsenceOutcome::ProvenAbsent(proof) = outcome else {
        panic!("expected a proof, got {outcome:?}");
    };
    // `東` and `京` in separate Units do not form the v1 single-Unit literal.
    assert_eq!(proof.claim_id(), claim());
    assert_eq!(proof.parent().resource_id, parent());
    assert_eq!(proof.generation(), published.key);
    assert_eq!(proof.bundle_digest(), pin.composite_digest);
    assert_eq!(proof.predicate(), "document.body.contains_exact");
    assert_eq!(
        proof.exact_text_sha256(),
        <[u8; 32]>::from(Sha256::digest("東京".as_bytes()))
    );
    assert_eq!(proof.counts(), (2, 3));
    assert_eq!(proof.revisions(), (2, 1));
}

#[tokio::test]
async fn literal_inside_a_unit_is_match_found_without_any_token_hit() {
    let published = publish_items(vec![text("東京都の規程\n"), text("Café\r\nメニュー\r\n")]).await;
    let pin = pinned(&published).await;
    // 京 is inside one analyzer token; Café and CRLF follow the normalized literal.
    for literal in ["京", "Café", "メニュー"] {
        let outcome = verify(
            &published,
            access(AccessDecision::Allowed, &[]),
            literal,
            &pin,
            budget(),
        )
        .await;
        assert_eq!(outcome, ExactTextAbsenceOutcome::MatchFound, "{literal}");
    }
}

#[tokio::test]
async fn partial_unsupported_or_changed_bindings_are_unknown() {
    // Partial (DOCX with an omitted header) and Unsupported items never prove absence.
    for raw in [
        vec![(partial_docx("東京の本文"), DOCX)],
        vec![
            text("東京の本文\n"),
            (b"\x89PNG\r\n\x1a\n".to_vec(), "image/png"),
        ],
    ] {
        let published = publish_items(raw).await;
        let pin = pinned(&published).await;
        let outcome = verify(
            &published,
            access(AccessDecision::Allowed, &[]),
            "大阪",
            &pin,
            budget(),
        )
        .await;
        assert_eq!(gap_code(&outcome), Some("document.body.absence_coverage"));
    }

    let published = publish_items(vec![text("東京の本文\n")]).await;
    let pin = pinned(&published).await;
    let allowed = || access(AccessDecision::Allowed, &[]);
    let forged = PinnedBodyBundle {
        composite_digest: [7; 32],
        ..pin
    };
    let outcome = verify(&published, allowed(), "大阪", &forged, budget()).await;
    assert_eq!(
        gap_code(&outcome),
        Some("document.body.absence_bundle_changed")
    );

    for exhausted in [
        ExactScanBudget {
            max_units: 0,
            ..budget()
        },
        ExactScanBudget {
            max_text_bytes: 1,
            ..budget()
        },
        ExactScanBudget {
            max_visible_items: 0,
            ..budget()
        },
        ExactScanBudget {
            deadline: Instant::now() - Duration::from_millis(1),
            ..budget()
        },
    ] {
        let outcome = verify(&published, allowed(), "大阪", &pin, exhausted).await;
        assert_eq!(gap_code(&outcome), Some("document.body.absence_budget"));
    }

    // A newer current Version makes the pinned parent no longer provable.
    let mut superseded = snapshot("s2", published.items.clone());
    superseded.live[0].snapshot.current_version_id =
        Some(DocumentVersionId::from_uuid(Uuid::from_u128(21)));
    published.harness.reader.replace(vec![superseded]);
    let outcome = verify(&published, allowed(), "大阪", &pin, budget()).await;
    assert_eq!(
        gap_code(&outcome),
        Some("document.body.absence_unverifiable")
    );
}

#[tokio::test]
async fn denied_unknown_or_revoked_read_is_one_undisclosed_unknown() {
    let published = publish_items(vec![text("東京の本文\n")]).await;
    let pin = pinned(&published).await;
    for scripted in [
        access(AccessDecision::Denied, &[]),
        access(AccessDecision::Unknown, &[]),
        // Allowed for the scan, revoked right before issue.
        access(AccessDecision::Denied, &[AccessDecision::Allowed]),
    ] {
        let outcome = verify(&published, scripted, "大阪", &pin, budget()).await;
        assert_eq!(
            gap_code(&outcome),
            Some("document.body.absence_unverifiable")
        );
    }

    // Only the registered selector for a required Claim may be scanned.
    let catalog = published.catalog_with(access(AccessDecision::Allowed, &[]), &[]);
    let outcome = catalog
        .verify_absence(&request(), &pin, &selector("大阪"), budget())
        .await
        .unwrap();
    assert_eq!(gap_code(&outcome), Some("document.body.absence_unbound"));
    let other_claim = ExactTextSelector {
        claim_id: ClaimId::from_uuid(Uuid::from_u128(77)),
        ..selector("大阪")
    };
    let catalog = published.catalog_with(
        access(AccessDecision::Allowed, &[]),
        std::slice::from_ref(&other_claim),
    );
    let outcome = catalog
        .verify_absence(&request(), &pin, &other_claim, budget())
        .await
        .unwrap();
    assert_eq!(gap_code(&outcome), Some("document.body.absence_unbound"));
}
