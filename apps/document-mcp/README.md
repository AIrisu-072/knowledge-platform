# Document Agent Tool Adapter v0

Read-only MCP over stdio. The generated Document Common API client is the only backend boundary. Start `document-server` separately using the fixed `poc-agent` profile; Human and Agent instances use the same owned PoC DB/storage. Never use production data or identities.

```sh
pnpm install --frozen-lockfile --ignore-scripts
pnpm --filter @knowledge-platform/document-mcp build
KP_DOCUMENT_API_BASE_URL=http://127.0.0.1:8081/ node apps/document-mcp/dist/main.cjs
```

Node24.21.0 is pinned. The default API endpoint is `http://127.0.0.1:8081/`. Configuration is frozen at startup, URLs with credentials/fragments are rejected, redirects are disabled, no cookies/identity headers are sent, and startup verifies the API returns provider `poc`, principal `poc-agent`, invocation `agent`. Static PoC identity is not production authentication. stdout is only MCP; startup failure diagnostics are generic stderr.

Tools: `document_get_root`, `document_list_folder`, `document_list`, `document_get`, `document_list_revisions`, `document_get_history`, `document_compare_versions`, `document_compare_revisions`, `document_list_files`. Each calls its matching generated operation once. Arguments use API field names; lists return one bounded page and an opaque cursor. Comparisons use API POST, with server-owned cache/audit side effects, without exposing a mutation tool. `tools/list` describes required fields, enum values and bounds.

The API owns authorization and semantics. OCC revision, content Version and issued human Major.Minor Revision are distinct. Unknown/Partial/None/truncation and unavailable legacy metadata remain unchanged; they are never represented as unchanged evidence. No raw text/full-byte download, Search/RAG, Agent write tools, remote MCP authentication or listener is included. Client HTTP deadlines are35s ordinary/session and50s comparison, greater than API30s/45s budgets; MCP cancellation also aborts HTTP. There are no automatic retries.

## Verification

```sh
pnpm --filter @knowledge-platform/document-mcp test
```

Focused tests exercise actual SDK protocol and actual stdio subprocess against synthetic HTTP for boundary isolation. They do not qualify a real Document runtime.

Mandatory real acceptance uses the C1 owned runtime harness context after browser journey and before shutdown:

```sh
KP_POC_RUNTIME_CONTEXT=/path/to/owned-run/context.json node apps/document-mcp/dist/runtime.cjs
```

Context schema: `{runId,human,agent,manifestPath,statePath}`. It requires the saved post-GUI regulation and PDF oracles, checks exact IDs/revisions and confirmed full/nonempty comparison semantics against the Human API (excluding per-call audit IDs). This harness validates exact discovery, reads all nine tools, compares GUI-produced regulation state with Human API, denies known human-only IDs, performs a Human API metadata change on the synthetic sandbox document, then revokes its Agent permission and verifies every read path remains denied. It never changes the regulation/PDF restart oracles. It writes `agent-acceptance.json` beside the context, including source head, executable hashes, workspace lock hash and the owned context hash. After the owner stops the Agent server, `node apps/document-mcp/test/outage.mjs` with the same context proves actual SDK initialization fails against the stopped server; its assertion also fails if the server is still live. Only this controlled test harness imports Human API mutation operations; product adapter entrypoints do not.

Do not call acceptance complete if native sandbox preflight, real workers, actual runtime or hosted gates fail. The separate scheduler identity decision remains open; no scheduler or production behavior is selected here.

## Dependencies and licenses

SDK server/client/core2.2.0 and zod4.6.5 are exact-pinned. Existing TypeScript/Webpack/Babel bundle the shared TypeScript-exporting client; SDK stays external. Full shipped notices are preserved under `third-party-notices/`, including the SDK MIT/Apache-2.0 transition notice. The only new individual exception is test-client `which2.0.2` and `isexe2.0.0` ISC, recorded in the bounded ADR. No general license-policy change applies.
