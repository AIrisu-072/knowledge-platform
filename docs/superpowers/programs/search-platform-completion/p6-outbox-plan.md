<a id="p6-durable-outbox-delivery-implementation-plan"></a>
# P6 永続 Outbox 配送の実装計画

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

[公開原文の固定リンク（commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c`）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-plan.md)

本書は意味保存の日本語訳であり、原設計の再承認・実装/資格の追加ではない。既存ハッシュは当時の原文/証拠のものであり、訳文のハッシュではない。以下の状態・予定・合否は当時の記録として保持する。本文の行番号は同 commit の各原文を指し、従来の見出しアンカーも維持している。掲載コマンドは今回の翻訳作業では実行していない。

用語: Outbox は配送待ちイベントの永続表、Source は検索対象の情報源。リースは期限付き処理権、フェンスは古い所有者の更新を拒否する条件、エポックは所有権の世代番号、Search のイベント処理記録（receipt）は索引処理結果の記録、検証の receipt は試験・実行結果の証拠記録を指す。CAS は期待する現値との一致を条件にした更新、GC は不要世代の回収、DLQ は打切りイベントの保管先。Search / Domain / Document / Folder / AccessPolicy / Projection / Unit / Graph などの構成名・型名は識別のため維持する。

> **自律作業エージェント向け:** 必須のサブスキルとして `superpowers:subagent-driven-development` または `superpowers:executing-plans` を使い、タスクごとに進める。各タスクには対象を限定した RED/GREEN サイクルと独立レビューのゲートがある。Completion Program（完成プログラム）は実装を既に許可しており、途中で人間の承認を求める段階は追加しない。

**目的:** コミット済み Domain Outbox のイベントを汎用ワーカーが少なくとも一度配送し、Search の Source フェンス・条件付き公開・永続処理記録と整合させる。

**アーキテクチャ:** `outbox-delivery` は配送状態だけを所有し、短い PostgreSQL 操作による処理権取得・リース更新・状態確定・回収と上限付きランナーを提供する。Search 接続処理は処理権取得前に P7 の分散 Source リースを取得し、不可視の準備領域への書込み後に Outbox フェンスと Source フェンスを同じ DB トランザクションで検査してポインターと処理記録を確定する。汎用配送確認はそのコミットを確認した後だけ実行する。

**技術構成:** Rust 2024 / 1.98、既存ワークスペースの SQLx 0.9、Tokio 1、PostgreSQL 18.6 フィクスチャ（最低 14）、testcontainers 0.28。

**仕様:** [凍結記録の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-freeze.md)（[日本語訳](p6-outbox-freeze.md)）、[正確な改訂設計の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design-revision-1.md)の SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`（[日本語訳](p6-outbox-design-revision-1.md)）、[独立 GO 判定の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-architecture-recheck.md)の SHA-256 `9728fffa22b501ada2f408735a3a325b592a2ba63b4d808e7ac800a5649be88c`（[日本語訳](p6-outbox-architecture-recheck.md)）。元設計 §3, §6–7 と `spec/data/transaction-consistency-requirements-v0.md`、`spec/operations/observability-audit-requirements-v0.md` も読む。

<a id="global-constraints"></a>
## 全体の制約

- `outbox_events` のイベント同一性、ペイロード、既存行、`delivered_at` と生成側の挿入を保持する。`audit_outbox_events` は別経路。`aggregate_version` を異種ペイロードから推測して埋めない。
- 汎用クレートは Search / P7 クレートをインポートしない。汎用配送処理だけが `attempt_count`、`available_at`、リース、DLQ、`delivered_at` を更新する。Search はこの表を配送確認しない。
- Search v0 は一つの設定済み Document `SourceId`、Source 当たり全プロセスで整合化 1 件、Source リース **取得後**に処理権取得 1 件。待機プロセスは処理権を取得しない。汎用バッチ上限は 32、処理中上限は 8。
- DB 方針は改訂と `max_attempts`、リース/再試行待機範囲を一行で固定する。初回処理権取得が行の `attempt_limit` を固定する。方針不一致または旧 `attempt_count >= max_attempts AND attempt_limit IS NULL` は起動/処理権取得/回収を安全側に拒否する。
- `READ COMMITTED` の短いトランザクション、SQL 文内一回の `clock_timestamp()`、`FOR UPDATE SKIP LOCKED` を使う。処理中に Domain 行ロックを保持しない。0 行のフェンスは `Lost`、DB エラー/コミット応答不明は `Unknown` として区別する。
- Search のイベント公開は Outbox 行 → `search_source_coordination` Source 行 → 索引世代キー順 → P3 構築保護 → 評価リース → Search 処理記録の順にロックする。手動再構築は Outbox 行を持たず Source から始める。P3/P7 共有 `source_control` は物理表 `search_source_coordination` に対応し、同じ `pointer_revision` / `build_fence_seq` 名前空間を使う。P3 の保留中の増分構築保護は別契約として維持する。
- Search ポインター、処理記録、Source リース、Domain Outbox は v0 で同一 PostgreSQL データベース。Projection/Unit/網羅性/字句索引/Graph は同じマニフェストと P1 バージョン付きバンドル処理記録の永続化済み READY を CAS 前に確認し、準備領域はイベント/エポックに束縛する。現在の索引世代 / 評価用保持 / 有効構築保護を後片付けが削除しない。期限切れ保護の未公開の対象は Source→ソート済み索引世代→保護→評価リースをロックして `DELETING`、**保護 DELETE、対象の子行 DELETE、対象の索引世代 DELETE** を同一トランザクションで行う（P3 外部キー制約に安全な修正）。
- `Published`、`Unchanged`、`Duplicate` は二つのフェンスと現在の索引世代の READY/ダイジェストと処理記録の同一トランザクション成功が必要。`Ignored` は v0 の何もしないことを明示した経路がないため成功にしない。未知の Document イベントは打切り、Source/DB/コンシューマー応答不明は再試行可能/結果不明。
- 資格確認用の初期値は `max_attempts=8`、リース 120 s、リース更新 30 s、再試行待機 1–300 s、総処理 15 min、終了待機 30 s。いずれも製品 SLO ではない。資格付けで値を変える場合は DB 方針改訂と試験を揃える。
- `sqlx.workspace` / `tokio.workspace` / `testcontainers.workspace` など既存のバージョン固定を再利用し、未認定の新依存を入れない。`tokio` の `signal` 機能追加は共有編集担当が単独で行う。API は [SQLx 0.9 トランザクション](https://docs.rs/sqlx/0.9.0/sqlx/struct.Transaction.html) と [PostgreSQL 行ロック](https://www.postgresql.org/docs/current/explicit-locking.html) に合わせる。
- Draft PR まで。マージ、デプロイ、本番マイグレーションは含めない。1 タスクの RED/GREEN と読み取り専用レビューを終えてから次に進み、最終検証は一度の対象を限定したゲートと正確な head に対するホステッドゲートに分ける。

<a id="file-and-ownership-map"></a>
## ファイルと編集責任の対応

| ファイル / 範囲 | 責務 / 唯一の編集担当 |
| --- | --- |
| `crates/outbox-delivery/Cargo.toml`, `src/lib.rs`; `crates/search-runtime/Cargo.toml`, `src/lib.rs` | `P6-G00` がパッケージファイルだけを作る。ルート Cargo 編集担当はパッケージ定義ファイルの存在を確認してから登録する。 |
| ルート `Cargo.toml`, `Cargo.lock` | `P6-I01` だけがワークスペース構成メンバー / Tokio signal 機能 / ロックを編集する専用タスク。P3/P7 の Cargo 変更と同時に書かず、後続の P7 編集担当に引き継ぐ。 |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql`, `tests/outbox_delivery_migration.rs` | `P6-I02` だけが Domain マイグレーションを作成。`0001`–`0008` と生成側は変更しない。 |
| `crates/outbox-delivery/src/{lib,model,policy,postgres,runner,observe}.rs`, `tests/*` | `P6-G01`–`G08` が順番に各ファイルの当該責務を実装。Search 型は入れない。 |
| `crates/search-application/src/indexing_service.rs`, `src/ports.rs`, `tests/port_contract.rs` | `P6-S01` が SQLx 非依存の型とポートを一度に確定。P3/P7 のポート編集と直列化。 |
| `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql`, `src/{source_lease,event_completion}.rs` | `P6-S02`/`S03` は P7 所有の共有 Source 制御/処理記録基盤に接続する実装境界。スキーマファイルの編集担当は `P6-I03` だけ。P7 のポインター / 保持 / GC 編集担当と直列化。P7 草案の `current_bundle_digest` を同じ Source 行に置く。 |
| `crates/search-source-document/src/{outbox,delivery}.rs`, `src/lib.rs`, `tests/outbox_delivery.rs` | `P6-S04`/`S05`。既存メモリ D4 経路を保持し、フェンス付きポートは完了処理の模擬実装で単体検証。 |
| `crates/search-runtime/tests/{outbox_delivery,source_process_recovery}.rs` | `P6-S06`/`S07` の実 P7 永続実行基盤縦断。`search-source-document` へ逆向き開発用依存を加えない。 |
| `crates/search-runtime/src/bin/search_outbox_worker.rs`, `crates/outbox-delivery/sql/least_privilege.sql` | `P6-I04` の構成 / ロールタスク。P7 永続実行基盤の完成後に接続する。 |
| 上記二つの規範上の `spec/` ファイル | `P6-N01` の単独の編集担当。物理列と読み取りモデル、少なくとも一度 / 順序非保証、トレース範囲を整合させる。 |

`P6-I02` と `P6-I03` のマイグレーションは別の移行元 / 連番であり、適用順は Domain `0009` → Search `0001`。Search `0001` は Domain のバージョン 1 と衝突しないよう SQLx Migrator の独立台帳 `search_runtime_sqlx_migrations` で実行し、その台帳を実 DB テストで検証する。Search 実行基盤クレートの `migrate(&PgPool)` はこの `0001` を含む単一正本とし、P7 が続ける番号は `0002` 以降。P3 Graph マイグレーションは別 `search_graph` スキーマ / 台帳で、P6 は編集しない。P6-S03/S04 の GREEN は P7 永続化済み READY / ポインター / 保護付き GC ポートが接続された後に成立する。汎用 P6 の GREEN と P6+P7 の本番統合は別の検証記録にする。

<a id="common-task-protocol"></a>
### 共通のタスク手順
各タスクは次の順で行う: (1) 記載したテスト名と検証条件を作る、(2) 記載した `cargo test ... -- --exact` を実行し意図した RED を保存、(3) インターフェースの最小実装、(4) 同じコマンドの GREEN と対象厳格な Clippy / fmt、(5) 差分を独立読み取り専用レビュー担当に渡す。testcontainers フィクスチャは既存 `postgres:18.6-bookworm` と `document_repository_postgres::migrate` を使い、実 DB の失敗を単体模擬実装の成功で代替しない。タスクごとのブランチへのコミットはレビュー担当 GREEN 後に行い、PR は Draft のままにする。

<a id="review-focus"></a>
## レビューの重点
1. 旧保留行で `attempt_limit=NULL` かつ上限到達: 起動を拒否し行 ID/件数を監査できることを `P6-G02` で固定する。
2. 処理権取得コミット後、ハンドラー開始前の期限切れ: 試行を待機だけで消費せず、ハンドラー未呼出を `P6-G05` で固定する。
3. Source 所有者の応答不明と待機側競合: 待機側は処理権を先取りせず、旧エポックが公開不能なことを `P6-S02` / `S07` で固定する。
4. GC 済み処理記録と同一ダイジェストの現行 Source: 過去の処理記録を保持用の固定参照にせず、現在の索引世代の READY/キー/ダイジェストを照合して新エポックの処理記録に収束することを `P6-S03` / `S07` で固定する。
5. Search 公開コミットと汎用配送確認コミットの各応答喪失: 再読で確認できるまで成功を推定せず、再配送で二重可視公開がないことを `P6-S07` で固定する。

---

<a id="p6-g00--crate-manifests-before-workspace-registration約-15-分"></a>
### P6-G00 — ワークスペース登録前のクレート定義ファイル作成（約 15 分）

**ファイル:** `crates/outbox-delivery/Cargo.toml`、`src/lib.rs`、`crates/search-runtime/Cargo.toml`、`src/lib.rs` だけを作成する。`outbox-delivery` は資格確認済みのワークスペース依存 `serde_json`、`sha2`、`thiserror`、`time`、`tokio`、`sqlx`、`uuid` と開発用依存 `testcontainers` を使う。`search-runtime` は `outbox-delivery`、`search-application`、`search-source-document`、`sqlx`、`tokio`、`uuid`、`time` に依存してよいが、`search-source-document` からの逆方向の依存は禁止する。両ライブラリの先頭は `#![forbid(unsafe_code)]` とする。

**インターフェース:** 定義ファイルのパッケージ名は `outbox-delivery` と `search-runtime`。ルートの構成メンバーを変更する前に、ライブラリのエントリーポイントを用意する。実際のモジュールは G01 と I03 で追加する。

- [ ] RED: 作成前には `test -f crates/outbox-delivery/Cargo.toml && test -f crates/search-runtime/Cargo.toml` が失敗する。
- [ ] GREEN: 定義ファイルとエントリーポイントを作成し、同じコマンドと `python3 -c 'import pathlib,tomllib; [tomllib.loads(pathlib.Path(p).read_text()) for p in ["crates/outbox-delivery/Cargo.toml","crates/search-runtime/Cargo.toml"]]'` が成功する。依存方向をレビューする。コンパイルは I01 の後に行う。

<a id="p6-i01--root-workspace-registrationshared-root-only-writer約-15-分"></a>
### P6-I01 — ルートのワークスペース登録（共有ルート専任の編集担当、約 15 分）

**ファイル:** ルートの `Cargo.toml`、`Cargo.lock` だけを変更する。G00 の二つの定義ファイルが存在することが前提。二つのワークスペース構成メンバーと `tokio` の `signal` 機能を追加し、それ以外の既存のバージョン固定と機能は維持する。

**インターフェース:** G00 の `lib.rs` は空の安全なエントリーポイント。`model`、`policy`、`postgres`、`runner`、`observe` と Search の `migrate`、`source_lease`、`event_completion` は後続タスクがファイル作成と同時に公開する将来の契約であり、I01 の Cargo check 時点では未実装モジュールを宣言しない。SQLx の型を `search-application` へ持ち込まない。

- [ ] RED: ワークスペースのパッケージが存在しないため `cargo check --locked -p outbox-delivery -p search-runtime` が失敗する。
- [ ] GREEN: 構成メンバーの宣言を追加し、Cargo でロックファイルを再生成する。同じコマンドと `cargo metadata --locked --format-version 1` が成功する。レジストリ由来の新しいパッケージや依存の循環がないことを確認する。
- [ ] レビュー: ルートの Cargo/ロックを編集するのはこのタスクだけ。P3/P7 の共有変更はこの後に順番待ちさせる。

<a id="p6-i02--additive-domain-schema-and-migration-preservationshared-writer約-15-分"></a>
### P6-I02 — 追加型 Domain スキーマと移行時のデータ保持（共有編集担当、約 15 分）

**ファイル:** `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql`、`crates/document-repository-postgres/tests/outbox_delivery_migration.rs` を作成する。既存の `src/lib.rs::migrate` を、埋め込み SQLx マイグレーションのエントリーポイントとして維持する。

**インターフェース:** 列 `lease_token UUID`、`lease_owner UUID`、`lease_expires_at TIMESTAMPTZ`、`last_attempt_at TIMESTAMPTZ`、`dead_lettered_at TIMESTAMPTZ`、`last_error_code TEXT`、`attempt_limit INTEGER`、`traceparent TEXT`、`tracestate TEXT` は、すべて既定値を NULL とする。凍結済みの四つの CHECK に `last_error_code IS NULL OR last_error_code IN ('unsupported_event','invalid_envelope','source_unavailable','indexing_failed','handler_timeout','delivery_unknown','delivery_unknown_at_limit')` を追加する。候補選択用の部分インデックスは `(available_at,occurred_at,event_id) WHERE delivered_at IS NULL AND dead_lettered_at IS NULL` とし、期限切れ・回収処理の候補用インデックスも設ける。`outbox_delivery_policy(policy_id SMALLINT PRIMARY KEY CHECK(policy_id=1), revision BIGINT, max_attempts INTEGER, lease_min_ms BIGINT, lease_max_ms BIGINT, backoff_min_ms BIGINT, backoff_max_ms BIGINT)` には正値・上限を検査する CHECK を付け、初期値 `(1,1,8,1000,120000,1000,300000)` を設定する。

- [ ] RED: `migration_preserves_legacy_domain_and_audit_rows` はイベント ID、JSONB、日時、件数、旧来の配送済み・保留状態、Audit の `delivered_at` を記録し、`0009` の適用後に同一性と追加列が NULL であることを検証する。`migration_rejects_partial_lease_and_double_terminal` は CHECK 違反を検証し、`processing_state_distinguishes_reclaim_and_reap_pending` は配送済み / 配送打切り / 処理中 / 期限切れ保留 / 試行上限到達かつ期限切れを検証する。`cargo test -p document-repository-postgres --test outbox_delivery_migration` を実行し、マイグレーション追加前の RED を得る。
- [ ] GREEN: 追加型 SQL と読み取りモデルの射影を書く（`VIEW` またはアダプターのクエリとし、ビュー `outbox_delivery_state` を選ぶ）。三つのコマンドをすべて繰り返す。合成した保留・配送済みデータの混在条件で `EXPLAIN (ANALYZE, BUFFERS)` を使い、候補選択と回収の実行計画を記録する。本番 DDL は実行しない。大規模表でのオンライン `CREATE INDEX CONCURRENTLY` は SQLx のマイグレーショントランザクション内に含められないため、別の展開手順にする。

<a id="p6-g01--typed-generic-contract-bounds-deterministic-retry約-15-分"></a>
### P6-G01 — 型付き汎用契約、上限、決定的な再試行（約 15 分）

**ファイル:** `crates/outbox-delivery/src/{model,policy}.rs` を作成し、`src/lib.rs` を変更する。`src/policy.rs` の単体テストモジュールで検証する。

**インターフェース:** `DeliveryEnvelope { event_id: Uuid, event_type: String, aggregate_type: String, aggregate_id: Uuid, payload: serde_json::Value, occurred_at: OffsetDateTime }`、`ClaimedEvent { envelope, attempt: i32, attempt_limit: i32, lease_token: Uuid, lease_owner: Uuid, lease_expires_at: OffsetDateTime }`、`FenceResult::Updated|Lost`、`DeliveryDecision::Applied|KnownNoop|Retryable(ErrorCode)|Terminal(ErrorCode)`。許可リスト型の `ErrorCode` は `UnsupportedEvent`、`InvalidEnvelope`、`SourceUnavailable`、`IndexingFailed`、`HandlerTimeout`、`DeliveryUnknown`、`DeliveryUnknownAtLimit` を含む。`DeliveryError` は `PolicyMismatch`、`LegacyExhausted { count, first_ids }`、`InvalidConfig`、`StoreUnknown` を区別し、ペイロードや秘密値をメッセージに含めない。`DeliveryPolicy { revision:i64, max_attempts:i32, lease_min_ms:i64, lease_max_ms:i64, backoff_min_ms:i64, backoff_max_ms:i64 }`、`DeliveryConfig { batch_size, max_in_flight, lease_duration, renew_interval, max_processing, drain_timeout, poll_interval, reap_batch }` と `validate() -> Result<(),DeliveryError>`、`DeliveryFuture<'a,T> = Pin<Box<dyn Future<Output=Result<T,DeliveryError>> + Send + 'a>>`、`HandlerFuture<'a,T> = Pin<Box<dyn Future<Output=T> + Send + 'a>>` を定義する。`OutboxStore` は G02–G04 の戻り値型を使って `verify_policy`、`claim(owner,limit,lease)`、`renew(event_id,token,lease)`、`settle_success(event_id,token)`、`settle_failure(event_id,token,code,terminal,backoff)`、`reap_exhausted(limit)` を宣言する。`retry_delay(event_id: Uuid, attempt: i32) -> Duration` は `min(300 s, 2^(attempt-1) s + SHA-256(event_id || attempt) mod (base/4+1) ms)` とし、飽和演算を使い、下限を 1 s とする。

- [ ] RED: `rejects_unbounded_or_unsafe_config`、`retry_delay_is_stable_bounded_and_increases`、`error_codes_are_allowlisted` で、バッチ `1..=32`、同時処理数 `1..=8`、`renew_interval < lease/3`、処理・終了待機・ポーリングの正の上限、同じ ID/試行数には同じ遅延、秘密値を含む自由記述のコードがないことを検証する。`cargo test -p outbox-delivery --lib policy::tests` を実行して RED を得る。
- [ ] GREEN: 型、検証、再試行を実装し、同じコマンドが PASS する。`sha2.workspace` を再利用し、遅延の揺らぎのための新しい依存は加えない。

<a id="p6-g02--db-policy-guard-old-row-audit-bounded-claim約-15-分"></a>
### P6-G02 — DB 方針の検査、旧行監査、上限付き処理権取得（約 15 分）

**ファイル:** `crates/outbox-delivery/src/postgres.rs` を作成し、`crates/outbox-delivery/tests/postgres_claim.rs` でテストする。

**インターフェース:** `PostgresOutboxStore::new(pool: PgPool, expected: DeliveryPolicy) -> Self` は G01 の `OutboxStore` を実装する。`verify_policy(&self) -> DeliveryFuture<'_,()>`、`claim(&self, owner: Uuid, limit: u32, lease: Duration) -> DeliveryFuture<'_,Vec<ClaimedEvent>>` を用意する。処理権取得の各トランザクションで方針を `FOR SHARE` によりロックし、すべての改訂・値フィールドと、上限未設定で試行を使い切った旧行を確認する。その後、凍結済みの CTE を使い、`limit<=free_permits`、行ごとの `gen_random_uuid()`、DB 時刻、`attempt_limit=COALESCE(old,db_max)` を適用し、戻る前にコミットする。`SELECT FOR UPDATE SKIP LOCKED` は処理権取得の競合調整だけに使い、ハンドラーの実行中ロックには使わない。

- [ ] RED: `policy_mismatch_and_legacy_exhausted_refuse_claim` は、`max_attempts`/改訂の不一致や試行を使い切った旧行が一つでもある場合に、試行数を増やす前に型付きエラーを返すことを検証する。`claim_caps_batch_and_pins_limit_once` は上限 32、異なるトークン、初回試行上限 8、旧保留行のペイロード不変、方針展開後も既存行の上限が維持されることを検証する。`concurrent_claims_are_disjoint` は 2/4/8 個の独立した接続プールを使い、同時に有効なトークンの重複がないことを確認する。`cargo test -p outbox-delivery --test postgres_claim` を実行して RED を得る。
- [ ] GREEN: DB トランザクションと診断を実装し、診断のイベント ID は先頭 32 件以下に限定する。同じコマンドが PASS する。検査から処理権取得の間に方針改訂が変わっても、ロック済みトランザクションを通過できないようにする。DB エラーは `StoreUnknown` とし、`Lost` にしない。

<a id="p6-g03--fenced-renew-ack-retry-and-dlq約-15-分"></a>
### P6-G03 — フェンス付き更新、配送確認、再試行、DLQ（約 15 分）

**ファイル:** `crates/outbox-delivery/src/postgres.rs` を変更し、`crates/outbox-delivery/tests/postgres_settle.rs` でテストする。

**インターフェース:** `renew(event_id: Uuid, lease_token: Uuid, lease: Duration) -> DeliveryFuture<'_,FenceResult>`、`settle_success(event_id, lease_token) -> DeliveryFuture<'_,FenceResult>`、`settle_failure(event_id, lease_token, code: ErrorCode, terminal: bool, backoff: Duration) -> DeliveryFuture<'_,FenceResult>`。すべて単一の `tick` CTE、現在のトークン、`lease_expires_at > tick.t` を使い、配送済み・打切り済みの行を除外する。失敗処理は DB の `attempt_limit` を使い、リースを解除したうえで、上限付き再試行を予約するか `dead_lettered_at` を設定する。`delivered_at` を設定するのは配送確認だけとする。

- [ ] RED: `expired_or_old_token_cannot_renew_ack_or_fail` は期限ちょうどと期限後の更新がすべて 0 行の `Lost` となり、変更がないことを検証する。`retry_uses_db_clock_and_row_limit` は `available_at` の範囲と試行 8 回目での打切り遷移を検証する。`unknown_ack_commit_is_not_success` は接続喪失を注入し、`delivered_at` の再読み取りを必須とする。`cargo test -p outbox-delivery --test postgres_settle` を実行して RED を得る。
- [ ] GREEN: `Result<FenceResult,DeliveryError>` の意味に従って SQL を実装し、バインド前に再試行待機時間の上限を検証する。三つのコマンドが PASS する。Search のコードが `settle_success` を直接呼ぶことは禁止する。

<a id="p6-g04--bounded-exhausted-reaper約-15-分"></a>
### P6-G04 — 試行上限到達行の上限付き回収（約 15 分）

**ファイル:** `crates/outbox-delivery/src/postgres.rs` を変更し、`crates/outbox-delivery/tests/postgres_reaper.rs` でテストする。

**インターフェース:** `reap_exhausted(limit: u32) -> DeliveryFuture<'_,u64>`。同一トランザクションで方針を確認する。凍結済みの CTE は `last_attempt_at NULLS FIRST,event_id` の順に並べ、バッチ数を制限し、`FOR UPDATE OF o SKIP LOCKED` を使って `delivery_unknown_at_limit` を書き、元の行を保持する。起動時、各処理権取得サイクルの前、終了待機の終端で呼ぶ。

- [ ] RED: `crashed_final_claim_reaped_once_by_competing_workers` は、二つの回収処理が、期限切れかつ試行上限到達の一行を正確に一度だけ更新し、ペイロード・試行数を保持し、再取得しないことを検証する。`policy_mismatch_refuses_reap` と `unexpired_final_attempt_is_not_reaped` は変更が 0 件であることを検証する。`cargo test -p outbox-delivery --test postgres_reaper` を実行して RED を得る。
- [ ] GREEN: SQL を実装し、同じコマンドが PASS する。件数を監視処理・警報に公開する。DB 障害はエラーのまま扱い、回復に成功したと見なさない。

<a id="p6-g05--permit-before-claim-and-dispatch-preflight約-15-分"></a>
### P6-G05 — 処理権取得前の実行枠確保と配送直前検査（約 15 分）

**ファイル:** `crates/outbox-delivery/src/runner.rs` を作成し、`crates/outbox-delivery/tests/runner_admission.rs` でテストする。

**インターフェース:** `ClaimPermit: Clone+Send+Sync` は `preflight/renew/release -> DeliveryFuture<'_,FenceResult>` を持つ。`ClaimAdmission` は関連型 `Permit: ClaimPermit` と `acquire() -> DeliveryFuture<'_,Option<Permit>>` を持つ。汎用経路には `NoopAdmission` を使う。`DeliveryHandler<P: ClaimPermit>::deliver(envelope: DeliveryEnvelope, context: DeliveryContext, permit: P) -> HandlerFuture<'_,DeliveryDecision>` を定義する。`DeliveryContext` は試行数、Outbox のトークン・期限、`Arc<AtomicBool>` による取り消しを保持する。`DeliveryRunner<S:OutboxStore,H:DeliveryHandler<A::Permit>,A:ClaimAdmission>::run_cycle(&self) -> Result<CycleSummary,DeliveryError>` は空きセマフォ枠、受付許可、`claim(min(batch_size,free_slots))` の順に取得する。Search の設定では必ず一つに制限する。ハンドラーの直前に Outbox と実行許可の**両方**を更新し、どちらかが `Lost` または結果不明ならハンドラーを呼ばない。

- [ ] RED: `batch_32_inflight_8_never_claims_queued_work` は、遅いハンドラーでも有効な処理権が最大 8 件で、取得済み・未配送のまま期限切れになる行がないことを検証する。`pre_dispatch_lost_never_calls_handler` は、実行許可または Outbox の失敗時に行を期限切れ待ちとして残し、ハンドラーを呼ばないことを検証する。`search_admission_denied_claims_zero` は、Source の実行許可を得られない待機を試行として数えないことを検証する。`cargo test -p outbox-delivery --test runner_admission` を実行して RED を得る。
- [ ] GREEN: 上限付きの実行枠・受付許可と直前検査を実装し、コマンドが PASS する。処理権を取得した行は、取得のコミット直後から更新管理を始める。

<a id="p6-g06--heartbeat-cancellation-and-graceful-drain約-15-分"></a>
### P6-G06 — 定期更新、取り消し、安全な終了待機（約 15 分）

**ファイル:** `crates/outbox-delivery/src/runner.rs` を変更し、`crates/outbox-delivery/tests/runner_lifecycle.rs` でテストする。

**インターフェース:** `run_until_shutdown(&self, shutdown: watch::Receiver<bool>) -> Result<RunSummary,DeliveryError>`。定期更新は設定された 30 s / 120 s で両リースを更新し、合計 15 min で停止する。SIGTERM/CTRL-C の受信で新規の受付・処理権取得を止め、上限付き終了待機の間は両方の更新を継続する。期限に達したら取り消しを伝え、未完了の処理には配送確認・失敗確定を行わず、まだ所有している Source の実行許可だけを解放し、最後の回収処理を実行する。DB・ハンドラーの障害時は、上限付き再接続待機と観測可能な警報を使う。

- [ ] RED: `renew_lost_cancels_and_never_settles`、`shutdown_drains_completed_and_leaves_unfinished`、`outage_does_not_spin_or_ack_unknown` は、取り消しの伝播、完了した行だけの配送確認、未完了行のリース失効による回復、ポーリング呼出し数の上限を検証する。`cargo test -p outbox-delivery --test runner_lifecycle` を実行して RED を得る。
- [ ] GREEN: Tokio の `select!`、定期実行、watch（変更通知）、上限付きタイマーを使ってライフサイクルを実装し、コマンドが PASS する。取り消しを無視するハンドラーでも、最終 DB フェンスは通過できない。

<a id="p6-g07--generic-real-process-crash-and-restart約-15-分"></a>
### P6-G07 — 汎用経路の実プロセス異常終了と再起動（約 15 分）

**ファイル:** テスト対象は `crates/outbox-delivery/tests/process_recovery.rs` だけ。RED で欠陥が見つかった場合も、本番コードの修正は G02–G06 の範囲内に保つ。

**インターフェース:** テスト実行ファイルは `current_exe()` を使い、通常は無視される `child_worker_fixture`、独立した接続プール・プロセス ID を指定して自分自身を子プロセスとして起動する。最終処理権取得のコミットを観測した後に子を強制終了する。本番用のフィクスチャ実行ファイルや新しい依存は追加しない。

- [ ] RED: `last_claim_kill9_restart_reaps_unknown_once` は元の ID・ペイロード、`attempt_count=8`、二つのプロセスによる回収で `delivery_unknown_at_limit` が正確に一つだけとなること、その後に処理権を取得しないことを確認する。`four_processes_disjoint_claims_and_recover_expired` は有効トークンの非重複と、強制終了後に試行数を増やして再取得することを確認する。`cargo test -p outbox-delivery --test process_recovery` を実行する。プロセス経路を接続するまでは最初に失敗する。
- [ ] GREEN: 汎用部分の最小限の修正だけを行い、実 PostgreSQL 18.6 上で同じコマンドが PASS する。一回の実行のプロセス・DB 日時を資格確認の記録として保持する。

<a id="p6-g08--observability-trace-and-db-role-boundary約-15-分"></a>
### P6-G08 — 可観測性、トレース、DB ロールの境界（約 15 分）

**ファイル:** `crates/outbox-delivery/src/observe.rs`、`sql/least_privilege.sql` を作成し、`crates/outbox-delivery/tests/observability_security.rs` でテストする。

**インターフェース:** `DeliveryObserver::record(DeliveryMetric)` が使うラベルは、値の種類を限定した `route`、`ErrorCode`、`outcome` だけとする。保留・処理中・DLQ、最古行の経過時間、処理権取得・配送確認・再試行・回収・古いフェンスの件数、ハンドラー実行時間、コミットから配送確認までの遅延、DB・コンシューマーのエラー分類を出力する。`validate_trace_context(traceparent: Option<&str>, tracestate: Option<&str>) -> Option<ValidatedTrace>` は W3C の形式・長さを満たすものだけを受け付け、ペイロード・操作主体・文書・トレース ID をメトリクスラベルとして記録しない。SQL は配送ロールに Domain Outbox と方針の `SELECT`、配送列だけの `UPDATE` を付与する。Document の変更や Audit の更新は禁止する。

- [ ] RED: `metrics_do_not_contain_high_cardinality_or_payload`、`invalid_trace_is_ignored_without_log_leak`、`delivery_role_cannot_mutate_document_or_audit` は出力ラベルと実際のロール権限を検証する。`cargo test -p outbox-delivery --test observability_security` を実行して RED を得る。
- [ ] GREEN: 監視フックと権限付与テンプレートを実装し、同じコマンドが PASS する。トレースが NULL の過去の生成行は、イベント ID だけで関連付けた新しいスパンを開始する。

<a id="p6-s01--search-eventfence-ports-and-allowlistshared-port-writer約-15-分"></a>
### P6-S01 — Search のイベント・フェンスポートと許可リスト（共有ポート編集担当、約 15 分）

**ファイル:** `crates/search-application/src/indexing_service.rs`、`src/ports.rs` を変更し、`crates/search-application/tests/port_contract.rs` でテストする。

**インターフェース:** `SourceFence { source_id: SourceId, owner_token: Uuid, epoch: i64 }`、`SearchSourceLease::fence(&self) -> SourceFence`、`SearchDeliveryFence { event_id: Uuid, outbox_token: Uuid, source: SourceFence }`、`CurrentGenerationSnapshot { key: Option<ProjectionGenerationKey>, manifest_digest: Option<String>, bundle_digest: Option<String>, pointer_revision: i64 }`、`CompletionMode::PublishCandidate|ReuseCurrent`、`CompleteEventRequest { fence, expected_current: CurrentGenerationSnapshot, candidate: ProjectionGenerationKey, manifest_digest: String, bundle_digest: String, mode }`、`SearchCompletionOutcome::Published(ProjectionGenerationKey)|Unchanged(ProjectionGenerationKey)|Duplicate(ProjectionGenerationKey)|Retry|Lost`、SQLx 非依存の `SearchEventCompletionPort::current_snapshot(source_id) -> BoxFuture<CurrentGenerationSnapshot>` と `complete_event_if_current(request) -> BoxFuture<SearchCompletionOutcome>`、`FencedDocumentIndexingPort::refresh_fenced(event, fence, cancel: Arc<AtomicBool>) -> BoxFuture<IndexingOutcome>` を定義する。`SearchError::FenceLost` と `SearchError::CompletionUnknown` を追加し、`DocumentIndexingService<P: FencedDocumentIndexingPort>::handle_delivery(event,fence,cancel) -> Result<IndexingOutcome,SearchError>` は検証を再利用する。`validate_document_event_route(event_type:&str, aggregate_type:&str) -> Result<(),SearchError>` を公開し、Document*→Document、Folder*→Folder、`AccessPolicyChanged`→Document/Folder/AccessPolicy と対応付ける。未知の Document/Folder/AccessPolicy の組合せは無効とし、v0 には何もしない正常経路を設けない。

- [ ] RED: `route_matrix_rejects_unknown_and_wrong_aggregate`、`fenced_port_preserves_search_only_types` で、現在の `relevant_event_type` の全ケースと AccessPolicy の三つの型、アプリケーション API に SQLx の型がないことを確認する。`cargo test -p search-application --test port_contract` を実行して RED を得る。
- [ ] GREEN: D4 のメモリ上の挙動を変えず、既存の非公開許可リストを共通の検証処理へ切り出す。コマンドが PASS する。P3/P7 のポート編集担当は、このインターフェースのレビュー後にだけ作業を始める。

<a id="p6-i03--shared-search-sourcereceipt-migrationshared-migration-writer約-15-分"></a>
### P6-I03 — Search の Source・処理記録の共有マイグレーション（共有移行編集担当、約 15 分）

**ファイル:** `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql` を作成し、`crates/search-runtime/tests/coordination_migration.rs` でテストする。Search 専用のマイグレーション台帳を使い、`src/lib.rs` で `search_runtime::migrate(&PgPool)` を公開する。Domain の `0009` を先に適用する。

**インターフェース:** 物理表 `search_source_coordination(source_id UUID PK, fence_epoch BIGINT NOT NULL DEFAULT 0, owner_token UUID NULL, lease_expires_at TIMESTAMPTZ NULL, current_generation_id UUID NULL, current_manifest_digest TEXT NULL, current_bundle_digest TEXT NULL, pointer_revision BIGINT NOT NULL DEFAULT 0, last_published_epoch BIGINT NOT NULL DEFAULT 0, build_fence_seq BIGINT NOT NULL DEFAULT 0)` に、所有者・期限の組、現在のキー・二つのダイジェストの三つ組の完全性と、非負・桁あふれ防止の制約を設ける。`search_index_receipts(source_id UUID,event_id UUID,generation_id UUID,digest TEXT,bundle_digest TEXT,fence_epoch BIGINT,recorded_at TIMESTAMPTZ)` は `(source_id,event_id)` を PK とし、`digest` は P6 の射影マニフェストのダイジェスト、`bundle_digest` は P1 のバージョン付き複合ダイジェストとする。これらの表を P7 の Source 制御と処理記録の名前空間とし、独立した Graph ポインターは作らない。P7 は取得前に登録済み Source の行を作り、後続のマイグレーションで世代・保持・保護の成果物を追加する。

- [ ] RED: 使い捨て DB に対して `coordination_schema_rejects_half_lease_and_duplicate_receipt`、`migration_keeps_pointer_and_receipt_same_database`、`source_row_is_preseeded_before_lease` を用意し、`cargo test -p search-runtime --test coordination_migration` を実行して RED を得る。
- [ ] GREEN: SQL、Search の移行処理、単独の編集責任を実装し、コマンドが PASS する。P3 の `source_control` という用語はこの行に対応し、P3 の `build_fence_seq` は同じカウンターを使い続ける。表名を暗黙に変えたり、独立したポインターを作ったりしない。

<a id="p6-s02--distributed-source-lease-adapterp7-owned-scope約-15-分"></a>
### P6-S02 — Source の分散リースアダプター（P7 所有範囲、約 15 分）

**ファイル:** `crates/search-runtime/src/source_lease.rs` を作成し、`crates/search-runtime/tests/source_lease.rs` でテストする。

**インターフェース:** `PostgresSourceAdmission::new(pool: PgPool, source_id: SourceId, ttl: Duration)` は汎用の `ClaimAdmission<Permit=SourceLease>` を実装し、`SourceLease` は `ClaimPermit` と `SearchSourceLease::fence() -> SourceFence` を実装する。`acquire_source` は DB 時刻、`owner_token=gen_random_uuid()`、`fence_epoch+1` を使う一つの条件付き `UPDATE` とし、桁あふれ時は安全側に拒否する。`renew/release` には Source ID・所有者トークン・世代番号・有効期限内の DB リースが必要。競合時は `Ok(None)` を返し、Outbox の処理権取得を**呼ばない**。

- [ ] RED: `two_four_eight_processes_have_one_source_owner`、`lost_owner_cannot_renew_release_or_claim`、`source_epoch_overflow_refuses_acquire` は独立した `PgPool` を使い、世代番号の単調増加と、古い所有者の操作が成功しないことを検証する。`cargo test -p search-runtime --test source_lease` を実行して RED を得る。
- [ ] GREEN: 短い SQL 操作を実装し、同じコマンドが PASS する。取得応答が `StoreUnknown` の場合は Outbox の処理権を取得しない。プロセス内メモリから所有権を推定しない。

<a id="p6-s03--atomic-search-completion-and-monotonic-receiptp7-owned-scope約-15-分-per-branch"></a>
### P6-S03 — Search 完了と単調な処理記録の原子的確定（P7 所有範囲、分岐ごとに約 15 分）

**ファイル:** `crates/search-runtime/src/event_completion.rs` を作成し、`crates/search-runtime/tests/event_completion.rs` でテストする。P7 実行基盤の世代準備判定・保持・保護付き GC アダプターが必須の依存となる。メモリ上の実行基盤でこの試験を満たすことはできない。

**インターフェース:** `PostgresSearchEventCompletion::current_snapshot(source_id) -> BoxFuture<CurrentGenerationSnapshot>` と `complete_event_if_current(CompleteEventRequest) -> BoxFuture<SearchCompletionOutcome>`。外部の Projection/Unit/網羅性/字句索引/Graph 成果物について、キー・マニフェスト・バンドルのダイジェストと READY を事前確認し、コミットまで P7 の準備領域リースまたは構築保護を保持する。一つの短い SQLx トランザクション内で Domain Outbox 行、`search_source_coordination`、世代キー・保護・評価リース・処理記録の順に、P3/P7 共通の順序でロックする。現在の Outbox トークンと DB 上の期限、Source の所有者・世代番号・期限、現在の期待するキー・マニフェスト・バンドルのダイジェスト・改訂、`last_published_epoch<=epoch`、候補の READY を確認する。要求された場合はポインターを公開する。`(source_id,event_id)` の処理記録を挿入・更新できるのは、既存の世代番号が低い場合だけ。同じ世代番号・キー・**両ダイジェスト**は冪等とし、同じ世代番号での不一致や既存の世代番号の方が高い場合は `Lost/Retry` としてロールバックする。`ReuseCurrent` は過去の処理記録があっても、現在のキー・両ダイジェストが READY であることを要求する。コミット応答が不明ならエラーを返し、成功とはしない。このポートは汎用配送の `delivered_at` を**更新しない**。

- [ ] RED: `publish_and_receipt_commit_or_rollback_together`、`stale_epoch_cannot_overwrite_new_pointer_or_receipt`、`same_epoch_conflict_is_not_duplicate`、`gc_receipt_is_not_a_pin_or_duplicate` は、ポインター・処理記録の原子性、G1 復帰後も G2 が維持されること、READY・現在値の確認、保護付き GC を検証する。`cargo test -p search-runtime --test event_completion` を実行して RED を得る。
- [ ] GREEN: 二つ目の接続や独立した `put` を使わず、P7 の同一トランザクションに束縛されたリポジトリ API を通じて実装し、同じコマンドが PASS する。P3 Graph の READY 確認と保留中の構築保護も、同じ P7 の準備判定・後片付け契約の一部として維持する。SQLSTATE `40001`/`40P01` では同じ期待改訂からトランザクション全体を有限回だけ再試行し、CAS の競合敗北を成功と報告しない。

<a id="p6-s04--d4-indexer-fenced-path約-15-分-per-outcome"></a>
### P6-S04 — D4 索引処理のフェンス付き経路（結果ごとに約 15 分）

**ファイル:** `crates/search-source-document/src/outbox.rs`、`src/lib.rs` を変更し、`crates/search-source-document/tests/outbox_indexing.rs` と `tests/outbox_delivery.rs` でテストする。

**インターフェース:** D4 テスト用に `DocumentOutboxIndexer::new` と旧来の `IndexingReceiptStore` を維持する。`with_fenced_completion(mut self, completion: Arc<dyn SearchEventCompletionPort>) -> Self` を追加し、`FencedDocumentIndexingPort::refresh_fenced` を実装する。`reconcile_once` は任意の `SearchDeliveryFence` と `Arc<AtomicBool>` による取り消しを受け付ける。本番用の分岐は S03 の `current_snapshot` を取得し、`Published`、`Unchanged`、`Duplicate` の**すべて**の結果に `complete_event_if_current` を使う。旧来の `receipts.put` や `runtime.publish_if_current` は呼ばない。Source の読み取りと各準備段階の後に取り消しを確認し、完了前には実行許可と Outbox の両方を直前検査する。候補世代は P7 の準備領域メタデータにイベント ID + Source 世代番号を記録する。未公開で失敗した候補や CAS の競合敗者は、現在・保持中のキーを直接削除せず、保護付き後片付けを使う。P7 は複合バンドルの処理記録を提供し、射影だけのダイジェスト一致では無処理を確定できない。

- [ ] RED: `fenced_published_uses_atomic_completion_only`、`fenced_unchanged_and_duplicate_revalidate_current_ready`、`cancelled_build_never_publishes_or_deletes_current` は、旧ポートが呼ばれないこと、現在値と処理記録の一致、後片付けのフェンスを検証する。`cargo test -p search-source-document --test outbox_delivery` を実行して RED を得る。
- [ ] GREEN: 必要な箇所だけで共通の構築ロジックを抽出する。三つのコマンドと、既存の対象を限定した `cargo test -p search-source-document --test outbox_indexing` がすべて PASS する。D4 のメモリ上の挙動はテスト経路として残すが、本番の永続性の証明にはしない。

<a id="p6-s05--search-bridge-routedecision-mapping約-15-分"></a>
### P6-S05 — Search 接続処理の経路と判定の対応（約 15 分）

**ファイル:** `crates/search-source-document/src/delivery.rs` を作成し、`src/lib.rs` を変更する。`crates/search-source-document/tests/outbox_delivery.rs` でテストする。

**インターフェース:** `DocumentSearchDeliveryHandler<I>::new(indexer: DocumentIndexingService<I>, source_id: SourceId)` は `P: ClaimPermit + SearchSourceLease` を条件として `DeliveryHandler<P>` を実装する。検証済みのイベント・集約型の組合せだけを `DocumentSourceEvent` に変換し、ペイロードは転送しない。S03 のコミット済み完了後の `Published|Unchanged|Duplicate` → `Applied`、`Ignored`/未知/誤った集約型 → `Terminal(UnsupportedEvent|InvalidEnvelope)`、`SourceUnavailable`・取り消し・時間切れ・コミット結果不明 → `Retryable` または結果不明とし、配送確認しない。行の状態を確定するのは汎用ランナーだけとする。

- [ ] RED: `bridge_routes_document_folder_policy_without_payload`、`unknown_document_event_goes_terminal_not_ignored`、`search_failure_leaves_row_unacked` は、経路の対応表、ペイロード漏えいがないこと、正しい判定を検証する。`cargo test -p search-source-document --test outbox_delivery` を実行して RED を得る。
- [ ] GREEN: 接続処理と公開を実装し、コマンドが PASS する。集約型が Document/Folder/AccessPolicy の `AccessPolicyChanged` を受け付ける。

<a id="p6-i04--composition-shutdown-signal-and-role-splitshared-integration-writer約-15-分"></a>
### P6-I04 — 構成の接続、終了シグナル、ロールの分離（共有統合編集担当、約 15 分）

**ファイル:** `crates/search-runtime/src/bin/search_outbox_worker.rs` を作成する。最終的な権限付与の対応付けに必要な範囲だけで `crates/search-runtime/src/lib.rs` と `crates/outbox-delivery/sql/least_privilege.sql` を変更する。ルートの Tokio signal 機能は既に I01 の専任範囲である。`crates/search-runtime/tests/worker_wiring.rs` でテストする。

**インターフェース:** 起動時にスキーマ・方針を移行・検証し、試行を使い切った旧行を拒否する。設定済み Source 行を事前作成し、配送用と Source 読み取り・Search 完了用の接続プールを分けて構築する。静的な Document Search 経路を一つ登録し、Search のバッチ・同時処理数を 1 に設定して、SIGTERM/CTRL-C に対応する `run_until_shutdown` を実行する。P7 の永続実行基盤だけを使い、プロセス内の `MemoryDocumentIndexRuntime` は禁止する。Search 完了ロールには Outbox の `SELECT` が必要であり、PostgreSQL の `FOR UPDATE` には少なくとも一つの Outbox 列への `UPDATE` が必要となる。行ロック用には `UPDATE(lease_token)` だけを付与し、`UPDATE(delivered_at)` や Audit/Document の変更権限は**付与しない**。アダプターはどの配送列にも書き込まず、状態更新は汎用ロールだけが行う。[PostgreSQL SELECT 権限](https://www.postgresql.org/docs/current/sql-select.html)。

- [ ] RED: `worker_refuses_mismatched_policy_or_memory_runtime`、`search_role_cannot_ack_or_touch_audit`、`sigterm_stops_claim_and_drains` は、安全側への起動拒否、ロール制限、上限付き終了待機を検証する。`cargo test -p search-runtime --test worker_wiring` を実行して RED を得る。
- [ ] GREEN: 既存の SQLx/Tokio 実行基盤と静的経路を接続し、資格未確認の新しい依存は加えない。コマンドが PASS し、`cargo check --locked -p search-runtime --bin search_outbox_worker` も成功する。

<a id="p6-s06--producer-to-ack-real-postgresql-slice約-15-分"></a>
### P6-S06 — 生成から配送確認までの実 PostgreSQL 縦断試験（約 15 分）

**ファイル:** `crates/search-runtime/tests/outbox_delivery.rs` でテストする。必要な修正も S03–S05 に対象を限定する。

- [ ] RED: `domain_event_to_current_ready_receipt_then_generic_ack`、`t10_and_access_revocation_reread_current_source`、`manual_rebuild_does_not_ack_pending_event` は実際の生成側トランザクション、Domain + Search のマイグレーション、P7 永続実行基盤を使う。処理記録・現在値・配送完了の証拠を分けて検証し、Audit の `delivered_at` と Document 行が変わらないことを確認する。`cargo test -p search-runtime --test outbox_delivery` を実行し、全経路の接続前に RED を得る。
- [ ] GREEN: S03–S05 を実際の実行基盤に接続し、同じコマンドが PASS する。これは P6/P7 統合の証拠であり、汎用 P6 だけの証拠にはしない。

<a id="p6-s07--distributed-process--fault--restart-qualification約-15-分-per-fault-schedule"></a>
### P6-S07 — 分散プロセス・障害・再起動の資格確認（障害注入手順ごとに約 15 分）

**ファイル:** `crates/search-runtime/tests/source_process_recovery.rs` を作成する。本番コードの修正は失敗した箇所を担当するタスクだけで行う。

- [ ] RED: `source_burst_two_four_eight_processes_no_prefetch_or_dlq` は有効な Source 所有者が一つで処理権取得=1 であること、`g1_stale_after_g2_cannot_publish_receipt_or_ack` は旧プロセスの復帰後も G2 の現在値・処理記録が維持されることを検証する。`publish_commit_unknown_and_ack_commit_unknown_reconcile` は再読み取り・再配送と、二回目の可視公開がないことを確認する。`expiry_boundary_and_gc_keep_current_or_pinned` は失効したフェンスの Lost と後片付けの保護を確認する。一つの使い捨て DB に対して独立した OS プロセス・接続プールを起動し、`kill -9`、期限切れ、接続喪失、再起動、同期バリアによる再開制御を注入する。`cargo test -p search-runtime --test source_process_recovery` を実行して RED を得る。
- [ ] GREEN: DB 行・マニフェスト・ダイジェストの確認とともに同じコマンドが PASS する。GC 済み成果物を指す過去の処理記録はメタデータだけとし、重複判定には現在の READY なキー・ダイジェストを要求する。プロセスログにペイロード、文書名、主体、秘密値を含めない。

<a id="p6-n01--normative-read-model-and-delivery-semantics単独-spec-writer約-15-分"></a>
### P6-N01 — 規範となる読み取りモデルと配送の意味（仕様の専任編集担当、約 15 分）

**ファイル:** `spec/data/transaction-consistency-requirements-v0.md` と `spec/operations/observability-audit-requirements-v0.md` の Outbox 節だけを変更する。

- [ ] RED: `aggregate_version` / `processing_state`、順序、DLQ、トレースの記述を凍結内容と比較し、矛盾する段落を正確にタスク記録へ残す。`rg -n 'aggregate_version|processing_state|outbox|trace' spec/data/transaction-consistency-requirements-v0.md spec/operations/observability-audit-requirements-v0.md` を実行する。
- [ ] GREEN: 導出状態 `DELIVERED|DEAD_LETTER|IN_FLIGHT|PENDING`、試行を使い切って回復待ちとなる状態の可視化、Search の単一宛先、少なくとも一度・順序非保証の配送、生成側ごとの将来のバージョン管理、検証済みトレースだけを規定する。一律の `aggregate_version` 埋戻しを主張しない。`mise run verify:fast` と対象を限定した仕様・アーキテクチャの静的検査が PASS する。

<a id="final-qualification-and-execution-order"></a>
## 最終的な資格確認と実行順序

1. `N01` は凍結済みの意味に基づいて、`I02` より前に直ちに着手できる。`G00 → I01` で二つのパッケージを確保し、I01 をルート Cargo/ロックの唯一の編集担当とする。`I02` は独立しており、N01 の後に開始できる。汎用実装は `G01 → G02 → G03 → G04 → G05 → G06 → G07 → G08` の順に進める。`I03` は I01/I02 の後に開始できるが、Search マイグレーションの編集権を専有する。`S01` はアプリケーションポートの独立した専任編集担当。`S02` には I03 が必要。`S03` には S01/S02 と P7 の永続 READY・ポインター・保護付き GC 実装が必要。その後は `S04 → S05 → I04 → S06 → S07` の順とする。ルート Cargo/ロック、`search-application` のポート、両マイグレーションディレクトリ、P7 Source 制御を複数の担当が同時に編集しない。
2. 汎用部分の資格確認: 上記の対象を限定したテスト名、`cargo fmt --all -- --check`、`cargo clippy --locked -p outbox-delivery --all-targets -- -D warnings` を使う。代表的な 1k/10k イベント、1/4/8 ワーカー、バッチ 1/16/32、ハンドラー 0/10/100 ms の組合せを一つ選び、定常状態で三回実行する。コード SHA、PostgreSQL/SQLx のバージョン、CPU/RSS、シード、events/s、p50/p95/p99 の処理権取得・配送確認、DB CPU/IO/ロック、更新漏れ、滞留処理の解消を記録する。異常終了・回収の結果と `EXPLAIN (ANALYZE, BUFFERS)` を保持し、候補値から SLO 達成を主張しない。
3. P7 後の統合資格確認: 対象を限定した S03–S07、`cargo clippy --locked -p search-runtime -p search-source-document -p search-application --all-targets -- -D warnings`、1k/10k の Document コーパス一つを使い、集中発生・再起動・T10・権限失効、Source の再構築コスト、コミットから可視化までの遅延を確認する。P3 Graph の READY、共有 Source ロック名前空間、現在・保持・構築保護の GC、処理記録と Outbox をそれぞれ検証する。実質的な組み立ての後に `mise run verify:fast` を一度実行し、正確な head に対するホステッド CI と独立した読み取り専用レビューを最終ゲートとする。汎用部分、Search 接続処理、P7 構成の記録は分けて残す。
4. 親タスクグラフは旧来の単一 `p6-implement.write_scope=crates/outbox-delivery` を上記の ID・編集範囲に置き換える必要がある。`p6-receipt` は P7 より前に汎用部分の GREEN を報告してよいが、Search 配送の統合は S03–S07 と P7 の資格確認まで保留とする。現在のグラフで `p7-design` が最終 `p6-receipt` に依存する関係は、P6+P7 の構成接続を完了済みと扱わず、汎用 P6 の証拠を受け取る形にする。すべての PR を Draft のまま維持する。この計画からマージ・デプロイ・本番マイグレーションの実行が許可されるわけではない。
