# Document Semantic Inspection v0 — Execution Status

- Capability: `Document Semantic Inspection v0`
- Execution mode: **Inline Execution**
- Overall phase: **PRODUCTION IMPLEMENTATION — TASK 5 RED SUPPLEMENT / SELECTED RUNTIME BLOCKER**
- Design path: **Architectural**
- Frozen Design merged: PR #7
- PoC execution branch: `test/document-semantic-inspection-poc-v0`
- Production planning branch: `plan/document-semantic-inspection-v0-production`
- Production planning PR: **#9 MERGED — `48045768d1d026eb785ee065877e401bbafd97ca`**
- Production implementation branch: `feat/document-semantic-inspection-v0`
- Production implementation PR: **#10 (Draft)**
- Current production implementation head: `9cb472d5f7a81a1f4db803305df3137683375788`
- PoC execution PR: **#8 (MERGED — `ab9ad6f9949128360e46fed07aca335bb6b10971`)**
- Production implementation baseline: `main@48045768d1d026eb785ee065877e401bbafd97ca`
- Baseline main CI: `36079233862` — **SUCCESS**
- Last PoC-qualified code head: `a4fcef1cb5cac5672199165f433bd303c25135a6`
- Task 3 dependency-preflight candidate head: `ec532ec12d89352d83dc9a85ae68a3da583c0ebb`
- Task 3 dependency-preflight DSI run: `35545142423` — **SUCCESS**
- Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Design approval: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- PoC Qualification Plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
- Production Implementation Plan: `docs/superpowers/plans/2026-09-24-document-semantic-inspection-v0-production-implementation.md` — **APPROVED 2026-09-25**

## Approval state

- Frozen Design: **APPROVED / FROZEN**
- PoC Qualification Plan: **APPROVED 2026-09-21**
- Production Implementation Plan: **APPROVED 2026-09-25**
- Production dependency promotion: **Task 1 sandbox dependencies and Task 4 TXT/CSV/HTML dependencies promoted through their approved gates; Task 5 dependencies not promoted**

PR #7 was advanced from review to approved state and merged after explicit user approval. The execution baseline is the resulting main merge commit `5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`.


## Production implementation state — current

The PoC is complete and all eight required format gates passed. A separate Production Implementation Plan now exists on `plan/document-semantic-inspection-v0-production`.

Current gates:

- Production Implementation Plan: **APPROVED 2026-09-25**
- Production planning PR #9: **MERGED**
- PR #8: **MERGED — `ab9ad6f9949128360e46fed07aca335bb6b10971`**
- Production dependency promotion: **Task 1 sandbox and Task 4 TXT/CSV/HTML dependencies promoted; Task 5 dependencies not promoted**
- Production implementation branch: **CREATED — `feat/document-semantic-inspection-v0`**
- Production Task 1 sandbox preflight: **COMPLETE / PASS**
- Production core crate: **TASK 2 COMPLETE / PASS**
- Production runtime: **TASKS 1–4 COMPLETE / PASS; Task 5 initial RED exists, but review gaps must be supplemented before GREEN**

The production-hardening gap is resolved for the frozen v0 profile. Task 1 preflight qualified and promoted the sandbox substrate, and its runtime enforces the no-network, no-credential, filesystem-confinement, fresh-process, and finite resource-profile boundary.

Required next order:

1. After the explicitly selected `gpt-6-luna/max` managed runtime responds, resume run `dsi-prod-task5-red-evidence-v3-20260925` with the same recorded spec; its current state has no pending operation and no write receipts.
2. Modify only `crates/document-semantic-inspection-worker/tests/docx_format_parity.rs` to assert tracked-change/comment `source_locator`, metadata-noise `last_modified_by`, and list/section order. Do not add production code or dependencies.
3. Commit/push the supplemental RED and require exact-head standard CI, Sandbox Preflight, DSI PoC, and independent review before Task 5 GREEN.
4. Continue Tasks 5–14 in plan order with only explicitly qualified dependencies; do not merge PR #10.

## Production Task 5 — DOCX RED REVIEW GAPS / MANAGED RUNTIME BLOCKER

