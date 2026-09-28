//! Provenance-preserving claims, before authority resolution.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::id::AssertionId;
use crate::predicate::TypedValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AssertionOrigin {
    Authoritative,
    Declared,
    Curated,
    Derived,
    Extracted,
    Observed,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    pub assertion_id: Option<AssertionId>,
    pub subject_ref: String,
    pub predicate: String,
    pub value: TypedValue,
    pub source_ref: String,
    pub origin: AssertionOrigin,
    pub authority_scope: String,
    pub evidence_refs: Vec<String>,
    pub observed_at: OffsetDateTime,
    pub effective_from: Option<OffsetDateTime>,
    pub effective_to: Option<OffsetDateTime>,
    pub derived_by: Option<String>,
}

impl Assertion {
    pub fn new(
        subject_ref: impl Into<String>,
        predicate: impl Into<String>,
        value: TypedValue,
        source_ref: impl Into<String>,
        origin: AssertionOrigin,
        authority_scope: impl Into<String>,
        observed_at: OffsetDateTime,
    ) -> Self {
        Self {
            assertion_id: None,
            subject_ref: subject_ref.into(),
            predicate: predicate.into(),
            value,
            source_ref: source_ref.into(),
            origin,
            authority_scope: authority_scope.into(),
            evidence_refs: Vec::new(),
            observed_at,
            effective_from: None,
            effective_to: None,
            derived_by: None,
        }
    }
}
