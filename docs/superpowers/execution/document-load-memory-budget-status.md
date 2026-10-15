# ローカル文書負荷試験のメモリ改善

## 2026-10-15 — 現在状態（main `ead6830e` を照合）

PR122はmain `373941fbfbcdd2cb85ecae0d5b9530ae85160241` に統合済み。workspaceの検証記録ではhead CI `38019410686` と統合後CI `38020892791` は全14成功、正確head `29dfd94e` のUbuntu短期資格と通常runtime/smallは成功・cleanup済み。新fresh4段階runの開始は記録されているが、10万件の成功を示す最終証拠は未確認。[local状況](document-load-local-scale-status.md)を再開先とする。

以下は各作業時点の履歴。未統合・未実行等の記述は、その節の日付時点の状態である。

Status: ACTIVE / 実装・TDD中

- 基点main408b68d4fa0476aa9d7e078826011efadef8a4c4、branch fix/document-load-memory-budget-20261010。
- 利用者の「進めて」でIDだけの一覧保持、役割別RSSとNode heap、専用Ubuntu RSS4GiB/利用可能メモリ予備4GiB、安全係数2維持、公開レビューCI統合後再試験を承認済み。
- 対象は検証harnessだけ。既存API・PDF・sandbox・標準CIの既定予算・既存サービス・認証を変更しない。
- TDD担当: ID保持api.ts、資源計測safety/hosted、localbudget/launcher/chain。独立レビューと実Ubuntu短期資格はこれから。
- 旧runは終了・cleanup済み。新規長時間runは未開始。次は3変更のRED/GREEN、全関連suite、独立レビュー、清潔最終headのUbuntu資格。
