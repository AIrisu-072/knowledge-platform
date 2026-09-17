# Document Publish v0 — Design Approval

- Capability: `Document Publish v0`
- Design Spec: `docs/superpowers/specs/2026-09-17-document-publish-v0-design.md`
- Design branch: `design/document-publish-v0`
- Design PR: `#5`
- Repository baseline: `main@2ead1e21222c3b122704b6e0ce31f3f902b93659`
- Approval date: **2026-09-17**
- Approval status: **APPROVED**

## Approval record

The user reviewed the written Design Spec and explicitly approved it on 2026-09-17.

The following design areas are frozen for implementation unless the user explicitly approves a design amendment:

- initial Publish only; no Version #2+, Withdraw, approval workflow, scheduled publication, or current-version replacement;
- caller-generated Application-layer UUIDv7 `PublishOperationId`;
- permanent `document_publish_operations` records for Publish-specific idempotency;
- same-operation replay semantics and distinct-operation Conflict semantics;
- `expected_document_revision` OCC plus a short PostgreSQL row lock for current-version transition;
- PRIMARY final-object preflight before a new Publish;
- composite FK for same-Document current-version ownership;
- Domain + atomic Repository enforcement that the new current Version is `PUBLISHED`;
- atomic authoritative state + Domain Outbox + mandatory Audit Outbox + publish-operation result;
- conservative `CommitOutcomeUnknown` recovery by retrying the exact same command and operation ID;
- real PostgreSQL concurrency, rollback, and ambiguity regressions;
- Publish v0 explicit out-of-scope list in the Design Spec.

Implementation-detail refinements are allowed only when they preserve these contracts and the higher-priority normative SSOT.
