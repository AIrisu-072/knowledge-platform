# Document Authoritative Core — Execution Status

- Capability: `Document Authoritative Core — Create/Get v0`
- Execution mode: **Inline Execution**
- Overall phase: **IMPLEMENTATION**
- Design: **APPROVED + MERGED**
- Implementation Plan: **ACCEPTED FOR INLINE EXECUTION**
- Product/runtime implementation started: **YES**

## Current repository flow

- Design PR: `#3` — **MERGED**
- Design merge commit / implementation baseline: `fda70596931007abcc8ac4139db78848bae9836a`
- Implementation PR: `#4` — **OPEN / DRAFT**
- Implementation branch: `feat/document-authoritative-core-v0`

Always fetch current branch/PR/CI state from GitHub before acting; do not assume an earlier chat state.

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
| 1. Workspace dependencies + architecture boundaries | `COMPLETE` | baseline run #30 green; RED run #31 failed as intended; generic boundary enforcement GREEN by run #33; dependency/Docker/policy follow-ups complete; final run #42 all required jobs green |
| 2. Infrastructure-free Domain invariants | `COMPLETE` | aggregate RED `d808b3d...` observed in run #48; implementation `1388cdab...` + format `bca681ae...`; run #50 all required jobs green |
| 3. Application ports + Create/Get orchestration | `COMPLETE` | RED tests `51b66bf...`, formatted RED `9abddf8...`; run #53 reached intended `E0432` unresolved Application API; implementation `56575af...`, format `e573db25...`, test-only Debug-bound fix `955678c...`; run #56 all required jobs green; rust-test log: 32/32 PASS including all four Application contract tests |
| 4. Durable local filesystem adapter | `IN_PROGRESS` | Step 1-3 next: add happy-path + failure-point RED tests only, then observe failure before implementation |
| 5. PostgreSQL schema + atomic repository | `NOT_STARTED` | — |
| 6. Unknown-commit recovery + reconciliation | `NOT_STARTED` | — |
| 7. Real filesystem + PostgreSQL vertical slice | `NOT_STARTED` | — |
| 8. SQLx metadata + CI + final evidence | `NOT_STARTED` | — |

## Verification evidence

### Design/plan gate

- PR #3 head `eb794feb2e8186102e2400163d2393be7901ee21` passed CI run #28.
- All required jobs passed: policy, rust-static, rust-test, security, portability-macos, container-build, required-check.
- PR #3 squash-merged as `fda70596931007abcc8ac4139db78848bae9836a`.
- `feat/document-authoritative-core-v0` was reset to that exact `main` SHA before implementation.

### Task 1 evidence

- Baseline `9a5ec380...` run #30 green.
- RED `1c860d82...` run #31 failed on the four new boundary assertions as intended.
- Generic enforcement `03e99028...` / format `756ab244...`; run #33 GREEN.
- Workspace scaffold `2fd311c9...`; Cargo.lock `dc392c9b...`; helper workflow removed `1006fe55...`.
- Docker workspace fix `16936b35...`; Zlib/internal path dependency policy fix `13b6d01d...`.
- Final run #42 (`35054390348`) all required jobs green.

### Task 2 evidence

- Domain value objects / typed IDs implemented without infrastructure dependencies.
- Aggregate RED `d808b3d40f8046cf5cfd219dc29bbb81bc133e9d`; run #48 observed expected rust-test/rust-static failures.
- Minimal implementation `1388cdab3c39f9d6b742ad1a1c4a93e4ee847b86`; format `bca681aebd5b32067a4c56c95d7535521a3ba9af`.
- Run #50 (`35055948809`) all required jobs green.

### Task 3 evidence

- Contract RED `51b66bf43104fe32503a3a094719f6708a2b3f8c`; first run #52 exposed formatting only.
- Test-only format `9abddf8d9daac31f98f82d757aea89964a14d629`; run #53 (`35057210781`) passed fmt and then failed with `E0432 unresolved imports` for the missing approved Application API — intended RED.
- Minimal Application implementation `56575af2120c5ee86dfe2440fed15034d56c4947`; format-only `e573db25b66e742ea74995491411a6acc0717da1`.
- Test-only `ContentReader: Debug` bound correction `955678c0c262327ab0e57e5dba39ca34a94d0f40`.
- CI run #56 (`35057830945`) passed policy, rust-static, rust-test, security, portability-macos, container-build, and required-check.
- rust-test evidence: 32 tests run, 32 passed; Application contract tests all PASS: storage-before-repository ordering, ambiguous-commit file retention/error mapping, authoritative not-found mapping, missing referenced binary → integrity violation.

## Current blocker / gate

None.

## Next exact action

1. Task 4 / Steps 1-2: add filesystem happy-path and injected failure-point tests only.
2. Observe RED before creating `FileSystemStorage` implementation.
3. Implement staging write/hash/count, file sync, same-filesystem rename, destination-directory durability, readback, and enumeration minimally to satisfy tests.
4. Verify adapter tests + clippy + architecture + full PR CI.

## Session handoff maintenance rule

Update this file whenever a Task starts/completes, a verification gate changes, a blocker appears/clears, a Design Freeze deviation is considered, or before session handoff.
