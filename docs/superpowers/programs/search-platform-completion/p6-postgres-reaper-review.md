<a id="p6-g04-postgresql-exhausted-reaper-独立レビュー"></a>
# P6-G04 PostgreSQLの試行上限到達行を回収する処理の独立レビュー

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-reaper-review.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 判定: **G04 限定 GO**。対象は `PostgresOutboxStore::reap_exhausted` と `postgres_reaper.rs` の機能契約だけ。2026-09-30 22:27 JST、未コミットの `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` を監査した。
- 根拠: `p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §3、`p6-outbox-plan.md` G04、`p6-domain-migration-recheck.md`、`p6-postgres-settle-review.md`。作成者の判定だけには依存せず、現行ソース/テスト/SQL と実 DB 結果で照合した。コード、仕様、タスクグラフ、Cargo/ロックファイルは変更していない。

## 契約照合

| 条件 | 現行コードと独立確認 |
| --- | --- |
| 六つのポリシー値と更新の反映排他 | `crates/outbox-delivery/src/postgres.rs:25-38,54-107,349-360`。改訂番号、最大試行回数、リース最小・最大、再試行待機最小・最大の全値を単一ポリシー行と比較し、同一トランザクション内で `FOR SHARE` を保持する。試験 `postgres_reaper.rs:263-306` は六つを個別に変更して `PolicyMismatch` と行不変を確認した。 |
| 旧行のNULL と行別最終回数 | `postgres.rs:89-107,360,365-368`。未完了で `attempt_limit IS NULL` かつ現ポリシーの上限到達行を先に `LegacyExhausted` として拒否する。上限到達行の回収処理はローカル設定の最大値を終端判定に使わず、固定済みの行別試行上限だけを使う。試験 `postgres_reaper.rs:293-305` は旧行を黙ってデッドレター化しない。 |
| DB 時刻、リース、終端状態 | `postgres.rs:362-377` は `MATERIALIZED` な単一 `clock_timestamp()` 値を有効期限条件と `dead_lettered_at` の両方に使う。配送済み・デッドレター化済みのどちらでもない、固定済みの上限到達、リース不在または期限到達の行のみを更新する。`postgres_reaper.rs:308-357` はリース更新済み最終試行、未上限、配送済み・デッドレター化済みの行の完全不変を確認した。 |
| 有限バッチと競合 | `postgres.rs:351-354,369-381` は 1..=32 を検査し、`last_attempt_at NULLS FIRST,event_id` の順で `FOR UPDATE OF o SKIP LOCKED` を用いる。別接続プールの二つのOS子プロセスは同期点後に同じ期限切れ最終行を回収し、件数 `[0,1]`、再回収 0 (`postgres_reaper.rs:109-261`)。別トランザクションが先頭行をロックした試験は 5 秒タイムアウト内で次の 32 行を回収し、残り 2 行を次回回収した (`:359-401`)。 |
| 原行と未知結果 | `postgres.rs:373-384` は終端時刻、リース三列、固定エラーコードだけを変更し、原行を消さず、コミット後に件数を返す。試験 `postgres_reaper.rs:203-260,391-397` は ID/型/集約/ペイロード/発生時刻/利用可能時刻/試行回数/行ごとの試行上限/最終試行時刻の保持、リース消去、未配送成功確定、再処理権取得不可を確認した。トランザクション開始/SQL/コミットの失敗は `StoreUnknown` (`postgres.rs:355-359,379-384`)、閉じた接続プールでもそのエラーを確認 (`postgres_reaper.rs:402-406`)。 |
| マイグレーションの受入 | `0009_outbox_delivery_v0.sql:14-37,39-66` はリース完全性、一方だけの終端状態、`delivery_unknown_at_limit` 許可リスト、有限ポリシー上限・下限、候補索引を定義する。既存マイグレーションの独立 4/4 実 DB 再確認は別検証記録に記録済み。 |

PostgreSQL 18 の公式資料でも、`SKIP LOCKED` は取得できないキュー行を飛ばし、`FOR SHARE` は同じポリシー行の更新を妨げる。実体化CTE は同じ SQL文内の参照に一つの結果を与え、`clock_timestamp()` 自体は呼ぶたびに変化するため、その単一評価が必要である。参照: [SELECT](https://www.postgresql.org/docs/18/sql-select.html)、[明示的ロック](https://www.postgresql.org/docs/18/explicit-locking.html)、[WITHクエリ](https://www.postgresql.org/docs/18/queries-with.html)、[日付・時刻関数](https://www.postgresql.org/docs/18/functions-datetime.html)。

<a id="fresh-verification-と入力固定"></a>
## 新たな検証と入力の固定

| 対象 | SHA-256 |
| --- | --- |
| `crates/outbox-delivery/src/postgres.rs` | `7956b053616763b933f08a3aca58e06a7797ee18acc8d1a9740d88e2ac53dd74` |
| `crates/outbox-delivery/tests/postgres_reaper.rs` | `c21ed7e782b9cda976be80ca8f9f02cad1e3f9c71d314e45c69a4215d0b10c1d` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `target/debug/deps/postgres_reaper-ba87f7f683163f68` | `6b82fa410be776d6b46b2c8923809044bffb9b727cc3739eb3037b7ecd8c2c9e` |

- 開始時と試験後の上記四 SHA は同じ。`postgres.rs` 更新時刻 22:13:45、テスト 21:12:06、SQL 18:39:18、キャッシュ済み実行ファイル 22:14:01 JST。`--list` は子フィクスチャ 1、通常の4テストを列挙した。`migrate` は `document-repository-postgres/src/lib.rs:61` の `sqlx::migrate!` を使う。更新時刻と SHA は現行入力からの再コンパイルを暗号学的に証明しないので、動的結果はこの **記録したキャッシュ済みバイナリ** に限定する。
- 新たな `target/debug/deps/postgres_reaper-ba87f7f683163f68 --test-threads=1 --nocapture`: **4 passed / 0 failed / 1 ignored / exit 0**、テスト本体 32.61 秒。実 PostgreSQL 18.6 イメージ ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`、Docker Server 29.4.0。試験所有の使い捨てコンテナ以外は操作していない。同時刻に別 P7 監査担当の PostgreSQL コンテナがあり得たため、時間を性能測定には用いない。
- 新たな `rustfmt --check --edition 2024 crates/outbox-delivery/src/postgres.rs crates/outbox-delivery/tests/postgres_reaper.rs`: exit 0。I02 が Cargo を排他使用中のため、今回 Cargo 再コンパイル、Clippy、ワークスペース全体の検証ゲートは走らせていない。

## 判定境界

G04 は最終処理権取得後に失効した行を結果不明の終端状態として一度だけ回収する。試験は実 OS 子プロセスと独立接続プールを使うが、実際のワーカーを強制終了する G07 再起動試験ではない。上限到達行の回収処理の COMMIT 応答喪失プロキシによる障害注入も今回の動的試験にはない。コード上のコミットエラーは `StoreUnknown` で、G03 では汎用側の配送成功確定の実 COMMIT 応答喪失を別途確認している。これは Search公開コミットの証拠ではない。

実配送用ロールの `FOR SHARE`/列権限、大表の索引・ロック時間、処理能力、G05–G07 ランナー・再起動、Searchイベント受領記録・公開と P7 永続基盤の組み合わせ、新たなビルドと対象HEADに固定したホストCI はそれぞれ別検証ゲート。当時の次の作業は、この限定 GO を汎用P6 検証記録に取り込み、I02 の Cargo実行枠解放後に必要な新たなビルドを行い、後続の実際のロール・ランナー・Search 統合を個別に検証すること。
