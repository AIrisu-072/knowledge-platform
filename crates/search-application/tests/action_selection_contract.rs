#[allow(dead_code)]
#[path = "../src/action_selection.rs"]
mod action_selection;

use action_selection::{
    ActionCandidate, ActionCostEstimate, ActionPriority, GapIdentity, KnownStateDigest,
    NoProgressHistory, NoProgressKey, Selection, SelectionError, select_next_action,
};
use search_core::discovery::{GapReason, InformationGap};
use search_core::id::GapId;
use uuid::Uuid;

fn action(
    id: &str,
    blocking: bool,
    authority_or_freshness_necessity: bool,
    expected_gap_resolution: u8,
    critical_need_impact: u8,
    cost: u32,
) -> ActionCandidate {
    let gap = InformationGap::new(id, GapReason::MissingFact, blocking);
    ActionCandidate::new(
        id,
        &gap,
        ActionPriority {
            authority_or_freshness_necessity,
            expected_gap_resolution,
            critical_need_impact,
            cost: estimate(
                Some(u64::from(cost)),
                Some(i64::from(cost)),
                Some("JPY"),
                Some(u64::from(cost)),
                Some(u64::from(cost)),
            ),
        },
    )
}

fn selected_id(actions: &[ActionCandidate]) -> &str {
    match select_next_action(
        actions,
        KnownStateDigest::from_bytes([1; 32]),
        &NoProgressHistory::default(),
        "JPY",
    )
    .expect("unique nonempty action IDs")
    {
        Selection::Selected(action) => action.action_id(),
        Selection::Exhausted => panic!("an eligible action should remain"),
    }
}

#[test]
fn action_priority_is_lexicographic_in_both_input_orders() {
    // Each winner loses every lower-priority comparison, so a weighted sum
    // cannot stand in for the specified ordering.
    let cases = [
        (
            action("z-blocking", true, false, 0, 0, 100),
            action("a-nonblocking", false, true, 100, 100, 0),
            "z-blocking",
        ),
        (
            action("z-authority", false, true, 0, 0, 100),
            action("a-other", false, false, 100, 100, 0),
            "z-authority",
        ),
        (
            action("z-resolves", false, false, 2, 0, 100),
            action("a-less-likely", false, false, 1, 100, 0),
            "z-resolves",
        ),
        (
            action("z-critical", false, false, 1, 2, 100),
            action("a-less-critical", false, false, 1, 1, 0),
            "z-critical",
        ),
        (
            action("z-cheap", false, false, 1, 1, 1),
            action("a-expensive", false, false, 1, 1, 2),
            "z-cheap",
        ),
        (
            action("a-stable-id", false, false, 1, 1, 1),
            action("z-stable-id", false, false, 1, 1, 1),
            "a-stable-id",
        ),
    ];

    for (winner, loser, expected) in cases {
        assert_eq!(selected_id(&[winner.clone(), loser.clone()]), expected);
        assert_eq!(selected_id(&[loser, winner]), expected);
    }
}

#[test]
fn authority_and_freshness_gaps_supply_the_necessity_priority() {
    let priority = ActionPriority {
        authority_or_freshness_necessity: false,
        expected_gap_resolution: 0,
        critical_need_impact: 0,
        cost: estimate(Some(100), Some(100), Some("JPY"), Some(100), Some(100)),
    };
    let other = action("a-fact", false, false, 100, 100, 0);
    for reason in [GapReason::Authority, GapReason::Freshness] {
        let gap = InformationGap::new("current authority or freshness", reason, false);
        let required = ActionCandidate::new("z-required", &gap, priority.clone());
        assert_eq!(selected_id(&[other.clone(), required]), "z-required");
    }
}

