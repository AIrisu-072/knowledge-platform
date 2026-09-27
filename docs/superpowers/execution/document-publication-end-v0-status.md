# Document Publication End v0 — 実行状況

- 状態: **ACTIVE — 日本語版設計と読み取り境界改訂 1 は承認済み。実装計画はレビュー待ち。**
- 基準: `feat/document-versioning-v0@96b068bc9484a219b435d0633ea6669ddb7d7f97`（PR #12、PR #11 が基点）。T10 ブランチは `design/document-publication-end-v0`、[Draft PR #13](https://github.com/AIrisu-072/knowledge-platform/pull/13) は PR #12 を base とする。
- 完了: T10 設計の日本語化 `3593444f19f9b5b4d0d3138fea68efaf26bf4594`、設計承認記録と T10 規範仕様の整合 `eca5465f498c4fb2f7e5815b4c88ef0bce461506`。依頼者は読み取り境界の改訂案 1 に「承認します。」と回答した。改訂承認記録、T10 設計 §6・受入項目 5 の更新、規範仕様の読み取り区分の明確化、日本語版 Production Implementation Plan の提案を設計・計画 head `19df8e24e8ba9afbd5170f367b966ff0f1ebf889` に記録した。
- 承認済みの読み取り境界: 既存の `get_document` と `open_primary_file` は T10 未終了の初版 `WORKING` を維持し、T10 後は同一 SQL statement の台帳ガードで NotFound とする。通常公開用には別の current `PUBLISHED` 専用 API を設ける。内部 Versioning・Create 結果照会は残す。凍結済み Authoritative Core 設計は変更しない。
- 検証: 設計・規範仕様・計画の staged diff に `git diff --cached --check` を実行して PASS。実装計画は提案文書であり、T10 Production コードの変更・テスト・ホスト CI はまだない。PR #11・#12 の head と OPEN／未マージ状態、PR #13 の Draft／OPEN 状態を 2026-09-27 に確認した。
- Branch / PR: 設計・計画 `design/document-publication-end-v0` / #13。計画承認後の実装候補は `feat/document-publication-end-v0` と別 Draft 実装 PR。PR #11・#12・#13 のマージ指示はない。
- Blocker: 実装計画の依頼者レビュー・承認。Versioning 設計の改訂提案はない。
- 次の exact action: PR #13 上の `docs/superpowers/plans/2026-09-27-document-publication-end-v0-production-implementation.md` をレビューする。承認されたら、その承認を記録し、承認済み設計 head から独立した実装ブランチを作成して Task 1 のローカル RED からインラインで進める。計画承認前に T10 Production コードを変更しない。
