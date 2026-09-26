use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailure, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const OFFICE_REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const FOOTNOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const ENDNOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const ROOT_RELATIONSHIPS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

#[test]
fn referenced_footnote_image_pixel_change_is_semantic_or_fail_closed() {
    assert_note_image_pixel_change_is_semantic_or_fail_closed(NoteKind::Footnote);
}

#[test]
fn referenced_endnote_image_pixel_change_is_semantic_or_fail_closed() {
    assert_note_image_pixel_change_is_semantic_or_fail_closed(NoteKind::Endnote);
}

#[test]
fn unreferenced_note_image_pixel_change_is_noise_or_fail_closed() {
    for kind in [NoteKind::Footnote, NoteKind::Endnote] {
        let referenced = [255, 0, 0, 255];
        let orphan_before = [0, 0, 255, 255];
        let orphan_after = [0, 255, 0, 255];
        let baseline = note_image_docx(kind, "media/image-one.png", referenced, orphan_before);
        let changed_orphan = note_image_docx(kind, "media/image-one.png", referenced, orphan_after);

        assert_same_or_fail_closed(&baseline, &changed_orphan, kind.description());
    }
}

fn assert_note_image_pixel_change_is_semantic_or_fail_closed(kind: NoteKind) {
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];

    // The relationship target stays fixed, and the same two decoded pixels are
    // present in both packages; only which pixel lives at the target part changes.
    let baseline = note_image_docx(kind, "media/image-one.png", red, blue);
    let same_target_pixel_changed = note_image_docx(kind, "media/image-one.png", blue, red);
    let same_target_change_detected = semantic_change_is_detected_or_unsupported(
        &baseline,
        &same_target_pixel_changed,
        kind.description(),
    );

    // Keep every media part byte-for-byte identical and change only the note's
    // relationship edge. This prevents a blanket media-part hash from masking
    // an adapter that never projects note image references.
    let changed_reference = note_image_docx(kind, "media/image-two.png", red, blue);
    let reference_change_detected = semantic_change_is_detected_or_unsupported(
        &baseline,
        &changed_reference,
        kind.description(),
    );
    assert!(
        same_target_change_detected && reference_change_detected,
        "{kind:?}: same-target pixel change detected={same_target_change_detected}; relationship target change detected={reference_change_detected}",
    );
}

fn semantic_change_is_detected_or_unsupported(
    first: &[u8],
    changed: &[u8],
    note_name: &str,
) -> bool {
    let first_output = inspect_valid(first);
    let changed_output = inspect_valid(changed);

    match (first_output, changed_output) {
        (Ok(first), Ok(changed)) => first.semantic_fingerprint() != changed.semantic_fingerprint(),
        (Err(first), Err(changed)) => {
            assert_eq!(
                first.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the first {note_name} image must be explicitly unsupported",
            );
            assert_eq!(
                changed.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the changed {note_name} image must be explicitly unsupported",
            );
            true
        }
        (first, changed) => panic!(
            "{note_name} images must both inspect distinctly or both fail explicitly: {first:?}, {changed:?}"
        ),
    }
}

fn assert_same_or_fail_closed(first: &[u8], changed: &[u8], note_name: &str) {
    let first_output = inspect_valid(first);
    let changed_output = inspect_valid(changed);

    match (first_output, changed_output) {
        (Ok(first), Ok(changed)) => assert_eq!(
            first.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "an unreferenced image cannot change {note_name} semantic identity",
        ),
        (Err(first), Err(changed)) => {
            assert_eq!(
                first.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the first {note_name} package must fail explicitly if unsupported",
            );
            assert_eq!(
                changed.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the changed {note_name} package must fail explicitly if unsupported",
            );
        }
        (first, changed) => panic!(
            "{note_name} packages must both inspect or both fail explicitly: {first:?}, {changed:?}"
        ),
    }
}

