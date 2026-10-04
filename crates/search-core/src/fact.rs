//! Typed facts supplied to deterministic applicability evaluation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::OffsetDateTime;

use crate::id::FactId;
use crate::predicate::TypedValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactOrigin {
    Explicit,
    Authoritative,
    Observed,
    Derived,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub fact_id: Option<FactId>,
    pub value: TypedValue,
    pub origin: FactOrigin,
    pub evidence_ref: Option<String>,
    pub observed_at: Option<OffsetDateTime>,
    pub valid_from: Option<OffsetDateTime>,
    pub valid_to: Option<OffsetDateTime>,
    pub confidence_basis_points: Option<u16>,
}

impl Fact {
    pub fn new(value: TypedValue, origin: FactOrigin) -> Self {
        Self {
            fact_id: None,
            value,
            origin,
            evidence_ref: None,
            observed_at: None,
            valid_from: None,
            valid_to: None,
            confidence_basis_points: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactSet {
    facts: BTreeMap<String, Fact>,
}

impl FactSet {
    pub fn insert(&mut self, key: impl Into<String>, fact: Fact) -> Option<Fact> {
        self.facts.insert(key.into(), fact)
    }

    pub fn get(&self, key: &str) -> Option<&Fact> {
        self.facts.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.facts.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}
