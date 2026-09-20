mod support;

use std::io::Cursor;

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, FixtureCase, FixtureManifest, InspectionAdapter, InspectionResult,
    run_case,
};
use support::ooxml::{add_unknown_relationship, docx_fixture, mutate_zip_entry_order};
use zip::ZipArchive;

fn manifest() -> FixtureManifest {
    FixtureManifest::from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("manifest.json"),
    )
    .expect("fixture manifest")
}

fn case(id: &str) -> FixtureCase {
    manifest()
        .cases
        .into_iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn inspect(id: &str) -> Result<InspectionResult, document_semantic_inspection_poc::PocError> {
    run_case(&case(id), &DocxAdapter)
}

fn fingerprint(id: &str) -> [u8; 32] {
    inspect(id)
        .unwrap_or_else(|error| panic!("{id}: {error}"))
        .semantic_fingerprint
}

fn assert_same(id: &str) {
    assert_eq!(fingerprint("docx/base"), fingerprint(id), "{id}");
}

fn assert_different(id: &str) {
    assert_ne!(fingerprint("docx/base"), fingerprint(id), "{id}");
}

fn assert_error(id: &str, code: ErrorCode) {
    let error = inspect(id).unwrap_err();
    assert_eq!(error.code(), code, "{id}: {error}");
}

#[test]
fn independent_raw_ooxml_fixture_builder_is_candidate_free() {
    let bytes = docx_fixture("fixture text");
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("raw builder ZIP");
    assert!(archive.by_name("[Content_Types].xml").is_ok());
    assert!(archive.by_name("word/document.xml").is_ok());
    assert_ne!(bytes, mutate_zip_entry_order(&bytes));

    let unknown = add_unknown_relationship(&bytes, "urn:example:relationships/semantic");
    let mut archive = ZipArchive::new(Cursor::new(&unknown)).expect("unknown ZIP");
    assert!(archive.by_name("word/semantic.xml").is_ok());
}

#[test]
fn noise_is_semantically_invariant() {
    for id in [
        "docx/metadata-noise",
        "docx/font-only",
        "docx/relationship-id-noise",
        "docx/package-order-noise",
    ] {
        assert_same(id);
    }
}

#[test]
fn version_significant_changes_change_fingerprint() {
    for id in [
        "docx/body-text-change",
        "docx/heading-list-change",
        "docx/table-merge-change",
        "docx/header-change",
        "docx/footer-change",
        "docx/footnote-change",
        "docx/endnote-change",
        "docx/hyperlink-target-change",
        "docx/image-content-change",
        "docx/section-change",
    ] {
        assert_different(id);
    }
}

#[test]
fn tracked_changes_use_proposed_final_projection_and_preserve_editorial_evidence() {
    assert_same("docx/tracked-replacement");
    let result = inspect("docx/tracked-replacement").expect("tracked fixture");
    assert!(result.output.editorial.tracked_changes_present);

    let projection: serde_json::Value =
        serde_json::from_slice(&result.output.semantic_projection).expect("projection JSON");
    let text = projection["plain_text"].as_str().expect("plain_text");
    assert!(text.contains("new text"));
    assert!(!text.contains("old text"));
}

#[test]
fn comments_are_editorial_not_version_identity() {
    for id in ["docx/comment-unresolved", "docx/comment-resolved"] {
        assert_same(id);
        assert!(inspect(id).expect("comment fixture").output.editorial.comments_present);
    }
}

#[test]
fn hostile_and_unknown_semantics_fail_closed() {
    assert_error(
        "docx/unknown-semantic-part",
        ErrorCode::UnsupportedSemanticConstruct,
    );
    assert_error("docx/malformed", ErrorCode::SemanticExtractionFailed);
    assert_error(
        "docx/deep-ooxml",
        ErrorCode::InspectionResourceLimitExceeded,
    );
}

#[test]
fn adapter_reports_docx_format() {
    use document_semantic_inspection_poc::FormatId;
    assert_eq!(DocxAdapter.format(), FormatId::Docx);
}
