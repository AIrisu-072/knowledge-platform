# Document Versioning v0 — Design

- Status: **APPROVED — design freeze active**
- Date: 2026-09-27
- Capability: `Document Versioning v0`
- Baseline: `main@09c8235755573d16e09b8af029c22dc27caf6472`
- Design branch: `design/document-versioning-v0`
- Scope class: Product Capability / Architectural

## 1. Purpose

Add Version #2+ creation and editing, semantic change detection, current-Version replacement, withdrawal with restoration of the preceding published Version, and durable scheduled publication. Reuse the existing Document Domain/Application/PostgreSQL boundaries and the successful-operation Publish ledger. Document Semantic Inspection (DSI) supplies format-native semantic evidence; Search Extraction does not decide Version identity.

This approved design does not authorize implementation without a separately approved plan. The approved Document Publish v0 operation remains the initial-Version baseline; this design explicitly extends its deferred Version #2+ and lifecycle behavior.

## 2. Normative context and precedence

This design must remain consistent with:

- `spec/data/logical-data-model-v0.md`;
- `spec/data/transaction-consistency-requirements-v0.md`;
- `spec/architecture/architecture-contract-v0.md`;
- `spec/operations/error-handling-resilience-requirements-v0.md`;
- `spec/operations/observability-audit-requirements-v0.md`;
- the approved Document Publish v0 design (`2026-09-17`);
- the approved Document Semantic Inspection v0 design (`2026-09-20`), especially §§2, 4, 7, 10, and 18.

`spec/` remains normative. The transaction requirements explicitly defer the current-Version behavior on withdrawal and the execution method for scheduled publication. Approval of this design settles those deferred choices; the corresponding normative sections must then be updated before production implementation. Any conflict with an already frozen rule requires an explicit amendment rather than an implicit change here.

## 3. Scope

### Included

- At most one mutable `WORKING` Version per Document; Version #2+ is based on its current `PUBLISHED` Version, while initial Version #1 has no base.
- Repository-transaction allocation of monotonic `version_no`.
- A Version containing one or more ordered ContentItems, each with exactly one authoritative immutable FileObject and zero or more renditions.
- Create, update, explicit rebase of a stale `WORKING` Version, Publish Version #2+, withdraw a `PUBLISHED` Version, schedule/cancel publication of either initial or later `WORKING` Versions, and execute due schedules.
- Semantic no-change decisions from DSI evidence, with all authoritative ContentItems inspected.
- OCC, stable UUIDv7 operation identities, commit-ambiguity recovery, Domain Outbox, and mandatory Audit Outbox.
- A safe migration from the existing Version #1 `version_files` representation.

### Excluded

- Document Diff, Search indexing or delivery implementation, HTTP/UI transport, approval workflow, AccessPolicy implementation, and legal deletion.
- Automatic authority-format migration, a durable common cross-format content IR, and treating ZIP as authoritative content.
- Automatic re-publication of a `WITHDRAWN` Version. If a first Version is withdrawn and no preceding published Version exists, the Document has no current Version; reopening that Document requires a separately designed operation.
- Hard real-time publication guarantees. A schedule is an at-least-once durable intent, subject to due-time validation and infrastructure availability.

## 4. Version and ContentItem model

### 4.1 Version lineage and lifecycle

The initial Version #1 has no base. Every new Version #2+ records `base_document_version_id`, equal to the Document's current `PUBLISHED` Version when the `WORKING` Version is created. The base belongs to the same Document and remains fixed unless an explicit rebase succeeds. A Version cannot be created when `current_version_id` is null. At most one `WORKING` Version exists per Document, enforced by a partial unique index as well as Domain checks.

The persisted lifecycle remains exactly `WORKING`, `PUBLISHED`, and `WITHDRAWN`. Content of a `PUBLISHED` or `WITHDRAWN` Version is immutable. `current_version_id` is either null or points to a `PUBLISHED` Version of the same Document. Withdrawal changes lifecycle and the current pointer; no additional withdrawal, restored, current, or superseded boolean is introduced.

