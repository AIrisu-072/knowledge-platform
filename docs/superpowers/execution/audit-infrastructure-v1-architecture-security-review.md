# Audit Infrastructure v1 — independent architecture/security review

## Review 1 — NO-GO for Design Freeze, 2026-10-02 UTC

Reviewed docs head: `6a9078e9dfe7698d055d749307bc59bb056eb0de`, base `d71753d46590bb4406a1c0b74894ab90a27a6c88`. Independent reviewer found six Important/P2 issues, no Critical finding and no global STOP. No schema/product implementation had started.

| Finding | Source evidence / risk | Revision proposed for re-review |
|---|---|---|
| R1 Legacy reason loss | `withdrawal.rs:193–222` stages no operation ID; `access_policy.rs:311–344` Management ledger stores digest/result, not reason | Design§4 compatibility matrix defers/quarantines every free-text reason-bearing variant; original evidence is unchanged/ineligible for cleanup. A reference is not full preservation; amendment/retrieval contract remains separate |
| R2 Allocated sequence is not commit order | Lower sequence can commit after reader/checkpoint moves beyond a higher one | Design§§5/7 transactional publication-counter locks held through source/sink COMMIT; all paths participate. Source bootstrap installs trigger behind a write barrier, uses anti-join batches and final zero-missing barrier. Tasks3/6/7 add delayed-lower commit/rollback and concurrent-bootstrap tests |
| R3 Expiration erases dedup identity | Duplicate source replay after expiration/crash can resurrect deleted evidence | Design§§6/7/8 permanent event-ID/digest/receipt registry survives live-row expiry; atomic tombstone transition; same expired replay returns Expired, conflicting replay fails. Tasks3/4/8 test expiry/crash/restored-source replay |
| R4 Retention plan staleness | Policy may become retain-all or longer between eligibility and delete | Design§8 binds plan to policy revision/epoch, locks/rechecks current policy and record state transactionally. Unknown/deleted policy or hold retains. Task8 adds extension/deletion/concurrent-policy tests |
| R5 Cleanup exceeds recovery horizon | Source deletion after live sink receipt makes older sink backup unrecoverable | Source destructive cleanup deferred in v1; no DELETE grant/API. Restore investigation/export/maintenance remains blocked until latest policy/tombstone/receipt/trusted checkpoint continuity is proven. Task8 injects missing-source plus old-sink restore and expects unrecoverable/blocked |
| R6 Limit occurs after unbounded load |32KiB output validation cannot bound materializing legacy JSONB/free reason | Design§5 SQL metadata admission before raw read/render/hash, rejects compressed values conservatively and bounds uncompressed storage≤24KiB/scalar text≤512 bytes. Task3 tests64MiB compressible reason/large raw fields and continued bounded events; no rejected body leaves storage |

These are proposed repairs, not reviewer approval. The initial NO-GO remains operative until an independent re-review explicitly returns GO. Core invariants previously judged sound remain: atomic mandatory staging/receipt, sink-outage separation, fenced unknown outcomes, Audit-only migrations/state, independent roles/pools, finite access-audit recursion, independently retained integrity checkpoints, deferred Organization/Search integration.

Verification for this revision: documentation-only diff, source evidence re-read, PostgreSQL18 official size/compression semantics checked; product/schema/SQL/build tests NOT RUN. Whitespace and link/scope checks are recorded in capability status. No existing frozen Document design or test was altered.


## Review 2 — residual R6 NO-GO, 2026-10-02 UTC

Reviewed revision: `3229f27fdacccab5ef08630853eba07cdc8a8de0`. Independent reviewer confirmed R1–R5 resolved at design level. Residual R6: PostgreSQL JSONB binary output also renders through `JsonbToCString`; compact numeric values can expand greatly despite a small uncompressed stored size. `1e100000` and valid high-scale `1e-16000` exercise this path. Compression/physical-size checks alone are insufficient.

