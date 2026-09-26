use std::io::{Cursor, Read, Write};

use document_semantic_inspection_core::{FormatId, SemanticFingerprint};
use document_semantic_inspection_worker::{
    AdapterProfile, SemanticAdapter, SemanticAdapterOutput, SpreadsheetAdapter, WorkerFailure,
    WorkerFailureCode,
};
use rxls::{StyleLossKind, Workbook};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const XLSX_BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const XLSX_FORMULA_SOURCE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/formula-source-change-same-cache.xlsx"
);
const XLSX_CACHED_RESULT_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/cached-result-only.xlsx"
);
const XLSX_CELL_VALUE_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/cell-value-change.xlsx"
);
const XLSX_DEFINED_NAME_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/defined-name-change.xlsx"
);
const XLSX_VERY_HIDDEN_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/very-hidden-change.xlsx"
);
const XLSX_TWO_SHEET_BASE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/sheet-add.xlsx"
);
const XLSX_SHEET_REMOVE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/xlsx/base.xlsx");
const XLSX_SHEET_ORDER_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/sheet-order-change.xlsx"
);
const XLSX_MERGED_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/merged-change.xlsx"
);
const XLSX_TABLE_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/table-add.xlsx"
);
const XLSX_HYPERLINK_CHANGE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/hyperlink-change.xlsx"
);
const XLSX_CHART_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/chart-add.xlsx"
);
const XLSX_IMAGE_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/image-add.xlsx"
);
const XLSX_EXTERNAL_REFERENCE_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/external-reference-add.xlsx"
);
const XLSX_ODBC_CONNECTION_ADD: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/odbc-connection-add.xlsx"
);
const XLSX_STYLE_ONLY: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/style-only.xlsx"
);
const XLSX_XML_ORDER_NOISE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsx/xml-order-noise.xlsx"
);
const XLSM_BASE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/xlsm/calamine-vba.xlsm"
);

const WORKSHEET_PATH: &str = "xl/worksheets/sheet1.xml";
const WORKSHEET_RELS_PATH: &str = "xl/worksheets/_rels/sheet1.xml.rels";
const WORKBOOK_PATH: &str = "xl/workbook.xml";
const CHART_PATH: &str = "xl/charts/chart1.xml";
const IMAGE_PATH: &str = "xl/media/image1.png";
const DRAWING_PATH: &str = "xl/drawings/drawing1.xml";
const BASE_A1_CELL: &str = r#"<c r="A1" t="inlineStr"><is><t>Hello</t></is></c>"#;

fn inspect_xlsx(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    SpreadsheetAdapter::XLSX.inspect(bytes, &AdapterProfile::default())
}

fn inspect_xlsm(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    SpreadsheetAdapter::XLSM.inspect(bytes, &AdapterProfile::default())
}

fn xlsx_fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    inspect_xlsx(bytes)
        .expect("qualified XLSX fixture should inspect")
        .semantic_fingerprint()
}

fn xlsm_fingerprint(bytes: &[u8]) -> SemanticFingerprint {
    inspect_xlsm(bytes)
        .expect("qualified XLSM fixture should inspect")
        .semantic_fingerprint()
}

fn replace_a1_cell(input: &[u8], replacement: &str) -> Vec<u8> {
    let worksheet = read_package_entry(input, WORKSHEET_PATH);
    let worksheet = String::from_utf8(worksheet).expect("worksheet XML should be UTF-8");
    let updated = worksheet.replace(BASE_A1_CELL, replacement);
    assert_ne!(updated, worksheet, "base A1 cell marker must be present");
    replace_package_entries(input, &[(WORKSHEET_PATH, updated.as_bytes())])
}

fn insert_xlsm_a1_cell(input: &[u8], cell: &str) -> Vec<u8> {
    let worksheet = read_package_entry(input, WORKSHEET_PATH);
    let worksheet = String::from_utf8(worksheet).expect("worksheet XML should be UTF-8");
    let updated = worksheet.replace(
        "<sheetData/>",
        &format!("<sheetData><row r=\"1\">{cell}</row></sheetData>"),
    );
    assert_ne!(updated, worksheet, "empty sheetData marker must be present");
    replace_package_entries(input, &[(WORKSHEET_PATH, updated.as_bytes())])
}

fn with_a1_default_style(input: &[u8]) -> Vec<u8> {
    let worksheet = read_package_entry(input, WORKSHEET_PATH);
    let worksheet = String::from_utf8(worksheet).expect("worksheet XML should be UTF-8");
    let updated = worksheet.replace(
        r#"<c r="A1" t="inlineStr">"#,
        r#"<c r="A1" s="0" t="inlineStr">"#,
    );
    assert_ne!(updated, worksheet, "base A1 cell marker must be present");
    replace_package_entries(input, &[(WORKSHEET_PATH, updated.as_bytes())])
}

