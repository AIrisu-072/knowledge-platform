# Document Authoritative Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first authoritative Document Platform capability: create one Document with immutable binary + `DocumentVersion #1 (WORKING)`, persist it safely across local filesystem and PostgreSQL, atomically stage Domain/Audit outbox records, and read it back with invariant-preserving failure recovery.

**Architecture:** Implement four Rust crates with strict dependency direction: `document-domain` contains infrastructure-free business types/invariants; `document-application` owns Create/Get orchestration and ports; `document-storage-fs` implements the file-first durable local store; `document-repository-postgres` implements PostgreSQL authoritative persistence and transactional outbox/audit insertion. The Application finalizes the file before invoking one atomic repository operation; finalized files are never eagerly deleted after an ambiguous DB failure and are later classified by reconciliation.

**Tech Stack:** Rust 1.98.1 / edition 2024, Tokio 1.x, PostgreSQL 18.x (PoC image `postgres:18.6-bookworm`), SQLx 0.9.x, serde/serde_json, thiserror 2.x, uuid 1.x UUIDv7, sha2 0.11.x SHA-256, time 0.3.x, tempfile 3.x, testcontainers 0.28.x.

**Spec:** `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md` — consolidated Design Spec approved by the user on 2026-09-16 in PR #3. Technical body reviewed at commit `1a410ddd87313bc5f15426fa8683430abf32be90`; self-review refinements are included on the design branch.

## Global Constraints

- `DocumentVersion.lifecycle_state` persists exactly `WORKING | PUBLISHED | WITHDRAWN`; this capability creates only `WORKING`.
- Initial `Document.current_version_id = None` and `Document.revision = 0`.
- Domain must not depend on SQLx, PostgreSQL, axum, filesystem APIs, or Firefly Framework.
- Application must not depend on SQLx or physical filesystem path types/APIs.
- File commit ordering is fixed: staging write/hash/count → file sync → same-filesystem atomic rename → directory durability step → PostgreSQL transaction.
- A finalized file must not be eagerly deleted because a DB commit can have unknown outcome.
- Authoritative business state + Domain Outbox + mandatory Audit Outbox commit in the same PostgreSQL transaction.
- Search, extraction, HTTP/OpenAPI transport, ReadState, AccessPolicy, version #2+, publish/withdraw, and outbox delivery are out of scope.
- No ORM, Firefly runtime dependency, generic queue, S3/MinIO, or SQLite substitute is added.
- Use PostgreSQL 18.6 for integration evidence; later PostgreSQL 18.x patch upgrades are dependency maintenance.
- Preserve existing `cargo-deny`, OSV, Gitleaks, architecture, portability, container, SBOM, and Development Assurance gates.

---

## File Structure

Create or modify the following files only as required by this capability:

```text
Cargo.toml
Cargo.lock
mise.toml
mise.lock
spec/architecture/dependency-rules.toml

tools/architecture-lint/src/config.rs
tools/architecture-lint/src/checks.rs
tools/architecture-lint/tests/policy.rs

crates/document-domain/
├─ Cargo.toml
└─ src/
   ├─ lib.rs
   ├─ error.rs
   ├─ ids.rs
   ├─ metadata.rs
   ├─ principal.rs
   ├─ document.rs
   └─ file.rs

crates/document-application/
├─ Cargo.toml
├─ src/
│  ├─ lib.rs
│  ├─ command.rs
│  ├─ error.rs
│  ├─ events.rs
│  ├─ ports.rs
│  ├─ service.rs
│  └─ reconciliation.rs
└─ tests/
   ├─ create_document_contract.rs
   └─ vertical_slice.rs

crates/document-storage-fs/
├─ Cargo.toml
└─ src/
   ├─ lib.rs
   ├─ error.rs
   ├─ ops.rs
   └─ storage.rs

crates/document-repository-postgres/
├─ Cargo.toml
├─ migrations/
│  └─ 0001_document_authoritative_core.sql
└─ src/
   ├─ lib.rs
   ├─ error.rs
   ├─ mapping.rs
   ├─ repository.rs
   └─ rows.rs

.sqlx/
  <generated SQLx query metadata files>
```

The integration test under `document-application/tests/vertical_slice.rs` may use the two infrastructure crates as **dev-dependencies only**; production dependency direction remains Application → ports, Infrastructure → Application/Domain.

