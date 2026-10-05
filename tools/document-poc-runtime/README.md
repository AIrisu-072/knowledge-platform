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

Scheduler Task R5 uses fixed `service` / `scheduler` audit attribution and has the separate
`mise run document:scheduler:acceptance` gate in the required `document-scheduler` CI job. This R6 harness does not qualify or start the scheduler.
It does not add Search, introduce
production identity, deploy, or create persistent access. The actual-process in-flight and
stalled-stream cases above must pass before this harness qualifies R6. They observe
drain while clients stall and completion after release, preserving the approved
unbounded-drain limitation. A cleanup SIGKILL after a failed observation never
counts as successful graceful shutdown or a production force-close policy.

## C3 ordered consistency candidate

The journey now also discovers `human-agent-consistency.spec.ts`. Its isolated
synthetic document follows initial GUI publication (1.0), Human API metadata
change (1.1), metadata no-op (identical state), GUI-created/published content
(2.0), and Human API withdrawal with current-version fallback (3.0). Metadata
editing and withdrawal are explicitly API-driven because the approved GUI has
no controls for those mutations. Every checkpoint reloads the actual GUI,
checks its observed API projection and visible revision/metadata, and launches
an actual SDK stdio client against the Agent adapter. It compares document,
current content version, OCC and human revision IDs, complete revision rows,
metadata, current-version summary and file metadata for every issued version.
It excludes only actor-local read acknowledgements and capability presentation.

A test-only loopback proxy forwards exactly one real metadata request, consumes
the upstream response, then closes its downstream without delivering any
response. No fabricated business result is served. Recovery resends the exact
saved operation ID/payload (including its original expected OCC revision), then
replays it again. The oracle requires the saved result, unchanged committed state,
and exactly one corresponding Human operation-history entry. This is an actual
response-loss scenario when run against the composition root; the helper's unit
server only verifies fault-control mechanics and is not acceptance evidence.

A separate GUI case loads an available create capability, removes Human Write
while retaining Read/ReadHistory/Administer through the synthetic document's
policy, submits the stale form, and requires authoritative FORBIDDEN with no
created version/success message. It verifies the selected file remains available
and recovers after restoring the original effective grants. The original stale
OCC case and the existing complete Agent denied-ID/revocation matrix are reused.

The final consistency snapshot joins the existing regulation/PDF restart oracle.
The compiled `consistency.cjs` test-helper hash is included in artifact provenance
and the bounded summary refuses missing provenance. Synthetic checkpoint and
stdio transcripts stay run-local; they are not automatically uploaded. Discovery,
compilation or mock helper tests do not establish real-runtime PASS. Native DSI/
Diff failure-recovery and reviewed visual usability evidence remain separate E0
requirements, as does scheduler acceptance owned by R5.

Local API/MCP contract commands invoke the already-qualified Redocly CLI. Use its
supported `REDOCLY_TELEMETRY=off` environment setting during validation to disable
anonymous telemetry; no external telemetry transmission is authorized here.

## C3 real worker launch failure/recovery candidate

`worker-failure.spec.ts` adds two API-only cases to the owned Playwright journey.
They use the actual Human composition root and generated client; they do not
claim to be GUI interactions or replace a worker with a fake implementation.

- DSI: create/publish a fresh synthetic base, remove execute permission from the
  run's copied DSI executable, then attempt a fresh Version upload. The frozen
  `ExtractorUnavailable` mapping requires503 `DEPENDENCY_UNAVAILABLE` with a
  secret-free problem and no Version/Revision/current-publication change. Restore
  the copy and resubmit the same operation/version/file IDs, expected OCC and
  bytes; replay must return the saved result. Only then publish and verify both
  originals. Preflight can leave a prepared immutable FileObject after failure;
  this test proves no false business Version/Revision/publication, not zero DB
  or storage writes of any kind.
- Diff: prepare a new published changed-content pair that has never been compared,
  then remove execute permission from the run's copied Diff executable. The
  existing unavailable-executor path requires500 `INTERNAL`; it is not an
  Unknown/Partial response. Require no result/fragment disclosure and unchanged
  authoritative state. After restoration, the identical pair must produce full,
  confirmed differences with the actual expected original/changed text fragments
  and source Version IDs. An earlier cache hit cannot mask the outage.

