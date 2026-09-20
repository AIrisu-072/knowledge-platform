# Document Semantic Inspection v0 PoC Qualification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Qualify the format parsers, semantic-normalization rules, sandbox assumptions, and deterministic/fail-closed behavior required by the frozen Document Semantic Inspection v0 Design, without promoting any PoC dependency into production code.

**Architecture:** Build an isolated Rust PoC workspace under `experiments/document-semantic-inspection/`. Every adapter returns an opaque format-native semantic projection plus common evidence metadata; the harness hashes the projection and evaluates BASE / SEMANTIC / NOISE / EDITORIAL / HOSTILE fixture relations. Candidate libraries remain confined to the experiment workspace. The plan ends with a qualification report and selection update; production Document Semantic Inspection gets a separate implementation plan after actual PoC results are known.

**Tech Stack:** Rust 1.98.1; isolated Cargo workspace; serde/serde_json; sha2; office_oxide 0.1.11 + strict raw OOXML sentinel for DOCX; rxls 0.1.3; calamine 0.36.1; ovba 0.7.1; tree-sitter 0.25 + MIT `tmepple/tree-sitter-vba` pinned at `c691f237b2a703732d4b6a1f01d5b4f73f94d41e`; pptx 0.1.0; powerpoint-ooxml 1.0.0; pdfium-render 0.9.4; lopdf 0.45.0; xml-sec 0.1.16; cms 0.2.3; x509-cert 0.2.5; pkix-path 0.3.2; pkix-chain 0.1.1; pkix-revocation 0.3.3; scraper 0.27.0 (html5ever 0.39 parser); csv 1.4.0; encoding_rs 0.8.41.

**Spec:** `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`

## Global Constraints

- Frozen Design approval: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`.
- Rust toolchain remains exactly `1.98.1` for repository/CI execution.
- PoC dependencies remain under `experiments/document-semantic-inspection/`; do not add them to root `[workspace.dependencies]` or production crates.
- Permitted dependency licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, PostgreSQL License, or equivalent permissive/public-domain terms. MPL/GPL/AGPL/LGPL/SSPL/BSL require rejection unless a separately approved ADR changes policy.
- Repository fixtures contain no customer/institution documents, secrets, personal data, or proprietary production files.
- Same bytes + same inspection profile must produce the same semantic fingerprint and evidence on every repeated run.
- Unknown version-significant constructs must never become silent success.
- Semantic success is all-or-nothing; partial extraction cannot pass the Versioning gate.
- XLSM/VBA is required in v0; macros are statically inspected and never executed.
- External workbook/URL/ODBC references are parsed as definitions and never dereferenced.
- scan-only PDF remains `RequiresOcr`; encrypted/password-protected Office/PDF remains unsupported in v0.
- ZIP is transport, not a semantic document format.
- PDF PoC uses PDFium release `chromium/7881` / PDFium `151.0.7881.0`, matching the explicit `pdfium_7881` API supported by `pdfium-render 0.9.4`; downloaded native artifacts are SHA-256 verified before use.
  - Linux x64: `1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d`
  - macOS arm64: `52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40`
  - macOS x64: `6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b`
- Every format promotion gate is 100% for required semantic/noise/editorial/fail-closed/determinism fixtures; “most files work” is not sufficient.
- Production implementation does not begin in this plan.

## Review Focus

1. **Unknown OOXML package parts:** a DOCX/XLSX/PPTX containing an unrecognized relationship/content type that could affect meaning must fail closed rather than produce a normal fingerprint. Task 3/4/5 tests pin this.
2. **VBA parser recovery:** any Tree-sitter `ERROR`/`MISSING` node or incomplete `ovba` module extraction must reject semantic success; Task 4 tests pin this.
3. **PDF parser disagreement:** required semantic disagreement between PDFium and lopdf must be an explicit failure, not a winner-selection heuristic; Task 6 tests pin this.
4. **Raw binding / format mismatch:** fixture bytes whose hash/size/media format do not match the manifest must fail before semantic comparison; Task 1 tests pin this.
5. **Host-dependent nondeterminism:** map order, locale, timezone, line endings, parser IDs, and native-library version must not change the semantic result; Task 8 runs repeated Linux/macOS evidence.

## Planned File Structure

```text
experiments/document-semantic-inspection/
  Cargo.toml
  Cargo.lock
  README.md
  deny.toml
  src/
    lib.rs
    error.rs
    model.rs
    manifest.rs
    canonical.rs
    runner.rs
    report.rs
    adapters/
      mod.rs
      text.rs
      csv.rs
      html.rs
      docx.rs
      spreadsheet.rs
      vba.rs
      pptx.rs
      pdf.rs
      signatures.rs
    bin/
      dsi-poc.rs
  tests/
    harness_contract.rs
    text_formats.rs
    docx.rs
    spreadsheet.rs
    pptx.rs
    pdf.rs
    signatures.rs
    cross_format.rs
    determinism_security.rs
    support/
      mod.rs
      ooxml.rs
      pdf_fixture.rs
  fixtures/
    manifest.json
    txt/
    csv/
    html/
    docx/
    xlsx/
    xlsm/
    pptx/
    pdf/
  provenance/
    third-party-fixtures.md
  scripts/
    install-pdfium.sh

