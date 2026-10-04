<a id="p6--durable-outbox-delivery-worker-設計案"></a>
# P6 — 永続 Outbox 配送ワーカー設計案

> 以下の状態・次の作業は当時の記録であり、現在の実行指示ではありません。現行の停止条件・実行可否は[最新の実行状態](../../execution/search-platform-completion-program-status.md)を参照してください。

[公開原文の固定リンク（commit `0ecf486719e3c9d71242e289a7564ad6d1032b3c`）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p6-outbox-design.md)

本書は意味保存の日本語訳であり、原設計の再承認・実装/資格の追加ではない。既存ハッシュは当時の原文/証拠のものであり、訳文のハッシュではない。以下の状態・予定・合否は当時の記録として保持する。本文の行番号は同 commit の各原文を指し、従来の見出しアンカーも維持している。掲載コマンドは今回の翻訳作業では実行していない。

用語: Outbox は配送待ちイベントの永続表、Source は検索対象の情報源。リースは期限付き処理権、フェンスは古い所有者の更新を拒否する条件、エポックは所有権の世代番号、Search のイベント処理記録（receipt）は索引処理結果の記録、検証の receipt は試験・実行結果の証拠記録を指す。CAS は期待する現値との一致を条件にした更新、GC は不要世代の回収、DLQ は打切りイベントの保管先。Search / Domain / Document / Folder / AccessPolicy / Projection / Unit / Graph などの構成名・型名は識別のため維持する。

- 状態: **DRAFT / アーキテクチャレビュー前**（設計凍結・実装・運用認定ではない）
- 日付: 2026-09-30 JST
- 基準: Completion Program（完成プログラム） P0 完了確認、Search / Discovery Platform v0 Phase D の D4 コンシューマー
- 目的: コミット済み Domain Outbox のイベントを汎用ワーカーが少なくとも一度 Search に配送し、配送状態・再試行・障害回復をワーカーが所有する。

## 1. 現行契約と変更境界

| 根拠 | 現状 | P6 で守る境界 |
| --- | --- | --- |
| `crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql:74` | `outbox_events(event_id PK, event_type, aggregate_type, aggregate_id, payload JSONB, occurred_at, available_at, attempt_count DEFAULT 0, delivered_at NULL)`。リース、DLQ、`aggregate_version`、トレース文脈列はない。別の `audit_outbox_events` もある。 | 既存行と生成側の挿入を壊さない追加型のマイグレーション。Domain の `outbox_events` だけを本 P6 の対象にする。Audit Store 配送・Audit 行の配送確認は別機能領域。 |
| `crates/document-repository-postgres/src/repository.rs:178`、`targeted_events.rs:18`、`versioning_mutation.rs:556`、`publication_end.rs:347` | Document / Folder / AccessPolicy の変更と Domain / Audit イベントは同一トランザクションで記録される。対象限定の生成側は省略列の既定値を使う。 | Document の業務トランザクションに Search 処理を入れない。イベント同一性、ペイロード、発生時刻はワーカーが書き換えない。 |
| `crates/search-application/src/indexing_service.rs:13` | `DocumentSourceEvent` はイベント ID/型、集約 ID、発生時刻だけを要求する。`DocumentIndexingService::handle` は未知の Document 系イベントをエラーとし、`Published/Unchanged/Duplicate/Ignored` を返す。 | 接続処理が `aggregate_type` とイベント型を検証してから呼ぶ。成功結果の意味を明確化し、未知イベントを `Ignored` として黙って配送確認しない。 |
| `crates/search-source-document/src/outbox.rs:47,642,925` | D4 は現行 Source のスナップショットを再読し、索引世代の CAS とイベント処理記録で重複/順序逆転に収束する。`delivered_at` を更新しない。処理記録ストアはトレイトで、現行試験はメモリ実装。 | 配送ワーカーだけが `attempt_count/available_at/delivered_at/lease/DLQ` を更新する。Search は自身の射影と処理記録を所有する。 |
| `crates/search-source-document/src/postgres.rs:132`、`docs/superpowers/execution/search-discovery-platform-v0-acceptance.md` | 正本スナップショットは単一の PostgreSQL 読み取り専用・反復可能読み取りトランザクション。Phase D の実装/ホステッド環境の証拠は D4 までで、汎用ワーカー、永続的な Search 実行基盤、配備は未実施。 | P6 単独のテスト成功を本番 Search 提供完了と呼ばない。P3/P7 の共有の永続的な索引世代/処理記録と最終的な接続が必要。 |
| `spec/data/transaction-consistency-requirements-v0.md:756`、`spec/operations/observability-audit-requirements-v0.md:420` | 少なくとも一度の配送、冪等なコンシューマー、再試行、失敗/配送打切り、Outbox 遅延可視化が要求される。規範は `aggregate_version` と `processing_state` も列挙するが、現行物理表にはない。 | 本 P6 は既存イベントを並べ替え不能なバージョン付きストリームと見なさない。`processing_state` は後述の明示的な読み取りモデルで提供する。`aggregate_version` を既存ペイロードから一律推測・埋戻ししない。規範との表現差は凍結時に明記して整合させる。 |

