# P1 Search Extraction — architecture repair revision 1

- Status: **設計修正版 / 独立再レビュー待ち**。`p1-extraction-design.md` は原案のまま保持する。本書と `p1-knowledgeunit-contract.md` を合わせて P1 の実装入力とする。設計記述は production 実装、parser 資格、CI、配備の証拠ではない。
- 対象: `p1-extraction-architecture-review.md` の P1 4件と P2 1件。Search / Discovery Platform v0 の承認済み設計、Phase D の Live / History・Source 正本・S1 priority concatenation・現行 `Read` は維持する。
- 現行実装の接点: `crates/search-source-document/src/{coverage,outbox,postgres,evidence}.rs`、`crates/search-application/src/{discovery_service,retrieval_execution,ports,evidence_resolution}.rs`、`crates/search-projection-memory/src/store.rs`、`crates/search-tantivy/src/{index,query,schema}.rs`。現行 `ProjectionGenerationManifest.digest` は projection-only であり、`DocumentCoveragePreflight::BodyRequired` は固定 `UnsupportedCoverage` を返す。

## 1. Finding → 修正の対応

| Review finding | 本書の閉じ方 |
| --- | --- |
| P1 `BodyRequired` が title / Graph と混ざり、request 単位の body scope がない | §2 の trusted `BodySearchSpec` を preflight → planner → executor → lexical port → qualification に貫通。body scope では body Unit hit 以外を qualified と数えない。現在 Read で絞った全 authoritative item だけで absence を判定。 |
| P1 projection digest 再計算と body digest が衝突 | §3 で既存 `manifest.digest` と `generation_digest()` を projection-only のまま維持。別の generation bundle receipt に Unit / coverage / lexical / Graph digest を固定し、同一 key の staging・validation・CAS・discard・pin を runtime が管理。 |
| P1 Unit identity / locator / provenance が未凍結 | `p1-knowledgeunit-contract.md` §1–4 が provider-neutral 型、NativeLocator v1、profile/UnitId の byte encoding と golden vector、raw/part 検証、P2 cache key を規定。§4 が Document 正本へ結ぶ。 |
| P1 Unit hit から exact evidence へ渡す参照がない | §5 の `KnowledgeUnitHitRef` と Source-owned exact-text selector / resolver を追加。retriever 内 dedup 後も選択 Unit を保持し、同一 raw / locator の再読取と現在 Read が成功した exact-text claim だけを `Extracted` にする。 |
| P2 `Failed` / retry / coverage / publish の写像不一致 | §6 の item operation state と全 failure matrix。全 item を一度記録、途中 Unit の危険な publication を禁止、永久失敗と一時失敗を分ける。 |

## 2. `BodyRequired` の request 単位の実行と gap

`DiscoveryRequest` 自体は intent / evidence ID であり、外部 caller に Source の raw file、Unit locator、`ExtractionProfileId`、coverage を指定させない。Document の trusted adapter / P5 transport が認証済み request と検索語から次を作る。既存 `DiscoveryService::discover(request)` は metadata 用に残す。

```rust
// 既存 LexicalQuery { text, limit } に field_scope を追加。
enum LexicalFieldScope { ExistingFields, BodyOnly }
struct LexicalQuery { text: String, limit: usize, field_scope: LexicalFieldScope }
struct BodySearchSpec {
    query: LexicalQuery,             // field_scope == BodyOnly、非空、上限内
    exact_text_claim: Option<ClaimId>, // trusted selector 登録済みの場合だけ
}
enum DiscoveryScope { Normal, BodyRequired(BodySearchSpec) }
impl DocumentCoveragePreflight {
    async fn discover(service: &DiscoveryService<'_>, request: DiscoveryRequest,
                      requirement: DocumentCoverageRequirement,
                      body: Option<BodySearchSpec>) -> Result<DiscoveryResult, SearchError>;
}
impl DiscoveryService<'_> {
    async fn discover_scoped(&self, request: DiscoveryRequest,
                             scope: DiscoveryScope) -> Result<DiscoveryResult, SearchError>;
}
```

