# Document Versioning v0 — Execution Status

- Status: **ACTIVE — PRODUCTION IMPLEMENTATION / TASK 9 RACE REPAIR REQUALIFICATION**
- Baseline: `main@09c8235755573d16e09b8af029c22dc27caf6472` (DSI v0 PR #10 merged).
- Planning: `design/document-versioning-v0@987890d635f9563bb7028841a026663a9379d6f3`; [Draft PR #11](https://github.com/AIrisu-072/knowledge-platform/pull/11) remains open and unmerged.
- Implementation: `feat/document-versioning-v0`; last qualified code head `68178c68ee32a1870c96260e69a9692e8e33a8df`, failed documentation head `a9a2d3b360debb7b208bae6845a20d12a13e4f7a`. [Draft PR #12](https://github.com/AIrisu-072/knowledge-platform/pull/12) is based on PR #11.
- Frozen Design: `docs/superpowers/specs/2026-09-27-document-versioning-v0-design.md`. Its approval, the Production Implementation Plan, and plan approval are recorded in adjacent files. **No Design amendment is proposed.**

## Completed implementation

- Tasks 1–8: Domain contract; canonical ContentItems/FileObjects; DSI-backed preflight; Version #2+ create/update/rebase; initial/later Publish; immediate-safe-base withdrawal; durable reservation/cancellation; due execution with runnable Linux scheduler. Focused RED/GREEN evidence is in Git and the local SDD ledger.
- Task 9 integration code: initial Publish, later update/rebase, scheduled due Publish, withdrawal/restoration, orphan reconciliation, migration dry-run rollback, and ambiguous legacy attachments. Initial scheduled Publish is covered by `due_transaction.rs`.
- Audit repair: test-only RED `4df0cb4836ea00a3f041b6b06a765c2952a5d96d` found manual Publish's unwanted `serviceExecutor:null` and a successful audit result on terminal scheduled failure. GREEN `68178c68ee32a1870c96260e69a9692e8e33a8df` passed focused due 7/7, vertical slice 2/2, and strict Application/Repository Clippy.

## Verification

- Local assembled `mise run verify`: **434/434 Rust tests passed, 4 intentionally skipped**; policy, security, static, and SQLx checks passed. This preceded the final audit repair; focused regressions and strict Clippy passed afterward.
- Linux arm64 `publication-scheduler` Docker target built. Linux-container canary with test PostgreSQL, FileStorage, and real sandboxed DSI worker passed 1/1. Scheduler fail-closed startup tests passed 2/2.
- Code head `68178c68ee32a1870c96260e69a9692e8e33a8df`: standard CI `36307529282` **SUCCESS** (all jobs, including Ubuntu Rust and macOS Intel/arm64 parity); DSI Sandbox Preflight `36307529226` **SUCCESS**; DSI PoC `36307529181` **SUCCESS**.
- Documentation head `a9a2d3b360debb7b208bae6845a20d12a13e4f7a`: standard CI `36308294331` **FAIL** on the existing concurrent due-publication replay assertion; Sandbox `36308294389` and DSI PoC `36308294391` **SUCCESS**. A second runner can see the old PENDING schedule and no Publish ledger, then see `is_due = false` after the first runner commits, returning `NotDue` instead of replaying Published. The focused RED is this exact CI failure.
- Minimal repair rechecks the Publish ledger when `is_due` becomes false and makes the concurrent assertion display both outcomes. Local focused due suite **7/7 PASS**, `cargo fmt --check` PASS; fresh hosted exact-head evidence remains required.
- `git diff 987890d..68178c6 --check` passed. PR #12 has 0 unresolved review threads at the latest check.

## Boundary and next exact action

- `spec/` remains normative. Production retains one canonical ContentItem model, separate Search Extraction and DSI paths, and no durable cross-format content IR. Withdrawal records `WITHDRAWN` and restores only the immediate safe published base, otherwise null. Scheduled publication uses the same Publish ledger and database-time revalidation.
- Blocker: the concurrent replay repair has not yet passed hosted exact-head checks. **Task 9 is not complete.**
- Next exact action: commit/push the two-file race repair with this record; obtain standard CI, Sandbox, and DSI PoC success at that exact GitHub head. Then record Task 9 completion in the local SDD ledger and update PR #12 with final head/run IDs. Keep PRs #11 and #12 draft and unmerged without an explicit merge instruction.
