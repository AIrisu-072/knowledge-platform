# Audit Infrastructure v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement task-by-task. Steps use checkboxes; independent architecture/security and whole-branch reviews are mandatory.

**Goal:** Deliver qualified versioned Audit evidence from existing Document transactions through a durable dispatcher to a separate Audit Authority.

**Architecture:** Preserve existing business/Audit staging atomicity and add Audit-owned delivery state and a separate sink pool. A pure contract crate validates CloudEvents and compatibility; a PostgreSQL adapter owns source/store operations and a bounded runner owns delivery. Investigation, integrity and maintenance never reuse Document business ACL or generic Search outbox state.

**Tech Stack:** Rust1.98.1, selected workspace serde/serde_json/sha2/time/uuid/Tokio/SQLx0.9, PostgreSQL18.6, existing JSON Schema validation tooling. No new SDK/service promotion without qualification.

**Spec:** `docs/superpowers/specs/2026-10-02-audit-infrastructure-v1-design.md`

## Global constraints

- Exact base `d71753d46590bb4406a1c0b74894ab90a27a6c88`; independent Draft stack; no merge/deploy/production migrations/identity/credentials.
- Read `AGENTS.md` and A0 reconstruction; leave Document acceptance/Search WIP/Organization specs and global active pointer untouched.
- Audit is never sampled; mandatory source stage failure rolls back the business transaction; sink failure does not.
- Preserve existing event identity/type/time/source/result and typed actor/resource meaning. Unknown historical evidence is absent/quarantined, never fabricated.
- Closed per-type metadata; depth≤4, fields≤64, strings≤512 UTF-8 bytes, envelope≤32KiB. Never truncate forbidden free text into an apparently valid event.
- Batch1–32, in-flight1–8, lease1–120s, attempts1–32, retry delay1–300s, poll1–60s; initial runner uses one in-flight event.
- Investigation page≤100; cursor binds a commit-closed publication watermark backed by a transactional counter held through commit; allocated sequences/UUID maxima do not prove completeness or causal order.
- Retention defaults to no automatic deletion; current policy revision is revalidated under the expiration lock, legal hold unsupported means retain. Source cleanup is deferred; permanent sink identity/receipt/tombstone registry never expires in v1.
- Separate Audit source migration ledger avoids Document0009 and Search generic outbox ownership. Bootstrap completion needs a table write barrier and zero receipt anti-join, not max(event_id).
- Source SQL admission is procedural: compression/physical-size gate (uncompressed storage≤24KiB/scalar text≤512 bytes), then internal JSONB depth≤4/nodes≤64/key≤128 bytes/string≤512 bytes and native numeric integer/signed64 gate, then intermediate≤256KiB and exact wire≤24KiB before return/hash. JSONB binary protocol is not an expansion bound.
- Sink lock order: publication counter → policy/control → identity registry sorted by event UUID, through commit; no remote operation under those locks.
- Use only synthetic data. Coordinate shared Cargo/DB resources; report NOT RUN/FAIL accurately.

## Review focus

1. Legacy reason-bearing variants are deferred/quarantined without sink success or cleanup eligibility; operation references/digests cannot replace mandatory text (Task1/2/9).
2. Unknown commit after sink insert or source ack must not fabricate success or a new event ID (Task4/5).
3. Ordinary reader direct SELECT must not bypass audit-of-read, and dispatcher cannot read/modify business facts (Task6).
4. Restore older sink must reconcile latest policy/tombstones/receipts/trusted checkpoint before investigation or maintenance; expired replay cannot resurrect and missing source is recovery-blocked (Task7/8).
5. Delayed lower transactions cannot fall behind a cursor; oversized/compressed legacy fields must quarantine before materialization while other events proceed (Task2/3/5/7).

## File and ownership map

