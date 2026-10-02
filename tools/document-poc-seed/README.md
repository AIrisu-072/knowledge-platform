# Synthetic Document PoC seed

This thin human client uses the existing generated `@knowledge-platform/document-api-client`
operations and `BinaryTransportBridge`. It has no repository/SQL/application connection and no
new third-party dependencies. The scoped build uses the existing workspace TypeScript compiler
and emits the existing client unchanged apart from Node ESM import resolution in build output.

## Prerequisites and run

Use the repository-pinned Node/pnpm and an already installed, frozen workspace dependency graph.
Run the separate runtime commands in order: `document-server migrate`,
`document-server bootstrap-poc`, then human and agent `document-server serve` processes using
the same disposable PostgreSQL database/storage. The root bootstrap must expose the approved
fixed `poc-users` full permission and `poc-agents` read/readHistory policy.

From the repository root:

```sh
pnpm --dir tools/document-poc-seed test
KP_RUNTIME_MODE=poc \
KP_DOCUMENT_API_BASE_URL=http://127.0.0.1:8080 \
KP_POC_SEED_MANIFEST=/path/to/private-disposable-poc/seed-manifest.json \
pnpm --dir tools/document-poc-seed seed
```

Omitting `KP_POC_SEED_MANIFEST` uses this tool's ignored `.state/manifest.json`. Keep an overridden
path outside Git or explicitly ignored. Do not delete the manifest and reseed the same database.
The endpoint must be a loopback HTTP origin; the server must report the fixed `poc-human`
session. No user-selectable principal/group, production identity, or scheduler is configured.
The CLI prints fixture document/folder IDs and a fixture-definition SHA-256, not database URLs.

## Fixtures and permissions

All content is synthetic Japanese UTF-8 plain text:

- PoC Shared: 規程サンプル (two published versions/revisions), マニュアルサンプル, 通達サンプル
- Agent Sandbox: Agent検証用文書 (two published versions/revisions)
- Human Only: Human専用検証文書 (two published versions/revisions)

The sandbox and human-only pairs provide distinct existing inputs for authorization probes.
Acceptance first proves the same pairs succeed through the Human API; identical or invented
IDs cannot qualify denial. Changing this fixture definition changes its hash, so older manifests
are rejected and only the owned disposable run is recreated.

Each folder receives an explicit policy before documents are created. The human group has
read/readHistory/write/publish/administer. Shared and sandbox policies grant the agent group
only read/readHistory. Human Only explicitly grants only the human group. No agent write grants
or agent write tools are introduced. Human/Agent authorization acceptance must run against real
composition-root instances; the fake transport tests alone do not establish authorization.

## Retry and conflict behavior

The manifest is written atomically and fsynced before each mutation and after its response.
Folder, policy, version and publication requests reuse their exact saved UUIDv7 operation IDs,
request payloads and OCC revisions on an interrupted retry. Initial create returns server-generated
UUIDv7 document/version/file IDs; these are persisted, never invented client-side. A successful
second run verifies stored IDs, metadata, versions, revision state, policies and downloaded file
hashes without resending mutation requests. Read/file-download audit may be appended normally
by the Common API; this is not a promise to suppress authorized-read audit.

A changed fixture definition, endpoint, existing same-name fixture, metadata, file bytes, ACL,
version or stored final state fails closed. Existing data/policy is never silently overwritten.
A per-manifest exclusive lock prevents simultaneous processes from racing. After a hard kill,
confirm the owning process stopped before manually removing only that manifest's `.lock` file.
The manifest itself must be retained.

### Initial-create unknown outcome limitation

`POST /v1/documents` does not accept a client operation ID or caller-supplied IDs. Its recovery
operation requires the server-generated document/version/file IDs. A lost response may hide a
committed creation. The seed therefore records pending creation first and refuses to send a
second create if those response IDs were not durably saved. It does not guess IDs, match by title,
add a new API, or assume an empty list proves rollback. This applies conservatively even if a
failure happened before submission. Preserve the manifest and diagnose the failure.

The fallback is an **operator-controlled full disposable PoC reset**: stop both servers; preserve
needed evidence; recreate only that explicitly identified disposable database and its dedicated
storage; move the old manifest aside; repeat migrate → bootstrap → serve → seed. The tool has
no cleanup/drop command. Never run that procedure on production/shared data or delete storage
that another environment uses.

## Verification boundary

`pnpm --dir tools/document-poc-seed test` type-checks/builds the actual generated client and runs
Node tests through its emitted code and BinaryTransportBridge. The fake transport is explicitly
unit/transport-contract coverage, not production backend evidence. It covers twice-run no new
mutations, interrupted exact replay, unknown initial creation, drift refusal, exclusive lock and
CLI mode/URL restrictions. Real runtime acceptance remains the coordinated C1/C3 gate.
