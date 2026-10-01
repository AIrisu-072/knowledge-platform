# Document Platform PoC Acceptance / Evaluation v0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task-by-task. This is an evidence/harness capability, not authorization to add product semantics.

**Status:** PLAN ONLY / NO ACCEPTANCE EVIDENCE. The separate E0 Draft contains this plan and status; E1 remains the later evidence PR after actual C1/C2 execution.

**Goal:** Establish real-runtime Human/Agent same-state behavior and document usability, failure/restart and next-step findings.

**Architecture:** An isolated process harness drives built Human GUI, both real servers, generated client, actual MCP stdio and the separate scheduler against one disposable PostgreSQL/FileSystemStorage dataset and real production workers.

**Tech Stack:** Approved C1/C2 stack, existing Playwright and hosted gates.

**Spec:** [C1 design](../specs/2026-10-01-document-poc-runtime-v0-design.md), [C2 design](../specs/2026-10-01-document-agent-tool-adapter-v0-design.md), requester’s C3 fixed acceptance scope.

## Global Constraints

- Synthetic data only. No production migration/deploy/identity/TLS/DNS/HA/backup; no Search modifications.
- Browser/API/stdio observations must belong to the same actual database/storage/run and exact head.
- Fake backend suites and fixture-assembled HTTP tests remain separate evidence classes.
- No invented latency, usability or security SLO. Report observed numbers, limits, environment and methodology.
- C0/C1/C2/C3 are separately reported. Never label unresolved scheduler identity or absent environment qualification as green.

## Review Focus

- Concurrent metadata or ACL changes after capability projection do not create false success (E1).
- Agent equality comparison accidentally includes data it is forbidden to see (E1).
- Worker crash and partial comparison are mistaken for no differences (E2).
- Restart uses new storage or fresh DB and falsely claims persistence (E2).
- Browser journey or screenshot silently uses mocked routes/older head (E3).

## Task E1: Human→Agent same-state and authorization evidence

Files: `apps/document-web/e2e-runtime/human-agent-consistency.spec.ts`; MCP runtime harness; `docs/superpowers/execution/document-poc-acceptance-v0-report.md`.

- [ ] RED assertions across real browser/API/stdio: human creates/publishes initial version → 1.0; metadata actual change → minor; no-op → unchanged; new published content → new major; withdrawal fallback → monotonic major. Use approved GUI semantics, not independently invented counters.
- [ ] Compare document/version/revision IDs, metadata and file summaries after each mutation using Agent tools. Restrict equality assertions to fields/actions that Agent is authorized to see; compare current shared state, not unrelated UI presentation details.
- [ ] Explicit human-only fixture must be absent in list and denied by known-ID detail/history/revision/file/comparison tools. Revoke existing permitted access and confirm subsequent calls deny it. Agent HTTP mutation lacks permission and MCP catalog has no mutation tool.
- [ ] Exercise current authorization/capability race and stale OCC conflict; human GUI retains actionable conflict state and does not claim success before authoritative response.
- [ ] GREEN actual process tests and evidence capture, including protocol request/response shapes with synthetic-only data and no credentials. Report failure as failure, not product fix authorization for new semantics.

## Task E2: Failure, lifecycle and persistence matrix

Files: process harness fault controls and runtime tests; same report/runbook.

- [ ] Record startup validation failures (config/mode/profile/bind/schema/worker/storage/dist), ready false/recovery and secret-free health/diagnostics.
- [ ] Stop/restart both servers without replacing DB/storage. Verify known row IDs, revisions, authoritative file content hashes and file audit contract. Include interrupted mutating request with existing operation-ID recovery; do not rerun under a new operation ID blindly.
- [ ] Induce real DSI and Diff worker failure using isolated test process/file controls; verify bounded error/Unknown/Partial behavior where existing contract specifies, no fake unchanged, no leaked fragment, no unsandboxed fallback. Restore and verify recovery.
- [ ] After scheduler identity STOP is resolved, launch existing scheduler separately: schedule future publication, stop before due, restart after due, observe exactly one permitted execution and persisted lifecycle/revision/audit. Revoke requester before due and verify denial with separate executor attribution. Until then label scheduler NOT QUALIFIED/STOP.
- [ ] Test SIGTERM draining, including a stalled download that remains draining until its test client is released/cancelled; do not report bounded shutdown for that path or introduce a force-close policy without the C1 decision gate. Test actual MCP EOF/cancel/outage and client-side preflight/tool abort deadlines against an accepted-but-never-responding upstream. Record operational timeout configuration, observed timings and environment without inventing business SLOs.

