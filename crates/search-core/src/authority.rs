//! Explicit, scoped authority resolution that preserves equal-rank conflicts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::assertion::{Assertion, AssertionOrigin};
use crate::predicate::{TypedValue, semantically_equal};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityPolicy {
    ranks: BTreeMap<(String, String, AssertionOrigin), u16>,
}

impl AuthorityPolicy {
    pub fn grant_rank(
        &mut self,
        authority_scope: impl Into<String>,
        predicate: impl Into<String>,
        origin: AssertionOrigin,
        rank: u16,
    ) {
        self.ranks
            .insert((authority_scope.into(), predicate.into(), origin), rank);
    }

    pub fn rank(
        &self,
        authority_scope: &str,
        predicate: &str,
        origin: AssertionOrigin,
    ) -> Option<u16> {
        self.ranks
            .get(&(authority_scope.into(), predicate.into(), origin))
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityConflict {
    pub subject_ref: String,
    pub predicate: String,
    pub authority_scope: String,
    pub rank: u16,
    pub values: Vec<TypedValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityResolution {
    Unresolved,
    Resolved(TypedValue),
    Conflict(AuthorityConflict),
}

pub fn resolve_assertions(
    assertions: &[Assertion],
    policy: &AuthorityPolicy,
    subject_ref: &str,
    predicate: &str,
    authority_scope: &str,
) -> AuthorityResolution {
    let scoped: Vec<(&Assertion, u16)> = assertions
        .iter()
        .filter(|assertion| {
            assertion.subject_ref == subject_ref
                && assertion.predicate == predicate
                && assertion.authority_scope == authority_scope
        })
        .filter_map(|assertion| {
            policy
                .rank(authority_scope, predicate, assertion.origin)
                .map(|rank| (assertion, rank))
        })
        .collect();

    // Inference cannot displace an explicit curated or authoritative assertion,
    // even if a policy accidentally grants it a larger numeric rank.
    let has_protected = scoped.iter().any(|(assertion, _)| {
        matches!(
            assertion.origin,
            AssertionOrigin::Authoritative | AssertionOrigin::Curated
        )
    });
    let eligible: Vec<(&Assertion, u16)> = scoped
        .into_iter()
        .filter(|(assertion, _)| !has_protected || assertion.origin != AssertionOrigin::Inferred)
        .collect();

    let Some(best_rank) = eligible.iter().map(|(_, rank)| rank).max().copied() else {
        return AuthorityResolution::Unresolved;
    };
    let mut values = Vec::new();
    for (assertion, rank) in eligible {
        if rank == best_rank
            && !values
                .iter()
                .any(|existing| semantically_equal(existing, &assertion.value))
        {
            values.push(assertion.value.clone());
        }
    }
    if values.len() == 1 {
        AuthorityResolution::Resolved(values.remove(0))
    } else {
        AuthorityResolution::Conflict(AuthorityConflict {
            subject_ref: subject_ref.into(),
            predicate: predicate.into(),
            authority_scope: authority_scope.into(),
            rank: best_rank,
            values,
        })
    }
}