`outbox_events.delivered_at` は現行の単一論理配送先の完了印である。P6 v0 は登録済み Domain イベントを Search 接続処理に渡す **一つの必須配送経路**としてこれを解釈する。第二の独立購読先が必要になったら `(event_id, destination_id)` を鍵とする配送状態表を別設計し、一つの `delivered_at` を複数宛先の成功と混同しない。`audit_outbox_events` の `delivered_at` は一切触らない。

## 2. 案の比較と決定

| 案 | 判定 | 理由 |
| --- | --- | --- |
| `SELECT ... FOR UPDATE` の行ロックを Search 索引化完了まで保持 | 不採用 | 全 Source 再読・索引構築の期間だけ業務 DB 接続/ロックが残り、障害・長時間処理に弱い。 |
| `LISTEN/NOTIFY` のみ、またはプロセスメモリキュー | 不採用 | 通知・プロセスの喪失後にコミット済みイベントを回復できない。通知を将来の起床ヒントにすることは可能。 |
| 別 CDC/ブローカーを P6 の必須経路にする | 今回は不採用 | Source DB とコネクターの運用・処理位置整合が別機能領域となる。現行スキーマと SQLx/PostgreSQL で必要な耐久性を実証する。 |
| **短い DB トランザクションで `FOR UPDATE SKIP LOCKED` 処理権取得、期限付きリース、トークン付き配送確認/失敗確定** | **採用** | ワーカー複数台の処理権取得を競合なく分散し、プロセス異常終了後はリース失効から回復できる。長い Search 処理中は DB 行ロックを保持しない。 |

