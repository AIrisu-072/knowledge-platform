# Document Platform PoC Runtime / Server Composition v0 — C1 Design

Status: FIXED-SCOPE DESIGN / PLAN REVIEW; scheduler identity decision remains STOP. No implementation or acceptance qualification claim.

Approval boundary: [C1 bounded authority record](2026-10-01-document-poc-runtime-v0-approval.md).

## Intent and authority

Make the existing Common Document API usable by Human GUI and Agent against the same real PostgreSQL and authoritative FileSystemStorage. The requester’s 2026-10-01 instruction fixes the runtime profiles, safety boundaries, separate processes, protocol and delivery sequence. Its §30 permits an approval record and implementation for a faithful transcription without new important semantic decisions. This candidate does not self-approve the unresolved scheduler identity below.

Publication baseline verified on GitHub on 2026-10-01 UTC: Draft PR [#36](https://github.com/AIrisu-072/knowledge-platform/pull/36), branch `feat/document-gui-integration-v0`, head `b578a9b49338066d0e4ee5495ea1280f991c122b`; main `d71753d46590bb4406a1c0b74894ab90a27a6c88`. The baseline CI, DSI Sandbox Preflight and DSI PoC runs succeeded, but C0 closure and the real-backend browser evidence remain incomplete. R1 stacks on this current PR36 head without treating it as acceptance-complete. If C0 changes its head, reconcile the stack and verify the new exact head before dependent implementation acceptance.

Read together:
- `AGENTS.md`, `spec/architecture/architecture-contract-v0.md`, `spec/architecture/dependency-rules.toml`
- `spec/selection/library-tool-selection-v0.md`, `deny.toml`
- `spec/operations/observability-audit-requirements-v0.md`, `spec/operations/error-handling-resilience-requirements-v0.md`
- Frozen GUI Design `2026-09-30-document-gui-integration-v0-design.md`, approved blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`
- GUI Production Plan, approved blob `0830c306ebb38290e4c3dc277f6c97a0759cf912`
- GUI Amendment 01 / approval and Production Plan Addendum 01 dated 2026-10-01
- [C1 plan](../plans/2026-10-01-document-poc-runtime-v0-implementation.md)
- C2 design is delivered in the separate A1 Draft PR; C3 acceptance plan is a separate plan-only Draft. Their future evidence does not exist in R1.

## Evidence boundary

Existing browser `apps/document-web/e2e/document-workspace.spec.ts` intercepts `**/v1/**`; its reported 6/6 run was on `bb5da30c7a831a9d79a16cc422f00adc89b70c69`. That is useful GUI coverage, not the requested production-composition browser journey. Existing `document-api-http/tests/e2e.rs` assembles real adapters inside a fixture and calls Router directly. C1/C3 must launch `document-server` and drive actual HTTP and browser traffic. C0 must not upgrade either older or fake evidence to a fresh real-runtime acceptance result.

## Composition and rejected alternatives

Selected: `crates/document-server` is a reusable composition root around the existing adapters and application services. Reject a throwaway PoC business implementation and reject adding infrastructure composition to `document-api-http`; both weaken the existing boundaries. Reject embedding scheduler execution in the server because the separate scheduler boundary is fixed.

Allowed dependencies: existing HTTP router crate, Application ports, PostgreSQL repository, FileSystemStorage, production DSI/Diff runners, selected Axum/Tokio/SQLx and standard library. Domain/Application never depend on document-server. HTTP continues to forbid PostgreSQL/storage/runner implementation dependencies. No business SQL, authorization evaluator, lifecycle logic, parser, or Diff interpretation belongs in document-server.

`compose_document_api(DocumentApiRouters::new(...))` already composes read, management, create, versioning, publication, file and Diff routes. Use those constructors, `EnsureSemanticInspection`, production runners, UUIDv7 IDs and a real UTC clock. Use the existing unavailable IdentityPresentation resolver for subject-ID fallback; no fabricated names or identity directory is necessary.

## Process-fixed identity

Only `StaticPoCIdentityAdapter` is added. Startup selects one enum, with no arbitrary subject/group fields:

| Profile | Provider | Principal | Group | InvocationKind |
|---|---|---|---|---|
| poc-human | poc | poc-human | poc-users | HumanInteractive |
| poc-agent | poc | poc-agent | poc-agents | Agent |

Each context includes its principal policy subject and exactly the corresponding group. The adapter never consumes request identity from headers, query, body or cookies. Creating a current context per resolution refreshes only the validity timestamp, never the profile; use the existing context validity contract rather than a startup context that silently expires during the PoC. Test expiry handling with a clock seam. Requests still pass all existing resource-level authorization and HumanInteractive gates. A static profile authenticates every caller reaching that port as that profile, which is why this is explicitly an isolated PoC, not production security.

`KP_RUNTIME_MODE` must explicitly equal `poc`; `production`, absent and unknown values fail closed. No fallback identity adapter exists. Linux remains the qualified runtime for production sandboxed workers.

## Configuration contract

`document-server serve`, `document-server migrate`, and a narrow `document-server bootstrap-poc` operation are separate commands. Parse environment once and produce redacted diagnostics; never log the database URL, credentials, full configuration or filesystem paths. Do not derive config from requests.

| Variable | Behavior |
|---|---|
| KP_RUNTIME_MODE | required, only `poc` supported |
| KP_IDENTITY_PROFILE | required for serve/bootstrap, exact profile enum above |
| KP_BIND | literal socket address; default human `127.0.0.1:8080`, agent `127.0.0.1:8081` |
| KP_DATABASE_URL | required, secret; compatible PostgreSQL connection URL |
| KP_STORAGE_ROOT | required for serve, pre-existing usable directory |
| KP_DSI_WORKER / KP_DIFF_WORKER | required for serve, executable files |
| KP_WEB_DIST | required for human serve, readable built dist with index and referenced assets |
| KP_POC_ALLOW_NON_LOOPBACK | strict boolean, default false; explicit true required outside loopback |
| KP_DSI_PDFIUM_RUNTIME_DIR | qualified native-runtime path wired explicitly into both DSI and Diff RunnerConfig; required for the PDF acceptance profile, otherwise PDF remains unqualified |

`serveWeb` is derived from the fixed profile: human=true, agent=false. This avoids an unnecessary general configuration framework. Ignore GUI dist for agent serving, and never expose its files there. Nonloopback override emits a conspicuous startup warning that all reachable callers obtain the fixed PoC identity; it does not enable TLS or production authentication. Malformed booleans/socket addresses/URLs fail rather than falling back. Bind only after all required startup checks; a port conflict fails cleanly and releases resources.

## Startup, migration and readiness

Startup order is config → mode → identity → DB pool → schema compatibility → storage → DSI executable → Diff executable → services → API → optional GUI → listener. Validate native runner requirements through their existing constructors/preflight; do not weaken Linux sandboxing or substitute in-process test workers when qualification fails. The configured qualified PDFium directory must be passed to both runners through their existing `with_pdfium_runtime_dir` methods; the Diff worker environment is cleared and cannot inherit it implicitly. PDF acceptance includes a real PDF comparison/display path, and missing PDFium is reported as unavailable/unqualified rather than a passing comparison.

`migrate` calls the existing PostgreSQL `migrate` function and its folder-name preflight. It does not serve or seed. It requires an explicit PoC mode/database configuration and must be run only against the disposable PoC database. `serve` never calls migration, initializes policy or creates document data. Add a read-only repository schema-compatibility method that validates the packaged migration version/checksum set, successful application and absence of unknown/missing migrations. At this PoC version, an exact migration set is the conservative supported compatibility rule; no implicit repair or downgrade occurs.

`GET /health/live`: process responds. `GET /health/ready`: recheck DB reachability, schema, usable storage, worker executability, immutable valid config/profile, and GUI artifacts when enabled. Return 200 `{ "status": "ok" }` or 503 `{ "status": "unavailable" }`; liveness returns the first form. No path, credentials, DB identifiers, errors or worker argv in responses. Readiness is a bounded operational probe, not a business success guarantee. Its storage probe uses a uniquely owned temporary probe file and removes only that file; it never changes authoritative document objects. Log only safe component/error categories. Do not cache a startup success forever.

On SIGINT/SIGTERM stop accepting, mark not ready, gracefully drain active requests and response streams, then close the pool. Existing operation timeouts end when a handler returns its Response; file-stream idle timeouts do not establish a total client-connection or drain deadline. Consequently this design does not promise bounded termination for a stalled/slow streaming client and does not select a force-close policy. Test that case explicitly: new work is refused, readiness is false, the process remains draining rather than claiming completion, and releasing/cancelling the test client permits shutdown. If qualification requires a finite forced cutoff, record the exact proposed transport/transaction consequences and STOP for that decision before adding it. Do not invent a performance SLO, undo an already committed mutation, or report cancellation as confirmed business failure. Unknown transport completion remains recoverable through existing operation-ID mechanisms.

## Same-origin GUI routing

Serve the existing Webpack production `apps/document-web/dist`; retain the approved Webpack/Jest/TS6 amendment. No CORS expansion, dev server, frontend API fork or browser document parser.

Dispatch exact `/health/*` first; reserve `/v1` and `/v1/*` for the existing API including errors/unknown routes. Static GET/HEAD may serve only assets inside the canonical dist root. SPA document navigation falls back to index; missing asset paths, non-GET methods, traversal, encoded traversal and unknown API/health paths must never receive index HTML. Reject symlinks escaping dist and dotfile/source-map exposure. Preserve API authentication/error/security middleware and add conservative static response headers without changing the API contract.

## Fresh database bootstrap and synthetic seed

There is no Common API operation to initialize the first root policy; `BootstrapRootPolicy::initialize_root_policy` already owns that operation. A separate explicit `bootstrap-poc` command may call it using the fixed human profile and `PostgresDocumentRepository::new_with_bootstrap_actor`. This is composition of an existing application contract, not raw INSERT and not a public bootstrap HTTP endpoint. The existing port rejects a previously initialized root and atomically records the initial policy/audit. Do not overwrite existing policies; the command reports an idempotent already-initialized result only after verifying the exact expected fixed policy through an authorized read, otherwise it fails closed.

Grant the human group Read/ReadHistory/Write/Publish/Administer at root; grant the agent group Read/ReadHistory only for traversal/read. Seed explicit policies on PoC Shared and Agent Sandbox; no agent write grant. An explicit human-only document/folder supplies a denial fixture and does not inherit agent access. These are synthetic disposable-database grants, not production entitlement decisions.

`tools/document-poc-seed` uses the generated client/BinaryTransportBridge and only the human Common API for folders, documents, versions, publication and policy mutation. Use synthetic Japanese sample regulation/manual/notice text, plus an Agent Sandbox sample and a human-only denied sample. Persist generated UUIDv7 IDs, operation IDs and expected fixture hash in a local manifest outside Git. Reuse exact IDs/operations on retry; conflicting existing data/ACL/content fails instead of being silently overwritten. Interrupted bootstrap/seed retries must not duplicate revisions/audit/business operations. Document full disposable-database recreation as the fallback, never a production cleanup command.

## Scheduler integration: material unresolved decision

The existing `document-publication-scheduler` binary calls `DueScheduler::connect`, which unconditionally returns `IdentityResolverRequired`. `connect_with_resolver` additionally requires an `IdentityContextResolver` and a separate `service_executor: PrincipalRef`. `scheduled_authorization.rs` resolves the original requester and makes a Service context while recording the executor separately; it deliberately does not add executor permissions to the requester.

The instruction fixes two interactive/agent profiles but does not name the scheduler's audit executor identity. Do not invent `poc-scheduler`, impersonate poc-human/poc-agent, or alter authorization to bypass this requirement. Candidate (not selected): provider `poc`, principal `poc-scheduler`, no group, no ACL grants, and no selectable HTTP profile. This adds one fixed audit-attribution identity, not authentication authority. The due execution context remains the original `poc-human` principal plus `poc-users` group with `InvocationKind::Service`, and records the separate executor. Retain original-requester re-resolution and current ACL checks, and the scheduler in its own process. The only existing literal is test-only `service` / `scheduler` in `document-application/tests/scheduled_authorization_contract.rs`; it is not a configured/approved runtime identity. Adopting that literal is an alternative requiring the same explicit decision. Reusing human/agent would conflate attribution and is not recommended. This is a narrow identity/audit semantic decision requiring clarification unless an existing approved source fixes it. Record STOP for scheduler-specific implementation and scheduling acceptance; unaffected C1 server/C2 reads can proceed after review. C1 cannot claim fully complete while scheduled publish is unqualified.

## Acceptance and non-goals

The C1 plan maps config safety, migration isolation, two-server shared state, secure health/static routing, real GUI journey, seed and restart persistence. C3 owns wider failure/evaluation evidence. Real production workers, PostgreSQL and storage are mandatory. Fake fixtures remain unit tests only.

No production identity/framework, AD/SSPI/Entra/WIA/biometrics, production TLS/DNS/HA/backup/deploy, Agent writes, Search integration or RAG. No PR #36 merge. New dependency/features require fresh exact graph/license/security qualification; the GUI's candidate-specific license exception is not transferable to new graphs.
