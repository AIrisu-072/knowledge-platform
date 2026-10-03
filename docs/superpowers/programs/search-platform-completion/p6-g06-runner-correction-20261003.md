# P6 G05/G06 synthetic runner correction — 2026-10-03

## Result and exact boundary

Scoped local correctness GO for the generic in-process admission/lifecycle gate. Fresh exact-source confirmation passes **runner_admission 5/5 and runner_lifecycle 35/35**, targeted strict Clippy, two-file rustfmt, and diff check. An independent read-only reviewer checked the final source, assertions, diagnostics, and frozen contract and found no remaining substantive defect in this scope. It did not independently rerun Cargo.

This is not full-package/workspace, real PostgreSQL, process-crash, Search bridge, P7, or whole-program qualification. Missing `outbox_delivery::observe` still prevents the current full hosted Rust gates. G07 and G08 remain open. The unidentified historical stopped operation was neither reconstructed nor retried; the newly reviewed synthetic scope does not clear other actions.

Local base is `3955eb5eeedebebc1a77afe3df44d0569953ad47`, tree `d22b3b3e784839dade91734d725f1e3384740aa0`, which stacks the separately reviewed but unpublished three-PoC-lock checkpoint. Last verified remote Draft #40 is `401b31047a64ed76c470477c6db15fc7e8221d2d`, root-dependency tree `2df2784e8017f1433a2fb96d8054e4aca3478299`. Publication of further source/verification records remains held pending explicit permission for the public repository; no merge or deployment.

## Frozen requirements and correction

Authority remains `p6-outbox-freeze.md`, revision-1 design, and G05/G06 in `p6-outbox-plan.md`; no design, public API/config, schema, policy, dependency or Search/P7 boundary changed.

- One total processing deadline begins before initial preflight and covers handler, heartbeat, final preflight and settlement. A private dependency-round budget is the smaller of remaining processing budget and one third of the configured lease duration. The latter is an implementation choice using existing bounds, not a new product SLO.
- A sticky shutdown drain deadline stays observable during admission, claim, reaping and cleanup. Pending operations cannot restart a fresh drain window. Ready known permits/claims are captured before stopping; release/reaper outcomes are never invented.
- Cancellation drop guards stop new work. Dependency I/O remains explicitly uncertain until the parent observes a result; unresolved preflight, heartbeat or settlement at deadline reports `StoreUnknown`, distinct from confirmed fence loss or handler-only incomplete work.
- Cancellation is checked after dependency responses and before the next renewal/preflight/handler/settlement step. A returned dependency error retains precedence over cancellation. The I/O marker is published before checking cancellation, closing the reviewed race at heartbeat/initial-I/O entry.
- G05 free-capacity-before-claim, Source cap 1, two-fence preflight and unchanged admission assertions remain covered. No event is acked/failed merely because a task is cancelled or an unresolved write was attempted.

Rust async bounds are cooperative. These tests do not prove preemption of non-yielding code, all multicore interleavings, or database commit outcomes. The I/O-marker ordering was statically reviewed; no model-checking result is claimed.

## RED / correction evidence

Each row records a real test-only RED before its associated production correction. Existing assertions were preserved; added fakes use only synthetic in-process futures and counters.

| Test stage | Observed RED | Subsequent defect addressed |
| --- | --- | --- |
| Pending operation/total budget | 4 pass, 17 fail, exit 101 | Total deadline and shutdown interrupts across preparation, heartbeat, settlement, cleanup |
| Shutdown uncertainty | 22 pass, 6 fail, exit 101 | Pending dependency writes must remain Unknown at drain expiry |
| Cancel before heartbeat | 28 pass, 1 fail, exit 101 | No new renewal after public cancellation flag |
| Response-boundary cancellation | 30 pass, 5 fail, exit 101 | Stop subsequent dependency/handler/settlement work after cancellation |