---

### Task 1: Freeze workspace dependencies and machine-enforce the new boundaries

**Files:**
- Modify: `Cargo.toml`
- Modify: `spec/architecture/dependency-rules.toml`
- Modify: `tools/architecture-lint/src/config.rs`
- Modify: `tools/architecture-lint/src/checks.rs`
- Modify: `tools/architecture-lint/tests/policy.rs`
- Create: `crates/document-domain/Cargo.toml`
- Create: `crates/document-domain/src/lib.rs`
- Create: `crates/document-application/Cargo.toml`
- Create: `crates/document-application/src/lib.rs`
- Create: `crates/document-storage-fs/Cargo.toml`
- Create: `crates/document-storage-fs/src/lib.rs`
- Create: `crates/document-repository-postgres/Cargo.toml`
- Create: `crates/document-repository-postgres/src/lib.rs`

**Interfaces:**
- Consumes: existing workspace/tooling baseline.
- Produces: four compileable empty capability crates and architecture rules that forbid invalid dependency directions.

- [ ] **Step 1: Add failing architecture-policy tests**

Add tests to `tools/architecture-lint/tests/policy.rs` that create fixture manifests/source files and assert findings for these violations:

```text
crates/document-domain/Cargo.toml -> sqlx dependency
crates/document-domain/src/lib.rs -> use std::path::PathBuf
crates/document-application/Cargo.toml -> sqlx dependency
crates/document-application/src/lib.rs -> use tokio::fs
```

Use stable finding codes:

```text
ARCH_FORBIDDEN_CRATE_DEPENDENCY
ARCH_FORBIDDEN_SOURCE_PATTERN
```

- [ ] **Step 2: Run the policy tests and verify they fail**

Run:

```bash
cargo test -p architecture-lint --test policy
```

Expected: FAIL because crate-boundary rules do not exist yet.

- [ ] **Step 3: Extend dependency-rules configuration**

Add a generic rule shape equivalent to:

```toml
[workspace.boundaries.document_domain]
crate_path = "crates/document-domain"
forbidden_dependencies = ["sqlx", "axum", "tokio"]
forbidden_source_patterns = ["std::fs", "std::path", "tokio::fs"]

[workspace.boundaries.document_application]
crate_path = "crates/document-application"
forbidden_dependencies = ["sqlx", "axum"]
forbidden_source_patterns = ["std::fs", "std::path", "tokio::fs"]
```

Parse `[dependencies]` and target-specific production dependency tables; do not reject `dev-dependencies` used only by integration tests.

- [ ] **Step 4: Implement boundary checks**

Implement generic manifest/source scanning in `tools/architecture-lint/src/checks.rs`. A forbidden dependency finding must identify the crate manifest and dependency name. A forbidden source-pattern finding must identify the exact source path and pattern.

- [ ] **Step 5: Add workspace members and dependency baseline**

Update root `Cargo.toml` with the four new workspace members and workspace dependencies equivalent to:

```toml
tokio = { version = "1", features = ["fs", "io-util", "macros", "rt-multi-thread", "sync", "time"] }
sqlx = { version = "0.9", default-features = false, features = ["runtime-tokio", "postgres", "migrate", "macros", "uuid", "json", "time"] }
uuid = { version = "1", features = ["v7", "serde"] }
sha2 = "0.11"
time = { version = "0.3", features = ["serde"] }
tempfile = "3"
testcontainers = "0.28"
```

Keep existing `serde`, `serde_json`, and `thiserror` workspace dependencies.

Create minimal crate manifests so:

```text
document-domain -> uuid, time, serde, serde_json, thiserror
document-application -> document-domain, tokio, serde_json, thiserror, uuid, time
document-storage-fs -> document-domain, document-application, tokio, sha2, thiserror, time
document-repository-postgres -> document-domain, document-application, sqlx, serde_json, thiserror, uuid, time
```

- [ ] **Step 6: Run architecture/static checks**

Run:

```bash
cargo test -p architecture-lint --test policy
mise run arch:check
cargo check --workspace
cargo deny check
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock spec/architecture/dependency-rules.toml tools/architecture-lint crates/document-*
git commit -m "chore: establish document core crate boundaries"
```

---

### Task 2: Implement infrastructure-free Domain invariants

