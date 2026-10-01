# P6 process-recovery draft — compile-only correction, 2026-10-01

## Scope and independent review

**WIP; P1–P7 and whole-program acceptance remain incomplete.** This is a one-line type annotation in a saved P6 test draft, plus this checkpoint. No production implementation, dependency, schema, fixture, scanner policy, parser, P3 workflow or runtime behavior is changed.

Parent is Draft PR #40 head `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`. In `crates/outbox-delivery/tests/process_recovery.rs:306`, `previous.get("lease_token")` becomes `previous.get::<Uuid, _>("lease_token")`. Independent static review verifies migration 0009 declares the column UUID, the outbox model uses Uuid, the same assertion's current-row decoder already requests Uuid, and the file imports `uuid::Uuid`. The annotation resolves SQLx Decode/PartialEq inference without altering the assertion or database action.

The reviewed patch exactly equals the producing worktree's sole diff against `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a`. No other source change occurred between the recorded RED and GREEN.

- Original source Git blob: `3cdfdb1373f61e1456995a5152389ebb42347585`
- Corrected source Git blob: `f930c0693cd881e2f95c9b9d234e602e1e075064`
- Original source SHA-256: `577adec41f3c1482892c3142d42af823b670886d81b6547514e19d86f4ce24d4`
- Corrected source SHA-256: `c6c513f61f916e3bc46474b348a0d877e00ae36f1067a1ebb397a33e009b65ba`
- Patch SHA-256: `81c9e2a56ec7d857cc13ab158508e4f5f4abd7dc1a720900f47af0820abe4eda`

## Bounded verification evidence

The producing worktree ran Rust/Cargo 1.98.1 with `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`, an isolated target directory and pinned toolchain/Cargo home. The same command ran before and after the one-line edit:

```sh
cargo check --locked -j 2 -p outbox-delivery --test process_recovery
```

- RED: exit 101, two E0283 diagnostics at line 306
- GREEN: exit 0, focused dev-profile check finished in 0.52s
- Producer also records `rustfmt --edition 2024 --check crates/outbox-delivery/tests/process_recovery.rs` and `git diff --check` passing
- Independent reviewer inspected the saved RED/GREEN outputs, exact patch, source bytes, schema/model and adjacent decoder, and reran the static diff check. The reviewer did **not** rerun Cargo
- RED output SHA-256: `4cf1eb8f912ba201811ffe307ffc273aa21b81be152d3d399d073af70093f6a9` (10,340 bytes)
- GREEN output SHA-256: `0ab5b3219badc4ece95cddaa0464a685199c0069ca7829a0d09c2397cb197107` (178 bytes)
- Earlier offline baseline stopped at missing cached arc-swap before compilation; normal locked dependency acquisition then completed without Cargo manifest/lock changes

No tests, DB server, worker subprocess scenario, parser/security tests, benchmarks, model execution or P3 qualification ran. Focused compilation is not G07 execution, G08 implementation, package-wide compile/test or whole-program qualification.

## Unresolved gates and current CI boundary

Original head `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a` completed hosted CI with failures: E0283 in this test; E0432 in `observability_security.rs:14` because `outbox_delivery::observe` does not exist; security scan findings; and DSI PoC's yanked `yoke-derive 0.8.3` advisory. SQLx was skipped and Rust test compilation stopped before execution. Sandbox, policy, container/SBOM, both macOS DSI parity variants and portability passed only at that prior head.

This correction addresses only the named E0283. Missing observe implementation, G07/G08 runtime evidence, P1 Office v2, P7 pending registration, all other capability acceptance and exact new-head hosted gates remain open. Gitleaks false-positive triage identified 28 verified source checksums and three synthetic local fixture matches; the exact-fingerprint proposal remains unapplied. The broader implementation safety hold and independent review requirement remain in force.

The initial `draft-source-manifest-20261001.json` remains immutable evidence for the initial published `99c7aca2e7ae3f1f60fa3b948b419a2ae5fc749a` snapshot. This note records the later source delta; do not treat the old source hash as the new file's hash. Excluded P3 hosted workflow/helper/test/proposal and generated/cache/model execution artifacts remain locally preserved and outside the PR.

**Exact next action:** verify the fast-forward Draft head and observe ordinary existing hosted CI for that exact new head, without claiming earlier passes transfer. Do not start remediation, parser/P3 execution, a broader implementation lane or scanner suppression from this compile-only receipt. No merge or deployment.