- `crates/audit-core/src/{lib,event,catalog,legacy,canonical,policy,ports}.rs`: pure event model, closed catalog, compatibility adapter, digests, policies and ports; no SQLx/HTTP/fs.
- `spec/telemetry/{audit-event.schema.json,audit-event-catalog.json}`: versioned normative schema/catalog, unique taxonomy and generation source.
- `crates/audit-postgres/src/{lib,source,store,investigation,integrity,retention}.rs`: SQLx adapters with separate source/store pools and bounded operations.
- `crates/audit-postgres/{migrations-source,migrations-store,sql}`: separate migration ledgers, immutable evidence/state, restricted functions and grant templates; no old migration edit.
- `crates/audit-delivery/src/{lib,dispatcher,health,main}.rs`: bounded sequential runtime and CLI composition; no Document business mutations.
- `crates/audit-acceptance/tests/`: real Document producer/separate-store integration; reuse existing fixture construction without acceptance-stack imports.
- `tools/audit-contract/`: schema/catalog consistency and synthetic positive/negative fixtures through existing validator.
- `docs/superpowers/execution/audit-infrastructure-v1-*`: source/qualification/review/status receipts.
- `spec/operations/audit-infrastructure-v1.md`: concise normative contract linked to existing audit requirements only after freeze.
- Workspace manifests/lock, dependency boundaries, mise tasks and CI receive additive audit checks only.

## Task1: Freeze, compatibility decisions and backend qualification plan (D-AUD1)

**Files:** design/A0/this plan, scope-authorization, architecture-security-review and status records.

**Produces:** reviewed immutable design/plan blob IDs; legacy compatibility table with supported/deferred disposition; bounded PostgreSQL qualification criteria.

- [x] Reconstruct source and exact GitHub qualification; report A0 before code.
- [ ] Review design/plan with an independent architecture/security reviewer; resolve every P1/P2, or isolate a substantive STOP with evidence.
- [ ] Confirm reason-bearing deferral preserves original evidence indefinitely and does not claim reference-only compatibility; review generic outbox convergence; no implementation until re-review GO.
- [ ] Record freeze hashes and current user scope authorization separately from any claim of user review of newly written artifacts.
- [ ] Verify docs links, no placeholders, no product changes, `git diff --check`; commit D-AUD1 packet.

## Task2: Schema, catalog and safe legacy adapter (A-AUD1)

**Files:** audit-core event/catalog/legacy/canonical, schemas/catalog, contract fixtures, manifests/architecture rules.

**Interfaces:** `AuditEnvelope::from_legacy(LegacyAuditRow) -> Result<AuditEnvelope, ValidationError>`; `AuditEnvelope::validate() -> Result<(), ValidationError>`; `canonical_bytes(&AuditEnvelope) -> Result<Vec<u8>, ValidationError>`; `event_digest(&AuditEnvelope) -> [u8;32]`. `LegacyAuditRow` is transport-free and is constructed only after Task3 SQL size/compression admission, with bounded scalar fields. A separate `LegacyAdmission` returns bounded `Quarantined { event_id, code }` without materializing data. Reason-bearing variants return `LegacyReasonContractUnqualified`; no reference-only envelope. Errors expose fixed codes, no rejected contents.

- [ ] Write failing tests for every existing event type with explicit qualified/deferred variant disposition, UUID uniqueness/catalog duplicate detection, CloudEvents1.0 JSON round-trip, UTC time, typed actor/resource, absent-vs-known correlations, manual-vs-scheduled serviceExecutor, camel/snake keys, legacy time arrays and version evolution.
- [ ] Add negative fixtures for forbidden/unknown/nested fields, credentials/storage/full ACL/document/query/free reason, zero/over-bound IDs, wrong result/resource, unknown event/version, exact32KiB and one-over. Required free-text reason must return quarantine and never be dropped to produce a valid envelope.
- [ ] Run contract/core tests and save genuine RED before product implementation.
- [ ] Implement model/catalog generation, explicit adapter disposition and bounded canonicalization. No CloudEvents SDK promotion; standards-compliant serializer remains at adapter boundary.
- [ ] Run Rust core and JSON Schema conformance both ways; runtime/schema must agree on every fixture. Run fmt/clippy/architecture and dependency policy; commit code and qualification separately from test-first evidence.

