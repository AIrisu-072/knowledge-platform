# Document Semantic Inspection v0 — PoC Qualification Report

- Status: **PASS**
- Qualification date: 2026-09-24
- Frozen Design: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Approved PoC Plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
- Qualification execution head: `a4fcef1cb5cac5672199165f433bd303c25135a6`
- Execution PR: **#8**
- DSI PoC run: `35957940553` — **SUCCESS**
  - Ubuntu 24.04 job `107500145955` — SUCCESS
  - macOS 15 Intel job `107500146107` — SUCCESS
  - macOS 15 arm64 job `107500146179` — SUCCESS
- Standard CI run: `35957940565` — **SUCCESS**, including required-check
- Production dependency promotion: **not performed by the PoC PR**
- Next gate: **Document Semantic Inspection v0 Production Implementation Plan**

## 1. Qualification result

The frozen Document Semantic Inspection v0 contract qualified without a Design amendment.

The final machine report returned:

- `overall = PASS`
- all 8 required formats = `PASS`
- all 8 per-format promotion gates = `promotion_eligible=true`
- semantic manifest = **91/91 cases PASS**
- cross-format capability contract = **4/4 tests PASS**
- determinism/security contract = **5/5 tests PASS**
- cargo-deny = advisories / bans / licenses / sources **PASS**
- hosted execution = Ubuntu + macOS Intel + macOS arm64 **PASS**

| Format | Semantic | Noise | Editorial | Fail-closed | Determinism fixtures | Promotion |
|---|---:|---:|---:|---:|---:|---|
| TXT | 1/1 | 2/2 | 0/0 | 1/1 | 4/4 | PASS |
| CSV | 2/2 | 1/1 | 0/0 | 2/2 | 4/4 | PASS |
| HTML | 3/3 | 1/1 | 0/0 | 1/1 | 5/5 | PASS |
| DOCX | 10/10 | 4/4 | 3/3 | 3/3 | 18/18 | PASS |
| XLSX | 14/14 | 3/3 | 0/0 | 1/1 | 19/19 | PASS |
| XLSM | 1/1 | 2/2 | 0/0 | 1/1 | 1/1 | PASS |
| PPTX | 12/12 | 4/4 | 1/1 | 1/1 | 18/18 | PASS |
| PDF | 5/5 | 1/1 | 1/1 | 5/5 | 8/8 | PASS |

The XLSM counts include the supplemental VBA gates that mutate the licensed synthetic XLSM seed outside the static manifest: one semantic logic-change case, two noise-equivalent source cases, and one invalid-syntax fail-closed case.

## 2. Qualified PoC composition

These choices are qualified **for the frozen Document Semantic Inspection v0 semantics only**. They do not automatically select the same tools for Search Extraction, legacy Office conversion, or unrelated parsing workloads.

| Area | Qualified composition | Role |
|---|---|---|
| TXT | `encoding_rs 0.8.41` + `unicode-normalization` | strict decode and semantic text normalization |
| CSV | `csv 1.4.0` | explicit-delimiter table semantics |
| HTML | `html5ever 0.39.0` + `markup5ever_rcdom 0.39.0` | DOM semantics without script execution |
| DOCX | `office_oxide 0.1.11` + `zip 8.6.0` deflate-only + `quick-xml 0.42.0` | typed model plus independent raw OOXML oracle/sentinel |
| XLSX/XLSM | `rxls 0.1.3` + `calamine 0.36.1` + raw SpreadsheetML oracle | typed workbook semantics plus differential checks |
| VBA | `ovba 0.7.1` + `tree-sitter 0.25.10` + vendored generated `tree-sitter-vba` parser at `c691f237b2a703732d4b6a1f01d5b4f73f94d41e` | static extraction and strict syntax gate; never executed |
| PPTX | `office_oxide 0.1.11` + raw PresentationML oracle/sentinel | typed slide model plus chart/SmartArt/package coverage |
| PDF | `pdfium-render 0.9.4` + PDFium `151.0.7881.0` + `lopdf 0.45.0` | semantic engine plus independent structural oracle |
| XMLDSig | `xml-sec 0.1.16` | XMLDSig cryptographic/reference verification |
| CMS/X.509 | `cms 0.2.3` + `x509-cert 0.2.5` + vendored `openssl 0.10.81` | CMS parsing, explicit-chain verification, offline CRL evidence |

