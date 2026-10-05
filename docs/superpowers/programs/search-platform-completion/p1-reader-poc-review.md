# P1-Q01 Search 本文 reader PoC 独立レビュー

- 判定日: 2026-09-30 JST
- 役割: read-only QualityAuditor。変更は本レビュー文書のみ。production reader、root Cargo、fixture、manifest、plan は変更しない。
- 判定: **限定候補 GO、現行の Q01 `Supported` 判定は要修正**。固定合成 corpus で raw からの native locator と text を得る候補としては有用。ただし式キャッシュ付き XLSX とそれを含む ZIP に `Supported` の誤判定があり、報告された 12 件の `Supported` をそのまま P1-Q01 完了証拠にはできない。式を含まない明示 subset に限るか、式の鮮度不明を `Partial`/`Unsupported` に倒して独立 oracle と期待値を再資格する。全 P1 の production 採用、一般文書の網羅性、Linux sandbox、Source/host 権限境界の判定は別工程。

## 照合した入力と実測

| 入力 | SHA-256 / 位置 |
| --- | --- |
| composed freeze | `p1-extraction-freeze.md` `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d`。同文書に記載された revision、最小 Unit freeze、body absence、Partial positive の優先順位を採用。 |
| full plan | `p1-extraction-plan.md` `8a02c655199b974d8bb3dfe865bc91138908f09aba1f57961c058c1a5cab85ec`、P1-Q01 と Global Constraints。 |
| corpus | `experiments/search-extraction-poc/manifest.json` `677641f08288bb4c0869973e273d3ddbe0ed2a96461cb06eef9d6794b2c74cfc`。41 個すべて synthetic/CC0 表記、raw hash と独立 Python oracle expectation は再照合 PASS。 |
| 保存結果 | `qualification-results.jsonl` `47e9f3b6beafe3f4ceabf5641ff919fbc9f01efee9935a2678b35996cdc38ad8`。41/41 qualified、`Supported` 12 / `Partial` 8 / `Unsupported` 18 / `FailedPermanent` 3、miss/unexpected/locator failure 各 0。最大 wall 201 ms、peak RSS 50,348,032 bytes、result 3,554 bytes。 |
| parser / native | `src/main.rs`、`src/readers.rs`、PoC `Cargo.toml`、`Cargo.lock` の `parser_build_sha256` `7fe89fe642b4af4dc01df99f7b229967fa27357a3be20b12577cf9a30aebb8b7` は独立再計算一致。既存 `target/debug/extraction-qualify` binary SHA-256 は `1e2af9767cd5db370d86f62b6ee58cbd68db4f670aba557d88bf985b3844b896`。mac-arm64 PDFium binary SHA-256 `1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7`、`licenses/pdfium.txt` SHA-256 `1fe9dea718fbd75cf149adaf4d8a22a4335604d964ddb76d1b45383dec8668c9` を現物で確認。 |

再実行: `python3 -m unittest discover -s experiments/search-extraction-poc/tests`、`python3 experiments/search-extraction-poc/run.py --verify-manifest` は PASS。既存 binary に manifest 全 41 件を一括入力し、pin 済み PDFium で **41/41 qualified**、内訳 12/8/18/3、miss/unexpected/locator failure 各 0。今回の最大 wall 545 ms、peak RSS 64,405,504 bytes、result 3,554 bytes。既存 binary は最後の `readers.rs` 更新後に作成されていたが、PoC の `parser_build_sha256` は実行時に source を読む値（`src/main.rs:38-45`）であり binary byte hash による build attestation ではない。`cargo deny --manifest-path experiments/search-extraction-poc/Cargo.toml --config experiments/search-extraction-poc/deny.toml check` は advisories/bans/licenses/sources が PASS。非阻害警告は `syn` 2/3 重複と license allow 未遭遇 2 件。結果行の `license_security: "cargo-deny:pass"` は `src/main.rs` の固定文字列なので、別の今回の `cargo deny` 実行を根拠とする。

固定 raw 上では、DOCX body block/入れ子 table、XLSX workbook 順・絶対 cell・rich shared string、PPTX slide/group/table、PDFium character index と ToUnicode oracle、Text line/BOM/CP932、CSV record/field、HTML DOM text-node path、ZIP member chain と inner locator がそれぞれ一致した。ZIP の全 modern leaf と inner `Partial` reason 伝播、PDF `3 Tr` 不可視 text の Unit 0、legacy DOC/XLS/PPT の明示非対応も確認した。Rust の locator 再解析は同一 `inspect()` の再走査（`src/main.rs:97-116`）であり、独立性の根拠は raw を別実装で読む `oracle.py` と manifest hash の照合にある。これは当該固定構造の証拠である。

## 指摘

### [P1] 式の鮮度が不明な XLSX を `Supported` にしている