`BodyRequired` では `body` が必須で、trusted caller が claim ID と exact phrase の対応を登録する。preflight は `BodyOnly` を service の構築時 `DiscoveryConfig.lexical_query` に頼らず **この request** で渡す。planner には Document Source の Lexical action だけを executable として渡し、Directory / Structured / Graph / Vector / probe はこの scope の body match を作れない。実装上ほかの retrieval が文脈に必要でも、body qualification 用集合には一切混ぜない。`LexicalRetrieverPort` の `BodyOnly` 未実装、lexical port 未接続、body bundle 未公開、action limit で未実行、scope の取り違えは typed `UnsupportedCoverage` または `Availability` の blocking gap と空の body qualified result。通常 scope の retriever 順・S1 `PriorityConcat` と Lexical 内の既存 canonical name → aliases → title → high signal → body の順は変えない。`BodyOnly` は body tier のみを実行する。

`BodyOnly` の Tantivy v2 は Resource doc の title/alias/high-signal/body を読まず、Unit doc の body field だけを検索する。Source-owned Unit を P1 body query の候補にし、Resource へ折りたたんでも `LexicalHit.unit_hit` を保持する。phrase match は Unit.text の `nfc-lf-v1` text で一致範囲を UTF-8 byte offset として確定する。Tantivy の score/token hit だけで exact span を作れなければ、その hit は `BodyRequired` の qualified result にしない。dedup は同一 parent Resource の Unit から `(body tier, score 降順, Unit ordinal, UnitId)` で決定的な代表を選び、unique Resource 数に `limit` を適用する。Unit の大量 hit による他 Resource の starvation は bounded refill と underfill trace で検出し、window を使い切った場合は completeness を主張しない。candidate identity は既存 current-access guard が要求する `"{source_id}:{parent Version ResourceId}"`、`resource_ref` は親 Version の `ResourceId` のまま。

executor は retrieval 時と結果公開直前に `DocumentCurrentAccessAdapter` で親 Version の current `PUBLISHED`、T10 非終了、現行 `Read` を確認する。`BodyRequired` の qualification は **同じ pinned generation の staged Unit manifest に実在する `KnowledgeUnitHitRef`、current Version/authoritative part/raw binding、現在 Read の全検証が成功した body hit** に限る。`HitRecord` / `RawRetrievalHit` の `unit_hit` を落とさない。title Assertion や Graph path によって `EvidenceSufficiency::Sufficient` になっても、body proof が無ければ body scope の `complete()` は false、qualified body resources は空である。肯定 hit の exact evidence は §5 でさらに raw bytes を再読取する。旧 generation にしか存在しない Unit は、current Version / part 照合で失効する。

No-hit の absence は次の **全条件** でのみ `document.body.contains_exact` の `ClaimState::Absent`（または同等の限定された no-match 判断）へ進める: (1) `BodyOnly` が同一 pinned bundle の全 matching Unit を調べ、`LexicalRetrievalBatch.exhausted_matching_units=true` を返し、現行 Read 後の可視 hit が 0 件、(2) trusted Source reader が現在の Live Version 全件と各全 authoritative ContentItem を列挙した、(3) caller の現在 `Read` を各 Document で確認した後の可視 item 集合が、pinned body coverage artifact の item identity・raw binding と一致する、(4) その可視 item がすべて `Supported` である。全 index の match が 0 件なら (1) は満たせるが、先頭 window が権限外 hit だけで尽きた場合は、後続の可視 hit があり得るため満たせない。bounded window の途中終了、port error、未実行でも absence は出さない。`Partial` / `Unsupported` / `Failed` が可視 item に一つでもあれば blocking `UnsupportedCoverage` gap とし、否定的 absence は出さない。新しい可視 Version/part、raw 変更、source outage、権限再確認の不確定は blocking `Availability` gap。肯定 hit があっても可視 corpus に gap があるなら該当 Resource は返せるが全体 completeness は主張しない。

gap / trace / count / item ID / reason の公開前に当該 Document の現在 Read を再確認する。Denied item は全情報と件数から除外し、Denied 側の Partial は可視 corpus を blocking しない。Unknown/error は ID と件数を伏せた単一 blocking gap に畳む。Read 取消と同時に変化した場合は候補、claim、rank、trace、item gap を再検査して伏せ、absence も出さない。対象を限定しない「全 Source に本文がない」という主張は作らない。

