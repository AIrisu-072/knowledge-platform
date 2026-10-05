# P1 Search Extraction — 設計案

- Status: **DRAFT / independent review・freeze待ち**
- 対象: Search Platform Completion Program P1。本文を検索可能な `KnowledgeUnit` に変換し、既存Document SourceのLive generationへ接続する。
- 基準: Search / Discovery Platform v0承認済み設計 `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`、Phase D受入 `docs/superpowers/execution/search-discovery-platform-v0-acceptance.md`。
- この文書は設計であり、ライブラリ選定・本番実装・容量SLOの検証結果ではない。

## 1. 目的と保持する契約

`BodyRequired` に対して、権限のある現行Document Versionのauthoritative ContentItemから得た本文だけを検索し、元ファイルへ戻れるlocatorとcoverageを返す。P2のVectorは同じUnitを入力にできる。Documentの正本、DSIのsemantic fingerprint、Searchの派生projectionを混同しない。

保持する条件:

1. Sourceが正本を保持し、SearchのUnit・Indexは再構築可能。Document transactionとSearchを分散transactionにしない（承認済みSearch設計 §2、`spec/architecture/architecture-contract-v0.md` AC-06/07）。
2. Liveは現行 `PUBLISHED` のVersionだけ。明示IDのHistorical/Authoring tier、T10、T4、current `Read` と `ReadHistory` の既存意味を変えない（`search-source-document::DocumentSourceTranslator`、`DocumentCurrentAccessAdapter`）。
3. DSI結果に本文はない。DSIのparser・PDFium配備・sandboxの技術基盤は共有できるが、DSIのfingerprint/evidenceを本文に変換しない（Phase D受入「Coverage・history・evidence」、`spec/data/logical-data-model-v0.md` §2.6）。
4. Search v0のS1は**routed retriever順のpriority concatenation**。本文追加はLexical retriever内部のbody field/Unit hitであり、Fusion順、raw score比較、Graphのcanonical typed n-ary semanticsを変更しない。
5. `retention_mode` と `DiscoveryMode::LocalContentSearch` が本文保持を許すSourceだけが永続body indexを作る。NO_RETENTION SourceのbodyをDocumentの永続Indexへ混入しない。

## 2. 最小provider-neutral契約（P2へのfreeze対象）

`search-core::knowledge_unit` を正本型の候補とする。Document固有UUIDやStorageKeyをcanonical Unit型へ埋め込まない。Source adapterがDocument IDへ対応づける。

```rust
struct ResourceVersionRef {
    source_id: SourceId,
    resource_id: ResourceId,          // DocumentではVersionのKnowledge Resource
    source_native_version: String,   // DocumentVersionIdの正規UUID文字列
}
struct ContentPartRef {
    source_native_part_id: String,   // DocumentではContentItemId
    logical_path: String,
    ordinal: u32,
}
struct RawBinding { sha256: [u8; 32], size_bytes: u64, media_type: String }
struct ExtractionTarget {
    version: ResourceVersionRef,
    part: ContentPartRef,
    authoritative_representation_ref: String,
    raw: RawBinding,
    profile: ExtractionProfileId,
}
enum BodyCoverage {
    Supported,
    Partial { reasons: Vec<CoverageReason> },
    Unsupported { reason: CoverageReason },
}
struct KnowledgeUnit {
    unit_id: UnitId,
    version: ResourceVersionRef,
    part: ContentPartRef,
    parent_unit_id: Option<UnitId>,
    ordinal: u32,
    kind: UnitKind,
    text: String,                    // 検索用のlossless範囲内正規化済みtext
    locator: NativeLocator,
    text_sha256: [u8; 32],
    provenance: UnitProvenance,       // raw digest, parser/profile, Source snapshot
}
struct ExtractionBatch {
    target: ExtractionTarget,
    detected_format: FormatId,
    coverage: BodyCoverage,
    units: Vec<KnowledgeUnit>,
    canonical_digest: [u8; 32],
}
trait ContentExtractor {             // Application port。worker protocolとは分離
    async fn extract(&self, target: ExtractionTarget, content: BoundedContent)
        -> Result<ExtractionBatch, ExtractionFailure>;
}
```

`NativeLocator` はversion付きtyped enum。最低限 `Docx { block_path, table_row?, table_cell? }`、`Spreadsheet { sheet_id_or_ordinal, sheet_name, row_start, col_start, row_end, col_end }`、`Pptx { slide_id_or_ordinal, shape_path }`、`Pdf { page_index, char_start, char_end, rect? }`、`Text { line_start, line_end }`、`Csv { record, field? }`、`Html { dom_path }`、`Archive { member_chain, inner }`。DOCXのページ番号やPDFの論理段落など、parserから確定できない位置は創作しない。外部へは検証可能なopaque locator codecを使い、StorageKeyや生のfilesystem pathは載せない。

