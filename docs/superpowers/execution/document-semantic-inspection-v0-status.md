# Document Semantic Inspection v0 — Execution Status

- Capability: `Document Semantic Inspection v0`
- Execution mode: **Inline Execution**
- Overall phase: **PRODUCTION IMPLEMENTATION — TASK 5 COMPLETE / TASK 6 RED NEXT**
- Design path: **Architectural**
- Frozen Design merged: PR #7
- PoC execution branch: `test/document-semantic-inspection-poc-v0`
- Production planning branch: `plan/document-semantic-inspection-v0-production`
- Production planning PR: **#9 MERGED — `48045768d1d026eb785ee065877e401bbafd97ca`**
- Production implementation branch: `feat/document-semantic-inspection-v0`
- Production implementation PR: **#10 (OPEN / Draft)**
- Last verified production code head: `14bcc4a63ec4ec56289619e4d76a9ca1792315ba`
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
- Production dependency promotion: **Task 1 sandbox, Task 4 TXT/CSV/HTML, and Task 5 DOCX parser composition promoted through their approved gates**

PR #7 was advanced from review to approved state and merged after explicit user approval. The execution baseline is the resulting main merge commit `5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`.


## Production implementation state — current

The PoC is complete and all eight required format gates passed. A separate Production Implementation Plan now exists on `plan/document-semantic-inspection-v0-production`.

Current gates:

- Production Implementation Plan: **APPROVED 2026-09-25**
- Production planning PR #9: **MERGED**
- PR #8: **MERGED — `ab9ad6f9949128360e46fed07aca335bb6b10971`**
- Production dependency promotion: **Task 1 sandbox, Task 4 TXT/CSV/HTML, and Task 5 DOCX parser composition promoted through their approved gates**
- Production implementation branch: **CREATED — `feat/document-semantic-inspection-v0`**
- Production Task 1 sandbox preflight: **COMPLETE / PASS**
- Production core crate: **TASK 2 COMPLETE / PASS**
- Production runtime: **TASKS 1–5 COMPLETE / PASS; Task 6 XLSX/XLSM/VBA RED next**

The production-hardening gap is resolved for the frozen v0 profile. Task 1 preflight qualified and promoted the sandbox substrate, and its runtime enforces the no-network, no-credential, filesystem-confinement, fresh-process, and finite resource-profile boundary.

Required next order:

1. Begin Task 6 RED contract tests for XLSX/XLSM/VBA in the approved plan, preserving PoC-qualified semantics and exact-head evidence.
2. Promote only the Task 6 parser composition after clean RED and its own qualification gate.
3. Require standard CI, DSI Sandbox Preflight, and DSI PoC SUCCESS at the final Task 6 exact head. Keep PR #10 Draft and unmerged.

## Production Task 5 — DOCX COMPLETE

