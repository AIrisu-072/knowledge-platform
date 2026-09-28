# Search / Discovery Platform v0 — Phase C Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement source-local projections and the adaptive evidence-driven Discovery runtime on top of Phase A contracts and Phase B-qualified adapters.

**Architecture:** Projection compilation is separated from retrieval. Directory/structured/temporal/access and a reference in-process HyperEdge graph are rebuildable derived state; lexical uses the Phase B-qualified Tantivy/analyzer path. Discovery orchestrates retrieval, qualification, probe/materialization ports, and evidence sufficiency without owning execution.

**Tech Stack:** Phase A crates; qualified Tantivy/analyzer dependencies only; standard-library collections for reference HyperEdge projection unless/until an explicitly selected durable Graph backend is promoted.

**Spec:** `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`

## Global Constraints

- Phase B evidence/selection record must exist before production retrieval dependency promotion.
- Full document body Search Extraction is not part of this phase.
- Every Projection is derived/rebuildable and tied to a generation manifest.
- HyperEdge semantics must remain n-ary; no lossy binary canonical graph.
- Raw score across heterogeneous retrievers/sources is not directly comparable.
- Evidence Sufficiency, not fixed Top-K, is the Discovery completion condition.

## Review Focus

1. Full vs incremental projection equivalence.
2. Index generation switch without mutating in-flight evaluation.
3. Candidate dedup/grouping across representations.
4. UNKNOWN resolution without infinite retries.
5. Graph expansion bounded by relation/role/access/temporal constraints.

---

### Task C1: Projection generation contracts and compiler

**Files:**
- Create: `crates/search-core/src/projection.rs`
- Create: `crates/search-application/src/projection.rs`
- Test: `crates/search-application/tests/projection_contract.rs`

**Interfaces:**
- `ProjectionFamily::{Directory,Structured,Lexical,Vector,Temporal,HyperGraph,Access}`
- `ProjectionGenerationManifest`
- `ProjectionInput`
- `ProjectionCompiler::compile_resource(...)`
- `ProjectionGenerationStore` port
- generation publish states: building / validated / current / failed as application-level state, not business truth.

- [ ] Write RED tests for versioned lens/schema/generation identity.
- [ ] Assert failed generation never replaces current.
- [ ] Implement compiler for Directory/Structured/Temporal/Access plus relation output.
- [ ] Run focused GREEN + strict Clippy.
- [ ] Commit `feat: compile discovery projections by generation`.

---

### Task C2: In-process source-local projection store

**Files:**
- Create: `crates/search-projection-memory/Cargo.toml`
- Create: `crates/search-projection-memory/src/lib.rs`
- Create: `crates/search-projection-memory/src/store.rs`
- Create: `crates/search-projection-memory/src/retrieval.rs`
- Test: `crates/search-projection-memory/tests/projection_store.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- implements `ProjectionGenerationStore`, `DirectoryRetrieverPort`, and `StructuredRetrieverPort`
- stores immutable generation segments for Directory / Structured / Temporal / Access metadata
- current generation switch is explicit and atomic within the adapter
- this adapter is rebuildable/in-process and is not a durable-source-of-truth claim.

- [ ] RED: failed/unvalidated generation cannot become current.
- [ ] RED: a Discovery evaluation pinned to generation N continues reading N after N+1 is published.
- [ ] RED: structured hard filter returns UNKNOWN facet separately from false mismatch.
- [ ] Implement immutable generation maps and deterministic retrieval.
- [ ] Run focused GREEN + strict Clippy.
- [ ] Commit `feat: add source local projection store`.

---

### Task C3: Qualified lexical adapter

**Files:**
- Create: `crates/search-tantivy/Cargo.toml`
- Create: `crates/search-tantivy/src/lib.rs`
- Create: `crates/search-tantivy/src/schema.rs`
- Create: `crates/search-tantivy/src/index.rs`
- Create: `crates/search-tantivy/src/query.rs`
- Test: `crates/search-tantivy/tests/lexical_contract.rs`
- Modify: root `Cargo.toml`
- Modify: `spec/architecture/dependency-rules.toml`

**Interfaces:**
- implements `LexicalRetrieverPort`
- source-local index generation selected by `ProjectionGenerationId`
- input fields: canonical_name, title, aliases, high_signal_text, body only when Source legitimately supplies body
- returns backend-neutral `FederatedCandidate`/lexical hit DTO, not Tantivy score types.

- [ ] RED: field-aware ranking/result/locator/generation tests.
- [ ] Promote only Phase B-qualified tokenizer dependency/config.
- [ ] GREEN: deterministic index build and query.
- [ ] Add architecture rule preventing Tantivy leakage into core/application.
- [ ] Commit `feat: add source-local tantivy lexical adapter`.

---

### Task C4: In-process Typed HyperEdge projection/retriever

**Files:**
- Create: `crates/search-graph-memory/Cargo.toml`
- Create: `crates/search-graph-memory/src/lib.rs`
- Create: `crates/search-graph-memory/src/index.rs`
- Create: `crates/search-graph-memory/src/traversal.rs`
- Test: `crates/search-graph-memory/tests/hypergraph_contract.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- implements `HyperGraphRetrieverPort`
- incidence indexes from Phase B semantic oracle
- accepts `GraphTraversalPlan`
- returns `GraphPathEvidence` + candidates
- explicitly rebuildable/in-process; not a claim of durable Graph backend selection.

