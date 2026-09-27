# Document Versioning v0 Production Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task by task in the current session. Steps use checkbox (`- [ ]`) syntax for tracking. The user selected inline execution without worker delegation.

**Status:** **APPROVED — production implementation authorized**

**Approval:** The user explicitly replied “承認します。” to this plan on 2026-09-27 JST. The approved proposal head was `design/document-versioning-v0@69371a01b8abff48a443554621816823af5ca9f2`. The approval record is `docs/superpowers/plans/2026-09-27-document-versioning-v0-production-implementation-approval.md`.

**Goal:** Implement Version #2+ management, semantic identity, safe withdrawal with preceding-Version restoration, and durable scheduled publication while preserving initial Publish and frozen DSI contracts.

**Architecture:** Keep Domain rules independent of infrastructure, orchestrate immutable storage and DSI in Application, and commit authoritative state/operation records/Outbox events atomically in PostgreSQL. Extend the existing Publish ledger/API for initial and later due publication. A dedicated Linux scheduler binary reads durable due intents and invokes the same Application Publish path.

**Tech Stack:** Rust 1.98.1 workspace, PostgreSQL/sqlx migrations, `time`, `uuid` v7, the existing DSI core/runner and filesystem storage adapters. `unicode-normalization 0.1.25` is already present and qualified in the production DSI dependency set; no new parser dependency is planned.

**Spec:** `docs/superpowers/specs/2026-09-27-document-versioning-v0-design.md` and its approval record; normative `spec/data/logical-data-model-v0.md` and `spec/data/transaction-consistency-requirements-v0.md`.

## Global Constraints

- Preserve `WORKING / PUBLISHED / WITHDRAWN`; add no withdrawal/restored/current/superseded flag or lifecycle state.
- A new Version #2+ starts only from current `PUBLISHED`; at most one `WORKING` per Document. `version_no` is allocated under the Document row lock.
- Version identity is normalized title plus ordered `logical_path + ordinal` ContentItems and compatible format-native DSI fingerprints. No Search Extraction dependency or durable common cross-format IR.
- All authoritative FileObjects must be registered and inspected before create/update; inspection failure is not semantic change. Authority-format migration is a separate operation.
- Keep successful Publish replay in `document_publish_operations`; caller operation IDs are UUIDv7. Commit ambiguity is resolved with the same ID.
- Publish quality rejects unresolved Track Changes, any embedded comments, and existing invalid/unverifiable signatures; unsigned files are permitted.
- Withdrawal restores only the immediate base when still `PUBLISHED` and safe. Otherwise current becomes null. The prior Version's content and original `published_at` stay unchanged.
- A schedule is a durable intent for Version #1 or #2+, not a Version state. Due execution revalidates and cannot publish early; cancellation precedes ordinary edits or a different manual Publish.
- Keep legacy ambiguous ATTACHMENT classification fail-closed. Do not silently retain `version_files` as a second editable authority.
- Do not add a `POC REQUIRED` dependency to production. The scheduler image may only package already approved DSI runtime components and their pinned native assets.
- Implementation is inline in this session. If the user later requests a managed worker, use exactly `gpt-6-sol` / `ultra`.
- Do focused RED/GREEN local checks per task. Do not dispatch hosted CI per task; run final exact-head standard CI, DSI Sandbox Preflight, and DSI PoC regression once after the complete implementation branch is ready. Do not merge a PR without an explicit user instruction.

## Review Focus

1. Legacy initial Version with an ATTACHMENT must remain readable through its historical path but refuse automatic ContentItem classification; Task 2 pins both behaviors.
2. Withdrawal must complete with null current if the immediate base is missing or storage/DSI validation is unavailable; Task 6 pins the former/current IDs and event reason.
3. A due schedule whose target, current, revision, or actor intent has changed must never publish; Tasks 7–8 pin cancel/edit/withdrawal and duplicate-worker races.
4. NFC-equivalent paths/titles, `..`, and same ordinal/path collisions must not produce divergent or ambiguous Version identity; Task 1 pins the normalized vectors.
5. An uncertain commit followed by same-ID replay must produce one revision and one event pair even across scheduler duplicates; Tasks 4, 5, 6, and 8 pin ledger behavior.

## File and interface map