## Task E3: Human/Agent evaluation, report and final exact head

Files: acceptance report/status; `docs/operations/document-poc-runtime-v0.md`; evidence workflow additions only if required.

- [ ] Walk Human usability: folder/list/detail, understanding Revision versus Version, Diff/new-old comparison, version creation, publish, AccessPolicy UI, error/conflict recovery; retain keyboard/focus/reduced-motion/1280+1440 checks. Distinguish automated assertions from human-review observations; do not invent a user study or scores.
- [ ] Evaluate actual MCP tools/list descriptions and arguments, API problem clarity, revision/Diff discovery and authorization. State which paths were scripted versus observed with an actual agent/client; do not call SDK protocol smoke an LLM usability study.
- [ ] Report status per matrix row as PASS/FAIL/BLOCKED/NOT RUN, exact commit, workflow/run URLs, OS/tool versions, lock hashes, fixture hash, binary/artifact provenance, ports and DB/storage run identity. Avoid credentials/actual documents. Attach screenshot/trace artifact IDs in the repository report where publishable synthetic artifacts exist.
- [ ] Inspect current repository Search state read-only at C3 end; list branch/head/spec/plan/status and next integration capability. Do not checkout/mutate another worker's tree, infer implementation from chat, or copy Search code into this stack.
- [ ] Prepare only Production Identity questions: domain/Entra/hybrid join, tickets, browser/WIA policy, server OS/location, DNS/reverse proxy, realm/SPN/service account/keytab handling, user/department groups/mapping and Agent/service identity. Never request secret values in chat or design the production adapter before answers.
- [ ] Final report must contain: C0/C1/C2/C3 status; PR stack; exact heads; hosted/local gates; composition; fixed identity; instances/shared state; health; migrations/runbook; real Human journey; nine MCP tools; real Agent results; consistency; limits; identity questions; Search next step; unmerged/undeployed PRs; blockers.
- [ ] Publish E1 evidence draft only after parent review and existing publication authority; verify exact-head hosted gates after all docs changes. Leave all PRs unmerged and not deployed.

## Acceptance ownership matrix

| Requirement | Owning task | Evidence required |
|---|---|---|
| Safe startup/profile/network/redaction | C1 R1–R3 / E2 | process tests and logs |
| Explicit migration/schema compatibility | C1 R2 | real disposable PostgreSQL, unchanged serve schema |
| Root bootstrap/API seed/idempotency | C1 R4 | real API IDs, repeated seed invariants |
| Production composition + GUI journey | C1 R6 / E1 | real binary HTTP + Playwright, no route mocking |
| Separate scheduler | C1 R5 / E2 | identity decision then process/transaction/audit evidence |
| Read-only MCP/generated client/stdio | C2 A1–A3 | SDK qualification + actual stdio client |
| Same state after human mutation | E1 | cross-channel IDs/revisions/metadata |
| Denied resource and policy revoke | C2 A3 / E1 | API+MCP negative cases/list filtering |
| Persistence/restart/worker/health | E2 | retained DB/storage hashes, failure/recovery |
| Usability and performance observations | E3 | named method/platform and observed values only |
| Search next capability/identity questions | E3 | current read-only evidence/questions only |

C3 cannot complete until material STOP items and missing real-runtime evidence are resolved. No amount of documentation or old successful CI can replace an unexecuted required acceptance path.
