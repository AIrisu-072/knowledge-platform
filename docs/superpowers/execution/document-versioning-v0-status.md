# Document Versioning v0 — Execution Status

- Status: **ACTIVE — PRODUCTION PLANNING**
- Baseline: `main@09c8235755573d16e09b8af029c22dc27caf6472`
- Active branch: `design/document-versioning-v0`
- Draft PR #11: `https://github.com/AIrisu-072/knowledge-platform/pull/11`. Last exact PR head before this planning update: `fbf42e9f41c1002a05aa79fc4932127620010c49`. Standard CI `36288001679`, DSI Sandbox Preflight `36288001693`, and DSI PoC `36288001688` are all **SUCCESS** at that head. These are documentation checks; no Versioning production code exists.
- User-approved Frozen Design: `docs/superpowers/specs/2026-09-27-document-versioning-v0-design.md`. Approval record: `docs/superpowers/specs/2026-09-27-document-versioning-v0-design-approval.md` (explicit user response: “これで承認します。”).
- Proposed Implementation Plan: `docs/superpowers/plans/2026-09-27-document-versioning-v0-production-implementation.md` (**not approved**). No Versioning production implementation has started.

## Completed prerequisite

- Document Semantic Inspection v0 production Tasks 1–14 are complete. PR #10 merged into `main` as `09c8235755573d16e09b8af029c22dc27caf6472` from final PR head `12047186787e2380bbc6ec74c4baa617b71b2525`.
- Exact merge-head CI `36281613991`: **SUCCESS**, including Ubuntu Rust tests and macOS Intel/arm64 semantic parity.
- Existing Document Publish v0 handles initial Version #1 publication. Its approved design explicitly defers Version #2+ publication and current-version replacement to Document Versioning.

## Design boundary

- `spec/` remains normative. The approved Document Semantic Inspection v0 design §18 carries forward one WORKING Version per Document, a base from the current PUBLISHED Version, repository-transaction version numbering, caller UUIDv7 operation IDs, Document revision increments, immutable PUBLISHED Versions, ContentItems, and reuse of the Publish operation ledger/API.
- The next design must preserve the separate Semantic Inspection and Search Extraction paths, avoid a durable common content IR, and require successful inspection of every authoritative ContentItem.
- User-selected design scope includes withdrawal and scheduled publication. The user requested inline execution in this session, so no worker is dispatched. The exact route for any later worker is `gpt-6-sol` with `ultra` effort.
- User-confirmed withdrawal rule: transition the withdrawn current Version to `WITHDRAWN`, then restore the immediately preceding eligible `PUBLISHED` Version as `current_version_id`; use null when no such Version exists. Do not add a withdrawal/restore flag. Existing lifecycle state and `withdrawn_at` express the withdrawn Version; Audit/Outbox history records the old and restored current IDs. Fallback integrity/quality behavior must be specified in the design.
- The Frozen Design extends existing Domain/Application/PostgreSQL and the Publish ledger/API, preserves immutable authoritative FileObjects and DSI inspection for every ContentItem, and executes durable scheduled publication through the same idempotent Publish transition with due-time revalidation. It requires immediate-base restoration on withdrawal, falling back to null if the base cannot be proved safe.
- The two normative `spec/data/` documents now resolve T4 and scheduled publication. Document-wide publication end (T10) remains separate because T4 can restore an older current Version. No Frozen Design amendment is proposed. There is no approval to write production Versioning code yet.

## Next exact action

Complete focused self-review of the normative updates and proposed Production Implementation Plan, then commit/push the approval/spec/plan record to PR #11. Obtain explicit user approval of the plan. After approval, create the implementation branch from the approved planning baseline and start Task 1 inline with focused RED/GREEN evidence; do not start code or merge PR #11 on design approval alone.
