# Document Platform Authoritative Core — Design v0

- Status: **DRAFT — approved sections consolidated; awaiting written-spec review**
- Date: 2026-09-16
- Capability: `Document Authoritative Core — Create/Get v0`
- Repository baseline: `main@8ad4aa074a42308c2767bb0e0b9451dbb879c4ca`
- Design branch: `design/document-authoritative-core-v0`
- Scope class: Product Capability / Architectural

## 1. Purpose

This design freezes the first Product Capability after Repository Bootstrap.

The capability establishes the authoritative core of the Document Platform so that one managed document can be created with its initial immutable binary and initial `DocumentVersion`, persisted safely across filesystem and PostgreSQL boundaries, and read back without violating the platform invariants.

This is **not** the complete version-management feature. It is the minimum authoritative substrate on which later version creation, publication, current-version switching, read-state, access policy, search indexing, and HTTP APIs can be built without changing the core ownership model.

## 2. Normative context

This design must remain consistent with the existing repository SSOT, especially:

- `spec/architecture/architecture-contract-v0.md`
- `spec/architecture/system-architecture-v0.md`
- `spec/data/logical-data-model-v0.md`
- `spec/data/transaction-consistency-requirements-v0.md`
- `spec/operations/error-handling-resilience-requirements-v0.md`
- `spec/operations/observability-audit-requirements-v0.md`
- `spec/selection/db-selection-criteria-v0.md`
- `spec/selection/library-tool-selection-v0.md`
- `spec/selection/rust-library-matrix-v0.md`

If this design conflicts with a higher-priority normative SSOT document, the higher-priority contract wins and this design must be amended before implementation proceeds.

## 3. Capability scope

### 3.1 Included use cases

The capability implements two application use cases:

1. `CreateDocument`
   - create a new `Document` identity;
   - create `DocumentVersion #1` in `WORKING` state;
   - persist the primary immutable binary to local filesystem storage;
   - create `FileObject` and `VersionFile` authoritative records;
   - persist document/version metadata;
   - atomically persist required Domain Outbox and Audit Outbox records with the authoritative DB state.

2. `GetDocument`
   - load the authoritative document, initial version, and file metadata;
   - allow binary read through the storage port;
   - distinguish user-level not-found from authoritative-integrity failure.

### 3.2 Explicitly excluded

The following are **not** part of this capability:

- `DocumentVersion #2+` creation;
- `PUBLISHED` transition;
- `WITHDRAWN` transition;
- `Document.current_version_id` switching;
- publication scheduling;
- historical-version UI/API;
- `ReadState`;
- AccessPolicy enforcement;
- Folder CRUD and Category CRUD;
- Search Platform implementation;
- extraction/indexing;
- HTTP/OpenAPI transport;
- UI;
- Outbox delivery worker;
- Audit Store delivery;
- Windows/SSPI identity acquisition;
- automatic periodic orphan cleanup scheduler.

Later capabilities must extend this design rather than bypass it.

## 4. Architectural boundary

The initial implementation uses a modular-monolith boundary while remaining deployable as one Rust application.

```text
                    +------------------------+
                    |    document-domain     |
                    +-----------^------------+
                                |
                    +-----------+------------+
                    | document-application   |
                    +--------^---------^------+
                             |         |
                     Port    |         |    Port
                             |         |
              +--------------+--+   +--+----------------+
              | postgres adapter |   | filesystem adapter |
              +------------------+   +--------------------+
```

Recommended implementation modules/crates:

```text
crates/
├─ document-domain
├─ document-application
├─ document-repository-postgres
└─ document-storage-fs
```

Crate granularity may be reduced if repository-bootstrap architecture rules prefer fewer crates, but the dependency directions are normative.

### 4.1 Dependency rules

`document-domain` must not depend on:

- SQLx;
- PostgreSQL types;
- filesystem path APIs;
- axum;
- Firefly Framework;
- search/extraction implementations.

