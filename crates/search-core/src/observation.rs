//! Remote observations remain separate from resource lifecycle state.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Presence {
    Present,
    Absent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reachability {
    Reachable,
    Unreachable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coverage {
    CompleteEnumeration,
    PartialEnumeration,
    QueryResult,
    DirectLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Freshness {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceObservation {
    pub resource_ref: String,
    pub observed_at: OffsetDateTime,
    pub observation_method: String,
    pub presence: Presence,
    pub reachability: Reachability,
    pub source_coverage: Coverage,
    pub remote_version: Option<String>,
    pub etag: Option<String>,
    pub digest: Option<String>,
}

impl ResourceObservation {
    pub fn new(
        resource_ref: impl Into<String>,
        observed_at: OffsetDateTime,
        observation_method: impl Into<String>,
        source_coverage: Coverage,
        presence: Presence,
    ) -> Self {
        Self {
            resource_ref: resource_ref.into(),
            observed_at,
            observation_method: observation_method.into(),
            presence,
            reachability: Reachability::Unknown,
            source_coverage,
            remote_version: None,
            etag: None,
            digest: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityConflict {
    pub remote_version: String,
    pub digests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveResourceState {
    pub presence: Presence,
    pub reachability: Reachability,
    pub observed_at: Option<OffsetDateTime>,
    pub integrity_conflict: Option<IntegrityConflict>,
}

pub fn derive_effective_resource_state(
    resource_ref: &str,
    observations: &[ResourceObservation],
) -> EffectiveResourceState {
    let observations: Vec<&ResourceObservation> = observations
        .iter()
        .filter(|observation| observation.resource_ref == resource_ref)
        .collect();
    let mut digests_by_version: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for observation in &observations {
        if let (Some(version), Some(digest)) = (
            observation.remote_version.as_deref(),
            observation.digest.as_deref(),
        ) {
            digests_by_version
                .entry(version)
                .or_default()
                .insert(digest);
        }
    }
    let integrity_conflict = digests_by_version
        .into_iter()
        .find(|(_, digests)| digests.len() > 1)
        .map(|(remote_version, digests)| IntegrityConflict {
            remote_version: remote_version.into(),
            digests: digests.into_iter().map(str::to_owned).collect(),
        });

    let latest = observations
        .into_iter()
        .max_by_key(|observation| observation.observed_at);
    let (presence, reachability, observed_at) = match latest {
        None => (Presence::Unknown, Reachability::Unknown, None),
        Some(observation) => {
            let presence = match (observation.presence, observation.source_coverage) {
                (Presence::Absent, Coverage::PartialEnumeration | Coverage::QueryResult) => {
                    Presence::Unknown
                }
                (presence, _) => presence,
            };
            (
                presence,
                observation.reachability,
                Some(observation.observed_at),
            )
        }
    };
    EffectiveResourceState {
        presence,
        reachability,
        observed_at,
        integrity_conflict,
    }
}
