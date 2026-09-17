# Document Publish v0 Implementation Plan

> **Execution rule:** implement task-by-task with TDD. Production implementation starts only after Design PR #5 is green and merged. Use `feat/document-publish-v0` created from the exact merged `main` head.

**Goal:** publish the initial `DocumentVersion #1` exactly once, moving `WORKING/current=None/revision=0` to `PUBLISHED/current=Version #1/revision=1`, while preserving durable idempotency, OCC, mandatory Domain/Audit outboxes, file-integrity preflight, and conservative unknown-commit recovery.

**Frozen Design:** `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`

## Global constraints

- Initial Publish only. No Version #2+, current replacement, Withdraw, approval workflow, scheduled publication, HTTP/OpenAPI, UI, Search indexing, outbox delivery, generic idempotency framework, or publish-operation cleanup.
- `PublishOperationId` is caller-generated, Application-layer, and UUIDv7-only.
- Same operation ID + same command returns the stored result without another state/event/audit mutation.
- Same operation ID + different command is `Conflict`.
- Different operation IDs never become replay-equivalent merely because they target the same Version.
- New Publish requires `expected_document_revision` OCC plus a short PostgreSQL row lock.
- New Publish performs PRIMARY final-object preflight before the PostgreSQL mutation.
- Missing/object-level unreadable authoritative object is `IntegrityViolation`; storage dependency outage is `StorageUnavailable`.
- Authoritative state + one Publish Domain Outbox + one mandatory Publish Audit Outbox + one successful `document_publish_operations` row commit atomically.
- Add `0002_document_publish_v0.sql`; never rewrite `0001_document_authoritative_core.sql`.
- `documents(document_id, current_version_id)` must reference `document_versions(document_id, document_version_id)`.
- Current-is-PUBLISHED semantics remain Domain + atomic Repository responsibility; no lifecycle trigger/helper state.
- PostgreSQL commit errors remain `CommitOutcomeUnknown`; no silent retry and no regenerated operation ID.
- Existing architecture/security/license/portability/container/SBOM/SQLx/Development Assurance gates remain mandatory.

## Planned files

```text
crates/document-domain/src/
  document.rs
  error.rs
  lib.rs

crates/document-application/src/
  command.rs
  error.rs
  events.rs
  ports.rs
  service.rs
  lib.rs

crates/document-application/tests/
  publish_document_contract.rs
  publish_vertical_slice.rs

crates/document-storage-fs/src/
  error.rs

crates/document-repository-postgres/migrations/
  0002_document_publish_v0.sql

crates/document-repository-postgres/src/
  lib.rs
  repository.rs
  publish.rs
  publish_rows.rs
  mapping.rs
  error.rs

crates/document-repository-postgres/tests/
  publish_schema.rs
  publish_transaction.rs
  publish_concurrency.rs
```

Do not perform unrelated refactors.

---

## Task 1 — Domain initial-Publish transition and published restoration

**Modify:**
- `crates/document-domain/src/document.rs`
- `crates/document-domain/src/error.rs`
- `crates/document-domain/src/lib.rs`

### Step 1 — RED: successful transition

Add Domain tests that create an initial aggregate, split it with `into_parts()`, publish at a fixed time, and assert:

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

### Step 2 — RED: rejected transitions

Add exact Domain errors and tests:

```rust
DomainError::VersionDocumentMismatch
DomainError::CurrentVersionAlreadySet
DomainError::VersionNotWorking
DomainError::RevisionOverflow
```

For every rejection, prove no partial mutation of current pointer, revision, lifecycle, or `published_at`.

### Step 3 — Verify RED

```bash
cargo test -p document-domain
```

Expected: FAIL because Publish behavior is absent.

### Step 4 — GREEN: implement the transition

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

Implement `Document::publish_initial_version` with this validation order:

```text
target.document_id == document.document_id
current_version_id == None
target.lifecycle_state == WORKING
revision.checked_add(1) succeeds
```

Only after every check succeeds:

```text
target.lifecycle_state = PUBLISHED
target.published_at = published_at
document.current_version_id = target.document_version_id
document.revision = next_revision
```

### Step 5 — GREEN: add narrow initial-PUBLISHED restoration

Add:

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

