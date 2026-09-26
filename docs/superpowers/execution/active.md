# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Semantic Inspection v0`
- Current phase: **PRODUCTION IMPLEMENTATION — TASKS 1–6 COMPLETE / TASK 7 PPTX RED COMPLETE, GREEN IN PROGRESS**
- Frozen Design PR: `#7` — merged
- PoC execution branch: `test/document-semantic-inspection-poc-v0`
- Production planning branch: `plan/document-semantic-inspection-v0-production`
- Production planning PR: `#9` — merged as `48045768d1d026eb785ee065877e401bbafd97ca`
- Production implementation branch: `feat/document-semantic-inspection-v0`
- Production implementation PR: `#10` — OPEN / Draft; Task 6 final verified code head `98072f1732157c85ac3d26ce7bf78d64cc456568`; read live GitHub for current PR head; do not merge
- Task 5 initial clean RED: `65d6896322b93c6731f8a836be8b79fcf53beac5`; authoritative repaired clean RED: `e3e6659d243233ff102c393e8be1515a05ba1398`; CI `36135132794` failed only on the expected missing DOCX APIs, Sandbox `36135132761` and DSI PoC `36135132763` passed
- Supplemental test-only head: `bec052e43dfeedb049ac725f8c697cf564ec60b1`; exact-head CI `36136763103` failed on the expected missing `DocxAdapter` / `editorial_provenance()` APIs, Sandbox `36136763068` and DSI PoC `36136762918` passed
- Intermediate ZIP-preflight test-only head: `593eddd14c15b098b377fc91b272239af1b24b12`; exact-head CI `36138987613` failed only on the expected missing DOCX APIs, Sandbox `36138987388` and DSI PoC `36138987376` passed. Its five cases failed as expected against the local pre-fix ZIP guard; fmt and strict worker Clippy passed.
- Prior ZIP-ambiguity test-only head: `9bc17d6ba0074e253266c99f8f59b1fee91dbf8d`; exact-head CI `36144055858` failed only on the missing `DocxAdapter`, `OoxmlCoverageSentinel`, and `editorial_provenance()` APIs. `fmt`, policy, security, macOS portability, and container-build passed; Sandbox `36144056194` and DSI PoC `36144056202` passed. This RED commit added no dependencies.
- Deep semantic test-only RED head: `bae0c63a4c828b43da6a6a797bbdbefeb8161d4b`; exact-head CI `36150028095` failed only on missing `DocxAdapter`, `OoxmlCoverageSentinel`, and `editorial_provenance()` APIs; formatting, policy, security, macOS portability, and container build passed. Sandbox `36150028138` and DSI PoC `36150028162` succeeded. The two new tests add no dependencies.
- PNG PoC test-only RED head: `3e29954c453bebeae3bab76240e4f6bc28fc58e8`; exact-head DSI PoC `36151814090` failed only on the new same-decoded-pixel/different-IDAT equality (DOCX 13/14 passed); Sandbox `36151814219` succeeded; standard CI `36151814139` failed only on planned missing production DOCX APIs, while formatting, policy, security, macOS portability, and container build passed.
- Corrected PNG/header/VML supplemental RED head: `3fede76427991e0c63bc22c5f6615043c796486e`; the indexed 1-bit PNG test fixture now encodes its pixel in the most significant bit. Exact-head DSI PoC `36158858140` failed only on the unchanged PNG IDAT equivalence case (DOCX 13/14 passed); Sandbox `36158858241` succeeded; standard CI `36158858108` failed on missing production DOCX APIs after fmt/policy/security/macOS/container passed.
- PNG decoder/checksum candidate head: `5d627ac6a39b6971077b081fcdab8e35e599f26f`; exact-head DSI PoC `36159892918` passed DOCX 14/14, then failed as intended on referenced header/footer image tests 0/2 before reaching PNG supplemental tests. Sandbox `36159892901` succeeded. Standard CI `36159892994` failed on the planned missing production DOCX APIs, while policy/security/macOS/container passed. The candidate is not a full PoC GREEN gate.
- PoC nonbody-image GREEN head: `e309cbeb9f9075c5c3b900df8905f8ae62490337`; DSI PoC `36161063199` SUCCESS, including PNG 12/12, DOCX 14/14, header/footer 2/2, VML 1/1, full 91-case manifest, and dependency/security gates. Sandbox `36161063009` SUCCESS. Standard CI `36161063006` failed only on planned missing production DOCX APIs. `png 0.18.1` is PoC-qualified but not yet a committed production dependency.
- Current PoC note test-only RED head: `b2bc048f9bcfe0fe6a95523e3d46b6e64ba5a7a0`; DSI PoC `36162476424` failed only on the three new referenced footnote/endnote image and note-table-structure tests after DOCX 14/14 passed. Sandbox `36162476347` SUCCESS. Standard CI `36162476288` failed on missing production DOCX APIs; policy/security/macOS/container passed.
- PoC note/list-marker test-only RED head: `dd62e881abcfd486e4d8ca02c9a35c2bf27e9704`; note table comparison uses an image-free note and `w:lvlText`/`w:start` changes are isolated. Exact-head DSI PoC `36164163811` failed only the note tests, Sandbox `36164163799` succeeded, and CI `36164163875` failed on missing production DOCX APIs.
- PoC note grammar intermediate head: `0a1a017afcfdd8c675afde1319f384a0414d17a4`; exact-head DSI PoC `36166088658` passed note tests 5/5 and DOCX 14/14, then failed only list-marker tests 2/2. Sandbox `36166088662` succeeded. CI `36166088667` passed fmt/policy/security/macOS/container and failed only on the expected missing production DOCX API.
- Current PoC note-reference test-only RED head: `326d80ed369d695035f3889fe87bb853986ac643`; local and hosted focused tests failed 4/4 as intended: two note-ID swaps had equal fingerprints, and two dangling ID references were accepted despite valid single-note baselines. Exact-head DSI PoC `36166774786` passed DOCX 14/14 and note 5/5, then failed only note-reference 4/4. Sandbox `36166774731` succeeded. CI `36166774884` passed fmt/policy/security/macOS/container and failed only on missing production DOCX APIs.
- Supplemental PoC orphan-note/even-header test-only RED head: `52c514d3540105e3acdcedc0caf315d48adcfd3a`; both new cases failed locally against pre-fix PoC source with accepted baselines. Exact-head DSI PoC `36170274726` failed only on the new even-header case after DOCX 14/14; the runner stops before the orphan-note binary, whose RED is locally observed. Sandbox `36170274935` succeeded. CI `36170274841` passed fmt/policy/security/macOS/container and failed only on the expected missing production DOCX APIs.
- Local PoC note-reference/orphan/text-box/even/first-header, selected header hyperlink, list-marker/style-list/default/special numbering, XML event-count, empty-paragraph spelling, and picture geometry/transform repairs passed focused suites. The full `poc:dsi:verify` sequence passed all 91 manifest cases and its dependency gate; production full workspace tests, strict Clippy, fmt, and cargo-deny passed locally. Hosted PoC and production GREEN evidence is listed below.
- PoC source/tests GREEN head `0945e0870e70509628a90237be39bf125afdc273`: exact-head DSI PoC `36180780592` and Sandbox `36180780595` SUCCESS. CI `36180780825` failed only on absent production DOCX APIs; other jobs succeeded.
- Supplemental production test-only RED head `9df2abc77f26867ebc4fd2cc8166e266a25f07ab` includes 21 DOCX tests and qualified zip 8.6.0 as dev-dependency only. Exact-head CI `36181633068` failed only on absent production DOCX APIs; DSI PoC `36181633146` and Sandbox `36181633106` succeeded. This is clean RED; production source/dependencies were not included at that head.
- Production DOCX final GREEN `14bcc4a63ec4ec56289619e4d76a9ca1792315ba`: standard CI `36182511870`, Sandbox `36182511893`, and DSI PoC `36182511885` all **SUCCESS** at this exact head. Precommit full workspace tests, strict Clippy, fmt, and cargo-deny passed locally. Task 5 is COMPLETE.
- Task 5 handoff documentation head `26afc64fd8703fdcf44af45497a2c9d0b41c30a1`: standard CI `36183492393`, Sandbox `36183492436`, DSI PoC `36183492364` all SUCCESS. No code or dependency change.
- Task 6 initial test-only RED head `75b0dbf8b2f59c24093098365c4257feceadddb2` contains XLSX/XLSM workbook semantics, SpreadsheetML package-safety, and worker-response external-dependency tests. Exact-head CI `36187706539` FAIL as expected only on unresolved `SpreadsheetAdapter` E0432; fmt, policy, security, macOS portability, and container-build passed. Sandbox `36187706713` and DSI PoC `36187706727` SUCCESS.
- Task 6 VBA supplemental test-only RED head `7c9e31c184f34c79aa44f450dace05c281341ec0` adds VBA logic/noise/fail-closed/static-only contracts and a PoC-generated synthetic comment fixture. Exact-head CI `36189581362` failed only on the missing `SpreadsheetAdapter` E0432 in rust-static/rust-test; fmt, policy, security, macOS portability, and container build passed. Sandbox `36189581450` and DSI PoC `36189581454` succeeded. No Task 6 production dependency or implementation was included at this clean RED head.
- Independent review findings for note structure, list markers, XML/style/geometry/numbering, headers, pictures, and bounded stderr were addressed with focused RED/GREEN. No frozen Design amendment was required.
- PoC execution PR: `#8` — merged as `ab9ad6f9949128360e46fed07aca335bb6b10971`
- Approved Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- Approved PoC Qualification Plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
- Production Implementation Plan: `docs/superpowers/plans/2026-09-24-document-semantic-inspection-v0-production-implementation.md` — **APPROVED 2026-09-25**
- Execution Status: `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
- Production implementation baseline: `main@48045768d1d026eb785ee065877e401bbafd97ca`
- Baseline main CI: `36079233862` — SUCCESS
- Production Task 1 RED head: `343aa9072da19da471a31b96e05eb92d80784820`
- Production Task 1 RED run: `36080157697` — FAIL as expected
- Production Task 1 qualified head: `0cd3345a12f53f30068c72e56ea8aead367cd0ff`
- Production Task 1 hosted GREEN run: `36084114757` — SUCCESS
- Production Task 2 RED head: `d0bab4a824b6f125a20a55edd9fec21b53233fb3`
- Production Task 2 RED CI: `36086136455` — FAIL as expected on unresolved core contract imports
- Production Task 2 GREEN head: `550913b93a2c37360544450ff9dc53164679cf8a`
- Production Task 2 standard CI: `36086709602` — SUCCESS
- Production Task 2 sandbox regression: `36086709619` — SUCCESS
- Production Task 2 DSI PoC regression: `36086709653` — SUCCESS
- Production Task 3 shell RED head: `1911a80b0e018119cb4c2c94161dc72a24641c00`
- Production Task 3 shell RED CI: `36094572575` — rust-static FAIL as expected on unresolved `run_worker_shell` after fmt PASS
- Production Task 3 GREEN head: `817c25330f5348b2ab2b0683141329241b3be3f2`
- Production Task 3 standard CI: `36094896067` — SUCCESS, including required-check
- Production Task 3 sandbox regression: `36094896150` — SUCCESS
- Production Task 3 DSI PoC regression: `36094896352` — SUCCESS
- Production Task 4 initial RED head: `9ed0f735474ff25381814cbad63ef2c5965c76f9`
- Production Task 4 initial RED standard CI: `36095951774` — FAIL
- Production Task 4 sandbox regression: `36095951807` — SUCCESS
- Production Task 4 DSI PoC regression: `36095951824` — SUCCESS
- Production Task 4 clean RED head: `64e1419a43d45c178e2f85cb1825ea9a1ff27aad`
- Production Task 4 clean RED standard CI: `36097679945` — FAIL as expected only on unresolved adapter contract imports
- Production Task 4 clean RED Sandbox regression: `36097679887` — SUCCESS
- Production Task 4 clean RED DSI PoC regression: `36097679960` — SUCCESS
- Production Task 4 first GREEN head: `50578e1f4722a2a5d461f5e13a1227d177b0ff3c`; CI `36099520593` exposed one strict Clippy warning, fixed in the final head
- Production Task 4 final GREEN head: `963a144add686b32afa510cf43a4ecce967f042b`
- Production Task 4 standard CI: `36099955617` — SUCCESS
- Production Task 4 Sandbox regression: `36099955599` — SUCCESS
- Production Task 4 DSI PoC regression: `36099955606` — SUCCESS
- Last PoC-qualified code head: `a4fcef1cb5cac5672199165f433bd303c25135a6`
- Task 4 DSI qualification: `35818792833` — SUCCESS
- Task 4 standard CI: `35818792843` — SUCCESS
- Task 5 DSI qualification: `35824677799` — SUCCESS
- Task 5 standard CI: `35824677794` — SUCCESS
- Task 6 DSI qualification: `35880533515` — SUCCESS (Linux + macOS Intel + macOS arm64)
- Task 6 standard CI: `35880533521` — SUCCESS
- Task 7 DSI qualification: `35947786029` — Linux SUCCESS; final macOS cross-host gate moves to Task 8
- Task 8 final cross-host DSI: `35957940553` — Ubuntu / macOS Intel / macOS arm64 SUCCESS
- Task 8/9 standard CI evidence: `35957940565` — SUCCESS
- Task 3 dependency-preflight DSI: `35545142423` — SUCCESS

## Mandatory resume order

When resuming this repository, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
4. frozen Design Spec
5. Design approval record
6. approved Production Implementation Plan
7. current GitHub state of `feat/document-semantic-inspection-v0`, PR #10, and exact-head CI

Repository and fresh GitHub state override remembered/chat state.

## Current scope

Design is approved and frozen. PoC Qualification Plan was explicitly approved on 2026-09-21.

Production Tasks 1–6 are complete. Task 6's final exact-head standard CI, Sandbox, and DSI PoC gates passed after its scoped container packaging repair. Task 4 promoted only its qualified TXT/CSV/HTML parser dependencies; `scraper` remains excluded. Task 5 promoted PoC-qualified `office_oxide 0.1.11`, deflate-only `zip 8.6.0`, `quick-xml 0.42.0`, and supplemental scoped `png 0.18.1` after clean RED and exact-head GREEN. Task 6 promoted only its qualified XLSX/XLSM/VBA parser composition. Task 7 initial clean RED is confirmed at `452c76d0d92aedb31c4c447cef5bae75388a8b63`, and supplemental clean RED at `6f32cbe87728a75ffd118862597cabbba54d2250`.

Task 2 produced one material qualification result: `scraper 0.27.0` was rejected because its transitive graph contains MPL-2.0. Direct `html5ever 0.39.0 + markup5ever_rcdom 0.39.0` passed the same semantic cases and the dependency gate.

## Current hard gate

The PoC qualification gate is **complete**. The Production Implementation Plan was explicitly approved by the user on 2026-09-25, and PR #8 is merged. Production Tasks 1–6 are complete after clean RED and fresh exact-head GREEN evidence. Task 7 PPTX initial and supplemental clean RED are complete; PoC GREEN is next. No Design amendment is in progress.

Task 6 final GREEN head `98072f1732157c85ac3d26ce7bf78d64cc456568` passed exact-head standard CI `36209740995`, Sandbox `36209740990`, and DSI PoC `36209741011`. Standard CI included a successful container-build and required-check. PR #10 remains OPEN / Draft. Read live GitHub before acting. Do not merge without explicit instruction.

Task 7 initial test-only RED head `452c76d0d92aedb31c4c447cef5bae75388a8b63` passed standard CI formatting, policy, security, macOS portability, and container build; standard CI `36210857702` failed only on the expected missing `PptxAdapter` E0432 in Rust static/test jobs. Exact-head Sandbox `36210857803` and DSI PoC `36210857696` succeeded. PR #10 remains OPEN / Draft.

Task 7 supplemental test-only RED head `6f32cbe87728a75ffd118862597cabbba54d2250` passed standard CI formatting, policy, security, macOS portability, and container build; standard CI `36212761629` failed only on expected missing `PptxAdapter` E0432. Exact-head Sandbox `36212761543` succeeded. Exact-head DSI PoC `36212761571` failed only on the new chart-title semantic assertion; other supplemental PoC cases had focused local behavioral RED. PR #10 remains OPEN / Draft.

An additional Task 7 coverage probe found that an unmodeled `customXml/item1.xml` with a forged known `slide+xml` content-type override passed the current PoC adapter. The PoC-only test was committed at `4aba9aacd4d2818b5b6e241982c15452875b88bf` after focused Rust 1.98.1 behavioral RED (only the expected fail-closed assertion). The matching production test was committed at `821ff96ceff02fbd6b7ccdcdbdc9d178d38ff4b2`; its focused compile RED was only the missing `PptxAdapter` E0432. Both tests verify ZIP/XML validity and mutation scope. PR #10 is OPEN / Draft at `821ff96ceff02fbd6b7ccdcdbdc9d178d38ff4b2`; PoC repair is uncommitted work in progress. No production adapter or dependency has been added.

Additional test-only head `c8904b6655a094ceacebdb8c1fb27af6950a28e5` records four focused PoC behavioral RED cases: `[Content_Types].xml` declared 64 MiB + 1 before format sniffing, chart value-point `idx` association, foreign namespace chart extension, and visible chart data-label setting. Matching production point-index, extension, and data-label tests compile RED only because `PptxAdapter` is absent; the existing production resource-limit test already covers the content-types preflight. Test fixture ZIP CRC/XML and mutation-scope preconditions passed in the focused PoC runs. The PoC path/content-type coverage and safe slide mapping are locally GREEN but remain uncommitted together with the pending chart/preflight fixes.

Exact-head hosted evidence at `c8904b6655a094ceacebdb8c1fb27af6950a28e5`: standard CI `36214493107` failed only on missing `PptxAdapter` E0432 in Rust static/test; formatting, policy, security, macOS portability, and container build succeeded. DSI Sandbox Preflight `36214493097` succeeded. DSI PoC `36214493094` failed only at the first new chart data-label fingerprint assertion after prior suites passed; the other new PoC cases had focused local behavioral RED. This is clean additional Task 7 RED evidence.

Independent ChartML review found that an explicit per-point `showCatName=false` override was discarded when global `showCatName=true`. The two test-only files were committed/pushed at `2c279a22c54ff1b0ca4118569689f54f2addb54e`: the PoC focused test reached only the intended fingerprint inequality failure after ZIP/XML and chart-only mutation checks; the production focused compile failed only on absent `PptxAdapter` E0432. PoC preflight, chart point mapping, labels, foreign-extension rejection, namespace prefix, and package coverage are locally GREEN; the per-point override repair is in progress. PR #10 remains OPEN / Draft. No Task 7 production adapter/dependency has been added.

Exact-head hosted evidence at `2c279a22c54ff1b0ca4118569689f54f2addb54e`: standard CI `36215933182` failed only on missing `PptxAdapter` E0432 in Rust static/test; formatting, policy, security, macOS portability, and container build succeeded. Sandbox `36215933216` succeeded. DSI PoC `36215933201` failed only at the earlier chart data-label RED assertion before reaching the new override test; the new override had its own focused local behavioral RED. A read-only probe of a slide relationship pointed at a chart-content-type part was rejected with `SemanticExtractionFailed` (fail closed), so no additional Task 7 mutation was needed.

The per-point override is now locally GREEN. Pinned Rust 1.98.1 full `mise run poc:dsi:verify` with pinned PDFium `7881` completed exit 0: all PoC tests passed, cargo deny reported advisories/bans/licenses/sources OK, the fixture report was `overall: PASS`, `pptx: PASS`, and 91/91 case verdicts pass. `git diff --check` passed. The source and this execution record remain uncommitted pending independent read-only audit; this local result is not hosted exact-head GREEN.

The independent final read-only audit returned **NO-GO** for one uncovered reader-visible ChartML case: adding/removing `<c:legend>` did not affect the PoC projection, although it changes visible series labels under frozen Design §9.4. Matching test-only PoC and production cases were committed/pushed at `de33d36c1885912ffc0fade0e4ab5bb40e3bce98`. The focused PoC test failed only at the expected fingerprint inequality after valid ZIP CRC/XML and chart-only mutation checks; the production focused compile failed only on missing `PptxAdapter` E0432; root `cargo fmt --all --check` passed. Exact-head CI `36216517105` failed only on the missing `PptxAdapter` E0432 in Rust static/test after fmt/policy/security/macOS/container succeeded; Sandbox `36216517133` succeeded; PoC `36216517125` failed only at the earlier expected chart data-label RED assertion before the legend binary. The local full PoC PASS above predates this legend case and is not hosted GREEN.

A scoped read-only ChartML inventory found five further classes currently omitted by the PoC parser: axis scales/number formats, data tables, trendline, error bars, and chart-level data-display switches. These can change visible chart data/labels; focused test-only fail-closed RED is being prepared before a narrow PoC source repair. No new chart semantics or Design amendment is being introduced.

The six focused ChartML cases were committed as test-only head `dc4e54c60bf54bfbcb13404145a693d8a8c5dd14` after valid ZIP/CRC/XML chart-only mutants were wrongly accepted by the PoC. Local PoC and production WIP repairs pass 6/6. Sandbox `36217261838` succeeded; PoC `36217261870` stopped at an earlier expected data-label RED; CI `36217261843` was cancelled by the next push.

Four malformed legend cases were locally RED and two valid cases passed before repair. Their test-only head is `0f12de602da24123f80e68ddea49ab53925c989e`. Local PoC and production repairs now pass the six-case suite and broader PPTX suites (PoC 31, production 35 tests). Exact-head CI `36217557087` failed only on missing `PptxAdapter` E0432 after fmt/policy/security/macOS/container passed; Sandbox `36217557044` succeeded; PoC `36217557052` stopped at the earlier expected data-label RED. PR #10 remains OPEN/Draft.

Independent production audits found the package-wide 2,000,000 XML-node cap and OPC Content Types/Relationships QName validation missing. Test-only package and SmartArt RED cases were committed at `78bdfd85d2b97536d09a792287e5906cbfaa3d17`: four wrong OPC roots and one aggregate XML-node overflow were accepted, and SmartArt role/duplicate-ID cases missed meaning or ambiguity. Exact-head CI `36218126620` failed only on missing `PptxAdapter` E0432 after fmt/policy/security/macOS/container succeeded; Sandbox `36218126612` succeeded; PoC `36218126635` failed at the earlier expected data-label RED. Local scoped PoC and production package/SmartArt repairs are uncommitted. Connector endpoint, picture crop/rotation/flip, and SmartArt semantics are within frozen Design §9.4/§13.5; auto-shape geometry/rotation and same-pixel PPTX PNG re-encoding remain outside approved v0 pending amendment.

Connector endpoint and picture crop/rotation/flip test-only RED cases were committed/pushed at `6640d6f5db6796a492d6923c6450c8bdb49b33d4`. Both adapters accepted valid same-geometry connector inputs with different referenced shapes and produced equal fingerprints. Both accepted same-image-byte, visually distinct crop/rotation/flip inputs with equal fingerprints. Focused ZIP CRC/XML, single-part mutation, and non-symmetric image preconditions passed. Exact-head CI `36218461687` failed only on missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36218461635` succeeded; PoC `36218461632` stopped at the earlier expected data-label RED. PoC and production connector repairs now pass their focused tests and PPTX regressions locally.