#[test]
fn exhausted_unchanged_state_stops_after_each_eligible_action_once() {
    let gap = InformationGap::new("missing claim", GapReason::MissingFact, true);
    let priority = ActionPriority {
        authority_or_freshness_necessity: false,
        expected_gap_resolution: 1,
        critical_need_impact: 1,
        cost: estimate(Some(1), Some(1), Some("JPY"), Some(1), Some(1)),
    };
    let first = ActionCandidate::new("a", &gap, priority.clone());
    let second = ActionCandidate::new("b", &gap, priority.clone());
    let unavailable = ActionCandidate::new("0-unavailable", &gap, priority).with_eligibility(false);
    let actions = [second.clone(), unavailable, first.clone()];
    let state = KnownStateDigest::from_bytes([7; 32]);
    let mut history = NoProgressHistory::default();

    assert!(
        matches!(select_next_action(&actions, state, &history, "JPY").expect("valid action IDs"), Selection::Selected(a) if a.action_id() == "a")
    );
    assert!(history.mark(NoProgressKey::for_action(&first, state)));
    assert!(!history.mark(NoProgressKey::for_action(&first, state)));
    assert!(
        matches!(select_next_action(&actions, state, &history, "JPY").expect("valid action IDs"), Selection::Selected(a) if a.action_id() == "b")
    );
    assert!(history.mark(NoProgressKey::for_action(&second, state)));
    assert!(matches!(
        select_next_action(&actions, state, &history, "JPY").expect("valid action IDs"),
        Selection::Exhausted
    ));
    assert_eq!(history.len(), 2);

    // A changed canonical known-state digest permits re-evaluation.
    let changed_state = KnownStateDigest::from_bytes([8; 32]);
    assert!(
        matches!(select_next_action(&actions, changed_state, &history, "JPY").expect("valid action IDs"), Selection::Selected(a) if a.action_id() == "a")
    );
}

#[test]
fn no_progress_key_scopes_an_attempt_to_gap_identity() {
    let fact_gap = InformationGap::new("claim", GapReason::MissingFact, true);
    let authority_gap = InformationGap::new("claim", GapReason::Authority, true);
    let priority = ActionPriority {
        authority_or_freshness_necessity: false,
        expected_gap_resolution: 1,
        critical_need_impact: 1,
        cost: estimate(Some(1), Some(1), Some("JPY"), Some(1), Some(1)),
    };
    let first = ActionCandidate::new("probe", &fact_gap, priority.clone());
    let other_gap = ActionCandidate::new("probe", &authority_gap, priority.clone());
    let state = KnownStateDigest::from_bytes([3; 32]);
    let mut history = NoProgressHistory::default();
    history.mark(NoProgressKey::for_action(&first, state));

    assert!(matches!(
        select_next_action(&[first], state, &history, "JPY").expect("valid action IDs"),
        Selection::Exhausted
    ));
    assert!(matches!(
        select_next_action(&[other_gap], state, &history, "JPY").expect("valid action IDs"),
        Selection::Selected(_)
    ));
    assert_ne!(
        GapIdentity::from(&fact_gap),
        GapIdentity::from(&authority_gap)
    );

    let mut identified = fact_gap.clone();
    identified.gap_id = Some(GapId::from_uuid(Uuid::from_u128(42)));
    let mut same_identity = authority_gap;
    same_identity.gap_id = identified.gap_id;
    assert_ne!(
        GapIdentity::from(&identified),
        GapIdentity::from(&same_identity)
    );

    let mut changed_fact = identified.clone();
    changed_fact.required_fact = "revised claim".into();
    let mut changed_evidence = identified.clone();
    changed_evidence.acceptable_evidence = vec!["primary".into()];

    let previous = ActionCandidate::new("probe", &identified, priority.clone());
    let mut identified_history = NoProgressHistory::default();
    identified_history.mark(NoProgressKey::for_action(&previous, state));
    for revised_gap in [same_identity, changed_fact, changed_evidence] {
        assert_ne!(
            GapIdentity::from(&identified),
            GapIdentity::from(&revised_gap)
        );
        let revised = ActionCandidate::new("probe", &revised_gap, priority.clone());
        assert!(matches!(
            select_next_action(&[revised], state, &identified_history, "JPY")
                .expect("valid action IDs"),
            Selection::Selected(_)
        ));
    }
}

#[test]
fn structural_gap_identity_canonicalizes_evidence_requirements() {
    let mut first = InformationGap::new("claim", GapReason::MissingFact, true);
    first.acceptable_evidence = vec!["direct".into(), "primary".into()];
    let mut reordered = first.clone();
    reordered.acceptable_evidence.reverse();
    assert_eq!(GapIdentity::from(&first), GapIdentity::from(&reordered));

    let mut different_requirement = first.clone();
    different_requirement
        .acceptable_evidence
        .push("verified".into());
    assert_ne!(
        GapIdentity::from(&first),
        GapIdentity::from(&different_requirement)
    );
    let mut different_blocking = first.clone();
    different_blocking.blocking = false;
    assert_ne!(
        GapIdentity::from(&first),
        GapIdentity::from(&different_blocking)
    );
}

