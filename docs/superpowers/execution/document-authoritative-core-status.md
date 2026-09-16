# Document Authoritative Core — Execution Status

- Capability: `Document Authoritative Core — Create/Get v0`
- Execution mode: **Inline Execution**
- Overall phase: **IMPLEMENTATION COMPLETE / PRE-MERGE EXACT-HEAD DOCS GATE**
- Design: **APPROVED + MERGED**
- Implementation Plan: **EXECUTED**
- Product/runtime implementation: **COMPLETE ON PR BRANCH**

## Current repository flow

- Design PR: `#3` — **MERGED**
- Design merge / implementation baseline: `fda70596931007abcc8ac4139db78848bae9836a`
- Implementation PR: `#4` — **OPEN / DRAFT pending exact-head docs CI**
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
| 4. Durable local filesystem adapter | `COMPLETE` | run #124 green; hierarchy durability regression PASS |
| 5. PostgreSQL schema + atomic repository | `COMPLETE` | PostgreSQL 18.6 contract/schema tests in current 50-test suite |
| 6. Unknown-commit recovery + reconciliation | `COMPLETE` | review regressions fixed and reverified through run #131 |
| 7. Real filesystem + PostgreSQL vertical slice | `COMPLETE` | current suite contains both real vertical slices |
| 8. SQLx reproducibility gate + final evidence | `COMPLETE` | sqlx-cli 0.9.0 pinned; `sqlx:check` required and green in run #131 |

## Final review corrections

Implementation review found five Important gaps/refinements after the original Task 1–8 execution. All are closed with explicit RED→GREEN evidence.

### 1. Reconciliation was initially Storage→DB only

Problem: `classify(true, None) = IntegrityViolation` existed, but `reconcile_storage()` iterated only storage objects, so an authoritative DB reference whose physical final object disappeared could not be discovered by a reconciliation scan.

Correction:

- `DocumentRepository::list_referenced_file_ids()` added.
- PostgreSQL implementation lists authoritative `version_files.file_id` values.
- reconciliation now performs authoritative-DB→Storage and remaining-Storage→DB comparison without reconstructing filesystem paths in Application code.
- no deletion/scheduler behavior was added.

Evidence:

- RED: run #113 failed because the scan returned `0` findings instead of required `1`.
- GREEN: run #116 passed all required jobs with **45/45 tests**.
- `reconciliation_detects_authoritative_reference_whose_final_object_is_missing` PASS.

### 2. Ambiguous create did not expose pre-generated IDs

Problem: `CommitOutcomeUnknown` retained the final file, but the Application error did not expose the pre-generated Document/Version/File IDs required for safe lookup by the caller.

Correction:

- `RepositoryError::CommitOutcomeUnknown` remains infrastructure-level and ID-free.
- `DocumentService::create_document()` maps it to structured `ApplicationError::CommitOutcomeUnknown { document_id, document_version_id, file_id }`.
- caller can feed the returned `document_id` to `lookup_create_outcome()`.
- Create is never silently retried and IDs are never regenerated.

Evidence:

- RED: run #117 failed with `E0559` for the three missing fields.
- GREEN: run #121 (`35074902777`) passed all required jobs with **46/46 tests, 0 skipped**.
- `ambiguous_commit_exposes_pre_generated_ids_for_safe_lookup` PASS.

### 3. Final rename durability covered only the leaf directory

Problem: after same-filesystem atomic rename, the filesystem adapter synchronized the final shard directory but did not explicitly synchronize ancestor `objects/` and configured storage-root directory entries that may have been created by `create_dir_all`.

Correction:

- after rename, Unix directory sync runs leaf-to-root over `objects/<prefix>`, `objects`, then configured storage root.
- file-first / DB-second ordering is unchanged.
- no Application/Domain contract or cleanup policy changed.

Evidence:

- RED head: `521243b3f137482d795405801f0a68c429457332`; run #123 (`35100779991`) failed after adding the hierarchy-durability regression.
- GREEN head: `6f43df12ecdc141c6c702b6b22a86e60b12f36df`; run #124 (`35101341747`) passed all required jobs with **47/47 tests**.
- `final_directory_durability_covers_prefix_objects_and_storage_root` PASS.

### 4. PostgreSQL dependency failures were collapsed into Internal

Problem: statement failures such as pool closure/timeout, connection I/O failure, and worker crash were mapped to `RepositoryError::Internal`, contradicting the resilience taxonomy that separates transient dependency unavailability from internal defects.

Correction:

- `PoolClosed`, `PoolTimedOut`, `Io(_)`, and `WorkerCrashed` map to `RepositoryError::Unavailable`.
- non-dependency statement errors remain `Internal("postgres operation failed")`.
- commit errors remain `CommitOutcomeUnknown`; ambiguous commit safety semantics are unchanged.

