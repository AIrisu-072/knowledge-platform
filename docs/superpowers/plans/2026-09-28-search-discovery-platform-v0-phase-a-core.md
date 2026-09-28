# Search / Discovery Platform v0 — Phase A Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish pure Rust Search/Discovery domain and application contracts so all later retrieval, graph, source, and Agent integrations can be implemented without changing approved semantics.

**Architecture:** Create `search-core` for pure domain/value types and deterministic evaluators, and `search-application` for orchestration ports. Enforce dependency direction in architecture-lint before any infrastructure adapter exists.

**Tech Stack:** Rust 1.98.1 / edition 2024; existing workspace `serde`, `serde_json`, `uuid`, `time`, `sha2`, `thiserror`; no new production dependency.

**Spec:** `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`

## Global Constraints

- `search-core`: no SQLx, Axum, Tokio, Tantivy, filesystem/path APIs, network APIs, provider SDKs.
- `search-application`: no SQLx, Axum, Tantivy, filesystem/path APIs, direct `document-repository-postgres`.
- Use stable identifiers/newtypes; display names are never identity.
- Predicate evaluation is typed, side-effect-free, terminating, four-valued.
- TypedRelationInstance retains participant roles and provenance.
- No backend/product selection in Phase A.

## Review Focus

1. Missing fact must evaluate UNKNOWN, not FALSE.
2. Conflicting assertions must remain conflict, not latest-wins.
3. Same name/schema must not force logical identity.
4. N-ary participant roles must remain distinct and order-independent unless role semantics say otherwise.
5. Bindings must expose exact representation/version/digest without storing current authorization as permanent fact.

---

### Task A1: Search workspace boundaries

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/search-core/Cargo.toml`
- Create: `crates/search-core/src/lib.rs`
- Create: `crates/search-application/Cargo.toml`
- Create: `crates/search-application/src/lib.rs`
- Modify: `spec/architecture/dependency-rules.toml`
- Modify: `tools/architecture-lint/src/config.rs`
- Modify: `tools/architecture-lint/tests/policy.rs`

**Interfaces:**
- Produces crates named exactly `search-core` and `search-application`.
- `search-application` depends on `search-core`; `search-core` depends on no project infrastructure crate.

- [ ] **Step 1: Add failing architecture-lint fixtures**

Add tests:
- `search_core_sqlx_dependency_is_rejected`
- `search_core_tantivy_dependency_is_rejected`
- `search_core_filesystem_source_is_rejected`
- `search_application_sqlx_dependency_is_rejected`
- `search_application_tantivy_dependency_is_rejected`

Expected: fail because Search boundaries are not configured.

- [ ] **Step 2: Run RED**

Run:
`cargo test -p architecture-lint --test policy search_`

Expected: at least the new boundary tests fail.

- [ ] **Step 3: Add crate scaffolds and boundary rules**

Add workspace members and rules:
- search_core forbidden dependencies: `sqlx`, `axum`, `tokio`, `tantivy`
- search_core forbidden source patterns: `std::fs`, `std::path`, `tokio::fs`
- search_application forbidden dependencies: `sqlx`, `axum`, `tantivy`
- search_application forbidden source patterns: `std::fs`, `std::path`, `tokio::fs`

- [ ] **Step 4: Run GREEN**

Run:
- `cargo test -p architecture-lint --test policy search_`
- `cargo check -p search-core -p search-application`

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `build: establish search discovery crate boundaries`

---

### Task A2: IDs, Source, Resource, Usage and Discovery profiles

**Files:**
- Create: `crates/search-core/src/id.rs`
- Create: `crates/search-core/src/source.rs`
- Create: `crates/search-core/src/resource.rs`
- Create: `crates/search-core/src/usage.rs`
- Create: `crates/search-core/src/profile.rs`
- Create: `crates/search-core/src/temporal.rs`
- Modify: `crates/search-core/src/lib.rs`
- Test: `crates/search-core/tests/resource_contract.rs`

**Interfaces:**
- Produces newtypes: `SourceId`, `ResourceId`, `LogicalResourceId`, `RepresentationId`, `ResourceVersionId`, `UsageProfileId`, `DiscoveryEvaluationId`, `BindingId`, `NeedId`, `ClaimId`, `GapId`, `RelationId`, `AssertionId`, `ProjectionGenerationId`.
- Produces `DiscoverableSource`, `DiscoverableResource`, `ResourceIdentity`, `ResourceBody`, `UsageProfile`, `DiscoveryProfile`, `TemporalDiscoveryProfile`.

- [ ] **Step 1: Write failing domain contract tests**

Tests assert:
- Source discovery modes and enumeration semantics round-trip through serde.
- six Resource families remain distinct.
- one Resource may carry multiple UsageProfile IDs.
- `not_applicable_when` is separate from positive applicability.
- `KNOWN / UNKNOWN / NOT_APPLICABLE / CONFLICT` facet states are distinct.
- Source `NO_RETENTION` is not encoded as “unreadable”.

- [ ] **Step 2: Run RED**

Run: `cargo test -p search-core --test resource_contract`

Expected: compile failure on undefined types.

- [ ] **Step 3: Implement exact types**

Required enums:
- `ResourceKind::{Knowledge, Semantic, Capability, AgentSkill, Workflow, Policy}`
- `DiscoveryMode::{LocalDirectory, LocalContentSearch, RemoteEnumeration, RemoteQuery, DirectAddress, LiveOnly}`
- `EnumerationSemantics::{Complete, Partial, QueryOnly, None}`
- `RetentionMode::{PersistentResource, PersistentDiscoveryMetadata, CacheWithExpiry, SessionOnly, NoRetention}`
- `FacetState<T>::{Known(T), Unknown, NotApplicable, Conflict}`

Keep Source-native identifiers as separate optional string values; do not derive global identity from names.

- [ ] **Step 4: Run GREEN**

Run:
- `cargo test -p search-core --test resource_contract`
- `cargo clippy -p search-core --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit**

