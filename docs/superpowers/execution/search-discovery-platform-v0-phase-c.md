# Search / Discovery Platform v0 — Phase C evidence

## Scope and branch

- Implementation branch: `feat/search-discovery-platform-v0-c`, stacked on `feat/search-tantivy-production-selection` (Draft PR #26, head `3080553c09500f688b68eb5f5df592e7fde0de95` when checked on 2026-09-29).
- Code head before this receipt: `3b4af0b`. This document commit will advance the branch head. No merge or deployment has occurred.
- S1 initial policy is routed retriever-order priority concatenation **after** hard eligibility. Raw lexical/graph scores are trace-only; graph-only candidates remain eligible for qualification. This is an initial deterministic fusion policy, without score calibration, reranking, or vector promotion.

## Delivered contracts

| Task | Commit | Evidence |
| --- | --- | --- |
| S1 / C1–C3 | `502fda9`, `172b962`, `8f6d4f4` | Selected initial fusion policy; compiled source-local projections; staged, validated, and published immutable generations. |
| C4 | `b0ab93b`, `4ebdae2` | Qualified patched Tantivy lexical adapter and typed n-ary HyperEdge reference retriever. |
| C5–C6 | `b14c3c7`, `02c51a4` | Hard-gate-first S1 candidate federation and source-aware staged route planning. |
| C7 | `9cd3439` | Bounded progressive materialization, targeted Probe, current access and Receipt binding. |
| C8 | `aa06542`, `b9b20b5`, `b48982e`, `80448d7`, `4485c61`, `8c414d0`, `6609414` | Action selection, temporal hard gates, direct Source evidence, Probe outcomes, pinned retriever execution, and evidence-driven Discovery Loop. |
| C9 | `775bd66` | Stable Session Binding, Working Set, and bounded Context manifest. |
| C10 | `6f309dc` | Same-snapshot real Projection Store/Tantivy/HyperGraph/DiscoveryService vertical with graph-only recall, S1 order, Claim sufficiency, and access revocation. |
| Gate repair | `3b4af0b` | Architecture policy test now expects the already-forbidden `search-tantivy` dependency and asserts rejection. |

## Local evidence and limit

- After C8/C10 commits, `cargo test --locked -p search-core -p search-application -p search-tantivy -p search-graph-memory -p search-projection-memory` passed, including Discovery Loop 46/46 and C10 vertical 1/1. Independent read-only C8 and C10 reviews found no remaining P1/P2 in their scopes.
- An initial default-profile `mise run verify` was interrupted during workspace test compilation when free disk fell below 1 GiB. A reduced-debug rerun passed secrets, dependency/security, workflow, fmt, workspace check and strict Clippy, architecture and API checks, then found an older architecture policy test expectation; only 21/857 nextest tests ran. The focused policy repair passed 19/19 and independent read-only review.
- The final reduced-debug `mise run verify` attempt passed the same pre-test stages, then failed while linking unrelated Document integration test binaries with `errno=28` (`No space left on device`). Local `mise run verify` is **not GREEN**. Debug info and incremental compilation were disabled for the two diagnostic reruns. Rebuildable Cargo output was cleaned after each; the code and focused test receipts remain unchanged.
- Hosted exact-head CI must qualify the final PR commit, including `rust-test`, `rust-static`, `security`, `required-check`, and triggered DSI Sandbox/PoC checks. A green earlier stacked PR head is not evidence for this branch head.

## Boundaries and next action

Actual Document Source bytes/DSI evidence, call-time authorization, outbox trigger/rebuild, T10/historical visibility, and full evaluation harness belong to Phase D. Full-body Search Extraction, vector/embedding, reranking, and a dedicated graph backend remain deferred. The C10 vertical is synthetic Source data through real adapters; it does not establish a live Document Source.

Next exact action: push this branch and open a stacked Draft PR against `feat/search-tantivy-production-selection`; inspect the final PR head and hosted checks. Start Phase D only from the checked Phase C head. Do not merge or deploy without the requester's explicit instruction.
