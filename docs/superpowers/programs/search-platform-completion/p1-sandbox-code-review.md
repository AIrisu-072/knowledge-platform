# P1-I02 shared sandbox / Search runner — independent code review

- 判定: **NO-GO（macOS 上の bounded source review）**。下記の失敗分類 3 件を修正して再監査する。Linux sandbox qualification は別の **未実施 gate**。
- 対象: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の dirty source。2026-09-30T22:59+0900。`p1-extraction-freeze.md`、`p1-extraction-plan.md` §P1-I02、`p1-extraction-design-revision-1.md` §6 を判定基準にした。実装者 receipt `p1-sandbox-code.md` は主張の照合にのみ使用。
- 本監査は read-only のコード・既存 binary 検証で、Cargo rebuild、Docker、Linux 実行、hosted CI は行っていない。I03–I05 の reader 不在中、実 Search worker が `WorkerUnavailable` を返すのは期待どおりの fail-closed stub であり、形式別抽出完了を意味しない。

## 要修正

1. **[P1] ZIP の構造化 `Unsupported(ResourceLimit)` が `Integrity` に変わる。** `crates/search-extraction-runner/src/executor.rs:143-153` は `ReaderFailure::Unsupported` を Unit 0 の report に変換する際、`reader_use=[]` を入れる。一方、`crates/search-extraction-core/src/validation.rs:333-340` は archive report の `reader_use` と登録済み `plan.nodes` の完全一致を要求し、archive plan は `validation.rs:71-80` により root node を必ず持つ。したがって ZIP reader が parse 中の確定 `ResourceLimit` を返しても `execute` は `Ok(Unsupported)` にならず `Integrity("archive reader-use")` で止まる。凍結 §6 の `Completed + Unsupported(ResourceLimit), Unit 0` に反する。trusted archive plan を使う失敗経路の検証を分け、ZIP の構造化失敗を対象 test に入れる必要がある。現行 `runner_failure_matrix.rs:82-98` は Text だけを検証している。
2. **[P1] 既知の worker protocol failure が retryable `WorkerKilled` に化ける。** `crates/search-extraction-worker/src/main.rs:37,57,76-83,121` は request decode / archive profile decode・ID 不一致 / response encode の `Failure::Protocol` を exit 79 にする。`crates/search-extraction-runner/src/executor.rs:236-249` は 76–78 だけを判別し、79 を `Retryable(WorkerKilled)` に落とす。固定 raw/profile や worker/host protocol 不一致を一時的な kill と扱うと、§6 の integrity/config incident と再試行抑止境界が崩れる。79 の明示分類と、その経路を通る test が必要。
3. **[P2] 不正な worker path が item の retryable availability になる。** `SearchRunnerConfig::new` / `SearchExtractionRunner::new` (`crates/search-extraction-runner/src/executor.rs:30-36,55-84`) は worker path を検証しない。相対 path は `crates/document-sandbox-runner/src/process.rs:53-55` で `Unavailable`、`executor.rs:232-234` で `Retryable(WorkerUnavailable)` になる。絶対 path でない設定は host configuration incident として constructor で拒否すべきであり、既知の一時的 availability だけを retryable にする。DSI は既に `crates/document-semantic-inspection-runner/src/linux.rs:17-32` で path を検証する。

## 静的に確認した境界と残る実測

