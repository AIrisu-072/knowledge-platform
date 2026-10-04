# P1→P2 KnowledgeUnit / extraction core contract v1

- Status: **設計案。独立レビューでこの版を確認してから P2 入力として freeze する**。本番 parser の採用、本文検索の実装・稼働を示すものではない。
- Scope: provider-neutral Unit の同一性、native locator、正本 binding、正規化、検証、P2 cache。Document 固有の ID / StorageKey は adapter に閉じる。Search v0 の Source 正本、Live / History、S1 順序、canonical parent candidate ID は変更しない。
- 基準: `spec/data/logical-data-model-v0.md` §2.6, §4.4、`p1-extraction-design.md` §1–3、`p1-extraction-architecture-review.md` の KnowledgeUnit / evidence 指摘。

## 1. 最小型と信頼境界

以下の `String` は Source が供給する opaque UTF-8 ID であり、利用者入力ではない。Document adapter は `source_native_version` と `source_native_part_id` に、それぞれ DocumentVersionId / ContentItemId の小文字・ハイフン付き UUID（36 ASCII bytes）を使う。`ResourceId` は **Version の** Search ResourceId であり、DocumentId ではない。`ContentPartRef.logical_path` は Document の `LogicalPath` と同じ NFC、case-sensitive、相対 `/` 区切り規則に従う。別 Source は同じ条件を満たす正本内安定 ID と path を登録する。

```rust
// search-core::knowledge_unit。Uuid は既存 search_core::id の typed UUID。
struct ResourceVersionRef {
    source_id: SourceId,
    resource_id: ResourceId,
    source_native_version: String,
}
struct ContentPartRef {
    source_native_part_id: String,
    logical_path: String,
    ordinal: u32,
}
struct RawBinding {
    sha256: [u8; 32],
    size_bytes: u64,
    media_type: String, // lowercase type/subtype essence; parameters は含めない
}
struct UnitProvenance {
    source_snapshot: String,                  // 正本 snapshot の opaque token
    authoritative_representation_ref: String, // Source 内の immutable representation ID
    raw: RawBinding,
    detected_format: FormatId,
    profile: ExtractionProfileId,
    parser_build_id: String,                 // qualified parser + native pin の登録済み build ID
}
struct KnowledgeUnit {
    unit_id: UnitId,                          // [u8; 32]
    version: ResourceVersionRef,
    part: ContentPartRef,
    parent_unit_id: Option<UnitId>,
    ordinal: u32,                            // part 内 0..N-1 の native traversal 順
    kind: UnitKind,
    text: String,                            // §3 の正規化済み UTF-8
    locator: NativeLocator,
    text_sha256: [u8; 32],
    provenance: UnitProvenance,
}
enum FormatId { Docx, Xlsx, Xlsm, Pptx, Pdf, Text, Csv, Html, Zip }
enum UnitKind { Heading, Paragraph, TableCell, SpreadsheetCell,
                SlideText, PdfText, PlainText, CsvField, HtmlText }
```

`source_snapshot`、representation、raw hash/size/MIME、Source/Version/Part、actor の現在 Read は trusted host / Source adapter だけが取得・検証する。sandbox worker が受けるのは検証済み bytes/FD、expected hash/size、format、profile、数値 budget だけである。worker は正規化前 text、`NativeLocator`、`UnitKind` と native traversal 順を返し、Source ID、Document/File ID、StorageKey、actor、secret、外部 URL を受け取らず、`UnitId` / `UnitProvenance` を作らない。host が text を正規化し ID/digest/provenance を付与する。

`UnitProvenance` は generation ID と独立し、Unit の同一性入力には使わない。ただし generation の immutable item manifest には Source snapshot、authoritative representation、raw binding と profile を必ず記録し、hit 解決時に全て照合する。同じ FileObject を指す別 ContentItem は別 `part` と別 Unit ID になる。

## 2. NativeLocator v1 — 一意な座標と往復

全整数は 0 始まり。範囲の終端は exclusive。native locator は **同じ raw hash と同じ profile で再解析するとちょうど一つの native 要素へ戻り、再構成した正規化 text が Unit.text と一致する**場合のみ有効。単なる表示用 page/name/bbox は identity に入れない。parser が一意な locator を出せない要素は Supported Unit にしない。

