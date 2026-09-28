# Search / Discovery Platform v0 — Phase B PoC qualification

Date: 2026-09-29 JST. Branch: `feat/search-discovery-platform-v0-b`, stacked on Phase A Draft PR #21. This report covers B1–B6 in the isolated `experiments/search-discovery-poc` crate; it does not assert production backend selection, deployment, or merge.

## Evidence identity and gate

- Machine-readable deterministic receipt: `experiments/search-discovery-poc/qualification-report.json`. `search-discovery-poc verify` checks five fixture hashes, locked candidate versions, hard eligibility against lexical kind/audience contracts, and reproducible lexical/fusion quality metrics. Captured timings are archival measurements, not rerun by this command; PostgreSQL correctness is exercised by the PoC test gate. `report --format json` only emits a verified receipt.
- Fixture SHA-256: lexical resources `968884c9befa1ad61805d66a12bfcc9067794e2af26bc2087a8767a703f552cd`, lexical queries `1e386096e607bfc04a7e19cb90dfd5c99eb3a343314d9ee487f8e42a74d7f030`, graph relations `ac2727be71953fd41d6a13ece2d48f7d9709ba8062b894fbd96e8cd4caf71867`, graph generator `c5df94b3b8f0be97309b1c0c82c9732726a3c8fb463943388d8975d0b1c737d4`, fusion cases `505ab0c9306396c15d3ec973c7c30f46b687858d172d6534b26836b004418136`.
- Locked direct candidates: Tantivy 0.26.2; Lindera, Lindera Analysis and Lindera IPADIC 6.2.0; Tokio Postgres 0.7.18; testcontainers 0.28.0; disposable PostgreSQL image `18.6-bookworm`. Lindera-tantivy 4.0.0 targets Tantivy 0.25.x, so this PoC used Lindera pretokenization with Tantivy 0.26.2 instead of the adapter.
- Final B6 `mise run poc:search:verify` passed: harness/receipt 4/4, lexical 3/3, HyperEdge 5/5, PostgreSQL parity/measurement 2/2, fusion 6/6, cargo-deny advisories/bans/licenses/sources. Isolated strict Clippy and fmt passed. Its local evidence is recorded in the execution status; hosted exact-head gates are checked after the Draft PR push.
- Dependency metadata gate: pass. Embedded dictionary data itself is fetched at build time, and its separate rights/packaging review is pending. Therefore overall production license decision is **pending** even though the isolated cargo-deny gate passed. The PoC-only deny allowlist includes `CDLA-Permissive-2.0` for transitive `webpki-roots`; this does not change the production license policy.

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

The five synthetic heterogeneous-score cases now apply hard kind/audience eligibility **before** rank evaluation. Each leaves one eligible candidate, so lexical-only, graph-only, priority concatenation and lexical+graph RRF (`k=20`) all yield Recall@10/MRR/nDCG `1.0/1.0/1.0`. Local p50 ranking times were approximately `0.0006/0.0006/0.0009/0.0015 ms`; these microsecond measurements are not a production performance comparison. A focused mutation test shows priority concatenation retains an eligible graph result when the lexical list is empty. Vector-like scores remain diagnostic only. Raw scores are trace-only, and changing their magnitude does not change RRF order. The previous raw-candidate MRR advantage for RRF was caused by hard-ineligible candidates and is **withdrawn**. These five cases cannot qualify a soft ranking improvement or fix `k`. See `experiments/search-discovery-poc/fixtures/fusion/README.md`.

## Selection Gate S1 — requester decision pending

The approved design already selects Tantivy and Phase C specifies a rebuildable in-process HyperEdge retriever. B2–B4 qualify those *limited* uses; Lindera/dictionary packaging and durable Graph backend remain deferred. Vector, Embedding and Reranker were not needed by this PoC and remain deferred.

The initial **fusion policy** is a material ranking choice that the approved design did not predetermine. The eligible fixture does not distinguish RRF from simpler baselines, so RRF is **not qualified** for promotion. A conservative S1 option is routed retriever-order priority concatenation behind `FusionStrategy`, with hard applicability applied first; it is deterministic, retains a graph-only fallback in the focused test and requires no extra dependency. This is a proposed initial policy, not an observed quality gain. Another option is to extend Phase B with hard-eligible multi-candidate and genuine graph-only relevance cases before selecting a strategy. Until the requester decides, no initial fusion strategy is selected and Phase C C5 must not begin; do not start Phase C as a whole. Tantivy default and the in-process typed HyperEdge reference remain limited qualified candidates; Lindera, a durable Graph backend, Vector/Embedding/Reranker and external fusion library are not promoted.
