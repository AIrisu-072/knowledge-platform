//! P1-I04 DOCX/XLSX/XLSM/PPTX readers: physical locators, known omissions and
//! fail-closed package checks.

mod support;

use search_core::knowledge_unit::{
    BudgetKey, DocxStep, FormatId, FormatSettings, NativeLocator, PptxTextSlot, UnitKind,
};
use search_extraction_core::{
    BodyCoverage, CoverageReason, NativeOmission, PermanentFailureCode, ReaderFailure, WorkerReport,
};
use search_extraction_worker::readers::extract_for_test;

fn read(format: FormatId, raw: &[u8]) -> Result<WorkerReport, ReaderFailure> {
    extract_for_test(
        format,
        &FormatSettings::None,
        raw,
        &mut support::meter(format),
    )
}

fn texts(report: &WorkerReport) -> Vec<(&str, UnitKind)> {
    report
        .fragments
        .iter()
        .map(|fragment| (fragment.text.as_str(), fragment.kind))
        .collect()
}

fn paragraph(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

fn heading(text: &str) -> String {
    format!(r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#)
}

#[test]
fn docx_heading_nested_table_and_header_omission() {
    let inner = format!(
        "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
        paragraph("内側")
    );
    let body = format!(
        "{}{}<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}{}</w:tc></w:tr></w:tbl>",
        heading("見出し"),
        paragraph("東京"),
        paragraph("同文。"),
        paragraph("右"),
        inner
    );
    let report = read(FormatId::Docx, &support::docx(&body, &[], &[])).unwrap();
    assert_eq!(report.coverage, BodyCoverage::Supported);
    assert_eq!(
        texts(&report),
        vec![
            ("見出し", UnitKind::Heading),
            ("東京", UnitKind::Paragraph),
            ("同文。", UnitKind::TableCell),
            ("右", UnitKind::TableCell),
            ("内側", UnitKind::TableCell),
        ]
    );
    use DocxStep::{BodyBlock, Cell, CellBlock, Row};
    assert_eq!(
        report.fragments[2].locator,
        NativeLocator::Docx {
            steps: vec![BodyBlock(2), Row(0), Cell(0), CellBlock(0)]
        }
    );
    assert_eq!(
        report.fragments[4].locator,
        NativeLocator::Docx {
            steps: vec![
                BodyBlock(2),
                Row(0),
                Cell(1),
                CellBlock(1),
                Row(0),
                Cell(0),
                CellBlock(0)
            ]
        }
    );
    support::assert_valid(FormatId::Docx, &report);

    // A referenced header is a located package omission → Partial.
    let header = r#"<?xml version="1.0" encoding="UTF-8"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>ヘッダ</w:t></w:r></w:p></w:hdr>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#;
    let body = format!(
        r#"{}<w:sectPr><w:headerReference w:type="default" r:id="rId9"/></w:sectPr>"#,
        paragraph("本文")
    );
    let raw = support::docx(
        &body,
        &[
            ("word/header1.xml", header.to_owned()),
            ("word/_rels/document.xml.rels", rels.to_owned()),
        ],
        &[(
            "word/header1.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
        )],
    );
    let report = read(FormatId::Docx, &raw).unwrap();
    assert_eq!(
        report.coverage,
        BodyCoverage::Partial {
            reasons: vec![CoverageReason::UnsupportedStructure]
        }
    );
    assert_eq!(texts(&report), vec![("本文", UnitKind::Paragraph)]);
    assert_eq!(
        report.known_omissions,
        vec![NativeOmission {
            package_path: Some("word/header1.xml".into()),
            physical_child_path: vec![],
            reason: CoverageReason::UnsupportedStructure,
        }]
    );
    support::assert_valid(FormatId::Docx, &report);
}

#[test]
fn ooxml_package_spoof_and_structure_fail_closed() {
    // Main part declared with a spreadsheet content type.
    let types = support::content_types(&[("word/document.xml", support::XLSX_MAIN)]);
    let rels = support::package_rels("word/document.xml");
    let document = r#"<w:document xmlns:w="w"><w:body/></w:document>"#;
    let spoofed = support::zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ]);
    assert_eq!(
        read(FormatId::Docx, &spoofed),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    // Unknown body block, DOCTYPE, unknown package part and broken XML.
    assert_eq!(
        read(FormatId::Docx, &support::docx("<w:sdt/>", &[], &[])),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    assert_eq!(
        read(
            FormatId::Docx,
            &support::docx(&paragraph("x"), &[("word/media/a.png", "png".into())], &[])
        ),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    let types = support::content_types(&[("word/document.xml", support::DOCX_MAIN)]);
    let doctype = support::zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        (
            "word/document.xml",
            br#"<!DOCTYPE x [<!ENTITY a "b">]><w:document xmlns:w="w"><w:body/></w:document>"#,
        ),
    ]);
    assert_eq!(
        read(FormatId::Docx, &doctype),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
    let broken = support::zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", b"<w:document><w:body>"),
    ]);
    assert_eq!(
        read(FormatId::Docx, &broken),
        Err(ReaderFailure::Permanent(
            PermanentFailureCode::CorruptDocument
        ))
    );
    assert_eq!(
        read(FormatId::Docx, b"PK\x03\x04 not a zip"),
        Err(ReaderFailure::Permanent(
            PermanentFailureCode::MalformedArchive
        ))
    );
    // XML depth beyond the registered profile is a resource limit, not a partial result.
    let mut meter = support::meter_with(FormatId::Docx, BudgetKey::XmlDepth, 3);
    assert_eq!(
        extract_for_test(
            FormatId::Docx,
            &FormatSettings::None,
            &support::docx(&paragraph("deep"), &[], &[]),
            &mut meter
        ),
        Err(ReaderFailure::Unsupported(CoverageReason::ResourceLimit))
    );
}