PostgreSQL は `SKIP LOCKED` をキュー型の表の複数コンシューマーに利用可能と説明する一方、取得順の完全な整合ビューは保証しない。よって `available_at, occurred_at, event_id` は処理権取得の優先順であって、集約ごとの厳密な順序保証ではない。Search D4 は古いイベントでも最新正本を再読する。[PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html)、[UPDATE](https://www.postgresql.org/docs/current/sql-update.html)。

<a id="3-additive-persistence-と状態機械"></a>
## 3. 既存を保持した追加型の永続化と状態機械

`crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` へ以下を追加する。既存のイベント列・PK・行は維持し、削除、履歴の一括初期化、`delivered_at` の書換えはしない。

| 追加列 | 型 / 意味 |
| --- | --- |
| `lease_token` | NULL を許容する UUID。処理権取得ごとに新しい UUID。配送確認/失敗確定/リース更新のフェンストークン。 |
| `lease_owner` | NULL を許容する UUID。ワーカープロセスインスタンスの診断 ID。トークンと共にのみ存在。 |
| `lease_expires_at` | NULL を許容する timestamptz。DB 時刻によるリース期限。 |
| `last_attempt_at` | NULL を許容する timestamptz。最新規の処理権取得時刻。 |
| `dead_lettered_at` | NULL を許容する timestamptz。配送打切り失敗の印。原イベントは保持。 |
| `last_error_code` | NULL を許容する text。許可リスト化した短いエラー分類のみ。例外メッセージ、ペイロード、秘密値は保存しない。 |
| `traceparent`, `tracestate` | NULL を許容する text。**新規生成側が妥当性確認して埋める場合のみ** W3C 文脈として使用。旧行は空のまま。スキーマ追加を生成側全件の遡及変更と混同しない。 |

状態は `delivered_at`、`dead_lettered_at`、リースから一意に導く `processing_state` 読み取りモデル（ビューまたはアダプター射影）とする。`DELIVERED` は `delivered_at IS NOT NULL`、`DEAD_LETTER` は `dead_lettered_at IS NOT NULL`、`IN_FLIGHT` は未完了かつ `lease_token IS NOT NULL AND lease_expires_at > DB clock`、その他は `PENDING`。`IN_FLIGHT` の期限切れは回復対象の `PENDING` として扱う。`delivered_at` と `dead_lettered_at` の同時非 NULL、トークン/所有者/有効期限の片側だけ存在を CHECK で禁止する。既存の配送済み行は追加列が全て NULL のまま `DELIVERED` と判定され、既存保留行は `PENDING` と判定されるので、危険な全面埋戻しを要しない。

部分インデックスを `delivered_at IS NULL AND dead_lettered_at IS NULL` の候補に付け、`available_at` と `lease_expires_at` の選択性を実測する。DDL ロック時間、既存保留中/配送済み件数、`attempt_count >= max_attempts` の旧行数、マイグレーション前後の行/ハッシュ・件数、クエリ実行計画を記録する。旧行が試行上限に達していたら起動を拒否し、上限設定または原因を確認する。大きい既存表で索引作成が書込みを長く止める場合はオンライン索引作成を別段階的展開手順に分け、SQLx マイグレーションのトランザクション設定を確認してから適用する。既存マイグレーションを編集せず、失敗時は新ワーカーの起動を止め、旧行を保持する。[SQLx Migrator](https://docs.rs/sqlx/0.9.0/sqlx/migrate/struct.Migrator.html)。

`aggregate_version` は現行スキーマと生成側に共通の値がない。`resultingDocumentRevision` があるイベントと Folder/AccessPolicy イベントを同じ規則で埋められないため、P6 の順序/フェンシングに使わない。将来必要なら生成側ごとの型付きバージョン契約と既存を保持して追加する列を別途定義する。

<a id="4-claim配送ackfail-の-transaction-契約"></a>
## 4. 処理権取得・配送・配送確認/失敗確定のトランザクション契約

`crates/outbox-delivery` を Domain 非依存のライブラリとする。中核ライブラリに Search 型をインポートせず、P7 のプロセス構成の組み立て箇所が Search 接続処理を登録する。`OutboxStore` は PostgreSQL アダプター、`DeliveryHandler` は登録済み経路への非同期呼出し、`DeliveryRunner` は上限付き実行ループ。インターフェースは以下の形で固定し、具体的な Rust 非同期トレイト表現は実装計画で選ぶ。

```text
OutboxStore::claim(limit, lease_duration, owner) -> Vec<ClaimedEvent>
OutboxStore::renew(event_id, lease_token, lease_duration) -> FenceResult
OutboxStore::settle_success(event_id, lease_token) -> FenceResult
OutboxStore::settle_failure(event_id, lease_token, ErrorClass, RetryPolicy) -> FenceResult
OutboxStore::reap_exhausted(limit) -> count
DeliveryHandler::deliver(DeliveryEnvelope, DeliveryContext) -> DeliveryDecision
DeliveryRunner::run_until_shutdown(shutdown_signal) -> RunSummary
```

`FenceResult` は `Updated | Lost` を区別し、DB エラーは別 `Result` にする。処理権取得 / リース更新 / 配送確認 / 失敗確定 / 打切り状態へ移行は各々短い `READ COMMITTED` トランザクションまたは単一の SQL 文でコミットし、ハンドラーを DB トランザクション内で呼ばない。SQLx トランザクションは明示的にコミット/ロールバックする。[SQLx Transaction](https://docs.rs/sqlx/0.9.0/sqlx/struct.Transaction.html)。

1. **処理権取得**: `delivered_at IS NULL AND dead_lettered_at IS NULL AND available_at <= clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp()) AND attempt_count < max_attempts` の行を `ORDER BY available_at, occurred_at, event_id LIMIT batch_size FOR UPDATE SKIP LOCKED` で選ぶ CTE と `UPDATE ... FROM claimed ... RETURNING` を一つのトランザクションにする。`attempt_count += 1`、新 `lease_token` / `lease_owner` / `lease_expires_at` / `last_attempt_at` を設定し、コミット後に配送実行。トークンは処理権取得ごとに衝突しない UUID とし、イベント ID と合わせてフェンスとする。`max_attempts`、バッチ、リースは設定上限で検証する。
2. **処理**: `DeliveryEnvelope { event_id, event_type, aggregate_type, aggregate_id, occurred_at, payload }` と `DeliveryContext { attempt, lease_token, deadline, trace }` を渡す。汎用ハンドラーは `Applied | KnownNoop | Retryable(error_code) | Terminal(error_code)` を返す。`KnownNoop` は経路定義で明示されたイベントのみ。時間切れ、接続喪失、コンシューマー応答不明は再試行可能/結果不明とし、決して成功扱いしない。
3. **リース更新**: 長いハンドラーの間はリースの 1/3 程度の間隔で、`event_id = ? AND lease_token = ? AND lease_expires_at > clock_timestamp() AND delivered_at IS NULL AND dead_lettered_at IS NULL` を条件に DB 時刻で延長する。更新 0 行ならフェンスを失ったとして、そのハンドラー結果を配送確認/失敗確定に使わない。定期更新に失敗した場合も同様。延長の総時間に上限を置き、通常処理時間を測ってから値を凍結する。
4. **配送確認**: `Applied/KnownNoop` の後だけ、リース更新と同じフェンス条件付き `UPDATE` で `delivered_at = clock_timestamp()`、リース 3 列を NULL にし、コミットを確認して成功と呼ぶ。0 行は古いリース / 完了済みとして再確認する。コミット応答不明なら配送成功を推測せず DB を読み、未完了なら再配送する。
5. **失敗確定**: 同じフェンス条件付き `UPDATE` でエラーコードを記録。再試行可能かつ `attempt_count < max_attempts` ならリースを消し、`available_at = DB clock + bounded exponential backoff + deterministic jitter` に更新する。打切り判定または上限到達なら `dead_lettered_at = DB clock` としてリースを消す。どちらも原イベントを削除しない。0 行なら旧ワーカーは何も上書きしない。
6. **回収**: 期限切れで `attempt_count >= max_attempts` の行は別の短いフェンス・ロック付き更新で `DEAD_LETTER` にする。上限未達の期限切れ行は次の処理権取得対象。回収処理が止まれば警報対象であり、黙って保留中として無期限に隠さない。

`clock_timestamp()` はトランザクション開始時刻で固定される `now()` と異なり SQL 文中も進むため、リース期限判定に使用する。ワーカーホストの時刻を DB フェンシングの正本としない。[PostgreSQL 日時関数](https://www.postgresql.org/docs/current/functions-datetime.html)。DB の行ロックは処理権取得トランザクションの間だけで、ハンドラーの外部副作用はフェンシングできない。**保証は少なくとも一度** であり正確に一度だけではない。リース期限とワーカー異常終了の間に重複が起こるため、コンシューマーの冪等性が必須である。

初期設定候補は `batch_size <= 32`、`max_in_flight <= 8`、リース 120 秒、リース更新 30 秒、再試行待機 1 秒〜5 分、`max_attempts = 8`。これらは製品測定値ではなく資格確認用の上限付き出発値。Search は全 Source 再構築なので、最初の接続は **Source あたり同時処理 1** に絞る。ワーカー多台の処理権取得安全性は別途実 DB で証明する。P7 の共有永続実行基盤/CAS が成立するまで、プロセス内限定 `MemoryDocumentIndexRuntime` を複数本番の複製インスタンスに配備しない。

<a id="5-search-bridge-と-durable-receipt"></a>
## 5. Search 接続処理と永続処理記録

`crates/search-source-document/src/delivery.rs` に接続処理を置く。`aggregate_type` は現行生成側の `Document/Folder/AccessPolicy` とイベント型の対応を明示的な許可リストで検証する（イベント型一覧は `DocumentIndexingService::relevant_event_type` と単一契約化する）。接続処理はペイロードを Search へ渡さず、`DocumentSourceEvent` を構成して既存 `DocumentIndexingService::handle` を呼ぶ。`Published/Unchanged/Duplicate` は成功、`Ignored` は明示的な無処理経路以外では `Terminal(unsupported_event)`、`SourceUnavailable` と索引化/処理記録失敗は再試行可能とする。未知・不正配送データは打切りで可視化し、他のイベントの配送を止めない。誤った型で `Ignored` を返すことを成功にしない。

Search のイベント処理記録は `outbox_events.delivered_at` と別の Search 所有記録である。現行 `IndexingReceiptStore` のメモリ実装は試験用であり、P6/P7 の実接続には PostgreSQL などの永続的なストア（イベント ID PK、索引世代キー、ダイジェスト、保存時刻）を `crates/search-source-document/src/receipt_postgres.rs` に実装する。射影公開後に処理記録保存が失敗した場合は配送確認せず再試行し、既存索引世代のダイジェスト/CAS と処理記録を再照合して収束させる。処理記録の存在だけで索引成果物の存在・整合を推定しない。P3 の永続的な Graph と P7 の共有索引世代実行基盤に接続して、再起動後も処理記録が指す索引世代が読めることを本番適合性確認条件とする。

単一イベントの `delivered_at` は Search ハンドラーの成功と DB 配送確認コミットの証拠に限定する。Search の索引構築が失敗しても Document のコミットとイベント行は維持する。手動 `rebuild()` はイベント配送とは独立した復旧経路であり、再構築成功だけで任意の保留中イベントを配送確認しない。

## 6. 障害・運用・セキュリティ

| 障害発生箇所 | 必要な状態と回復 |
| --- | --- |
| 生成側コミット前/後 | ロールバックならイベントなし。コミット済みならワーカー停止中もイベントが残る。 |
| 処理権取得コミット前/後 | 前なら処理権の取得前。後の異常終了ならリース失効後に処理権を再取得。 |
| Search 公開成功、処理記録/配送確認失敗 | 配送未確認のまま再配送。Search の現行索引世代と永続処理記録で冪等に収束。 |
| 配送確認コミット応答喪失 | DB の `delivered_at` を再読。未確認なら再配送を許容。 |
| リース失効後に旧ハンドラーが返る | 旧トークンの配送確認/失敗確定は 0 行。外部副作用は重複し得るため Search CAS/冪等性で扱う。 |
| 連続失敗、処理不能なイベント | 上限付き再試行の後、原行を `DEAD_LETTER` として保持。エラー分類、発生時刻、試行数を観測する。再投入は経路/原因修正後の監査付き運用担当者操作として別途設計し、自動でペイロード改変しない。 |
| DB/コンシューマー障害 | 新規の処理権取得を止めて上限付き再接続・再試行待機。処理中リースを更新できなければ結果を不明とし、回復後に再配送。 |

実行ループは `poll -> bounded claim -> bounded concurrent dispatch -> renew -> fenced settle`。次のポーリング前に未決イベント全件が終わることを要求せずセマフォで同時実行数を制御する。SIGTERM/CTRL-C 時は新規の処理権取得を止め、処理中のリース更新を続けながら一定時間終了待機し、完了分のみ配送確認/失敗確定する。期限内に終わらないものは成功にせずリース失効に委ねる。Search の索引構築の非同期処理を途中で打ち切った場合に未公開準備領域が残り得るため、P7 の索引世代の後片付け/整合化を検証する。

ワーカー DB 接続主体は Domain イベントの `SELECT` と配送管理列の `UPDATE` のみに絞り、Document 正本の変更権限と Audit Outbox の更新権限を渡さない。Search 接続処理の正本スナップショット読取りと処理記録保存には別の最小権限接続プールを使う。経路は静的登録し、イベントペイロードで実行ファイル/URL を動的に選ばない。DB 上の JSONB ペイロードはハンドラーに必要な場合だけ渡し、Search 接続処理は転送しない。ログ/トレースにはイベント ID、経路、試行、エラーコードを最小限だけ記録し、ペイロード、文書名、操作主体、秘密値を出さない。`traceparent`/`tracestate` は W3C 形式・長さを妥当性確認後だけスパンに結び、旧行は新規スパンとイベント ID で相関する。旧生成側からの端から端までのトレースは未達として明記する。メトリクスラベルにイベント/文書/主体/トレース ID を使わない。

最低限必要なメトリクスは保留中/処理中/配送打切り件数、最古未完了イベントの経過時間、処理権取得/配送確認/再試行/リース失効/旧フェンス件数、ハンドラー実行時間、コミットから配送確認までの遅延、DB/コンシューマーエラー分類。Search 側の索引化済みバージョン、索引更新の遅延、最後の成功は別メトリクスとして Source のスナップショット/索引世代から計算する。`outbox_events` のペイロードや `occurred_at` だけから Search の鮮度を証明しない。

<a id="7-実装-artifact-と-qualification"></a>
## 7. 実装成果物と資格確認

現行タスクグラフの `p6-implement.write_scope = crates/outbox-delivery` だけではマイグレーション、Search 接続処理、処理記録、ワークスペース登録を変更できない。**凍結時に**親が `P6 generic worker`、`P6 Search adapter`、`P6 shared migration/workspace integration` を別タスクとして編集範囲分離し、依存順を確定してから実装作業を割り当てる。マイグレーション番号と共有 `Cargo.toml`/`Cargo.lock` の編集権は親が単独の編集担当に割り当てる。今回の設計ワーカーは文書以外を書かない。

1. `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql`: 既存を保持して追加する列、CHECK、候補選択用インデックス。`crates/document-repository-postgres/src/lib.rs::migrate` の既存埋め込みマイグレーション経路を維持する。旧来の保留行 / 配送済み行を含むマイグレーションロールバック・再実行試験。
2. `crates/outbox-delivery/Cargo.toml`, `src/lib.rs`, `src/postgres.rs`, `src/runner.rs`: 汎用ストア/ハンドラー/経路、処理権取得/リース更新/状態確定、上限設定、終了処理。汎用ライブラリは Search に依存しない。P7 の静的なプロセス構成の組み立て箇所が Search アダプターと永続実行基盤を結ぶ。`Cargo.toml`/`Cargo.lock` のワークスペース登録は共有統合タスクで行う。
3. `crates/search-application/src/indexing_service.rs`, `crates/search-source-document/src/delivery.rs`, `src/receipt_postgres.rs`, `src/lib.rs`: Search 接続処理、イベント型契約、永続 Search 処理記録。処理記録表は Search 側のマイグレーション正本に置き、P7 の永続的な索引世代ストアと同一 DB に置くかを凍結で決める。Document の `outbox_events` と同じ表へ混在させない。
4. `crates/outbox-delivery/tests/postgres_delivery.rs`: 実 PostgreSQL フィクスチャで 2/4/8 ワーカーの同時に有効な処理権の非重複、バッチ上限、有効期限/再取得、旧トークン配送確認/失敗確定/リース更新 0 行、再試行/再試行待機、最大試行後 DLQ、配送確認コミットの結果不明、DB 障害、安全な終了を検証する。リース失効後の重複は許容し、同時に有効なリースの重複は許容しない。
5. `crates/search-source-document/tests/outbox_delivery.rs` と既存 `tests/outbox_indexing.rs`: 実際の生成側イベント → 汎用処理権取得 → Search 接続処理 → 処理記録 → フェンス付き配送確認の縦断、重複/逆順、Search 失敗時の配送未確認、公開後の処理記録保存失敗、ワーカー再起動後の保留行の回復、T10・権限失効の最新正本再読を検証する。**索引/処理記録成果物自体のプロセス再起動後整合は P7 共有永続実行基盤の縦断試験で別途証明する。** Audit Outbox の `delivered_at` と Document 正本が変わらない検証条件を含める。
6. `spec/data/transaction-consistency-requirements-v0.md` と運用仕様の差分は、`processing_state` の読み取りモデル、単一配送先、順序/aggregate_version の非保証、DLQ とトレースの範囲を凍結時に規範化する。未承認の暗黙変更として扱わない。

性能は「汎用キュー」と「Search 全 Source 再構築」を分けて測る。合成 1千/1万イベント、1/4/8 ワーカー、バッチ 1/16/32、ハンドラー 0/10/100 ms の組合せから代表点を選び、3 回以上の定常状態での実行で events/s、p50/p95/p99 処理権取得・配送確認遅延、DB CPU/IO/ロック待機、リース更新漏れ、滞留処理の解消を記録する。次に合成 1千/1万 Document の実 Search ハンドラーでイベント集中発生と再起動、T10/権限失効を測り、Source 内の件数に対する再構築コストとコミットから可視化までの遅延を記録する。正確なコード head、PostgreSQL/SQLx 版、CPU/メモリ、フィクスチャのシード、並列度を残し、測定前に SLO 達成や本番処理能力を宣言しない。失敗注入と回復の試験は対象を限定したゲートとし、CI 全体を習慣的に反復しない。

P6 の完了判定は汎用ワーカーの永続性/フェンシングと Search 接続処理の実 DB 縦断、独立レビュー、測定の検証記録が揃った時点に限る。P7 の永続実行基盤・配備/識別情報/秘密値・運用 SLO は別の確認事項である。マージ/デプロイや本番マイグレーションは本設計文書の作成では行わない。
