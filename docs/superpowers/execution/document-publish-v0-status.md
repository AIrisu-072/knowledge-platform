# Document Publish v0 — Execution Status

- Capability: `Document Publish v0`
- Execution mode: **Inline Execution**
- Overall phase: **DESIGN APPROVED / IMPLEMENTATION PLAN READY / DESIGN PR MERGE GATE**
- Design: **APPROVED — design freeze active**
- Implementation Plan: **CREATED + SELF-REVIEWED**
- Product/runtime implementation: **NOT STARTED**

## Current repository flow

- Previous capability implementation PR: `#4` — **MERGED**
- Publish design baseline: `main@2ead1e21222c3b122704b6e0ce31f3f902b93659`
- Design branch: `design/document-publish-v0`
- Design PR: `#5`
- Approved Design Spec: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-17-document-publish-v0-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-17-document-publish-v0-implementation.md`

Always fetch current PR #5 head, state, and exact-head CI from GitHub before acting. Repository/GitHub state overrides chat memory.

## Frozen design decisions

- Initial Publish only; no Version #2+, Withdraw, approval workflow, scheduled publication, HTTP/UI/Search implementation, outbox delivery, or current-version replacement.
- Command identifies both `DocumentId` and target `DocumentVersionId`.
- OCC uses caller-supplied `expected_document_revision` plus a short PostgreSQL row lock.
- `PublishOperationId` is caller-generated UUIDv7 in the Application layer.
- Idempotency uses permanent `document_publish_operations` records with no TTL/cleanup in v0.
- Same operation ID + same command replays the stored result; same ID + different command is Conflict.
- Distinct operation IDs are not replay-equivalent merely because the same Version is already published.
- Publish does not require `approved_at`.
- New Publish preflights the PRIMARY final object; object absence/object-level unreadability is IntegrityViolation, dependency-level storage outage remains StorageUnavailable.
- `current_version_id` same-Document ownership is enforced by a composite FK.
- `PUBLISHED` current semantics are enforced by Domain + atomic Repository transaction; no lifecycle trigger/helper column.
- Authoritative state, one Domain Outbox event, one mandatory Audit Outbox event, and the successful operation result commit atomically.
- Commit ambiguity recovers only by exact-command retry with the same operation ID.

## Written Design approval

The user explicitly approved the written Design Spec on 2026-09-17.

The Design Spec is marked `APPROVED — design freeze active`, and the approval record is committed on the Design branch.

Any change to lifecycle scope, current-version replacement semantics, idempotency, OCC/locking, file preflight semantics, transaction boundary, event/audit semantics, database ownership constraints, or capability scope requires an explicit Design amendment before implementation.

## Implementation Plan self-review

The Implementation Plan was derived from the frozen Design and self-reviewed before execution.

Corrections made during self-review:

- `DocumentPublishRepository` remains separate from existing `DocumentRepository` so Create/Get/reconciliation fakes are not widened.
- `DocumentService::new` is planned to move out of the `R: DocumentRepository`-bound impl so Publish-only repository fakes can instantiate the service.
- Publish-after-Create requires `GetDocument` to understand the narrow initial-PUBLISHED state; the Plan explicitly adds `InitialDocument::restore_published` and adapter mapping.
- PostgreSQL Publish read helpers are implemented before, but the full `DocumentPublishRepository` trait impl is deferred until all three methods exist; no incomplete production trait impl/stub is allowed.
- rollback atomicity uses a deterministic duplicate Domain-Outbox EventId collision; no production fault flag is introduced.
- distinct-operation and same-operation concurrency tests use an explicit `tokio::sync::Barrier`.
- unknown-commit tests use test-only repository wrappers for before-commit and after-commit realities.
- placeholder scan found no `TODO` or `TBD`; implementation choices required for execution are made explicitly in the Plan.

## Planned implementation tasks

1. Domain initial-Publish transition + initial-PUBLISHED restoration.
2. Application contracts + segregated `DocumentPublishRepository`.
3. Storage error distinction + Application replay/preflight orchestration.
4. `0002_document_publish_v0.sql` + schema constraints.
5. PostgreSQL Publish read helpers + published Get mapping.
6. Atomic PostgreSQL Publish transaction + complete adapter trait.
7. Real concurrency + exact-command unknown-commit recovery + Create→Publish→Get vertical slice.
8. Full repository gates + self-review + exact-head implementation PR evidence.

## Current gate

No product/runtime implementation starts from the Design branch.

Before implementation:

1. PR #5 must be green on its exact documentation/plan head.
2. PR #5 must be merged into `main` by an explicit user merge decision.
3. `feat/document-publish-v0` must be created from the exact merged `main` head.
4. Inline Execution then follows the approved Implementation Plan task-by-task with TDD.

## Next exact action

1. Fetch PR #5 current head/state and exact-head CI after the final approval/plan/status commits.
2. Update PR #5 body to reflect written approval and the finalized Implementation Plan.
3. If exact-head CI is green and no blocking review exists, mark PR #5 Ready for review.
4. **Do not merge PR #5 without an explicit user merge instruction.**

## Session handoff rule

Before session switch/context exhaustion, record current Task/Step, exact verification evidence, branch/PR/head, blockers, next exact action, and any approved Design amendment here.

Do not claim implementation completion without fresh exact-head evidence.