The base link is the immediate predecessor for restoration. Withdrawal never searches past that predecessor to expose an older Version automatically. If the predecessor is no longer `PUBLISHED` or cannot be proved safe to expose, the new current is null.

### 4.2 ContentItems

Each ContentItem is scoped to one Version and has a stable `logical_path`, nonnegative `ordinal`, and exactly one authoritative representation. `(document_version_id, logical_path, ordinal)` is unique. A Version has at least one ContentItem. The manifest order is deterministic by `ordinal` and then `logical_path`; duplicate normalized keys are rejected. Paths are Unicode NFC, case-sensitive, relative paths with `/` separators. Empty segments, leading `/`, `.`/`..`, traversal, and ambiguous normalized paths are rejected. A stable ContentItem identity may be used for storage, but cross-Version matching uses manifest keys and semantics rather than a guessed global ContentItem ID.

An authoritative representation references an immutable FileObject and a successful DSI record under the selected, versioned inspection profile. Renditions reference FileObjects but do not participate in Version identity. Adding or regenerating an equivalent rendition does not create a Version. Each item has one authoritative representation at a time; a change of authoritative format is an explicit Authority Migration operation outside ordinary Version update.

### 4.3 Semantic Version identity

Version identity consists of normalized title plus the ordered ContentItem manifest. Each manifest entry contributes its `logical_path`, `ordinal`, authoritative representation format/profile, and format-native DSI semantic fingerprint. Title normalization is versioned and deterministic: Unicode NFC, CRLF/CR to LF, and removal of leading/trailing whitespace; internal whitespace and case remain significant. An empty normalized title is invalid. The implementation plan must pin the exact normalization and serialization vectors before code is written.

Raw SHA-256 is used for immutable-file binding and integrity, not semantic equality. Same-format authoritative items compare their DSI fingerprints under a compatible profile. Different authoritative formats are not equated by comparing digests; format changes require the separately approved capability-evidence Authority Migration path. Reader-relevant title, manifest order/path, or authoritative semantics must differ from the base to create or retain a new `WORKING` Version. Management metadata and renditions alone do not create a Version.

## 5. Immutable file and inspection boundary

Application finalizes each candidate object in immutable FileStorage and registers its FileObject before calling `EnsureSemanticInspection(FileId, profile)`. DSI rechecks the authoritative raw binding and persists only complete successful evidence. Every authoritative item in both the candidate and comparison base must have successful compatible inspection. Inspection failure, inability to inspect, or ambiguous semantic evidence fails closed; it is not interpreted as a content change.

The FileObject registration and later Version transaction cannot be made one cross-system ACID transaction. An object/FileObject left unreferenced after a failed Version operation is handled by the existing reconciliation/retention path, never by pretending the Version committed. The implementation plan must define an explicit orphan observation and cleanup policy. No FileId, storage credential, or database credential is passed to the DSI worker; the frozen DSI trust boundary remains intact.

## 6. Create, update, and rebase `WORKING`

### Create

The caller supplies a UUIDv7 create operation ID, Document ID, expected Document revision, target Version ID, title, ordered manifest, and actor. Application inspects all authoritative items and the current base before the repository transaction. Under a short Document row lock, Repository rechecks the operation ID, expected revision, current `PUBLISHED` base, absence of another `WORKING` Version, immutable FileObject/inspection bindings, and semantic difference. It allocates `version_no` inside that transaction, creates the Version and ContentItems, increments Document revision once, stores the successful operation result, and appends Domain and Audit Outbox records atomically.

### Update

The caller supplies a distinct UUIDv7 update operation ID and expected Document revision. Update is permitted only while the target remains `WORKING`, has no active publication schedule, and its recorded base still equals the current `PUBLISHED` Version. The new complete manifest replaces the `WORKING` manifest atomically after all authoritative items inspect and differ from the base. `version_no` and `document_version_id` do not change; Document revision increments once. A failed or ambiguous update leaves the previous complete manifest authoritative.

### Explicit rebase

