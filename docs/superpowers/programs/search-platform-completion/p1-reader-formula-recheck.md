# P1-Q01 XLSX／ZIP 数式 cache 補正の独立再監査

- 判定日: 2026-09-30 JST
- 対象: 固定合成 corpus の XLSX／ZIP reader 候補。read-only で source・raw fixture・manifest・既存 binary を照合し、本書だけを追加した。Cargo build は実行していない。
- 判定: **固定合成・既知構造の候補 subset に限り GO**。旧レビューの「式 cache の `<v>` があれば `Supported` とする」誤判定は、今回の実行結果では解消した。式 cache は肯定 Unit に使われず、式 cell の省略が分かるときだけ検証済みの残存 Unit とともに `Partial` になる。一般文書・production の `Supported` 認定はこの証拠から行わない。

## 固定入力と再実行

| 対象 | 再照合した SHA-256 |
| --- | --- |
| source bundle (`src/main.rs`, `src/readers.rs`, PoC `Cargo.toml`/`Cargo.lock` の名前と bytes を順に hash) | `a6d30a4fdfc6ef04b4265f813336fe98b18a9a713d942df5f8afb3d281af2afd` |
| `manifest.json` | `2ba1bc92d1b479a54e75cc38a02edf90a6da35237de21c8a3986f0ea685bbc35` |
| 実行した `target/debug/extraction-qualify` | `ee685c67d3531f5ed91057f8cda00d597ed9f65f1eb5141559ec781fd40abc4d` |
| macOS arm64 PDFium `libpdfium.dylib` | `1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7` |

source bundle と manifest は実行前後で同一 hash、binary と PDFium も実行前後で同一 hash を確認した。`parser_build_sha256` は `src/main.rs:40-47` の実行時 source 読み取り値であり、binary と source のビルド同一性の証明には使っていない。binary 自体の byte hash と挙動を別に記録した。

`python3 -B -m unittest discover -s experiments/search-extraction-poc/tests -v` は **6/6 PASS**。`python3 -B experiments/search-extraction-poc/run.py --verify-manifest` は raw SHA・origin/license・独立 Python oracle の **45/45 PASS**。固定 manifest 全 45 行を既存 binary に直接一括入力し、pin 済み PDFium を指定して **45/45 qualified**。全行で coverage/reason/known omission、Unit 一致数、miss 0、unexpected 0、locator 再解析失敗 0 を照合した。内訳は `Supported 10 / Partial 13 / Unsupported 19 / FailedPermanent 3`、12 format。今回の最大観測値は wall 587 ms、peak RSS 60,030,976 bytes、result 3,683 bytes、最大 raw 1,048,577 bytes。これは macOS arm64 の小 corpus 実測である。

## 数式と Archive の意味確認

| raw fixture | raw の式と cache | 実測 coverage / Unit | omission |
| --- | --- | --- | --- |
| `xlsx-cache-gap` | C4 `=B2`、`<v>` 欠落 | `Partial` / 3 | `MissingFormulaCache`、`xl/worksheets/sheet1.xml`、`[1,0]` |
| `xlsx-cache-complete` | B2=`東京`、C4 `=B2`／cache `10` | `Partial` / 3 | `UnsupportedStructure`、同位置 |
| `xlsx-cache-freshness-unknown` | C4 `=2+2`／cache `4` | `Partial` / 3 | `UnsupportedStructure`、同位置 |
| `xlsx-shared-string-rich` | rich shared string と C4 の式／cache `10` | `Partial` / 3 | `UnsupportedStructure`、同位置 |
| `xlsx-shared-string-rich-formula-free` | 式なし、rich shared string | `Supported` / 4 | なし |
| `xlsx-formula-only` | C4 の式／cache `10` のみ | `Unsupported` / 0 | Unit なし、reason `UnsupportedStructure` |
| `zip-modern-leaves` | `b.xlsx` に C4 の式 | `Partial` / 17 | member chain `b.xlsx` → worksheet `[1,0]` |
| `zip-nested-formula` | `inner.zip` 内の `b.xlsx` に C4 の式 | `Partial` / 4 | member chain `inner.zip` → `b.xlsx` → worksheet `[1,0]` |

raw OOXML を Python `zipfile` / XML で再読取し、表の式・cache と member 構造を確認した。C4 の locator は上記 XLSX／ZIP の期待 Unit に含まれない。`src/readers.rs:764-783` はこの profile の worksheet/row/cell 直下を既知 child に制限し、`src/readers.rs:806-823` は式 cell を cache の有無にかかわらず除外して物理 omission を記録する。`src/readers.rs:860-866` は残存 Unit がある場合だけ `Partial`、0 件なら `Unsupported` とする。`src/readers.rs:1060-1136` の ZIP 再帰は leaf reason と全 member chain を伝播し、今回の raw oracle と一致した。45 件中 `Supported` に formula omission は 0 件。

[Microsoft の SpreadsheetML 説明](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-formulas)は `<v>` を最後の計算時点の cached value と定義している。従って `<v>` の存在や `2+2` と `4` の見かけ上一致は、現在の値を検証した証拠ではない。

## 適用境界

- cache が見かけ上正しい `4` でも鮮度を証明せず `UnsupportedStructure` として省略する。古さを計算で検出したという主張ではない。`Partial` の残存 Unit は肯定検索候補であり、本文不存在の証拠にはならない。
- 今回の candidate GO は固定された既知構造の raw に限る。未知 package/reader-visible part と構造、他形式の `Partial` に必要な全物理 omission、Archive の全 `ArchiveReaderNode`／profile pin、Source 親・Version・raw・current Read の host 再検証は未資格。`p1-reader-poc-review.md` の一般 corpus に関する留保は残る。
- Linux fresh-process sandbox、PDFium の Linux pin、1/10/50 MiB・CPU/AS/scratch/output budget、admission/worker/host 三層、production 縦断と p95/p99 は今回実施していない。binary の再ビルドをしていないため、hash 一致と挙動は確認できても source からの build provenance は別 gate。実行行の `license_security` は固定文字列であり、今回 `cargo deny` を実行した証拠ではない。

次の exact action: この限定 GO を P1-Q01 の固定 reader 候補判定に反映し、production 採用前に一般 scope・host binding・Linux sandbox／budget の別 gate を通す。
