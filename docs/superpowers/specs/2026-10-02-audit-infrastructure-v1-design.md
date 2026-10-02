# Audit Infrastructure v1 — Design

Status: WRITTEN DESIGN / INDEPENDENT REVIEW REQUIRED. Scope authorized 2026-10-02; freeze occurs only after P1/P2 resolution. This is an independent cross-cutting track based on the A0 reconstruction, not Organization Client design.

## 1. Purpose and invariants

Complete the existing Document transactional Audit boundary with a reusable, durable evidence pipeline. Preserve event meanings and mandatory staging atomicity. Business state must not commit alone when required Audit creation fails. A committed source event remains a fact when its destination is unavailable; delivery is outside the business transaction, at-least-once, with idempotent ingest.

Audit is never sampled. It is neither DomainBusinessEvent, telemetry trace/metric/log, ManagementAnalytics/KPI authority, nor Personal Chat/Memory/preferences/private drafts. No new Organization business concepts, Personal AI implementation, GUI, external infrastructure, paid service, production identity or production deployment is included.

## 2. Dependencies and isolation

Base: `d71753d46590bb4406a1c0b74894ab90a27a6c88`. Existing Document event generation, PostgreSQL18/SQLx0.9, UUIDv7, serde/serde_json, sha2, time and Tokio are sufficient candidates. Qualification uses Linux, synthetic events and disposable databases. No POC REQUIRED dependency is promoted by this design.

Logical authority and credentials are separate:

1. Document database: immutable event staging plus Audit-specific delivery state.
2. Dispatcher: reads a committed staging snapshot; owns Audit lease/retry/ack state only.
3. Audit Authority: independent pool/database supported from day one; immutable accepted evidence, ingestion sequence, receipts and investigation/maintenance operations.
4. Operational observer: bounded low-cardinality health/metrics, never an Audit substitute.

Audit migrations have their own schema/ledger. Do not consume the next Document migration number used by another Draft stack. No generic `outbox_events` schema or worker changes. Do not depend on `document-server`, GUI/MCP or unqualified scheduler wiring.

## 3. Backend and integrity decision

The following are qualification candidates, not a production rollout decision:

| Candidate | Benefit | Cost / result |
|---|---|---|
| Separate PostgreSQL Audit schema/database, existing SQLx | Existing selected stack, concurrent writers, unique event IDs, privileges, transactional access-audit, native backup tools; physical separation via pool | Preferred bounded backend; must pass privilege, recovery, backup/restore and fault tests before selection changes |
| Local append files with manifest | Simple append bytes, no new service | Concurrent append, query authorization, rotation, atomic receipts and recovery require a new storage engine; not preferred |
| New dedicated/WORM/managed Audit product | Stronger independent protection possible | New operational service/terms/credentials, violates current independent scope; defer |

Integrity alternatives:

- Row digest alone detects accidental modification when the expected digest is trusted; it does not defeat a privileged attacker rewriting row and digest or deleting both.
- Global hash chain adds a serial write head and complicated concurrent/recovery/retention semantics; without independent anchors an administrator can still rewrite the chain. Not selected by default.
- Per-event canonical SHA-256 plus ordered checkpoint manifests and source-to-sink reconciliation is the v1 candidate. It avoids chain-dependent digests, supports idempotent comparison and bounded verification; publication itself is serialized through commit as specified in §7; an independently retained checkpoint is necessary to detect malicious complete rewrite/deletion.
- Signed checkpoints/WORM or independently operated anchoring are future deployment options. v1 does not claim cryptographic protection from a database owner with both source and sink/backup/checkpoint access.

Checkpoint verification binds ordered `(ingest_sequence,event_id,event_digest)` entries, a commit-closed publication watermark/count (§7), schema version and retention tombstones to a manifest digest. Compare against a separately retained trusted checkpoint, not merely a digest read back from the same mutable database. No global hash-chain requirement crosses the business transaction.

## 4. Event contract and compatibility