| Unit | Responsibility | Planned files |
|---|---|---|
| Domain Version model | ContentItem/path/title identity, base lineage and lifecycle transitions | `crates/document-domain/src/versioning.rs`, `document.rs`, `error.rs`, `lib.rs`, `Cargo.toml` |
| Authoritative PostgreSQL schema | ContentItems, base FK, one-WORKING constraint, operation and schedule ledgers | `crates/document-repository-postgres/migrations/0004_document_versioning_v0.sql`, `src/versioning_rows.rs` |
| Canonical initial read/write | Backfill simple Version #1, block ambiguous attachments, remove Version #1-only read assumption | `crates/document-repository-postgres/src/repository.rs`, `mapping.rs`, `rows.rs`, `src/versioning.rs` |
| Application preflight | FileObject registration, DSI ensure, semantic manifest and Publish quality | `crates/document-application/src/versioning_preflight.rs`, `publish_quality.rs`, `ports.rs`, `error.rs` |
| Version commands | Create/update/rebase idempotent orchestration | `crates/document-application/src/versioning_command.rs`, `versioning_service.rs`, `events.rs`, `lib.rs`; `crates/document-repository-postgres/src/versioning.rs` |
| Publish/withdrawal | Version #2+ current switch and safe immediate-base restoration | `crates/document-domain/src/document.rs`; `crates/document-application/src/service.rs`, `versioning_service.rs`, `ports.rs`; `crates/document-repository-postgres/src/publish.rs`, `withdrawal.rs` |
| Schedule and runtime | Reserve/cancel/due ledger, retry, and runnable Linux scheduler | `crates/document-application/src/schedule.rs`; `crates/document-repository-postgres/src/schedule.rs`; `crates/document-publication-scheduler/{Cargo.toml,src/main.rs,src/runner.rs}`; `Cargo.toml`, `Dockerfile` |

The interface names below are the intended public boundaries. Keep SQL transaction details within Repository and the DSI worker trust boundary unchanged. Each task may add a small focused helper file where necessary, but any changed public signature must be reflected in this plan before implementing dependent tasks.

---

### Task 1: Domain ContentItem and Version transition contract

**Files:** Create `crates/document-domain/src/versioning.rs`; modify `src/document.rs`, `src/error.rs`, `src/lib.rs`, `Cargo.toml`; test in `src/versioning.rs` and `src/document.rs`.

**Interfaces:** Export `LogicalPath::new(&str) -> Result<Self, DomainError>`, `VersionManifest::new(Title, Vec<SemanticContentItem>) -> Result<Self, DomainError>`, and `DocumentVersion::base_document_version_id() -> Option<DocumentVersionId>`. Add `Document::publish_next_version(&mut self, target: &mut DocumentVersion, published_at: OffsetDateTime) -> Result<PublishTransition, DomainError>` and `Document::withdraw_version(&mut self, target: &mut DocumentVersion, eligible_base: Option<&DocumentVersion>, withdrawn_at: OffsetDateTime) -> Result<WithdrawTransition, DomainError>`. Application supplies only a preflight-qualified eligible base; Domain validates its identity/state and returns resulting current/revision without storage or DSI access.

Pin `VersionManifest::identity_digest()` to SHA-256 over a versioned, length-prefixed byte stream: domain tag `document-version-identity-v0\0`, normalized UTF-8 title, `u32` item count, then each item in `(ordinal, logical_path)` order with UTF-8 path, `u32` ordinal, format ID, inspection profile ID, and the 32-byte DSI semantic fingerprint. Lengths and ordinals are big-endian `u32`; FileId, raw hash, rendition, and management metadata are excluded. Test vectors must include NFC-equivalent inputs, CRLF/LF title normalization, reordered items, changed logical path, and changed semantic digest.

- [ ] Write RED tests named `versioning_path_normalizes_nfc_and_rejects_ambiguous_segments`, `version_identity_uses_title_order_path_and_semantic_digest`, `one_working_base_and_rebase_rules`, and `withdraw_transition_restores_immediate_published_base_or_null`. Pin same/different manifest pairs, no extra lifecycle state, and unchanged predecessor `published_at`.
- [ ] Run `cargo test -p document-domain versioning_` and the focused `withdraw_transition` test; confirm only new contract assertions fail.
- [ ] Implement path/title normalization, manifest identity, base accessor and transitions. Use existing `VersionNo`, `Title`, and ID types; keep Domain free of SQL, Search, and DSI runtime types. Add only already qualified `unicode-normalization = "0.1.25"` if needed.
- [ ] Run focused Domain tests and `cargo fmt --check`; commit the Domain contract and tests.

