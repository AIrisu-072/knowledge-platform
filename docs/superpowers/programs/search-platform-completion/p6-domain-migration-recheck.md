# P6-I02 Domain migration policy bounds 独立再確認

- 判定日時: 2026-09-30 18:59 JST。branch `feat/search-platform-completion-core`、HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット migration/test を対象とする。
- 対象 SHA-256: `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` = `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb`、`crates/document-repository-postgres/tests/outbox_delivery_migration.rs` = `ab22a4a07abd7ce1cd3adb4ee308bcb7d382e55d1f20ccede73b9b0d72bdda81`。実行後も同一ハッシュを確認した。
- 判定: **GO（0009 migration と v0 DB policy 設定境界に限定）**。先行 `p6-domain-migration-review.md` の P2、絶対上限がない問題は、この SQL と実 PostgreSQL 境界試験で解消した。generic delivery の claim/ack/reap、runtime startup、Search/P7 統合、本番 migration の判定ではない。

## 契約との照合

- `p6-policy-bounds-ruling.md` の `max_attempts` 1..=32、lease min/max 1000..=120000 ms、backoff min/max 1000..=300000 ms、各 `max >= min`、`revision > 0` は SQL `0009:39-51` の CHECK に一致する。seed `(1,1,8,1000,120000,1000,300000)` は `0009:53-55` で維持される。`policy_requires_finite_bounds_and_ordered_ranges` (`tests/outbox_delivery_migration.rs:345-388`) は両端の受理と、下限未満・上限超過・逆転・revision 0 の SQLSTATE `23514` を確認する。上限は policy 行の制約であり、イベント行の `attempt_limit` は従来どおり NULL または正数 (`0009:14-16`) である。
- 旧 Domain 行への追加は NULL 列のみ (`0009:3-12`)。`migration_preserves_legacy_domain_and_audit_rows` (`tests/outbox_delivery_migration.rs:120-273`) は旧 pending/delivered と上限到達行の全旧列、JSONB、時刻、件数、および Audit 行を migration 前後で比較し、追加列の NULL、旧上限行の非終端維持、旧形式 producer insert、公開 `migrate` からの embedded version 9 を確認する。既存 `0001`–`0008` migration と `crates/document-repository-postgres/src` は作業木で変更なしと確認した。
- lease 全列の同時 NULL/非 NULL、二重 terminal の禁止、terminal 行の lease 禁止、error code allowlist は `0009:17-37` の CHECK に残る。`migration_rejects_partial_lease_and_double_terminal` (`tests/outbox_delivery_migration.rs:288-343`) は代表的な違反の SQLSTATE `23514` と制約名を確認する。`processing_state_distinguishes_reclaim_and_reap_pending` (`tests/outbox_delivery_migration.rs:390-444`) は旧上限行を静かに dead-letter 化せず、期限切れ lease と上限到達の read model を区別する。

## Fresh verification

- Docker Server 29.4.0、ローカル cache の `postgres:18.6-bookworm` で、`target/debug/deps/outbox_delivery_migration-ea75b01b56bff1f3 --test-threads=1 --nocapture` を実行。**4 passed、0 failed、exit 0**。テスト名は `migration_preserves_legacy_domain_and_audit_rows`、`migration_rejects_partial_lease_and_double_terminal`、`policy_requires_finite_bounds_and_ordered_ranges`、`processing_state_distinguishes_reclaim_and_reap_pending`。使い捨て PostgreSQL は順次起動し、実行後に稼働中の同 image container はない。
- 実行バイナリ SHA-256 は `4e2082604023fa186f07a4a98aeed6a8a368f27a8c1302af9ac992b3204ad3a7`。バイナリ更新時刻 18:39:56 は test 18:12:44 および SQL 18:39:18 より後で、`--list` は上記 4 件を表示した。今回は build slot と空き容量の制約から fresh Cargo compile は行っていないため、コンパイラによる両 source hash の再現性までは主張しない。SQL はテスト内で現行ファイルを実行時に読む (`tests/outbox_delivery_migration.rs:55-61`)。
- 247 行の合成 mix に対する候補/reaper `EXPLAIN (ANALYZE, BUFFERS)` は両方 Seq Scan。索引採用、大表での性能、lock 時間、本番 DDL はこの結果で検証していない。

次の P6 全体受入では、別担当の runtime policy guard と claim/reap の実 DB 証拠を合わせて判定する。`p6-policy-bounds-fix-receipt.md` はこの再確認時点では未作成であり、本判定は上記の現行 source と実 DB 結果に基づく。