If publication or withdrawal changes current while a `WORKING` Version exists, that Version is stale and cannot publish. An explicit rebase command, with a UUIDv7 operation ID and expected revision, may set its base to the new current `PUBLISHED` Version after reinspection and semantic comparison against that base. Rebase never silently changes content or creates a new Version number. If current is null, rebase is unavailable. A pending schedule must be cancelled or terminally invalidated first.

Create/update/rebase operation records retain command identity and result without a v0 TTL. Exact replay returns the stored result with no additional revision or events; reuse of an ID with different command identity is Conflict. On uncertain commit outcome, the caller retries the same ID. A new ID is never used to guess whether the earlier mutation committed.

## 7. Publish Version #2+

`PublishDocument` and its existing `document_publish_operations` ledger remain the publication contract. Version #1 initial publication retains its approved preconditions. The Version #2+ branch requires the target to be `WORKING`, its base to equal the current `PUBLISHED` Version, the expected Document revision to match, and no different active schedule to own the target. The target manifest must still be semantically different from its base.

Before a new Publish transaction, Application checks every authoritative final object can be opened and that its DSI raw binding/evidence is valid. Publish quality fails closed for unresolved tracked changes, any embedded comments, and existing invalid or unverifiable digital signatures. An unsigned file is permitted. Inspection success is distinct from publish-quality acceptance. The exact successful-operation replay is resolved before preflight, as in Publish v0.

Under a Document lock, Repository reloads target, base/current, manifest, inspection references, schedule ownership, and revision. It changes target `WORKING -> PUBLISHED`, sets `published_at`, points `current_version_id` to target, increments Document revision once, records the successful Publish operation, and appends Domain/Audit Outbox records in one transaction. The previous current remains `PUBLISHED` but is historical. Distinct concurrent Publish IDs cannot both succeed. `CommitOutcomeUnknown` recovery uses the same Publish ID and ledger.

## 8. Withdrawal and preceding-Version restoration

A withdrawal command supplies a caller UUIDv7 operation ID, Document ID, target `PUBLISHED` Version, expected Document revision, actor, reason, and timestamp. It is valid for a current or historical `PUBLISHED` Version. The command ledger gives exact replay and commit-ambiguity recovery; ID reuse with a different command is Conflict.

When the target is current, the restoration candidate is exactly `target.base_document_version_id`. Application checks that candidate is still `PUBLISHED` and that its authoritative content can be opened and passes the same applicable publication-quality checks. If the candidate is absent, no longer `PUBLISHED`, missing, or cannot be proved safe because validation is unavailable, withdrawal still succeeds with `current_version_id = null`. It does not skip automatically to a more distant ancestor. Repository rechecks candidate state and evidence under the transaction lock; preflight does not claim cross-system ACID guarantees. When the target is historical, current stays unchanged.

The transaction changes the target to `WITHDRAWN`, sets `withdrawn_at`, switches current to the eligible predecessor or null, increments Document revision once, records the successful operation, and writes Domain/Audit Outbox events atomically. These events contain the withdrawn Version ID, former current ID, resulting current ID, actor, reason, and whether restoration was withheld by validation. Search consumers can remove the withdrawn Version and reindex the restored current through normal Outbox delivery. The events and Audit history show that a prior current was restored; no separate `restored` flag or lifecycle state exists.

| Before withdrawal | After withdrawal | Historical evidence |
|---|---|---|
| Target #N is current; its immediate base #N-1 is safe and `PUBLISHED` | #N is `WITHDRAWN`; #N-1 remains `PUBLISHED` and becomes current | Withdrawal event records `former_current=#N`, `resulting_current=#N-1` |
| Target #N is current; no eligible immediate base | #N is `WITHDRAWN`; current becomes null | Withdrawal event records `former_current=#N`, `resulting_current=null` and the reason |
| Target is historical `PUBLISHED` | Target becomes `WITHDRAWN`; current does not change | Withdrawal event records the unchanged current |