### Task 2: Canonical schema, legacy backfill, and initial read/write

**Files:** Create `crates/document-repository-postgres/migrations/0004_document_versioning_v0.sql`, `src/versioning_rows.rs`, `tests/versioning_schema.rs`, `tests/versioning_legacy.rs`; modify `src/repository.rs`, `mapping.rs`, `rows.rs`, `lib.rs`, and `crates/document-application/src/ports.rs` where the initial aggregate needs canonical ContentItems.

**Interfaces:** Canonical tables are `content_items(document_version_id, logical_path, ordinal, ...)` and `content_representations(content_item_id, file_id, role, ...)`. Add `document_versions.base_document_version_id` with same-Document FK, a partial unique index for one `WORKING`, `document_version_operations`, `document_publish_schedules`, and an explicit legacy-remediation marker. `get_authoritative_document` loads the requested/current Version rather than assuming `version_no = 1`.

- [ ] Write RED PostgreSQL tests for same-Document base ownership, one `WORKING`, one authoritative representation per item, deterministic single-PRIMARY backfill as `primary`/0, and a legacy ATTACHMENT that remains historically readable but is blocked from Versioning until classified.
- [ ] Run `cargo test -p document-repository-postgres --test versioning_schema --test versioning_legacy`; confirm the new schema/reader expectations fail.
- [ ] Add the migration and canonical initial Create/Publish read-write path in one coherent change. Preserve historical `version_files` rows as compatibility data only; reject ambiguous migration rather than guessing a role. Generalize the Version #1-only mapping.
- [ ] Run the two focused new tests plus existing `repository_contract` and `publish_transaction`; commit the migration and compatibility path.

### Task 3: Immutable FileObject registration, DSI manifest, and quality preflight

**Files:** Create `crates/document-application/src/versioning_preflight.rs`, `publish_quality.rs`, `tests/versioning_preflight.rs`; modify `src/ports.rs`, `error.rs`, `lib.rs`; implement `register_file_object` in `crates/document-repository-postgres/src/versioning.rs`.

**Interfaces:** `VersioningRepository::register_file_object(file: FileObject) -> Result<(), RepositoryError>` is idempotent only for the same raw binding. `VersioningPreflight::prepare(items, InspectionProfileVersion::DsiV0) -> Result<PreparedManifest, ApplicationError>` finalizes immutable objects, registers FileObjects, calls existing `EnsureSemanticInspection::ensure(FileId, profile)`, and returns a complete ordered manifest. `check_publish_quality(&PreparedManifest) -> Result<(), ApplicationError>` evaluates editorial/signature evidence and file openability.

- [ ] Write RED tests for every authoritative item being inspected, raw-binding mismatch, unsupported/ambiguous inspection, orphan FileObject after later Version failure, unsigned success, and tracked-change/comment/invalid-signature quality failure. Include rendition-only additions not changing Version identity.
- [ ] Run `cargo test -p document-application --test versioning_preflight`; verify intended RED. Use fakes; do not launch the DSI worker for these Application contracts.
- [ ] Implement the new capability-specific repository port and preflight helpers. Reuse the frozen DSI runner/ensure path; no FileId, credentials, storage locator, or Principal reaches the worker. Record an unreferenced FileObject for reconciliation instead of treating it as a committed Version.
- [ ] Run focused Application and PostgreSQL FileObject registration tests; commit preflight and quality gates.

### Task 4: Create, update, and explicit rebase with operation replay

**Files:** Create `crates/document-application/src/versioning_command.rs`, `versioning_service.rs`, `tests/versioning_commands.rs`; implement `crates/document-repository-postgres/src/versioning.rs` and `tests/versioning_transaction.rs`; modify Application/Repository `ports.rs`, `events.rs`, `lib.rs`.

**Interfaces:** Add UUIDv7 `VersionOperationId`; commands `CreateVersionCommand`, `UpdateWorkingVersionCommand`, `RebaseWorkingVersionCommand` each carry expected Document revision and actor. `DocumentVersionService::create_version/update_working/rebase_working` uses `PreparedManifest`; `VersioningRepository::create_version/update_working/rebase_working` are capability-specific atomic methods sharing one internal successful-operation ledger and returning the stored result or Conflict.

