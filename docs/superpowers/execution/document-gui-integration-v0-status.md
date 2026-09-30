# Document GUI Integration v0 — Capability Execution Status

## 2026-10-01 JST — G4 COMPLETE / G5 NEXT

- 状態: **G0〜G4 COMPLETE。次はG5 Revision comparison / Diff Display Projection。** Frozen Design / approved Planに意味変更なし。
- Implementation branch `feat/document-gui-integration-v0`、exact head `27c069c657679676d5c8c51b3f206ad12234bb0e`。G4 RED test-only commit `b9b6e3be02d55cd8c3fc2177f7520f8b91fa2909`、GREEN code commit `27c069c657679676d5c8c51b3f206ad12234bb0e`。Product branch未push、Draft product PR未作成。
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、approved Production Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`、Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。PR #27/#29/#30/#31/#32/#35 MERGED。GitHub main `d71753d46590bb4406a1c0b74894ab90a27a6c88`、main CI `36718016267` SUCCESS。
- G4 RED: committed tests `b9b6e3be02d55cd8c3fc2177f7520f8b91fa2909` failed to compile because the Application presentation types and HTTP session route did not exist. GREEN: `27c069c657679676d5c8c51b3f206ad12234bb0e`.
- G4実装: `IdentityPresentationResolver` Application portを追加し、request refsをdeduplicateして1 batch callで解決。resolver unavailable、response omission、notFoundでは`IdentityRef`のprovider/kind/subjectIdを保ち、displayName/secondaryTextをnullにして`resolution`を返す。History actorとAccessPolicy grantsをinline enrichment。`GET /v1/session`はverified principal、presentation、invocationKind、expiresAtを返す。spoof header/queryを無視すること、resolver unavailableでもsession/History/Policy readが成功することを確認。Identity display nameはDBに保存せず、production AD/SSPI接続やallow-all authは追加していない。OpenAPI 3.2.1を維持し、PolicyGrant read projectionとPolicyGrantInputを分離。
- G4 local verification: `cargo test -p document-application --test identity_presentation` 2/2 PASS、`cargo test -p document-api-http --test session_http` 2/2 PASS、`cargo test -p document-api-http --test read_http identity -- --test-threads=1` 2/2 PASS。`node --test tools/api-contract/contract.test.mjs` 11/11 PASS、Redocly CLI 2.52.1 `lint spec/api/openapi.yaml` PASS、`cargo fmt --all -- --check` / `git diff --check` PASS。中間hosted CIなし。Dependencies / migrations / Cargo/pnpm lockは変更なし。
- Docker focused PostgreSQL testsは成功。`docker system df`は古いcontainer snapshot欠落を報告し、disk空きは約771 MiB。Postgres test suitesはserialで実行し、G9前に容量を再確認。
- Blockerなし。Product branch未push/PR未作成。Product PR merge、deploy、本番migration実行、AD/SSPI接続なし。
- 次のexact action: Frozen Design / Plan G5と既存`DocumentDiffService`、HTTP comparison handler、revision metadata snapshot read modelを確認し、same-version metadata-only comparisonとbounded display projectionをREDから実装する。

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
