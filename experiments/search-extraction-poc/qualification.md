# P1-Q01 reader 資格判定

## 判定

2026-10-01 独立レビューで hidden worksheet の物理 part 欠落を `Partial` と誤判定する defect が再現され、先行 binary に対する **NO-GO** が出た。欠落・壊れた XML・誤った宣言 MIME の独立変異 RED から修正し、下記の fresh Linux binary で GREEN と全 45 件を再資格した。独立再レビューは待機中であり、production 採用は **NO-GO** のまま。

**固定合成 corpus に限り reader 候補 GO。production 採用は保留。** 45 件すべてで direct reader、独立 raw-byte oracle、期待 Unit と native locator が一致した。式セルは cache の有無にかかわらず Unit にせず、残る検証済みセルがある場合だけ物理位置付き omission と `Partial` にする。式セルだけなら Unit 0 `Unsupported`。ZIP は leaf の omission に member chain を付けて伝播する。`XLSX` の式なし rich shared string と、ZIP 内の DOCX/XLSX/XLSM/PPTX/PDF/HTML/Text/CSV も読んだ。`Supported` はこの小さな構造 subset に限る。補正後の独立 read-only review、広い corpus、Linux sandbox と production admission/worker/host 検証までは P1-Q01 の採用判定および P1 全体を完了扱いしない。

2026-10-01 cloud Linux x86_64 の同一 45 raw bytes も fresh source build で **45/45 qualified**（`Supported` 10、`Partial` 11、`Unsupported` 21、`FailedPermanent` 3）。Linux 固有の PDFium 7881 pin を照合し、追加の executable admission probes（related/未参照 Office part、物理 omission と hidden worksheet 正本 part 検証、unlocated PPTX shape、曖昧 PDF/動的 HTML の fail-closed、未実行 security scan の非自己証明）を含む Python suite **15/15 PASS**。全 `Partial` は固定 corpus 上で物理 omission があり、曖昧 PDF と動的 HTML は証明できる omission がないため Unit 0 `Unsupported` とした。独立した `cargo-deny 0.20.2` scan は advisories/bans/licenses/sources **PASS**、警告は `syn` 2/3 重複と未使用 allow 2 件。これは in-process PoC の再資格であり、production 採用は引き続き **NO-GO**。[Linux receipt](linux-qualification-20261001.md) に exact hashes、RED→GREEN、欠落 gate を記録した。以下の macOS 数値・未実施記述は当時の履歴であり、現行 Linux 再資格の数値と混ぜない。

対象 plan は `p1-extraction-plan.md` SHA-256 `8a02c655199b974d8bb3dfe865bc91138908f09aba1f57961c058c1a5cab85ec`、composed freeze は `p1-extraction-freeze.md`。`generate.py` の原本はすべて synthetic で、外部・顧客ファイルを含まない。各 byte hash と license 宣言は `manifest.json`。同文面「東京」「同文。」を異なる形式・Part に置き、Unit/Part/locator の取り違えを検出する。

## 実測と到達範囲

