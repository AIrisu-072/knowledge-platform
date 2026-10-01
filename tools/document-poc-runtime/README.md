# Real Document PoC runtime acceptance (C1 R6)

This harness launches the actual `document-server` binary twice, with fixed human
and agent profiles, one disposable PostgreSQL database and one dedicated filesystem
storage root. The human process serves a copy of the Webpack **production build**.
All fixture mutations use the existing generated Common API client and binary
transport bridge. The browser never intercepts API routes. There is no fixture
router, development server, system-browser substitution, in-process worker or
sandbox-relaxation fallback.

## Run

Use the repository-pinned Node, pnpm and Rust toolchains and the frozen installed
workspace graph. Install the existing pinned PDFium using the repository installer
and Playwright's pinned Chromium, then run from the repository root:

```sh
node --test tools/document-poc-runtime/test/*.test.mjs
pnpm --filter @knowledge-platform/document-web exec tsc -p ../../tools/document-poc-runtime/tsconfig.json
pnpm --filter @knowledge-platform/document-web exec playwright install chromium
export KP_DSI_PDFIUM_RUNTIME_DIR="$(bash experiments/document-semantic-inspection/scripts/install-pdfium.sh)"
node tools/document-poc-runtime/run.mjs
```

Missing browser libraries must be reported with their exact prerequisite failure;
the harness does not install system packages or select a different browser. The
normal runner builds the locked Rust server, DSI worker and Diff worker and builds
the production GUI. It then validates and hashes those binaries, all GUI assets
and `libpdfium.so`. It passes the same qualified PDFium path explicitly to both
profiles; production composition passes it to both production runners.

If `TEST_DATABASE_URL` is absent, Docker must be available. The runner creates one
uniquely named/labeled `postgres:18.6-bookworm` instance with a random **loopback**
port, random in-memory password, private tmpfs data and no host volume or privileged
container. Its image ID and available RepoDigests are recorded. Cleanup verifies
the exact created container ID's ownership label before removing only that
container. There is no global Docker cleanup.

An existing fresh, explicitly disposable **loopback** database can instead be
supplied with both `TEST_DATABASE_URL` and `KP_POC_DISPOSABLE_DATABASE=true`.
Connection-string query overrides are rejected. The harness never drops or resets
that database. Its lifecycle remains the caller's responsibility. The local
`with-postgres` helper, runner and all children must live in the **same exec cell**.
Do not reuse an already seeded database: each invocation owns a new manifest and
storage root, and the seeder fails closed on conflicts.

Diagnostic-only `KP_POC_BINARY_DIR` requires `--prebuilt` and selects a prepared binary directory; a normal qualified run rejects that override.
`KP_POC_EVIDENCE_DIR` selects a private parent for uniquely created run directories.
`CARGO_TARGET_DIR` is respected. The diagnostic `--prebuilt` option does not run the
build stage: evidence records that stage as `not-run`, source correspondence as
unverified, and the complete acceptance gate cannot pass. Hosted acceptance must
omit `--prebuilt`.

## Acceptance sequence

1. Build/hash production artifacts, provision one disposable PostgreSQL instance
2. Explicit `migrate`, explicit human `bootstrap-poc`, and bootstrap replay
3. Start human/agent processes on unique loopback ports; wait for actual readiness
4. Seed synthetic fixtures through human HTTP twice, verifying idempotent replay
5. Run `playwright.runtime.config.ts` journey phase against both real instances:
   - folder/list/detail; keyboard navigation and same-origin HTTP
   - revisions, persisted publication history/actor attribution, revision Diff
   - original download hashes and effective policy; save an explicit unchanged
     read-only agent policy through the GUI
   - GUI upload/new version and confirmation-based immediate publication;
     agent reads see the identical persisted version/revision/file state
   - fixed agent identity despite request claims; no agent GUI; private resource
     denial and publication write denial
   - synthetic PDFs from the existing repository DSI fixtures: actual inspection,
     publication, production PDF Diff `display`, full coverage and PDF text
     fragments, matching original bytes and real browser display
   - genuine concurrent-version OCC rejection, missing document/backend errors,
     reserved unknown routes, keyboard focus, reduced motion, 1280/1440 layout
6. Send fixed W3C traceparent requests to both actual processes and retain the
   safe, route-template correlated diagnostics for assertions after shutdown
7. Verify readiness fails while liveness survives real database connectivity loss
   through an owned transparent TCP proxy; reconnect to the **same** PostgreSQL
   instance. The proxy forwards actual PostgreSQL traffic, implements no database
   protocol/business behavior and never changes data. Check recovery after removal
   of execute permission on the owned worker copies, disappearance of the owned
   storage path and disappearance of the copied GUI index; restore each resource
8. Observe actual process shutdown under two active requests, with no test router:
   - ordinary metadata PATCH: wait for actual HTTP `100 Continue`, send only part
     of its typed synthetic JSON, SIGTERM, require refusal of new connections and
     a still-running draining process, then complete the body and require the
     successful response and exact operation ID
   - restart human, download a separate Common-API-created 32 MiB synthetic
     WORKING original using a raw TCP client paused after response headers;
     require an incomplete body before SIGTERM, refusal of new connections and
     still-draining process; resume, verify all bytes/hash, then require clean exit
   The 500 ms still-draining observation and client-release windows are test
   observations, not claims of a bounded production drain or a new SLO. If a
   prerequisite or observation fails, that stage fails; neither case is skipped.
