# Document GUI Integration v0 — Implementation Session Handoff

対象repository: **`AIrisu-072/knowledge-platform`**

このsessionでは、承認済み **Document GUI Integration v0** のProduction Implementationを実行してください。

## 1. Source of Truth

会話履歴や記憶から進捗を再構成せず、GitHub/repositoryの現在状態を正本として扱ってください。

最初に必ず以下を順に確認してください。

1. `AGENTS.md`
2. `docs/superpowers/execution/active.md`
3. `docs/superpowers/execution/document-gui-integration-v0-status.md`
4. `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design.md`
5. `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design-approval.md`
6. `docs/superpowers/plans/2026-09-30-document-gui-integration-v0-production-implementation.md`
7. `docs/superpowers/plans/2026-09-30-document-gui-integration-v0-production-implementation-approval.md`
8. PR #35、PR #27/#29/#30/#31/#32、最新main、exact-head CI、Cargo/pnpm lock、migration番号、architecture rules

期待するFrozen Design blob:

`f132910ca5d3e638502f0b38447d9a1ec4020f24`

Production Implementation Plan blobはapproval record記載値を必ず照合してください。

## 2. Mandatory approval gate

以下のどれかが成立しない場合は**product implementationを開始しない**でください。

- Design Approvalが存在し、Design blobが `f132910ca5d3e638502f0b38447d9a1ec4020f24` と一致
- Production Implementation Plan Approvalが存在
- Approvalのplan blobが実際のPlan blobと一致
- Approval記録が実装開始を許可している

不足時はGitHub current stateと不足approvalだけを報告して停止してください。

## 3. Scope

承認済みPlanの **G0〜G9を依存順に最後まで実行**してください。

- G0 predecessor closure
- G1 DocumentRevision
- G2 GUI Read Model
- G3 Action Capability
- G4 Identity Presentation
- G5 Revision Comparison / Diff Display
- G6 OpenAPI / typed client / Binary Bridge
- G7 Frontend foundation
- G8 Mock 1〜7 production GUI
- G9 cross-cutting acceptance

## 4. Frozen invariants

- `DocumentVersion.version_no`、`DocumentRevision major.minor`、`Document.revision`は別物。
- display MajorはDocument単位で単調増加し、Withdraw fallbackでも逆行させない。
- WORKING Versionを正式Revisionとして扱わない。
- T5 Document metadata実変更だけMinor+1。Folder/ACL/ReadState/schedule/T10単独では改訂番号を増やさない。
- historical metadataを推測・捏造しない。
- capability projectionはUI hint。mutationで必ず再authorization。
- Identity display nameをDocument DB正本へコピーしない。
- FrontendでOffice/PDF parserを持たない。
- Diff displayはauthoritative bytes + source locatorから認可/監査付きbounded projection。
- Partial/UnknownをSame/Fullへ変換しない。
- OpenAPI 3.2.1をdowngradeしない。
- Presentationからraw fetch/API URL/business invariantを排除。
- Query server stateを別global storeへコピーしない。
- Motion completionをbusiness completionへ使わない。
- production allow-all Identity禁止。

## 5. Branch / PR strategy

実装開始時のcurrent GitHub stateを見てPlan G0を実施してください。

- predecessor #27/#29/#30/#31/#32はPlan G0の範囲で順序どおりmainへ統合する。
- GUI Design PR #35は承認済みDesign/Plan docsをmainへ統合するための設計PRとして扱う。predecessor merge後にmainへretargetし、exact-head gate成功後にPlan G0の範囲でmergeする。
- product implementationは**PR #35まで統合された最新main**から新しいfeature branchを作る。
- mainへ直接pushしない。
- product implementationはUnit A/B/C/Dでstacked Draft PRに分けてよい。
- commit/push/Draft PR作成・更新は実装範囲。
- **product implementation PRのmerge、本番deploy、本番AD/SSPI接続は行わない。**

## 6. Verification

- Taskごとにfocused RED→GREEN。
- hosted CIはDelivery Unitの最終headを中心に実行し乱発しない。
- migration/concurrency/replay/authorizationは実PostgreSQLで検証。
- GUIはVitest/RTL/Playwrightでfunctional/keyboard/focus/error/motionを検証。
- WCAG 2.2 AAは自動検査だけで完了宣言しない。
- visual regressionは重要情報の破壊検出を目的とする。
- final same headでStandard CI、DSI Sandbox Preflight、DSI PoC、frontend E2Eを確認。
- pending/cancelled/failedをSUCCESS扱いしない。

## 7. Source Design

Reviewed visual artifact SHA-256:

`ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`

利用可能ならhashを照合して参照してください。Artifactがsession内に存在しなくても、repositoryのFrozen Design / UX requirementsを正本として実装を進め、画像を推測で再発明しないでください。Source Designの意味はDesignに記録済みです。

## 8. STOP

Frozen Design / Production PlanのSTOP conditionsに該当したら、その場で推測実装を止め、statusへ:

- blocker
- evidence
- last GREEN head
- required decision
- next exact action

を記録してください。

## 9. Completion target

STOP条件がない限りG0〜G9を完了まで進めてください。

最終報告:
1. 完了G Task / Unit
2. PR一覧
3. final exact head
4. migration/revision semantics evidence
5. GUI/API/OpenAPI/client qualification
6. frontend library qualification
7. Mock 1〜7 production coverage
8. keyboard/WCAG/motion/visual/performance evidence
9. local/hosted CI
10. remaining blockers/evidence limits
11. merge/deploy/AD接続を行っていないこと

**Product implementation PRはmergeせずreview-readyで終了してください。**
