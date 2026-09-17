# Document Publish v0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the first safe, idempotent publication operation for `DocumentVersion #1`, transitioning an initial Document from `WORKING/current=None/revision=0` to `PUBLISHED/current=Version #1/revision=1` with atomic Domain/Audit outbox records and durable publish-operation recovery.

**Architecture:** Preserve the existing `document-domain` → `document-application` ← infrastructure boundaries. Domain owns the initial publication invariant/transition; Application owns command validation, replay-before-preflight orchestration, event construction, and error translation; a capability-specific `DocumentPublishRepository` port isolates Publish from the existing Create/Get repository interface; PostgreSQL owns the final OCC + short row-lock decision and atomically commits authoritative state, Domain Outbox, Audit Outbox, and the successful publish-operation result.

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

Do not start production implementation from the Design branch. After Design PR #5 is green and merged, create implementation branch `feat/document-publish-v0` from the exact merged `main` head, then execute this plan in order.

---

## File Structure

```text
crates/document-domain/src/
├─ document.rs
├─ error.rs
└─ lib.rs

crates/document-application/src/
├─ command.rs
├─ error.rs
├─ events.rs
├─ ports.rs
├─ service.rs
└─ lib.rs

crates/document-application/tests/
├─ publish_document_contract.rs
└─ publish_vertical_slice.rs

crates/document-storage-fs/src/
└─ error.rs

crates/document-repository-postgres/migrations/
└─ 0002_document_publish_v0.sql

crates/document-repository-postgres/src/
├─ lib.rs
├─ repository.rs
├─ publish.rs
├─ publish_rows.rs
├─ mapping.rs
└─ error.rs

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
  - Publish-specific Domain errors.

- [ ] **Step 1: Write failing successful-transition Domain tests**

Start from `InitialDocument::create`, split with `into_parts()`, publish, and assert:

```rust
let transition = document
    .publish_initial_version(&mut version, published_at)
    .expect("initial working version should publish");

assert_eq!(version.lifecycle_state(), LifecycleState::Published);
assert_eq!(version.published_at(), Some(published_at));
assert_eq!(document.current_version_id(), Some(version.document_version_id()));
assert_eq!(document.revision(), 1);
assert_eq!(transition.resulting_document_revision(), 1);
assert_eq!(version.approved_at(), None);
```

- [ ] **Step 2: Write failing invalid-transition Domain tests**

Cover exact errors:

```rust
DomainError::VersionDocumentMismatch
DomainError::CurrentVersionAlreadySet
DomainError::VersionNotWorking
DomainError::RevisionOverflow
```

Build the required private-field fixtures inside the existing `document-domain` unit-test module. For every rejected transition, assert the Document revision/current pointer and target lifecycle/published timestamp are unchanged.

- [ ] **Step 3: Run Domain tests and verify RED**

```bash
cargo test -p document-domain
```

Expected: FAIL because Publish transition/errors are absent.

- [ ] **Step 4: Implement `PublishTransition` and Domain errors**

Add:

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

- [ ] **Step 5: Implement the minimal transition with no partial mutation on failure**

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

- [ ] **Step 6: Add narrow restoration for initial PUBLISHED state**

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

Do not add a generic arbitrary lifecycle rehydration API.

- [ ] **Step 7: Run Domain and architecture checks**

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

### Task 2: Define Publish Application contracts and the segregated Publish repository port

**Files:**
- Modify: `crates/document-application/src/command.rs`
- Modify: `crates/document-application/src/error.rs`
- Modify: `crates/document-application/src/events.rs`
- Modify: `crates/document-application/src/ports.rs`
- Modify: `crates/document-application/src/lib.rs`
- Create: `crates/document-application/tests/publish_document_contract.rs`

**Interfaces:**
- Consumes: Task 1 Domain types and existing `Clock`, `IdGenerator`, `FileStorage`.
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

- [ ] **Step 1: Write failing UUIDv7 and command-validation tests**

Use complete inputs:

```rust
let valid_uuid = Uuid::parse_str("01890f7a-6f6e-7b0a-8000-000000000001").unwrap();
let invalid_uuid = Uuid::from_u128(1);
let operation_id = PublishOperationId::try_from_uuid(valid_uuid).unwrap();
let document_id = DocumentId::from_uuid(Uuid::from_u128(10));
let version_id = DocumentVersionId::from_uuid(Uuid::from_u128(11));
let principal = PrincipalRef::new("test-idp", "actor-1").unwrap();

