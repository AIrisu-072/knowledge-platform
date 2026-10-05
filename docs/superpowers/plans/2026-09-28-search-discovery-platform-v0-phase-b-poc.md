# Search / Discovery Platform v0 — Phase B Retrieval / HyperGraph PoC Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Qualify the retrieval/index candidates required for Search / Discovery v0 without promoting unproven dependencies into production crates.

**Architecture:** Keep all experimental code under `experiments/search-discovery-poc`. Exercise Japanese lexical retrieval, HyperEdge incidence/traversal semantics, and backend feasibility using synthetic/public fixtures. Record evidence first; production adapters are Phase C.

**Tech Stack:** Isolated Cargo project; Tantivy 0.26.x selected baseline; Lindera/lindera-tantivy only as PoC candidates; existing SQLx/testcontainers may be used in PoC to compare PostgreSQL incidence storage; pure-Rust in-memory incidence is the reference correctness implementation.

**Spec:** `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`

## Global Constraints

- PoC is not a workspace production dependency.
- No production crate promotion until the selection record is updated.
- Use only synthetic/public fixtures; no client data.
- HyperEdge participant roles and false-composite prevention are correctness gates, not performance trade-offs.
- Do not select Vector/Embedding/Reranker unless lexical/graph evidence demonstrates a need.
- No exact production SLO is frozen here; report measured distributions and correctness.

## Review Focus

1. Japanese compound/business terms that tokenize differently across dictionaries.
2. Resource aliases/concepts that lexical-only may miss.
3. Shared HyperEdge participants that can create false paths in binary flattening.
4. High-degree nodes and bounded traversal.
5. Query-only/partial Source semantics in synthetic federation fixtures.

---

### Task B1: Create isolated Search Discovery PoC harness

**Files:**
- Create: `experiments/search-discovery-poc/Cargo.toml`
- Create: `experiments/search-discovery-poc/deny.toml`
- Create: `experiments/search-discovery-poc/src/lib.rs`
- Create: `experiments/search-discovery-poc/src/report.rs`
- Create: `experiments/search-discovery-poc/src/bin/search-discovery-poc.rs`
- Create: `experiments/search-discovery-poc/tests/harness.rs`
- Modify: `mise.toml`

**Interfaces:**
- CLI: `search-discovery-poc verify` and `search-discovery-poc report --format json`.
- Report fields: fixture_set, candidate_versions, lexical metrics, graph correctness metrics, graph traversal measurements, dependency/license gate state.

- [ ] **Step 1: Write RED harness test**

Assert `verify` subcommand and deterministic JSON report schema exist.

- [ ] **Step 2: Run RED**

Run: `cargo test --manifest-path experiments/search-discovery-poc/Cargo.toml`

Expected: missing harness APIs/files.

- [ ] **Step 3: Implement minimal harness and mise tasks**

Add:
- `poc:search:test`
- `poc:search:deny`
- `poc:search:verify`

- [ ] **Step 4: Run GREEN**

Run: `mise run poc:search:verify`

- [ ] **Step 5: Commit**

Commit message: `test: add search discovery qualification harness`

---

### Task B2: Japanese lexical baseline

**Files:**
- Create: `experiments/search-discovery-poc/fixtures/lexical/resources.json`
- Create: `experiments/search-discovery-poc/fixtures/lexical/queries.json`
- Create: `experiments/search-discovery-poc/src/lexical.rs`
- Create: `experiments/search-discovery-poc/tests/lexical.rs`

**Interfaces:**
- `LexicalCase { query, expected_resource_ids, forbidden_resource_ids }`
- `LexicalMeasurement { analyzer_id, recall_at_10, mrr, ndcg_at_10, index_bytes, build_ms, query_p50_ms, query_p95_ms }`

- [ ] **Step 1: Add corpus before implementation**

Synthetic/public Japanese cases must include:
- 法人/個人
- 融資/預金
- 住所変更
- 事業性融資
- 規程/手順/マニュアル
- katakana/ASCII/date/number mixtures
- aliases and confusable resources

No institution/customer names.

- [ ] **Step 2: RED baseline test**

Require the expected resource to be recoverable in top-10 for mandatory exact/alias cases and forbidden hard-confusable resource not to be treated as a qualification result merely because it ranks highly.

- [ ] **Step 3: Implement analyzers**

Compare:
- Tantivy baseline tokenizer/analyzer
- Lindera candidates permitted by its feature/dictionary packaging

Do not add Lindera to workspace dependencies.

- [ ] **Step 4: Measure and report**

Run fixture repeatedly enough to record stable relative measurements; do not claim a universal SLO.

- [ ] **Step 5: Commit**

Commit message: `test: qualify japanese lexical retrieval candidates`

---

### Task B3: HyperEdge correctness reference