`UnitId` は`source_id + resource_id + source_native_version + source_native_part_id + profile + canonical native locator + unit ordinal`の長さ付きcanonical encodingからSHA-256で決定する。generation IDは含めない。`text_sha256` はNFC・改行・構造区切りの正規化後UTF-8に対するSHA-256。Unit全体の`canonical_digest`は順序付きUnit ID、親、locator、kind、text digest、coverageとprofileから計算する。衝突、重複locator、親不在、循環、ordinal逆転、別Version/Partへの帰属は受理しない。同じVersionの複数ContentItemは `source_native_part_id` と `logical_path + ordinal` の双方で区別する。

Trusted hostは`ExtractionTarget`とread-only bytesを作る。sandbox workerへは**profile、format、expected raw SHA-256/size、bytes/FD、数値budgetだけ**を渡す。Source ID、Document/File ID、StorageKey、actor、secret、外部URLは渡さない。workerはnative locatorを返し、hostがSource/Version/Partとprovenanceを付けて上記不変条件を検証する。`ExtractionFailure`は `UnsupportedFormat`、`CorruptDocument`、`EncryptedDocument`、`MalformedArchive`、`ArchiveLimitExceeded`、`TextExtractionFailed`、`TemporaryExtractorFailure`、`RawBindingMismatch`、`SandboxUnavailable`、`ResourceLimitExceeded` を区別する。安全に解析した `Partial` と失敗を同一視しない。

## 3. Coverageと検索の意味

Coverageは**ContentItemごと**に計算し、Document Versionでは全authoritative itemの集合として集約する。`Supported` は定義済みreader-visible text scopeの全範囲が抽出・locator付与・検証された場合のみ。`Partial` は得られたUnitを検索可能にするが、未読範囲と理由を記録する。`Unsupported` はUnitゼロ。未知形式、暗号化、OCR必須、未対応構造、サイズ超過を区別する。parser failureは別の `Failed` 運用状態として記録し、成功coverageを偽装しない。Rendition、DSI result、VBA実行結果はauthoritative bodyへ混ぜない。

`BodyRequired` はtyped `LexicalFieldScope::BodyOnly` で実行し、title一致だけで満たさない。全Itemが `Supported` のときにだけbody全体のnegative absenceを判断可能とする。`Partial`/`Unsupported`/`Failed` が1件でもあればitem-scopedのblocking `UnsupportedCoverage` gapを残し、検索結果ゼロを不存在証拠にしない。検索可能Unitに肯定hitがあれば、結果は出せるがVersion全体のcoverageは完全と表示しない。本文scopeを選べないquery pathは `BodyRequired` を実行せずtyped error/gapで止める。通常title/許可metadata検索は従来どおり動く。

Lexical hitだけで任意の事実claimをPRIMARYにしない。exact text spanのclaimを扱う場合のみ、generationにpinしたUnit、parent Version、ContentItem、raw digest、locator、現在のDocument `Read`、body text digestをresolverで再照合し、`AssertionOrigin::Extracted` の直接証拠として扱う。Sourceのpublication/authorization/policyの主張にbody文字列を昇格させない。複数Unitが同じraw FileObjectに由来する場合、独立Source数を増やさない。

## 4. Format adapterと選定

パーサは `search-extraction-worker` 内の `FormatExtractor` として差し替える。共通の `plain_text()` 一括結果からlocatorを逆算しない。次は**PoC候補**であり、Search Extraction用途のproduction dependency選定は別途corpus評価・license/security確認後とする。既存DSI用途のSELECTEDを自動昇格しない。