| tag / variant | canonical fields と検証 | Unit 粒度 |
| --- | --- | --- |
| `1 Docx { steps: Vec<DocxStep> }` | `/word/document.xml` の main body の XML child traversal。最初は `BodyBlock(index)`。table node の後は `Row(index) → Cell(index) → CellBlock(index)`、cell 内 table はこの組を再帰する。index は各親の物理 `<w:p>` / `<w:tbl>`、`<w:tr>`、`<w:tc>` の 0 始まり順。grid 結合後の見かけの cell 座標を使わず、実 node を再訪して検証する。 | paragraph または cell 内 paragraph。見出しは style を検証して `Heading`、それ以外は `Paragraph` / `TableCell`。 |
| `2 Spreadsheet { sheet_ordinal, row, col }` | XLSX/XLSM の workbook `<sheet>` 順の 0 始まり ordinal **だけ**を採用し、sheet ID/name は locator に含めない。row/col は sheet 全体の絶対 0 始まり cell 座標。`Range::used_cells()` の相対座標には `Range::start()` を加えてから記録。 | cached/display value を持つ一つの cell。formula を実行しない。 |
| `3 Pptx { slide_ordinal, shape_path, text_slot }` | slide は presentation 順の 0 始まり ordinal **だけ**。`shape_path` は `<p:spTree>` から group を含む各親の shape-like child の 0 始まり物理 index 列で、空列は禁止。`text_slot` は `ShapeParagraph { paragraph }` または `TableCellParagraph { row, col, paragraph }`。全値 0 始まりの物理 paragraph/row/cell。 | shape paragraph または table cell paragraph。 |
| `4 Pdf { page_index, char_start, char_end }` | pin した PDFium の当該 page `PdfPageText` character enumeration の 0 始まり **character index** 半開区間。`end > start`、同 page の列挙上限以下。Unicode scalar や UTF-8 byte offset と混同しない。bbox は任意の表示 metadata で ID に含めない。 | 一つの連続 text-layer 範囲。 |
| `5 Text { line_start, line_end }` | 厳密 decode 後、CRLF / CR / LF を separator とした物理 line の 0 始まり半開区間。終端 separator は追加の空 line を作らない。空の中間 line は座標に数える。`end > start`。 | 非空の連続 line 範囲。 |
| `6 Csv { record, field }` | 検証済み dialect で quote 内改行を保って parse した論理 record / field の 0 始まり ordinal。header も record 0。物理 line 番号は使わない。 | 一つの非空 field。 |
| `7 Html { text_node_path: Vec<u32> }` | HTML tree-builder が構成した Document root から text node まで、**text/comment/空白 node を含む全 child** の 0 始まり index 列。空列・非 text 終点は禁止。CSS selector、DOM element-only nth-child、line 番号は使わない。 | 一つの非空 text node。heading 判定は祖先構造から行う。 |
| `8 Archive { members: Vec<String>, inner: Box<BaseLocator> }` | 明示許可された ZIP container の 1 個以上の member chain。各 member は ZIP UTF-8 flag または profile で固定した decoder に従い厳密 decode → NFC。`/` 区切り相対 path、空成分、`.`、`..`、先頭 `/`、末尾 `/`、backslash、control、symlink、正規化後重複を拒否。case-sensitive。`inner` は tag 1–7 の一つで Archive 再帰を許さず、nested ZIP は member 列へ追加する。 | inner の UnitKind を保持。 |

`DocxStep` は `BodyBlock(u32)=1, Row(u32)=2, Cell(u32)=3, CellBlock(u32)=4`。`PptxTextSlot` は `ShapeParagraph=1, TableCellParagraph=2`。ZIP の member path は raw bytes と decoded path の一対一を検証し、同じ NFC path へ衝突する二 member、曖昧な非 UTF-8 decode、暗号化 member を受理しない。OOXML の内部 ZIP path を `Archive` と誤認しない。PDF OCR、HTML の外部取得/JS 実行、macro 実行は v1 scope 外である。

## 3. 正規化、profile、canonical UnitId

`text_normalization = nfc-lf-v1`。format 固有の厳密 decode 後、CRLF → LF、残る CR → LF、その後 Unicode NFC を施す。case、全角/半角、かな、空白、句読点を変えない。`text_sha256 = SHA-256(text.as_bytes())`。`TextSpan { start_byte: u32, end_byte: u32 }` はこの **正規化済み Unit.text の UTF-8 byte offset** の半開区間で、両端が char boundary、`start < end <= text.len()`。native PDF character range とは別座標である。全文 Unit と一致する text span を Source resolver が照合する。

`ExtractionProfileId` は `sha256:` + 64 小文字 hex の content-addressed ID。次の登録済み `ExtractionProfileDefinition` の **順序固定 binary encoding** の SHA-256 を値とする。`FormatId` 1–9 tag、`parser_name` と `parser_version` は ASCII identifier、`parser_build_sha256` は qualified parser artifact hash、`native_binary_sha256: Option<[u8;32]>` は PDF では必須、`scope_revision: u32`、`segmentation_revision: u32`、`normalization_revision: u32`（v1 は 1）、`locator_revision: u32`（v1 は 1）、`format_settings` は `None=0 | Text { charset: String }=1 | Csv { charset: String, delimiter: u8, quote: u8 }=2 | Archive { member_decoder: String }=3`、`limits` は `BTreeMap<BudgetKey, u64>`。`BudgetKey` の固定 tag は `InputBytes=1, ZipEntries=2, ZipEntryBytes=3, ZipTotalBytes=4, ZipDepth=5, Units=6, UnitUtf8Bytes=7, WorkerOutputBytes=8, XmlDepth=9, XmlNodes=10, PdfPages=11, PdfOperations=12, HtmlNodes=13, CsvRecords=14, CsvFieldBytes=15`。全 key を一度ずつ昇順に符号化し、非適用は 0。definition の encoding は `b"extraction-profile:v1\0"`、上記 field 順に下記の `frame`（数値は固定幅 BE payload、option は 1 byte discriminant + value、map は `u32 count` + tag `u8` + `u64 BE`）を付ける。profile registry は ID をこの定義から再計算し、parser build / PDFium pin / format settings / scope / segmentation / normalization / locator / budget のいずれかを変えた場合に別 ID を要求する。取得時の Source snapshot や generation は profile に入れない。