## 3. 同一 generation の immutable bundle

**既存の `ProjectionGenerationManifest.digest` は変更しない。** `search-projection-memory::generation_digest(source_id, resources, registry)` の projection-only v1 値のまま store が再計算し、`resource_count` は従来の Resource projection 数である。`ProjectionGenerationManifest.coverage` も Source enumeration を意味し、body coverage に転用しない。原案 §5.6 の「manifest.digest に body を混ぜる」は撤回し、以下を別の runtime-owned receipt とする。

```rust
struct BodyItemEntry {
    version: ResourceVersionRef,
    part: ContentPartRef,
    authoritative_representation_ref: String,
    raw: RawBinding,
    detected_format: FormatId,
    profile: ExtractionProfileId,
    operation: ItemOperationState,      // §6。published は Completed または FailedPermanent
    coverage: Option<BodyCoverage>,     // Completed のとき Some、FailedPermanent は None
    units: Vec<KnowledgeUnit>,          // Failed/Unsupported は空
}
struct BodyUnitManifest {
    key: ProjectionGenerationKey,
    source_snapshot: String,
    entries: Vec<BodyItemEntry>,         // 全 Live authoritative item を厳密に一度ずつ
}
struct BodyCoverageArtifact {
    key: ProjectionGenerationKey,
    items: Vec<(ResourceVersionRef, ContentPartRef, ItemOperationState,
                Option<BodyCoverage>, RawBinding, u32 /* unit_count */)>,
}
struct ArtifactReceipt { key: ProjectionGenerationKey, digest: [u8; 32], count: u64 }
struct GenerationBundleReceipt {
    key: ProjectionGenerationKey,
    source_snapshot: String,
    projection_digest: [u8; 32],
    unit_manifest: ArtifactReceipt,
    body_coverage: ArtifactReceipt,
    lexical: ArtifactReceipt,
    graph: ArtifactReceipt,
    profile_set_digest: [u8; 32],
    lexical_schema_version: String,   // P1 = "schema-2"
    composite_digest: [u8; 32],
}
```

各 `ArtifactReceipt.key` は `GenerationBundleReceipt.key` と一致しなければならない。`projection_digest` は `manifest.digest` の `sha256:` hex を decode した 32 bytes。`BodyUnitManifest` は `(ResourceId, part.ordinal, part.logical_path, part ID)` 昇順で、Version/part/representation/raw/profile/format/operation/coverage、各 ordinal 順 Unit の ID/親/kind/locator/text digest を長さ付き canonical bytes に符号化し、`SHA-256("body-unit-manifest:v1\0" || bytes)` とする。text 本文は digest 入力に含めず、Unit.text から再計算した text digest の一致を validate する。coverage artifact は同じ item 集合からのみ導出し、item identity/raw/operation/coverage/unit_count を同順に `SHA-256("body-coverage:v1\0" || bytes)`。profile set は各 entry の `(profile ID, parser build)` を重複排除・昇順符号化し、`SHA-256("body-profile-set:v1\0" || bytes)`。canonical field は UTF-8 NFC、UUID 16 bytes、整数 BE、列 count u32 と `frame(bytes)=u32 BE length || bytes`。None/Some と enum は固定 tag を付け、未知 tag/非 canonical bytes は拒否する。Source snapshot と generation ID は artifact の **照合 field** であり、rebuild equivalence digest からは除く。

Lexical receipt digest は schema/analyzer、Resource doc の全許可 field、Unit doc の `(parent ResourceId, UnitId, part, locator, text_sha256, body text)` の deterministic input 順から生成し、Tantivy の segment file bytes を使わない。Graph receipt digest は既存 typed n-ary relation と Document owner mapping の canonical staged input から生成する。`composite_digest = SHA-256("document-generation-bundle:v1\0" || source UUID bytes || projection_digest || unit_manifest.digest || body_coverage.digest || lexical.digest || graph.digest || profile_set_digest || frame(lexical_schema_version UTF-8) || u64 BE unit_manifest.count || u64 BE body_coverage.count)`。各 digest/count は staging data から **runtime が再計算**し、indexer 申告値を盲信しない。`unit_manifest.count` は Unit 総数、`body_coverage.count` は authoritative item 数。body のみ変更しても composite digest が変わる。

