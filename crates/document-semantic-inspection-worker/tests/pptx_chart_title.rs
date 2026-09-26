use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{AdapterProfile, PptxAdapter, SemanticAdapter};
use quick_xml::Reader;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] =
    include_bytes!("../../../experiments/document-semantic-inspection/fixtures/pptx/base.pptx");
const CHART_PART: &str = "ppt/charts/chart1.xml";
const TITLE_CACHE: &str = "<c:title><c:tx><c:strRef><c:f>Sheet1!$A$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>";
const TITLE_CACHE_END: &str =
    "</c:v></c:pt></c:strCache></c:strRef></c:tx><c:layout/><c:overlay val=\"0\"/></c:title>";

fn with_chart_title_cache_label(input: &[u8], label: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("qualified PPTX ZIP");
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("qualified PPTX entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("read qualified PPTX entry");

        if name == CHART_PART {
            let chart = String::from_utf8(contents).expect("chart XML is UTF-8");
            assert_eq!(chart.matches("<c:chart><c:plotArea>").count(), 1);
            let chart = chart.replace(
                "<c:chart><c:plotArea>",
                &format!("<c:chart>{TITLE_CACHE}{label}{TITLE_CACHE_END}<c:plotArea>"),
            );
            assert!(chart.contains(&format!("<c:v>{label}</c:v>")));
            entries.push((name, chart.into_bytes()));
        } else {
            entries.push((name, contents));
        }
    }

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in entries {
        writer
            .start_file(name, options)
            .expect("start synthetic PPTX entry");
        writer
            .write_all(&contents)
            .expect("write synthetic PPTX entry");
    }
    writer
        .finish()
        .expect("finish synthetic PPTX ZIP")
        .into_inner()
}

fn part(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("synthetic PPTX ZIP");
    let mut entry = archive.by_name(name).expect("required PPTX part");
    let mut contents = Vec::new();
    entry.read_to_end(&mut contents).expect("read PPTX part");
    contents
}

fn validate_zip_and_chart_xml(input: &[u8]) {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("synthetic PPTX ZIP");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("valid PPTX central entry");
        std::io::copy(&mut entry, &mut std::io::sink()).expect("valid PPTX entry data");
    }

    let chart = part(input, CHART_PART);
    let mut reader = Reader::from_reader(chart.as_slice());
    reader.config_mut().check_end_names = true;
    loop {
        match reader.read_event().expect("well-formed chart XML") {
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
}

#[test]
fn pptx_cached_chart_title_label_changes_version_identity() {
    let baseline = with_chart_title_cache_label(BASE, "Quarterly Sales");
    let changed = with_chart_title_cache_label(BASE, "Quarterly Revenue");

    validate_zip_and_chart_xml(&baseline);
    validate_zip_and_chart_xml(&changed);

    let baseline_chart = part(&baseline, CHART_PART);
    let changed_chart = part(&changed, CHART_PART);
    assert_ne!(baseline_chart, changed_chart);
    assert_eq!(
        String::from_utf8(baseline_chart)
            .expect("baseline chart XML")
            .replace("Quarterly Sales", "CHART_TITLE_LABEL"),
        String::from_utf8(changed_chart)
            .expect("changed chart XML")
            .replace("Quarterly Revenue", "CHART_TITLE_LABEL"),
        "the cached chart title label must be the only chart XML change"
    );

    let mut baseline_archive = ZipArchive::new(Cursor::new(&baseline)).expect("baseline ZIP");
    let mut changed_archive = ZipArchive::new(Cursor::new(&changed)).expect("changed ZIP");
    assert_eq!(baseline_archive.len(), changed_archive.len());
    for index in 0..baseline_archive.len() {
        let mut baseline_entry = baseline_archive.by_index(index).expect("baseline entry");
        let mut changed_entry = changed_archive.by_index(index).expect("changed entry");
        assert_eq!(baseline_entry.name(), changed_entry.name());
        if baseline_entry.name() == CHART_PART {
            continue;
        }
        let mut baseline_bytes = Vec::new();
        let mut changed_bytes = Vec::new();
        baseline_entry
            .read_to_end(&mut baseline_bytes)
            .expect("read baseline entry");
        changed_entry
            .read_to_end(&mut changed_bytes)
            .expect("read changed entry");
        assert_eq!(baseline_bytes, changed_bytes, "unexpected part change");
    }

    let profile = AdapterProfile::default();
    let baseline_output = PptxAdapter
        .inspect(&baseline, &profile)
        .expect("baseline chart title should be supported");
    let changed_output = PptxAdapter
        .inspect(&changed, &profile)
        .expect("changed chart title should be supported");
    assert_ne!(
        baseline_output.semantic_fingerprint(),
        changed_output.semantic_fingerprint(),
        "cached chart title labels are semantic under Frozen Design §9.4"
    );
}
