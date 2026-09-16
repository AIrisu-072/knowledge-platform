# Document Authoritative Core — Execution Status

- Capability: `Document Authoritative Core — Create/Get v0`
- Execution mode: **Inline Execution**
- Overall phase: **PRE-IMPLEMENTATION GATE**
- Design: **APPROVED**
- Implementation Plan: **ACCEPTED FOR INLINE EXECUTION**
- Product/runtime implementation started: **NO**

## Current repository flow

- Design branch: `design/document-authoritative-core-v0`
- Design PR: `#3` — `docs: approve authoritative core design and implementation plan`
- Reserved implementation branch: `feat/document-authoritative-core-v0`
- Important: the implementation branch was created before PR #3 was merged. Do **not** implement on it yet. After PR #3 is merged, reset/move that branch to the merged `main` head before Task 1 so the squash-merged design commits are not duplicated in the implementation PR.

Always fetch the current branch/PR/CI state from GitHub before acting; do not assume the SHA recorded in an earlier chat is still current.

## Approved artifacts

- Design Spec: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md`
- Approval record: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-16-document-authoritative-core-implementation.md`

## Frozen decisions

The following are Design Freeze items and must not change during implementation without evidence + change proposal + explicit approval + spec update:

- Domain / Application / Infrastructure dependency direction.
- Initial `DocumentVersion #1` is `WORKING`.
- Initial `Document.current_version_id = None`.
- Initial `Document.revision = 0`.
- File-first / DB-second ordering.
- Staging write/hash/count → file sync → same-filesystem atomic rename → directory durability step → PostgreSQL transaction.
- Finalized files are not eagerly deleted after an ambiguous DB commit result.
- Authoritative business state + Domain Outbox + mandatory Audit Outbox are inserted in the same PostgreSQL transaction.
- PostgreSQL 18.x + SQLx 0.9.x; no ORM.
- No Firefly runtime dependency in Capability 1; Firefly remains reference/source-level reuse material for later Outbox Delivery work.
- HTTP/OpenAPI transport, Search, Extraction, ReadState, AccessPolicy, Version #2+, Publish/Withdraw, and Outbox delivery are outside this capability.

## Implementation task tracker

| Task | Status | Evidence / Notes |
|---|---|---|
| 1. Workspace dependencies + architecture boundaries | `NOT_STARTED` | — |
| 2. Infrastructure-free Domain invariants | `NOT_STARTED` | — |
| 3. Application ports + Create/Get orchestration | `NOT_STARTED` | — |
| 4. Durable local filesystem adapter | `NOT_STARTED` | — |
| 5. PostgreSQL schema + atomic repository | `NOT_STARTED` | — |
| 6. Unknown-commit recovery + reconciliation | `NOT_STARTED` | — |
| 7. Real filesystem + PostgreSQL vertical slice | `NOT_STARTED` | — |
| 8. SQLx metadata + CI + final evidence | `NOT_STARTED` | — |

## Verification evidence

### Design/plan stage

- Repository Bootstrap was previously merged and main CI was green before this capability design began.
- Design Spec Sections 1–5 were explicitly approved by the user.
- OSS Fit-Gap was performed for Mayan EDMS and Firefly OpenCore/Framework.
- Firefly crate/module selective-reuse analysis was completed; direct Firefly production dependencies were removed from Capability 1.
- PR #3 was opened as documentation-only before implementation.

### Implementation baseline

Not yet established. Before Task 1:

1. PR #3 CI must be green.
2. PR #3 must be merged to `main`.
3. Fetch the new `main` head.
4. Move/reset `feat/document-authoritative-core-v0` to that exact merged `main` head.
5. Verify the repository baseline using the existing project gates before making Task 1 production changes.

## Current blocker / gate

No design blocker. Implementation is intentionally gated on PR #3 merge and clean implementation-branch baseline.

## Next exact action

1. Fetch PR #3 and its latest CI run.
2. If CI is green, merge PR #3 (prefer the repository's established squash-merge pattern).
3. Fetch the new `main` SHA.
4. Move `feat/document-authoritative-core-v0` to the new `main` SHA before any implementation commit.
5. Run/confirm baseline verification.
6. Mark **Task 1 / Step 1** `IN_PROGRESS` here and begin the TDD steps from the Implementation Plan.

## Session handoff maintenance rule

Update this file whenever:

- a Task starts or completes;
- a verification gate materially changes state;
- a blocker appears or is cleared;
- a Design Freeze deviation is proposed/approved/rejected;
- the session is about to end or context is becoming constrained.

For each update, preserve enough detail that a new agent can identify the next action without reading chat history.