`audit-core` owns a storage/transport-independent model and validation. The JSON transport follows CloudEvents1.0.2 structured JSON: `specversion="1.0"`, stable `id`, `source`, existing `type`, `subject`, UTC RFC3339 `time`, `datacontenttype="application/json"`, and a versioned `dataschema`. Keep existing `document.*`, `folder.*`, `access_policy.changed`, `authorization.denied` values. Do not substitute the illustrative namespaced type in the old requirements for the implemented taxonomy.

Payload version1 carries actor issuer/principal and known invocation type when available; optional distinct service executor; action; typed resource and optional version; result/reason code; optional operation/request/trace correlation; closed event-specific metadata; and explicit adapter provenance. Missing historical correlation/actor kind stays absent or `unknown`; do not infer Human/Agent from a principal name or fabricate IDs as old request correlations.

For a qualified legacy variant, the adapter preserves event ID/type/time/source/subject/result/actor/resource and every required evidence field. It records `adapter_version=1` and source format `document-audit-outbox-v0`. A reference or digest alone is not proof that required legacy reason text remains retrievable. `withdrawal.rs` stages no operation ID; Management operation ledgers preserve command digest/result, not the supplied reason text. Therefore no reference-only substitution or `caller_reason_recorded` replacement is qualified.

Compatibility disposition is explicit:

| Legacy variant | v1 disposition | Preservation requirement |
|---|---|---|
| Known ID/code-only create/Version/Publish/schedule/read/file/Diff/denial | Eligible for schema qualification | Preserve all approved fields and any available service executor/correlation |
| Stable terminal/restoration reason codes without free-text reason | Eligible after closed code validation | Preserve code and meaning; no guessed reason |
| Any caller free-text `reason` variant, including withdrawal, publication-end and regular Management/Folder/ACL changes | DEFERRED / QUARANTINED (`legacy_reason_contract_unqualified`) | Preserve original staging evidence unchanged and indefinitely; no sink success, delivered flag, verified/qualified count or cleanup eligibility |
| Unknown, oversized or compressed-unbounded legacy shape | QUARANTINED with fixed diagnostic code | Keep original evidence; no unbounded fetch or payload diagnostic |

Only a separately reviewed retrieval/preservation contract, or an explicit narrow user-approved semantic amendment, can remove the reason-bearing deferral. Even a publication-end row with a real reason-bearing ledger is deferred until that retrieval/access/retention contract is qualified. An unrelated operation reference may still be retained as correlation, never as full evidence preservation. Core schema/store/delivery proceeds independently; A4/A5 report these producer cases as preserved-source/quarantined, not end-to-end delivery PASS.

No raw document body, excerpt, full query, ACL values, storage locator, credentials, tokens, customer financial data, unbounded prompts/chat, chain-of-thought, personal memory or uncommitted draft body is admitted. Metadata is a closed per-type allowlist, with scalar/array/type/value checks, depth≤4, at most64 fields, strings≤512 UTF-8 bytes and envelope≤32KiB. IDs have narrower grammar/length bounds. Free text is not made safe merely by truncation. Unknown keys fail closed. Never record a rejected payload in errors/DLQ/metrics.

The normative schema is `spec/telemetry/audit-event.schema.json`; a checked-in event catalog declares unique event types, source resource kinds, allowed metadata and action/result rules. JSON Schema and runtime validator use the same catalog and conformance fixtures. Breaking payload meaning increments schema major version; additions require explicit catalog compatibility. Old staged rows can be adapted without business-table migration. Unknown future versions quarantine visibly.

## 5. Source staging and integrity receipt

Existing business transactions continue inserting their existing `audit_outbox_events`. An additive Audit source adapter/migration creates `audit_delivery` state and a receipt capture trigger in that same transaction. For bounded admitted rows, capture a deterministic immutable source digest; delivery state is separate from event facts. For unbounded/unqualified rows, atomically capture only an event-ID/size-class/quarantine marker, with digest explicitly unavailable. Failure to capture mandatory receipt/marker aborts the original insert and business transaction. A quarantine marker is not complete integrity verification.

