# Search / Discovery Platform v0 Production Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the approved Search / Discovery Platform v0 inside `AIrisu-072/knowledge-platform` as a typed, source-federated, evidence-driven discovery subsystem with first-class HyperEdge graph semantics, without prematurely fixing deferred physical backends.

**Architecture:** Build pure Search domain/application contracts first, then qualify retrieval/index candidates in isolated PoCs, promote only approved backends, assemble adaptive Discovery, and finally connect Document Platform as the first real Source. Search remains logically separate from Document authority and Execution Control Plane; all indexes/projections are derived and rebuildable.

**Tech Stack:** Rust 1.98.1 / edition 2024, existing workspace libraries, Tantivy only after its selected baseline is exercised, Lindera only after PoC qualification, PostgreSQL/SQLx or other Graph backend only after explicit selection evidence, existing mise/CI/architecture-lint/assurance tooling.

**Spec:** `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`

**Design Approval:** `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design-approval.md`

## Global Constraints

- Repository and GitHub current state are authoritative; never reconstruct implementation state from chat history.
- Production code starts only after this plan is explicitly approved.
- Keep `search-core` free of SQLx, Axum, Tokio, Tantivy, filesystem APIs, Graph DB SDKs, LLM SDKs, and provider-specific types.
- Keep `search-application` free of SQLx, Axum, Tantivy, Graph DB SDKs, and direct Document repository implementation dependencies.
- Document / CRM / business systems remain authoritative Sources. Search state is derived/rebuildable.
- Similarity is candidate generation only; hard access/temporal/applicability/authority/contrast semantics outrank ranking.
- Missing facts are UNKNOWN, never silently FALSE.
- Canonical graph semantics are Typed N-ary Relation / HyperEdge. Do not flatten to lossy binary canonical edges.
- Graph RAG is internal to Search Platform; do not create a separate public Graph-RAG product/API.
- Retention and materialization are orthogonal. NO_RETENTION data must not leak into persistent projection, logs, audit, or evaluation artifacts.
- Discovery is dynamic; Session Binding is stable and never silently rebound.
- Search does not own Task DAG, tool execution, credentials, current authorization, retry ledger, compensation, or human approval.
- Optimize by avoiding unnecessary work, never by weakening correctness/evidence/retention contracts.
- Use TDD for every production behavior. Each task ends with focused verification and a commit.
- New production dependencies marked POC REQUIRED in selection specs cannot be promoted before qualification and explicit selection recording.
- Update `docs/superpowers/execution/search-discovery-platform-v0-status.md` before session switch/context exhaustion.
- Do not modify the repository-wide `active.md` while another parallel capability owns it; use the Search-specific status file.

## Review Focus

1. **Missing / conflicting facts:** UNKNOWN and CONFLICT must survive filtering and produce InformationGap instead of false exclusion.
2. **N-ary relation traps:** shared participants must never create a false composite relation/path.
3. **Remote/partial Source semantics:** query miss or provider outage must not become deletion/absence.
4. **Access/retention:** candidate existence and temporary remote content must not leak before authorization or beyond retention.
5. **Generation/binding drift:** index rebuilds, source updates, or provider version changes must not silently mutate existing Session Bindings.

---

## Execution Phases

### Phase A — Core Contracts and Deterministic Qualification

Plan: `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-a-core.md`

Deliverables:

- `search-core` and `search-application` workspace crates
- architecture-lint boundaries
- Source / Resource / Usage / Lens / Intent models
- Typed Predicate IR + four-valued evaluation
- Assertion / Authority / Logical Identity / Observation / Temporal contracts
- Typed HyperEdge relation contract
- Evidence / Gap / QualifiedResource / Binding contracts
- Search application ports with no infrastructure dependency

Gate A:

- focused tests GREEN
- `mise run verify:fast` GREEN
- exact-head hosted CI / Sandbox / PoC GREEN if workflows run for the branch
- no production retrieval/index backend added

### Phase B — Retrieval / HyperGraph PoC and Selection Evidence

Plan: `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-b-poc.md`

Deliverables:

- isolated `experiments/search-discovery-poc`
- Japanese lexical baseline / Tantivy + tokenizer comparison
- HyperEdge incidence/traversal correctness harness
- false-composite relation trap suite
- graph/storage candidate measurements without changing Domain contracts
- selection report and explicit backend/dependency decision record

