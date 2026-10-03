# P6 G07 pure fixture guard checkpoint — 2026-10-03

## Bounded local result

The two-file source checkpoint is `fda5ff8a77cb7e237d55665c532134392267a651`, tree `14c66619732559e3403eaea096cb297afc142c9d`. It adds pure configuration/identity/Unknown/deadline/environment/output-control helpers and six regression tests. The actual G07 child entrypoint remains deliberately unwired. No real PostgreSQL, Docker, recovery child, parser, P3/P7 or security-probe runtime occurred.

Fresh named pure verification passes: Python policy 53/53; the six exact Rust helper cases 6/6, with all other integration-test entries filtered; selected offline/locked compilation; strict Clippy for `process_recovery`; rustfmt for the two changed files; diff check. Independent read-only source review found no blocker in this bounded pure scope. This does not qualify G07 process recovery, P6 or the Search program.

## Chronological evidence and corrections

1. Initial reviewed helper RED: Python 53 ordinary assertion failures against inert stubs (exit 1); selected Rust compile exit 0; six separately selected Rust tests each fail at the intended missing-helper assertion (exit 101). Neither test suite reaches DB/child paths.
2. Minimum pure implementations then pass Python 53/53 and Rust 6/6 under the same named source/command boundary. The initial launcher/fixture scope-path drafts disagreed; the attempted `receipts/scope.json` drift was reverted before execution to the recorded `B/scope.json` contract. No executed result is attributed to that transient candidate.
3. First selected strict Clippy fails on the existing shared test fixture's `large_enum_variant`: Docker's `ContainerAsync` occupies at least 848 bytes versus the external variant's 48. No other lint failure is shown. Format/diff stages were not reached in that stopped quality sequence.
4. Explicitly scoped internal refactor boxes only the existing `ContainerAsync` field and its construction. Four call sites retain the guard opaquely; no production API or fixture control path changes. The same container retains its guard lifetime, with one heap allocation added. This is a source/layout review, not a claim that legacy Docker behavior ran.
5. Fresh post-refactor checkpoint passes Python 53/53, Rust 6/6, selected strict Clippy, two-file rustfmt and diff check. Actual test artifact SHA-256 is `110da64de4356eb22a9b191f4a446ededab104e14e9a630ea723f817f1278498`.
6. Independent interface review exposed two pre-runtime gaps: Python's draft DB name omitted the run prefix accepted by Rust, and archive-valid-input coverage was incomplete. A new 34-case pure supplemental suite reproduces **25 intended assertion failures with nine passing controls**. That RED is preserved; no policy fix or bootstrap success is claimed here. Its subsequent correction remains a separate step.

## Source and supervision boundary

Only `crates/outbox-delivery/tests/process_recovery.rs` and narrow additions plus the two-line box refactor in `tests/support/postgres.rs` change application source. Pure reducers are not wired into any effectful fixture yet; the existing child/DB paths remain unexecuted. Existing Domain migrations, production runner/store, API, dependency/policy files and frozen semantics remain unchanged.

Source SHA-256:
- process_recovery.rs: `56ace805d42c7b7474f0ff9e162803ac1c76191f2bdd25e6a8635b202ce46658`
- support/postgres.rs: `43a2d78441a5b1744003d360a745c053495a0a796e0a7cfec7d78829482b6137`

All commands used existing pinned Rust 1.98.1/Python and a newly owned Cargo target, locked/offline dependencies and empty-base environments. Compilation can execute the existing trusted build-script/proc-macro closure; compiled Docker support was not invoked. The selected compiler artifact was bound by exact path/hash before the six direct test invocations, whose environment carried no database/child configuration.

The fixed supervisor retained actual exit status, bounded stdout/stderr, source/tool/binary hashes, disk observations and initial/terminal receipts. It checked the 1,536 MiB floor and 8 GiB owned-growth budget, reserved bounded termination/drain time, retained the direct leader's identity until its original process session was non-running, and never signaled by process name. This is trusted compiler/pure-test supervision, not containment of adversarial daemonizing software. Unknown supervision stops rather than inventing success. Every substantive stage in the successful checkpoint has null abnormal reason.

[Machine evidence](p6-g07-pure-guards-evidence-20261003.json) preserves actual results and raw log hashes, including the failed lint and supplemental RED. Full raw streams, command manifests, source snapshots and independent evidence checks remain in the local task evidence directory; this small repository checkpoint does not claim every raw artifact is committed or externally durable.

## Remaining gates and exact next action

Correct and re-review the supplemental pure DB-name/archive cases, then prepare actual owned fixture and launcher implementation. Their exact executable/source/command manifest must receive independent review before official tool acquisition, bootstrap or the two real-process cases. One dedicated peer-auth Unix-socket cluster, positive ownership proofs, finite budgets, child watchdogs, no Docker fallback and explicit Unknown outcomes remain required.

Draft #40's last verified published head remains `401b31047a64ed76c470477c6db15fc7e8221d2d`; separate PoC-lock and G06/G07 checkpoints are local only pending public-sharing authorization. Missing `outbox_delivery::observe`, all broader G07/G08/P1–P7 and final acceptance gates remain open. The unidentified historical stop is not retried or declared cleared. No merge or deployment.