| 形式 | 最初のbody scopeとnative locator | 候補・PARTIAL / UNSUPPORTED条件 |
| --- | --- | --- |
| DOCX | main bodyの段落、見出し、表の行・cell。body block index / table座標。 | `office_oxide 0.1.11` のtyped `Body.elements`を主候補。header/footer、脚注、text box、変更履歴などreader-visible範囲の取りこぼしがあればPARTIAL、曖昧な構造はerror。 |
| XLSX | worksheetの表示値、sheet + 絶対row/col、表range。 | `calamine 0.36.1` の`Range.start()`と`used_cells()`、`rxls 0.1.3`を独立照合候補。formulaは実行せずcached valueのみ。欠落/古いcache、chart text、非表示scopeを判定できない場合はPARTIAL。 |
| XLSM | XLSXと同じworksheet本文。 | macroは実行しない。VBA sourceを本文scopeに含めないことを明示し、macro内容を含む全体coverageはPARTIAL。 |
| PPTX | slide内のvisible shape text、表cell、slide/shapeのIDまたは順序。 | `office_oxide 0.1.11` のtyped slide modelを候補。notes、chart、SmartArt、隠しslide等の対象判定に欠落があればPARTIAL。 |
| PDF | text layerのpage・character range、妥当ならbbox。 | 既存pinの `pdfium-render 0.9.4` + PDFium `151.0.7881.0` を候補。`lopdf 0.45.0`を構造照合に使用。scanned/image-onlyは `RequiresOcr`、文字順/visibilityが曖昧ならPARTIAL。OCRはP1外。 |
| TXT | 厳密decode後の行range。 | 既存`encoding_rs`とUnicode正規化。BOM/宣言charsetを優先し、無根拠なlossy推測をしない。判定不能encodingはUNSUPPORTED。 |
| CSV | 厳密decodeしたrecord・field。 | `csv 1.4.0`。delimiter/quote/charsetをSource契約か検証済み規則から確定し、曖昧なdialectはPARTIALまたはUNSUPPORTED。 |
| HTML | text nodeをheading/DOM順にまとめ、DOM path。 | `html5ever 0.39.0` + `markup5ever_rcdom`。script/style/templateを索引しない。CSS/JS後の視覚表示を保証できないものはPARTIAL。外部参照は取得しない。 |
| ZIP container | 正規化されたmember pathとinner locatorの連鎖。 | `zip 8.6.0`を候補。明示的に許可されたSourceのcontainerだけ再帰解析する。VersioningのZIPは搬送手段であり、ContentItemのauthoritative意味として自動追加しない。暗号、重複/曖昧path、symlink、unsupported codec、深さ/展開量超過はUNSUPPORTED/失敗。 |
| DOC/XLS/PPT・その他 | なし。 | legacy形式のライブラリ宣伝だけでSUPPORTEDにしない。Search専用corpus資格までUNSUPPORTED。 |

Runtimeの `SUPPORTED/PARTIAL/UNSUPPORTED` は拡張子だけで決めず、magic、宣言MIME、container内構造、実際の解析結果を照合する。MIME/拡張子不一致を黙って別形式として採用しない。日本語ではNFCと改行正規化だけをExtraction側で行い、全角/半角、漢字、かな、句読点、空白による意味差を潰さない。形態素/Analyzer版はLexical projection側の責務とする。

