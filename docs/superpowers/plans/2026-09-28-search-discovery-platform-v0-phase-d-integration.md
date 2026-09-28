# Search / Discovery Platform v0 — Phase D Document Source / Qualification Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect the real Document Platform as the first authoritative Source, prove eventual/rebuildable indexing and authorization boundaries, and produce Search / Discovery v0 acceptance evidence.

**Architecture:** A dedicated infrastructure adapter reads Document search snapshots and domain outbox signals without adding Search work to Document transactions. It indexes only data legitimately available from existing Document/DSI contracts: title/metadata/lifecycle/folder/access/DSI evidence and relations. Full document body Search Extraction remains a separate, unimplemented capability until separately designed.

**Tech Stack:** Search crates from Phases A–C; existing Document crates; PostgreSQL/SQLx only in the infrastructure adapter if needed; no DSI semantic contract changes.

**Spec:** `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`

## Global Constraints

- Document Platform remains authoritative.
- Search failure never rolls back a committed Document transaction.
- Do not parse document bodies in this adapter.
- Do not treat DSI fingerprint/evidence as full text.
- Current access must filter candidate visibility; Search projection is not final execution authorization.
- T10/current/withdrawn/history semantics must match Document contracts.
- Outbox processing is at-least-once/idempotent.

## Review Focus

1. Publish/update/withdraw/T10 races with indexing.
2. Access policy revocation before/after projection update.
3. Duplicate/reordered outbox delivery.
4. Index rebuild from authoritative snapshot after lost events.
5. DSI evidence available vs missing/invalid without fabricating body content.

---

### Task D1: Document Source adapter contract