| 形式 | 固定 case | PoC 判定 | reader と境界 |
|---|---:|---|---|
| DOCX | 5 | GO: bounded subset | bounded `quick-xml` + `zip`。見出し・段落・入れ子表を再位置指定。header は `Partial`、OPC spoof/深い XML/未知 body block は Unit 0。一般の変更履歴、textbox、footnote 等は未資格。 |
| XLSX | 8 | GO: bounded subset | 同上。非 A1 起点の絶対 cell と workbook 順。式 cache 欠落、明らかに古い値、鮮度不明の値を別 raw fixture で試し、いずれも式 Unit を除外。式なし rich shared string は `Supported`、範囲外 index は Unit 0 `CorruptDocument`。一般の style、chart、未確認 typed cell は未資格。 |
| XLSM | 1 | GO: bounded subset | 同上。sheet cell のみ。macro は実行せず、既知省略として `Partial`。 |
| PPTX | 2 | GO: bounded subset | 同上。複数 slide、group shape、table cell。notes は `Partial`。chart/SmartArt 等は未資格。 |
| PDF | 5 | GO: bounded subset | `pdfium-render 0.9.4` + PDFium 7881 + `lopdf 0.45.0`。native character index、2 page/縦組の順序不確定は物理 omission を証明できず `Unsupported`、image-only は `RequiresOcr`、`3 Tr` 不可視 text/未知 content operator は `UnsupportedStructure`、破損は `FailedPermanent`。描画を伴う広い PDF は未資格。 |
| Text | 3 | GO: bounded subset | UTF-8/明示 Windows-31J。BOM、CRLF、物理 line を検証。無効 bytes は Unit 0。 |
| CSV | 3 | GO: bounded subset | `csv 1.4.0`。quote 内改行と `(record,field)`。曖昧 delimiter、巨大 field は Unit 0。明示 comma/UTF-8 以外は未資格。 |
| HTML | 3 | GO: bounded subset | `html5ever 0.39.0` DOM。native text-node path、heading、hidden/script、depth。動的 visibility は既知 omission の位置を証明できず Unit 0 `Unsupported`。CSS/JS による最終視覚表示は未保証。 |
| ZIP | 12 | GO: bounded subset | `zip 8.6.0` + 明示 leaf dispatch。nested member chain、Office/PDF/HTML/Text/CSV leaf、式付き XLSX/XLSM の `Partial` 理由と物理 omission の伝播、悪い inner の atomic Unit 0 を確認。path traversal、symlink、duplicate、NFC 衝突、暗号、圧縮 bomb は Unit 0。ZIP composite profile では PDF leaf に備えて PDFium binary も照合する。 |
| DOC/XLS/PPT | 各 1 | 明示非対応 | magic-only。`UnsupportedFormat`、Unit 0。 |

独立 Python oracle は raw bytes の XML/CSV/HTML/PDF ToUnicode/ZIP を読み、Rust reader の出力と固定期待 text/kind/order/native locator を比較する。Rust 側も同一 raw から locator を再解析し、重複や一意性の破れを失格にする。plain-text 一括出力から locator を逆算しない。

2026-09-30 macOS arm64 の同一 parser build 全形式 run は **45/45 qualified**（`Supported` 10、`Partial` 13、`Unsupported` 19、`FailedPermanent` 3）。旧 41 件で `Supported` とされた式付き `xlsx-cache-complete`、`xlsx-shared-string-rich`、`zip-modern-leaves` は `Partial` に変更した。miss、unexpected、locator 再解析・一意性失敗、式 omission 不一致は各 0。最大 wall **444 ms**、peak RSS **50,495,488 bytes**、structured result **3,683 bytes**、scratch **0 bytes**。parser source bundle SHA-256 は `a6d30a4fdfc6ef04b4265f813336fe98b18a9a713d942df5f8afb3d281af2afd`、実行 binary SHA-256 は `ee685c67d3531f5ed91057f8cda00d597ed9f65f1eb5141559ec781fd40abc4d`、`manifest.json` は `2ba1bc92d1b479a54e75cc38a02edf90a6da35237de21c8a3986f0ea685bbc35`、`qualification-results.jsonl` は `1b12f3684186a029929baca96a87ee1407c3d758f2130e0334946d82c9fec21a`。source bundle 値は実行時に source を読むもので binary attestation ではない。これらは固定合成 corpus 上の測定であり、一般文書の full coverage を示さない。

初期候補 ceiling は以下。0 は該当しない形式。Office は DOCX/XLSX/XLSM/PPTX、ZIP の CSV budget は inner CSV に適用する。PoC が coverage を確定する前後に実装した値であり、production の admission/worker/host 三層一致を証明するものではない。

