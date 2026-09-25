fn main() {
    println!("cargo:rerun-if-changed=vendor/tree-sitter-vba/c691f237/src/parser.c");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-vba/c691f237/src/tree_sitter/parser.h");
    cc::Build::new()
        .include("vendor/tree-sitter-vba/c691f237/src")
        .file("vendor/tree-sitter-vba/c691f237/src/parser.c")
        .warnings(false)
        .compile("tree-sitter-vba-c691f237");
}