Other direct PoC pins include `der 0.7.10`, `tree-sitter-language 0.1.8`, `url 2`, `sha2 0.11`, `serde/serde_json`, `thiserror 2`, and `hex 0.4`. The experiment remains an independent Cargo workspace with its own lockfile.

### PDFium native identity

Release: `chromium/7881` / PDFium `151.0.7881.0`.

| Platform | SHA-256 |
|---|---|
| Linux x64 | `1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d` |
| macOS arm64 | `52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40` |
| macOS x64 | `6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b` |

The installer verifies the platform artifact SHA-256 and `VERSION BUILD=7881` before use.

## 3. Candidate rejections and explicit rulings

| Candidate | Result | Evidence / reason |
|---|---|---|
| `scraper 0.27.0` | REJECTED for this repository | transitive `cssparser/selectors` path includes MPL-2.0; direct html5ever composition passed the same HTML contract |
| `stemma 0.5.0` | REJECTED | vulnerable transitive Quick-XML line and legacy ZIP graph license conflict |
| `docx-review-core 0.1.1` | REJECTED | vulnerable transitive Quick-XML line |
| `docxml 0.3.1` | REJECTED | default ZIP codec graph introduced license expressions outside the existing allowlist |
| direct `tree-sitter-vba` git crate at `c691f237...` | REJECTED as a crate package | upstream commit references missing `bindings/rust/build.rs`; generated parser from the exact revision was vendored instead |
| `pptx 0.1.0` | REJECTED | Quick-XML 0.39.4 affected by RUSTSEC-2026-0194 / RUSTSEC-2026-0195 |
| `powerpoint-ooxml 1.0.0` | REJECTED | mandatory OPC/ZIP default codec graph violates current license allowlist |
| `pkix-chain 0.1.1` | REJECTED | yanked; fresh lock generation cannot select it |
| `pkix-chain 0.4.1 + pkix-path 0.3.2` | REJECTED | pulls `rsa 0.9.10`, rejected under RUSTSEC-2023-0071; no advisory exception was added |

No security, advisory, source, or license exception was introduced to preserve a planned parser name.

## 4. Parser/oracle disagreement policy and resolutions

- DOCX uses a typed Office Oxide view plus project-owned raw OOXML checks. Required shared facts must agree; unknown potentially semantic package content fails closed.
- XLSX/XLSM uses rxls as the primary typed workbook view and Calamine/raw SpreadsheetML as independent evidence for shared facts. External workbook/ODBC references are definitions only and are never dereferenced.
- PPTX uses Office Oxide for typed slide/text/table/image/link/note/group facts while raw PresentationML owns chart, SmartArt, and package-coverage facts.
- PDF requires both PDFium and lopdf to open accepted documents. The explicit parser-disagreement fixture returns `ParserDisagreement`; there is no “trust one parser” winner heuristic.
- Cross-format authority migration is capability-based. `PRESENT / ABSENT / NOT_REPRESENTABLE / NOT_VERIFIABLE` and equivalence fingerprints are evaluated per document; there is no format-pair blanket allowlist.

## 5. Signature evidence

Digital signatures remain outside semantic identity.

Qualified evidence covers:

- detached CMS valid/tampered/invalid-digest/expired/revoked/unknown-issuer/broken-chain/unsupported-algorithm/malformed cases;
- XMLDSig valid/invalid/unverifiable cases through xml-sec;
- exact PDF ByteRange reconstruction before detached CMS verification;
- OOXML OPC `digital-signature/origin -> signature` relationship traversal for DOCX/XLSX/PPTX;
- explicit caller-provided trust anchors and offline CRL bytes only.

No system trust, network CRL, OCSP, AIA retrieval, or macro execution is enabled. An invalid or unverifiable signature is persisted as evidence rather than converted into semantic success/failure; later Publish policy may fail closed on that evidence.

## 6. Determinism and sandbox evidence

For every successful fixture:

- 20 in-process executions produced identical semantic fingerprint, projection, capability, editorial, dependency, and signature evidence.
- 5 fresh child processes produced identical normalized snapshots across `TZ` / `LANG` variations.

