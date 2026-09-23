mod support;

use document_semantic_inspection_poc::{
    ErrorCode, FixtureCase, FixtureManifest, FormatId, PptxAdapter, run_case,
};

fn manifest() -> FixtureManifest {
    FixtureManifest::from_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("manifest.json"),
    )
    .expect("manifest")
}

fn case(id: &str) -> FixtureCase {
    manifest()
        .cases
        .into_iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn inspect(id: &str) -> document_semantic_inspection_poc::InspectionResult {
    run_case(&case(id), &PptxAdapter).unwrap_or_else(|error| panic!("{id}: {error}"))
}

fn fingerprint(id: &str) -> [u8; 32] {
    inspect(id).semantic_fingerprint
}

fn same(id: &str) {
    assert_eq!(fingerprint("pptx/base"), fingerprint(id), "{id}");
}

fn different(id: &str) {
    assert_ne!(fingerprint("pptx/base"), fingerprint(id), "{id}");
}

#[test]
fn raw_presentationml_fixtures_pin_independent_golden_semantics() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/pptx/base.pptx"),
    )
    .expect("PPTX base");

    let names = support::presentationml::entry_names(&bytes);
    for required in [
        "ppt/presentation.xml",
        "ppt/slides/slide1.xml",
        "ppt/slides/slide2.xml",
        "ppt/charts/chart1.xml",
        "ppt/diagrams/data1.xml",
        "ppt/notesSlides/notesSlide1.xml",
        "ppt/media/image1.png",
    ] {
        assert!(names.iter().any(|name| name == required), "missing {required}");
    }

    let chart = support::presentationml::part_text(&bytes, "ppt/charts/chart1.xml");
    assert!(chart.contains("Series A"));
    assert!(chart.contains("<c:v>10</c:v>"));

    let smartart = support::presentationml::part_text(&bytes, "ppt/diagrams/data1.xml");
    assert!(smartart.contains("Node A"));

    let notes = support::presentationml::part_text(&bytes, "ppt/notesSlides/notesSlide1.xml");
    assert!(notes.contains("Speaker note"));
}

#[test]
fn slide_and_shape_structure_changes_are_version_significant() {
    for id in [
        "pptx/slide-add",
        "pptx/slide-remove",
        "pptx/slide-order-change",
        "pptx/shape-association-change",
        "pptx/group-change",
    ] {
        different(id);
    }
}

#[test]
fn embedded_semantic_construct_changes_are_version_significant() {
    for id in [
        "pptx/table-change",
        "pptx/chart-change",
        "pptx/smartart-change",
        "pptx/image-change",
        "pptx/hyperlink-change",
        "pptx/speaker-note-change",
    ] {
        different(id);
    }
}

#[test]
fn presentation_formatting_and_internal_identity_noise_is_invariant() {
    for id in [
        "pptx/theme-only",
        "pptx/font-only",
        "pptx/background-only",
        "pptx/id-order-noise",
    ] {
        same(id);
    }
}

#[test]
fn unknown_presentation_semantics_fail_closed() {
    let error = run_case(&case("pptx/unknown-semantic-part"), &PptxAdapter).unwrap_err();
    assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct);
}

#[test]
fn adapter_reports_pptx_format() {
    use document_semantic_inspection_poc::InspectionAdapter;
    assert_eq!(PptxAdapter.format(), FormatId::Pptx);
}