9. Stop agent and assert the actual-process logs contain each known trace ID,
   fixed route template/status and expected readiness failure categories, without
   credentials, URLs, private paths or request sentinel values
10. Restart both against the same DB/storage and ports; run the persistence browser
    phase. Compare document/current-version IDs, exact Revision records,
    publication ledger identities and every original hash. Verify the in-flight
    metadata mutation and large original also survived restart
11. Confirm final graceful shutdown and clean up only owned processes/container

The browser suite has five serial journey tests and one restart test. It needs
`KP_POC_RUNTIME_CONTEXT`, `KP_POC_RUNTIME_PHASE` and `KP_POC_BROWSER_OUTPUT` set by
the runner; invoking it without its owned runtime context fails rather than
launching a development server. Retries are zero. The seed manifest remains intact
for evidence; it is not replayed after the GUI intentionally changes fixture state.

## Evidence and failure semantics

Every stage begins `not-run` and records start/end UTC timestamps and
`passed`, `failed`, or `blocked` only when observed. A missing qualified platform,
Docker/PDFium/browser resource, or known production DSI/Diff startup-unavailable
category is **blocked** and exits nonzero. A browser assertion or other executed
failure is **failed**, not a skip. Later stages stay **not-run**. No failed result
is converted into passing acceptance. The top-level `acceptanceQualified` means
only this R6 harness ran all its stages, including its build; it never means C1,
C2 or C3 are complete. Record and review the exact tested Git head, dirty state,
artifact hashes, process generations, browser report, and cleanup result.

Diagnostics redact the complete database URL and password before persistence,
including values split across subprocess output chunks. Raw process output is
never written. Child logs are finalized on child exit. No environment dump or
command-line database credential is recorded. The entire run directory is private
local evidence and must **not** be blindly uploaded: it includes storage, copies of
executables, seed/context manifests and other disposable working files.

CI currently publishes only a bounded allowlisted summary, with no new upload
workflow/action or blanket artifact upload:

```sh
node tools/document-poc-runtime/ci-summary.mjs
```

Run this in an `if: always()` step. It tolerates missing/malformed reports by
printing categorical unavailable evidence and exiting nonzero. It cannot turn a
failed runtime job green. The summary permits only a validated Git head/dirty
flag, observed Node/pnpm/rustc/Playwright/platform versions, source-lock and
binary/native/GUI hashes, fixed profiles, known stage names/statuses and fixed
failure codes. It does not print raw reasons, URLs, paths, env/argv, file contents
or logs. Owned PostgreSQL version is observed with the fixed read-only
`SHOW server_version` query; an external database's version is explicitly
unverified. Startup observation is 60 seconds, exceeding DB acquire 5 seconds +
DSI preflight 10 seconds + Diff preflight 30 seconds; it is not a production SLO.

The summary also exposes bounded diagnostics for setup/browser failures. Database
steps disclose only a fixed operation/category, availability booleans and bounded
command exit codes. Playwright test and top-level collection errors disclose only
known source basenames with line/column, fixed error categories/matchers, bounded
HTTP statuses and allowlisted Problem codes. Browser records share a 20-record
budget; missing/malformed reports remain unavailable. Both diagnostic families
are sanitized again at the summary boundary. Raw error values, titles, selectors,
stack traces, SQL, responses and attachment content are never printed.

Browser screenshots, videos, traces and detailed logs remain **run-local only**.
They are generated for diagnosis but are not durable hosted artifact evidence
under the current CI publication scope. Record this retention/review gap rather
than claiming externally reviewable screenshots/traces. The report itself
contains hashes and ephemeral process/container IDs, not connection URLs,
passwords, storage contents or environment variables.

## Qualification boundary

Unit tests validate harness safety/reporting and use real small OS/network
processes only to test orchestration. Type checking and Playwright `--list` prove
that the browser suite loads; neither is runtime acceptance.

At implementation time the provided local production-server smoke passed explicit
migration/bootstrap but failed closed at production DSI preflight. The diagnostic
was `worker exited without a valid failure classification`. Pinned Chromium
acquisition also failed with invalid downloaded archives. These are blocking
prerequisites; the full GUI/PDF/shared-state/restart journey has **not run locally**.
Do not retry denied sandbox controls or present fake/fixture evidence as GREEN.
The hosted normal Linux job must establish the actual result.

Scheduler Task R5 remains explicitly STOP pending its separate identity decision.
This harness does not start or change the scheduler, add Search, introduce
production identity, deploy, or create persistent access. The actual-process in-flight and
stalled-stream cases above must pass before this harness qualifies R6. They observe
drain while clients stall and completion after release, preserving the approved
unbounded-drain limitation. A cleanup SIGKILL after a failed observation never
counts as successful graceful shutdown or a production force-close policy.
