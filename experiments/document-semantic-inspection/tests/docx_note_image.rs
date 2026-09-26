mod support;

use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint as semantic_fingerprint,
};
use support::ooxml::{PngDeflateEncoding, decode_rgba8_png_fixture, rgba8_png_fixture};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const PACKAGE_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_RELATIONSHIPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WORDPROCESSINGML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const DRAWINGML: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WORDPROCESSING_DRAWING: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const PICTURE: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

#[derive(Clone, Copy, Debug)]
enum NoteKind {
    Footnote,
    Endnote,
}

impl NoteKind {
    fn part_name(self) -> &'static str {
        match self {
            Self::Footnote => "footnotes",
            Self::Endnote => "endnotes",
        }
    }

    fn reference_name(self) -> &'static str {
        match self {
            Self::Footnote => "footnoteReference",
            Self::Endnote => "endnoteReference",
        }
    }

    fn note_relationship(self) -> &'static str {
        match self {
            Self::Footnote => "footnotes",
            Self::Endnote => "endnotes",
        }
    }
}

#[test]
fn referenced_footnote_image_changes_semantics_or_fails_closed() {
    assert_note_image_target_is_semantic(NoteKind::Footnote);
}

#[test]
fn referenced_endnote_image_changes_semantics_or_fails_closed() {
    assert_note_image_target_is_semantic(NoteKind::Endnote);
}

#[test]
fn note_table_structure_changes_semantics_or_fails_closed() {
    let image_one = rgba8_png_fixture([220, 20, 60, 255], PngDeflateEncoding::FixedHuffman);
    let image_two = rgba8_png_fixture([30, 144, 255, 255], PngDeflateEncoding::FixedHuffman);
    assert_eq!(decode_rgba8_png_fixture(&image_one), [220, 20, 60, 255]);
    assert_eq!(decode_rgba8_png_fixture(&image_two), [30, 144, 255, 255]);

    let paragraph_note = text_only_note_docx(&note_image_docx(
        NoteKind::Footnote,
        "image1.png",
        &image_one,
        &image_two,
    ));
    let table_note = note_paragraph_in_table(&paragraph_note, NoteKind::Footnote);
    assert_only_part_changed(&paragraph_note, &table_note, "word/footnotes.xml");

    let paragraph = DocxAdapter
        .inspect(&paragraph_note, &InspectionProfile::default())
        .expect("plain-text note paragraph remains supported");
    match DocxAdapter.inspect(&table_note, &InspectionProfile::default()) {
        Ok(table) => assert_ne!(
            semantic_fingerprint(&paragraph.semantic_projection),
            semantic_fingerprint(&table.semantic_projection),
            "footnote table structure changed while the semantic fingerprint stayed equal"
        ),
        Err(error) => assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct),
    }
}

#[test]
fn note_run_boundary_does_not_insert_visible_whitespace() {
    let image_one = rgba8_png_fixture([220, 20, 60, 255], PngDeflateEncoding::FixedHuffman);
    let image_two = rgba8_png_fixture([30, 144, 255, 255], PngDeflateEncoding::FixedHuffman);
    let spaced = text_only_note_docx(&note_image_docx(
        NoteKind::Footnote,
        "image1.png",
        &image_one,
        &image_two,
    ));
    let mut parts = read_parts(&spaced);
    let note_path = "word/footnotes.xml";
    let note_xml = String::from_utf8(parts[note_path].clone()).expect("note XML");
    assert!(note_xml.contains("<w:t>Note text</w:t>"));
    parts.insert(
        note_path.to_owned(),
        note_xml
            .replacen(
                "<w:t>Note text</w:t>",
                "<w:t>Note</w:t></w:r><w:r><w:t>text</w:t>",
                1,
            )
            .into_bytes(),
    );
    let joined = write_parts(parts.into_iter().collect());
    assert_only_part_changed(&spaced, &joined, note_path);

    let spaced = DocxAdapter
        .inspect(&spaced, &InspectionProfile::default())
        .expect("valid spaced note");
    let joined = DocxAdapter
        .inspect(&joined, &InspectionProfile::default())
        .expect("valid adjacent note runs");
    assert_ne!(
        semantic_fingerprint(&spaced.semantic_projection),
        semantic_fingerprint(&joined.semantic_projection),
        "run boundaries must not synthesize visible spaces in note text"
    );
}