.github/workflows/
  dsi-poc.yml

docs/superpowers/execution/
  document-semantic-inspection-v0-poc-report.md

spec/selection/
  library-tool-selection-v0.md
  rust-library-matrix-v0.md

mise.toml
```

Generated report scratch files stay under `target/` and are not committed. Do not perform unrelated refactors.

---

### Task 1: Isolated PoC workspace and deterministic harness

**Files:**
- Create: `experiments/document-semantic-inspection/Cargo.toml`
- Create: `experiments/document-semantic-inspection/Cargo.lock`
- Create: `experiments/document-semantic-inspection/README.md`
- Create: `experiments/document-semantic-inspection/deny.toml`
- Create: `experiments/document-semantic-inspection/src/lib.rs`
- Create: `experiments/document-semantic-inspection/src/error.rs`
- Create: `experiments/document-semantic-inspection/src/model.rs`
- Create: `experiments/document-semantic-inspection/src/manifest.rs`
- Create: `experiments/document-semantic-inspection/src/canonical.rs`
- Create: `experiments/document-semantic-inspection/src/runner.rs`
- Create: `experiments/document-semantic-inspection/src/report.rs`
- Create: `experiments/document-semantic-inspection/src/adapters/mod.rs`
- Create: `experiments/document-semantic-inspection/src/bin/dsi-poc.rs`
- Create: `experiments/document-semantic-inspection/tests/harness_contract.rs`
- Create: `experiments/document-semantic-inspection/fixtures/manifest.json`
- Modify: `mise.toml`
- Create: `.github/workflows/dsi-poc.yml`

**Interfaces:**
- Consumes: synthetic fixture bytes and `fixtures/manifest.json`.
- Produces:
  ```rust
  pub trait InspectionAdapter {
      fn format(&self) -> FormatId;
      fn inspect(&self, input: &[u8], profile: &InspectionProfile)
          -> Result<AdapterOutput, PocError>;
  }

  pub struct AdapterOutput {
      pub semantic_projection: Vec<u8>,
      pub capabilities: Vec<CapabilityEvidence>,
      pub editorial: EditorialEvidence,
      pub external_dependencies: Vec<ExternalDependency>,
      pub signatures: Vec<SignatureEvidence>,
      pub diagnostics: Vec<Diagnostic>,
  }

  pub struct InspectionResult {
      pub semantic_fingerprint: [u8; 32],
      pub output: AdapterOutput,
  }
  ```

- [x] **Step 1: Write failing harness contract tests**

Add tests that reject duplicate case IDs, missing BASE references, undeclared fixture classes, raw SHA mismatch, raw size mismatch, and format mismatch before an adapter result is accepted.

```rust
#[test]
fn raw_binding_mismatch_fails_before_adapter_success() {
    let case = fixture_case("txt/base.txt", "00".repeat(32), 999);
    let err = run_case(&case, &AlwaysSuccessAdapter).unwrap_err();
    assert!(matches!(err, PocError::RawBindingMismatch { .. }));
}

#[test]
fn unknown_fixture_class_is_rejected() {
    let json = r#"[{"id":"x","class":"MAYBE","path":"txt/base.txt"}]"#;
    assert!(matches!(
        FixtureManifest::from_json(json).unwrap_err(),
        PocError::InvalidManifest(_)
    ));
}
```

- [x] **Step 2: Run the new test to prove RED**

Run:

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test harness_contract
```

Expected: FAIL because the isolated workspace/harness does not exist.

- [x] **Step 3: Create the isolated Cargo workspace**

Use an independent workspace so root production Cargo metadata does not absorb PoC dependencies:

```toml
[workspace]
members = ["."]
resolver = "3"

[package]
name = "document-semantic-inspection-poc"
version = "0.0.0"
edition = "2024"
rust-version = "1.98"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11"
thiserror = "2"
hex = "0.4"

[dev-dependencies]
tempfile = "3"
```

Generate and commit this experiment's own `Cargo.lock`.

- [x] **Step 4: Implement the common model without a common content IR**

Use common metadata only; keep `semantic_projection` opaque bytes owned by each adapter.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FixtureClass { Base, Semantic, Noise, Editorial, Hostile }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpectedOutcome {
    Success,
    SameAs { case_id: String },
    DifferentFrom { case_id: String },
    Error { code: ErrorCode },
}

