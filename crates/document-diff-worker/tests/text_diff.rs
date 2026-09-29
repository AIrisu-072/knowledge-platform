use document_diff_core::{
    ChangeOperation, ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId,
    ResourceProfileVersion, SourceLocator, UnverifiedReason, WorkerDiffRequest,
    WorkerProtocolVersion,
};
use document_diff_worker::{TextComparator, WorkerError, run_worker_shell};
use sha2::{Digest, Sha256};

fn request(base: &[u8], target: &[u8]) -> WorkerDiffRequest {
    WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Txt,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    }
}

fn compare(base: &[u8], target: &[u8]) -> document_diff_core::WorkerDiffResponse {
    let request = request(base, target);
    let mut budget = ComparisonBudget::new(100_000, 100_000);
    let result = TextComparator::compare(&request, base, target, &mut budget).unwrap();
    result.validate_against(&request).unwrap();
    result
}

#[test]
fn crlf_and_unicode_normalization_are_semantically_invariant() {
    let base = "café\n".as_bytes();
    for target in ["café\r\n".as_bytes(), "cafe\u{301}\n".as_bytes()] {
        let result = compare(base, target);
        assert_eq!(result.coverage, DiffCoverage::Full);
        assert!(result.changes.is_empty());
        assert!(result.unverified_regions.is_empty());
    }
}

#[test]
fn modified_line_has_old_and_new_raw_spans() {
    let result = compare(
        b"header\nold value\nfooter\n",
        b"header\r\nnew value\r\nfooter\r\n",
    );
    assert_eq!(result.coverage, DiffCoverage::Full);
    assert_eq!(result.changes.len(), 1);
    let change = &result.changes[0];
    assert_eq!(change.operation, Some(ChangeOperation::Modified));
    assert_eq!(change.facet, "text");
    assert_eq!(
        change.base,
        Some(SourceLocator::TextSpan {
            line: 2,
            byte_start: 7,
            byte_end: 17,
        })
    );
    assert_eq!(
        change.target,
        Some(SourceLocator::TextSpan {
            line: 2,
            byte_start: 8,
            byte_end: 19,
        })
    );
}

#[test]
fn inserted_and_removed_lines_keep_only_the_authoritative_side() {
    let added = compare(b"alpha\nomega\n", b"alpha\ninserted\nomega\n");
    assert_eq!(added.coverage, DiffCoverage::Full);
    assert_eq!(added.changes.len(), 1);
    assert_eq!(added.changes[0].operation, Some(ChangeOperation::Added));
    assert_eq!(added.changes[0].base, None);
    assert_eq!(
        added.changes[0].target,
        Some(SourceLocator::TextSpan {
            line: 2,
            byte_start: 6,
            byte_end: 15,
        })
    );

    let removed = compare(b"alpha\nremoved\nomega\n", b"alpha\nomega\n");
    assert_eq!(removed.coverage, DiffCoverage::Full);
    assert_eq!(removed.changes.len(), 1);
    assert_eq!(removed.changes[0].operation, Some(ChangeOperation::Removed));
    assert_eq!(removed.changes[0].target, None);
    assert_eq!(
        removed.changes[0].base,
        Some(SourceLocator::TextSpan {
            line: 2,
            byte_start: 6,
            byte_end: 14,
        })
    );
}

#[test]
fn ambiguous_decoding_keeps_the_whole_item_unverified() {
    let result = compare(b"valid\n", b"invalid\xff\n");
    assert_eq!(result.coverage, DiffCoverage::None);
    assert!(result.changes.is_empty());
    assert_eq!(result.unverified_regions.len(), 1);
    assert_eq!(
        result.unverified_regions[0].reason,
        UnverifiedReason::UnsupportedSemanticConstruct
    );
    assert_eq!(
        result.unverified_regions[0].base,
        Some(SourceLocator::ContentItem)
    );
    assert_eq!(
        result.unverified_regions[0].target,
        Some(SourceLocator::ContentItem)
    );
}

#[test]
fn shell_dispatch_and_direct_adapter_both_preserve_raw_binding() {
    let base = b"old\n";
    let target = b"new\n";
    let request = request(base, target);
    let shell = run_worker_shell(
        &serde_json::to_vec(&request).unwrap(),
        std::io::Cursor::new(base),
        std::io::Cursor::new(target),
    )
    .unwrap();
    assert_eq!(shell.coverage, DiffCoverage::Full);
    assert_eq!(shell.changes.len(), 1);

    let mut budget = ComparisonBudget::new(100, 100);
    assert_eq!(
        TextComparator::compare(&request, base, b"tampered\n", &mut budget).unwrap_err(),
        WorkerError::RawBindingMismatch
    );
}
