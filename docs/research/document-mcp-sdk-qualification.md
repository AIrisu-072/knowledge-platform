# Document MCP SDK qualification — bounded metadata gate

Observed: 2026-10-01 UTC. Current status: bounded license exception APPROVED; artifact integrity/notices, Node24 stdio and product TypeScript6 compilation qualified. Real Document runtime acceptance is still PENDING. Historical gate observations below are retained chronologically.

## Candidate and sources

Official stable v2 documentation: https://ts.sdk.modelcontextprotocol.io/v2/
Official registry exact candidates: @modelcontextprotocol/server 2.2.0 and @modelcontextprotocol/client 2.2.0, transitive core 2.2.0. Direct runtime should use server; client is for the actual stdio test harness. All three registry artifacts declare MIT and Node >=20. Current upstream main manifests declare Apache-2.0 for core/client; both are permitted, but artifact licenses must still be inspected rather than equating main with published bytes.

Registry metadata is saved in this directory with exact versions and dist integrity. No lifecycle scripts were executed. A standalone package outside the repository was resolved by pinned pnpm12.4.1 using install --lockfile-only --ignore-scripts. Node24.21.0 meets declared engines; executable compatibility is NOT yet verified.

## Exact dependency graph

- @modelcontextprotocol/client@2.2.0: MIT
- @modelcontextprotocol/core@2.2.0: MIT
- @modelcontextprotocol/server@2.2.0: MIT
- cross-spawn@7.0.6: MIT
- eventsource-parser@3.1.1: MIT
- eventsource@3.0.7: MIT
- isexe@2.0.0: ISC
- jose@6.2.12: MIT
- path-key@3.1.1: MIT
- pkce-challenge@5.0.1: MIT
- shebang-command@2.0.0: MIT
- shebang-regex@3.0.0: MIT
- which@2.0.2: ISC
- zod@4.6.5: MIT

## Initial license blocker (resolved by the bounded approval below)

Repository spec/selection/library-tool-selection-v0.md §2.1 and architecture contract do not list ISC as approved. The current GUI exception is graph-specific. SDK client → cross-spawn7.0.6 → which2.0.2 → isexe2.0.0 introduces two ISC packages. Do not reuse the GUI approval, remove the actual SDK client acceptance, or silently replace the official SDK.

Required decision: a bounded ADR exception for exactly which2.0.2 and isexe2.0.0 in this MCP client test-harness dependency graph, preserving their license notices and prohibiting an automatic blanket ISC allowance. If refused, keep A2 blocked and research a separately approved approach.

## Security

Pinned pnpm audit --json exited0 with zero advisories across14 resolved dependencies. audit.json retained. This is time-specific registry evidence, not an absolute security guarantee.

Official GHSA-6qxp-vccf-f47h (published September30): client >=2.0.0,<2.2.0 affected,2.2.0 patched; stdio clients expressly not affected. Source https://github.com/modelcontextprotocol/typescript-sdk/security/advisories/GHSA-6qxp-vccf-f47h . No remote OAuth client is required here.

## Initial remaining qualification

After approval, verify published package integrity and shipped licenses, frozen install without scripts, Node24.21.0/TS6.0.3 compilation/import, actual stdio initialize/tools/list/close, and affected repository gates. No product code or product lockfile was changed. No server socket, credentials, persistent access, or user documents involved.

Last GREEN: isolated lock resolution + registry audit only; no A2 runtime GREEN head.
Next exact action: obtain bounded license exception, then continue artifact license/integrity and stdio package qualification before product integration.

Lock SHA256: b18ab2388ca4c4e2e374255393eca59ed9fe12717f3c0ce19f7bb891cd5f4a5c

## Approved continuation and artifact smoke — 2026-10-01 22:00 UTC

User message Sentinel_c975c8b0e8548191b5aacfdec160a2b1 approved the specifically requested which2.0.2 and isexe2.0.0 ISC exception, retaining license notices. It does not approve unrelated exceptions. The repository owner will record bounded ADR authority.

Frozen pnpm12.4.1 install --ignore-scripts passed supply-chain lock checks for14 entries. Installed artifacts retain individual license files; hashes in license-file-hashes.json. SDK shipped LICENSE explains MIT→Apache-2.0 transition: new code Apache-2.0, prior unconsenting contributions retain MIT; documentation excluding specification CC-BY-4.0. Both code licenses are permitted. Preserve full notices; do not import documentation prose into product or silently label all shipped SDK material MIT.

Actual Node24.21.0 subprocess stdio smoke PASS: initialize, tools/list exact synthetic_ping, tools/call returns pong, cleanclose, empty stderr. Files smoke-server.mjs/smoke-client.mjs and smoke.log retain evidence. This uses only synthetic protocol data, no HTTP, DB, credentials or document content. It proves SDK runtime transport compatibility, not Document tool implementation or fullTScompilation. TypeScript6.0.3 compile of product adapter and generated-client artifact remains a product gate.

Next action: record exact graph exception and qualification in A2 docs; implement/review the approved nine-tool adapter; run product compiler and actual server acceptance.

## Product integration continuation

2026-10-01: approved exception recorded in `docs/decisions/2026-10-01-document-mcp-client-license-exception.md`. Pinned TypeScript6.0.3 compiles the adapter/shared generated client; existing Webpack5.111.1 and Babel7.29.7 produce Node24 CommonJS executable artifacts. SDK dependencies stay external (published require exports), preserving complete package notices. No new bundler or generated DTO modification. Workspace lock addition changes no existing package versions. Full14-package license files are copied byte-for-byte into apps/document-mcp/third-party-notices with SHA256 manifest. This includes SDK LICENSE transition notice and shipped documentation-license text, without importing SDK documentation into product functionality.

Operational safeguards: ordinary/session HTTP deadline35s > API30s; comparison50s > API45s > production diff worker budget. Every request carries an AbortSignal; MCP cancellation is combined with its deadline. Redirects are rejected and cookies omitted. No retries. These are bounded client operational deadlines, not business SLOs.

Local focused transport tests use synthetic HTTP only for isolation; actual Document acceptance is a separate mandatory hosted gate. A passing synthetic transport test never qualifies real backend acceptance.

## Contract-validation reuse

The adapter reuses already-selected Ajv8.20.0 plus existing locked ajv-formats3.0.1, rather than narrowing API UUID/RFC3339 formats to Zod's stricter variants. Existing workspace transitive packages reused by this validator are fast-deep-equal3.1.3 (MIT), fast-uri3.1.8 (BSD-3-Clause), json-schema-traverse1.0.0 (MIT), require-from-string2.0.2 (MIT). Ajv and ajv-formats are MIT. These six package versions already existed in the workspace lock; no new exception or package upgrade is introduced. Their complete notices are preserved alongside the fourteen SDK graph packages. The original SDK qualification graph remains exactly14; the adapter also uses this separately existing, qualified validator graph.