`document-application` may depend on domain types and application ports, but not on PostgreSQL or physical filesystem details.

Infrastructure adapters implement application/domain contracts.

## 5. Domain model

### 5.1 Core types

The capability owns or introduces these domain concepts:

```text
DocumentId
DocumentVersionId
FileId
FolderId
EventId
AuditEventId

PrincipalRef
Document
DocumentVersion
FileObject
VersionFile

VersionNo
LifecycleState
ContentHash
Metadata
StorageKey
Timestamp
```

Infrastructure representations are not exposed through the domain contract.

Examples:

- PostgreSQL `JSONB` is an adapter representation of `Metadata`;
- a filesystem `PathBuf` is an adapter implementation detail behind `StorageKey`;
- SQL row types never enter Domain/Application public interfaces.

### 5.2 Initial aggregate state

`CreateDocument` produces:

```text
Document
├─ document_id
├─ folder_id
├─ current_version_id = None
├─ revision = 0
└─ metadata

DocumentVersion #1
├─ document_version_id
├─ document_id
├─ version_no = 1
├─ lifecycle_state = WORKING
├─ title
├─ created_by = PrincipalRef
├─ metadata
└─ created_at

FileObject
├─ file_id
├─ content_hash = SHA-256
├─ size_bytes
├─ media_type
├─ storage_key
└─ created_at

VersionFile
├─ document_version_id
├─ file_id
├─ role = PRIMARY
├─ ordinal
└─ original_filename
```

The initial `WORKING` version is **not** a current/published version. `Document.current_version_id` therefore remains `NULL`/`None` until a later publication capability atomically establishes a current `PUBLISHED` version.

### 5.3 Lifecycle invariant

Persisted lifecycle state remains exactly:

```text
WORKING
PUBLISHED
WITHDRAWN
```

This capability only creates `WORKING`.

UI concepts such as Draft, Current, Superseded, Archived, Deleted, or Waiting for Publication are not added as persisted lifecycle values.

## 6. Application ports

The Application layer depends on the following conceptual ports:

```text
IdGenerator
Clock
FileStorage
DocumentRepository
```

`PrincipalRef` is supplied as trusted application context for this capability. Identity acquisition and Windows/SSPI integration are separate capabilities.

### 6.1 Repository atomic contract

The Application layer does not expose a generic DB transaction primitive.

Instead, the repository exposes an operation equivalent to:

```text
DocumentRepository::create_initial_document(CreateInitialDocumentRecord)
```

Its contract is:

> Authoritative Document, Version, File reference, Domain Outbox, and mandatory Audit Outbox state is committed atomically, or no DB state is committed.

The adapter owns the concrete SQL transaction.

## 7. File storage and File Commit Protocol

DB ACID and filesystem durability cannot share one atomic transaction. The capability therefore uses a **file-first / DB-second** protocol.

### 7.1 Storage namespaces

The local store separates:

```text
<storage-root>/
├─ staging/
└─ objects/
```

A final storage key is derived only from system-generated `FileId` (optional deterministic sharding is allowed). Original filenames never determine storage identity or filesystem location.

The staging and final object roots must reside on the **same filesystem/mount** so finalization can use a same-filesystem atomic rename.

### 7.2 Write/finalize flow

```text
input stream
   ↓
staging object
   ↓
stream write + SHA-256 + byte count
   ↓
flush / sync_all
   ↓
same-filesystem atomic rename
   ↓
final immutable object
   ↓
PostgreSQL transaction
```

The filesystem adapter returns a `StoredFile` containing at least:

- `StorageKey`;
- SHA-256 content hash;
- size;
- media type where applicable.

### 7.3 Directory durability refinement

`sync_all` on the file plus atomic rename provides the intended protocol shape, but rename-entry crash durability can depend on platform/filesystem directory-sync semantics. The implementation PoC must explicitly verify and, on supported Unix targets, perform parent-directory synchronization (or an equivalent durability step) before this capability is considered crash-durable.