Hostile/resource tests execute through `scripts/run-sandboxed-case.sh`:

- CPU limit: 8 seconds;
- file limit: 2048 blocks;
- Linux virtual-memory limit: 2,097,152 KiB;
- controller timeout test terminates a deliberately stuck child;
- representative malformed/deep/oversized/invalid-VBA inputs return controlled non-zero errors;
- no partial success file is left behind;
- representative fixture-body fragments are absent from stderr/report output.

macOS applies the CPU/file limits; Linux additionally applies the virtual-memory limit because `ulimit -v` is not portable to the hosted macOS shell.

## 7. Unsupported/fail-closed boundaries

The PoC explicitly rejects or classifies, rather than silently succeeding on:

- ambiguous TXT decode;
- inconsistent or delimiter-ambiguous CSV;
- script-required HTML semantics;
- unknown semantic OOXML content/relationships;
- malformed/deep OOXML and oversized package entries;
- incomplete or syntactically invalid VBA;
- scan-only PDF -> `RequiresOcr`;
- encrypted PDF -> `EncryptedContentUnsupported`;
- broken PDF xref;
- PDF semantic parser disagreement;
- ambiguous PDF read structure.

OCR, external-reference dereference, VBA execution, and production document conversion remain outside this PoC.

## 8. Fixture evidence

All manifest cases below passed their declared relation/error expectation at the qualification head.

