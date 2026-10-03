# P1-I02 shared Linux sandbox and Search runner code receipt

- Branch / base at implementation: `feat/search-platform-completion-core` / `80a4796`.
- Owner: P1-I02 only. Root Cargo manifests and lock were prepared by the parent lane with existing qualified pins; this lane changed no manifest or lock entry.
- Status: source implementation and macOS fail-closed verification complete. Hosted Linux enforcement, Search failure matrix, and DSI Linux regression remain an exact-head qualification gate.

## RED and implementation

- Actual RED: `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-extraction-runner --locked --test runner_isolation --test runner_failure_matrix` exited 101 before production code. It reported missing `seal_worker_sandbox` (E0425) and missing `SandboxProcessRunner`, `SandboxRunConfig`, `SandboxRunErrorKind` (E0432). This is the test-first boundary for P1-I02.
- `document-sandbox-runner` now owns fresh child processes, read-only raw/trust FDs, private mode-0700 staging/scratch, cleared environment and inherited FDs, CPU/AS/file limits, 16 MiB stdout, 1 MiB stderr, 1 GiB scratch, ten-second maximum wall time, process-group kill on failure, and a worker-side mandatory Linux Landlock `FullyEnforced` plus seccomp seal.
- DSI `linux.rs` delegates the process mechanics and maps shared error kinds back into its existing public `RunnerError` cases. DSI `sandbox.rs` delegates the seal. DSI request/response protocol and worker failure code table remain in DSI.
- The Search host accepts only a trusted registered profile, independently validates request/raw size/SHA-256 and exact PDFium native binary SHA-256, launches one bounded worker, decodes the bounded DTO, and validates any successful report. The worker repeats raw/profile/native checks before sealing; it emits only retryable `WorkerUnavailable` until the qualified format readers are integrated in I03–I05. It cannot publish a successful body report in that state.
- For archive profiles, `used_leaf_chains` is omitted from canonical profile bytes by the existing codec. The worker derives the non-ZIP leaf chains from bounded canonical node frames, then calls the existing archive decoder and ID check before checking every PDF node pin. No reader code or parser library was added.

## Frozen failure classification

| Observation | Search result |
| --- | --- |
| Structured `ReaderFailure::Unsupported(ResourceLimit)` | Validated `Unsupported(ResourceLimit)`, zero Units |
| Structured permanent / retryable code | Typed `ExtractionError::Permanent` / `Retryable` using frozen codes |
| Trusted stdout / stderr cap | `Permanent(WorkerOutputLimit)` |
| Timeout | `Retryable(Timeout)` with no in-run retry |
| Worker kill, panic, process resource limit, unclassified exit | `Retryable(WorkerKilled)` |
| Missing Linux seal or native pin | `Configuration` incident |
| Host or worker raw SHA/size mismatch, truncated/invalid report | `Integrity` incident |
| macOS or other non-Linux host | Explicit unavailable; never qualification PASS |

## Verification observed on macOS

- Focused Search runner tests: 2/2 non-Linux fail-closed cases passed. Linux-only adversarial assertions are present in `runner_isolation.rs` and `runner_failure_matrix.rs` but are cfg-skipped here.
- `cargo test -p document-semantic-inspection-runner -p search-extraction-core -p search-extraction-worker --locked`: DSI baseline 1/1, Search Core 11/11, worker build passed; DSI Linux isolation was cfg-skipped.
- `cargo test -p search-extraction-worker --locked`: worker main compiled after archive/native pin change.
- `cargo clippy -p document-sandbox-runner -p search-extraction-core -p search-extraction-runner -p search-extraction-worker -p document-semantic-inspection-runner --all-targets --locked -- -D warnings`: passed.
- Scoped `rustfmt --edition 2024 --check`: passed.
- Disk at final local checks: about 1.7 GiB free, above the 1.5 GiB stop floor. No local Linux Docker build was attempted; its known Landlock `NotEnforced` result cannot qualify isolation.

## Exact-head gate still required

Run both Search runner integration tests plus DSI `runner_baseline` and `runner_isolation` on hosted Linux where Landlock is `FullyEnforced`. The tests exercise forbidden outside reads, network/spawn, inherited FDs and environment, missing seal, timeout, output/scratch limits, worker kill/panic, partial/truncated output, typed failure map, raw binding, and wrong native pin. Record the hosted job URL, commit SHA, and control status before changing this receipt to Linux-qualified.

## Source SHA-256 after local checks

| File | SHA-256 |
| --- | --- |
| `document-sandbox-runner/src/lib.rs` | `b93691aa4e0d5abbe09bb77f57eecfd1593320a4f8e66567fcb6d6ad3bf17b57` |
| `document-sandbox-runner/src/linux.rs` | `5e1e954fb9edf92ee9d788f8d26aaa14b21eb20c9b9af649c2aeba2896cf2381` |
| `document-sandbox-runner/src/process.rs` | `a10965804fd121bfa45716e2a77b9cbf8f5c750352ce278052fbb02b30fb94ab` |
| `document-sandbox-runner/src/sandbox.rs` | `5e08013371f2ae1fd9c8319ba9520c1278f909a373fd8da975d5253b1d6e7a36` |
| `search-extraction-runner/src/executor.rs` | `fc1f87797f99bd83247de746da633ab946b258810c3afc39aad4060a28e3ee8a` |
| `search-extraction-worker/src/main.rs` | `9cf464dbfbd903f3b1a2b33b1dfc79a6e8e1eca36ea638ffa186aa1e5ad3b26d` |
| `search-extraction-core/src/validation.rs` | `72057087a87a9baa17dcd8f4b91cde5561a8a6dd8fd9195bf879005b15c28453` |
| `document-semantic-inspection-runner/src/linux.rs` | `b100403fa9726126e6ad70a3ec37cb8f795cce3f10476f22a123c68fc2529b33` |
| `document-semantic-inspection-runner/src/sandbox.rs` | `0a679a92a9cbe6ced4190dd3dc4e73f05c4016e43463b24c623b18fa4a5bda48` |

The initial DSI SHA-256 values were `lib.rs=726ae699a78813b8de2758a09fd4c301abdee9526d366849398d7e6cfe776d14`, `linux.rs=03b5e724f346f6a2f477265adc872fee76e967355e672decfdb611fbe2014ef6`, and `sandbox.rs=33c3d2105d6c4508b0d0f5443e603c29c37119847bed3006b9f2d2ee2226c81b`.
