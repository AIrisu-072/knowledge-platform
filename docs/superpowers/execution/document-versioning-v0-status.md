# Document Versioning v0 — Execution Status

- Status: **ACTIVE — DESIGN DISCOVERY**
- Baseline: `main@09c8235755573d16e09b8af029c22dc27caf6472`
- Active branch: `design/document-versioning-v0`
- Proposed Design Spec head: `76d3d951f5dca82ad1955a916c083db3dd66d41b`, pushed to `design/document-versioning-v0`. Draft PR #11: `https://github.com/AIrisu-072/knowledge-platform/pull/11`. No Versioning production code exists.
- Exact design-head GitHub runs: standard CI `36287858738`, DSI Sandbox Preflight `36287858740`, DSI PoC `36287858760` — all **IN_PROGRESS** at the 2026-09-27 status check. Baseline merge-head CI `36281613991` was **SUCCESS**. Documentation diff check **PASS**.
- Design proposal: `docs/superpowers/specs/2026-09-27-document-versioning-v0-design.md` (**PROPOSED; not approved**). Implementation and implementation plan have not started.

## Completed prerequisite

- Document Semantic Inspection v0 production Tasks 1–14 are complete. PR #10 merged into `main` as `09c8235755573d16e09b8af029c22dc27caf6472` from final PR head `12047186787e2380bbc6ec74c4baa617b71b2525`.
- Exact merge-head CI `36281613991`: **SUCCESS**, including Ubuntu Rust tests and macOS Intel/arm64 semantic parity.
- Existing Document Publish v0 handles initial Version #1 publication. Its approved design explicitly defers Version #2+ publication and current-version replacement to Document Versioning.

## Design boundary

- `spec/` remains normative. The approved Document Semantic Inspection v0 design §18 carries forward one WORKING Version per Document, a base from the current PUBLISHED Version, repository-transaction version numbering, caller UUIDv7 operation IDs, Document revision increments, immutable PUBLISHED Versions, ContentItems, and reuse of the Publish operation ledger/API.
- The next design must preserve the separate Semantic Inspection and Search Extraction paths, avoid a durable common content IR, and require successful inspection of every authoritative ContentItem.
- User-selected design scope includes withdrawal and scheduled publication. The user requested inline execution in this session, so no worker is dispatched. The exact route for any later worker is `gpt-6-sol` with `ultra` effort.
- User-confirmed withdrawal rule: transition the withdrawn current Version to `WITHDRAWN`, then restore the immediately preceding eligible `PUBLISHED` Version as `current_version_id`; use null when no such Version exists. Do not add a withdrawal/restore flag. Existing lifecycle state and `withdrawn_at` express the withdrawn Version; Audit/Outbox history records the old and restored current IDs. Fallback integrity/quality behavior must be specified in the design.
- The proposed Design Spec extends existing Domain/Application/PostgreSQL and the Publish ledger/API, preserves immutable authoritative FileObjects and DSI inspection for every ContentItem, and executes durable scheduled publication through the same idempotent Publish transition with due-time revalidation. It proposes immediate-base restoration on withdrawal, falling back to null if the base cannot be proved safe. The user has confirmed the no-extra-flag restoration direction but has not yet approved the full written Design Spec.
- No Design/profile amendment is proposed. There is no approval to write production Versioning code yet.

## Next exact action

Review the proposed Design Spec with the user, especially first-Version withdrawal, fallback-to-null on failed predecessor validation, scheduled intent/cancellation, and stale `WORKING` rebase. Incorporate feedback and seek explicit written-spec approval. Then update the deferred normative T4/schedule rules, prepare a separate implementation plan, and seek plan approval before implementation.
