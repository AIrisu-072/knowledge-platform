# Document Agent Tool Adapter v0 — C2 Design

Status: FIXED-SCOPE DESIGN / PLAN REVIEW; official SDK qualification, implementation and acceptance NOT STARTED.

Approval boundary: [C2 bounded authority record](2026-10-01-document-agent-tool-adapter-v0-approval.md).

## Goal and fixed authority

Expose a small read-first MCP-over-stdio presentation adapter for the existing Common Document HTTP API. The requester’s 2026-10-01 §16–24 and §30 fix protocol, generated-client reuse, nine read tools, endpoint and scope. This design does not introduce a new business API, identity mechanism or extraction system.

Prerequisite: [C1 design](2026-10-01-document-poc-runtime-v0-design.md). Execution: [C2 plan](../plans/2026-10-01-document-agent-tool-adapter-v0-implementation.md). Cross-channel evaluation is delivered in a separate C3 plan-only Draft. This A1 documentation branch stacks on R1; A2 implementation must incorporate a verified R2 real runtime before acceptance. C0 closure remains pending and scheduler executor selection remains unapproved.

## Boundary and dependencies

Create TypeScript `apps/document-mcp`, consuming `@knowledge-platform/document-api-client`. The adapter may depend on the official MCP SDK after qualification and existing workspace tooling. It must not depend on Rust Application/Repository, access DB/filesystem storage, reproduce business DTOs/ACL/lifecycle/Diff rules, or call GUI implementation internals. Compile the shared client into the adapter artifact as needed: its current package exports `.ts`, so plain Node cannot be assumed to load it unchanged. Reuse existing TypeScript/build tooling, not a new bundler unless separately qualified.

Only stdio transport; no listener, remote MCP authentication or generic agent framework. stdout is exclusively MCP protocol; diagnostics go to stderr and contain no document fragment, secret, full endpoint credential or raw API body.

At implementation start verify official/current maintained SDK and stable API using official upstream and registry metadata; record exact version, provenance, license including transitives, advisories, Node `24.21.0` compatibility, required features and lock hash. Do not guess a pin now or promote a POC REQUIRED dependency. Candidate-specific GUI exceptions do not approve the SDK's dependency graph. A policy mismatch is STOP, not a reason to choose an unofficial SDK silently.

## Endpoint and identity

`KP_DOCUMENT_API_BASE_URL` defaults to `http://127.0.0.1:8081/`; resolve and freeze at process startup. Validate HTTP(S) URL, disallow embedded credentials and fragment, and do not accept a URL/identity override in tool arguments. The document server owns identity. No identity headers, cookies, group fields, tokens or principal selection in MCP. During preflight call generated `getSession` and verify the intended PoC Agent session; configuration pointing to the human port must fail rather than using stronger authority silently.

Use a dedicated generated client instance, never shared mutable global configuration. The package currently does not re-export the generated Client/createClient/createConfig facilities at its public root; a small hand-maintained `src/index.ts` re-export from `./generated/client` is an allowed packaging change, without editing generated files or duplicating DTOs. Disable automatic redirects that could change the configured API destination. Bound session preflight and every tool HTTP call with an explicit client-side abort deadline passed through the generated client signal; its current fetch implementation supplies no timeout by itself. Follow the existing client > API > dependency timeout hierarchy, document the operational values during qualification, and propagate transport cancellation. Test connection/preflight hangs and an upstream that accepts but never responds. Server budgets alone cannot bound those cases; these limits are operational safeguards, not an invented business SLO. Do not automatically retry comparison POSTs or hide ambiguous/partial results.

## Tool-to-API mapping

Each invocation calls one generated operation and returns its actual typed response without semantic normalization. Tool JSON Schema describes arguments using the existing operation's request field names, required fields, enums and bounds; contract tests compare it to OpenAPI/generated types rather than hand-maintaining business DTOs.

| MCP tool | Generated operation | Scope |
|---|---|---|
| document_get_root | getRootFolder | root metadata |
| document_list_folder | listFolderChildren | children; existing cursor/page limits |
| document_list | listDocuments | existing mode/filter/sort/cursor semantics |
| document_get | getDocument | authorized detail and capabilities |
| document_list_revisions | listDocumentRevisions | issued revisions, existing paging |
| document_get_history | getDocumentHistory | authorized history |
| document_compare_versions | compareDocumentVersions | bounded comparison/display projection |
| document_compare_revisions | compareDocumentRevisions | bounded content + metadata comparison |
| document_list_files | listVersionFiles | file metadata only |

Comparison operations use POST but do not expose document mutation. Existing comparison cache/audit side effects remain owned by the API. The adapter's read-only characterization means no create/update/publish/schedule/ACL mutation tool, not a promise that the server writes no audit/cache record. No download/full-byte or LLM raw-text tool. List calls return one bounded page and preserve opaque cursors; no unbounded auto-pagination.

Descriptions must distinguish document OCC revision, content version identity and human Major.Minor Revision, and describe required version/revision pair and mode. They must say that Diff Unknown/Partial/None/truncation is incomplete evidence, not unchanged, while returning those fields unaltered. Metadata unavailable_legacy remains unavailable.

Return the generated API value as structured MCP content when supported by the qualified SDK, with a JSON text representation for standard clients. Preserve RFC9457 code/status/trace correlation on API failure as tool error content; invalid MCP arguments use protocol validation. Redact response content only where existing security policy requires it, not to hide incomplete comparison results. Do not put arbitrary upstream HTML/network diagnostics into output. Tool annotations describe read-only/non-destructive intent and are never treated as authorization.

## Verification and limitations

Actual MCP client connects through an actual subprocess stdio transport and walks root → children → documents → detail → revisions → history → comparisons/file metadata against the real agent server. Compare IDs/revisions with human API/GUI on the same database/storage. Known human-only IDs must stay denied, and inaccessible resources must not appear through list/history/comparison/file tools. After human ACL revocation, current reads remain denied; no adapter cache is authoritative.

Test tools/list exact surface, argument errors, cursor bounds, unknown/partial preservation, server unavailable, API forbidden/not-found/conflict/timeout, protocol-safe stdout, cancellation and process exit. Client text/structured results must agree. Full Search/Q&A, Agent writes, raw full text and production authentication remain separate capabilities.