This refinement does not change the approved file-first ordering; it closes a durability detail inside the filesystem adapter.

### 7.4 Failure policy

The Application layer must **not** immediately delete a finalized file when the subsequent DB operation fails.

Reason: a DB commit error can represent an unknown commit outcome. Deleting the file could create the forbidden state:

```text
DB FileObject reference exists
+
physical file missing
```

Safety rule:

> A finalized file is retained until DB state proves it is an orphan and the configured grace period has elapsed.

## 8. PostgreSQL authoritative schema

The first migration contains the minimum authoritative structures needed by this capability:

```text
folders
documents
document_versions
file_objects
version_files
outbox_events
audit_outbox_events
```

Physical naming should follow repository naming conventions; the logical schema below is normative.

### 8.1 `folders`

Folder management is not implemented yet, but Document ownership already requires a folder reference. A minimal folder table and a system root folder are therefore permitted.

Minimum logical fields:

```text
folder_id
parent_folder_id
name
status
revision
created_at
```

`CreateDocument` accepts an existing `FolderId`.

### 8.2 `documents`

```text
document_id UUID PK
folder_id UUID FK
current_version_id UUID NULL
revision BIGINT NOT NULL
metadata JSONB NOT NULL
created_at TIMESTAMPTZ NOT NULL
```

`current_version_id` is `NULL` in this capability. The migration may add its FK after both `documents` and `document_versions` exist; the later Publish capability must additionally enforce that any non-null current version belongs to the same document and is `PUBLISHED`.

### 8.3 `document_versions`

```text
document_version_id UUID PK
document_id UUID FK
version_no BIGINT NOT NULL
lifecycle_state {WORKING,PUBLISHED,WITHDRAWN}
title TEXT NOT NULL
revision_reason TEXT NULL
approved_at TIMESTAMPTZ NULL
scheduled_publish_at TIMESTAMPTZ NULL
published_at TIMESTAMPTZ NULL
withdrawn_at TIMESTAMPTZ NULL
effective_from TIMESTAMPTZ NULL
effective_to TIMESTAMPTZ NULL
created_by_identity_provider TEXT
created_by_principal_id TEXT
metadata JSONB NOT NULL
created_at TIMESTAMPTZ NOT NULL
UNIQUE(document_id, version_no)
```

Database constraints must at least require `published_at IS NOT NULL` for `PUBLISHED` and `withdrawn_at IS NOT NULL` for `WITHDRAWN`.

### 8.4 `file_objects`

```text
file_id UUID PK
content_hash BYTEA NOT NULL
media_type TEXT NOT NULL
size_bytes BIGINT NOT NULL
storage_locator TEXT UNIQUE NOT NULL
created_at TIMESTAMPTZ NOT NULL
```

Database constraints must require `octet_length(content_hash) = 32` and `size_bytes >= 0`.

`content_hash` is integrity/provenance data in v0 and is **not** a deduplication key. No `UNIQUE(content_hash)` constraint is added.

### 8.5 `version_files`

```text
document_version_id UUID FK
file_id UUID FK
role {PRIMARY,ATTACHMENT}
ordinal INTEGER
original_filename TEXT NOT NULL
PRIMARY KEY(document_version_id, file_id)
```

This capability creates only `PRIMARY`. A partial unique index must enforce at most one `PRIMARY` row per `document_version_id`.

## 9. Identifier policy

IDs are generated by the Application layer before filesystem finalization and DB transaction start.

Use UUIDv7 for stable entity/event identities unless an existing SSOT contract requires another representation.

At minimum:

```text
DocumentId
DocumentVersionId
FileId
Domain Event IDs
Audit Event IDs
```

are available before persistence starts.

This allows the storage key and DB identities to share one stable application-generated identity without making Domain code dependent on PostgreSQL ID generation.

## 10. CreateDocument transaction

After file finalization, the PostgreSQL adapter performs one authoritative transaction equivalent to:

