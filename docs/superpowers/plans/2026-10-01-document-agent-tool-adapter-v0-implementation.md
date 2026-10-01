# Document Agent Tool Adapter v0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task-by-task.

**Goal:** Expose nine read-first tools through actual MCP stdio against the real fixed-Agent Document API.

**Architecture:** TypeScript thin presentation adapter uses a dedicated generated API client. Server owns identity, business logic, authorization and audit.

**Tech Stack:** Existing Node `24.21.0`, pnpm `12.4.1`, TypeScript `6.0.3`; official SDK subject to exact qualification.

**Spec:** [C2 design](../specs/2026-10-01-document-agent-tool-adapter-v0-design.md)

**Authority:** [C2 bounded authority record](../specs/2026-10-01-document-agent-tool-adapter-v0-approval.md); SDK selection/qualification is pending, not waived by this record.

## Global Constraints

- No DB/Application imports, mutation tools, full-byte/text tool, Search/RAG, HTTP MCP or remote authentication.
- API only through `@knowledge-platform/document-api-client`; OpenAPI 3.2.1 unchanged.
- `KP_DOCUMENT_API_BASE_URL=http://127.0.0.1:8081/` default; startup-frozen destination, no per-tool identity/destination override.
- SDK exact pin after official/current/license/security/transitive/Node evidence. Stop for policy mismatch.
- Existing GUI license exception is graph-specific, not a blanket MCP dependency waiver.
- Design/Plan in A1, a docs-only Draft stacked on R1. A2 implementation must incorporate both verified C1 runtime R2 and A1 documentation; no merge/deploy. C0 closure and separate scheduler qualification remain incomplete.

## Review Focus

- MCP accidentally configured to human endpoint must not silently gain human authority (A2).
- API returns Partial/Unknown/unavailableLegacy; tool preserves exact semantics (A2).
- Cursor/list and comparison arguments cannot trigger unbounded auto-fetch (A2).
- stderr/stdout and network errors cannot leak source content/credentials or corrupt protocol (A3).
- Denied known IDs remain denied after an earlier permitted read/revocation (A3).

## Task A1: SDK qualification and executable package boundary

Files: `docs/research/document-mcp-sdk-qualification.md`; `apps/document-mcp/{package.json,tsconfig.json,src/main.ts}`; pnpm lock/workspace configuration where required.

- [ ] Before dependency installation, inspect official SDK release/registry/upstream maintenance, license tree, advisories and Node engines. Record source URLs, exact version, timestamp, lock hash and result. No invented version or inherited exception.
- [ ] RED packaging smoke: `pnpm --filter @knowledge-platform/document-mcp build` produces a Node-runnable artifact resolving the existing `.ts`-exporting generated package; protocol import compiles on pinned Node/TS.
- [ ] Implement package and minimal stdio start using SDK's qualified actual API; choose exact entrypoint/export names from verified SDK, not guessed contracts. No extra server/transport dependencies beyond necessary graph.
- [ ] GREEN build/import/start/close smoke, frozen install, peer/license/security checks. Commit reviewed qualification and package only after authorized.

## Task A2: Typed client and exact nine-tool registry

Files under `apps/document-mcp`: `src/config.ts`, `src/api.ts`, `src/tools.ts`, `src/errors.ts`, `test/tools.test.ts`, `test/architecture.test.ts`; modify `packages/document-api-client/src/index.ts` to re-export the existing generated `Client`, `createClient`, `createConfig` from `./generated/client`, with a public-export type test. Do not modify generated output.

Interfaces:
- `loadConfig(env: NodeJS.ProcessEnv): McpConfig` with frozen `apiBaseUrl` only; no identity overrides.
- `createDocumentClient(config: McpConfig): Client` uses generated exported client type.
- `verifyAgentSession(client: Client): Promise<void>` consumes generated getSession result.
- `registerDocumentTools(server: QualifiedSdkServer, client: Client): void`; `QualifiedSdkServer` means the exact imported SDK type determined in A1, not a custom wrapper DTO.
- `toToolError(problem: unknown): QualifiedSdkToolResult` maps stable API error information, never uncensored response dump.

- [ ] RED tool catalog equals the nine names in Design; every name invokes exactly its mapped generated operation, with preserved args/response. Assert no raw fetch/API URL construction in tool handlers and no business DTO duplicates.
- [ ] RED config and session: malformed URL/credentials/fragment rejected, human session rejected, request URL/identity fields rejected, redirects cannot move destination. Session preflight and every tool call receive an explicit abort deadline through the generated client; verify client > API > dependency budget hierarchy and cancellation. Test both preflight and tool requests against an upstream that accepts but never responds; generated fetch and server timeouts alone are insufficient.
- [ ] RED boundary samples: max page/+1, opaque cursor round-trip, metadata legacy unavailable, Diff Same/Different/Unknown × Full/Partial/None, truncation and RFC9457 forbidden/not-found/conflict/timeout are preserved. Invalid arguments rejected before API call.
- [ ] Run package tests and record RED. Implement small registries and SDK input validation from existing operation schemas; no generic business framework or auto-pagination.
- [ ] GREEN `pnpm --filter @knowledge-platform/document-mcp test` and `typecheck`; generated API contract tests remain green. Commit reviewed slice.

## Task A3: Actual stdio + real Agent runtime acceptance

Files: `test/runtime.test.ts`, `test/stdio.test.ts`; shared C1 process harness; `mise.toml` task `document:poc:agent`; execution `document-agent-tool-adapter-v0-status.md`.

- [ ] RED actual SDK client subprocess initializes, tools/list discovers exact surface, then root→folder→documents→detail→revisions→history→both comparisons→file metadata against real C1 agent HTTP. Record startup binary paths/hashes/head, DB/storage run IDs, SDK lock hash; no mock HTTP accepted as this evidence.
- [ ] Include inaccessible ID, list filtering, policy revoke after prior success, server outage, invalid args, transport cancellation and EOF/termination. stdout must parse exclusively as MCP messages; logs/stderr must not include document content/secret.
- [ ] Implement only missing adapter glue; business failures are returned faithfully. Any needed new API business semantic is STOP.
- [ ] GREEN `mise run document:poc:agent`; compare selected IDs/major.minor/metadata/file summaries with Human API/GUI for the same real state. No file downloads through MCP.
- [ ] Run full affected local gates, publish A2 draft after review, and check required hosted gates on exact pushed head. Any evidence-only new commit needs exact-head checks again.

Completion requires A1 policy qualification and actual stdio/real API evidence, not merely unit tests. Scheduler STOP does not prevent read tools from qualifying but does prevent claiming all C1/C3 complete.
