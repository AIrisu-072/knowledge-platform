# Document Agent Tool Adapter v0 — Bounded Authority Record

Date: 2026-10-01 UTC

Status: FIXED REQUIREMENTS AUTHORIZED BY REQUEST §30; SDK qualification and implementation NOT STARTED.

## Source and scope

The requester's Document Platform PoC Runtime / Server Composition / Agent Integration instruction of 2026-10-01, §§16–24 and §30, explicitly authorizes faithful design/plan transcription followed by implementation of:

- MCP over stdio in `apps/document-mcp`, using the generated `@knowledge-platform/document-api-client`
- only the configured Common Document HTTP API and the process-fixed `poc-agent` server identity
- the nine read-first tools mapped in the design; no Agent mutation surface
- existing authoritative metadata, revisions, history, comparison/display and file metadata
- no separate business API, direct DB/Application access, full-text/raw-byte endpoint, Search/RAG integration or production authentication
- official/current SDK qualification, exact pinning and full dependency/license/security/Node compatibility checks before promotion
- actual stdio and real Agent server acceptance against the same state as Human GUI

The requester requires separate A1 Design+Plan and A2 implementation PRs (§29), RED → GREEN → local → exact-head hosted verification (§32), and no merge/deploy. This record documents existing bounded authority. It does not approve a not-yet-selected SDK or grant a new license exception, API semantic change, production access or future Agent write tools.

## Reviewed document identities

The exact Git blob IDs below identify the design/plan reviewed for fidelity to the fixed request; they are not a claim of an additional user blob-by-blob approval. Subsequent semantic changes require renewed review and any required approval.

- Design: `docs/superpowers/specs/2026-10-01-document-agent-tool-adapter-v0-design.md`
- Design Git blob: `9d61d2d60949f256f7de2a29e924618a2682424c`
- Plan: `docs/superpowers/plans/2026-10-01-document-agent-tool-adapter-v0-implementation.md`
- Plan Git blob: `784c265d18ca0eb65e02ef92161a265fc8dc7922`

## Prerequisites and exclusions

- A1 is documentation only, stacked on R1. A2 must incorporate verified real-runtime R2 and A1; docs-only publication cannot qualify Agent or Human/Agent consistency.
- The GUI's candidate-specific dependency/license exceptions do not apply to the MCP dependency graph. STOP for any policy mismatch or required new business semantics.
- C1 scheduler executor identity remains unapproved; this authority does not select it. Scheduler work and C1/C3 completion remain blocked until its separate decision and acceptance.
- C0 closure and real-backend browser evidence remain incomplete. Successful inherited CI is not new-head runtime acceptance.
