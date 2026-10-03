# P2 L/LG repaired baseline — independent F1–F3 recheck

- 判定日: 2026-09-30 JST。対象は `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未 commit PoC 入力を固定した `baseline-refinement-run-manifest.json` と `baseline-refinement-run.log`。旧 `report.md` / `baseline-run.log` の数値は採用しない。
- **判定: GO（合成 L/LG baseline を、同一入力・同一条件の実モデル比較の基準に使うことに限る）。** 先行レビューの F1–F3 について、この範囲の blocking 指摘は解消した。dense/Vector arm 自体の測定、P1 本文実接続、P2 の production 採択は未完了である。

## 独立照合した実行 receipt

| 項目 | 照合結果 |
| --- | --- |
| 実入力 | `p2-l-lg-pinned-inputs-v1`、到達可能なローカル 6 package / 431 files と継承元 root `Cargo.toml`。記録済み aggregate SHA-256 `e491c50e915deca95756065f673b5c5ab875c724e391b00819840926c3a74de1` を manifest 内容から再計算し、各 file/tree hash と照合した。監査時の該当現行ファイルも記録値に一致した。 |
| 保存物 | manifest SHA-256 `605113e14ce8d69755014a784b056223939625288db31bea3ac1d1994cfd6d22`、run log SHA-256 `c5f04793d5d48fb6f540f2e6d68b40e6d4d3cc38df378d269f8b46bd33c05257`、PoC `Cargo.lock` SHA-256 `8d5bdb829c0d5cd322c730d169087cad1f76cefb80edc83c0b7a18660705c81f`。log hash は manifest と一致。 |
| 実行 phase | `focused_tests`、`baseline`、`focused_clippy` は順に exit 0。各 phase の before/after aggregate は上記 SHA で一致。保存ログは Rust 13/13、PoC strict Clippy 完了を示す。 |
| 監査時の再検証 | `python3 -B -m unittest discover -s scripts -p test_pinned_baseline.py -v`: 1/1 PASS。保存ログが指す既存の Rust test binary 4 本を Cargo 再ビルドなしで直接実行: 1+4+4+4 = 13/13 PASS。保存 qrels と 32 件の rank を独立再計算し、L/LG の Recall@5 `0.625/1.000`、nDCG@10 `0.7497/0.9706` を確認。 |

F1: `scripts/pinned_baseline.py`:62–117 は isolated PoC の `cargo metadata --offline --locked` の到達 graph をたどり、PoC、`search-core`、`search-application`、`search-graph-memory`、`search-tantivy`、patched `tantivy` 0.26.2 の実パスと bytes を固定する。`Cargo.toml`:12–26 の path dependencies と `[patch.crates-io]` がこの経路に一致する。root manifest と toolchain/config も入力に含み、`:153–181` は各 phase の前後 snapshot が違えば reject する。Rust source 変更・`build.rs` 追加の digest 感度テストは 1/1 PASS。したがって旧 receipt の「dirty HEAD だけでは実行コードが決まらない」という F1 は、この保存実行に対して解消した。registry package の展開済み source bytes はこの manifest に列挙せず isolated lock に依存し、phase 内の一時変更を戻す事象は前後比較では捕捉しない。

F2: `corpus.rs`:213–235 は Resource 0 の同一 Version に、別 Part ID・logical path・raw bytes・representation・1/2 行 locator・Unit ID を持つ二つの Text Unit を生成する。`:277–325`、`validate.rs`:195–274 は独立した合成 Source Part binding から実 `KnowledgeUnit` を作って `validate_part_units` に通す。`tests/frozen_unit_codec.rs`:61–120 は Part、representation、raw、locator を差し替え、Unit ID / raw digest を再計算しても拒否されることを確認した。`run.rs`:303–344 は検証済み Unit 本文を親 1 件の `SourceSuppliedBody` にまとめて実 Tantivy に渡す。第二 Part 固有語の `qpart` は親 0 を一度返し、`q0` では L の `[0,1]` と Graph 重複を含む LG の `[0,1,2,3]` が `PriorityConcat` 後に親 0 を一度だけ計上した（run log:146–159）。この証拠は **親 document にまとめる L/LG baseline** のもの。将来の dense の個別 Unit hit → 親 Version folding は、この試験では通っておらず、dense 採点時に別途検証する。

F3: `metrics.rs`:58–94 は正例 0 件でも先に重複 rank を拒否し、Source-current visible parent 以外への disclosure と visible false positive を別々に数える。正例 0 件の Recall/MRR/nDCG は未定義のまま。`qaccess` は denied/unknown の 6 親（6,8,16,18,26,28）のみに当たる fixture で、`run.rs`:772–807 の実 Tantivy preauthorization probe は 6 件を確認した。同じ test の `RetrievalExecutor` 通過後の stage trace と最終 rank は空。保存 L/LG log:145–160 でも `qaccess=0`、`qnone=0`、可視・非関連の `qfalse=1`、unauthorized disclosure `0` と確認した。`tests/relevance.rs`:61–95 は正例 0 件の重複拒否と、仮想漏出を可視 false positive と混同しない count を確認した。ここでいう `raw_parent_ranks` は `run.rs`:664–693 の **現行 Read 後** の trace であり、Tantivy preauthorization raw hit を意味しない。

## 比較へ持ち込む境界

- 同じ seed `20260930`、七 query、gold/Read、32/256/1024 Resources で L/LG を測定した。三規模とも同じ 32 challenge Resources のため、同じ quality score はより難しい規模への一般化を示さない。各 arm/scale の latency 標本は七 query で、p95/p99 は最大標本に相当する。LG の first-in-arm は共有 index 上で L の後、RSS は `ps` の一点、index は RAM-only、更新は一件変更後の全世代再構築一標本である。SLO・peak memory・incremental 同値の判定には使わない。
- 次の exact action: この manifest の固定入力を L/LG 比較基準として保持し、同じ corpus・pin・Read・window・S1 順序で実 embedding の D/LD/LDG を測る。dense の Unit hit → 親 folding と Source binding/authorization trace、model/runtime/engine の pin と parity は dense 結果の受入時に別 gate で確認する。入力が後で変わっても今回の historical receipt は消えないが、新しい code を同一版と呼ばず、その code での比較には新たな snapshot/実行 receipt を作る。
