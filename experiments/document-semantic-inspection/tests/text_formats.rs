use document_semantic_inspection_poc::{
    CsvAdapter, ErrorCode, FixtureCase, FixtureManifest, HtmlAdapter, InspectionAdapter, TextAdapter,
    run_case,
};

fn manifest() -> FixtureManifest {
    FixtureManifest::from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("manifest.json"),
    )
    .expect("fixture manifest must be valid")
}

fn case(id: &str) -> FixtureCase {
    manifest()
        .cases
        .into_iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("missing fixture {id}"))
}

fn fingerprint(id: &str, adapter: &dyn InspectionAdapter) -> [u8; 32] {
    run_case(&case(id), adapter)
        .unwrap_or_else(|error| panic!("{id} failed: {error}"))
        .semantic_fingerprint
}

fn assert_same(left: &str, right: &str, adapter: &dyn InspectionAdapter) {
    assert_eq!(fingerprint(left, adapter), fingerprint(right, adapter));
}

fn assert_different(left: &str, right: &str, adapter: &dyn InspectionAdapter) {
    assert_ne!(fingerprint(left, adapter), fingerprint(right, adapter));
}

fn assert_error(id: &str, expected: ErrorCode, adapter: &dyn InspectionAdapter) {
    let error = run_case(&case(id), adapter).unwrap_err();
    assert_eq!(error.code(), expected, "{id}: {error}");
}

#[test]
fn txt_noise_is_invariant_and_text_change_is_semantic() {
    assert_same("txt/base", "txt/crlf", &TextAdapter);
    assert_same("txt/base", "txt/unicode-nfd", &TextAdapter);
    assert_different("txt/base", "txt/text-change", &TextAdapter);
    assert_error(
        "txt/ambiguous-decode",
        ErrorCode::SemanticExtractionFailed,
        &TextAdapter,
    );
}

#[test]
fn csv_is_tabular_and_fails_closed_on_structure_or_delimiter() {
    assert_same("csv/base", "csv/quote-noise", &CsvAdapter);
    assert_different("csv/base", "csv/cell-change", &CsvAdapter);
    assert_different("csv/base", "csv/row-change", &CsvAdapter);
    assert_error(
        "csv/inconsistent",
        ErrorCode::SemanticExtractionFailed,
        &CsvAdapter,
    );
    assert_error(
        "csv/delimiter-ambiguous",
        ErrorCode::UnsupportedSemanticConstruct,
        &CsvAdapter,
    );
}

#[test]
fn html_ignores_decoration_but_preserves_visible_structure_and_targets() {
    assert_same("html/base", "html/noise", &HtmlAdapter);
    assert_different("html/base", "html/text-change", &HtmlAdapter);
    assert_different("html/base", "html/link-change", &HtmlAdapter);
    assert_different("html/base", "html/image-change", &HtmlAdapter);
    assert_error(
        "html/js-only",
        ErrorCode::UnsupportedSemanticConstruct,
        &HtmlAdapter,
    );
}

#[test]
fn adapters_report_their_formats() {
    use document_semantic_inspection_poc::FormatId;
    assert_eq!(TextAdapter.format(), FormatId::Txt);
    assert_eq!(CsvAdapter.format(), FormatId::Csv);
    assert_eq!(HtmlAdapter.format(), FormatId::Html);
}