## Task3: Atomic source receipts and append-oriented sink (A-AUD2)

**Files:** audit-postgres source/store migrations and adapters, SQL role templates, real-PG tests.

**Interfaces:** `AuditSource::migrate(&PgPool)`, `AuditStore::migrate(&PgPool)` with separate ledgers; `AuditSource::admit(event_id) -> LegacyAdmission`; `AuditSource::bootstrap_legacy(limit:u32) -> BootstrapReport` and `finish_bootstrap() -> BootstrapCompleteness` with a zero anti-join under the documented source barrier; `AuditStore::ingest(&AuditEnvelope, SourceReceipt) -> Result<IngestReceipt, StoreError>`. `IngestReceipt` binds event ID, envelope digest, source receipt, commit-closed publication sequence, stored_at and Live/Expired disposition from the permanent identity registry; errors distinguish unavailable/unknown/schema/conflict.

- [ ] Write failing real-PG tests: legacy rows survive migration, audit INSERT captures receipt/opaque marker in same transaction, receipt failure rolls business/stage back, source payload cannot be changed by producer/dispatcher roles, separate pools/databases work.
- [ ] RED: delayed lower source/Authority publication holds higher watermark back through COMMIT; rollback publishes no gap. Concurrent pre-trigger insert drains before installation; older-UUID rows and concurrent bootstrap batches cannot disappear behind a max-ID cursor; final anti-join/barrier proves completeness.
- [ ] RED: a highly compressible64MiB synthetic reason, a large uncompressed JSONB and oversized actor/source strings are quarantined using size/compression metadata before payload extraction/serialization/hash/client fetch; bounded unrelated rows continue. Also store compact `1e100000`, valid `1e-16000`, and nested numeric arrays; a native numeric scale/range gate must reject them before rendering or jsonb_send. Probe rejects any attempted raw client fetch/hash/render before the procedural stage allows it. Verify physical→internal-structure/numeric→bounded-render→wire-size ordering, explicit256KiB intermediate proof and bounded client memory; do not infer bounds from compressed size or binary protocol alone.
- [ ] Write duplicate races: identical event returns original receipt/one row; same ID different digest fails without mutation; expired identical ID returns Expired without a new live row; source/source-receipt conflict fails; unknown legacy delivered flag remains unresolved.
- [ ] Run RED; implement additive migration/trigger/receipt, guarded size admission, permanent identity registry and transactional publication counters for source/store. Keep domain `outbox_events` untouched.
- [ ] Add counter→policy/control→ordered-registry lock-order concurrency tests for ingest/expiration/policy/control events; no reverse-order deadlock or remote call under lock. Run tests plus existing Document mandatory-audit rollback regressions on the adapted schema. Verify migration failure/retry/checksum/legacy bootstrap behavior and commit.

## Task4: Durable lease/retry/quarantine (A-AUD2)

**Files:** source state/operations, audit-core policy/ports, real-PG concurrency tests.

**Interfaces:** `claim(owner,limit,lease) -> Vec<Claim>`; `renew(event_id,token,lease) -> Fence`; `ack(event_id,token,&IngestReceipt) -> Fence`; `fail(event_id,token,ErrorCode,Disposition) -> Fence`; `reap(limit) -> Count`; fixed `Fence::{Updated,Lost}`, unknown DB errors are never Lost. `replay_quarantined` requires authorized maintenance action and never replaces event identity.

- [ ] RED: two claimers no simultaneous ownership, expired token cannot ack/renew, DB clock bounds, persisted policy/attempt limits, bounded deterministic backoff, limit→quarantine, invalid schema retained as terminal evidence.
- [ ] RED: lost COMMIT response for claim/ack/failure remains Unknown; delivered status requires an exact committed sink live/expired receipt, with expired disposition separately counted; no raw errors/payload in quarantine.
- [ ] Implement short locked claim/settle transactions and side-state fences. Run real-PG tests and strict lint; commit.

## Task5: Dispatcher/runtime and pipeline health (A-AUD2)

**Files:** audit-delivery dispatcher/health/main, failure-injection/restart tests, CLI config docs.

