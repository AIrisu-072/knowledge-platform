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
