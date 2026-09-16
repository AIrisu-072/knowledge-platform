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
├─ revision = initial revision
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
Documents
DocumentVersions
FileObjects
VersionFiles
Domain Outbox
Audit Outbox
```

Physical naming should follow repository naming conventions; the logical schema below is normative.

### 8.1 `folders`

Folder management is not implemented yet, but Document ownership already requires a folder reference. A minimal folder table and a system root folder are therefore permitted.

Minimum logical fields:

```text
folder_id
parent_folder_id
name
status/administrative fields only if required by existing SSOT
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

`current_version_id` is `NULL` in this capability.

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

### 8.4 `file_objects`

```text
file_id UUID PK
content_hash BYTEA NOT NULL      # SHA-256, 32 bytes
media_type TEXT NOT NULL
size_bytes BIGINT NOT NULL
storage_locator TEXT UNIQUE NOT NULL
created_at TIMESTAMPTZ NOT NULL
```

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

This capability creates only `PRIMARY`.

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
| absent | staging | > grace | `StaleStaging` |
| absent | final | > grace | `Orphan` |
| present | absent | any | `IntegrityViolation` |

The reconciler must never use original filenames as object identity.

Automatic periodic scheduling and destructive cleanup policy are deferred to an Operational Capability.

## 17. OSS and library selection freeze for this capability

### 17.1 Production dependencies

The capability uses the existing repository selection policy and freezes the following technology family:

```text
PostgreSQL 18.x
SQLx 0.9.x
Tokio 1.x
serde / serde_json
thiserror 2.x
uuid 1.x with UUIDv7 support
sha2 0.11.x with SHA-256
time 0.3.x
std / tokio filesystem APIs
```

Exact patch versions are lockfile-controlled and may receive compatible security/bugfix updates without reopening architecture, provided the capability contract and CI evidence remain valid.

### 17.2 Test dependencies

```text
testcontainers
tempfile
```

Integration tests use real PostgreSQL 18.x, not SQLite as a behavioral substitute.

### 17.3 Explicit non-dependencies for this capability

Do not add merely because they are already selected elsewhere or may be useful later:

```text
axum
tower-http
ORM frameworks
Firefly runtime crates
generic queue products
S3/MinIO clients
Tantivy/Lindera
OpenTelemetry exporter
SSPI
Office/PDF extraction libraries
Event Sourcing frameworks
```

## 18. Existing OSS Fit-Gap decision

### 18.1 Mayan EDMS

Decision: **REJECTED as a production dependency**.

Reasons:

- current licensing does not satisfy the repository's permissive-license gate;
- lifecycle/delete semantics do not match the Knowledge Platform authoritative model;
- it is a full DMS platform rather than a thin capability library.

Mayan may be used only as an architectural/operational reference; its code is not copied into this repository under the current policy.

### 18.2 Firefly OpenCore / Firefly Framework Rust

Decision for Capability 1: **no direct production dependency**.

Firefly is Apache-2.0 and contains useful implementation patterns, but direct crate adoption currently adds more framework coupling than it removes:

- `firefly-ecm::LocalStore` lacks the required staging/sync/atomic-finalization/reconciliation contract;
- `firefly-data-sqlx` depends on a broad Firefly graph and currently targets a different SQLx/framework stack;
- `firefly-transactional` provides propagation/nesting machinery beyond this capability's single aggregate transaction requirement;
- `firefly-eda-postgres` is coupled to Firefly EDA/tokio-postgres and does not insert business outbox rows inside this capability's SQLx business transaction;
- `firefly-testkit` does not replace the actual container runtime needed for real PostgreSQL integration testing.

### 18.3 Approved future selective reuse

For a later **Outbox Delivery Capability**, the following Apache-2.0 Firefly implementation ideas are approved as source-level reference candidates:

From `firefly-eda-postgres`:

- monotonic PostgreSQL cursor;
- per-consumer-group offsets;
- `LISTEN`/`NOTIFY` wake-up with polling fallback;
- stable advisory-lock key generation;
- at-least-once drain behavior.

From `firefly-eventsourcing`:

- attempt counting;
- retry limits;
- last-error tracking;
- dead-letter semantics.

If Firefly source is copied or modified later, the capability doing so must preserve applicable Apache-2.0 notices, record third-party attribution, and mark modifications. Capability 1 does not copy Firefly source.

## 19. Test strategy

Completion is evidence-based against invariants, not based on test count.

### 19.1 Domain unit tests

Verify at minimum:

- initial version number is `1`;
- initial lifecycle is `WORKING`;
- `current_version_id` remains `None`;
- one primary VersionFile is produced;
- invalid version numbers cannot be constructed;
- required title/invariant validation;
- invalid content-hash length rejected;
- negative file size rejected.

### 19.2 Application contract tests

Use test doubles to verify orchestration independent of PostgreSQL/filesystem:

```text
FileStorage finalization
    occurs before
DocumentRepository atomic create
```

Repository failure must not cause the Application to delete a finalized file.

### 19.3 Filesystem adapter tests

With real temporary directories verify:

- staging object creation;
- streaming write;
- SHA-256 calculation;
- exact size;
- sync/finalization;
- final read-back byte equality;
- no staging residue after success;
- storage identity unaffected by hostile/unusual original filename;
- same-filesystem atomic rename behavior;
- parent-directory durability step where required by target filesystem semantics.

### 19.4 PostgreSQL adapter tests

Use real PostgreSQL 18.x and migrations.

Verify DB-enforced invariants including:

- `(document_id, version_no)` uniqueness;
- Version→Document FK;
- VersionFile→Version/File FK;
- lifecycle CHECK/enum constraints;
- required NOT NULL constraints;
- unique storage locator;
- initial current-version behavior.