assert!(PublishOperationId::try_from_uuid(invalid_uuid).is_err());
assert!(
    PublishDocumentCommand::new(
        operation_id,
        document_id,
        version_id,
        -1,
        principal,
    )
    .is_err()
);
```

- [ ] **Step 2: Run the focused test and verify RED**

```bash
cargo test -p document-application --test publish_document_contract
```

Expected: FAIL because Publish contracts do not exist.

- [ ] **Step 3: Add `PublishOperationId`, command, and result types**

Implement:

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

Use this exact command constructor shape:

```rust
pub fn new(
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_document_version_id: DocumentVersionId,
    expected_document_revision: i64,
    principal: PrincipalRef,
) -> Result<Self, ApplicationError>
```

Reject negative expected revision. Add getters for every field.

`PublishDocumentResult` contains operation/document/version IDs, resulting revision, and `published_at`, with getters plus:

```rust
pub fn from_persisted(
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    resulting_document_revision: i64,
    published_at: OffsetDateTime,
) -> Self
```

- [ ] **Step 4: Add identity, stored-operation, candidate, and transaction-record values**

Define:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCommandIdentity {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_document_version_id: DocumentVersionId,
    expected_document_revision: i64,
    principal: PrincipalRef,
}
```

Provide `PublishCommandIdentity::from_command(&PublishDocumentCommand)` and getters.

Define:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOperationRecord {
    identity: PublishCommandIdentity,
    result: PublishDocumentResult,
}
```

Provide `matches_identity(&PublishCommandIdentity) -> bool`, `identity()`, `result()`, and `into_parts()`.

Define `PublishCandidate` containing `Document`, `DocumentVersion`, `FileObject`, and `VersionFile`, with getters and:

```rust
pub fn into_parts(self) -> (Document, DocumentVersion, FileObject, VersionFile)
```

Define `PublishInitialVersionRecord` containing one `PublishOperationRecord`, one `DomainEventRecord`, and one `AuditEventRecord`, with getters and `into_parts()`.

- [ ] **Step 5: Add `DocumentPublishRepository` without changing `DocumentRepository`**

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

Existing Create/Get/reconciliation fakes must not need Publish methods.

- [ ] **Step 6: Add Publish event constants**

```rust
pub const DOCUMENT_VERSION_PUBLISHED: &str = "DocumentVersionPublished";
pub const AUDIT_DOCUMENT_VERSION_PUBLISHED: &str = "document.version.published";
```

- [ ] **Step 7: Extend error types without breaking Create**

Add to `RepositoryError`:

```rust
DocumentNotFound,
DocumentVersionNotFound,
Conflict,
BusinessRule,
```

Add equivalent Application categories and:

```rust
PublishCommitOutcomeUnknown {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
},
```

Keep the existing Create variant:

```rust
CommitOutcomeUnknown {
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
    file_id: FileId,
}
```

Add `StorageError::ObjectUnreadable`. Map `NotFound` and `ObjectUnreadable` to `ApplicationError::IntegrityViolation`; keep `Unavailable` as `StorageUnavailable`.

- [ ] **Step 8: Run Application/static checks**

```bash
cargo test -p document-application --test publish_document_contract
cargo test -p document-application --no-run
mise run arch:check
```

Expected: PASS.

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
- Consumes: Task 2 contracts and Task 1 Domain transition.
- Produces: `DocumentService::publish_document(...)` with replay-before-preflight semantics.

- [ ] **Step 1: Add exact filesystem open-error unit tests**

Add a `#[cfg(test)] mod tests` inside `crates/document-storage-fs/src/error.rs` and assert:

