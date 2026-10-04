# P1 hard ResourceLimit pure-wire correction — parent receipt

- Scope: validation.rs and extraction_contract.rs only, uncommitted feat/search-platform-completion-core@80a4796.
- Fix worker completion_p1_limit_fix reported actual adversarial RED against old Partial(ResourceLimit) acceptance (exit101), then 11/11 GREEN and strict pure-crate Clippy/owned formatting. This parent receipt records the worker report; it does not independently rerun its historical RED or Clippy.
- Partial with ResourceLimit alone or mixed is rejected. Unsupported(ResourceLimit) has zero Units. Known UnsupportedStructure and MissingFormulaCache Partial remain usable.
- SHA256 crates/search-extraction-core/src/validation.rs: `e598dd93ef175ea68b6a11cddb4e14464e059baf0a1c383ac444b2a8f7852f11`
- SHA256 crates/search-extraction-core/tests/extraction_contract.rs: `8038d4ae431708be20392eeb468f7c2a7172bb085534efd5ecebec95d08f7786`
- Fresh independent read-only recheck is p1-wire-resource-limit-recheck.md. Sandbox process enforcement, Linux isolation, format readers and production body evidence remain separate gates.
