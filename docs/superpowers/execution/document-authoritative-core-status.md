# Document Authoritative Core — Execution Status

- Capability: `Document Authoritative Core — Create/Get v0`
- Execution mode: **Inline Execution**
- Overall phase: **IMPLEMENTATION**
- Design: **APPROVED + MERGED**
- Implementation Plan: **ACCEPTED FOR INLINE EXECUTION**
- Product/runtime implementation started: **NO — baseline verification first**

## Current repository flow

- Design PR: `#3` — **MERGED**
- Design merge commit / current implementation baseline: `fda70596931007abcc8ac4139db78848bae9836a`
- Implementation branch: `feat/document-authoritative-core-v0`
- Implementation branch was force-aligned to the exact merged `main` head before production implementation.

Always fetch the current branch/PR/CI state from GitHub before acting; do not assume an earlier chat state.

## Approved artifacts

- Design Spec: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md`
- Approval record: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-16-document-authoritative-core-implementation.md`

## Frozen decisions

- Domain / Application / Infrastructure dependency direction.
- Initial `DocumentVersion #1` is `WORKING`.
- Initial `Document.current_version_id = None`.
- Initial `Document.revision = 0`.
- File-first / DB-second ordering.
- Staging write/hash/count → file sync → same-filesystem atomic rename → directory durability step → PostgreSQL transaction.
- Finalized files are not eagerly deleted after an ambiguous DB commit result.
- Authoritative business state + Domain Outbox + mandatory Audit Outbox are inserted in the same PostgreSQL transaction.
- PostgreSQL 18.x + SQLx 0.9.x; no ORM.
- No Firefly runtime dependency in Capability 1.
- HTTP/OpenAPI transport, Search, Extraction, ReadState, AccessPolicy, Version #2+, Publish/Withdraw, and Outbox delivery are outside this capability.

## Implementation task tracker

| Task | Status | Evidence / Notes |
|---|---|---|
| 1. Workspace dependencies + architecture boundaries | `IN_PROGRESS` | Step 0 baseline verification pending on implementation PR CI |
| 2. Infrastructure-free Domain invariants | `NOT_STARTED` | — |
| 3. Application ports + Create/Get orchestration | `NOT_STARTED` | — |
| 4. Durable local filesystem adapter | `NOT_STARTED` | — |
| 5. PostgreSQL schema + atomic repository | `NOT_STARTED` | — |
| 6. Unknown-commit recovery + reconciliation | `NOT_STARTED` | — |
| 7. Real filesystem + PostgreSQL vertical slice | `NOT_STARTED` | — |
| 8. SQLx metadata + CI + final evidence | `NOT_STARTED` | — |

## Verification evidence

### Design/plan gate

- PR #3 head `eb794feb2e8186102e2400163d2393be7901ee21` passed CI run #28.
- All required jobs passed: policy, rust-static, rust-test, security, portability-macos, container-build, required-check.
- PR #3 squash-merged as `fda70596931007abcc8ac4139db78848bae9836a`.
- `feat/document-authoritative-core-v0` was then reset to that exact `main` SHA.

### Implementation baseline

Pending CI on implementation PR before Task 1 / Step 1 production-related changes.

## Current blocker / gate

No design blocker. Baseline verification must be green before writing Task 1 failing architecture tests.

## Next exact action

1. Open draft implementation PR from `feat/document-authoritative-core-v0` to `main`.
2. Confirm baseline CI green.
3. Implement **Task 1 / Step 1 RED**: add architecture-policy tests for forbidden Domain/Application dependencies/source patterns only.
4. Run CI and confirm the new policy test fails for the intended missing feature.
5. Only then implement Task 1 architecture boundary enforcement and workspace dependency baseline.

## Session handoff maintenance rule

Update this file whenever a Task starts/completes, a verification gate changes, a blocker appears/clears, a Design Freeze deviation is considered, or before session handoff.