- [ ] Write RED contract and PostgreSQL tests for no current base, semantic no-change, duplicate `WORKING`, transactional `version_no`, stale revision/base, update preserving Version ID/number, explicit rebase, same-ID replay, changed-command ID conflict, and one Domain/Audit event pair. Include a two-creator concurrency race.
- [ ] Run `cargo test -p document-application --test versioning_commands` and `cargo test -p document-repository-postgres --test versioning_transaction`; capture focused RED.
- [ ] Implement Application orchestration and short Document-locked transactions. Allocate version number only after locking; compare complete manifests; keep a failed update from partially replacing ContentItems. Retain successful operation rows without TTL and map uncertain commits to same-ID recovery.
- [ ] Run the two focused suites and `cargo fmt --check`; commit Version operations.

### Task 5: Publish Version #2+ through the existing Publish ledger/API

**Files:** Modify `crates/document-domain/src/document.rs`, `crates/document-application/src/versioning_service.rs`, `ports.rs`, `events.rs`, `crates/document-repository-postgres/src/publish.rs`, `repository.rs`; create `crates/document-application/tests/publish_next_version.rs`, `crates/document-repository-postgres/tests/publish_next_transaction.rs`.

**Interfaces:** Keep `PublishDocumentCommand` and `PublishOperationId`. Extend `DocumentPublishRepository` with `publish_next_version(record: PublishVersionRecord) -> Result<PublishDocumentResult, RepositoryError>`. `DocumentVersionService::publish_document(command: PublishDocumentCommand)` uses DSI/quality preflight and chooses initial scheduled or next-Version path from the loaded state; both use the same successful-operation ledger and replay rules. Existing `DocumentService::publish_document` remains the compatible manual Version #1 API.

- [ ] Write RED tests for Version #2+ current switch, old current staying `PUBLISHED`, stale base/current/revision Conflict, all authoritative items passing DSI/quality, manual Publish blocked by a different pending schedule, same-ID replay, and an initial-Version compatibility case.
- [ ] Run `cargo test -p document-application --test publish_next_version` and `cargo test -p document-repository-postgres --test publish_next_transaction`; confirm intended RED.
- [ ] Extend Publish candidate read/preflight and the transaction under the existing Document lock. Recheck every authoritative binding and schedule ownership inside the transaction; insert Publish ledger, current switch, revision, and Domain/Audit events atomically. Do not change the initial manual Publish business contract.
- [ ] Run focused new suites plus existing `publish_document_contract` and `publish_transaction`; commit Publish extension.

### Task 6: Withdraw current/historical Version and restore the immediate safe base

**Files:** Create `crates/document-repository-postgres/src/withdrawal.rs`, `tests/withdrawal_transaction.rs`, `crates/document-application/tests/withdrawal_contract.rs`; modify `crates/document-application/src/versioning_command.rs`, `versioning_service.rs`, `ports.rs`, `events.rs` and Domain transition code.

**Interfaces:** `WithdrawVersionCommand` has caller UUIDv7 operation ID, expected Document revision, actor, target Version and reason. `DocumentVersionService::withdraw_version` preflights only the immediate base as fallback. `VersioningRepository::withdraw_version(record) -> Result<WithdrawVersionResult, RepositoryError>` commits `WITHDRAWN`, current, revision, schedule invalidation, successful operation row, and Domain/Audit Outbox together.

- [ ] Write RED tests for safe #2→#1 restoration, #1→null, historical withdrawal preserving current, base already `WITHDRAWN`, base object missing, validation service unavailable, unchanged original `published_at`, no search past base, cancelled/stale schedule, same-ID replay and changed-ID conflict.
- [ ] Run `cargo test -p document-application --test withdrawal_contract` and `cargo test -p document-repository-postgres --test withdrawal_transaction`; capture focused RED.
- [ ] Implement fallback preflight and the locked transaction. A fallback that cannot be proved safe yields null while withdrawal still succeeds; emit former/resulting current IDs and reason. Do not add a withdrawal/restored flag or mutate historical content.
- [ ] Run focused suites, including one two-withdrawer race; commit withdrawal.

### Task 7: Reserve and cancel scheduled publication

**Files:** Create `crates/document-application/src/schedule.rs`, `tests/schedule_contract.rs`, `crates/document-repository-postgres/src/schedule.rs`, `tests/schedule_transaction.rs`; modify Application/Repository `ports.rs`, `events.rs`, `lib.rs`.