```rust
assert_eq!(
    super::map_open_error(io::Error::from(io::ErrorKind::PermissionDenied)),
    StorageError::ObjectUnreadable,
);
assert_eq!(
    super::map_open_error(io::Error::from(io::ErrorKind::NotFound)),
    StorageError::NotFound,
);
assert_eq!(
    super::map_open_error(io::Error::from(io::ErrorKind::ConnectionReset)),
    StorageError::Unavailable,
);
```

- [ ] **Step 2: Add failing Application orchestration tests**

Build `FakePublishRepository` implementing only `DocumentPublishRepository`. Extend fake storage with an open-call counter. Prove:

```text
stored operation + same identity
  -> stored result
  -> candidate lookup 0
  -> storage open 0
  -> publish transaction 0

stored operation + different identity
  -> Conflict
  -> storage open 0

new operation + missing object
  -> IntegrityViolation
  -> publish transaction 0

new operation + storage outage
  -> StorageUnavailable
  -> publish transaction 0

new operation + readable object
  -> publish transaction exactly 1
```

Also make the fake Publish repository return `RepositoryError::CommitOutcomeUnknown` from its write method and assert structured `PublishCommitOutcomeUnknown` contains the exact operation/document/version IDs.

- [ ] **Step 3: Run focused tests and verify RED**

```bash
cargo test -p document-application --test publish_document_contract
cargo test -p document-storage-fs
```

Expected: FAIL until orchestration/error mapping exists.

- [ ] **Step 4: Implement filesystem object-unreadable mapping**

```rust
pub(crate) fn map_open_error(error: io::Error) -> StorageError {
    match error.kind() {
        io::ErrorKind::NotFound => StorageError::NotFound,
        io::ErrorKind::PermissionDenied => StorageError::ObjectUnreadable,
        _ => StorageError::Unavailable,
    }
}
```

Do not change write/sync/finalize mappings.

- [ ] **Step 5: Implement Publish in a separate `R: DocumentPublishRepository` service impl**

Do not require `DocumentRepository` for the Publish method itself. Existing Create/Get methods remain under their existing `R: DocumentRepository` impl.

Use this order:

```rust
let identity = PublishCommandIdentity::from_command(&command);

if let Some(stored) = self
    .repository
    .get_publish_operation(command.publish_operation_id())
    .await?
{
    if stored.matches_identity(&identity) {
        return Ok(stored.result().clone());
    }
    return Err(ApplicationError::Conflict);
}

let candidate = self
    .repository
    .get_publish_candidate(
        command.document_id(),
        command.target_document_version_id(),
    )
    .await?;

let published_at = self.clock.now();
let (mut document, mut version, file, _version_file) = candidate.into_parts();
let transition = document
    .publish_initial_version(&mut version, published_at)
    .map_err(map_publish_domain_error)?;

let storage_key = file.storage_key().clone();
let reader = self.storage.open(&storage_key).await?;
drop(reader);
```

Then create the result, EventId, AuditEventId, payloads, `PublishOperationRecord`, and `PublishInitialVersionRecord`, and call the repository write once.

- [ ] **Step 6: Build exact Publish payloads**

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

Use `resource_version_id = Some(command.target_document_version_id())`.

- [ ] **Step 7: Map Publish errors explicitly**

```text
VersionDocumentMismatch  -> IntegrityViolation
CurrentVersionAlreadySet -> Conflict
VersionNotWorking        -> BusinessRule
RevisionOverflow         -> IntegrityViolation
```

Map repository categories directly. Map repository `CommitOutcomeUnknown` to `PublishCommitOutcomeUnknown` using the command identity.

- [ ] **Step 8: Run contract/storage/Application tests**

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

### Task 4: Add Publish migration and database constraints

**Files:**
- Create: `crates/document-repository-postgres/migrations/0002_document_publish_v0.sql`
- Create: `crates/document-repository-postgres/tests/publish_schema.rs`

