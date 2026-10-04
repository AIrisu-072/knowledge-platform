<a id="p6-g02-postgresql-claim-永続化独立レビュー"></a>
# P6-G02 PostgreSQLの処理権取得・永続化の独立レビュー

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-postgres-claim-review.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 判定: **G02 限定 GO**。対象は `PostgresOutboxStore::verify_policy` / `claim` のポリシー検査、旧行診断、上限付きの処理権取得と PostgreSQL 競合試験。G03/G04、権限付き実行、Search 統合、P6 全体、本番配備の GO ではない。
- 確認日時: 2026-09-30 19:25 JST。`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット作業木。ソース/テスト/バイナリは試験前後で同じ SHA-256、SQL は現行値が先行マイグレーション再確認の値と一致。
- 契約: `p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §2–3、`p6-outbox-plan.md` P6-G02、`p6-policy-bounds-ruling.md`、`p6-policy-lock-role-ruling.md`、`p6-domain-migration-recheck.md`、`spec/data/transaction-consistency-requirements-v0.md` §7。

## G02 の照合

- `crates/outbox-delivery/src/postgres.rs:25-52` は期待ポリシーの改訂番号・試行回数/リース/再試行待機有界性を検査し、処理権取得の 1..=32 件と整数ミリ秒リースを DB ポリシーの範囲に限定する。`0009_outbox_delivery_v0.sql:39-55` の CHECK と初期値に整合する。
- `postgres.rs:54-108` は処理権取得トランザクション内でポリシー1行を `FOR SHARE` し、改訂番号を含む 6 値を完全比較する。未完了で `attempt_limit IS NULL` かつ現行 DB 上限に達した旧行は `LegacyExhausted { count, first_ids }` とし、ID は最大 32 件、ペイロードは診断に含めない。旧行をリセット／終端化しない。
- `postgres.rs:153-219` は同じトランザクションでガード後に、実体化した `clock_timestamp()`、`FOR UPDATE OF o SKIP LOCKED`、`LIMIT` を使って候補を選ぶ。未完了・利用可能・リース失効・行別試行上限未満を条件にし、`attempt_limit=COALESCE(old,db_max)`、行別 `gen_random_uuid()`、試行回数/リース/最終試行時刻のみを更新する。コミット成功後だけ処理権取得を返し、DB／コミット応答不明は `StoreUnknown` にする。`payload`、Domain 業務行、Audit 行は更新しない。`FOR UPDATE` は処理権取得の短い排他で、ハンドラー実行中の排他ではない。
- PostgreSQL 18 公式資料は `FOR SHARE` が同じポリシー行への UPDATE をトランザクション終了まで防ぎ、`SKIP LOCKED` がキュー消費者の競合回避に使えると明記する。したがってポリシーの検査と処理権取得の間で設定更新を通さないというコード上の構成は妥当。ただし競合ポリシー更新の反映の同期点試験は今回の 3 件に含まれない。[行ロック](https://www.postgresql.org/docs/18/explicit-locking.html)、[SELECTのロック句](https://www.postgresql.org/docs/18/sql-select.html)。

<a id="fresh-な実-db-証拠と由来"></a>
## 新たな実DB検証の証拠と由来

| 対象 | SHA-256 |
|---|---|
| `crates/outbox-delivery/src/postgres.rs` | `f5c55a91db2dd6e6ed165b62e7c76924c6c9ea8a8892377bc07966d691bcda36` |
| `crates/outbox-delivery/tests/postgres_claim.rs` | `1dba8534fe57dcaf650a9c57dbc0320bf3941c4b20955171a24f536b77fb71da` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `/tmp/search-completion-preserved-poc-bin/postgres_claim-18305c7da81e38fb` | `32b95381fa05e01713e6f49ffa4699f053a299d73253aa89b24fcd257f6f9a45` |

- 実行: `/tmp/search-completion-preserved-poc-bin/postgres_claim-18305c7da81e38fb --test-threads=1 --nocapture`。**3 passed、0 failed、exit 0、51.57 s**。順に `claim_caps_batch_and_pins_limit_once`、`concurrent_claims_are_disjoint`、`policy_mismatch_and_legacy_exhausted_refuse_claim` が成功。Docker Server 29.4.0、フィクスチャは `postgres:18.6-bookworm`（キャッシュ済みイメージID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`）。試験後に同イメージの稼働コンテナなし。
- `tests/postgres_claim.rs:77-177` は設定不一致と旧上限 33 件の拒否、32 ID 診断、旧上限行の試行回数未変更を確認。`:180-276` はバッチ 32、トークンの一意性、初回試行上限固定、ポリシー更新の反映後の既存試行上限/ペイロード維持を確認。`:278-368` は独立接続プールの 2/4/8 ワーカーで有効な処理権取得重複がなく、期限切れ後の再処理権取得が次の試行回数と別トークンになることを確認。
- `p6-domain-migration-recheck.md` の SQL ハッシュと現行 SQL ハッシュは一致する。テストバイナリの更新時刻 19:00:39 JST は対象ソース/テスト/SQL の更新時刻より後。ただし今回 Cargo 再コンパイルは行っておらず、`document_repository_postgres::migrate` は `sqlx::migrate!` の埋め込みマイグレーションを使う。時刻とハッシュはキャッシュ済みバイナリが現行ソース/SQL の正確なバイト列からビルドされたことの暗号学的証明ではない。ここで確認したのは、現行ファイルの静的レビューと、そのキャッシュ済みバイナリによる新たな PostgreSQL 3 件の実行結果である。

## 残る境界

- `FOR SHARE` は PostgreSQL の仕様上、ポリシー表の少なくとも 1 列への `UPDATE` 権限が必要。今回のフィクスチャは管理者接続であり、実配送用ロールでのロック成功・ポリシー値の変更拒否は未実証。`p6-policy-lock-role-ruling.md` の `UPDATE(policy_id)` 限定権限付与と G08/I04 の実際のロール試験を通すまで配備判定に使わない。[PostgreSQLのSELECT権限](https://www.postgresql.org/docs/18/sql-select.html)。
- `claim(limit)` は呼出側の空き実行許可枠数を受け取らない。`limit <= free_permits`、Search の Source リース取得後処理権取得 1 件、実行枠への割当直前のフェンスは G05/S02 以降の責務で、G02 からは成立を主張しない。`begin()` はトランザクション分離レベルを明示しないため、今回の PostgreSQL フィクスチャは既定の `READ COMMITTED` に依存する。本番接続プール・セッションの分離レベル設定とロック所要時間は未検証。[PostgreSQL トランザクション分離](https://www.postgresql.org/docs/18/transaction-iso.html)。
- `postgres.rs:222-253` の G03 リース更新/配送結果確定と G04 期限切れ処理の回収は明示的な `StoreUnknown` 未実装の仮置き。配送成功確定、再試行、DLQ、クラッシュ後の回収、コミット結果不明後の再照合は未実装・未検証。処理権取得のコミット結果不明は `StoreUnknown` として返す静的経路を確認したが、障害注入試験は今回の対象にない。これらを含む本番実装の完了は宣言しない。
