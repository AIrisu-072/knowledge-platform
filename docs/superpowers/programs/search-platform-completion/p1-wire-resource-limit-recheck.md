# P1-I01 ResourceLimit pure wire contract — independent recheck

- 判定: **GO（P1-I01 の pure coverage / validation 契約のみ）**。前回 `p1-wire-code-review.md` の単一 blocking finding は、指定した 2 ファイルの現在内容について解消した。
- 対象: `validate_worker_report`、`checked_completed_report`、`ReaderFailure::item_outcome` と focused contract tests。基準は `p1-extraction-freeze.md`、`p1-extraction-plan.md` P1-I01、`p1-wire-split-ruling.md`、`p1-extraction-design-revision-1.md` §6。worktree は `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の dirty state。

## 独立確認

1. `crates/search-extraction-core/src/validation.rs:277-296` は非空 Unit・既知 omission・理由集合の整合に加え、`Partial` の理由に `ResourceLimit` が一つでも含まれれば拒否する。`UnsupportedStructure` 単独を受理し、`ResourceLimit` 単独と混合を拒否する adversarial test は `crates/search-extraction-core/tests/extraction_contract.rs:157-191`。`checked_completed_report` も同じ validator を通る（`validation.rs:341-350`）。
2. `resource_limit_failure()` は `ReaderFailure::Unsupported(ResourceLimit)`（`validation.rs:353-356`）。`ReaderFailure::item_outcome` は `Completed + Unsupported(ResourceLimit)` に写像し、`Failure` を completed report として受け取らない（`protocol.rs:73-85`, `extraction_contract.rs:193-210`）。`Unsupported` report に途中 Unit が残る場合は拒否し、Unit 0 で受理する（`validation.rs:298-301`, `extraction_contract.rs:212-219`）。これは hard budget の pure outcome を確認したもので、実 reader が途中 Unit を破棄することの実証ではない。
3. `Partial(MissingFormulaCache)` の検証済み spreadsheet fragment と既知 omission は引き続き受理される（`extraction_contract.rs:221-241`）。前回の `Partial(UnsupportedStructure)` 正例も維持される（`extraction_contract.rs:125-155`）。従って修正は既知構造／formula cache の限定肯定経路を閉じていない。

## 実行証拠と入力同一性

- 2026-09-30 JST、`target/debug/deps/extraction_contract-7cf722cedfa9d882 --test-threads=1` をこの review で再実行し、**11/11 PASS、exit 0**。上記 adversarial 3 件を含む。Cargo は S01 exclusive のため実行していない。
- 再実行 binary SHA-256 `155122d38d991de1f3d3dd583e300b5f8858345f69752d0c5e60efaa0c78f21a`、mtime `2026-09-30T20:19:27+0900`。source mtime は `validation.rs` `20:19:08+0900`、`extraction_contract.rs` `20:17:07+0900`。binary は両 source より後に更新されたが、mtime だけではその source を compile したことの暗号学的証明にならない。今回の独立 fresh compile / strict Clippy / CI は行っていない。
- 開始時の SHA-256: `validation.rs` `e598dd93ef175ea68b6a11cddb4e14464e059baf0a1c383ac444b2a8f7852f11`、`extraction_contract.rs` `8038d4ae431708be20392eeb468f7c2a7172bb085534efd5ecebec95d08f7786`。`protocol.rs` `32eac05bf775c321a1ca6b3b89275aa29114e3cfa270929dd1fe2740d04b0dea`、`coverage.rs` `540dff852cf51f6aab97584ab9a37aa10e0bd4c52b80631677c9266f9062eb0b`、`budget.rs` `32ccfe120a95585d438040d562a4c85fbfd1b66068063202fa773b8f1a0c57ff0`。
- 設計入力 SHA-256: `p1-wire-code-review.md` `20939f6d30d37327c34b2cb23bbdd92a09899a90ac8d02444927d7c0cadd718e`、`p1-wire-split-ruling.md` `90a050ab6aa681f0a314e582327d5e3d34b7f0f18b19d120f5bfc343356af8d7`、`p1-extraction-plan.md` `8a02c655199b974d8bb3dfe865bc91138908f09aba1f57961c058c1a5cab85ec`、`p1-extraction-freeze.md` `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d`、revision `b1e84e476f614eb30e3bd1c5982e7e0ff2a35e33aef9cddd069618315dcbe742`。開始・終了時で上記 source/test/protocol/coverage/budget/設計入力群と executable の hash が一致した。
- `p1-fix-wire-resource-limit-receipt.md` は review 中に作成され、SHA-256 `09f4d025e2f363413355a6d9bae9080e168c584aded4bdf6cda02f2c3d838888`。receipt の対象 2 file hash は本 review の開始・終了値と一致。修正 worker の adversarial RED（旧受理、exit 101）→11/11 GREEN と strict pure-crate Clippy は parent receipt に記載されているが、この独立 review では historical RED・fresh compiler・Clippy を再実行していない。

## 判定の境界

この GO は pure report validator と typed failure の局所判定だけを開く。raw/locator の再読取、reader の実 hard budget 中断、Linux sandbox enforcement、Source binding、publication / absence、P1 production 完了、hosted CI は対象外。設計変更は提案しない。
