# Document Publish v0 — Design

- Status: **REVIEW — conversational design approved; written-spec review pending**
- Date: 2026-09-17
- Capability: `Document Publish v0`
- Repository baseline: `main@2ead1e21222c3b122704b6e0ce31f3f902b93659`
- Design branch: `design/document-publish-v0`
- Scope class: Product Capability / Architectural

## 1. Purpose

This capability adds the first publication operation on top of the merged Document Authoritative Core.

The supported use case is intentionally narrow: publish the initial `DocumentVersion #1` of an existing Document whose `current_version_id` is still `None`.

On success, the operation atomically establishes the initial current published version in PostgreSQL while preserving the existing Domain / Application / Infrastructure dependency boundaries, mandatory Domain Outbox and Audit Outbox behavior, optimistic concurrency control, and conservative commit-ambiguity semantics.

This capability does not add version creation, current-version replacement, withdrawal, approval workflow, scheduled publication, HTTP transport, UI, Search implementation, or outbox delivery.

## 2. Normative context

This design extends and must remain consistent with:

- `spec/architecture/architecture-contract-v0.md`
- `spec/architecture/system-architecture-v0.md`
- `spec/data/logical-data-model-v0.md`
- `spec/data/transaction-consistency-requirements-v0.md`
- `spec/operations/error-handling-resilience-requirements-v0.md`
- `spec/operations/observability-audit-requirements-v0.md`
- `docs/superpowers/specs/2026-09-16-document-authoritative-core-design.md`

If this design conflicts with higher-priority normative SSOT, the normative SSOT wins and this design must be amended before implementation proceeds.

## 3. Scope

### 3.1 Included

`PublishDocument` for the initial Version only:

- caller supplies a stable publish operation identity;
- caller identifies both `DocumentId` and target `DocumentVersionId`;
- caller supplies expected Document revision for OCC;
- Application verifies the target PRIMARY file is still available before publication;
- PostgreSQL atomically:
  - validates idempotency;
  - serializes the current-version transition;
  - validates the expected revision;
  - changes target lifecycle state from `WORKING` to `PUBLISHED`;
  - sets `published_at`;
  - sets `Document.current_version_id` to the target Version;
  - increments `Document.revision` exactly once;
  - appends one Domain Outbox event;
  - appends one mandatory Audit Outbox event;
  - persists one successful `document_publish_operations` record.

### 3.2 Explicitly excluded

- `DocumentVersion #2+` creation;
- replacing an existing current Version;
- `WITHDRAWN` transition;
- approval workflow or mutation of `approved_at`;
- scheduled publication or scheduler;
- `effective_from` / `effective_to` policy;
- AccessPolicy / authorization;
- HTTP / OpenAPI transport;
- UI;
- Search indexing implementation;
- Outbox delivery worker;
- Audit Store delivery;
- generic idempotency framework;
- cleanup of publish-operation records;
- full-file hash recomputation at publish time;
- PostgreSQL triggers that implement lifecycle transition logic.

## 4. Command and result contract

### 4.1 Publish operation identity

`PublishOperationId` is an Application-layer UUIDv7 newtype.

It is caller-generated before the operation begins. This is required so that a caller can safely retry the exact same command if the database commit result becomes unknown.

It is not a Domain entity identity and is therefore not added to `document-domain`.

### 4.2 Command

Conceptual contract:

```text
PublishDocumentCommand
├─ publish_operation_id: PublishOperationId
├─ document_id: DocumentId
├─ target_document_version_id: DocumentVersionId
├─ expected_document_revision: i64
└─ principal: PrincipalRef
```

`expected_document_revision` must be non-negative.

The command identity used for idempotency comparison consists of:

- `document_id`;
- `target_document_version_id`;
- `expected_document_revision`;
- principal identity provider;
- principal stable ID.

Reusing one `publish_operation_id` with different command identity is a Conflict.

### 4.3 Result

```text
PublishDocumentResult
├─ publish_operation_id
├─ document_id
├─ document_version_id
├─ resulting_document_revision
└─ published_at
```

Initial success and an idempotent replay return the same logical result.

No `replayed` flag is exposed. Replay is an execution concern, not a different business outcome.

## 5. Initial-publication state transition

The Domain owns the publication invariant and transition.

Conceptually:

```text
Document::publish_initial_version(
    &mut self,
    target: &mut DocumentVersion,
    published_at: OffsetDateTime,
) -> Result<PublishTransition, DomainError>
```

### 5.1 Preconditions

For a new initial publication:

```text
target.document_id == document.document_id
document.current_version_id == None
target.lifecycle_state == WORKING
```

No `approved_at` requirement is introduced by Publish v0.

`scheduled_publish_at` is not interpreted or executed by this capability.

### 5.2 Successful transition