**Interfaces:** `SchedulePublishCommand` carries the future `PublishOperationId`, UTC due instant, expected revision and initiating actor. `CancelScheduleCommand` carries a distinct UUIDv7 operation ID and expected revision. `DocumentVersionService::schedule_publish/cancel_schedule` calls `PublicationScheduleRepository::reserve/cancel`, which owns the schedule ledger and atomic `scheduled_publish_at` projection. The accepted post-reservation Document revision is stored for due Publish.

- [ ] Write RED tests for initial/later Version reservation, nonfuture time rejection, quality failure before reservation, one active schedule, exact replay, ID mismatch, cancellation, rescheduling with a new ID, and edit/manual Publish blocked while pending.
- [ ] Run `cargo test -p document-application --test schedule_contract` and `cargo test -p document-repository-postgres --test schedule_transaction`; capture focused RED.
- [ ] Implement the repository transaction and Application preflight. Record initiating Principal as authorized durable intent; cancellation is the v0 revocation path. Update `approved_at`/`scheduled_publish_at`, revision, schedule row, and Domain/Audit Outbox atomically.
- [ ] Run both focused suites; commit reservation/cancellation.

### Task 8: Due execution and runnable Linux scheduler

**Files:** Create `crates/document-publication-scheduler/Cargo.toml`, `src/main.rs`, `src/runner.rs`, `tests/due_publication.rs`; modify root `Cargo.toml`, `Dockerfile`, Application/Repository schedule modules and `tools/architecture-lint` configuration only if the new crate needs a declared boundary.

**Interfaces:** `PublicationScheduleRepository::list_due(database_now, limit)` returns persisted pending IDs; `DocumentVersionService::execute_due(publish_operation_id)` reuses the stored actor and invokes the existing Publish operation. The binary wires `PostgresDocumentRepository`, `FileSystemStorage`, `RunnerInspectionExecutor`, and `InspectionProfileVersion::DsiV0`, polling with bounded backoff and graceful shutdown on Linux. Add a named scheduler target to `Dockerfile` without changing the existing `assure` default target.

- [ ] Write RED tests for no early Publish, due #1 and #2+, duplicate concurrent runners, current/revision/manifest change, cancellation, transient retry under the same ID, terminal quality failure, and success with exactly one Publish ledger row/event pair. Add a binary startup test that fails closed when the mandatory DSI sandbox is unavailable.
- [ ] Run `cargo test -p document-publication-scheduler --test due_publication` and the focused schedule repository tests; capture focused RED.
- [ ] Implement due selection and transaction checks using the database clock. Publish and schedule completion commit together; terminal failure clears the projection and increments revision once, while infrastructure failures retain the pending intent and use the same ID. Package a runnable Linux artifact and document required nonsecret runtime configuration; never log database credentials or document content.
- [ ] Run the focused scheduler and Application/Repository schedule tests plus a single Linux-container due-publication canary against test PostgreSQL and FileStorage; commit the runnable scheduler.

### Task 9: Whole-capability evidence and handoff

**Files:** Add focused integration cases to `crates/document-repository-postgres/tests/versioning_vertical_slice.rs`; update `docs/superpowers/execution/document-versioning-v0-status.md` and `active.md`. Open/update the Production PR only after the complete implementation is reviewable.

**Interfaces:** The integration path exercises create Version #1 → Publish → create/update/rebase Version #2 → schedule/due Publish → withdraw and restore, with DSI-backed ContentItems. The final branch must retain one authoritative FileObject/ContentItem model and a runnable scheduler.

- [ ] Write a RED vertical slice and migration/rollback canary covering the approved Design's nine acceptance areas, including orphan reconciliation, ambiguous legacy attachments, and an initial scheduled Publish.
- [ ] Run only the focused vertical slice until GREEN. Then run `mise run verify` once for the assembled branch; fix any actual failures without changing frozen semantics.
- [ ] Push the coherent implementation head and obtain exact-head standard CI, DSI Sandbox Preflight, and DSI PoC regression **SUCCESS**. If a code fix changes HEAD, obtain fresh evidence for that HEAD; do not claim completion on prior runs.
- [ ] Record exact head, run IDs/results, remaining blockers, Design amendments (if any), and the next exact action in Active/Status. Review the whole PR once. Do not merge without an explicit user instruction.

## Plan approval gate

This plan is approved for inline production implementation. If implementation evidence requires changing frozen Version identity, withdrawal restoration, scheduled intent, Publish quality, or the DSI boundary, stop that implementation path and request a Design amendment before proceeding.