- Task 5 implementation code head: `9cb472d5f7a81a1f4db803305df3137683375788`; status-only handoff commit pushed: `c6cd0a915758c62a58951c4a2f50ff8d31d2434c`; worktree clean.
- GitHub PR #10: OPEN / Draft; the current branch ref and workflow PR association confirm head `c6cd0a915758c62a58951c4a2f50ff8d31d2434c`; no merge authorized.
- Exact-head standard CI `36114655294`: FAILURE on the unimplemented Task 5 adapter/sentinel contract; security, policy, macOS portability, and container-build jobs passed, while `rust-static` and `rust-test` failed on the expected missing Task 5 APIs.
- Exact-head DSI Sandbox Preflight `36114655281`: SUCCESS. Exact-head DSI PoC regression `36114655219`: SUCCESS.
- Status-only handoff head `c6cd0a915758c62a58951c4a2f50ff8d31d2434c`: CI `36124450393` — expected Task 5 RED failure; Sandbox `36124450533` — SUCCESS; DSI PoC `36124450545` — SUCCESS.
- Independent read-only review: NO-GO for Task 5 GREEN until tests assert tracked-change/comment source locators, `metadata-noise` editorial `last_modified_by`, list-item order, and section order.
- Task 5 production dependencies have not been promoted; no Task 5 GREEN implementation has started.
- First RED evidence run `dsi-prod-task5-red-evidence-20260925` is preserved: source inventory was 183,311 bytes against a 140,000-byte startup budget; a split was rejected at `max_tasks=1`; zero write receipts. Do not resume that run.
- Run `dsi-prod-task5-red-evidence-v2-20260925` ended with `app-server timeout` at its 15-minute deadline; requested/resolved model `gpt-6-luna`, reported input 18,361 tokens, no write receipts.
- Read-only route check `dsi-worker-route-health-20260925-1` also ended at its 3-minute deadline; it is not evidence that the model is unavailable.
- Run `dsi-prod-task5-red-evidence-v3-20260925` used `gpt-6-luna/max`, with 60-minute total time, 100k input threshold, and three allowed fresh-session rotations. Two fresh workers failed before their first tool call with `turn failed or stopped without interrupt acknowledgement`; both resolved to `gpt-6-luna`, reported no input usage, and produced no write receipts. Current run state has `pending=null`, task incomplete, and no repository changes.
- Local `gh` API requests timed out. GitHub REST workflow records associate handoff head `c6cd0a915758c62a58951c4a2f50ff8d31d2434c` with PR #10; `git ls-remote` confirms the branch ref. The PR metadata endpoint still returned the prior head during the same lookup.

Next exact action: once the selected `gpt-6-luna/max` runtime responds, run `toolbox-context resume --workspace "/Users/airisu/.codex/worktrees/dsi-v0-production-task4/knowledge-platform" --run dsi-prod-task5-red-evidence-v3-20260925`; then verify the one-file RED patch, commit/push it, collect all three exact-head CI results, and obtain independent review before GREEN.

## Production Task 4 — COMPLETE

- Initial RED head: `9ed0f735474ff25381814cbad63ef2c5965c76f9`
- Initial standard CI: `36095951774` — **FAIL**, with the intended unresolved adapter imports plus a formatting failure
- Clean RED head: `64e1419a43d45c178e2f85cb1825ea9a1ff27aad`
- Clean RED standard CI: `36097679945` — **FAIL as expected only on unresolved Task 4 adapter contract imports; fmt and policy/security checks pass**
- Clean RED Sandbox regression: `36097679887` — **SUCCESS**
- Clean RED DSI PoC regression: `36097679960` — **SUCCESS**
- First GREEN head: `50578e1f4722a2a5d461f5e13a1227d177b0ff3c`; standard CI `36099520593` found `clippy::collapsible_if` in the new text adapter; Sandbox `36099520660` and DSI PoC `36099520656` were **SUCCESS**
- Final GREEN head: `963a144add686b32afa510cf43a4ecce967f042b`
- Final standard CI: `36099955617` — **SUCCESS**, including required-check
- Final DSI Sandbox Preflight regression: `36099955599` — **SUCCESS**
- Final DSI PoC regression: `36099955606` — **SUCCESS**
- Text-format parity tests: **4/4 PASS**
- Worker contract tests: **12/12 PASS**
- Production dependencies promoted: `encoding_rs 0.8.41`, `unicode-normalization 0.1.25`, `csv 1.4.0`, `html5ever 0.39.0`, `markup5ever_rcdom 0.39.0`; `scraper` was not added
- CSV keeps the PoC's explicit delimiter contract and fails closed when a delimiter is not supplied; HTML never executes JavaScript and script-required profiles fail closed
- PR #10 remains **OPEN / Draft / mergeable**; unresolved review threads: **0**
- No Design amendment was required

