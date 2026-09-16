# Document Authoritative Core — Execution Status

- Capability: `Document Authoritative Core — Create/Get v0`
- Execution mode: **Inline Execution**
- Overall phase: **IMPLEMENTATION COMPLETE / FINAL PR GATE**
- Design: **APPROVED + MERGED**
- Implementation Plan: **EXECUTED**
- Product/runtime implementation: **COMPLETE ON PR BRANCH**

## Current repository flow

- Design PR: `#3` — **MERGED**
- Design merge / implementation baseline: `fda70596931007abcc8ac4139db78848bae9836a`
- Implementation PR: `#4` — **OPEN**
- Implementation branch: `feat/document-authoritative-core-v0`

Always fetch current branch/PR/CI state from GitHub before acting. Do not reconstruct execution state from conversation history.

## Approved artifacts

- Design Spec: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md`
- Approval record: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-16-document-authoritative-core-implementation.md`

## Frozen decisions preserved

- Domain / Application / Infrastructure dependency direction.
- Initial `DocumentVersion #1 = WORKING`, `current_version_id = None`, `revision = 0`.
- File-first / DB-second ordering: stage → write/hash/count → file sync → same-filesystem atomic rename → destination-directory durability → PostgreSQL transaction.
- Finalized files are never eagerly deleted after ambiguous DB commit outcome.
- Authoritative business state + Domain Outbox + mandatory Audit Outbox commit in one PostgreSQL transaction.
- PostgreSQL 18.x + SQLx 0.9.x; no ORM and no Firefly runtime dependency.
- HTTP/OpenAPI, Search, Extraction, ReadState, AccessPolicy, Version #2+, Publish/Withdraw, and Outbox delivery remain out of scope.

## Task tracker

| Task | Status | Primary evidence |
|---|---|---|
| 1. Workspace dependencies + architecture boundaries | `COMPLETE` | final Task 1 CI run #42 green |
| 2. Infrastructure-free Domain invariants | `COMPLETE` | run #50 green |
| 3. Application ports + Create/Get orchestration | `COMPLETE` | run #56 green; 32/32 tests |
| 4. Durable local filesystem adapter | `COMPLETE` | run #65 green; 36/36 tests |
| 5. PostgreSQL schema + atomic repository | `COMPLETE` | run #87 green; 39/39 tests on PostgreSQL 18.6 |
| 6. Unknown-commit recovery + reconciliation | `COMPLETE` | run #96 green; later review regressions fixed and reverified |
| 7. Real filesystem + PostgreSQL vertical slice | `COMPLETE` | runs #103/#105 green; 44/44 tests by #105 |
| 8. SQLx reproducibility gate + final evidence | `COMPLETE` | sqlx-cli 0.9.0 pinned; `sqlx:check` in `rust-static`; run #121 green |

## Final review corrections

Implementation review found two Important gaps after the original Task 1–8 execution. Both were fixed with explicit RED→GREEN evidence.

### 1. Reconciliation was initially Storage→DB only

Problem: `classify(true, None) = IntegrityViolation` existed, but `reconcile_storage()` iterated only storage objects, so an authoritative DB reference whose physical final object disappeared could not be discovered by a reconciliation scan.

Correction:

- `DocumentRepository::list_referenced_file_ids()` added.
- PostgreSQL implementation lists authoritative `version_files.file_id` values.
- reconciliation now performs both authoritative-DB→Storage and remaining-Storage→DB comparison without reconstructing filesystem paths in Application code.
- no deletion/scheduler behavior was added.

Evidence:

- RED: run #113 failed exactly because the scan returned `0` findings instead of required `1`.
- GREEN: run #116 passed all required jobs with **45/45 tests**.
- regression: `reconciliation_detects_authoritative_reference_whose_final_object_is_missing` PASS.

### 2. Ambiguous create did not expose pre-generated IDs

Problem: `CommitOutcomeUnknown` retained the final file, but the Application error did not expose the pre-generated Document/Version/File IDs required for safe lookup by the caller.

Correction:

- `RepositoryError::CommitOutcomeUnknown` remains infrastructure-level and ID-free.
- `DocumentService::create_document()` maps that error to:

```text
ApplicationError::CommitOutcomeUnknown {
  document_id,
  document_version_id,
  file_id
}
```

- caller can feed the returned `document_id` to `lookup_create_outcome()`.
- Create is never silently retried and IDs are never regenerated.

Evidence:

- RED: run #117 failed with `E0559` for the three missing fields.
- GREEN exact implementation head before this status-only commit: `ae84e008b1af7fba5ab798053b640ee40c3c3807`.
- run #121 (`35074902777`) passed all required jobs with **46/46 tests, 0 skipped**.
- `ambiguous_commit_exposes_pre_generated_ids_for_safe_lookup` PASS.
- persisted and non-persisted ambiguous-commit recovery tests remained PASS.

## Final verified capability evidence before this status-only commit

Exact implementation head: `ae84e008b1af7fba5ab798053b640ee40c3c3807`

CI run #121 (`35074902777`):

- `policy` — PASS
- `rust-static` — PASS
  - `fmt` — PASS
  - `check:rust` — PASS
  - `sqlx:check` — PASS
- `rust-test` — PASS
- `security` — PASS
- `portability-macos` — PASS
- `container-build` — PASS
- `required-check` — PASS

Rust tests: **46 run / 46 passed / 0 skipped**.

The suite includes:

- Domain invariant tests.
- Application create/get/unknown-commit contracts.
- bidirectional reconciliation including missing authoritative physical file detection.
- durable filesystem/hash/failure-injection/enumeration tests.
- PostgreSQL 18.6 migration constraints and atomic rollback/round-trip tests.
- real filesystem + real PostgreSQL Create → Get → open vertical slice.
- physical-file-loss → `IntegrityViolation` while authoritative DB state remains intact.

## SQLx reproducibility note

`cargo:sqlx-cli = 0.9.0` is pinned and `mise run sqlx:check` is a required `rust-static` CI gate using disposable `postgres:18.6-bookworm` plus repository migrations.

The repository intentionally uses runtime `sqlx::query/query_scalar/query_as` rather than `query!`/`query_as!` macros. Therefore SQLx does not generate `.sqlx` offline macro-cache files for the current query set. No metadata is fabricated. SQL correctness is exercised against real PostgreSQL 18.6 integration/vertical tests, while `sqlx:check` verifies the pinned tool, migration, workspace, and prepare/check workflow remain reproducible.

## Scope / dependency audit

- No HTTP transport was added to this capability.
- No Search/Extraction implementation was added.
- No Firefly runtime dependency was added.
- Infrastructure crates appear in `document-application` only as test-only dev-dependencies for the real vertical slice.
- License/source policy was not relaxed.
- Temporary write-enabled helper workflows were removed; `.github/workflows/` contains only the normal `ci.yml` workflow at the verified implementation head.

## Current blocker / gate

No implementation blocker is known.

This status update is a docs-only commit and therefore creates a new PR head. Before marking PR #4 Ready for review, fetch and verify the full required CI suite for that exact new head.

## Next exact action

1. Verify all required CI jobs for the exact status-update head.
2. Update PR #4 body to the actual implemented scope and evidence.
3. Mark PR #4 Ready for review if the exact-head CI is green.
4. **Do not merge PR #4 without an explicit user instruction.**

## Session handoff maintenance rule

Update this file whenever the verification gate changes, a blocker appears/clears, or before session handoff. Repository state and fresh GitHub CI evidence remain authoritative.