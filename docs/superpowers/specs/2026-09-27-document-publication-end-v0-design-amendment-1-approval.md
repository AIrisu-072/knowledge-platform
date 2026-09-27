# Document Publication End v0 — 設計改訂 1 承認記録

- 状態: **承認済み**
- 承認日: 2026-09-27 JST
- 対象: `2026-09-27-document-publication-end-v0-design-amendment-1.md`
- 提案時の PR #13 head: `fb96693d793eaf238e78626a0b78a0978514e535`
- 承認の根拠: 依頼者は、読み取り境界の改訂案 1 を提示された後、「承認します。」と回答した。

## 承認された範囲

- 既存の `DocumentService::get_document` と `open_primary_file` は、T10 未終了の Document について、凍結済み Authoritative Core の `WORKING` 初版の読込を維持する。T10 終了後は同一 SQL statement 内の操作台帳確認によって旧版へのフォールバックを遮断し、NotFound とする。
- 通常公開用には、現行 `PUBLISHED` 版だけを返す `get_current_published_document` と `open_current_primary_file` を追加する。
- Versioning の内部操作・履歴 snapshot、Create の commit 結果照会は通常公開 API から分離して残す。Search hit の現行性照合は維持する。
- 凍結済み Document Authoritative Core 設計、T10 の公開終了 transaction、T4 の取下げ、公開予約の意味は変更しない。

## 次のゲート

T10 の日本語版実装計画をレビューする。計画の承認前に Production コードを変更しない。