**Interfaces:**
- Consumes: `0001_document_authoritative_core.sql`.
- Produces: composite current-Version FK and permanent Publish operation table.

- [ ] **Step 1: Write the failing schema integration test**

Use PostgreSQL `18.6-bookworm`, run `migrate(&pool)`, then prove:

1. valid initial Working Document/Version remains insertable;
2. assigning another Document's Version to `current_version_id` is rejected;
3. negative `expected_document_revision` is rejected;
4. `resulting_document_revision != expected_document_revision + 1` is rejected;
5. operation target `(document_id, target_document_version_id)` must belong to the same Document;
6. duplicate `publish_operation_id` is rejected.

- [ ] **Step 2: Run schema test and verify RED**

```bash
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
```

Expected: FAIL because migration/table/composite FK do not exist.

- [ ] **Step 3: Add `0002_document_publish_v0.sql`**

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

Do not add triggers, TTL columns, cleanup state, or generic command tables.

- [ ] **Step 4: Run migration/schema checks**

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

### Task 5: Add PostgreSQL Publish read helpers and make Get understand initial PUBLISHED state

**Files:**
- Create: `crates/document-repository-postgres/src/publish_rows.rs`
- Create: `crates/document-repository-postgres/src/publish.rs`
- Modify: `crates/document-repository-postgres/src/lib.rs`
- Modify: `crates/document-repository-postgres/src/mapping.rs`
- Create: `crates/document-repository-postgres/tests/publish_transaction.rs`

**Interfaces:**
- Consumes: Task 1 restoration, Task 2 Publish value types, Task 4 schema.
- Produces crate-internal helpers:
  - `get_publish_operation`
  - `get_publish_candidate`
  - published-state authoritative mapping.
- Does **not** implement `DocumentPublishRepository` on `PostgresDocumentRepository` yet; the full trait impl is added atomically in Task 6 when all three methods exist.

- [ ] **Step 1: Write failing read-path and published-Get tests**

Inside `publish.rs`, add `#[cfg(test)]` tests for the crate-internal read helpers using real PostgreSQL. Prove:

```text
get_publish_operation(unknown) -> None
get_publish_candidate(document, version) -> Working candidate + PRIMARY file metadata
```

Seed one `document_publish_operations` row directly and prove exact identity/result reconstruction.

In `publish_transaction.rs`, seed a normal Create, manually perform one valid initial publication SQL transaction, and assert existing `get_document()` now returns:

```text
PUBLISHED
published_at = fixed timestamp
current_version_id = Version #1
revision = 1
```

