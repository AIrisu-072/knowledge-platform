# P1-I01 pure wire contract — independent review

- 判定: **NO-GO（限定 1 件）**。`Partial(ResourceLimit)` の受理を閉じた後に P1-I01 pure validator を再判定する。reader の production 採用、raw parser fidelity、host の Source binding、Linux sandbox enforcement、P1 全体の完了はこの判定の対象外。
- 対象: `search-extraction-core` の protocol / coverage / budget / validation、`document-sandbox-runner`・`search-extraction-runner`・`search-extraction-worker` の bootstrap、root workspace / lock。基準は composed `p1-extraction-freeze.md`、full `p1-extraction-plan.md` P1-I01、`p1-wire-split-ruling.md`、revision §6、最小 Unit / Archive profile 契約。
- 入力同一性: `protocol.rs` `32eac05bf775c321a1ca6b3b89275aa29114e3cfa270929dd1fe2740d04b0dea`、`validation.rs` `1df9efcf9de5f8448ea7dac3a35eaf8a7b27e4c4d492aa5b43d99941116248de`、`budget.rs` `32ccfe120a95585d438040d562a4c85fbfd1b66068063202fa7738f1a0c57ff0`、`extraction_contract.rs` `89a3ab61e113b85c86b650832e7473b2077fdf553b68a1f07dcfa9054fb33481`、root `Cargo.toml` `6bd7d132cfb539579c54e03d1b48ab1a456a547fd77ad7782bb59ec9aa3df1e1`、`Cargo.lock` `bdd91022f766115f809bb795951ca4c354ce461c8305d561054ebc8fbe628e78` は `p1-wire-code.md` の値と一致。root では他 lane の local crate も同時に増えているため、root 差分全体を P1 固有変更とは帰属しない。新規 registry package は lock 差分になく、対象 4 crate の manifest に新 reader dependency はない。
- 実行証拠: 既存 `target/debug/deps/extraction_contract-7cf722cedfa9d882 --test-threads=1` は **8/8 PASS**。この binary は既存 build の実行であり、今回の fresh compiler / `cargo --locked` / CI 結果ではない。対象差分の `git diff --check` は PASS。receipt の RED、Clippy、metadata は今回再実行していない。

## Blocking finding

**[P1] `ResourceLimit` を `Partial` として通す。** `crates/search-extraction-core/src/validation.rs:271-300` は `Partial` の非空 Unit / omission / reason 集合一致を検査するが、`CoverageReason::ResourceLimit` を禁止しない。例えば有効な `Text` fragment 1 件、`scope_items=1`、`traversal_complete=true`、`NativeOmission { package_path: None, physical_child_path: vec![3], reason: ResourceLimit }`、`Partial { reasons: [ResourceLimit] }` は現行の純粋 validator 条件を満たす。既存テスト `extraction_contract.rs:125-155` は `UnsupportedStructure` の Partial だけを正例にしており、この逆例を固定していない。

凍結 revision `p1-extraction-design-revision-1.md:184,191` は parse 中の hard budget 到達を `Completed + Unsupported(ResourceLimit)`・**途中 Unit 全破棄**と定め、処理中断を `Partial` にしない。Partial の検証 Unit は後続の限定肯定 claim に使えるため、この型の誤分類は publication boundary に影響する。`traversal_complete` は worker の申告値であり、単独では hard budget 未到達の証明にならない。

**修正条件:** `validate_worker_report` で `Partial` の `ResourceLimit` を拒否し、同じ構造の adversarial contract test を追加する。hard budget は `ReaderFailure::Unsupported(ResourceLimit)` から Unit 0 に写像する既存経路を維持する。修正後、同一 focused test の fresh GREEN と独立再確認を行う。

## 確認できた範囲と残る境界

全 15 `BudgetKey` の存在・絶対 ceiling、aggregate と peak/per-value の区別、profile ID/definition/budget の request binding、Archive `reader_use` と登録 plan の構造的一致、response 上限・未知 tag・trailing/truncated byte の拒否はコードと 8 件の contract test に整合する。`WorkerRequest` に Source / principal / StorageKey / FileObject ID / credential はなく、typed failure と incomplete report を success に変換する API もない。

この pure validator は raw 上の実際の reader-visible scope、NativeOmission の物理位置、reader の実行設定、locator / text round-trip、parser / PDFium の配備 hash を証明しない。`Supported` の完全性と `Partial` の既知 omission は P1-Q01 / I03–I05 と trusted host の再検査が必須であり、worker の `traversal_complete` や `reader_use` 申告だけで production `Supported` を許可しない。P3 dual encoder selection も未受入であり、program 全体の production READY は宣言しない。