Commit message: `feat: define discoverable source and resource contracts`

---

### Task A3: Typed Predicate IR and four-valued evaluator

**Files:**
- Create: `crates/search-core/src/predicate.rs`
- Create: `crates/search-core/src/fact.rs`
- Modify: `crates/search-core/src/lib.rs`
- Test: `crates/search-core/tests/predicate_contract.rs`

**Interfaces:**
- `TypedValue::{Bool,String,Integer,Decimal,Date,DateTime,Duration,ConceptRef,ResourceRef,List,Set,Money,Quantity}`
- `PredicateExpr` supports AND/OR/NOT, EQ/NE/LT/LTE/GT/GTE, IN/CONTAINS/INTERSECTS/SUBSET, EXISTS/MISSING, SAME_CONCEPT/IS_A/DESCENDANT_OF.
- `TruthValue::{True,False,Unknown,Error}`
- `PredicateEvaluator::evaluate(&PredicateExpr, &FactSet, &dyn ConceptResolver) -> TruthValue`

- [ ] **Step 1: Write failing evaluator tests**

Must cover:
- missing required fact => UNKNOWN;
- wrong type => ERROR;
- false known comparison => FALSE;
- AND/OR preserve UNKNOWN correctly;
- MISSING distinguishes absent from known false;
- Money comparison never uses float;
- INFERRED fact remains inspectable by evidence policy;
- semantic operators use `ConceptResolver`, not string similarity.

- [ ] **Step 2: Run RED**

Run: `cargo test -p search-core --test predicate_contract`

Expected: compile fail.

- [ ] **Step 3: Implement IR/evaluator**

Use deterministic recursion with explicit maximum expression depth constant in core; arbitrary scripts/regex/network access are not supported.

- [ ] **Step 4: Run GREEN**

Run:
- `cargo test -p search-core --test predicate_contract`
- `cargo clippy -p search-core --all-targets -- -D warnings`

- [ ] **Step 5: Commit**

Commit message: `feat: add typed applicability predicate evaluator`

---

### Task A4: Assertion, authority, identity, observation and temporal semantics

**Files:**
- Create: `crates/search-core/src/assertion.rs`
- Create: `crates/search-core/src/authority.rs`
- Create: `crates/search-core/src/identity.rs`
- Create: `crates/search-core/src/observation.rs`
- Modify: `crates/search-core/src/temporal.rs`
- Test: `crates/search-core/tests/authority_identity_contract.rs`