- [ ] Port Phase B false-composite fixtures as production contract tests.
- [ ] RED on missing adapter.
- [ ] Implement bounded incidence traversal.
- [ ] Assert access/temporal/authority constraints are applied before expansion where data is available.
- [ ] Commit `feat: add typed hyperedge reference retriever`.

---

### Task C5: Candidate federation and logical grouping

**Files:**
- Create: `crates/search-application/src/candidate.rs`
- Create: `crates/search-application/src/federation.rs`
- Test: `crates/search-application/tests/federation_contract.rs`

**Interfaces:**
- `CandidateIdentityClass::{DurableResource,RemoteStableReference,EphemeralCandidate}`
- `FederatedCandidate`
- `CandidateFederator::merge(...)`
- logical grouping preserves representation hits and per-retriever trace; no cross-source raw-score arithmetic.

- [ ] RED duplicate/logical grouping tests.
- [ ] Assert same logical resource from MCP/REST-like representations groups once while keeping representations.
- [ ] Implement deterministic grouping.
- [ ] GREEN + commit `feat: federate discovery candidates`.

---

### Task C6: Source routing and retrieval profiles

**Files:**
- Create: `crates/search-application/src/routing.rs`
- Create: `crates/search-application/src/retrieval.rs`
- Test: `crates/search-application/tests/routing_contract.rs`

**Interfaces:**
- `SourceRole::{Required,Preferred,Expansion}`
- `SourceRoutePlan`
- `RetrieverProfile::{Identity,Capability,Knowledge,EvidenceInvestigation,Exploratory}`
- `RetrievalAction`
- `RetrieverCursorState`

- [ ] RED: Required source cannot be silently skipped.
- [ ] RED: query-only Source miss cannot become absence.
- [ ] Implement routing from Source Registry capabilities + Need requirements.
- [ ] Implement staged action list without executing all retrievers.
- [ ] Commit `feat: plan federated discovery routes`.

---

### Task C7: Progressive materialization and probe orchestration

**Files:**
- Create: `crates/search-core/src/materialization.rs`
- Create: `crates/search-application/src/materialization.rs`
- Test: `crates/search-application/tests/materialization_contract.rs`

**Interfaces:**
- `MaterializationState::{ReferenceOnly,Metadata,Probed,Fragment,FullContent}`
- `MaterializationPolicy::{InlineFull,DiscriminativeFirst,TargetedFragment,ReferenceOnly}`
- `ProbeExecutionLocation::{Local,Provider,None}`
- `ProbeOutcome::{Found,NotFoundByProbe,Unsupported,Failed}`

- [ ] RED: probe no-hit is not false fact.
- [ ] RED: NO_RETENTION FullContent is allowed in-memory but cannot be persisted by Session Working Store.
- [ ] Implement state transitions and policy evaluator.
- [ ] Commit `feat: add progressive discovery materialization`.

---

### Task C8: Evidence-driven DiscoveryService

**Files:**
- Create: `crates/search-application/src/discovery_service.rs`
- Create: `crates/search-application/src/action_selection.rs`
- Test: `crates/search-application/tests/discovery_loop.rs`

**Interfaces:**
- `DiscoveryService::discover(request: DiscoveryRequest) -> Result<DiscoveryResult, SearchError>`
- action priority: blocking requirement → authority/freshness → expected gap resolution → critical need impact → cost
- no-progress key includes gap + known-state digest + attempted action.

- [ ] RED: structured match sufficient => vector/remote ports not called.
- [ ] RED: UNKNOWN hard discriminator => targeted probe, not exclusion.
- [ ] RED: sufficient evidence cancels/not-starts remaining actions.
- [ ] RED: repeated no-progress terminates UNRESOLVED.
- [ ] Implement orchestrator over ports.
- [ ] Commit `feat: execute evidence-driven discovery loop`.

---

### Task C9: Session Working Set, Binding, Context manifest

**Files:**
- Create: `crates/search-application/src/session.rs`
- Create: `crates/search-application/src/context.rs`
- Test: `crates/search-application/tests/session_context.rs`

**Interfaces:**
- `SessionWorkingSet`
- `TaskContextManifest`
- `ContextSegment` with type/trust/provenance/digest
- no credential fields
- stable segment ordering

- [ ] RED: generation changes do not silently mutate binding.
- [ ] RED: remote content marked untrusted.
- [ ] RED: only task-selected tool schemas enter manifest.
- [ ] Implement task-scoped context assembly.
- [ ] Commit `feat: compile task scoped discovery context`.

---

### Task C10: Phase C integration / verification

**Files:**
- Create: `docs/superpowers/execution/search-discovery-platform-v0-phase-c.md`
- Modify: Search status

- [ ] Add synthetic vertical test: Source Registry → projections → lexical/graph candidates → applicability → evidence → DiscoveryResult.
- [ ] Run `cargo test -p search-core -p search-application -p search-tantivy -p search-graph-memory`.
- [ ] Run `mise run verify`.
- [ ] Record exact head and hosted gates.
- [ ] Commit `docs: qualify search discovery runtime`.