Do not introduce arbitrary lifecycle rehydration.

### Step 6 — Verify and commit

```bash
cargo test -p document-domain
mise run arch:check
git add crates/document-domain/src/document.rs crates/document-domain/src/error.rs crates/document-domain/src/lib.rs
git commit -m "feat: add initial document publish transition"
```

---

## Task 2 — Application contracts and segregated Publish repository port

**Modify:**
- `crates/document-application/src/command.rs`
- `crates/document-application/src/error.rs`
- `crates/document-application/src/events.rs`
- `crates/document-application/src/ports.rs`
- `crates/document-application/src/lib.rs`

**Create:**
- `crates/document-application/tests/publish_document_contract.rs`

### Step 1 — RED: UUIDv7 and command validation

Use complete deterministic inputs:

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

Run:

```bash
cargo test -p document-application --test publish_document_contract
```

Expected: FAIL.

### Step 2 — GREEN: command/result types

Add `PublishOperationId(Uuid)` with `try_from_uuid` requiring `value.get_version_num() == 7` and `as_uuid()`.

Use this command constructor:

```rust
pub fn new(
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_document_version_id: DocumentVersionId,
    expected_document_revision: i64,
    principal: PrincipalRef,
) -> Result<Self, ApplicationError>
```

Reject negative expected revision. Add getters.

Add `PublishDocumentResult` with:

```text
publish_operation_id
document_id
document_version_id
resulting_document_revision
published_at
```

and a public `from_persisted` constructor for adapter reconstruction.

### Step 3 — GREEN: identity, operation, candidate, and transaction record

Add:

```rust
pub struct PublishCommandIdentity {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    target_document_version_id: DocumentVersionId,
    expected_document_revision: i64,
    principal: PrincipalRef,
}
```

Provide `from_command`, field getters, and equality.

Add:

```rust
pub struct PublishOperationRecord {
    identity: PublishCommandIdentity,
    result: PublishDocumentResult,
}
```

Provide `matches_identity`, `identity`, `result`, and `into_parts`.

Add `PublishCandidate` containing `Document`, `DocumentVersion`, `FileObject`, `VersionFile`, with getters and:

```rust
pub fn into_parts(self) -> (Document, DocumentVersion, FileObject, VersionFile)
```

Add `PublishInitialVersionRecord` containing one `PublishOperationRecord`, one `DomainEventRecord`, and one `AuditEventRecord`, with getters and `into_parts`.

### Step 4 — GREEN: interface segregation

Add without changing existing `DocumentRepository`:

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

Existing Create/Get/reconciliation fakes must compile unchanged.

### Step 5 — GREEN: event and error contracts

Add:

```rust
pub const DOCUMENT_VERSION_PUBLISHED: &str = "DocumentVersionPublished";
pub const AUDIT_DOCUMENT_VERSION_PUBLISHED: &str = "document.version.published";
```

Add Repository categories:

```text
DocumentNotFound
DocumentVersionNotFound
Conflict
BusinessRule
```

Add corresponding Application categories plus:

```rust
PublishCommitOutcomeUnknown {
    publish_operation_id: PublishOperationId,
    document_id: DocumentId,
    document_version_id: DocumentVersionId,
}
```

Keep existing Create `CommitOutcomeUnknown { document_id, document_version_id, file_id }` unchanged.

Add `StorageError::ObjectUnreadable`; map `NotFound` and `ObjectUnreadable` to `IntegrityViolation`, and `Unavailable` to `StorageUnavailable`.

### Step 6 — Verify and commit

```bash
cargo test -p document-application --test publish_document_contract
cargo test -p document-application --no-run
mise run arch:check
git add crates/document-application/src crates/document-application/tests/publish_document_contract.rs
git commit -m "feat: define document publish application contracts"
```

---

## Task 3 — Storage distinction and Application Publish orchestration

**Modify:**
- `crates/document-storage-fs/src/error.rs`
- `crates/document-application/src/service.rs`
- `crates/document-application/tests/publish_document_contract.rs`

### Step 1 — RED: filesystem open-error distinction

Add a `#[cfg(test)] mod tests` inside `document-storage-fs/src/error.rs` proving:

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

### Step 2 — RED: Application ordering

