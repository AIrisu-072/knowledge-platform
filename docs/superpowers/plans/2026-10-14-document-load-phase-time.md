# 文書段階試験の工程別時間予測 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. 利用者は本修正の実装・独立レビュー・公開・CI・main統合・再試験を承認済み。

**Goal:** 固定のプロセス再起動を全件比例外挿する誤判定を修正する。

**Architecture:** 全経過時間を有界な非重複monotonic区間で覆う。文書走査を行わないプロセス再起動だけを固定費とし、他の区間は件数比例計上する。工程別証拠の欠陥を開始許可に読み替えない。

**Tech Stack:** Node 24.21.0 / ESM / node:test。依存追加なし。

**Spec:** [承認済み10万件計画](2026-10-09-document-load-local-scale.md)、[現行メモリ予算](../../operations/document-load-local-memory.md)、[本変更の操作と算式](../../operations/document-load-phase-time.md)。2026-10-14の利用者「その対応で進めてください」が工程別計測と公開・統合・再試験の承認。

## Global Constraints

- 安全係数2、small5分・1000件120分・1万件330分・10万件72時間、総80時間と最後15分cleanup枠を維持。
- 専用localのRSS4GiB・利用可能memory予備4GiB、disk予備1GiB、同一run/source/corpus/DB/storage、fresh段階順を維持。
- 未確認の登録を再送しない。旧runの実測を新runへ流用しない。GPU・既存サービス・本番認証/TLS設定を変更しない。

## Review Focus

- 工程の欠測・重複・未割当・不正数値はゼロと扱わない。
- 背景probeとHTTPの重なりを加算したり、probe合計を実elapsedから引いたりしない。
- 固定費は許可した再起動区間だけ。DB/storage identity照合や走査を固定費へ混ぜない。
- 旧reportは従来予測へfallbackし、新しい不正reportは拒否。
- 成功reportのtotalと最終区間を一致させ、タイミング証跡失敗は成功にしない。

## Task A: 有界な時間モデルとadmission

Files: `tools/document-load-qualification/phase-time.mjs`, `safety.mjs`, `test/phase-time.test.mjs`, `test/safety.test.mjs`。

Interface: `createPhaseTimer({now,startedAt,documentCount})` の `transition(name,kind)` / `finish(totalElapsedMs)`、`projectElapsedTime(report,nextCount,safetyFactor)`。schemaVersion1、最大64区間、0からtotalまで連続したstartMs/endMs。

- [x] 固定費一回・比例費N/n、欠測・重複・不正・未割当・上限・旧reportfallbackのREDを実行。
- [x] 計測と検証、`ceil(2 × [fixed + (total-fixed) × N/n])` を実装しGREEN。

## Task B: 実controller計測と回帰

Files: `controller.mjs`, `workflow.mjs`, `test/controller.test.mjs`, `test/workflow.test.mjs`。

- [x] 実controllerの全時間coverage、restartのみ固定、正例ループ件数、失敗時証跡のREDを実行。
- [x] BG観測待機後〜restart返却だけを固定として囲む。他の処理と最後のcloseまで保守側に計上しGREEN。

## Task C: 同じPRで出荷・現地試験

- [ ] 全harness/runtime試験・型検査・独立レビュー。最終headをUbuntuで通常資格とsmallへ通す。
- [ ] 日本語Draft PRを一件公開。正確head CI全成功後Ready、期待head指定でmain統合、統合後CI全成功。
- [ ] 最新clean mainで重複・資源を確認し、新規専用DBからfresh4段階pipelineを一回起動。runID/SHA/開始/期限/観測方法を先に報告。

2026-10-15再開: 製品head777eのUbuntu短期資格は成功。main追従後の組合せレビュー/CI/統合を優先する。新規長時間pipelineとデモ起動は今回の実行範囲外で、親の調整後に扱う。1000予測128.93分の未達を保持する。