Withdrawal invalidates any pending schedule whose recorded base/current assumption it changes. An existing `WORKING` Version based on the withdrawn current becomes stale and requires explicit rebase before publication. With no eligible predecessor, the Document remains without a current Version; it cannot create another Version under the frozen current-base rule.

## 9. Durable scheduled publication

### 9.1 Reservation

Scheduling is an explicit future Publish intent, not another Version lifecycle state. It supports Version #1 with null base/current and Version #2+ with a current `PUBLISHED` base. The caller supplies a UUIDv7 `publish_operation_id`, Document ID, target `WORKING` Version, expected Document revision, actor, and a future UTC instant. A schedule row keyed by the future Publish ID stores the target, base/current ID, accepted Document revision, due instant, actor, immutable target manifest/inspection identity, and operation status. A partial unique constraint permits one pending schedule per Document/target. `document_versions.scheduled_publish_at` is maintained in the same transaction as the schedule row as a read projection, not an independent command source.

Application performs the full Publish preflight and quality checks when accepting the schedule. In a short transaction, Repository rechecks revision, target/base/current, inspection bindings, and schedule uniqueness; records the pending intent; sets `approved_at` if not already set and `scheduled_publish_at`; increments Document revision once; and writes Domain/Audit Outbox records. Here `approved_at` records publication readiness, not a separate human approval workflow. The stored accepted revision is the post-schedule revision expected by the due Publish. A repeated identical schedule ID is an exact replay; a different payload with the same ID is Conflict.

Scheduling records an authorized publication decision by the initiating actor. At due time, the scheduler executes that recorded intent as the service executor, not by impersonating the actor. The initiating actor and service executor are both recorded in audit evidence. Cancellation is the v0 mechanism for revoking a pending publication decision; a general AccessPolicy/approval workflow is outside this capability.

### 9.2 Due execution

A durable worker selects pending rows with `scheduled_publish_at <= database_now`. It invokes the existing Publish operation using the stored `publish_operation_id`; duplicate workers are safe because the Publish ledger and Document lock admit one successful transaction. A lease may reduce duplicate work but is not relied on for correctness. The transaction itself checks the database clock so publication cannot occur early.