Create `FakePublishRepository` implementing only `DocumentPublishRepository`, plus fake storage with an open counter. Prove:

```text
stored same operation -> stored result; candidate/storage/write counts stay 0
stored operation ID with different identity -> Conflict; storage count 0
new operation + missing object -> IntegrityViolation; write count 0
new operation + storage outage -> StorageUnavailable; write count 0
new operation + readable object -> write count 1
```

Make the fake write return `RepositoryError::CommitOutcomeUnknown` and prove Application returns `PublishCommitOutcomeUnknown` carrying the exact three IDs.

Run:

```bash
cargo test -p document-application --test publish_document_contract
cargo test -p document-storage-fs
```

Expected: FAIL.

### Step 3 — GREEN: storage mapping

Implement:

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

### Step 4 — GREEN: make service construction independent of repository capability

Move `DocumentService::new` into an impl that requires only the existing `IdGenerator`, `Clock`, and `FileStorage` bounds, not `DocumentRepository`. Keep Create/Get/reconciliation methods under `R: DocumentRepository`. Add Publish under a separate `R: DocumentPublishRepository` impl.

This permits Publish-only test fakes without widening `DocumentRepository`.

### Step 5 — GREEN: Publish orchestration order

Implement:

```text
1. derive PublishCommandIdentity
2. get_publish_operation
3. stored same identity -> return stored result immediately
4. stored different identity -> Conflict immediately
5. get_publish_candidate
6. take clock timestamp
7. run Domain initial-Publish transition on candidate copy/value
8. open PRIMARY storage key and drop reader after successful open
9. generate one EventId and one AuditEventId
10. construct PublishDocumentResult
11. construct exact Domain/Audit payloads
12. construct PublishOperationRecord and PublishInitialVersionRecord
13. call repository.publish_initial_version exactly once
14. map RepositoryError::CommitOutcomeUnknown to PublishCommitOutcomeUnknown
```

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

Map Domain failures:

```text
VersionDocumentMismatch  -> IntegrityViolation
CurrentVersionAlreadySet -> Conflict
VersionNotWorking        -> BusinessRule
RevisionOverflow         -> IntegrityViolation
```

### Step 6 — Verify and commit

```bash
cargo test -p document-storage-fs
cargo test -p document-application --test publish_document_contract
cargo test -p document-application
git add crates/document-storage-fs/src/error.rs crates/document-application/src/service.rs crates/document-application/tests/publish_document_contract.rs
git commit -m "feat: orchestrate initial document publish"
```

---

## Task 4 — Publish migration and database constraints

**Create:**
- `crates/document-repository-postgres/migrations/0002_document_publish_v0.sql`
- `crates/document-repository-postgres/tests/publish_schema.rs`

### Step 1 — RED: schema constraints

Using PostgreSQL 18.6 and `migrate(&pool)`, prove:

1. valid initial Working Document/Version remains insertable;
2. assigning another Document's Version to `current_version_id` is rejected;
3. negative `expected_document_revision` is rejected;
4. resulting revision unequal to expected revision + 1 is rejected;
5. operation target document/version ownership mismatch is rejected;
6. duplicate `publish_operation_id` is rejected.

Run:

```bash
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
```

Expected: FAIL.

### Step 2 — GREEN: migration

Add exactly:

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

No trigger, TTL, cleanup state, or generic command table.

### Step 3 — Verify and commit

```bash
cargo test -p document-repository-postgres --test schema_constraints -- --nocapture
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
mise run sqlx:check
git add crates/document-repository-postgres/migrations/0002_document_publish_v0.sql crates/document-repository-postgres/tests/publish_schema.rs
git commit -m "feat: add document publish persistence schema"
```

---

## Task 5 — PostgreSQL Publish read helpers and initial-PUBLISHED Get mapping

**Create:**
- `crates/document-repository-postgres/src/publish.rs`
- `crates/document-repository-postgres/src/publish_rows.rs`
- `crates/document-repository-postgres/tests/publish_transaction.rs`

**Modify:**
- `crates/document-repository-postgres/src/lib.rs`
- `crates/document-repository-postgres/src/mapping.rs`

Do not implement `DocumentPublishRepository` on `PostgresDocumentRepository` in this Task. Add the complete trait impl only in Task 6.