#[test]
fn duplicate_action_ids_are_rejected_before_no_progress_or_eligibility() {
    let first = action("probe", true, false, 1, 1, 1);
    let second = action("probe", true, false, 2, 2, 2).with_eligibility(false);
    let state = KnownStateDigest::from_bytes([4; 32]);
    let mut history = NoProgressHistory::default();
    history.mark(NoProgressKey::for_action(&first, state));

    for actions in [
        [first.clone(), second.clone()],
        [second.clone(), first.clone()],
    ] {
        assert_eq!(
            select_next_action(&actions, state, &history, "JPY").unwrap_err(),
            SelectionError::DuplicateActionId {
                action_id: "probe".into()
            }
        );
    }
}

#[test]
fn duplicate_action_ids_on_different_gaps_are_rejected() {
    let priority = ActionPriority {
        authority_or_freshness_necessity: false,
        expected_gap_resolution: 1,
        critical_need_impact: 1,
        cost: estimate(Some(1), Some(1), Some("JPY"), Some(1), Some(1)),
    };
    let fact_gap = InformationGap::new("claim", GapReason::MissingFact, true);
    let authority_gap = InformationGap::new("authority", GapReason::Authority, true);
    let actions = [
        ActionCandidate::new("shared", &fact_gap, priority.clone()),
        ActionCandidate::new("shared", &authority_gap, priority),
    ];

    assert_eq!(
        select_next_action(
            &actions,
            KnownStateDigest::from_bytes([5; 32]),
            &NoProgressHistory::default(),
            "JPY",
        )
        .unwrap_err(),
        SelectionError::DuplicateActionId {
            action_id: "shared".into()
        }
    );
}

#[test]
fn empty_action_ids_are_rejected() {
    for id in ["", " \t"] {
        assert_eq!(
            select_next_action(
                &[action(id, true, false, 1, 1, 1)],
                KnownStateDigest::from_bytes([6; 32]),
                &NoProgressHistory::default(),
                "JPY",
            )
            .unwrap_err(),
            SelectionError::EmptyActionId { index: 0 }
        );
    }
}

fn estimate(
    remote_calls: Option<u64>,
    monetary_cost_minor_units: Option<i64>,
    currency: Option<&str>,
    latency_ms: Option<u64>,
    content_bytes: Option<u64>,
) -> ActionCostEstimate {
    ActionCostEstimate {
        remote_calls,
        monetary_cost_minor_units,
        currency: currency.map(str::to_owned),
        latency_ms,
        content_bytes,
    }
}

fn cost_candidate(id: &str, cost: ActionCostEstimate) -> ActionCandidate {
    let gap = InformationGap::new(id, GapReason::MissingFact, false);
    ActionCandidate::new(
        id,
        &gap,
        ActionPriority {
            authority_or_freshness_necessity: false,
            expected_gap_resolution: 1,
            critical_need_impact: 1,
            cost,
        },
    )
}

fn selected_id_in_currency<'a>(actions: &'a [ActionCandidate], currency: &str) -> &'a str {
    match select_next_action(
        actions,
        KnownStateDigest::from_bytes([9; 32]),
        &NoProgressHistory::default(),
        currency,
    )
    .expect("valid action IDs and evaluation currency")
    {
        Selection::Selected(action) => action.action_id(),
        Selection::Exhausted => panic!("an eligible action should remain"),
    }
}

#[test]
fn cost_subaxes_are_lexicographic_in_both_input_orders() {
    // A lower-cost earlier subaxis wins even when all later subaxes cost more.
    let cases = [
        (
            estimate(Some(0), Some(99), Some("JPY"), Some(99), Some(99)),
            estimate(Some(1), Some(0), Some("JPY"), Some(0), Some(0)),
        ),
        (
            estimate(Some(0), Some(1), Some("JPY"), Some(99), Some(99)),
            estimate(Some(0), Some(2), Some("JPY"), Some(0), Some(0)),
        ),
        (
            estimate(Some(0), Some(0), Some("JPY"), Some(1), Some(99)),
            estimate(Some(0), Some(0), Some("JPY"), Some(2), Some(0)),
        ),
        (
            estimate(Some(0), Some(0), Some("JPY"), Some(0), Some(1)),
            estimate(Some(0), Some(0), Some("JPY"), Some(0), Some(2)),
        ),
    ];

    for (lower, higher) in cases {
        let winner = cost_candidate("z-lower", lower);
        let loser = cost_candidate("a-higher", higher);
        assert_eq!(
            selected_id_in_currency(&[winner.clone(), loser.clone()], "JPY"),
            "z-lower"
        );
        assert_eq!(selected_id_in_currency(&[loser, winner], "JPY"), "z-lower");
    }
}