Do not change old migrations/checksums. Install the capture trigger under an Audit-staging table write barrier that waits for preceding insert transactions; all later inserts include capture. Bootstrap processes legacy rows in bounded batches using `NOT EXISTS` receipt anti-joins, not a maximum UUID/sequence as proof of completeness. A final short staging-table SHARE barrier, acquired before the source publication-counter lock, verifies zero missing receipts and commits `bootstrap_complete` with its source watermark. If the final anti-join is nonempty or the barrier times out, bootstrap remains incomplete and retries; never report a complete source reconciliation prefix. No long-running writer is silently excluded.

Source receipt publication uses a transactional counter row locked through source transaction commit, so its visible watermark is commit-closed. Multiple receipts in one business transaction publish atomically. Capture/bootstrap use consistent table→counter lock order. A delayed lower receipt transaction prevents a higher receipt watermark from becoming visible; rollback publishes neither its receipt nor its counter increment. Qualification must exercise contention/deadlock/retry and bounded lock timeout behavior. This is an explicit serialization cost, not a throughput claim.

Before dispatcher/bootstrap payload fetch or source-receipt hashing, a restricted procedural SQL admission function executes these stages in order. Do not express the stages as reorderable WHERE/AND predicates or assume the binary JSONB protocol avoids text serialization.

1. Inspect stored-column metadata only: use `pg_column_compression` and `pg_column_size`; reject any compressed variable-width value, total uncompressed JSONB+variable-text storage>24KiB, or scalar text field>512 bytes. No `data::text`, JSON child extraction, body hash or raw return occurs before this gate. Compressed physical size is never treated as an uncompressed bound.
2. Traverse the now physically bounded JSONB internally, with depth≤4, total nodes≤64 (including object entries/array elements), key UTF-8 bytes≤128 and string-value UTF-8 bytes≤512. Reject unknown/free-reason variants according to §4. Numeric values must use native JSONB→numeric conversion without a text intermediate, have scale=0 and lie within signed64 bounds. Numeric comparison/scale inspection precedes rendering: compact `1e100000` and valid stored high-scale `1e-16000` must quarantine without numeric-to-text expansion. Reject unsupported/fractional values rather than coercing them.
3. Only this structurally/numerically bounded value may be rendered in the SQL function. Bound the rendered intermediate by256KiB, including escaped keys/strings and surrounding fields, and prove that bound from the preceding node/string/key/integer limits. Then check exact serialized legacy-row wire bytes≤24KiB before returning anything to Rust or computing the admitted source digest. Final projected CloudEvents bytes must still be≤32KiB. A failed size check returns only event UUID/fixed code. Qualification verifies the bound; no generic unbounded `SELECT data` or `jsonb_send` happens on an unadmitted value.

These procedural guards also apply to migration/bootstrap and receipt capture, not just the dispatcher. Quarantine contains UUID and fixed admission/compatibility code, never raw content, truncated preview or a hash of an unbounded body. Preserve old storage unchanged. Tests include64MiB compressible reason, large uncompressed strings, compact huge exponent, high-scale numeric, nested numeric arrays, mixed bounded/unbounded rows and an observer that fails if guarded content crosses the client boundary. Restricted worker grants expose only this admitted projection; no arbitrary raw staging SELECT.

Existing nonzero attempts or delivered flags without verifiable sink receipts are not presumed successful; surface a reconciliation decision. Invalid/unqualified legacy rows remain quarantined and counted while unrelated bounded events continue.

Normal producer privilege is INSERT on staging and business data, without UPDATE/DELETE of evidence or sink access. Dispatcher can claim/settle state and read staging, never mutate business state or event payload. Enforce source immutability at the DB boundary; privileged maintenance is separate and explicitly audited.

