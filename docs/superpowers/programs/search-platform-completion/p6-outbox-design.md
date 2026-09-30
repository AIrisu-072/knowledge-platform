# P6 — Durable Outbox Delivery Worker 設計案

- Status: **DRAFT / architecture review 前**（Design Freeze・実装・運用認定ではない）
- Date: 2026-09-30 JST
- Baseline: Completion Program P0 closure、Search / Discovery Platform v0 Phase D の D4 consumer
- Goal: committed Domain Outbox を generic worker が at-least-once で Search に配送し、配送状態・再試行・障害回復を worker が所有する。

## 1. 現行契約と変更境界

| 根拠 | 現状 | P6 で守る境界 |
| --- | --- | --- |
| `crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql:74` | `outbox_events(event_id PK, event_type, aggregate_type, aggregate_id, payload JSONB, occurred_at, available_at, attempt_count DEFAULT 0, delivered_at NULL)`。lease、DLQ、`aggregate_version`、trace context 列はない。別の `audit_outbox_events` もある。 | 既存行と producer の insert を壊さない additive migration。Domain の `outbox_events` だけを本 P6 の対象にする。Audit Store 配送・Audit 行の ack は別 capability。 |
| `crates/document-repository-postgres/src/repository.rs:178`、`targeted_events.rs:18`、`versioning_mutation.rs:556`、`publication_end.rs:347` | Document / Folder / AccessPolicy の変更と Domain / Audit event は同一 transaction で記録される。targeted producer は省略列の既定値を使う。 | Document の業務 transaction に Search 処理を入れない。event identity、payload、発生時刻は worker が書き換えない。 |
| `crates/search-application/src/indexing_service.rs:13` | `DocumentSourceEvent` は event ID/type、aggregate ID、発生時刻だけを要求する。`DocumentIndexingService::handle` は未知の Document 系 event を error とし、`Published/Unchanged/Duplicate/Ignored` を返す。 | bridge が `aggregate_type` と event type を検証してから呼ぶ。成功 outcome の意味を明確化し、未知 event を `Ignored` として黙って ack しない。 |
| `crates/search-source-document/src/outbox.rs:47,642,925` | D4 は現行 Source snapshot を再読し、generation CAS と event receipt で duplicate/reorder に収束する。`delivered_at` を更新しない。receipt store は trait で、現行試験は memory 実装。 | delivery worker だけが `attempt_count/available_at/delivered_at/lease/DLQ` を更新する。Search は自身の projection と receipt を所有する。 |
| `crates/search-source-document/src/postgres.rs:132`、`docs/superpowers/execution/search-discovery-platform-v0-acceptance.md` | 正本 snapshot は単一 PostgreSQL read-only repeatable-read transaction。Phase D の実装/hosted evidence は D4 までで、generic worker、durable Search runtime、配備は未実施。 | P6 単独のテスト成功を production Search serving 完了と呼ばない。P3/P7 の共有 durable generation/receipt と最終接続が必要。 |
| `spec/data/transaction-consistency-requirements-v0.md:756`、`spec/operations/observability-audit-requirements-v0.md:420` | at-least-once、idempotent consumer、retry、failed/dead-letter、outbox lag 可視化が要求される。規範は `aggregate_version` と `processing_state` も列挙するが、現行物理表にはない。 | 本 P6 は既存 event を並べ替え不能な versioned stream と見なさない。`processing_state` は後述の明示的な read model で提供する。`aggregate_version` を既存 payload から一律推測・backfill しない。規範との表現差は Freeze 時に明記して整合させる。 |

`outbox_events.delivered_at` は現行の単一論理配送先の完了印である。P6 v0 は登録済み Domain event を Search bridge に渡す **一つの必須配送経路**としてこれを解釈する。第二の独立 subscriber が必要になったら `(event_id, destination_id)` を鍵とする配送状態表を別設計し、一つの `delivered_at` を複数宛先の成功と混同しない。`audit_outbox_events` の `delivered_at` は一切触らない。

## 2. 案の比較と決定

