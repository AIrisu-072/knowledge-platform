# P7-01 Source ownership migration 実装 receipt

- 状態: **P7-01 局所実装・実 PostgreSQL focused RED/GREEN 完了。独立 read-only review 待ち。** P7 全体、production factory、READY、実 role の受入判定ではない。
- Branch / 作業時 HEAD: `feat/search-platform-completion-core` / `80a47960d025e4dfdea1eacade28b15d218725ff`（未コミット作業木）。本 receipt はコード・テストを再変更せず、既に観測した実行結果を記録した。
- 入力: `p7-shared-durable-freeze.md` SHA-256 `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110`、改訂 1 `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe`、最終実装 plan `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517`。P6-I03 Search `0001` の独立レビューは `p6-coordination-migration-review.md` の局所 GO に限る。

## 実装範囲

- `crates/search-runtime/migrations/0002_search_source_ownership_v1.sql`: 全 tenant 共通 `SourceId` の owner/kind 不変性、Document/Remote 別 desired-set revision/digest、Source 行と ownership 行の遅延整合検査、revision/activation 単調性、owner key・DTO version/bytes・digest の上限と形式、receipt の明示 `bundle_version` を追加した。既存 Source/current/receipt の owner/kind/revision/generation mapping は推測しない。
- legacy Source がある場合、trusted host の durable authority から与えた `search_legacy_source_backfill_proof` と `search_legacy_generation_backfill_proof` を同一 DB で要求する。全 Source と current・歴史的 receipt の参照 generation key を照合し、欠落・矛盾は migration transaction を拒否する。成功時も旧 pointer/receipt と旧 receipt の未知 `bundle_version=NULL` を保持する。この二 proof table の作成・証拠導出は host 側の明示 backfill protocol であり、本 task は host authority を実装していない。
- `crates/search-runtime/tests/source_ownership_migration.rs`: 六つの独立した実 DB 試験を追加。`crates/search-runtime/tests/coordination_migration.rs`: P6-I03 の三試験で 0002 適用後も元の Source/pointer/receipt 意味論を確認するため、owner 登録 fixture と Search ledger 2 件の期待値だけを更新した。

## RED → GREEN と fixture

- fixture は `testcontainers` の `postgres:18.6-bookworm`、各 test の disposable container / `source_ownership_migration_test` database / `PgPool`。Domain migration と Search migration を実 SQLx `migrate` 入口で実行し、必要な反例では `0001` だけを `search_runtime_sqlx_migrations` に先行適用した。試験接続 role は `postgres`。image の content digest や production role grant はこの fixture では検証していない。
- 0002 実装前: `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-runtime --locked --test source_ownership_migration -- --test-threads=1` は未作成の表・列・ledger・guard に対して意図どおり **6/6 RED**、exit 101。
- 0002 初回後、6 件中 1 件は期待 SQLSTATE と実 FK SQLSTATE の違いで失敗し、期待値を修正した。さらに `registration_revision` と Source 側 revision を同時に増やし、`activation_epoch` を据え置けた反例を追加すると targeted test が **RED 1/1**（成功してはならない UPDATE が成功、exit 101）。owner trigger に registration revision の activation 条件を加え、即時拒否の期待値を合わせて再確認した。
- 同じ focused command を新 migration の埋込再ビルド後に再実行し、`source_ownership_migration` **6 passed / 0 failed、exit 0、50.24s**。Domain `_sqlx_migrations` の 9 件 `(version,checksum)` は前後で一致し、Search `search_runtime_sqlx_migrations` は `0001`,`0002` の順の 2 件、`0001` checksum は先行値と一致、再適用でも両 checksum が一致した。Document/Remote の revision/digest は独立に進み、片側 NULL/逆行/同 revision 別 digest は拒否された。
- 同試験は owner key/kind/DTO/digest の不正、tombstone 後の owner/kind/SourceId UPDATE と ownership DELETE、Source と ownership の片側変更、activation 据置更新、新 receipt の version 欠落・旧 receipt の version 推測を拒否した。証明なし旧 current/receipt は `0002` が失敗し、pointer・receipt・ledger `0001` のみ・ownership 表不在を保持した。別 current と歴史 receipt の両 generation key と owner 一致が揃った明示 proof だけが成功した。
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-runtime --locked --test coordination_migration -- --test-threads=1`: **3 passed / 0 failed、exit 0、9.62s**。`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo clippy -p search-runtime --locked --all-targets -- -D warnings`: **exit 0、36.45s**。対象二 Rust ファイルの `rustfmt --check --edition 2024` は PASS。`cargo fmt --all -- --check` は並行作成中の別 crate `crates/search-extraction-worker/src/main.rs` 不在で exit 1 となり、workspace 全体の fmt 成功は主張しない。

## 現行入力 SHA-256 と残る gate

| 対象 | SHA-256 |
| --- | --- |
| Search `0001`（不変） | `a9904c78307c5ae39099021569639f28ea08c99f7f65a9f60a51bf89bd1cc5a4` |
| `search-runtime/src/lib.rs`（不変、公開 `migrate` 入口） | `81dd87ab19013b00b760003febf4a46a96c57aecc30d992b7fd20ac32241f42e` |
| Search `0002` | `a2abe81f2233267ff909af629f6571f68c1af36e52fd08670cf1f75daf8ff1ca` |
| `tests/source_ownership_migration.rs` | `29c4fcf84502113c5a153d576ec1865ce8ec5bdc897585244422d99bd6f83b41` |
| `tests/coordination_migration.rs` | `8a8150f93ffbd3756f3c978204049ebba5c2b33197abe0ecad29217da9fdaa51` |

P7-02 の source-neutral durable reconcile / `is_current` と trusted composition root の complete Document・Remote namespace 初期化、P7-03 の実 role/grant、P7-12 の startup ownership/current scan はそれぞれ別 gate。現在の `search-runtime::migrate` に 0002 が含まれることは、production factory が未証明 legacy Source や片 namespace の状態で API を開かないことの検証にはならない。P3 native backend 選定と二 canonical encoder の GO まで Graph/P7 READY・publish・durable pin は閉じる。独立 read-only review、統合・exact-head hosted gate、本番 migration、commit・push・merge・deploy は未実施。

次の exact action: 独立 reviewer が本 SHA の SQL/test に対し legacy proof の完全性、trigger/SQL bypass、checksum 分離、factory/startup 未接続境界を確認する。