Both require the actual HTTP status and `application/problem+json` media type, the exact request path and fixed safe typed Problem shape; a matching body on the wrong transport status or nested fragment in a permitted field fails. They also check live/ready responses during the fault and recovery. Runtime context
carries only the already-recorded DSI/Diff artifact hashes, not arbitrary worker
paths. The control resolves the fixed basename within the private owned run
folder, opens without following a symlink, rejects shared hardlinks, checks700
permissions and the exact built hash, changes only that descriptor to600, and
restores/checks700 plus the same hash in `finally`. The original build binaries
are never chmodded. The existing worker-readiness test uses this same guard.
Recovery snapshots join the existing same-DB/storage restart oracle.

The unavailable-launch case is deliberately bounded; it does not claim coverage
of every worker crash, resource exhaustion or parser failure. The full production
runtime still must execute these cases on an exact head. Local permission-control
fixtures are never executed as workers and count only as harness tests. No new
sandbox bypass, worker fallback, business mapping, dependency or upload is added.

## WORKING multi-original editor candidate

The existing owned journey includes `working-version-editor.spec.ts` (two cases)
and restart includes `working-version-editor-persistence.spec.ts` (one case).
Both use top-level screenshot/trace/video `off`, the same two synthetic profiles,
disposable PostgreSQL18.6 and existing Chromium. No new runner, public artifact,
Playwright private environment variable or global capture setting is introduced.

The first case repairs an uninspected invalid-UTF-8 original in never-published
Version1 with null base/current, preserving its media type, ID/number and unpublished
state without requiring old-original inspection. The second prepares two originals and two
renditions through the full-manifest API, creates a new WORKING by replacing one
original in the GUI, checks exact filenames and all retained file IDs/bytes, then
updates only the other original. Only the selected original's old renditions are
omitted. The old publication and all its files remain intact while editing and
a rejected stale publication leaves it unchanged. A successful GUI publish switches
current directly to the new version and preserves the previous version in history.
A separate `.working-editor.json` sidecar stores observed hashes, exact manifest,
version/revision state and ledger source IDs for both HTTP-process restarts.

This slice's local evidence is limited to pure tests, compilation and collection;
the exact-head hosted run must establish database/browser/restart/owned cleanup.
Finite diagnostic source/stage allowlists include the new cases, without exposing
filenames, content or raw assertion messages.

### WORKING実応答喪失の追加受入（2026-10-05承認、local検証完了）

公開D2 `2e1e17f4` の通常経路を保持した別branch
`feat/document-working-response-loss-20261005` で、既存5filesだけを補修する。
最初に純粋guard/配線のREDを取り、localhost専用proxyと既存2journeyを実装し、
独立review・型・collectionを確認する。初回PUT・新版POST・公開base付PUTの
backend成功完了後にGUI応答だけを失わせ、結果不明表示から明示同内容再送する。
raw multipart/Content-Type、受信/dispatch回数、retry前後のcommit済みsnapshot、
revision/履歴の非重複を確認する。自動再試行を明示回復とは扱わない。

同じ使捨てhosted・固定Human origin・2合成profileだけを使用する。専用pageの
公開context proxy optionに限定し、AgentのAPIRequestContextは直接接続する。
GET/HEAD以外は対象Doc/Version POST/PUTと回復後の同版publish1回だけを許可する。
外部宛先/CONNECT/upgrade/認証情報を拒否し、raw payloadはメモリ内だけに置く。
context・listener・socket・upstream・timerの終了を必須とする。既存metadata喪失
契約、製品コード、runner、依存、全体設定、画像/trace/videoとartifact公開0は維持。
ローカルではlistener/browser/DBを起動しない。純粋TDD・型・collectionだけを資格とし、
実通信とcleanupの成功は公開後の既存hostedが成立するまで未確認である。

2026-10-05 09:04 UTC時点: 純粋proxy6件・既存の安全なruntime純粋124件、
配線13件（124の内数）・runtime型・collection18+5が成功し、独立source reviewはGO。
初回collectionは未生成MCP bundleで失敗したが、既存MCPのcompile-only後に成功した。
listenerを使う既存/追加HTTP単体試験は当地では未実行。scope全体に50秒を掛ける
途中案はREDで補正し、初回送信/明示再送の観測だけ各50秒に限定した。
製品/生成SDK/lock/runner/全体設定と元metadata喪失helper本文は不変。
約400行の追加は固定宛先・byte透過・失敗保持・cleanupと回帰試験に必要と独立確認した。
次の操作はこの別commitを公開D2へ追加し、同一headの既存hostedで実socket喪失、
GUI明示回復、HTTP再起動、owned cleanupと公開artifact0を確認することである。