#[test]
fn incomplete_or_mismatched_cost_never_looks_cheaper_than_complete_cost() {
    let complete = cost_candidate(
        "z-complete",
        estimate(Some(1), Some(1), Some("JPY"), Some(1), Some(1)),
    );
    let incomplete = [
        estimate(Some(0), None, None, Some(0), Some(0)),
        estimate(Some(0), Some(0), Some("USD"), Some(0), Some(0)),
        estimate(Some(0), Some(-1), Some("JPY"), Some(0), Some(0)),
        estimate(None, Some(0), Some("JPY"), Some(0), Some(0)),
        estimate(Some(0), Some(0), Some("JPY"), None, Some(0)),
        estimate(Some(0), Some(0), Some("JPY"), Some(0), None),
    ];

    for cost in incomplete {
        let unknown = cost_candidate("a-incomplete", cost);
        assert_eq!(
            selected_id_in_currency(&[unknown.clone(), complete.clone()], "JPY"),
            "z-complete"
        );
        assert_eq!(
            selected_id_in_currency(&[complete.clone(), unknown], "JPY"),
            "z-complete"
        );
    }

    // A mismatched currency cannot gain an advantage merely because the
    // comparable candidate also has an unknown, later cost dimension.
    let known_partial = cost_candidate(
        "z-known-partial",
        estimate(Some(1), Some(1), Some("JPY"), Some(1), None),
    );
    let mismatched = cost_candidate(
        "a-mismatched",
        estimate(Some(0), Some(0), Some("USD"), Some(0), Some(0)),
    );
    assert_eq!(
        selected_id_in_currency(&[known_partial.clone(), mismatched.clone()], "JPY"),
        "z-known-partial"
    );
    assert_eq!(
        selected_id_in_currency(&[mismatched, known_partial], "JPY"),
        "z-known-partial"
    );
}

#[test]
fn blocking_requirement_outranks_an_incomplete_cost_estimate() {
    let gap = InformationGap::new("required", GapReason::MissingFact, true);
    let blocking = ActionCandidate::new(
        "z-blocking",
        &gap,
        ActionPriority {
            authority_or_freshness_necessity: false,
            expected_gap_resolution: 0,
            critical_need_impact: 0,
            cost: estimate(None, None, None, None, None),
        },
    );
    let nonblocking = cost_candidate(
        "a-complete",
        estimate(Some(0), Some(0), Some("JPY"), Some(0), Some(0)),
    );
    assert_eq!(
        selected_id_in_currency(&[blocking.clone(), nonblocking.clone()], "JPY"),
        "z-blocking"
    );
    assert_eq!(
        selected_id_in_currency(&[nonblocking, blocking], "JPY"),
        "z-blocking"
    );
}

#[test]
fn monetary_cost_uses_the_callers_trusted_evaluation_currency() {
    let usd = cost_candidate(
        "z-usd",
        estimate(Some(1), Some(1), Some("USD"), Some(1), Some(1)),
    );
    let jpy = cost_candidate(
        "a-jpy",
        estimate(Some(0), Some(0), Some("JPY"), Some(0), Some(0)),
    );
    assert_eq!(
        selected_id_in_currency(&[usd.clone(), jpy.clone()], "USD"),
        "z-usd"
    );
    assert_eq!(selected_id_in_currency(&[usd, jpy], "JPY"), "a-jpy");
}

#[test]
fn empty_evaluation_currency_is_rejected() {
    let actions = [cost_candidate(
        "probe",
        estimate(Some(0), Some(0), Some("JPY"), Some(0), Some(0)),
    )];
    assert_eq!(
        select_next_action(
            &actions,
            KnownStateDigest::from_bytes([10; 32]),
            &NoProgressHistory::default(),
            "",
        )
        .unwrap_err(),
        SelectionError::EmptyEvaluationCurrency
    );
}