### Step 1 — RED: read helpers

Inside `publish.rs`, add real-PostgreSQL unit tests for crate-internal helpers:

```text
unknown operation ID -> None
known operation row -> exact PublishCommandIdentity + PublishDocumentResult
valid target -> Working PublishCandidate with PRIMARY file metadata
```

Candidate lookup must distinguish:

```text
Document absent                    -> DocumentNotFound
target Version absent              -> DocumentVersionNotFound
target belongs to another Document -> IntegrityViolation
PRIMARY DB reference/file missing  -> IntegrityViolation
```

### Step 2 — RED: published Get mapping

In `publish_transaction.rs`:

1. Create a normal initial Document through existing production Create flow.
2. In one direct test SQL transaction set Version #1 to `PUBLISHED`, set a fixed `published_at`, set Document current to that Version, and set revision to 1.
3. Call existing `get_document` and require initial-PUBLISHED state instead of `IntegrityViolation`.

Run:

```bash
cargo test -p document-repository-postgres publish -- --nocapture
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
```

Expected: FAIL.

### Step 3 — GREEN: focused SQLx rows and read helpers

Add:

```text
PublishOperationRow
PublishCandidateRow
LockedPublishStateRow
```

`PublishOperationRow` contains every operation identity/result column.

`PublishCandidateRow` contains Document, target Version, PRIMARY FileObject, and VersionFile fields required to build a `PublishCandidate`.

Implement:

```rust
pub(crate) async fn get_publish_operation(
    pool: &PgPool,
    operation_id: PublishOperationId,
) -> Result<Option<PublishOperationRecord>, RepositoryError>
```

and:

```rust
pub(crate) async fn get_publish_candidate(
    pool: &PgPool,
    document_id: DocumentId,
    target_version_id: DocumentVersionId,
) -> Result<PublishCandidate, RepositoryError>
```

Malformed persisted actor/domain values map to `IntegrityViolation`.

### Step 4 — GREEN: initial-PUBLISHED authoritative reconstruction

Extend `mapping.rs` so only these Publish-v0 facts are accepted:

```text
version_no = 1
lifecycle_state = PUBLISHED
published_at is present
current_version_id = this Version
Document.revision = 1
```

Preserve existing initial file/metadata/time invariants. Reconstruct with `InitialDocument::restore_published`.

Reject:

```text
PUBLISHED + current None
PUBLISHED + current points elsewhere
PUBLISHED + revision != 1
WORKING + current is non-null
```

as `IntegrityViolation`.

### Step 5 — Register modules without an incomplete trait impl

Register `publish` and `publish_rows` in `lib.rs`; keep helpers `pub(crate)`. Do not add a trait impl containing panic, `unimplemented!()`, or a success stub.

### Step 6 — Verify and commit

```bash
cargo test -p document-repository-postgres publish -- --nocapture
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test repository_contract -- --nocapture
cargo test -p document-application --test vertical_slice -- --nocapture
git add crates/document-repository-postgres/src crates/document-repository-postgres/tests/publish_transaction.rs
git commit -m "feat: add document publish repository reads"
```

---

## Task 6 — Atomic PostgreSQL Publish transaction and complete adapter trait

**Modify:**
- `crates/document-repository-postgres/src/publish.rs`
- `crates/document-repository-postgres/src/repository.rs`
- `crates/document-repository-postgres/tests/publish_transaction.rs`

### Step 1 — RED: successful atomic publication

Create through existing production Create flow, then call Publish and assert SQL state:

```text
Version #1 lifecycle = PUBLISHED
published_at = requested timestamp
current_version_id = Version #1
revision = 1
Publish Domain Outbox delta = 1
Publish Audit Outbox delta = 1
Publish operation delta = 1
```

Stored operation result and event payload must equal the returned result and supplied records.

### Step 2 — RED: replay, misuse, OCC, lifecycle

Prove:

```text
same operation + same command -> identical result and no count/revision change
same operation ID + changed principal -> Conflict
same operation ID + changed Document -> Conflict
same operation ID + changed Version -> Conflict
same operation ID + changed expected revision -> Conflict
stale expected revision -> Conflict
current already another Version -> Conflict
target already PUBLISHED under distinct operation -> Conflict
target WITHDRAWN -> BusinessRule
cross-Document target -> IntegrityViolation
```

