# Search extraction reader PoC (P1-Q01)

`spec/` と composed P1 freeze に従う、Search 本文 reader の隔離評価です。`generate.py` は外部データを使わず合成 DOCX/XLSX/XLSM/PPTX/PDF/Text/CSV/HTML/ZIP と旧形式 magic を生成します。`oracle.py` は原本 bytes を Python 標準ライブラリで再走査し、`src/readers.rs` の Rust reader と独立に native locator、text、構造、順序、coverage を検証します。DSI の output/fingerprint は入力にも期待値にも使いません。

## 実行

Repository root から:

```sh
python3 experiments/search-extraction-poc/generate.py
python3 -m unittest discover -s experiments/search-extraction-poc/tests
python3 experiments/search-extraction-poc/run.py --verify-manifest

# Linux native-pin/admission probe は fresh Linux binary を指定して別途実行する。
source /workspace/shared/search-toolchain/env.sh  # cloud toolchain が存在する場合
export PDFIUM_DYNAMIC_LIB_PATH=/absolute/existing/pdfium/7881/linux/lib
CARGO_TARGET_DIR="$PWD/experiments/search-extraction-poc/target" \
  cargo build --manifest-path experiments/search-extraction-poc/Cargo.toml --locked --bin extraction-qualify
export P1_QUALIFIER_BIN="$PWD/experiments/search-extraction-poc/target/debug/extraction-qualify"
python3 -B -m unittest discover -s experiments/search-extraction-poc/tests -v

# PDF は既存の検証済み PDFium 7881 binary directory を設定する。再ダウンロード不要。
export PDFIUM_DYNAMIC_LIB_PATH=/absolute/existing/pdfium/7881/lib
for format in docx xlsx xlsm pptx pdf text csv html zip doc xls ppt; do
  python3 experiments/search-extraction-poc/run.py --qualify --format "$format" --target "$PWD/target"
done

cargo deny --manifest-path experiments/search-extraction-poc/Cargo.toml \
  --config experiments/search-extraction-poc/deny.toml check
```

`run.py --qualify` は独立 `[workspace]` の `cargo run --locked --bin extraction-qualify` を起動し、fixture ごとに一行 JSONL を出します。失格行があれば終了 code は非 0 です。`--target` は共有 target を再利用する選択肢で、Cargo.lock はこの PoC 内だけにあります。ビルドは `CARGO_INCREMENTAL=0`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_BUILD_JOBS=2` が既定です。

`manifest.json` が fixture bytes の SHA-256、合成由来、license、期待 Unit、coverage、reason の正本です。`expected.json` は人が差分確認しやすい Unit 一覧です。`qualification-results.jsonl` は実測時の証拠で、再実行すると時間・RSS は変わります。測定範囲と採用判定は [qualification.md](qualification.md) を参照してください。

PoC の macOS 実行は Linux fresh-process sandbox の強制証拠ではありません。Production reader や Source binding、host 側 locator 再検証はここでは実装しません。
Linux の in-process PoC 実測も fresh-process sandbox/production reader の資格とは区別します。