## Production Task 3 — COMPLETE

- Initial Task 3 contract head: `a3c695276073532308ef3e57833d256b64d712ab`
- Authoritative shell RED head: `1911a80b0e018119cb4c2c94161dc72a24641c00`
- Authoritative shell RED standard CI: `36094572575` — **FAIL as expected**
- RED failure: formatting passed, then `check:rust` failed on unresolved import `document_semantic_inspection_worker::run_worker_shell`.
- GREEN worker-shell head: `817c25330f5348b2ab2b0683141329241b3be3f2`
- Standard CI: `36094896067` — **SUCCESS**, including required-check
- DSI Sandbox Preflight regression: `36094896150` — **SUCCESS**
- DSI PoC regression: `36094896352` — **SUCCESS**
- Workspace tests: **100/100 PASS**
- Worker contract tests: **12/12 PASS**
- PR #10 unresolved review threads: **0**
- Worker protocol remains free of FileId / DocumentId / DocumentVersionId / Principal / StorageKey / DB credentials / storage credentials.
- Worker binary consumes bounded JSON request from stdin plus inherited read-only input FD.
- Worker recomputes raw SHA-256 and size before semantic work and rejects raw-binding mismatch.
- Format detection uses bytes/container structure plus declared media compatibility, never filename extension.
- Malformed request, unsupported/mismatched format, panic, and pre-adapter semantic paths fail closed with no partial success on stdout.
- Extractor provenance carries build / adapter / parser-library / native dependency identities and remains outside semantic fingerprint.
- Production parser dependencies: **NOT PROMOTED**.

## Production Task 2 — COMPLETE

- RED contract head: `d0bab4a824b6f125a20a55edd9fec21b53233fb3`
- RED standard CI: `36086136455` — **FAIL as expected**
- RED failure: after formatting passed, `check:rust` / `test:rust` failed because the contract test imported the not-yet-implemented core types/functions; representative error was unresolved imports from `document_semantic_inspection_core`.
- GREEN implementation head: `d0ea866b9f7ae28532304705a3a689f030e7f2d2`
- Final formatted GREEN head: `550913b93a2c37360544450ff9dc53164679cf8a`
- Standard CI: `36086709602` — **SUCCESS**, including required-check
- Sandbox regression: `36086709619` — **SUCCESS**
- DSI PoC regression: `36086709653` — **SUCCESS**
- Core contract tests: **7/7 PASS**
- Contract covers: `dsi-v0` profile, eight required formats, SHA-256/32-byte fingerprint, capability/evidence wire shape, worker-safe request fields, deterministic canonicalization, unknown protocol rejection, bounded result decode, and versioned golden snapshot.
- Core crate remains infrastructure-free; no parser, SQL, storage, or OS sandbox dependency was added.
- Production parser dependency promotion: **NOT STARTED**

## Production Task 1 — COMPLETE

- Baseline: `main@48045768d1d026eb785ee065877e401bbafd97ca`
- RED head: `343aa9072da19da471a31b96e05eb92d80784820`
- RED hosted run: `36080157697` / job `107900137601` — **FAIL as expected**
- RED failure: unresolved sandbox launcher contract imports
- Qualified sandbox code/build head: `0cd3345a12f53f30068c72e56ea8aead367cd0ff`
- Hosted GREEN run before final lock/docs: `36084114757` — **SUCCESS**
- Resource profile tests: **4/4 PASS**
- Sandbox contract tests: **9/9 PASS**
- cargo-deny advisories/bans/licenses/sources: **PASS**
- Selected composition: `landlock 0.4.7` + `seccompiler 0.5.0` + `libc 0.2.189` + `thiserror 2.0.21`
- Independent sandbox `Cargo.lock`: **COMMITTED**
- Every required production resource class: **FINITE / TESTED**
- Selection documents: **UPDATED**
- Production parser dependencies: **NOT PROMOTED**