**Interfaces:**
- `AssertionOrigin::{Authoritative,Declared,Curated,Derived,Extracted,Observed,Inferred}`
- `Assertion`
- `AuthorityConflict`
- `IdentityState::{Resolved,Provisional,Unresolved,Conflict}`
- `ResourceObservation`
- `Presence::{Present,Absent,Unknown}`
- `Reachability::{Reachable,Unreachable,Unknown}`
- `Coverage::{CompleteEnumeration,PartialEnumeration,QueryResult,DirectLookup}`
- `Freshness::{Fresh,Stale,Unknown}`
- `TemporalEvaluationContext`

- [ ] **Step 1: Write RED tests**

Assert:
- inferred assertion cannot replace authoritative assertion;
- equal-authority conflicting values produce conflict;
- same name/schema is not automatically resolved identity;
- query-result miss leaves Presence UNKNOWN;
- complete enumeration omission may represent absence only via explicit observation input;
- stale means expired current guarantee, not invalid historical evidence;
- same version ID + changed digest produces integrity conflict representation.

- [ ] **Step 2: Run RED**

Run: `cargo test -p search-core --test authority_identity_contract`

- [ ] **Step 3: Implement deterministic resolution helpers**

Create pure functions:
- `resolve_assertions(...)`
- `derive_effective_resource_state(...)`
- `evaluate_temporal_profile(...)`

Do not implement provider-specific precedence tables; consume an explicit `AuthorityPolicy`.

- [ ] **Step 4: Run GREEN**

Run focused test + strict Clippy.

- [ ] **Step 5: Commit**

Commit message: `feat: model discovery authority identity and observations`

---

### Task A5: Typed HyperEdge relation and traversal contracts

**Files:**
- Create: `crates/search-core/src/relation.rs`
- Create: `crates/search-core/src/graph.rs`
- Test: `crates/search-core/tests/hyperedge_contract.rs`

**Interfaces:**
- `RelationNamespace::{Discovery,Semantic,Evidence}`
- `RelationParticipant { role, resource_ref }`
- `TypedRelationInstance { relation_id, relation_type, participants, qualifiers, temporal_scope, authority, provenance, evidence_refs }`
- `RelationPathPattern`
- `GraphTraversalPlan`
- `GraphPathEvidence`

- [ ] **Step 1: Write RED tests**

Fixture contains:
- R1 = A company + Product B + Collateral X
- R2 = A company + Product C + Collateral Y

Assert no API can infer Product B + Collateral Y as one relation.
Assert participant order does not alter relation semantics while roles do.
Assert traversal constraints include namespace, relation type, from/to role, temporal/access/authority requirements, expansion budget.

- [ ] **Step 2: Run RED**

Run: `cargo test -p search-core --test hyperedge_contract`

- [ ] **Step 3: Implement relation/traversal value objects**

No binary canonical edge type. Any future shortcut type must contain `derived_from_relation_id`.

- [ ] **Step 4: Run GREEN**

Run focused test + strict Clippy.

- [ ] **Step 5: Commit**

Commit message: `feat: define typed hyperedge discovery graph contract`

---

### Task A6: Applicability, contrast, evidence, gaps and DiscoveryResult

**Files:**
- Create: `crates/search-core/src/intent.rs`
- Create: `crates/search-core/src/applicability.rs`
- Create: `crates/search-core/src/contrast.rs`
- Create: `crates/search-core/src/evidence.rs`
- Create: `crates/search-core/src/discovery.rs`
- Test: `crates/search-core/tests/discovery_contract.rs`

**Interfaces:**
- `IntentSignature` with fact origins.
- `ApplicabilityState::{Applicable,Excluded,Unresolved,Invalid}`
- `DiscriminatorImportance::{Hard,Soft}`
- `ContrastSet`
- `InformationGap`
- `QualifiedResource`
- `RejectedCandidate`
- `EvidenceRequirement`, `Claim`, `EvidenceRole`, `EvidenceSufficiency`
- `DiscoveryResult`

