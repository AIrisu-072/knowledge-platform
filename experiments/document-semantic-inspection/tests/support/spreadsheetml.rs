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

pub fn mutate_vba_module(input: &[u8], module_name: &str, source: &str) -> Vec<u8> {
    let vba_project = read_entry(input, "xl/vbaProject.bin");
    let project = ovba::open_project(vba_project.clone()).expect("parse VBA seed");
    let module = project
        .modules
        .iter()
        .find(|module| module.name == module_name)
        .unwrap_or_else(|| panic!("missing VBA module {module_name}"));
    let stream_name = module.stream_name.clone();
    let text_offset = module.text_offset;
    drop(project);

    let mut compound =
        cfb::CompoundFile::open(Cursor::new(vba_project)).expect("open VBA CFB container");
    let stream_path = format!("/VBA/{stream_name}");
    let mut stream_bytes = Vec::new();
    compound
        .open_stream(&stream_path)
        .expect("open VBA module stream")
        .read_to_end(&mut stream_bytes)
        .expect("read VBA module stream");
    assert!(
        text_offset <= stream_bytes.len(),
        "VBA module text offset outside stream"
    );

    let mut replacement = stream_bytes[..text_offset].to_vec();
    replacement.extend_from_slice(&compress_vba_literals(source.as_bytes()));
    {
        let mut stream = compound
            .create_stream(&stream_path)
            .expect("replace VBA module stream");
        stream
            .write_all(&replacement)
            .expect("write VBA module stream");
    }
    compound.flush().expect("flush VBA CFB container");
    let mutated_vba = compound.into_inner().into_inner();
    replace_entry(input, "xl/vbaProject.bin", &mutated_vba)
}

fn compress_vba_literals(source: &[u8]) -> Vec<u8> {
    assert!(!source.is_empty(), "VBA source fixture must not be empty");

    let mut chunk = Vec::with_capacity(source.len() + source.len().div_ceil(8));
    for group in source.chunks(8) {
        chunk.push(0); // eight literal-token flags
        chunk.extend_from_slice(group);
    }
    assert!(
        chunk.len() <= 4096,
        "test VBA source must fit one MS-OVBA compressed chunk"
    );

    // MS-OVBA 2.4.1.1.5: compressed chunk header is 0xB000 OR
    // (CompressedChunkSize - 3).  CompressedChunkSize includes the 2-byte
    // header, so for D data bytes the stored size field is D - 1.
    let header = 0xB000u16 | u16::try_from(chunk.len() - 1).expect("chunk size");
    let mut out = Vec::with_capacity(3 + chunk.len());
    out.push(0x01); // CompressedContainer signature
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(&chunk);
    out
}