**Interfaces:** `Dispatcher::tick() -> DispatchReport`; `Dispatcher::run_until_shutdown(cancel)`; `PipelineHealth` distinguishes produced/pending/stored/acked/verified/quarantined. Uses Task4 source port and Task3 sink port.

- [ ] RED: store down leaves source pending and business committed; crash after store before ack replays one event; duplicate and expired delivery; oversized legacy row skipped without client payload materialization; invalid admitted source digest; timeout/unknown receipt; shutdown stops claims, releases only confirmed ownership; no busy-spin/unbounded queue.
- [ ] RED: health counters never label attempted/delivered as verified; fixed metric codes contain no IDs/payload.
- [ ] Implement sequential bounded runner and CLI without production credentials or network service creation. Test restart on persisted real databases, no Document business access, lock-free sink calls, and CPU/backlog bounds; commit.

## Task6: Independently authorized, auditable investigation/export (A-AUD2)

**Files:** investigation adapter, restricted SQL functions/grants, audit control-event catalog, tests.

**Interfaces:** `investigate(VerifiedAuditCaller, AuditQuery, Cursor, limit) -> Page`; `export(VerifiedAuditCaller, AuditQuery, limit) -> ExportChunk`. Verified callers carry issuer/principal and explicit audit capability/scope from a trusted boundary; no Organization role vocabulary. Cursor/ExportChunk bind the trusted commit-closed watermark, canonical JSONL/count and digest. Restore/recovery epoch must be READY before these methods can disclose data.

- [ ] RED: deny reader/exporter mismatch, outside source/class scope, direct table SELECT bypass, writer reading evidence, business role update/delete, oversized query/page, unauthorized access even if denial-audit insert fails.
- [ ] RED: one control event committed before disclosure/export; failure to stage that event returns no data; controlled delayed-lower transaction cannot be skipped by advancing a page/checkpoint; cursor page order stable with concurrent append; self-audit is finite and does not recurse.
- [ ] Implement least-privilege DB boundary plus bounded application API, no GUI. Validate query data minimization and export receipt; commit.

## Task7: Checkpoints and source/sink reconciliation (A-AUD2)

**Files:** integrity adapter/core digest types, synthetic mutation/deletion/order tests.

**Interfaces:** `checkpoint(VerifiedAuditCaller, ClosedWatermark) -> Checkpoint`; `verify(VerifiedAuditCaller, &TrustedCheckpoint) -> IntegrityReport`; `reconcile(BootstrapCompleteness, source_watermark, sink_watermark, limit) -> ReconciliationReport` with fixed findings. Reject unclosed/client-invented watermarks or incomplete source bootstrap.

- [ ] RED: event payload modified, digest modified, event deleted, reordered checkpoint, duplicate conflict, source delivered but sink missing, stored-unacked, absent correlation versus corrupted correlation, invalid schema, finite closed watermark during verification audit writes; a delayed lower source/store append cannot appear later behind a completed prefix; every source receipt has a live/expired/pending/quarantined disposition.
- [ ] Implement per-row hash plus ordered manifests, explicit anchor provenance and threat limitations; do not claim same-DB hash defeats privileged rewrite. Reconcile both tables using commit-closed membership plus the source anti-join completeness receipt, without generating IDs or resurrecting expired payloads.
- [ ] Test independent expected-checkpoint copy and bounded scans; commit.

## Task8: Retention and backup/restore recovery (A-AUD2)

**Files:** retention adapter/restricted functions, backup/restore runbook, disposable recovery tests.

**Interfaces:** `RetentionPolicy` with revision/source/class/duration/no-auto-expire; `eligible(cutoff,limit) -> RetentionPlan { policy_id, policy_revision, authority_epoch, closed_watermark, event_ids }`; `expire(VerifiedAuditAdmin,&RetentionPlan) -> MaintenanceReceipt`, rechecking the current locked policy and identity state in its deletion transaction. No source-cleanup API or source DELETE grant exists in v1; source evidence and permanent sink receipt/tombstone registry are retained. Backup/restore uses existing pg_dump/pg_restore tooling, not a custom backup format.

