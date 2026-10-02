# Document Platform PoC Runtime v0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task-by-task. This PR is documentation only. Follow the bounded authority record; scheduler Task R5 uses the bounded 2026-10-02 attribution decision.

**Goal:** Launch two fixed-identity Document servers on real shared state and exercise the Human GUI through the actual composition root.

**Architecture:** A pure Rust composition root assembles existing services/adapters/routers. Migration and bootstrap are explicit commands. Publication scheduling remains a separate process and its fixed executor attribution is isolated from requester authorization.

**Tech Stack:** Existing Rust/Axum/Tokio/SQLx/PostgreSQL/FileSystemStorage/DSI/Diff; approved Webpack React GUI and generated TS client.

**Spec:** [C1 design](../specs/2026-10-01-document-poc-runtime-v0-design.md)

## Global constraints

- Verified publication baseline: PR36 head `b578a9b49338066d0e4ee5495ea1280f991c122b`; C0 closure is pending. Reconcile any later C0 head and exact-head evidence before dependent acceptance.
- No business logic in server; no infrastructure dependencies into Domain/Application/HTTP.
- Only HTTP profiles `poc-human` and `poc-agent`, fixed at startup; production fails closed; default loopback.
- `serve` never migrates/seeds. Explicit migration targets disposable PoC DB only.
- Preserve OpenAPI 3.2.1, authorization/audit/OCC/Revision/Diff semantics and qualified worker sandboxing.
- Coordinate resource-intensive builds with concurrent work; keep changes in an isolated worktree. Review each slice before commit/push.
- Plan implementation uses RED → GREEN → local → exact-head hosted gates; no merge or deploy.

## Review Focus

- Headers/cookies/query pretending to be human on the agent port remain Agent (Task R1).
- Nonloopback/unknown mode/malformed booleans and secrets in failures fail closed/redact (R1).
- Migrated but checksum-changed/partially migrated/newer DB never silently starts or repairs (R2).
- Encoded traversal/symlink escape/unknown API cannot return a secret or SPA success (R3).
- Restart/interrupted seed/unknown mutation completion do not duplicate or overwrite state (R4/R6).

## File and interface map

Create `crates/document-server/{Cargo.toml,src/lib.rs,src/main.rs,src/config.rs,src/identity.rs,src/composition.rs,src/health.rs,src/web.rs,src/bootstrap.rs}` and focused `tests/` targets. Add workspace membership only in R1.

- `RuntimeConfig::from_env(&impl ConfigSource, Command) -> Result<RuntimeConfig, ConfigError>`; `Command::{Serve,Migrate,BootstrapPoc}`. Debug/errors redact secret fields.
- `PoCIdentityProfile::{Human,Agent}`; `StaticPoCIdentityAdapter::new(profile)` implements existing `IdentityAdapter` and produces the fixed subjects. A clock seam is test-only construction, not user identity config.
- `compose_runtime(config: &RuntimeConfig) -> Result<Runtime, StartupError>` owns pool/router/readiness resources; `Runtime::serve(listener, shutdown_future)` drains cleanly.
- Repository `check_schema_compatibility(&PgPool) -> Result<(), SchemaCompatibilityError>` is read-only and shares embedded migration metadata with migrate, avoiding a second migration list.
- `readiness(&RuntimeHealth) -> Result<(), ReadinessFailure>` returns internal safe categories; public HTTP only status JSON.
- `web_router(dist: &Path) -> Result<Router, StartupError>` accepts validated root; outer routing reserves API/health before fallback.
- `bootstrap_poc(&PgPool, profile) -> Result<BootstrapOutcome, BootstrapError>` calls only the existing root policy port; only Human permitted.

Interface names may be adjusted for existing Rust conventions during implementation without changing behavior; a dependency, authority or lifecycle change returns to STOP.

## Task R1: Safe configuration and fixed identity

Files: server manifest/config/identity/main/lib, root Cargo.toml; `tests/config_identity.rs`; architecture rules/tests.