#[test]
fn xlsx_absolute_cells_shared_strings_and_formula_omissions() {
    let sheet = concat!(
        r#"<row r="3"><c r="C3" t="inlineStr"><is><t>東京</t></is></c><c r="D3" t="s"><v>0</v></c></row>"#,
        r#"<row r="4"><c r="C4"><f>C3</f><v>10</v></c><c r="D4"><f>D3</f></c><c r="E4"><v>42</v></c></row>"#
    );
    let shared = r#"<si><r><rPr/><t>同</t></r><r><t>文。</t></r></si>"#;
    let raw = support::workbook(
        support::XLSX_MAIN,
        &[("一", sheet, None), ("隠し", "", Some("hidden"))],
        Some(shared),
        &[],
    );
    let report = read(FormatId::Xlsx, &raw).unwrap();
    let cell = |row, col| NativeLocator::Spreadsheet {
        sheet_ordinal: 0,
        row,
        col,
    };
    assert_eq!(
        report
            .fragments
            .iter()
            .map(|fragment| (fragment.text.as_str(), fragment.locator.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("東京", cell(2, 2)),
            ("同文。", cell(2, 3)),
            ("42", cell(3, 4))
        ]
    );
    assert_eq!(
        report.coverage,
        BodyCoverage::Partial {
            reasons: vec![
                CoverageReason::UnsupportedStructure,
                CoverageReason::MissingFormulaCache
            ]
        }
    );
    let sheet1 = Some("xl/worksheets/sheet1.xml".to_owned());
    assert_eq!(
        report.known_omissions,
        vec![
            NativeOmission {
                package_path: sheet1.clone(),
                physical_child_path: vec![1, 0],
                reason: CoverageReason::UnsupportedStructure,
            },
            NativeOmission {
                package_path: sheet1,
                physical_child_path: vec![1, 1],
                reason: CoverageReason::MissingFormulaCache,
            },
            NativeOmission {
                package_path: Some("xl/worksheets/sheet2.xml".into()),
                physical_child_path: vec![],
                reason: CoverageReason::UnsupportedStructure,
            },
        ]
    );
    support::assert_valid(FormatId::Xlsx, &report);

    // Formula-only sheets cannot become Partial without any verified Unit.
    let formulas = support::workbook(
        support::XLSX_MAIN,
        &[("一", r#"<row r="1"><c r="A1"><f>1+1</f></c></row>"#, None)],
        None,
        &[],
    );
    assert_eq!(
        read(FormatId::Xlsx, &formulas),
        Err(ReaderFailure::Unsupported(
            CoverageReason::MissingFormulaCache
        ))
    );
    // Out-of-range shared string index.
    let broken = support::workbook(
        support::XLSX_MAIN,
        &[(
            "一",
            r#"<row r="1"><c r="A1" t="s"><v>7</v></c></row>"#,
            None,
        )],
        Some("<si><t>a</t></si>"),
        &[],
    );
    assert_eq!(
        read(FormatId::Xlsx, &broken),
        Err(ReaderFailure::Permanent(
            PermanentFailureCode::CorruptDocument
        ))
    );
}

#[test]
fn xlsm_macro_is_never_body_scope() {
    let raw = support::workbook(
        support::XLSM_MAIN,
        &[(
            "一",
            r#"<row r="1"><c r="A1" t="inlineStr"><is><t>本文</t></is></c></row>"#,
            None,
        )],
        None,
        &[(
            "xl/vbaProject.bin",
            b"\xd0\xcf\x11\xe0 synthetic macro bytes",
        )],
    );
    let report = read(FormatId::Xlsm, &raw).unwrap();
    assert_eq!(texts(&report), vec![("本文", UnitKind::SpreadsheetCell)]);
    assert_eq!(
        report.known_omissions,
        vec![NativeOmission {
            package_path: Some("xl/vbaProject.bin".into()),
            physical_child_path: vec![],
            reason: CoverageReason::UnsupportedStructure,
        }]
    );
    support::assert_valid(FormatId::Xlsm, &report);
    // The same bytes read as XLSX fail the declared content type.
    assert_eq!(
        read(FormatId::Xlsx, &raw),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
}

#[test]
fn pptx_group_table_slides_and_notes() {
    let table = r#"<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>表</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>同文。</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#;
    let first = format!(
        "{}<p:grpSp>{}{}</p:grpSp>",
        support::shape("東京"),
        support::shape("群1"),
        support::shape("群2")
    );
    let raw = support::presentation(&[&first, table], Some("ノート"));
    let report = read(FormatId::Pptx, &raw).unwrap();
    let slot = |paragraph| PptxTextSlot::ShapeParagraph { paragraph };
    assert_eq!(
        report
            .fragments
            .iter()
            .map(|fragment| (
                fragment.text.as_str(),
                fragment.kind,
                fragment.locator.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "東京",
                UnitKind::SlideText,
                NativeLocator::Pptx {
                    slide_ordinal: 0,
                    shape_path: vec![0],
                    text_slot: slot(0)
                }
            ),
            (
                "群1",
                UnitKind::SlideText,
                NativeLocator::Pptx {
                    slide_ordinal: 0,
                    shape_path: vec![1, 0],
                    text_slot: slot(0)
                }
            ),
            (
                "群2",
                UnitKind::SlideText,
                NativeLocator::Pptx {
                    slide_ordinal: 0,
                    shape_path: vec![1, 1],
                    text_slot: slot(0)
                }
            ),
            (
                "表",
                UnitKind::SlideText,
                NativeLocator::Pptx {
                    slide_ordinal: 1,
                    shape_path: vec![0],
                    text_slot: PptxTextSlot::TableCellParagraph {
                        row: 0,
                        col: 0,
                        paragraph: 0
                    }
                }
            ),
            (
                "同文。",
                UnitKind::SlideText,
                NativeLocator::Pptx {
                    slide_ordinal: 1,
                    shape_path: vec![0],
                    text_slot: PptxTextSlot::TableCellParagraph {
                        row: 0,
                        col: 1,
                        paragraph: 0
                    }
                }
            ),
        ]
    );
    assert_eq!(
        report.known_omissions,
        vec![NativeOmission {
            package_path: Some("ppt/notesSlides/notesSlide1.xml".into()),
            physical_child_path: vec![],
            reason: CoverageReason::UnsupportedStructure,
        }]
    );
    support::assert_valid(FormatId::Pptx, &report);

    // A chart graphic frame is not locatable text: the item is unsupported.
    let chart = r#"<p:graphicFrame><a:graphic><a:graphicData><c:chart xmlns:c="c"/></a:graphicData></a:graphic></p:graphicFrame>"#;
    assert_eq!(
        read(FormatId::Pptx, &support::presentation(&[chart], None)),
        Err(ReaderFailure::Unsupported(
            CoverageReason::UnsupportedStructure
        ))
    );
}
