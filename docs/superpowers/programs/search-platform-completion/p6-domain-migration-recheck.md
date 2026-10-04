<a id="p6-i02-domain-migration-policy-bounds-独立再確認"></a>
# P6-I02 Domainマイグレーションとポリシー上限・下限の独立再確認

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-domain-migration-recheck.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 判定日時: 2026-09-30 18:59 JST。ブランチ `feat/search-platform-completion-core`、HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` の未コミットマイグレーション/テストを対象とする。
- 対象 SHA-256: `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` = `6059108aa478df2b5c9af5417e650ce9d661f74e77236fa0e2b8e02e626cf7bb`、`crates/document-repository-postgres/tests/outbox_delivery_migration.rs` = `ab22a4a07abd7ce1cd3adb4ee308bcb7d382e55d1f20ccede73b9b0d72bdda81`。実行後も同一ハッシュを確認した。
- 判定: **GO（0009 マイグレーションと v0 DB ポリシー設定境界に限定）**。先行 `p6-domain-migration-review.md` の P2、絶対上限がない問題は、この SQL と実 PostgreSQL 境界試験で解消した。汎用配送の処理権取得/配送成功確定/期限切れ処理の回収、実行時の起動処理、Search/P7 統合、本番マイグレーションの判定ではない。

## 契約との照合

- `p6-policy-bounds-ruling.md` の `max_attempts` 1..=32、リース最小・最大 1000..=120000 ms、再試行待機最小・最大 1000..=300000 ms、各 `max >= min`、`revision > 0` は SQL `0009:39-51` の CHECK に一致する。初期値 `(1,1,8,1000,120000,1000,300000)` は `0009:53-55` で維持される。`policy_requires_finite_bounds_and_ordered_ranges` (`tests/outbox_delivery_migration.rs:345-388`) は両端の受理と、下限未満・上限超過・逆転・改訂番号 0 の SQLSTATE `23514` を確認する。上限はポリシー行の制約であり、イベント行の `attempt_limit` は従来どおり NULL または正数 (`0009:14-16`) である。
- 旧 Domain 行への追加は NULL 列のみ (`0009:3-12`)。`migration_preserves_legacy_domain_and_audit_rows` (`tests/outbox_delivery_migration.rs:120-273`) は旧待機中・配送済みと上限到達行の全旧列、JSONB、時刻、件数、および Audit 行をマイグレーション前後で比較し、追加列の NULL、旧上限行の非終端維持、旧形式生成側のINSERT、公開 `migrate` からの埋込バージョン 9 を確認する。既存 `0001`–`0008` マイグレーションと `crates/document-repository-postgres/src` は作業木で変更なしと確認した。
- リース全列の同時 NULL/非 NULL、二重の終端状態の禁止、終端行のリース禁止、エラーコードの許可リストは `0009:17-37` の CHECK に残る。`migration_rejects_partial_lease_and_double_terminal` (`tests/outbox_delivery_migration.rs:288-343`) は代表的な違反の SQLSTATE `23514` と制約名を確認する。`processing_state_distinguishes_reclaim_and_reap_pending` (`tests/outbox_delivery_migration.rs:390-444`) は旧上限行を静かにデッドレター化せず、期限切れリースと上限到達の読み取りモデルを区別する。

<a id="fresh-verification"></a>
## 新たな検証

- Docker Server 29.4.0、ローカルキャッシュの `postgres:18.6-bookworm` で、`target/debug/deps/outbox_delivery_migration-ea75b01b56bff1f3 --test-threads=1 --nocapture` を実行。**4 passed、0 failed、exit 0**。テスト名は `migration_preserves_legacy_domain_and_audit_rows`、`migration_rejects_partial_lease_and_double_terminal`、`policy_requires_finite_bounds_and_ordered_ranges`、`processing_state_distinguishes_reclaim_and_reap_pending`。使い捨て PostgreSQL は順次起動し、実行後に稼働中の同イメージコンテナはない。
- 実行バイナリ SHA-256 は `4e2082604023fa186f07a4a98aeed6a8a368f27a8c1302af9ac992b3204ad3a7`。バイナリ更新時刻 18:39:56 はテスト 18:12:44 および SQL 18:39:18 より後で、`--list` は上記 4 件を表示した。今回はビルド実行枠と空き容量の制約から新たなCargoコンパイルは行っていないため、コンパイラによる両ソースハッシュの再現性までは主張しない。SQL はテスト内で現行ファイルを実行時に読む (`tests/outbox_delivery_migration.rs:55-61`)。
- 247 行の合成データの組合せに対する候補/上限到達行の回収処理 `EXPLAIN (ANALYZE, BUFFERS)` は両方 Seq Scan。索引採用、大表での性能、ロック時間、本番 DDL はこの結果で検証していない。

次の P6 全体受入では、別担当の実行時ポリシー検査と処理権取得/期限切れ処理の回収の実 DB 証拠を合わせて判定する。`p6-policy-bounds-fix-receipt.md` はこの再確認時点では未作成であり、本判定は上記の現行ソースと実 DB 結果に基づく。
