# Document GUI Integration v0 — Capability Execution Status

## 2026-09-30 JST — Written Design approved / Production Plan review pending

- 状態: **DESIGN APPROVED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**。
- branch: `design/document-gui-integration-v0`。基点はDocument HTTP/OpenAPI Transport v0 final accepted head `3f870a92525afb6741e1ee72ee6c932eac0f0511`。
- Design: `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design.md`。
- Frozen Design blob: `f132910ca5d3e638502f0b38447d9a1ec4020f24`。依頼者が2026-09-30に明示承認。承認記録 `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design-approval.md`。
- Human GUI Source Design artifact SHA-256: `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`。Mock 1〜7 / Design System / Motion / Keyboard / Error states / GUI API gapsをレビュー済み。ZIP自体はrepository normative SSOTではない。
- 主要設計決定: `DocumentVersion.version_no`（内容世代）、`DocumentRevision Major.Minor`（人間向け改訂）、`Document.revision`（OCC）を分離。表示MajorはVersion番号と直結せずDocument単位で単調増加し、Withdraw fallbackでも逆行させない。
- metadata改訂対象はT5の `document_type / owning_department / category / extensions`。実変更だけMinor+1。Folder/ACL/ReadState/schedule/T10単独では表示Revisionを増やさない。
- GUI gapはRead Model、Action Capability、Identity Presentation、Revision/Diff Display、Typed Client/Binary Bridgeの5境界として統合。Diff表示はauthoritative bytes + source locatorから監査付きbounded projectionとして生成し、既存comparisonへ`projection=display`、正式Revision比較は専用`revision-comparisons` endpointとする。
- Frontend方針: React 19 / Vite 8 / TanStack Router+Query+Table+Virtual / Motion / CSS Modules+CSS Custom Properties。React Aria Componentsはpreferred PoC candidate。TanStack Store/XStateはv0 productionへ追加しない。
- predecessor residual: PR #32 exact headのStandard CI `36662871915`、Sandbox `36662871921`、DSI PoC `36662871930` はSUCCESSだが、旧HTTP Active/Statusは最終COMPLETEへ未更新。stacked PR #27/#29/#30/#31/#32も未merge。承認後のProduction PlanではG0として最初に閉じる。
- Production Implementation Plan: `docs/superpowers/plans/2026-09-30-document-gui-integration-v0-production-implementation.md` blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`。G0〜G9を単一計画で実行する。**未承認**。
- Implementation handoff: `docs/superpowers/handoffs/2026-09-30-document-gui-integration-v0-implementation.md`。Plan approval blob一致を必須gateとする。
- blocker: Production Implementation Planの依頼者承認。承認前にmigration、product code、OpenAPI product差分、frontend app、production dependency promotion、predecessor mergeへ進まない。
- 次のexact action: Production Implementation Planを依頼者へ提示する。明示承認後にplan approval recordを作成し、別session promptからG0へ進む。
