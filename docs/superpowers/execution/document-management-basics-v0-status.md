# Document Management Basics v0 — 準備状況

- 状態: **DESIGN PROPOSED / WRITTEN SPEC REVIEW PENDING / IMPLEMENTATION NOT STARTED**
- 日付: 2026-09-28 JST
- 対象: transaction T5〜T9、利用者向け文書一覧・属性絞り込み・版/操作履歴・ファイル参照。
- 設計: `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md`
- 設計ブランチ: `design/document-management-basics-v0`
- 基準main: `55dc3d3a430c8f36e1db8277fee15c4429258466`

## 承認状態

- 設計を先行し、開発ログ一元管理の完了後に実装する進め方: 依頼者承認済み。
- 必要なT11/T12のイベント記録を各機能に含め、最後の1PRを横断統合・検証とする進め方: 依頼者承認済み。
- 書面設計の詳細: 未承認。本書から設計凍結を推測しない。
- Production Implementation Plan: 未作成。書面設計承認後に作成・レビューする。
- 開発ログ一元管理の完了: この作業では未確認。
- 本番実装・migration・依存追加・API/CLI/GUI実装: 未着手。
- merge指示: なし。

## 今回準備したもの

- T番号と既存Versioning実装Task番号を分離した対象定義。
- metadata、Folder、Policy、ReadState、一覧、履歴、ファイル参照の設計案。
- 認可の適用、予約との競合、型付きイベント、原子的記録、移行、25件の受入条件。
- 既存規範へ反映する差分提案と、後続統合PRの範囲。

## 取得した基準情報と検証の範囲

- GitHubで取得したmainは上記基準SHA。
- 同じSHAに対するpush CI `36362239773` は `completed / success` を取得した。これは既存mainの証拠であり、本設計PRや未実装機能の成功証拠ではない。
- 設計作成前のopen PR一覧は0件だった。以後はGitHubを再取得して判断する。
- 参照したコード: Applicationの既存登録処理、イベント対象型、現行公開参照、およびVersioningのmigration/操作台帳。既存版管理を再実装する計画にはしていない。
- ローカルRustテスト・配備先検証は今回実行していない。
- 設計のreview観点: scopeと承認状態、予約revision/認可の分離、T10非復活、既読と代理参照の分離、業務/監査の原子性、履歴のOutbox保持依存防止。
- この設計ブランチでは新規Markdownのみ追加する。既存 `active.md` と先行作業の実行記録は上書きしない。

## 次のexact action

1. GitHubでこのブランチをheadとする設計PR、実際のheadとチェック結果を取得する。
2. written Design Specのレビューを受ける。未承認の間は本番実装計画・製品コードへ進まない。
3. 書面設計承認後、必要な規範差分を追跡し、Production Implementation Planを作成・レビューする。
4. 計画承認後もログ一元管理完了が未確認なら `READY / WAITING_FOR_LOG_CENTRALIZATION` として保持する。
5. 依頼者の完了確認と実装開始指示があり、着手時main差分とexact-head gateに問題がなければ、今回のCapabilityをactiveへ切り替えて実装する。

履歴やこの記録だけからログ一元管理完了を推測しない。自動監視・自動開始は設定していない。書面設計の各提案と既存Design Freezeとの差分は、承認前に確定仕様として扱わない。
