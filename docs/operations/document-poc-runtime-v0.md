# Document PoC runtime v0 runbook

This is an isolated synthetic-data PoC, not production authentication or deployment.
The two instances expose the existing Common Document API and share PostgreSQL and
FileSystemStorage. Anyone who can reach an instance acts as that instance's fixed profile.
Do not expose either port to untrusted clients.

## Build and supported environment

Use the repository-pinned Rust, Node and pnpm toolchains. Linux is required for the production
DSI/Diff sandbox; successful parser tests on another platform do not qualify runtime isolation.

```sh
pnpm install --frozen-lockfile --ignore-scripts
cargo build --locked -p document-server -p document-semantic-inspection-worker -p document-diff-worker
pnpm --filter @knowledge-platform/document-web build
PDFIUM_DYNAMIC_LIB_PATH="$(bash experiments/document-semantic-inspection/scripts/install-pdfium.sh)"
export PDFIUM_DYNAMIC_LIB_PATH
```

The PDFium installer verifies both archive and native-library SHA-256 for the pinned
151.0.7881.0 runtime. Pass its directory explicitly to **both** workers through the server
configuration below. A text-only startup probe is not PDF qualification; the real PDF
comparison/display acceptance must also pass.

## Explicit disposable-database preparation

Create a dedicated disposable PostgreSQL 18.6 database, a pre-existing dedicated storage
directory and a built GUI dist. Supply the database URL privately through the environment;
never put credentials in a command transcript, checked-in file or artifact. The runtime does
not guess whether an operator-supplied database is production. Confirm ownership and the
synthetic/disposable purpose before either explicit database command.

```sh
export KP_RUNTIME_MODE=poc
# Set KP_DATABASE_URL privately to the dedicated disposable PoC database.
document-server migrate
KP_IDENTITY_PROFILE=poc-human document-server bootstrap-poc
```

`migrate` alone applies existing packaged migrations and the existing folder-name preflight.
`serve` only reads the exact applied migration version/checksum/success set. It never creates
schema, repairs migration history, bootstraps permissions or seeds documents. Missing, failed,
changed or unknown migration records reject startup.

`bootstrap-poc` is human-only and calls the existing root-policy port. It grants `poc-users`
Read/ReadHistory/Write/Publish/Administer and `poc-agents` Read/ReadHistory. A repeated command
reports already initialized only after an authorized read verifies the exact fixed root
policy. Different or unreadable policies fail closed without being overwritten.

## Two separate processes

Both shells inherit the same private `KP_DATABASE_URL`, `KP_STORAGE_ROOT`, absolute worker
paths and qualified `KP_DSI_PDFIUM_RUNTIME_DIR`. Start each process separately:

```sh
export KP_STORAGE_ROOT=/absolute/path/to/disposable-poc/storage
export KP_DSI_WORKER=/absolute/path/to/target/debug/document-semantic-inspection-worker
export KP_DIFF_WORKER=/absolute/path/to/target/debug/document-diff-worker
export KP_DSI_PDFIUM_RUNTIME_DIR="$PDFIUM_DYNAMIC_LIB_PATH"

# Human terminal
KP_IDENTITY_PROFILE=poc-human \
KP_WEB_DIST=/absolute/path/to/apps/document-web/dist \
document-server serve

# Agent terminal, with the same shared database/storage/worker environment
KP_IDENTITY_PROFILE=poc-agent document-server serve
```

The human default is `127.0.0.1:8080`; agent default is `127.0.0.1:8081`. `KP_BIND` accepts a
literal socket address. Other addresses require the exact boolean
`KP_POC_ALLOW_NON_LOOPBACK=true` and produce a warning. The default is false; malformed values
are errors. The override does not create TLS or production authentication.

Only `poc-human` (`poc/poc-human`, group `poc-users`, HumanInteractive) and `poc-agent`
(`poc/poc-agent`, group `poc-agents`, Agent) exist. Profile selection happens once at startup;
headers, cookies, query and body cannot override it. Context validity is refreshed per request.
`KP_RUNTIME_MODE=production`, unknown modes/profiles and missing required values fail closed.
The human serves the production GUI at `/` with same-origin `/v1/...`; the agent serves no GUI.

## Health and shutdown

