use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    ErrorCode, InspectionAdapter, InspectionProfile, PptxAdapter,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!("../fixtures/pptx/base.pptx");
const CHART: &str = "ppt/charts/chart1.xml";

#[test]
fn value_axis_range_is_rejected_until_chart_axes_are_modeled() {
    let mutant = package_with_value_axis_range();
    assert_valid_chart_only_mutation(&mutant, "<c:max val=\"100\"/>");
    assert_unsupported_chart_construct(&mutant, "a visible value-axis range");
}

#[test]
fn visible_chart_data_table_is_rejected_until_modeled() {
    let mutant = package_with_chart_fragment("</c:plotArea>", "<c:dTable/>");
    assert_valid_chart_only_mutation(&mutant, "<c:dTable/>");
    assert_unsupported_chart_construct(&mutant, "a visible chart data table");
}

#[test]
fn chart_trendline_is_rejected_until_modeled() {
    let mutant = package_with_chart_fragment(
        "<c:cat>",
        "<c:trendline><c:trendlineType val=\"linear\"/><c:dispEq val=\"1\"/></c:trendline>",
    );
    assert_valid_chart_only_mutation(&mutant, "<c:trendline>");
    assert_unsupported_chart_construct(&mutant, "a visible chart trendline");
}

#[test]
fn chart_error_bars_are_rejected_until_modeled() {
    let mutant = package_with_chart_fragment(
        "<c:cat>",
        concat!(
            "<c:errBars><c:errDir val=\"y\"/><c:errBarType val=\"both\"/>",
            "<c:errValType val=\"fixedVal\"/><c:noEndCap val=\"0\"/>",
            "<c:val val=\"1.5\"/></c:errBars>"
        ),
    );
    assert_valid_chart_only_mutation(&mutant, "<c:errBars>");
    assert_unsupported_chart_construct(&mutant, "visible chart error bars");
}

#[test]
fn plot_visible_only_switch_is_rejected_until_modeled() {
    let mutant = package_with_chart_fragment("</c:plotArea>", "<c:plotVisOnly val=\"0\"/>");
    assert_valid_chart_only_mutation(&mutant, "<c:plotVisOnly val=\"0\"/>");
    assert_unsupported_chart_construct(&mutant, "the plotVisOnly display switch");
}

#[test]
fn blank_value_display_switch_is_rejected_until_modeled() {
    let mutant = package_with_chart_fragment("</c:plotArea>", "<c:dispBlanksAs val=\"zero\"/>");
    assert_valid_chart_only_mutation(&mutant, "<c:dispBlanksAs val=\"zero\"/>");
    assert_unsupported_chart_construct(&mutant, "the dispBlanksAs display switch");
}

fn assert_unsupported_chart_construct(mutant: &[u8], description: &str) {
    PptxAdapter
        .inspect(BASE, &InspectionProfile::default())
        .expect("qualified baseline PPTX must inspect");

    let error = match PptxAdapter.inspect(mutant, &InspectionProfile::default()) {
        Ok(_) => panic!("{description} must fail closed until its semantics are modeled"),
        Err(error) => error,
    };
    assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct);
}

fn package_with_value_axis_range() -> Vec<u8> {
    package_with_chart_fragment(
        "</c:plotArea>",
        "<c:valAx><c:scaling><c:max val=\"100\"/><c:min val=\"0\"/></c:scaling></c:valAx>",
    )
}

fn package_with_chart_fragment(anchor: &str, fragment: &str) -> Vec<u8> {
    let mut parts = read_parts(BASE);
    let chart = parts
        .iter_mut()
        .find(|(name, _)| name == CHART)
        .expect("base PPTX contains its chart part");
    let xml = std::str::from_utf8(&chart.1).expect("base chart XML is UTF-8");
    assert_eq!(
        xml.matches(anchor).count(),
        1,
        "chart anchor must be unique"
    );
    chart.1 = xml
        .replace(anchor, &format!("{fragment}{anchor}"))
        .into_bytes();
    write_parts(parts)
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("base is a ZIP archive");
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

fn read_part(archive: &[u8], part_name: &str) -> Vec<u8> {
    read_parts(archive)
        .into_iter()
        .find(|(name, _)| name == part_name)
        .unwrap_or_else(|| panic!("missing ZIP part {part_name}"))
        .1
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("mutant ZIP part starts");
        writer.write_all(&contents).expect("mutant ZIP part writes");
    }
    writer.finish().expect("mutant ZIP finishes").into_inner()
}

fn assert_valid_chart_only_mutation(mutant: &[u8], expected_fragment: &str) {
    let baseline_parts = read_parts(BASE);
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
        "the mutant keeps the same package entries in the same order"
    );

    let mut changed_parts = Vec::new();
    for ((name, original), (_, changed)) in baseline_parts.iter().zip(&mutant_parts) {
        if original != changed {
            changed_parts.push(name.as_str());
        }
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(changed).expect("mutant XML remains UTF-8");
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
    }
    assert_eq!(changed_parts, [CHART]);
    let chart = String::from_utf8(read_part(mutant, CHART)).expect("chart XML is UTF-8");
    assert!(chart.contains(expected_fragment));
}