Canonical digest version is explicit. Canonicalization recursively sorts object keys, retains array order and exact JSON primitive values, rejects non-finite numbers and records its algorithm version. Existing legacy timestamp arrays, where present in metadata, are validated and preserved as legacy metadata rather than falsely labeled RFC3339; envelope time is RFC3339.

## 6. Delivery state machine

State: pending → leased → delivered; leased → retry_wait; pending/leased → quarantined. Quarantine is retained terminal evidence requiring an explicit audited replay decision; it is not deletion or successful delivery.

Bounded policy: batch1–32, in-flight1–8, lease1–120s, attempts1–32, delay1–300s, poll1–60s. Persist policy revision and attempt limit on admission. Claim with database clock and `FOR UPDATE SKIP LOCKED`; generate unique lease token and owner. Expired leases are reclaimable; stale token cannot renew, settle or acknowledge. Admission never claims more rows than available dispatch capacity.

For each claim: validate immutable source/digest → project/validate envelope → ingest with event-ID idempotency → receive committed receipt → fenced ack. A permanent identity/receipt registry, retained across live envelopes and expiration tombstones, owns deduplication. Same ID/same digest returns its original live or expired receipt; an expired replay never recreates the payload. Same ID/different digest is integrity conflict and quarantine. Store unavailable/timeout/unknown commit retries the same immutable event ID. Crash after store commit before ack converges to duplicate receipt on restart. A failed/unknown ack never becomes a claimed success. Exhausted retry moves to visible quarantine, distinguishing uncertain outcome from invalid envelope; reconcile sink before authorized replay.

Initial runner may be sequential (`in-flight=1`) while retaining bounded batch contract; no unbounded queue. Shutdown stops claims, drains bounded work, and leaves uncertain/unfinished leases for expiry. Store outage delays delivery but cannot roll back a previously committed business mutation.

## 7. Audit Authority ingestion, ordering and roles

Ingest validates schema before append. A permanent `event_identity` registry owns unique event ID, canonical-envelope digest, source receipt and publication sequence. A separate live-envelope row may expire; its identity/receipt registry and authorized tombstone never expire in v1. Duplicate comparison consults this registry regardless of live-row presence. Ingest/expiration transitions are transactional; conflicting identity cannot overwrite either a live or expired record. Preserve source occurred_at and source receipt for correlation/reconciliation.

A PostgreSQL `nextval`/identity allocation is not commit order and must not define a completeness cursor. Instead each authority has one transactional publication-counter row. Every new ingest or control-event append locks that row, assigns the next `ingest_sequence`, inserts its registry/envelope, and advances the counter in the same transaction while holding the lock through COMMIT. Rollback advances neither. No subsequent transaction can publish a higher number while a lower number remains uncommitted. The visible counter value is the commit-closed watermark. All publication paths, including investigation control events and maintenance, must use it; direct insertion is denied. All sink state-changing/control paths use lock order publication counter → policy/control rows → identity registry rows sorted by event UUID. Ingest, expiration, policy changes and audited investigation must not acquire those locks in reverse order. Reads that need a later control event revalidate authorization in this order before disclosure; no remote call or streamed response holds these locks. The short serialization is an intentional v1 tradeoff to qualify.

A reader/checkpoint first reads watermark W and only pages immutable registry entries≤W. Its own control event publishes later and cannot enlarge that snapshot. Publication order is not business causal order or wall-clock order. Tests pause the lower append before commit, attempt a higher append/read/checkpoint, then commit and roll back the lower transaction in separate cases; no entry can be skipped behind an advanced cursor.

Roles are responsibilities, not Organization roles: producer/stager, dispatcher/ingester, investigator-reader, exporter, maintenance-admin. No normal business API updates/deletes Audit Authority. Separate least-privilege grant templates and SQL integration tests prove unauthorized write/read/export/maintenance attempts are rejected. Application checks alone are insufficient: direct table access must not bypass audited investigation. No production credential generation/grant is executed by this track.

