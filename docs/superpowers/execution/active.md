# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Semantic Inspection v0`
- Current phase: **POC QUALIFICATION EXECUTION / TASK 2 COMPLETE / TASK 3 NEXT**
- Frozen Design PR: `#7` — merged
- Execution branch: `test/document-semantic-inspection-poc-v0`
- Execution PR: `#8` — Draft
- Approved Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- Approved PoC Qualification Plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
- Execution Status: `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
- Execution baseline: `main@5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`
- Last qualified code head: `b9da1bfa5fdf07f1b99a13248fe3233fae1082c9`

## Mandatory resume order

When resuming this repository, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-semantic-inspection-v0-status.md`
4. frozen Design Spec
5. Design approval record
6. approved PoC Qualification Plan
7. current GitHub state of `test/document-semantic-inspection-poc-v0`, PR #8, and exact-head CI

Repository and fresh GitHub state override remembered/chat state.

## Current scope

Design is approved and frozen. PoC Qualification Plan was explicitly approved on 2026-09-21.

Task 1 and Task 2 are complete. Candidate dependencies remain confined to `experiments/document-semantic-inspection/`; no production dependency promotion has occurred.

Task 2 produced one material qualification result: `scraper 0.27.0` was rejected because its transitive graph contains MPL-2.0. Direct `html5ever 0.39.0 + markup5ever_rcdom 0.39.0` passed the same semantic cases and the dependency gate.

## Current hard gate

The next task is **Task 3 — DOCX qualification**.

Do not start production Semantic Inspection implementation. Do not accept unknown potentially semantic OOXML package parts as normal success.

## Next exact action

Start Task 3 with independent OOXML fixture generation and RED tests before implementing `DocxAdapter`.

## Resume command

> `AIrisu-072/knowledge-platform` の `AGENTS.md` と Active Execution Pointer に従い、Document Semantic Inspection v0 のPoC Qualification Task 3（DOCX）から再開してください。PR #8・Execution Status・exact-head CIを正本にしてください。

## End-of-session rule

Before intentional session switch/context exhaustion, record exact branch/head, CI evidence, Plan approval state, blockers, and next exact action.
