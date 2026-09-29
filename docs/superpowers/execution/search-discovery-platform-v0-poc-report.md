# Search / Discovery Platform v0 — Phase B PoC qualification

Date: 2026-09-29 JST. Branch: `feat/search-discovery-platform-v0-b`, stacked on Phase A Draft PR #21. This report covers B1–B6 in the isolated `experiments/search-discovery-poc` crate; it does not assert production backend selection, deployment, or merge.

The Phase B advisory exception described below is the historical qualification on its exact head. A [follow-up PoC-only Tantivy patch](search-discovery-platform-v0-tantivy-patch.md) removes that exception and rechecks the same deterministic quality evidence; production promotion remains a separate gate.

## Evidence identity and gate

- Machine-readable deterministic receipt: `experiments/search-discovery-poc/qualification-report.json`. `search-discovery-poc verify` checks six fixture hashes, locked candidate versions, fusion target IDs against lexical query relevance truth, hard eligibility against lexical kind/audience contracts, and reproducible lexical/fusion quality metrics. Captured timings are archival measurements, not rerun by this command; PostgreSQL correctness and the typed path behind the graph-only relevance case are exercised by the PoC test gate. `report --format json` only emits a verified receipt.
- Fixture SHA-256: lexical resources `968884c9befa1ad61805d66a12bfcc9067794e2af26bc2087a8767a703f552cd`, lexical queries `1e386096e607bfc04a7e19cb90dfd5c99eb3a343314d9ee487f8e42a74d7f030`, graph relations `ac2727be71953fd41d6a13ece2d48f7d9709ba8062b894fbd96e8cd4caf71867`, graph generator `c5df94b3b8f0be97309b1c0c82c9732726a3c8fb463943388d8975d0b1c737d4`, fusion cases `8fe75e39c5224560aa3088cb2b9dbb001502f5426fe2332779c52662977a1f76`, fusion graph relations `35ba415091406aff86e7365a3f71d309a9e358562e16645c73f75abf6e272e3d`.
- Locked direct candidates: Tantivy 0.26.2; Lindera, Lindera Analysis and Lindera IPADIC 6.2.0; Tokio Postgres 0.7.18; testcontainers 0.28.0; disposable PostgreSQL image `18.6-bookworm`. Lindera-tantivy 4.0.0 targets Tantivy 0.25.x, so this PoC used Lindera pretokenization with Tantivy 0.26.2 instead of the adapter.
- B6 extension `mise run poc:search:verify` passed after independent review: relevance-truth unit test 1/1, harness/receipt 5/5, lexical 3/3, HyperEdge 5/5, PostgreSQL parity/measurement 2/2, fusion 8/8, cargo-deny advisories/bans/licenses/sources. The review found that receipt verification did not reject a hard-eligible but irrelevant fusion target; a focused RED test reproduced the missing validator and GREEN passed after adding the check. Isolated strict Clippy, fmt and diff check passed. The earlier `mise run security:deps` passed with one advisory filtered by the PoC-scoped exception; no dependency was changed by this extension. The execution status records hosted exact-head outcomes.
- Dependency metadata gate on the original Phase B head: pass with a **PoC-local, expiring advisory exception**. Hosted CI security on head `cf1d859` found `RUSTSEC-2026-0253` in transitive `lru 0.16.4` via Tantivy 0.26.2. [RustSec](https://rustsec.org/advisories/RUSTSEC-2026-0253.html) says the `LruCache::pop()` issue requires a key whose `Drop` panics; Tantivy 0.26.2's only `LruCache` here is `LruCache<usize, Block>` in `src/store/reader.rs`, and `usize` has no `Drop`. `experiments/search-discovery-poc/osv-scanner.toml` filters this advisory and its aliases for this PoC lockfile through 2026-12-31. Local `mise run security:deps` passed and reported the filtered advisory. [Upstream changed its main-branch dependency to `lru 0.18.2` on 2026-08-06](https://github.com/quickwit-oss/tantivy/commit/5ca39332002c2c87fb5d2abc707cf527b3319d42), but the [latest published Tantivy tag remains 0.26.2](https://github.com/quickwit-oss/tantivy/tags) as of 2026-09-29; no release date for the updated line is recorded there. Thus an upstream-release wait has no evidence-based duration. The PoC exception does **not** clear the production Tantivy dependency gate; assess a published upstream fix or a separately reviewed production reachability decision before promotion. Embedded dictionary data itself is fetched at build time, and its separate rights/packaging review is pending. The PoC-only deny allowlist includes `CDLA-Permissive-2.0` for transitive `webpki-roots`; neither allowance changes the production license policy.

## B2 — Japanese lexical retrieval

Twelve synthetic resources and 13 queries cover 法人/個人, 融資/預金, 住所変更, 事業性融資, 規程/手順/マニュアル, aliases, and mixed API/date/number text. Both candidates recovered all 11 mandatory exact/alias cases and preserved the hard kind/audience discriminator after candidate generation.

Five alternating local runs, each with 13 queries repeated five times, gave these medians (unoptimized build):

| Candidate | Recall@10 | MRR | nDCG@10 | Index bytes | Build ms | Query p50/p95 ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Tantivy default | 0.8462 | 0.8077 | 0.8178 | 5,035 | 96.16 | 0.072 / 0.159 |
| Lindera IPADIC pretokenization | 0.9231 | 0.8231 | 0.8475 | 5,939 | 100.18 | 0.150 / 0.196 |

Both missed diagnostic `compound-corporate-loan`; Tantivy default also missed `compound-location-change`. These are visibility/coverage gaps, not evidence that a different retriever has solved them. Index bytes exclude the embedded dictionary/binary. The corpus is too small to establish Japanese production quality or a performance SLO. See `experiments/search-discovery-poc/fixtures/lexical/README.md` for method and limitations.

## B3/B4 — Typed HyperEdge and backend feasibility

The pure-Rust reference preserves relation identity and participant roles. It rejected the Product B + Collateral Y false composite from separate loan relations, ignored a role-swapped trap, isolated evidence namespace, enforced branching/path budgets, and retained both relation IDs on a true two-hop path. PostgreSQL incidence tables returned the same full typed path evidence on these fixtures and on generated sparse/moderate/high-degree graphs with four roles per relation. Shared input validation rejects duplicate `(role, resource_id)` pairs and malformed relations before either backend builds.

Representative local measurements (one run, 25 queries per p50, unoptimized build):

| Case | Relations | Rust build ms | Rust heap lower bound | PG load ms | PG table/index bytes | Rust / PG query p50 ms | Returned rows / distinct relations / expanded nodes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Constrained 2-hop | 9 | — | — | — | — | 0.0029 / 0.872 | 3 / 2 / 2 |
| Sparse | 128 | 0.599 | 112,675 B | 17.980 | 221,184 B | 0.0009 / 0.634 | 1 / 1 / 1 |
| Moderate | 512 | 2.421 | 396,269 B | 26.107 | 606,208 B | 0.0041 / 0.741 | 8 / 8 / 1 |
| High degree, constrained | 1,024 | 4.923 | 790,219 B | 40.520 | 1,081,344 B | 0.0344 / 1.595 | 1 / 1 / 1 |

The Rust memory figure excludes BTreeMap node/allocator overhead; PostgreSQL bytes exclude WAL/server/container memory. Returned rows are SQL output rows, not planner-scanned rows. Expanded nodes count frontier path instances on which a step issued SQL, including repeats of the same resource through distinct paths. The in-process reference must rebuild after restart. The PostgreSQL PoC drops/reloads tables, so durability recovery, generation readiness, concurrent update, access/temporal/authority filters and production scale remain unmeasured. No durable Graph backend is selected. See `experiments/search-discovery-poc/fixtures/graph/README.md`.

## B5 — Rank fusion

The requester chose additional Phase B evidence at S1. The original five synthetic heterogeneous-score cases retain only one candidate each after hard kind/audience eligibility and cannot distinguish strategies. Three added cases use two distinct lexical-query relevance truths with at least three eligible workflow candidates, plus one `graph-only-corporate-loan` case. In that case, actual Tantivy default evaluation misses `loan-corp` for `compound-corporate-loan`, while a typed three-participant HyperEdge path with a `product-b` constraint returns only `loan-corp`; a test checks the relation ID, path target and graph candidate list. This is still synthetic graph evidence, not a durable Graph or routing measurement.

Five local runs of eight cases, each with 100 rank repetitions per case/strategy, produced the following stable quality values and p50 medians (unoptimized build):

| Strategy | Recall@10 | MRR | nDCG@10 | Fusion p50 ms |
| --- | ---: | ---: | ---: | ---: |
| Lexical only | 0.875 | 0.7917 | 0.8125 | 0.00025 |
| Graph only | 1.0 | 0.9167 | 0.9375 | 0.00025 |
| Priority concat | 1.0 | 0.9167 | 0.9375 | 0.000375 |
| Lexical + graph RRF `k=20` | 1.0 | 0.9375 | 0.9539 | 0.000625 |
| Vector-like only (diagnostic) | 0.875 | 0.7500 | 0.7827 | 0.00025 |
| Three-list RRF `k=20` (diagnostic) | 1.0 | 0.8750 | 0.9077 | 0.000792 |

The lexical+graph RRF MRR difference over priority concat is `+0.0208` on this hand-built set: it improves one added case from rank 3 to 1 and worsens another from rank 1 to 2. Priority concat also retains the typed graph-only hit when lexical misses. Raw scores are trace-only and changing their magnitude does not change RRF order. The vector-like list is diagnostic only. The sub-microsecond timing values are too small to compare operational performance reliably. The two multi-eligible retriever rankings were constructed as sensitivity probes, not measured end-to-end retrieval output; the original five non-discriminating cases still dominate the aggregate. The old raw-candidate RRF advantage remains **withdrawn**. See `experiments/search-discovery-poc/fixtures/fusion/README.md` for case-level ranks and limitations.

## Selection Gate S1 — initial priority concatenation selected

The approved design already selects Tantivy and Phase C specifies a rebuildable in-process HyperEdge retriever. B2–B4 qualify those *limited* uses; Lindera/dictionary packaging and durable Graph backend remain deferred. Vector, Embedding and Reranker were not needed by this PoC and remain deferred.

The initial **fusion policy** is a material ranking choice that the approved design did not predetermine. The requested additional evidence shows a graph-only fallback and exposes both RRF benefit and regression among hard-eligible candidates, but the hand-built eight-case set does not qualify a general RRF gain or fix `k`. On 2026-09-29 the requester selected routed retriever-order priority concatenation behind `FusionStrategy`, with hard applicability applied first. It is deterministic, retains the graph-only fallback and needs no extra dependency; its rank-3 result in the rescue case remains an explicit limitation. This is an initial policy decision, not an observed universal quality gain. The separately reviewed Tantivy dependency patch was selected for the production lexical adapter in the selection record. Both selection gates are resolved for Phase C entry; production code and exact-head dependency/security checks are still required. Tantivy default and the in-process typed HyperEdge reference remain limited functional candidates; Lindera, a durable Graph backend, Vector/Embedding/Reranker and external fusion library are not promoted.