#[test]
fn note_revision_is_editorial_or_explicitly_unsupported() {
    let image_one = rgba8_png_fixture([220, 20, 60, 255], PngDeflateEncoding::FixedHuffman);
    let image_two = rgba8_png_fixture([30, 144, 255, 255], PngDeflateEncoding::FixedHuffman);
    let plain = text_only_note_docx(&note_image_docx(
        NoteKind::Footnote,
        "image1.png",
        &image_one,
        &image_two,
    ));
    let mut parts = read_parts(&plain);
    let note_path = "word/footnotes.xml";
    let note_xml = String::from_utf8(parts[note_path].clone()).expect("note XML");
    assert!(note_xml.contains("<w:r><w:t>Note text</w:t></w:r>"));
    parts.insert(
        note_path.to_owned(),
        note_xml
            .replacen(
                "<w:r><w:t>Note text</w:t></w:r>",
                "<w:ins w:id=\"17\" w:author=\"Test\"><w:r><w:t>Note text</w:t></w:r></w:ins>",
                1,
            )
            .into_bytes(),
    );
    let revision = write_parts(parts.into_iter().collect());
    assert_only_part_changed(&plain, &revision, note_path);

    match DocxAdapter.inspect(&revision, &InspectionProfile::default()) {
        Ok(output) => assert!(
            output.editorial.tracked_changes_present,
            "an accepted note revision must retain tracked-change evidence"
        ),
        Err(error) => assert_eq!(error.code(), ErrorCode::UnsupportedSemanticConstruct),
    }
}

fn assert_note_image_target_is_semantic(kind: NoteKind) {
    let image_one = rgba8_png_fixture([220, 20, 60, 255], PngDeflateEncoding::FixedHuffman);
    let image_two = rgba8_png_fixture([30, 144, 255, 255], PngDeflateEncoding::FixedHuffman);
    assert_eq!(decode_rgba8_png_fixture(&image_one), [220, 20, 60, 255]);
    assert_eq!(decode_rgba8_png_fixture(&image_two), [30, 144, 255, 255]);

    let baseline = note_image_docx(kind, "image1.png", &image_one, &image_two);
    let swapped = note_image_docx(kind, "image2.png", &image_one, &image_two);
    assert_only_note_image_target_changed(&baseline, &swapped, kind);
    assert_docx_image_reference_chain(&baseline, kind, "image1.png");
    assert_docx_image_reference_chain(&swapped, kind, "image2.png");

    assert_semantics_distinguish_or_explicitly_reject(
        &baseline,
        &swapped,
        &format!("{} image target", kind.part_name()),
    );
}

fn assert_semantics_distinguish_or_explicitly_reject(
    baseline: &[u8],
    changed: &[u8],
    description: &str,
) {
    let baseline_result = DocxAdapter.inspect(baseline, &InspectionProfile::default());
    let changed_result = DocxAdapter.inspect(changed, &InspectionProfile::default());
    match (baseline_result, changed_result) {
        (Ok(baseline), Ok(changed)) => assert_ne!(
            semantic_fingerprint(&baseline.semantic_projection),
            semantic_fingerprint(&changed.semantic_projection),
            "{description} changed while the semantic fingerprint stayed equal"
        ),
        (Err(baseline), Err(changed))
            if baseline.code() == ErrorCode::UnsupportedSemanticConstruct
                && changed.code() == ErrorCode::UnsupportedSemanticConstruct => {}
        (baseline, changed) => panic!(
            "{description} packages must either differ semantically or both fail closed as UnsupportedSemanticConstruct; baseline={:?}, changed={:?}",
            error_summary(baseline),
            error_summary(changed)
        ),
    }
}