`DocumentIndexRuntime` に `stage_body_unit_manifest(key, manifest)`、`stage_body_coverage(key, artifact)`、`validate_bundle(key) -> GenerationBundleReceipt`、`pin_current_bundle(source_id) -> Option<(ProjectionGenerationManifest, GenerationBundleReceipt)>`、`discard_body_generation(key)` を加える。lexical / Graph builder は `ArtifactReceipt` を返し、既存 `validate_generation` は projection-only 検証として維持する。`MemoryDocumentIndexRuntime` はすべてを共有する runtime lock 下で `Staging → Validated → Published | Discarded` を管理し、validated 後は artifact を変更できない。`publish_if_current(key, expected_current)` は **同じ lock の下で**全 receipt/key/count/digest の再検証後に既存 projection pointer CAS を最後に呼ぶ。CAS false/障害/stage 失敗なら Graph ownership・Graph・lexical・Unit/coverage・projection の当該未公開 key を全て discard し、旧 pointer と公開済み artifact を保持する。cleanup 失敗は incident として返し、未公開 artifact を query へ出さない。公開後 receipt 書込みだけ失敗した場合は公開 bundle を保持し、同じ event の再試行で receipt を補完する。

`DocumentOutboxIndexer::reconcile_once` は projection digest と composite digest、schema/profile を no-op / `IndexingReceipt` の比較対象にし、旧 projection-only generation に bundle が無ければ body 対応済みとはみなさず再構築する。full rebuild と incremental は同じ Source snapshot item ordering、raw binding、profile、Unit canonicalization から同じ各 digest を得る。incremental で変更のない Unit を再利用するときも `key` と snapshot を新 generation に bind し、全 item 検証を省かない。query は `pin_current_bundle()` の一つの結果から全 port を pin し、欠落/不一致なら body scope を blocking gap で停止する。公開済み旧 generation は in-flight pin が消えるまで immutable に保つ。

## 4. Document 正本からの一回の binding

`PostgresDocumentSnapshotReader::read` の単一 `REPEATABLE READ, READ ONLY` snapshot に `content_items`、その **AUTHORITATIVE** `content_representations`、immutable `file_objects` を `(ordinal, logical_path, content_item_id)` 順で加える。各 Live Version の item が一つ以上あり、item ごとに authoritative representation がちょうど一つ、FileObject が実在し、path/ordinal が Document の規範に合うことを検証する。旧 `version_files`、rendition、DSI result、macro 実行結果から補わない。`VersionSnapshotRecord` に同 snapshot の read-only `Vec<AuthoritativeItemBinding>`（ContentItemId、path、ordinal、representation/file ID、hash/size/MIME、trusted StorageKey）を追加する。`DocumentOutboxSnapshot` の source snapshot、Version/T10、document/access revision と一緒に保持する。

trusted adapter が `FileStorage::open(StorageKey)` から上限内で bytes を読み、FileObject SHA-256/size と **worker 前・worker 応答後**に照合する。不一致は integrity incident、全新 generation 公開禁止、旧 pointer 維持。Source/item/representation を worker へ渡さず、worker から返った locator/text は `p1-knowledgeunit-contract.md` の ID、scope、round-trip、profile 不変条件を host が再検証する。全 authoritative item の `BodyItemEntry` を一度ずつ作り、§6 の publication-safe state だけを stage する。Live は現行 `PUBLISHED` Version のみ。History は明示 Document/Version ID の既存 `Read` + `ReadHistory` lookup に残し、P1 Live body index へ紛れ込ませない。

公開直前には新たな正本 read で current Version/T10、document/access revision、全 authoritative item binding を比較する。変化したら未公開 key を discard し、同じ `reconcile` の bounded retry で再読取する。Source DB と Search の分散 transaction は作らない。CAS 直後の変更は outbox/reconciliation で追い、query 側の現行 Version/Read/part 検証により古い Unit を結果へ出さない。Source outage、整合性不明、CAS failure では旧 pointer 維持、body coverage を成功として補完しない。

