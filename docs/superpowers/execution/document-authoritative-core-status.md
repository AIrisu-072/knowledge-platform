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
| 1. Workspace dependencies + architecture boundaries | `COMPLETE` | baseline run #30 green; RED run #31 failed on the four new boundary assertions as intended; generic boundary enforcement GREEN by run #33; generated Cargo.lock fixed; Docker workspace copy fixed; Zlib/internal path dependency policy fixed; final run #42 all required jobs green |
| 2. Infrastructure-free Domain invariants | `IN_PROGRESS` | Step 1 value-object RED tests next |
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
- `feat/document-authoritative-core-v0` was reset to that exact `main` SHA before implementation.

### Task 1 evidence

- PR #4 baseline commit `9a5ec3806e8430d910555b6919af2c13d60d096f` passed CI run #30.
- RED test commit `1c860d8277323fee5b24fcb7a12680de1e1a5bd0`: CI run #31 failed in `rust-test` because all four expected `ARCH_FORBIDDEN_*` findings were not yet implemented; other existing gates stayed green.
- Generic architecture boundary implementation commit `03e99028473dff4915b5f2568d74777625fb203b`; rustfmt-only follow-up `756ab244d9894c2e28c55dfeec61537d585a17ae`; run #33 closed the RED→GREEN cycle.
- Workspace/crate scaffold commit `2fd311c9e2b13ee75e8ba10cf14f8360591f8d81` added four capability crates and selected dependency baseline.
- Cargo.lock was generated on a GitHub-hosted runner and committed as `dc392c9bd16d67504103119326addcebfb860167`; the temporary write-enabled helper workflow was removed immediately in `1006fe5525e124f1197a47bdb5da992ffdd4bb33`.
- Docker workspace regression was root-caused to missing `COPY crates ./crates` and fixed in `16936b35fe3523a5fc251f2780d5f8dff35f1ced`.
- Security gate root cause was limited to permissive `Zlib` not yet allow-listed and internal path dependencies lacking explicit versions. `Zlib` was independently confirmed OSI-approved/permissive; minimal policy fix commit: `13b6d01dc90b275358c624c649d76ba22472aee3`.
- Final Task 1 CI run #42 (`35054390348`) passed `policy`, `rust-static`, `rust-test`, `security`, `portability-macos`, `container-build`, and `required-check`.

## Current blocker / gate

None.

## Next exact action

1. Task 2 / Step 1: add Domain value-object tests only (`VersionNo`, `ContentHash`, `FileSize`, `Title`) and verify RED in PR CI.
2. Implement minimal validated value objects + typed IDs and verify GREEN.
3. Add initial aggregate RED tests.
4. Implement `InitialDocument::create` and close Task 2 with domain tests + architecture + clippy evidence.

## Session handoff maintenance rule

Update this file whenever a Task starts/completes, a verification gate changes, a blocker appears/clears, a Design Freeze deviation is considered, or before session handoff.