fn error_summary<T>(result: Result<T, PocError>) -> Result<(), (ErrorCode, String)> {
    result
        .map(|_| ())
        .map_err(|error| (error.code(), error.to_string()))
}

fn note_image_docx(kind: NoteKind, target: &str, image_one: &[u8], image_two: &[u8]) -> Vec<u8> {
    let note_part = kind.part_name();
    let note_path = format!("word/{note_part}.xml");
    let note_rels_path = format!("word/_rels/{note_part}.xml.rels");
    let note_relationship_type = format!("{OFFICE_RELATIONSHIPS}/{}", kind.note_relationship());

    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="png" ContentType="image/png"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/{note_path}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{note_part}+xml"/>
</Types>"#
    );
    let root_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_RELATIONSHIPS}">
<Relationship Id="rIdOffice" Type="{OFFICE_RELATIONSHIPS}/officeDocument" Target="word/document.xml"/>
</Relationships>"#
    );
    let document_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_RELATIONSHIPS}">
<Relationship Id="rIdNotes" Type="{note_relationship_type}" Target="{note_part}.xml"/>
</Relationships>"#
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORDPROCESSINGML}" xmlns:r="{OFFICE_RELATIONSHIPS}">
<w:body><w:p><w:r><w:t>Body text</w:t></w:r><w:r><w:{reference} w:id="1"/></w:r></w:p><w:sectPr/></w:body>
</w:document>"#,
        reference = kind.reference_name()
    );
    let notes = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:{note_part} xmlns:w="{WORDPROCESSINGML}" xmlns:r="{OFFICE_RELATIONSHIPS}" xmlns:wp="{WORDPROCESSING_DRAWING}" xmlns:a="{DRAWINGML}" xmlns:pic="{PICTURE}">
<w:{note} w:id="1"><w:p><w:r><w:t>Note text</w:t></w:r><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="note image"/><a:graphic><a:graphicData uri="{PICTURE}"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="note image"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdNoteImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:{note}>
</w:{note_part}>"#,
        note = match kind {
            NoteKind::Footnote => "footnote",
            NoteKind::Endnote => "endnote",
        }
    );
    let note_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_RELATIONSHIPS}">
<Relationship Id="rIdNoteImage" Type="{OFFICE_RELATIONSHIPS}/image" Target="media/{target}"/>
</Relationships>"#
    );

    write_parts(vec![
        ("[Content_Types].xml".into(), content_types.into_bytes()),
        ("_rels/.rels".into(), root_relationships.into_bytes()),
        ("word/document.xml".into(), document.into_bytes()),
        (
            "word/_rels/document.xml.rels".into(),
            document_relationships.into_bytes(),
        ),
        (note_path, notes.into_bytes()),
        (note_rels_path, note_relationships.into_bytes()),
        ("word/media/image1.png".into(), image_one.to_vec()),
        ("word/media/image2.png".into(), image_two.to_vec()),
    ])
}

fn assert_only_note_image_target_changed(baseline: &[u8], swapped: &[u8], kind: NoteKind) {
    let note_rels = format!("word/_rels/{}.xml.rels", kind.part_name());
    let baseline_parts = read_parts(baseline);
    let swapped_parts = read_parts(swapped);
    assert_eq!(
        baseline_parts.keys().collect::<Vec<_>>(),
        swapped_parts.keys().collect::<Vec<_>>()
    );
    for (name, baseline_part) in &baseline_parts {
        let swapped_part = &swapped_parts[name];
        if name == &note_rels {
            let baseline_relationships = String::from_utf8_lossy(baseline_part);
            let swapped_relationships = String::from_utf8_lossy(swapped_part);
            assert!(baseline_relationships.contains("Target=\"media/image1.png\""));
            assert!(swapped_relationships.contains("Target=\"media/image2.png\""));
            assert_eq!(
                baseline_relationships.replace("image1.png", "image2.png"),
                swapped_relationships
            );
        } else {
            assert_eq!(
                baseline_part, swapped_part,
                "unexpected part change: {name}"
            );
        }
    }
}