`NativeLocator` binary codec は `b"native-locator:v1\0" || tag:u8 || payload`。payload の各 `u32` は big-endian 4 bytes、各 `String` は `frame(UTF-8 NFC)`、各列は `u32 BE count` の後に固定順の要素、nested `inner` は `frame(inner locator bytes)` とする。tag 1 は `steps` の `(step tag:u8,index:u32)` 列、2 は `(sheet_ordinal,row,col)`、3 は `slide_ordinal, shape_path, text_slot tag, slot fields`、4 は `(page_index,char_start,char_end)`、5 は `(line_start,line_end)`、6 は `(record,field)`、7 は text-node path、8 は member path 列と inner。unsupported/unknown tag、余分な trailing bytes、非 canonical string は拒否する。

`frame(bytes) = len(bytes) as u32 BE || bytes`（len は `u32::MAX` 以下）。UUID は小文字文字列ではなく `Uuid::as_bytes()` の 16 bytes。`UnitId` は以下の 9 field を **記載順にそれぞれ frame** した SHA-256 値である。

```text
SHA-256(
  "knowledge-unit:v1\0" ||
  frame(source_id UUID bytes) || frame(resource_id UUID bytes) ||
  frame(source_native_version UTF-8 NFC bytes) ||
  frame(source_native_part_id UTF-8 NFC bytes) ||
  frame(logical_path UTF-8 NFC bytes) || frame(part.ordinal u32 BE) ||
  frame(profile_id ASCII bytes) || frame(NativeLocator v1 bytes) ||
  frame(unit.ordinal u32 BE)
)
```

ID の外部 codec は `ku1:` + 64 小文字 hex。generation、raw hash、text hash、parent ID、parser の OS path は UnitId に入れない。parser/profile が変わると profile ID が変わる。同じ locator に同じ profile で異なる text が現れた場合、ID を信じて evidence を再利用せず `text_sha256` と raw binding を照合する。

**Golden vectors (codec 受入):** source UUID `00000000-0000-0000-0000-000000000001`、Resource UUID `...0002`、native version `...0003`、synthetic profile ID `sha256:` + 64 個の `0`、`Text { line_start:0,line_end:1 }`、Unit ordinal 0。この locator の hex は `6e61746976652d6c6f6361746f723a763100050000000000000001`。part UUID `...0004` / path `primary` / part ordinal 0 なら `ku1:c6b9bfcc8a9d53ee19966146ccfce5a8b2f6f792f7cab53d4a9154377e867ca1`。同一 text/locator でも part UUID `...0005` / path `attachment` / part ordinal 1 なら `ku1:3b01330d059d71802ec8b3bc216ff9739b3765843892fe4b8fa9bdfa987b115e`。`東京\r\n` の normalized text は `東京\n`、text SHA-256 は `866bff0df548a00eaad416ca1fc987f20d94dae000fee8abd356dfb29bd15934`。synthetic profile ID は serializer fixture であり production profile registry の承認を意味しない。

## 4. Host validation と P2 cache

host は item ごとに `version` / `part` / authoritative representation / raw hash・size / profile / parser build を正本 snapshot と照合する。worker 起動前と応答後に immutable FileObject bytes の SHA-256 と size を照合し、**不一致なら Unit と新 generation を受理しない**。Unit は同一 part に属し、0..N-1 の重複なし ordinal、native traversal 順、重複しない locator を持つ。`parent_unit_id` は同じ Version/Part の前出 Unit だけを参照でき、自己参照・循環・不存在を拒否する。`UnitKind` と format/locator の組合せ、raw 上の locator round-trip、text 再構成、text digest、UnitId を全て再検証する。Unit 数・text 長・locator 深さは profile budget 以下。`Supported` の対象 Unit の脱落、範囲重複、未説明の範囲は拒否する。部分読取の条件は P1 revision の item coverage 契約に従う。

P2 の Vector 入力はこの検証済み `KnowledgeUnit` の `unit_id`, `text`, `text_sha256`, `provenance.profile`, 親 Version/Part と generation binding を読む。embedding cache key は少なくとも `(embedding_model_id, UnitId, text_sha256, ExtractionProfileId)` とし、hit には generation と Source-owned parent binding を保持する。`UnitId` 単独の cache hit を再利用しない。Vector の similarity hit は exact text evidence や `BodyRequired` の lexical exact-hit 条件を自動では満たさない。Vector の S1 Fusion 順と Source の Read 権限は既存契約のまま扱う。
