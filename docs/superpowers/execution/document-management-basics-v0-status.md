# Document Management Basics v0 — 準備状況

- 状態: **DESIGN APPROVED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**
- 日付: 2026-09-28 JST
- 対象: transaction T5〜T9、認可付き一覧・版/操作履歴・ファイル参照、必要なT11/T12。
- 設計: `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design.md`
- 設計承認: `docs/superpowers/specs/2026-09-28-document-management-basics-v0-design-approval.md`
- 実装計画: `docs/superpowers/plans/2026-09-28-document-management-basics-v0-production-implementation.md`
- 設計・計画ブランチ: `design/document-management-basics-v0`
- PR: #15 — Draft / Open / 未マージ。最新headとchecksはGitHubから再取得する。
- 基準main: `55dc3d3a430c8f36e1db8277fee15c4429258466`
- 承認対象の提示head: `0314f91a4e68221ed06778d36eaf228d0adfec86`
- 凍結した設計本文blob: `38010802a04c285336810e9b9c637c656ed1a76b`。本文は変更していない。

## 承認・実装開始条件

| 項目 | 状態 |
|---|---|
| 書面設計 | 依頼者承認済み。承認記録を参照 |
| Production Implementation Plan | 作成済み・レビュー待ち。未承認 |
| 実行方法 | 計画レビューで確定。過去T10の実行方法を自動流用しない |
| 開発ログ一元管理 | 完了未確認。別作業の状態を推測しない |
| 製品実装開始 | 未指示・未着手 |
| 規範差分の反映 | 計画で追跡。実装前に差分レビューする |
| merge | 指示なし |

## 今回の準備内容

- 書面設計の承認対象をcommit/blobで特定して記録した。
- PR A（認可と既存操作）、B（属性・Folder・移動）、C（既読・一覧・履歴）、D（T11/T12統合1本）の4単位、MB-01〜11の実装計画を作成した。
- 対象ファイル、共通型とport、DB保存単位、digest/cursor、RED/GREEN手順、25受入条件の対応、migration/復元とexact-head gateを記載した。
- 各機能で必要なイベントと原子性試験を完成させる。後続Dを未記録機能の後付け工程にしない。
- 計画レビュー対象の技術上限・DTO・history代表版・nullable終端証跡を計画第7節に列挙した。

## 検証の記録と限界

- 作業開始時GitHubのmainとPR #15は上記SHAだった。旧PR headのCI run `36367663785` は取得時in_progressで、SUCCESSは未確認だった。更新後headの証拠には流用しない。
- AGENTS、active、T10 status/設計/計画、今回の設計全文とstatus、Domain型、PostgreSQL modules/test fixture、scheduler assemblyを参照した。
- ローカルcloneは実行環境でgithub.comの名前解決に失敗した。GitHub connectorの読み書きは使用可能。ローカルRustテスト、実DB試験、配備検証は未実行。
- 計画の構造・MB-01〜11の順序・DMB-01〜25の対応・65個の未着手チェック欄・canonical vector・空白/競合markerをローカルの文書検査で確認した。これはRust実装試験ではない。計画のローカルblob SHAとGitHub create_blobのSHAは `3b5cc84a8593134cdd7e01ea026bd2a124fa9585` で一致した。
- 更新後PRの実際の差分とhead/checksはPRコメントへ記録する。CI待ちを成功扱いにしない。
- 製品コード、migration、依存、OpenAPI、規範spec、既存active pointerは変更しない。

## 次のexact action

1. PR #15の最新headと差分を取得し、計画をレビューする。
2. 計画承認と実行方法の決定を記録する。未確認なら実装へ進まない。
3. ログ一元管理未完了なら `PLAN APPROVED / WAITING_FOR_LOG_CENTRALIZATION` として保持する。
4. 完了確認と実装開始指示があった場合に限り、最新mainとの差分・実採番・規範反映・環境・exact-head checksを確認する。
5. その時点で今回のCapabilityをactiveへ接続し、MB-01のREDから実装する。

自動監視・自動開始は設定していない。本文の過去PROPOSED表示は承認記録で更新された状態と区別する。MB-01〜11と25件の受入条件はいずれも実装・実行済みではない。
