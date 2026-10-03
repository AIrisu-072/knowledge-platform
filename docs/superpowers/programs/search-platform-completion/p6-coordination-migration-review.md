# P6-I03 Search coordination migration 独立レビュー

- 判定日時: 2026-09-30 19:24 JST。branch `feat/search-platform-completion-core`、HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット Source を対象とする。
- 判定: **GO（P6-I03 の Search `0001` schema と独立 SQLx migration ledger に限定）**。Domain `0009` の後、同一 PostgreSQL database に一つの Source pointer/receipt namespace を作り、Domain ledger を保持できた。P6-S02/S03、P7 READY・pin・guard・GC、配信 ack、role、production migration の合格判定ではない。
- 入力契約: `p6-outbox-freeze.md:5-10`、`p6-outbox-plan.md:167-174`、`p7-shared-durable-freeze.md:11`、`p6-domain-migration-recheck.md:1-19`。凍結 design SHA `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、plan SHA `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`、P7 design SHA `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` は現行ファイルと一致した。

## 契約との照合

- `crates/search-runtime/src/lib.rs:5-12` は `migrate!` の Search migration に `search_runtime_sqlx_migrations` を設定し、Domain `migrate` は既定 `_sqlx_migrations` のまま (`crates/document-repository-postgres/src/lib.rs:41-61`)。`crates/search-runtime/tests/coordination_migration.rs:200-280` は Domain 9 件の `(version, checksum)` を Search 適用前後で一致させ、Search ledger 1 件の再適用後 checksum も一致させる。Domain `0009` の SQL/test hash は先行独立再確認の記録と一致した。
- `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql:3-37` は `source_id` PK の単一物理 Source 行、`fence_epoch`、owner/expiry、current generation/manifest/bundle、`pointer_revision`、`last_published_epoch`、`build_fence_seq` を保持する。owner/expiry と current 3 列は各々 all-NULL/all-present、counter は非負、published epoch は fence 以下。manifest と bundle は両方 `sha256:` + 64 桁 lowercase hex を要求する。検索した Search migration は `0001` のみで、別の Source pointer は見つからない。P3 の `source_control` はこの行の別名という計画境界に合う。
- `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql:39-59` は receipt の `(source_id,event_id)` PK、Source FK、正の fence epoch、両 digest の形式を要求する。`generation_id` に FK がないのは、履歴 receipt が退役 generation を GC pin しないための意図的な境界で、P7 凍結設計と一致する。
- `crates/search-runtime/tests/coordination_migration.rs:222-265` は同じ pool に Domain outbox 行を先に作り、Source pointer と Search receipt を一つの transaction で記録した後、3 表を join できることを検証する。`284-342` は Source 行が未登録なら条件付き lease UPDATE が 0 行、事前登録後なら default counter 0 から epoch 1、上限到達時は取得なしと確認する。これは直接 SQL の schema/fence 試験であり、登録・lease の公開 adapter 実装を検証したものではない。

## Fresh verification と限界

- 実行: `/tmp/search-completion-preserved-poc-bin/coordination_migration-6b2b83dfa7eeae90 --test-threads=1 --nocapture`。`postgres:18.6-bookworm` を testcontainers で使う独立した disposable PostgreSQL テスト 3 件が **3 passed / 0 failed、exit 0、45.25s**。テスト名は `coordination_schema_rejects_half_lease_and_duplicate_receipt`、`migration_keeps_pointer_and_receipt_same_database`、`source_row_is_preseeded_before_lease`。
- 対象 SHA-256（実行前後一致）: SQL `a9904c78307c5ae39099021569639f28ea08c99f7f65a9f60a51bf89bd1cc5a4`、`src/lib.rs` `81dd87ab19013b00b760003febf4a46a96c57aecc30d992b7fd20ac32241f42e`、test `579617c402c2e7ecd2823a632c0416359505afb3da1ff2c1bcecd185c838218c`。binary SHA-256 は `8c5820764cf9468b7f4a11dd0cd3e763ff02d558f554f5310be5ae5347ffc21d`。binary mtime 18:52:43 は SQL/lib 18:52:27、test 18:42:09 より後で、`--list` は指定 3 件を表示した。容量と build 所有の境界に従い Cargo 再コンパイルは行っていないため、埋込 migration と現行 source hash の build 再現性までは主張しない。
- SQL 制約と直接 SQL 試験は、実装されていない Source admission API、lease 更新・失効、receipt 単調 upsert、READY/current CAS、pin/guard/GC、outbox ack、権限分離を証明しない。P7 `0002+` と P3 Graph migration/READY/publish は別 gate のまま。`p6-domain-migration-recheck.md` の Domain policy GO も generic claim/ack または統合 P6/P7 GO に昇格させない。

次の exact action: P6-S02 の分散 Source lease adapter と P7 `0002+` の共有 durable 基盤を、それぞれの計画・独立レビューで検証する。Search event completion/配信の最終判定は P6-S03 以降と P7 READY/pointer/GC を同一実 PostgreSQL の縦断で確認した後に行う。
