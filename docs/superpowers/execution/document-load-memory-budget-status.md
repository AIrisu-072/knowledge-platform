# ローカル文書負荷試験のメモリ改善

Status: ACTIVE / 実装・TDD中

- 基点main408b68d4fa0476aa9d7e078826011efadef8a4c4、branch fix/document-load-memory-budget-20261010。
- 利用者の「進めて」でIDだけの一覧保持、役割別RSSとNode heap、専用Ubuntu RSS4GiB/利用可能メモリ予備4GiB、安全係数2維持、公開レビューCI統合後再試験を承認済み。
- 対象は検証harnessだけ。既存API・PDF・sandbox・標準CIの既定予算・既存サービス・認証を変更しない。
- TDD担当: ID保持api.ts、資源計測safety/hosted、localbudget/launcher/chain。独立レビューと実Ubuntu短期資格はこれから。
- 旧runは終了・cleanup済み。新規長時間runは未開始。次は3変更のRED/GREEN、全関連suite、独立レビュー、清潔最終headのUbuntu資格。
