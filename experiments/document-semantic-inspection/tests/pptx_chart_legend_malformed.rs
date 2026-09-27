use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter, fingerprint,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";
const FOREIGN_NS: &str = "urn:dsi:test:foreign";

#[test]
fn chart_legend_rejects_schema_invalid_direct_delete_child() {
    let mutant = with_legend("<c:delete val=\"1\"/>", false);
    assert_zip_crc_and_xml_events(&mutant);
    assert_only_chart_xml_changed(BASE, &mutant);

    assert_fail_closed(
        &mutant,
        "c:delete is not a direct child of c:legend in the supported ChartML profile",
    );
}

#[test]
fn chart_legend_position_rejects_foreign_namespaced_val_attribute() {
    let mutant = with_legend("<c:legendPos x:val=\"l\"/>", true);
    assert_zip_crc_and_xml_events(&mutant);
    assert_only_chart_xml_changed(BASE, &mutant);

    assert_fail_closed(
        &mutant,
        "legendPos val must be an unqualified ChartML attribute",
    );
}

#[test]
fn chart_legend_overlay_rejects_foreign_namespaced_val_attribute() {
    let mutant = with_legend("<c:overlay x:val=\"1\"/>", true);
    assert_zip_crc_and_xml_events(&mutant);
    assert_only_chart_xml_changed(BASE, &mutant);

    assert_fail_closed(
        &mutant,
        "overlay val must be an unqualified ChartML attribute",
    );
}

#[test]
fn chart_legend_position_rejects_duplicate_val_attributes() {
    let mutant = with_legend("<c:legendPos val=\"l\" val=\"r\"/>", false);
    // Duplicate attributes are not well-formed XML, but quick_xml's default event reader accepts
    // the token stream. The semantic adapter must still fail closed on this input.
    assert_zip_crc_and_xml_events(&mutant);
    assert_only_chart_xml_changed(BASE, &mutant);

    assert_fail_closed(
        &mutant,
        "duplicate unqualified legendPos val attributes must not be accepted",
    );
}

#[test]
fn explicit_default_legend_settings_match_implicit_defaults() {
    let implicit = with_legend("", false);
    let explicit = with_legend("<c:legendPos val=\"r\"/><c:overlay val=\"0\"/>", false);
    assert_zip_crc_and_xml_events(&implicit);
    assert_zip_crc_and_xml_events(&explicit);
    assert_only_chart_xml_changed(BASE, &implicit);
    assert_only_chart_xml_changed(BASE, &explicit);

    let implicit_output = inspect(&implicit, "implicit legend defaults must inspect");
    let explicit_output = inspect(&explicit, "explicit legend defaults must inspect");
    assert_eq!(
        fingerprint(&implicit_output.semantic_projection),
        fingerprint(&explicit_output.semantic_projection),
        "right position and overlay=false are the ChartML defaults"
    );
}

#[test]
fn visible_legend_position_and_overlay_changes_are_semantic() {
    let defaults = with_legend("", false);
    let left = with_legend("<c:legendPos val=\"l\"/>", false);
    let overlaid = with_legend("<c:overlay val=\"1\"/>", false);
    for mutant in [&defaults, &left, &overlaid] {
        assert_zip_crc_and_xml_events(mutant);
        assert_only_chart_xml_changed(BASE, mutant);
    }

    let defaults_output = inspect(&defaults, "default visible legend must inspect");
    let left_output = inspect(&left, "left-positioned legend must inspect");
    let overlaid_output = inspect(&overlaid, "overlaid legend must inspect");
    assert_ne!(
        fingerprint(&defaults_output.semantic_projection),
        fingerprint(&left_output.semantic_projection),
        "visible legend position changes chart semantics"
    );
    assert_ne!(
        fingerprint(&defaults_output.semantic_projection),
        fingerprint(&overlaid_output.semantic_projection),
        "visible legend overlay changes chart semantics"
    );
}

fn assert_fail_closed(bytes: &[u8], reason: &str) {
    let error = match PptxAdapter.inspect(bytes, &InspectionProfile::default()) {
        Ok(_) => panic!("{reason}"),
        Err(error) => error,
    };
    assert!(
        matches!(
            error.code(),
            ErrorCode::UnsupportedSemanticConstruct | ErrorCode::SemanticExtractionFailed
        ),
        "unexpected failure classification for {reason}: {error}"
    );
}

fn inspect(bytes: &[u8], message: &str) -> document_semantic_inspection_poc::AdapterOutput {
    PptxAdapter
        .inspect(bytes, &InspectionProfile::default())
        .unwrap_or_else(|error| panic!("{message}: {error}"))
}

fn with_legend(children: &str, declare_foreign_prefix: bool) -> Vec<u8> {
    replace_chart(|bytes| {
        let xml = std::str::from_utf8(bytes).expect("base chart XML is UTF-8");
        assert!(!xml.contains("<c:legend"), "base chart has no legend");
        assert!(xml.contains("</c:plotArea></c:chart>"));
        let mut chart = xml.replace(
            "</c:plotArea></c:chart>",
            &format!("</c:plotArea><c:legend>{children}</c:legend></c:chart>"),
        );
        if declare_foreign_prefix {
            chart = chart.replace(
                "<c:chartSpace ",
                &format!("<c:chartSpace xmlns:x=\"{FOREIGN_NS}\" "),
            );
        }
        chart.into_bytes()
    })
}

fn replace_chart(transform: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let chart = parts
        .iter_mut()
        .find(|(name, _)| name == CHART)
        .unwrap_or_else(|| panic!("missing PPTX part {CHART}"));
    chart.1 = transform(&chart.1);
    write_parts(parts)
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("ZIP entry reads with a valid CRC");
        parts.push((name, contents));
    }
    parts
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("synthetic PPTX ZIP entry starts");
        writer
            .write_all(&contents)
            .expect("synthetic PPTX ZIP entry writes");
    }
    writer
        .finish()
        .expect("synthetic PPTX ZIP finishes")
        .into_inner()
}

fn assert_zip_crc_and_xml_events(bytes: &[u8]) {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("mutant remains a valid ZIP");
    let mut names = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("mutant central-directory entry");
        let name = file.name().to_owned();
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .expect("mutant ZIP entry has a valid CRC");
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("mutant XML remains UTF-8");
            let mut reader = Reader::from_str(xml);
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(error) => panic!("mutant part {name} is malformed XML: {error}"),
                }
            }
        }
        names.push(name);
    }
    assert!(names.iter().any(|name| name == "[Content_Types].xml"));
    assert!(names.iter().any(|name| name == "ppt/presentation.xml"));
}

fn assert_only_chart_xml_changed(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(
        baseline_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        mutant_parts
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        "mutant keeps the package entries in the same order"
    );

    let changed_parts = baseline_parts
        .iter()
        .zip(&mutant_parts)
        .filter_map(|((name, original), (_, changed))| {
            (original != changed).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(changed_parts, [CHART]);
}
