# Search experiment lock metadata recovery — 2026-10-03

## Scope and source identity

**Three bounded static dependency gates PASS; no PoC or program acceptance claim.** This follow-on repairs exactly the three experiment locks left open by the [root dependency checkpoint](dependency-metadata-recovery-20261003.md). It is a Search-branch change, not an edit to separate Document PRs #36–43.

The isolated sibling worktree starts from local commit `bb6cd56b1b3f587438b77527cabba1ba8da3792c`, tree `2df2784e8017f1433a2fb96d8054e4aca3478299`, matching published Search Draft [PR #40](https://github.com/AIrisu-072/knowledge-platform/pull/40) head `401b31047a64ed76c470477c6db15fc7e8221d2d`. The existing recovery worktree is preserved. The parent granted an exclusive metadata slot after its separate G06 operation ended; this task used the slot only for Cargo metadata/resolution and cargo-deny. The slot was released after the scans and metadata checks ended.

Only these package records change: `yoke-derive 0.8.3 → 0.8.4`, checksum `33811428bee40dbceb6d545e95754741d17a6aef9a4849f0fd62e2ba4f412a78 → ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`, in each of:

- `experiments/document-semantic-inspection/Cargo.lock`
- `experiments/search-http-client-poc/Cargo.lock`
- `experiments/search-discovery-poc/Cargo.lock`

Parsed lock comparisons retain all 337, 200 and 376 package entries respectively. In each file exactly one package differs, and exactly its version and checksum differ. A separate byte-for-byte comparison with the baseline plus those two literal replacements passes. Every other package field, dependency, source, version and feature is unchanged. All manifests, deny policies, root lock, source, tests, workflows, scanner exceptions and frozen contracts remain unchanged. Active/status files are intentionally left to the parent checkpoint owner.

## Official provenance and policy

Fresh official [crates.io-index metadata](https://github.com/rust-lang/crates.io-index/blob/master/yo/ke/yoke-derive), observed Git blob `17a2065c79a44a3d7d4f658e96e467706a957070` on 2026-10-03, marks 0.8.3 yanked and 0.8.4 not yanked. Both declare the same dependency requirements and features. Existing yoke 0.8.3 permits yoke-derive ^0.8.2. The cached official 0.8.4 archive has SHA-256 `ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71`, matching the index and all three candidate locks. Unicode-3.0 is permitted by each unchanged deny policy; the replacement's Rust 1.82 minimum fits Rust 1.98.1. This is a real patch-version source change, not a claim that crate bytes or runtime behavior are identical.

The applicable policy remains `spec/selection/library-tool-selection-v0.md` §§2.1–2.2, 20–21 and 24, `spec/architecture/dependency-rules.toml`, and each experiment's existing `deny.toml`. No dependency promotion or policy exception is introduced.

## Exact command evidence

Verification window: 2026-10-03 05:35–05:37 UTC. Existing verified tools: Cargo 1.98.1 (`797e8a9bc 2026-08-05`) and cargo-deny 0.20.2. Commands ran at the isolated worktree root with:

```sh
export PATH=/workspace/scratch/13897606dfde/organization-tauri-toolchain/rust/bin:/workspace/scratch/13897606dfde/organization-tauri-toolchain/deny/cargo-deny-0.20.2-x86_64-unknown-linux-musl:$PATH
export CARGO_HOME=/workspace/scratch/13897606dfde/organization-tauri-toolchain/cargo-home
export CARGO_TERM_COLOR=never
```

For each literal `p` value listed above without `experiments/` or `/Cargo.lock`, these commands were run in order. All three baseline scans preceded the updates; each update's exact delta was checked before proceeding; then all three candidate scans and metadata checks completed:

```sh
cargo deny --manifest-path "experiments/$p/Cargo.toml" --config "experiments/$p/deny.toml" --locked check --hide-inclusion-graph
cargo update --manifest-path "experiments/$p/Cargo.toml" --offline -p yoke-derive@0.8.3 --precise 0.8.4
cargo deny --manifest-path "experiments/$p/Cargo.toml" --config "experiments/$p/deny.toml" --locked check --hide-inclusion-graph
cargo metadata --manifest-path "experiments/$p/Cargo.toml" --locked --offline --no-deps --format-version 1
```

| Experiment | Baseline deny | Targeted update | Candidate deny | Locked metadata | Retained warnings |
|---|---:|---:|---:|---:|---|
| document-semantic-inspection | exit 1 | exit 0 | exit 0 | exit 0 | 23 duplicate, 2 unused license allowances, 1 ovba missing-license-field |
| search-http-client-poc | exit 1 | exit 0 | exit 0 | exit 0 | 1 duplicate |
| search-discovery-poc | exit 1 | exit 0 | exit 0 | exit 0 | 6 duplicate, 1 unused license allowance |

Each baseline has exactly one error, yanked yoke-derive 0.8.3, and terminal `advisories FAILED, bans ok, licenses ok, sources ok`. Each candidate has no errors and terminal `advisories ok, bans ok, licenses ok, sources ok`. Before/after warning message multisets are identical. No warning is suppressed. The first shell wrapper expected the root checkpoint's exit 3 and stopped after the DSI baseline exit 1; the full diagnostic showed an advisory-only failure, so the remaining baseline scans proceeded expecting exit 1. No scanner failure was hidden and no repeat of the DSI baseline was needed.

Every metadata command reports the corresponding 0.0.0 experiment package and leaves its lock SHA-256 unchanged. Metadata and dependency scans do not compile the project, run build scripts/proc macros, execute parser/PoC/model/database/process/security probes or use an alternate runtime harness. `git diff --check` passes after normalizing only trailing spaces/tabs in stored diagnostic logs.

## Lock identities

| Lock | Baseline SHA-256 | Candidate SHA-256 |
|---|---|---|
| document-semantic-inspection | `3e4419f233ccd59fc860d2bbbc2c1b65982f78de0a9a7dffee12194eb409bd31` | `b2cdbd9de4c26023c790a1708ff68f56f8070783caa81f89262694a50d42001f` |
| search-http-client-poc | `6eeb2322981274c59d2973eae41035db33f5cfa2ee6afe72767c1755be877cca` | `1e953759a16c58295c375976c50804430e3a985b288046bac6f9be8d2e95a905` |
| search-discovery-poc | `1c15005b6c35146a2d462737d04eb99828d72ddbc14f9b8bf8c735024e559e0d` | `2751f39b33e7c1b939a46b1136580ec72c633113158e0487b8b64796b7066b28` |

## Saved diagnostic logs

The following logs retain the complete output and diagnostic ordering. Only trailing spaces/tabs on each line were removed for repository whitespace policy. Update logs were already normalized.

| Log | Bytes | SHA-256 |
|---|---:|---|
| [poc-lock-document-semantic-inspection-after-20261003.log](poc-lock-document-semantic-inspection-after-20261003.log) | 14570 | `2fbb5a5533818532f6ad4de8ba550a1f7baee2ba6384cc8d5f2ce84c182c3ce3` |
| [poc-lock-document-semantic-inspection-before-20261003.log](poc-lock-document-semantic-inspection-before-20261003.log) | 15103 | `eb395d93fc9734b28c39739f0db80044b599345b4d03767e2113419ae9737a48` |
| [poc-lock-document-semantic-inspection-update-20261003.log](poc-lock-document-semantic-inspection-update-20261003.log) | 112 | `f01f656b0256ac4fc05481b4dc7b193c195d6c31d1cbff51911fef22c20d5618` |
| [poc-lock-search-discovery-poc-after-20261003.log](poc-lock-search-discovery-poc-after-20261003.log) | 4129 | `3bcf6eb5fc180ba48133d2a5f49d492c4fe96b92ad538448c19931faab8b941e` |
| [poc-lock-search-discovery-poc-before-20261003.log](poc-lock-search-discovery-poc-before-20261003.log) | 4654 | `39518407177026991129320288cfc0dd35a06122e72a75ed2e2d94a0106ba5ce` |
| [poc-lock-search-discovery-poc-update-20261003.log](poc-lock-search-discovery-poc-update-20261003.log) | 111 | `0111ea3643b446081e118e02ab27d857977173f35dad8017b6750f9e0f7bdfd0` |
| [poc-lock-search-http-client-poc-after-20261003.log](poc-lock-search-http-client-poc-after-20261003.log) | 614 | `71ace229d36888eb6d8d1411f73a8dba60f771a5ce6fe2b12acdfa3a6b01b2f1` |
| [poc-lock-search-http-client-poc-before-20261003.log](poc-lock-search-http-client-poc-before-20261003.log) | 1141 | `a544d93e2bb2e222b444ddfa5b433a481bf2f59c39f1346ff56f56a00726f35f` |
| [poc-lock-search-http-client-poc-update-20261003.log](poc-lock-search-http-client-poc-update-20261003.log) | 111 | `016587140e84640072a450158a0f201f1d831b146b57a7f472aa523bf08e422f` |

Unnormalized scan output identities, before trailing-whitespace normalization:

| Experiment / phase | Bytes | SHA-256 |
|---|---:|---|
| document-semantic-inspection / before | 15149 | `ee29e19e61ba282e5735c53c6c8d7b67b7873eda2c41817dd88f9eb821c161f1` |
| document-semantic-inspection / after | 14616 | `9e6b19eb53d1dee805c260dc096ba2444280f5a204aa01224addbdbbb50a3c22` |
| search-http-client-poc / before | 1143 | `8e1036e7ccdbddec0bc3ecbd79e8c27a6ada79e25badbbfc7b758edeaa3f0164` |
| search-http-client-poc / after | 616 | `d384814fd4891687f3175a81e44f4c248f08ba97087d32d4e4c55949cccf972c` |
| search-discovery-poc / before | 4666 | `fd900ef75afb1d6ff0b0041d63c1b2adadc6d01d0b6727c7f1b195727b183175` |
| search-discovery-poc / after | 4141 | `62422a67c088ce5195e61b564578222fbec51cffe103f5f06bc7a008153fa02f` |

## Remaining independent gates and next action

This clears only the three local default-feature static dependency scans. It does not qualify the changed proc-macro source, any experiment's behavior, optional feature combinations, DSI hosted qualification, dictionary/native assets, the full root tree or any P1–P7/final acceptance. No project `build`, `check`, `test`, `clippy`, aggregate verification, parser, PoC, model, database, process-recovery, security-probe or P3 qualification ran in this task.

At the live base head check, PR #40 remained OPEN/Draft; Sandbox run 37099916942 had succeeded, DSI PoC run 37099916959 had failed, and CI run 37099916947 was still in progress. Those base-head results do not qualify this unpublished candidate. The missing observe implementation and remaining P6/final gates are owned separately; this receipt neither advances nor contradicts any separate G05/G06 verification. The unidentified historical safety-stop remains unresolved; this bounded metadata repair does not authorize retrying it.

Next: independent read-only review of the exact three-lock delta and this receipt/log set, then parent-owned publication/checkpoint reconciliation and ordinary exact-head hosted CI observation if approved. This task has made no remote publication, workflow dispatch, merge or deployment. The root receipt's historical three-lock blocker is superseded only by this bounded evidence, not rewritten into whole-PoC success.