Evidence:

- behavioral RED head: `001f2c59166d81b13354c8ccdfebd8af6a140b90`; run #126 (`35102445434`) failed the dependency classification assertion after formatting was clean.
- GREEN head: `f2b7fba3e084565bdfe36570bf7c68d02d4f4116`; run #127 (`35103287090`) passed all required jobs with **49/49 tests**.
- `dependency_statement_errors_are_reported_as_unavailable` PASS.

### 5. Referenced staging bytes could be misclassified as cleanup-safe

Problem: when DB authority referenced a `FileId`, the final object was missing, and a staging object with the same `FileId` still existed, reconciliation emitted both `IntegrityViolation` and `StaleStaging`. That could expose the only surviving bytes for an authoritative reference as a cleanup candidate.

Correction:

- authoritative DB references are classified first against final storage.
- any storage object whose `FileId` is still present in the authoritative referenced-ID set is excluded from the unreferenced cleanup-classification loop.
- a referenced file with no final object therefore yields one `IntegrityViolation`; same-ID staging bytes are retained and are not labeled `StaleStaging`.
- no cleanup/scheduler action was added.

Evidence:

- behavioral RED head: `c4ab410d424e0cb1f620664c8be421d43b3210e4`; run #130 (`35104726797`) failed because findings length was `2` instead of required `1`.
- GREEN head: `63fca3e4f527c73d42877380a609dbce870ca207`; run #131 (`35105544172`) passed all required jobs.
- Rust suite: **50 tests run / 50 passed / 0 skipped**.
- `reconciliation_does_not_mark_referenced_staging_for_cleanup_when_final_is_missing` PASS.

## Latest verified runtime implementation evidence

Exact runtime implementation head before this docs-only SSOT consistency commit:

`63fca3e4f527c73d42877380a609dbce870ca207`

CI run #131 (`35105544172`) — **PASS**:

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

Rust tests: **50 run / 50 passed / 0 skipped** across 19 binaries.

The current suite includes:

- Domain invariant tests.
- Application create/get/unknown-commit contracts.
- bidirectional reconciliation including missing authoritative physical-file detection.
- referenced-staging cleanup-safety regression.
- durable filesystem/hash/failure-injection/enumeration tests.
- final directory hierarchy durability regression.
- PostgreSQL dependency-error classification tests.
- PostgreSQL 18.6 migration constraints and atomic rollback/round-trip tests.
- real filesystem + real PostgreSQL Create → Get → open vertical slice.
- physical-file-loss → `IntegrityViolation` while authoritative DB state remains intact.

## Design SSOT consistency correction

The approved Design Spec still carried a stale top-level `DRAFT` status even though:

- the explicit approval record declares the design approved;
- Design PR #3 is merged;
- the Implementation Plan has been executed;
- implementation and final review evidence are complete.

This docs-only commit changes that stale metadata to `APPROVED — design freeze active`. It does **not** change any frozen design contract, dependency decision, capability scope, or acceptance criterion.

## SQLx reproducibility note

`cargo:sqlx-cli = 0.9.0` is pinned and `mise run sqlx:check` is a required `rust-static` CI gate using disposable `postgres:18.6-bookworm` plus repository migrations.

The repository intentionally uses runtime `sqlx::query/query_scalar/query_as` rather than `query!`/`query_as!` macros. Therefore SQLx does not generate `.sqlx` offline macro-cache files for the current query set. No metadata is fabricated. SQL correctness is exercised against real PostgreSQL 18.6 integration/vertical tests, while `sqlx:check` verifies the pinned tool, migration, workspace, and prepare/check workflow remain reproducible.

## Scope / dependency audit

- No HTTP transport was added to this capability.
- No Search/Extraction implementation was added.
- No Firefly runtime dependency was added.
- Infrastructure crates appear in `document-application` only as test-only dev-dependencies for the real vertical slice.
- License/source policy was not relaxed.
- `.github/workflows/` remains on the normal repository CI flow; no temporary write-enabled helper workflow is required for final evidence.

## Current blocker / gate

No implementation blocker or unresolved Critical/Important review finding is known.

This SSOT consistency update is documentation-only and creates a new PR head. The full required CI suite must be green for that exact new head before PR #4 is marked Ready and merge readiness is asserted.

After exact-head CI is green, the only remaining gate is the user's explicit merge decision.

## Next exact action

1. Verify all required CI jobs for the exact SSOT-consistency commit head.
2. If green, update PR #4 body with the final exact head/evidence without changing the branch tree.
3. Mark PR #4 Ready for review.
4. **Do not merge PR #4 without an explicit user instruction.**

## Session handoff maintenance rule

Update this file whenever the verification gate changes, a blocker appears/clears, or before session handoff. Repository state and fresh GitHub CI evidence remain authoritative.