pub fn fingerprint(projection: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(projection).into()
}
```

All map-like canonical data used inside an adapter must use sorted structures (`BTreeMap`/`BTreeSet`) or explicit sorting before serialization.

- [x] **Step 5: Add manifest validation and report output**

`dsi-poc verify` reads the manifest, executes cases, checks relation expectations, and writes a JSON + Markdown report under `target/dsi-poc/`.

Exit non-zero on any failed required case.

- [x] **Step 6: Add root mise tasks**

Add exactly these entrypoints:

```toml
[tasks."poc:dsi:test"]
run = ["cargo test --locked --manifest-path experiments/document-semantic-inspection/Cargo.toml"]

[tasks."poc:dsi:run"]
run = ["cargo run --locked --manifest-path experiments/document-semantic-inspection/Cargo.toml --bin dsi-poc -- verify"]

[tasks."poc:dsi:deny"]
run = ["cargo deny --manifest-path experiments/document-semantic-inspection/Cargo.toml --config experiments/document-semantic-inspection/deny.toml check"]

[tasks."poc:dsi:verify"]
depends = ["poc:dsi:test", "poc:dsi:run", "poc:dsi:deny"]
```

The experiment `deny.toml` mirrors the repository permissive-license allowlist.

- [x] **Step 7: Add path-scoped hosted CI**

Create `.github/workflows/dsi-poc.yml` with:

- `pull_request.paths` for the experiment, this workflow, `mise.toml`, and frozen Design/Plan files;
- `permissions: contents: read`;
- pinned checkout and mise actions matching existing CI;
- Ubuntu job running `mise run poc:dsi:verify`;
- no self-hosted runner and no production credentials.

Do not add this PoC workflow to the production `required-check` fan-in; merge gating for this capability will explicitly inspect both standard CI and DSI PoC CI.

- [x] **Step 8: Verify GREEN**

Run:

```bash
mise run poc:dsi:verify
mise run arch:check
mise run ci:lint
```

Expected: PASS with only harness fixtures.

- [x] **Step 9: Commit**

```bash
git add experiments/document-semantic-inspection mise.toml .github/workflows/dsi-poc.yml
git commit -m "test: add semantic inspection poc harness"
```

---

### Task 2: TXT, CSV, and HTML baseline adapters

**Files:**
- Modify: `experiments/document-semantic-inspection/Cargo.toml`
- Create: `experiments/document-semantic-inspection/src/adapters/text.rs`
- Create: `experiments/document-semantic-inspection/src/adapters/csv.rs`
- Create: `experiments/document-semantic-inspection/src/adapters/html.rs`
- Create: `experiments/document-semantic-inspection/tests/text_formats.rs`
- Create fixtures under: `fixtures/txt/`, `fixtures/csv/`, `fixtures/html/`
- Modify: `fixtures/manifest.json`

**Interfaces:**
- Consumes: `InspectionAdapter`.
- Produces: `TextAdapter`, `CsvAdapter`, `HtmlAdapter`.

- [x] **Step 1: Add exact PoC dependencies**

```toml
encoding_rs = "=0.8.41"
csv = "=1.4.0"
scraper = "=0.27.0"
unicode-normalization = "0.1"
url = "2"
```

Update the isolated lockfile.

> **Qualification result (2026-09-21):** the planned `scraper = 0.27.0` wrapper was exercised but rejected by the permissive-license gate because its transitive `cssparser` / `selectors` path includes MPL-2.0. The HTML implementation therefore uses the frozen Design's underlying `html5ever` candidate directly as `html5ever = "=0.39.0"` plus `markup5ever_rcdom = "=0.39.0"`. The same HTML fixtures and semantic contract remained unchanged.

- [x] **Step 2: Write RED fixtures/tests**

TXT:
- UTF-8 LF baseline;
- CRLF variant = SAME;
- canonically equivalent Unicode = SAME;
- text edit = DIFFERENT;
- invalid/ambiguous decode = ERROR.

CSV:
- quoted/unquoted equivalent table = SAME;
- row/cell/column edit = DIFFERENT;
- inconsistent column count = ERROR;
- delimiter ambiguity = ERROR.

HTML:
- whitespace/attribute-order/pure-style edit = SAME;
- visible text, heading/list/table/link/image edit = DIFFERENT;
- script-required content fixture = ERROR without executing script.

Example assertion:

```rust
assert_same("csv/base", "csv/quote-noise");
assert_different("html/base", "html/link-target-change");
assert_error("html/js-only-content", ErrorCode::UnsupportedSemanticConstruct);
```

- [x] **Step 3: Run RED**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test text_formats
```

Expected: FAIL because adapters are absent.

- [x] **Step 4: Implement strict text/CSV/HTML projections**

TXT projection: deterministic UTF-8 after allowed encoding decode, Unicode normalization, and line-ending normalization.

CSV projection: deterministic row/column JSON array; configure `csv::ReaderBuilder::flexible(false)`; delimiter is explicit from the fixture/profile and never guessed by best-effort heuristics.

