# P3-P04 native qualification predeclaration

Status: prepared, not measured. This file precedes the new timed runs. The audited
`refinement-*final*` receipts and `refinement-fixture.json` are unchanged.

## Fixed inputs and measurement cells

- Seed `314159`; original ignored `fixture-{100,1000,3000}.json` hashes are pinned
  in `qualification.py`. The 100-group semantic extension must reproduce
  `refinement-fixture.json` byte for byte. Larger profiles use the same 16 named
  scenarios, the unchanged Source-2 canary from the 100-group fixture, and a
  fresh actual `MemoryGraphRetriever` check before any backend timing.
- Query classes: `nary-repeated-role`, `two-hop`,
  `hidden-degree-visible-budget`, `negative-offset-nanosecond`,
  `source-one-revoked`, `relation-only-add`. The complete 16-case oracle and
  full logical digest are correctness gates separate from timing.
- For each backend/profile/class: 200 **successful raw** samples for every
  `cold`/`warm` × `1/2/4/8` reader cell while one incremental writer is active.
  A failed or missing sample never enters a percentile. p50/p95/p99 use nearest
  rank, retaining raw nanoseconds and any error in JSONL.
- `cold` means a fresh host-side query state/read transaction and no retained
  materialized frontier; `warm` means reused native connection/DB handle after
  a warmup. The database/OS page cache cannot be declared cold without an
  independently verified cache reset. Record this distinction in results.
- PostgreSQL uses separate psycopg connections per reader/writer; Neo4j uses
  separate concurrent Query API HTTP calls and reports HTTP transport cost;
  redb uses parallel native Rust read transactions on a shared `Arc<Database>`
  and one native writer transaction, never the old serial JSON-lines pipe.
- Query timing includes native incidence lookup, persisted n-ary participant
  reconstruction, persisted candidate Source policy and scenario revision lookup,
  host typed traversal and path hash comparison. Source policy is stored in each
  candidate backend and fetched on each decision; fixture RAM is used only for
  the independent expected result/full-set audit. Source revisions 1/2/3/4 are
  separate fixed scenario snapshots in the native store, not a mutable live
  production Source row.
- Every raw record says whether the host client was recreated, whether a native
  read transaction was opened per request, whether OS/database cache was
  reset (always false), and whether a warmup was run. For redb, both modes use
  the same shared `Arc<Database>` and a fresh read transaction per request;
  warm receives one untimed request per reader. Do not interpret redb cold as
  disk-cold or an independent process.
- There are 432 required cells and 86,400 accepted raw samples across the
  three backends/profiles. Each cell writes a machine counter snapshot and an
  atomic report checkpoint. A 30-second cell or five-minute profile canary
  stops for diagnosis; resume appends raw attempts without counting incomplete
  attempts as one of the 200 accepted samples. A harness source change needs a
  written before/after amendment before resume.

## Admission and stop bounds

- Candidates run sequentially in an exclusive CPU window with no Cargo build or
  model inference. Before a container: `df` ≥2 GiB; use cached image only. For
  the fixed correctness fixture, owned container growth <512 MiB and stop/remove
  it if free space <1.5 GiB. These are the accepted narrow capacity bounds.
- Before 1,000 and 3,000 timing, record observed 100-profile peak and proposed
  projected peak with adequate headroom to the parent. Do not start larger
  profile until its measured peak admission is accepted. Candidate-owned budget
  is 1.5 GiB disk and 6 GiB process RSS; local canary is 5 min per candidate
  and profile, 30 s per query/fault case. These bounds do not imply an SLO.
- Retain machine-readable build/query/update/disk/RSS/WAL or file, lock-wait,
  restart/restore/fault and publication probe records. Save tool, image, fixture,
  source and raw-output SHA-256. Do not remove any artifact before recording it.
- Fault order: stage/full and relation-only delta; audit complete native rows;
  timed readers plus writer; process restart; separate backup/restore;
  corruption, missing seed reverse key, current Source drift between traversal
  and final read, and failed-stage probes on disposable copies; GC/publish probes.
  PostgreSQL also tests one physical Source row with guard, READY, pointer,
  evaluation pin and GC across independent connections, including concurrent
  stage/validate, publish/pin, pin/GC, expiry/query and CAS-loss cleanup.
- Neo4j batch HTTP stage lacks an atomic READY commit across all chunks. redb
  lacks an atomic commit with the PostgreSQL Source row. Record these missing
  production protocols as failures or explicit `UNMEASURED`, never as `PASS`.
- The PostgreSQL shared Source probe must use the accepted P7 physical schema
  after it exists. An isolated substitute table is not evidence for that gate.

## Selection boundary

`qualification.py:select_backend` refuses any missing candidate/hard gate and
requires a separate independent review GO plus a written comparative decision.
This worker records candidate evidence only. It does not change production
dependencies or migration, and its report cannot assert `Selected` alone.
