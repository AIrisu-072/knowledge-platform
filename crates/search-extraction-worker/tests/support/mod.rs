//! Synthetic, runtime-generated fixtures for reader contract tests.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use search_core::knowledge_unit::{
    BudgetKey, ExtractionProfileDefinitionV1, FormatId, FormatSettings,
};
use search_extraction_core::{BudgetMeter, ExtractionBudgets, RegisteredProfile, WorkerReport};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

pub fn limits(format: FormatId) -> BTreeMap<BudgetKey, u64> {
    let mut limits: BTreeMap<_, _> = BudgetKey::ALL.into_iter().map(|key| (key, 0)).collect();
    limits.insert(BudgetKey::InputBytes, 268_435_456);
    limits.insert(BudgetKey::Units, 100_000);
    limits.insert(BudgetKey::UnitUtf8Bytes, 1_048_576);
    limits.insert(BudgetKey::WorkerOutputBytes, 16_777_216);
    if matches!(
        format,
        FormatId::Docx | FormatId::Xlsx | FormatId::Xlsm | FormatId::Pptx | FormatId::Zip
    ) {
        limits.insert(BudgetKey::ZipEntries, 20_000);
        limits.insert(BudgetKey::ZipEntryBytes, 67_108_864);
        limits.insert(BudgetKey::ZipTotalBytes, 536_870_912);
        limits.insert(
            BudgetKey::ZipDepth,
            if format == FormatId::Zip { 3 } else { 1 },
        );
        limits.insert(BudgetKey::XmlDepth, 256);
        limits.insert(BudgetKey::XmlNodes, 2_000_000);
    }
    if matches!(format, FormatId::Pdf | FormatId::Zip) {
        limits.insert(BudgetKey::PdfPages, 1_024);
        limits.insert(BudgetKey::PdfOperations, 1_000_000);
    }
    if matches!(format, FormatId::Html | FormatId::Zip) {
        limits.insert(BudgetKey::HtmlNodes, 2_000_000);
    }
    if matches!(format, FormatId::Csv | FormatId::Zip) {
        limits.insert(BudgetKey::CsvRecords, 1_000_000);
        limits.insert(BudgetKey::CsvFieldBytes, 1_048_576);
    }
    limits
}

pub fn settings(format: FormatId) -> FormatSettings {
    match format {
        FormatId::Text => FormatSettings::Text {
            charset: "utf-8".into(),
        },
        FormatId::Csv => FormatSettings::Csv {
            charset: "utf-8".into(),
            delimiter: b',',
            quote: b'"',
        },
        FormatId::Zip => FormatSettings::Archive {
            member_decoder: "utf-8".into(),
        },
        _ => FormatSettings::None,
    }
}

pub fn definition(format: FormatId) -> ExtractionProfileDefinitionV1 {
    ExtractionProfileDefinitionV1 {
        format,
        parser_name: "search-extraction-worker".into(),
        parser_version: "1".into(),
        parser_build_sha256: [7; 32],
        native_binary_sha256: (format == FormatId::Pdf).then_some([8; 32]),
        scope_revision: 1,
        segmentation_revision: 1,
        normalization_revision: 1,
        locator_revision: 1,
        format_settings: settings(format),
        limits: limits(format),
    }
}

pub fn meter(format: FormatId) -> BudgetMeter {
    BudgetMeter::new(ExtractionBudgets::new(limits(format)).unwrap()).unwrap()
}

pub fn meter_with(format: FormatId, key: BudgetKey, value: u64) -> BudgetMeter {
    let mut limits = limits(format);
    limits.insert(key, value);
    BudgetMeter::new(ExtractionBudgets::new(limits).unwrap()).unwrap()
}

/// The pure host-side contract must accept every report the reader produced.
pub fn assert_valid(format: FormatId, report: &WorkerReport) {
    let profile = RegisteredProfile::register_definition(definition(format)).unwrap();
    search_extraction_core::validate_worker_report(report, &profile).unwrap();
}

pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        // A trailing `/` writes an explicit directory entry.
        if name.ends_with('/') {
            writer.add_directory(*name, options).unwrap();
            continue;
        }
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

pub const DOCX_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
pub const XLSX_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
pub const XLSM_MAIN: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
pub const PPTX_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";