Final exact-head verification is required after the Task 1 completion documentation/selection changes. The authoritative report is `docs/superpowers/execution/document-semantic-inspection-v0-sandbox-preflight.md`.

## Task 1 — COMPLETE

Isolated workspace, deterministic harness, manifest validation, report output, root mise entrypoints, and path-scoped hosted DSI PoC CI are implemented.

TDD / CI evidence:

- RED commit: `0650625db9ad0fd89f6292b6eddce3e4f6b48ac0`
- RED DSI run: `35520524705` — FAIL as expected; the first observed failure exposed an invalid independently-copied PoC lockfile before compile.
- Harness implementation commit: `1980d03b18ca90f4896ef17b2206f1013cd452e6`
- Exact locked Task 1 head: `c9ad1c9f5ec6e47c53f0409027bf98e3886b8252`
- DSI PoC run: `35520894240` — **SUCCESS**
- Standard CI run: `35520894277` — **SUCCESS**

The lockfile issue was repaired by generating the isolated workspace's own lock and committing it. The PoC crate is marked `publish = false`; the private experiment itself is therefore excluded from third-party license evaluation while dependencies remain enforced.

## Task 2 — COMPLETE

TXT / CSV / HTML semantic adapters and 17 synthetic qualification cases are implemented.

Behavior pinned by tests:

- TXT: CRLF and canonical Unicode noise are invariant; content changes differ; ambiguous encoding fails closed.
- CSV: quote syntax noise is invariant; row/cell changes differ; inconsistent columns and missing explicit delimiter fail closed.
- HTML: whitespace/decorative attributes are invariant; visible text/link/image changes differ; script-required semantics fail closed without script execution.

TDD / qualification evidence:

- RED commit: `f3b62f9ac413658ece3d80e5843f25009c1db0f4`
- RED DSI run: `35521340386` — FAIL as expected with unresolved `TextAdapter`, `CsvAdapter`, and `HtmlAdapter`.
- Initial GREEN candidate: `d412dc1c0a53f4d7647c394b1bf24814bbe1c6e3`
- Initial GREEN DSI run: `35521588349` — semantic tests and `dsi-poc verify` passed all 17 cases, but `cargo-deny` rejected `scraper 0.27.0` because its transitive `cssparser/selectors` graph contains MPL-2.0.
- Replacement commit: `b14b7954cf64295ac808b96e3ef65c698ad9def2`
- Replacement DSI run: `35521818109` — **SUCCESS** using direct `html5ever 0.39.0 + markup5ever_rcdom 0.39.0`.
- Exact locked Task 2 head: `b9da1bfa5fdf07f1b99a13248fe3233fae1082c9`
- Exact locked DSI run: `35521964417` — **SUCCESS**

### HTML candidate decision

`scraper 0.27.0` is **REJECTED for this repository** under the existing dependency-license policy. This is a qualification result, not a Design semantic change.

The accepted Task 2 HTML PoC substrate is:

- `html5ever = "=0.39.0"`
- `markup5ever_rcdom = "=0.39.0"`

The same frozen HTML semantic fixtures were retained across the candidate swap.

## Task 3 — DEPENDENCY PREFLIGHT COMPLETE

Candidate selection evidence:

- `stemma 0.5.0` — **REJECTED**: vulnerable transitive Quick-XML line; legacy ZIP graph also violates the repository license gate.
- `docx-review-core 0.1.1` — **REJECTED**: vulnerable transitive Quick-XML line.
- `docxml 0.3.1` — **REJECTED**: default ZIP codec graph includes license expressions outside the current allowlist.
- `office_oxide 0.1.11` + direct `zip 8.6.0` deflate-only + `quick-xml 0.42.0` — **PREFLIGHT PASS**.
- DSI PoC run `35545142423`: existing 17 semantic cases, CLI verification, advisories, license and source gates all passed with this candidate graph.

Ruling: do not weaken security/advisory/license policy to preserve a planned parser name. Keep the frozen semantic contract and qualify the replacement candidate against the same DOCX fixtures.

## Task 3 RED evidence

- RED contract head: `21fea55cffebcb53dac5886ffedcbb923bc19cd5`
- DSI PoC run: `35546037242` — **FAIL as expected**
- Exact failure: unresolved import `document_semantic_inspection_poc::DocxAdapter`; fixture/manifest/raw-binding validation introduced no earlier failure.

## Task 3 — COMPLETE

