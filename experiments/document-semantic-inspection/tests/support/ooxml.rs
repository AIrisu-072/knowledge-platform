use std::io::{Cursor, Read, Write};

use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdOffice" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

pub fn docx_fixture(body_text: &str) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{body_text}</w:t></w:r></w:p></w:body></w:document>"#
    );
    write_parts(vec![
        ("[Content_Types].xml".to_owned(), CONTENT_TYPES.as_bytes().to_vec()),
        ("_rels/.rels".to_owned(), ROOT_RELS.as_bytes().to_vec()),
        ("word/document.xml".to_owned(), document.into_bytes()),
    ])
}

pub fn mutate_zip_entry_order(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.reverse();
    write_parts(parts)
}

pub fn add_unknown_relationship(input: &[u8], rel_type: &str) -> Vec<u8> {
    let mut parts = read_parts(input);
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdUnknown" Type="{rel_type}" Target="semantic.xml"/></Relationships>"#
    );
    parts.push(("word/_rels/document.xml.rels".to_owned(), rels.into_bytes()));
    parts.push((
        "word/semantic.xml".to_owned(),
        br#"<?xml version="1.0"?><semantic><value>meaning</value></semantic>"#.to_vec(),
    ));
    write_parts(parts)
}

fn read_parts(input: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("fixture ZIP");
    let mut parts = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("fixture entry");
        let name = file.name().to_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read fixture entry");
        parts.push((name, data));
    }
    parts
}

fn write_parts(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer.start_file(name, options).expect("start fixture entry");
        writer.write_all(&data).expect("write fixture entry");
    }
    writer.finish().expect("finish fixture package").into_inner()
}


pub fn xml_serialization_noise(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        String::from_utf8(data)
            .expect("document XML")
            .replace("><", ">\n<")
            .into_bytes()
    })
}

pub fn margin_only_noise(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        let text = String::from_utf8(data).expect("document XML");
        text.replace(
            "</w:body>",
            r#"<w:sectPr><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body>"#,
        )
        .into_bytes()
    })
}

pub fn image_ancillary_noise(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/media/image1.png", |mut data| {
        const IEND_LEN: usize = 12;
        assert!(data.len() >= IEND_LEN, "PNG fixture");
        let chunk = hex::decode(
            "0000002574455874436f6d6d656e740073616d6520706978656c20646966666572656e7420656e636f64696e67b825bc2a",
        )
        .expect("valid tEXt chunk");
        let insert_at = data.len() - IEND_LEN;
        data.splice(insert_at..insert_at, chunk);
        data
    })
}

pub fn format_only_tracked_change(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        let text = String::from_utf8(data).expect("document XML");
        text.replacen(
            "<w:r>",
            r#"<w:r><w:rPr><w:rPrChange w:id="90" w:author="Format Editor"><w:rPr><w:b/></w:rPr></w:rPrChange></w:rPr>"#,
            1,
        )
        .into_bytes()
    })
}

pub fn move_markup_same_final_text(input: &[u8]) -> Vec<u8> {
    map_part(input, "word/document.xml", |data| {
        let text = String::from_utf8(data).expect("document XML");
        let original = "<w:r><w:t>fixture text</w:t></w:r>";
        let moved = r#"<w:moveFrom w:id="91" w:author="Mover"><w:r><w:t>fixture text</w:t></w:r></w:moveFrom><w:moveTo w:id="92" w:author="Mover"><w:r><w:t>fixture text</w:t></w:r></w:moveTo>"#;
        assert!(text.contains(original), "fixture paragraph");
        text.replacen(original, moved, 1).into_bytes()
    })
}

pub fn add_relationship_cycle(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    map_parts_text(&mut parts, "[Content_Types].xml", |text| {
        text.replace(
            "</Types>",
            r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
        )
    });
    parts.push((
        "word/header1.xml".into(),
        br#"<?xml version="1.0"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#.to_vec(),
    ));
    parts.push((
        "word/_rels/document.xml.rels".into(),
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#.to_vec(),
    ));
    parts.push((
        "word/_rels/header1.xml.rels".into(),
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdBack" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="document.xml"/></Relationships>"#.to_vec(),
    ));
    write_parts(parts)
}

pub fn add_traversal_relationship(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push((
        "word/_rels/document.xml.rels".into(),
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdTraversal" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="../escape.xml"/></Relationships>"#.to_vec(),
    ));
    write_parts(parts)
}

pub fn add_traversal_entry(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push(("../escape.xml".into(), b"<escape/>".to_vec()));
    write_parts(parts)
}

pub fn add_archive_bomb(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push(("word/bomb.xml".into(), vec![b'a'; 9 * 1024 * 1024]));
    write_parts_with_method(parts, CompressionMethod::Deflated)
}

pub fn add_duplicate_entry(input: &[u8]) -> Vec<u8> {
    let mut parts = read_parts(input);
    parts.push(("word/document.xml".into(), b"<duplicate/>".to_vec()));
    write_parts(parts)
}

fn map_part<F>(input: &[u8], name: &str, f: F) -> Vec<u8>
where
    F: FnOnce(Vec<u8>) -> Vec<u8>,
{
    let mut parts = read_parts(input);
    let (_, data) = parts
        .iter_mut()
        .find(|(part, _)| part == name)
        .unwrap_or_else(|| panic!("missing part {name}"));
    *data = f(std::mem::take(data));
    write_parts(parts)
}

fn map_parts_text<F>(parts: &mut [(String, Vec<u8>)], name: &str, f: F)
where
    F: FnOnce(String) -> String,
{
    let (_, data) = parts
        .iter_mut()
        .find(|(part, _)| part == name)
        .unwrap_or_else(|| panic!("missing part {name}"));
    let text = String::from_utf8(std::mem::take(data)).expect("XML fixture");
    *data = f(text).into_bytes();
}

fn write_parts_with_method(
    parts: Vec<(String, Vec<u8>)>,
    method: CompressionMethod,
) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(method)
        .unix_permissions(0o600);
    for (name, data) in parts {
        writer.start_file(name, options).expect("start fixture entry");
        writer.write_all(&data).expect("write fixture entry");
    }
    writer.finish().expect("finish fixture package").into_inner()
}
