# 文書管理残タスク：分担と実行計画

基点はmain `2a37d35cd228344f98e0194de16d5336fa786e3c`。依頼者の10項目を対象とし、既存API接続を先行する。既存設計の意味を変更する案は確認後に実装する。

## 所有境界

| 項目 | Mac側の範囲 | 状態・依存 |
|---|---|---|
| 1 作業版原本構成 | working-version helper/editor | 相対パス・ordinal・形式互換追補の確認待ち |
| 2 過去公開版取下げ | Detail選択・lifecycle DOM試験 | 既存接続あり。未取得選択IDから現行版へfallbackする誤対象を修正 |
| 3 改訂単体詳細 | API adapter・専用panel・Detail・DOM試験 | 既存getDocumentRevisionへ接続 |
| 4 scheduler | クラウド親担当 | Macは調査のみ、製品編集しない |
| 5 配備・復旧・認証TLS | 運用追補 | 対象ホスト未確定、実環境設定は承認待ち |
| 6 PR85 Document誤判定 | クラウド親担当 | Macの2ファイル抽出試験を共有し差分を戻した |
| 7 初回複数原本 | multipart/application/repository/recovery/GUI | atomic作成の新設計確認待ち |
| 8 主体検索・ACL追加 | directory port/API/draft reconciliation | 新設計確認待ち、実アカウント変更なし |
| 9 原本viewer | bounded binary取得・専用viewer | 形式/容量の確認待ち、既読契機は現行を保持 |
| 10 大量検証 | 検索manifest再利用＋Document測定 | 手順作成。対象環境・資源枠・SLOを確認後実測 |

## 既存接続の実装順序

1. 旧版選択一致と改訂単体詳細の回帰試験でREDを確認する。
2. 改訂adapter・専用panel、選択IDのfail closedを実装する。保存時snapshotを現行metadataで代用しない。read-state契機は変えない。
3. 非表示・別選択・遅延応答・401/403/404・キャッシュ再認可・画面遷移を試験する。既存UNKNOWN固定再送と履歴を保持する。
4. 日本語手順と未承認の設計案を同じDraftにまとめる。共有Cargo/migration/Search sourceへ変更しない。
5. 独立レビュー・必要修正・正確なhead CI後に親へ報告する。mainの同時更新を再確認し、親と統合順序を調整する。旧baseの試験を新headへ転用しない。

新機能の設計確認は[原本構成/初回複数](../specs/2026-10-08-document-original-management.md)、[主体検索/viewer](../specs/2026-10-08-document-principal-search-viewer.md)へ。運用・大量測定は[配備/復旧/段階測定](../../operations/document-deployment-and-scale-validation.md)へ。進捗と正確な証拠は[状況](../execution/document-remaining-20261008-status.md)へ残す。
