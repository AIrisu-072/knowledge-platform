# Document GUI Integration v0 — Capability Execution Status

## 2026年10月4日 受入済み後続sourceを用いた統合

- PR36旧head自身にsame-head frontend/実backend統合の証拠が不足していた記録を、そのまま履歴として保持する
- 後続[PR43](https://github.com/AIrisu-072/knowledge-platform/pull/43) exact `6103e4d4e3bb0d45ba03e1d2935492de7f11394a` は実compositionと制限付き画面レビューを経て、[所有者が最終受入](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)した。最終報告R2はPR46 `88628331f32e141c71944e9f7f093deb348f51ac`
- 今回はPR57までの受入sourceとPR36最新を一つの候補へ統合する。新candidateのexact CI・実browser/backend受入はこれからで、下記の旧headの結果を流用しない
- 凍結GUI design/plan/approvalの原本、Version/Revision/OCC、認可、Diffの意味は変更しない。新機能や本番Identityの導入はしない
- 現在のgateと次の操作は[統合状況](document-organization-integration-status.md)。候補の実CI成功後に統合経路のG9証拠を記録する。PR36の過去headを遡及的なPASSにはしない

以下は当時の記録。

---

## Current checkpoint — 2026-10-02 UTC — hosted gates GREEN / integrated frontend evidence pending

- 状態: **G0〜G8 COMPLETE / IMPLEMENTATION COMPLETE / G9 FINAL ACCEPTANCE PENDING / NOT MERGED / NOT DEPLOYED**. The product implementation is present, and the three hosted gates are green. The approved Plan requires final same-head frontend E2E and the real-backend integrated journey as well; therefore **G0〜G9 COMPLETE / ACCEPTANCE GREEN / REVIEW READY** is not yet asserted by this evidence reconciliation.
- Verified GitHub state (2026-10-02 00:15 UTC / 09:15 JST): [Draft PR #36](https://github.com/AIrisu-072/knowledge-platform/pull/36), branch `feat/document-gui-integration-v0`, exact head `b578a9b49338066d0e4ee5495ea1280f991c122b`, base `main` at `d71753d46590bb4406a1c0b74894ab90a27a6c88`; OPEN / Draft / NOT MERGED. Review submissions: 0; inline review threads: 0 (unresolved: 0). Review-ready acceptance is not a GitHub review approval.
- Observed exact-head hosted gates: [Standard CI 36860705179](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705179) **SUCCESS**, [DSI Sandbox Preflight 36860705023](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705023) **SUCCESS**, [DSI PoC 36860705167](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705167) **SUCCESS**. Standard CI required-check and all nine jobs succeeded. DSI PoC qualification succeeded; its optional qualification-macos job was skipped, not passed. Standard CI separately passed DSI semantic parity on macOS ARM and Intel.
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24` and approved Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912` match the committed files. Source Design ZIP SHA-256 remains the approval-recorded `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`; the ZIP was not reread in this docs-only pass. Design Amendment 01 / Approval and Plan Addendum 01 are the only approved toolchain/license exception. Current pnpm lock SHA-256 `bb74081c198fcb1c9c8038933c434f0a00b50a2aa7a436c3def15156881d6847` matches the amendment's first-party workspace-link inventory record; general license policy and UI/API semantics are unchanged.

### G9 observed hosted acceptance

[Standard CI rust-test job 110363914358](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705179/job/110363914358) logs **741 tests passed, 6 skipped** at this exact PR head. Specific observed PASS entries include:

- `document-api-http::e2e::postgres_filesystem_and_workers_complete_the_document_http_journey`: real PostgreSQL + FileSystemStorage + production DSI/Diff runner lifecycle, including metadata minor, content major, withdrawal fallback and monotonic human Revision assertions in the committed test
- `document-api-http::diff_http::revision_comparison_keeps_content_and_metadata_projections_separate` and `same_version_revision_comparison_returns_metadata_only_with_audit`; Application comparison tests preserve unavailable legacy metadata and avoid content Diff for the same authoritative Version
- `document-api-http::read_http::history_and_policy_resolve_identity_presentations_once_per_response` and `unavailable_identity_presentation_does_not_fail_history_or_policy_reads`; session uses verified actor only
- `document-api-http::management_http::capability_snapshot_does_not_authorize_a_later_mutation_after_schedule_reservation` and current-authority/revocation transaction tests; capability remains a UI hint, with mutation authorization authoritative
- `document-api-http::file_download_http::range_audit_and_storage_failures_never_stream_file_bytes`; audit/storage failure does not disclose bytes
- bounded display projection tests, including exact-limit/one-byte-over fragment tests and raw-binding fail-closed behavior

Hosted Standard CI also passed policy/API/assurance, Rust static checks, security, container/SBOM, macOS portability, both macOS DSI semantic parity jobs, and required-check. These are observed hosted results, not local reruns on the cloud docs worktree.

### Frontend evidence provenance and remaining acceptance gates

- The repository's prior checkpoint and PR description record local Jest **11 suites / 34 tests PASS**, TypeScript/build PASS, OpenAPI/client contract **12/12 PASS**, and Playwright **6/6 PASS**. The exact frontend E2E execution head explicitly recorded in the repository is `bb5da30c7a831a9d79a16cc422f00adc89b70c69`, with `T_usable=349.7 ms`, `T_input=16.5 ms`, and `motionSpatial=180 ms`. These measurements are historical local evidence; no new measurements were collected here.
- `git diff bb5da30 b578a9b -- apps/document-web packages/document-api-client pnpm-lock.yaml package.json pnpm-workspace.yaml` is empty. App tree `bee8e9eefb032f51d487bc66c1810ba19fc58227` and client tree `6fc5f94e0e0a726a0638a61c37a5704f577480f9` are identical. This supports unchanged frontend test inputs but does not manufacture a same-head run receipt after backend changes.
- Mock 1–7 snapshot baselines are in `apps/document-web/e2e/document-workspace.spec.ts-snapshots/`. `docs/superpowers/execution/document-gui-integration-v0-accessibility-review.md` records keyboard/focus, Japanese landmarks, seven sampled contrast pairs, reduced motion, 1280/1440 no-overflow and visual state checks. This remains source-recorded browser review, not a new cloud visual inspection or external WCAG certification.
- Existing production-build advisories remain: main JS 553 KiB, entrypoint 566 KiB, detail chunk 41.9 KiB. No approved numeric T_usable/T_input threshold is invented.
- **Remaining G9 same-head evidence gap:** Plan G9 requires final same-head Standard CI / Sandbox / DSI PoC + frontend E2E. The hosted workflow has no frontend Playwright step, and the repository has no exact-`b578a9b` local frontend execution receipt.
- **Remaining integrated journey evidence gap:** all six committed Playwright tests use the `page.route('**/v1/**')` mock fixture in `apps/document-web/e2e/document-workspace.spec.ts`. The hosted Rust lifecycle proves the backend path separately; the browser suite proves presentation against mocked responses. Neither is a receipt for the approved Plan G9 real PostgreSQL + FileSystemStorage + production DSI/Diff + HTTP + frontend journey. A same-head rerun of the existing mocked suite alone does not close this requirement. No integrated browser/backend run was found in the reviewed repository/workflow evidence.
- Snapshot baselines are `chromium-darwin` artifacts. A Linux rerun must not silently replace or approve new platform-specific baselines. Any required test/harness or baseline changes are outside this C0 docs-only update and need a separately scoped verification task.

- Downstream runtime evidence remains separate: Draft [R2 PR #41](https://github.com/AIrisu-072/knowledge-platform/pull/41) at `bf5ec20d0f27f6e40f53b50ab3b1827c75f3f8ad` has [CI 36943094817](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36943094817) FAILURE; Draft [A2 PR #42](https://github.com/AIrisu-072/knowledge-platform/pull/42) at `e500144f5bafe3bb80ae12314fef7d0c9ca7e170` has [CI 36943178284](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36943178284) FAILURE. Their separate Sandbox/DSI PoC workflows succeeded, but real-runtime acceptance is not green. These later heads do not supply a PR36 exact-head frontend receipt. C1/C2/C3 are not accepted by this C0 record; the scheduler identity decision and historical security-scan qualification remain unresolved.

### C0 docs-only closure boundary / next action

- This reconciliation changes only Active/Status records. It preserves DocumentRevision/OCC/Version separation, comparison semantics, action-capability authorization, identity presentation ownership, Mock 1–7 implementation, and the frozen approved design/plan/amendment records. Historical checkpoints below remain historical evidence.
- Cloud validation is limited to documentation consistency, frozen blob/lock hashes, tracked-file scope and whitespace/repository-policy checks. This refresh did not rerun product verification, browser E2E or heavy builds; prior recorded results retain their original provenance.
- Next exact action: obtain the missing same-head and integrated browser/backend evidence or explicitly scope the verification work needed to create it; do not declare full acceptance from the mocked suite alone. Review this docs-only diff and confirm no concurrent branch writer before publishing the authorized C0 update to PR #36. A docs-only commit still changes PR head: recheck all three hosted gates at the new exact SHA and report their actual results, including pending/failure states. Old run IDs above attest only `b578a9b`.
- PR #36 stays Draft. **No main merge, deploy, production migration execution, or production AD/SSPI connection is authorized.**

---

## Superseded checkpoint — 2026-10-01 JST — G8 COMPLETE / G9 FINAL ACCEPTANCE IN PROGRESS

- 状態: **G0〜G8 COMPLETE / G9 exact-head acceptance IN PROGRESS**。Implementation branch `feat/document-gui-integration-v0`、G8 code commit `d6dcda031e8101a9b46bd688ecc92b68468629ad`、`origin/main`より29 commits ahead。GitHub main `d71753d46590bb4406a1c0b74894ab90a27a6c88`。この記録時点でfeature branch push / product PR / hosted branch CIはなし。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。Design Amendment 01 / approval / Plan Addendum 01のcandidate-specific toolchain/license scope以外にsemantics差分なし。
- G8はMock 1〜7を完成。Document list/folder/context panel/detail、WORKING Versionと正式Revision timeline、新版native file picker/drop、公開/予約公開、Diff Partial/Unknownと原本誘導、AccessPolicyのeffective permissionsとeditable draftを実装。presentation層はtyped generated client/BinaryTransportBridgeを使用。
- Schedule entryでroute transition後にmethodが初期値へ戻るREDをE2Eで確認。publication methodを親workspace状態へ持ち上げ、pending解除後にaction triggerへfocusを戻してGREEN化。PublishはAPI response後のみ成功表示し、JST入力をUTCへ送信。
- Local G8 evidence: `npm test` **11 suites / 34 tests PASS**; `npm run typecheck` PASS; `npm run build` PASS; `npm run e2e` **6/6 PASS**; `node --test tools/api-contract/contract.test.mjs` **12/12 PASS**; `cargo fmt --all -- --check` PASS; `git diff --check` PASS. Mock 1〜7 screenshot baselinesを追加・視覚確認。
- Build residual: Webpack main JS 553 KiB / entrypoint 566 KiB / detail chunk 41.9 KiB。production buildは3 performance advisoriesを出す。Approved designに数値T_usable/T_input上限はないためfinal E2Eで観測値を記録し、閾値は作らない。
- Exact local frontend E2E on head `bb5da30c7a831a9d79a16cc422f00adc89b70c69`: **6/6 PASS**. Captured `T_usable=349.7 ms`, `T_input=16.5 ms`, `motionSpatial=180 ms`; the approved design defines no numeric cutoff.
- Browser-level accessibility checklistを `docs/superpowers/execution/document-gui-integration-v0-accessibility-review.md` に記録。日本語document lang/landmarks/button names、7 contrast token pairs、reduced motion、1280/1440 overflow、keyboard/focus、Mock 1〜7 stateを検査。
- G9 backend lifecycle testを `crates/document-api-http/tests/e2e.rs` で拡張: real PostgreSQL + FileSystemStorage + production DSI/Diff runnersを通るHTTP lifecycleにmetadata minor revision comparison、content major revision comparison、withdraw fallback、restored current Version、revision monotonicityを追加。HostにDocker socketがなくlocal PostgreSQL E2Eは未実行。file Audit 0-byte failure / fail-soft identity / stale capability raceの既存focused testsはStandard CIで再確認する。
- Design/Plan境界: implementation branch pushとDraft PR作成は許可範囲。merge、deploy、production migration execution、本番AD/SSPI接続は禁止。
- Blocker: hosted exact-head matrix pending。Next exact action: commit this evidence update、push implementation branch、create Draft PRし、Standard CI / DSI Sandbox Preflight / DSI PoCの同一PR head結果を記録する。

---

## Superseded checkpoint — 2026-10-01 JST — G7 COMPLETE / G8 Mock 1–7 IN PROGRESS

- 状態: **G0〜G7 COMPLETE / G8 Mock 1–7 IN PROGRESS / G9 NOT STARTED**。G7 RED commit `182215f`; GREEN code head `a6e340f`。依頼者は推奨されたVite/Vitest置換を承認し、候補graphに含まれる列挙外licenseのみを個別承認した。一般license policyおよびFrozen DesignのUI/API semanticsは変更しない。
- Design Amendment 01 / Approval / Plan Addendum 01を作成。Feature-scoped G7 selection: Webpack `5.111.1`, webpack-cli `7.2.3`, webpack-dev-server `6.0.0`, Jest/babel-jest `30.5.2`, Babel `7.29.7`, TypeScript `6.0.3`。
- Candidate lock SHA-256 `ee2e1430204112a91a31cbfa34a286ab1effca56ac35d918bb3f4df77d05ea16`に固定した個別license approval: ISC (34), BlueOak-1.0.0 (8), CC-BY-4.0, Python-2.0, MIT-0, Unlicense, CC0-1.0, 0BSD, `(MIT OR CC0-1.0)`. 新license/graphは承認外。明示除外licenseはcandidate graphにない。
- GitHub current state: main `d71753d46590bb4406a1c0b74894ab90a27a6c88`; local branch `feat/document-gui-integration-v0` at `a6e340f`, 26 commits ahead before this status update. Remote feature branch / product PR / exact-head CIはなし。main CI `36718016267` SUCCESS。PR #27/#29/#30/#31/#32/#35 MERGED。
- Candidate qualification before implementation: Node 24.21.0 / pnpm 12.4.1 frozen install PASS; peer check PASS; low-threshold audit PASS; React Aria focused PoC 6/6 PASS。現在のshellはNode 26.3.1、pinned pnpm shimは起動失敗。アプリlocal binariesは利用可能。
- G7 RED commit `182215f`: 5 expected missing-foundation tests FAIL / 8 PASS; React Aria suite PASS.
- G7 GREEN at `a6e340f`: Jest 14/14 PASS; TypeScript check PASS; Webpack production build PASS with performance warnings for a 292 KiB entrypoint (JS 289 KiB); dev server compile PASS and `/`, `/documents` served the app shell; diff check PASS. Hosted CI is deferred to G9.
- Candidate package/lock, amendment records, and G7 foundation are committed. `.superpowers` SDD ledger remains local ignored context.
- 次のexact action: inspect the approved Source Design screens and generated client operations; implement Mock 1–7 using the typed client / BinaryTransportBridge.

---

## 2026-10-01 JST — G7 candidate license approval STOP

- 状態: **G0〜G6 COMPLETE / G7 candidate qualification STOP / G8〜G9 NOT STARTED**。Vite置換と既存license policy維持の選択を受け、Webpack/Jest候補を検査した。Architecture Contract §5は掲載外licenseを個別承認としている。
- Branch `feat/document-gui-integration-v0`、local HEAD `82ca1337c71d7ef5b2c3179b59a8e08941411de0`、`origin/main`より24 commits ahead。GitHub main `d71753d46590bb4406a1c0b74894ab90a27a6c88`、CI `36718016267` SUCCESS。GitHub上にimplementation branch/PRはなく、branch exact-head CIも未実行。PR #27/#29/#30/#31/#32/#35はMERGED。
- Last GREEN code head `3b3d2b737942c0fd4eb27abb3777395507dcf0e2`（G6）。Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。
- 候補: Webpack `5.111.1` / webpack-cli `7.2.3` / webpack-dev-server `6.0.0`; Jest `30.5.2`; Babel `7.29.7`; Node `24.21.0`; TypeScript `6.0.3`; React/TanStack/Motion/Ajv/RTL/Playwright。Clean lock再生成後、Vite/Vitest/LightningCSSは依存treeにない。Babel 8のpeer mismatchを受けBabel 7へ変更。任意の`eslint-plugin-jsx-a11y`が引く`axe-core@4.13.0` MPL-2.0と、`identity-obj-proxy`のdual MPL licenseは、それぞれ補助依存を除去して候補graphから外した。
- focused evidence: Node `24.21.0` / pnpm `12.4.1`でworkspace filtered frozen install PASS、isolated candidate install PASS、`pnpm peers check` PASS、`pnpm audit --audit-level=low` PASS（known advisoriesなし）。
- license inventoryには明示除外のGPL/AGPL/LGPL/MPL/SSPL/BSL/source-availableはない。一方、non-listed licenseが含まれ個別承認待ち: ISC (34 packages)、BlueOak-1.0.0 (8)、CC-BY-4.0 (`caniuse-lite`)、Python-2.0 (`argparse`)、MIT-0、Unlicense、CC0-1.0、0BSD、`(MIT OR CC0-1.0)`。Architecture Contractの現行列挙licenseはApache-2.0 / MIT / BSD-2-Clause / BSD-3-Clause / PostgreSQL License / Public Domain。これらのcandidate-specific approvalは未取得。
- Candidate manifest/lockとG7 contract/config/test filesはlocal uncommitted。G7 production UI code、Design/Plan Amendment、dependency promotion、product push/PR/hosted CIは未実施。
- blocker: license policyのindividual approval。次のexact action: listed non-allowlisted license IDsについてこのcandidateだけの個別承認を受けるか、現行列挙licenseだけに限定して別tool stackを評価するか決定を得る。その後G7を続行。

---

## Superseded checkpoint — 2026-10-01 JST — G7 STOP / dependency license decision required

- 状態: **G0〜G6 COMPLETE / G7 STOP / G8〜G9 NOT STARTED**。Frozen Design差分・amendmentなし。添付依頼のSTOP条件「selected dependencyがlicense/security policy不適合」に該当。
- Branch `feat/document-gui-integration-v0`、local HEAD `82ca1337c71d7ef5b2c3179b59a8e08941411de0`、`origin/main`より24 commits ahead。GitHub main `d71753d46590bb4406a1c0b74894ab90a27a6c88`、CI `36718016267` SUCCESS。branch remoteなし、product PRなし。PR #27/#29/#30/#31/#32/#35はMERGED。
- Last GREEN code head `3b3d2b737942c0fd4eb27abb3777395507dcf0e2`（G6）。Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。
- Blocker evidence: the current G7 candidate uses Vite 8.3.1. Registry metadata reports Vite 8.3.1 depends on `lightningcss ^1.33.0`; `pnpm-lock.yaml` resolves `vite@8.3.1 -> lightningcss@1.33.0`; npm registry reports `lightningcss@1.33.0` license `MPL-2.0`. Architecture contract permits Apache-2.0/MIT/BSD-2/BSD-3/PostgreSQL/Public Domain and excludes MPL-2.0; LINT-02 requires no unapproved dependency license in the tree. `pnpm why lightningcss --recursive` could not run because the pinned pnpm 12.4.1 CLI is missing/broken, but lock snapshot and registry metadata establish the conflict.
- G7 focused PoC: React Aria qualification tests 6/6 PASS on Vitest 4.1.11. `pnpm audit --audit-level=low` previously passed with no known advisories. React Aria has not been promoted to a production dependency. Local `apps/document-web` contains only manifest/config/qualification tests; no production UI source. Design-system RED reports missing `src/design-system/tokens.css`; this is expected while implementation has not started. Other local contract tests remain unrun.
- Working tree changes are local and uncommitted: `.gitignore`, `pnpm-lock.yaml`, `apps/document-web` package/config/tests and `.superpowers` checkpoints. No product implementation PR or hosted product CI exists.
- Required decision: (A) keep license policy unchanged and approve a Design Amendment replacing Vite 8 with a build tool whose full dependency graph qualifies; then rerun focused G7 qualification; or (B) formally amend license policy to allow this MPL-2.0 transitive dependency and proceed with Vite 8. No exception or substitution is self-approved.
- 次のexact action: wait for the user’s A/B decision, record the approved design/policy amendment, then resume G7. G8/G9, product PR creation, push, merge, deployment, production migration, and AD/SSPI connection remain unstarted/prohibited as specified.

---

## 2026-10-01 JST — G6 COMPLETE / G7 NEXT

- 状態: **G0〜G6 COMPLETE。次はG7 Frontend foundation / Operational Design System。** Frozen Design / approved Planに意味変更なし、Design amendmentなし。
- Implementation branch `feat/document-gui-integration-v0`。G6 contract RED commit `405ac30ac379423cbd9c055168c0a35232a5357a`、Binary Bridge RED commit `5c4324076ce2abb6285ce4cfefc8d966da7c756f`、GREEN commit/head `3b3d2b737942c0fd4eb27abb3777395507dcf0e2`。Product branchはlocal only、Draft product PR未作成。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。PR #27/#29/#30/#31/#32/#35 MERGED、main `d71753d46590bb4406a1c0b74894ab90a27a6c88`、main CI `36718016267` SUCCESS。
- G6実装: OpenAPI 3.2.1 / JSON Schema 2020-12でG1〜G5 contractとexamplesを揃えた。`@hey-api/openapi-ts` 0.99.0、TypeScript 6.0.3をexact-pinし、4 generated files / 34 operationsを追加。TS7 candidateはgenerator startupで`ts.SyntaxKind.AnyKeyword` incompatibilityとなったため、Design所定のTS6 fallbackを選択。`type-contracts.ts`でprojection/fragment/verdict unionsとnullabilityを固定。BinaryTransportBridgeはcreate document、create/update version、manifest item+renditionのpart mapping、Blob/ReadableStream download、RFC 9457 Problem normalizationを提供し、request/response DTOはgenerated typesを使用。
- G6 qualification: contract REDではrevision projection/display paging/union/nullability欠落を検出。Binary REDではbridge未実装を検出。`node --test tools/api-contract/contract.test.mjs` 12/12 PASS、Redocly 2.52.1 lint (全request/response examples schema validation) PASS、TS6 typecheck PASS、client/binary/generated-operation tests 6/6 PASS、34 OpenAPI operationIdとgenerated SDK operation set一致。generator再実行後の全4生成ファイルhashが一致。`pnpm audit --audit-level=low` 既知脆弱性0件。`js-yaml 4.3.2` overrideをworkspaceへ置き、candidateのlicense inventoryはPoC時にpermissive-onlyで確認。Hosted CIはユーザー方針どおりTask単位で実行せず、G9 final exact-head gateに集約。
- G6で追加したファイル: `packages/document-api-client/`（generated SDK/types、Bridge、compile-time qualification、focused tests）。Root scriptsはhostのpnpm shimに依存しないTypeScript/Node呼び出しとした。
- Blocker: なし。Product branch/PRは未push/未作成。product PR merge、deploy、本番migration実行、AD/SSPI接続なし。直近disk空き約855 MiB、G9前に再確認してPostgres suitesはserial実行する。
- 次のexact action: G7でReact 19 / Vite 8 / TanStack stack / Motion / Ajv / testing stackの現行公式package versionとReact Aria Componentsをfocused qualifyし、foundation/token/architecture testsを追加する。React Ariaのkeyboard/a11y/composition/performance/license/securityに問題があればBase UI比較へSTOPする。

## 2026-10-01 JST — G5 COMPLETE / G6 NEXT

- 状態: **G0〜G5 COMPLETE。次はG6 OpenAPI 3.2 / typed client / Binary Bridge。** Frozen Design / approved Planに意味変更なし、Design amendmentなし。
- Implementation branch `feat/document-gui-integration-v0`。G5 RED test-only commit `d2c563b7941e84af10161b18e55f549a09290cd4`、GREEN code head `c12e81943098862ab0dec75c6b1bed9b048ed1f0`。このcheckpoint docs commit後のexact headをActive Pointerに記録する。Product branchはlocal only、Draft product PR未作成。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。PR #27/#29/#30/#31/#32/#35 MERGED。GitHub main `d71753d46590bb4406a1c0b74894ab90a27a6c88`、main CI `36718016267` SUCCESS。
- G5 RED: `d2c563b7941e84af10161b18e55f549a09290cd4` added revision metadata/content comparison contracts before the service existed. GREEN: `c12e81943098862ab0dec75c6b1bed9b048ed1f0`.
- G5実装: Revision metadata snapshotをRFC 6901 path単位で比較し`same`/`different`/`unavailableLegacy`を区別。同じDocumentVersion pairではcontent diffを実行せず、異なるpairは既存DocumentDiffServiceを再利用。`POST /revision-comparisons`とcomparison display projectionを追加。TXT/CSV/HTML fragmentをauthoritative file bytes/source locatorからsandbox workerで生成し、Office/PDF previewやFrontend parserは追加していない。semantic verdict/coverageを保持し、current authorization、file/result/display audit correlation、no persistent display cache/no fragment auditを維持。
- G5 bounds: pageSize default 50/max 100、fragment 16 KiB、item 32 KiB、JSON page 1 MiB。unverified regionもcursorで順序づけて返す。**Ruling:** oversized unverified detailを落とさずpage capを守るため、cursor sequenceをchanged itemsの後にunverified regionsへ続ける。cost if wrong: 初回pageに全regionを期待するclientは`nextCursor`を追う必要がある。これは表示/ページングだけの実装判断で、Diff verdict/coverageとFrozen Design semanticsは変更しない。
- G5 local verification: `cargo test -p document-application --test revision_comparison -- --test-threads=1` 3/3、`cargo test -p document-api-http --test diff_http -- --test-threads=1` 12/12、`cargo test -p document-diff-worker --test display_projection -- --test-threads=1` 4/4、HTTP display unit 2/2、Application 32 KiB boundary 1/1。`cargo clippy -p document-api-http -p document-application -p document-diff-worker --all-targets --no-deps -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` PASS。中間hosted CIはユーザー方針どおり未実行し、G9へ集約。広い依存lintは既存Postgres capability/revision helper警告で失敗したため、対象crate限定で再実行してPASS。
- Disk空き約1.1 GiBを観測。G9前に再確認し、Postgres suitesはserialで実行する。Blockerなし。Product branch/PRは未push/未作成。Product PR merge、deploy、本番migration実行、AD/SSPI接続なし。
- 次のexact action: G6でG1〜G5のOpenAPI 3.2.1 request/response、display union、paging、nullability、examplesをAPI実装へ一致させ、schema/examples testsを追加。その後`@hey-api/openapi-ts`をexact APIでPoCし、3.2.1 retention/operations/unions/nullability/determinism/TS compile/license/securityを判定する。

---

## Superseded checkpoint — 2026-10-01 JST G3 COMPLETE / G4 NEXT

- 状態: **G0〜G3 COMPLETE。次はG4 Identity Presentation / session。** Frozen Design / approved Planに意味変更なし。
- Implementation branch `feat/document-gui-integration-v0`。G3 GREEN code head `ba199191c91f11091c5fa7df87af57f441074a11`。Product branch未push、Draft product PR未作成。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。Design amendmentなし。
- G3 initial test-only contract commit `0a675ac94f00e2c7a894eabd0de721d73e6cba1a`。このsessionで観測したsupplemental REDはcurrent Published Versionなしの`endPublication`が`lifecycle`ではなく`notCurrent`であるべき差分。GREEN commit `ba199191c91f11091c5fa7df87af57f441074a11`。
- G3実装: Document/Version detailおよびFolder readへtyped Action Capability Projectionを追加。各操作を現在権限、lifecycle、pending schedule、stale base、human-interactive contextから評価。root folderのrename/moveはunsupported。listには重いcapability計算を追加しない。CapabilityはUI hintで、Mutationは従来どおりサーバ側で再認可。古いavailable表示の後にpolicy revokeまたはschedule reservationが起きたMutation拒否を検証。
- Reason mapping: `endPublication`はcurrent Publishedがない場合`notCurrent`、T10終了後は`lifecycle`。high-cost publication quality/DSI preflightはMutation時に実行することをOpenAPI説明へ明記。これは既承認reason集合内の割当で、設計意味変更ではない。
- Local verification: `cargo test -p document-api-http --test read_http -- --test-threads=1` 10/10 PASS、`cargo test -p document-api-http --test management_http -- --test-threads=1` 3/3 PASS、`node --test tools/api-contract/contract.test.mjs` 11/11 PASS、Redocly CLI 2.52.1 lint PASS、`cargo fmt --all -- --check` PASS、`git diff --check` PASS。Node/OpenAPI CLI実行のため一時導入した`node_modules`は削除。Cargo/pnpm lock、dependency、migration変更なし。
- 初回の並列read_http suiteはPostgreSQL testcontainer同時起動でdisk fullとなり8件が開始時に失敗した。直列再実行は10/10 PASS。Disk空きは823 MiB観測、Docker `system df`は既存container snapshot欠落でエラー。コード失敗ではないがG9実DB試験前に容量を再確認する。Postgresを使うfocused suitesは直列にする。
- Current GitHub: main `d71753d46590bb4406a1c0b74894ab90a27a6c88`、PR #27/#29/#30/#31/#32/#35 MERGED、main CI `36718016267` SUCCESS。Product branch/PRは未push/未作成。中間hosted CIなし、G9のexact-head gateで実行予定。
- Blocker: なし。product PRはmergeせずDraftで終了。deploy、本番migration実行、AD/SSPI接続は禁止。
- 次のexact action: G4のIdentityPresentationResolver port、History/AccessPolicy DTO、認証adapterを確認し、batch enrichment/fail-soft subjectId fallback/`GET /v1/session`をREDから実装する。Production AD接続やallow-all identityを追加しない。

---

## Superseded checkpoint — 2026-10-01 JST G2 COMPLETE / G3 NEXT

- 状態: **G0〜G2 COMPLETE。次はG3 Action Capability Projection。** Frozen Design / approved Planに意味変更なし。
- Implementation branch `feat/document-gui-integration-v0`。G2 GREEN code head `6d58cef3a119424d005018cb412bad183da624d2`。Draft product PRは未作成、main未merge、deployなし。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。
- G2 RED: `bb0a26352cf03cee3196de6799c3de5fa65a2273` でlist projection欠落とrevision route 404、`aaf94d2e5720af95fabdc3be50011499ae033c91` で既存Version responseの`updatedAt`欠落を確認。GREEN: `6d58cef3a119424d005018cb412bad183da624d2`。
- 実装: `document_versions.updated_at`はcontent/lifecycle/schedule projection変更で維持。GUI list/Version APIは同一bounded SQL projectionでVersion/file summary、最新Revision、readState、display timestampを取得。Human Revision list/detailはcurrent Read+ReadHistory認可、principal/access revision/query-bound keyset cursor、metadata snapshot/actor/reasonを返す。OpenAPI例とoperation/evidence contractを更新。Search Extraction、Document Diff、Frontend parserを結合していない。
- Verification: HTTP read integration 7/7、Revision cursor contract 4/4、router dispatch 1/1、error registry 1/1、API Node contract 10/10、Redocly OpenAPI 3.2.1 lint PASS、`cargo fmt --all -- --check`と`git diff --check` PASS。Migration 0010の`document_version_updated_at` PostgreSQL focused testは前工程でPASS。
- `mise run api:check`はuntrusted configで拒否。pinned pnpm 12.4.1 shim欠落のため、同taskの`redocly lint`および`node --test tools/api-contract/contract.test.mjs`を直接実行しPASS。中間hosted CIは省略し、G9のexact-head gateへ集約。
- Blocker: なし。次のexact action: G3の設計記述と既存detail routeを調査し、Document/Version/Folderのcapability contract REDを追加する。各capabilityは`available | disabled(reason)`、列挙reasonにpermission/lifecycle/pendingSchedule/staleBase/notCurrent/notHumanInteractive/unsupportedを含める。mutationは常に権限・状態を再評価する。

---

## 2026-09-30 JST — G1 COMPLETE / G2 NEXT

- 状態: **G0〜G1 COMPLETE。G2 GUI Read Model / Revision APIs / file summaryへ着手。**
- 基準main / G1 base: `d71753d46590bb4406a1c0b74894ab90a27a6c88`。Implementation branch `feat/document-gui-integration-v0`、local GREEN head `069fcf23ee19c9592e15499aea1d2ddda6448512`。Product PRはまだ作成していない。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、approval recordあり。Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。意味変更なし。
- G0 predecessor merge commits: #27 `95f60f02fbc4205bfc38b6097d419fadee9682a1`; #29 `2ebfbd46f80c65590950d35d7ef9534377a72035`; #30 `2a49a2ddc28a77fba286d5d70d17464fcf4949a1`; #31 `6240ebbebb0db45a7360efbf568d63a2a6101db3`; #32 `5a81fd856d81b557e4936f663aa8b0ab3fcaa5e2`; #35 `d71753d46590bb4406a1c0b74894ab90a27a6c88`。
- G0 evidence: PR #32 exact head `04ccb84a6d9a99f63eca8d7512888225393058fa` passed CI `36713044816`, Sandbox `36713044612`, DSI PoC `36713044474`; main CI `36714907650` SUCCESS. PR #35 exact head `4b9df28d054526114dd8a48956d5ac4ddecd5f46` passed CI `36716092331`, Sandbox `36716092340`, DSI PoC `36716092268`; merge/main CI `36718016267` SUCCESS.
- G1 RED commits: schema contract `1fd1b14dce6f4674f7757861972e9a042a652252`; publication/metadata/withdrawal issuance test delta `a48f4c2566568d1eb9fc02baf034dbabf864dc09`. Expected failures were absent revision relation/rows; transaction tests reached missing-row assertions rather than unrelated policy or formatting failures. GREEN implementation commit: `069fcf23ee19c9592e15499aea1d2ddda6448512`.
- G1 implementation: migration 0009 creates append-only `document_revisions`, preserves deterministic published ordering, marks unrecoverable legacy snapshots unavailable, excludes WORKING-only documents, and supports rollback/rerun. Domain types validate revision number, T5 snapshot, status/source/provenance. Initial/content publish, metadata change, and eligible withdrawal fallback insert a revision atomically with existing operation/OCC/outbox/audit transactions.
- Verification on the exact G1 tree: `cargo fmt --all -- --check` PASS; `cargo test -p document-domain --lib` 24/24 PASS; focused Postgres tests 28/28 PASS across schema, legacy backfill, initial/next publication, replay, metadata no-op/non-revision operations, T10, withdrawal fallback/no fallback, and concurrent publish. Intermediate hosted CI was intentionally deferred; G9 owns same-head CI/Sandbox/DSI PoC and frontend E2E.
- Blocker / STOP: なし。`toolbox-context parent restore` launcherは `~/.local/bin/parent-context.py` 欠落で失敗したが、repository stateを優先して継続。直接スクリプトのstatusには候補runなし。
- 次のexact action: G2で既存HTTP/API・Application list/history projectionを調べ、`document_versions.updated_at`、single-query/bounded file summary、GUI displayVersion/displayRevision、revision list/detailのfocused REDを作る。cursor binding/current authorization/T10 history semanticsを保持し、G2の契約をGREENにする。

## Superseded checkpoint — G0 COMPLETE / G1 schema RED

## Superseded checkpoint — 2026-09-30 JST G0 predecessor integration / PR #35 finalization in progress

- 状態: **G0 predecessor PR #27/#29/#30/#31/#32 MERGED。PR #35はmainへretarget済み。PR #35のexecution docs reconciliationとexact-head gatesが残る。G1〜G9は未開始。**
- Frozen Design blob: f132910ca5d3e638502f0b38447d9a1ec4020f24。承認済みProduction Plan blob: 0830c306ebb38290e4c3dc277f6c97a0759cf912。Plan Approval recordにG0〜G9の明示承認と禁止境界を記録済み。
- Source Design ZIP SHA-256: ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86。
- G0 merge commits: #27 95f60f02fbc4205bfc38b6097d419fadee9682a1; #29 2ebfbd46f80c65590950d35d7ef9534377a72035; #30 2a49a2ddc28a77fba286d5d70d17464fcf4949a1; #31 6240ebbebb0db45a7360efbf568d63a2a6101db3; #32 5a81fd856d81b557e4936f663aa8b0ab3fcaa5e2.
- PR #32 head 04ccb84a6d9a99f63eca8d7512888225393058fa: Standard CI 36713044816, Sandbox 36713044612, DSI PoC 36713044474 SUCCESS. Merge commit tree bd2b7df1717503ff3ef937ede581e0b235f20e87 matches that exact PR head. Main push CI 36714907650 is in progress.
- PR #35 is open/Draft, base main, no unresolved review threads. Branch merge with latest main found only an active.md content conflict; it is being reconciled. The final PR head and three gates are pending.
- Frozen design差分なし。product implementation branchはまだ作成していない。Product PRのmain merge、production deploy、production migration execution、本番AD/SSPI接続は禁止。
- 次のexact action: main CI 36714907650完了を確認し、PR #35のActive/Status/HTTP closure記録を反映してpush。新しいPR #35 headのStandard CI/Sandbox/DSI PoCを確認し、全てSUCCESSなら#35をmergeしてmain push CIを確認。最新mainからproduct branchを作りG1を開始する。

## Approved scope and immutable decisions

- DocumentVersion.version_no、human-facing DocumentRevision Major.Minor、Document.revision OCCは別概念のまま維持する。
- Frozen Design、承認済みProduction Plan、Source Design checksumは上記blob/hashのまま。amendment gateは未使用。
- Plan boundaries: G0のpredecessor/Design-Plan PR mergeとDraft product PR作成は許可。product PRのmain merge、production deploy/migration実行、本番AD/SSPI接続は不可。