An authorized investigation/search/export interface uses bounded filters (event IDs/types/source, actor/resource IDs, time range), a bounded page≤100 and a stable `(after_sequence, closed_watermark)` cursor; the watermark cannot be client-invented or advanced mid-scan. Search is exact structured filtering, not free-text indexing. Audit access has its own policy and must not reuse Document ACL as the investigation permission. Selected source/event class scope is explicit.

Each read/search/export/configuration/integrity-verification request appends one `audit.*` control event with caller, action, bounded query shape, result/count or receipt, and correlation before disclosing results. Denied access records a minimal `audit.access.denied` when possible and never discloses data even if that recording fails. Internal writes do not recursively call the investigation interface; writing an Audit record does not itself generate another record. Integrity checks have a finite scan high-water so their own result event does not extend the scan indefinitely.

## 8. Retention, export, backup and restore

Retention is explicit policy data, not a fixed company year count. Default configuration is no automatic expiration. Policies match approved event class/source and specify minimum duration and effective revision. Legal hold is reserved for future policy extension and defaults to retain when unsupported; do not invent legal policy.

Expiration plans bind candidate event IDs, stable cutoff, policy ID/revision, authority epoch and closed watermark; they are advisory, not permission to use stale policy. `expire` locks the current policy/control row and affected identity rows and re-evaluates current duration, retain-all/hold status, qualification and current live state in its deletion transaction. Policy updates use the same control lock. Unknown/deleted policy, revision mismatch or unsupported/unknown hold returns retain/stale-plan with no deletion. A concurrent extension either commits before expiration and prevents it, or follows the already-authorized expiration linearization point; no stale plan bypasses an extension.

Authorized sink expiration atomically removes only the live envelope, marks the permanent identity/receipt registry expired, appends the control event, and records a tombstone containing identity/digest/sequence/maintenance ID but no removed body/actor. A matching replay returns the expired receipt without resurrection; conflicting replay fails. The registry/tombstone remains indefinitely. Undelivered, unverified and reason-contract-unqualified evidence is ineligible. Tests cover expiry then dispatcher crash/replay, retention extension between plan and execution, and restored-source replays.

Destructive source staging/receipt cleanup is DEFERRED in v1. A live sink receipt is not evidence that every supported sink backup/WAL horizon can recover it. No source-delete API/grant is installed; all source evidence, especially deferred legacy reasons, remains protected from expiry. This consciously limits end-to-end retention erasure and storage reclamation; sink expiration must not be presented as deleting every retained source copy. A future source-cleanup contract requires a qualified recoverable backup/WAL horizon, external checkpoint continuity and a separately reviewed preservation/retrieval policy. No destructive maintenance runs against real data.

Export returns bounded JSONL plus version/count/high-water and manifest digest. Export access and the manifest are audited; exports inherit explicit access and retention policy. Backup/restore runbook uses existing PostgreSQL tools and includes both Authority data and source delivery state, configuration and trusted checkpoint handling. Restore opens a new, fail-closed recovery epoch: investigation/export/maintenance stay unavailable until retained source receipts, permanent identity/expiration registry, current policy and latest independently retained trusted checkpoint/recovery manifest reconcile. An older backup must not make a tombstoned event live or reset a newer retention policy. If the latest trusted policy/tombstone/receipt continuity cannot be established, remain recovery-blocked; do not solve uncertainty by replaying expired bodies or choosing an older convenient checkpoint. Only qualified ingestion/reconciliation paths operate during recovery.

Synthetic acceptance performs backup, restart, restore into a new database, revalidation and duplicate delivery, including restore older than an expiration and source replays that must not resurrect it. It also injects the forbidden state of a source row missing after a simulated unauthorized cleanup, then restores a sink backup predating that event: the outcome is explicit unrecoverable/recovery-blocked, never readiness. Source cleanup itself is not a supported v1 action. A successful SQL restore alone is not integrity verification. A backup under the same compromised administrator is not an independent trust anchor.

## 9. Health, reconciliation and operational safety