```text
target.lifecycle_state = PUBLISHED
target.published_at = published_at
document.current_version_id = target.document_version_id
document.revision = document.revision + 1
```

The transition must use checked revision increment semantics.

### 5.3 Existing current Version

Publish v0 does not replace current Versions.

```text
current_version_id == None
    -> initial publication may proceed

current_version_id == target
    -> only the exact same publish_operation_id may replay successfully

current_version_id == Some(other)
    -> Conflict
```

The general T3 previous-current behavior from the transaction-consistency SSOT is deferred until Version #2+ and current-version replacement are designed together.

## 6. Idempotency semantics

### 6.1 Successful-operation record

A dedicated table is used rather than a generic idempotency subsystem:

```text
document_publish_operations
```

Only successful Publish transactions leave a record.

Records are retained without TTL in v0.

### 6.2 Replay rules

```text
same publish_operation_id + same command identity
    -> return the stored PublishDocumentResult
    -> no revision increment
    -> no additional Domain Outbox event
    -> no additional Audit Outbox event

same publish_operation_id + different command identity
    -> Conflict
```

A different `publish_operation_id` is never treated as a replay merely because it targets an already-published Version.

This preserves the normative rule that concurrent Publish requests allow exactly one business operation to succeed.

## 7. File-integrity preflight

The authoritative-core invariant requires a Version-referenced FileObject to remain usable.

Before attempting a new Publish transaction, Application loads the publication candidate and verifies the target PRIMARY final object is available through the `FileStorage` port.

The check is intentionally limited to existence / storage-level readability or openability.

Publish v0 does not recompute the complete SHA-256 hash because immutable storage and hash verification were established during creation.

If the target PRIMARY file is unavailable:

```text
IntegrityViolation
no Publish DB transaction
no revision change
no current_version_id change
no Publish Domain Outbox event
no Publish Audit Outbox event
```

A previously successful `publish_operation_id` replay is resolved before the file preflight so that later storage failure does not change the historical result of an already-committed command.

The file preflight is not part of PostgreSQL ACID. Authoritative DB state is reloaded and revalidated inside the publication transaction after the preflight.

## 8. Application / repository boundary

The Application layer does not receive a generic transaction primitive.

The Repository port gains capability-specific operations equivalent to:

```text
get_publish_operation(publish_operation_id)
get_publish_candidate(document_id, target_document_version_id)
publish_initial_version(PublishInitialVersionRecord)
```

Exact Rust names may be refined by the implementation plan, but responsibilities are normative:

- operation lookup provides replay / misuse detection before storage preflight;
- candidate lookup provides authoritative Document, target Version, and PRIMARY File metadata without exposing SQL rows;
- `publish_initial_version` owns the atomic PostgreSQL mutation and final concurrency decision.

## 9. PostgreSQL transaction algorithm

For a new operation, the adapter uses one short transaction.

Conceptual order:

```text
BEGIN
  |
  |-- lookup publish_operation_id
  |     |-- same command -> Replay(saved result)
  |     |-- different command -> Conflict
  |     `-- absent -> continue
  |
  |-- lock target Document row FOR UPDATE
  |
  |-- lookup publish_operation_id again
  |     |-- same command -> Replay(saved result)
  |     |-- different command -> Conflict
  |     `-- absent -> continue
  |
  |-- load target Version and authoritative state
  |-- verify target belongs to Document
  |-- verify target is WORKING
  |-- verify current_version_id is None
  |-- verify Document.revision == expected_document_revision
  |
  |-- claim publish_operation_id in document_publish_operations
  |     `-- unique conflict resolved as replay or Conflict
  |
  |-- target lifecycle_state -> PUBLISHED
  |-- target published_at -> operation timestamp
  |-- Document.current_version_id -> target
  |-- Document.revision -> expected + 1
  |-- insert Domain Outbox event
  |-- insert mandatory Audit Outbox event
  `-- COMMIT
```

The second operation lookup is required because another transaction may have committed the same operation while this transaction waited for the Document row lock.

The operation-row insert remains inside the same transaction. Any later validation or mutation failure rolls back the operation row with all other state.

### 9.1 Concurrency model

Publish v0 uses both:

- OCC through `expected_document_revision`;
- a short row-level lock for the current-version switch.

This follows the normative model: OCC is the default, while short high-integrity current-version transitions may use row-level locking.

For two distinct operation IDs using the same expected revision on the same Document, exactly one may commit. The other receives Conflict and must reload current authoritative state.

## 10. Commit outcome ambiguity

Existing authoritative-core behavior is preserved: any PostgreSQL commit error is conservatively treated as `CommitOutcomeUnknown` because the client may not know whether the server committed.

Application returns:

```text
CommitOutcomeUnknown {
    publish_operation_id,
    document_id,
    document_version_id,
}
```

