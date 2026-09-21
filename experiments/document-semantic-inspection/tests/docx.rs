mod support;

use std::io::Cursor;
use std::process::Command;

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, FixtureCase, FixtureManifest, InspectionAdapter, InspectionResult,
    run_case,
};
use support::ooxml::{
    add_archive_bomb, add_duplicate_entry, add_relationship_cycle, add_traversal_entry,
    add_traversal_relationship, add_unknown_relationship, docx_fixture, format_only_tracked_change,
    image_ancillary_noise, margin_only_noise, move_markup_same_final_text,
    mutate_zip_entry_order, xml_serialization_noise,
};
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


fn normalized_result(result: &InspectionResult) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "semantic_fingerprint": hex::encode(result.semantic_fingerprint),
        "semantic_projection": hex::encode(&result.output.semantic_projection),
        "capabilities": &result.output.capabilities,
        "editorial": &result.output.editorial,
        "external_dependencies": &result.output.external_dependencies,
        "signatures": &result.output.signatures,
        "diagnostics": &result.output.diagnostics,
    }))
    .expect("normalized inspection JSON")
}

#[test]
fn xml_serialization_and_margin_noise_are_invariant() {
    let base = docx_fixture("fixture text");
    let profile = document_semantic_inspection_poc::InspectionProfile::default();
    let expected = DocxAdapter.inspect(&base, &profile).expect("base");
    for mutated in [
        xml_serialization_noise(&base),
        margin_only_noise(&base),
    ] {
        let actual = DocxAdapter.inspect(&mutated, &profile).expect("noise");
        assert_eq!(expected.semantic_fingerprint, actual.semantic_fingerprint);
    }
}

#[test]
fn meaning_equivalent_png_ancillary_reencoding_is_invariant() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/docx/base.docx"),
    )
    .expect("base DOCX");
    let profile = document_semantic_inspection_poc::InspectionProfile::default();
    let base = DocxAdapter.inspect(&bytes, &profile).expect("base");
    let noisy = DocxAdapter
        .inspect(&image_ancillary_noise(&bytes), &profile)
        .expect("image ancillary noise");
    assert_eq!(base.semantic_fingerprint, noisy.semantic_fingerprint);
}

#[test]
fn editorial_provenance_preserves_change_and_comment_details() {
    let tracked = inspect("docx/tracked-replacement").expect("tracked");
    let tracked_json = serde_json::to_value(&tracked.output.editorial).expect("editorial JSON");
    let changes = tracked_json["tracked_changes"].as_array().expect("tracked_changes");
    assert!(!changes.is_empty());
    assert!(changes.iter().any(|change| change["kind"] == "insertion"));
    assert!(changes.iter().any(|change| change["kind"] == "deletion"));
    assert!(changes.iter().all(|change| change["unresolved"] == true));

    let unresolved = inspect("docx/comment-unresolved").expect("unresolved comment");
    let unresolved_json = serde_json::to_value(&unresolved.output.editorial).expect("editorial JSON");
    let comments = unresolved_json["comments"].as_array().expect("comments");
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["resolved_state"], "unresolved");
    assert!(comments[0]["content"].as_str().is_some_and(|value| !value.is_empty()));

    let resolved = inspect("docx/comment-resolved").expect("resolved comment");
    let resolved_json = serde_json::to_value(&resolved.output.editorial).expect("editorial JSON");
    assert_eq!(resolved_json["comments"][0]["resolved_state"], "resolved");

    let metadata = inspect("docx/metadata-noise").expect("metadata noise");
    let metadata_json = serde_json::to_value(&metadata.output.editorial).expect("editorial JSON");
    assert!(metadata_json["last_modified_by"].as_str().is_some());
}

#[test]
fn format_only_and_move_tracking_are_editorial_but_not_semantic() {
    let base = docx_fixture("fixture text");
    let profile = document_semantic_inspection_poc::InspectionProfile::default();
    let expected = DocxAdapter.inspect(&base, &profile).expect("base");

    let format_only = DocxAdapter
        .inspect(&format_only_tracked_change(&base), &profile)
        .expect("format-only change");
    assert_eq!(expected.semantic_fingerprint, format_only.semantic_fingerprint);
    let format_json = serde_json::to_value(&format_only.output.editorial).expect("editorial JSON");
    assert!(format_json["tracked_changes"]
        .as_array()
        .expect("tracked changes")
        .iter()
        .any(|change| change["kind"] == "format"));

    let moved = DocxAdapter
        .inspect(&move_markup_same_final_text(&base), &profile)
        .expect("move markup");
    assert_eq!(expected.semantic_fingerprint, moved.semantic_fingerprint);
    let move_json = serde_json::to_value(&moved.output.editorial).expect("editorial JSON");
    let changes = move_json["tracked_changes"].as_array().expect("tracked changes");
    assert!(changes.iter().any(|change| change["kind"] == "move_from"));
    assert!(changes.iter().any(|change| change["kind"] == "move_to"));
}

#[test]
fn relationship_cycles_traversal_archive_bombs_and_duplicates_fail_closed() {
    let base = docx_fixture("fixture text");
    let profile = document_semantic_inspection_poc::InspectionProfile::default();

    for bytes in [add_relationship_cycle(&base), add_traversal_relationship(&base)] {
        let error = DocxAdapter.inspect(&bytes, &profile).unwrap_err();
        assert_eq!(error.code(), ErrorCode::SemanticExtractionFailed);
    }

    let traversal = DocxAdapter
        .inspect(&add_traversal_entry(&base), &profile)
        .unwrap_err();
    assert_eq!(traversal.code(), ErrorCode::SemanticExtractionFailed);

    let bomb = DocxAdapter.inspect(&add_archive_bomb(&base), &profile).unwrap_err();
    assert_eq!(bomb.code(), ErrorCode::InspectionResourceLimitExceeded);

    let duplicate = DocxAdapter
        .inspect(&add_duplicate_entry(&base), &profile)
        .unwrap_err();
    assert_eq!(duplicate.code(), ErrorCode::SemanticExtractionFailed);
}

#[test]
fn docx_results_are_deterministic_in_process_and_across_fresh_processes() {
    use document_semantic_inspection_poc::ExpectedOutcome;

    let success_cases: Vec<_> = manifest()
        .cases
        .into_iter()
        .filter(|case| case.format == document_semantic_inspection_poc::FormatId::Docx)
        .filter(|case| !matches!(case.expected, ExpectedOutcome::Error { .. }))
        .collect();

    for case in &success_cases {
        let first = run_case(case, &DocxAdapter).expect("first run");
        let expected = normalized_result(&first);
        for _ in 1..20 {
            let actual = run_case(case, &DocxAdapter).expect("repeat run");
            assert_eq!(expected, normalized_result(&actual), "{}", case.id);
        }
    }

    let exe = env!("CARGO_BIN_EXE_dsi-poc");
    let mut baseline: Option<Vec<u8>> = None;
    for _ in 0..5 {
        let output = Command::new(exe)
            .args(["snapshot", "docx"])
            .output()
            .expect("fresh dsi-poc process");
        assert!(
            output.status.success(),
            "snapshot failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if let Some(expected) = &baseline {
            assert_eq!(expected, &output.stdout);
        } else {
            baseline = Some(output.stdout);
        }
    }
}