Next proposed repair: procedural physical/compression gate → bounded internal JSONB traversal/native numeric integer+signed64 checks → proven≤256KiB intermediate rendering → exact≤24KiB wire check before transfer/hash. No reorderable predicate-order assumption or raw `jsonb_send` on unadmitted rows. Task3 adds compact-exponent/high-scale/numeric-array tests. Also state sink lock order counter→policy/control→identity IDs across every publication/expiration/control path. These edits await independent re-review; no implementation or freeze claimed.


## Review 3 — GO for A1 freeze, 2026-10-02 UTC

Independently reviewed head `e48acee7bf6d63fe3352698cd8de5be0dd1fb8cf`, tree `113b6f78fe94253e7a8ec00793a298e2fdecff9c`. The reviewer confirmed residual R6 closed at design level by ordered physical→structural/native-numeric→bounded-render→wire admission and confirmed the explicit sink lock order. No remaining P1/P2 or global STOP. Clean tree and whitespace check verified. No code/tests/builds ran in this review.

GO permits the A1 freeze and A2 test-first qualification under the existing bounded authorization. It does not qualify any backend/runtime. Reason-bearing producer delivery remains deferred/quarantined; source cleanup remains absent; restore fail-closed behavior and all qualification tests remain mandatory.

Frozen design blob: `a1d2002afb7525f19f40138a82365bc975f493ed`. Frozen plan blob: `3fd2d2ef2b8891dd63345233c5f67f69412dfbed`. This records independent technical review plus the separately recorded user scope authority, not a claim of artifact-specific user review.


## Bounded compatibility addendum — GO, 2026-10-02 UTC

Independent review approved committed addendum `dfdd123a49629c4c681fed5db8ee6cd4a4d9b5d6`, blob `c932988f7d9aadd8d1d77271fe184a9ca250026a`, with no P1/P2. Source confirms three32-byte Diff digest arrays and the `diff-test` correlation token.512 total source nodes retain the unchanged depth/field/physical/wire/render limits; explicit legacy correlation preserves evidence without relabeling it as trace context. Exact512/513, escaping/numeric and malformed-trace qualification remain required. Original frozen design/plan are unchanged. This review covers committed addendum/source only, not in-progress A2 implementation.


## A2 independent code review1 — NO-GO, 2026-10-02 UTC

Reviewed `c177ca0c3dcc3f5fdbf9265a3e5af0392f12a2ef`, tree `549ba13f74fb0f187437f57d795abee20e2c9086`. Two Important/P2 findings: `access_policy.changed` target fields were not cross-bound to resource, and duplicate raw JSON members could be discarded by last-wins parsing before privacy/canonical admission. Independent17-test rerun and direct probes reproduced both. No new global STOP. Three regression tests observed RED, then repairs restrict ACL resource/target semantics and reject every duplicate decoded key before constructing maps. A mixed-sign legacy timestamp edge also received RED→GREEN without normalizing source evidence. Local21-test GREEN and narrow checks are supplemental only; independent re-review GO is still required before A3 or schema publication.


## A2 independent code re-review — GO, 2026-10-02 UTC

Reviewed repaired head `dd17021ec0938fe77430769d3ee5a2c1d2b6d97a`, tree `c8d79ecdfbced36da931d4a88da5884db68188a9`. Both original P2 probes now reject. Mixed-sign legacy tuples reject in runtime/schema while valid signed offsets retain their values. Independent Rust21/21, Node4/4, generation and diff checks passed, plus seven direct parser edge probes (nested array/Unicode-escaped/null duplicate members, recursion bound, trailing data and duplicate root ID). No remaining Critical/Important finding. Candidate stayed clean/unchanged during review.

GO covers A2 code/schema review only. Exact-head hosted/full aggregate and A3 backend/source SQL/failure/recovery qualification remain separate. Deferred reason-bearing delivery and source cleanup remain deferred; no producer/backend claim follows from this receipt.
