use std::io::{Cursor, Write};

use document_semantic_inspection_worker::{
    AdapterProfile, DocxAdapter, OoxmlCoverageSentinel, SemanticAdapter, SemanticAdapterOutput,
    WorkerFailure, WorkerFailureCode,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const ROOT_RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

#[test]
fn selected_header_external_hyperlink_target_changes_identity_or_fails_closed() {
    let baseline_package = header_hyperlink_docx("https://a.example/reference");
    let changed_package = header_hyperlink_docx("https://b.example/reference");

    OoxmlCoverageSentinel::validate_package(&baseline_package)
        .expect("the baseline is a structurally valid DOCX package");
    OoxmlCoverageSentinel::validate_package(&changed_package)
        .expect("the changed target remains a structurally valid DOCX package");

    let baseline = inspect(&baseline_package);
    let changed = inspect(&changed_package);

    match (&baseline, &changed) {
        (Ok(baseline), Ok(changed)) => assert_ne!(
            baseline.semantic_fingerprint(),
            changed.semantic_fingerprint(),
            "changing only the selected header hyperlink target must change semantic identity",
        ),
        (Ok(_), Err(changed)) => assert_eq!(
            changed.code(),
            WorkerFailureCode::UnsupportedSemanticConstruct,
            "an unmodeled changed hyperlink target must fail closed as unsupported",
        ),
        (Err(baseline), Err(changed)) => {
            assert_eq!(
                baseline.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "a valid but unmodeled baseline hyperlink must fail closed as unsupported",
            );
            assert_eq!(
                changed.code(),
                WorkerFailureCode::UnsupportedSemanticConstruct,
                "an unmodeled changed hyperlink target must fail closed as unsupported",
            );
        }
        (Err(baseline), Ok(_)) => panic!(
            "the changed target was accepted while the valid baseline was rejected: {baseline}"
        ),
    }
}

fn inspect(bytes: &[u8]) -> Result<SemanticAdapterOutput, WorkerFailure> {
    DocxAdapter.inspect(bytes, &AdapterProfile::default())
}

fn header_hyperlink_docx(target: &str) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}"><w:body>
<w:p><w:r><w:t>Stable body text</w:t></w:r></w:p>
<w:sectPr>
<w:headerReference w:type="default" r:id="rIdHeader"/>
<w:pgSz w:w="12240" w:h="15840"/>
</w:sectPr>
</w:body></w:document>"#
    );
    let document_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}">
<Relationship Id="rIdHeader" Type="{REL_NS}/header" Target="header1.xml"/>
</Relationships>"#
    );
    let header = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr xmlns:w="{WORD_NS}" xmlns:r="{REL_NS}">
<w:p><w:hyperlink r:id="rIdHeaderLink"><w:r><w:t>Open reference</w:t></w:r></w:hyperlink></w:p>
</w:hdr>"#
    );
    let header_relationships = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PACKAGE_REL_NS}">
<Relationship Id="rIdHeaderLink" Type="{REL_NS}/hyperlink" Target="{target}" TargetMode="External"/>
</Relationships>"#
    );
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{CONTENT_TYPES_NS}">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
</Types>"#
    );

    let parts = [
        ("[Content_Types].xml", content_types.into_bytes()),
        ("_rels/.rels", ROOT_RELATIONSHIPS.as_bytes().to_vec()),
        ("word/document.xml", document.into_bytes()),
        (
            "word/_rels/document.xml.rels",
            document_relationships.into_bytes(),
        ),
        ("word/header1.xml", header.into_bytes()),
        (
            "word/_rels/header1.xml.rels",
            header_relationships.into_bytes(),
        ),
    ];
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