Application does not silently retry with a new ID.

Recovery procedure:

1. resend the same command with the same `publish_operation_id`;
2. if the previous transaction committed, the operation record returns the stored result;
3. if it did not commit, the operation record is absent and the normal Publish flow executes;
4. in either case the final business mutation occurs at most once.

## 11. Database migration

Add:

```text
crates/document-repository-postgres/migrations/0002_document_publish_v0.sql
```

Do not rewrite `0001_document_authoritative_core.sql`.

### 11.1 Current-Version ownership constraint

The existing single-column `current_version_id` foreign key is insufficient to prove that the referenced Version belongs to the same Document.

Add a candidate key:

```sql
UNIQUE (document_id, document_version_id)
```

on `document_versions`, then replace the single-column current-version FK with the equivalent composite FK:

```text
documents(document_id, current_version_id)
    -> document_versions(document_id, document_version_id)
```

With `current_version_id = NULL`, no current Version is asserted.

When non-null, cross-Document current-version assignment is rejected by PostgreSQL.

The requirement that current points only to a `PUBLISHED` Version is enforced by Domain invariants plus the atomic Repository transition, not by a lifecycle trigger or redundant helper column.

### 11.2 Publish operation table

Conceptual schema:

```text
document_publish_operations
├─ publish_operation_id UUID PRIMARY KEY
├─ document_id UUID NOT NULL
├─ target_document_version_id UUID NOT NULL
├─ expected_document_revision BIGINT NOT NULL
├─ actor_identity_provider TEXT NOT NULL
├─ actor_principal_id TEXT NOT NULL
├─ published_at TIMESTAMPTZ NOT NULL
├─ resulting_document_revision BIGINT NOT NULL
└─ created_at TIMESTAMPTZ NOT NULL
```

Required constraints include:

```text
expected_document_revision >= 0
resulting_document_revision = expected_document_revision + 1
(document_id, target_document_version_id)
    -> document_versions(document_id, document_version_id)
```

The record is a durable command-result record, not an Audit Store substitute.

## 12. Domain and Audit events

### 12.1 Domain Outbox

One event is emitted per committed business publication:

```text
DocumentVersionPublished
```

Aggregate remains `Document`.

Payload contains only identifiers and transition facts needed by downstream consumers:

```text
documentId
documentVersionId
resultingDocumentRevision
publishedAt
publishOperationId
```

No document body, filename, or arbitrary metadata is included.

### 12.2 Mandatory Audit Outbox

Document publication is a mandatory audited lifecycle operation.

The logical audit action is:

```text
document.version.published
```

The future CloudEvents delivery envelope must be compatible with:

```text
source  = urn:knowledge-platform:document
type    = com.knowledge-platform.document.version.published.v1
subject = document/{document_id}/version/{document_version_id}
```

The current Audit Outbox representation may retain its existing internal event naming convention as long as the delivery boundary can deterministically produce the required CloudEvents envelope.

Audit data records at least:

```text
publishOperationId
expectedDocumentRevision
resultingDocumentRevision
actor stable identity
result = success
publishedAt
```

Actor/resource identifiers already present as structured Audit fields need not be duplicated unless required by the final envelope mapping.

No document body, filename, or sensitive arbitrary metadata is copied into Audit data.

Successful business state, Domain Outbox, Audit Outbox, and publish-operation result are committed in one PostgreSQL transaction.

Failed validation / Conflict requests do not create a success Audit record inside a rolled-back business transaction. Any later failure-audit policy belongs to the error/API audit design and is not introduced here.

## 13. Error model

Publish v0 distinguishes at least:

```text
DocumentNotFound
DocumentVersionNotFound
Conflict
BusinessRule
IntegrityViolation
RepositoryUnavailable
CommitOutcomeUnknown
Internal
```

Expected mapping:

| Condition | Result |
|---|---|
| Document absent | `DocumentNotFound` |
| target Version absent | `DocumentVersionNotFound` |
| stale expected revision | `Conflict` |
| another current Version exists | `Conflict` |
| same operation ID reused for different command | `Conflict` |
| distinct operation attempts already-published target | `Conflict` |
| target state is not publishable by v0 | `BusinessRule` |
| target belongs to another Document | `IntegrityViolation` |
| PRIMARY File missing/unavailable as authoritative object | `IntegrityViolation` |
| PostgreSQL dependency unavailable before commit | `RepositoryUnavailable` |
| commit result unknown | structured `CommitOutcomeUnknown` |
| unexpected adapter/programming defect | `Internal` |

Infrastructure-specific SQLx or filesystem errors are not exposed through the Application contract.

## 14. Expected implementation surface

### Domain

```text
crates/document-domain/src/document.rs
crates/document-domain/src/error.rs
```

### Application

