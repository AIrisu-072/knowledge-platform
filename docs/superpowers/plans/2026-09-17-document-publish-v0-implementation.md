# Document Publish v0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the first safe, idempotent publication operation for `DocumentVersion #1`, transitioning an initial Document from `WORKING/current=None/revision=0` to `PUBLISHED/current=Version #1/revision=1` with atomic Domain/Audit outbox records and durable publish-operation recovery.

**Architecture:** Preserve the existing `document-domain` → `document-application` ← infrastructure boundaries. Domain owns the initial publication invariant/transition; Application owns command validation, replay-before-preflight orchestration, event construction, and error translation; a new capability-specific `DocumentPublishRepository` port isolates Publish from the existing Create/Get repository interface; PostgreSQL owns the final OCC + short row-lock decision and atomically commits authoritative state, Domain Outbox, Audit Outbox, and the successful publish-operation result.

**Tech Stack:** Rust 1.98.1 / edition 2024, Tokio 1.x, PostgreSQL 18.6 (`postgres:18.6-bookworm`), SQLx 0.9.x, uuid 1.x UUIDv7, serde/serde_json, thiserror 2.x, time 0.3.x, tempfile 3.x, testcontainers 0.28.x.

**Spec:** `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`

## Global Constraints

- Implement **initial Publish only**. Do not add Version #2+, current-version replacement, Withdraw, approval workflow, scheduled publication, HTTP/OpenAPI transport, UI, Search indexing, outbox delivery, or a generic idempotency framework.
- `PublishOperationId` is caller-generated, Application-layer, UUIDv7-only, and must survive exact-command retry.
- Idempotency is Publish-specific and durable through `document_publish_operations`; records have no TTL or cleanup in v0.
- Same operation ID + same command returns the stored result without additional mutation/events; same ID + different command is `Conflict`.
- Different operation IDs are not replay-equivalent merely because they target the same already-published Version.
- OCC uses `expected_document_revision`; the PostgreSQL final transition also uses a short row-level lock.
- A new Publish must preflight the PRIMARY final object before the DB mutation. Missing/object-level unreadable authoritative storage is `IntegrityViolation`; dependency-level storage outage remains `StorageUnavailable`.
- Authoritative business state + one Publish Domain Outbox event + one mandatory Publish Audit Outbox event + one successful publish-operation result commit in one PostgreSQL transaction.
- `documents(document_id, current_version_id)` must reference `document_versions(document_id, document_version_id)` so a current Version cannot belong to another Document.
- `current_version_id != NULL` must represent a `PUBLISHED` Version through Domain invariants plus the atomic Repository transition; do not add lifecycle triggers or redundant helper columns.
- PostgreSQL commit errors remain conservatively `CommitOutcomeUnknown`; Application never retries with a new operation ID.
- Do not rewrite `0001_document_authoritative_core.sql`; add migration `0002_document_publish_v0.sql`.
- Preserve existing architecture, security, license, portability, container, SBOM, SQLx, and Development Assurance gates.
- Production dependency direction must remain unchanged. Infrastructure may depend on Application/Domain; Domain/Application must not gain PostgreSQL or physical filesystem dependencies.

## Execution prerequisite

Do not start production implementation from the Design branch. After Design PR #5 is green and merged, create/reset implementation branch `feat/document-publish-v0` from the exact merged `main` head, then execute this plan in order.

---

## File Structure

The implementation is expected to touch or create these focused units:

```text
crates/document-domain/src/
├─ document.rs                 # initial Publish invariant/transition + initial-published restoration
├─ error.rs                    # Publish-specific Domain invariant errors
└─ lib.rs                      # exports + Domain tests

crates/document-application/src/
├─ command.rs                  # PublishOperationId / PublishDocumentCommand / PublishDocumentResult
├─ error.rs                    # Publish Application/Repository errors + storage unreadable category
├─ events.rs                   # Publish Domain/Audit event constants and records
├─ ports.rs                    # Publish records/candidate + DocumentPublishRepository
├─ service.rs                  # replay → candidate → Domain validation → storage preflight → repository
└─ lib.rs                      # public exports

crates/document-application/tests/
├─ publish_document_contract.rs
└─ publish_vertical_slice.rs

crates/document-storage-fs/src/
└─ error.rs                    # object-level unreadable vs dependency-unavailable mapping

crates/document-repository-postgres/migrations/
└─ 0002_document_publish_v0.sql

crates/document-repository-postgres/src/
├─ lib.rs
├─ repository.rs               # existing Create/Get + delegates/implements Publish port
├─ publish.rs                  # Publish-specific SQL reads + atomic transaction
├─ publish_rows.rs             # SQLx rows for operation/candidate/locked state
├─ mapping.rs                  # Working + initial-Published authoritative reconstruction
└─ error.rs                    # existing statement/commit mapping retained

crates/document-repository-postgres/tests/
├─ publish_schema.rs
├─ publish_transaction.rs
└─ publish_concurrency.rs
```

