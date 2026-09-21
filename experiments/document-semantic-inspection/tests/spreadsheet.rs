mod support;

use document_semantic_inspection_poc::{ErrorCode, FixtureCase, FixtureManifest, SpreadsheetAdapter, VbaAdapter, run_case};

fn manifest() -> FixtureManifest {
    FixtureManifest::from_path(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/manifest.json")).expect("manifest")
}
fn case(id: &str) -> FixtureCase { manifest().cases.into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("missing {id}")) }
fn fp(id: &str) -> [u8;32] { run_case(&case(id), &SpreadsheetAdapter).unwrap_or_else(|e| panic!("{id}: {e}")).semantic_fingerprint }
fn same(id:&str){assert_eq!(fp("xlsx/base"),fp(id),"{id}")} fn diff(id:&str){assert_ne!(fp("xlsx/base"),fp(id),"{id}")}

#[test]
fn spreadsheet_semantic_and_noise_relations_are_strict() {
    for id in ["xlsx/formula-source-change-same-cache","xlsx/very-hidden-change","xlsx/cell-value-change","xlsx/defined-name-change","xlsx/merged-change","xlsx/hyperlink-change"] { diff(id); }
    for id in ["xlsx/cached-result-only","xlsx/style-only","xlsx/xml-order-noise"] { same(id); }
}
#[test]
fn unknown_spreadsheet_package_semantics_fail_closed() {
    let e=run_case(&case("xlsx/unknown-semantic-part"),&SpreadsheetAdapter).unwrap_err();
    assert_eq!(e.code(),ErrorCode::UnsupportedSemanticConstruct);
}
#[test]
fn xlsm_seed_extracts_vba_without_execution() {
    let r=run_case(&case("xlsm/base"),&SpreadsheetAdapter).expect("xlsm");
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
