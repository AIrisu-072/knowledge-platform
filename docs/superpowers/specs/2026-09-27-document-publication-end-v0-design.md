# Document Publication End v0 — Design

- Status: **PROPOSED — written-spec review required**
- Date: 2026-09-27
- Capability: `Document Publication End v0` (transaction T10)
- Scope class: Product Capability / Architectural
- Baseline: `feat/document-versioning-v0@96b068bc9484a219b435d0633ea6669ddb7d7f97` (PR #12, based on unmerged PR #11)

## 1. Purpose and approved direction

End publication of an entire Document without deleting its content or withdrawing an individual Version. This is the separate T10 operation required by `spec/data/transaction-consistency-requirements-v0.md`: Version withdrawal (T4) may restore its immediate predecessor, so T4 cannot reliably remove the whole Document from normal use and search.

The user approved the in-chat direction on 2026-09-27: clear the current pointer, retain historical Versions and files, record an idempotent publication-end operation, invalidate pending schedules, emit a Search exclusion event, and enforce current-only normal reads. Re-publication is a later, separately designed operation. This written spec is a proposal until separately reviewed and approved; no production implementation is authorized by the conversational approval alone.

## 2. Normative context and scope

`spec/data/logical-data-model-v0.md` and `spec/data/transaction-consistency-requirements-v0.md` are normative. The approved Document Versioning v0 Design and its implementation are the immediate baseline. Audit creation follows `spec/operations/observability-audit-requirements-v0.md`; Search indexes are eventually consistent under the transaction requirements. The deferred T10 details in `spec/` must be reconciled after approval and before production implementation. A conflict with a frozen Versioning rule requires an explicit amendment; this proposal does not change withdrawal restoration, Version identity, DSI, or scheduled-publication semantics.

Included: ending a currently published Document, its transaction and exact replay, schedule invalidation, normal-read visibility, and source-side Search exclusion evidence. Excluded: HTTP/UI transport, Search delivery/index implementation, AccessPolicy implementation, legal deletion, automatic reopening, and changing the content or lifecycle of historical Versions. Search Extraction and DSI remain separate; neither is needed to end publication.

## 3. State and meaning

The Document remains the same logical object. T10 changes `Document.current_version_id` from a current `PUBLISHED` Version to null and increments `Document.revision` once. The former current Version stays `PUBLISHED`; its `published_at`, content, base link, and files do not change. No `DocumentVersion` becomes `WITHDRAWN`, and no new Document flag, Version lifecycle state, or redundant `ended` column is added.

A null current pointer alone is ambiguous: it may also mean never published or withdrawn without a safe predecessor. Therefore a durable, append-only T10 operation record is the evidence that this Document's publication was deliberately ended. A Document-level “公開終了” label may be derived from that record and null current. Version-level labels still follow their own lifecycle/current-pointer rules; a former current `PUBLISHED` Version is historical. Normal readers see no current Version after T10. Authorized historical access is a separate AccessPolicy-bound path and is never a fallback for normal reads.

T10 v0 is terminal for ordinary publication. Existing `WORKING` and historical Versions remain stored, but create/update/rebase, reserve/due/manual Publish, and any other path that would establish a new current Version must reject a Document with a T10 record. A later reopening capability must define its own audited transaction and visibility rules. Historical T4 withdrawal remains permitted when authorized, provided it leaves current null; it cannot reopen the Document.

## 4. Command and replay

`EndDocumentPublication` carries a caller-generated UUIDv7 operation ID, Document ID, expected Document revision, expected current Version ID, actor `PrincipalRef`, and a nonblank reason. The expected current ID prevents ending a different Version after a concurrent switch. Command identity includes every caller-supplied field; the server stores a deterministic digest of that identity. The operation result contains the operation ID, Document ID, former current Version ID, resulting null current, resulting revision, and UTC `ended_at`.

A dedicated `document_publication_end_operations` ledger has the operation ID as primary key and a unique Document ID for v0's one terminal end. It stores the command digest, expected current/revision, actor, reason, former current, result, and timestamp with same-Document foreign keys. Exact same-ID replay returns the stored result before checking the now-null current or stale expected revision, without another revision or event. Same ID with a different command is Conflict; a different ID after a completed T10 is BusinessRule/AlreadyEnded without mutation. On uncertain commit outcome the caller retries or looks up the same operation ID; it must not invent a new one.

A new T10 request requires the expected current to exist, belong to the Document, and be `PUBLISHED`; null current is a business rejection, including never-published and T4-withdrawn Documents. This matches the approved scope of ending an active publication. Missing Document is NotFound. Stale revision/current is Conflict. AccessPolicy and transport authorization remain outside this capability, as in Versioning v0. No public T10 transport is introduced; a future transport must authorize the actor before invoking this internal Application command.

## 5. Atomic transaction and concurrency

Application resolves exact replay and validates command shape and the trusted actor reference, then calls a capability-specific repository operation. Repository locks the Document row first, checks the T10 ledger again, expected revision/current, and the current Version's same-Document `PUBLISHED` state. It then:

1. sets `current_version_id = null` and increments Document revision once;
2. terminally invalidates every `PENDING` publication schedule for that Document with stable reason `document_publication_ended`, clearing each target's `scheduled_publish_at` projection while retaining schedule history;
3. inserts the T10 operation result and one Domain Outbox plus one mandatory Audit Outbox event.

These writes commit together. The event types are `DocumentPublicationEnded` and `document.publication.ended`. Their payload identifies the Document, former current Version, resulting null current, resulting revision, operation ID, actor, reason, UTC end time, and invalidated schedule count; it contains no document body or storage credentials. The Domain event tells Search consumers to remove all normal-search entries for the Document. At-least-once delivery must be idempotent by event ID/document revision. Audit delivery may lag, but failure to create its outbox record aborts T10.

The Document row lock and revision check serialize T10 against T3 Publish, T4 withdrawal, schedule reservation/cancellation/due execution, and Version mutation. If another operation wins first, T10 fails Conflict and the caller rereads. If T10 wins, later due workers observe terminal schedules and cannot Publish. All paths that could create or publish a Version, including manual initial Publish when current is null, and all schedule-reservation paths must check the terminal T10 record inside their own locked transaction; preflight alone cannot enforce this. A storage or DSI outage does not prevent T10 because it reduces public exposure and does not restore any content.

## 6. Normal reads and Search consistency

The existing PostgreSQL `load_current` query uses `COALESCE(current_version_id, latest WORKING, latest Version)`. `DocumentService::get_document` and `open_primary_file` currently use that aggregate, so clearing the pointer alone would still expose an old file. T10 requires a separate current-publication query that joins exactly `documents.current_version_id` to a same-Document `PUBLISHED` Version, with no fallback. A null current returns no normal-read result. A nonnull pointer to a non-`PUBLISHED` or foreign Version is an integrity error, not a fallback.

The normal Document read and file-open API must use that current-only query. The existing fallback loader may remain only as a clearly named internal authoring/operation snapshot path; it cannot serve normal/public reads. Existing draft-read callers must move to an explicitly separate authoring path rather than silently inheriting public visibility. T10 does not add a public historical-content endpoint. Any future historical read requires AccessPolicy authorization and a specific Version ID.

Search exclusion is asynchronous. Before showing a Search hit or serving its file/content to a normal user or LLM, the Document-side validator checks that the result's Document and Version IDs still match a current `PUBLISHED` Version. A stale hit is suppressed after T10 even if index deletion is delayed. Source-side rebuilds enumerate only current `PUBLISHED` Versions, so a T10 Document cannot reappear from historical `PUBLISHED` rows. This design specifies the Document-side contract; Search consumer delivery belongs to its own capability.

## 7. Required acceptance evidence and implementation gate

The later implementation plan must provide focused RED/GREEN evidence for:

1. T10 on a current `PUBLISHED` Document yields null current and one revision; former current remains byte-for-byte historical `PUBLISHED` with original `published_at` and FileObjects.
2. No current, foreign/nonpublished current, stale expected current/revision, and two concurrent different T10 IDs cannot create a misleading end record.
3. Same-ID exact replay, changed-command ID conflict, and uncertain-commit recovery create one ledger row and one Domain/Audit event pair.
4. Pending initial/later schedules are terminal with cleared projections; duplicate due workers and manual initial/later Publish cannot restore current after T10.
5. Normal Document/file reads and stale Search-result validation hide ended content while a separate internal/historical snapshot remains available only through its defined boundary.
6. T4 historical withdrawal remains distinct; no Version is changed to `WITHDRAWN` merely to end the Document, and no storage/DSI preflight blocks T10.
7. The exclusion event and current-only source query contract for Search rebuild, Audit atomicity, and preservation of raw/historical records satisfy the T10 normative boundary; a Search consumer/rebuild implementation is outside this capability.

Use focused local checks during implementation and one coherent exact-head hosted CI gate; do not run all three hosted workflows per small task. The written spec requires user review, then a separately reviewed implementation plan before production code. PR #11 and PR #12 remain unmerged; T10 design and implementation do not authorize merging them.
