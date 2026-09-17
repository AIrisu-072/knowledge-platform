# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Publish v0`
- Current phase: **DESIGN REVIEW**
- Design branch: `design/document-publish-v0`
- Design PR: `#5` — **OPEN / DRAFT**
- Design Spec under review: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design approval record: **not created — written-spec review pending**
- Implementation Plan: **not created**
- Execution Status: `docs/superpowers/execution/document-publish-v0-status.md`
- Baseline: `main@2ead1e21222c3b122704b6e0ce31f3f902b93659`

## Mandatory resume order

When resuming this repository in a new chat/session/agent context, do **not** reconstruct state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-publish-v0-status.md`
4. `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
5. the current GitHub state of `design/document-publish-v0`, PR `#5`, and exact-head CI
6. only after written-spec approval: the approval record and Implementation Plan created for this capability

Repository state and fresh GitHub evidence override remembered/chat state.

## Resume command for a new ChatGPT session

A sufficient handoff prompt is:

> `AIrisu-072/knowledge-platform` の `AGENTS.md` を読み、Active Execution Pointerに従ってDocument Publish v0のDesign Reviewを再開してください。会話履歴から状態を再構成せず、repositoryのExecution Status・Design Spec・PR #5・GitHubの現在状態を正本にしてください。

## Current hard gate

The conversational design has been approved, but the written Design Spec itself is awaiting user review.

Do not create the Implementation Plan or begin production implementation until the written Design is explicitly approved.

## End-of-session rule

Before intentionally switching sessions, or whenever the conversation limit is approaching, update the capability Execution Status with:

- completed Design / Task / Step state;
- exact verification and evidence;
- branch and PR in use;
- blockers or unresolved decisions;
- the **next exact action**;
- any implementation-visible design deviation and its approval state.

Do not mark work complete without fresh verification evidence.