At due time, Application repeats file, DSI, and publish-quality validation. Repository rechecks the target is the same `WORKING` snapshot, base still equals current (including null/null for Version #1), the accepted Document revision still matches, and the schedule is pending and due. It enters the existing initial-Publish branch for Version #1 or the new replacement branch for Version #2+. On success, Publish and schedule completion commit atomically; the same Publish result is returned on replay. A cancelled, edited, rebased, withdrawn, or otherwise stale intent never publishes. Scheduled initial publication has the new DSI/quality gate; the existing manual initial-Publish contract is otherwise preserved.

A transient infrastructure failure keeps the same schedule/Publish ID pending for bounded backoff and visible retry, without a Document revision change. A permanent business, integrity, or quality failure terminally closes the schedule, clears `scheduled_publish_at`, increments Document revision once, records the reason and Audit/Outbox evidence, and leaves the Version `WORKING` and current unchanged. The scheduler does not invent a new operation ID. Past-due pending schedules are observable as delayed, without adding a Version lifecycle state or promising exact-time execution.

### 9.3 Cancel and edit

Cancelling a pending schedule is an idempotent, audited command with caller UUIDv7 identity and expected Document revision. It marks the intent cancelled, clears `scheduled_publish_at`, and increments Document revision in one transaction. `approved_at` may remain, so the existing derived UI state becomes non-public. Rescheduling uses a new Publish ID after cancellation. Ordinary `WORKING` update or rebase is rejected while a schedule is pending; callers cancel first. Manual Publish with a different ID is likewise rejected until cancellation.

## 10. Data migration and repository boundary

Use additive PostgreSQL migrations; do not rewrite `0001` or `0002`. Add a same-Document base-Version foreign key, the one-`WORKING` partial unique index, canonical ContentItem/representation tables and constraints, Versioning operation ledger, and schedule table/indexes. Keep the existing composite same-Document current-Version foreign key.

Existing Version #1 records with exactly one unambiguous PRIMARY FileObject and no ATTACHMENT rows are backfilled as one canonical ContentItem with `logical_path = "primary"` and `ordinal = 0`. New initial-Version creation uses the same canonical manifest key. Do not infer whether legacy ATTACHMENT rows are authoritative ContentItems or renditions. Ambiguous legacy Versions are marked for remediation and blocked from Versioning operations until classified. Update initial creation, initial Publish preflight, and authoritative reads to use the canonical ContentItem model in the same release; retained legacy `version_files` rows are historical compatibility data, not a second editable authority.

Repository ports remain capability-specific: load candidate/operation, create or update `WORKING`, rebase, Publish, withdraw, reserve/cancel schedule, and acquire due intents. Application does not receive a generic SQL transaction primitive. The existing read path hardcoded to Version #1 must become Version-aware. Database constraints and short transactions enforce ownership, uniqueness, and atomicity; Domain rules enforce lifecycle and semantic preconditions.

## 11. Failure, observability, and concurrency

- DSI failure is distinct from publish-quality failure and Versioning business Conflict.
- Stale Document revision, changed current/base, duplicate `WORKING`, invalid manifest, or semantic no-change do not mutate authoritative state.
- Storage object absence is an integrity failure; storage-service outage is a dependency failure. During withdrawal, either prevents automatic restoration but does not prevent withdrawing the unsafe current Version.
- Every successful lifecycle/current transition has one Domain Outbox and one mandatory Audit Outbox record in the same transaction. Replay creates neither duplicate.
- Commit errors with uncertain outcome return the operation ID and require same-ID lookup/retry; no blind new-ID retry.
- Scheduled attempts expose due time, last attempt, next retry, terminal reason, and published operation ID without turning attempt status into DocumentVersion lifecycle state.
- Transaction ordering is Document row, then Version/schedule rows, then operation/outbox writes. OCC plus the Document lock serializes create, publish, withdrawal, and schedule transitions.

## 12. Required acceptance evidence for the later plan

The implementation plan must cover focused RED/GREEN evidence and the minimum decisive verification for:

1. exactly one `WORKING`, transactional `version_no`, UUIDv7 replay and commit ambiguity;
2. title/path/order and per-item semantic equality versus meaningful change, with every authoritative item inspected;
3. format-native parity and no Search Extraction dependency or common IR;
4. Version #2+ Publish replacement, quality gates, and preservation of the old `PUBLISHED` Version;
5. current and historical withdrawal, predecessor restoration, no predecessor, and unsafe predecessor yielding null without a new flag;
6. stale `WORKING` rebase and scheduled-intent invalidation after withdrawal;
7. initial and later scheduled due, early attempt, duplicate workers, cancellation, edits, current/revision change, transient retry, and terminal failure;
8. migration of an unambiguous Version #1 and fail-closed ambiguous legacy attachments;
9. Domain/Audit Outbox atomicity and exact replay without duplicate events.

Run focused local checks while implementing; reserve exact-head hosted CI for coherent milestones and final approval evidence. A documentation-only design proposal does not need a production test run.

## 13. Frozen consequences and implementation gate

The approval freezes these explicit consequences:

- Withdrawal restores only the immediate recorded base if it is still safe; otherwise current becomes null. No older ancestor is silently exposed.
- Withdrawal of Version #1 leaves no current and cannot be reversed by Versioning v0.
- A schedule is a durable authorized intent; due execution uses the stored actor and checks the final content/current/revision again.
- Active schedules block ordinary edits and manual Publish until cancelled.
- The approved Publish v0 manual initial-Version behavior remains compatible; newly scheduled initial publication follows the new DSI/quality gate, and Version #2+ uses the replacement branch.

The approval record is `docs/superpowers/specs/2026-09-27-document-versioning-v0-design-approval.md`. The deferred T4/scheduled-publication rules in `spec/` must be updated, then an implementation plan written from this frozen design. Production Versioning code remains gated on separate plan approval.