- Initial clean RED head: `65d6896322b93c6731f8a836be8b79fcf53beac5`; authoritative repaired clean RED head: `e3e6659d243233ff102c393e8be1515a05ba1398`.
- Authoritative repaired RED exact-head CI `36135132794`: **FAIL as expected on missing DOCX APIs only**. Sandbox `36135132761` and DSI PoC `36135132763`: **SUCCESS**.
- Supplemental test-only head: `bec052e43dfeedb049ac725f8c697cf564ec60b1`. Its tests assert tracked-change/comment `source_locator`, metadata-noise `last_modified_by`, list order, and section order. PR #10 is **OPEN / Draft**.
- Supplemental exact-head CI `36136763103`: **FAIL as expected**. `rust-static` and `rust-test` fail because `docx_semantic_edges.rs` imports the not-yet-implemented `DocxAdapter` and calls the missing `editorial_provenance()` API. `fmt`, policy, security, macOS portability, and container-build passed; required-check failed because its predecessor jobs failed.
- Supplemental DSI Sandbox Preflight `36136763068`: **SUCCESS**. DSI PoC `36136762918`: **SUCCESS**.
- Intermediate ZIP-preflight test-only head `593eddd14c15b098b377fc91b272239af1b24b12` matched the local, remote, and PR #10 head at that time. Its five cases failed as intended against the local pre-fix ZIP guard (**0/5 PASS**); `cargo fmt --check` and strict worker Clippy passed.
- ZIP-preflight exact-head CI `36138987613`: **FAIL as expected only on missing DOCX APIs**. `fmt`, policy, security, macOS portability, and container-build passed. Sandbox `36138987388` and DSI PoC `36138987376`: **SUCCESS**.
- Prior ZIP-ambiguity test-only head `9bc17d6ba0074e253266c99f8f59b1fee91dbf8d` added no dependency. Exact-head CI `36144055858`: **FAIL only on the planned missing `DocxAdapter`, `OoxmlCoverageSentinel`, and `editorial_provenance()` APIs**; `fmt`, policy, security, macOS portability, and container-build passed. Sandbox `36144056194` and DSI PoC `36144056202`: **SUCCESS**.
- Deep semantic test-only head `bae0c63a4c828b43da6a6a797bbdbefeb8161d4b` was a prior PR #10 head. Exact-head CI `36150028095`: **FAIL only on missing `DocxAdapter`, `OoxmlCoverageSentinel`, and `editorial_provenance()` APIs**; `fmt`, policy, security, macOS portability, and container-build passed. Sandbox `36150028138` and DSI PoC `36150028162`: **SUCCESS**. The commit contains only `docx_semantic_deep_a.rs` and `docx_semantic_deep_b.rs`, with no production dependency or implementation change.
- PoC PNG test-only RED head `3e29954c453bebeae3bab76240e4f6bc28fc58e8` was a prior PR #10 head. Exact-head DSI PoC `36151814090`: **FAIL only on identical decoded PNG pixels with different IDAT encoding**; DOCX 13/14 passed. Sandbox `36151814219`: **SUCCESS**. Standard CI `36151814139`: **FAIL only on missing production DOCX APIs**; formatting, policy, security, macOS portability, and container build passed. This commit changes only two PoC test/support files and adds no dependency.
- Corrected supplemental RED head `3fede76427991e0c63bc22c5f6615043c796486e` fixes a 1-bit PNG fixture's MSB-first pixel encoding. DSI PoC `36158858140`: **FAIL only on PNG IDAT equivalence** after DOCX 13/14; Sandbox `36158858241`: **SUCCESS**; standard CI `36158858108`: **FAIL only on missing production DOCX APIs** after fmt/policy/security/macOS/container pass.
- PNG decoder/checksum candidate head `5d627ac6a39b6971077b081fcdab8e35e599f26f` was intermediate: DSI PoC `36159892918` passed DOCX 14/14 then failed on referenced header/footer image tests 0/2. Sandbox `36159892901` succeeded; standard CI `36159892994` failed on missing production DOCX APIs.
- PoC nonbody-image GREEN head `e309cbeb9f9075c5c3b900df8905f8ae62490337`: DSI PoC `36161063199` **SUCCESS**, including PNG **12/12**, DOCX **14/14**, header/footer image **2/2**, VML image **1/1**, all **91 manifest cases**, and dependency/security gates. Sandbox `36161063009`: **SUCCESS**. Standard CI `36161063006`: **FAIL on planned missing production DOCX APIs** after fmt/policy/security/macOS/container success. Independent decoder/fixture review judged `png 0.18.1` qualified for the scoped DOCX pixel decoder. No production promotion is committed yet.
- Current PoC note test-only RED head `b2bc048f9bcfe0fe6a95523e3d46b6e64ba5a7a0`: DSI PoC `36162476424` **FAIL only on three new note image/table-structure tests**, after DOCX **14/14 PASS**. Sandbox `36162476347`: **SUCCESS**. Standard CI `36162476288`: **FAIL on missing production DOCX APIs**; policy/security/macOS/container succeeded. This head contains no production implementation or dependency change.
- Corrected PoC note/list-marker test-only RED head `dd62e881abcfd486e4d8ca02c9a35c2bf27e9704`: note-table comparison uses image-free notes so table structure is isolated, and two cases change `w:lvlText` / `w:start` with every other package part fixed. DSI PoC `36164163811` failed only the three note tests; Sandbox `36164163799` succeeded; CI `36164163875` failed on missing production DOCX APIs.
- PoC note-structure GREEN / list-marker RED head `0a1a017afcfdd8c675afde1319f384a0414d17a4`: DSI PoC `36166088658` passed note tests **5/5** and DOCX **14/14**, then failed only on list-marker tests **2/2**. Sandbox `36166088662` **SUCCESS**. CI `36166088667` passed fmt, policy, security, macOS, and container; `rust-static` / `rust-test` failed only on the expected missing production DOCX API.
- Current PoC note-reference test-only RED head `326d80ed369d695035f3889fe87bb853986ac643`: four focused local and hosted tests fail as intended, showing equal fingerprints after swapping two note IDs and accepted dangling references despite valid single-note baselines. DSI PoC `36166774786` passed DOCX **14/14** and note **5/5**, then failed only the four new note-reference cases. Sandbox `36166774731` **SUCCESS**. CI `36166774884` passed fmt/policy/security/macOS/container and failed only on the expected missing production DOCX APIs. This commit contains only the new PoC note-reference test.
- Supplemental PoC orphan-note/even-header test-only RED head `52c514d3540105e3acdcedc0caf315d48adcfd3a` adds only two PoC test changes. Both cases have valid accepted baselines and fail locally against the pre-fix source: an unreferenced ordinary note is accepted and even header content changes fingerprint with no `settings.xml`. Exact-head DSI PoC `36170274726` failed only on the new even-header case after DOCX 14/14 passed; the runner stops at the first failing test binary, so the orphan-note RED remains locally observed only. Sandbox `36170274935` succeeded. CI `36170274841` passed formatting, policy, security, macOS portability, and container build; rust-static/rust-test failed only on the planned missing production DOCX APIs.
- PoC semantic-boundary GREEN `0945e0870e70509628a90237be39bf125afdc273` contains only PoC DOCX source/tests. Exact-head DSI PoC `36180780592` **SUCCESS** (qualification job), Sandbox `36180780595` **SUCCESS**. Standard CI `36180780825` failed only on the expected absent production `DocxAdapter`/`OoxmlCoverageSentinel` imports; policy, security, macOS portability, and container build succeeded.
- Supplemental production test-only RED head `9df2abc77f26867ebc4fd2cc8166e266a25f07ab` adds 21 DOCX tests plus PoC-qualified `zip 8.6.0` as a **dev-dependency only**, with its seven lockfile additions; no production adapter/dependency was committed. Exact-head CI `36181633068` **FAIL as expected only on unresolved `DocxAdapter`/`OoxmlCoverageSentinel` imports**; fmt, policy, security, macOS portability, and container build succeeded. Sandbox `36181633106` and DSI PoC `36181633146` **SUCCESS**. This is the clean supplemental production RED.
- Production DOCX final GREEN `14bcc4a63ec4ec56289619e4d76a9ca1792315ba` promotes only the qualified Task 5 parser composition and the bounded DOCX adapter/sentinel. Precommit full `cargo test --workspace --locked`, strict workspace Clippy, root fmt, cargo-deny, and `git diff --cached --check` passed locally with pinned Rust 1.98.1. Exact-head standard CI `36182511870` **SUCCESS**, including required-check; Sandbox `36182511893` **SUCCESS**; DSI PoC `36182511885` **SUCCESS**. This satisfies Task 5's final exact-head gate. PR #10 remains OPEN/Draft and unmerged.
- Before the PoC GREEN commit, note-reference/orphan/text-box/even-header/direct-numbering/XML-event/style-list/picture-geometry repairs passed note-reference **6/6**, note-image **5/5**, even-header **2/2**, numbering **2/2**, instance restart **1/1**, global comment-count **2/2**, style-list **2/2**, geometry **1/1**, DOCX **14/14**, and PNG **12/12** locally. The XML node-count fixture reproduced RED with 2,000,002 valid comments across two parts; style-defined `lvlText`/`numFmt` and `rect`→`ellipse` fixtures reproduced focused RED before scoped repairs. The full local PoC gate passed Cargo tests, **91/91 manifest cases**, dependency policy, and final `overall: PASS`. These changes are in hosted GREEN head `0945e087` above.
- Before production GREEN, the DOCX draft passed note fail-closed tests, direct numbering **2/2**, style-list **2/2**, picture geometry **1/1**, numbering-instance restart **1/1**, XML event bound **1/1**, format-parity **11/11**, semantic-edge **5/5**, ZIP preflight **7/7**, ZIP gap/Unicode extra **2/2**, PNG decoded-pixel parity, the full workspace test suite, strict Clippy, fmt, and cargo-deny. The four earlier independent-audit gaps had focused local RED before scoped GREEN; they are included in hosted GREEN head `14bcc4a6` above.
- Task 5 promoted PoC-qualified `office_oxide 0.1.11`, deflate-only `zip 8.6.0`, `quick-xml 0.42.0`, and supplemental `png 0.18.1` scoped to bounded DOCX pixel decoding. The plan records the supplemental qualification without a frozen Design amendment.
- Final independent review found additional local DOCX gaps in selected header links, first-page header selection, picture transforms and extents, default and special numbering, empty-paragraph XML spelling, image position, and parser-disagreement stderr. Each reproduced a focused RED where the current adapter had a gap; the PoC now projects or rejects the relevant meaning, and production projects or rejects it with bounded diagnostics. Focused tests are locally GREEN. The review did not establish a new Design/profile change.
- After the final review repairs, the exact `poc:dsi:verify` sequence run with pinned Rust 1.98.1/PDFium passed all **91/91** manifest cases and the dependency gate; production workspace tests, strict Clippy, fmt, and cargo-deny passed locally. Hosted exact-head GREEN for both PoC and production is recorded above.
- No frozen Design amendment is required by the current evidence. Do not merge PR #10.

### Historical managed-worker attempts

Earlier `dsi-prod-task5-red-evidence-*` managed runs timed out or failed before a first tool call and produced no write receipts. Those attempts explain the prior runtime-blocker note, but are not the current blocker: the supplemental RED is now committed and its exact-head workflows have completed.

Next exact action: create Task 6 XLSX/XLSM/VBA RED contract tests only, covering the approved sheet/cell/formula/external-reference/VBA relations. Commit/push a clean RED before promoting the Task 6 qualified parser dependencies. Keep PR #10 Draft and unmerged; no Design amendment is in progress.

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

**PoC Qualification and Production Tasks 1–5 are complete.**

Task 5 final production head `14bcc4a6` passed standard CI `36182511870`, Sandbox `36182511893`, and DSI PoC `36182511885`. The current exact action is Task 6 XLSX/XLSM/VBA RED tests and its clean hosted RED gate, followed by qualified dependency promotion and GREEN verification. Task 5's frozen Design remains unchanged.

PR #8 is merged. Keep production work on PR #10 and do not merge it without an explicit user instruction.
