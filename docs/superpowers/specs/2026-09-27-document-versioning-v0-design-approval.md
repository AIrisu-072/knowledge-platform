# Document Versioning v0 — Design Approval

- Capability: `Document Versioning v0`
- Design Spec: `docs/superpowers/specs/2026-09-27-document-versioning-v0-design.md`
- Approval date: 2026-09-27 JST
- Approval source: explicit user response to the proposed Design Spec and Draft PR #11 — “これで承認します。”
- Approved proposal head: `design/document-versioning-v0@fbf42e9f41c1002a05aa79fc4932127620010c49`
- Status: **APPROVED — DESIGN FREEZE ACTIVE**

## Frozen scope

The approval covers:

- one `WORKING` Version per Document and Version #2+ creation/update/rebase from the current `PUBLISHED` base;
- repository-transaction `version_no` allocation, caller UUIDv7 operation IDs, OCC, and durable exact replay;
- ordered ContentItems, exactly one authoritative FileObject per item, and DSI-backed semantic identity without Search Extraction or a durable common cross-format IR;
- Version #2+ Publish through the existing Publish operation ledger/API, preserving the prior Version as historical `PUBLISHED`;
- withdrawal expressed by the existing `WITHDRAWN` lifecycle state, `withdrawn_at`, current pointer, and Audit/Outbox history, without a new withdrawal/restored flag;
- restoration of only the immediate recorded predecessor when it remains `PUBLISHED` and safe to expose, otherwise null current;
- durable scheduled publication of initial and later Versions through the same Publish operation ID, with due-time revalidation, cancellation, retry/terminal failure handling, and no new Version lifecycle state;
- fail-closed publication quality for unresolved tracked changes, embedded comments, and existing invalid or unverifiable signatures;
- canonical ContentItem migration with fail-closed handling of ambiguous legacy attachments;
- separately approved implementation planning before production code.

## Verification at approval

Draft PR #11 was open at the approved proposal head. Exact-head standard CI `36288001679`, DSI Sandbox Preflight `36288001693`, and DSI PoC `36288001688` were **SUCCESS**. These are documentation-branch checks, not Versioning implementation evidence.

## Change control

Changing a frozen Version identity rule, current/withdrawal restoration rule, scheduled-publication semantics, Publish/DSI trust boundary, or fail-closed condition requires an explicit Design amendment and user approval before implementation. Implementation details that preserve these rules belong in the separately approved plan.
