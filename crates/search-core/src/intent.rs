//! Typed intent facets retain their provenance across discovery.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentFactOrigin {
    Explicit,
    Derived,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentFact<T> {
    pub value: T,
    pub origin: IntentFactOrigin,
}

impl<T> IntentFact<T> {
    pub const fn new(value: T, origin: IntentFactOrigin) -> Self {
        Self { value, origin }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentSignature {
    pub purpose: IntentFact<String>,
    pub business_domain: Option<IntentFact<String>>,
    pub business_operation: Option<IntentFact<String>>,
    pub subject: Option<IntentFact<String>>,
    pub object: Option<IntentFact<String>>,
    pub product_scope: Option<IntentFact<String>>,
    pub lifecycle_phase: Option<IntentFact<String>>,
    pub actor: Option<IntentFact<String>>,
    pub trigger: Option<IntentFact<String>>,
    pub requested_action: Option<IntentFact<String>>,
    pub expected_effect: Option<IntentFact<String>>,
    pub temporal_target: Option<IntentFact<OffsetDateTime>>,
    pub required_authority: Option<IntentFact<String>>,
    pub required_freshness: Option<IntentFact<String>>,
    pub constraints: Vec<IntentFact<String>>,
    pub unresolved_facets: Vec<String>,
}

impl IntentSignature {
    pub fn new(purpose: IntentFact<String>) -> Self {
        Self {
            purpose,
            business_domain: None,
            business_operation: None,
            subject: None,
            object: None,
            product_scope: None,
            lifecycle_phase: None,
            actor: None,
            trigger: None,
            requested_action: None,
            expected_effect: None,
            temporal_target: None,
            required_authority: None,
            required_freshness: None,
            constraints: Vec::new(),
            unresolved_facets: Vec::new(),
        }
    }
}