## 5. Unit hit → verified exact-text evidence

現在の `LexicalRetrieverPort::retrieve -> Vec<FederatedCandidate>` と `RawRetrievalHit` のみでは Unit 参照を運べない。P1 の provider-neutral 境界を次に変更する。`opaque_locator` は host が staged Unit と比較できる locator codec / handle で、StorageKey や公開 request 由来の path ではない。

```rust
struct KnowledgeUnitHitRef {
    generation: ProjectionGenerationKey,
    parent_resource: ResourceId,
    version: ResourceVersionRef,
    part: ContentPartRef,
    authoritative_representation_ref: String,
    unit_id: UnitId,
    span: TextSpan,                   // Unit.text の正規化済み UTF-8 byte 範囲
    text_sha256: [u8; 32],
    raw: RawBinding,
    profile: ExtractionProfileId,
    opaque_locator: String,
}
struct LexicalHit { candidate: FederatedCandidate, unit_hit: Option<KnowledgeUnitHitRef> }
struct LexicalRetrievalBatch {
    hits: Vec<LexicalHit>,
    exhausted_matching_units: bool, // BodyOnly の全 matching Unit を調べた場合のみ true
}
// LexicalRetrieverPort::retrieve(...) -> BoxFuture<'a, LexicalRetrievalBatch>
// RawRetrievalHit / HitRecord に unit_hit: Option<KnowledgeUnitHitRef> を追加。
struct ExactTextSelector {
    claim_id: ClaimId,
    parent_resource: ResourceId,
    predicate: String,               // v1 は "document.body.contains_exact" のみ
    expected_exact_text: String,     // nfc-lf-v1、非空、trusted caller の claim binding
}
struct VerifiedExtractedTextEvidence {
    assertion: Assertion,            // origin == AssertionOrigin::Extracted
    resolved: ResolvedAssertionEvidence,
    matched_span: TextSpan,
}
trait ExactTextEvidencePort: Send + Sync {
    fn selector_for<'a>(&'a self, generation: ProjectionGenerationKey,
                        claim_id: ClaimId) -> BoxFuture<'a, Option<ExactTextSelector>>;
    fn resolve_hit<'a>(&'a self, request: &'a DiscoveryRequest,
                       hit: &'a KnowledgeUnitHitRef,
                       selector: &'a ExactTextSelector)
        -> BoxFuture<'a, Option<VerifiedExtractedTextEvidence>>;
}
```

Tantivy は Unit doc 内の actual Unit text を照合して `span` と Unit ref を返す。Source adapter `DocumentExactTextEvidenceCatalog` は staged Unit manifest に同 key/parent/part/UnitId/locator/text digest/profile/raw/representation が存在すること、選択 span が UTF-8 char boundary で `expected_exact_text` と一致することを確認する。次に現行 Version/T10 と authoritative ContentItem/representation/FileObject binding、現行 Read を確認し、同じ raw hash/size の Source bytes を再読取して同じ profile/locator で text span を再構成する。Read と binding を公開直前にもう一度確認する。途中で変化・不明・不一致なら `None` と blocking gap、candidate/claim/trace を伏せる。`ResolvedAssertionEvidence` は同じ generation/source/parent/evidence ref を返し、`content_digest = text_sha256`、`is_summary=false`、Source-owned upstream origin、直接の citation chain を持つ。`Assertion` の subject は parent Version、predicate は上記 allowlist、value は exact text、origin は `Extracted` とする。

`assemble_resource_claims` の stored title/document_type/category Assertion 経路はそのまま維持し、別関数 `assemble_verified_unit_text_claim(selector, verified)` が **required ClaimId と同じ selector** の exact-text claim のみ追加する。`EvidenceRole::Primary` はこの限定述語の直接 text 存在証拠にだけ許す。一般の本文 hit / vector similarity /汎用 candidate locator は任意の publication、access、policy、metadata、事実 claim の PRIMARY にならず、未検証なら `ClaimState::Unknown` のまま。`SourceId` は同じ FileObject や複数 Unit で増殖しない。claim を返す直前にも既存 service の current-access 再検査を適用する。

