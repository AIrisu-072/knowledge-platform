# Document Semantic Inspection v0 — Execution Status

## Latest production gate — 2026-09-27 JST

- Branch / PR: `feat/document-semantic-inspection-v0` / PR #10 **OPEN / Draft** pending the documentation-only final gate; last exact verified implementation head `5045fa8e6274054f19931c2ba11bbec62fb546d5`.
- Tasks 1–12 **COMPLETE**. Task 12 exact-head standard CI `36253230526`, Sandbox `36253230478`, and DSI PoC `36253230464` all **SUCCESS** at `feb3affb82ba2ed144a87d58706391d53890d7f5`.
- Task 13 test-only RED `30f8fa2b80bce1070053251cc1c26b061ed3c45a`; GREEN code `59d6679511751791e07180c61d8bd3b48ae6382b`. Initial local parity RED was the missing qualified media-type profile handling; the supplemental cross-format RED was missing `assess_authority_migration`. Both now pass. Original PoC `a4fcef1...` corpus remains intact; the executable corrected corpus comes from PoC head `e309cbeb9f9075c5c3b900df8905f8ae62490337` / qualification `36161063199` and changes only 20 synthetic DOCX PNG bytes plus raw manifest hashes/sizes. All 91 relation/error expectations are unchanged (`d786494a95e9670974e8945a86aecf1d0d5252e809ef130036cfa6e78cbaff81` after stripping raw fields).
- Local Task 13 verification: 91/91 cases, 20 repeated inspections per successful case, 5 fresh worker processes under locale/timezone variants, cross-format capability decisions, VBA and signature targets **PASS**. Original malformed PNG remains rejected.
- Task 14 Linux inherited-FD review found an unlisted inheritable descriptor could reach the worker. Focused Docker/Linux RED failed 0/1; the allowlist fix passed 1/1 with and without the explicit trust FD. Local `mise run verify:full` **SUCCESS** in 200.27 s: 385 Rust tests passed, 4 platform-skipped; fmt, strict Clippy, architecture/API checks, `cargo deny check`, secrets/dependency/workflow scans, container build, and SBOM all passed.
- Final implementation head `5045fa8e6274054f19931c2ba11bbec62fb546d5`: hosted CI `36255207130`, Sandbox `36255207090`, and DSI PoC `36255207056` all **SUCCESS**. The required standard CI matrix passed production semantic parity on macOS Intel and arm64; Ubuntu `rust-test` passed the same corpus and all worker/runner tests.
- Review: 0 unresolved PR #10 threads and no submitted reviews at the latest check. Frozen Design/profile amendment: **none proposed**. Blocker: **none** at the verified implementation head.
- Next exact action: commit/push this evidence-only status change; require exact-head hosted CI on the resulting documentation tree; mark PR #10 Ready for review when it passes. Then await review feedback and explicit merge instruction. Do not merge without that instruction.

The latest production gate above supersedes historical “current” paragraphs below.

- Capability: `Document Semantic Inspection v0`
- Execution mode: **Inline Execution**
- Overall phase: **PRODUCTION IMPLEMENTATION — TASKS 1–14 VERIFIED AT IMPLEMENTATION HEAD / DOCUMENTATION-ONLY FINAL CI AND READY GATE NEXT**
- Design path: **Architectural**
- Frozen Design merged: PR #7
- PoC execution branch: `test/document-semantic-inspection-poc-v0`
- Production planning branch: `plan/document-semantic-inspection-v0-production`
- Production planning PR: **#9 MERGED — `48045768d1d026eb785ee065877e401bbafd97ca`**
- Production implementation branch: `feat/document-semantic-inspection-v0`
- Production implementation PR: **#10 (OPEN / Draft)**
- Last exact-head triple-verified production code head: `eeed985f229bbcacd08a7ea955b305e4fc30f010` (Task 8 signatures)
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
- Production dependency promotion: **Task 1 sandbox, Task 4 TXT/CSV/HTML, Task 5 DOCX, and Task 6 XLSX/XLSM/VBA parser compositions passed their approved gates**

PR #7 was advanced from review to approved state and merged after explicit user approval. The execution baseline is the resulting main merge commit `5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`.


## Production implementation state — current

The PoC is complete and all eight required format gates passed. A separate Production Implementation Plan now exists on `plan/document-semantic-inspection-v0-production`.

Current gates:

- Production Implementation Plan: **APPROVED 2026-09-25**
- Production planning PR #9: **MERGED**
- PR #8: **MERGED — `ab9ad6f9949128360e46fed07aca335bb6b10971`**
- Production dependency promotion: **Task 1 sandbox, Task 4 TXT/CSV/HTML, Task 5 DOCX, and Task 6 XLSX/XLSM/VBA parser compositions passed their approved gates**
- Production implementation branch: **CREATED — `feat/document-semantic-inspection-v0`**
- Production Task 1 sandbox preflight: **COMPLETE / PASS**
- Production core crate: **TASK 2 COMPLETE / PASS**
- Production runtime: **TASKS 1–7 COMPLETE / PASS; Task 8 PDF supplemental clean RED complete, GREEN repair in progress**

The production-hardening gap is resolved for the frozen v0 profile. Task 1 preflight qualified and promoted the sandbox substrate, and its runtime enforces the no-network, no-credential, filesystem-confinement, fresh-process, and finite resource-profile boundary.

Required next order:

1. Complete Task 8 PDF Step 2 GREEN against the supplemental RED evidence at `e6e3d6d1d9f785827237a14d3a3c45d75fc33a13` and require same-head standard CI, Sandbox, and DSI PoC success.
2. Execute Task 8 signatures as a separate RED/GREEN cycle, then Tasks 9–14 in the approved order. Keep PR #10 Draft and unmerged.

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
- Task 5 handoff documentation head `26afc64fd8703fdcf44af45497a2c9d0b41c30a1` also passed exact-head standard CI `36183492393`, Sandbox `36183492436`, and DSI PoC `36183492364`. It made no code or dependency change.
- Before the PoC GREEN commit, note-reference/orphan/text-box/even-header/direct-numbering/XML-event/style-list/picture-geometry repairs passed note-reference **6/6**, note-image **5/5**, even-header **2/2**, numbering **2/2**, instance restart **1/1**, global comment-count **2/2**, style-list **2/2**, geometry **1/1**, DOCX **14/14**, and PNG **12/12** locally. The XML node-count fixture reproduced RED with 2,000,002 valid comments across two parts; style-defined `lvlText`/`numFmt` and `rect`→`ellipse` fixtures reproduced focused RED before scoped repairs. The full local PoC gate passed Cargo tests, **91/91 manifest cases**, dependency policy, and final `overall: PASS`. These changes are in hosted GREEN head `0945e087` above.
- Before production GREEN, the DOCX draft passed note fail-closed tests, direct numbering **2/2**, style-list **2/2**, picture geometry **1/1**, numbering-instance restart **1/1**, XML event bound **1/1**, format-parity **11/11**, semantic-edge **5/5**, ZIP preflight **7/7**, ZIP gap/Unicode extra **2/2**, PNG decoded-pixel parity, the full workspace test suite, strict Clippy, fmt, and cargo-deny. The four earlier independent-audit gaps had focused local RED before scoped GREEN; they are included in hosted GREEN head `14bcc4a6` above.
- Task 5 promoted PoC-qualified `office_oxide 0.1.11`, deflate-only `zip 8.6.0`, `quick-xml 0.42.0`, and supplemental `png 0.18.1` scoped to bounded DOCX pixel decoding. The plan records the supplemental qualification without a frozen Design amendment.
- Final independent review found additional local DOCX gaps in selected header links, first-page header selection, picture transforms and extents, default and special numbering, empty-paragraph XML spelling, image position, and parser-disagreement stderr. Each reproduced a focused RED where the current adapter had a gap; the PoC now projects or rejects the relevant meaning, and production projects or rejects it with bounded diagnostics. Focused tests are locally GREEN. The review did not establish a new Design/profile change.
- After the final review repairs, the exact `poc:dsi:verify` sequence run with pinned Rust 1.98.1/PDFium passed all **91/91** manifest cases and the dependency gate; production workspace tests, strict Clippy, fmt, and cargo-deny passed locally. Hosted exact-head GREEN for both PoC and production is recorded above.
- No frozen Design amendment is required by the current evidence. Do not merge PR #10.