fn with_sheet_state(input: &[u8], state: &str) -> Vec<u8> {
    let workbook = read_package_entry(input, WORKBOOK_PATH);
    let workbook = String::from_utf8(workbook).expect("workbook XML should be UTF-8");
    let updated = workbook.replace(r#"state="visible""#, &format!(r#"state="{state}""#));
    assert_ne!(
        updated, workbook,
        "visible worksheet marker must be present"
    );
    replace_package_entries(input, &[(WORKBOOK_PATH, updated.as_bytes())])
}

fn reorder_workbook_sheet_attributes(input: &[u8]) -> Vec<u8> {
    let workbook = read_package_entry(input, WORKBOOK_PATH);
    let workbook = String::from_utf8(workbook).expect("workbook XML should be UTF-8");
    let updated = workbook.replace(
        r#"<sheet name="Sheet1" sheetId="1" state="visible" r:id="rId1"/>"#,
        r#"<sheet r:id="rId1" state="visible" sheetId="1" name="Sheet1"/>"#,
    );
    assert_ne!(updated, workbook, "base worksheet element must be present");
    replace_package_entries(input, &[(WORKBOOK_PATH, updated.as_bytes())])
}

fn change_chart_title(input: &[u8], title: &str) -> Vec<u8> {
    let chart = read_package_entry(input, CHART_PATH);
    let chart = String::from_utf8(chart).expect("chart XML should be UTF-8");
    assert_eq!(
        chart.matches("Example Chart").count(),
        1,
        "chart title should have one scoped replacement target"
    );
    let updated = chart.replace("Example Chart", title);
    assert_ne!(updated, chart, "base chart title must be present");
    replace_package_entries(input, &[(CHART_PATH, updated.as_bytes())])
}

fn assert_only_package_entry_changed(input: &[u8], changed: &[u8], expected_path: &str) {
    let mut before = ZipArchive::new(Cursor::new(input)).expect("valid spreadsheet ZIP");
    let mut after = ZipArchive::new(Cursor::new(changed)).expect("valid spreadsheet ZIP");
    assert_eq!(
        before.len(),
        after.len(),
        "package entry count must be stable"
    );

    for index in 0..before.len() {
        let before_name = before
            .by_index(index)
            .expect("read original package entry")
            .name()
            .to_owned();
        let after_name = after
            .by_index(index)
            .expect("read changed package entry")
            .name()
            .to_owned();
        assert_eq!(
            before_name, after_name,
            "package entry order must be stable"
        );
        let before_contents = read_package_entry(input, &before_name);
        let after_contents = read_package_entry(changed, &after_name);
        if before_name == expected_path {
            assert_ne!(
                before_contents, after_contents,
                "target package entry must change"
            );
        } else {
            assert_eq!(
                before_contents, after_contents,
                "unrelated package entry {before_name} must remain byte-identical"
            );
        }
    }
}

fn assert_rgb8_one_pixel_png(bytes: &[u8]) {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let reader = decoder.read_info().expect("valid one-pixel PNG fixture");
    let info = reader.info();
    assert_eq!(info.width, 1, "PNG width must remain one pixel");
    assert_eq!(info.height, 1, "PNG height must remain one pixel");
    assert_eq!(
        info.color_type,
        png::ColorType::Rgb,
        "PNG color type must remain RGB"
    );
    assert_eq!(
        info.bit_depth,
        png::BitDepth::Eight,
        "PNG bit depth must remain eight"
    );
}

fn change_image_pixel(input: &[u8], pixel: [u8; 3]) -> Vec<u8> {
    assert_rgb8_one_pixel_png(&read_package_entry(input, IMAGE_PATH));

    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("write one-pixel PNG header");
        writer
            .write_image_data(&pixel)
            .expect("write one-pixel PNG data");
    }
    assert_rgb8_one_pixel_png(&encoded);
    replace_package_entries(input, &[(IMAGE_PATH, &encoded)])
}

fn internal_hyperlink_variant() -> Vec<u8> {
    let worksheet = read_package_entry(XLSX_BASE, WORKSHEET_PATH);
    let worksheet = String::from_utf8(worksheet).expect("worksheet XML should be UTF-8");
    let updated = worksheet.replace(
        r#"<hyperlink ref="A1" r:id="rIdHyper"/>"#,
        r#"<hyperlink ref="A1" location="'Sheet1'!B1" display="Open cell"/>"#,
    );
    assert_ne!(updated, worksheet, "base hyperlink marker must be present");

    let empty_relationships =
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;
    replace_package_entries(
        XLSX_BASE,
        &[
            (WORKSHEET_PATH, updated.as_bytes()),
            (WORKSHEET_RELS_PATH, empty_relationships),
        ],
    )
}

