# Search root dependency metadata recovery — 2026-10-03

## Scope and source identity

**Bounded dependency gate PASS; Search P1–P7 and whole-program acceptance remain incomplete.** The requester asked to continue Search on 2026-10-03. This independently reviewed recovery starts from live Draft PR #40 head `a945fbd32145a3109e35cb9cb056cea052698138`, tree `398edbe70174700aa55a2844dff8bd2033242582`, stacked on `80a47960d025e4dfdea1eacade28b15d218725ff`. The older restored worktree is preserved and is not the source of this patch.

The five configuration/lock files change only:

- Eight existing local path dependencies gain `version = "0.0.0"`, matching the referenced packages: one in `document-semantic-inspection-runner`, one in `search-extraction-core`, three in `search-extraction-runner`, and three in `search-extraction-worker`.
- Root `Cargo.lock` updates only `yoke-derive 0.8.3` to `0.8.4`, including its published checksum. All 558 package entries are retained; every other package field, dependency requirement, path and feature is unchanged.

No product source, test assertion, migration, runtime behavior, dependency policy, scanner exception, workflow or frozen semantic contract changes. The pre-existing exact31 scanner exceptions are unchanged.

## Package provenance

Fresh official [crates.io-index metadata](https://github.com/rust-lang/crates.io-index/blob/master/yo/ke/yoke-derive), observed Git blob `17a2065c79a44a3d7d4f658e96e467706a957070`, identifies `0.8.3` as yanked and `0.8.4` as not yanked at this check. The two versions have identical declared dependency requirements. Existing `yoke 0.8.3` permits `yoke-derive ^0.8.2`. The replacement retains the allowed Unicode-3.0 license; its Rust 1.82 minimum fits the pinned Rust 1.98.1 toolchain.

The cached official `yoke-derive-0.8.4.crate` archive matches SHA-256 `ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`, the checksum in the new lockfile. This is a real patch-version source update, not a claim that the package bytes are unchanged.

## Fresh verification

Tools: Rust 1.98.1, Cargo 1.98.1, cargo-deny 0.20.2, already-installed verified Linux toolchain. No project compilation or test executable was run in this checkpoint.

1. Baseline TOML inventory reproduced eight unversioned local path dependencies. The same inventory after the edit finds zero and validates the target package versions.
2. Initial offline targeted resolution stopped before changing the lock because the restored cache lacked the `sqlx` index entry. Normal crates.io metadata acquisition then completed `cargo update -p yoke-derive@0.8.3 --precise 0.8.4`. Parsed old/new lock comparison confirms the sole changed package fields are the intended version and checksum.
3. `cargo metadata --locked --no-deps --format-version 1` passes after repair. This is metadata validation, not a build or test.
4. The same dependency-policy command ran against an isolated clean baseline worktree and the candidate, with the same toolchain and unchanged `deny.toml`:

```sh
cargo deny --locked check --hide-inclusion-graph
```

- Baseline: exit **3**; eight wildcard dependencies across four diagnostics plus yanked `yoke-derive 0.8.3`; advisories/bans FAILED, licenses/sources OK.
- Candidate: exit **0**; advisories, bans, licenses and sources **OK**.
- Both retain 29 warnings: 26 duplicate-package warnings, two unused license allowances and one missing license-field warning for `ovba 0.7.1`. No warning or deny policy was suppressed.

Saved command outputs (only trailing spaces/tabs on individual lines are removed for repository whitespace policy; diagnostic text and ordering are unchanged):

- [Baseline log](dependency-metadata-before-20261003.log): 18,346 bytes; SHA-256 `027766ab5cf96740f028be5d157ae4e45283cc2c475a8b36902ecd0a999ba37c`. The unnormalized output was 18,398 bytes, SHA-256 `25244ded24b97038ff623a8fffdd440776f3eccbfc738645e45ef060ac5a7c2b`.
- [Candidate log](dependency-metadata-after-20261003.log): 15,286 bytes; SHA-256 `5fbe6264fbc595a91303aac2d2c9fadb6463b6cb6306bd82f2bf08d2d339e59c`. The unnormalized output was 15,338 bytes, SHA-256 `9a4a605e92a6cedb0023b5165cd03339ecefa58cbe1b611194bbad72045df221`.

The five-file diff received independent read-only GO for package identity, exact lock delta, official provenance, license/toolchain compatibility and unchanged policy. `git diff --check` passes. These dependency checks do not exercise package build scripts, proc macros or runtime behavior.

## Remaining gates and next action

Three separate experiment lockfiles still pin `yoke-derive 0.8.3`: `experiments/document-semantic-inspection/Cargo.lock`, `experiments/search-http-client-poc/Cargo.lock`, and `experiments/search-discovery-poc/Cargo.lock`. This root gate does **not** clear their dependency gates or the hosted DSI PoC workflow.

The existing `outbox_delivery::observe` implementation is still missing; G05/G06 fresh runner verification, G07/G08 execution and all capability/final acceptance remain open. Historical P1/P2/P7 receipts do not qualify this current complete tree. The unidentified earlier safety-stopped operation was not retried, and this metadata recovery does not declare that historical hold resolved or authorize a parser, database, model, process-recovery, security-probe or P3 qualification operation.

Next: publish only this independently reviewed bounded checkpoint through Draft PR #40 and inspect ordinary exact-head CI. Review the next explicitly bounded P6 gate against the frozen G05 → G06 → G07 → G08 sequence before execution; keep separate experiment lock repairs scoped and independently verified. No merge or deployment.
