# Work PostgreSQL adapter — Browser PoC

このadapterは、固定された模擬workflowの最小実装です。Documentのtable、migration ledger、認可、publicationは変更しません。

- `migrate` は明示的な管理コマンド専用です。`work.schema_migrations` に固有のversion/checksumを保存します。通常起動では `check_schema_compatibility` のみを実行します
- `seed_synthetic` は明示的な模擬データ投入です。既存workflowを上書き・初期化しません。Document IDは共有入力参照であり、private draftの保存先ではありません
- 一つの固定workflowを `work.workflow_instances.body` に保存し、更新時に `FOR UPDATE` で直列化します。履歴、独立operation ledger、必須event stagingは同一transactionです
- 合成Organization policy（組織単位・役割・正式割当・委任）は別集約として `work.organization_policies` に保存します（migration 0007）。Work操作はpolicy行を `FOR SHARE` してからworkflow行を `FOR UPDATE` し、commit時刻で担当の有効性を再評価します。policy操作はpolicy行を `FOR UPDATE` し、同じledger（`policy_id`）と必須stagingを同一transactionで記録します。staging payloadへ利用者の理由文は入れません
- operationの完全一致再送は、現在の利用権限を再確認して元の結果を返します。異なるcommand digestは拒否します。commitの結果を確認できない場合は `COMMIT_OUTCOME_UNKNOWN` を返します
- schema-bound text draftはUTF-8で8KiB以下、workflow内16件以下です。提出snapshotはvalue/revisionを固定します。ローカルfile upload、外部providerのprivate保管、Search、native Workspaceは未対応です。担当変更と期限付き委任は合成policyの範囲で対応します
- 共有Documentの本文は既存Document APIで現在の認可を受けます。WorkはDocument本文を取得・複製せず、provider accessを付与しません
- `event_staging` はローカルの必須stagingです。Audit配送や別providerとの分散transactionの成立を表しません

## 実DB検証

`tests/postgres_transaction.rs` は明示的にignoredです。通常の `cargo test` で実DBを使った合格とは扱いません。実行には許可された新規・破棄可能なPostgreSQL databaseと `WORK_POC_TEST_DATABASE_URL` が必要です。database名の末尾は `_work_poc_test` に限定しています。試験はWork schemaを作成し、staging障害用triggerを一時作成します。既存・本番databaseでは実行しません。

本slice作業では実DB・socket実行を行っていません。純粋domain/HTTP試験とcompileは、PostgreSQLの原子性・再起動・同時実行の実証の代用ではありません。