公式API確認: [office_oxide typed DOCX](https://docs.rs/office_oxide/0.1.11/office_oxide/docx/index.html)、[office_oxide Document API](https://docs.rs/office_oxide/0.1.11/office_oxide/struct.Document.html)、[calamine Range](https://docs.rs/calamine/0.36.1/calamine/struct.Range.html)、[rxls typed cells](https://docs.rs/rxls/0.1.3/rxls/)、[PdfPageText](https://docs.rs/pdfium-render/0.9.4/pdfium_render/prelude/struct.PdfPageText.html)、[zip-rs](https://docs.rs/zip/8.6.0/zip/)；これらはAPI存在の根拠であり、本Repositoryでの抽出忠実度証拠ではない。

## 5. 正本から同一generationの本文Indexまで

1. `PostgresDocumentSnapshotReader::read` の単一 `REPEATABLE READ, READ ONLY` transactionで、既存Version/live/history/T10/access revisionに加え、各Versionの `content_items` と **AUTHORITATIVE** `content_representations` と `file_objects` を `(ordinal, logical_path, content_item_id)` 順に読む。`content_item_id`、representation/file ID、logical path、hash、size、MIME、StorageKeyをtrusted read modelに封じる。旧`version_files`やrenditionから補完しない。破損した参照/重複はSnapshot integrity error。
2. `DocumentOutboxIndexer` はLiveへ翻訳したVersionだけをP1 body対象にする。trusted adapterが `FileStorage::open` でimmutable objectを上限付きで読み、FileObjectのsizeとSHA-256をworker前とworker応答後に照合する。**byte digest不一致は全body index生成を拒否**し、旧generationを保持してintegrity incidentを出す。FileObject IDやStorageKeyはworkerへ渡さない。
3. `ContentExtractor` が各itemのUnit/coverageを返す。hostは全Unitのparentが同じResource Versionと同じContentItemであること、raw binding、profile、locator、text digest、順序、上限を検証し、`BodyExtractionManifest`（item ID、raw digest、profile、coverage、Unit count/digest）を組む。temporary/worker failureは新generationを公開せず再試行対象にする。確定的Unsupportedはmetadata generationと明示coverageを公開できる。corrupt/malformedはUnitゼロの `Failed` 状態として記録し、同じ失敗を無限retryしない。
4. `DocumentSourceTranslator` のtitle/許可metadataの既存入力は維持する。`LexicalBuildInput` にSource-owned `KnowledgeUnitIndexInput` を別collectionとして追加する。`SourceSuppliedBody` に全Versionの文字列を連結しない。`search-tantivy` schema v2は従来のResource docとUnit docを区別し、Unit docにparent ResourceId、UnitId、ContentPartRef、native locator、text、text digestを持つ。`manifest.resource_count`はResource projection数のまま、Unit数は別検証値。body fieldだけにUnit textをindexし、title/alias/high-signalをUnit docへ複写しない。
5. Queryは同じpinned generationを読む。Lexical retriever内で従来field優先順を保持し、body tierに複数Unit hitを入れる。`BodyOnly`はbody tierのみ。Unit候補の`candidate_id`は既存current-access guardが認める **`source_id:parent Version ResourceId`**、`resource_ref`もparentで、UnitId/ContentItem/locatorは検証済み内部hitに保持する。異なるContentItemのUnitを同一locatorとして潰さない。同一Resourceの複数Unitはretriever内で決定的に代表hitを選び、候補数の上限はUnit数でなくResource数に適用する。bounded refill/overflow traceを設け、同一文書の大量Unitが他文書をwindowから押し出さない。P2は同じUnit ID/text/digestを読むが、S1 Fusionは変更しない。
6. `ProjectionGenerationManifest.digest` の導出に既存projection digestに加え、順序付き `BodyExtractionManifest` のdigest、extraction profileとlexical schema版を入れる。`coverage`（Source enumeration）へbody coverageを流用せず、generation-keyed body coverage artifactとして検証・公開する。projection、lexical、Unit/coverage artifact、Graphを同じkeyで私有構築・検証してからSource pointer CAS。CAS負け/失敗では全artifactを破棄し、公開済み世代と混ぜない。full rebuildとincrementalは同じraw binding/profileから同一Unit/digestを作る。
7. extraction中に正本が変わり得るため、公開直前にcurrent Version/T10、document/access revision、全authoritative item manifestを再確認し、変化なら破棄して再読する。DBとの分散transactionは作れないので、その直後の変更はoutbox/reconciliationで収束させ、query時の現行Version + `Read` 再認可で古いUnitの露出を遮断する。Source outageやCAS failure時は旧generationを維持し、body coverageを成功扱いにしない。

既存 `DocumentCurrentAccessAdapter::evaluate` はcanonical candidate IDと現行Versionを再確認する（`crates/search-source-document/src/postgres.rs`）。`DocumentCoveragePreflight` の `BodyRequired` 固定Unsupported分岐は、同じgenerationのbody coverageとtyped BodyOnly executionへ置換する。P5 HTTPはこの入口だけを呼び、公開requestからSource-owned raw file/locatorを注入させない。History本文はP1 Live indexへ流さず、後続の別tier設計まで明示IDの既存History lookupを維持する。

## 6. Sandbox・資源・ログ

ProductionはLinuxの**fresh process / mandatory sandbox**だけでuntrusted bytesをparseする。DSI runnerのLandlock・seccomp・FD閉鎖・rlimit・bounded stdout/stderr・temp監視・timeout・PDFium pin/SHA-256検証を共通runner infrastructureへ抽出して再利用し、既存DSI protocol/挙動を回帰で固定する。DSIの`WorkerResponse`やsemantic fingerprintをSearch本文に流用しない。macOS parser parityは開発/CIの補助証拠であり、Linux sandbox enforcementの代わりにしない。sandbox不可、native PDFium binding不一致、制限未適用はfail closed。

初期の絶対admission ceilingはDSIで実装済みの安全側境界を上限として採用する: input 256 MiB、ZIP entry 20,000、1 entry展開64 MiB、全展開512 MiB、worker仮想メモリ2 GiB、scratch 1 GiB、result 16 MiB、wall 10秒/CPU 8秒。Search固有の最大Unit数、Unit text長、ZIP深さ、XML depth/node数、PDF page/operation数、HTML DOM node数、CSV cell/row数はPoCでこの上限内に明示budgetを決め、受理前・解析中・結果検証時の三層でチェックする。budget超過は失敗/明示coverageであり、途中Unitを無言でSUPPORTEDとして公開しない。ZIPは中央directoryと実展開bytesの両方を検証し、path traversal、重複、Unicode正規化衝突、symlink、重なり、暗号、nested bombを拒否する。OOXML内ZIPも同じpreflight対象。

Queryと通常ログへ本文、抽出text、ファイル名、StorageKey、snippet、秘密情報を無条件出力しない。trace/metricはformat、status/reason code、byte/Unit count、処理時間、resource high-water、profile/generationのopaque IDのみ。利用者向けsnippetが必要な場合はcurrent access再確認後、generation/Unitに束縛した明示APIで生成する。Process stdoutは構造化応答だけ、stderrは上限付き固定codeへ変換する。

## 7. 資格試験と採用判定

顧客データを使わず、合成・公開ライセンスを確認したcorpusで、固定expected Unit text・kind・親・順序・native locator・coverage reasonをmanifest化する。少なくともDOCX見出し/段落/表、XLSX/XLSM日本語セルとformula cache、PPTX shape/slide/table、PDF複数page/縦組・文字順・image-only、TXT BOM/CP932宣言、CSV引用改行、HTML DOM/非表示、ZIP入れ子を含む。各Unit locatorから元の同一hashファイルの該当箇所へ戻れることを独立oracleで確認し、**同じ文字列が複数ContentItemに現れるケース**を必須にする。Supported判定の対象text/locator取りこぼしは0件、Partial/Unsupportedは理由が期待値と一致することをgateにする。日本語は全角/半角・濁点結合・改行・句読点・セル座標の回帰を含める。

安全系は実バイトhash/size不一致、MIME spoof、壊れたXML/OPC/PDF、暗号化、ZIP bomb/path/symlink/重複、HTML深さ、CSV巨大field、parser panic、worker kill、sandbox欠落を注入する。0件の漏洩/無制限実行/誤ったSupported、旧generation維持、部分artifact掃除を確認する。Access取消・T10・Version差替え・race中のQuery、`BodyRequired` とtitle一致、partial coverage下の否定的absence禁止を実PostgreSQL縦断で確認する。DSIのLinux sandbox/PDFium pin gateも再実行し、Search worker自体のLinux enforcementを別途検証する。

性能は1/10/50 MiB級の各format代表、最大Unit・多数小ContentItem、圧縮率の高いZIPでcold/warmを分け、wall/CPU時間、peak RSSまたはcgroup high-water、scratch、output bytes、Unit数、index build時間、Query p50/p95/p99とbody候補のunique Resource充足率を測る。10秒/2 GiB等のceilingを超えるitemをSupportedにしない。採用reportにはcorpus別成功率、Partial/Unsupported内訳、最悪case、環境、parser/PDFium/lock hash、p95/p99、回帰差分を残す。ここで観測した値からP7/G1の容量・SLO案を作り、未測定の本番SLOを主張しない。`POC REQUIRED`候補は資格試験、license/security、独立レビューで選定されるまでproduction Cargoへ追加しない。

## 8. 実装Taskと変更範囲の提案

| Task | 変更候補と受入観点 |
| --- | --- |
| P1-1 contract | `crates/search-core/src/knowledge_unit.rs`、`crates/search-extraction-core/{src,tests}`。Unit ID/locator/coverage/digest不変条件とprovider-neutral serializer。P2へこの契約をfreeze。 |
| P1-2 parser PoC | `experiments/search-extraction-poc/`、合成/公開corpusとlibrary比較。format別fidelity・日本語・locator・資源を測り、採用を記録。 |
| P1-3 sandbox | 共通process runner infrastructure、`crates/search-extraction-runner`、`crates/search-extraction-worker`。DSI runnerのsecurity parityを壊さない。root Cargo/lock変更はparentの直列調整。 |
| P1-4 Document binding | `crates/search-source-document/src/{postgres,model,extraction,outbox}.rs`。同一snapshotの全authoritative item、raw digest照合、same-generation CAS/cleanup。 |
| P1-5 lexical | `crates/search-tantivy/src/{schema,index,query}.rs` と対象tests。Resource/Unit別doc、canonical parent candidate、BodyOnly、unique Resource window、S1不変。 |
| P1-6 coverage/evidence | `crates/search-source-document/src/{coverage,evidence}.rs`、必要最小限の`crates/search-application/src/{ports,discovery_service}.rs`。gap、current access、Unit解決。 |
| P1-7 vertical/assurance | 実PostgreSQL/FS/sandbox縦断、rebuild/CAS/race、安全・format corpus、`mise run verify:fast`の対象gate、独立read-only review。 |

P1の完了判定は、実装・focused gate・独立review・exact-head hosted verificationの証拠を別々に記録する。P1設計だけでは本文検索は稼働していない。
