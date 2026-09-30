# P1 Search Extraction — 独立architecture review

- 判定: **NO-GO（現行案のままの `KnowledgeUnit` 契約freezeとP1本番経路着手）**。下記のP1指摘を設計へ反映し、契約・経路を再照合する。これは設計監査であり、本文検索の実装・動作確認ではない。
- 対象: `p1-extraction-design.md`（未commitのdraft）、承認済みSearch設計、Phase D受入、関連するDocument正本・Search/DSI実装。読取時のcheckoutは `feat/search-platform-completion-program@a877f447dcdc767be2a2e328886420fb4e7e8337`。ビルド・DB試験・ライブラリ資格試験は実行していない。

## Blocking findings

### [P1] `BodyRequired` を本文hitだけに拘束する実行経路が未定義

案の§3（行83）と§5（行114、118）は `LexicalFieldScope::BodyOnly` を置くが、現在の `DocumentCoveragePreflight::discover` は `BodyRequired` で検索を呼ばず、`DiscoveryService` の `lexical_query` はservice構築時の設定である（`crates/search-source-document/src/coverage.rs:17-43`、`crates/search-application/src/discovery_service.rs:56-69,167-179`）。Plannerは同じ評価でDirectory/Structured/Graphも実行でき、claim解決は正本title等のAssertionを拾う（`crates/search-application/src/retrieval.rs:34-43`、`crates/search-application/src/discovery_service.rs:608-640`）。Lexical portだけをBodyOnlyにしても、他のretrieverやtitle claimで `BodyRequired` が満たされたように見える経路が残る。

**修正:** trusted callerからのrequest単位のtyped body scopeを `DocumentCoveragePreflight` → planning/execution → lexical queryまで通し、`BodyRequired` のqualified resultには同一generationの検証済みUnit hitを必須にする。非本文retrieverのhitを本文一致として数えず、scopeを実行できない構成はtyped gap/errorで止める。no-hit時は対象となる**現在Read可能な**Live Versionの全authoritative item coverageだけを参照してabsenceを判定する。item ID、欠落理由、件数をgap/traceへ出す前にも現行Readを確認し、権限不明なら非開示のblocking gapにする。title-only、Graph-only、1件Partial、権限のないPartial、取消競合の縦断試験を必要とする。

### [P1] 本文を加えたgeneration digestは現行storeの検証と両立しない

案の§5（行115）は `ProjectionGenerationManifest.digest` に `BodyExtractionManifest` を加える。一方、現行 `DocumentOutboxIndexer` はprojection-only `generation_digest` を設定し（`crates/search-source-document/src/outbox.rs:793-807`）、`MemoryProjectionStore::validate_segment` は同じprojection-only関数で再計算して一致を要求する（`crates/search-projection-memory/src/store.rs:203-269,273-315`）。indexer側だけを複合digestにするとvalidationが必ず失敗する。現在の `DocumentIndexRuntime` にUnit/coverage artifactのstage/validate/discard操作もない（`crates/search-source-document/src/outbox.rs:76-122,665-684`）。

**修正:** schema v2の複合digestの入力と順序を共有契約として定義し、projection storeの再計算、runtimeのUnit/coverage staging・検証・cleanup、lexical/Graphとの同一key確認を同時に変更範囲へ入れる。`resource_count` は既存Resource projection数のままとし、Unit数を別に検証する。bodyだけ変更、CAS敗北、stage失敗、full/incremental同値で公開pointerと全artifactの不変条件を試験する。

### [P1] P2へ渡す `KnowledgeUnit` のcanonical identity/locatorがまだfreezeできない

案の§2（行24-77）は `UnitKind`、`UnitProvenance`、`CoverageReason`、`ExtractionProfileId` の正確な値・検証規則を定義していない。`Spreadsheet { sheet_id_or_ordinal }` と `Pptx { slide_id_or_ordinal }` は同一箇所を複数のcanonical locatorで表せる。PDFのcharacter rangeの座標系、DOCX block pathの根と表cell内path、archive member名の正規化、Text/CSVの行・record起点も未固定である。このままでは同じraw bytesから異なる `UnitId` を生成でき、P2のUnit参照・再抽出・根拠への往復が安定しない。`provenance` に必須とするraw hash/size、authoritative part binding、snapshot、parser buildも型として未確定である。

**修正:** `NativeLocator` のversion付き各variantについて座標・起点・正規化・一意性・round-trip規則を固定し、曖昧な `*_or_ordinal` をcanonicalな一方へ決める。Unit IDのdomain separator、長さ付きencoding、UUID表現、ordinal/親の順序、profile改訂条件、text digestの正規化をgolden vectorにする。`UnitProvenance` の必須フィールドとhost付与/worker非公開境界を型で示し、同一文字列の別ContentItem、同一FileObjectの複数item、parser版差、locator改変・循環の試験でfreezeする。P2のcacheはUnit ID単独を内容同一性とみなさず、`text_sha256`/profileを照合する。