**Files:**
- Create: `crates/document-domain/src/error.rs`
- Create: `crates/document-domain/src/ids.rs`
- Create: `crates/document-domain/src/metadata.rs`
- Create: `crates/document-domain/src/principal.rs`
- Create: `crates/document-domain/src/document.rs`
- Create: `crates/document-domain/src/file.rs`
- Modify: `crates/document-domain/src/lib.rs`

**Interfaces:**
- Consumes: `uuid::Uuid`, `time::OffsetDateTime`, JSON values only as value-object internals.
- Produces: typed IDs, `PrincipalRef`, `Metadata`, `LifecycleState`, `Document`, `DocumentVersion`, `ContentHash`, `StorageKey`, `FileObject`, `VersionFile`, `InitialDocument`.

- [ ] **Step 1: Write failing ID/value-object tests**

Define tests for:

```rust
assert!(VersionNo::new(0).is_err());
assert_eq!(VersionNo::new(1).unwrap().get(), 1);
assert!(ContentHash::from_slice(&[0u8; 31]).is_err());
assert!(FileSize::new(-1).is_err());
assert!(Title::new("   ").is_err());
```

- [ ] **Step 2: Run and verify RED**

```bash
cargo test -p document-domain
```

Expected: FAIL because types are not implemented.

- [ ] **Step 3: Implement typed IDs and validated value objects**

Use UUID-backed newtypes such as:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(Uuid);

impl DocumentId {
    pub const fn from_uuid(value: Uuid) -> Self { Self(value) }
    pub const fn as_uuid(self) -> Uuid { self.0 }
}
```

Implement the same shape for `DocumentVersionId`, `FileId`, `FolderId`, `EventId`, and `AuditEventId`.

`ContentHash` stores exactly `[u8; 32]`. `StorageKey` stores an opaque non-empty relative key and rejects absolute/parent traversal components before it can reach an adapter.

- [ ] **Step 4: Write failing initial-aggregate tests**

Assert an `InitialDocument::create(...)` equivalent produces:

```text
version_no = 1
lifecycle = WORKING
current_version_id = None
document.revision = 0
file role = PRIMARY
ordinal = 0
```

Also assert no public constructor permits an initial `PUBLISHED` version.

- [ ] **Step 5: Implement initial aggregate construction**

Use a constructor signature equivalent to:

```rust
pub struct CreateInitialDocument {
    pub document_id: DocumentId,
    pub version_id: DocumentVersionId,
    pub file_id: FileId,
    pub folder_id: FolderId,
    pub title: Title,
    pub document_metadata: Metadata,
    pub version_metadata: Metadata,
    pub principal: PrincipalRef,
    pub stored_file: StoredFileDescriptor,
    pub original_filename: String,
    pub created_at: OffsetDateTime,
}

impl InitialDocument {
    pub fn create(input: CreateInitialDocument) -> Result<Self, DomainError>;
}
```

`StoredFileDescriptor` is a domain value containing `StorageKey`, `ContentHash`, non-negative size, and media type; it contains no `PathBuf`.

- [ ] **Step 6: Run Domain tests and static policy**

```bash
cargo test -p document-domain
mise run arch:check
cargo clippy -p document-domain --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/document-domain
 git commit -m "feat: add document authoritative domain model"
```

---

### Task 3: Implement Application ports and Create/Get orchestration

**Files:**
- Create: `crates/document-application/src/command.rs`
- Create: `crates/document-application/src/error.rs`
- Create: `crates/document-application/src/events.rs`
- Create: `crates/document-application/src/ports.rs`
- Create: `crates/document-application/src/service.rs`
- Modify: `crates/document-application/src/lib.rs`
- Create: `crates/document-application/tests/create_document_contract.rs`

**Interfaces:**
- Consumes: Domain types from Task 2.
- Produces: `ContentReader`, `IdGenerator`, `Clock`, `FileStorage`, `DocumentRepository`, `CreateDocumentCommand`, `CreateDocumentResult`, `AuthoritativeDocument`, `DocumentService`.

- [ ] **Step 1: Define the port contracts in tests first**

Use an application-owned stream alias:

```rust
pub type ContentReader = std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send + Unpin>>;
```

Define ports equivalent to:

```rust
pub trait IdGenerator: Send + Sync {
    fn next_uuid_v7(&self) -> uuid::Uuid;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> time::OffsetDateTime;
}

