//! Source-native freshness and effective-time coordinates.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TemporalDiscoveryProfile {
    pub freshness_anchor_at: Option<OffsetDateTime>,
    pub freshness_basis: Option<String>,
    pub effective_from: Option<OffsetDateTime>,
    pub effective_to: Option<OffsetDateTime>,
}