```text
BEGIN

1. validate referenced Folder exists
2. INSERT FileObject
3. INSERT Document
4. INSERT DocumentVersion #1 (WORKING)
5. INSERT VersionFile (PRIMARY)
6. INSERT Domain Outbox records
7. INSERT mandatory Audit Outbox records

COMMIT
```

Any failure before successful commit rolls back all DB changes.

The initial create operation creates a new aggregate and therefore does not yet require publish-time row locking/OCC. `READ COMMITTED` is sufficient for this capability unless PoC evidence demonstrates otherwise.

Version creation concurrency and publication/current-version switching are deferred and will introduce explicit OCC/locking rules.

## 11. Domain Outbox and Audit Outbox

Domain events and Audit events are separate responsibilities and use separate logical tables/contracts.

### 11.1 Domain Outbox

Purpose:

- future Search/Extraction synchronization;
- integration subscribers;
- eventual-consistency projections.

Minimum logical fields:

```text
event_id UUID PK
event_type TEXT
aggregate_type TEXT
aggregate_id UUID
payload JSONB
occurred_at TIMESTAMPTZ
available_at TIMESTAMPTZ
attempt_count INTEGER
delivered_at TIMESTAMPTZ NULL
```

Initial events include the semantic equivalents of:

```text
DocumentCreated
DocumentVersionCreated
```

### 11.2 Audit Outbox

Purpose:

- mandatory, non-sampled audit evidence;
- later delivery to the dedicated Audit Store.

Minimum logical fields:

```text
event_id UUID PK
event_type TEXT
source TEXT
subject TEXT
actor_identity_provider TEXT
actor_principal_id TEXT
resource_id UUID
resource_version_id UUID NULL
result TEXT
trace_id TEXT NULL
data JSONB
occurred_at TIMESTAMPTZ
attempt_count INTEGER
delivered_at TIMESTAMPTZ NULL
```

The envelope should remain compatible with the repository's CloudEvents-oriented audit contract, but Audit Store delivery is outside this capability.

Initial audit events include semantic equivalents of:

```text
document.created
document.version.created
```

### 11.3 Atomicity

A successful authoritative business commit implies required Domain Outbox and mandatory Audit Outbox rows exist in the same transaction.

A rolled-back business operation implies none of those rows exist.

## 12. CreateDocument application flow

```text
CreateDocument(command)
      │
      ├─ application validation
      ├─ generate stable IDs
      ├─ FileStorage.put_immutable(...)
      │    ├─ stage
      │    ├─ write/hash/count
      │    ├─ sync
      │    └─ finalize atomically
      │
      ├─ construct valid Domain state
      └─ DocumentRepository.create_initial_document(...)
               ↓
          atomic DB commit
```

Search and extraction are not invoked synchronously inside this flow.

## 13. GetDocument and binary read

`GetDocument` loads authoritative metadata from PostgreSQL.

Binary access occurs separately through:

```text
FileStorage.open(StorageKey)
```

If the Document does not exist, return a not-found application/domain result.

If the DB contains a `FileObject` reference but the physical object does not exist, return an `IntegrityViolation`, not `DocumentNotFound`.

## 14. Error and recovery model

The capability needs at least these application-level error categories:

```text
Validation
FolderNotFound
DocumentNotFound
StorageWriteFailed
StorageSyncFailed
StorageFinalizeFailed
RepositoryUnavailable
IntegrityViolation
CommitOutcomeUnknown
Internal
```

Raw `sqlx::Error` and `std::io::Error` must be mapped inside infrastructure adapters and must not leak through the Domain/Application contract.

HTTP/RFC 9457 mapping is deferred to the Common Document API capability.

## 15. Crash-state model

### 15.1 Staging crash

```text
staging file exists
DB state absent
```

Old staging files are safe cleanup candidates after a grace period.

### 15.2 Finalized file, DB absent

```text
final object exists
DB reference absent
```

This is a finalized orphan candidate. It is retained until reconciliation proves it is unreferenced and old enough for cleanup.