pub trait FileStorage: Send + Sync {
    async fn put_immutable(&self, request: StoreFileRequest) -> Result<StoredFile, StorageError>;
    async fn open(&self, key: &StorageKey) -> Result<ContentReader, StorageError>;
    async fn list_objects(&self) -> Result<Vec<StorageObjectInfo>, StorageError>;
}

pub trait DocumentRepository: Send + Sync {
    async fn create_initial_document(&self, record: CreateInitialDocumentRecord) -> Result<(), RepositoryError>;
    async fn get_authoritative_document(&self, id: DocumentId) -> Result<Option<AuthoritativeDocument>, RepositoryError>;
    async fn file_reference_exists(&self, file_id: FileId) -> Result<bool, RepositoryError>;
}
```

- [ ] **Step 2: Write RED contract tests with fakes**

The fake storage records `finalized_at_step`; the fake repository records `repository_called_at_step`. Assert:

```rust
assert!(storage.finalized_at_step() < repo.repository_called_at_step());
```

Make the repository return `RepositoryError::CommitOutcomeUnknown` and assert the fake storage's delete counter stays `0`.

- [ ] **Step 3: Run RED**

```bash
cargo test -p document-application --test create_document_contract
```

Expected: FAIL because `DocumentService` is missing.

- [ ] **Step 4: Implement event records and CreateDocument**

`CreateDocument` must pre-generate IDs for document, version, file, two Domain events, and two Audit events; call storage once; build the Domain aggregate; then invoke one repository operation.

Create the initial event types exactly as constants/typed values:

```text
DocumentCreated
DocumentVersionCreated
document.created
document.version.created
```

Both event groups use the same operation timestamp from `Clock`.

- [ ] **Step 5: Implement GetDocument + binary open integrity mapping**

`get_document(id)` maps repository `None` to `ApplicationError::DocumentNotFound`.

`open_primary_file(id)` first loads authoritative metadata and then calls `FileStorage::open`. If the storage adapter reports the referenced final object missing, map it to `ApplicationError::IntegrityViolation` rather than not-found.

- [ ] **Step 6: Run tests**

```bash
cargo test -p document-application
mise run arch:check
cargo clippy -p document-application --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/document-application
 git commit -m "feat: orchestrate document create and get"
```

---

### Task 4: Implement durable local filesystem storage

**Files:**
- Create: `crates/document-storage-fs/src/error.rs`
- Create: `crates/document-storage-fs/src/ops.rs`
- Create: `crates/document-storage-fs/src/storage.rs`
- Modify: `crates/document-storage-fs/src/lib.rs`
- Modify: `crates/document-storage-fs/Cargo.toml`

**Interfaces:**
- Consumes: `FileStorage`, `StoreFileRequest`, `StoredFile`, `StorageObjectInfo` from Application and storage value types from Domain.
- Produces: `FileSystemStorage` implementing the approved file-first durability protocol.

- [ ] **Step 1: Write RED happy-path filesystem tests**

Using `tempfile::TempDir`, store bytes `b"authoritative-content"` and assert:

```text
staging directory contains no completed part file
objects/<derived FileId key> exists
returned size == input length
returned SHA-256 == SHA-256(input)
read-back bytes == input bytes
```

Pass original filenames `../../escape.pdf`, `同名.pdf`, and a 255-character name and assert final key/path is unchanged because only `FileId` determines storage identity.

- [ ] **Step 2: Write RED failure-point tests**

Implement an internal `StorageOps` seam used only inside this crate so tests can fail exactly at:

```rust
enum FsFailurePoint { Write, SyncFile, Rename, SyncDirectory }
```

Each injected failure must return the corresponding storage error and must never report a successful final object.

- [ ] **Step 3: Run RED**

```bash
cargo test -p document-storage-fs
```

Expected: FAIL until storage exists.

- [ ] **Step 4: Implement staging + hash + sync + rename**

Use one storage root:

```text
root/staging/<file-id>.part
root/objects/<first-two-hex>/<file-id>
```

Write the stream in chunks while updating `sha2::Sha256` and byte count. Call `File::sync_all()` before rename. Rename only within the same root filesystem.

- [ ] **Step 5: Implement directory durability**

On Unix production targets, open/sync the destination parent directory after rename so the directory entry is included in the durability protocol. Keep platform-specific code inside this adapter; Domain/Application remain platform-neutral.

- [ ] **Step 6: Implement object enumeration**

Return `StorageObjectInfo` for staging/final objects with object kind, parsed `FileId` when valid, and modified timestamp. Enumeration must ignore malformed/unrecognized filenames as `StorageObjectKind::Unknown` rather than deleting them.

- [ ] **Step 7: Run adapter tests**

```bash
cargo test -p document-storage-fs
cargo clippy -p document-storage-fs --all-targets -- -D warnings
mise run arch:check
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/document-storage-fs
 git commit -m "feat: add durable local document storage"
