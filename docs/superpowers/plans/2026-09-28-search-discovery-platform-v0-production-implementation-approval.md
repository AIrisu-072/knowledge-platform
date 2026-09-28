# Search / Discovery Platform v0 — Production Implementation Plan Approval

- Approval status: **APPROVED**
- Approved by: requester
- Approval date: 2026-09-29 JST
- Repository: `AIrisu-072/knowledge-platform`
- Planning branch: `design/search-discovery-platform-v0`
- Planning PR: #20
- Plan-review head before this approval record: `c9ba9a457c5825014a5726cac2ad076e0019efd7`

## Approved plans

The requester explicitly approved the complete Search / Discovery Platform v0 Production Implementation Plan set:

- `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-production-implementation.md`
- `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-a-core.md`
- `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-b-poc.md`
- `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-c-runtime.md`
- `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-d-integration.md`

Approved task structure:

- Phase A: A1–A8
- Phase B: B1–B6
- Phase C: C1–C10
- Phase D: D1–D9

## Approval scope

This approval authorizes a fresh implementation session to begin Production implementation from Task A1, provided it first verifies the repository/GitHub current state and this approval record.

The implementation session may:

1. create the isolated implementation branch/worktree described by the approved plan;
2. execute Phase A tasks using TDD;
3. proceed through the approved phase sequence and gates;
4. create stacked Draft PRs as described in the approved plan;
5. update the Search-specific execution status and evidence records.

## Preserved gates

This approval does **not** waive later selection/review gates.

In particular:

- POC REQUIRED dependencies/backends still require Phase B evidence before production promotion;
- material backend/tool choices not predetermined by the approved Design must stop at Selection Gate S1 if the plan requires requester review;
- explicit merge instructions remain required for planning or implementation PRs;
- deployment and production migration remain separately gated.

## Frozen boundaries

Implementation must preserve the approved Design, including:

- Search Platform remains inside Knowledge Platform;
- federated/source-local Discovery;
- typed DiscoverableResource and Assertion / Authority semantics;
- UNKNOWN is not FALSE;
- Typed N-ary Relation / HyperEdge is canonical Graph semantics;
- Graph RAG is internal to Search Platform;
- Search Extraction remains distinct from Document Semantic Inspection;
- Document current-access checks reuse Document authorization semantics rather than duplicating ACL logic in Search;
- Search consumer does not own the generic Document Outbox delivery lifecycle;
- Evidence Sufficiency controls Discovery completion;
- Session Binding is stable;
- Execution Control Plane / Task DAG / credentials / Human Oversight remain outside Search;
- correctness/evidence/retention are not weakened for performance.

## Implementation start

After this approval record is committed, the exact planning head containing this record becomes the approved implementation baseline.

A fresh implementation session must re-read live `main`, PR #20, the Design, this approval, all approved plans, and exact-head hosted checks before starting.

If no material conflict exists, it should create `feat/search-discovery-platform-v0-a` from the approved planning head and begin **Task A1 RED** without asking for another implementation confirmation.