### 15.3 Successful DB commit

```text
final object exists
DB reference exists
```

Healthy authoritative state.

### 15.4 Commit outcome unknown

```text
final object exists
DB commit result unknown to caller
```

The Application must not delete the file. Pre-generated IDs allow DB re-query. A later reconciler uses DB state as authority.

## 16. Storage reconciliation

The capability includes reconciliation **classification/detection**, not a full operational scheduler.

Required classifications:

| DB | Storage | Age | Classification |
|---|---|---|---|
| present | present | any | `Healthy` |
| absent | staging | grace elapsed | `StaleStaging` |
| absent | final | grace elapsed | `Orphan` |
| present | absent | any | `IntegrityViolation` |

Final cleanup is allowed only after the grace period and positive proof that no authoritative DB reference exists.

## 17. OSS and library selection

Selection is capability-scoped and frozen for implementation unless evidence triggers the change-control process.

### 17.1 Production baseline

```text
PostgreSQL 18.x
  initial PoC baseline: PostgreSQL 18.6
  later 18.x patch updates: dependency maintenance, not architecture change

Rust
├─ Tokio 1.x
├─ SQLx 0.9.x
├─ serde / serde_json
├─ thiserror 2.x
├─ uuid 1.x with UUIDv7 support
├─ sha2 0.11.x using SHA-256
└─ time 0.3.x

Filesystem
└─ std / tokio::fs
```

SQLx must enable only the features required by PostgreSQL/Tokio/migrations/macros/uuid/json/time. Multi-database support is not enabled merely for abstraction.

HTTP dependencies such as axum/tower-http are selected platform-wide but are not direct dependencies of this capability unless actually required by another existing crate.

### 17.2 Test-only baseline

```text
testcontainers 0.28.x
tempfile 3.x
```

Integration tests target a pinned PostgreSQL 18.6 official image rather than SQLite substitution.

### 17.3 Existing OSS evaluation

#### Mayan EDMS

- mature DMS and useful design reference;
- rejected as a production/code dependency under the current license gate because the current release is GPL-2.0;
- no Mayan source code is copied into this repository under the current policy.

#### Firefly OpenCore / Firefly Framework Rust

- Apache-2.0 and valuable as a reference implementation;
- not adopted as a runtime dependency in this capability;
- direct crate use would introduce mismatched domain semantics and/or a larger framework dependency graph than needed;
- Firefly ECM `LocalStore` does not implement the required staging + sync + atomic-rename + orphan-reconciliation durability protocol;
- Firefly `data-sqlx` currently couples to a wider Firefly stack and an older SQLx baseline;
- Firefly `eda-postgres` and `eventsourcing` remain reusable references for a later **Outbox Delivery Capability**.

Future source-level reuse of Apache-2.0 Firefly algorithms or code requires retained license/copyright attribution and a modification notice; such reuse is not part of this capability.

## 18. Test strategy and acceptance criteria

Completion is invariant-driven. Test count alone is not evidence.

### 18.1 Domain unit tests

Prove at least:

- initial version number is `1`;
- initial lifecycle is `WORKING`;
- `current_version_id = None`;
- initial Document `revision = 0`;
- one primary `VersionFile` is created;
- invalid version numbers, content hashes, file sizes, and required domain values are rejected.

### 18.2 Application contract tests

With test doubles:

- prove file finalization happens before repository create;
- prove repository failure does not trigger immediate final-file deletion;
- prove generated IDs are stable across the operation;
- prove Search/Extraction are not synchronously invoked.

### 18.3 Filesystem adapter tests

Using real temporary filesystem storage:

- stage/write/hash/count/sync/finalize/read-back;
- verify staging disappears after successful finalization;
- verify SHA-256 and byte size;
- verify original filename cannot affect storage path;
- inject write/sync/finalize failure;
- validate same-filesystem rename assumption and parent-directory durability behavior on the supported Linux production target.

### 18.4 PostgreSQL adapter tests

