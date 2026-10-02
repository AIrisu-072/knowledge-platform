# ADR: bounded MCP test-client ISC exception

Date: 2026-10-01 UTC
Status: explicitly approved, bounded to the graph below

## Decision and authority

The requester explicitly approved the assistant’s question identifying only `which@2.0.2` and `isexe@2.0.0` as ISC dependencies required for official MCP SDK executable qualification, on condition that their license notices remain. User reply: 「例外承認します」 (message Sentinel_c975c8b0e8548191b5aacfdec160a2b1). This is the explicit ADR exception required by `spec/selection/library-tool-selection-v0.md` §2.1 and Architecture Contract LINT-02.

The approved edge is `@modelcontextprotocol/client@2.2.0 → cross-spawn@7.0.6 → which@2.0.2 → isexe@2.0.0`, used by the actual stdio test harness. No general ISC allowlist change, unrelated package, dependency upgrade, runtime permission, production authentication or license waiver is approved. The pre-existing GUI exception is not used as authority.

## Reproducibility and notices

Isolated14-package SDK lock SHA256: `b18ab2388ca4c4e2e374255393eca59ed9fe12717f3c0ce19f7bb891cd5f4a5c`. Exact graph and sources are in `docs/research/document-mcp-sdk-qualification.md`; the product workspace pnpm lock pins the same artifacts. Full shipped license notices and hash manifest are in `apps/document-mcp/third-party-notices/`. SDK artifacts report MIT but include the MIT/Apache-2.0 transition; retain their full shipped notices rather than relabeling all material.

## Consequences and review gate

The official SDK client remains a development/test dependency. The server runs stdio only. Any additional non-allowlisted license or change to the two approved package versions requires renewed qualification and approval; no silent graph expansion. No legal acceptance on a third-party service is involved.
