# Vendored tree-sitter-vba parser provenance

- Upstream repository: `tmepple/tree-sitter-vba`
- Upstream commit: `c691f237b2a703732d4b6a1f01d5b4f73f94d41e`
- Upstream package declaration: MIT
- Vendored files:
  - `src/parser.c`
  - `src/tree_sitter/parser.h`
- Reason: the upstream commit's `Cargo.toml` references missing `bindings/rust/build.rs`, so the git crate cannot compile. The generated parser source itself is present and is compiled locally without changing the grammar revision.
- Scope: PoC qualification only; no production promotion.