The final 35 tests comprise four original cases, 29 newly demonstrated RED regressions, and two positive controls (dependency timeout before lease expiry and returned-error precedence). The final confirmation runs the exact final 35 cases plus five unchanged admission cases.

An additional **post-fix negative control** copies the final exact 35-test file into a detached local-base worktree with the original unmodified runner. It yields **5 pass / 30 fail, exit 101**. This is a counterfactual regression-sensitivity check performed after correction, not a claim that the final full suite existed before the first fix. The original runner is `e04313b441f9848bc8ff58eaa2a117fc8af2c839b06276972bd6f2cb43dc85de` (SHA-256).

## Final verification and identity

- Runner SHA-256: `fdb3c1164e6db4d092fe8745af0c952e5e33a122441cd6a4eaa4377867ea22cb`
- Lifecycle tests SHA-256: `4e8b3fa50f7446ba3b7dd843c91d346b8b309fd9d685b16b7ba978dbf5799db2`
- Unchanged admission tests SHA-256: `9fa72c22fa0cb555c0a293ca29e02644c6722e83f7008452ccb71cb407d82d20`
- `cargo test --locked --offline -j 2 -p outbox-delivery --test runner_admission --test runner_lifecycle -- --test-threads=1`: exit 0, 5 + 35 pass
- `cargo clippy --locked --offline -j 2 -p outbox-delivery --test runner_admission --test runner_lifecycle -- -D warnings`: exit 0
- `rustfmt --edition 2024 --check crates/outbox-delivery/src/runner.rs crates/outbox-delivery/tests/runner_lifecycle.rs`: exit 0
- `git diff --check`: exit 0 before documentation packaging; repeat on final staged checkpoint

The pinned official Rust 1.98.1 toolchain uses an isolated Search target and offline existing dependency cache. Missing exact-version rustfmt/Clippy components were restored only after their publisher-manifest SHA-256 checks matched. Their official URLs/hashes are in the machine receipt. No new project dependency or system security setting was introduced. `DATABASE_URL` and `P6_TEST_DATABASE_URL` were unset; selected tests make no external calls. Existing trusted dependency build scripts/proc macros may run during compilation; compiled testcontainers support was not invoked.

The final confirmation wrapper retained actual commands, exit codes, elapsed times, log hashes and source hashes. It observed minimum free space 25,272,307,712 bytes versus the 1,610,612,736-byte floor. Raw logs and chronological wrappers were retained locally; committed copies only strip trailing line whitespace and trailing blank lines. [Machine receipt](p6-g06-evidence-20261003.json) records raw and committed hashes. The negative control's exit is directly captured, but unlike final confirmation it has no separate continuous-disk-sampler receipt; this limit is explicit.

## Counterfactual cache isolation check

The first switch back from the negative-control worktree reused its lifecycle executable in the shared target: the lifecycle binary/depfile timestamp remained at that control build and the same 30 cases failed, despite the candidate source hashes being unchanged. Admission had rebuilt and passed. Touching only the two owned candidate source mtimes forced a rebuild; the same command then passed **5/5 + 35/35**, followed by targeted strict Clippy, rustfmt and diff check exit 0. No source bytes or assertions changed and no cache files were deleted. Both the stale-cache diagnostic and corrected fresh-build logs are retained with exact hashes. Future counterfactual worktrees must use separate Cargo targets. This failed cache-switch attempt is not omitted or represented as a successful verification.

The first staged documentation check reported two trailing blank lines in copied logs. A follow-up documentation-only normalization removes them, updates the committed-log hashes, and passes the full checkpoint-range diff check. Runtime source and raw diagnostic bytes remain unchanged.

## Next exact action

Preserve and review this local checkpoint. Await public-sharing authorization before publishing the separately stacked PoC metadata and G06 records. Prepare an independently reviewed G07 bootstrap/test scope with exact official tool hashes, a dedicated synthetic PostgreSQL cluster, bounded owned-child cleanup and explicit unknown outcomes before installing or executing G07. No P3/P7 or previously denied security probe is authorized by this result.