- `GET /health/live`: 200 with `{"status":"ok"}` while the listener is active
- `GET /health/ready`: 200 status ok, or 503 with `{"status":"unavailable"}`
- Readiness rechecks PostgreSQL, exact migration compatibility, storage write/read/cleanup,
  actual canonical adapter targets, effective worker execution permission and GUI artifacts
- Probe files are uniquely owned and removed; authoritative objects are never modified
- API/health paths cannot fall through to SPA HTML, including encoded prefixes; missing assets,
  traversal, escaping/hidden/source-map aliases and non-GET static requests are rejected
- Safe HTTP observations retain trace IDs, route templates and typed error categories; only
  allowlisted application/runtime tracing targets are enabled. Configuration values, database
  errors, paths, bodies and credentials are not logged by the runtime

SIGINT/SIGTERM stops accepting connections, marks not ready, drains requests/response streams,
then closes the pool. There is **no total drain deadline or forced cutoff**. A stalled streaming
client can keep the process draining until it releases/cancels. A client timeout or disconnect
does not prove a mutation failed; use the existing operation-ID recovery contracts.

## Synthetic seed and retries

```sh
KP_RUNTIME_MODE=poc \
KP_DOCUMENT_API_BASE_URL=http://127.0.0.1:8080 \
KP_POC_SEED_MANIFEST=/absolute/path/to/private-disposable-poc/seed-manifest.json \
pnpm --dir tools/document-poc-seed seed
```

See [the seed contract](../../tools/document-poc-seed/README.md). It creates only synthetic
Japanese text and explicit shared/sandbox/human-only policies through the generated Common API
client. No agent write grants are added. The persisted manifest records IDs, operation requests
and expected hashes; do not discard it and seed the same database again.

Initial create IDs are server-generated and the API has no client operation ID for that call.
If its response/IDs are lost, the seed refuses a second POST rather than guessing whether the
first committed. After diagnosing the failure, the explicit fallback is to stop the two owned
processes, preserve evidence, recreate **only the identified disposable database and dedicated
storage**, move the old manifest aside and repeat preparation. There is no automated production
cleanup or deletion command.

## Scheduler and other boundaries

The existing publication scheduler remains a separate process. Its PoC service-executor identity
has not been approved, so scheduler startup wiring and scheduled-publication acceptance remain
STOP. Do not select a scheduler identity, reuse an HTTP principal or add ACL authority by hand.
Immediate human publication remains within the existing API.

No production AD/WIA/Kerberos/Entra/biometric identity, TLS/DNS/HA/backup/deployment, Agent write
tools, raw-text/RAG endpoint, Search integration, merge or deployment is included.

## Reproducibility and dependency scope

The runtime composes existing selected Axum/Tokio/SQLx/repository/storage/worker libraries.
New feature/direct-use qualification includes `tower-http 0.7.1` fs, Tokio net/signal,
`percent-encoding 2.3.2`, `rustix 1.1.4` fs/std (safe effective-access check), existing HTML parser
for built asset validation, and `tracing-subscriber 0.3.23` fmt/std/registry without ANSI,
environment filters or log bridging. The latter artifact reports MIT. Rust graph advisories,
bans, licenses and sources passed after these changes; no license exception was added.

Focused runtime tests require a disposable PostgreSQL instance through `TEST_DATABASE_URL`,
or start their own PostgreSQL18.6 testcontainer when that variable is absent. They never fall
back to `KP_DATABASE_URL` and never skip a missing required dependency. Unit/transport fixtures
are not evidence of the full GUI → production-composition journey.


## Real composition-root acceptance entrypoint

After installing the pinned browser (`pnpm --filter @knowledge-platform/document-web exec playwright install chromium`), run
`mise run document:poc:runtime`. See [the acceptance harness](../../tools/document-poc-runtime/README.md)
for the owned disposable database, process lifecycle, genuine in-flight/stalled-stream cases,
exact-source provenance and safe summary contract. The default harness builds its own binaries;
custom binary directories are permitted only in explicitly unqualified `--prebuilt` diagnostics.
No mocked API route, development server or system-browser fallback can qualify this gate.

The GitHub job emits `node tools/document-poc-runtime/ci-summary.mjs` output even after a failure.
This bounded summary intentionally omits raw log messages, paths, URLs, credentials, manifests
and working files. Screenshots/traces stay run-local; no additional upload action or durable
visual-review evidence is claimed. Runtime failure remains a failing required predecessor.