Fresh exact-head qualification evidence:

- Qualified head: `b4dae3c89fa84ce50deada7f268aa5b04830da5d`
- DSI PoC run: `35551781760` — **SUCCESS**
- Standard CI: **SUCCESS** at the same head
- DOCX tests: **13/13 PASS**
- Manifest verification: **38 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- Determinism: 20 in-process repetitions per successful DOCX case plus 5 fresh-process snapshots **PASS**
- Hostile OOXML: relationship cycles, traversal, duplicate entries, archive-bomb/resource cases fail closed as required
- Editorial evidence: tracked-change details and resolved/unresolved comment state preserved separately from semantic identity

No Design amendment was required. `office_oxide 0.1.11` remains a PoC-qualified candidate only; no production dependency promotion has occurred.

## Task 4 — DEPENDENCY PREFLIGHT COMPLETE

Dependency evidence:

- `rxls = 0.1.3`
- `calamine = 0.36.1` with picture support
- `ovba = 0.7.1`
- `tree-sitter = 0.25.10`
- VBA grammar: exact upstream revision `c691f237b2a703732d4b6a1f01d5b4f73f94d41e`, vendored generated parser because the upstream Rust-package bindings are incomplete
- DSI PoC run `35606038480` — **SUCCESS**
- Existing DOCX/TXT/CSV/HTML suite remained green: manifest verification **38 cases PASS**
- cargo-deny: advisories/bans/licenses/sources **PASS**

The initial direct git-crate attempt failed at compile time only because the upstream commit references a missing `bindings/rust/build.rs`; this is recorded as packaging failure, not a grammar semantic failure.

## Task 4 — RED COMPLETE

Fixture / RED evidence:

- Independent raw SpreadsheetML XLSX corpus added without using rxls or Calamine serialization.
- Calamine synthetic `tests/vba.xlsm` imported from commit `0af05f4f6030351e3b8a999ea0810c8618368776` with MIT provenance.
- Local XLSM seed SHA-256: `2fe9f89f4a969658c1e3f9b0e8c70ccb155840aa6ee1bad6b15df603f41da1c0`.
- RED head: `fe1abea83d2669c1a847203cdcfe84926acbdb4f`
- DSI PoC run: `35607642860` — **FAIL as expected**
- Exact failure: unresolved imports `SpreadsheetAdapter` and `VbaAdapter`; dependency graph and existing suites compiled before the RED failure.

Ruling: represent format-specific instances as `SpreadsheetAdapter::XLSX` and `SpreadsheetAdapter::XLSM`, because the shared `InspectionAdapter` trait exposes exactly one `FormatId` per instance.

## Task 4 — COMPLETE

Fresh exact-head qualification evidence:

- Qualified code head: `cefd042776b64cd80b0a009989eafd48ef5e256d`
- DSI PoC run: `35818792833` — **SUCCESS**
- Standard CI run: `35818792843` — **SUCCESS**
- Spreadsheet/VBA tests: **8/8 PASS**
- Manifest verification: **59 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- XLSX semantics: sheet add/remove/order and visibility, cells, formula source vs cached-value noise, defined names, merges, tables, hyperlinks, charts, images, and external definitions are fingerprinted
- Differential oracle: rxls vs Calamine agreement required for positioned sheet metadata, non-formula cell values, formula source, defined names, and hyperlinks
- XLSM/VBA: real macro container mutated locally; comment/whitespace-only changes remain invariant, logic changes differ, invalid syntax fails closed
- External references: external-workbook and ODBC definitions remain evidence only; no dereference/connection is performed
- Unknown potentially semantic package content/relationships fail closed

No Design amendment was required. Task 4 dependencies remain PoC-only; production promotion remains prohibited.

## Task 5 — DEPENDENCY PREFLIGHT COMPLETE

Candidate evidence:

- `pptx 0.1.0` — **REJECTED**: `quick-xml 0.39.4` is affected by RUSTSEC-2026-0194 and RUSTSEC-2026-0195.
- `powerpoint-ooxml 1.0.0` — **REJECTED**: mandatory `opc-ooxml 1.0.0 -> zip ^8` default codec graph violates the current license allowlist.
- Planned-candidate preflight DSI run: `35819356362` — FAIL at cargo-deny; existing semantic verification still passed 59 cases.
- Replacement: existing qualified `office_oxide 0.1.11` PPTX reader + independent raw PresentationML oracle/sentinel.
- Replacement exact locked DSI run: `35819760081` — **SUCCESS**.
- No Task 5 dependency was promoted or added after the ruling.

