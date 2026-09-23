mod support;

use document_semantic_inspection_poc::{fingerprint as semantic_fingerprint, ErrorCode, FixtureCase, FixtureManifest, InspectionAdapter, InspectionProfile, SpreadsheetAdapter, VbaAdapter, run_case};
use support::spreadsheetml::mutate_vba_module;

fn manifest() -> FixtureManifest {
    FixtureManifest::from_path(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/manifest.json")).expect("manifest")
}
fn case(id: &str) -> FixtureCase { manifest().cases.into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("missing {id}")) }
fn inspect_xlsx(id: &str) -> document_semantic_inspection_poc::InspectionResult {
    run_case(&case(id), &SpreadsheetAdapter::XLSX).unwrap_or_else(|e| panic!("{id}: {e}"))
}
fn fp(id: &str) -> [u8;32] { inspect_xlsx(id).semantic_fingerprint }
fn same(id:&str){assert_eq!(fp("xlsx/base"),fp(id),"{id}")}
fn diff(id:&str){assert_ne!(fp("xlsx/base"),fp(id),"{id}")}
fn diff_from(base:&str,id:&str){assert_ne!(fp(base),fp(id),"{base} vs {id}")}

#[test]
fn spreadsheet_semantic_and_noise_relations_are_strict() {
    for id in ["xlsx/formula-source-change-same-cache","xlsx/very-hidden-change","xlsx/cell-value-change","xlsx/defined-name-change","xlsx/merged-change","xlsx/hyperlink-change"] { diff(id); }
    for id in ["xlsx/cached-result-only","xlsx/style-only","xlsx/xml-order-noise"] { same(id); }
}
#[test]
fn unknown_spreadsheet_package_semantics_fail_closed() {
    let e=run_case(&case("xlsx/unknown-semantic-part"), &SpreadsheetAdapter::XLSX).unwrap_err();
    assert_eq!(e.code(),ErrorCode::UnsupportedSemanticConstruct);
}
#[test]
fn xlsm_seed_extracts_vba_without_execution() {
    let r=run_case(&case("xlsm/base"), &SpreadsheetAdapter::XLSM).expect("xlsm");
    assert!(r.output.capabilities.iter().any(|c| c.capability=="vba_logic" && c.present));
}
#[test]
fn strict_vba_parser_canonicalizes_trivia_and_rejects_recovery() {
    let base="Option Explicit\nSub Main()\nDim X As Integer\nX = 1 + 2\nEnd Sub\n";
    let trivia="option explicit\r\nsub main()\r\n 'comment\r\n dim x as integer\r\n x=1+2\r\nend sub\r\n";
    let changed="Option Explicit\nSub Main()\nDim X As Integer\nX = 1 + 3\nEnd Sub\n";
    assert_eq!(VbaAdapter::canonicalize_source(base).expect("base"),VbaAdapter::canonicalize_source(trivia).expect("trivia"));
    assert_ne!(VbaAdapter::canonicalize_source(base).expect("base"),VbaAdapter::canonicalize_source(changed).expect("changed"));
    let e=VbaAdapter::canonicalize_source("Sub Main(\n").unwrap_err();
    assert_eq!(e.code(),ErrorCode::SemanticExtractionFailed);
}

#[test]
fn spreadsheet_required_structure_surface_is_version_significant() {
    diff("xlsx/sheet-add");
    diff_from("xlsx/two-sheet-base", "xlsx/sheet-order-change");
    for id in ["xlsx/table-add", "xlsx/chart-add", "xlsx/image-add"] {
        diff(id);
    }
}

#[test]
fn external_workbook_definition_is_semantic_but_never_dereferenced() {
    diff("xlsx/external-reference-add");
    let result = inspect_xlsx("xlsx/external-reference-add");
    assert!(
        result.output.external_dependencies.iter().any(|dependency| {
            dependency.kind == "external_workbook"
                && dependency.definition.contains("external-book.xlsx")
        }),
        "external workbook target must be preserved as a definition"
    );
}

const VBA_BASE_SOURCE: &str = "Attribute VB_Name = \"testVBA\"\r\nPublic Sub test()\r\n    MsgBox \"Hello from vba!\"\r\nEnd Sub\r\n";

fn inspect_xlsm_bytes(bytes: &[u8]) -> Result<document_semantic_inspection_poc::AdapterOutput, document_semantic_inspection_poc::PocError> {
    SpreadsheetAdapter::XLSM.inspect(bytes, &InspectionProfile::default())
}

#[test]
fn xlsm_vba_source_variants_are_qualified_on_real_macro_container() {
    let seed = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/xlsm/calamine-vba.xlsm"),
    )
    .expect("licensed XLSM seed");
    let base = inspect_xlsm_bytes(&seed).expect("base XLSM");
    let base_fp = semantic_fingerprint(&base.semantic_projection);

    let comment_only = mutate_vba_module(
        &seed,
        "testVBA",
        &VBA_BASE_SOURCE.replace(
            "Public Sub test()\r\n",
            "Public Sub test()\r\n    ' qualification comment\r\n",
        ),
    );
    let whitespace_only = mutate_vba_module(
        &seed,
        "testVBA",
        "attribute vb_name = \"testVBA\"\r\npublic sub test()\r\n\tmsgbox   \"Hello from vba!\"\r\nend sub\r\n",
    );
    let logic_change = mutate_vba_module(
        &seed,
        "testVBA",
        &VBA_BASE_SOURCE.replace("Hello from vba!", "Changed logic"),
    );
    let invalid_syntax = mutate_vba_module(
        &seed,
        "testVBA",
        &VBA_BASE_SOURCE.replace("Public Sub test()", "Public Sub test("),
    );

    for same in [&comment_only, &whitespace_only] {
        let inspected = inspect_xlsm_bytes(same).expect("trivia-only VBA variant");
        assert_eq!(base_fp, semantic_fingerprint(&inspected.semantic_projection));
    }

    let changed = inspect_xlsm_bytes(&logic_change).expect("logic-changing VBA variant");
    assert_ne!(base_fp, semantic_fingerprint(&changed.semantic_projection));

    let error = inspect_xlsm_bytes(&invalid_syntax).unwrap_err();
    assert_eq!(error.code(), ErrorCode::SemanticExtractionFailed);
}
