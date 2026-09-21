use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub fn read_entry(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("fixture zip");
    let mut file = archive.by_name(name).expect("fixture entry");
    let mut data = Vec::new();
    file.read_to_end(&mut data).expect("read fixture");
    data
}
pub fn replace_entry(input: &[u8], name: &str, replacement: &[u8]) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("fixture zip");
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("fixture entry");
        let entry_name = file.name().to_owned();
        let mut data = Vec::new(); file.read_to_end(&mut data).expect("read fixture");
        if entry_name == name { data = replacement.to_vec(); }
        entries.push((entry_name, data));
    }
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let opt = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (entry_name, data) in entries { out.start_file(entry_name, opt).expect("start"); out.write_all(&data).expect("write"); }
    out.finish().expect("finish").into_inner()
}