```

---

### Task 5: Implement PostgreSQL schema and atomic repository

**Files:**
- Create: `crates/document-repository-postgres/migrations/0001_document_authoritative_core.sql`
- Create: `crates/document-repository-postgres/src/error.rs`
- Create: `crates/document-repository-postgres/src/mapping.rs`
- Create: `crates/document-repository-postgres/src/rows.rs`
- Create: `crates/document-repository-postgres/src/repository.rs`
- Modify: `crates/document-repository-postgres/src/lib.rs`
- Modify: `crates/document-repository-postgres/Cargo.toml`

**Interfaces:**
- Consumes: `DocumentRepository`, `CreateInitialDocumentRecord`, Domain models.
- Produces: `PostgresDocumentRepository::new(PgPool)`, `migrate(&PgPool)`, atomic create/get/reference lookup.

- [ ] **Step 1: Write the migration with explicit DB constraints**

The migration must create:

```text
folders
documents
document_versions
file_objects
version_files
outbox_events
audit_outbox_events
```

Required constraints include:

```sql
CHECK (revision >= 0)
CHECK (version_no >= 1)
CHECK (lifecycle_state IN ('WORKING','PUBLISHED','WITHDRAWN'))
CHECK (lifecycle_state <> 'PUBLISHED' OR published_at IS NOT NULL)
CHECK (lifecycle_state <> 'WITHDRAWN' OR withdrawn_at IS NOT NULL)
CHECK (octet_length(content_hash) = 32)
CHECK (size_bytes >= 0)
UNIQUE (document_id, version_no)
UNIQUE (storage_locator)
```

Create a partial unique index equivalent to:

```sql
CREATE UNIQUE INDEX uq_version_files_primary
ON version_files(document_version_id)
WHERE role = 'PRIMARY';
```

Seed one system root folder with a fixed UUID constant shared by integration tests.

- [ ] **Step 2: Write RED real-Postgres constraint tests**

Use `testcontainers` with image `postgres:18.6-bookworm`, run migrations, then execute deliberately invalid SQL for each constraint and assert PostgreSQL rejects it.

- [ ] **Step 3: Run RED**

```bash
cargo test -p document-repository-postgres --test '*' -- --nocapture
```

Expected: FAIL until repository/tests are wired.

- [ ] **Step 4: Implement atomic `create_initial_document`**

Open one SQLx transaction and execute in this order:

```text
folder existence check
INSERT file_objects
INSERT documents
INSERT document_versions
INSERT version_files
INSERT two outbox_events
INSERT two audit_outbox_events
COMMIT
```

Map missing folder to `RepositoryError::FolderNotFound`. All statement errors before commit roll back and map to a non-ambiguous repository failure.

- [ ] **Step 5: Implement conservative commit-error mapping**

A `tx.commit().await` error is mapped to `RepositoryError::CommitOutcomeUnknown` because the caller cannot safely infer rollback from a transport/session failure after COMMIT was issued.

Do not compensate by deleting the final file.

- [ ] **Step 6: Implement get/reference queries**

`get_authoritative_document` joins the initial/currently-addressed version and primary file metadata without relying on Search indexes. `file_reference_exists` answers from `file_objects`/`version_files` authoritative state.

- [ ] **Step 7: Prove transaction rollback at every write stage**

In integration tests, install temporary PostgreSQL triggers that raise an exception on `documents`, `document_versions`, `version_files`, `outbox_events`, and `audit_outbox_events` one stage at a time. After each failure assert every authoritative/outbox/audit row count for the attempted aggregate is zero.

- [ ] **Step 8: Run PostgreSQL tests**

```bash
cargo test -p document-repository-postgres -- --nocapture
cargo clippy -p document-repository-postgres --all-targets -- -D warnings
```

Expected: PASS against PostgreSQL 18.6.

- [ ] **Step 9: Commit**

```bash
git add crates/document-repository-postgres
 git commit -m "feat: persist document core atomically in postgres"
