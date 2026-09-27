# Document Versioning v0 — Execution Status

- Status: **ACTIVE — DESIGN DISCOVERY**
- Baseline: `main@09c8235755573d16e09b8af029c22dc27caf6472`
- Active branch: `design/document-versioning-v0`
- Implementation: **not started**; no Versioning design spec or implementation plan has been approved.

## Completed prerequisite

- Document Semantic Inspection v0 production Tasks 1–14 are complete. PR #10 merged into `main` as `09c8235755573d16e09b8af029c22dc27caf6472` from final PR head `12047186787e2380bbc6ec74c4baa617b71b2525`.
- Exact merge-head CI `36281613991`: **SUCCESS**, including Ubuntu Rust tests and macOS Intel/arm64 semantic parity.
- Existing Document Publish v0 handles initial Version #1 publication. Its approved design explicitly defers Version #2+ publication and current-version replacement to Document Versioning.

## Design boundary

- `spec/` remains normative. The approved Document Semantic Inspection v0 design §18 carries forward one WORKING Version per Document, a base from the current PUBLISHED Version, repository-transaction version numbering, caller UUIDv7 operation IDs, Document revision increments, immutable PUBLISHED Versions, ContentItems, and reuse of the Publish operation ledger/API.
- The next design must preserve the separate Semantic Inspection and Search Extraction paths, avoid a durable common content IR, and require successful inspection of every authoritative ContentItem.
- Initial scope for withdrawal and scheduled publication is pending user selection. Managed-worker model and reasoning effort are also pending user selection under `AGENTS.md`.
- No Design/profile amendment is proposed. There is no approval to write production Versioning code yet.

## Next exact action

Resolve the two pending selections; inspect only the relevant existing Publish, Application, repository, and Semantic Inspection contracts; present the smallest coherent Versioning design for approval. After design approval, write and review the design spec, then prepare a separate implementation plan and request its approval before implementation.