### Step 3 — RED: deterministic rollback after partial transaction work

Preinsert an `outbox_events` row whose `event_id` equals the Domain EventId carried by the Publish record. Execute Publish.

The duplicate Domain Outbox PK must occur after operation claim/state-update statements and force total rollback. Assert:

```text
Version remains WORKING
current_version_id remains NULL
revision unchanged
no document_publish_operations row
no Publish Audit row
only the preexisting colliding Domain Outbox row exists
```

No production fault flag is allowed.

Run:

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
```

Expected: FAIL until transaction code exists.

### Step 4 — GREEN: exact transaction algorithm

Inside one SQLx transaction:

```text
1. lookup operation ID
2. matching stored identity -> return stored result
3. mismatching stored identity -> Conflict
4. SELECT Document FOR UPDATE; absent -> DocumentNotFound
5. lookup operation ID again; replay or Conflict
6. load target Version; distinguish absent versus cross-Document ownership
7. verify PRIMARY DB reference and FileObject still exist
8. reconstruct Domain state and run initial-Publish transition
9. verify Domain resulting revision and timestamp equal the proposed stored result
10. INSERT every document_publish_operations identity/result column with ON CONFLICT DO NOTHING; created_at = published_at
11. zero inserted rows -> refetch operation; matching replay or Conflict
12. conditional Version UPDATE from WORKING to PUBLISHED + published_at
13. conditional Document UPDATE using expected revision and current_version_id IS NULL
14. insert supplied Domain Outbox event
15. insert supplied mandatory Audit Outbox event
16. COMMIT
```

Use `map_statement_error` before commit and `map_commit_error` only for `tx.commit()`.

Document OCC update:

```sql
UPDATE documents
SET current_version_id = $1,
    revision = $2
WHERE document_id = $3
  AND revision = $4
  AND current_version_id IS NULL
```

Version state update:

```sql
UPDATE document_versions
SET lifecycle_state = 'PUBLISHED',
    published_at = $1
WHERE document_version_id = $2
  AND document_id = $3
  AND lifecycle_state = 'WORKING'
