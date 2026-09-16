# Active Execution Pointer

- Status: **ACTIVE**
- Execution mode: **Inline Execution**
- Active capability: `Document Authoritative Core — Create/Get v0`
- Design PR: `#3`
- Approved Design Spec: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md`
- Design approval record: `docs/superpowers/specs/2026-09-16-document-authoritative-core-design-approval.md`
- Implementation Plan: `docs/superpowers/plans/2026-09-16-document-authoritative-core-implementation.md`
- Execution Status: `docs/superpowers/execution/document-authoritative-core-status.md`

## Mandatory resume order

When resuming this repository in a new chat/session/agent context, do **not** reconstruct implementation state from conversation history.

Read in this order:

1. `AGENTS.md`
2. this file
3. `docs/superpowers/execution/document-authoritative-core-status.md`
4. `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md`
5. `docs/superpowers/plans/2026-09-16-document-authoritative-core-implementation.md`
6. the current GitHub branch/PR/CI state named by the execution-status file

Repository state and CI evidence override remembered/chat state.

## Resume command for a new ChatGPT session

A sufficient handoff prompt is:

> `AIrisu-072/knowledge-platform` の `AGENTS.md` を読み、Active Execution Pointerに従ってDocument Authoritative CoreのInline Executionを再開してください。会話履歴から状態を再構成せず、repositoryのExecution StatusとGitHubの現在状態を正本にしてください。

## End-of-session rule

Before intentionally switching sessions, or whenever the conversation limit is approaching, update the capability Execution Status with:

- completed Task/Step numbers;
- current Task/Step;
- exact verification commands and results/evidence;
- branch and PR in use;
- blockers or unresolved decisions;
- the **next exact action**;
- any implementation-visible design deviation and its approval state.

Do not mark work complete without fresh verification evidence.
