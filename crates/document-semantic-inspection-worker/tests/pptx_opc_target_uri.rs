use std::io::{Cursor, Read, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, PptxAdapter, SemanticAdapter, WorkerFailureCode,
};
use quick_xml::{Reader, events::Event};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const BASE: &[u8] = include_bytes!(
    "../../../experiments/document-semantic-inspection/fixtures/pptx/image-change.pptx"
);
const IMAGE_PART: &str = "ppt/media/image1.png";
const SLIDE_RELATIONSHIPS: &str = "ppt/slides/_rels/slide1.xml.rels";
const IMAGE_TARGET: &str = "../media/image1.png";
const MALFORMED_CANCELLABLE_TARGET: &str = "../media/image%Q1.png/../image1.png";

#[test]
fn malformed_percent_escape_in_cancellable_relationship_segment_fails_closed() {
    assert_image_bearing_baseline_is_accepted();

    let mutant = mutate_only_image_relationship_target(BASE);
    assert_only_relationship_target_changed(BASE, &mutant);
    assert_valid_zip_crc_and_xml(BASE);
    assert_valid_zip_crc_and_xml(&mutant);

    let failure = PptxAdapter
        .inspect(&mutant, &AdapterProfile::default())
        .expect_err("malformed percent escapes must fail before path normalization");
    assert!(
        matches!(
            failure.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
                | WorkerFailureCode::SemanticExtractionFailed
        ),
        "malformed OPC relationship target should fail closed: {failure}"
    );
}

fn assert_image_bearing_baseline_is_accepted() {
    let parts = read_parts(BASE);
    let image = parts
        .iter()
        .find(|(name, _)| name == IMAGE_PART)
        .map(|(_, contents)| contents)
        .expect("baseline contains the referenced image part");
    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));

    let relationships = parts
        .iter()
        .find(|(name, _)| name == SLIDE_RELATIONSHIPS)
        .map(|(_, contents)| std::str::from_utf8(contents).expect("relationship XML is UTF-8"))
        .expect("baseline contains slide relationships");
    assert_eq!(relationships.matches(IMAGE_TARGET).count(), 1);

    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified image-bearing PPTX baseline is accepted");
}

fn mutate_only_image_relationship_target(archive: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(archive);
    let mut changed_relationship_count = 0;

    for (name, contents) in &mut parts {
        if name == SLIDE_RELATIONSHIPS {
            let xml = std::str::from_utf8(contents).expect("relationship XML is UTF-8");
            assert_eq!(xml.matches(IMAGE_TARGET).count(), 1);
            *contents = xml
                .replacen(IMAGE_TARGET, MALFORMED_CANCELLABLE_TARGET, 1)
                .into_bytes();
            changed_relationship_count += 1;
        }
    }

    assert_eq!(
        changed_relationship_count, 1,
        "exactly one slide relationship part is mutated"
    );

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("mutant ZIP entry starts");
        writer
            .write_all(&contents)
            .expect("mutant ZIP entry is written");
    }
    writer
        .finish()
        .expect("mutant ZIP archive is complete")
        .into_inner()
}

fn assert_only_relationship_target_changed(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());

    let mut saw_relationship_change = false;
    for ((baseline_name, baseline_contents), (mutant_name, mutant_contents)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        assert_eq!(
            mutant_name, baseline_name,
            "ZIP member names remain unchanged"
        );

        if baseline_name == SLIDE_RELATIONSHIPS {
            let xml = std::str::from_utf8(baseline_contents).expect("baseline relationship XML");
            assert_eq!(xml.matches(IMAGE_TARGET).count(), 1);
            let expected = xml
                .replacen(IMAGE_TARGET, MALFORMED_CANCELLABLE_TARGET, 1)
                .into_bytes();
            assert_ne!(baseline_contents, &expected);
            assert_eq!(mutant_contents, &expected);
            let mutant_xml = std::str::from_utf8(mutant_contents)
                .expect("mutant relationship XML remains UTF-8");
            assert_eq!(mutant_xml.matches(MALFORMED_CANCELLABLE_TARGET).count(), 1);
            saw_relationship_change = true;
        } else {
            assert_eq!(
                mutant_contents, baseline_contents,
                "ZIP member payload {baseline_name} remains byte-identical"
            );
        }
    }

    assert!(
        saw_relationship_change,
        "the slide relationship target was mutated"
    );
}

fn assert_valid_zip_crc_and_xml(archive: &[u8]) {
    for (name, contents) in read_parts(archive) {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let xml = std::str::from_utf8(&contents).expect("PPTX XML part is UTF-8");
            let mut reader = Reader::from_str(xml);
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(error) => panic!("PPTX XML part {name} is malformed: {error}"),
                }
            }
        }
    }
}

fn read_parts(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(Cursor::new(archive)).expect("PPTX is a readable ZIP archive");
    let mut parts = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("ZIP central-directory entry");
        let name = entry.name().to_owned();
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .expect("ZIP entry payload and CRC are valid");
        parts.push((name, contents));
    }
    parts
}
