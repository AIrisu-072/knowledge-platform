use document_diff_core::{AlignmentBudget, AlignmentKind, ItemAnchor, align_items};
use document_semantic_inspection_core::{FormatId, InspectionProfileVersion};

fn anchor(path: &str, ordinal: u32, fingerprint: Option<u8>, format: FormatId) -> ItemAnchor {
    ItemAnchor {
        logical_path: path.to_owned(),
        ordinal,
        format: Some(format),
        inspection_profile: InspectionProfileVersion::DsiV0,
        semantic_fingerprint: fingerprint.map(|value| [value; 32]),
    }
}

#[test]
fn exact_unique_path_and_unique_fingerprint_align_without_ids() {
    let base = vec![
        anchor("a", 0, Some(1), FormatId::Txt),
        anchor("b", 1, Some(2), FormatId::Txt),
        anchor("old/c", 2, Some(3), FormatId::Txt),
    ];
    let target = vec![
        anchor("a", 0, Some(4), FormatId::Txt),
        anchor("b", 4, Some(2), FormatId::Txt),
        anchor("new/c", 2, Some(3), FormatId::Txt),
    ];
    let mut budget = AlignmentBudget::new(100);
    let outcome = align_items(&base, &target, &mut budget);
    assert_eq!(outcome.pairs.len(), 3);
    assert_eq!(outcome.pairs[0].kind, AlignmentKind::Exact);
    assert_eq!(outcome.pairs[1].kind, AlignmentKind::Reordered);
    assert_eq!(outcome.pairs[2].kind, AlignmentKind::Relocated);
    assert!(outcome.unresolved.is_empty());
    assert!(outcome.unmatched_base.is_empty());
    assert!(outcome.unmatched_target.is_empty());
}

#[test]
fn duplicate_fingerprints_and_move_plus_edit_remain_unresolved() {
    let base = vec![
        anchor("old/a", 0, Some(1), FormatId::Txt),
        anchor("old/b", 1, Some(1), FormatId::Txt),
    ];
    let target = vec![
        anchor("new/a", 0, Some(1), FormatId::Txt),
        anchor("new/b", 1, Some(2), FormatId::Txt),
    ];
    let outcome = align_items(&base, &target, &mut AlignmentBudget::new(100));
    assert!(outcome.pairs.is_empty());
    assert_eq!(outcome.unresolved.len(), 1);
    assert!(outcome.unmatched_base.is_empty());
    assert!(outcome.unmatched_target.is_empty());
}

#[test]
fn format_change_at_exact_anchor_remains_matched_for_service_review() {
    let base = [anchor("primary", 0, Some(1), FormatId::Txt)];
    let target = [anchor("primary", 0, Some(1), FormatId::Html)];
    let outcome = align_items(&base, &target, &mut AlignmentBudget::new(1));
    assert_eq!(outcome.pairs.len(), 1);
    assert_eq!(outcome.pairs[0].kind, AlignmentKind::Exact);
}

#[test]
fn candidate_limit_marks_the_remaining_range_unresolved() {
    let base = [anchor("a", 0, Some(1), FormatId::Txt)];
    let target = [anchor("a", 0, Some(1), FormatId::Txt)];
    let mut budget = AlignmentBudget::new(0);
    let outcome = align_items(&base, &target, &mut budget);
    assert!(outcome.exhausted);
    assert!(outcome.pairs.is_empty());
    assert_eq!(outcome.unresolved.len(), 1);
}
