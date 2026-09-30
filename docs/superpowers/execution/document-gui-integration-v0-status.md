# Document GUI Integration v0 — Capability Execution Status

## 2026-09-30 JST — G0 COMPLETE / G1 schema RED

- 状態: **G0 COMPLETE。G1はtest-only schema contract REDを確認し、migration/domain/transaction実装前。**
- Frozen Design blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`、承認済みProduction Plan blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`。Plan Approval recordあり。Source Design ZIP SHA-256 `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86` と添付ZIPが一致。
- G0 merge commits: #27 `95f60f02fbc4205bfc38b6097d419fadee9682a1`; #29 `2ebfbd46f80c65590950d35d7ef9534377a72035`; #30 `2a49a2ddc28a77fba286d5d70d17464fcf4949a1`; #31 `6240ebbebb0db45a7360efbf568d63a2a6101db3`; #32 `5a81fd856d81b557e4936f663aa8b0ab3fcaa5e2`; #35 `d71753d46590bb4406a1c0b74894ab90a27a6c88`。
- PR #32 exact head `04ccb84a6d9a99f63eca8d7512888225393058fa`: Standard CI `36713044816`、Sandbox `36713044612`、DSI PoC `36713044474` SUCCESS。Main push CI `36714907650` SUCCESS。
- PR #35 exact head `4b9df28d054526114dd8a48956d5ac4ddecd5f46`: Standard CI `36716092331`、Sandbox `36716092340`、DSI PoC `36716092268` SUCCESS。PR head treeとmerge treeは `0d5e1b473c9c0324b55d1171396a17d65e2a925c` で一致。Main push CI `36718016267` SUCCESS。HTTP predecessor statusはCLOSED / MERGED / NOT DEPLOYED。
- Product branch `feat/document-gui-integration-v0` はlatest main `d71753d46590bb4406a1c0b74894ab90a27a6c88` から作成済み。Product PR未作成。RED: `document_revision_relation_is_installed_by_migration` と `document_revision_schema_enforces_identity_snapshot_and_append_only_rules` がtable不在だけを理由にFAIL。既存versioning schema testと `cargo fmt --all -- --check` はPASS。test-only変更と本Status記録は未commit。
- Revision/OCC/Versionの分離、legacy metadata非捏造、Frozen Design / Plan / ZIP hashは維持。Design amendmentなし。Product PRはDraft/unmerged、production deploy / production DB migration実行 / production AD・SSPI接続は禁止。
- Blocker / STOP: なし。
- 次のexact action: test-only REDとG0 closeout記録をcommitし、`0009_document_revisions_v0.sql` schema GREENを得る。続けてlegacy fixture backfillをRED/GREENにし、publish/metadata/withdraw transactionへ進む。

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
