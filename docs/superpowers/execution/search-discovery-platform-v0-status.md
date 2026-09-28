# Search / Discovery Platform v0 — Execution Status

## Current checkpoint — Phase A A1–A6 committed, A7 RED next, 2026-09-29 JST

- Status: **PRODUCTION PLAN APPROVED / PHASE A IN PROGRESS**. Search implementation branch `feat/search-discovery-platform-v0-a` is based on approved planning head `252245f5bbf63958739d2f9b6d82cf39d4ec94f6`. Latest code head before this status update: `d05a6fe4306f63d6124d14536e8219dfe7da5145`. No Phase A PR yet; verify live Git for the status-commit exact head.
- Planning PR #20 was OPEN/Draft on `design/search-discovery-platform-v0` at the start check, with no unresolved review threads; main was `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`. Parallel Document Diff work had no material Search conflict. Recheck live GitHub at Phase A PR creation.
- A1 `af9fb294`: Search crate boundaries; RED architecture tests 7 failures, GREEN 7/7, full policy 18/18, strict Clippy/fmt/arch passed.
- A2 `58c4352b`: typed Source/Resource/Usage/DiscoveryLens/Cost contracts; RED missing modules, GREEN 7/7, strict Clippy/fmt/arch passed.
- A3 `ddeafb8`: typed Predicate IR and four-valued evaluator; RED missing modules, GREEN 15/15 contract tests plus `search-core` suite 22/22, strict Clippy/fmt/arch passed. Money/decimal use integer-based exact comparison; expression and nested collection evaluation have depth limits.
- A4 `da37023`: Assertion/Authority/Identity/Observation/Temporal contracts; RED missing modules and scoped observation RED, GREEN 9/9 contract tests plus `search-core` suite 31/31, strict Clippy/fmt/arch passed.
- A5 `f0c0dc3`: typed n-ary HyperEdge and constrained traversal; RED missing modules, GREEN 4/4 contract tests, strict Clippy/fmt/arch passed. Cross-relation false composite fixture is negative.
- A6 `d05a6fe`: Applicability/Contrast/Evidence/Discovery contracts; RED missing modules, then RED false-SUFFICIENT tests, GREEN 12/12 contract tests, strict Clippy/fmt/arch passed. Authority/freshness requirements without evaluators remain `UNRESOLVED`; independent upstream origins are required for corroboration.
- Hosted planning-head CI `36444150997` failed only in security tool installation: `mise.lock` requires an SLSA signer for `google/osv-scanner@2.5.1`; DSI Sandbox `36444151151` succeeded; DSI PoC did not trigger on the docs-only planning head. This inherited toolchain gate must be fixed and exact-head hosted gates rerun before declaring Phase A complete.
- Managed read-only inspection runs `search-v0-a1-inspect-20260929` and `search-v0-a2-inspect-20260929` remain inspect-before-resume with pending reads; no code edits by those workers. Do not automatically replay. Phase A is following the user-authorized inline `executing-plans` fallback. Parent checkpoint and ignored SDD ledger contain task evidence.
- Design Freeze difference: none. Graph backend, tokenizer, Vector engine, embedding, reranker, and fusion choices remain deferred to Phase B PoC/Selection Gate S1.
- Next exact action: Task A7 RED tests in `crates/search-application/tests/port_contract.rs` and `binding_contract.rs`, then provider-neutral ports and stable Binding GREEN; run focused tests, strict Clippy/fmt/arch, commit. A8 follows. Do not modify repository-wide `active.md` or merge.

---

## Current checkpoint — Phase A A1/A2 committed, A3 RED next, 2026-09-29 JST