HTML projection: parse without script execution; emit only version-significant DOM semantics in document order. Normalize URI strings; ignore pure CSS/style attributes. Reject when the fixture marks content as script-dependent.

- [x] **Step 5: Verify and commit**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test text_formats
mise run poc:dsi:verify
git add experiments/document-semantic-inspection
git commit -m "test: qualify text csv and html semantic inspection"
```

---

### Task 3: DOCX qualification with tracked-change provenance and OOXML coverage sentinel

**Files:**
- Modify: `experiments/document-semantic-inspection/Cargo.toml`
- Create: `src/adapters/docx.rs`
- Create: `tests/docx.rs`
- Create: `tests/support/mod.rs`
- Create: `tests/support/ooxml.rs`
- Create fixtures under: `fixtures/docx/`
- Modify: `fixtures/manifest.json`

**Interfaces:**
- Produces: `DocxAdapter`.
- Typed semantic candidate: `office_oxide = "=0.1.11"`.
- Independent editorial/package oracle: project-owned strict raw OOXML inspection.
- Raw package coverage sentinel:
  ```toml
  zip = { version = "=8.6.0", default-features = false, features = ["deflate"] }
  quick-xml = "=0.42.0"
  ```

> **Task 3 candidate-selection Ruling (2026-09-21):**
> - `stemma 0.5.0` and `docx-review-core 0.1.1` are **REJECTED**. Their transitive Quick-XML lines are affected by current RustSec DoS advisories; `stemma` also carries an incompatible license path through its legacy ZIP graph. Advisory/license exceptions are not permitted by the frozen security/dependency gates.
> - `docxml 0.3.1` is **REJECTED** under the existing license allowlist because its default `zip 7.2` codec graph introduces unapproved `bzip2-1.0.6` and `CC0-1.0 OR MIT-0` license expressions.
> - `office_oxide 0.1.11` plus a project-owned strict OOXML sentinel is the Task 3 PoC candidate. Dependency preflight passed unchanged security/license gates in DSI PoC run `35545142423`.
> - The frozen DOCX semantic contract is unchanged. This ruling changes only the candidate implementation/oracle composition. Cost if wrong: Task 3 fails its semantic fixtures and remains unqualified; production promotion remains prohibited.

- [x] **Step 1: Add dependencies and pass dependency preflight**

```toml
office_oxide = "=0.1.11"
zip = { version = "=8.6.0", default-features = false, features = ["deflate"] }
quick-xml = "=0.42.0"
```

Pin the isolated `Cargo.lock`; do not regenerate it in hosted CI.

- [x] **Step 2: Build independent minimal OOXML fixture generation**

`tests/support/ooxml.rs` must construct package parts directly with `zip` and literal OOXML, not by serializing through `stemma`.

Expose helpers such as:

```rust
pub fn docx_fixture(spec: DocxFixtureSpec) -> Vec<u8>;
pub fn mutate_zip_entry_order(input: &[u8]) -> Vec<u8>;
pub fn add_unknown_relationship(input: &[u8], rel_type: &str) -> Vec<u8>;
```

Fixtures include:
- body text;
- heading/list structure;
- table cell/merge;
- header/footer;
- footnote/endnote;
- hyperlink;
- image bytes;
- tracked insert/delete/replacement;
- resolved/unresolved comment;
- save metadata / relationship-ID / XML-order / formatting-only noise;
- unknown relationship/content type;
- malformed/deep OOXML.

- [x] **Step 3: Write RED tests**

```rust
assert_same("docx/base", "docx/metadata-noise");
assert_same("docx/base", "docx/font-only");
assert_different("docx/base", "docx/table-merge-change");

let editorial = inspect_case("docx/tracked-replacement").unwrap().output.editorial;
assert!(editorial.has_unresolved_changes());
assert_eq!(projected_text("docx/tracked-replacement"), "new text");

assert_error("docx/unknown-semantic-part", ErrorCode::UnsupportedSemanticConstruct);
```

Also compare the typed parser's proposed-final view and shared structural facts against independent raw-OOXML golden expectations. Track-change/comment counts and resolved state come from the project-owned raw OOXML oracle; any disagreement on a shared required fact fails the PoC case.

- [ ] **Step 4: Run RED**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test docx
```

- [ ] **Step 5: Implement DOCX adapter and coverage sentinel**

The adapter uses `office_oxide` as the typed DOCX semantic candidate and serializes an adapter-owned semantic projection containing only frozen version-significant semantics. A separate project-owned raw OOXML sentinel/oracle enumerates package content types, relationships, revision/comment evidence, and hostile-container conditions before semantic success.

Rules:
- known non-semantic metadata parts may be ignored explicitly;
- unknown relationships/content types are classified;
- unknown constructs that may affect reader-visible/version-significant semantics return `UnsupportedSemanticConstruct`;
- parser-generated IDs never enter projection bytes.

- [ ] **Step 6: Determinism repetition**