Ruling: preserve the frozen PPTX semantic contract and reject unsafe/non-compliant parser candidates rather than weakening security/license gates.

## Task 5 — RED COMPLETE

- RED contract head: `e0c0011b92e95ce81cb95da0a4cb9edcf294c7b0`
- DSI PoC run: `35823478842` — **FAIL as expected**
- Exact failure: unresolved import `document_semantic_inspection_poc::PptxAdapter`; fixture/manifest binding introduced no earlier failure.

## Task 5 — COMPLETE

Fresh exact-head qualification evidence:

- Qualified code head: `eb09b72ac64a35c2ef503df38570a1d34d38a7b0`
- DSI PoC run: `35824677799` — **SUCCESS**
- Standard CI run: `35824677794` — **SUCCESS**
- PPTX tests: **7/7 PASS**
- Manifest verification: **78 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- PPTX corpus: **19 cases**
- Semantic identity covers visible text, slide existence/order, text/shape association, tables, chart series/data, SmartArt meaning, images, hyperlinks, speaker notes, and grouping/object relationships.
- Noise invariance covers theme/font/background-only edits plus internal shape/relationship IDs and package ordering.
- Comment-only edits preserve semantic identity while emitting editorial evidence.
- Unknown potentially semantic content types/relationships fail closed.
- `office_oxide 0.1.11` supplies typed slide/text/table/image/link/note/group facts; required chart/SmartArt semantics remain owned by the independent raw PresentationML oracle.

No Design amendment was required and no Task 5 runtime dependency was added after the preflight ruling.

## Task 6 plan consistency

The stale `chromium/8057` references in Task 6 were corrected to `chromium/7881`. The frozen Global Constraints already specify PDFium `151.0.7881.0`, its exact Linux/macOS hashes, and the `pdfium_7881` API supported by `pdfium-render 0.9.4`. This is a plan consistency repair, not a Design change.

## Task 6 — COMPLETE

Fresh exact-head qualification evidence:

- Qualified code head: `7f2dcbfa186d11d66e633fefb2c0bfc629fb7f6a`
- DSI PoC run: `35880533515` — **SUCCESS**
- Standard CI run: `35880533521` — **SUCCESS**
- Hosted targets: Ubuntu 24.04, macOS 15 Intel, macOS 15 arm64 — **all SUCCESS**
- PDF tests: **7/7 PASS** on each hosted target
- Manifest verification: **91 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- Native engine: PDFium `151.0.7881.0` / `chromium/7881`; each platform artifact SHA-256 verified before extraction
- Dual-engine policy: required PDFium/lopdf fact disagreement returns `ParserDisagreement`; no winner heuristic
- Fail-closed: scan-only -> `RequiresOcr`; encrypted -> `EncryptedContentUnsupported`; broken structure -> `SemanticExtractionFailed`; ambiguous text order is rejected
- Editorial separation: PDF annotation-content changes are preserved as editorial evidence without changing semantic identity where the frozen contract classifies them as editorial

No Design amendment was required. PDF dependencies and native runtime remain PoC-only.

## Task 7 — DEPENDENCY PREFLIGHT IN PROGRESS

Initial preflight result:

- Planned `pkix-chain 0.1.1` — **REJECTED** because the crates.io release is yanked and cannot participate in a newly generated lock.
- Failed DSI run: `35936215206` — lock generation stopped before compilation/deny evaluation.
- Replacement candidate: `pkix-chain 0.4.1` with `crl` and `ocsp` features.
- Upstream 0.4.1 is aligned to `pkix-path 0.3.2`, `pkix-revocation 0.3.3`, and `x509-cert 0.2`; no semantic-policy change is required.
- Offline-only revocation boundary remains mandatory.

### PKIX security preflight result

