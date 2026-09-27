# Document Publication End v0 — 実行状況

- 状態: **ACTIVE — 日本語版設計承認済み／読み取り境界の設計改訂案 1 が承認待ち**。
- 基準: `feat/document-versioning-v0@96b068bc9484a219b435d0633ea6669ddb7d7f97`（PR #12、PR #11 が基点）。T10 ブランチは `design/document-publication-end-v0`、[Draft PR #13](https://github.com/AIrisu-072/knowledge-platform/pull/13) は PR #12 を base とする。
- 完了: T10 設計書を技術契約を変えずに日本語化した commit `3593444f19f9b5b4d0d3138fea68efaf26bf4594`。依頼者の「日本語で書き直したら承認します」という条件を満たした承認記録と、T10 に関する `spec/` の整合を commit `eca5465f498c4fb2f7e5815b4c88ef0bce461506` に記録した。
- 現在: `docs/superpowers/specs/2026-09-27-document-publication-end-v0-design-amendment-1.md` を提案中。凍結済み Authoritative Core は `WORKING` 初版の `GetDocument` と原本読込を要求する。承認済み T10 §6 の既存 API を current-only に変える記述とは衝突するため、既存の下書き取得を維持しつつ T10 後は遮断し、通常公開用に current-only API を追加する限定改訂を提案した。元の T10 設計本文は承認なしに変更していない。
- Verification: 日本語版と旧版で 7 節・技術識別子を照合し、不可視文字・placeholder・whitespace を確認した。規範仕様の差分も `git diff --check` で確認した。T10 の本番コード・ホスト CI はまだない。
- Versioning: PR #11・#12 は Ready for review、未マージ。PR #11 head `987890d635f9563bb7028841a026663a9379d6f3` は CI `36294084935`／Sandbox `36294084939`／PoC `36294084954`、PR #12 head `96b068bc9484a219b435d0633ea6669ddb7d7f97` は CI `36310042488`／Sandbox `36310042492`／PoC `36310042485` が成功。T10 実装の証拠ではない。
- Blocker: T10 読み取り境界の改訂承認。実装計画はこの判断後に完成・レビューする。Versioning 設計の改訂提案はなく、PR #11・#12 のマージには別途明示指示が必要。
- 次の exact action: 改訂案 1 をレビューし、承認されたら T10 設計 §6 と計画 Task 4 を整合させ、計画全体をレビューに出す。計画承認前に T10 Production コードを変更しない。