## 6. failure、item state、coverage、公開判定

```rust
enum BodyCoverage {
    Supported,
    Partial { reasons: Vec<CoverageReason> },
    Unsupported { reason: CoverageReason },
}
enum CoverageReason {
    RequiresOcr, UnsupportedFormat, Encrypted, UnsupportedStructure,
    UnsupportedEncoding, UnsupportedDialect, UnsupportedCodec,
    MissingFormulaCache, AmbiguousReadingOrder, DynamicVisibility,
    ResourceLimit,
}
enum PermanentFailureCode {
    CorruptDocument, MalformedArchive, TextExtractionFailed,
    WorkerOutputLimit,
}
enum RetryableFailureCode { SourceIo, WorkerUnavailable, WorkerKilled, Timeout }
enum ItemOperationState {
    Completed,
    FailedPermanent { code: PermanentFailureCode },
    Retryable { code: RetryableFailureCode }, // build journal only; published manifest 禁止
}
```

`Completed` は coverage `Some`。`FailedPermanent` は coverage `None`・Unit 0。`Retryable` も coverage `None`・Unit 0 だが **新 generation を公開しない**。公開する `BodyUnitManifest` と `BodyCoverageArtifact` は current Live の全 authoritative item を一度ずつ記録し、`Completed` または `FailedPermanent` のみを含む。`Partial` は `Vec<CoverageReason>` が空でなく、少なくとも一つの locator 検証済み Unit と、reader-visible scope の既知の未読範囲/理由を持つ。reader が全対象を最後まで列挙し、既知 omission 以外に欠落がない場合だけ可。`Unsupported` は Unit 0。`Supported` は対象 scope が全て読取・locate 済みで、text が本来空なら Unit 0 も可。処理中の hard budget 中断、出力切断、panic、kill から途中 Unit を `Partial` や `Supported` として公開しない。

| worker / host outcome | item state と Unit | 新 generation / 再試行 | 可視 `BodyRequired` gap |
| --- | --- | --- | --- |
| 全 reader-visible scope を検証 | `Completed + Supported`、完全 Unit | bundle validate 後 publish 可 | なし。全可視 item が同じ条件なら no-hit absence 可。 |
| traversal 完了、位置と理由の分かる省略のみ | `Completed + Partial(reasons)`、検証済み Unit のみ | publish 可。全体 completeness 不可 | その Read 可能 item に blocking `UnsupportedCoverage`。 |
| 未対応形式/構造・暗号化・OCR 必須・encoding/dialect/codec 非対応・pre-admission limit | `Completed + Unsupported(reason)`、Unit 0 | publish metadata + coverage 可。同じ raw/profile は自動 retry しない | Read 可能 item に blocking `UnsupportedCoverage`。 |
| ZIP/XML/PDF/CSV/HTML 等の parse 中 budget を trusted worker が**構造化された確定 code**で報告 | `Completed + Unsupported(ResourceLimit)`、**途中 Unit 全破棄** | raw 前後照合と code 検証後 publish metadata 可。budget/profile を変えた時のみ再抽出 | Read 可能 item に blocking `UnsupportedCoverage`。 |
| corrupt document、malformed archive、決定的 text extraction failure | `FailedPermanent(code)`、coverage None、Unit 0 | metadata + typed failure artifact publish 可。同じ raw/profile 無限 retry 禁止 | Read 可能 item に blocking `UnsupportedCoverage`、absence 禁止。 |
| trusted runner の result byte cap 超過 | `FailedPermanent(WorkerOutputLimit)`、coverage None、Unit 0。切断応答は破棄 | raw 前後照合後 metadata + failure publish 可。同じ raw/profile の自動 retry 禁止 | Read 可能 item に blocking `UnsupportedCoverage`。 |
| temporary Source I/O、worker unavailable/kill、timeout、runner が分類不能の途中終了 | `Retryable(code)`、coverage None、Unit 0 | 新 generation 全破棄、旧 pointer 維持。既存 bounded retry 後も不成功なら incident と停止、同じ操作を無限 replay しない | pinned 旧 bundle と現在可視 item が不一致なら非開示 blocking `Availability`。 |
| raw hash/size/representation 不一致、sandbox 不在、native pin 不一致、Unit/locator/provenance 検証失敗 | item 成功・失敗として偽装しない | **新 generation 公開禁止**、全 artifact discard、旧 pointer 維持。integrity/config incident。変更なしの自動 retry 禁止 | 非開示 blocking `Availability` / `UnsupportedCoverage`。 |
| Source revision/Version/T10/item manifest 変化、pointer CAS 敗北、artifact stage/validate 失敗 | item state ではない build failure | 未公開 key 全 discard、旧 pointer 維持。Source 変化/CAS のみ bounded reread retry | current 可視 corpus に旧 coverage を流用せず blocking `Availability`。 |