### 19.5 Atomic repository failure injection

Inject failure after each logical DB stage and prove zero partial authoritative state remains:

```text
FileObject INSERT
Document INSERT
DocumentVersion INSERT
VersionFile INSERT
Domain Outbox INSERT
Audit Outbox insertion
```

On failure, business rows, Domain Outbox rows, and Audit Outbox rows must all roll back.

### 19.6 Full vertical-slice integration test

Use real filesystem and real PostgreSQL simultaneously:

```text
CreateDocument
  ↓
final immutable file
  +
Document
DocumentVersion #1 / WORKING
current_version_id = NULL
FileObject
VersionFile / PRIMARY
Domain Outbox
Audit Outbox
  ↓
GetDocument
  ↓
FileStorage.open
  ↓
input bytes == output bytes
SHA-256 matches
```

### 19.7 File-failure tests

Inject:

```text
write failure
sync failure
finalize/rename failure
```

No authoritative DB rows, Domain Outbox rows, or Audit Outbox rows may be created.

### 19.8 DB-failure test after file finalization

Expected state:

```text
finalized file exists
DB reference absent
```

This is a safe orphan candidate, not grounds for immediate Application deletion.

### 19.9 Commit outcome unknown test

Simulate the case where PostgreSQL committed but the caller cannot know the result due to connection failure.

Acceptance:

- application surfaces `CommitOutcomeUnknown` (or an equivalent explicit internal category);
- finalized file is retained;
- pre-generated IDs can be used for re-query;
- committed DB reference + file remains intact.

Also verify the complementary case where commit did not occur and the file becomes an orphan candidate.

### 19.10 Reconciliation tests

Prove all required classifications:

```text
Healthy
StaleStaging
Orphan
IntegrityViolation
```

### 19.11 GetDocument integrity test

If a DB FileObject reference exists but its physical object has been removed, binary read must surface `IntegrityViolation`, not `DocumentNotFound`.

### 19.12 SQL/migration verification

CI must verify migrations against a clean PostgreSQL 18.x instance and keep SQLx offline query metadata synchronized, including an equivalent of:

```text
cargo sqlx prepare --check --workspace
```

when compile-time checked SQL is used.

### 19.13 Architecture lint

Existing architecture enforcement must be extended so at minimum:

```text
document-domain              X sqlx / filesystem / axum
document-application         X sqlx / physical filesystem paths
document-repository-postgres -> domain/application allowed
document-storage-fs          -> domain/application allowed
```

Violations fail CI.

### 19.14 License/security gate

All added dependencies must pass the repository's existing `cargo-deny`/security policy. No Firefly git dependency is introduced by this capability.

## 20. Performance policy

Correctness and authoritative integrity are the acceptance priority for Capability 1.

No arbitrary latency SLO is frozen before representative workload evidence exists.

The implementation should nevertheless make it possible to capture at least:

```text
file size
write duration
sync/finalize duration
DB transaction duration
total CreateDocument duration
```

Future SLOs are derived from measured workload, not guessed during this design.

## 21. Definition of Done

Capability 1 is complete only when evidence demonstrates:

```text
CreateDocument                         PASS
GetDocument                            PASS
real file persistence                  PASS
PostgreSQL authoritative persistence   PASS
initial DocumentVersion = WORKING      PASS
current_version_id = NULL              PASS
FileObject integrity metadata          PASS
Domain Outbox same transaction         PASS
Mandatory Audit same transaction       PASS
partial DB state prevention            PASS
file-first failure safety              PASS
CommitOutcomeUnknown safety            PASS
orphan/missing-file detection          PASS
architecture dependency rules          PASS
license/security CI                    PASS
main CI                                PASS
```

The following are explicitly not required to declare this capability complete:

```text
Version #2+
Publish / Withdraw
current-version switching
ReadState
AccessPolicy
Outbox delivery worker
Audit Store delivery
Search indexing
Extraction
HTTP API
UI
```

The resulting system state is:

> A version-management-ready Document Platform authoritative core that can safely create, persist, and read the first WORKING version of a document while preserving DB/file integrity across expected failure boundaries.

## 22. Design freeze and change control

After this spec is reviewed and marked approved, implementation must treat it as the Capability 1 design baseline.

Implementation agents may choose local code structure and algorithms freely **inside** these boundaries, but may not silently replace:

- the ownership model;
- module dependency direction;
- PostgreSQL as authoritative DB;
- file-first durability ordering;
- Domain/Audit Outbox atomicity requirement;
- lifecycle semantics;
- selected technology family;
- permissive-license gate.

If PoC/implementation evidence disproves an assumption, the required flow is:

```text
Evidence
  ↓
Design / selection change proposal
  ↓
review + approval
  ↓
SSOT/design update
  ↓
implementation change
```

No technology substitution is justified solely by implementation convenience.

## 23. Deferred decisions

These decisions are intentionally left to later capabilities and must not be accidentally resolved inside Capability 1:

- T4 current-version behavior when the current version is withdrawn (restore prior published vs `NULL`);
- concurrent creation of Version #2+;
- publication OCC/locking details;
- ReadState implementation;
- AccessPolicy/AD integration;
- API idempotency key policy;
- Outbox delivery/retry/DLQ implementation;
- Audit Store product selection;
- Search/Extraction integration;
- object-store migration/S3 support;
- automatic reconciliation scheduling and destructive cleanup policy.

---

## Review status

Sections 1–5 underlying this document were approved interactively. This consolidated document remains `DRAFT` until the written form is reviewed as a whole. After written-spec approval, the next step is a separate detailed Implementation Plan; production implementation must not start before that plan is reviewed according to the project's development-assurance workflow.