instead of `IntegrityViolation`.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test -p document-repository-postgres publish -- --nocapture
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
```

Expected: FAIL because helpers/published mapping do not exist.

- [ ] **Step 3: Add `PublishOperationRow`, `PublishCandidateRow`, and `LockedPublishStateRow`**

`PublishOperationRow` contains all identity/result columns from `document_publish_operations`.

`PublishCandidateRow` contains:

```text
document_id
folder_id
current_version_id
document_revision
document_metadata
document_created_at
version fields
PRIMARY file fields
VersionFile fields
```

`LockedPublishStateRow` contains the locked Document/target Version values needed by Task 6.

- [ ] **Step 4: Implement crate-internal operation lookup**

```rust
pub(crate) async fn get_publish_operation(
    pool: &PgPool,
    operation_id: PublishOperationId,
) -> Result<Option<PublishOperationRecord>, RepositoryError>
```

Map actor fields using `PrincipalRef::new`; malformed persisted values are `IntegrityViolation`.

- [ ] **Step 5: Implement crate-internal candidate lookup with exact error distinctions**

Preserve these outcomes:

```text
Document absent                    -> DocumentNotFound
target Version absent              -> DocumentVersionNotFound
target belongs to another Document -> IntegrityViolation
PRIMARY DB reference/file missing  -> IntegrityViolation
```

Use separate existence checks or a left-join strategy that does not collapse all failures into one missing inner-join row.

- [ ] **Step 6: Extend authoritative mapping for initial PUBLISHED state**

Accept only these initial-published facts:

```text
version_no == 1
lifecycle_state == "PUBLISHED"
published_at == Some(fixed time)
current_version_id == Some(document_version_id)
document_revision == 1
```

Keep existing initial file/metadata/time invariants. Reconstruct through:

```rust
InitialDocument::restore_published(create_input, published_at)
```

Reject inconsistent combinations such as `PUBLISHED + current None`, wrong current Version, `PUBLISHED + revision != 1`, or `WORKING + current Some` as `IntegrityViolation`.

- [ ] **Step 7: Register focused modules, but do not add an incomplete trait impl**

Add `mod publish;` and `mod publish_rows;` in `lib.rs` or the existing internal module declaration location. Keep helpers `pub(crate)`.

Do not add `impl DocumentPublishRepository for PostgresDocumentRepository` until Task 6, because Rust trait implementations must provide every required method and committed trees must never contain `unimplemented!()`/panic/success stubs.

- [ ] **Step 8: Run read/Get regressions**

```bash
cargo test -p document-repository-postgres publish -- --nocapture
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test repository_contract -- --nocapture
cargo test -p document-application --test vertical_slice -- --nocapture
```

Expected: PASS; existing Working reads remain valid and the initial Published state round-trips.

- [ ] **Step 9: Commit**

```bash
git add crates/document-repository-postgres/src crates/document-repository-postgres/tests/publish_transaction.rs
git commit -m "feat: add document publish repository reads"
```

---

### Task 6: Implement the atomic PostgreSQL Publish transaction and the full Publish repository trait

**Files:**
- Modify: `crates/document-repository-postgres/src/publish.rs`
- Modify: `crates/document-repository-postgres/src/repository.rs`
- Modify: `crates/document-repository-postgres/tests/publish_transaction.rs`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: complete `DocumentPublishRepository` implementation on `PostgresDocumentRepository`.

- [ ] **Step 1: Add failing successful-Publish transaction test**

Create one Document through the existing Create flow, build a Publish record, call PostgreSQL Publish, and assert:

```text
Version lifecycle = PUBLISHED
published_at = requested timestamp
current_version_id = Version #1
revision = 1
Publish Domain Outbox delta = 1
Publish Audit Outbox delta = 1
Publish operation delta = 1
```

Assert stored event payload and operation result match the returned `PublishDocumentResult`.

- [ ] **Step 2: Add failing replay, misuse, OCC, and lifecycle tests**

Cover:

```text
same operation + same command -> same result, no state/count change
same operation ID + changed principal -> Conflict
same operation ID + changed Document -> Conflict
same operation ID + changed Version -> Conflict
same operation ID + changed expected revision -> Conflict
stale expected revision -> Conflict
current already Some(other) -> Conflict
target already PUBLISHED under another operation -> Conflict
target WITHDRAWN -> BusinessRule
cross-Document target -> IntegrityViolation
```

- [ ] **Step 3: Add a deterministic rollback test using an Outbox PK collision**

Preinsert an `outbox_events` row whose `event_id` equals the Domain EventId carried by the `PublishInitialVersionRecord` under test. Then execute Publish.

The Publish transaction must encounter the duplicate-key statement error **after** it has claimed the operation and attempted state mutation, roll back, and leave:

```text
Version = WORKING
current_version_id = NULL
revision unchanged
no document_publish_operations row
no Publish Audit Outbox row
only the preexisting colliding Domain Outbox row
```

Do not add a production fault flag or optional-atomicity path.

- [ ] **Step 4: Run transaction tests and verify RED**

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
```

Expected: FAIL until mutation/replay handling exists.

- [ ] **Step 5: Implement `publish_initial_version` transaction helper**

Inside one SQLx transaction perform exactly:

