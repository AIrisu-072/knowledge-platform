use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const ORIGIN_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin";
const SIGNATURE_REL: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature";

pub fn add_ooxml_signature(package: &[u8], signature_xml: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(package)).expect("input OOXML zip");
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("OOXML entry");
        let name = file.name().to_owned();
        if name.starts_with("_xmlsignatures/") {
            continue;
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data).expect("read OOXML entry");

        let data = match name.as_str() {
            "[Content_Types].xml" => insert_before(
                data,
                b"</Types>",
                format!(
                    "<Override PartName=\"/_xmlsignatures/origin.sigs\" ContentType=\"application/vnd.openxmlformats-package.digital-signature-origin\"/><Override PartName=\"/_xmlsignatures/sig1.xml\" ContentType=\"application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml\"/>"
                )
                .as_bytes(),
            ),
            "_rels/.rels" => insert_before(
                data,
                b"</Relationships>",
                format!(
                    "<Relationship Id=\"rIdDsiSignatureOrigin\" Type=\"{ORIGIN_REL}\" Target=\"_xmlsignatures/origin.sigs\"/>"
                )
                .as_bytes(),
            ),
            _ => data,
        };
        entries.push((name, data));
    }

    entries.push((
        "_xmlsignatures/origin.sigs".to_owned(),
        b"<SignatureOrigin xmlns=\"http://schemas.openxmlformats.org/package/2006/digital-signature\"/>".to_vec(),
    ));
    entries.push((
        "_xmlsignatures/_rels/origin.sigs.rels".to_owned(),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdSignature1\" Type=\"{SIGNATURE_REL}\" Target=\"sig1.xml\"/></Relationships>"
        )
        .into_bytes(),
    ));
    entries.push(("_xmlsignatures/sig1.xml".to_owned(), signature_xml.to_vec()));

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        writer
            .start_file(name, options)
            .expect("start OOXML output entry");
        writer.write_all(&data).expect("write OOXML output entry");
    }
    writer
        .finish()
        .expect("finish OOXML output zip")
        .into_inner()
}

fn insert_before(mut source: Vec<u8>, marker: &[u8], insertion: &[u8]) -> Vec<u8> {
    let offset = source
        .windows(marker.len())
        .rposition(|window| window == marker)
        .expect("OOXML marker");
    source.splice(offset..offset, insertion.iter().copied());
    source
}