```

---

### Task 6: Implement reconciliation and unknown-commit recovery contracts

**Files:**
- Create: `crates/document-application/src/reconciliation.rs`
- Modify: `crates/document-application/src/service.rs`
- Modify: `crates/document-application/src/lib.rs`
- Add tests in: `crates/document-application/tests/create_document_contract.rs`

**Interfaces:**
- Consumes: `FileStorage::list_objects`, `DocumentRepository::file_reference_exists`, `Clock`.
- Produces: `ReconciliationClassification::{Healthy, StaleStaging, Orphan, IntegrityViolation}` and safe retry/query behavior after `CommitOutcomeUnknown`.

- [ ] **Step 1: Write RED classification tests**

Cover exactly:

```text
DB present + final present -> Healthy
DB absent + staging + grace elapsed -> StaleStaging
DB absent + final + grace elapsed -> Orphan
DB present + file absent -> IntegrityViolation
DB absent + final + grace not elapsed -> no cleanup candidate
```

- [ ] **Step 2: Implement pure classification first**

Create a pure function equivalent to:

```rust
pub fn classify(
    db_referenced: bool,
    object: Option<&StorageObjectInfo>,
    now: OffsetDateTime,
    grace: Duration,
) -> ReconciliationClassification;
```

Keep deletion/scheduling out of scope.

- [ ] **Step 3: Add CommitOutcomeUnknown service test**

Use a fake repository that persists the record in memory and then returns `CommitOutcomeUnknown`. Assert:

```text
CreateDocument returns CommitOutcomeUnknown
final file remains
query by pre-generated DocumentId finds the record
```

Also test a fake repository that returns `CommitOutcomeUnknown` without persisting; the same file remains and reconciliation later classifies it `Orphan` only after grace.

- [ ] **Step 4: Implement safe lookup helper**

Expose an application operation that can query a known `DocumentId` after an ambiguous create result; do not silently retry Create with newly generated IDs.

- [ ] **Step 5: Run Application tests**

```bash
cargo test -p document-application
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/document-application
 git commit -m "feat: classify document storage recovery states"
```

---

### Task 7: Prove the full real-filesystem + real-PostgreSQL vertical slice

**Files:**
- Create: `crates/document-application/tests/vertical_slice.rs`
- Modify: `crates/document-application/Cargo.toml` dev-dependencies only

**Interfaces:**
- Consumes: public Application service, `FileSystemStorage`, `PostgresDocumentRepository`.
- Produces: executable evidence that the approved architecture works as one slice.

- [ ] **Step 1: Add dev-dependencies only**

Add `document-storage-fs`, `document-repository-postgres`, `testcontainers`, and `tempfile` under `[dev-dependencies]` of `document-application`. Do not add Infrastructure crates under production `[dependencies]`.

- [ ] **Step 2: Write RED happy-path vertical test**

Start PostgreSQL 18.6, migrate, create a temp storage root, construct the real adapters, and execute:

```text
CreateDocument
→ GetDocument
→ open_primary_file
```

Assert:

```text
DocumentVersion.version_no == 1
LifecycleState == Working
Document.current_version_id == None
Document.revision == 0
input bytes == read-back bytes
SHA-256 matches
one FileObject
one VersionFile PRIMARY
two Domain Outbox rows
two Audit Outbox rows
```

- [ ] **Step 3: Run RED**

```bash
cargo test -p document-application --test vertical_slice -- --nocapture
```

Expected: FAIL until adapter wiring is complete.

- [ ] **Step 4: Complete only the minimal wiring required by the test**

Do not add HTTP, background workers, Search, or additional version lifecycle operations.

- [ ] **Step 5: Add integrity-failure vertical test**

After successful create, remove the physical final file directly from the temp test root and call `open_primary_file`. Assert `ApplicationError::IntegrityViolation`.

- [ ] **Step 6: Run all capability tests**

```bash
cargo nextest run -p document-domain -p document-application -p document-storage-fs -p document-repository-postgres
mise run arch:check
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/document-application Cargo.lock
 git commit -m "test: prove document authoritative vertical slice"