Do not refactor unrelated Create/Get/reconciliation behavior while adding Publish.

---

### Task 1: Add the Domain initial-Publish transition and published-state restoration

**Files:**
- Modify: `crates/document-domain/src/document.rs`
- Modify: `crates/document-domain/src/error.rs`
- Modify: `crates/document-domain/src/lib.rs`

**Interfaces:**
- Consumes: existing `Document`, `DocumentVersion`, `InitialDocument`, `LifecycleState`, `OffsetDateTime`.
- Produces:
  - `PublishTransition`
  - `Document::publish_initial_version(&mut self, target: &mut DocumentVersion, published_at: OffsetDateTime) -> Result<PublishTransition, DomainError>`
  - `InitialDocument::restore_published(input: CreateInitialDocument, published_at: OffsetDateTime) -> Result<InitialDocument, DomainError>`
  - Publish-specific `DomainError` variants used by Application/PostgreSQL mapping.

- [ ] **Step 1: Write failing Domain tests for the successful transition**

Add tests in `crates/document-domain/src/lib.rs` that start from `InitialDocument::create(...)`, split it with `into_parts()`, call `publish_initial_version`, and assert:

```rust
let transition = document
    .publish_initial_version(&mut version, published_at)
    .expect("initial working version should publish");

assert_eq!(version.lifecycle_state(), LifecycleState::Published);
assert_eq!(version.published_at(), Some(published_at));
assert_eq!(document.current_version_id(), Some(version.document_version_id()));
assert_eq!(document.revision(), 1);
assert_eq!(transition.resulting_document_revision(), 1);
```

Also assert `approved_at` is not required and remains unchanged.

- [ ] **Step 2: Write failing Domain tests for invalid transitions**

Cover all frozen invariants with exact expected errors:

```rust
DomainError::VersionDocumentMismatch
DomainError::CurrentVersionAlreadySet
DomainError::VersionNotWorking
DomainError::RevisionOverflow
```

Construct cross-Document and non-WORKING cases inside the Domain test module where private-field fixtures can be built when necessary. Verify `revision` and lifecycle fields are unchanged after each rejected call.

- [ ] **Step 3: Run Domain tests and verify RED**

Run:

```bash
cargo test -p document-domain
```

Expected: FAIL because Publish transition/errors are not implemented.

- [ ] **Step 4: Implement `PublishTransition` and Domain errors**

Add errors equivalent to:

```rust
#[error("document version belongs to another document")]
VersionDocumentMismatch,
#[error("document already has a current version")]
CurrentVersionAlreadySet,
#[error("document version is not working")]
VersionNotWorking,
#[error("document revision overflow")]
RevisionOverflow,
```

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishTransition {
    resulting_document_revision: i64,
}

impl PublishTransition {
    pub const fn resulting_document_revision(self) -> i64 {
        self.resulting_document_revision
    }
}
```

- [ ] **Step 5: Implement the minimal initial-Publish transition**

Implement in this order so failures do not partially mutate Domain state:

```rust
pub fn publish_initial_version(
    &mut self,
    target: &mut DocumentVersion,
    published_at: OffsetDateTime,
) -> Result<PublishTransition, DomainError> {
    if target.document_id != self.document_id {
        return Err(DomainError::VersionDocumentMismatch);
    }
    if self.current_version_id.is_some() {
        return Err(DomainError::CurrentVersionAlreadySet);
    }
    if target.lifecycle_state != LifecycleState::Working {
        return Err(DomainError::VersionNotWorking);
    }

    let next_revision = self
        .revision
        .checked_add(1)
        .ok_or(DomainError::RevisionOverflow)?;

    target.lifecycle_state = LifecycleState::Published;
    target.published_at = Some(published_at);
    self.current_version_id = Some(target.document_version_id);
    self.revision = next_revision;

    Ok(PublishTransition {
        resulting_document_revision: next_revision,
    })
}
```

- [ ] **Step 6: Add restoration for an initial published aggregate**

Implement:

```rust
pub fn restore_published(
    input: CreateInitialDocument,
    published_at: OffsetDateTime,
) -> Result<Self, DomainError> {
    let mut aggregate = Self::create(input)?;
    aggregate
        .document
        .publish_initial_version(&mut aggregate.version, published_at)?;
    Ok(aggregate)
}
```

This is intentionally narrow: it restores only the two states supported after this capability—initial `WORKING` and initial `PUBLISHED`. Do not add a generic arbitrary lifecycle rehydration API.

- [ ] **Step 7: Run Domain tests and architecture checks**

Run:

```bash
cargo test -p document-domain
mise run arch:check
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/document-domain/src/document.rs crates/document-domain/src/error.rs crates/document-domain/src/lib.rs
git commit -m "feat: add initial document publish transition"
```

---

### Task 2: Define Publish Application contracts and an interface-segregated repository port

**Files:**
- Modify: `crates/document-application/src/command.rs`
- Modify: `crates/document-application/src/error.rs`
- Modify: `crates/document-application/src/events.rs`
- Modify: `crates/document-application/src/ports.rs`
- Modify: `crates/document-application/src/lib.rs`
- Create: `crates/document-application/tests/publish_document_contract.rs`

**Interfaces:**
- Consumes: Domain Publish transition/errors from Task 1 and existing `Clock`, `IdGenerator`, `FileStorage`.
- Produces:
  - `PublishOperationId`
  - `PublishDocumentCommand`
  - `PublishDocumentResult`
  - `PublishCommandIdentity`
  - `PublishOperationRecord`
  - `PublishCandidate`
  - `PublishInitialVersionRecord`
  - `DocumentPublishRepository`
  - Publish event constants.

- [ ] **Step 1: Write failing tests for `PublishOperationId` and command validation**

Use a deterministic valid UUIDv7 such as:

```rust
let valid = Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap();
let invalid = Uuid::from_u128(1);
```

Assert:

```rust
assert!(PublishOperationId::try_from_uuid(valid).is_ok());
assert!(PublishOperationId::try_from_uuid(invalid).is_err());
assert!(PublishDocumentCommand::new(..., -1, ...).is_err());
```

- [ ] **Step 2: Run the new Application test and verify RED**

Run:

```bash
cargo test -p document-application --test publish_document_contract
```

Expected: FAIL because Publish contracts do not exist.

- [ ] **Step 3: Add command/result types**

Implement a UUIDv7-only newtype:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublishOperationId(Uuid);

impl PublishOperationId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, ApplicationError> {
        if value.get_version_num() == 7 {
            Ok(Self(value))
        } else {
            Err(ApplicationError::Validation(
                "publish operation id must be UUIDv7".to_owned(),
            ))
        }
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}
```