Run each DOCX case 20 times in one process and in 5 fresh process invocations; all successful semantic/evidence JSON must match byte-for-byte after report normalization.

- [ ] **Step 7: Verify and commit**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test docx
mise run poc:dsi:verify
git add experiments/document-semantic-inspection
git commit -m "test: qualify docx semantic inspection"
```

---

### Task 4: XLSX/XLSM qualification, VBA extraction, and strict VBA syntax gate

**Files:**
- Modify: `experiments/document-semantic-inspection/Cargo.toml`
- Create: `src/adapters/spreadsheet.rs`
- Create: `src/adapters/vba.rs`
- Create: `tests/spreadsheet.rs`
- Create fixtures under: `fixtures/xlsx/`, `fixtures/xlsm/`
- Create: `provenance/third-party-fixtures.md`
- Modify: `fixtures/manifest.json`

**Interfaces:**
- Primary spreadsheet candidate: `rxls = "=0.1.3"`.
- Differential oracle: `calamine = { version = "=0.36.1", features = ["picture"] }`.
- VBA container/source: `ovba = "=0.7.1"`.
- VBA syntax candidate: `tree-sitter = "0.25"` plus:
  ```toml
  tree-sitter-vba = { git = "https://github.com/tmepple/tree-sitter-vba", rev = "c691f237b2a703732d4b6a1f01d5b4f73f94d41e" }
  ```

- [ ] **Step 1: Add spreadsheet/VBA dependencies and lock them**

Update `Cargo.lock` and confirm `cargo deny` accepts every direct/transitive license/source.

- [ ] **Step 2: Build XLSX fixtures independently of rxls**

Use raw SpreadsheetML ZIP/XML helpers for:
- value/type changes;
- formula source changes with unchanged cached value;
- sheet add/remove/order;
- hidden/veryHidden content;
- named ranges;
- merged cells;
- tables;
- hyperlink/external-reference definitions;
- chart series/data;
- image changes;
- cached result/XML ordering/style-only noise;
- unknown OOXML relationship/content type.

- [ ] **Step 3: Add a licensed synthetic XLSM seed with provenance**

Import Calamine's synthetic `tests/vba.xlsm` from upstream commit `0af05f4f6030351e3b8a999ea0810c8618368776` solely as a PoC seed. Record source path, upstream commit, MIT license, local SHA-256, and the fact that it is third-party test data in `provenance/third-party-fixtures.md`.

Do not use production/customer XLSM.

Derive local semantic/noise variants from the seed by changing workbook XML independently of the VBA binary. For VBA source-change/noise fixtures, use `ovba` to extract modules and a dedicated fixture builder that replaces the VBA module source stream while preserving the rest of the synthetic project; if replacement cannot be implemented without corrupting MS-OVBA, mark the candidate **not qualified** rather than skipping VBA cases.

- [ ] **Step 4: Write RED XLSX/XLSM tests**

```rust
assert_different("xlsx/base", "xlsx/formula-source-change-same-cache");
assert_different("xlsx/base", "xlsx/very-hidden-change");
assert_same("xlsx/base", "xlsx/style-only");

assert_different("xlsm/base", "xlsm/vba-logic-change");
assert_same("xlsm/base", "xlsm/vba-comment-only");
assert_same("xlsm/base", "xlsm/vba-whitespace-only");
assert_error("xlsm/vba-invalid-syntax", ErrorCode::SemanticExtractionFailed);
```

For all required cell/formula/sheet/name/link fields, compare rxls against Calamine/golden expectations. Parser disagreement does not use majority vote; the case fails pending analysis.

- [ ] **Step 5: Implement spreadsheet projection**

The projection must include sorted:
- sheet identity/order/visibility;
- typed cells;
- formula source, not cached result;
- defined names;
- merged/table structure;
- links and external-reference definitions;
- meaningful chart/image evidence available under the candidate contract.

Style-only/cached-result-only fields remain outside semantic bytes.

- [ ] **Step 6: Implement strict VBA gate**

`ovba` extracts every module and reference. Tree-sitter parses each source module.

Hard gate:

```rust
fn parse_vba_strict(source: &[u8]) -> Result<VbaTree, PocError> {
    let tree = parse_with_tree_sitter(source)?;
    if contains_error_or_missing(tree.root_node()) {
        return Err(PocError::SemanticExtractionFailed("VBA syntax recovery node".into()));
    }
    Ok(canonicalize_vba_tree(tree))
}
```

Canonicalization:
- lower-case case-insensitive identifiers/keywords where semantically neutral;
- remove comments and formatting trivia;
- preserve declarations, literals, procedure/module structure, calls/operators, conditional compilation, and reference/project metadata;
- stable child ordering only where the language semantics are order-insensitive; otherwise preserve source order.

If the Tree-sitter candidate cannot parse required synthetic/realistic VBA without recovery nodes, record FAIL and do not silently replace it with a hand-written permissive tokenizer.

- [ ] **Step 7: Verify and commit**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test spreadsheet
mise run poc:dsi:verify
git add experiments/document-semantic-inspection
git commit -m "test: qualify xlsx xlsm and vba inspection"
```

