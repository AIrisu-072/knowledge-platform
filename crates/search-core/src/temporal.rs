//! Source-native freshness and effective-time coordinates.

use serde::{Deserialize, Serialize};
use time::Duration;
use time::OffsetDateTime;

use crate::id::DiscoveryEvaluationId;
use crate::observation::Freshness;
use crate::predicate::TruthValue;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TemporalDiscoveryProfile {
    pub freshness_anchor_at: Option<OffsetDateTime>,
    pub freshness_basis: Option<String>,
    pub effective_from: Option<OffsetDateTime>,
    pub effective_to: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalEvaluationContext {
    pub evaluation_id: DiscoveryEvaluationId,
    pub evaluated_at: OffsetDateTime,
    pub temporal_target: OffsetDateTime,
    pub business_timezone: String,
}

impl TemporalEvaluationContext {
    pub fn new(
        evaluation_id: DiscoveryEvaluationId,
        evaluated_at: OffsetDateTime,
        temporal_target: OffsetDateTime,
        business_timezone: impl Into<String>,
    ) -> Self {
        Self {
            evaluation_id,
            evaluated_at,
            temporal_target,
            business_timezone: business_timezone.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalEvaluation {
    pub freshness: Freshness,
    pub effective_at_target: TruthValue,
}

pub fn evaluate_temporal_profile(
    profile: &TemporalDiscoveryProfile,
    context: &TemporalEvaluationContext,
    max_current_age: Option<Duration>,
) -> TemporalEvaluation {
    let freshness = match (profile.freshness_anchor_at, max_current_age) {
        (Some(anchor), Some(limit))
            if limit >= Duration::ZERO && anchor <= context.evaluated_at =>
        {
            if context.evaluated_at - anchor > limit {
                Freshness::Stale
            } else {
                Freshness::Fresh
            }
        }
        _ => Freshness::Unknown,
    };
    let effective_at_target = if profile.effective_from.is_none() && profile.effective_to.is_none()
    {
        TruthValue::Unknown
    } else {
        let in_window = profile
            .effective_from
            .is_none_or(|from| context.temporal_target >= from)
            && profile
                .effective_to
                .is_none_or(|to| context.temporal_target < to);
        if in_window {
            TruthValue::True
        } else {
            TruthValue::False
        }
    };
    TemporalEvaluation {
        freshness,
        effective_at_target,
    }
}