| BudgetKey | Office | PDF | Text | CSV | HTML | ZIP |
|---|---:|---:|---:|---:|---:|---:|
| InputBytes | 268435456 | 268435456 | 268435456 | 268435456 | 268435456 | 268435456 |
| ZipEntries | 20000 | 0 | 0 | 0 | 0 | 20000 |
| ZipEntryBytes | 67108864 | 0 | 0 | 0 | 0 | 67108864 |
| ZipTotalBytes | 536870912 | 0 | 0 | 0 | 0 | 536870912 |
| ZipDepth | 1 | 0 | 0 | 0 | 0 | 3 |
| Units | 100000 | 100000 | 100000 | 100000 | 100000 | 100000 |
| UnitUtf8Bytes | 1048576 | 1048576 | 1048576 | 1048576 | 1048576 | 1048576 |
| WorkerOutputBytes | 16777216 | 16777216 | 16777216 | 16777216 | 16777216 | 16777216 |
| XmlDepth | 256 | 0 | 0 | 0 | 0 | 0 |
| XmlNodes | 2000000 | 0 | 0 | 0 | 0 | 0 |
| PdfPages | 0 | 1024 | 0 | 0 | 0 | 0 |
| PdfOperations | 0 | 1000000 | 0 | 0 | 0 | 0 |
| HtmlNodes | 0 | 0 | 0 | 0 | 2000000 | 0 |
| CsvRecords | 0 | 0 | 0 | 1000000 | 0 | 1000000 |
| CsvFieldBytes | 0 | 0 | 0 | 1048576 | 0 | 1048576 |

追加 ceiling: wall 10 s、CPU 8 s、worker AS 2 GiB、scratch 1 GiB。PoC は wall/RSS/structured result/scratch を報告するが、macOS 実行には CPU/AS の強制と Landlock/seccomp がない。小 fixture の最大 raw は 1,048,577 bytes。1/10/50 MiB 級、最大 Unit、多数 Part、Linux fresh process での上限到達と p95/p99 はまだ測っていない。式セルの `known_omissions` は package path、`sheetData` 相対の物理 `[row, cell]` index、ZIP member chain と reason を返す。他形式の `Partial` を含む production `NativeOmission` 全体、admission と host 側の制限一致は未実装。

## 依存と security

PDFium 151.0.7881.0 の既存 mac-arm64 binary を read-only 再利用し、SHA-256 `1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7` を実行前照合。隣接 license file は再配布条件付き BSD 形式で SHA-256 `1fe9dea718fbd75cf149adaf4d8a22a4335604d964ddb76d1b45383dec8668c9`。現行 Linux PDFium binary SHA-256 は `f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64` と再照合済みだが、強制 Search sandbox は未実施。現行 `license_security` JSON 値は `not-executed` 固定で、実 `cargo-deny` PASS は [Linux receipt](linux-qualification-20261001.md) の独立コマンド証拠に限る。macOS 当時の scan は `formula-fix-receipt.md` に記録されている。

`office_oxide 0.1.11` の [Document::from_reader](https://docs.rs/office_oxide/0.1.11/office_oxide/struct.Document.html)、`calamine 0.36.1` の [Range](https://docs.rs/calamine/0.36.1/calamine/struct.Range.html)、[PdfPageText](https://docs.rs/pdfium-render/0.9.4/pdfium_render/prelude/struct.PdfPageText.html)、[zip-rs](https://docs.rs/zip/8.6.0/zip/) は公式 API と local pinned source を確認した。`office_oxide`、`calamine`、`rxls` は本 PoC で実行していないため Search 本文用途として**未資格**。OOXML の現在の候補は raw ZIP + bounded XML であり、DSI の parser output は使用しない。Linux での実 `cargo-deny` scan は上記 cloud receipt を参照。

## 2026-09-30 時点の次 action（履歴）

1. 独立 read-only reviewer が修正後の `qualification-results.jsonl` と固定 corpus の期待/実測を照合し、式 Unit 除外、数式 omission の物理位置、ZIP member chain、`Partial` から否定証明を出さない境界を監査する。
2. DOCX field/tracked change/textbox、XLSX 他の rich/typed cells・chart、PPTX chart/SmartArt/notes、PDF 他の不可視/回転/ToUnicode edge、HTML CSS/DOM repair、ZIP 内の変則構造を独立 oracle 付き corpus に追加する。既知 omission と locator が証明できるときだけ `Partial` を使う。
3. 1/10/50 MiB と上限境界、Linux fresh-process sandbox、native Linux PDFium pin、admission/worker/host の limit と locator 再検証を通してから production dependency/pin を判断する。
