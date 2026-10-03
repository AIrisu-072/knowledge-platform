# P6-G04 PostgreSQL exhausted reaper 独立レビュー

- 判定: **G04 限定 GO**。対象は `PostgresOutboxStore::reap_exhausted` と `postgres_reaper.rs` の機能契約だけ。2026-09-30 22:27 JST、未コミットの `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` を監査した。
- 根拠: `p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §3、`p6-outbox-plan.md` G04、`p6-domain-migration-recheck.md`、`p6-postgres-settle-review.md`。作成者の判定だけには依存せず、現行 source/test/SQL と実 DB 結果で照合した。コード、spec、task graph、Cargo/lock は変更していない。

## 契約照合

| 条件 | 現行コードと独立確認 |
| --- | --- |
| 六つの policy 値と rollout 排他 | `crates/outbox-delivery/src/postgres.rs:25-38,54-107,349-360`。revision、max attempts、lease min/max、backoff min/max の全値を singleton policy 行と比較し、同一 transaction 内で `FOR SHARE` を保持する。試験 `postgres_reaper.rs:263-306` は六つを個別に変更して `PolicyMismatch` と行不変を確認した。 |
| legacy NULL と行別最終回数 | `postgres.rs:89-107,360,365-368`。未完了で `attempt_limit IS NULL` かつ現 policy の上限到達行を先に `LegacyExhausted` として拒否する。reaper は local max を terminal 判定に使わず、固定済みの行別 limit だけを使う。試験 `postgres_reaper.rs:293-305` は legacy 行を黙って dead-letter 化しない。 |
| DB 時刻、lease、terminal | `postgres.rs:362-377` は `MATERIALIZED` な単一 `clock_timestamp()` 値を expiry 条件と `dead_lettered_at` の両方に使う。未 delivered/dead、固定済みの上限到達、lease 不在または期限到達の行のみを更新する。`postgres_reaper.rs:308-357` は renew 済み最終試行、未上限、delivered/dead 行の完全不変を確認した。 |
| 有限 batch と競合 | `postgres.rs:351-354,369-381` は 1..=32 を検査し、`last_attempt_at NULLS FIRST,event_id` の順で `FOR UPDATE OF o SKIP LOCKED` を用いる。別 pool の二つの子 OS process は barrier 後に同じ期限切れ最終行を回収し、件数 `[0,1]`、再回収 0 (`postgres_reaper.rs:109-261`)。別 transaction が先頭行を lock した試験は 5 秒 timeout 内で次の 32 行を回収し、残り 2 行を次回回収した (`:359-401`)。 |
| 原行と未知結果 | `postgres.rs:373-384` は terminal 時刻、lease 三列、固定 error code だけを変更し、原行を消さず、commit 後に件数を返す。試験 `postgres_reaper.rs:203-260,391-397` は ID/type/aggregate/payload/occurred/available/attempt count/row limit/last attempt の保持、lease 消去、未 ack、再 claim 不可を確認した。begin/SQL/commit の失敗は `StoreUnknown` (`postgres.rs:355-359,379-384`)、閉じた pool でもその error を確認 (`postgres_reaper.rs:402-406`)。 |
| migration の受入 | `0009_outbox_delivery_v0.sql:14-37,39-66` は lease 完全性、一方だけの terminal、`delivery_unknown_at_limit` allowlist、有限 policy bounds、候補 index を定義する。既存 migration の独立 4/4 実 DB 再確認は別 receipt に記録済み。 |

PostgreSQL 18 の公式資料でも、`SKIP LOCKED` は取得できない queue 行を飛ばし、`FOR SHARE` は同じ policy 行の更新を妨げる。materialized CTE は同じ statement 内の参照に一つの結果を与え、`clock_timestamp()` 自体は呼ぶたびに変化するため、その単一評価が必要である。参照: [SELECT](https://www.postgresql.org/docs/18/sql-select.html)、[Explicit Locking](https://www.postgresql.org/docs/18/explicit-locking.html)、[WITH Queries](https://www.postgresql.org/docs/18/queries-with.html)、[Date/Time Functions](https://www.postgresql.org/docs/18/functions-datetime.html)。

## Fresh verification と入力固定

| 対象 | SHA-256 |
| --- | --- |
| `crates/outbox-delivery/src/postgres.rs` | `7956b053616763b933f08a3aca58e06a7797ee18acc8d1a9740d88e2ac53dd74` |
| `crates/outbox-delivery/tests/postgres_reaper.rs` | `c21ed7e782b9cda976be80ca8f9f02cad1e3f9c71d314e45c69a4215d0b10c1d` |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` | `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb` |
| `target/debug/deps/postgres_reaper-ba87f7f683163f68` | `6b82fa410be776d6b46b2c8923809044bffb9b727cc3739eb3037b7ecd8c2c9e` |

- 開始時と試験後の上記四 SHA は同じ。`postgres.rs` mtime 22:13:45、test 21:12:06、SQL 18:39:18、cached executable 22:14:01 JST。`--list` は子 fixture 1、通常 4 test を列挙した。`migrate` は `document-repository-postgres/src/lib.rs:61` の `sqlx::migrate!` を使う。mtime と SHA は現行入力からの再コンパイルを暗号学的に証明しないので、動的結果はこの **記録した cached binary** に限定する。
- Fresh `target/debug/deps/postgres_reaper-ba87f7f683163f68 --test-threads=1 --nocapture`: **4 passed / 0 failed / 1 ignored / exit 0**、test 本体 32.61 秒。実 PostgreSQL 18.6 image ID `sha256:3725f4e2499eef5134592b3b4ab79a543ed7f8e533b05b5b637af926630f6650`、Docker Server 29.4.0。試験所有の使い捨て container 以外は操作していない。同時刻に別 P7 auditor の PostgreSQL container があり得たため、時間を性能測定には用いない。
- Fresh `rustfmt --check --edition 2024 crates/outbox-delivery/src/postgres.rs crates/outbox-delivery/tests/postgres_reaper.rs`: exit 0。I02 が Cargo を排他使用中のため、今回 Cargo 再コンパイル、Clippy、workspace gate は走らせていない。

## 判定境界

G04 は最終 claim 後に失効した行を結果不明の terminal として一度だけ回収する。試験は実 OS 子 process と独立 pool を使うが、実際の worker を kill する G07 restart 試験ではない。reaper の COMMIT 応答喪失 proxy fault も今回の動的試験にはない。コード上の commit error は `StoreUnknown` で、G03 では generic ack の実 COMMIT 応答喪失を別途確認している。これは Search publication commit の証拠ではない。

実 delivery role の `FOR SHARE`/列権限、large-table index/lock 時間、throughput、G05–G07 runner/restart、Search receipt/publish と P7 durable composition、fresh build と hosted exact-head CI はそれぞれ別 gate。次の action は、この限定 GO を generic P6 receipt に取り込み、I02 の Cargo slot 解放後に必要な fresh build を行い、後続の実 role・runner・Search 統合を個別に検証すること。
