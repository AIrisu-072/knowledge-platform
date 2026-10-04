# P3 final fixed-fixture capacity ruling

Status: accepted implementation environment ruling by Program Orchestrator, 2026-09-30. The program already authorizes reversible local qualification. This changes no business meaning, backend choice, corpus, qrels, correctness gate, retention, authority, or live deployment. Original plans and historic receipts remain immutable.

## Evidence and bounded applicability

The final native-incidence fixture is SHA-256 `d870c538f762e0fdaf30b8f901a48b3b8411504eac1da4bbe36766ee10fc0cf3`, 403 first-Source resources/230 relations plus the small second Source and 16 oracle scenarios. Native redb, cached PostgreSQL 18.6 and cached Neo4j images have already run this exact size sequentially. The source-only redb rebuild completed without large new dependencies. Owned PG/Neo containers were removed after each run. The prior 3 GiB reserve is a conservative environment budget from the broad qualification plan; it is not a frozen business requirement. Repeatedly waiting at that boundary while another small pure build consumes a few hundred MiB adds no correctness evidence.

## Narrow final correctness rerun

For only the fixed fixture's native row/incidence/digest/current-authority/corruption correctness rerun, the worker may start a single owned cached container with at least **2 GiB available** after recording `df`. The disposable container and data growth must be measured and kept below **512 MiB**; check available disk before/after staging and stop/remove that owned container if available space falls below **1.5 GiB**. No new image pull, model weights, corpus scale increase, Graph production migration, backup retention increase, shared Docker cleanup, or large Rust compile is allowed in this window. A pure Core/protocol build may proceed under the parent queue; model asset downloads remain held.

A performance/capacity selection run uses its separately predeclared peak budget and exclusive measured execution window; correctness elapsed time observed while pure builds run is not a benchmark or SLO. If the bounded assumptions fail, stop the local rerun and recover only owned regenerable cache after all owners release it. Do not alter fixture/gates or report an unexecuted backend PASS.