fn read_package_entry(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("valid spreadsheet ZIP");
    let mut entry = archive.by_name(name).expect("required spreadsheet part");
    let mut contents = Vec::new();
    entry
        .read_to_end(&mut contents)
        .expect("read spreadsheet part");
    contents
}

fn replace_package_entries(input: &[u8], replacements: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("valid spreadsheet ZIP");
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read spreadsheet ZIP entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("read spreadsheet ZIP entry");
        if let Some((_, replacement)) = replacements.iter().find(|(path, _)| *path == name) {
            contents = replacement.to_vec();
        }
        entries.push((name, contents));
    }

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("write spreadsheet ZIP entry");
        writer
            .write_all(&contents)
            .expect("write spreadsheet ZIP contents");
    }
    writer
        .finish()
        .expect("finish spreadsheet ZIP")
        .into_inner()
}

#[test]
fn spreadsheet_adapter_formats_are_explicit_and_match_the_input_format() {
    assert_eq!(SpreadsheetAdapter::XLSX.format(), FormatId::Xlsx);
    assert_eq!(SpreadsheetAdapter::XLSM.format(), FormatId::Xlsm);

    assert_eq!(
        SpreadsheetAdapter::XLSX
            .inspect(XLSM_BASE, &AdapterProfile::default())
            .unwrap_err()
            .code(),
        WorkerFailureCode::FormatMismatch
    );
    assert_eq!(
        SpreadsheetAdapter::XLSM
            .inspect(XLSX_BASE, &AdapterProfile::default())
            .unwrap_err()
            .code(),
        WorkerFailureCode::FormatMismatch
    );
}

#[test]
fn xlsx_sheet_presence_order_and_visibility_are_semantic() {
    let one_sheet = xlsx_fingerprint(XLSX_BASE);
    let two_sheet = xlsx_fingerprint(XLSX_TWO_SHEET_BASE);
    assert_ne!(one_sheet, two_sheet, "adding a sheet changes identity");
    assert_ne!(
        two_sheet,
        xlsx_fingerprint(XLSX_SHEET_REMOVE),
        "removing a sheet changes identity"
    );
    assert_ne!(
        two_sheet,
        xlsx_fingerprint(XLSX_SHEET_ORDER_CHANGE),
        "reordering sheets changes identity"
    );
    assert_ne!(
        one_sheet,
        xlsx_fingerprint(XLSX_VERY_HIDDEN_CHANGE),
        "very-hidden sheet state changes identity"
    );
    assert_ne!(
        one_sheet,
        xlsx_fingerprint(&with_sheet_state(XLSX_BASE, "hidden")),
        "ordinary hidden sheet state changes identity"
    );
}

