# Search / Discovery Platform v0 — Execution Status

## Current checkpoint — Design APPROVED / Production Plan Review Pending, 2026-09-28 JST

- Status: **DESIGN APPROVED / NORMATIVE RECONCILIATION COMPLETE / PRODUCTION PLAN WRITTEN / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**.
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
- Production Implementation Plan approval: **not yet granted**. Do not start implementation until an explicit approval record exists.

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
Tasks B1–B5.

Creates isolated `experiments/search-discovery-poc` for Japanese lexical retrieval and HyperEdge correctness/backend feasibility.

POC REQUIRED dependencies/backends cannot be promoted without recorded evidence. Material unpredetermined selection remains a requester gate.

### Phase C — Runtime
Tasks C1–C10.

Implements rebuildable projections, in-memory projection generation store, qualified lexical adapter, typed HyperEdge reference retriever, federation/routing/materialization, evidence-driven DiscoveryService, Session Working Set and Context manifest.

### Phase D — Document Source / Acceptance
Tasks D1–D9.

Uses Document Platform as the first real Source, adds a read-only current-access use case by reusing existing Document authorization semantics, consumes existing outbox events as invalidation/index triggers, projects deterministic Document relations, adds Evaluation harness, and proves the vertical slice.

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

1. requester reviews and explicitly approves the Production master plan and Phase A–D plans;
2. record that approval in `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation-approval.md`;
3. verify the approved planning head / PR #20 exact state;
4. a fresh implementation session creates an isolated worktree/branch `feat/search-discovery-platform-v0-a` from the approved planning head;
5. implementation starts at **Task A1 RED**.

Until step 1 is explicit, Production implementation is blocked.