`src/readers.rs:779-824` は `<f>` があっても `<v>` 要素さえ存在すれば `MissingFormulaCache` を付けず、値を Unit にして `Supported` にする。`generate.py:67-76,232-244` の `xlsx-cache-complete`、`xlsx-shared-string-rich` と ZIP 内 `b.xlsx` は B2 が「東京」、C4 が式 `B2` なのに cached `<v>10</v>`。raw-byte oracle（`oracle.py:103-113`）も同じ cached byte `10` を期待するため 41/41 一致は古い cache を検出しない。Microsoft の [Open XML formula 説明](https://learn.microsoft.com/en-us/office/open-xml/spreadsheet/working-with-formulas)でも `<v>` は最後に計算した値であり、現在値の保証ではない。凍結設計 `p1-extraction-design.md` §4 は古い cache や鮮度判定不能を `Partial` とする。

このまま `Supported` を Source の本文完全性や exact negative proof に使用してはならない。Q01 の補正は、式を持つファイルを候補の `Supported` subset から除外し `UnsupportedStructure`/Unit 0 とするか、凍結契約に適合する既知 omission として `Partial` にし、未検証の cache Unit を肯定証拠から除くこと。必要なら coverage reason の設計追補を別途決める。XLSX、rich shared string、ZIP leaf の期待値と oracle をその意味に合わせて再実行する。式を持たない XLSX の rich shared string そのものは今回の指摘対象外。

### [P2] `Supported` を固定 subset 外に広げるための scope 判定が未完成

`src/readers.rs:638-646` の DOCX 既知省略検出は header/footer/footnotes の package 名だけで、未認識の reader-visible part 全体を列挙しない。`src/readers.rs:747-811` の XLSX は `sheetData` 内の `row`、行内の `c` だけを走査し、それ以外の子要素を拒否せず、inline string の `t` は再帰連結する。現行 fixture の構造では一致しているが、未知の同階層要素や拡張を持つ raw に対する「全対象 scope を最後まで列挙した」証拠はない。`Supported` を許す形式 profile は認識済み package/child structure に閉じ、未知は Unit 0 `Unsupported`、物理位置と理由を確定できた省略だけ `Partial` とする必要がある。ここは一般 corpus / production admission の gate であり、固定構造の候補評価を取り消すものではない。

### [P2] `Partial` の省略位置と Archive 全 reader 使用は次工程で証明が必要

DOCX header と PPTX notes fixture はそれぞれ package part の存在で `Partial` になるが、`src/readers.rs:638-645,925-931` は main body/slide からの relationship と省略対象の物理 child path を返さない。固定 fixture の reason 一致は確認できる一方、凍結 revision §6 の「既知の未読範囲」を host が検証できる `NativeOmission` には未到達。`src/readers.rs:1020-1084` の ZIP は member chain と reason を出すが、`p1-knowledgeunit-amendment.md` §1 が要求する outer/nested/leaf 全 `ArchiveReaderNode`、実効 charset/CSV dialect/build/native pin を `reader_use` として返さない。PoC のハードコードされた UTF-8/comma/PDFium pin の固定 leaf subset としてのみ候補 GO。production profile/UnitId、Source Part/Version/raw binding と照合する段階で全 node と省略位置を検証する。

## 採用範囲と残件

- 本 PoC の 41 件は現形式 reader 候補の小さい固定合成 corpus。`office_oxide`、`calamine`、`rxls` は実行されておらず Search 本文用途として未資格。一般 DOCX/XLSX/XLSM/PPTX/PDF/HTML、旧 DOC/XLS/PPT、未宣言 charset/dialect、複雑な ZIP を「対応済み」と表示しない。
- manifest の `part_ordinal` は全件 0。異なる ContentItem/Version に同文面・同 raw FileObject がある場合の親 binding、Source 正本 raw/MIME/size、current Read は Q01 reader 実測の対象ではない。P1 の別工程で host が再検証する。
- 観測最大は小 fixture（最大 raw 1,048,577 bytes）の in-process macOS 値で、10 s/2 GiB/1 GiB scratch/16 MiB と各 15 `BudgetKey` の上限到達試験ではない。`scratch_bytes:0` は PoC 行の固定値。1/10/50 MiB、CPU/AS 強制、Linux fresh process Landlock/seccomp、Linux PDFium pin、admission/worker/host 三層一致、実 DB/FS 縦断と p95/p99 は P1-V01 等の別 gate。安全または license の実測 blocker は今回確認しなかった。

次の exact action: 式 cache の `Supported` 判定と期待 oracle を上記範囲で修正し、対象 XLSX/ZIP だけを同じ parser・asset pin で再資格する。その結果で Q01 の `Supported` 数と候補 subset を更新し、その後 I01 以降の production gate に渡す。