| Fixture | Class | Format | Expected | Result |
|---|---|---|---|---|
| `txt/base` | BASE | TXT | success | PASS |
| `txt/crlf` | NOISE | TXT | same as `txt/base` | PASS |
| `txt/unicode-nfd` | NOISE | TXT | same as `txt/base` | PASS |
| `txt/text-change` | SEMANTIC | TXT | different from `txt/base` | PASS |
| `txt/ambiguous-decode` | HOSTILE | TXT | error `semantic_extraction_failed` | PASS |
| `csv/base` | BASE | CSV | success | PASS |
| `csv/quote-noise` | NOISE | CSV | same as `csv/base` | PASS |
| `csv/cell-change` | SEMANTIC | CSV | different from `csv/base` | PASS |
| `csv/row-change` | SEMANTIC | CSV | different from `csv/base` | PASS |
| `csv/inconsistent` | HOSTILE | CSV | error `semantic_extraction_failed` | PASS |
| `csv/delimiter-ambiguous` | HOSTILE | CSV | error `unsupported_semantic_construct` | PASS |
| `html/base` | BASE | HTML | success | PASS |
| `html/noise` | NOISE | HTML | same as `html/base` | PASS |
| `html/text-change` | SEMANTIC | HTML | different from `html/base` | PASS |
| `html/link-change` | SEMANTIC | HTML | different from `html/base` | PASS |
| `html/image-change` | SEMANTIC | HTML | different from `html/base` | PASS |
| `html/js-only` | HOSTILE | HTML | error `unsupported_semantic_construct` | PASS |
| `docx/base` | BASE | DOCX | success | PASS |
| `docx/body-text-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/comment-resolved` | EDITORIAL | DOCX | same as `docx/base` | PASS |
| `docx/comment-unresolved` | EDITORIAL | DOCX | same as `docx/base` | PASS |
| `docx/deep-ooxml` | HOSTILE | DOCX | error `inspection_resource_limit_exceeded` | PASS |
| `docx/endnote-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/font-only` | NOISE | DOCX | same as `docx/base` | PASS |
| `docx/footer-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/footnote-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/header-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/heading-list-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/hyperlink-target-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/image-content-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/malformed` | HOSTILE | DOCX | error `semantic_extraction_failed` | PASS |
| `docx/metadata-noise` | NOISE | DOCX | same as `docx/base` | PASS |
| `docx/package-order-noise` | NOISE | DOCX | same as `docx/base` | PASS |
| `docx/relationship-id-noise` | NOISE | DOCX | same as `docx/base` | PASS |
| `docx/section-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/table-merge-change` | SEMANTIC | DOCX | different from `docx/base` | PASS |
| `docx/tracked-replacement` | EDITORIAL | DOCX | same as `docx/base` | PASS |
| `docx/unknown-semantic-part` | HOSTILE | DOCX | error `unsupported_semantic_construct` | PASS |
| `xlsx/base` | BASE | XLSX | success | PASS |
| `xlsx/formula-source-change-same-cache` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/cached-result-only` | NOISE | XLSX | same as `xlsx/base` | PASS |
| `xlsx/very-hidden-change` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/style-only` | NOISE | XLSX | same as `xlsx/base` | PASS |
| `xlsx/cell-value-change` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/defined-name-change` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/merged-change` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/hyperlink-change` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/xml-order-noise` | NOISE | XLSX | same as `xlsx/base` | PASS |
| `xlsx/unknown-semantic-part` | HOSTILE | XLSX | error `unsupported_semantic_construct` | PASS |
| `xlsm/base` | BASE | XLSM | success | PASS |
| `xlsx/two-sheet-base` | BASE | XLSX | success | PASS |
| `xlsx/sheet-add` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/sheet-order-change` | SEMANTIC | XLSX | different from `xlsx/two-sheet-base` | PASS |
| `xlsx/table-add` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/external-reference-add` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/chart-add` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/image-add` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/odbc-connection-add` | SEMANTIC | XLSX | different from `xlsx/base` | PASS |
| `xlsx/sheet-remove` | SEMANTIC | XLSX | different from `xlsx/two-sheet-base` | PASS |
| `pptx/base` | BASE | PPTX | success | PASS |
| `pptx/slide-add` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/slide-remove` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/slide-order-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/shape-association-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/table-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/chart-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/smartart-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/image-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/hyperlink-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/speaker-note-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/group-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/theme-only` | NOISE | PPTX | same as `pptx/base` | PASS |
| `pptx/font-only` | NOISE | PPTX | same as `pptx/base` | PASS |
| `pptx/background-only` | NOISE | PPTX | same as `pptx/base` | PASS |
| `pptx/id-order-noise` | NOISE | PPTX | same as `pptx/base` | PASS |
| `pptx/unknown-semantic-part` | HOSTILE | PPTX | error `unsupported_semantic_construct` | PASS |
| `pptx/text-change` | SEMANTIC | PPTX | different from `pptx/base` | PASS |
| `pptx/comment-only` | EDITORIAL | PPTX | same as `pptx/base` | PASS |
| `pdf/base` | BASE | PDF | success | PASS |
| `pdf/object-id-producer-noise` | NOISE | PDF | same as `pdf/base` | PASS |
| `pdf/text-change` | SEMANTIC | PDF | different from `pdf/base` | PASS |
| `pdf/page-order-change` | SEMANTIC | PDF | different from `pdf/base` | PASS |
| `pdf/link-change` | SEMANTIC | PDF | different from `pdf/base` | PASS |
| `pdf/annotation-change` | EDITORIAL | PDF | same as `pdf/base` | PASS |
| `pdf/form-value-change` | SEMANTIC | PDF | different from `pdf/base` | PASS |
| `pdf/image-change` | SEMANTIC | PDF | different from `pdf/base` | PASS |
| `pdf/scan-only` | HOSTILE | PDF | error `requires_ocr` | PASS |
| `pdf/broken-xref` | HOSTILE | PDF | error `semantic_extraction_failed` | PASS |
| `pdf/encrypted` | HOSTILE | PDF | error `encrypted_content_unsupported` | PASS |
| `pdf/parser-disagreement` | HOSTILE | PDF | error `parser_disagreement` | PASS |
| `pdf/ambiguous-read-order` | HOSTILE | PDF | error `unsupported_semantic_construct` | PASS |

Additional non-manifest signature and XLSM/VBA vectors are covered by their dedicated test suites and by the promotion-gate supplemental counts above.

## 9. Promotion decision

All frozen v0 required semantic-inspection formats passed the qualification gate:

- DOCX
- XLSX
- XLSM
- PPTX
- native-text PDF
- TXT
- CSV
- HTML

Therefore the PoC result is:

> **POC QUALIFICATION COMPLETE / PRODUCTION PLAN REQUIRED**

This means the selected composition may be used as the basis for a new **Document Semantic Inspection v0 Production Implementation Plan**. It does **not** mean the PoC dependencies have already been promoted into root production crates, nor does it authorize production implementation inside this PoC PR.