**Selection Gate S1:** Do not promote a POC REQUIRED tokenizer, Graph backend, Vector engine, embedding runtime, reranker, or fusion library until evidence is reviewed and the selection document records the decision. If a choice remains material and not predetermined by the approved spec, stop for requester approval.

### Phase C — Projection, Retrieval, Evidence-Driven Discovery

Plan: `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-c-runtime.md`

Deliverables:

- selected projection/index adapters
- source-local directory / structured / lexical / graph projections
- projection generation manifest + atomic publication semantics
- adaptive candidate retrieval / logical grouping / qualification
- GraphTraversalPlan / bounded HyperEdge expansion
- progressive probe/materialization ports
- Evidence Requirement / Sufficiency loop
- DiscoveryResult + trace
- Session Working Set / Binding / Context manifest contracts

Gate C:

- deterministic/full-vs-incremental projection tests
- applicability / graph / evidence fixtures GREEN
- no unsupported backend promotion
- `mise run verify` GREEN

### Phase D — Document Source Vertical Slice and Qualification

Plan: `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-d-integration.md`

Deliverables:

- Document Platform Source Adapter boundary
- outbox-driven invalidation/indexing trigger without distributed transaction
- authorized/rebuildable Document discovery snapshot
- Document → Directory / title/metadata Lexical / Structured / Temporal / HyperGraph vertical slice
- current/past/T10/withdrawal/access-policy regressions
- evaluation harness and failure attribution
- Search/Discovery v0 acceptance record

Gate D:

- Document transaction remains independent of Search success
- access revocation removes visibility without leaking existence
- source miss / stale projection / rebuild / replay are covered
- SOURCE_KNOWLEDGE_ABSENT is separated from retrieval miss
- standard hosted gates GREEN
- final independent review before merge

---

## Branch / PR Strategy

After this plan is approved:

1. keep design/plan PR #20 as the approved planning base unless repository state has changed materially;
2. create `feat/search-discovery-platform-v0-a` from the approved planning head;
3. Phase A PR A targets the planning branch/PR;
4. Phase B branch/PR stacks on A;
5. Phase C stacks on B;
6. Phase D stacks on C;
7. do not merge any stacked PR without explicit requester instruction;
8. if parallel Document work changes `main`, re-read the new main and reconcile only actual conflicts; never silently rebase away approved semantics.

## Verification Policy

Per task:

- focused unit/contract test
- `cargo fmt --all -- --check`
- strict Clippy for touched crates

Per phase:

- `mise run verify:fast`
- `mise run verify` before phase completion
- exact-head hosted CI, DSI Sandbox, and DSI PoC when those repository gates are triggered/required
- isolated PoC `cargo deny` / license/source gate before any dependency promotion

Final:

- `mise run verify`
- Search/Discovery evaluation harness
- whole-stack vertical slice
- independent code review
- acceptance record

## Definition of Done

Search / Discovery Platform v0 is complete only when:

- typed Source/Resource/Assertion/HyperEdge contracts exist and are tested;
- at least Document Platform works as a real Source through the common adapter boundary;
- source-local lexical/structured/temporal/graph retrieval is operational using qualified backends for fields actually supplied by the Source;
- Document full-body lexical extraction is not silently implemented through DSI; if required, a separately approved Search Extraction capability must provide it;
- Applicability/Contrast/UNKNOWN/Authority/Temporal semantics are enforced before soft ranking;
- Evidence Sufficiency can stop or return UNRESOLVED without fabricated certainty;
- Session Binding and generation drift are tested;
- access/retention/prompt-injection boundaries are covered;
- evaluation reports stage-level failure attribution;
- all required repository gates are GREEN;
- no approved Design invariant is weakened by optimization.


## Explicit dependency: Search Extraction

Current Document Semantic Inspection v0 returns semantic fingerprints, capability/evidence/provenance and dependency information; it is not a full Search Extraction payload.

Therefore this plan MUST NOT:
- reinterpret DSI fingerprint/evidence as full document text;
- add format-specific body extraction inside Search core/runtime as an incidental implementation detail;
- claim Document full-text search from title/metadata-only indexing.

Phase D may index Document title, metadata, lifecycle, folder/access projection, DSI capability/evidence-derived structured fields and relations. Full body/section/table/sheet/slide Search Extraction is a separate capability requiring its own design approval before implementation.