```text
1. lookup operation ID
2. stored + matching identity -> return stored result
3. stored + different identity -> Conflict
4. SELECT Document FOR UPDATE; missing -> DocumentNotFound
5. lookup operation ID again; replay or Conflict
6. load target Version and verify target ownership
7. verify PRIMARY DB reference/FileObject still exists
8. reconstruct Domain Document/Version and run initial Publish transition
9. verify transition result/timestamp equals proposed stored result
10. INSERT all document_publish_operations identity/result columns ON CONFLICT DO NOTHING
11. zero-row claim -> refetch; matching replay or Conflict
12. conditionally UPDATE target Version from WORKING to PUBLISHED + published_at
13. conditionally UPDATE Document current_version_id + revision using expected revision/current NULL
14. insert supplied Domain Outbox event
15. insert supplied mandatory Audit Outbox event
16. COMMIT
```

Use `map_statement_error` for pre-commit errors and `map_commit_error` only for `tx.commit()`.

- [ ] **Step 6: Keep final OCC/state guards in SQL**

Document update:

```sql
UPDATE documents
SET current_version_id = $1,
    revision = $2
WHERE document_id = $3
  AND revision = $4
  AND current_version_id IS NULL
```

Require exactly one affected row.

Version update:

```sql
UPDATE document_versions
SET lifecycle_state = 'PUBLISHED',
    published_at = $1
WHERE document_version_id = $2
  AND document_id = $3
  AND lifecycle_state = 'WORKING'
```

Require exactly one affected row. Map rejected lifecycle to `BusinessRule` unless a distinct committed operation/current-version conflict is the actual cause.

- [ ] **Step 7: Implement the complete `DocumentPublishRepository` trait**

In `repository.rs`:

```rust
impl DocumentPublishRepository for PostgresDocumentRepository {
    async fn get_publish_operation(...) -> Result<..., RepositoryError> {
        publish::get_publish_operation(&self.pool, operation_id).await
    }

    async fn get_publish_candidate(...) -> Result<..., RepositoryError> {
        publish::get_publish_candidate(&self.pool, document_id, target_version_id).await
    }

    async fn publish_initial_version(...) -> Result<..., RepositoryError> {
        publish::publish_initial_version(&self.pool, record).await
    }
}
```

Use the exact signatures defined in Task 2; do not duplicate SQL in `repository.rs`.

- [ ] **Step 8: Run Publish and existing repository regressions**

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

### Task 7: Prove concurrency, exact-command recovery, and Create → Publish → Get end-to-end behavior

**Files:**
- Create: `crates/document-repository-postgres/tests/publish_concurrency.rs`
- Create: `crates/document-application/tests/publish_vertical_slice.rs`

**Interfaces:**
- Consumes: complete production Publish flow.
- Produces: real PostgreSQL concurrency evidence and real filesystem + PostgreSQL vertical evidence.

- [ ] **Step 1: Write failing distinct-operation concurrency test with an explicit barrier**

Use PostgreSQL 18.6 with pool size >= 4. Create a `tokio::sync::Barrier` for the two worker tasks so both reach the Publish call together, then await both with `tokio::join!`.

Inputs:

```text
same Document
same target Version
same expected revision = 0
different PublishOperationId
different Event/Audit IDs
```

Assert exactly one `Ok` and one `RepositoryError::Conflict`, then assert final DB state:

```text
revision = 1
current_version_id = target
Publish operation rows = 1
Publish Domain events = 1
Publish Audit events = 1
```

- [ ] **Step 2: Write failing same-operation concurrency test with the same barrier pattern**

Both workers submit the exact same `PublishInitialVersionRecord` identity and logical command. Both must receive equal `PublishDocumentResult`; state mutation, revision increment, Domain event, Audit event, and operation row occur exactly once.

- [ ] **Step 3: Run concurrency test and verify RED**

```bash
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
```

Expected: FAIL until race handling is correct.

- [ ] **Step 4: Fix only defects exposed by the concurrency tests**

