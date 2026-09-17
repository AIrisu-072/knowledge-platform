# Document Publish v0 — Execution Status

- Capability: `Document Publish v0`
- Execution mode: **Inline Execution**
- Overall phase: **DESIGN REVIEW**
- Design: **CONVERSATIONALLY APPROVED / WRITTEN SPEC REVIEW PENDING**
- Implementation Plan: **NOT CREATED**
- Product/runtime implementation: **NOT STARTED**

## Current repository flow

- Previous capability implementation PR: `#4` — **MERGED**
- Previous capability merge commit / Publish design baseline: `2ead1e21222c3b122704b6e0ce31f3f902b93659`
- Design branch: `design/document-publish-v0`
- Design Spec: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design PR: **to be created from this branch**

Always fetch the current branch / PR / CI state from GitHub before acting. Repository state overrides remembered/chat state.

## Approved conversational decisions represented in the written Design

- Scope is initial Publish only; no Version #2+, Withdraw, approval workflow, scheduled publish, HTTP/UI/Search implementation, or current-version replacement.
- `PublishDocumentCommand` identifies both `DocumentId` and target `DocumentVersionId`.
- OCC uses `expected_document_revision`.
- `PublishOperationId` is caller-generated UUIDv7 in the Application layer.
- Idempotency uses a dedicated permanent `document_publish_operations` table.
- Same operation ID + same command replays the stored result; same ID + different command is Conflict.
- Distinct operation IDs are never treated as replay merely because the same Version is already published.
- Existing current Version replacement is prohibited in v0.
- Publish does not require `approved_at`.
- PRIMARY final-object availability is checked before a new Publish transaction; object absence is IntegrityViolation while dependency-level storage outage remains StorageUnavailable.
- PostgreSQL transaction combines short row lock + revision OCC.
- `current_version_id` same-Document ownership is enforced with a composite FK.
- `PUBLISHED` current-state semantics remain Domain + atomic Repository responsibility; no lifecycle trigger is added.
- Domain Outbox, mandatory Audit Outbox, authoritative state, and successful publish-operation result commit atomically.
- Commit ambiguity is recovered only by retrying the exact same command with the same operation ID.
- Publish operation records have no TTL in v0.

## Written Design self-review

Completed on branch head after the initial Design write.

Checks performed:

- placeholder scan: no `TODO` / `TBD` placeholders found;
- scope check: capability remains initial Publish only;
- consistency check: retry, concurrency, operation-ID uniqueness, and current-version rules are mutually consistent;
- repository-fit refinement: storage object absence vs dependency outage is explicitly separated;
- repository-fit refinement: UUIDv7 validation is explicit;
- audit compatibility: internal Audit Outbox representation may remain adapter-specific while future CloudEvents delivery must produce the required publication envelope.

## Current gate

The conversational design was approved before the file was written, but the written Design Spec has not yet received user review approval.

Per the design workflow, do **not**:

- mark the Design `APPROVED — design freeze active`;
- create the Implementation Plan;
- start production implementation;

until the user reviews the written Design Spec.

## Next exact action

1. Open a Draft Design PR for `design/document-publish-v0` against `main`.
2. Verify the exact PR head and any triggered CI for the documentation-only branch.
3. Ask the user to review `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`.
4. If the user approves the written artifact, update the Design status to `APPROVED — design freeze active`, add an approval record, then invoke the writing-plans workflow to create the implementation plan.

## Change-control rule

No production implementation begins while the written-spec review gate is open. Any material change to the approved conversational decisions must be reflected in the written Design and re-reviewed before planning.
