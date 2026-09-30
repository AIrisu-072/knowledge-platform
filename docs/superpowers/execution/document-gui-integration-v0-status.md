# Document GUI Integration v0 — Capability Execution Status

## 2026-09-30 JST — G0 predecessor integration complete / PR #35 finalization in progress

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