- [ ] Add RED tests `profiles_are_fixed_and_headers_cannot_override`, `unknown_and_production_modes_fail`, `bind_override_is_explicit`, `errors_redact_url_and_paths`, `profile_does_not_expire_at_startup_ttl`. Assert exact provider/principal/group/invocation strings and agent context never gains human subjects.
- [ ] Run `cargo test -p document-server --test config_identity`; record expected missing-interface failure.
- [ ] Implement interfaces above with strict command-specific config. Share selected existing dependencies/features only after inventory; record added Cargo features and graph changes.
- [ ] Run target GREEN and architecture lint. Review stdout/stderr failure captures for secrets.
- [ ] Commit the independently tested slice only after review/publication authorization.

## Task R2: Real adapter composition, schema and command separation

Files: `src/composition.rs`, `src/main.rs`; repository `src/schema_compatibility.rs`/`lib.rs`; `tests/startup.rs`, `tests/migration.rs`.

- [ ] RED: fresh DB `serve` fails without creating schema; explicit migrate succeeds; second migrate preserves state; stale/checksum mismatch/failed/newer migrations reject serve. Missing storage/worker/non-executable worker/native sandbox failure prevents listener. Unavailable DB diagnostics expose no URL.
- [ ] Run `cargo test -p document-server --test migration --test startup` against disposable PostgreSQL; record failures before implementation.
- [ ] Compose production adapters and all existing route families. Read current worker config/PDFium contracts; explicitly wire the qualified PDFium runtime directory into both production DSI and Diff runner configurations and validate it for the PDF acceptance profile. Do not copy fixture-only fake executors. Implement schema read in repository and command dispatch.
- [ ] GREEN target tests; `cargo test -p document-api-http --test contract every_openapi_operation_is_dispatched_to_exactly_one_handler_family`. Capture real Linux runner preflight evidence, no skip-as-pass.
- [ ] Commit reviewed slice. If selected feature adds unqualified dependency/license, STOP with graph.

## Task R3: Health, static GUI and shutdown

Files: `src/health.rs`, `src/web.rs`, `src/composition.rs`; `tests/health_static.rs`, `tests/shutdown.rs`.

- [ ] RED: live stays available during dependency outage; ready 503 on DB/storage/worker/dist invalidity, 200 after recovery. Body has status only. Human `/documents/...` serves built SPA; `/v1/unknown`, `/health/unknown`, POST static, missing assets, dotfiles/maps, encoded traversal and escaping symlinks do not serve index or outside files. Agent `/` never serves GUI.
- [ ] Run `cargo test -p document-server --test health_static --test shutdown`; record RED.
- [ ] Implement reserved route dispatch and vetted static library composition, bounded readiness and signal/drain lifecycle. Reuse selected tower-http static facility only after feature/transitive qualification. Keep API headers/problem contract intact.
- [ ] GREEN tests include SIGTERM during an ordinary request: it completes or preserves existing unknown-outcome recovery semantics; listener stops and the process exits after drain. Add a deliberately stalled download: refuse new work, mark not ready, observe still-draining status, release/cancel the client, then verify exit and resource cleanup. Handler and stream-idle timeouts do not prove a total drain deadline. Record the limitation; do not force-close remaining clients or claim bounded termination without an explicit policy decision. Observe timings without inventing a business SLO.
- [ ] Commit reviewed slice.

## Task R4: Explicit bootstrap and API-driven synthetic seed

Files: server `src/bootstrap.rs`, `tests/bootstrap.rs`; new `tools/document-poc-seed/{package.json,src/main.ts,src/seed.ts,test/seed.test.ts}`; `docs/operations/document-poc-runtime-v0.md`.

- [ ] RED: uninitialized root requires explicit bootstrap; agent bootstrap rejected; existing unexpected policy never overwritten; bootstrap retry has no new audit/policy mutation. API seed retry/interruption reuses manifest IDs and refuses altered fixture data. Agent reads shared fixtures, cannot write or read human-only fixture.
- [ ] Run `cargo test -p document-server --test bootstrap` and seed package test once harness is scaffolded; record expected missing behavior.
- [ ] Implement existing root port composition; all subsequent seeding uses generated operations/BinaryTransportBridge. Local manifest is ignored, no customers/secrets/actual docs committed. Runbook lists migrate → bootstrap → human+agent serve → seed; command examples use environment names/placeholders, no password literal.
- [ ] GREEN tests through real HTTP verify exact twice-run IDs/version/revision/policy and no duplicates. Document safe disposable recreation separately.
- [ ] Commit reviewed slice.