---

### Task 5: PPTX qualification and package coverage sentinel

**Files:**
- Modify: `Cargo.toml` in experiment
- Create: `src/adapters/pptx.rs`
- Create: `tests/pptx.rs`
- Create fixtures under: `fixtures/pptx/`
- Modify: `fixtures/manifest.json`

**Interfaces:**
- Primary candidate: `pptx = "=0.1.0"`.
- Differential structural candidate: `powerpoint-ooxml = "=1.0.0"`.
- Uses the raw OOXML package coverage helper from Task 3.

- [ ] **Step 1: Add pinned candidates**

Update lockfile and deny gate.

- [ ] **Step 2: Generate raw PresentationML fixtures**

Direct OOXML fixtures cover:
- slide add/remove/order;
- text/shape association;
- table;
- chart data/series;
- SmartArt;
- image;
- hyperlink;
- speaker notes;
- meaningful group/shape relationship;
- theme/font/background-only noise;
- internal IDs/XML ordering noise;
- unknown package part/relationship.

- [ ] **Step 3: RED tests**

```rust
assert_different("pptx/base", "pptx/slide-order-change");
assert_different("pptx/base", "pptx/speaker-note-change");
assert_same("pptx/base", "pptx/theme-only");
assert_error("pptx/unknown-semantic-part", ErrorCode::UnsupportedSemanticConstruct);
```

For semantics both candidates expose, require agreement with golden expectations.

- [ ] **Step 4: Implement projection + coverage sentinel**

No unknown relationship/content type that can carry presentation meaning may be ignored. Internal shape IDs and theme-only formatting are excluded.