```text
crates/document-application/src/command.rs
crates/document-application/src/error.rs
crates/document-application/src/events.rs
crates/document-application/src/ports.rs
crates/document-application/src/service.rs
crates/document-application/src/lib.rs
```

Expected concepts include:

```text
PublishOperationId
PublishDocumentCommand
PublishDocumentResult
PublishCandidate
PublishInitialVersionRecord
PublishRepositoryOutcome
```

### PostgreSQL adapter

```text
crates/document-repository-postgres/migrations/0002_document_publish_v0.sql
crates/document-repository-postgres/src/repository.rs
crates/document-repository-postgres/src/error.rs
crates/document-repository-postgres/src/mapping.rs
crates/document-repository-postgres/src/rows.rs
```

Implementation may split publish-specific repository code into focused modules rather than indefinitely growing `repository.rs`.

No production dependency direction is changed.

## 15. Test strategy

Implementation follows TDD and must preserve RED -> GREEN evidence for the new behavior.

### 15.1 Domain tests

At minimum:

- `WORKING + current None` publishes successfully;
- `published_at` is set;
- `current_version_id` becomes target;
- revision increments exactly once;
- cross-Document target is rejected;
- existing current Version is rejected;
- `WITHDRAWN` target is rejected;
- checked revision increment cannot overflow silently.

### 15.2 Application contract tests

At minimum:

- prior successful operation replay returns stored result before file preflight;
- same operation ID with different command returns Conflict;
- PRIMARY file missing prevents Repository publication;
- readable PRIMARY file permits Repository call;
- Repository `CommitOutcomeUnknown` retains operation/document/version identities in the Application error.

### 15.3 Real PostgreSQL transaction tests

Successful Publish proves:

```text
Version #1.lifecycle_state = PUBLISHED
Version #1.published_at != NULL
Document.current_version_id = Version #1
Document.revision: 0 -> 1
Domain Outbox delta = 1
Audit Outbox delta = 1
Publish Operation delta = 1
```

Additional tests must prove:

- idempotent same-operation replay does not change revision or event counts;
- operation-ID misuse conflicts;
- stale expected revision conflicts;
- existing different current Version conflicts;
- composite FK rejects cross-Document current assignment;
- transaction rollback leaves no partial business/event/audit/operation state.

### 15.4 Concurrency tests

Using real PostgreSQL:

```text
same Document
same expected revision
different publish_operation_id

=> exactly one success
=> exactly one Conflict
=> final revision = 1
=> Domain Outbox count = 1
=> Audit Outbox count = 1
=> Publish Operation count = 1
```

Also verify concurrent submission of the same operation ID and same command:

```text
both callers receive the same logical result
business mutation occurs once
revision increments once
Domain/Audit events are emitted once
```

### 15.5 Commit ambiguity regression

Use the existing failure-injection strategy or an equivalent deterministic seam to prove both possible outcomes:

- transaction committed but acknowledgement was lost -> retry replays stored result;
- transaction did not commit -> retry performs the one allowed Publish;

In both cases final authoritative state reflects one Publish only.

## 16. Definition of Done

Publish v0 is complete only when fresh evidence proves the following end state from an initial Create:

```text
Before Publish
DocumentVersion #1 = WORKING
Document.current_version_id = None
Document.revision = 0

After Publish
DocumentVersion #1 = PUBLISHED
DocumentVersion #1.published_at != None
Document.current_version_id = Version #1
Document.revision = 1
```

and exactly one successful-operation side-effect set exists:

```text
Domain Outbox = 1 Publish event
Audit Outbox = 1 Publish audit event
document_publish_operations = 1 result record
```

Fresh verification must include:

- Domain tests;
- Application contract tests;
- PostgreSQL migration/schema tests;
- PostgreSQL publication transaction tests;
- real concurrency tests;
- file-preflight regression;
- commit-ambiguity recovery regression;
- existing architecture/static/security/portability/container/Development Assurance gates.

No capability-complete claim may be made from stale or non-exact-head CI evidence.

## 17. Design freeze / change control

After written-spec approval, this Design becomes frozen for Publish v0.

Implementation-visible changes to any of the following require an explicit design amendment before they are implemented:

- supported lifecycle transitions;
- current-version replacement semantics;
- idempotency identity or retention semantics;
- OCC / locking semantics;
- file-integrity preflight semantics;
- PostgreSQL transaction boundary;
- mandatory Domain/Audit event semantics;
- database ownership constraint strategy;
- capability scope.

Implementation details that preserve these contracts may be refined in the Implementation Plan without reopening the design.

## 18. Written-spec review gate

The conversational design decisions represented here were approved by the user before this file was written.

However, the written artifact itself must be reviewed before its status changes to `APPROVED — design freeze active` and before an Implementation Plan is created.
