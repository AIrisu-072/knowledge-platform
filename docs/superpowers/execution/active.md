# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Semantic Inspection v0`
- Current phase: **DESIGN REVIEW**
- Design path: **Architectural**
- Design branch: `design/document-semantic-inspection-v0`
- Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Execution Status: `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
- Baseline: `main@73492983dd324fcd53d4b485719e5c31048f9335`

## Mandatory resume order

When resuming this repository, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
4. `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
5. current GitHub state of branch `design/document-semantic-inspection-v0`

Repository and fresh GitHub state override remembered/chat state.

## Current scope

The written Design covers:

- overall architecture and capability boundaries;
- data contracts;
- `ensure` processing flow;
- sandbox/security boundary;
- failure taxonomy;
- supported-format policy;
- format-specific acceptance criteria;
- cross-format representation/equivalence rules;
- detailed-diff boundary;
- library-selection policy;
- PoC Coverage Matrix and production promotion gate.

No production implementation has started.

## Current hard gate

The written Design Spec is awaiting user review.

Required order:

1. user reviews the written Design Spec;
2. apply requested changes if any;
3. obtain explicit approval of the written Spec;
4. create/finalize the Design approval record;
5. freeze Design;
6. invoke the implementation-planning workflow.

Do not start PoC implementation or add PoC libraries to production dependencies before the written Design approval gate.

## Resume command

> `AIrisu-072/knowledge-platform` の `AGENTS.md` と Active Execution Pointer に従い、Document Semantic Inspection v0 のDesign Reviewから再開してください。Execution Status・Design Spec・現在のdesign branchを正本にしてください。

## End-of-session rule

Before intentional session switch/context exhaustion, record exact branch/head, review/approval state, blockers, and next action.