### Historical managed-worker attempts

Earlier `dsi-prod-task5-red-evidence-*` managed runs timed out or failed before a first tool call and produced no write receipts. Those attempts explain the prior runtime-blocker note, but are not the current blocker: the supplemental RED is now committed and its exact-head workflows have completed.

That historical next action was completed by the Task 6 clean RED heads recorded below. The current next action is in the Task 6 section.

## Production Task 6 — XLSX / XLSM / VBA COMPLETE

- Base head: `26afc64fd8703fdcf44af45497a2c9d0b41c30a1`; all three baseline workflows succeeded as above.
- Initial test-only RED head: `75b0dbf8b2f59c24093098365c4257feceadddb2`. It contains workbook semantic/noise, hostile SpreadsheetML package, and external-dependency worker-response tests. Local focused adapter tests fail only on unresolved `SpreadsheetAdapter`; the worker-shell test compiles and fails with exit 65 because `Xlsx` is not yet dispatched. Rust 1.98.1 whole-workspace fmt and staged diff checks passed. Independent test review led to ordinary-hidden, applied-style, actual XML attribute-order, chart-title, image-pixel, and XLSM-repack assertions; a temporary PoC probe accepted each mutation. No Task 6 parser dependency or implementation was added.
- Initial head exact-head standard CI `36187706539`: **FAIL as expected only on unresolved `SpreadsheetAdapter` E0432** in `rust-static` and `rust-test`; fmt, policy, security, macOS portability, and container-build passed. Sandbox `36187706713`: **SUCCESS**. DSI PoC `36187706727`: **SUCCESS**. This is clean initial XLSX/XLSM RED; VBA supplemental RED remains before Task 6 GREEN.
- VBA supplemental test-only RED head: `7c9e31c184f34c79aa44f450dace05c281341ec0`. It adds logic-change, whitespace/comment/case-noise, invalid/incomplete/missing VBA fail-closed, and `Auto_Open` static-only contracts. The synthetic comment-only XLSM fixture is PoC-generated from the qualified seed (31,134 bytes, SHA-256 `94ded42b0aaf3077bc78222db3c788f2f3c4e79a2d679f0950cd258d26a4db24`) and passed PoC fingerprint equality. The actual mutated `Auto_Open` XLSM passed PoC inspection without execution. Rust 1.98.1 full fmt passed; the focused production test failed only on missing `SpreadsheetAdapter` E0432. All temporary generator/probe test files were removed.
- Supplemental exact-head standard CI `36189581362`: **FAIL as expected only on unresolved `SpreadsheetAdapter` E0432** in `rust-static` and `rust-test`; fmt, policy, security, macOS portability, and container build passed. Sandbox `36189581450` and DSI PoC `36189581454`: **SUCCESS**, all at `7c9e31c184f34c79aa44f450dace05c281341ec0`. This is the clean Task 6 supplemental RED; no production parser dependency was added.
- GREEN implementation is in progress. Package-safety, sheet/oracle, and static VBA components are scoped separately. Review identified an `ovba 0.7.1` internal unbounded read/decompression path; the production boundary must prove finite memory handling before Task 6 GREEN can be qualified. No Design amendment is in progress.
- Local stock `ovba 0.7.1` release RED: the qualified VBA seed returned 89 source bytes; a crafted compressed variant returned **16,781,378 bytes**, beyond the approved 16 MiB source budget, and trailing decoded `/VBA/dir` bytes were accepted. The production workspace now applies a narrow same-version local source patch; its provenance is recorded under `third_party/document-semantic-inspection/ovba-0.7.1/PROVENANCE.md`. These observations are local and do not substitute for hosted GREEN.
- Local GREEN so far on the uncommitted Task 6 tree: pinned Rust 1.98.1 `cargo check --locked -p document-semantic-inspection-worker`, `cargo test --workspace --locked`, strict workspace Clippy, workspace `cargo fmt --check`, and `cargo deny check` passed. Focused `spreadsheet_semantics` **8/8**, `spreadsheet_worker_response` **1/1**, package safety **5/5**, XML namespace noise **2/2**, XLSM/VBA semantics **6/6**, VBA resource regression **2/2 in debug and release**, PoC spreadsheet **8/8**, and isolated patched-ovba unit tests **9/9** passed. The patch bounds internal CFB/MS-OVBA reads and decoded source before allocation and rejects trailing decoded directory bytes. Its 64 MiB decoded-directory ceiling is derived from the approved 64 MiB per-entry bound; unit tests accept the exact generic decompression limit, reject one over, and assert the production constant matches that ceiling. These checks predate the next image-count repair and must be rerun. Hosted exact-head GREEN remains pending.
- Independent review found that the approved **4,096 image** limit was applied only after rxls parsed drawings. A new test directly called package preflight with 4,096 and 4,097 qualified drawing-picture anchors: 4,096 passed, while 4,097 also returned `Ok(())`, producing the intended **local RED** (one test failed; fmt passed). This extra RED does not change the frozen profile value or parser composition.
- The pre-rxls image-count guard is now locally GREEN: its 4,096/4,097 boundary test passed, package module tests **4/4** and package-safety tests **5/5** passed. After this repair, pinned Rust 1.98.1 `cargo test --workspace --locked`, strict workspace Clippy, workspace fmt, and cargo-deny all passed. These checks precede the next connection-definition repair and will be rerun.
- Final independent review found two further connection-definition gaps: a credential-bearing ODBC connection string or URI userinfo can be copied to `ExternalDependency.normalized_reference`, and unsupported `xl/connections.xml` children can be ignored without fail-closed behavior. Focused supplemental local RED tests are in progress. Ordinary qualified ODBC definition behavior must remain; credential-bearing/unknown constructs must produce generic failure with no partial result or secret echo. No hosted GREEN exists yet.
- First supplemental connection RED is confirmed locally with pinned Rust 1.98.1: the ordinary qualified ODBC fixture passed, but the same valid XLSX with `PWD=DSI_ODBC_PWD_LEAK_SENTINEL` in `dbPr/@connection` returned worker exit 0. The new test failed specifically because a credential-bearing definition was accepted; compilation and formatting were not the cause. An unknown-child `webPr` RED is next, before production parser edits.
- The DSI Sandbox Preflight and DSI PoC workflows previously filtered out production worker-only changes. Their PR path filters now include the DSI production crates, vendored parser, root Rust manifests, and execution status files, so the required regressions can run at the same Task 6 GREEN head. These workflow edits are uncommitted until the full local gate passes.
- Task 6 GREEN implementation candidate head: `7cf4986084649bcf5bccc5e8cca50930b090b75e`, committed/pushed to PR #10. Its qualified dependency set and bounded adapter passed pinned Rust 1.98.1 workspace tests, strict Clippy, fmt, cargo-deny, and repository policy locally. The final scoped result-bound review found no unbounded dynamic `WorkerResponse` field.
- Candidate exact-head CI `36209080797`: **FAIL**. `rust-test`, `rust-static`, policy, security, and macOS portability succeeded. Only `container-build` failed: Dockerfile did not copy tracked `third_party/` before `cargo build`, so Cargo could not read `/src/third_party/document-semantic-inspection/ovba-0.7.1/Cargo.toml` for the qualified `[patch.crates-io]`. The required-check consequently failed. This is a packaging failure, not a failing adapter contract. Exact-head Sandbox `36209080849`: **SUCCESS**. Exact-head DSI PoC `36209080799`: **SUCCESS**.
- The scoped Dockerfile fix adds `COPY third_party ./third_party` before the image's Cargo build. The original CI failure is its RED evidence. Its independent read-only review found that the one-line copy covers both the local ovba patch and VBA tree-sitter grammar. The local cross-architecture container build was stopped after the exact-head hosted container-build succeeded; local success is not claimed.
- Final repaired GREEN head: `98072f1732157c85ac3d26ce7bf78d64cc456568`. Exact-head standard CI `36209740995`: **SUCCESS**, including `container-build`, `rust-test`, `rust-static`, policy, security, macOS portability, and required-check. DSI Sandbox Preflight `36209740990`: **SUCCESS**. DSI PoC `36209741011`: **SUCCESS**. Task 6 meets its final exact-head gate. No Frozen Design amendment was required.
- Task 6 promoted only the PoC-qualified `rxls 0.1.3`, `calamine 0.36.1` with picture support, `ovba 0.7.1` with a scoped bounded local patch, `tree-sitter 0.25.10`, `tree-sitter-language 0.1.8`, and the exact qualified VBA grammar revision. VBA remains static-only. PR #10 remains OPEN/Draft and unmerged.

