use std::collections::BTreeMap;

use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemAnchor {
    pub logical_path: String,
    pub ordinal: u32,
    pub format: Option<FormatId>,
    pub inspection_profile: InspectionProfileVersion,
    pub semantic_fingerprint: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentKind {
    Exact,
    Reordered,
    Relocated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlignedPair {
    pub base_index: usize,
    pub target_index: usize,
    pub kind: AlignmentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedCluster {
    pub base_indices: Vec<usize>,
    pub target_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignmentOutcome {
    pub pairs: Vec<AlignedPair>,
    pub unmatched_base: Vec<usize>,
    pub unmatched_target: Vec<usize>,
    pub unresolved: Vec<UnresolvedCluster>,
    pub exhausted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlignmentBudget {
    limit: u64,
    used: u64,
}

impl AlignmentBudget {
    pub const fn new(max_candidates: u64) -> Self {
        Self {
            limit: max_candidates,
            used: 0,
        }
    }
    pub const fn candidates_used(&self) -> u64 {
        self.used
    }
    pub const fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }
    fn charge(&mut self, count: u64) -> bool {
        match self.used.checked_add(count) {
            Some(next) if next <= self.limit => {
                self.used = next;
                true
            }
            _ => false,
        }
    }
}

pub fn align_items(
    base: &[ItemAnchor],
    target: &[ItemAnchor],
    budget: &mut AlignmentBudget,
) -> AlignmentOutcome {
    let mut pairs = Vec::new();
    let mut base_used = vec![false; base.len()];
    let mut target_used = vec![false; target.len()];

    let mut base_exact: BTreeMap<(&str, u32), Vec<usize>> = BTreeMap::new();
    let mut target_exact: BTreeMap<(&str, u32), Vec<usize>> = BTreeMap::new();
    for (i, item) in base.iter().enumerate() {
        base_exact
            .entry((&item.logical_path, item.ordinal))
            .or_default()
            .push(i);
    }
    for (i, item) in target.iter().enumerate() {
        target_exact
            .entry((&item.logical_path, item.ordinal))
            .or_default()
            .push(i);
    }
    for (key, old) in &base_exact {
        if let Some(new) = target_exact.get(key)
            && old.len() == 1
            && new.len() == 1
        {
            if !budget.charge(1) {
                return finish(pairs, &base_used, &target_used, true);
            }
            pair(
                &mut pairs,
                &mut base_used,
                &mut target_used,
                old[0],
                new[0],
                AlignmentKind::Exact,
            );
        }
    }

    let mut base_paths: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut target_paths: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, item) in base.iter().enumerate().filter(|(i, _)| !base_used[*i]) {
        base_paths.entry(&item.logical_path).or_default().push(i);
    }
    for (i, item) in target.iter().enumerate().filter(|(i, _)| !target_used[*i]) {
        target_paths.entry(&item.logical_path).or_default().push(i);
    }
    for (path, old) in &base_paths {
        if let Some(new) = target_paths.get(path)
            && old.len() == 1
            && new.len() == 1
        {
            if !budget.charge(1) {
                return finish(pairs, &base_used, &target_used, true);
            }
            pair(
                &mut pairs,
                &mut base_used,
                &mut target_used,
                old[0],
                new[0],
                AlignmentKind::Reordered,
            );
        }
    }

    type FingerprintKey = (FormatId, InspectionProfileVersion, [u8; 32]);
    let mut base_fingerprints: BTreeMap<FingerprintKey, Vec<usize>> = BTreeMap::new();
    let mut target_fingerprints: BTreeMap<FingerprintKey, Vec<usize>> = BTreeMap::new();
    for (i, item) in base.iter().enumerate().filter(|(i, _)| !base_used[*i]) {
        if let (Some(format), Some(fingerprint)) = (item.format, item.semantic_fingerprint) {
            base_fingerprints
                .entry((format, item.inspection_profile, fingerprint))
                .or_default()
                .push(i);
        }
    }
    for (i, item) in target.iter().enumerate().filter(|(i, _)| !target_used[*i]) {
        if let (Some(format), Some(fingerprint)) = (item.format, item.semantic_fingerprint) {
            target_fingerprints
                .entry((format, item.inspection_profile, fingerprint))
                .or_default()
                .push(i);
        }
    }
    for (key, old) in &base_fingerprints {
        if let Some(new) = target_fingerprints.get(key) {
            let count = (old.len() as u64).saturating_mul(new.len() as u64);
            if !budget.charge(count) {
                return finish(pairs, &base_used, &target_used, true);
            }
            if old.len() == 1 && new.len() == 1 {
                pair(
                    &mut pairs,
                    &mut base_used,
                    &mut target_used,
                    old[0],
                    new[0],
                    AlignmentKind::Relocated,
                );
            }
        }
    }
    finish(pairs, &base_used, &target_used, false)
}

fn pair(
    pairs: &mut Vec<AlignedPair>,
    base_used: &mut [bool],
    target_used: &mut [bool],
    base_index: usize,
    target_index: usize,
    kind: AlignmentKind,
) {
    base_used[base_index] = true;
    target_used[target_index] = true;
    pairs.push(AlignedPair {
        base_index,
        target_index,
        kind,
    });
}

fn finish(
    mut pairs: Vec<AlignedPair>,
    base_used: &[bool],
    target_used: &[bool],
    exhausted: bool,
) -> AlignmentOutcome {
    pairs.sort_by_key(|pair| (pair.base_index, pair.target_index));
    let base_remaining: Vec<_> = base_used
        .iter()
        .enumerate()
        .filter_map(|(i, used)| (!used).then_some(i))
        .collect();
    let target_remaining: Vec<_> = target_used
        .iter()
        .enumerate()
        .filter_map(|(i, used)| (!used).then_some(i))
        .collect();
    let ambiguous = exhausted || (!base_remaining.is_empty() && !target_remaining.is_empty());
    if ambiguous {
        AlignmentOutcome {
            pairs,
            unmatched_base: Vec::new(),
            unmatched_target: Vec::new(),
            unresolved: vec![UnresolvedCluster {
                base_indices: base_remaining,
                target_indices: target_remaining,
            }],
            exhausted,
        }
    } else {
        AlignmentOutcome {
            pairs,
            unmatched_base: base_remaining,
            unmatched_target: target_remaining,
            unresolved: Vec::new(),
            exhausted,
        }
    }
}
