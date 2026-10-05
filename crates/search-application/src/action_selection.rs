//! Pure priority selection for the evidence-driven Discovery loop.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use search_core::discovery::{GapReason, InformationGap};
use search_core::id::GapId;

/// Gap ID and the requirements it currently names. A reused ID with changed
/// requirements is a new no-progress identity. Evidence-class order has no
/// semantic meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapIdentity {
    gap_id: Option<GapId>,
    required_fact: String,
    reason: GapReason,
    blocking: bool,
    acceptable_evidence: Vec<String>,
}

impl From<&InformationGap> for GapIdentity {
    fn from(gap: &InformationGap) -> Self {
        let mut acceptable_evidence = gap.acceptable_evidence.clone();
        acceptable_evidence.sort();
        acceptable_evidence.dedup();
        Self {
            gap_id: gap.gap_id,
            required_fact: gap.required_fact.clone(),
            reason: gap.reason,
            blocking: gap.blocking,
            acceptable_evidence,
        }
    }
}

/// C7-compatible estimate dimensions. `None` means unknown, not zero.
/// Monetary estimates are comparable only in the caller's trusted evaluation
/// currency; negative amounts and missing/mismatched currency are unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCostEstimate {
    pub remote_calls: Option<u64>,
    pub monetary_cost_minor_units: Option<i64>,
    pub currency: Option<String>,
    pub latency_ms: Option<u64>,
    pub content_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPriority {
    /// True when this action is needed to resolve authority or freshness.
    pub authority_or_freshness_necessity: bool,
    /// Higher ordinal estimate wins after the preceding hard priorities.
    pub expected_gap_resolution: u8,
    /// Higher ordinal impact wins after expected resolution.
    pub critical_need_impact: u8,
    /// Compared only after the preceding four priority axes tie.
    pub cost: ActionCostEstimate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCandidate {
    action_id: String,
    gap: GapIdentity,
    blocking_requirement: bool,
    eligible: bool,
    priority: ActionPriority,
}

impl ActionCandidate {
    /// `action_id` must remain stable for the same action parameters across
    /// evaluations. A changed target or expanded probe window needs a new ID.
    pub fn new(
        action_id: impl Into<String>,
        gap: &InformationGap,
        mut priority: ActionPriority,
    ) -> Self {
        priority.authority_or_freshness_necessity |=
            matches!(gap.reason, GapReason::Authority | GapReason::Freshness);
        Self {
            action_id: action_id.into(),
            gap: GapIdentity::from(gap),
            blocking_requirement: gap.blocking,
            eligible: true,
            priority,
        }
    }

    pub fn action_id(&self) -> &str {
        &self.action_id
    }

    pub fn with_eligibility(mut self, eligible: bool) -> Self {
        self.eligible = eligible;
        self
    }
}

/// Digest supplied by C8 from its canonical known facts and evidence state.
/// Canonicalization must be deterministic and independent of input ordering:
/// unchanged facts/evidence retain the digest, while a meaningful change to
/// either changes it. Exclude elapsed time, attempt counts and action history;
/// this pure selector never computes or validates the digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownStateDigest([u8; 32]);

impl KnownStateDigest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoProgressKey {
    pub gap: GapIdentity,
    pub known_state_digest: KnownStateDigest,
    pub attempted_action_id: String,
}

impl NoProgressKey {
    pub fn for_action(action: &ActionCandidate, known_state_digest: KnownStateDigest) -> Self {
        Self {
            gap: action.gap.clone(),
            known_state_digest,
            attempted_action_id: action.action_id.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct NoProgressHistory {
    keys: Vec<NoProgressKey>,
}

impl NoProgressHistory {
    /// Record only after an attempted action produced no new known state.
    /// Repeated records are idempotent and do not create a retry budget.
    pub fn mark(&mut self, key: NoProgressKey) -> bool {
        if self.keys.contains(&key) {
            return false;
        }
        self.keys.push(key);
        true
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[derive(Debug)]
pub enum Selection<'a> {
    Selected(&'a ActionCandidate),
    /// No eligible action remains for this unchanged known state.
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    EmptyActionId { index: usize },
    DuplicateActionId { action_id: String },
    EmptyEvaluationCurrency,
}

/// Pick one action by the four approved priority axes, then cost and stable ID.
/// Cost is lexicographic: comparable monetary currency first, complete estimate
/// first, then fewer remote calls, lower monetary minor units, lower latency,
/// fewer content bytes. On each dimension known precedes unknown. No currency
/// conversion or weighted sum is performed. Recorded no-progress keys make an
/// unchanged finite action set terminate.
pub fn select_next_action<'a>(
    actions: &'a [ActionCandidate],
    known_state_digest: KnownStateDigest,
    no_progress: &NoProgressHistory,
    trusted_evaluation_currency: &str,
) -> Result<Selection<'a>, SelectionError> {
    let mut seen = BTreeSet::new();
    for (index, action) in actions.iter().enumerate() {
        if action.action_id.trim().is_empty() {
            return Err(SelectionError::EmptyActionId { index });
        }
        if !seen.insert(action.action_id.as_str()) {
            return Err(SelectionError::DuplicateActionId {
                action_id: action.action_id.clone(),
            });
        }
    }
    if trusted_evaluation_currency.trim().is_empty() {
        return Err(SelectionError::EmptyEvaluationCurrency);
    }

    Ok(actions
        .iter()
        .filter(|action| {
            action.eligible
                && !no_progress
                    .keys
                    .contains(&NoProgressKey::for_action(action, known_state_digest))
        })
        .max_by(|left, right| compare_priority(left, right, trusted_evaluation_currency))
        .map_or(Selection::Exhausted, Selection::Selected))
}

fn compare_priority(
    left: &ActionCandidate,
    right: &ActionCandidate,
    trusted_evaluation_currency: &str,
) -> Ordering {
    left.blocking_requirement
        .cmp(&right.blocking_requirement)
        .then_with(|| {
            left.priority
                .authority_or_freshness_necessity
                .cmp(&right.priority.authority_or_freshness_necessity)
        })
        .then_with(|| {
            left.priority
                .expected_gap_resolution
                .cmp(&right.priority.expected_gap_resolution)
        })
        .then_with(|| {
            left.priority
                .critical_need_impact
                .cmp(&right.priority.critical_need_impact)
        })
        .then_with(|| {
            compare_cost(
                &left.priority.cost,
                &right.priority.cost,
                trusted_evaluation_currency,
            )
        })
        .then_with(|| right.action_id.cmp(&left.action_id))
}

fn compare_cost(
    left: &ActionCostEstimate,
    right: &ActionCostEstimate,
    trusted_evaluation_currency: &str,
) -> Ordering {
    let left_money = comparable_monetary_cost(left, trusted_evaluation_currency);
    let right_money = comparable_monetary_cost(right, trusted_evaluation_currency);
    let left_complete = left.remote_calls.is_some()
        && left_money.is_some()
        && left.latency_ms.is_some()
        && left.content_bytes.is_some();
    let right_complete = right.remote_calls.is_some()
        && right_money.is_some()
        && right.latency_ms.is_some()
        && right.content_bytes.is_some();

    left_money
        .is_some()
        .cmp(&right_money.is_some())
        .then_with(|| left_complete.cmp(&right_complete))
        .then_with(|| compare_cost_axis(left.remote_calls, right.remote_calls))
        .then_with(|| compare_cost_axis(left_money, right_money))
        .then_with(|| compare_cost_axis(left.latency_ms, right.latency_ms))
        .then_with(|| compare_cost_axis(left.content_bytes, right.content_bytes))
}

fn comparable_monetary_cost(
    cost: &ActionCostEstimate,
    trusted_evaluation_currency: &str,
) -> Option<u64> {
    match (cost.monetary_cost_minor_units, cost.currency.as_deref()) {
        (Some(amount), Some(currency)) if currency == trusted_evaluation_currency => {
            u64::try_from(amount).ok()
        }
        _ => None,
    }
}

fn compare_cost_axis(left: Option<u64>, right: Option<u64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.cmp(&left),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}