- `pkix-chain 0.1.1`: **REJECTED** — yanked.
- `pkix-chain 0.4.1 + pkix-path 0.3.2`: **REJECTED** under the unchanged advisory gate.
- DSI run `35936602764`: lock generation and compilation succeeded; cargo-deny failed on `rsa 0.9.10` via `pkix-path 0.3.2`, RUSTSEC-2023-0071.
- No advisory ignore/exception will be added.
- Replacement preflight candidate: `openssl 0.10.81` with `vendored`, retaining `cms 0.2.3`, `x509-cert 0.2.5`, and `xml-sec 0.1.16`.
- Trust/chain/revocation remains offline-only: explicit trust anchors and CRL fixture bytes; no system trust and no network fetch.

## Task 7 — COMPLETE

Qualification evidence:

- Exact qualified head: `83dacc87cbfa02e85fee765d46ddcdcf63e5e6cc`
- DSI PoC run: `35947786029` Linux job — **SUCCESS**
- Signature tests: **7/7 PASS**
- Existing semantic manifest: **91 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- `pkix-chain 0.1.1`: **REJECTED** — yanked
- `pkix-chain 0.4.1 + pkix-path 0.3.2`: **REJECTED** — `rsa 0.9.10` triggers RUSTSEC-2023-0071
- Qualified PoC verification substrate: `xml-sec 0.1.16` + `cms 0.2.3` + `x509-cert 0.2.5` + `openssl 0.10.81` vendored
- Trust boundary: explicit caller-supplied anchors; offline CRL evidence only; no system trust and no network CRL/OCSP/AIA retrieval
- CMS negative vectors cover tamper, digest, time, revocation, unknown issuer, broken chain, unsupported algorithm, malformed signature
- XMLDSig vectors cover valid/invalid/unverifiable classes using xml-sec
- PDF ByteRange wrapper verifies exact covered bytes before CMS validation
- OOXML OPC signature wrapper follows `digital-signature/origin -> signature` relationships and is exercised against DOCX/XLSX/PPTX
- Signature evidence remains outside semantic identity and preserves signer/certificate/coverage/diagnostic fields

The standard CI policy/security/container/rust-static/rust-test jobs on this head are green; macOS portability and DSI macOS jobs are runner-queued and are intentionally consolidated into Task 8's cross-host final gate.

No Design amendment was required and no production dependency promotion is authorized.

## Task 8 — COMPLETE

Qualification evidence at code head `4232facae820e5914d4c9e4ed2433f58396bc2c5`:

- DSI PoC run `35952274108` Ubuntu qualification — **SUCCESS**
- Standard CI run `35952274070` — **SUCCESS**
- Cross-format capability tests: **4/4 PASS**
- Determinism/security tests: **5/5 PASS**
- Semantic manifest: **91 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- Final machine report: **overall PASS**
- Required formats: TXT / CSV / HTML / DOCX / XLSX / XLSM / PPTX / PDF — **all PASS**
- Every per-format gate: `promotion_eligible=true`
- Determinism: 20 in-process repetitions for every successful fixture; 5 child-process snapshots under TZ/LANG variation
- Sandbox/resource evidence: hard CPU/file/VM limits, controller timeout, malformed/deep/oversized/invalid-VBA cases, no partial success output, no representative fixture-body leakage
- Cross-format migration uses capability-state/equivalence evidence; there is no format-pair blanket allowlist
- PR #8 has been moved out of Draft; unresolved review threads: **0**

Final cross-host evidence at the same head:
- DSI PoC run `35957940553`: Ubuntu `107500145955`, macOS Intel `107500146107`, macOS arm64 `107500146179` — **all SUCCESS**
- Standard CI run `35957940565` — **SUCCESS**, including required-check

## Task 9 — QUALIFICATION REPORT / SELECTION UPDATE COMPLETE

- Human report: `docs/superpowers/execution/document-semantic-inspection-v0-poc-report.md`
- Machine result at qualification head: **overall PASS**
- Required formats: TXT / CSV / HTML / DOCX / XLSX / XLSM / PPTX / PDF — **all PASS**
- Selection documents updated from PoC evidence only.
- No production dependency was added to root production crates in this PoC.
- No unresolved blocking PR review finding existed before the final report update.

## Current gate / next exact action

**PoC Qualification and Production Tasks 1–4 are complete.**

Next exact action: begin Production Task 5 RED for DOCX semantics and the OOXML coverage sentinel. Use the qualified fixtures and keep Task 5 parser dependencies unpromoted until the clean RED evidence is recorded.

Do not implement production Semantic Inspection crates inside PR #8.
