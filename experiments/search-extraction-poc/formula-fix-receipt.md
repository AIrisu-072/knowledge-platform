# P1-Q01 式 cache 誤判定補正 receipt

- 日付・範囲: 2026-09-30 JST、`experiments/search-extraction-poc/**` の固定 synthetic corpus と隔離 PoC reader のみ。旧 `p1-reader-poc-review.md` とその hash 履歴は変更していない。
- 根拠: [Microsoft Open XML の formula 説明](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-formulas)では `<v>` は最後の計算値の cache。存在だけでは現在値の証明にならない。composed P1 freeze の `Partial` は検証済み Unit と位置・理由が分かる omission を要し、`Partial` から本文不存在を証明できない。

## RED → 補正

先に `test_formula_cache_never_supplies_positive_unit` を追加した。旧 manifest では `xlsx-cache-complete` の式 C4 `=B2` / cached `10` が期待 Unit に含まれ、テストは失敗した。期待を補正した後、保存旧 binary `/tmp/search-completion-preserved-poc-bin/extraction-qualify-current` を実行すると `xlsx-cache-complete`、`xlsx-cache-freshness-unknown`、`xlsx-shared-string-rich` は `Supported` のまま cached Unit を余分に返し失格だった。これは旧 reader の挙動再現であり、旧 binary が現 source から作られたという attestation ではない。

reader は `<f>` を持つ cell を、`<v>` の有無や内容にかかわらず Unit から除外する。残る非数式 cell の検証済み Unit があるときは `Partial` とし、`known_omissions` に worksheet package path、`sheetData` 相対の物理 `[row, cell]` index、reason を返す。`<v>` 欠落は `MissingFormulaCache`、存在しても鮮度を検証できないものは凍結 enum の `UnsupportedStructure` とした。式のみで Unit が残らない sheet は `Unsupported` / Unit 0。ZIP は leaf の `Partial` reason と omission に outer/nested member chain を付けて伝播する。いずれの `Partial` も exact negative proof の根拠にはならない。

独立 Python oracle は raw OOXML を再走査する。元の B2=`東京`、C4=`B2`/cached `10` を古い cache、C4=`2+2`/cached `4` を鮮度不明、`<v>` なしを欠落として別 fixture に固定した。式なし rich shared string は絶対 cell locator の `Supported` を保持。式のみ sheet の Unit 0 と nested ZIP の `inner.zip`→`b.xlsx` omission chain も固定した。

## GREEN と pin

- Python unittest **6/6**、raw SHA・独立 oracle と manifest **45/45**。旧 41 件に新規 4 件を追加した。
- 新 source を `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2` で `--locked` build。全 12 format の `run.py --qualify` を同じ binary と固定 PDFium binary で実行し、**45/45 qualified**。miss、unexpected Unit、locator roundtrip failure、formula omission mismatch は各 **0**。
- coverage: `Supported 10 / Partial 13 / Unsupported 19 / FailedPermanent 3`。旧 41 件で `Supported` だった `xlsx-cache-complete`、`xlsx-shared-string-rich`、`zip-modern-leaves` は `Partial`。旧 41 件だけの `Supported` は 12→9。新しい式なし rich shared string 1 件を含む全体では 10。
- 観測最大: wall **444 ms**、peak RSS **50,495,488 bytes**、structured result **3,683 bytes**、scratch 報告 **0 bytes**。最大 raw **1,048,577 bytes**。macOS arm64 の小 corpus 値。
- strict Clippy `--all-targets -- -D warnings`、`cargo fmt --check`、`cargo deny check` は PASS。deny の `syn` 2/3 重複と未遭遇 license allow 2 件は非阻害警告。
- PDFium 151.0.7881.0 mac-arm64 library SHA-256 `1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7` を現物照合して read-only 再利用。別 hash の pypdfium2 binary は使用していない。
- ビルド前後の空き容量観測は約 **2.9 GiB**、共用 `target` は **237→516 MiB**。root Cargo と lock は変更していない。

## 再現用 digest

| 対象 | SHA-256 |
| --- | --- |
| `src/readers.rs` | `0a75fc215dad6a4b91797f2fe0a89f8ae4853765c8d1dbaf1a2f7e17c0026017` |
| `src/main.rs` | `0eb241ed25f0bcd08f0d35906734cb56267b2bc3a084b6fae13ec7d8b122057d` |
| source bundle (`main.rs`, `readers.rs`, PoC Cargo.toml/lock の名前と bytes を順に SHA-256) | `a6d30a4fdfc6ef04b4265f813336fe98b18a9a713d942df5f8afb3d281af2afd` |
| 実行 `target/debug/extraction-qualify` binary | `ee685c67d3531f5ed91057f8cda00d597ed9f65f1eb5141559ec781fd40abc4d` |
| `manifest.json` | `2ba1bc92d1b479a54e75cc38a02edf90a6da35237de21c8a3986f0ea685bbc35` |
| `qualification-results.jsonl` | `1b12f3684186a029929baca96a87ee1407c3d758f2130e0334946d82c9fec21a` |

`parser_build_sha256` は実行時に source を読む値で、binary attestation ではない。実行 binary byte hash を別に記録した。

## 残る gate

この判定は固定 synthetic corpus の macOS reader 候補に限る。他形式の `Partial` の全 omission 位置、未知構造を含む一般 corpus、Linux fresh-process sandbox と PDFium pin、admission/worker/host 三層、Source/parent/Read binding、production 採用は未検証。次の exact action は修正後の reader・oracle・45 件の JSONL を独立 read-only reviewer に渡し、数式の肯定/否定境界と ZIP omission chain を再監査すること。
