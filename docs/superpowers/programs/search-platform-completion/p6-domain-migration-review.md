# P6-I02 Domain migration 独立レビュー

- 対象: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット `0009_outbox_delivery_v0.sql` と `outbox_delivery_migration.rs`。各 SHA-256 は `31c566da7c7a67d7713bb59480d056ba8dc75591114951f10f66dfef278f7435`、`5811ed5ce60fe2dccee1d18c996a9ffd24287bbb131f5bf5202a65711ea563a1`。
- 根拠: `p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §2–3、`p6-outbox-plan.md` P6-I02、`spec/data/transaction-consistency-requirements-v0.md` §7。
- 判定: **静的レビューは下記 1 件の要修正・要決定あり。実 PostgreSQL qualification は未確認**。generic worker と Search/P7 統合の受入判定ではない。

## 指摘

1. **P2 — policy 上限が DB で bounded ではない。** `0009_outbox_delivery_v0.sql:39-47` の CHECK は `max_attempts > 0`、`lease_min_ms/backoff_min_ms > 0` と `max >= min` のみ。`lease_max_ms`、`backoff_max_ms`、`max_attempts` に v0 の絶対上限はなく、例えば `max_attempts=2147483647`、`lease_max_ms=9223372036854775807` を許す。`p6-outbox-plan.md:81` は「positive/bounded CHECKs」を要求し、現行 `DeliveryConfig::validate` は実使用 lease を 120 秒以下に制限する。DB policy の変更時に runtime validation との境界が曖昧になる。v0 の DB 上限を設計上確定して CHECK と境界テストに反映するか、上限は runtime 設定だけで守る契約へ計画を明示的に修正すること。

## 確認できた点

- `0009_outbox_delivery_v0.sql:3-12` は旧表への NULL 列追加であり、既存列・PK・producer insert を変更しない。`tests/outbox_delivery_migration.rs:120-273` は旧 pending/delivered、旧上限行、JSONB/時刻、Audit 行、producer 互換、embedded SQLx migrator が version 9 を検出することを確認する構成。
- `0009_outbox_delivery_v0.sql:14-37` の lease 完全性と terminal 排他は `IS NULL` / `IS NOT NULL` による真偽値で、NULL の 3 値論理に抜けはない。各 lease 列の 3 個同時 NULL または 3 個同時非 NULL のみ許し、terminal 行には lease を残せない。allowlist 外 error code も拒否する。`tests/outbox_delivery_migration.rs:288-343` は代表組合せを検証するが、全 8 lease 組合せと全 terminal 組合せの網羅試験ではない。
- `0009_outbox_delivery_v0.sql:54-62` は候補優先順 `(available_at,occurred_at,event_id)` と上限到達 reaper 用の partial index を追加する。`64-79` の view は単一 DB tick に対して `DELIVERED` → `DEAD_LETTER` → 有効 lease の `IN_FLIGHT` → `PENDING` を導出し、上限到達・lease 失効を `recovery_pending=true` とする。旧 `attempt_limit IS NULL` の上限行は view 上 `PENDING,false` のまま残し、起動・claim・reap 拒否は P6-G02 の責務となる。最終試行後の結果不明 terminal code `delivery_unknown_at_limit` は許可リストにある。
- `tests/outbox_delivery_migration.rs:401-432` は合成 240 行の `EXPLAIN (ANALYZE, BUFFERS)` を出力するが、索引採用や大表での lock 時間・実クエリの性能を合格条件として検証しない。大表での `CREATE INDEX CONCURRENTLY` は計画上、別 rollout step。

## fresh verification

`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p document-repository-postgres --test outbox_delivery_migration -- --nocapture` を同じ target cache で実行。build は 0.42 秒で再利用されたが、3 件とも `tests/outbox_delivery_migration.rs:34` の testcontainers PostgreSQL 起動時に `WaitContainer(WaitLog(Io(...error reading a body from connection)))` で失敗し、migration assertion には到達していない。その後 Docker API socket `~/.orbstack/run/docker.sock` も不在。実 DB の RED/GREEN、索引 plan、保存不変性はこのレビューでは未検証。Docker 復旧後に同じ対象 3 テストを一度再実行して証拠を残すこと。
