# Search / Discovery Platform v0 — Phase A evidence

## Scope and branch

- Approved planning base: `design/search-discovery-platform-v0@252245f5bbf63958739d2f9b6d82cf39d4ec94f6` (planning PR #20).
- Implementation branch: `feat/search-discovery-platform-v0-a`.
- A1–A7 code is committed. Phase A remains **gate pending** until the final Draft PR head has green hosted checks.
- No Search retrieval/index backend or PoC-only dependency has been promoted to production crates.

## Task commits

| Task | Commit | Delivered contract |
| --- | --- | --- |
| A1 | `af9fb29` | Search crate and architecture boundaries |
| A2 | `58c4352` | Typed Source, Resource, Usage and Discovery profiles |
| A3 | `ddeafb8` | Typed Predicate IR and four-valued evaluation |
| A4 | `da37023` | Assertion, Authority, Identity, Observation and Temporal contracts |
| A5 | `f0c0dc3` | Typed n-ary HyperEdge and constrained traversal |
| A6 | `d05a6fe` | Applicability, Contrast, Evidence, Gaps and Discovery result |
| A7 | `763f486` | Provider-neutral application ports and stable Binding |
| Review fixes | `d82e397` | Fact provenance, independent origin, n-ary pattern, identity, decimal and freshness boundaries |
| CI tool configuration | `6e243d7` | Pinned OSV Scanner SLSA signer and issuer in mise configuration |

## Local verification

- A1 architecture policy tests: 7/7 new rules and 18/18 full policy tests passed.
- Final Search suite: `cargo test -p search-core -p search-application` passed, **58/58** tests across seven integration test binaries.
- Final focused strict Clippy (`search-core`, `search-application`), `cargo fmt --all --check`, `mise run arch:check`, and `git diff --check` passed.
- A read-only independent review found six contract edge cases. RED tests reproduced all six; the review-fix commit made the focused tests green. The reviewer did not run tests or approve hosted gates.
- `mise run verify:fast` passed `fmt`, workspace `cargo check --locked`, workspace strict Clippy, architecture check and OpenAPI lint. Its `test:rust` step stopped at link time with `No space left on device` (`errno=28`) for `document-semantic-inspection-worker` tests. This is a **local capacity blocker**, not a passing fast gate or a Search test failure. The run used `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`, and `CARGO_BUILD_JOBS=2`; peak current-worktree `target` was 4.3 GiB and remaining disk reached 133 MiB. The build artifacts in this worktree were cleaned after preserving the diagnostic result.
- The OSV Scanner signer configuration passed a local forced `mise install` with checksum and SLSA verification. Hosted security remains to be verified for the exact PR head.

## Deferred decisions and next gate

The concrete graph backend, Japanese tokenizer, vector/embedding, reranker and fusion choices remain deferred to Phase B PoC and Selection Gate S1. DSI is not represented as full document-body Search Extraction. Search does not own Document access policy or generic outbox delivery.

Next exact action: push the Phase A branch, create a stacked **Draft** PR against `design/search-discovery-platform-v0`, and inspect CI, DSI Sandbox and DSI PoC on the exact PR head. Phase B B1 RED starts only after the Phase A gate is qualified. Do not merge either PR.
