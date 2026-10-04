<a id="p6-i03-search-coordination-migration-独立レビュー"></a>
# P6-I03 Search調整基盤のマイグレーション独立レビュー

[固定された公開原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-coordination-migration-review.md)に対応する意味保存の日本語訳です。原設計の再承認、実装・実行時の適格性検証の追加ではありません。既存ハッシュと実行結果は当時の原文・証拠を指し、訳文のハッシュや現在の検証結果ではありません。以下の状態と次の作業は当時の記録です。[最新の実行状態](../../execution/search-platform-completion-program-status.md)を優先してください。

- 判定日時: 2026-09-30 19:24 JST。ブランチ `feat/search-platform-completion-core`、HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット Source を対象とする。
- 判定: **GO（P6-I03 の Search `0001` スキーマと独立 SQLx マイグレーション履歴管理表に限定）**。Domain `0009` の後、同一 PostgreSQL データベースに一つの Sourceポインターとイベント受領記録の名前空間を作り、Domain 履歴管理表を保持できた。P6-S02/S03、P7 READY・固定保持・ガード・GC、配送成功確定、ロール、本番マイグレーションの合格判定ではない。
- 入力契約: `p6-outbox-freeze.md:5-10`、`p6-outbox-plan.md:167-174`、`p7-shared-durable-freeze.md:11`、`p6-domain-migration-recheck.md:1-19`。凍結設計 SHA `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、計画 SHA `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`、P7 設計 SHA `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` は現行ファイルと一致した。

## 契約との照合

- `crates/search-runtime/src/lib.rs:5-12` は `migrate!` の Search マイグレーションに `search_runtime_sqlx_migrations` を設定し、Domain `migrate` は既定 `_sqlx_migrations` のまま (`crates/document-repository-postgres/src/lib.rs:41-61`)。`crates/search-runtime/tests/coordination_migration.rs:200-280` は Domain 9 件の `(version, checksum)` を Search 適用前後で一致させ、Search 履歴管理表 1 件の再適用後チェックサムも一致させる。Domain `0009` の SQL/テストハッシュは先行独立再確認の記録と一致した。
- `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql:3-37` は `source_id` PK の単一物理 Source 行、`fence_epoch`、所有者/有効期限、現在の世代/マニフェスト/バンドル、`pointer_revision`、`last_published_epoch`、`build_fence_seq` を保持する。所有者/有効期限と現在値を表す3列は各々 全列NULL・全列非NULL、カウンターは非負、公開済みエポックはフェンスエポック以下。マニフェストとバンドルは両方 `sha256:` + 64 桁小文字16進数を要求する。検索した Search マイグレーションは `0001` のみで、別の Source ポインターは見つからない。P3 の `source_control` はこの行の別名という計画境界に合う。
- `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql:39-59` はイベント受領記録の `(source_id,event_id)` PK、Source FK、正のフェンスエポック、両ダイジェストの形式を要求する。`generation_id` に FK がないのは、過去のイベント受領記録が退役世代を GC 固定保持しないための意図的な境界で、P7 凍結設計と一致する。
- `crates/search-runtime/tests/coordination_migration.rs:222-265` は同じ接続プールに Domain outbox 行を先に作り、Source ポインターとSearchイベント受領記録を一つのトランザクションで記録した後、3 表を結合できることを検証する。`284-342` は Source 行が未登録なら条件付きリース UPDATE が 0 行、事前登録後なら初期値カウンター 0 からエポック 1、上限到達時は取得なしと確認する。これは直接 SQL のスキーマ/フェンス試験であり、登録・リースの公開アダプター実装を検証したものではない。

<a id="fresh-verification-と限界"></a>
## 新たな検証と限界

- 実行: `/tmp/search-completion-preserved-poc-bin/coordination_migration-6b2b83dfa7eeae90 --test-threads=1 --nocapture`。`postgres:18.6-bookworm` を testcontainers で使う独立した使い捨て PostgreSQL テスト 3 件が **3 passed / 0 failed、exit 0、45.25s**。テスト名は `coordination_schema_rejects_half_lease_and_duplicate_receipt`、`migration_keeps_pointer_and_receipt_same_database`、`source_row_is_preseeded_before_lease`。
- 対象 SHA-256（実行前後一致）: SQL `a9904c78307c5ae39099021569639f28ea08c99f7f65a9f60a51bf89bd1cc5a4`、`src/lib.rs` `81dd87ab19013b00b760003febf4a46a96c57aecc30d992b7fd20ac32241f42e`、テスト `579617c402c2e7ecd2823a632c0416359505afb3da1ff2c1bcecd185c838218c`。バイナリ SHA-256 は `8c5820764cf9468b7f4a11dd0cd3e763ff02d558f554f5310be5ae5347ffc21d`。バイナリ更新時刻 18:52:43 は SQL/lib 18:52:27、テスト 18:42:09 より後で、`--list` は指定 3 件を表示した。容量とビルド所有の境界に従い Cargo 再コンパイルは行っていないため、埋込マイグレーションと現行ソースハッシュのビルド再現性までは主張しない。
- SQL 制約と直接 SQL 試験は、実装されていない Sourceの受入API、リース更新・失効、イベント受領記録の単調な追加または更新、READY/現在値 CAS、固定保持/ガード/GC、outbox 配送成功確定、権限分離を証明しない。P7 `0002+` と P3 Graph マイグレーション/READY/公開は別検証ゲートのまま。`p6-domain-migration-recheck.md` の Domain ポリシー GO も汎用処理権取得/配送成功確定または統合 P6/P7 GO に昇格させない。

当時の次の具体的な作業: P6-S02 の分散 Sourceリースのアダプターと P7 `0002+` の共有永続基盤を、それぞれの計画・独立レビューで検証する。Search イベント完了/配信の最終判定は P6-S03 以降と P7 READY/ポインター/GC を同一実 PostgreSQL の縦断で確認した後に行う。