- [ ] RED: no default expiration, exact boundary, unaudited/unverified/undelivered/reason-deferred evidence cannot disappear, absent admin denied, unsupported legal hold retains, config action audited. Plan then retain-all/extended policy/deleted policy must delete nothing; controlled concurrent policy update and expiration serialize under the same lock.
- [ ] RED: atomic live-row→permanent-expired-registry/tombstone transition plus audit; dispatcher crash then duplicate delivery and restored-source replay returns Expired without payload recreation; conflicting replay remains a conflict. Every source deletion path is denied in v1.
- [ ] RED: restore older sink with newer source detects/replays qualified missing IDs only after latest policy/tombstone/receipt/checkpoint continuity is established; an older pre-expiration backup plus source replay must not resurrect expired evidence. Missing trusted latest manifest leaves investigation/export/maintenance recovery-blocked. Inject missing source as though unauthorized cleanup occurred, restore sink before that event, and assert explicit unrecoverable/recovery-blocked rather than READY. Restore original checkpoint verifies; tampered backup fails; tombstone distinct from unexplained deletion.
- [ ] Implement revision-fenced sink policy/expiration, durable registry/tombstones and fail-closed restore epochs; defer source cleanup; perform real disposable backup/restore and restart. Record operational assumptions; commit.

## Task9: Concrete Document producer acceptance (A-AUD3)

**Files:** audit-acceptance tests and only narrowly needed additive Document adapter/correlation files approved by freeze.

**Interfaces:** Existing Document Application/Repository services plus Tasks2–8. No alternate business implementation, PR43 fixtures or PoC identity shortcut.

- [ ] RED/qualification fixtures: actual qualified create/Publish/first read/file/Diff/denial/scheduler variants → transactional source → dispatcher → separate store. Actual reason-bearing withdrawal/publication-end/metadata/move/Folder/ACL variants → atomic preserved-source quarantine; prove no sink success/delivered flag/cleanup eligibility and label these producer slices DEFERRED.
- [ ] Exercise mandatory outbox/receipt insert failures and inspect unchanged business state; store outage proves committed state plus pending Audit. Replays/no-op/read duplicates retain existing counts.
- [ ] Preserve old event meaning/taxonomy, original timestamp/actors and safe correlation. Keep every legacy-reason producer DEFERRED until a separate preservation contract or narrow user-approved amendment qualifies it; no reason_reference substitution. Do not claim complete A4 end-to-end coverage while these variants remain deferred.
- [ ] Test forbidden content across event/source diagnostics/export, real PostgreSQL concurrency and recovery. Run Document regression suites; commit.

## Task10: Independent acceptance, qualification record and future handoff (A-AUD3)

**Files:** normative audit-v1 contract, capability status/acceptance/review, organization-handoff, selection qualification.

- [ ] Independently review full implementation and security/transaction/privacy/recovery evidence; resolve P1/P2 with fresh RED→GREEN regression.
- [ ] Run repository `mise run verify:fast`, `mise run verify`, and required extended controls; distinguish blocked infrastructure from failures. No passing claim from focused-only tests.
- [ ] Qualify bounded PostgreSQL backend only after evidence; keep CloudEvents SDK deferred if unused. Verify no WIP dependency, old migration edits, production data/credentials or Organization concepts.
- [ ] Parent publishes bounded Draft stack and verifies exact remote tree/head and hosted CI/Sandbox/triggered PoC. Do not claim whole-track completion before those receipts.
- [ ] Write A6 contract for future approved Org producers and Search privacy/NO_RETENTION boundaries; no Product/Domain meaning, rollout or main merge.

## Execution handoff

The current user's explicit instruction authorizes A1–A6 autonomous progression within the listed STOP conditions and Draft-only boundary. It is not a fabricated claim of artifact-specific user review. Native execution follows independent design/security review, and parent supplies publication and a fresh reviewer. Each task saves actual RED/GREEN/NOT RUN receipts; defer only affected parts when a substantive STOP arises.