- Status: **PRODUCTION PLAN APPROVED / PHASE A IN PROGRESS**. The approval record is `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation-approval.md`.
- Approved planning base: `design/search-discovery-platform-v0@252245f5bbf63958739d2f9b6d82cf39d4ec94f6`, PR #20 OPEN/Draft, base `main@6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`, unresolved review threads 0. Planning branch is 30 commits ahead of main and 0 behind at the start check.
- Implementation branch: `feat/search-discovery-platform-v0-a`, isolated worktree. Latest code commit at this checkpoint: A2 `58c4352b0dadafad91ebce24e20111e136ca4239`; no Phase A PR yet. The status-record commit itself advances the branch head, so verify live Git for the current exact head.
- A1 commit `af9fb2945f327adff6fa174cea507f6a366ea5b1`: Search crate and architecture boundaries. RED: 7/7 new policy tests failed for absent rules. GREEN: 7/7; full policy 18/18, Search crate `cargo check`, strict Clippy, fmt, and `mise run arch:check` passed.
- A2 commit `58c4352b0dadafad91ebce24e20111e136ca4239`: stable typed IDs, Source/Resource/Usage/Profile/Temporal/Cost contracts. RED: `resource_contract` failed compilation only on absent modules. GREEN: 7/7, strict `search-core` Clippy, fmt, and architecture check passed.
- Planning-head hosted CI `36444150997` on `252245f5` **FAIL** in security tool installation: mise requires an SLSA signer for the pinned `google/osv-scanner@2.5.1`; other CI jobs succeeded. DSI Sandbox `36444151151` **SUCCESS**. DSI PoC did not trigger for the docs-only planning PR head. The same tool lock configuration exists on `main` and the planning branch; this is not a Search code test result.
- Parallel Document Diff work observed on a separate branch with one added design document and no material Search implementation conflict. Do not change repository-wide `active.md`.
- Managed read-only inspection runs `search-v0-a1-inspect-20260929` and `search-v0-a2-inspect-20260929` stopped at pending reads, with no code edits. Do not automatically replay them. The approved inline fallback is being used for Phase A tasks; the parent checkpoint and ignored SDD ledger record evidence.
- Design Freeze difference: none. Physical backend/tokenizer/vector/reranker/fusion selection remains deferred.
- Next exact action: write `crates/search-core/tests/predicate_contract.rs` for Task A3, run `cargo test -p search-core --test predicate_contract` to confirm RED, then implement the typed four-valued Predicate IR.

---

## Current checkpoint — Design APPROVED / Production Plan APPROVED / Implementation Ready, 2026-09-29 JST

- Status: **DESIGN APPROVED / NORMATIVE RECONCILIATION COMPLETE / PRODUCTION PLAN APPROVED / IMPLEMENTATION READY — TASK A1 RED NEXT**.
- Repository baseline at design start: `main@6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`.
- Design branch: `design/search-discovery-platform-v0`.
- Draft planning PR: #20.
- Written Design Spec: `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`.
- Design approval: `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design-approval.md`.
- Production master plan: `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation.md`.
- Phase plans:
  - `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-a-core.md`
  - `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-b-poc.md`
  - `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-c-runtime.md`
  - `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-d-integration.md`
- Production code / migration / production dependency changes: **none**.
- Production Implementation Plan approval: **APPROVED**. Approval record: `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation-approval.md`.

## Approved design

The requester explicitly approved the written Design Spec after D1〜D9 were fixed individually.

The approved architecture includes:

- Search Platform inside Knowledge Platform, with Discovery as its higher-level capability;
- Federated Source Registry and source-local discovery;
- typed DiscoverableResource / UsageProfile / DiscoveryLens;
- typed Predicate IR, UNKNOWN semantics, Applicability / Contrast;
- Assertion / Authority / Logical Identity / Observation / Temporal models;
- first-class Typed N-ary Relation / HyperEdge Graph Projection;
- adaptive evidence-driven Retrieval / Probe / Materialization;
- Evidence Requirement / Sufficiency / Failure Attribution;
- session-stable Binding / Task-scoped Context Compiler boundary;
- separation from Task DAG / Execution Control Plane / credentials / Human Oversight;
- correctness-first cost/performance architecture;
- stage-wise Evaluation / Assurance.

Physical Graph/Vector backend, embedding model, reranker, transport, physical topology and exact production SLOs remain intentionally deferred.

## Normative reconciliation

After Design approval, the approved meaning was reconciled into the planning branch only:

- `spec/architecture/architecture-contract-v0.md`
- `spec/architecture/system-architecture-v0.md`
- `spec/architecture/system-architecture-v0.d2`
- `spec/data/logical-data-model-v0.md`
- `spec/data/logical-data-model-v0.d2`
- `spec/data/data-characteristics-v0.md`
- `spec/data/transaction-consistency-requirements-v0.md`
- `spec/selection/library-tool-selection-v0.md`
- `spec/operations/observability-audit-requirements-v0.md`

