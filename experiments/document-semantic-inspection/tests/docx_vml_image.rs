mod support;

use std::io::{Cursor, Write};

use document_semantic_inspection_poc::{
    AdapterOutput, DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile,
    fingerprint as semantic_fingerprint,
};
use support::ooxml::{PngDeflateEncoding, decode_rgba8_png_fixture, rgba8_png_fixture};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="png" ContentType="image/png"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/vml-image.png"/>
</Relationships>"#;

fn vml_image_docx(png: &[u8]) -> Vec<u8> {
    let document = br##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:v="urn:schemas-microsoft-com:vml"
 xmlns:o="urn:schemas-microsoft-com:office:office">
<w:body>
<w:p><w:r><w:t>same visible text</w:t></w:r></w:p>
<w:p><w:r><w:pict>
<v:shapetype id="_x0000_t75" coordsize="21600,21600" o:spt="75" o:preferrelative="t" path="m@4@5l@4@11@9@11@9@5xe">
<v:stroke joinstyle="miter"/>
<v:formulas><v:f eqn="if lineDrawn pixelLineWidth 0"/><v:f eqn="sum @0 1 0"/><v:f eqn="sum 0 0 @1"/><v:f eqn="prod @2 1 2"/><v:f eqn="prod @3 21600 pixelWidth"/><v:f eqn="prod @3 21600 pixelHeight"/><v:f eqn="sum @0 0 1"/><v:f eqn="prod @6 1 2"/><v:f eqn="prod @7 21600 pixelWidth"/><v:f eqn="sum @8 21600 0"/><v:f eqn="prod @7 21600 pixelHeight"/><v:f eqn="sum @10 21600 0"/></v:formulas>
<v:path o:extrusionok="f" gradientshapeok="t" o:connecttype="rect"/>
<o:lock v:ext="edit" aspectratio="t"/>
</v:shapetype>
<v:shape id="_x0000_i1025" type="#_x0000_t75" style="width:1pt;height:1pt">
<v:imagedata r:id="rIdImage" o:title="same alternative text"/>
</v:shape>
</w:pict></w:r></w:p>
<w:sectPr/>
</w:body></w:document>"##;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, bytes) in [
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("word/document.xml", document.as_slice()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS.as_bytes()),
        ("word/media/vml-image.png", png),
    ] {
        writer
            .start_file(name, options)
            .expect("start DOCX package part");
        writer.write_all(bytes).expect("write DOCX package part");
    }
    writer.finish().expect("finish DOCX package").into_inner()
}

fn plain_text(output: &AdapterOutput) -> String {
    serde_json::from_slice::<serde_json::Value>(&output.semantic_projection)
        .expect("semantic projection JSON")["plain_text"]
        .as_str()
        .expect("plain_text string")
        .to_owned()
}

#[test]
fn vml_embedded_image_semantics_are_fingerprinted_or_rejected() {
    let profile = InspectionProfile::default();
    let baseline = support::ooxml::docx_fixture("same visible text");
    let baseline_output = DocxAdapter
        .inspect(&baseline, &profile)
        .expect("positive baseline DOCX inspection");
    assert_eq!(plain_text(&baseline_output), "same visible text");

    let first_png = rgba8_png_fixture([19, 77, 141, 255], PngDeflateEncoding::Stored);
    let second_png = rgba8_png_fixture([19, 77, 142, 255], PngDeflateEncoding::Stored);
    assert_eq!(decode_rgba8_png_fixture(&first_png), [19, 77, 141, 255]);
    assert_eq!(decode_rgba8_png_fixture(&second_png), [19, 77, 142, 255]);

    let first = vml_image_docx(&first_png);
    let second = vml_image_docx(&second_png);
    let first_result = DocxAdapter.inspect(&first, &profile);
    let second_result = DocxAdapter.inspect(&second, &profile);

    match (first_result, second_result) {
        (Ok(first), Ok(second)) => {
            assert_eq!(plain_text(&first), plain_text(&second));
            assert_eq!(plain_text(&first), plain_text(&baseline_output));
            assert_ne!(
                semantic_fingerprint(&first.semantic_projection),
                semantic_fingerprint(&second.semantic_projection),
                "different VML-embedded image pixels must change DOCX semantic identity"
            );
        }
        (Err(first), Err(second))
            if first.code() == ErrorCode::UnsupportedSemanticConstruct
                && second.code() == ErrorCode::UnsupportedSemanticConstruct =>
        {
            // The adapter explicitly rejects VML image semantics instead of ignoring them.
        }
        (first, second) => panic!(
            "VML image handling must fingerprint both images or explicitly reject both; first={first:?}, second={second:?}"
        ),
    }
}
