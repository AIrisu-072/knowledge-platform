<a id="p6-i02-domain-migration-独立レビュー"></a>
# P6-I02 Domainマイグレーションの独立レビュー

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-domain-migration-review.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 対象: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット `0009_outbox_delivery_v0.sql` と `outbox_delivery_migration.rs`。各 SHA-256 は `31c566da7c7a67d7713bb59480d056ba8dc75591114951f10f66dfef278f7435`、`5811ed5ce60fe2dccee1d18c996a9ffd24287bbb131f5bf5202a65711ea563a1`。
- 根拠: `p6-outbox-freeze.md`、`p6-outbox-design-revision-1.md` §2–3、`p6-outbox-plan.md` P6-I02、`spec/data/transaction-consistency-requirements-v0.md` §7。
- 判定: **静的レビューは下記 1 件の要修正・要決定あり。実 PostgreSQL 適格性検証は未確認**。汎用ワーカーと Search/P7 統合の受入判定ではない。

## 指摘

1. **P2 — DBでポリシーに有限な上限が設けられていない。** `0009_outbox_delivery_v0.sql:39-47` の CHECK は `max_attempts > 0`、`lease_min_ms/backoff_min_ms > 0` と `max >= min` のみ。`lease_max_ms`、`backoff_max_ms`、`max_attempts` に v0 の絶対上限はなく、例えば `max_attempts=2147483647`、`lease_max_ms=9223372036854775807` を許す。`p6-outbox-plan.md:81` は「正値かつ有限な上下限を持つCHECK制約」を要求し、現行 `DeliveryConfig::validate` は実使用リースを 120 秒以下に制限する。DB ポリシーの変更時に実行時検証との境界が曖昧になる。v0 の DB 上限を設計上確定して CHECK と境界テストに反映するか、上限は実行時設定だけで守る契約へ計画を明示的に修正すること。

## 確認できた点

- `0009_outbox_delivery_v0.sql:3-12` は旧表への NULL 列追加であり、既存列・PK・生成側のINSERT を変更しない。`tests/outbox_delivery_migration.rs:120-273` は旧待機中・配送済み、旧上限行、JSONB/時刻、Audit 行、生成側互換、埋込SQLxマイグレーターがバージョン9 を検出することを確認する構成。
- `0009_outbox_delivery_v0.sql:14-37` のリース完全性と終端状態の排他は `IS NULL` / `IS NOT NULL` による真偽値で、NULL の 3 値論理に抜けはない。各リース列の 3 個同時 NULL または 3 個同時非 NULL のみ許し、終端行にはリースを残せない。許可リスト外エラーコードも拒否する。`tests/outbox_delivery_migration.rs:288-343` は代表組合せを検証するが、全 8 リース組合せと全終端状態組合せの網羅試験ではない。
- `0009_outbox_delivery_v0.sql:54-62` は候補優先順 `(available_at,occurred_at,event_id)` と上限到達行の回収処理用の部分索引を追加する。`64-79` のビューは単一 DBで取得した時刻に対して `DELIVERED` → `DEAD_LETTER` → 有効リースの `IN_FLIGHT` → `PENDING` を導出し、上限到達・リース失効を `recovery_pending=true` とする。旧 `attempt_limit IS NULL` の上限行はビュー上 `PENDING,false` のまま残し、起動・処理権取得・期限切れ処理の回収拒否は P6-G02 の責務となる。最終試行後の結果不明終端コード `delivery_unknown_at_limit` は許可リストにある。
- `tests/outbox_delivery_migration.rs:401-432` は合成 240 行の `EXPLAIN (ANALYZE, BUFFERS)` を出力するが、索引採用や大表でのロック時間・実クエリの性能を合格条件として検証しない。大表での `CREATE INDEX CONCURRENTLY` は計画上、別展開手順。

<a id="fresh-verification"></a>
## 新たな検証

`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p document-repository-postgres --test outbox_delivery_migration -- --nocapture` を同じビルド出力のキャッシュで実行。ビルドは 0.42 秒で再利用されたが、3 件とも `tests/outbox_delivery_migration.rs:34` の testcontainers PostgreSQL 起動時に `WaitContainer(WaitLog(Io(...error reading a body from connection)))` で失敗し、マイグレーションのアサーションには到達していない。その後 Docker APIソケット `~/.orbstack/run/docker.sock` も不在。実 DB の RED/GREEN、索引計画、保存不変性はこのレビューでは未検証。当時の次の作業は、Docker復旧後に同じ対象3テストを一度再実行して証拠を残すことだった。現在の再実行指示ではない。
