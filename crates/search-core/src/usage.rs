//! A Resource can be selected for multiple distinct uses.

use serde::{Deserialize, Serialize};

use crate::id::{PredicateId, ResourceId, UsageProfileId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageProfile {
    pub usage_profile_id: UsageProfileId,
    pub purpose: String,
    pub business_domain: Option<String>,
    pub business_operation: Option<String>,
    pub subject: Option<String>,
    pub object: Option<String>,
    pub product_scope: Option<String>,
    pub lifecycle_phase: Option<String>,
    pub actor: Option<String>,
    pub trigger: Option<String>,
    pub action: Option<String>,
    pub effects: Option<String>,
    pub applicable_when: Vec<PredicateId>,
    pub not_applicable_when: Vec<PredicateId>,
    pub required_context: Vec<String>,
    pub required_resources: Vec<ResourceId>,
    pub expected_input: Option<String>,
    pub expected_outcome: Option<String>,
}

impl UsageProfile {
    pub fn new(usage_profile_id: UsageProfileId, purpose: impl Into<String>) -> Self {
        Self {
            usage_profile_id,
            purpose: purpose.into(),
            business_domain: None,
            business_operation: None,
            subject: None,
            object: None,
            product_scope: None,
            lifecycle_phase: None,
            actor: None,
            trigger: None,
            action: None,
            effects: None,
            applicable_when: Vec::new(),
            not_applicable_when: Vec::new(),
            required_context: Vec::new(),
            required_resources: Vec::new(),
            expected_input: None,
            expected_outcome: None,
        }
    }
}