- `document-sandbox-runner/src/process.rs:69-119,135-175,208-249,261-349,357-410` は private staging/scratch、0400 raw/trust file、`env_clear`、明示 FD 3/4 と他 FD の `CLOEXEC`、CPU/AS/file rlimit、stdout 16 MiB・stderr 1 MiB、scratch 1 GiB の監視、最大 10 秒と失敗時の process-group kill を実装する。これはコード上の制御確認であり、Linux kernel 上の拒否結果ではない。
- `document-sandbox-runner/src/linux.rs:16-95` は DSI/Search marker を排他的に要求し、single thread、Landlock V3 `HardRequirement` と `FullyEnforced`、seccomp syscall deny を順に要求する。`sandbox.rs:7-15` と `lib.rs:143-175` は非 Linux を明示的に拒否する。[landlock 公式 crate docs](https://docs.rs/landlock/latest/landlock/) も `FullyEnforced` の実行時確認を求める。hosted Linux で拒否と enforcement status を実測するまで PASS としない。
- `search-extraction-runner/src/executor.rs:113-161,165-221` は登録 profile、raw size/SHA、PDF node pin と実 `libpdfium.so` SHA を host で確認し、bounded response を decode/validate する。`search-extraction-worker/src/main.rs:48-115` は raw/profile/native を再確認してから seal を呼び、形式 reader を呼ぶコードはまだない。archive の canonical leaf は worker 側で再構成されている。source raw/locator の最終 round-trip と body publication は後続 task の gate。
- DSI `linux.rs:81-121` と `sandbox.rs:5-9` は共有 process/seal に委譲し、既存 DSI `WorkerResponse` と failure code table は `linux.rs:124-171` に残る。shared code は旧 DSI process/seal の定数・制御をほぼ移したが、DSI **Linux** isolation regression はここでは未確認。

## 実行証拠と資格境界

| 確認 | 結果・制約 |
| --- | --- |
| 現在 source より新しい cached `target/debug/deps/runner_isolation-1a4883c7367f30ae` SHA `5f186eadf85e860afbc67e1328fd6f252959b956cbfdcd41ee15dc01043933fe` | 2026-09-30 22:31:34+0900、macOS 非 Linux 1/1 PASS。Linux test は cfg 非実行。 |
| 現在 source より新しい cached `target/debug/deps/runner_failure_matrix-fee9d24628177aa7` SHA `81cf319adb9adfc7816369c26c293d018af7a367ff7f14cdf7a8e14be5978ebc` | 22:31:35、macOS 非 Linux 1/1 PASS。Linux typed matrix は cfg 非実行。 |
| cached `target/debug/deps/extraction_contract-e91896494a738c73` SHA `8aeb43299ab1c39300826ae09c853cc9dfc920b038299818215d1ea5fffec8ea` | 22:26:15、該当 Core source より新しい。Search Core 11/11 PASS。Search runner の ZIP failure 経路は含まない。 |
| cached `target/debug/deps/runner_baseline-8685323daecd5be4` SHA `9e5b3a9f231248b31ee96bf8b38e0d229242339c76f4d14cbe9caed18431e9d3` | 22:26:15、DSI hostile fixture shape 1/1 PASS。共有 lib source `document-sandbox-runner/src/lib.rs` は 22:28:08 に更新されており、共有 runner の fresh regression 証拠にできない。`runner_isolation-bce9b540b6f688c7` も同じ 22:26:15 の旧 binary である。 |

実装者 receipt の RED と strict Clippy は照合したが、この監査では再実行していない。disk 空きは監査時 2.2 GiB（1.5 GiB stop floor 超）で、build を開始していない。実装 dirty source は foundation Draft PR #34 `80a4796` の hosted SUCCESS 対象外。**修正後の exact-head hosted Linux** で Search `runner_isolation` / `runner_failure_matrix` と DSI `runner_baseline` / `runner_isolation` を実行し、Landlock `FullyEnforced` と seccomp 拒否、FD/env、timeout/output/scratch/process kill、PDFium pin、実 Search worker の seal/stub 応答を記録するまでは Linux qualification を保留する。

## Exact source/test SHA-256

下表の 22 ファイルを記載順に `shasum -a 256` した path 付き出力の aggregate SHA-256 は、監査開始時・終了時とも `2e60ce00b312b8388c0be42478020d3292a2efe5a7886193e7fdbb607db1e988`。

| File (`crates/` 以下) | SHA-256 |
| --- | --- |
| `document-sandbox-runner/src/lib.rs` | `b93691aa4e0d5abbe09bb77f57eecfd1593320a4f8e66567fcb6d6ad3bf17b57` |
| `document-sandbox-runner/src/linux.rs` | `5e1e954fb9edf92ee9d788f8d26aaa14b21eb20c9b9af649c2aeba2896cf2381` |
| `document-sandbox-runner/src/process.rs` | `a10965804fd121bfa45716e2a77b9cbf8f5c750352ce278052fbb02b30fb94ab` |
| `document-sandbox-runner/src/sandbox.rs` | `5e08013371f2ae1fd9c8319ba9520c1278f909a373fd8da975d5253b1d6e7a36` |
| `search-extraction-runner/src/lib.rs` | `e5fa854f1ed86ca9ccb7a0ea5906dac5f321defd2c5cbdf75e541d8689222387` |
| `search-extraction-runner/src/executor.rs` | `fc1f87797f99bd83247de746da633ab946b258810c3afc39aad4060a28e3ee8a` |
| `search-extraction-runner/tests/runner_isolation.rs` | `5e41549d8baf15278428f29cc353d752651fdeaf478709c108a0504fc2a70fb3` |
| `search-extraction-runner/tests/runner_failure_matrix.rs` | `320fc007cca7e676d5bed5e1efa50a46844051dd8c8d2fbd6595ab9c9b65ea4e` |
| `search-extraction-runner/tests/support/hostile_worker.rs` | `44b10ec8e37d8f69942312529629e3b894d84da487bf7edf5d480015bd936e1d` |
| `search-extraction-worker/src/lib.rs` | `605a5fa76c4aaebe2714da851e5cff7d364a9aa123e6a77dedea2c53ec6382ed` |
| `search-extraction-worker/src/main.rs` | `9cf464dbfbd903f3b1a2b33b1dfc79a6e8e1eca36ea638ffa186aa1e5ad3b26d` |
| `search-extraction-core/src/protocol.rs` | `32eac05bf775c321a1ca6b3b89275aa29114e3cfa270929dd1fe2740d04b0dea` |
| `search-extraction-core/src/coverage.rs` | `540dff852cf51f6aab97584ab9a37aa10e0bd4c52b80631677c9266f9062eb0b` |
| `search-extraction-core/src/budget.rs` | `32ccfe120a95585d438040d562a4c85fbfd1b66068063202fa7738f1a0c57ff0` |
| `search-extraction-core/src/validation.rs` | `72057087a87a9baa17dcd8f4b91cde5561a8a6dd8fd9195bf879005b15c28453` |
| `search-extraction-core/tests/extraction_contract.rs` | `8038d4ae431708be20392eeb468f7c2a7172bb085534efd5ecebec95d08f7786` |
| `search-core/src/knowledge_unit/profile.rs` | `369c48bfa1c6cd73ac52ed2e10f18b08dfb1fff0f63613522c7fef1c36cd6aef` |
| `document-semantic-inspection-runner/src/lib.rs` | `9f33094ab320d12b8c26ca87662c94d371c124142bd4f861510baca60ad7965f` |
| `document-semantic-inspection-runner/src/linux.rs` | `b100403fa9726126e6ad70a3ec37cb8f795cce3f10476f22a123c68fc2529b33` |
| `document-semantic-inspection-runner/src/sandbox.rs` | `0a679a92a9cbe6ced4190dd3dc4e73f05c4016e43463b24c623b18fa4a5bda48` |
| `document-semantic-inspection-runner/tests/runner_baseline.rs` | `213300760e081430462871acdcda482dfd74555c0d9ba4d5786f1104f63cc942` |
| `document-semantic-inspection-runner/tests/runner_isolation.rs` | `5af9899f19ccf576d1b4834aad94feb783ca75459d9ed2371634824f7259de74` |
