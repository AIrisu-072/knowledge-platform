use std::collections::{BTreeMap, BTreeSet};

use crate::{CapabilityEvidence, CapabilityState};

/// A capability-based assessment only; this does not perform a rendition or migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityMigrationDecision {
    Eligible,
    Denied { missing: Vec<String> },
}

/// Preserve the PoC-qualified cross-format capability rule without comparing
/// format-specific semantic fingerprints or constructing a shared content model.
pub fn assess_authority_migration(
    source: &[CapabilityEvidence],
    target: &[CapabilityEvidence],
) -> AuthorityMigrationDecision {
    let target_by_id: BTreeMap<&str, &CapabilityEvidence> = target
        .iter()
        .map(|capability| (capability.capability_id.as_str(), capability))
        .collect();
    let mut missing = BTreeSet::new();

    for source_capability in source {
        if !source_capability.version_significant {
            continue;
        }
        match source_capability.presence {
            CapabilityState::Absent => continue,
            CapabilityState::NotRepresentable | CapabilityState::NotVerifiable => {
                missing.insert(source_capability.capability_id.clone());
                continue;
            }
            CapabilityState::Present => {}
        }

        let Some(target_capability) = target_by_id.get(source_capability.capability_id.as_str())
        else {
            missing.insert(source_capability.capability_id.clone());
            continue;
        };
        if target_capability.presence != CapabilityState::Present
            || (source_capability.equivalence_fingerprint.is_some()
                && source_capability.equivalence_fingerprint
                    != target_capability.equivalence_fingerprint)
        {
            missing.insert(source_capability.capability_id.clone());
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
