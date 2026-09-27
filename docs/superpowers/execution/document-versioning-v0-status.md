# Document Versioning v0 — Execution Status

- Status: **ACTIVE — DESIGN DISCOVERY**
- Baseline: `main@09c8235755573d16e09b8af029c22dc27caf6472`
- Active branch: `design/document-versioning-v0`
- Last committed branch head before this checkpoint update: `4b9df6962697f6969c42e45499be8071412f85f9`; no Versioning design code or CI run exists. The baseline merge-head CI is `36281613991` (**SUCCESS**).
- Implementation: **not started**; no Versioning design spec or implementation plan has been approved.

## Completed prerequisite

- Document Semantic Inspection v0 production Tasks 1–14 are complete. PR #10 merged into `main` as `09c8235755573d16e09b8af029c22dc27caf6472` from final PR head `12047186787e2380bbc6ec74c4baa617b71b2525`.
- Exact merge-head CI `36281613991`: **SUCCESS**, including Ubuntu Rust tests and macOS Intel/arm64 semantic parity.
- Existing Document Publish v0 handles initial Version #1 publication. Its approved design explicitly defers Version #2+ publication and current-version replacement to Document Versioning.

## Design boundary

- `spec/` remains normative. The approved Document Semantic Inspection v0 design §18 carries forward one WORKING Version per Document, a base from the current PUBLISHED Version, repository-transaction version numbering, caller UUIDv7 operation IDs, Document revision increments, immutable PUBLISHED Versions, ContentItems, and reuse of the Publish operation ledger/API.
- The next design must preserve the separate Semantic Inspection and Search Extraction paths, avoid a durable common content IR, and require successful inspection of every authoritative ContentItem.
- User-selected design scope includes withdrawal and scheduled publication. The user requested inline execution in this session, so no worker is dispatched. The exact route for any later worker is `gpt-6-sol` with `ultra` effort.
- Withdrawal of the current PUBLISHED Version remains a pending business decision. The current recommendation is to set `current_version_id` to null and never auto-publish an old Version; this also blocks new WORKING creation until a current PUBLISHED base is explicitly restored.
- An inline architectural proposal has been presented: extend the existing Domain/Application/PostgreSQL and Publish ledger/API, preserve immutable authoritative FileObjects and DSI inspection for every ContentItem, and execute durable scheduled publication through the same idempotent Publish transition with due-time revalidation. The user has not yet approved that proposal.
- No Design/profile amendment is proposed. There is no approval to write production Versioning code yet.

## Next exact action

Receive the user's current-withdrawal rule and response to the presented architectural proposal. Incorporate both, then write and review the Versioning design spec only after conversational approval. Seek explicit written-spec approval before preparing a separate implementation plan; seek plan approval before implementation.