Add `PublishDocumentCommand::new(...)` with non-negative `expected_document_revision` validation and getters. Add `PublishDocumentResult` with getters and a public `from_persisted(...)` constructor so the PostgreSQL adapter can reconstruct a stored result without exposing SQL row types.

- [ ] **Step 4: Add Publish identity/record/candidate port values**

Define exact values equivalent to:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCommandIdentity {
    pub publish_operation_id: PublishOperationId,
    pub document_id: DocumentId,
    pub target_document_version_id: DocumentVersionId,
    pub expected_document_revision: i64,
    pub principal: PrincipalRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOperationRecord {
    identity: PublishCommandIdentity,
    result: PublishDocumentResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCandidate {
    document: Document,
    version: DocumentVersion,
    file: FileObject,
    version_file: VersionFile,
}
```

`PublishOperationRecord` must expose `matches_identity(&PublishCommandIdentity) -> bool` and `result() -> &PublishDocumentResult`.

- [ ] **Step 5: Add the capability-specific Publish repository trait**

Do **not** widen existing `DocumentRepository`; existing Create/Get/reconciliation fakes should continue compiling unchanged.

Add:

```rust
#[allow(async_fn_in_trait)]
pub trait DocumentPublishRepository: Send + Sync {
    async fn get_publish_operation(
        &self,
        operation_id: PublishOperationId,
    ) -> Result<Option<PublishOperationRecord>, RepositoryError>;

    async fn get_publish_candidate(
        &self,
        document_id: DocumentId,
        target_version_id: DocumentVersionId,
    ) -> Result<PublishCandidate, RepositoryError>;

    async fn publish_initial_version(
        &self,
        record: PublishInitialVersionRecord,
    ) -> Result<PublishDocumentResult, RepositoryError>;
}
```

Define `PublishInitialVersionRecord` to contain one immutable `PublishOperationRecord`, one `DomainEventRecord`, and one `AuditEventRecord` with getters/`into_parts()`.

- [ ] **Step 6: Add Publish event constants**

Add exact event names:

```rust
pub const DOCUMENT_VERSION_PUBLISHED: &str = "DocumentVersionPublished";
pub const AUDIT_DOCUMENT_VERSION_PUBLISHED: &str = "document.version.published";
```

Continue using `DomainEventRecord` aggregate type `Document` and the existing mandatory Audit record shape.

- [ ] **Step 7: Extend error taxonomy without breaking Create**

Add Repository errors:

```rust
DocumentNotFound,
DocumentVersionNotFound,
Conflict,
BusinessRule,
```

Add Application errors with the same categories plus a Publish-specific structured unknown-commit variant:

```rust
PublishCommitOutcomeUnknown {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
},
```

Keep the existing Create `CommitOutcomeUnknown { document_id, document_version_id, file_id }` variant unchanged so existing tests/consumers do not break merely because Publish was added. Both represent the same conceptual error category until a future transport Error Registry maps them.

Add `StorageError::ObjectUnreadable` and map both `NotFound` and `ObjectUnreadable` to `ApplicationError::IntegrityViolation`; keep `Unavailable` mapped to `StorageUnavailable`.

- [ ] **Step 8: Run Application/static tests**

Run:

```bash
cargo test -p document-application --test publish_document_contract
cargo test -p document-application --no-run
mise run arch:check
```

Expected: PASS for type/validation tests; no production dependency boundary regression.

- [ ] **Step 9: Commit**

```bash
git add crates/document-application/src crates/document-application/tests/publish_document_contract.rs
git commit -m "feat: define document publish application contracts"
```

---

### Task 3: Implement storage error distinction and Publish Application orchestration

**Files:**
- Modify: `crates/document-storage-fs/src/error.rs`
- Modify: `crates/document-application/src/service.rs`
- Modify: `crates/document-application/tests/publish_document_contract.rs`

**Interfaces:**
- Consumes: Task 2 Publish contracts/port and Task 1 Domain transition.
- Produces: `DocumentService::publish_document(...)` with replay-before-preflight semantics.

- [ ] **Step 1: Add failing storage-error mapping test**

In the existing filesystem error unit-test module (or add one beside `error.rs` if none exists), assert:

```rust
assert_eq!(
    map_open_error(io::Error::from(io::ErrorKind::PermissionDenied)),
    StorageError::ObjectUnreadable,
);
assert_eq!(
    map_open_error(io::Error::from(io::ErrorKind::NotFound)),
    StorageError::NotFound,
);
```

A connection/device-style I/O error that is not object-specific must remain `StorageError::Unavailable`.

- [ ] **Step 2: Add failing Application orchestration tests**

Build `FakePublishRepository` implementing only `DocumentPublishRepository` plus the existing fake storage/clock/ID tools. Add tests proving this exact order/behavior:

```text
stored operation + same identity
  -> returns stored result
  -> candidate lookup count = 0
  -> storage open count = 0
  -> publish transaction count = 0

stored operation + different identity
  -> Conflict
  -> storage open count = 0

new operation + candidate + missing object
  -> IntegrityViolation
  -> publish transaction count = 0

new operation + storage dependency outage
  -> StorageUnavailable
  -> publish transaction count = 0

new operation + readable object
  -> calls publish transaction exactly once
```

Also assert a repository `CommitOutcomeUnknown` maps to `PublishCommitOutcomeUnknown` with the exact operation/document/version IDs.

- [ ] **Step 3: Run focused tests and verify RED**

Run:

```bash
cargo test -p document-application --test publish_document_contract
cargo test -p document-storage-fs
```

Expected: FAIL until orchestration/error mapping exists.

- [ ] **Step 4: Implement filesystem object-unreadable classification**

Update `map_open_error`:

```rust
pub(crate) fn map_open_error(error: io::Error) -> StorageError {
    match error.kind() {
        io::ErrorKind::NotFound => StorageError::NotFound,
        io::ErrorKind::PermissionDenied => StorageError::ObjectUnreadable,
        _ => StorageError::Unavailable,
    }
}
```

Do not reclassify write/sync/finalize behavior for this capability.

- [ ] **Step 5: Implement `DocumentService::publish_document` in a separate trait-bound impl**

Keep existing Create/Get methods under `R: DocumentRepository`. Add a second impl block requiring `R: DocumentRepository + DocumentPublishRepository` only for Publish, so current behavior remains available unchanged.

Implement this order:

```rust
let identity = PublishCommandIdentity::from_command(&command);

if let Some(stored) = self.repository.get_publish_operation(command.publish_operation_id()).await? {
    if stored.matches_identity(&identity) {
        return Ok(stored.result().clone());
    }
    return Err(ApplicationError::Conflict);
}

let candidate = self
    .repository
    .get_publish_candidate(command.document_id(), command.target_document_version_id())
    .await?;

let published_at = self.clock.now();
let (mut document, mut version, file, version_file) = candidate.into_parts();
let transition = document
    .publish_initial_version(&mut version, published_at)
    .map_err(map_publish_domain_error)?;

let storage_key = file.storage_key().clone();
let reader = self.storage.open(&storage_key).await?;
drop(reader);
```

Then generate one EventId and one AuditEventId, build the exact Design payload, construct `PublishOperationRecord` + `PublishInitialVersionRecord`, and call `repository.publish_initial_version(record)`.

- [ ] **Step 6: Build exact Publish Domain/Audit payloads**

Domain payload:

```rust
json!({
    "documentId": document.document_id().as_uuid().to_string(),
    "documentVersionId": version.document_version_id().as_uuid().to_string(),
    "resultingDocumentRevision": transition.resulting_document_revision(),
    "publishedAt": published_at,
    "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
})
```

Audit data:

```rust
json!({
    "publishOperationId": command.publish_operation_id().as_uuid().to_string(),
    "expectedDocumentRevision": command.expected_document_revision(),
    "resultingDocumentRevision": transition.resulting_document_revision(),
    "result": "success",
    "publishedAt": published_at,
})
```

Use event names from Task 2 and `resource_version_id = Some(target_version_id)`.

- [ ] **Step 7: Map Domain and Repository errors explicitly**

For the Application pre-validation transition:

```text
VersionDocumentMismatch      -> IntegrityViolation
CurrentVersionAlreadySet     -> Conflict
VersionNotWorking            -> BusinessRule
RevisionOverflow             -> IntegrityViolation
```

For the Repository result, preserve `DocumentNotFound`, `DocumentVersionNotFound`, `Conflict`, `BusinessRule`, `IntegrityViolation`, `Unavailable`, and structured Publish commit ambiguity.

- [ ] **Step 8: Run contract/storage tests and full Application tests**

Run:

```bash
cargo test -p document-storage-fs
cargo test -p document-application --test publish_document_contract
cargo test -p document-application
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/document-storage-fs/src/error.rs crates/document-application/src/service.rs crates/document-application/tests/publish_document_contract.rs
git commit -m "feat: orchestrate initial document publish"
```

---

### Task 4: Add the Publish migration and database ownership/idempotency constraints

**Files:**
- Create: `crates/document-repository-postgres/migrations/0002_document_publish_v0.sql`
- Create: `crates/document-repository-postgres/tests/publish_schema.rs`

**Interfaces:**
- Consumes: current `0001_document_authoritative_core.sql` schema.
- Produces: composite current-Version FK and permanent `document_publish_operations` schema.

- [ ] **Step 1: Write the failing Publish schema integration test**

Start disposable PostgreSQL `18.6-bookworm`, run `migrate(&pool)`, then assert:

1. a valid initial working Document/Version remains insertable;
2. assigning another Document's Version to `current_version_id` is rejected;
3. `document_publish_operations.expected_document_revision < 0` is rejected;
4. `resulting_document_revision != expected_document_revision + 1` is rejected;
5. operation target `(document_id, target_document_version_id)` must identify a Version owned by that Document;
6. duplicate `publish_operation_id` is rejected.

- [ ] **Step 2: Run schema test and verify RED**

Run:

```bash
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
```

Expected: FAIL because migration/table/composite FK do not exist.

- [ ] **Step 3: Add migration `0002_document_publish_v0.sql`**

Use exact constraint intent:

```sql
ALTER TABLE document_versions
    ADD CONSTRAINT uq_document_versions_document_id_version_id
    UNIQUE (document_id, document_version_id);

ALTER TABLE documents
    DROP CONSTRAINT fk_documents_current_version;

ALTER TABLE documents
    ADD CONSTRAINT fk_documents_current_version
    FOREIGN KEY (document_id, current_version_id)
    REFERENCES document_versions(document_id, document_version_id);

CREATE TABLE document_publish_operations (
    publish_operation_id UUID PRIMARY KEY,
    document_id UUID NOT NULL,
    target_document_version_id UUID NOT NULL,
    expected_document_revision BIGINT NOT NULL
        CHECK (expected_document_revision >= 0),
    actor_identity_provider TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    published_at TIMESTAMPTZ NOT NULL,
    resulting_document_revision BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_document_publish_operations_revision
        CHECK (resulting_document_revision = expected_document_revision + 1),
    CONSTRAINT fk_document_publish_operations_target
        FOREIGN KEY (document_id, target_document_version_id)
        REFERENCES document_versions(document_id, document_version_id)
);
```

Do not add TTL columns, cleanup state, triggers, or generic command tables.

- [ ] **Step 4: Run migration/schema tests**

Run:

```bash
cargo test -p document-repository-postgres --test schema_constraints -- --nocapture
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
mise run sqlx:check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/document-repository-postgres/migrations/0002_document_publish_v0.sql crates/document-repository-postgres/tests/publish_schema.rs
git commit -m "feat: add document publish persistence schema"
```

---

### Task 5: Add PostgreSQL Publish reads and make `GetDocument` understand initial-Published state

**Files:**
- Create: `crates/document-repository-postgres/src/publish_rows.rs`
- Create: `crates/document-repository-postgres/src/publish.rs`
- Modify: `crates/document-repository-postgres/src/lib.rs`
- Modify: `crates/document-repository-postgres/src/repository.rs`
- Modify: `crates/document-repository-postgres/src/mapping.rs`
- Modify: `crates/document-repository-postgres/src/rows.rs` only if shared row shape requires it
- Create or extend: `crates/document-repository-postgres/tests/publish_transaction.rs`

**Interfaces:**
- Consumes: Task 1 `InitialDocument::restore_published`, Task 2 Publish port values, Task 4 schema.
- Produces: operation lookup, candidate lookup, published-state authoritative mapping; no mutation yet beyond test seeding.

- [ ] **Step 1: Write failing read-path tests**

Seed a normal Create through the existing service/repository, then assert:

```text
get_publish_operation(unknown) -> None
get_publish_candidate(document, version) -> WORKING candidate with PRIMARY file metadata
```

Seed a `document_publish_operations` row directly and assert it reconstructs the exact `PublishCommandIdentity` and `PublishDocumentResult`.

Then manually set Version #1 to `PUBLISHED`, set `published_at`, set Document current version, and revision to 1 in one SQL transaction; assert existing `get_document()` returns a published authoritative aggregate rather than `IntegrityViolation`.

- [ ] **Step 2: Run focused repository tests and verify RED**

Run:

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
```

Expected: FAIL because Publish reads and published mapping do not exist; current `to_authoritative` accepts only initial WORKING state.

- [ ] **Step 3: Add focused SQLx row types**

In `publish_rows.rs`, define rows for:

```text
PublishOperationRow
PublishCandidateRow
LockedPublishStateRow
```

`PublishOperationRow` must contain all persisted command identity fields plus stored result fields. `PublishCandidateRow` must contain Document + target Version + PRIMARY File/VersionFile values needed to build the Domain/Application candidate without exposing SQLx types through the port.

- [ ] **Step 4: Implement operation lookup**

Add helper in `publish.rs`:

```rust
pub(crate) async fn get_publish_operation(
    pool: &PgPool,
    operation_id: PublishOperationId,
) -> Result<Option<PublishOperationRecord>, RepositoryError>
```

Select by PK and map actor fields through `PrincipalRef::new`; malformed persisted data becomes `RepositoryError::IntegrityViolation`.

- [ ] **Step 5: Implement candidate lookup with distinct missing/ownership errors**

The adapter must distinguish:

```text
missing Document                     -> DocumentNotFound
missing target Version               -> DocumentVersionNotFound
target exists but belongs elsewhere  -> IntegrityViolation
missing PRIMARY DB reference/file row -> IntegrityViolation
```

Do not infer `DocumentVersionNotFound` from a failed inner join. Query or left-join in a way that preserves these distinctions.

- [ ] **Step 6: Extend authoritative mapping for initial PUBLISHED state**

Keep existing Working validation. Add a published path that accepts only:

```text
version_no == 1
lifecycle_state == "PUBLISHED"
published_at == Some(_)
current_version_id == Some(document_version_id)
document_revision == 1
```

with the existing initial-version/file invariants. Reconstruct via:

```rust
InitialDocument::restore_published(create_input, published_at)
```

and convert to `AuthoritativeDocument`.

Reject inconsistent combinations such as `PUBLISHED + current=None`, wrong current Version, revision other than 1, or `WORKING + current Some` as `IntegrityViolation`.

- [ ] **Step 7: Implement `DocumentPublishRepository` read methods on PostgreSQL adapter**

Delegate `get_publish_operation` / `get_publish_candidate` to focused helpers in `publish.rs`. Leave `publish_initial_version` for Task 6.

If Rust requires the trait method to exist before Task 6, implement it temporarily only in the Task 6 commit—not with an `unimplemented!()` or success stub. Keep Task 5 tests calling read helpers directly or structure `publish.rs` public-to-crate helpers so every committed tree remains production-safe.

- [ ] **Step 8: Run read/Get regressions**

Run:

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test repository_contract -- --nocapture
cargo test -p document-application --test vertical_slice -- --nocapture
```

Expected: existing Working reads still PASS and manually-published initial state now round-trips.

- [ ] **Step 9: Commit**

```bash
git add crates/document-repository-postgres/src crates/document-repository-postgres/tests/publish_transaction.rs
git commit -m "feat: add document publish repository reads"
```

---

### Task 6: Implement the atomic PostgreSQL Publish transaction and idempotent replay

**Files:**
- Modify: `crates/document-repository-postgres/src/publish.rs`
- Modify: `crates/document-repository-postgres/src/repository.rs`
- Modify: `crates/document-repository-postgres/src/lib.rs` if module exports require it
- Modify: `crates/document-repository-postgres/tests/publish_transaction.rs`

**Interfaces:**
- Consumes: Task 2 `PublishInitialVersionRecord`, Task 1 Domain transition, Task 4 schema, Task 5 mappings.
- Produces: production `DocumentPublishRepository::publish_initial_version` with atomic state/event/audit/operation commit.

- [ ] **Step 1: Add failing successful-Publish transaction test**

Create a Document through existing Create flow, build one `PublishInitialVersionRecord`, call PostgreSQL `publish_initial_version`, then assert from SQL:

```text
Version #1.lifecycle_state = PUBLISHED
published_at = requested timestamp
Document.current_version_id = Version #1
Document.revision = 1
Publish Domain Outbox delta = 1
Publish Audit Outbox delta = 1
document_publish_operations delta = 1
```

Assert event payload values and the stored operation result exactly match the returned `PublishDocumentResult`.

- [ ] **Step 2: Add failing replay/misuse/OCC/state tests**

Add cases:

```text
same operation + same command -> identical result, no count/revision change
same operation ID + changed principal/document/version/revision -> Conflict
stale expected revision -> Conflict
current_version_id already Some(other) -> Conflict
target already PUBLISHED under a distinct operation -> Conflict
target WITHDRAWN -> BusinessRule
cross-Document target -> IntegrityViolation
```

- [ ] **Step 3: Add failing rollback atomicity test**

Force an error after the operation claim but before commit using a test-only invalid Audit/Outbox insert condition or a transaction helper seam. Assert rollback leaves:

```text
Version WORKING
current_version_id NULL
revision unchanged
no Publish operation row
no Publish Domain event
no Publish Audit event
```

Do not add a production flag that can disable atomic components.

- [ ] **Step 4: Run transaction tests and verify RED**

Run:

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
```

Expected: FAIL until atomic mutation is implemented.

- [ ] **Step 5: Implement the exact transaction algorithm**

Inside one SQLx transaction:

```text
1. lookup operation ID
2. if found: compare full identity; replay or Conflict
3. SELECT Document FOR UPDATE; missing -> DocumentNotFound
4. lookup operation ID again; replay or Conflict
5. load/lock target Version and validate ownership
6. validate DB PRIMARY reference still exists
7. reconstruct Domain Document/Version and call publish_initial_version
8. require returned revision == record.result.resulting_document_revision
9. require transition timestamp/result matches proposed operation result
10. INSERT document_publish_operations ... ON CONFLICT DO NOTHING
11. if insert count == 0: refetch operation; replay or Conflict
12. UPDATE document_versions to PUBLISHED + published_at
13. UPDATE documents current_version_id + revision with expected revision guard
14. insert exactly one Domain Outbox event
15. insert exactly one Audit Outbox event
16. COMMIT
```

Use `map_statement_error` before commit and retain `map_commit_error` for `tx.commit()`.

- [ ] **Step 6: Preserve final OCC checks in SQL**

Even after locking/Domain validation, use conditional updates and assert one affected row. Equivalent Document update:

```sql
UPDATE documents
SET current_version_id = $1,
    revision = $2
WHERE document_id = $3
  AND revision = $4
  AND current_version_id IS NULL
```

Zero affected rows is `RepositoryError::Conflict` unless a previously committed same operation is discovered and replayed.

Version update must require `lifecycle_state = 'WORKING'` and one affected row.

- [ ] **Step 7: Insert events from the Application record without regenerating identity**

Persist the EventId, AuditEventId, timestamps, payloads, actor/resource fields supplied in `PublishInitialVersionRecord`. Do not generate replacement event IDs in the adapter. Successful replay must not insert events again.

- [ ] **Step 8: Run transaction + existing repository tests**

Run:

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test repository_contract -- --nocapture
cargo test -p document-repository-postgres --test schema_constraints -- --nocapture
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/document-repository-postgres/src crates/document-repository-postgres/tests/publish_transaction.rs
git commit -m "feat: implement atomic initial document publish"
```

---

### Task 7: Prove concurrency, exact-command retry, and full Create → Publish → Get behavior

**Files:**
- Create: `crates/document-repository-postgres/tests/publish_concurrency.rs`
- Create: `crates/document-application/tests/publish_vertical_slice.rs`
- Modify: test-only helpers in those files only as required

**Interfaces:**
- Consumes: complete production Publish flow from Tasks 1–6.
- Produces: real PostgreSQL concurrency/ambiguity evidence and real filesystem + PostgreSQL vertical evidence.

- [ ] **Step 1: Write failing distinct-operation concurrency test**

Use PostgreSQL 18.6 with pool size >= 4. Seed one initial Document, then launch two `publish_initial_version` calls concurrently with:

```text
same Document
same target Version
same expected revision = 0
different PublishOperationId
different Event/Audit IDs
```

Use `tokio::join!` or a barrier to overlap requests. Assert exactly one `Ok` and one `RepositoryError::Conflict`.

Query final state and assert:

```text
revision = 1
current_version_id = target
Publish operation rows = 1
Publish Domain events = 1
Publish Audit events = 1
```

- [ ] **Step 2: Write failing same-operation concurrency test**

Launch the exact same operation identity/command concurrently. Assert both callers receive equal `PublishDocumentResult`, while DB mutation/events/operation row exist exactly once.

- [ ] **Step 3: Run concurrency test and verify RED**

Run:

```bash
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
```

Expected: FAIL until race handling/second lookup/claim behavior is correct.

- [ ] **Step 4: Fix only race defects exposed by the RED test**

Do not weaken assertions. Maintain Document-first lock ordering and operation recheck. If a same-operation contender waits on the unique operation claim, it must return the committed stored result after the winner commits rather than surfacing a duplicate-key Internal error.

- [ ] **Step 5: Write full Create → Publish → Get/open vertical slice**

In `publish_vertical_slice.rs` use:

- real `LocalFileStorage` under `tempfile::TempDir`;
- real PostgreSQL 18.6;
- real `PostgresDocumentRepository`;
- `DocumentService`;
- deterministic Clock/ID generator for server-generated event IDs;
- a caller-supplied valid UUIDv7 PublishOperationId.

Flow:

```text
create_document
-> get_document == WORKING/current None/revision 0
-> publish_document(expected revision 0)
-> get_document == PUBLISHED/current target/revision 1
-> open_primary_file returns original bytes
```

Assert one additional Domain event, one additional Audit event, and one operation row after Publish.

- [ ] **Step 6: Add deterministic commit-ambiguity recovery wrappers**

Because `DocumentService` is generic over repository ports, keep ambiguity injection test-only instead of adding a production fault flag.

Create a wrapper implementing `DocumentRepository + DocumentPublishRepository` around real PostgreSQL repository with two one-shot modes:

```text
BeforeCommitUnknown:
  first publish call returns RepositoryError::CommitOutcomeUnknown without delegating;
  retry delegates normally.

AfterCommitUnknown:
  first publish call delegates and commits successfully, then masks Ok as CommitOutcomeUnknown;
  retry delegates/looks up the already-committed operation.
```

Assert Application first returns `PublishCommitOutcomeUnknown`, retry uses the **same command and operation ID**, and both modes converge to exactly one Publish mutation/event/audit/operation row.

- [ ] **Step 7: Run vertical/concurrency tests**

Run:

```bash
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
cargo test -p document-application --test publish_vertical_slice -- --nocapture
```

Expected: PASS with no skipped cases.

- [ ] **Step 8: Run all Rust tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/document-repository-postgres/tests/publish_concurrency.rs crates/document-application/tests/publish_vertical_slice.rs
git commit -m "test: prove document publish concurrency and recovery"
```

---

### Task 8: Run repository gates, self-review the implementation, and record exact-head evidence

**Files:**
- Modify: `docs/superpowers/execution/document-publish-v0-status.md`
- Modify: `docs/superpowers/execution/active.md` only when the execution phase/PR changes
- Modify: implementation PR body/metadata through GitHub; no runtime code unless review finds a defect

**Interfaces:**
- Consumes: complete implementation from Tasks 1–7.
- Produces: exact-head verification evidence and a reviewable implementation PR; does not merge without an explicit merge decision.

- [ ] **Step 1: Run fast verification**

```bash
mise run verify:fast
```

Expected: PASS.

- [ ] **Step 2: Run normal verification**

```bash
mise run verify
```

Expected: PASS.

- [ ] **Step 3: Run full verification**

```bash
mise run verify:full
```

Expected: PASS, including architecture, Rust tests/static checks, security/license gates, portability, container, SBOM, and SQLx/PostgreSQL checks configured by the repository.

- [ ] **Step 4: Run explicit Publish evidence tests once more on the final local tree**

```bash
cargo test -p document-domain
cargo test -p document-application --test publish_document_contract -- --nocapture
cargo test -p document-application --test publish_vertical_slice -- --nocapture
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
```

Expected: PASS, 0 skipped Publish cases.

- [ ] **Step 5: Self-review against the frozen Design Spec**

Check each Design section explicitly:

```text
initial Publish only                      implemented
UUIDv7 caller operation ID                implemented + validated
same-op replay / misuse conflict          implemented
OCC + short row lock                      implemented
file preflight semantics                  implemented
composite current ownership FK            implemented
PUBLISHED/current atomic semantics         implemented
Domain + Audit Outbox atomicity            implemented
operation result atomicity                 implemented
commit ambiguity recovery                  implemented
current replacement / Withdraw / scheduler absent
HTTP/UI/Search/outbox worker               absent
```

If review reveals a contract-level change, stop and request a Design amendment rather than silently changing the frozen design.

- [ ] **Step 6: Update Execution Status with exact evidence**

Record:

- completed Task/Step numbers;
- exact implementation branch/head SHA;
- all verification commands/results;
- exact test counts where available;
- current PR number/state;
- unresolved findings/blockers;
- next exact action;
- any approved Design amendment (normally none).

- [ ] **Step 7: Push/create implementation PR and require exact-head hosted CI**

Implementation branch:

```text
feat/document-publish-v0
```

PR target:

```text
main
```

Do not claim implementation complete from local verification. Require the PR-triggered CI on the exact final head to complete successfully for every required job.

- [ ] **Step 8: Review hosted CI and PR feedback**

Fetch the exact-head workflow run, all required jobs, inline review threads, and submitted reviews. Fix Critical/Important findings with TDD evidence and rerun exact-head CI after every branch-tree change.

- [ ] **Step 9: Stop at the merge gate**

When the exact final PR head is green and no blocking review finding remains, mark the PR Ready for review and update the PR body with evidence.

Do **not** merge the implementation PR without an explicit user merge instruction.