| 案 | 判定 | 理由 |
| --- | --- | --- |
| `SELECT ... FOR UPDATE` の row lock を Search indexing 完了まで保持 | 不採用 | 全 Source 再読・index build の期間だけ業務 DB 接続/lock が残り、障害・長時間処理に弱い。 |
| `LISTEN/NOTIFY` のみ、または process memory queue | 不採用 | 通知・process の喪失後に committed event を回復できない。通知を将来の起床ヒントにすることは可能。 |
| 別 CDC/broker を P6 の必須経路にする | 今回は不採用 | Source DB と connector の運用・offset 整合が別 capability となる。現行 schema と SQLx/PostgreSQL で必要な耐久性を実証する。 |
| **短い DB transaction で `FOR UPDATE SKIP LOCKED` claim、期限付き lease、token 付き ack/fail** | **採用** | worker 複数台の claim を競合なく分散し、process crash 後は lease expiry から回復できる。長い Search 処理中は DB row lock を保持しない。 |

PostgreSQL は `SKIP LOCKED` を queue-like table の複数 consumer に利用可能と説明する一方、取得順の完全な整合 view は保証しない。よって `available_at, occurred_at, event_id` は claim の優先順であって、aggregate ごとの厳密な順序保証ではない。Search D4 は古い event でも最新正本を再読する。[PostgreSQL SELECT](https://www.postgresql.org/docs/current/sql-select.html)、[UPDATE](https://www.postgresql.org/docs/current/sql-update.html)。

## 3. Additive persistence と状態機械

`crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql` へ以下を追加する。既存の event 列・PK・行は維持し、削除、履歴の一括初期化、`delivered_at` の書換えはしない。

| 追加列 | 型 / 意味 |
| --- | --- |
| `lease_token` | nullable UUID。claim ごとに新しい UUID。ack/fail/renew の fencing token。 |
| `lease_owner` | nullable UUID。worker process instance の診断 ID。token と共にのみ存在。 |
| `lease_expires_at` | nullable timestamptz。DB clock による lease 期限。 |
| `last_attempt_at` | nullable timestamptz。最新 claim 時刻。 |
| `dead_lettered_at` | nullable timestamptz。terminal failure の印。原 event は保持。 |
| `last_error_code` | nullable text。allowlist 化した短い error class のみ。例外 message、payload、秘密値は保存しない。 |
| `traceparent`, `tracestate` | nullable text。**新規 producer が妥当性確認して埋める場合のみ** W3C context として使用。旧行は空のまま。schema 追加を producer 全件の遡及変更と混同しない。 |

状態は `delivered_at`、`dead_lettered_at`、lease から一意に導く `processing_state` read model（view または adapter projection）とする。`DELIVERED` は `delivered_at IS NOT NULL`、`DEAD_LETTER` は `dead_lettered_at IS NOT NULL`、`IN_FLIGHT` は未完了かつ `lease_token IS NOT NULL AND lease_expires_at > DB clock`、その他は `PENDING`。`IN_FLIGHT` の期限切れは回復対象の `PENDING` として扱う。`delivered_at` と `dead_lettered_at` の同時非 NULL、token/owner/expiry の片側だけ存在を CHECK で禁止する。既存の delivered 行は追加列が全て NULL のまま `DELIVERED` と判定され、既存 pending 行は `PENDING` と判定されるので、危険な全面 backfill を要しない。

partial index を `delivered_at IS NULL AND dead_lettered_at IS NULL` の候補に付け、`available_at` と `lease_expires_at` の選択性を実測する。DDL lock 時間、既存 pending/delivered 件数、`attempt_count >= max_attempts` の旧行数、migration 前後の row/hash・count、query plan を記録する。旧行が試行上限に達していたら起動を拒否し、上限設定または原因を確認する。大きい既存表で index 作成が書込みを長く止める場合はオンライン index 作成を別 rollout step に分け、SQLx migration の transaction 設定を確認してから適用する。既存 migration を編集せず、失敗時は新 worker の起動を止め、旧行を保持する。[SQLx Migrator](https://docs.rs/sqlx/0.9.0/sqlx/migrate/struct.Migrator.html)。

`aggregate_version` は現行 schema と producer に共通の値がない。`resultingDocumentRevision` がある event と Folder/AccessPolicy event を同じ規則で埋められないため、P6 の ordering/fencing に使わない。将来必要なら producer ごとの型付き version 契約と additive 列を別途定義する。

## 4. Claim・配送・ack/fail の transaction 契約

`crates/outbox-delivery` を Domain 非依存の library とする。core library に Search 型を import せず、P7 の process composition root が Search bridge を登録する。`OutboxStore` は PostgreSQL adapter、`DeliveryHandler` は登録済み route への async invocation、`DeliveryRunner` は bounded run loop。interface は以下の形で固定し、具体的な Rust async trait 表現は実装計画で選ぶ。

```text
OutboxStore::claim(limit, lease_duration, owner) -> Vec<ClaimedEvent>
OutboxStore::renew(event_id, lease_token, lease_duration) -> FenceResult
OutboxStore::settle_success(event_id, lease_token) -> FenceResult
OutboxStore::settle_failure(event_id, lease_token, ErrorClass, RetryPolicy) -> FenceResult
OutboxStore::reap_exhausted(limit) -> count
DeliveryHandler::deliver(DeliveryEnvelope, DeliveryContext) -> DeliveryDecision
DeliveryRunner::run_until_shutdown(shutdown_signal) -> RunSummary
```

`FenceResult` は `Updated | Lost` を区別し、DB error は別 `Result` にする。claim / renew / ack / fail / terminal 化は各々短い `READ COMMITTED` transaction または単一 statement で commit し、handler を DB transaction 内で呼ばない。SQLx transaction は明示的に commit/rollback する。[SQLx Transaction](https://docs.rs/sqlx/0.9.0/sqlx/struct.Transaction.html)。

1. **Claim**: `delivered_at IS NULL AND dead_lettered_at IS NULL AND available_at <= clock_timestamp() AND (lease_expires_at IS NULL OR lease_expires_at <= clock_timestamp()) AND attempt_count < max_attempts` の行を `ORDER BY available_at, occurred_at, event_id LIMIT batch_size FOR UPDATE SKIP LOCKED` で選ぶ CTE と `UPDATE ... FROM claimed ... RETURNING` を一つの transaction にする。`attempt_count += 1`、新 `lease_token` / `lease_owner` / `lease_expires_at` / `last_attempt_at` を設定し、commit 後に dispatch。token は claim ごとに衝突しない UUID とし、event ID と合わせて fence とする。`max_attempts`、batch、lease は設定上限で検証する。
2. **Handle**: `DeliveryEnvelope { event_id, event_type, aggregate_type, aggregate_id, occurred_at, payload }` と `DeliveryContext { attempt, lease_token, deadline, trace }` を渡す。generic handler は `Applied | KnownNoop | Retryable(error_code) | Terminal(error_code)` を返す。`KnownNoop` は route 定義で明示された event のみ。timeout、connection loss、consumer response 不明は retryable/unknown とし、決して成功扱いしない。
3. **Renew**: 長い handler の間は lease の 1/3 程度の間隔で、`event_id = ? AND lease_token = ? AND lease_expires_at > clock_timestamp() AND delivered_at IS NULL AND dead_lettered_at IS NULL` を条件に DB clock で延長する。更新 0 行なら fence を失ったとして、その handler 結果を ack/fail に使わない。heartbeat に失敗した場合も同様。延長の総時間に上限を置き、通常処理時間を測ってから値を凍結する。
4. **Ack**: `Applied/KnownNoop` の後だけ、renew と同じ fence 条件付き `UPDATE` で `delivered_at = clock_timestamp()`、lease 3 列を NULL にし、commit を確認して成功と呼ぶ。0 行は stale lease / already completed として再確認する。commit 応答不明なら delivery 成功を推測せず DB を読み、未完了なら再配送する。
5. **Fail**: 同じ fence 条件付き `UPDATE` で error code を記録。retryable かつ `attempt_count < max_attempts` なら lease を消し、`available_at = DB clock + bounded exponential backoff + deterministic jitter` に更新する。terminal 判定または上限到達なら `dead_lettered_at = DB clock` として lease を消す。どちらも原 event を削除しない。0 行なら旧 worker は何も上書きしない。
6. **Reap**: 期限切れで `attempt_count >= max_attempts` の行は別の短い fenced/locked 更新で `DEAD_LETTER` にする。上限未達の期限切れ行は次の claim 対象。reaper が止まれば警報対象であり、黙って pending として無期限に隠さない。

`clock_timestamp()` は transaction 開始時刻で固定される `now()` と異なり statement 中も進むため、lease 期限判定に使用する。worker host の clock を DB fencing の正本としない。[PostgreSQL date/time functions](https://www.postgresql.org/docs/current/functions-datetime.html)。DB の row lock は claim transaction の間だけで、handler の外部 side effect は fencing できない。**保証は at-least-once** であり exactly-once ではない。lease 期限と worker crash の間に重複が起こるため、consumer の idempotency が必須である。

初期設定候補は `batch_size <= 32`、`max_in_flight <= 8`、lease 120 秒、renew 30 秒、backoff 1 秒〜5 分、`max_attempts = 8`。これらは製品測定値ではなく qualification 用の上限付き出発値。Search は全 Source 再構築なので、最初の接続は **source あたり in-flight 1** に絞る。worker 多台の claim 安全性は別途実 DB で証明する。P7 の shared durable runtime/CAS が成立するまで、process-local `MemoryDocumentIndexRuntime` を複数 production replica に配備しない。

## 5. Search bridge と durable receipt

`crates/search-source-document/src/delivery.rs` に bridge を置く。`aggregate_type` は現行 producer の `Document/Folder/AccessPolicy` と event type の対応を明示 allowlist で検証する（event type 一覧は `DocumentIndexingService::relevant_event_type` と単一契約化する）。bridge は payload を Search へ渡さず、`DocumentSourceEvent` を構成して既存 `DocumentIndexingService::handle` を呼ぶ。`Published/Unchanged/Duplicate` は成功、`Ignored` は explicit no-op route 以外では `Terminal(unsupported_event)`、`SourceUnavailable` と indexing/receipt failure は retryable とする。未知・不正 envelope は terminal で可視化し、他の event の配送を止めない。誤った型で `Ignored` を返すことを成功にしない。

Search の event receipt は `outbox_events.delivered_at` と別の Search 所有記録である。現行 `IndexingReceiptStore` の memory 実装は試験用であり、P6/P7 の実接続には PostgreSQL などの durable store（event ID PK、generation key、digest、保存時刻）を `crates/search-source-document/src/receipt_postgres.rs` に実装する。projection publish 後に receipt 保存が失敗した場合は ack せず retry し、既存 generation の digest/CAS と receipt を再照合して収束させる。receipt の存在だけで index artifact の存在・整合を推定しない。P3 の durable Graph と P7 の shared generation runtime に接続して、再起動後も receipt が指す generation が読めることを production qualification 条件とする。

単一 event の `delivered_at` は Search handler の成功と DB ack commit の証拠に限定する。Search build が失敗しても Document の commit と event row は維持する。手動 `rebuild()` は event delivery とは独立した復旧経路であり、rebuild 成功だけで任意の pending event を ack しない。

## 6. 障害・運用・セキュリティ

| failure point | 必要な状態と回復 |
| --- | --- |
| producer commit 前/後 | rollback なら event なし。commit 済みなら worker 停止中も event が残る。 |
| claim commit 前/後 | 前なら未 claim。後の crash なら lease expiry 後に再 claim。 |
| Search publish 成功、receipt/ack 失敗 | 未 ack のまま再配送。Search の現行 generation と durable receipt で idempotent に収束。 |
| ack commit 応答喪失 | DB の `delivered_at` を再読。未確認なら再配送を許容。 |
| lease 失効後に旧 handler が返る | 旧 token の ack/fail は 0 行。外部 side effect は重複し得るため Search CAS/idempotency で扱う。 |
| 連続 failure、poison event | bounded retry の後、原行を `DEAD_LETTER` として保持。error class、発生時刻、試行数を観測する。再投入は route/原因修正後の監査付き operator 操作として別途設計し、自動で payload 改変しない。 |
| DB/consumer outage | 新 claim を止めて bounded reconnect/backoff。処理中 lease を更新できなければ結果は unknown とし、回復後に再配送。 |

run loop は `poll -> bounded claim -> bounded concurrent dispatch -> renew -> fenced settle`。次の poll 前に未決 event 全件が終わることを要求せず semaphore で同時数を制御する。SIGTERM/CTRL-C 時は新 claim を止め、in-flight の lease 更新を続けながら一定時間 drain し、完了分のみ ack/fail する。期限内に終わらないものは成功にせず lease expiry に委ねる。Search build の future を途中で打ち切った場合に未公開 staging が残り得るため、P7 の generation cleanup/reconciliation を検証する。

worker DB principal は Domain event の `SELECT` と配送管理列の `UPDATE` のみに絞り、Document 正本の mutation 権限と Audit outbox の更新権限を渡さない。Search bridge の正本 snapshot 読取りと receipt 保存には別の最小権限 pool を使う。route は静的登録し、event payload で executable/URL を動的に選ばない。DB 上の JSONB payload は handler に必要な場合だけ渡し、Search bridge は転送しない。log/trace には event ID、route、attempt、error code を最小限だけ記録し、payload、文書名、actor、秘密値を出さない。`traceparent`/`tracestate` は W3C 形式・長さを妥当性確認後だけ span に結び、旧行は新規 span と event ID で相関する。旧 producer からの end-to-end trace は未達として明記する。metric label に event/document/principal/trace ID を使わない。

最低 metric は pending/in-flight/dead-letter 件数、最古未完了 event の age、claim/ack/retry/lease-expired/stale-fence 件数、handler duration、commit-to-ack lag、DB/consumer error class。Search 側の indexed version、index lag、最後の成功は別 metric として source snapshot/generation から計算する。`outbox_events` の payload や `occurred_at` だけから Search の freshness を証明しない。

## 7. 実装 artifact と qualification

現行 task graph の `p6-implement.write_scope = crates/outbox-delivery` だけでは migration、Search bridge、receipt、workspace 登録を変更できない。**Freeze 時に**親が `P6 generic worker`、`P6 Search adapter`、`P6 shared migration/workspace integration` を別 task として write scope 分離し、依存順を確定してから実装を dispatch する。migration 番号と shared `Cargo.toml`/`Cargo.lock` の編集権は親が単一 writer に割り当てる。今回の design worker は文書以外を書かない。

1. `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql`: additive 列、CHECK、candidate index。`crates/document-repository-postgres/src/lib.rs::migrate` の既存 embedded migration 経路を維持する。旧 pending / delivered を含む migration rollback・再実行試験。
2. `crates/outbox-delivery/Cargo.toml`, `src/lib.rs`, `src/postgres.rs`, `src/runner.rs`: generic store/handler/route、claim/renew/settle、bound 設定、shutdown。generic library は Search に依存しない。P7 の static process composition root が Search adapter と durable runtime を結ぶ。`Cargo.toml`/`Cargo.lock` の workspace 登録は shared integration task で行う。
3. `crates/search-application/src/indexing_service.rs`, `crates/search-source-document/src/delivery.rs`, `src/receipt_postgres.rs`, `src/lib.rs`: Search bridge、event type 契約、durable Search receipt。receipt table は Search 側の migration 正本に置き、P7 の durable generation store と同一 DB に置くかを Freeze で決める。Document の `outbox_events` と同じ表へ混在させない。
4. `crates/outbox-delivery/tests/postgres_delivery.rs`: 実 PostgreSQL fixture で 2/4/8 worker の disjoint live claims、batch cap、expiry/reclaim、旧 token ack/fail/renew 0 行、retry/backoff、最大試行後 DLQ、unknown ack commit、DB outage、graceful shutdown を検証する。lease expiry 後の重複は許容し、同時有効 lease の重複は許容しない。
5. `crates/search-source-document/tests/outbox_delivery.rs` と既存 `tests/outbox_indexing.rs`: 実 producer event → generic claim → Search bridge → receipt → fenced ack の縦断、重複/逆順、Search 失敗時の未 ack、publish 後 receipt 失敗、worker restart 後の pending 回復、T10・access revocation の最新正本再読を検証する。**index/receipt artifact 自体の process restart 後整合は P7 shared durable runtime の縦断試験で別途証明する。** Audit outbox の `delivered_at` と Document 正本が変わらない assertion を含める。
6. `spec/data/transaction-consistency-requirements-v0.md` と運用 spec の差分は、`processing_state` の read model、単一配送先、ordering/aggregate_version の非保証、DLQ と trace の範囲を Freeze 時に規範化する。未承認の暗黙変更として扱わない。

性能は「generic queue」と「Search 全 Source 再構築」を分けて測る。合成 1千/1万 event、1/4/8 worker、batch 1/16/32、handler 0/10/100 ms の組合せから代表点を選び、3 回以上の steady-state run で events/s、p50/p95/p99 claim・ack latency、DB CPU/IO/lock wait、lease renew miss、backlog drain を記録する。次に合成 1千/1万 Document の実 Search handler で event burst と再起動、T10/revocation を測り、source 件数に対する rebuild cost と commit-to-visible lag を記録する。exact code head、PostgreSQL/SQLx 版、CPU/メモリ、fixture seed、並列度を残し、測定前に SLO 達成や production capacity を宣言しない。失敗注入と recovery の試験は focused gate とし、full CI を習慣的に反復しない。

P6 の完了判定は generic worker の durability/fencing と Search bridge の実 DB 縦断、独立 review、測定 receipt が揃った時点に限る。P7 の durable runtime・配備/identity/secret・運用 SLO は別の確認事項である。merge/deploy や本番 migration は本設計文書の作成では行わない。
