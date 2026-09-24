use crate::{CapabilityEvidence, CapabilityState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum AuthorityMigrationDecision {
    Eligible,
    Denied { missing: Vec<String> },
}

pub fn assess_authority_migration(
    source: &[CapabilityEvidence],
    target: &[CapabilityEvidence],
) -> AuthorityMigrationDecision {
    let target_by_id: BTreeMap<&str, &CapabilityEvidence> = target
        .iter()
        .map(|capability| (capability.capability.as_str(), capability))
        .collect();

    let mut missing = BTreeSet::new();

    for source_capability in source {
        if !source_capability.version_significant {
            continue;
        }

        match source_capability.state {
            CapabilityState::Absent => continue,
            CapabilityState::NotRepresentable | CapabilityState::NotVerifiable => {
                missing.insert(source_capability.capability.clone());
                continue;
            }
            CapabilityState::Present => {}
        }

        let Some(target_capability) = target_by_id.get(source_capability.capability.as_str()) else {
            missing.insert(source_capability.capability.clone());
            continue;
        };

        if target_capability.state != CapabilityState::Present {
            missing.insert(source_capability.capability.clone());
            continue;
        }

        if let Some(source_fingerprint) = source_capability.equivalence_fingerprint.as_deref() {
            if target_capability.equivalence_fingerprint.as_deref() != Some(source_fingerprint) {
                missing.insert(source_capability.capability.clone());
            }
        }
    }

    if missing.is_empty() {
        AuthorityMigrationDecision::Eligible
    } else {
        AuthorityMigrationDecision::Denied {
            missing: missing.into_iter().collect(),
        }
    }
}