## Production Task 7 — PPTX COMPLETE

- Base head: `98072f1732157c85ac3d26ce7bf78d64cc456568`, with all three Task 6 final workflows SUCCESS.
- Initial test-only RED head: `452c76d0d92aedb31c4c447cef5bae75388a8b63`. `crates/document-semantic-inspection-worker/tests/pptx_semantics.rs` contains eight tests from the PoC-qualified PPTX fixtures. It covers slide/order, text/shape/group, table/chart/SmartArt/image/link/speaker-note significance; theme/font/background/internal-ID/package-order invariance; comment-only editorial evidence; unknown semantic part fail-closed; and worker shell dispatch/raw binding. Pinned direct Rust 1.98.1 `cargo fmt --all -- --check` and diff checks passed; focused test compilation failed only on the intended unresolved `PptxAdapter` import (E0432). Independent read-only test review found no initial RED blocker.
- Initial exact-head standard CI `36210857702`: **FAIL as expected only on unresolved `PptxAdapter` E0432** in `rust-static` and `rust-test`. `rust-static` formatting, policy, security, macOS portability, and container build passed; `required-check` failed consequent to the planned Rust failures. Exact-head DSI Sandbox Preflight `36210857803`: **SUCCESS**. Exact-head DSI PoC `36210857696`: **SUCCESS**. PR #10 was verified OPEN/Draft at this same head. This is the clean authoritative Task 7 initial RED.
- Read-only promotion audit identified Task 7 supplemental RED candidates for PPTX package ambiguity, full content-type/part coverage, per-entry/XML/slides/shapes/images/resource bounds, and XML namespace-prefix invariance. A separate probe found that valid chart-title cache label changes are omitted; frozen Design §9.4 explicitly includes chart labels. Reproduce and scope each material case before repair; do not change Design/profile semantics silently.
- A scoped design review found that identical decoded PNG pixels after re-encoding are required noise-same in frozen Design §13.2 for **DOCX**, while PPTX §13.5 and approved Task 7 do not establish that invariant. A PoC PPTX same-pixel test was locally RED, but it is **excluded from authoritative Task 7 RED and from promotion**. Applying that new PPTX fingerprint rule requires a separately approved Design amendment. This candidate is not a Task 7 blocker.
- A separate scoped design review put auto-shape preset geometry and rotation changes on **HOLD**: Design §9.4/§13.5 and Task 7 do not define those as v0 fingerprint differences. A future rule to project or reject them needs an approved Design amendment; they are not Task 7 blockers.
- Supplemental PoC tests reproduce valid XML-prefix/SmartArt, presentation relationship-prefix, generic-XML unknown-part, and chart-title label gaps. Focused PoC cases are locally behavioral RED against the existing source; independent read-only test review found no fixture blocker. Production package-safety, 64 MiB per-entry resource, chart-title, and presentation-prefix test-only cases also passed independent read-only review. Pinned Rust 1.98.1 root fmt and focused production compilations pass except for the expected missing `PptxAdapter` E0432. Generated `experiments/document-semantic-inspection/target/` remains untracked and must not be staged.
- Supplemental test-only RED head: `6f32cbe87728a75ffd118862597cabbba54d2250`, on local, remote, and PR #10 head. Exact-head standard CI `36212761629`: **FAIL as expected only on missing `PptxAdapter` E0432** in `rust-static` and `rust-test`; formatting, policy, security, macOS portability, and container build passed. Exact-head Sandbox `36212761543`: **SUCCESS**. Exact-head DSI PoC `36212761571`: **FAIL as expected on the new chart-title semantic assertion**, after preceding test suites passed. Cargo stopped at that first failing test binary; supplemental SmartArt/presentation-prefix/unknown-part tests each failed for their intended behavior in focused local runs. This records the clean supplemental RED before source repair.
- A further test-only coverage RED was committed at PoC head `4aba9aacd4d2818b5b6e241982c15452875b88bf` and production head `821ff96ceff02fbd6b7ccdcdbdc9d178d38ff4b2`. A new `customXml/item1.xml` part with a forged known `slide+xml` content-type override passed the uncommitted PoC adapter; focused Rust 1.98.1 PoC test failed only at the expected `UnsupportedSemanticConstruct` assertion after ZIP/XML and mutation-scope checks passed. The matching production focused compile failed only on missing `PptxAdapter` E0432. This proves that content-type allowlisting alone is insufficient; package coverage must consider part path and type together. PoC repair is now uncommitted work in progress. PR #10 is OPEN / Draft at `821ff96ceff02fbd6b7ccdcdbdc9d178d38ff4b2`.
- Further test-only head `c8904b6655a094ceacebdb8c1fb27af6950a28e5` adds four focused PoC behavioral RED cases: `[Content_Types].xml` declared 64 MiB + 1 before format sniffing (current adapter accepted it), chart value-point `idx` association (current fingerprints equal), foreign namespace chart extension (current adapter accepted it), and chart data-label `showCatName` change (current fingerprints equal). Baseline inspection, ZIP CRC/XML checks, and mutation-scope assertions passed before each intended RED. Matching production point-index, extension, and data-label tests compile RED only on missing `PptxAdapter` E0432; the existing production resource-limit test covers oversized content types. The new rules are within frozen Design §9.4 chart data/series/labels and §13.5 unknown semantic content fail-closed, not Design amendments. PoC path/content-type coverage, chart title, namespace-prefix handling, and safe slide path mapping are locally GREEN in uncommitted source; chart/preflight repair and hosted GREEN remain.
- Exact-head hosted `c8904b6655a094ceacebdb8c1fb27af6950a28e5` standard CI `36214493107` **FAIL as expected only on missing `PptxAdapter` E0432** in Rust static/test; formatting, policy, security, macOS portability, and container build passed. Sandbox `36214493097` **SUCCESS**. DSI PoC `36214493094` **FAIL as expected only on the first new chart data-label fingerprint assertion** after preceding suites passed. The other new PoC tests each reached their intended behavioral RED locally. This is clean additional Task 7 RED evidence.
- Independent ChartML review found that a per-point explicit `showCatName=false` override vanished when global `showCatName=true`. Two matching test-only files were committed/pushed at `2c279a22c54ff1b0ca4118569689f54f2addb54e`. The focused PoC test passed ZIP CRC/XML/chart-only mutation checks and failed only at the expected fingerprint inequality assertion. The production focused compile failed only on missing `PptxAdapter` E0432. PoC chart/preflight/package/prefix tests are otherwise locally GREEN; the per-point override fix and full PoC gate are next. PR #10 remains OPEN / Draft, and no production Task 7 dependency/adapter is present.
- Exact-head hosted `2c279a22c54ff1b0ca4118569689f54f2addb54e` standard CI `36215933182` **FAIL as expected only on missing `PptxAdapter` E0432** in Rust static/test; formatting, policy, security, macOS portability, and container build passed. Sandbox `36215933216` **SUCCESS**. DSI PoC `36215933201` **FAIL as expected only on the earlier chart data-label assertion** before Cargo reached the new override test; focused local override test was behavior RED. An independent read-only slide-relationship/target-content-type mutant was rejected with `SemanticExtractionFailed`, so it did not expose an acceptance gap.
- PoC source now retains explicit per-point `false` label/delete values. Pinned Rust 1.98.1 full `mise run poc:dsi:verify` with locally installed, hash-pinned PDFium `7881` completed **exit 0**: all PoC tests passed; `cargo deny` reported advisories/bans/licenses/sources OK; fixture report `overall: PASS`, `pptx: PASS`, and 91/91 case verdicts pass. `git diff --check` passed. This is local GREEN only; source and execution records remain uncommitted while independent review checks the final diff, and no hosted exact-head GREEN has been claimed.
- Final independent read-only audit returned **NO-GO**: `parse_chart_semantic` silently omitted `<c:legend>` even though a visible legend changes reader-visible series labels under frozen Design §9.4. Matching PoC and production test-only cases were committed/pushed at `de33d36c1885912ffc0fade0e4ab5bb40e3bce98`: focused PoC behavior RED was only the fingerprint inequality after ZIP CRC/XML/chart-only mutation checks; focused production compile RED was only missing `PptxAdapter` E0432; root fmt passed. Exact-head standard CI `36216517105` failed only on that E0432 in Rust static/test; fmt/policy/security/macOS/container passed. Sandbox `36216517133` succeeded. PoC `36216517125` failed only at the preceding chart data-label RED assertion before reaching the legend binary. A separate slide-relationship/target-content-type mismatch probe was rejected with `SemanticExtractionFailed` and does not block.
- A scoped read-only ChartML inventory found further silent omission of axis scales/number formats, data tables, trendline, error bars, and chart-level data-display switches. These can change visible chart data/labels. Focused test-only fail-closed RED is in progress before PoC source promotion. Production `pptx_package.rs` and `pptx.rs` are separate uncommitted worker implementations; no Task 7 production dependency or adapter is promoted at the current PR head.
- Unmodeled ChartML six-case test-only head `dc4e54c60bf54bfbcb13404145a693d8a8c5dd14` had focused PoC behavior RED only on accepted valid mutants; local PoC and production WIP repairs pass 6/6. Sandbox `36217261838` succeeded; PoC `36217261870` stopped at earlier expected data-label RED; CI `36217261843` was cancelled by the next push.
- Malformed legend test-only head `0f12de602da24123f80e68ddea49ab53925c989e` had four local acceptance RED and two positive GREEN cases. Local PoC and production repairs pass all six plus their broader PPTX suites. Exact-head CI `36217557087` failed only on absent `PptxAdapter` E0432 after fmt/policy/security/macOS/container passed; Sandbox `36217557044` succeeded; PoC `36217557052` stopped at earlier expected data-label RED. PR #10 remains OPEN/Draft.
- Independent production audits found package-wide 2,000,000 XML-node and OPC QName/root gaps plus connector/SmartArt/picture semantics to scope. Test-only package and SmartArt RED head `78bdfd85d2b97536d09a792287e5906cbfaa3d17` records four wrong OPC roots and aggregate node overflow accepted, SmartArt role changes with equal fingerprints, and duplicate IDs accepted. CI `36218126620` failed only on missing `PptxAdapter` E0432 after fmt/policy/security/macOS/container passed; Sandbox `36218126612` succeeded; PoC `36218126635` stopped at earlier expected data-label RED. Local package and SmartArt repairs are uncommitted.
- Connector endpoint and picture crop/rotation/flip test-only head `6640d6f5db6796a492d6923c6450c8bdb49b33d4` records valid ZIP/CRC/XML and single-part mutations with equal fingerprints despite in-scope meaning/visible changes. Exact-head CI `36218461687` failed only on absent `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36218461635` succeeded; PoC `36218461632` stopped at the earlier expected data-label RED. Connector fixes pass focused PoC and production PPTX regressions locally.
- PoC and production picture source repairs now pass focused crop/rotation/flip tests 3/3. The PoC passed 17 other PPTX test targets excluding root-relationship RED; production passed 43/43 other PPTX regressions. Both source files passed pinned formatting and changed-file whitespace checks. No source is committed.
- New test-only head `b2742bb34dc3132ac6674cb4a660d9556a5376c6` records two PPTX OPC root relationship gaps: missing `_rels/.rels` and duplicate `officeDocument` relationships were accepted by both adapters. A valid XML character reference in the presentation ContentType attribute caused `FormatMismatch` in both scanners despite unchanged meaning. The mutated ZIPs have valid CRC/XML and only the intended package part changes. Focused PoC and production WIP suites were each 1 PASS / 3 expected behavioral FAIL; pinned formatting passed. Exact-head CI `36219047006` failed only on missing `PptxAdapter` E0432 after fmt/policy/security/macOS/container passed; Sandbox `36219046955` succeeded; PoC `36219046957` failed only at the earlier chart data-label RED. No source fix is promoted yet.
- Production root-relationship/ContentType fix passes focused 4/4 tests and the entire worker crate integration suite under pinned Rust 1.98.1, including package, chart, SmartArt, connector, picture, and aggregate XML-node regressions. It remains uncommitted. PoC corresponding source fix is next.
- Corresponding PoC root-relationship/ContentType fix passes focused 4/4 and PPTX regression 45/45 locally under pinned Rust 1.98.1; source remains uncommitted. An independent picture review found visible tile/grayscale constructs silently ignored, redundant local namespace declarations falsely rejected by PoC, and production accepting out-of-range rotation. Test-only head `fa11d63844036853314e1ddbe923238140399aa5` records PoC focused fill/effect 0/2 expected FAIL and namespace/range 1 PASS / 2 expected FAIL, plus production namespace/range 2 PASS / 1 expected FAIL. Production fill/effect focused run was deferred with disk free near 2 GiB. Exact-head CI `36220064505` failed only on absent `PptxAdapter` E0432 after fmt/policy/security/macOS/container passed; Sandbox `36220064507` succeeded; PoC `36220064503` failed only at the earlier chart data-label RED. A further package review found false rejection of an inter-element XML whitespace character reference; scoped test-only RED is in preparation. Official `cargo clean` removed 12.4 GiB of repository root build cache, restoring about 12 GiB free; PoC build cache and pinned PDFium remain.
- OPC whitespace character-reference test-only head `19f68b3065f2864039d73557861202f5809b7cb6` has pinned PoC focused 1/1 PASS with base/literal-space/reference fingerprint equality. Pinned production WIP focused test failed only because the valid character reference was rejected as `UnsupportedSemanticConstruct`; baseline/literal-space accepted. Exact-head CI `36220599599` failed only on absent `PptxAdapter` E0432 after fmt/policy/security/macOS/container passed; Sandbox `36220599703` succeeded; PoC `36220599596` failed only at the earlier chart data-label RED. PoC picture strict source fixes pass focused 5/5 locally. Full pinned Rust 1.98.1 `mise run poc:dsi:verify` with pinned PDFium `7881` completed exit 0: all PoC test targets PASS, `cargo deny` advisories/bans/licenses/sources OK, fixture report `overall: PASS`, eight format gates PASS, and 91/91 case verdicts pass. This is local GREEN only; hosted PoC GREEN remains pending. Production OPC repair passes focused 1/1 and package regressions 12/12 locally, and production picture strict cases pass 5/5. Pinned Rust 1.98.1 production `cargo test --workspace --locked` exit 0, `cargo fmt --all -- --check` PASS, and root `cargo deny check` advisories/bans/licenses/sources OK. Final independent semantic audit flagged possible SmartArt sibling order and referenced slide-layout visible-text omissions; scoped test-only confirmation is underway.
- SmartArt sibling `srcOrd` change was accepted with equal fingerprints by PoC and production WIP in a valid three-point diagram; only the diagram data part changed. Test-only RED head `4d0b103834e09f8b6853c8065b83b00abccba046` was committed/pushed. Exact-head CI `36221473575` failed only on expected missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36221473522` succeeded; PoC `36221473524` failed only on the earlier chart data-label RED. Scoped order source repairs pass focused PoC/production tests locally. Supplemental duplicate `srcOrd`/`destOrd` tests had focused intended RED in both adapters and were committed/pushed test-only at `2b496b0616650bc30fc37384c6dbb38016f04445`. Exact-head CI `36221992539` failed only on expected missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36221992489` succeeded; PoC `36221992513` failed only on the earlier chart data-label RED. PoC duplicate-order source repair passes focused 3/3, role/ID 2/2, and base PPTX 7/7 locally. Full pinned Rust 1.98.1 PoC verification with PDFium 7881 exited 0: all tests and cargo deny passed; fixture report 91/91 passed. This source is included in the present PoC promotion commit; hosted GREEN remains pending. Production duplicate-order repair passes focused order 3/3, role/ID 2/2, base PPTX 8/8, and strict worker-library Clippy locally; it remains uncommitted. A standard slide-layout package with proven visible-text difference was rejected by both adapters on unsupported printerSettings content type, so no accepted-input layout omission was demonstrated and no test was retained. Eight mechanical lints in uncommitted production `pptx.rs` were fixed. The production Task 7 adapter is not yet promoted.
- PoC PPTX source promotion head `b5c02d7581ae6feb40cfdca0df797f679b499fb6` is pushed. Exact-head CI `36222458187` failed only on expected missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36222458170` and DSI PoC `36222458191` both succeeded. Uncommitted production WIP passed pinned `cargo test --workspace --locked`, root fmt, cargo deny, and strict root all-target Clippy after a behavior-preserving `single_match` lint fix in the PPTX chart-title test; its focused test passed. Independent read-only review identified possible accepted-input omissions for shape-level click hyperlinks, linked SmartArt layout, and chart source formulas within frozen Design §9.4/§13.5. Three scoped test-only probes are underway. An inline unknown graphic payload candidate lacked end-to-end accepted-input/visible-meaning proof and was closed without a test or source change.
- The three supplemental Task 7 candidates were focused behavioral RED in both PoC and production WIP: shape-level click hyperlink target, chart `c:f` source range with unchanged cached values, and linked SmartArt `linDir` layout direction. Each accepted baseline and mutant passed ZIP CRC/XML and mutation-scope checks before the fingerprint inequality assertion alone failed. The SmartArt fixture has all four required diagram relationships; LibreOffice converted both files but image-level render difference was not established. DrawingML layout direction is within frozen Design SmartArt meaning. Test-only head `23fab28346f714c791e088db8ae6d6faa732c0af` also includes a behavior-preserving chart-title test Clippy fix; pinned fmt and staged diff checks passed. Exact-head CI `36223753349` failed only on expected missing `PptxAdapter` E0432 in Rust jobs after formatting, policy, security, macOS portability, and container build passed; Sandbox `36223753388` succeeded; DSI PoC `36223753350` failed only on the new chart `c:f` fingerprint assertion after preceding tests passed. The other two new tests had focused local behavioral RED. This is clean supplemental Task 7 RED evidence. PoC/production source repairs are in progress in separate scoped files; no production source is promoted.
- Local supplemental GREEN: pinned Rust 1.98.1 focused PoC/production tests pass for all three new cases; PoC PPTX regression 56/56 and full `mise run poc:dsi:verify` with pinned PDFium exit 0, including fixture report 91/91. Production PPTX integration tests 62/62, full workspace tests, strict all-target Clippy, root fmt, and cargo deny all exit 0. Source is still uncommitted; independent source/package audits and hosted GREEN remain pending.
- Independent read-only source/package audits returned NO-GO: accepted chart-title `c:f` formula and shape-click `tooltip`/`tgtFrame` are omitted in production but projected by PoC; equivalent SmartArt attribute character-reference spelling changes production identity; malformed percent escapes in OPC part names/relationship targets are accepted by the production package sentinel. Four scoped test-only RED files are in progress. Source remains uncommitted; no Design amendment is proposed.
- Four scoped production tests had local focused RED after accepted baseline, ZIP CRC/XML, and mutation-scope checks: chart-title source formula 1/1, shape-link attributes 2/2, equivalent SmartArt character reference 1/1, malformed OPC URI 1/1. Pinned root fmt and staged diff checks pass. Test-only head `fcd33d973f6cff57a0a94b96b6a8e9b0014ac129` was pushed. Exact-head CI `36225655043` failed only on absent `PptxAdapter` E0432 in Rust static/test after fmt, policy, security, macOS portability, and container build passed; Sandbox `36225655039` succeeded; PoC `36225655098` failed only at the earlier chart-series `c:f` RED assertion. The four added production behaviors were reproduced locally against WIP. This is clean supplemental RED; no source is included in this head.
- Local production GREEN for four audit repairs: all five focused new tests PASS. Pinned full workspace tests, strict all-target Clippy, root fmt, cargo deny, and diff check exit 0. Source remains uncommitted; final independent read-only audit is pending, and no hosted GREEN is claimed.
- Final independent read-only audit found no other proven source defect. An added Target-only malformed OPC URI test passes 1/1 against current WIP; root fmt and strict Clippy remain green. The formula-only chart-title candidate was accepted by both WIP adapters with equal fingerprints, but no corpus fixture has a workbook or chart `externalData`; Microsoft Open XML guidance and independent scope review leave its PowerPoint meaning unproven. Temporary tests were removed; no source change or Design amendment was made for that candidate.
- PoC PPTX source and independent OPC Target regression were promoted at exact head `94bf42a24a12f0f21d7f218600598a32ee9bb96d`. Hosted DSI PoC `36226998892` and Sandbox `36226998852` SUCCESS. Standard CI `36226998879` failed only on absent `PptxAdapter` E0432 in Rust static/test after fmt, policy, security, macOS portability, and container build passed. Production adapter candidate is staged separately; pinned full workspace tests, strict all-target Clippy, fmt, cargo deny, and staged diff checks passed locally. No production GREEN or Task 7 completion yet.
- Final production GREEN head `42eeb9c2724d10a3a49d56a6d3f08a6336369de7`: exact-head standard CI `36227354015`, DSI Sandbox Preflight `36227354068`, and DSI PoC `36227353971` all **SUCCESS**. Standard CI passed Rust tests/static, policy, security, macOS portability, container build, and required-check. Local pinned full workspace tests, strict all-target Clippy, fmt, cargo deny, and staged diff check passed before promotion. The independent final read-only audit found no other proven source blocker. Task 7 is **COMPLETE**; PR #10 remains OPEN / Draft and unmerged.
- Task 7 handoff action was completed by the Task 8 PDF clean RED recorded below.

## Production Task 8 — PDF SUPPLEMENTAL CLEAN RED COMPLETE / GREEN REPAIR IN PROGRESS

- PDF semantics test-only head: `49d66402d0b4ea482ace6bda955820ec0ead6c5c`. No Task 8 parser dependency or implementation was included. The tests cover text, page order, link, visible form value, image, annotation as editorial evidence, producer/object-id noise, scan-only, encrypted, broken xref, PDFium/lopdf disagreement, ambiguous read order, and exact PDFium native binary identity/hash for the three qualified platforms.
- Exact-head standard CI `36228434980`: **FAIL as expected only on unresolved `PdfAdapter` E0432** in rust-static and rust-test. `cargo fmt --check`, policy, security, macOS portability, and container build passed; required-check failed consequent to the two expected Rust failures.
- Exact-head DSI Sandbox Preflight `36228435009`: **SUCCESS**. Exact-head DSI PoC `36228434957`: **SUCCESS**.
- Local pinned fmt passed, focused compile failed only on E0432, and independent read-only review returned GO. This is the clean authoritative Task 8 Step 1 PDF RED.
- First PDF GREEN candidate head `56233352a412aede6f0a459c17354965c2244e94` promoted only approved `pdfium-render 0.9.4` (`pdfium_7881`, `thread_safe`), exact PDFium `151.0.7881.0`, and `lopdf 0.45.0` without default features. Pinned full workspace tests, strict Clippy, fmt, security/dependency gates, and PoC gate passed locally. Exact-head standard CI `36232593899`, Sandbox `36232593835`, and DSI PoC `36232593806` all **SUCCESS**; the PoC success was a same-head rerun after a transient mise-action network reset. Independent read-only review then returned **NO-GO** because the adapter omitted PDF paint invocation/count/order/placement, included unused image resources, could skip opaque Contents decode failures, and omitted nonlocal link action targets. This candidate is not Task 8 completion.
- Earlier supplemental test-only head `9b5d672da319558897992ff92c72a499bef88f68` matched local/origin/PR #10 when verified (checked 2026-09-26); PR remains OPEN/Draft. Production tests add six PDFium-raster-confirmed paint relations, unknown-filter Contents failure, and GoToR target distinction/rejection. PoC tests add invocation/unused-resource, opaque stream, and GoToR cases. Exact-head CI `36233288836` **FAIL as expected only on the PDF action assertion** in rust-test; rust-static/fmt, policy, security, macOS portability, and container build passed. Sandbox `36233288838` **SUCCESS**. DSI PoC `36233288842` **FAIL as expected only on two opaque/action assertions**. Focused local RED also observed all paint relations. This is clean supplemental RED.
- Second supplemental test-only RED head `bae18ac0bd250e80dbcda05060b3e750daaaa3c9` matched local/origin/PR #10 at that point (checked 2026-09-26); PR remained OPEN/Draft. Five tests isolate direct `/Dest` link targets (production and PoC), noncommutative CTM order, transformed nested Form BBox clipping, and Image `/OC` visibility. PDFium API/raster assertions confirmed link targets, 128-pixel CTM difference, clip pixel difference, and optional-content visibility before semantic assertions. Exact-head CI `36235672485` **FAIL as expected only on `pdf_action_semantics` and direct-destination assertions** in rust-test; rust-static/fmt, policy, security, macOS portability, and container build passed. Sandbox `36235672432` **SUCCESS**. DSI PoC `36235672588` **FAIL as expected only on the new CTM assertion**. Other new cases were focused local RED; PoC direct-destination test passed current WIP and failed in an isolated committed-head snapshot. This is clean supplemental RED.
- Scoped production and PoC PDF source repairs are in progress; no GREEN source is committed. Local `deny.toml` WIP narrows the transitive `libloading 0.9.0` ISC exception instead of allowing ISC globally. Independent review and exact-head GREEN are required. No Design amendment is proposed.
- Local uncommitted PDF repairs passed the focused production and PoC PDF suites. Pinned full `mise run verify` passed all 345 workspace tests (2 skipped), including an additional invisible-text invariance case, fmt, strict Clippy, architecture, dependency/security and API checks. Full `mise run poc:dsi:verify` passed all tests, 91 manifest cases, and its dependency gate. The PoC nested-image test harness initializes PDFium before an invalid-inline-image probe to remove test-order dependence. Independent review returned **NO-GO**: overlapping text/image paint order can change PDFium-visible pixels while the adapters' separate text and image projections stay equal. Production PDF operation/paint caps also need synthetic boundary tests under Plan §1.3. New raster-backed RED/GREEN and exact-head gates are required.
- Paint-order test-only head `e6e3d6d1d9f785827237a14d3a3c45d75fc33a13` contains only new production and PoC tests. Same native text/image/geometry in reversed overlapping draw order produced different pinned PDFium rasters (PoC: 246 changed pixels). In an isolated checkout of this exact committed head, both focused tests failed only the final fingerprint inequality. PR #10, local, and origin heads match and remain OPEN/Draft. Standard CI `36237744551` **FAIL** only on previously recorded GoToR and direct `/Dest` assertions in fail-fast `rust-test`; rust-static/fmt, policy, security, macOS portability, and container build succeeded. Sandbox `36237744572` **SUCCESS**. DSI PoC `36237744554` **FAIL** only on previously recorded CTM assertion before reaching the new test. This is clean supplemental RED with exact-head focused evidence despite workflow fail-fast.
- Local uncommitted PDF paint-order repairs pass focused production and PoC regressions (PoC: 26 tests across nine targets). Production operation-count and image-paint caps have synthetic exact-boundary and one-over tests passing. Pinned full `mise run verify` exited 0 with 348/348 workspace tests passing (3 skipped), including fmt, strict Clippy, policy/security and API checks. Pinned full `mise run poc:dsi:verify` exited 0 with all tests, 91/91 fixture cases, and advisories/bans/licenses/sources passing. A shared Cargo target retained an absolute build-script path from an isolated RED checkout; `cargo clean --package document-semantic-inspection-worker` removed that stale artifact before the successful focused and full verification. These are local GREEN results only, with no committed source or hosted exact-head GREEN. Independent PDF review and a Type 3 glyph-resource probe remain in progress.
- Final read-only PDF audit found one further concrete blocker: Type 3 glyph CharProcs can draw images that the page `Do` walk misses; the earlier Tr3 concern was withdrawn under ISO 32000-1/2. Synthetic red/blue Type 3 glyph images differ in exactly one raw image sample; pinned PDFium `7881` extracts the same native text `A` and renders different raster bytes. Both production and PoC WIP adapters accepted both with equal fingerprints. The final production/PoC tests permit either unequal successful fingerprints or paired `UnsupportedSemanticConstruct`; both focused tests remained RED only at their final fingerprint assertions. Test-only head `a53a52a12d0ef29e6e4f9a2f10a7dc69e6931de9` contains just those tests and matches local/origin/PR #10 OPEN/Draft. Exact-head CI `36239695270` failed only at the known GoToR and direct `/Dest` assertions; rust-static/fmt, policy, security, macOS portability and container build succeeded. Sandbox `36239695276` succeeded. PoC `36239695252` failed only at the known CTM assertion before Type 3. The focused Type 3 relation was RED against the prior adapters. Production and PoC selected-Type3 fail-closed repairs now pass pinned full local gates: `mise run verify` 351/351 workspace tests (4 skipped), and `mise run poc:dsi:verify` all tests, 91/91 fixture cases, and dependency gates. Independent narrow read-only Type 3 review returned GO: page/Form-scoped selected-font resolution and fail-closed Type 3 rejection close the visual-semantic gap. No GREEN source is committed yet. Production malformed `TJ` array had focused local RED (`Ok(true)` for a boolean member) and GREEN `ParserDisagreement`; the actual strict decoder now has a passing 1,000,000/1,000,001 operation boundary test. No Design amendment is proposed.
- PDF Step 2 GREEN candidate head `acdf430c732bd504dfe6e38c0894c07b8bc8657b` was committed/pushed; local/origin/PR #10 heads match OPEN/Draft. It contains the scoped PDF repairs, narrow ISC license exception, invisible-text test, and current record. Exact-head CI `36240609880`, Sandbox `36240609839`, and PoC `36240609860` all completed SUCCESS at the same head. Task 8 Step 2 PDF is COMPLETE; signature Steps 3–4 remain.
- Task 8 Step 3 signature test-only RED prepared locally without production source/dependency promotion. Production focused compile fails only on missing `SignatureInspector`, `SignatureTrustContext`, and explicit worker trust injection API. PoC focused OOXML signature test fails only at the self-contained XML false-Valid classification. Root pinned `cargo fmt --all -- --check` passed. Static synthetic PDF ByteRange fixtures and test-only source/hash provenance are included. Exact-head RED commit/CI remains pending.
- Initial signature test-only RED head `303cc898b59fd3025bff97a93c431dbb598dd75d` committed/pushed with only tests, synthetic PDF fixtures, and execution docs; no production source or parser dependency promotion. PR #10 is OPEN/Draft at that head. Focused Production compile failed only on missing signature APIs; focused PoC failed only self-contained XMLDSig false-Valid. Root fmt and staged diff checks passed. Exact-head CI `36241424330` failed only missing signature API imports in rust-static all-target Clippy and rust-test; fmt, cargo check, policy, security, container and macOS portability passed. Sandbox `36241424298` succeeded. PoC `36241424407` failed only the intended self-contained XMLDSig false-Valid assertion (6 other signature tests passed). This is clean initial signature RED.
- Supplemental signature test-only RED constructs a valid same-document XMLDSig with an injected unsigned Manifest claiming `word/document.xml` coverage. PoC verifies the XML itself, but the wrapped DOCX is incorrectly reported `Valid`; the focused test fails only at the expected `Unverifiable`. Matching Production contract forbids authenticated coverage from the unsigned Manifest. Test-only supplemental head `a592d6804ef75aadf98c904320d63066970e4f42` was committed/pushed; PR #10 remains OPEN/Draft at this head. Exact-head CI `36242162962`, Sandbox `36242162900`, and PoC `36242162901` are running. No signature source or dependency is promoted.
- Read-only signature risk audit identified PoC acceptance of a self-contained XMLDSig as an OOXML package signature without checking signed package parts, and a possible forged PDF `/ByteRange` in raw comments. Reproduce and fix these in separate signature Step 3 RED/GREEN; do not promote unverified PoC behavior. No Design amendment is proposed.
- Exact next action: confirm supplemental unsigned-Manifest RED head `a592d6804ef75aadf98c904320d63066970e4f42` CI `36242162962`, Sandbox `36242162900`, and PoC `36242162901` outcomes, then implement Task 8 Step 4 GREEN with only qualified signature dependencies, explicit offline trust, and separate evidence; require same-head triple GREEN before Task 9. Keep PR #10 OPEN/Draft and unmerged.

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

### Production execution update — Task 11 complete / Task 12 GREEN pending (2026-09-27 JST)

- Task 11 exact head `7e93994a21e546916f7b0f7abd354677cd9d5e55`: standard CI `36249696461`, Sandbox `36249696404`, PoC `36249696311` all **SUCCESS**. PostgreSQL schema, immutable API, raw-binding cache integrity, and concurrent complete semantic-result convergence are verified. Task 11 is **COMPLETE**.
- Task 12 test-only RED head `adeda5ac60d490c02af4fd8cecd5071de870c263`: Rust 1.98.1 focused compile failed only on absent `RunnerInspectionExecutor` E0432. It included no production runner-to-Application bridge.
- Task 12 local GREEN source head `d113aabbb4432b6c14fc4e6af042a79294e18ce0`: bounded Application reader-to-Linux runner bridge, typed failure mapping, and production worker binary setup for Linux `test:rust`. Mac focused compile plus strict Application and runner Clippy, root fmt, and Linux amd64 focused integration-test compilation PASS. Vertical-slice cases cover authoritative create/inspect/cache hit, raw mismatch, malformed/corrupt content without row, scan-only/encrypted PDF, injected timeout/resource/malformed failure without row, explicit invalid signature evidence, external link without fetch, and static XLSM inspection. Local Docker Landlock `NotEnforced` prevents a valid runtime GREEN claim; hosted Ubuntu exact-head gate is pending.
- PR #10 OPEN/Draft and unmerged. No Frozen Design amendment, parser dependency promotion, Search Extraction coupling, or common durable content IR in Task 12.
- **Next exact action:** push Task 12 GREEN with this record once, inspect standard CI / Sandbox / DSI PoC at the same head, repair observed failures if any, and then begin Task 13 91-case parity RED.

### Production execution update — Task 10 complete / Task 11 GREEN pending (2026-09-26)

- Task 10 GREEN exact head `f7f70130c3b51aeb0f894866c4a3441ac89adabd`: standard CI `36249100418`, DSI Sandbox Preflight `36249100413`, DSI PoC `36249100409` all **SUCCESS**. Focused Application contract 7/7 PASS, including cache order, raw-binding integrity, result validation, and signature evidence convergence. Task 10 is **COMPLETE**.
- Task 11 test-only RED head `c2a8216c0fdf8e06359e5b7e7d06870fe2c048b9`: repository target compiled only to missing `SemanticInspectionRepository` methods (E0599); schema target failed only because `document_semantic_inspections` did not exist (42P01). No production schema/source was present at that head.
- Task 11 local GREEN source head `ed0db578710a56f0bc336ea77a4e1144788a2cfa`: schema 1/1, repository 3/3, concurrent convergence 2/2 PASS against PostgreSQL 18.6; strict repository all-target Clippy, Rust 1.98.1 fmt, and diff checks PASS. It adds a typed immutable key/raw/fingerprint record and JSONB evidence, with no durable common content IR. Hosted exact-head gates remain pending; Task 11 is not yet COMPLETE.
- PR #10 remains OPEN/Draft and unmerged. No Frozen Design amendment or Task 11 parser dependency promotion.
- **Next exact action:** push Task 11 GREEN and this record once, inspect same-head standard CI / Sandbox / DSI PoC, then begin Task 12 test-only RED using real filesystem, PostgreSQL, Application, Linux runner, and worker.

### Production execution update — 2026-09-26 (supersedes older gate text below)

- Tasks 1–9: **COMPLETE**. Task 9 GREEN exact head `51fea3f1aa216246acfded287a50148b275da599`: standard CI `36248487276` **SUCCESS**, DSI Sandbox Preflight `36248487335` **SUCCESS**, DSI PoC `36248487264` **SUCCESS**. Hosted Ubuntu runner isolation succeeded; local Docker's Landlock `NotEnforced` behavior remains a fail-closed environment observation, not a production fallback.
- Task 10: RED head `015c7c3af9e828686cdcaa9eb0ff992d5fa71adf` was committed locally. Focused compile failed only on missing `EnsureSemanticInspection`, record, repository, executor, and execution-error APIs. GREEN source commit `636a93a` passed the six focused contracts and strict Application Clippy. A supplemental converged-signature-evidence contract then failed behaviorally against that source and passed after the scoped repair (focused contract now 7/7 PASS). Root Rust 1.98.1 fmt and diff checks pass after formatting. Task 10 exact-head hosted gates remain pending; do not declare complete yet.
- Branch `feat/document-semantic-inspection-v0`, PR #10 OPEN/Draft and unmerged. No Design change/amendment, production dependency promotion, or Search Extraction coupling in Task 10.
- **Next exact action:** commit the supplemental Task 10 repair and this status, push once, inspect exact-head standard CI / Sandbox / DSI PoC. If all pass, mark Task 10 COMPLETE and proceed to Task 11 test-only RED for immutable PostgreSQL persistence and concurrency convergence.

**PoC Qualification and Production Tasks 1–8 are COMPLETE. Task 9 Linux sandbox runner GREEN is local; exact-head hosted verification is next.**

Signature clean initial RED head `303cc898b59fd3025bff97a93c431dbb598dd75d` had CI `36241424330` FAIL only on missing signature APIs, Sandbox `36241424298` SUCCESS, and PoC `36241424407` FAIL only on the self-contained XMLDSig false Valid case. Supplemental unsigned-Manifest RED head `a592d6804ef75aadf98c904320d63066970e4f42` had CI `36242162962` FAIL, Sandbox `36242162900` SUCCESS, and PoC `36242162901` FAIL as intended. GREEN head `eeed985f229bbcacd08a7ea955b305e4fc30f010` passed exact-head standard CI `36245310311`, Sandbox `36245310222`, and PoC `36245310177` (all SUCCESS). Focused signature contract 12/12, actual binary explicit-trust FD 2/2, strict worker Clippy, fmt, and staged diff checks passed. Independent reviews cleared package-part coverage, trust transport, and CMS SignerInfo/certificate binding blockers. Task 8 is COMPLETE. Next exact action: commit Task 9 test-only Linux sandbox runner RED, record the focused failure, then implement runner GREEN. Frozen Design remains unchanged; no amendment is proposed.

Task 9 test-only RED commit `630501e12ed1283813b3015ce42bf498480116eb` was local only: synthetic worker baseline 1/1 PASS; isolation test compilation failed only on missing `LinuxSandboxRunner`, `RunnerConfig`, `RunnerInput`, and `RunnerError` APIs. GREEN candidate adds the qualified Landlock 0.4.7, seccompiler 0.5.0, libc 0.2.189, and thiserror 2.0.21 composition, bounded Linux supervisor, separate read-only input/trust FDs, private scratch, RLIMIT before worker native initialization, and worker-side Landlock/seccomp after PDFium warm-up. Focused Linux runner and worker compile and strict Clippy pass under Rust 1.98.1; macOS runner/worker strict Clippy and synthetic baseline 1/1 pass. Local Docker's Landlock status is `NotEnforced` and the isolation test fails closed there. Exact-head hosted Ubuntu enforcement/CI, Sandbox, and PoC results are required before completion. Next exact action: commit/push GREEN candidate and inspect those three runs. Frozen Design remains unchanged; no amendment is proposed.

Current head `acdf430c732bd504dfe6e38c0894c07b8bc8657b` is the PDF Step 2 GREEN candidate on local, origin, and PR #10 (OPEN/Draft); its exact-head CI `36240609880`, Sandbox `36240609839`, and PoC `36240609860` all succeeded. Earlier head `a53a52a12d0ef29e6e4f9a2f10a7dc69e6931de9` is the test-only Type 3 glyph supplemental RED. Its focused production and PoC tests failed only at equal-fingerprint assertions after pinned PDFium native-text/raster proof; CI `36239695270` failed only at known PDF link assertions, Sandbox `36239695276` succeeded, and PoC `36239695252` failed only at known CTM. Earlier paint-order RED at `e6e3d6d1d9f785827237a14d3a3c45d75fc33a13` remains recorded above. Local Type 3 GREEN passes pinned full Production 351/351 tests (4 skipped) and PoC 91/91 fixtures plus all regression and dependency gates. Independent narrow Type 3 review returned GO. Initial signature exact-head RED is clean. Next: supplemental package-part coverage RED, then Step 4 GREEN. Signature RED/GREEN follows. No Design amendment is proposed.

### Historical Task 6 local repair evidence

The following interim local states were superseded by the final Task 6 exact-head GREEN above.

Supplemental Task 6 local RED evidence (Rust 1.98.1, `spreadsheet_credential_leak.rs`, before source repair): the seven focused synthetic cases compiled and failed at behavioral assertions. ODBC `PWD=` and hyperlink/external-workbook URI userinfo reached stdout; `webPr` without `dbPr`, `refreshOnLoad`, foreign-namespace `dbPr`, and duplicate `dbPr` were accepted. The ordinary qualified ODBC fixture still passed. The test file passed rustfmt; no credential value was printed in diagnostics. The current source repair must return a generic failure and no result for these inputs while retaining qualified noncredential ODBC semantics.

Task 6 local GREEN after the scoped connection repair: pinned Rust 1.98.1 `cargo test --workspace --locked` **PASS** (including credential cases 8/8, spreadsheet semantics 8/8, XLSM/VBA 6/6), strict workspace Clippy **PASS**, `cargo fmt --all -- --check` **PASS**, `cargo deny check` **PASS**, and staged diff whitespace check **PASS**. The repository policy script from `mise.toml` also passed when executed directly; `mise run repo:policy` attempted an unrelated automatic `cargo-nextest` installation and was interrupted before running the policy task. Independent final review and fresh hosted exact-head GREEN remain pending. The only unstaged/untracked path is generated PoC `target/`, which must not be committed.

A subsequent independent review found three further accepted credential-bearing forms: an ODBC `SERVER=user:secret@host`, a scheme-less external workbook `user:secret@host/path`, and a hyperlink query with a credential alias. Supplemental compile-valid RED: `spreadsheet_credential_leak.rs` had **8 PASS / 3 expected FAIL**, each behavioral acceptance with the synthetic marker in stdout. Scoped local GREEN now rejects userinfo-like `@` in external targets and ODBC server values, and rejects query/fragment external targets as outside the qualified credential-free subset. Pinned Rust 1.98.1 focused results: credential suite **11/11 PASS**, spreadsheet semantics **8/8 PASS**, response **1/1 PASS**. The post-repair full local gate then passed: `cargo test --workspace --locked`, strict workspace Clippy, `cargo fmt --all -- --check`, `cargo deny check`, staged diff whitespace check, and the repository policy script executed directly from `mise.toml`. Fresh independent final review and hosted exact-head GREEN remain pending. New `gpt-6-luna/max` worker attempts initially failed with a 401 authentication error; no model substitution was made. An exact-model read-only reviewer retry is now running.

That independent review returned NO-GO because other free-form ODBC/header/SQL fields could serialize userinfo-like values. New compile-valid local RED: all nine mutated qualified ODBC fields were accepted/echoed, and an SQL string literal carrying the synthetic marker was accepted. The repair now validates a typed v0 ODBC subset: bounded name/identifier/driver labels, host-form server, numeric nonzero port/id, finite metadata values, exact command type, and a simple `SELECT identifiers FROM identifier` SQL grammar. Unknown forms fail closed; successful `ExternalDependency.normalized_reference` is built only from validated canonical fields, with `UID` represented by a SHA-256 digest. The qualified fixture remains accepted. Pinned Rust 1.98.1 focused GREEN: credential suite **13/13 PASS**, spreadsheet semantics **8/8 PASS**, response **1/1 PASS**. The latest `cargo test --workspace --locked`, strict Clippy, fmt, cargo-deny, staged diff check, and repository policy script all **PASS**. Fresh independent review and exact-head hosted GREEN remain required before Task 6 COMPLETE.

The next independent pass found two further bounds. A 1,025-byte ODBC connection parsed as unsupported instead of a resource failure before segment allocation; a compile-valid local RED test captured that mismatch. The qualified five-field ODBC subset now has a 1,024-byte pre-split bound, field-length checks, and at most five segments. Focused credential GREEN is **14/14 PASS**. The reviewer also traced `rxls 0.1.3`'s `DrawingMetadataPartial` signal: a nested drawing anchor causes the parser to omit an image while the adapter formerly returned success. A compile-valid local RED test reproduced that success despite the loss signal; the adapter now fails closed on `DrawingMetadataPartial` and its parser resource-limit signal. Focused spreadsheet GREEN is **9/9 PASS**, and worker-response **1/1 PASS**. Fresh pinned Rust 1.98.1 full workspace tests, strict Clippy, fmt, cargo-deny, repository policy, and staged/unstaged diff checks all **PASS**. These additions are local and uncommitted; final independent review and exact-head hosted GREEN remain required.

A further independent review returned NO-GO on the approved **16 MiB structured-result bound**. A compile-valid local RED with 2,200 qualified ODBC definitions produced a **19,167,709-byte successful stdout response**. The scoped repair checks a serialized external-dependency byte budget before each accumulation and runs a bounded streaming JSON byte counter before the canonical response clone, Value conversion, and stdout write. The shell also checks the final canonical byte length. The same oversized input now returns a resource-limit failure with empty stdout; the focused worker-response suite is **2/2 PASS**. A separate test accepts an exact 16 MiB serialized response and rejects one byte over. That review then found `compare_hyperlinks` created two large comparison Vecs before the dependency budget. A supplemental unit contract first failed to compile because the early hyperlink oracle budget was absent; the source now limits aggregate rxls hyperlink targets before Calamine opens and limits Calamine target/location text before comparison Vec construction. The exact/one-over hyperlink budget test passes. Fresh pinned Rust 1.98.1 full workspace tests, strict Clippy, fmt, cargo-deny, repository policy, and staged/unstaged diff checks all **PASS** after this repair. Independent review remains in progress; no hosted Task 6 GREEN head exists yet.

The hyperlink-bound review confirmed the comparison Vec path was closed but found spreadsheet comments were cloned into editorial evidence before the response-size check. A supplemental unit contract first failed to compile without the comment budget. The repair measures borrowed `CommentEvidence` JSON bytes with the bounded streaming counter and applies an aggregate 16 MiB check before cloning each comment. Exact-bound and one-byte-over comment tests pass. Fresh pinned Rust 1.98.1 full workspace tests, strict Clippy, fmt, cargo-deny, repository policy, and staged/unstaged diff checks all **PASS** after this repair. Independent review of the complete Task 6 response-field accumulation is in progress; hosted Task 6 GREEN remains pending.

PR #8 is merged. Keep production work on PR #10 and do not merge it without an explicit user instruction.
