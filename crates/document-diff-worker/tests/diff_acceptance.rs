use std::time::Instant;

use document_diff_core::{
    ComparisonBudget, DiffCoverage, DiffProfileVersion, FormatId, ResourceProfileVersion,
    SourceLocator, UnverifiedReason, WorkerDiffRequest, WorkerDiffResponse, WorkerProtocolVersion,
};
use document_diff_worker::{
    CsvComparator, DocxComparator, HtmlComparator, PdfComparator, PptxComparator,
    SpreadsheetComparator, TextComparator,
};
use sha2::{Digest, Sha256};

struct Case {
    format: FormatId,
    base: &'static [u8],
    changed: &'static [u8],
    equal_noise: &'static [u8],
    unverified: &'static [u8],
}

const CASES: &[Case] = &[
    Case {
        format: FormatId::Txt,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/txt/base.txt"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/txt/text-change.txt"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/txt/crlf.txt"
        ),
        unverified: b"invalid\xff\n",
    },
    Case {
        format: FormatId::Csv,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/csv/base.csv"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/csv/cell-change.csv"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/csv/quote-noise.csv"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/csv/inconsistent.csv"
        ),
    },
    Case {
        format: FormatId::Html,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/html/base.html"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/html/text-change.html"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/html/noise.html"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/html/js-only.html"
        ),
    },
    Case {
        format: FormatId::Docx,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/docx/base.docx"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/docx/body-text-change.docx"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/docx/metadata-noise.docx"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/docx/unknown-semantic-part.docx"
        ),
    },
    Case {
        format: FormatId::Xlsx,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/xlsx/cell-value-change.xlsx"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/xlsx/cached-result-only.xlsx"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/xlsx/unknown-semantic-part.xlsx"
        ),
    },
    Case {
        format: FormatId::Xlsm,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"
        ),
        changed: include_bytes!(
            "../../../experiments/document-diff/fixtures/xlsm/logic-change.xlsm"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-diff/fixtures/xlsm/whitespace-noise.xlsm"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-diff/fixtures/xlsm/invalid-syntax.xlsm"
        ),
    },
    Case {
        format: FormatId::Pptx,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pptx/text-change.pptx"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pptx/theme-only.pptx"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pptx/unknown-semantic-part.pptx"
        ),
    },
    Case {
        format: FormatId::Pdf,
        base: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pdf/base.pdf"
        ),
        changed: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pdf/text-change.pdf"
        ),
        equal_noise: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pdf/object-id-producer-noise.pdf"
        ),
        unverified: include_bytes!(
            "../../../experiments/document-semantic-inspection/fixtures/pdf/scan-only.pdf"
        ),
    },
];

fn compare(
    format: FormatId,
    base: &[u8],
    target: &[u8],
) -> (WorkerDiffResponse, ComparisonBudget, u128) {
    let request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format,
        base_raw_sha256: Sha256::digest(base).into(),
        base_size_bytes: base.len() as u64,
        target_raw_sha256: Sha256::digest(target).into(),
        target_size_bytes: target.len() as u64,
    };
    let mut budget = ComparisonBudget::new(8_000_000, 100_000);
    let started = Instant::now();
    let result = match format {
        FormatId::Txt => TextComparator::compare(&request, base, target, &mut budget),
        FormatId::Csv => CsvComparator::compare(&request, base, target, &mut budget),
        FormatId::Html => HtmlComparator::compare(&request, base, target, &mut budget),
        FormatId::Docx => DocxComparator::compare(&request, base, target, &mut budget),
        FormatId::Xlsx => SpreadsheetComparator::xlsx(&request, base, target, &mut budget),
        FormatId::Xlsm => SpreadsheetComparator::xlsm(&request, base, target, &mut budget),
        FormatId::Pptx => PptxComparator::compare(&request, base, target, &mut budget),
        FormatId::Pdf => PdfComparator::compare(&request, base, target, &mut budget),
    }
    .unwrap();
    let elapsed = started.elapsed().as_millis();
    result.validate_against(&request).unwrap();
    (result, budget, elapsed)
}

