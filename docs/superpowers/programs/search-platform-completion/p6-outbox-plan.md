# P6 Durable Outbox Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or `superpowers:executing-plans` task by task. Each task has a focused RED/GREEN cycle and an independent reviewer gate. The Completion Program already authorizes implementation; no intermediate human approval is added.

**Goal:** Commit 済み Domain outbox を generic worker が at-least-once で配送し、Search の Source fence・条件付き公開・durable receipt と整合させる。

**Architecture:** `outbox-delivery` は配送状態だけを所有し、短い PostgreSQL claim/renew/settle/reap と bounded runner を提供する。Search bridge は claim 前に P7 の分散 Source lease を取得し、不可視 staging の後に outbox fence と Source fence を同じ DB transaction で検査して pointer と receipt を確定する。generic ack はその commit を確認した後だけ実行する。

**Tech Stack:** Rust 2024 / 1.98、既存 workspace の SQLx 0.9、Tokio 1、PostgreSQL 18.6 fixture（最低 14）、testcontainers 0.28。

**Spec:** [`p6-outbox-freeze.md`](p6-outbox-freeze.md)、exact revised design SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`、independent GO SHA-256 `9728fffa22b501ada2f408735a3a325b592a2ba63b4d808e7ac800a5649be88c`。元設計 §3, §6–7 と `spec/data/transaction-consistency-requirements-v0.md`、`spec/operations/observability-audit-requirements-v0.md` も読む。

## Global Constraints

- `outbox_events` の event identity、payload、既存行、`delivered_at` と producer insert を保持する。`audit_outbox_events` は別経路。`aggregate_version` を異種 payload から推測して埋めない。
- generic crate は Search / P7 crate を import しない。generic だけが `attempt_count`、`available_at`、lease、DLQ、`delivered_at` を更新する。Search はこの表を ack しない。
- Search v0 は一つの設定済み Document `SourceId`、Source 当たり全 process で reconcile 1 件、Source lease **取得後**に claim 1 件。待機 process は claim しない。generic batch 上限は 32、in-flight 上限は 8。
- DB policy は revision と `max_attempts`、lease/backoff 範囲を一行で固定する。初回 claim が行の `attempt_limit` を固定する。policy 不一致または旧 `attempt_count >= max_attempts AND attempt_limit IS NULL` は起動/claim/reap を fail closed にする。
- `READ COMMITTED` の短い transaction、statement 内一回の `clock_timestamp()`、`FOR UPDATE SKIP LOCKED` を使う。処理中に Domain row lock を保持しない。0 行の fence は `Lost`、DB error/commit 応答不明は `Unknown` として区別する。
- Search の event publish は outbox row → `search_source_coordination` Source row → generation key 順 → P3 build guard → evaluation lease → Search receipt の順に lock する。manual rebuild は outbox row を持たず Source から始める。P3/P7 共有 `source_control` は物理表 `search_source_coordination` に対応し、同じ `pointer_revision` / `build_fence_seq` namespace を使う。P3 の pending incremental build guard は別契約として維持する。
- Search pointer、receipt、Source lease、Domain outbox は v0 で同一 PostgreSQL database。Projection/Unit/coverage/lexical/Graph は同一 manifest と P1 versioned bundle receipt の durable READY を CAS 前に確認し、staging は event/epoch に束縛する。current / evaluation pin / 有効 build guard を cleanup が削除しない。期限切れ guard の unpublished target は Source→sorted generation→guard→evaluation lease を lock して `DELETING`、**guard DELETE、target child DELETE、target generation DELETE** を同一 transaction で行う（P3 FK-safe correction）。
- `Published`、`Unchanged`、`Duplicate` は二つの fence と current READY/digest と receipt の同一 transaction 成功が必要。`Ignored` は v0 の明示 no-op route がないため成功にしない。unknown Document event は terminal、Source/DB/consumer 応答不明は retryable/unknown。
- qualification 用の初期値は `max_attempts=8`、lease 120 s、renew 30 s、backoff 1–300 s、総処理 15 min、drain 30 s。いずれも製品 SLO ではない。資格付けで値を変える場合は DB policy revision と試験を揃える。
- `sqlx.workspace` / `tokio.workspace` / `testcontainers.workspace` など既存の version pin を再利用し、未認定の新 dependency を入れない。`tokio` の `signal` feature 追加は共有 writer が単独で行う。API は [SQLx 0.9 transaction](https://docs.rs/sqlx/0.9.0/sqlx/struct.Transaction.html) と [PostgreSQL row lock](https://www.postgresql.org/docs/current/explicit-locking.html) に合わせる。
- Draft PR まで。merge、deploy、本番 migration は含めない。1 Task の RED/GREEN と read-only review を終えてから次に進み、最終 verification は一度の focused gate と exact-head hosted gate に分ける。

## File and ownership map

| File / scope | Responsibility / sole writer |
| --- | --- |
| `crates/outbox-delivery/Cargo.toml`, `src/lib.rs`; `crates/search-runtime/Cargo.toml`, `src/lib.rs` | `P6-G00` が package files だけを作る。root Cargo writer は package manifest の存在を確認してから登録する。 |
| Root `Cargo.toml`, `Cargo.lock` | `P6-I01` だけが workspace membership / Tokio signal feature / lock を編集する専用 task。P3/P7 の Cargo 変更と同時に書かず、後続 P7 writer に引き継ぐ。 |
| `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql`, `tests/outbox_delivery_migration.rs` | `P6-I02` だけが Domain migration を作成。`0001`–`0008` と producer は変更しない。 |
| `crates/outbox-delivery/src/{lib,model,policy,postgres,runner,observe}.rs`, `tests/*` | `P6-G01`–`G08` が順番に各ファイルの当該責務を実装。Search 型は入れない。 |
| `crates/search-application/src/indexing_service.rs`, `src/ports.rs`, `tests/port_contract.rs` | `P6-S01` が SQLx 非依存の型と port を一度に確定。P3/P7 の port 編集と直列化。 |
| `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql`, `src/{source_lease,event_completion}.rs` | `P6-S02`/`S03` は P7 所有の共有 Source control/receipt 基盤に接続する実装境界。schema file の writer は `P6-I03` だけ。P7 の pointer / pin / GC writer と直列化。P7 draft の `current_bundle_digest` を同じ Source row に置く。 |
| `crates/search-source-document/src/{outbox,delivery}.rs`, `src/lib.rs`, `tests/outbox_delivery.rs` | `P6-S04`/`S05`。既存 memory D4 経路を保持し、fenced port は fake completion で単体検証。 |
| `crates/search-runtime/tests/{outbox_delivery,source_process_recovery}.rs` | `P6-S06`/`S07` の実 P7 durable runtime 縦断。`search-source-document` へ逆向き dev-dependency を加えない。 |
| `crates/search-runtime/src/bin/search_outbox_worker.rs`, `crates/outbox-delivery/sql/least_privilege.sql` | `P6-I04` の composition / role task。P7 durable runtime の完成後に wiring。 |
| 上記二つの normative `spec/` file | `P6-N01` の単独 writer。物理列と read model、at-least-once / unordered、trace 範囲を整合。 |

`P6-I02` と `P6-I03` の migration は別 source / 連番であり、適用順は Domain `0009` → Search `0001`。Search `0001` は Domain の version 1 と衝突しないよう SQLx Migrator の独立 ledger `search_runtime_sqlx_migrations` で実行し、その ledger を実 DB test で検証する。Search runtime crate の `migrate(&PgPool)` はこの `0001` を含む単一正本とし、P7 が続ける番号は `0002` 以降。P3 Graph migration は別 `search_graph` schema / ledger で、P6 は編集しない。P6-S03/S04 の GREEN は P7 durable READY / pointer / guarded GC port が接続された後に成立する。generic P6 の GREEN と P6+P7 の production integration は別 receipt にする。

### Common task protocol

各 Task は次の順で行う: (1) 記載した test 名と assertion を作る、(2) 記載した `cargo test ... -- --exact` を実行し意図した RED を保存、(3) Interface の最小実装、(4) 同じ command の GREEN と対象 strict Clippy / fmt、(5) 差分を独立 read-only reviewer に渡す。testcontainers fixture は既存 `postgres:18.6-bookworm` と `document_repository_postgres::migrate` を使い、実 DB の failure を unit mock の成功で代替しない。Task ごとの branch commit は reviewer GREEN 後に行い、PR は Draft のままにする。

## Review Focus

1. 旧 pending 行で `attempt_limit=NULL` かつ上限到達: 起動を拒否し行 ID/count を監査できることを `P6-G02` で固定する。
2. claim commit 後、handler 開始前の期限切れ: 試行を待機だけで消費せず、handler 未呼出を `P6-G05` で固定する。
3. Source owner の応答不明と standby 競合: standby は先取り claim せず、旧 epoch が公開不能なことを `P6-S02` / `S07` で固定する。
4. GC 済み receipt と同一 digest の現行 Source: historical receipt を pin にせず、current READY/key/digest を照合して新 epoch receipt に収束することを `P6-S03` / `S07` で固定する。
5. Search publish commit と generic ack commit の各応答喪失: 再読で確認できるまで成功を推定せず、再配送で二重可視 publish がないことを `P6-S07` で固定する。

---

### P6-G00 — crate manifests before workspace registration（約 15 分）

**Files:** Create only `crates/outbox-delivery/Cargo.toml`, `src/lib.rs`, `crates/search-runtime/Cargo.toml`, `src/lib.rs`. `outbox-delivery` uses the qualified `serde_json`, `sha2`, `thiserror`, `time`, `tokio`, `sqlx`, `uuid` workspace deps and `testcontainers` dev-dep. `search-runtime` may depend on `outbox-delivery`, `search-application`, `search-source-document`, `sqlx`, `tokio`, `uuid`, `time`; `search-source-document` must not depend back on it. Both libraries start with `#![forbid(unsafe_code)]`.

**Interface:** Manifest package names are `outbox-delivery` and `search-runtime`; library entrypoints exist before root membership changes. G01 and I03 add the actual modules.

- [ ] RED: `test -f crates/outbox-delivery/Cargo.toml && test -f crates/search-runtime/Cargo.toml` fails before creation.
- [ ] GREEN: create manifests and entrypoints, then the same command and `python3 -c 'import pathlib,tomllib; [tomllib.loads(pathlib.Path(p).read_text()) for p in ["crates/outbox-delivery/Cargo.toml","crates/search-runtime/Cargo.toml"]]'` pass. Review dependency direction; compilation follows I01.

### P6-I01 — root workspace registration（shared root-only writer、約 15 分）

**Files:** Modify only root `Cargo.toml`, `Cargo.lock`. Requires the two G00 manifests to exist. Add the two workspace members and `tokio` `signal` feature; keep existing version pins and features otherwise.

**Interface:** G00 の `lib.rs` は空の安全な entrypoint。`model`, `policy`, `postgres`, `runner`, `observe` と Search `migrate`, `source_lease`, `event_completion` は後続 Task がファイル作成と同時に export する将来の契約であり、I01 の Cargo check 時点では未実装 module を宣言しない。No SQLx type crosses into `search-application`.

- [ ] RED: `cargo check --locked -p outbox-delivery -p search-runtime` fails because workspace packages do not exist.
- [ ] GREEN: add member declarations, regenerate lockfile through Cargo; same command and `cargo metadata --locked --format-version 1` pass. Verify no new registry package and no dependency cycle.
- [ ] Review: only this task edits root Cargo/lock. P3/P7 shared changes queue behind it.

### P6-I02 — additive Domain schema and migration preservation（shared writer、約 15 分）

**Files:** Create `crates/document-repository-postgres/migrations/0009_outbox_delivery_v0.sql`, `crates/document-repository-postgres/tests/outbox_delivery_migration.rs`. Existing `src/lib.rs::migrate` remains embedded SQLx migration entrypoint.

**Interface:** Columns `lease_token UUID`, `lease_owner UUID`, `lease_expires_at TIMESTAMPTZ`, `last_attempt_at TIMESTAMPTZ`, `dead_lettered_at TIMESTAMPTZ`, `last_error_code TEXT`, `attempt_limit INTEGER`, `traceparent TEXT`, `tracestate TEXT`, all NULL by default. Four Freeze CHECKs plus `last_error_code IS NULL OR last_error_code IN ('unsupported_event','invalid_envelope','source_unavailable','indexing_failed','handler_timeout','delivery_unknown','delivery_unknown_at_limit')`; partial candidate index `(available_at,occurred_at,event_id) WHERE delivered_at IS NULL AND dead_lettered_at IS NULL`; expiry/reaper candidate index. `outbox_delivery_policy(policy_id SMALLINT PRIMARY KEY CHECK(policy_id=1), revision BIGINT, max_attempts INTEGER, lease_min_ms BIGINT, lease_max_ms BIGINT, backoff_min_ms BIGINT, backoff_max_ms BIGINT)` with positive/bounded CHECKs and seeded `(1,1,8,1000,120000,1000,300000)`.

- [ ] RED: `migration_preserves_legacy_domain_and_audit_rows` snapshots event IDs, JSONB, timestamps, counts, old delivered/pending state and Audit `delivered_at`, applies `0009`, asserts equality and NULL added columns; `migration_rejects_partial_lease_and_double_terminal` asserts CHECK violation; `processing_state_distinguishes_reclaim_and_reap_pending` asserts delivered / dead / in-flight / expired pending / exhausted expired. Run `cargo test -p document-repository-postgres --test outbox_delivery_migration`: RED before migration.
- [ ] GREEN: write additive SQL and read-model projection (`VIEW` or adapter query, choose view `outbox_delivery_state`); repeat all three commands. `EXPLAIN (ANALYZE, BUFFERS)` on synthetic pending/delivered mix records candidate and reaper plans; do not run production DDL. An online `CREATE INDEX CONCURRENTLY` for a large table is a separate rollout step because SQLx migration transaction cannot contain it.

### P6-G01 — typed generic contract, bounds, deterministic retry（約 15 分）

**Files:** Create `crates/outbox-delivery/src/{model,policy}.rs`, modify `src/lib.rs`, test `src/policy.rs` unit module.

**Interfaces:** `DeliveryEnvelope { event_id: Uuid, event_type: String, aggregate_type: String, aggregate_id: Uuid, payload: serde_json::Value, occurred_at: OffsetDateTime }`; `ClaimedEvent { envelope, attempt: i32, attempt_limit: i32, lease_token: Uuid, lease_owner: Uuid, lease_expires_at: OffsetDateTime }`; `FenceResult::Updated|Lost`; `DeliveryDecision::Applied|KnownNoop|Retryable(ErrorCode)|Terminal(ErrorCode)`; allowlisted `ErrorCode` includes `UnsupportedEvent`, `InvalidEnvelope`, `SourceUnavailable`, `IndexingFailed`, `HandlerTimeout`, `DeliveryUnknown`, `DeliveryUnknownAtLimit`. `DeliveryError` distinguishes `PolicyMismatch`, `LegacyExhausted { count, first_ids }`, `InvalidConfig`, `StoreUnknown` without payload/secret messages. `DeliveryPolicy { revision:i64, max_attempts:i32, lease_min_ms:i64, lease_max_ms:i64, backoff_min_ms:i64, backoff_max_ms:i64 }`; `DeliveryConfig { batch_size, max_in_flight, lease_duration, renew_interval, max_processing, drain_timeout, poll_interval, reap_batch }` and `validate() -> Result<(),DeliveryError>`. `DeliveryFuture<'a,T> = Pin<Box<dyn Future<Output=Result<T,DeliveryError>> + Send + 'a>>`; `HandlerFuture<'a,T> = Pin<Box<dyn Future<Output=T> + Send + 'a>>`. `OutboxStore` declares `verify_policy`, `claim(owner,limit,lease)`, `renew(event_id,token,lease)`, `settle_success(event_id,token)`, `settle_failure(event_id,token,code,terminal,backoff)`, `reap_exhausted(limit)` with G02–G04 return types. `retry_delay(event_id: Uuid, attempt: i32) -> Duration`: `min(300 s, 2^(attempt-1) s + SHA-256(event_id || attempt) mod (base/4+1) ms)` with saturating arithmetic and 1 s floor.

- [ ] RED: `rejects_unbounded_or_unsafe_config`, `retry_delay_is_stable_bounded_and_increases`, `error_codes_are_allowlisted` assert batch `1..=32`, in-flight `1..=8`, `renew_interval < lease/3`, positive processing/drain/poll limits, same ID/attempt same delay and no secret-bearing freeform code. Run `cargo test -p outbox-delivery --lib policy::tests`: RED.
- [ ] GREEN: implement types, validation and retry; same commands PASS. Reuse `sha2.workspace`; no new jitter dependency.

### P6-G02 — DB policy guard, old-row audit, bounded claim（約 15 分）

**Files:** Create `crates/outbox-delivery/src/postgres.rs`; test `crates/outbox-delivery/tests/postgres_claim.rs`.

**Interfaces:** `PostgresOutboxStore::new(pool: PgPool, expected: DeliveryPolicy) -> Self` implements G01 `OutboxStore`; `verify_policy(&self) -> DeliveryFuture<'_,()>`; `claim(&self, owner: Uuid, limit: u32, lease: Duration) -> DeliveryFuture<'_,Vec<ClaimedEvent>>`. Every claim transaction locks policy `FOR SHARE`, checks all revision/value fields and old exhausted NULL-limit rows, then uses the Freeze CTE with `limit<=free_permits`, per-row `gen_random_uuid()`, DB clock, and `attempt_limit=COALESCE(old,db_max)`; commit before return. `SELECT FOR UPDATE SKIP LOCKED` is only claim arbitration, never a handler lock.

- [ ] RED: `policy_mismatch_and_legacy_exhausted_refuse_claim` asserts differing `max_attempts`/revision and one old exhausted row return typed errors before any attempt increment; `claim_caps_batch_and_pins_limit_once` asserts 32 cap, distinct tokens, first attempt limit 8, old pending payload unchanged, existing row limit survives policy rollout; `concurrent_claims_are_disjoint` uses 2/4/8 independent pools and checks no simultaneously live duplicate token. Run `cargo test -p outbox-delivery --test postgres_claim`: RED.
- [ ] GREEN: implement the DB transaction and diagnostics with at most first 32 event IDs; the same command PASS. Policy revision changes between check/claim cannot pass the locked transaction. DB errors are `StoreUnknown`, never `Lost`.

### P6-G03 — fenced renew, ack, retry and DLQ（約 15 分）

**Files:** Modify `crates/outbox-delivery/src/postgres.rs`; test `crates/outbox-delivery/tests/postgres_settle.rs`.

**Interfaces:** `renew(event_id: Uuid, lease_token: Uuid, lease: Duration) -> DeliveryFuture<'_,FenceResult>`; `settle_success(event_id, lease_token) -> DeliveryFuture<'_,FenceResult>`; `settle_failure(event_id, lease_token, code: ErrorCode, terminal: bool, backoff: Duration) -> DeliveryFuture<'_,FenceResult>`. All use one `tick` CTE, current token, `lease_expires_at > tick.t`, no delivered/dead row. Failure uses DB `attempt_limit`, clears lease, and either schedules bounded retry or sets `dead_lettered_at`; ack alone sets `delivered_at`.

- [ ] RED: `expired_or_old_token_cannot_renew_ack_or_fail` asserts all 0-row `Lost` at/beyond expiry, no mutation; `retry_uses_db_clock_and_row_limit` asserts `available_at` bounds and terminal transition at attempt 8; `unknown_ack_commit_is_not_success` injects connection loss and requires reread of `delivered_at`. Run `cargo test -p outbox-delivery --test postgres_settle`: RED.
- [ ] GREEN: implement SQL with `Result<FenceResult,DeliveryError>` semantics, validate bound backoff before bind; three commands PASS. No Search code is allowed to call `settle_success` directly.

### P6-G04 — bounded exhausted reaper（約 15 分）

**Files:** Modify `crates/outbox-delivery/src/postgres.rs`; test `crates/outbox-delivery/tests/postgres_reaper.rs`.

**Interface:** `reap_exhausted(limit: u32) -> DeliveryFuture<'_,u64>`; policy check in same transaction; Freeze CTE orders by `last_attempt_at NULLS FIRST,event_id`, limits batch, `FOR UPDATE OF o SKIP LOCKED`, writes `delivery_unknown_at_limit`, retains original row. Called at startup, before every claim cycle, and at drain end.

- [ ] RED: `crashed_final_claim_reaped_once_by_competing_workers` asserts two reapers update one expired max-attempt row exactly once, preserve payload/attempt count, never reclaim; `policy_mismatch_refuses_reap` and `unexpired_final_attempt_is_not_reaped` assert 0 mutation. Run `cargo test -p outbox-delivery --test postgres_reaper`: RED.
- [ ] GREEN: implement SQL; same commands PASS. Expose count to observer/alarm; DB outage remains error and does not pretend successful recovery.

### P6-G05 — permit before claim and dispatch preflight（約 15 分）

**Files:** Create `crates/outbox-delivery/src/runner.rs`; test `crates/outbox-delivery/tests/runner_admission.rs`.

**Interfaces:** `ClaimPermit: Clone+Send+Sync` with `preflight/renew/release -> DeliveryFuture<'_,FenceResult>`; `ClaimAdmission` with associated `Permit: ClaimPermit` and `acquire() -> DeliveryFuture<'_,Option<Permit>>`; `NoopAdmission` for generic routes. `DeliveryHandler<P: ClaimPermit>::deliver(envelope: DeliveryEnvelope, context: DeliveryContext, permit: P) -> HandlerFuture<'_,DeliveryDecision>`. `DeliveryContext` holds attempt, outbox token/deadline, and `Arc<AtomicBool>` cancellation. `DeliveryRunner<S:OutboxStore,H:DeliveryHandler<A::Permit>,A:ClaimAdmission>::run_cycle(&self) -> Result<CycleSummary,DeliveryError>` first acquires free semaphore slots, then admission, then `claim(min(batch_size,free_slots))`; Search configuration enforces exactly one. Immediately before handler it renews **both** outbox and permit; either `Lost` or unknown prevents handler call.

- [ ] RED: `batch_32_inflight_8_never_claims_queued_work` asserts max 8 live claims with slow handler, no claimed-but-undispatched expiry; `pre_dispatch_lost_never_calls_handler` asserts permit/outbox failure leaves row for expiry and no handler invocation; `search_admission_denied_claims_zero` asserts blocked Source permit is not an attempt. Run `cargo test -p outbox-delivery --test runner_admission`: RED.
- [ ] GREEN: implement bounded slots/admission and preflight; commands PASS. A claimed row starts renewal management immediately after claim commit.

### P6-G06 — heartbeat, cancellation and graceful drain（約 15 分）

**Files:** Modify `crates/outbox-delivery/src/runner.rs`; test `crates/outbox-delivery/tests/runner_lifecycle.rs`.

**Interfaces:** `run_until_shutdown(&self, shutdown: watch::Receiver<bool>) -> Result<RunSummary,DeliveryError>`. Heartbeat renews both leases at configured 30 s / 120 s and stops at 15 min total. SIGTERM/CTRL-C receiver stops new admission/claim, continues both renewals while bounded drain runs; deadline sends cancellation, does not ack/fail incomplete work, releases only still-owned Source permit, runs final reaper. DB/handler outage uses bounded reconnect backoff and observable alarm.

- [ ] RED: `renew_lost_cancels_and_never_settles`, `shutdown_drains_completed_and_leaves_unfinished`, `outage_does_not_spin_or_ack_unknown` assert cancel propagation, exactly the completed row acked, incomplete row lease expires for recovery, bounded poll calls. Run `cargo test -p outbox-delivery --test runner_lifecycle`: RED.
- [ ] GREEN: implement lifecycle with Tokio `select!`, intervals, watch and bounded timers; commands PASS. A handler that ignores cancellation still cannot pass final DB fences.

### P6-G07 — generic real-process crash and restart（約 15 分）

**Files:** Test `crates/outbox-delivery/tests/process_recovery.rs` only; production fixes stay in G02–G06 scopes if RED exposes a defect.

**Interface:** Test executable spawns itself via `current_exe()` with an ignored `child_worker_fixture`, separate pool/process ID, then kills the child after observing committed final claim; no production fixture binary or new dependency.

- [ ] RED: `last_claim_kill9_restart_reaps_unknown_once` checks original ID/payload, `attempt_count=8`, two-process reaper exactly one `delivery_unknown_at_limit`, no claim afterward; `four_processes_disjoint_claims_and_recover_expired` checks live token disjointness and after-kill reclaim with higher attempt. Run `cargo test -p outbox-delivery --test process_recovery`: first fail until process path is wired.
- [ ] GREEN: make only minimal generic fixes; the same command PASS on real PostgreSQL 18.6. Preserve one run's process/DB timestamps as qualification receipt.

### P6-G08 — observability, trace and DB role boundary（約 15 分）

**Files:** Create `crates/outbox-delivery/src/observe.rs`, `sql/least_privilege.sql`; test `crates/outbox-delivery/tests/observability_security.rs`.

**Interfaces:** `DeliveryObserver::record(DeliveryMetric)` uses bounded labels `route`, `ErrorCode`, `outcome` only; emit pending/in-flight/DLQ, oldest age, claim/ack/retry/reap/stale counts, handler duration, commit-to-ack lag, DB/consumer error classes. `validate_trace_context(traceparent: Option<&str>, tracestate: Option<&str>) -> Option<ValidatedTrace>` accepts W3C format/length only and never logs payload/actor/document/trace ID as metric label. SQL grants delivery role `SELECT` on Domain outbox and policy, `UPDATE` only delivery columns; no Document mutation or Audit update.

- [ ] RED: `metrics_do_not_contain_high_cardinality_or_payload`, `invalid_trace_is_ignored_without_log_leak`, `delivery_role_cannot_mutate_document_or_audit` assert emitted labels and real role permissions. Run `cargo test -p outbox-delivery --test observability_security`: RED.
- [ ] GREEN: implement observer hooks and grant template; the same command PASS. Historic producer rows with NULL trace start a new span correlated by event ID only.

### P6-S01 — Search event/fence ports and allowlist（shared port writer、約 15 分）

**Files:** Modify `crates/search-application/src/indexing_service.rs`, `src/ports.rs`; test `crates/search-application/tests/port_contract.rs`.

**Interfaces:** `SourceFence { source_id: SourceId, owner_token: Uuid, epoch: i64 }`; `SearchSourceLease::fence(&self) -> SourceFence`; `SearchDeliveryFence { event_id: Uuid, outbox_token: Uuid, source: SourceFence }`; `CurrentGenerationSnapshot { key: Option<ProjectionGenerationKey>, manifest_digest: Option<String>, bundle_digest: Option<String>, pointer_revision: i64 }`; `CompletionMode::PublishCandidate|ReuseCurrent`; `CompleteEventRequest { fence, expected_current: CurrentGenerationSnapshot, candidate: ProjectionGenerationKey, manifest_digest: String, bundle_digest: String, mode }`; `SearchCompletionOutcome::Published(ProjectionGenerationKey)|Unchanged(ProjectionGenerationKey)|Duplicate(ProjectionGenerationKey)|Retry|Lost`; SQLx-free `SearchEventCompletionPort::current_snapshot(source_id) -> BoxFuture<CurrentGenerationSnapshot>` and `complete_event_if_current(request) -> BoxFuture<SearchCompletionOutcome>`; `FencedDocumentIndexingPort::refresh_fenced(event, fence, cancel: Arc<AtomicBool>) -> BoxFuture<IndexingOutcome>`. Add `SearchError::FenceLost` and `SearchError::CompletionUnknown`; `DocumentIndexingService<P: FencedDocumentIndexingPort>::handle_delivery(event,fence,cancel) -> Result<IndexingOutcome,SearchError>` reuses validation. Export `validate_document_event_route(event_type:&str, aggregate_type:&str) -> Result<(),SearchError>`: Document*→Document, Folder*→Folder, `AccessPolicyChanged`→Document/Folder/AccessPolicy. Unknown Document/Folder/AccessPolicy combination is invalid; v0 no no-op.

- [ ] RED: `route_matrix_rejects_unknown_and_wrong_aggregate`, `fenced_port_preserves_search_only_types` checks all current `relevant_event_type` cases and AccessPolicy three types, no SQLx type in application API. Run `cargo test -p search-application --test port_contract`: RED.
- [ ] GREEN: factor the existing private allowlist into the shared validator without changing D4 memory behavior; commands PASS. P3/P7 port writers start only after this interface is reviewed.

### P6-I03 — shared Search Source/receipt migration（shared migration writer、約 15 分）

**Files:** Create `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql`; test `crates/search-runtime/tests/coordination_migration.rs`; expose `search_runtime::migrate(&PgPool)` in `src/lib.rs` with its own Search migration ledger. Domain `0009` is applied first.

**Interface:** Physical `search_source_coordination(source_id UUID PK, fence_epoch BIGINT NOT NULL DEFAULT 0, owner_token UUID NULL, lease_expires_at TIMESTAMPTZ NULL, current_generation_id UUID NULL, current_manifest_digest TEXT NULL, current_bundle_digest TEXT NULL, pointer_revision BIGINT NOT NULL DEFAULT 0, last_published_epoch BIGINT NOT NULL DEFAULT 0, build_fence_seq BIGINT NOT NULL DEFAULT 0)` with complete owner/expiry, complete current key/digest trio and nonnegative/overflow guards. `search_index_receipts(source_id UUID,event_id UUID,generation_id UUID,digest TEXT,bundle_digest TEXT,fence_epoch BIGINT,recorded_at TIMESTAMPTZ)` with `(source_id,event_id)` PK; `digest` is the P6 projection manifest digest, `bundle_digest` is P1 versioned composite digest. These tables are P7 Source control and receipt namespace; no separate Graph pointer. P7 creates registered Source rows before acquisition and later adds generation/pin/guard artifacts in subsequent migrations.

- [ ] RED: `coordination_schema_rejects_half_lease_and_duplicate_receipt`, `migration_keeps_pointer_and_receipt_same_database` and `source_row_is_preseeded_before_lease` against disposable DB; run `cargo test -p search-runtime --test coordination_migration`: RED.
- [ ] GREEN: implement SQL, Search migrator, and one writer ownership; commands PASS. P3's `source_control` term maps to this row, and P3 `build_fence_seq` remains the same counter. Do not silently rename table or create independent pointer.

### P6-S02 — distributed Source lease adapter（P7-owned scope、約 15 分）

**Files:** Create `crates/search-runtime/src/source_lease.rs`; test `crates/search-runtime/tests/source_lease.rs`.

**Interfaces:** `PostgresSourceAdmission::new(pool: PgPool, source_id: SourceId, ttl: Duration)` implements generic `ClaimAdmission<Permit=SourceLease>`; `SourceLease` implements `ClaimPermit` plus `SearchSourceLease::fence() -> SourceFence`. `acquire_source` is one conditional `UPDATE` with DB tick, `owner_token=gen_random_uuid()`, `fence_epoch+1`, overflow fail closed. `renew/release` require source ID+owner token+epoch+unexpired DB lease. Contention returns `Ok(None)` and does **not** call outbox claim.

- [ ] RED: `two_four_eight_processes_have_one_source_owner`, `lost_owner_cannot_renew_release_or_claim`, `source_epoch_overflow_refuses_acquire` use independent `PgPool`s and assert monotonic epoch, no successful stale operation. Run `cargo test -p search-runtime --test source_lease`: RED.
- [ ] GREEN: implement short SQL operations; the same command PASS. A `StoreUnknown` acquisition response causes no outbox claim; do not infer ownership from process memory.

### P6-S03 — atomic Search completion and monotonic receipt（P7-owned scope、約 15 分 per branch）

**Files:** Create `crates/search-runtime/src/event_completion.rs`; tests `crates/search-runtime/tests/event_completion.rs`. P7 runtime's generation readiness / pin / guarded GC adapter is a required dependency; no memory runtime may satisfy this test.

**Interface:** `PostgresSearchEventCompletion::current_snapshot(source_id) -> BoxFuture<CurrentGenerationSnapshot>` and `complete_event_if_current(CompleteEventRequest) -> BoxFuture<SearchCompletionOutcome>`. Precheck external Projection/Unit/coverage/lexical/Graph artifact key/manifest/bundle digest READY and hold P7 staging lease or build guard through commit. In one short SQLx transaction lock Domain outbox row, then `search_source_coordination`, then generation keys/guard/evaluation leases/receipt in shared P3/P7 order; check current outbox token and DB expiry, Source owner/epoch/expiry, current expected key/manifest/bundle digest/revision, `last_published_epoch<=epoch`, candidate READY. Publish pointer if requested. Insert or update `(source_id,event_id)` receipt only when old epoch is lower; same epoch/key/**both digests** is idempotent, same epoch mismatch or higher epoch is `Lost/Retry`, with rollback. `ReuseCurrent` requires current key/both digests READY even if historical receipt exists. Commit response unknown returns error, never success. This port does **not** update generic `delivered_at`.

- [ ] RED: `publish_and_receipt_commit_or_rollback_together`, `stale_epoch_cannot_overwrite_new_pointer_or_receipt`, `same_epoch_conflict_is_not_duplicate`, `gc_receipt_is_not_a_pin_or_duplicate` assert pointer/receipt atomicity, G2 survives G1 return, READY/current check and guarded GC. Run `cargo test -p search-runtime --test event_completion`: RED.
- [ ] GREEN: implement through P7 transaction-bound repository API, not a second connection or independent `put`; the same command PASS. P3 Graph READY check and pending build guard remain part of the same P7 readiness/cleanup contract. SQLSTATE `40001`/`40P01` retries the whole transaction finitely from the same expected revision, never reports CAS loss as success.

### P6-S04 — D4 indexer fenced path（約 15 分 per outcome）

**Files:** Modify `crates/search-source-document/src/outbox.rs`, `src/lib.rs`; test `crates/search-source-document/tests/outbox_indexing.rs` plus `tests/outbox_delivery.rs`.

**Interfaces:** Keep `DocumentOutboxIndexer::new` and legacy `IndexingReceiptStore` for D4 tests. Add `with_fenced_completion(mut self, completion: Arc<dyn SearchEventCompletionPort>) -> Self` and implement `FencedDocumentIndexingPort::refresh_fenced`. `reconcile_once` accepts optional `SearchDeliveryFence` and `Arc<AtomicBool>` cancellation; production branch obtains S03 `current_snapshot` and uses `complete_event_if_current` for **all** `Published`, `Unchanged`, `Duplicate` outcomes, never calling legacy `receipts.put` or `runtime.publish_if_current`. Check cancellation after Source read and each staging tier, then both permit/outbox preflight before completion. Candidate generation records event ID + Source epoch in P7 staging metadata; unpublished failure/CAS loser uses guarded cleanup, not direct delete of current/pinned key. P7 supplies composite bundle receipt; a matching projection-only digest cannot establish no-op.

- [ ] RED: `fenced_published_uses_atomic_completion_only`, `fenced_unchanged_and_duplicate_revalidate_current_ready`, `cancelled_build_never_publishes_or_deletes_current` assert legacy ports uncalled, current/receipt match and cleanup fence. Run `cargo test -p search-source-document --test outbox_delivery`: RED.
- [ ] GREEN: extract shared build logic only where needed; all three commands and focused existing `cargo test -p search-source-document --test outbox_indexing` PASS. D4 memory behavior remains a test path, not a production durable proof.

### P6-S05 — Search bridge route/decision mapping（約 15 分）

**Files:** Create `crates/search-source-document/src/delivery.rs`, modify `src/lib.rs`; test `crates/search-source-document/tests/outbox_delivery.rs`.

**Interfaces:** `DocumentSearchDeliveryHandler<I>::new(indexer: DocumentIndexingService<I>, source_id: SourceId)` implements `DeliveryHandler<P>` where `P: ClaimPermit + SearchSourceLease`; only validated event/aggregate combinations become `DocumentSourceEvent` (no payload forwarding). `Published|Unchanged|Duplicate` after S03 committed completion → `Applied`; `Ignored`/unknown/wrong aggregate → `Terminal(UnsupportedEvent|InvalidEnvelope)`; `SourceUnavailable`, cancel, timeout, commit unknown → `Retryable` or unknown without ack. Generic runner alone settles rows.

- [ ] RED: `bridge_routes_document_folder_policy_without_payload`, `unknown_document_event_goes_terminal_not_ignored`, `search_failure_leaves_row_unacked` assert route matrix, no payload leak, correct decisions. Run `cargo test -p search-source-document --test outbox_delivery`: RED.
- [ ] GREEN: implement bridge and export; commands PASS. `AccessPolicyChanged` with Document/Folder/AccessPolicy aggregate is accepted.

### P6-I04 — composition, shutdown signal and role split（shared integration writer、約 15 分）

**Files:** Create `crates/search-runtime/src/bin/search_outbox_worker.rs`; modify `crates/search-runtime/src/lib.rs` and `crates/outbox-delivery/sql/least_privilege.sql` only for final grant mapping; root Tokio signal feature is already reserved to I01. Test `crates/search-runtime/tests/worker_wiring.rs`.

**Interfaces:** Startup migrates/validates schema and policy, rejects legacy exhausted rows, precreates configured Source row, builds separate delivery and Source-read/Search-completion pools, registers one static Document Search route, sets batch/in-flight to 1 for Search, then `run_until_shutdown` on SIGTERM/CTRL-C. P7 durable runtime only; process-local `MemoryDocumentIndexRuntime` is forbidden. Search completion role needs `SELECT` on outbox and PostgreSQL `FOR UPDATE` needs `UPDATE` on at least one outbox column; grant only `UPDATE(lease_token)` for row lock, **not** `UPDATE(delivered_at)` or Audit/Document mutation. The adapter never writes any delivery column; generic role alone performs state updates. [PostgreSQL SELECT privileges](https://www.postgresql.org/docs/current/sql-select.html).

- [ ] RED: `worker_refuses_mismatched_policy_or_memory_runtime`, `search_role_cannot_ack_or_touch_audit`, `sigterm_stops_claim_and_drains` assert startup fail closed, role restriction, bounded drain. Run `cargo test -p search-runtime --test worker_wiring`: RED.
- [ ] GREEN: wire existing SQLx/Tokio runtime and static route, no new unqualified deps; commands PASS. `cargo check --locked -p search-runtime --bin search_outbox_worker` passes.

### P6-S06 — producer-to-ack real PostgreSQL slice（約 15 分）

**Files:** Test `crates/search-runtime/tests/outbox_delivery.rs`; only focused S03–S05 fixes if needed.

- [ ] RED: `domain_event_to_current_ready_receipt_then_generic_ack`, `t10_and_access_revocation_reread_current_source`, `manual_rebuild_does_not_ack_pending_event` use real producer transactions, Domain + Search migrations and P7 durable runtime; assert receipt/current/delivered evidence separately, Audit `delivered_at` and Document rows untouched. Run `cargo test -p search-runtime --test outbox_delivery`: RED before full connection.
- [ ] GREEN: connect S03–S05 to the real runtime; the same command PASS. This is integrated P6/P7 evidence, not generic-only P6 evidence.

### P6-S07 — distributed process / fault / restart qualification（約 15 分 per fault schedule）

**Files:** Create `crates/search-runtime/tests/source_process_recovery.rs`; production fixes only at the failed owning task.

- [ ] RED: `source_burst_two_four_eight_processes_no_prefetch_or_dlq` asserts one live Source owner and claim=1; `g1_stale_after_g2_cannot_publish_receipt_or_ack` asserts G2 current/receipt survives old process; `publish_commit_unknown_and_ack_commit_unknown_reconcile` checks reread/redispatch and no second visible publication; `expiry_boundary_and_gc_keep_current_or_pinned` checks expired fence Lost and cleanup protection. Spawn independent OS processes/pools against one disposable DB, inject `kill -9`, expiry, connection loss, restart and barrier-controlled resume. Run `cargo test -p search-runtime --test source_process_recovery`: RED.
- [ ] GREEN: the same command PASS with DB row/manifest/digest checks. Historical receipt pointing to GC artifact is metadata only; duplicate requires current READY key/digest. Keep process logs free of payload, document title, principal or secret values.

### P6-N01 — normative read model and delivery semantics（単独 spec writer、約 15 分）

**Files:** Modify only `spec/data/transaction-consistency-requirements-v0.md` and `spec/operations/observability-audit-requirements-v0.md` at their outbox sections.

- [ ] RED: compare `aggregate_version` / `processing_state`, ordering, DLQ and trace text with Freeze; record exact conflicting paragraphs in task receipt. Run `rg -n 'aggregate_version|processing_state|outbox|trace' spec/data/transaction-consistency-requirements-v0.md spec/operations/observability-audit-requirements-v0.md`.
- [ ] GREEN: specify derived state `DELIVERED|DEAD_LETTER|IN_FLIGHT|PENDING` plus exhausted recovery-pending visibility, single Search destination, at-least-once/unordered, producer-specific future versioning and validated trace only. Do not claim a universal `aggregate_version` backfill. `mise run verify:fast` and focused spec/architecture lint PASS.

## Final qualification and execution order

1. `N01` may run immediately from the frozen semantics, before `I02`. `G00 → I01` reserves the two packages, with I01 as root Cargo/lock's sole writer. `I02` is independent and can start after N01. Generic implementation follows `G01 → G02 → G03 → G04 → G05 → G06 → G07 → G08`. `I03` may start after I01/I02 but has exclusive Search migration ownership. `S01` is a separate exclusive application-port writer. `S02` requires I03. `S03` requires S01/S02 and the P7 durable READY/pointer/guarded GC implementation. `S04 → S05 → I04 → S06 → S07`. No simultaneous writer touches root Cargo/lock, `search-application` ports, either migration directory, or P7 Source control.
2. Generic qualification: focused test names above, `cargo fmt --all -- --check`, `cargo clippy --locked -p outbox-delivery --all-targets -- -D warnings`; one representative 1k/10k event, 1/4/8 workers, batch 1/16/32 and handler 0/10/100 ms matrix with three steady-state runs, recording code SHA, PostgreSQL/SQLx versions, CPU/RSS, seed, events/s, p50/p95/p99 claim/ack, DB CPU/IO/locks, renew misses and backlog drain. Retain crash/reaper result and `EXPLAIN (ANALYZE, BUFFERS)`; no SLO claim from candidate values.
3. Integrated qualification after P7: focused S03–S07, `cargo clippy --locked -p search-runtime -p search-source-document -p search-application --all-targets -- -D warnings`, one Document corpus 1k/10k with burst/restart/T10/revocation, source rebuild cost and commit-to-visible lag. Verify P3 Graph READY, shared Source lock namespace, current/pin/build-guard GC and receipt/outbox separately. Run `mise run verify:fast` once after the material assembly; hosted exact-head CI / independent read-only review provide the final gate. Record generic, Search bridge and P7 composition receipts separately.
4. Parent task graph must replace the old single `p6-implement.write_scope=crates/outbox-delivery` with the IDs/scopes above. `p6-receipt` may report generic GREEN before P7, but integrated Search delivery remains pending until S03–S07 and P7 qualification. The current graph's `p7-design` dependency on final `p6-receipt` must consume generic P6 evidence rather than treating P6+P7 composition as already done. All PRs remain Draft; no merge/deploy/production migration action follows from this plan.
