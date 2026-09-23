use std::io::{Cursor, Read};

use zip::ZipArchive;

pub fn read_part(input: &[u8], name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("PPTX ZIP");
    let mut file = archive.by_name(name).unwrap_or_else(|_| panic!("missing PPTX part {name}"));
    let mut out = Vec::new();
    file.read_to_end(&mut out).expect("read PPTX part");
    out
}

pub fn part_text(input: &[u8], name: &str) -> String {
    String::from_utf8(read_part(input, name)).expect("synthetic XML is UTF-8")
}

pub fn entry_names(input: &[u8]) -> Vec<String> {
    let mut archive = ZipArchive::new(Cursor::new(input)).expect("PPTX ZIP");
    let mut names = Vec::new();
    for index in 0..archive.len() {
        names.push(archive.by_index(index).expect("PPTX entry").name().to_owned());
    }
    names
}
