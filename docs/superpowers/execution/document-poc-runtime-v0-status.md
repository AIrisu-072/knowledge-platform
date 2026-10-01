# Document Platform PoC Runtime / Server Composition v0 — Capability Status

## 2026-10-01 UTC — R1 documentation publication checkpoint

- Status: DESIGN / PLAN ONLY; C1 PRODUCT IMPLEMENTATION NOT STARTED; C1 ACCEPTANCE NOT RUN.
- R1 branch: `design/document-poc-runtime-v0-r1`, stacked on PR [#36](https://github.com/AIrisu-072/knowledge-platform/pull/36) branch `feat/document-gui-integration-v0` at `b578a9b49338066d0e4ee5495ea1280f991c122b`. See this branch's Draft PR for its publication head and exact-head hosted results.
- GitHub baseline rechecked 2026-10-01 UTC: PR36 remains OPEN/Draft, no unresolved review threads; main is `d71753d46590bb4406a1c0b74894ab90a27a6c88`.
- Baseline hosted runs: [CI 36860705179](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705179), [DSI Sandbox Preflight 36860705023](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705023), [DSI PoC 36860705167](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36860705167) all completed SUCCESS for b578a9b. This does not verify a new documentation head or establish C0/C1 acceptance completion.
- C0 remains incomplete pending closure/evidence reconciliation. R1 neither modifies PR36 nor claims G0–G9 COMPLETE, ACCEPTANCE GREEN or REVIEW READY.
- [Design](../specs/2026-10-01-document-poc-runtime-v0-design.md), [implementation plan](../plans/2026-10-01-document-poc-runtime-v0-implementation.md), [bounded authority record](../specs/2026-10-01-document-poc-runtime-v0-approval.md).
- Delivery: R1 design/plan → later R2 runtime implementation; separate A1 Agent design/plan → A2 Agent implementation after verified R2; separate C3 plan → E1 evaluation evidence. C2/C3 implementation and evidence remain NOT STARTED/NOT RUN. All PRs stay Draft, unmerged and undeployed.
- Current exact action: publish independently reviewed R1 documentation, verify remote and hosted head, then implement approved C1 slices in R2 under the recorded boundaries. Scheduler-specific work remains STOP.

## STOP-01 — Scheduler executor identity unresolved

- Blocker: the separate publication scheduler cannot start through its existing default entrypoint; its service-executor attribution is not named by the two approved server profiles.
- Evidence: `crates/document-publication-scheduler/src/main.rs` calls `DueScheduler::connect`; `src/runner.rs` returns `IdentityResolverRequired`; `connect_with_resolver` requires a resolver plus `service_executor: PrincipalRef`. `crates/document-application/src/scheduled_authorization.rs` preserves the original requester, sets Service invocation and records the executor separately without adding executor grants.
- Candidate, NOT APPROVED: provider `poc`, principal `poc-scheduler`, no groups/ACL grants and no selectable HTTP profile; original requester re-resolved and current ACL rechecked. A test-only `service` / `scheduler` literal is not runtime approval.
- Last verified inherited hosted GREEN head: `b578a9b49338066d0e4ee5495ea1280f991c122b` for the three baseline runs above. Last C1 runtime acceptance GREEN head: none.
- Required decision: explicit exact PoC scheduler executor identity or an authoritative prior approval. General instructions to continue do not select this identity.
- Next exact action: obtain that narrow decision and record it before Task R5 or scheduling acceptance. Continue only unaffected approved server/read-only work. C1/C3 cannot be marked complete while scheduler acceptance is blocked.

## C0 / real-runtime evidence gap

`apps/document-web/e2e/document-workspace.spec.ts` intercepts `**/v1/**`; its recorded 6/6 result belongs to older `bb5da30c7a831a9d79a16cc422f00adc89b70c69`. Existing backend E2E uses real PostgreSQL/storage/production workers but composes a Router fixture rather than the missing document-server binary. Neither is evidence of the requested real browser → composition-root journey. C1 Task R6 and C3 must supply it; C0 still needs its own honest closure record.

## Verification boundary

This documentation change is checked by independent requirements review, relative-link and scope checks, secret/private-context scan, blob identification and `git diff --check`. Product tests, dependency installation/SDK qualification, real runtime, browser, MCP and migrations were NOT RUN in R1. New-head hosted gates must be read from the Draft PR; inherited successes cannot be reused as a new-head claim.

Conditional STOP remains mandatory for any new dependency/license policy mismatch, change to frozen business/auth/Revision/Diff semantics, production identity/deployment, or a separate Agent business API. SDK selection belongs to the separate C2 implementation qualification.