#[test]
fn xlsx_cell_values_and_types_formula_sources_and_defined_names_are_semantic() {
    let base = xlsx_fingerprint(XLSX_BASE);
    assert_ne!(base, xlsx_fingerprint(XLSX_CELL_VALUE_CHANGE));
    assert_ne!(base, xlsx_fingerprint(XLSX_FORMULA_SOURCE_CHANGE));
    assert_ne!(base, xlsx_fingerprint(XLSX_DEFINED_NAME_CHANGE));

    assert_eq!(base, xlsx_fingerprint(XLSX_CACHED_RESULT_ONLY));
    assert_eq!(base, xlsx_fingerprint(XLSX_STYLE_ONLY));
    assert_eq!(base, xlsx_fingerprint(XLSX_XML_ORDER_NOISE));

    let text_one = replace_a1_cell(
        XLSX_BASE,
        r#"<c r="A1" t="inlineStr"><is><t>1</t></is></c>"#,
    );
    let number_one = replace_a1_cell(XLSX_BASE, r#"<c r="A1" t="n"><v>1</v></c>"#);
    assert_ne!(
        xlsx_fingerprint(&text_one),
        xlsx_fingerprint(&number_one),
        "equal displayed values with different cell types remain distinct"
    );
}

#[test]
fn xlsx_applied_style_and_xml_attribute_order_are_noise() {
    let applied_style_base = with_a1_default_style(XLSX_BASE);
    let applied_style_change = with_a1_default_style(XLSX_STYLE_ONLY);
    assert_eq!(
        xlsx_fingerprint(&applied_style_base),
        xlsx_fingerprint(&applied_style_change),
        "font and size changes to the style applied to populated A1 must not change identity"
    );

    assert_eq!(
        xlsx_fingerprint(XLSX_BASE),
        xlsx_fingerprint(&reorder_workbook_sheet_attributes(XLSX_BASE)),
        "reordering workbook XML attributes must not change identity"
    );
}

#[test]
fn xlsx_merges_tables_links_charts_and_images_are_semantic() {
    let base = xlsx_fingerprint(XLSX_BASE);
    assert_ne!(base, xlsx_fingerprint(XLSX_MERGED_CHANGE));
    assert_ne!(base, xlsx_fingerprint(XLSX_TABLE_ADD));
    assert_ne!(base, xlsx_fingerprint(XLSX_HYPERLINK_CHANGE));
    assert_ne!(base, xlsx_fingerprint(XLSX_CHART_ADD));
    assert_ne!(base, xlsx_fingerprint(XLSX_IMAGE_ADD));
    let changed_chart = change_chart_title(XLSX_CHART_ADD, "Revenue Chart");
    assert_only_package_entry_changed(XLSX_CHART_ADD, &changed_chart, CHART_PATH);
    assert_ne!(
        xlsx_fingerprint(XLSX_CHART_ADD),
        xlsx_fingerprint(&changed_chart),
        "changing an existing chart title changes identity"
    );
    let changed_image = change_image_pixel(XLSX_IMAGE_ADD, [0, 255, 0]);
    assert_only_package_entry_changed(XLSX_IMAGE_ADD, &changed_image, IMAGE_PATH);
    assert_ne!(
        xlsx_fingerprint(XLSX_IMAGE_ADD),
        xlsx_fingerprint(&changed_image),
        "changing pixels in an existing image changes identity"
    );
}

#[test]
fn xlsx_external_workbook_and_odbc_definitions_are_semantic_without_loading_targets() {
    let base = xlsx_fingerprint(XLSX_BASE);

    // Neither referenced workbook nor database is included in the fixture package.
    assert_ne!(base, xlsx_fingerprint(XLSX_EXTERNAL_REFERENCE_ADD));
    assert_ne!(base, xlsx_fingerprint(XLSX_ODBC_CONNECTION_ADD));
}

#[test]
fn xlsx_parser_oracle_disagreement_fails_closed_without_document_text() {
    let _accepted_base = xlsx_fingerprint(XLSX_BASE);
    let conflicting_internal_link = internal_hyperlink_variant();
    let failure = inspect_xlsx(&conflicting_internal_link)
        .expect_err("conflicting parser interpretations must fail closed");

    assert_eq!(failure.code(), WorkerFailureCode::ParserDisagreement);
    assert!(
        !failure.message().contains("Hello"),
        "parser diagnostics must not disclose cell text"
    );
}

#[test]
fn xlsm_uses_workbook_cell_semantics_for_the_qualified_macro_enabled_fixture() {
    let base = xlsm_fingerprint(XLSM_BASE);
    assert_eq!(
        base,
        xlsm_fingerprint(&replace_package_entries(XLSM_BASE, &[])),
        "reserializing unchanged XLSM package parts preserves the accepted baseline identity"
    );

    let changed_cell = insert_xlsm_a1_cell(
        XLSM_BASE,
        r#"<c r="A1" t="inlineStr"><is><t>Workbook content</t></is></c>"#,
    );

    assert_ne!(base, xlsm_fingerprint(&changed_cell));
}

#[test]
fn partial_drawing_projection_fails_closed_instead_of_omitting_an_image() {
    let drawing = String::from_utf8(read_package_entry(XLSX_IMAGE_ADD, DRAWING_PATH))
        .expect("qualified drawing is UTF-8");
    assert_eq!(drawing.matches("<xdr:pic>").count(), 1);
    let nested = drawing
        .replace("<xdr:pic>", "<xdr:oneCellAnchor><xdr:pic>")
        .replace("</xdr:pic>", "</xdr:pic></xdr:oneCellAnchor>");
    let mutated = replace_package_entries(XLSX_IMAGE_ADD, &[(DRAWING_PATH, nested.as_bytes())]);

    let parsed = Workbook::open(&mutated).expect("rxls accepts the drawing package");
    assert!(parsed.sheets.iter().any(|sheet| {
        sheet
            .style_losses()
            .iter()
            .any(|loss| loss.kind == StyleLossKind::DrawingMetadataPartial)
    }));

    let failure = match inspect_xlsx(&mutated) {
        Err(failure) => failure,
        Ok(_) => panic!("partial drawing parsing must not silently omit image semantics"),
    };
    assert_eq!(
        failure.code(),
        WorkerFailureCode::UnsupportedSemanticConstruct
    );
}
