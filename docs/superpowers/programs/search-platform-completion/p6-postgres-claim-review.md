# P6-G02 PostgreSQL claim 永続化・独立レビュー

- 判定: **G02 限定 GO**。対象は `PostgresOutboxStore::verify_policy` / `claim` の policy guard、旧行診断、bounded claim と PostgreSQL 競合試験。G03/G04、権限付き実行、Search 統合、P6 全体、本番配備の GO ではない。
- 確認日時: 2026-09-30 19:25 JST。`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット作業木。source/test/binary は試験前後で同じ SHA-256、SQL は現行値が先行 migration 再確認の値と一致。
- 契約: `p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §2–3、`p6-outbox-plan.md` P6-G02、`p6-policy-bounds-ruling.md`、`p6-policy-lock-role-ruling.md`、`p6-domain-migration-recheck.md`、`spec/data/transaction-consistency-requirements-v0.md` §7。

## G02 の照合

- `crates/outbox-delivery/src/postgres.rs:25-52` は期待 policy の revision・attempt/lease/backoff 有界性を検査し、claim の 1..=32 件と整数ミリ秒 lease を DB policy の範囲に限定する。`0009_outbox_delivery_v0.sql:39-55` の CHECK と初期値に整合する。
- `postgres.rs:54-108` は claim transaction 内で policy 1 行を `FOR SHARE` し、revision を含む 6 値を完全比較する。未完了で `attempt_limit IS NULL` かつ現行 DB 上限に達した旧行は `LegacyExhausted { count, first_ids }` とし、ID は最大 32 件、payload は診断に含めない。旧行を reset／terminal 化しない。
- `postgres.rs:153-219` は同じ transaction で guard 後に、materialized `clock_timestamp()`、`FOR UPDATE OF o SKIP LOCKED`、`LIMIT` を使って候補を選ぶ。未完了・利用可能・lease 失効・行別試行上限未満を条件にし、`attempt_limit=COALESCE(old,db_max)`、行別 `gen_random_uuid()`、attempt/lease/last_attempt のみを更新する。commit 成功後だけ claim を返し、DB／commit 応答不明は `StoreUnknown` にする。`payload`、Domain 業務行、Audit 行は更新しない。`FOR UPDATE` は claim の短い排他で、handler 実行中の排他ではない。
- PostgreSQL 18 公式資料は `FOR SHARE` が同じ policy 行への UPDATE を transaction 終了まで防ぎ、`SKIP LOCKED` が queue 消費者の競合回避に使えると明記する。したがって policy の check と claim の間で設定更新を通さないというコード上の構成は妥当。ただし競合 policy rollout の barrier 試験は今回の 3 件に含まれない。[row lock](https://www.postgresql.org/docs/18/explicit-locking.html)、[SELECT locking clause](https://www.postgresql.org/docs/18/sql-select.html)。

## Fresh な実 DB 証拠と由来

| 対象 | SHA-256 |
|---|---|
| `crates/outbox-delivery/src/postgres.rs` | `f5c55a91db2dd6e6ed165b62e7c76924c6c9ea8a8892377bc07966d691bcda36` |
| `crates/outbox-delivery/tests/postgres_claim.rs` | `1dba8534fe57dcaf650a9c57dbc0320bf3941c4b20955171a24f536b77fb71da` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `/tmp/search-completion-preserved-poc-bin/postgres_claim-18305c7da81e38fb` | `32b95381fa05e01713e6f49ffa4699f053a299d73253aa89b24fcd257f6f9a45` |

- 実行: `/tmp/search-completion-preserved-poc-bin/postgres_claim-18305c7da81e38fb --test-threads=1 --nocapture`。**3 passed、0 failed、exit 0、51.57 s**。順に `claim_caps_batch_and_pins_limit_once`、`concurrent_claims_are_disjoint`、`policy_mismatch_and_legacy_exhausted_refuse_claim` が成功。Docker Server 29.4.0、fixture は `postgres:18.6-bookworm`（cached image ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`）。試験後に同 image の稼働 container なし。
- `tests/postgres_claim.rs:77-177` は設定 mismatch と旧上限 33 件の拒否、32 ID 診断、旧上限行の attempt 未変更を確認。`:180-276` は batch 32、token の一意性、初回 limit 固定、policy rollout 後の既存 limit/payload 維持を確認。`:278-368` は独立 pool の 2/4/8 worker で live claim 重複がなく、期限切れ後の再 claim が次の attempt と別 token になることを確認。
- `p6-domain-migration-recheck.md` の SQL hash と現行 SQL hash は一致する。テストバイナリの更新時刻 19:00:39 JST は対象 source/test/SQL の更新時刻より後。ただし今回 Cargo 再コンパイルは行っておらず、`document_repository_postgres::migrate` は `sqlx::migrate!` の埋め込み migration を使う。時刻と hash は cached binary が現行 source/SQL の exact bytes からビルドされたことの暗号学的証明ではない。ここで確認したのは、現行ファイルの静的レビューと、その cached binary による新たな PostgreSQL 3 件の実行結果である。

## 残る境界

- `FOR SHARE` は PostgreSQL の仕様上、policy 表の少なくとも 1 列への `UPDATE` 権限が必要。今回の fixture は管理者接続であり、実 delivery role での lock 成功・policy 値の変更拒否は未実証。`p6-policy-lock-role-ruling.md` の `UPDATE(policy_id)` 限定 grant と G08/I04 の実 role 試験を通すまで配備判定に使わない。[PostgreSQL SELECT privilege](https://www.postgresql.org/docs/18/sql-select.html)。
- `claim(limit)` は呼出側の free dispatch permit 数を受け取らない。`limit <= free_permits`、Search の Source lease 取得後 claim 1 件、dispatch 直前の fence は G05/S02 以降の責務で、G02 からは成立を主張しない。`begin()` は isolation level を明示しないため、今回の PostgreSQL fixture は既定の `READ COMMITTED` に依存する。本番 pool/session の isolation 設定と lock 所要時間は未検証。[PostgreSQL transaction isolation](https://www.postgresql.org/docs/18/transaction-iso.html)。
- `postgres.rs:222-253` の G03 renew/settle と G04 reap は明示的な `StoreUnknown` placeholder。ack、retry、DLQ、crash 回収、未知 commit 後の再照合は未実装・未検証。claim の未知 commit は `StoreUnknown` として返す静的経路を確認したが、障害注入試験は今回の対象にない。これらを含む production completion は宣言しない。
