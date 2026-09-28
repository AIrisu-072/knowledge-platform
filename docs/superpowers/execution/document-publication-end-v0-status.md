# Document Publication End v0 — 実行状況

- 状態: **ACTIVE — Production Tasks 1–5 完了。実装 Draft PR #14 のレビュー待ち。**
- 基準: `feat/document-versioning-v0@96b068bc9484a219b435d0633ea6669ddb7d7f97`（PR #12、PR #11 が基点）。承認済み設計・計画は `design/document-publication-end-v0@8415d8995ea719d6a510fe7f4aafc1ebf01bfa80`、[Draft PR #13](https://github.com/AIrisu-072/knowledge-platform/pull/13)。PR #13 の exact-head 標準 CI `36326032482`、Sandbox `36326032479`、DSI PoC `36326032480` はすべて SUCCESS。
- 実装: `feat/document-publication-end-v0`、[Draft PR #14](https://github.com/AIrisu-072/knowledge-platform/pull/14)。検証済み実装 head `79c7ff944cfde49574d1960c2ffc5e20cdd066a7`。Task 1 は Domain/Application 終了契約、Task 2 は PostgreSQL 一括 transaction、Task 3 は再公開防止、Task 4 は編集・通常公開・内部読み取りの分離。Task 5 の実 DB 縦断経路は初版公開→後続版公開→予約→T10 を確認した。
- RED/GREEN: Task 1 の未実装 Domain/Application 契約、Task 2 の未実装 PostgreSQL port、Task 3 の終了後の初版再 Publish、Task 4 の未実装読み取り port/API を RED として記録。Task 5 の縦断テストは初回 GREEN、補足 UTC 正規化テストは RED→GREEN。詳細はこの worktree の SDD ledger に記録した。
- ローカル検証: pin 済み PDFium の `PDFIUM_DYNAMIC_LIB_PATH` を設定した `mise run verify` が **459/459 テスト成功、既定の除外 4 件**。fmt、strict Clippy、architecture、API、security も PASS。最初の試行で見つかったテストのロック範囲と旧 migration fixture を修正し、PDFium 未設定で失敗した既存 PDF テストも設定後に通過した。
- GitHub: 実装 head `79c7ff944cfde49574d1960c2ffc5e20cdd066a7` の標準 CI `36332829274`、DSI Sandbox Preflight `36332829304`、DSI PoC `36332829327` はすべて **SUCCESS**。標準 CI の required-check、Ubuntu Rust、macOS Intel/arm64、policy/security、container build も SUCCESS。PR #14 の差分レビューに指摘なし、未解決 thread は 0 件。PR #11・#12・#13 は未マージ。
- Blocker / 未解決判断: なし。Design Freeze 差分提案なし。UTC 正規化は承認済み設計の明示要件に従う修正。
- 次の exact action: この完了記録だけを commit/push し、その PR #14 最終 head の標準 CI、Sandbox、PoC を確認する。結果を PR #14 に追記し、レビューまたは明示的なマージ指示を待つ。明示指示なしに PR #11・#12・#13・#14 をマージしない。
