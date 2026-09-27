# Vendored tree-sitter-vba parser provenance

- Upstream repository: `tmepple/tree-sitter-vba`
- Exact upstream grammar commit: `c691f237b2a703732d4b6a1f01d5b4f73f94d41e`
- Upstream package declaration: MIT
- Generated source: `src/parser.c`, SHA-256 `5e5a9df735d01ef349ffd6e74040776e8bb77a460e122150cc4de88bf4276e77`
- Parser header: `src/tree_sitter/parser.h`, SHA-256 `180b893c8734778fd32f372dfbc27bd6ad1cd2221f26150b31256ff6716320d2`
- Production scope: Document Semantic Inspection v0 static VBA syntax inspection only. The parser is never used to execute VBA.
- Qualification: the same generated files and grammar revision passed the merged DSI PoC qualification. The upstream commit's Cargo package refers to a missing Rust build file, so the production worker compiles the qualified generated parser source directly with `cc 1.2.41`.