fn assert_only_part_changed(baseline: &[u8], changed: &[u8], expected_name: &str) {
    let baseline_parts = read_parts(baseline);
    let changed_parts = read_parts(changed);
    assert_eq!(
        baseline_parts.keys().collect::<Vec<_>>(),
        changed_parts.keys().collect::<Vec<_>>()
    );
    for (name, baseline_part) in &baseline_parts {
        if name == expected_name {
            assert_ne!(baseline_part, &changed_parts[name]);
        } else {
            assert_eq!(
                baseline_part, &changed_parts[name],
                "unexpected part change: {name}"
            );
        }
    }
}

fn note_paragraph_in_table(input: &[u8], kind: NoteKind) -> Vec<u8> {
    let note_path = format!("word/{}.xml", kind.part_name());
    let mut parts = read_parts(input);
    let note_xml = String::from_utf8(parts[&note_path].clone()).expect("note XML");
    let note_name = match kind {
        NoteKind::Footnote => "footnote",
        NoteKind::Endnote => "endnote",
    };
    let note_start = format!("<w:{note_name} w:id=\"1\">");
    let note_end = format!("</w:{note_name}>");
    let paragraph = "<w:p>";
    let paragraph_end = "</w:p>";
    let content_start =
        note_xml.find(&note_start).expect("note start") + note_start.len() + paragraph.len();
    let content_end = note_xml[content_start..]
        .find(paragraph_end)
        .map(|offset| content_start + offset)
        .expect("note paragraph end");
    let content = &note_xml[content_start..content_end];
    let table = format!(
        "{note_start}<w:tbl><w:tr><w:tc><w:p>{content}</w:p></w:tc></w:tr></w:tbl>{note_end}"
    );
    let replacement = format!("{note_start}{paragraph}{content}{paragraph_end}{note_end}");
    parts.insert(
        note_path,
        note_xml.replacen(&replacement, &table, 1).into_bytes(),
    );
    write_parts(parts.into_iter().collect())
}

fn text_only_note_docx(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    let note_path = "word/footnotes.xml";
    let note_xml = String::from_utf8(parts[note_path].clone()).expect("note XML");
    let drawing_start = note_xml.find("<w:r><w:drawing>").expect("drawing run");
    let drawing_end = note_xml[drawing_start..]
        .find("</w:drawing></w:r>")
        .map(|offset| drawing_start + offset + "</w:drawing></w:r>".len())
        .expect("drawing run end");
    let mut text_only = note_xml;
    text_only.replace_range(drawing_start..drawing_end, "");
    parts.insert(note_path.to_owned(), text_only.into_bytes());
    parts.remove("word/_rels/footnotes.xml.rels");
    parts.remove("word/media/image1.png");
    parts.remove("word/media/image2.png");
    write_parts(parts.into_iter().collect())
}

fn assert_docx_image_reference_chain(bytes: &[u8], kind: NoteKind, target: &str) {
    let parts = read_parts(bytes);
    let note_part = kind.part_name();
    let reference = kind.reference_name();
    let note_xml = String::from_utf8_lossy(&parts[&format!("word/{note_part}.xml")]);
    let document_xml = String::from_utf8_lossy(&parts["word/document.xml"]);
    let note_rels = String::from_utf8_lossy(&parts[&format!("word/_rels/{note_part}.xml.rels")]);

    assert!(document_xml.contains(&format!("<w:{reference} w:id=\"1\"/>")));
    assert!(note_xml.contains("<a:blip r:embed=\"rIdNoteImage\"/>"));
    assert!(note_rels.contains(&format!("Target=\"media/{target}\"")));
    assert!(parts.contains_key(&format!("word/media/{target}")));
}

fn read_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("valid DOCX ZIP");
    let mut parts = std::collections::BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("valid ZIP part");
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).expect("read ZIP part");
        parts.insert(entry.name().to_owned(), contents);
    }
    parts
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer.start_file(name, options).expect("start DOCX part");
        writer.write_all(&data).expect("write DOCX part");
    }
    writer.finish().expect("finish DOCX package").into_inner()
}