fn inspect_valid(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes the OOXML coverage sentinel");
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn note_image_docx(
    kind: NoteKind,
    relationship_target: &str,
    image_one: [u8; 4],
    image_two: [u8; 4],
) -> Vec<u8> {
    let document = format!(
        r#"<w:document xmlns:w="{WORD_NS}" xmlns:r="{OFFICE_REL_NS}"><w:body><w:p><w:r><w:t>Stable body</w:t></w:r><w:r><w:{reference} w:id="1"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        reference = kind.reference_element(),
    );
    let note_xml = format!(
        r#"<w:{root} xmlns:w="{WORD_NS}" xmlns:r="{OFFICE_REL_NS}" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
<w:{element} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{element}>
<w:{element} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{element}>
<w:{element} w:id="1"><w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="note image"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="note image"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdNoteImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:{element}>
</w:{root}>"#,
        root = kind.root_element(),
        element = kind.note_element(),
    );
    let document_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdNote" Type="{OFFICE_REL_NS}/{relationship_type}" Target="{part_name}.xml"/></Relationships>"#,
        relationship_type = kind.relationship_type(),
        part_name = kind.part_name(),
    );
    let note_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdNoteImage" Type="{OFFICE_REL_NS}/image" Target="{relationship_target}"/></Relationships>"#
    );
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="{MAIN_CONTENT_TYPE}"/><Override PartName="/word/{part_name}.xml" ContentType="{note_content_type}"/></Types>"#,
        part_name = kind.part_name(),
        note_content_type = kind.content_type(),
    );
    let note_part_name = format!("word/{}.xml", kind.part_name());
    let note_relationships_name = format!("word/_rels/{}.xml.rels", kind.part_name());

    stored_docx(vec![
        ("[Content_Types].xml".to_owned(), content_types.into_bytes()),
        (
            "_rels/.rels".to_owned(),
            ROOT_RELATIONSHIPS.as_bytes().to_vec(),
        ),
        ("word/document.xml".to_owned(), document.into_bytes()),
        (
            "word/_rels/document.xml.rels".to_owned(),
            document_relationships.into_bytes(),
        ),
        (note_part_name, note_xml.into_bytes()),
        (note_relationships_name, note_relationships.into_bytes()),
        ("word/media/image-one.png".to_owned(), tiny_png(image_one)),
        ("word/media/image-two.png".to_owned(), tiny_png(image_two)),
    ])
}

#[derive(Debug, Clone, Copy)]
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

    fn root_element(self) -> &'static str {
        self.part_name()
    }

    fn note_element(self) -> &'static str {
        match self {
            Self::Footnote => "footnote",
            Self::Endnote => "endnote",
        }
    }

    fn reference_element(self) -> &'static str {
        match self {
            Self::Footnote => "footnoteReference",
            Self::Endnote => "endnoteReference",
        }
    }

    fn relationship_type(self) -> &'static str {
        self.part_name()
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Footnote => FOOTNOTES_CONTENT_TYPE,
            Self::Endnote => ENDNOTES_CONTENT_TYPE,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Footnote => "footnote",
            Self::Endnote => "endnote",
        }
    }
}

fn stored_docx(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, contents) in parts {
        writer
            .start_file(name, options)
            .expect("start synthetic DOCX part");
        writer
            .write_all(&contents)
            .expect("write synthetic DOCX part");
    }
    writer
        .finish()
        .expect("finish synthetic DOCX ZIP")
        .into_inner()
}

fn tiny_png(pixel: [u8; 4]) -> Vec<u8> {
    let scanline = [0, pixel[0], pixel[1], pixel[2], pixel[3]];
    let mut zlib = vec![0x78, 0x01, 0x01];
    let length = u16::try_from(scanline.len()).expect("PNG scanline length");
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&scanline);
    zlib.extend_from_slice(&adler32(&scanline).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    append_png_chunk(&mut png, *b"IHDR", &ihdr);
    append_png_chunk(&mut png, *b"IDAT", &zlib);
    append_png_chunk(&mut png, *b"IEND", &[]);
    png
}

fn append_png_chunk(output: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(data.len())
            .expect("PNG chunk size")
            .to_be_bytes(),
    );
    output.extend_from_slice(&kind);
    output.extend_from_slice(data);
    let mut checksum_input = kind.to_vec();
    checksum_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&checksum_input).to_be_bytes());
}

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let (mut first, mut second) = (1u32, 0u32);
    for byte in bytes {
        first = (first + u32::from(*byte)) % MODULUS;
        second = (second + first) % MODULUS;
    }
    (second << 16) | first
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