**Files:**
- Create: `crates/search-source-document/Cargo.toml`
- Create: `crates/search-source-document/src/lib.rs`
- Create: `crates/search-source-document/src/model.rs`
- Create: `crates/search-source-document/src/translate.rs`
- Test: `crates/search-source-document/tests/translation_contract.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- `DocumentSourceSnapshot` contains only explicit source fields: document/version IDs, title, metadata, lifecycle/current relation, folder, effective timestamps if present, access projection input, DSI capability/evidence refs.
- `DocumentSourceTranslator::translate(snapshot) -> SourceProjectionInput`
- no raw document body field unless a future Search Extraction contract supplies it.

- [ ] RED translation tests for current published, historical published, withdrawn, T10-ended, working-authoring visibility classes.
- [ ] Implement translation.
- [ ] Assert title may feed lexical projection but no claim of full-body content.
- [ ] Commit `feat: add document discovery source adapter`.

---

### Task D2: PostgreSQL read-side snapshot adapter

**Files:**
- Create: `crates/search-source-document/src/postgres.rs`
- Test: `crates/search-source-document/tests/postgres_snapshot.rs`
- Modify: crate Cargo.toml with SQLx as infrastructure dependency

**Interfaces:**
- `DocumentSnapshotReader::load_document_version(...)`
- `DocumentSnapshotReader::enumerate_live(...)`
- `DocumentSnapshotReader::enumerate_historical(...)`
- read-side queries may use Document schema but must not mutate it.

- [ ] RED against disposable PostgreSQL fixture using existing migrations.
- [ ] Assert T10 current-null and withdrawn/history semantics.
- [ ] Assert access-policy source data is sufficient for Search Access Projection without leaking ACL body.
- [ ] Implement read-only snapshot queries.
- [ ] Commit `feat: read document discovery snapshots`.

---

### Task D3: Outbox-triggered idempotent indexing service

**Files:**
- Create: `crates/search-source-document/src/outbox.rs`
- Create: `crates/search-application/src/indexing_service.rs`
- Test: `crates/search-source-document/tests/outbox_indexing.rs`

**Interfaces:**
- recognizes relevant existing Domain event types, including create/version/publish/withdraw/publication-end/metadata/move/access-policy changes.
- event is a trigger/invalidation signal, not the sole source of truth.
- handler re-reads authoritative snapshot before publishing new Search generation.

- [ ] RED duplicate event/reordered event tests.
- [ ] RED lost event followed by full rebuild test.
- [ ] Implement idempotent trigger handling keyed by event ID + current source snapshot.
- [ ] Assert Search failure leaves Document DB transaction/state untouched.
- [ ] Commit `feat: index document source from outbox triggers`.

---

### Task D4: Document-derived HyperEdge relations

**Files:**
- Create: `crates/search-source-document/src/relations.rs`
- Test: `crates/search-source-document/tests/relation_projection.rs`

**Interfaces:**
- relation candidates may include document/version/current/supersedes/folder/governed-by/access/evidence references that are explicitly supported by source data.
- DSI external dependency/evidence relations retain provenance and origin.
- no LLM-inferred authoritative relation in this task.

- [ ] RED participant/provenance tests.
- [ ] Implement deterministic relations only.
- [ ] Assert no relation survives if its source evidence is unavailable without correct UNKNOWN/absence semantics.
- [ ] Commit `feat: project document hyperedge relations`.

---

### Task D5: Access and generation race tests

**Files:**
- Create: `crates/search-source-document/tests/access_visibility.rs`
- Create: `crates/search-source-document/tests/generation_races.rs`

- [ ] Test access revoked after old Search generation exists: old projection must not authorize visibility when current access evaluator denies.
- [ ] Test generation switch during Discovery evaluation: evaluation stays trace-bound to its generation or restarts explicitly.
- [ ] Test publish/withdraw/T10 between trigger and snapshot read: latest authoritative snapshot wins.
- [ ] Test source outage does not delete Resource.
- [ ] Commit `test: harden document discovery consistency`.

---

### Task D6: Evaluation harness

**Files:**
- Create: `crates/search-evaluation/Cargo.toml`
- Create: `crates/search-evaluation/src/lib.rs`
- Create: `crates/search-evaluation/src/scenario.rs`
- Create: `crates/search-evaluation/src/report.rs`
- Create: `crates/search-evaluation/tests/scenarios.rs`
- Modify: root `Cargo.toml`
- Modify: `mise.toml`

**Interfaces:**
- `EvaluationScenario`
- failure classes include `SourceKnowledgeAbsent`, `SourceRoutingMiss`, `RetrievalMiss`, `GraphPathMiss`, `ApplicabilityError`, `EvidenceLocatorError`.
- report contains stage metrics without source/customer content.

- [ ] Add confusable/temporal/hyperedge/remote/evidence/security/coverage scenarios.
- [ ] Assert false-composite and hard false-accept are reported separately.
- [ ] Assert source-knowledge-absent is not scored as retriever miss.
- [ ] Add `mise run eval:search`.
- [ ] Commit `feat: add search discovery evaluation harness`.

---

### Task D7: Final vertical slice

**Files:**
- Create: `crates/search-source-document/tests/vertical_slice.rs`

Scenario:
1. authoritative Document current published snapshot exists;
2. index source-local title/metadata/structured/temporal/access/relation projections;
3. Need resolves Source route;
4. lexical/structured/graph produce candidate;
5. applicability/temporal/access qualify;
6. evidence sufficiency returns QualifiedResource;
7. access revoked and next query no longer exposes Resource;
8. T10 removes live visibility but historical authorized route may still resolve history;
9. full-body query that requires unavailable Search Extraction returns explicit gap/unsupported coverage rather than fabricated match.

- [ ] Write RED vertical test.
- [ ] Implement only missing integration glue.
- [ ] Run GREEN.
- [ ] Commit `test: prove document discovery vertical slice`.

---

### Task D8: Final qualification / acceptance

**Files:**
- Create: `docs/superpowers/execution/search-discovery-platform-v0-acceptance.md`
- Modify: `docs/superpowers/execution/search-discovery-platform-v0-status.md`

- [ ] Run `mise run eval:search`.
- [ ] Run `mise run verify`.
- [ ] Run/inspect exact-head hosted CI, DSI Sandbox, DSI PoC.
- [ ] Perform whole-branch independent review.
- [ ] Record known deferred capabilities: full Search Extraction, vector/embedding if still unselected, dedicated graph backend if still deferred, remote provider adapters, public transport/API.
- [ ] Do not claim those deferred capabilities as implemented.
- [ ] Commit `docs: qualify search discovery platform v0`.