pub fn content_types(overrides: &[(&str, &str)]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    );
    for (part, kind) in overrides {
        out.push_str(&format!(
            r#"<Override PartName="/{part}" ContentType="{kind}"/>"#
        ));
    }
    out.push_str("</Types>");
    out
}

pub fn package_rels(main: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{main}"/></Relationships>"#
    )
}

pub fn docx(body_xml: &str, extra: &[(&str, String)], extra_types: &[(&str, &str)]) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{body_xml}</w:body></w:document>"#
    );
    let mut types = vec![("word/document.xml", DOCX_MAIN)];
    types.extend_from_slice(extra_types);
    let types = content_types(&types);
    let rels = package_rels("word/document.xml");
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ];
    for (name, bytes) in extra {
        entries.push((name, bytes.as_bytes()));
    }
    zip(&entries)
}

pub fn workbook(
    main_type: &str,
    sheets: &[(&str, &str, Option<&str>)],
    shared: Option<&str>,
    extra: &[(&str, &[u8])],
) -> Vec<u8> {
    let mut types = vec![("xl/workbook.xml", main_type)];
    let mut sheet_list = String::new();
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut parts = Vec::new();
    for (index, (name, sheet_data, state)) in sheets.iter().enumerate() {
        let id = index + 1;
        let path = format!("xl/worksheets/sheet{id}.xml");
        let state = state
            .map(|state| format!(r#" state="{state}""#))
            .unwrap_or_default();
        sheet_list.push_str(&format!(
            r#"<sheet name="{name}" sheetId="{id}" r:id="rId{id}"{state}/>"#
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{id}.xml"/>"#
        ));
        parts.push((
            path,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet_data}</sheetData></worksheet>"#
            ),
        ));
    }
    rels.push_str("</Relationships>");
    let paths: Vec<String> = parts.iter().map(|(path, _)| path.clone()).collect();
    for path in &paths {
        types.push((
            path.as_str(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
        ));
    }
    if shared.is_some() {
        types.push((
            "xl/sharedStrings.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml",
        ));
    }
    let types = content_types(&types);
    let package = package_rels("xl/workbook.xml");
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheet_list}</sheets></workbook>"#
    );
    let shared_xml = shared.map(|items| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{items}</sst>"#
        )
    });
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", package.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
    ];
    for (path, xml) in &parts {
        entries.push((path.as_str(), xml.as_bytes()));
    }
    if let Some(xml) = &shared_xml {
        entries.push(("xl/sharedStrings.xml", xml.as_bytes()));
    }
    entries.extend_from_slice(extra);
    zip(&entries)
}

pub fn presentation(slides: &[&str], notes_for_first: Option<&str>) -> Vec<u8> {
    let mut types = vec![("ppt/presentation.xml", PPTX_MAIN)];
    let mut list = String::new();
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut parts = Vec::new();
    for (index, tree) in slides.iter().enumerate() {
        let id = index + 1;
        list.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{id}"/>"#, 255 + id));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{id}.xml"/>"#
        ));
        parts.push((
            format!("ppt/slides/slide{id}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree>{tree}</p:spTree></p:cSld></p:sld>"#
            ),
        ));
    }
    rels.push_str("</Relationships>");
    if let Some(note) = notes_for_first {
        types.push((
            "ppt/notesSlides/notesSlide1.xml",
            "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml",
        ));
        parts.push((
            "ppt/slides/_rels/slide1.xml.rels".into(),
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide1.xml"/></Relationships>"#.into(),
        ));
        parts.push((
            "ppt/notesSlides/notesSlide1.xml".into(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{note}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#
            ),
        ));
    }
    let types = content_types(&types);
    let package = package_rels("ppt/presentation.xml");
    let presentation = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>{list}</p:sldIdLst></p:presentation>"#
    );
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", package.as_bytes()),
        ("ppt/presentation.xml", presentation.as_bytes()),
        ("ppt/_rels/presentation.xml.rels", rels.as_bytes()),
    ];
    for (path, xml) in &parts {
        entries.push((path.as_str(), xml.as_bytes()));
    }
    zip(&entries)
}

pub fn shape(text: &str) -> String {
    format!(r#"<p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#)
}
