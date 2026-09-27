mod support;

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use document_semantic_inspection_poc::{
    AdapterOutput, DocxAdapter, ErrorCode, InspectionAdapter, InspectionProfile, PocError,
    fingerprint,
};
use support::ooxml::{
    PngDeflateEncoding, decode_rgba8_png_fixture, docx_fixture, rgba8_png_fixture,
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

#[test]
fn changing_a_referenced_header_image_cannot_be_silently_ignored() {
    let before =
        docx_with_header_footer_targets("media/header-image.png", "media/footer-image.png");
    let after = docx_with_header_footer_targets("media/footer-image.png", "media/footer-image.png");
    assert_docx_has_distinct_visible_image_assets(&before);
    assert_only_relationship_target_differs(
        &before,
        &after,
        "word/_rels/header1.xml.rels",
        "media/header-image.png",
        "media/footer-image.png",
    );

    assert_fingerprints_distinguish_or_fail_closed(
        &before,
        &after,
        "referenced header image pixels",
    );
}

#[test]
fn changing_a_referenced_footer_image_cannot_be_silently_ignored() {
    let before =
        docx_with_header_footer_targets("media/header-image.png", "media/footer-image.png");
    let after = docx_with_header_footer_targets("media/header-image.png", "media/header-image.png");
    assert_docx_has_distinct_visible_image_assets(&before);
    assert_only_relationship_target_differs(
        &before,
        &after,
        "word/_rels/footer1.xml.rels",
        "media/footer-image.png",
        "media/header-image.png",
    );

    assert_fingerprints_distinguish_or_fail_closed(
        &before,
        &after,
        "referenced footer image pixels",
    );
}

fn assert_fingerprints_distinguish_or_fail_closed(before: &[u8], after: &[u8], context: &str) {
    let before_result = inspect_docx(before);
    let after_result = inspect_docx(after);
    match (before_result, after_result) {
        (Ok(before), Ok(after)) => assert_ne!(
            fingerprint(&before.semantic_projection),
            fingerprint(&after.semantic_projection),
            "{context} must change semantic identity"
        ),
        (Err(before), Err(after))
            if before.code() == ErrorCode::UnsupportedSemanticConstruct
                && after.code() == ErrorCode::UnsupportedSemanticConstruct => {}
        (before, after) => panic!(
            "{context} must be represented in the fingerprint or rejected explicitly; before={before:?}, after={after:?}"
        ),
    }
}

fn inspect_docx(bytes: &[u8]) -> Result<AdapterOutput, PocError> {
    DocxAdapter.inspect(bytes, &InspectionProfile::default())
}

fn docx_with_header_footer_targets(header_target: &str, footer_target: &str) -> Vec<u8> {
    let mut parts = read_parts(&docx_fixture("same body text"));

    let document = String::from_utf8(
        parts
            .get("word/document.xml")
            .expect("document part")
            .clone(),
    )
    .expect("document XML");
    let document = document.replacen(
        "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"",
        &format!("xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\""),
        1,
    );
    assert_ne!(
        document,
        String::from_utf8(parts["word/document.xml"].clone()).unwrap()
    );
    let document = document.replace(
        "</w:body>",
        "<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/><w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/></w:sectPr></w:body>",
    );
    assert!(document.contains("r:id=\"rIdHeader\""));
    assert!(document.contains("r:id=\"rIdFooter\""));
    parts.insert("word/document.xml".into(), document.into_bytes());

    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).expect("types XML");
    let content_types = content_types.replace(
        "</Types>",
        "<Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/><Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/></Types>",
    );
    assert!(content_types.contains("/word/header1.xml"));
    assert!(content_types.contains("/word/footer1.xml"));
    parts.insert("[Content_Types].xml".into(), content_types.into_bytes());

    parts.insert(
        "word/_rels/document.xml.rels".into(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdHeader\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/><Relationship Id=\"rIdFooter\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer\" Target=\"footer1.xml\"/></Relationships>"
            .to_owned()
            .into_bytes(),
    );
    parts.insert(
        "word/header1.xml".into(),
        picture_part("hdr", "Header image", "header-image.png", 1),
    );
    parts.insert(
        "word/footer1.xml".into(),
        picture_part("ftr", "Footer image", "footer-image.png", 2),
    );
    parts.insert(
        "word/_rels/header1.xml.rels".into(),
        image_relationships(header_target),
    );
    parts.insert(
        "word/_rels/footer1.xml.rels".into(),
        image_relationships(footer_target),
    );
    parts.insert(
        "word/media/header-image.png".into(),
        rgba8_png_fixture([220, 24, 32, 255], PngDeflateEncoding::Stored),
    );
    parts.insert(
        "word/media/footer-image.png".into(),
        rgba8_png_fixture([24, 32, 220, 255], PngDeflateEncoding::Stored),
    );

    write_parts(parts)
}

fn picture_part(root: &str, name: &str, image_name: &str, id: u32) -> Vec<u8> {
    let root_element = match root {
        "hdr" | "ftr" => root,
        _ => panic!("unexpected Word part root {root}"),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root_element} xmlns:w="{W_NS}" xmlns:r="{R_NS}" xmlns:wp="{WP_NS}" xmlns:a="{A_NS}" xmlns:pic="{PIC_NS}"><w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="914400" cy="914400"/><wp:docPr id="{id}" name="{name}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="{PIC_NS}"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="{image_name}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:{root_element}>"#
    )
    .into_bytes()
}

fn image_relationships(target: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdImage\" Type=\"{IMAGE_REL}\" Target=\"{target}\"/></Relationships>"
    )
    .into_bytes()
}

fn assert_docx_has_distinct_visible_image_assets(docx: &[u8]) {
    let parts = read_parts(docx);
    assert_eq!(
        decode_rgba8_png_fixture(&parts["word/media/header-image.png"]),
        [220, 24, 32, 255]
    );
    assert_eq!(
        decode_rgba8_png_fixture(&parts["word/media/footer-image.png"]),
        [24, 32, 220, 255]
    );
}

fn assert_only_relationship_target_differs(
    before: &[u8],
    after: &[u8],
    changed_part: &str,
    before_target: &str,
    after_target: &str,
) {
    let before_parts = read_parts(before);
    let after_parts = read_parts(after);
    assert_eq!(
        before_parts.keys().collect::<Vec<_>>(),
        after_parts.keys().collect::<Vec<_>>()
    );
    let before_relationships =
        String::from_utf8(before_parts[changed_part].clone()).expect("before relationship XML");
    let after_relationships =
        String::from_utf8(after_parts[changed_part].clone()).expect("after relationship XML");
    assert!(before_relationships.contains(&format!("Target=\"{before_target}\"")));
    assert!(after_relationships.contains(&format!("Target=\"{after_target}\"")));
    assert_ne!(before_target, after_target);
    assert_eq!(
        before_relationships.replace(before_target, "IMAGE_TARGET"),
        after_relationships.replace(after_target, "IMAGE_TARGET")
    );
    for (name, before_data) in &before_parts {
        if name != changed_part {
            assert_eq!(
                Some(before_data),
                after_parts.get(name),
                "unexpected part change: {name}"
            );
        }
    }
}

fn read_parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("valid DOCX ZIP");
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("ZIP entry");
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read ZIP entry");
        assert!(
            parts.insert(name.clone(), data).is_none(),
            "duplicate part {name}"
        );
    }
    parts
}

fn write_parts(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer.start_file(name, options).expect("start ZIP entry");
        writer.write_all(&data).expect("write ZIP entry");
    }
    writer.finish().expect("finish DOCX ZIP").into_inner()
}
