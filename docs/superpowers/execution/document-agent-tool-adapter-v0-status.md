# Document Agent Tool Adapter v0 — Capability Status

## 2026-10-01 UTC — A1 documentation publication checkpoint

- Status: DESIGN / PLAN ONLY. SDK SELECTION / QUALIFICATION NOT STARTED; PRODUCT IMPLEMENTATION NOT STARTED; ACTUAL STDIO / REAL-SERVER ACCEPTANCE NOT RUN.
- A1 branch: `design/document-agent-tool-adapter-v0-a1`, docs-only stack on `design/document-poc-runtime-v0-r1`. See its Draft PR for the exact base/head and hosted results.
- [Design](../specs/2026-10-01-document-agent-tool-adapter-v0-design.md), [implementation plan](../plans/2026-10-01-document-agent-tool-adapter-v0-implementation.md), [bounded authority record](../specs/2026-10-01-document-agent-tool-adapter-v0-approval.md).
- Nine planned tools: `document_get_root`, `document_list_folder`, `document_list`, `document_get`, `document_list_revisions`, `document_get_history`, `document_compare_versions`, `document_compare_revisions`, `document_list_files`. No tool implementation is delivered here.
- Current exact action: verify documentation publication and hosted head; start Task A1 SDK qualification within policy, then separate A2 implementation. A2 real-runtime acceptance requires verified C1 R2 plus A1. Preserve actual client abort deadlines and protocol-safe stdout.
- C0 closure is pending. C1 scheduler STOP remains unapproved; read-only MCP preparation does not select an executor or make C1/C3 complete.
- Verification in this PR: independent requirements review, generated-operation mapping inspection, link/scope/private-context checks, blob identification and `git diff --check`. Product tests, installation, SDK audit, real API and actual stdio are NOT RUN. New-head hosted status must come from the Draft PR, not inherited green results.
- All delivery stays Draft, unmerged and undeployed. No production identity, Search implementation or Agent writes.

## STOP record if SDK qualification fails

Record blocker, exact dependency/graph/license/security evidence, candidate and lock hash, last verified GREEN head (none for C2 acceptance yet), required decision and next exact action. Do not inherit the GUI's graph-specific exceptions or silently replace the official SDK with an unqualified dependency.