Against PostgreSQL 18.6:

- migration succeeds on clean DB;
- DB constraints reject invalid lifecycle values/timestamp combinations;
- `(document_id, version_no)` is unique;
- FK violations are rejected;
- SHA-256 length and non-negative file size constraints hold;
- at most one primary file per version;
- successful `create_initial_document` creates all authoritative/outbox/audit rows;
- injected failure at each internal write point leaves no partial DB state.

### 18.5 Full vertical-slice test

Use real filesystem + real PostgreSQL:

```text
CreateDocument
→ final file
→ authoritative DB rows
→ Domain Outbox + Audit Outbox
→ GetDocument
→ FileStorage.open
→ input bytes == output bytes
→ SHA-256 matches
```

### 18.6 Commit outcome unknown

Fault injection must cover the case where PostgreSQL committed but the caller cannot observe success.

Acceptance:

- return/map `CommitOutcomeUnknown`;
- do not remove the finalized file;
- permit lookup by pre-generated IDs;
- preserve healthy DB-reference + file state when commit actually succeeded;
- classify an unreferenced finalized object as an orphan only after DB proof and grace period.

### 18.7 Reconciliation

Prove all required classifications:

- `Healthy`;
- `StaleStaging`;
- `Orphan`;
- `IntegrityViolation`.

### 18.8 SQLx metadata and CI

Generate and commit SQLx offline query metadata where the chosen SQLx 0.9 workflow requires it, and gate it in CI with the matching `cargo sqlx prepare --check --workspace` flow.

### 18.9 Architecture policy

Machine-enforce at least:

```text
document-domain X sqlx
document-domain X axum
document-domain X filesystem implementation APIs

document-application X sqlx
document-application X physical filesystem paths

postgres adapter -> domain/application allowed
filesystem adapter -> domain/application allowed
```

The existing formatting, Clippy, test, cargo-deny, OSV, Gitleaks, workflow lint, portability, container, SBOM, and Development Assurance gates remain required.

## 19. Definition of Done

The capability is complete only when all of the following have evidence:

```text
CreateDocument                         PASS
GetDocument                            PASS
real immutable file persistence        PASS
PostgreSQL authoritative persistence   PASS
initial Version = WORKING / #1         PASS
current_version_id = NULL              PASS
Document.revision = 0 on create        PASS
FileObject integrity metadata          PASS
Domain Outbox same transaction         PASS
Mandatory Audit same transaction       PASS
no partial DB state                    PASS
file-first failure safety              PASS
commit-outcome-unknown safety          PASS
orphan/missing-file detection          PASS
architecture dependency rules          PASS
license/security CI                    PASS
main CI                                PASS
```

The following are intentionally **not** required for this Definition of Done:

- Version #2+;
- publish/withdraw;
- current version switching;
- ReadState;
- AccessPolicy;
- Outbox delivery worker/retry/DLQ;
- Audit Store delivery;
- Search/Extraction;
- HTTP API;
- UI.

## 20. Design Freeze and change control

After written approval, implementation must use this architecture and capability-scoped library baseline.

Implementation agents are free to choose local code structure and algorithms inside the approved contracts, but must not silently change:

- ownership boundaries;
- lifecycle semantics;
- DB/file commit ordering;
- outbox/audit atomicity;
- selected primary dependencies;
- capability scope;
- acceptance criteria.

If implementation or PoC evidence invalidates a frozen assumption, the required flow is:

```text
Evidence
↓
Design / Selection change proposal
↓
Explicit approval
↓
SSOT / Design update
↓
Implementation change
```

No dependency is replaced merely because another library is more convenient during implementation.

## 21. Next step after approval

After this Design Spec is approved, create a detailed implementation plan under:

```text
docs/superpowers/plans/2026-09-16-document-authoritative-core-implementation.md
```

The plan must use TDD, map tasks to the acceptance criteria above, specify concrete file paths and verification commands, and preserve the Development Assurance evidence chain.