**Files:**
- Create: `experiments/search-discovery-poc/src/hypergraph.rs`
- Create: `experiments/search-discovery-poc/fixtures/graph/relations.json`
- Create: `experiments/search-discovery-poc/tests/hypergraph_correctness.rs`

**Interfaces:**
- incidence index keys:
  - `resource_id -> relation_id[]`
  - `(resource_id, role) -> relation_id[]`
  - `relation_type -> relation_id[]`
  - `(resource_id, relation_type) -> relation_id[]`
- traversal accepts typed relation/role/path constraints and returns relation IDs + target resources + path evidence.

- [ ] **Step 1: Create false-composite RED fixtures**

At minimum:
- R1: Company A / Product B / Collateral X
- R2: Company A / Product C / Collateral Y
- role-swap trap
- high-degree concept node
- evidence namespace relation

Assert false composite Product B + Collateral Y is never returned as one relation/path.

- [ ] **Step 2: Run RED**

Run focused graph correctness test.

- [ ] **Step 3: Implement pure-Rust reference incidence/traversal**

This is the semantic oracle, not the selected durable backend.

- [ ] **Step 4: GREEN + property-style permutations**

Permute participant insertion order and ensure roles preserve meaning.

- [ ] **Step 5: Commit**

Commit message: `test: qualify typed hyperedge traversal semantics`

---

### Task B4: Graph backend feasibility measurement

**Files:**
- Create: `experiments/search-discovery-poc/src/graph_backend.rs`
- Create: `experiments/search-discovery-poc/tests/graph_backend.rs`
- Create: `experiments/search-discovery-poc/fixtures/graph/generate.rs`

**Interfaces:**
- Candidate implementations:
  - in-memory incidence reference
  - PostgreSQL incidence tables in disposable testcontainer
- Dataset scales are recorded in the report rather than hard-coded as production limits.

- [ ] **Step 1: Generate deterministic synthetic graphs**

Include sparse, moderate-degree, and high-degree distributions; preserve true n-ary roles.

- [ ] **Step 2: Assert semantic parity**

Both candidates must return the same typed relations/paths for all correctness fixtures.

- [ ] **Step 3: Measure**

Record:
- build/load time
- memory estimate where available
- typed one-relation traversal latency
- constrained multi-step traversal latency
- high-degree constrained traversal
- rows/nodes/relations expanded

- [ ] **Step 4: Record limitations**

Explicitly state whether persistence/rebuild/readiness characteristics differ.

- [ ] **Step 5: Commit**

Commit message: `test: measure hypergraph backend candidates`

---

### Task B5: Rank fusion baseline

**Files:**
- Create: `experiments/search-discovery-poc/src/fusion.rs`
- Create: `experiments/search-discovery-poc/tests/fusion.rs`

**Interfaces:**
- input is per-retriever ordered candidate lists; raw backend scores are retained only as trace metadata.
- compare a thin Reciprocal Rank Fusion baseline with deterministic no-fusion/priority baselines.
- report ranking quality and overhead; do not treat graph hard relations or hard applicability as soft fusion scores.

- [ ] **Step 1: RED heterogeneous-score fixture**

Create lexical/vector-like/graph candidate fixtures whose raw score scales are intentionally incompatible. Assert no implementation adds raw scores across retrievers.

- [ ] **Step 2: Implement rank-only RRF reference**

Use a configurable constant in the PoC report; do not freeze the production value yet.

- [ ] **Step 3: Measure**

Record MRR/nDCG on the synthetic retrieval fixtures plus fusion latency/allocations where practical.

- [ ] **Step 4: Decide production strategy**

Record whether thin internal RRF is qualified as the initial strategy or whether fusion remains deferred. Hard eligibility remains outside fusion.

- [ ] **Step 5: Commit**

Commit message: `test: qualify rank fusion baseline`

---

### Task B6: Selection report and gate

**Files:**
- Create: `docs/superpowers/execution/search-discovery-platform-v0-poc-report.md`
- Modify: `spec/selection/library-tool-selection-v0.md`
- Modify: `docs/superpowers/execution/search-discovery-platform-v0-status.md`

- [ ] **Step 1: Run full PoC gate**

Run: `mise run poc:search:verify`

Expected: tests + cargo-deny PASS.

- [ ] **Step 2: Write evidence report**

Report exact dependency versions, fixture hashes, metrics, correctness failures, limitations.

- [ ] **Step 3: Record only evidence-supported selections**

Tantivy may remain selected.
Lindera may be promoted only if qualification passes.
Graph contract remains required; dedicated/durable backend selection may remain deferred.

- [ ] **Step 4: Stop at Selection Gate S1 if material choice remains**

Do not make a policy/product decision solely from convenience. Present evidence to requester if the approved design did not predetermine the choice.

- [ ] **Step 5: Commit**

Commit message: `docs: record search discovery poc qualification`
