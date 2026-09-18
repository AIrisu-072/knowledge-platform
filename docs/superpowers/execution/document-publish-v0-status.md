# Document Publish v0 — Execution Status

- Capability: `Document Publish v0`
- Execution mode: **Inline Execution**
- Overall phase: **IMPLEMENTATION COMPLETE / IMPLEMENTATION PR FINAL GATE**
- Design: **APPROVED — design freeze active**
- Implementation Plan: **TASKS 1–8 EXECUTED**
- Product/runtime implementation: **COMPLETE**
- Approved Design Spec: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-17-document-publish-v0-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-17-document-publish-v0-implementation.md`

## Current repository flow

- Design PR `#5`: **MERGED**
- Design merge / implementation baseline: `main@51fb06ae2c62886ab032fb2767104b65cc33c8ee`
- Implementation branch: `feat/document-publish-v0`
- Implementation PR: `#6`
- Latest verified runtime implementation head: `a89a7d6e1f1f85722521b8ef89b43c183f9ee9d9`
- Runtime CI: run `#211` / id `35302575027` — **SUCCESS**
- Runtime Rust tests: **81/81 PASS, 0 skipped**

The branch may advance with documentation-only evidence commits after the verified runtime head. Always fetch PR #6 current head and exact-head CI before acting.

## Implemented capability

Document Publish v0 now provides:

- caller-generated Application-layer UUIDv7 `PublishOperationId`;
- command identity containing Document, target Version, expected Document revision, and actor;
- permanent `document_publish_operations` idempotency records;
- exact-command replay with stored result;
- operation-ID misuse detection as Conflict;
- initial-only Publish: `WORKING -> PUBLISHED`;
- `published_at` assignment;
- `Document.current_version_id = target`;
- `Document.revision: 0 -> 1`;
- no approval prerequisite;
- PRIMARY final-object preflight before a new Publish;
- storage object absence/unreadability -> IntegrityViolation;
- storage dependency outage -> StorageUnavailable;
- PostgreSQL short Document row lock plus expected-revision OCC;
- composite FK enforcing same-Document current-version ownership;
- atomic authoritative state + one Domain Outbox + one mandatory Audit Outbox + one operation-result record;
- rollback of all Publish state on transaction failure;
- conservative `CommitOutcomeUnknown` handling;
- exact-command retry recovery after both pre-commit and post-commit ambiguous outcomes;
- initial PUBLISHED-state reconstruction through existing GetDocument;
- real PostgreSQL concurrency protection;
- real filesystem + PostgreSQL Create -> Publish -> Get/open vertical slice.

## Explicitly still out of scope

No Version #2+ creation, current-version replacement, Withdraw, approval workflow, scheduled publication, automatic publisher, AccessPolicy, HTTP/OpenAPI, UI, Search indexing, outbox delivery worker, Audit Store delivery, generic idempotency framework, operation cleanup, or lifecycle trigger was added.

## TDD / execution evidence

Representative RED evidence:

1. Domain Publish API missing:
   - head `052453d25e76e4613a8226ca0aafc7b9d63582a3`
   - CI `#154`
   - `fmt` passed; Rust compile/test failed because `publish_initial_version` and required Domain errors did not yet exist.
2. Application orchestration missing:
   - head `b0a5657382134169d251e6066edf1737facd3921`
   - CI `#168`
   - contract tests failed because `publish_document` / segregated Publish repository behavior was not yet implemented.
3. PostgreSQL Publish read helpers missing:
   - head `da1df6a7fc926c1762e25eb46951e1ca4c59c6a4`
   - CI `#180`
   - tests failed on missing `get_publish_operation` / `get_publish_candidate`.
4. Atomic PostgreSQL Publish adapter missing:
   - head `6e7361ccb0e3d83c68aba6597a397cc193c0c499`
   - CI `#196`
   - transaction tests failed because `PostgresDocumentRepository` did not yet implement `DocumentPublishRepository`.
5. Transaction classification regression found by tests:
   - head `c3b03fc26b3b4f0f617d09da12e68ebc01486f0e`
   - CI `#201`
   - all static gates passed; real PostgreSQL tests exposed `current_version_id = other` being classified as IntegrityViolation instead of Conflict.

Task 7 added real concurrency and recovery evidence. The final recovery/vertical tests did not require additional product behavior beyond the already-correct implementation; they verify the frozen contract.

## Final runtime verification evidence

Exact runtime head:

`a89a7d6e1f1f85722521b8ef89b43c183f9ee9d9`

Hosted PR CI run `#211` (`35302575027`) is **SUCCESS**:

- `policy`: PASS
  - architecture checks
  - repository policy
  - API check
  - assurance scan/plan/run/report
- `rust-static`: PASS
  - `fmt`
  - `check:rust`
  - `sqlx:check`
- `rust-test`: PASS
- `security`: PASS
- `portability-macos`: PASS
- `container-build`: PASS / SBOM
- `required-check`: PASS

The hosted CI decomposes and passes the dependency closure used by `verify:fast`, `verify`, and `verify:full`. This connector session did **not** separately execute those three local aggregate commands, so they are not falsely recorded as local runs.

Rust evidence:

- **81 tests across 24 binaries**
- **81 passed, 0 skipped**
- Publish application contract tests: PASS
- Domain Publish invariant tests: PASS
- Publish schema test: PASS
- Publish transaction tests: PASS
- distinct-operation concurrency: PASS
- same-operation concurrent replay: PASS
- real Create -> Publish -> Get/open vertical slice: PASS
- before-commit unknown retry: PASS
- after-commit unknown retry: PASS

## Frozen Design coverage self-review

- initial Publish only — **implemented**
- caller UUIDv7 operation ID — **implemented + validated**
- same-operation replay — **implemented**
- operation-ID misuse Conflict — **implemented**
- OCC + short row lock — **implemented**
- file-preflight semantics — **implemented**
- same-Document current composite FK — **implemented**
- PUBLISHED/current atomic semantics — **implemented**
- Domain Outbox atomicity — **implemented**
- mandatory Audit Outbox atomicity — **implemented**
- operation-result atomicity — **implemented**
- unknown-commit exact-command recovery — **implemented**
- current replacement — **absent**
- Withdraw / scheduler / approval flow — **absent**
- HTTP / UI / Search / outbox worker — **absent**

No Design amendment was required.

## Self-review findings

- Critical: **0**
- Important: **0**
- Minor cleanup completed: removed unused Publish row scaffolding after GREEN verification.
- No unresolved known contract mismatch remains.

## Current gate

The runtime implementation is complete and verified. Documentation/evidence commits may move the PR head beyond the runtime head and therefore require their own exact-head hosted CI before PR #6 is marked Ready.

Do not merge PR #6 without an explicit user merge instruction.

## Next exact action

1. Update the Active Execution Pointer to PR #6 implementation-complete state.
2. Run/fetch hosted CI for the exact final documentation/evidence head.
3. Fetch inline review threads and submitted reviews.
4. If exact-head CI is green and no blocking review exists, update PR #6 evidence and mark it Ready for review.
5. **Stop at the merge gate.**

## Session handoff rule

Before resuming, read the repository SSOT and fetch current PR #6 state/head/CI. GitHub state overrides chat memory.

Do not claim merge completion unless PR #6 is actually merged.