```

---

### Task 8: Add SQLx metadata, pinned tooling, CI gates, and final evidence

**Files:**
- Modify: `mise.toml`
- Modify: `mise.lock`
- Modify: `.github/workflows/ci.yml`
- Create/Update: `.sqlx/*`
- Modify: `Cargo.lock`
- If assurance scan requires mapping updates, modify only the generated/declared assurance configuration already used by `tools/assurance`; do not create a second assurance system.

**Interfaces:**
- Consumes: complete capability from Tasks 1-7.
- Produces: reproducible SQLx verification and repository-wide completion evidence.

- [ ] **Step 1: Pin SQLx CLI**

Add:

```toml
"cargo:sqlx-cli" = "0.9.0"
```

to `mise.toml`, include it in bootstrap/install tasks, then regenerate `mise.lock` with the repository's normal locked-tool flow.

- [ ] **Step 2: Add deterministic SQLx prepare/check tasks**

Add `mise run sqlx:prepare` and `mise run sqlx:check` scripts that:

1. start `postgres:18.6-bookworm` with a unique local container name;
2. wait for `pg_isready` inside the container;
3. set `DATABASE_URL` to the temporary database;
4. run the migration from `crates/document-repository-postgres/migrations`;
5. run `cargo sqlx prepare --workspace -- --all-targets` or `--check`;
6. remove the container in a shell trap.

The task must fail closed if migration or prepare/check fails.

- [ ] **Step 3: Generate `.sqlx` metadata**

Run:

```bash
mise run sqlx:prepare
```

Commit the resulting `.sqlx` metadata.

- [ ] **Step 4: Extend CI**

In `rust-static`, install the pinned SQLx CLI through mise and run:

```bash
mise run sqlx:check
```

Keep all GitHub Actions build/test logic delegated through mise. Do not add a second workflow.

- [ ] **Step 5: Run capability and repository gates**

Run locally:

```bash
mise run fmt
mise run check:rust
mise run arch:check
mise run arch:negative-smoke
mise run sqlx:check
mise run test:rust
mise run security
mise run assure:scan
mise run assure:plan
mise run assure:run
mise run assure:report
mise run verify:full
```

Expected: every command exits 0.

- [ ] **Step 6: Verify scope exclusions by source scan**

Run:

```bash
git diff --name-only main...HEAD
git grep -n -E 'axum|tower-http|tantivy|lindera|firefly-' -- crates/document-* Cargo.toml || true
```

Review any match. Expected production result: no new HTTP/Search/Firefly runtime dependency and no code implementing version #2+, publish/withdraw, ReadState, or AccessPolicy.

- [ ] **Step 7: Commit final verification wiring**

```bash
git add mise.toml mise.lock .github/workflows/ci.yml .sqlx Cargo.lock
git commit -m "ci: verify document authoritative core"
```

- [ ] **Step 8: Push and require hosted CI evidence before completion claim**

After pushing, require the PR-triggered workflow on the exact implementation HEAD to report success for every required predecessor and `required-check`. Do not mark the capability complete from local results alone.

---

## Self-Review Checklist Applied to This Plan

### Spec coverage

- Domain ownership/lifecycle: Tasks 2-3.
- File-first durability + directory sync: Task 4.
- PostgreSQL authoritative schema/atomicity/outbox/audit: Task 5.
- Commit outcome unknown + orphan reconciliation: Task 6.
- Real Create/Get evidence: Task 7.
- SQLx offline/reproducibility, architecture/license/security/assurance gates: Task 8.
- Explicitly excluded features are absent from every task.

### Placeholder scan

The plan contains no `TBD`, `TODO`, "implement later", or unspecified "add tests" instructions. Each task names exact files, interfaces, commands, expected behavior, and commit boundary.

### Type consistency

- `DocumentId`, `DocumentVersionId`, `FileId`, `FolderId`, `StorageKey`, `ContentHash` originate in `document-domain` and are reused unchanged.
- `FileStorage`, `DocumentRepository`, stream types, repository records, and application errors originate in `document-application`; adapters implement those exact contracts.
- Infrastructure crates appear in Application only as dev-dependencies for the vertical integration test.
- PostgreSQL commit ambiguity maps to the single application-visible `CommitOutcomeUnknown` semantic; no compensating final-file delete exists.