PoC and production picture crop/rotation/flip fixes now pass their focused 3/3 tests. The PoC passed 17 PPTX regression test targets excluding the separately RED root-relationship suite. The production adapter passed 43/43 other PPTX regressions. Pinned formatting and changed-file whitespace checks passed. All source remains uncommitted pending integrated verification and review.

A further local test-only probe of PPTX package root relationships found that missing `_rels/.rels` and duplicate `officeDocument` relationships were accepted by both adapters. A legal XML character reference in the presentation ContentType value was rejected as `FormatMismatch` by raw serialized-value sniffing even though it only changes serialization. Matching scoped PoC and production tests passed pinned formatting and were committed/pushed test-only at `b2742bb34dc3132ac6674cb4a660d9556a5376c6`; focused PoC and production WIP results were each 1 PASS / 3 expected behavioral FAIL. Exact-head CI `36219047006` failed only on missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36219046955` succeeded; PoC `36219046957` stopped at the earlier expected chart data-label RED. Source fixes have not yet been promoted.

The uncommitted production package fix now passes the focused root-relationship/ContentType suite 4/4 and the entire worker crate integration suite under pinned Rust 1.98.1, including package safety, namespace, chart, SmartArt, connector, picture, and aggregate XML-node regressions. PoC root-relationship/ContentType source repair is next; no hosted GREEN has yet been claimed.

The corresponding uncommitted PoC root-relationship/ContentType repair now passes focused 4/4 and PPTX regression 45/45 under pinned Rust 1.98.1. An independent picture review found in-scope omissions of visible tile/grayscale image behavior, redundant namespace serialization, and production rotation range. Matching test-only RED files were committed/pushed at `fa11d63844036853314e1ddbe923238140399aa5`: PoC focused fill/effect 0/2 expected FAIL and namespace/range 1 PASS / 2 expected FAIL; production namespace/range 2 PASS / 1 expected FAIL. Production fill/effect focused local run was deferred while disk free was about 2 GiB. Exact-head CI `36220064505` failed only on absent `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36220064507` succeeded; PoC `36220064503` stopped at the earlier expected chart data-label RED. Source fixes are uncommitted. A further package review found that an inter-element XML whitespace character reference is falsely rejected; a scoped test-only RED is being prepared. The repository root build cache was removed with official `cargo clean`, freeing about 12.4 GiB; PoC build cache and pinned PDFium remain.

The scoped OPC whitespace character-reference test-only head is `19f68b3065f2864039d73557861202f5809b7cb6`. Pinned PoC focused test 1/1 PASS with base/literal-space/reference fingerprint equality. Pinned production WIP focused test failed only because the valid character reference was rejected as `UnsupportedSemanticConstruct`; baseline and literal-space variants were accepted. Exact-head CI `36220599599` failed only on absent `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36220599703` succeeded; PoC `36220599596` stopped at the earlier expected chart data-label RED. PoC picture strict source fixes pass focused 5/5 locally. Full pinned Rust 1.98.1 `mise run poc:dsi:verify` with pinned PDFium `7881` completed exit 0: all PoC test targets passed, `cargo deny` advisories/bans/licenses/sources OK, fixture report `overall: PASS`, all eight format gates PASS, and 91/91 case verdicts pass. This is local GREEN only; hosted PoC GREEN remains pending. Production OPC repair passes focused 1/1 and package regressions 12/12 locally, and production picture strict cases pass 5/5. Pinned Rust 1.98.1 production `cargo test --workspace --locked` exit 0, `cargo fmt --all -- --check` PASS, and root `cargo deny check` advisories/bans/licenses/sources OK. A final independent semantic audit found possible omissions of SmartArt sibling order and referenced slide-layout visible text; scoped test-only confirmation is in progress before source promotion.

The SmartArt sibling `srcOrd` change was locally accepted by PoC and production WIP with equal fingerprints despite a valid three-point graph and only the diagram data part changing. The two focused test-only RED files were committed/pushed at `4d0b103834e09f8b6853c8065b83b00abccba046`. Exact-head CI `36221473575` failed only on expected missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36221473522` succeeded; PoC `36221473524` failed only on the earlier chart data-label RED. Scoped SmartArt order source repairs pass focused PoC and production tests locally. Supplemental duplicate `srcOrd` / `destOrd` fail-closed tests had focused intended RED in both adapters and were committed/pushed test-only at `2b496b0616650bc30fc37384c6dbb38016f04445`. Exact-head CI `36221992539` failed only on expected missing `PptxAdapter` E0432 after formatting, policy, security, macOS portability, and container build passed; Sandbox `36221992489` succeeded; PoC `36221992513` failed only on the earlier chart data-label RED. PoC duplicate-order source repair passes focused 3/3, role/ID 2/2, and base PPTX 7/7 locally. Full pinned Rust 1.98.1 PoC verification with PDFium 7881 exited 0: all tests and cargo deny passed; fixture report 91/91 passed. This source is included in the present PoC promotion commit; hosted GREEN remains pending. Production duplicate-order repair passes focused order 3/3, role/ID 2/2, base PPTX 8/8, and strict worker-library Clippy locally; it remains uncommitted. A standard slide-layout package with proven visible text difference was rejected by both adapters on an unsupported printerSettings content type, so no accepted-input layout omission was demonstrated and no test was retained. Eight mechanical Clippy lints in uncommitted production `pptx.rs` were fixed. The production Task 7 adapter is not yet promoted.

