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
const MALFORMED_IMAGE_PART: &str = "ppt/media/image%Q1.png";
const SLIDE_RELATIONSHIPS: &str = "ppt/slides/_rels/slide1.xml.rels";
const IMAGE_TARGET: &str = "../media/image1.png";
const MALFORMED_IMAGE_TARGET: &str = "../media/image%Q1.png";

#[test]
fn malformed_percent_escape_in_image_part_uri_fails_closed() {
    assert_image_bearing_baseline_is_accepted();

    let mutant = rename_image_part_and_relationship_target(BASE);
    assert_only_image_uri_mutations(BASE, &mutant);
    assert_valid_zip_crc_and_xml(BASE);
    assert_valid_zip_crc_and_xml(&mutant);

    let failure = PptxAdapter
        .inspect(&mutant, &AdapterProfile::default())
        .expect_err("malformed OPC percent escapes must fail closed");
    assert!(
        matches!(
            failure.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct
                | WorkerFailureCode::SemanticExtractionFailed
        ),
        "malformed OPC URI should be rejected as unsupported or semantically invalid: {failure}"
    );
}

fn assert_image_bearing_baseline_is_accepted() {
    let parts = read_parts(BASE);
    let image = parts
        .iter()
        .find(|(name, _)| name == IMAGE_PART)
        .map(|(_, bytes)| bytes)
        .expect("image-bearing fixture contains the referenced image part");
    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));

    let relationships = parts
        .iter()
        .find(|(name, _)| name == SLIDE_RELATIONSHIPS)
        .map(|(_, bytes)| std::str::from_utf8(bytes).expect("relationships are UTF-8 XML"))
        .expect("image-bearing fixture contains slide relationships");
    assert_eq!(relationships.matches(IMAGE_TARGET).count(), 1);

    PptxAdapter
        .inspect(BASE, &AdapterProfile::default())
        .expect("qualified image-bearing PPTX is an accepted semantic baseline");
}

fn rename_image_part_and_relationship_target(archive: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(archive);
    let mut renamed_image_count = 0;
    let mut changed_relationship_count = 0;

    for (name, contents) in &mut parts {
        if name == IMAGE_PART {
            *name = MALFORMED_IMAGE_PART.to_owned();
            renamed_image_count += 1;
        } else if name == SLIDE_RELATIONSHIPS {
            let xml = std::str::from_utf8(contents).expect("relationships are UTF-8 XML");
            assert_eq!(xml.matches(IMAGE_TARGET).count(), 1);
            *contents = xml
                .replacen(IMAGE_TARGET, MALFORMED_IMAGE_TARGET, 1)
                .into_bytes();
            changed_relationship_count += 1;
        }
    }

    assert_eq!(renamed_image_count, 1, "image part is renamed exactly once");
    assert_eq!(
        changed_relationship_count, 1,
        "the image relationship target changes exactly once"
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

fn assert_only_image_uri_mutations(baseline: &[u8], mutant: &[u8]) {
    let baseline_parts = read_parts(baseline);
    let mutant_parts = read_parts(mutant);
    assert_eq!(baseline_parts.len(), mutant_parts.len());

    let mut saw_image_rename = false;
    let mut saw_relationship_change = false;
    for ((baseline_name, baseline_data), (mutant_name, mutant_data)) in
        baseline_parts.iter().zip(&mutant_parts)
    {
        let expected_name = if baseline_name == IMAGE_PART {
            saw_image_rename = true;
            MALFORMED_IMAGE_PART
        } else {
            baseline_name.as_str()
        };
        assert_eq!(
            mutant_name.as_str(),
            expected_name,
            "only the image part name changes"
        );

        if baseline_name == SLIDE_RELATIONSHIPS {
            let xml = std::str::from_utf8(baseline_data).expect("baseline relationships XML");
            let expected = xml
                .replacen(IMAGE_TARGET, MALFORMED_IMAGE_TARGET, 1)
                .into_bytes();
            assert_ne!(baseline_data, &expected);
            assert_eq!(mutant_data, &expected);
            saw_relationship_change = true;
        } else {
            assert_eq!(
                mutant_data, baseline_data,
                "unrelated package part {baseline_name} remains byte-identical"
            );
        }
    }

    assert!(saw_image_rename, "mutant renamed the image ZIP part");
    assert!(
        saw_relationship_change,
        "mutant changed the image relationship target"
    );
}

fn assert_valid_zip_crc_and_xml(archive: &[u8]) {
    let parts = read_parts(archive);
    for (name, contents) in parts {
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