- [ ] **Step 1: Write RED tests**

Assert:
- hard mismatch excludes regardless of similarity/rank metadata;
- missing hard discriminator => UNRESOLVED + blocking gap;
- inferred-only hard fact may be rejected by minimum evidence rule;
- nonblocking unknown can survive in QualifiedResource;
- conflicting required claim => CONFLICTED, not SUFFICIENT;
- required claim absent => INSUFFICIENT/UNRESOLVED;
- rejected candidate keeps reason trace.

- [ ] **Step 2: Run RED**

Run: `cargo test -p search-core --test discovery_contract`

- [ ] **Step 3: Implement deterministic qualification helpers**

Functions:
- `evaluate_applicability(...)`
- `resolve_contrast(...)`
- `evaluate_evidence_sufficiency(...)`

Do not introduce ranking/fusion here.

- [ ] **Step 4: Run GREEN**

Run focused test + strict Clippy.

- [ ] **Step 5: Commit**

Commit message: `feat: add discovery qualification and evidence contracts`

---

### Task A7: Binding and application ports

**Files:**
- Create: `crates/search-core/src/binding.rs`
- Create: `crates/search-application/src/ports.rs`
- Create: `crates/search-application/src/source_registry.rs`
- Create: `crates/search-application/src/qualification.rs`
- Modify: `crates/search-application/src/lib.rs`
- Test: `crates/search-application/tests/port_contract.rs`
- Test: `crates/search-application/tests/binding_contract.rs`

**Interfaces:**
- `BindingMode::{SnapshotPinned,RemoteVersionPinned,SessionSnapshot,LiveReference}`
- `LogicalResourceBinding`
- `RepresentationBinding`
- Port traits returning `BoxFuture<'a, Result<T, SearchError>>` where asynchronous I/O is required:
  - `SourceRegistryPort`
  - `DirectoryRetrieverPort`
  - `StructuredRetrieverPort`
  - `LexicalRetrieverPort`
  - `VectorRetrieverPort`
  - `HyperGraphRetrieverPort`
  - `ProbePort`
  - `CurrentAccessEvaluatorPort`
  - `MaterializerPort`
- Keep ports provider/backend neutral.

- [ ] **Step 1: Write RED tests**

Use fakes to assert:
- registry can return Source capabilities without Source body;
- application accepts HyperGraph candidate without knowing graph backend;
- existing binding remains unchanged when registry later returns newer representation;
- LiveReference requires revalidation marker;
- ports expose no SQLx/Tantivy/provider types.

- [ ] **Step 2: Run RED**

Run:
- `cargo test -p search-application --test port_contract`
- `cargo test -p search-application --test binding_contract`

- [ ] **Step 3: Implement ports and in-memory registry fake/reference implementation**

The in-memory registry is for contract/testing/bootstrap, not a persistence decision.

- [ ] **Step 4: Run GREEN**

Run:
- focused tests
- `cargo clippy -p search-application --all-targets -- -D warnings`
- `mise run arch:check`

- [ ] **Step 5: Commit**

Commit message: `feat: establish search application ports and bindings`

---

### Task A8: Phase A verification and evidence

**Files:**
- Modify: `docs/superpowers/execution/search-discovery-platform-v0-status.md`
- Create: `docs/superpowers/execution/search-discovery-platform-v0-phase-a.md`

- [ ] **Step 1: Run focused Search tests**

Run:
`cargo test -p search-core -p search-application`

Expected: all PASS.

- [ ] **Step 2: Run repository fast gate**

Run: `mise run verify:fast`

Expected: PASS.

- [ ] **Step 3: Record exact head and tests**

Record:
- task commits
- test counts
- architecture-lint result
- known deferred backend decisions
- next exact action = Phase B PoC RED

- [ ] **Step 4: Push Phase A branch and create/update Draft PR A**

Base is the approved planning branch/PR, not an inferred branch.

- [ ] **Step 5: Check exact-head hosted gates**

Do not declare Phase A complete until the required hosted gates for that exact head are GREEN.

- [ ] **Step 6: Commit evidence update**

Commit message: `docs: qualify search discovery phase a core`
