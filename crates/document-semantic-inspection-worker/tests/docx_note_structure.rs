use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, WorkerFailure,
    WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const OFFICE_REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const MAIN_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const ROOT_RELATIONSHIPS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const FOOTNOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const ENDNOTES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";

#[test]
fn referenced_footnote_and_endnote_paragraph_vs_table_is_semantic_or_explicitly_unsupported() {
    for kind in [NoteKind::Footnote, NoteKind::Endnote] {
        let paragraph = note_structure_docx(kind, NoteStructure::Paragraph);
        let table = note_structure_docx(kind, NoteStructure::Table);

        // The packages have no media parts; only the referenced note block shape changes.
        assert_ne!(paragraph, table, "{kind:?} fixture structure must differ");
        assert_structure_change_is_detected_or_rejected(&paragraph, &table, kind.name());
    }
}

fn assert_structure_change_is_detected_or_rejected(
    paragraph: &[u8],
    table: &[u8],
    note_name: &str,
) {
    let paragraph_output = inspect_sentinel_valid_docx(paragraph)
        .expect("plain-text note paragraph remains supported");
    let table_output = inspect_sentinel_valid_docx(table);

    match table_output {
        Ok(table) => assert_ne!(
            paragraph_output.semantic_fingerprint(),
            table.semantic_fingerprint(),
            "referenced {note_name} paragraph and table structures cannot share a successful fingerprint",
        ),
        Err(table) => {
            assert_eq!(
                table.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "the table form must fail with an explicit unsupported-construct result",
            );
        }
    }
}

fn inspect_sentinel_valid_docx(
    bytes: &[u8],
) -> Result<document_semantic_inspection_worker::SemanticAdapterOutput, WorkerFailure> {
    OoxmlCoverageSentinel::validate_package(bytes)
        .expect("synthetic DOCX passes the OOXML coverage sentinel");
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn note_structure_docx(kind: NoteKind, structure: NoteStructure) -> Vec<u8> {
    let document = format!(
        r#"<w:document xmlns:w="{WORD_NS}" xmlns:r="{OFFICE_REL_NS}"><w:body><w:p><w:r><w:t>Stable body</w:t></w:r><w:r><w:{reference} w:id="1"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        reference = kind.reference_element(),
    );
    let note_part = format!(
        r#"<w:{root} xmlns:w="{WORD_NS}">
<w:{element} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{element}>
<w:{element} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{element}>
<w:{element} w:id="1">{content}</w:{element}>
</w:{root}>"#,
        root = kind.part_name(),
        element = kind.note_element(),
        content = structure.content(),
    );
    let document_relationships = format!(
        r#"<Relationships xmlns="{PACKAGE_REL_NS}"><Relationship Id="rIdNotes" Type="{OFFICE_REL_NS}/{relationship_type}" Target="{part_name}.xml"/></Relationships>"#,
        relationship_type = kind.part_name(),
        part_name = kind.part_name(),
    );
    let note_content_type = kind.content_type();
    let content_types = format!(
        r#"<Types xmlns="{CONTENT_TYPES_NS}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="{MAIN_CONTENT_TYPE}"/><Override PartName="/word/{part_name}.xml" ContentType="{note_content_type}"/></Types>"#,
        part_name = kind.part_name(),
    );
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
        (
            format!("word/{}.xml", kind.part_name()),
            note_part.into_bytes(),
        ),
    ])
}

fn stored_docx(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
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

#[derive(Debug, Clone, Copy)]
enum NoteKind {
    Footnote,
    Endnote,
}

impl NoteKind {
    fn name(self) -> &'static str {
        match self {
            Self::Footnote => "footnote",
            Self::Endnote => "endnote",
        }
    }

    fn part_name(self) -> &'static str {
        match self {
            Self::Footnote => "footnotes",
            Self::Endnote => "endnotes",
        }
    }

    fn note_element(self) -> &'static str {
        self.name()
    }

    fn reference_element(self) -> &'static str {
        match self {
            Self::Footnote => "footnoteReference",
            Self::Endnote => "endnoteReference",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Footnote => FOOTNOTES_CONTENT_TYPE,
            Self::Endnote => ENDNOTES_CONTENT_TYPE,
        }
    }
}

#[derive(Clone, Copy)]
enum NoteStructure {
    Paragraph,
    Table,
}

impl NoteStructure {
    fn content(self) -> &'static str {
        match self {
            Self::Paragraph => r#"<w:p><w:r><w:t>Stable note text</w:t></w:r></w:p>"#,
            Self::Table => {
                r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>Stable note text</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#
            }
        }
    }
}