Key reconciliation:

- Graph Representation is no longer `future`; Graph retrieval semantics are REQUIRED.
- Concrete Graph backend remains POC REQUIRED / deferred.
- Search canonical model is expanded beyond document text into typed Resource / Assertion / HyperEdge semantics.
- on-demand work is limited to approved targeted Probe / Progressive Materialization; query-time unconditional full extraction remains prohibited.
- source-local/federated discovery and Evidence-driven completion are normative.
- Graph RAG remains an internal Search Platform capability, not a separate external system.

## Implementation program

The implementation program is split so a fresh session can proceed without redesigning the architecture:

### Phase A — Core Contracts
Tasks A1–A8.

Creates pure `search-core` / `search-application` boundaries, Predicate IR, Authority/Identity/Observation, HyperEdge, Applicability/Evidence, Bindings and application ports.

No retrieval/index backend is promoted.

### Phase B — PoC / Selection
Tasks B1–B6.

Creates isolated `experiments/search-discovery-poc` for Japanese lexical retrieval, HyperEdge correctness/backend feasibility, and rank-fusion qualification.

POC REQUIRED dependencies/backends cannot be promoted without recorded evidence. Material unpredetermined selection remains a requester gate.

### Phase C — Runtime
Tasks C1–C10.

Implements rebuildable projections, in-memory projection generation store, qualified lexical adapter, typed HyperEdge reference retriever, federation/routing/materialization, evidence-driven DiscoveryService, Session Working Set and Context manifest.

### Phase D — Document Source / Acceptance
Tasks D1–D9.

Uses Document Platform as the first real Source, adds a read-only current-access use case by reusing existing Document authorization semantics, implements a transport-neutral idempotent Search consumer for existing Domain events without owning the generic outbox delivery lifecycle, projects deterministic Document relations, adds Evaluation harness, and proves the vertical slice.

## Search Extraction boundary

Current DSI is **not** full Search Extraction.

Search v0 may index data explicitly available from current Document contracts, including title/metadata/lifecycle/folder/access and DSI capability/evidence-derived structured facts/relations.

It MUST NOT claim full Document body search by reinterpreting DSI fingerprints/evidence as text.

Full body / section / table / sheet / slide Search Extraction requires a separately approved capability before implementation.

## Plan self-review

- Placeholder scan: no TBD / TODO / FIXME / PLACEHOLDER.
- Task numbering: A1–A8, B1–B5, C1–C10, D1–D9, no gaps.
- Spec coverage checked for Source Registry, typed Resource, Usage/Discovery Profile, Predicate, Assertion/Authority, Observation, HyperEdge, Projection Generation, Source Routing, Probe, Evidence, Binding, Context, Evaluation, Search Extraction boundary and Document current-access boundary.
- Interface gap repairs completed before review:
  - `SearchError`, `DiscoveryRequest`, `FederatedCandidate` ownership fixed in Phase A;
  - source-local Directory/Structured projection store added to Phase C;
  - Document current-access reauthorization use case added to Phase D.
- No Production code or dependency promotion was performed while writing the plan.

## Parallel-work rule

Document Platform / Document Diff work may continue independently.

This Search planning branch intentionally does not modify `docs/superpowers/execution/active.md`, so a parallel implementation capability can retain the repository-wide active pointer.

At implementation-session start, re-read live `main`, PR #20 and any parallel Document changes. If main advanced, reconcile actual conflicts before creating the implementation worktree; do not silently change approved Search semantics.

## Next exact action

1. a fresh implementation session re-reads repository/GitHub live state, PR #20, Design/approval, Production Plan/approval, and the four phase plans;
2. verify that the approved planning branch is not materially conflicted by parallel Document work and that the exact-head hosted gates are acceptable;
3. create an isolated worktree/branch `feat/search-discovery-platform-v0-a` from the approved planning branch head;
4. begin **Task A1 RED** exactly as written in the Phase A plan;
5. keep this Search-specific status current and do not take over repository-wide `active.md` while another parallel capability owns it.

No further implementation confirmation is required unless an explicit selection gate, material repository conflict, or Design Freeze conflict is encountered.
