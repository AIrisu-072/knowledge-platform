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
| 3. Application ports + Create/Get orchestration | `COMPLETE` | RED tests `51b66bf...`, formatted RED `9abddf8...`; run #53 reached intended `E0432`; implementation `56575af...` + follow-ups; run #56 all required jobs green; 32/32 tests PASS |
| 4. Durable local filesystem adapter | `COMPLETE` | RED tests `a4f5ea31...`; test format `be285371...`; Cargo-generated lock refresh `6e900510...`, helper removed `ec724b9b...`; clean RED run #62 failed on missing `FileSystemStorage` / `ops::FsFailurePoint`; implementation `6ccf4f5b...`, format `4bf05484...`, Clippy-only fix `5a2af7c3...`; run #65 all required jobs green; 36/36 tests PASS including all four storage tests |
| 5. PostgreSQL schema + atomic repository | `COMPLETE` | migration/constraint RED established and then GREEN on PostgreSQL 18.6 by run #76; atomic repository clean RED run #83 failed only on missing `PostgresDocumentRepository` / `map_commit_error`; implementation `48eedda5...` + rustfmt `29c5c328...`; clean head `cd8523b2...`; run #87 all required jobs green; 39/39 tests PASS including real-Postgres schema constraints and atomic rollback/round-trip contract |
| 6. Unknown-commit recovery + reconciliation | `IN_PROGRESS` | Task 6 RED classification + ambiguous-commit recovery tests next |
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
- Workspace scaffold `2fd311c9...`; Cargo.lock generated on hosted runner; temporary helper removed.
- Docker workspace and dependency-policy follow-ups completed.
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
- CI run #56 (`35057830945`) passed all required jobs.
- rust-test evidence: 32 tests run, 32 passed; all four Application contract tests PASS.

### Task 4 evidence

- Storage RED tests committed as `a4f5ea314547f3cf5188be47ac6fb98ffbc04011`.
- Initial run #58 exposed test formatting and stale lockfile before the intended RED; production code was not added.
- Test-only format fix `be28537149a46aa799e54c9cae9781af1698506e`.
- Cargo generated the updated lockfile on a GitHub-hosted runner in `6e900510958cfd86752f0a245df22ecbb5fee329`; temporary write-enabled helper workflow was removed immediately in `ec724b9bd5488576f21d14fb39f9b1610214eb20`.
- Clean RED CI run #62 (`35058760437`) passed fmt and architecture policy, then failed with `E0432` because `FileSystemStorage` and `ops::FsFailurePoint` were not implemented — intended RED.
- Minimal filesystem implementation `6ccf4f5b94b5613b0d779a7258a3f01de76489a6`; format-only follow-up `4bf05484a8eb0ebd8385914bf236fbdcd0906532`; one-line Clippy `op-ref` correction `5a2af7c3cd9b00b08676d106cf74466c73c87a5a`.
- CI run #65 (`35060162912`) passed policy, rust-static, rust-test, security, portability-macos, container-build, and required-check.
- rust-test evidence: 36 tests run, 36 passed, including all four `document-storage-fs` tests: immutable store/hash/readback, FileId-only identity, precise failure injection, and staging/final/unknown enumeration without deletion.

### Task 5 evidence

- Real-PostgreSQL schema/constraint RED was established before migration implementation; migration then passed PostgreSQL 18.6 constraint coverage in run #76.
- Atomic repository RED was refined until run #83 failed only because `PostgresDocumentRepository` and `map_commit_error` were absent.
- Minimal atomic repository implementation: `48eedda58aa7ec13285b90876393666833e67cde`; rustfmt-only follow-up `29c5c328...`; temporary helper removed with clean branch head `cd8523b2331fb78d77a58d02dcb11f2567dc1702`.
- CI run #87 (`35063300538`) passed policy, rust-static, rust-test, security, portability-macos, container-build, and required-check.
- rust-test evidence: 39 tests run, 39 passed, including `schema_constraints::migration_seeds_root_and_enforces_authoritative_constraints` and `repository_contract::repository_persists_reads_and_rolls_back_authoritative_state_atomically` against PostgreSQL 18.6.

## Current blocker / gate

None.

## Next exact action

1. Task 6 / Step 1: add RED reconciliation classification tests for healthy/stale/orphan/integrity/grace-not-elapsed behavior.
2. Verify the RED fails for missing reconciliation API, not formatting or unrelated test defects.
3. Implement only the pure classification required to make that subcycle GREEN.
4. Then add the separate `CommitOutcomeUnknown` recovery/query RED tests before service changes.

## Session handoff maintenance rule

Update this file whenever a Task starts/completes, a verification gate changes, a blocker appears/clears, a Design Freeze deviation is considered, or before session handoff.