#[test]
fn eight_qualified_formats_have_no_false_unchanged_or_false_change() {
    let mut false_unchanged = 0;
    let mut false_change = 0;
    let mut locator_errors = 0;
    for case in CASES {
        let (same, _, _) = compare(case.format, case.base, case.base);
        assert_eq!(same.coverage, DiffCoverage::Full, "{:?} exact", case.format);
        assert!(same.changes.is_empty(), "{:?} exact", case.format);

        let (changed, budget, elapsed) = compare(case.format, case.base, case.changed);
        false_unchanged += usize::from(changed.changes.is_empty());
        locator_errors += changed
            .changes
            .iter()
            .filter(|change| {
                change
                    .base
                    .as_ref()
                    .is_some_and(|locator| locator.validate().is_err())
                    || change
                        .target
                        .as_ref()
                        .is_some_and(|locator| locator.validate().is_err())
            })
            .count();
        assert!(!changed.changes.is_empty(), "{:?} changed", case.format);
        assert_ne!(
            changed.coverage,
            DiffCoverage::None,
            "{:?} changed",
            case.format
        );

        let (noise, _, _) = compare(case.format, case.base, case.equal_noise);
        false_change += noise.changes.len();
        assert_eq!(
            noise.coverage,
            DiffCoverage::Full,
            "{:?} noise",
            case.format
        );
        assert!(noise.changes.is_empty(), "{:?} noise", case.format);

        let (unknown, _, _) = compare(case.format, case.base, case.unverified);
        assert_ne!(
            unknown.coverage,
            DiffCoverage::Full,
            "{:?} unknown",
            case.format
        );
        assert!(
            !unknown.unverified_regions.is_empty(),
            "{:?} unknown",
            case.format
        );
        println!(
            "format={:?} changed_ms={elapsed} candidates={} changes={} coverage={:?}",
            case.format,
            budget.candidates_used(),
            budget.changes_used(),
            changed.coverage
        );
    }
    assert_eq!((false_unchanged, false_change, locator_errors), (0, 0, 0));
}

#[test]
fn finite_candidate_and_change_limits_accept_boundary_and_reject_one_more() {
    let mut budget = ComparisonBudget::new(8_000_000, 100_000);
    budget.consume_candidates(8_000_000).unwrap();
    assert!(budget.consume_candidates(1).is_err());
    assert_eq!(budget.candidates_used(), 8_000_000);
    budget.consume_changes(100_000).unwrap();
    assert!(budget.consume_changes(1).is_err());
    assert_eq!(budget.changes_used(), 100_000);

    let mut request = WorkerDiffRequest {
        protocol_version: WorkerProtocolVersion::V0,
        diff_profile_version: DiffProfileVersion::V0,
        resource_profile_version: ResourceProfileVersion::V0,
        format: FormatId::Txt,
        base_raw_sha256: [1; 32],
        base_size_bytes: 256 * 1024 * 1024,
        target_raw_sha256: [2; 32],
        target_size_bytes: 256 * 1024 * 1024,
    };
    request.validate().unwrap();
    request.base_size_bytes += 1;
    assert!(request.validate().is_err());
}

#[test]
fn large_native_text_at_line_boundary_is_bounded_and_one_more_is_unverified() {
    let at_limit = "a\n".repeat(100_000);
    let (same, budget, elapsed) = compare(FormatId::Txt, at_limit.as_bytes(), at_limit.as_bytes());
    assert_eq!(same.coverage, DiffCoverage::Full);
    assert_eq!(budget.candidates_used(), 100_000);
    let over_limit = "a\n".repeat(100_001);
    let (unknown, _, _) = compare(FormatId::Txt, at_limit.as_bytes(), over_limit.as_bytes());
    assert_eq!(unknown.coverage, DiffCoverage::None);
    assert!(unknown.changes.is_empty());
    assert_eq!(
        unknown.unverified_regions[0].reason,
        UnverifiedReason::ResourceLimit
    );
    assert_eq!(
        unknown.unverified_regions[0].base,
        Some(SourceLocator::ContentItem)
    );
    println!(
        "large_txt_lines=100000 elapsed_ms={elapsed} candidates={}",
        budget.candidates_used()
    );
}