- [ ] **Step 5: Verify and commit**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test pptx
mise run poc:dsi:verify
git add experiments/document-semantic-inspection
git commit -m "test: qualify pptx semantic inspection"
```

---

### Task 6: PDF dual-engine qualification with pinned PDFium

**Files:**
- Modify: experiment `Cargo.toml`
- Create: `src/adapters/pdf.rs`
- Create: `tests/pdf.rs`
- Create: `tests/support/pdf_fixture.rs`
- Create: `scripts/install-pdfium.sh`
- Create fixtures under: `fixtures/pdf/`
- Modify: `fixtures/manifest.json`
- Modify: `.github/workflows/dsi-poc.yml`

**Interfaces:**
- Semantic engine: `pdfium-render = "=0.9.4"`.
- Structural engine: `lopdf = { version = "=0.45.0", default-features = false }`.
- PDFium native build: chromium/8057 with hashes from Global Constraints.

- [ ] **Step 1: Add pinned Rust dependencies**

Disable unnecessary lopdf defaults. Configure pdfium-render only with features required for dynamic binding and current Pdfium API compatibility.

- [ ] **Step 2: Implement deterministic PDFium installer**

`scripts/install-pdfium.sh`:
- detects Linux x64 or macOS x64/arm64;
- downloads only release `chromium/8057`;
- verifies the exact SHA-256 listed in Global Constraints;
- extracts under `target/dsi-poc/pdfium/7881/<platform>/`;
- prints the library directory for `PDFIUM_DYNAMIC_LIB_PATH`;
- refuses unknown platforms or checksum mismatch.

No native binary is committed.

- [ ] **Step 3: Generate independent minimal PDF fixtures**

`pdf_fixture.rs` builds small deterministic PDFs directly from PDF syntax for:
- visible text;
- page order;
- links;
- form field values;
- annotations;
- image object;
- malformed/broken xref;
- encrypted marker fixture where supported;
- image-only/scan-only fixture.

Also include equivalent semantic PDFs with different object numbers/producer metadata to test noise invariance.

- [ ] **Step 4: RED tests**

```rust
assert_same("pdf/base", "pdf/object-id-producer-noise");
assert_different("pdf/base", "pdf/page-order-change");
assert_error("pdf/scan-only", ErrorCode::RequiresOcr);
assert_error("pdf/broken-xref", ErrorCode::SemanticExtractionFailed);
```

Add a constructed fixture where lopdf sees a required object/annotation/link not represented by the PDFium semantic result; expected result is `ParserDisagreement`.

- [ ] **Step 5: Implement dual-engine PDF adapter**

Rules:
- both engines must open the document;
- lopdf structural facts and PDFium semantic facts are compared for required page count, annotations/links/forms/object presence relevant to the profile;
- ambiguous text/read order is fail-closed;
- scan-only means no reliable native text and reader-visible content is image-only -> `RequiresOcr`;
- PDFium build/version/hash is emitted in extractor provenance;
- disagreement is never resolved by “trust PDFium” or “trust lopdf”.

- [ ] **Step 6: Extend Linux/macOS PoC workflow**

Install pinned PDFium before `mise run poc:dsi:verify`. Add a macOS PoC job using the same installer. Do not allow “PDF tests skipped because library missing”.

- [ ] **Step 7: Verify and commit**

```bash
PDFIUM_DYNAMIC_LIB_PATH="$(experiments/document-semantic-inspection/scripts/install-pdfium.sh)"   cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test pdf
mise run poc:dsi:verify
git add experiments/document-semantic-inspection .github/workflows/dsi-poc.yml
git commit -m "test: qualify pdf semantic inspection"
```

---

### Task 7: Digital-signature evidence qualification

**Files:**
- Modify: experiment `Cargo.toml`
- Create: `src/adapters/signatures.rs`
- Create: `tests/signatures.rs`
- Create signature fixtures under DOCX/XLSX/PPTX/PDF fixture trees
- Modify: `fixtures/manifest.json`

**Interfaces:**
- XMLDSig: `xml-sec = "=0.1.16"`.
- CMS: `cms = { version = "=0.2.3", features = ["std", "sha2", "signature"] }`.
- X.509 model: `x509-cert = "=0.2.5"` for compatibility with cms/pkix line.
- Path/revocation: `pkix-path = "=0.3.2"`, `pkix-chain = "=0.1.1"`, `pkix-revocation = { version = "=0.3.3", features = ["crl", "ocsp"] }`.

- [ ] **Step 1: Add exact crypto dependencies and verify license/source gate**

No network revocation fetch is enabled; CRL/OCSP evidence is supplied as fixture bytes.

- [ ] **Step 2: Generate known-good and known-bad signature fixtures**

Required vectors:
- valid;
- signed content tampered;
- invalid digest;
- expired certificate;
- revoked certificate using offline CRL;
- unknown issuer;
- broken chain;
- unsupported algorithm;
- malformed signature.

For XMLDSig, use project-owned synthetic XML/OOXML signature fixtures and cross-check against xml-sec's verification semantics.

For PDF/CMS, build a small synthetic detached CMS signature over the PDF ByteRange bytes using test-only keys/certs generated and committed only as non-secret deterministic fixtures. Never commit a live private key; test keys are clearly marked `TEST ONLY`.

- [ ] **Step 3: RED tests**

```rust
assert_eq!(signature_state("sig/valid"), SignatureValidity::Valid);
assert_eq!(signature_state("sig/tampered"), SignatureValidity::Invalid);
assert_eq!(signature_state("sig/revoked"), SignatureValidity::Invalid);
assert_eq!(signature_state("sig/unknown-issuer"), SignatureValidity::Unverifiable);
```

Inspection success with invalid/unverifiable signatures is permitted at this PoC layer; evidence must preserve the state so Publish can fail later.

- [ ] **Step 4: Implement format-specific wrappers**

Do not expose xml-sec/CMS types to the common harness. Emit only `SignatureEvidence`.

PDF wrapper must validate:
- ByteRange structure;
- exact covered bytes;
- CMS signer/digest/signature;
- chain policy;
- supplied offline CRL/OCSP where the fixture requires it.

- [ ] **Step 5: Verify and commit**

```bash
cargo test --manifest-path experiments/document-semantic-inspection/Cargo.toml --test signatures
mise run poc:dsi:verify
git add experiments/document-semantic-inspection
git commit -m "test: qualify document signature evidence"
```

---

### Task 8: Cross-format capability, determinism, hostile-input, and sandbox evidence

**Files:**
- Create: `tests/cross_format.rs`
- Create: `tests/determinism_security.rs`
- Modify: `src/runner.rs`
- Modify: `src/report.rs`
- Modify: `.github/workflows/dsi-poc.yml`
- Modify: `fixtures/manifest.json`

**Interfaces:**
- Consumes all adapters from Tasks 2–7.
- Produces promotion-gate evidence per format.

- [ ] **Step 1: RED cross-format capability tests**

Model capability preservation explicitly:

```rust
let decision = assess_authority_migration(&xlsm, &pdf);
assert_eq!(decision, AuthorityMigrationDecision::Denied {
    missing: vec![
        CapabilityId::FormulaLogic,
        CapabilityId::VbaLogic,
        CapabilityId::HiddenContent,
    ],
});
```

DOCX->PDF may only be eligible if every source version-significant capability in that specific fixture is representable and verified; no format-pair blanket allowlist.

- [ ] **Step 2: RED host-nondeterminism tests**

For each successful case:
- 20 in-process runs;
- 5 child-process runs;
- environment variations for `TZ`, `LANG`, and hash-map randomization where controllable.

Normalize only `inspected_at`/runtime diagnostics out of comparison. Fingerprint/capability/editorial/dependency/signature evidence must remain identical.

- [ ] **Step 3: RED hostile/resource tests**

Run malformed/truncated/deep/oversized fixtures through a child-process harness with hard timeout. Assert:
- non-zero controlled error rather than panic escape;
- no partial result file;
- no fixture body emitted in stderr/report;
- process termination on timeout/resource breach.

The PoC may use OS process limits available on Linux/macOS; record exact command/limit evidence in the report.

- [ ] **Step 4: Implement promotion-gate aggregation**

For every required format compute:

```text
semantic-change     passed/required
noise-invariance    passed/required
editorial           passed/required
fail-closed         passed/required
determinism         passed/required
security/resource   pass/fail
license/dependency  pass/fail
```

`promotion_eligible=true` only when every required count is complete and 100%.

- [ ] **Step 5: Run full Linux/macOS evidence**

```bash
mise run poc:dsi:verify
mise run verify
```

Hosted `dsi-poc.yml` must succeed on Ubuntu and macOS for the exact qualification head.

- [ ] **Step 6: Commit**

```bash
git add experiments/document-semantic-inspection .github/workflows/dsi-poc.yml
git commit -m "test: enforce semantic inspection qualification gates"
```

---

### Task 9: Qualification report, selection decisions, and production-plan gate

**Files:**
- Create: `docs/superpowers/execution/document-semantic-inspection-v0-poc-report.md`
- Modify: `spec/selection/library-tool-selection-v0.md`
- Modify: `spec/selection/rust-library-matrix-v0.md`
- Modify: `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
- Modify: `docs/superpowers/execution/active.md`

