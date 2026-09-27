# Document Publication End v0 — 実行状況

- 状態: **ACTIVE — Production Tasks 1–4 完了。Task 5 の実装・ローカル検証完了、exact-head CI と PR レビュー待ち。**
- 基準: `feat/document-versioning-v0@96b068bc9484a219b435d0633ea6669ddb7d7f97`（PR #12、PR #11 が基点）。承認済み設計・計画は `design/document-publication-end-v0@8415d8995ea719d6a510fe7f4aafc1ebf01bfa80`、[Draft PR #13](https://github.com/AIrisu-072/knowledge-platform/pull/13)。PR #13 の exact-head 標準 CI `36326032482`、Sandbox `36326032479`、DSI PoC `36326032480` はすべて SUCCESS。
- 実装: `feat/document-publication-end-v0`。検証済みコード head `9715eb60445bb3b543b2ff6d4414136df49ebdaa`。Task 1 は Domain/Application 終了契約、Task 2 は PostgreSQL 一括 transaction、Task 3 は再公開防止、Task 4 は編集・通常公開・内部読み取りの分離。Task 5 の実 DB 縦断経路は初版公開→後続版公開→予約→T10 を確認した。
- RED/GREEN: Task 1 の未実装 Domain/Application 契約、Task 2 の未実装 PostgreSQL port、Task 3 の終了後の初版再 Publish、Task 4 の未実装読み取り port/API を RED として記録。Task 5 の縦断テストは初回 GREEN、補足 UTC 正規化テストは RED→GREEN。詳細はこの worktree の SDD ledger に記録した。
- ローカル検証: pin 済み PDFium の `PDFIUM_DYNAMIC_LIB_PATH` を設定した `mise run verify` が **459/459 テスト成功、既定の除外 4 件**。fmt、strict Clippy、architecture、API、security も PASS。最初の試行で見つかったテストのロック範囲と旧 migration fixture を修正し、PDFium 未設定で失敗した既存 PDF テストも設定後に通過した。
- GitHub: 実装 Draft PR は作成前。まとまった実装を一度 push し、同一 head の標準 CI・DSI Sandbox Preflight・DSI PoC を確認する。PR #11・#12・#13 は未マージのまま維持する。
- Blocker / 未解決判断: なし。Design Freeze 差分提案なし。UTC 正規化は承認済み設計の明示要件に従う修正。
- 次の exact action: Active/Status のこの実装チェックポイントを commit し、`feat/document-publication-end-v0` を push する。PR #13 を base とする別の Draft 実装 PR を開き、その exact head の標準 CI、Sandbox、PoC を確認してから PR 差分をレビューする。明示指示なしに PR #11・#12・#13 または実装 PR をマージしない。