Coverage reason は item の本文可読性だけを表し、Source enumeration の `Coverage::CompleteEnumeration` を上書きしない。永続 `FailedPermanent` は `BodyCoverage::Failed` という架空の variant を作らず、operation state で表す。外部 gap の詳細は §2 の現在 Read filter を通した item に限定する。失敗 code と raw hash/profile の組で再処理の抑止・profile 改訂後の再抽出を区別する。

## 7. 実装ファイル、検証、freeze 条件

| Task | 最小変更範囲と受入 |
| --- | --- |
| P1-1 core freeze | `crates/search-core/src/knowledge_unit.rs` と `crates/search-extraction-core/`。`p1-knowledgeunit-contract.md` の型、codec、golden vector、同文面の別 ContentItem、同 FileObject の別 item、profile/parser 差、parent/locator 破損を検証。独立 review 後に P2 にこの契約だけ渡せる。 |
| P1-2 Source snapshot/extraction | `crates/search-source-document/src/{model,postgres,outbox,extraction}.rs`。同一 snapshot の item/representation/FileObject 結合、FileStorage 読取、raw 前後照合、全 item manifest、公開直前再照合。format parser は原案 §4/§7 の合成・公開 corpus、license/security で選定し、`POC REQUIRED` を資格前に production Cargo に入れない。 |
| P1-3 sandbox | 共通 runner infrastructure と `crates/search-extraction-{runner,worker}/`。Linux fresh process の Landlock/seccomp/FD/rlimit/temp/stdout-stderr/timeout と PDFium pin を Search worker 自身で検証し、既存 DSI protocol 回帰を保持。sandbox 不可は fail closed。macOS parser parity は補助証拠。 |
| P1-4 lexical/bundle | `crates/search-tantivy/src/{schema,index,query}.rs`、`crates/search-projection-memory/src/store.rs`、`crates/search-source-document/src/outbox.rs`。Resource/Unit 別 doc、body field のみの Unit、scope/unique Resource refill、projection-only digest と composite bundle の再計算、same-key stage/CAS/cleanup/pin。 |
| P1-5 coverage/evidence | `crates/search-source-document/src/{coverage,evidence,postgres}.rs`、`crates/search-application/src/{ports,retrieval,retrieval_execution,discovery_service,evidence_resolution}.rs`。trusted per-request BodyOnly、現在 Read による item gap filter、exact Unit DTO と Source resolver、body hard proof、title/Graph-only 排除。 |
| P1-6 focused / vertical | 合成 fixtures と実 PostgreSQL/FS/Linux sandbox の縦断。title-only/Graph-only/正しい Unit hit、同文面の別 item、hit 差替え、Read 取消/Unknown、Partial・Unauthorized Partial・新 Version、body-only 変更、full/incremental 同値、CAS 敗北、stage 失敗、raw mismatch、budget/worker failure 全表を検証。`mise run verify:fast` の対象 gate と独立 read-only 再レビューを実施。 |

原案の resource ceilings、形式別 fidelity corpus、日本語回帰、ログ非開示、性能測定は引き続き適用する。P1 の production 完了は実装、focused/縦断 gate、独立 review、exact-head hosted gate を別々に記録して判断する。本修正版に human approval gate や Search v0 の Design Freeze 変更は追加しない。