## Task R5: Separate scheduler qualification — attribution decision recorded

Files: existing scheduler `src/main.rs`, adapter wiring file/tests; runbook. Do not implement a business loop in document-server.

- [x] Record the requester-delegated short reusable executor attribution: provider `service`, principal `scheduler`, no authorization/login identity. See bounded authority record dated 2026-10-02.
- [ ] RED missing/unknown requester and revoked policy; assert executor is recorded separately and contributes no policy subjects. Agent requests do not gain HumanInteractive permission.
- [ ] Wire `connect_with_resolver`, preserving existing due-claim/idempotency/transaction semantics. Resolve only explicitly approved PoC identities; no generic production resolver.
- [ ] Run `cargo test -p document-application --test scheduled_authorization_contract` and `cargo test -p document-publication-scheduler --test due_publication --test linux_container_canary`, plus real separate-process schedule→due→publish, stopped/restarted scheduler and revoked-before-due tests. Record actual target names after discovery.
- [ ] Commit only after all focused checks. Missing real-process evidence keeps R5/C1 completion blocked; scoped tests are not acceptance.

## Task R6: Composition-root Human journey and exact-head gates

Files: `apps/document-web/playwright.runtime.config.ts`, `apps/document-web/e2e-runtime/document-runtime.spec.ts`, `tools/document-poc-runtime/` process harness, `mise.toml`, relevant CI workflow, C1 execution status/runbook.

- [ ] RED actual binary journey: root/folder/list/detail/revisions/history/diff/file/policy/new version/publish; assert actual persisted IDs and audit where existing contract requires. No `page.route('**/v1/**')`, fake API, fixture-only router, or Webpack development server.
- [ ] Include synthetic PDF inspection and an actual PDF Diff/display comparison using the qualified PDFium path in both runners; absent/invalid runtime must remain unavailable/unqualified.
- [ ] Harness builds approved production GUI and both production workers; launches disposable PostgreSQL, isolated storage and two server processes. Unique per-run ports are allowed test details; documented user defaults remain 8080/8081. Never reuse another agent's process/database/worktree.
- [ ] Run `pnpm --filter @knowledge-platform/document-web exec playwright test --config playwright.runtime.config.ts`; GREEN must include actual HTTP server logs/provenance and browser artifacts. Use keyboard/focus/reduced-motion/error journeys from approved GUI requirements.
- [ ] Restart both servers with same DB/storage; assert document/revision/file hashes persist and Human/Agent share state. Run graceful shutdown, DB/storage/worker fail/recover and denied-resource cases.
- [ ] Add named `mise run document:poc:runtime` entrypoint for reproducible combined harness. Run affected unit/type/build/API/architecture/security/license checks then `mise run verify:fast` and required aggregate gates with coordinated capacity. Any missing Docker/Linux sandbox/browser/native resource is BLOCKED, not PASS.
- [ ] Push reviewed R2 draft branch stacked onto R1; verify remote head and all required Standard CI/Sandbox/DSI PoC/real-runtime jobs for that exact head. A docs-only evidence commit changes the head and requires rechecking.

## Delivery stack and completion

R1 C1 Design+Plan+bounded authority record is a separate Draft based on the current PR36 head; PR36 closure is pending. R2 C1 implementation is a later separate PR based on R1. A1 C2 Design+Plan can be reviewed as a docs-only stack on R1; A2 implementation must incorporate verified R2 runtime and A1. A separate C3 plan-only Draft precedes E1 acceptance evidence. No implementation or acceptance evidence is delivered by these design PRs. The bounded authority record identifies design/plan blobs and the narrow scheduler-attribution decision. Preserve the existing global active pointer; maintain this capability-specific status and coordinate any future pointer change.

C1 completion requires all R1–R6 evidence including separate scheduler qualification. Report what passed, failed, blocked or not run, with head/platform/process provenance. Leave all product PRs unmerged and not deployed.