Keep produced, claim/attempted, sink-committed/stored, source-acked/delivered and integrity-verified evidence distinct. Health includes backlog/count/oldest age, retry/quarantine count, lease expiry, sink availability, last successful receipt and verification lag. Low-cardinality metric labels are fixed codes/classes only; no principal/resource/query/payload/error text. Audit pipeline can be degraded while Document accepts safely staged mutations. Loss of transactional staging makes the corresponding mutation fail; sink outage alone does not make it disappear.

Reconciliation compares source event IDs and receipt digests with sink receipts/tombstones only after bootstrap completeness is established, using commit-closed source and sink watermarks and fixed finite snapshot membership, classifying pending, stored-unacked, delivered-missing, hash mismatch, duplicate conflict, unauthorized mutation/deletion, expired-as-authorized and unresolved. Full reconciliation requires every source receipt in the closed prefix to have an explicit live, expired, pending or quarantined disposition and the bootstrap anti-join completeness record; a maximum identifier/count alone is insufficient. Repair is conservative: retry identical qualified events, never generate new IDs, recreate expired envelopes, or rewrite facts. Reconcile unknown commit before destructive actions. Restart does not reset attempt history or fabricate delivered receipts.

## 10. Delivery plan and acceptance

D-AUD1 contains A0/design/plan/authorization and independent review. A-AUD1 qualifies schema/compatibility/privacy. A-AUD2 qualifies PostgreSQL source/store, dispatcher, integrity/roles/retention/export/recovery. A-AUD3 exercises concrete existing Document operations through the real transaction → outbox → dispatcher → separate store path, plus operational acceptance/handoff. All are independent Draft branches. Parent coordinates publication and verifies exact remote tree/head/CI.

Required failure cases: source insert/receipt failure rolls back business; store down leaves source pending and business committed; dispatcher crash; after-store-before-ack; simultaneous duplicate; changed event ID content; stale lease; unknown commit; invalid schema; forbidden payload; unauthorized investigation/export; retention before/after cutoff with audit/tombstone; restart; restored backup; source/sink missing event; checkpoint mutation/deletion/order mismatch. Concrete producers: create, publish, first read confirmation, file/Diff access, denial and qualified library scheduler actor attribution must reach the sink when their variants qualify; reason-bearing withdrawal/publication-end/metadata/ACL/Folder variants must prove atomic preserved-source quarantine, not a false delivered/qualified receipt. Ordinary list reads/no-op/replay must not gain new Document audit semantics.

A6 publishes only a handoff contract: future Org Phase1/2-approved producers can provide explicit stable identity/resource/correlation and a separately reviewed versioned metadata catalog. No WorkContext/Workflow/Role/Assignment/Delegation meaning is frozen here. Search NO_RETENTION policy remains stronger than convenience telemetry/audit recording.

## 11. STOP and verification boundaries

Stop only the affected dependency when approved Audit meaning would break, PR43 dependency is unavoidable, Org semantics are essential, migration must break old events, external operational/paid service is needed, license/security policy fails, production deployment/credentials are needed, integrity conflicts with transaction atomicity, or generic-outbox ownership requires a major architecture decision. Continue independent portions when possible.

Independent architecture/security review resolves P1/P2 before schema implementation; bounded backend qualification and new crate/files/additive migrations/adapters are allowed implementation work. Full repository checks, focused real-DB failure tests and exact-head hosted gates remain separate receipts. NOT RUN/FAIL/PENDING are never relabeled green.

Primary standards: [CloudEvents1.0.2](https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/spec.md), [JSON Event Format](https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/formats/json-format.md), [PostgreSQL18 privileges](https://www.postgresql.org/docs/18/sql-grant.html), [stored value size/compression functions](https://www.postgresql.org/docs/18/functions-admin.html#FUNCTIONS-ADMIN-DBSIZE), [TOAST](https://www.postgresql.org/docs/18/storage-toast.html). These define protocol/mechanism; repository contracts define this capability's meaning.