Preserve Document-first lock ordering and the second operation lookup. If a same-operation contender observes/awaits the winner's operation claim, it must return the committed stored result after the winner commits, not a duplicate-key Internal error.

- [ ] **Step 5: Write full Create → Publish → Get/open vertical slice**

Use:

```text
LocalFileStorage + tempfile::TempDir
PostgreSQL 18.6
PostgresDocumentRepository
DocumentService
fixed Clock
server Event/Audit IdGenerator
caller-provided valid UUIDv7 PublishOperationId
```

Flow:

```text
create_document
-> get_document == WORKING/current None/revision 0
-> publish_document(expected revision 0)
-> get_document == PUBLISHED/current target/revision 1
-> open_primary_file returns original bytes
```

Assert Publish adds exactly one Domain event, one Audit event, and one publish-operation row.

- [ ] **Step 6: Add deterministic commit-ambiguity wrappers without production fault flags**

Create a test-only repository wrapper implementing both `DocumentRepository` and `DocumentPublishRepository` around the real PostgreSQL repository.

Mode `BeforeCommitUnknown`:

```text
first publish write -> return RepositoryError::CommitOutcomeUnknown without delegating
retry -> delegate normally
```

Mode `AfterCommitUnknown`:

```text
first publish write -> delegate and commit successfully, then mask Ok as CommitOutcomeUnknown
retry -> delegate; operation lookup/replay returns stored result
```

In both modes, the first Application call must return `PublishCommitOutcomeUnknown`; retry uses the exact same `PublishDocumentCommand`; final DB state contains exactly one Publish mutation/event/audit/operation row.

- [ ] **Step 7: Run concurrency and vertical tests**

```bash
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
cargo test -p document-application --test publish_vertical_slice -- --nocapture
```

Expected: PASS with no skipped Publish cases.

- [ ] **Step 8: Run all Rust tests**

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

### Task 8: Run repository gates, self-review implementation, and record exact-head evidence

**Files:**
- Modify: `docs/superpowers/execution/document-publish-v0-status.md`
- Modify: `docs/superpowers/execution/active.md` when execution phase/PR changes
- Update: implementation PR body/metadata through GitHub

**Interfaces:**
- Consumes: Tasks 1–7.
- Produces: exact-head evidence and a reviewable implementation PR.

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

Expected: PASS, including configured architecture, Rust, security/license, portability, container, SBOM, SQLx/PostgreSQL, and Development Assurance gates.

- [ ] **Step 4: Run explicit Publish evidence tests on the final local tree**

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

Verify explicitly:

```text
initial Publish only                       implemented
UUIDv7 caller operation ID                 implemented + validated
same-op replay / misuse conflict           implemented
OCC + short row lock                       implemented
file preflight semantics                   implemented
composite current ownership FK             implemented
PUBLISHED/current atomic semantics          implemented
Domain + Audit Outbox atomicity             implemented
operation result atomicity                  implemented
commit ambiguity recovery                   implemented
current replacement / Withdraw / scheduler absent
HTTP/UI/Search/outbox worker                absent
```

If a contract-level change is required, stop and request a Design amendment.

- [ ] **Step 6: Update Execution Status with exact evidence**

Record completed Tasks/Steps, implementation branch/head SHA, exact commands/results/test counts, implementation PR state, blockers/findings, next exact action, and any approved Design amendment.

- [ ] **Step 7: Push/create implementation PR and require exact-head hosted CI**

Use branch:

```text
feat/document-publish-v0
```

with base:

```text
main
```

Do not claim completion from local results. Require PR-triggered CI on the exact final head.

- [ ] **Step 8: Review hosted CI and PR feedback**

Fetch exact-head workflow run and every required job, inline review threads, and submitted reviews. Fix Critical/Important findings with TDD evidence and rerun exact-head CI after every tree change.

- [ ] **Step 9: Stop at the explicit merge gate**

When the exact final implementation PR head is green and no blocking finding remains, mark it Ready and update its body with evidence.

Do **not** merge the implementation PR without an explicit user merge instruction.
