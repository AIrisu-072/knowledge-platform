use std::path::PathBuf;

fn main() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../third_party/document-semantic-inspection/tree-sitter-vba/src");
    let parser = source_root.join("parser.c");
    let header = source_root.join("tree_sitter/parser.h");
    println!("cargo:rerun-if-changed={}", parser.display());
    println!("cargo:rerun-if-changed={}", header.display());
    cc::Build::new()
        .include(&source_root)
        .file(&parser)
        .warnings(false)
        .compile("tree-sitter-vba-c691f237");
}