**Interfaces:**
- Consumes exact PoC JSON/Markdown results and exact CI run IDs.
- Produces authoritative library qualification decisions and the next planning gate.

- [ ] **Step 1: Generate and inspect the final machine report**

Run:

```bash
mise run poc:dsi:verify
cargo run --locked   --manifest-path experiments/document-semantic-inspection/Cargo.toml   --bin dsi-poc -- report --format json > target/dsi-poc/final.json
```

Every format must have an explicit outcome: `PASS`, `FAIL`, or `BLOCKED`. There is no implicit success.

- [ ] **Step 2: Write the human qualification report**

Record:
- exact repository head;
- exact dependency versions/git revisions;
- PDFium tag + binary hash;
- per-fixture results;
- parser/oracle disagreements and resolutions;
- unsupported constructs;
- determinism evidence;
- sandbox/resource evidence;
- Linux/macOS CI run IDs;
- license/dependency evidence;
- promotion decision for every required format.

No raw customer content is included.

- [ ] **Step 3: Update selection documents from evidence only**

For each candidate:
- change to `SELECTED` only if the frozen promotion gate passes;
- leave `POC REQUIRED` or mark `REJECTED` when it does not;
- state exact technical reason, not popularity/age.

If a required v0 format fails qualification, **do not weaken the Design**. Status becomes blocked pending supplemental adapter/library or explicit Design amendment.

- [ ] **Step 4: Update Active/Execution status**

If all required format gates pass:

```text
Phase = POC QUALIFICATION COMPLETE / PRODUCTION PLAN REQUIRED
Next exact action = write Document Semantic Inspection v0 Production Implementation Plan
```

If any required gate fails:

```text
Phase = POC QUALIFICATION BLOCKED
Next exact action = resolve named failing semantic gate; production implementation remains prohibited
```

- [ ] **Step 5: Final verification**

```bash
mise run poc:dsi:verify
mise run verify:full
```

Require:
- standard repository CI green;
- DSI PoC Ubuntu green;
- DSI PoC macOS green;
- no unresolved blocking PR review finding.

- [ ] **Step 6: Commit and stop**

```bash
git add docs/superpowers/execution spec/selection experiments/document-semantic-inspection .github/workflows/dsi-poc.yml mise.toml
git commit -m "docs: record semantic inspection poc qualification"
```

**STOP.** Do not create production Semantic Inspection crates in this plan. If qualification passes, write a new Production Implementation Plan from the frozen Design plus this evidence. If it fails, return to the failing PoC gate or Design amendment process.

## Self-Review Checklist

- Spec coverage: architecture boundary, no common durable IR, deterministic semantic fingerprint, editorial separation, signatures, external dependencies, required formats, XLSM/VBA, PDF dual-engine, fail-closed behavior, sandbox/resource evidence, cross-format capability, and promotion gate are all exercised by Tasks 1–9.
- Placeholder scan: no unresolved placeholder action remains in the plan.
- Type consistency: all adapters implement one `InspectionAdapter`; all successful outputs converge to `AdapterOutput`/`InspectionResult`; all fixture relations use `ExpectedOutcome`.
- Review Focus:
  - unknown OOXML parts -> Tasks 3/4/5;
  - VBA recovery -> Task 4;
  - PDF disagreement -> Task 6;
  - raw binding/format mismatch -> Task 1;
  - host nondeterminism -> Task 8.
- Scope check: production implementation is intentionally excluded because exact production adapters are evidence-dependent. PoC Qualification is independently testable and reviewable.