### [P1] exact text spanをverified evidenceへ運ぶ経路がない

案の§3（行85）と§5（行114）はUnit hitから `AssertionOrigin::Extracted` の直接証拠を得る方針だが、現行 `LexicalRetrieverPort` は `Vec<FederatedCandidate>` を返し、`RawRetrievalHit` にUnit ID/part/span/raw digestを保持する型はない（`crates/search-application/src/ports.rs:230-255`、`retrieval_execution.rs:53-61`）。さらに `assemble_resource_claims` はgeneration内のstored Assertionとselectorからのみclaimを作り、現行 `DocumentEvidenceCatalog` が選択・解決できるのはtitle/document_type/categoryである（`crates/search-application/src/evidence_resolution.rs:21-52`、`crates/search-source-document/src/evidence.rs:20-45,158-247`）。candidateの汎用 `locator: String` だけでは、選ばれたUnitとexact spanの由来を検証できない。

**修正:** lexical hitからqualification/evidenceまで保存するprovider-neutral `KnowledgeUnitHitRef`（generation、parent Resource、ContentPart、Unit ID、span、text/raw digest、opaque locator）の境界を定義する。同一Resourceへのdedup後も選択Unitの参照を失わないことを定め、Source-owned resolverがpinしたUnit、正本ContentItem/representation、raw binding、locatorでの再読取、現在Readを検証したときだけexact text claimを `Extracted` とする。一般の本文一致を任意のSource権限・公開・policy claimのPRIMARYに昇格させない。同文面の別item、別Unitへの参照差替え、raw差替え、Read取消の試験を必要とする。

### [P2] failureとcoverageの写像が矛盾し、`Failed` の保存形が未定義

案の§2（行77）では `EncryptedDocument`、`ArchiveLimitExceeded` 等を `ExtractionFailure` とするが、§3（行81）は暗号化・サイズ超過を `Unsupported` の理由とし、§5（行112）はcorrupt/malformedをUnitゼロの `Failed` として公開する。`BodyCoverage` 型には `Failed` がなく、manifestにもoperation stateの型がない。worker result limit、ZIP/XML/PDF budget超過後の安全なUnitを `Partial` として出せる条件も定まらない。実装ごとにretry、metadata generation公開、blocking gapが異なり得る。

**修正:** worker failure code → item extraction state（Supported/Partial/Unsupported/Failed/Retryable）→ generation publish/retain → `BodyRequired` gapの表を契約に加える。`Failed` はcoverageと別のtyped運用stateにし、全authoritative itemを一度ずつ記録する。raw hash/size不一致とsandbox欠落は新generation公開禁止、確定的unsupportedとcorruptは明示state、temporary failureは旧generation保持と定める。limit到達時の途中Unit公開条件を厳密にし、`Supported` への昇格を禁止する。

## 確認できた設計上の適合点と実装時の注意

- Source正本と派生Unitの分離、Live現行PUBLISHEDだけのindex、DSI fingerprintを本文へ転用しない境界、S1のretriever優先順を変えない方針、Resource単位のcandidate identityは承認済みSearch/Document契約と整合する（案§1、§5、`spec/data/logical-data-model-v0.md:204-222`、Phase D受入 `search-discovery-platform-v0-acceptance.md:33-39`）。
- raw SHA-256/sizeをworker前後に照合し、fresh Linux sandboxを必須とする方針は妥当。既存DSI runnerはrlimitをexec前、Landlock/seccompをparser前に適用する（`crates/document-semantic-inspection-runner/src/linux.rs:54-83,185-201`、`sandbox.rs:1-39`）。Search workerで同等の強制を別途検証し、ZIP/OOXMLはparser前のpreflightと展開中の実byte/node budgetを双方必要とする。既存DSIの成功をSearch workerの証拠とは数えない。
- libraryはPoC候補のままにする判断が妥当。`office_oxide::Document::open` は拡張子でformat判定するため、format照合後の `from_reader(..., format)` を候補として評価する（[office_oxide 0.1.11 API](https://docs.rs/office_oxide/0.1.11/office_oxide/struct.Document.html)）。`calamine::Range::start()` は絶対座標、`used_cells()` のrow/colは相対座標なのでlocator生成時に加算が要る（[calamine 0.36.1 API](https://docs.rs/calamine/0.36.1/calamine/struct.Range.html)）。いずれも抽出忠実度の証拠ではない。案§7の形式別corpus、native locator往復、partial理由、資源・安全の有意味な試験とlicense/security選定が必要で、現時点でproduction dependencyを決定しない。

## 再判定の最小条件

上記P1の型と実行経路を案へ反映し、P2へ渡す契約golden vectorと `BodyRequired`/generation/evidenceの接続を図または型シグネチャで固定する。その後にP2指摘の写像表を確定してから、独立レビューでfreezeを再判定する。承認済みSearch v0のS1/Source authority意味を変更する必要はない。