## Next exact action

Obtain hosted DSI PoC GREEN for this PoC source promotion head while production CI still fails only on absent `PptxAdapter`. Finish production duplicate-order fail-closed repair, root workspace tests, strict Clippy, and independent review. Then commit the bounded production adapter and require fresh exact-head standard CI, Sandbox, and PoC successes before Task 7 COMPLETE. Continue Tasks 8–14 sequentially. Keep PR #10 Draft and unmerged; no Design amendment is in progress.

## Resume command

> `AIrisu-072/knowledge-platform` のrepositoryとGitHubの現在状態を正本として続行してください。最初に `AGENTS.md`、このActive、Execution Status、Frozen Design、Design approval、承認済みProduction Plan、live branch/PR/CIの順に確認してください。Production Tasks 1–6 COMPLETE。Task 7 PPTX の最新test-only RED headは `2b496b0616650bc30fc37384c6dbb38016f04445` です。CI `36221992539` はmissing `PptxAdapter`のみでFAIL、Sandbox `36221992489` SUCCESS、PoC `36221992513` は以前のchart data-label REDでFAILです。このcommitにPoC PPTX source修正が含まれ、pinned Rust/PDFium full verification exit 0、91/91 fixture PASSです。まずこのheadのhosted PoC GREENを取得してください。Production sourceのSmartArt重複順序修正と最終root gates/reviewを終えた後、production adapterをpromotionしてください。Task 7完了には同一headのstandard CI・Sandbox・PoC SUCCESSが必要です。PPTX PNG再エンコード同一視とauto-shape geometry/rotation候補はDesign amendment未承認のため除外します。PR #10はOPEN/Draftのままmergeしません。その後Tasks 8–14を順に進めてください。

## End-of-session rule

Before intentional session switch/context exhaustion, record exact branch/head, CI evidence, Plan approval state, blockers, and next exact action.