```

Require one affected row from each.

### Step 5 — GREEN: complete `DocumentPublishRepository` on PostgreSQL adapter

Implement the three exact Task 2 signatures in `repository.rs`, each delegating to `publish.rs`:

```text
get_publish_operation -> publish::get_publish_operation
get_publish_candidate -> publish::get_publish_candidate
publish_initial_version -> publish::publish_initial_version
```

Do not duplicate SQL in `repository.rs`.

### Step 6 — Verify and commit

```bash
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test repository_contract -- --nocapture
cargo test -p document-repository-postgres --test schema_constraints -- --nocapture
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
git add crates/document-repository-postgres/src crates/document-repository-postgres/tests/publish_transaction.rs
git commit -m "feat: implement atomic initial document publish"
```

---

## Task 7 — Concurrency, unknown-commit recovery, and full vertical slice

**Create:**
- `crates/document-repository-postgres/tests/publish_concurrency.rs`
- `crates/document-application/tests/publish_vertical_slice.rs`

### Step 1 — RED: distinct-operation concurrency

Use PostgreSQL 18.6 with pool size at least 4. Use `tokio::sync::Barrier` to release two workers together and `tokio::join!` to await them.

Both use:

```text
same Document
same target Version
same expected revision 0
different PublishOperationId
different EventId/AuditEventId
```

Require exactly one success and one `Conflict`, with final:

```text
revision = 1
current_version_id = target
Publish operation rows = 1
Publish Domain events = 1
Publish Audit events = 1
```

### Step 2 — RED: same-operation concurrency

Use the same barrier pattern, but both workers submit the same logical operation record. Both callers must receive equal `PublishDocumentResult`; authoritative mutation, revision increment, Domain event, Audit event, and operation row occur once.

Run:

```bash
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
```

Expected: FAIL until race handling is correct.

### Step 3 — GREEN: fix only race defects

Preserve Document-first lock ordering and the second operation lookup. A same-operation contender must resolve to the winner's committed operation record, not a duplicate-key Internal error.

### Step 4 — Full Create → Publish → Get/open vertical slice

Use real:

```text
LocalFileStorage under tempfile::TempDir
PostgreSQL 18.6
PostgresDocumentRepository
DocumentService
```

Use a fixed Clock, deterministic server-generated Event/Audit IDs, and a caller-supplied valid UUIDv7 operation ID.

Flow:

```text
create_document
get_document => WORKING/current None/revision 0
publish_document(expected revision 0)
get_document => PUBLISHED/current target/revision 1
open_primary_file => original bytes
```

Assert one additional Publish Domain event, one Publish Audit event, and one operation row.

### Step 5 — Deterministic unknown-commit recovery without production fault flags

In `publish_vertical_slice.rs`, wrap the real repository with a test-only type implementing both repository traits.

`BeforeCommitUnknown` behavior:

```text
first publish write returns CommitOutcomeUnknown without delegating
retry delegates normally
```

`AfterCommitUnknown` behavior:

```text
first publish write delegates and commits successfully, then masks success as CommitOutcomeUnknown
retry delegates and resolves stored operation replay
```

For each mode:

```text
first Application call = PublishCommitOutcomeUnknown
retry uses the exact same PublishDocumentCommand
final state = one Publish mutation + one Domain event + one Audit event + one operation row
```

### Step 6 — Verify and commit

```bash
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
cargo test -p document-application --test publish_vertical_slice -- --nocapture
cargo test --workspace
git add crates/document-repository-postgres/tests/publish_concurrency.rs crates/document-application/tests/publish_vertical_slice.rs
git commit -m "test: prove document publish concurrency and recovery"
```

---

## Task 8 — Final repository gates, implementation self-review, exact-head evidence

**Modify:**
- `docs/superpowers/execution/document-publish-v0-status.md`
- `docs/superpowers/execution/active.md` when phase/PR changes
- implementation PR body/metadata through GitHub

### Step 1 — Run repository verification entrypoints

```bash
mise run verify:fast
mise run verify
mise run verify:full
```

All must PASS.

### Step 2 — Run explicit Publish evidence tests on the final local tree

```bash
cargo test -p document-domain
cargo test -p document-application --test publish_document_contract -- --nocapture
cargo test -p document-application --test publish_vertical_slice -- --nocapture
cargo test -p document-repository-postgres --test publish_schema -- --nocapture
cargo test -p document-repository-postgres --test publish_transaction -- --nocapture
cargo test -p document-repository-postgres --test publish_concurrency -- --nocapture
```

All Publish cases must PASS with zero skipped cases.

### Step 3 — Self-review frozen Design coverage

Verify explicitly:

```text
initial Publish only                       implemented
caller UUIDv7 operation ID                 implemented + validated
same-operation replay                      implemented
operation-ID misuse conflict               implemented
OCC + short row lock                       implemented
file-preflight semantics                   implemented
same-Document current composite FK         implemented
PUBLISHED/current atomic semantics          implemented
Domain Outbox atomicity                     implemented
mandatory Audit Outbox atomicity            implemented
operation-result atomicity                  implemented
unknown-commit exact-command recovery       implemented
current replacement absent                  confirmed
Withdraw/scheduler/approval absent          confirmed
HTTP/UI/Search/outbox worker absent         confirmed
```

A contract-level mismatch requires an explicit Design amendment before code changes continue.

### Step 4 — Record exact evidence

Update Execution Status with:

```text
completed Task/Step numbers
implementation branch
exact implementation head SHA
verification commands/results/test counts
implementation PR number/state
review findings/blockers
next exact action
approved Design amendments, if any
```

### Step 5 — Create implementation PR and require exact-head CI

Use:

```text
branch = feat/document-publish-v0
base = main
```

Local green results are insufficient. Require PR-triggered hosted CI on the exact final implementation head.

### Step 6 — Review CI and PR feedback

Fetch every required job for the exact head, all inline review threads, and submitted reviews. Fix Critical/Important findings with RED→GREEN evidence. Every branch-tree change requires new exact-head CI.

### Step 7 — Stop at merge gate

When exact-head CI is green and no blocking review finding remains, update the implementation PR evidence and mark it Ready.

**Do not merge the implementation PR without an explicit user merge instruction.**
