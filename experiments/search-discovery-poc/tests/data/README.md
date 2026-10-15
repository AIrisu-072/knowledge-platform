# 無filter Tokenizer 6.2.0の基準

`tokenizer-6.2.0-golden.json` はmain `373941fbfbcdd2cb85ecae0d5b9530ae85160241` の旧コード・旧lockで取得した実出力。旧productionと同じ `Tokenizer::new(Segmenter::new(Mode::Normal, embedded IPADIC, None))`、filter追加なし。59入力/257token、Tantivy defaultとLindera各13検索case。SHA256 `b5f857d4f6ec2a88e5884dd134d6448753bc0c8fe49325e8e29bc268745f36b8`。

再現する場合はそのmainの別checkoutへ、保存した `capture-old-reference.rs.txt` を `experiments/search-discovery-poc/tests/old_reference.rs` としてコピーし、固定Rust1.98.1で以下を実行する。現在のmanifestへanalysisを再追加しない。出力は別private pathを指定し、既存goldenを自動更新して差分を隠さない。

```sh
KP_OLD_REFERENCE_PATH=/absolute/private/old-golden.json \
cargo test --locked --manifest-path experiments/search-discovery-poc/Cargo.toml \
  --test old_reference -- --nocapture
```

fixture本文/alias/query・境界入力を元順序で保存する。qualityはJSON小数に加えf64のu64 bitsを記録し、JSON parserの1ULP丸めと本当の計算差を区別する。timingsはこのgoldenに入れず、別の実measure-lexical記録に保存する。
