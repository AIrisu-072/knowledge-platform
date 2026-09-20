# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Semantic Inspection v0`
- Current phase: **DESIGN FROZEN / POC QUALIFICATION PLAN REVIEW**
- Design branch: `design/document-semantic-inspection-v0`
- Design PR: `#7`
- Approved Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- PoC Qualification Plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
- Execution Status: `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
- Baseline: `main@73492983dd324fcd53d4b485719e5c31048f9335`

## Mandatory resume order

When resuming this repository, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
4. `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
5. `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
6. `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
7. current GitHub state of branch `design/document-semantic-inspection-v0`, PR #7, and exact-head CI

Repository and fresh GitHub state override remembered/chat state.

## Current scope

Design is approved and frozen.

The current Plan qualifies candidate parsers/adapters under `experiments/document-semantic-inspection/` only. Production implementation is deliberately deferred until PoC evidence determines which adapters may be promoted.

## Current hard gate

The PoC Qualification Plan requires explicit user approval before execution.

After approval, follow the repository/Superpowers execution workflow. Do not promote a PoC library into production merely because it builds; every required fixture gate must pass.

## Resume command

> `AIrisu-072/knowledge-platform` の `AGENTS.md` と Active Execution Pointer に従い、Document Semantic Inspection v0 のfrozen DesignとPoC Qualification Plan reviewから再開してください。現在のPR #7・exact-head CI・Execution Statusを正本にしてください。

## End-of-session rule

Before intentional session switch/context exhaustion, record exact branch/head, CI evidence, Plan approval state, blockers, and next exact action.
