# P1 本文 exact absence・lexical seal 追補

- Status: **設計追補 / 独立再判定待ち**。`p1-extraction-architecture-recheck.md` の P1 2件、P2 1件と P1/P4 の公開メソッド名衝突を閉じるための実装入力であり、production 実装・検証完了の証拠ではない。
- 適用順: 本書は `p1-extraction-design-revision-1.md` §2 の `DiscoveryService` 宣言、§2 の no-hit/`Absent` 条件（特に行47）、§3 の bundle validation、§5 の exact selector 解決・claim 組立、§7 の対応試験について優先する。それ以外は同改訂案、`p1-knowledgeunit-contract.md` と Archive/Vector 追補、承認済み Search 設計を維持する。元文書は変更しない。

## 1. P1/P4 が共有する Discovery 入口

```rust
impl DiscoveryService<'_> {
    async fn discover(&self, request: DiscoveryRequest)
        -> Result<DiscoveryResult, SearchError>; // 既存入口
    async fn discover_with_content_scope(&self, request: DiscoveryRequest,
        scope: DiscoveryScope) -> Result<DiscoveryResult, SearchError>; // P1
    async fn discover_scoped(&self, request: DiscoveryRequest,
        context: ScopedDiscoveryExecution<'_>) -> Result<DiscoveryResult, SearchError>; // P4
}
// ScopedDiscoveryExecution.content_scope: DiscoveryScope
```

P1 の `discover_scoped(request, DiscoveryScope)` は上記 `discover_with_content_scope` に改名する。P4 の `discover_scoped(request, ScopedDiscoveryExecution)` は維持する。両入口と `discover(request)` は同一の内部 evaluation loop に渡す薄い adapter とし、本文判定専用の第二の business loop を作らない。P4 の Source/routing/lease 境界と P1 の `BodyRequired` preflight、`BodyOnly`、qualification は各既存契約に従う。

## 2. 公開前の Unit ↔ lexical doc seal

`validate_bundle(key)` と `publish_if_current` の再検証では、receipt の個別 digest/count が合うだけでは不足する。同じ `ProjectionGenerationKey` の `BodyUnitManifest` と**構築後に実際に検索可能な** lexical Unit doc 集合を、両方向で一対一照合する。`Completed + Supported/Partial` の全 Unit はちょうど1件の doc に対応し、`Completed + Unsupported` と `FailedPermanent` の item の doc は0件とする。余分、欠落、重複した doc は拒否する。

各対応は Source、generation、親 `ResourceVersionRef`/`ResourceId`、`ContentPartRef`、authoritative representation、`RawBinding`、`UnitId`、ordinal、kind、locator、profile、`text_sha256` と、索引に渡した正規化 `Unit.text` の全bytesを比較する。manifest の本文 digest はその text bytes から再計算する。builder の入力だけでなく構築結果の doc を列挙し、staging receipt の key/count/digest と実doc集合を照合する。seal が成立しなければ新 generation を publish せず、§3 の全未公開 artifact を discard して旧 pointer を保つ。正しい個別 digest を持つ別々の subset を合成しても seal を通さない。この seal は通常の本文indexの忠実性条件であり、tokenizer による literal substring の完全候補化を証明するものではない。

## 3. `contains_exact` の対象と trusted binding

v1 の `document.body.contains_exact` は、指定された**一つの現行 Live 親 Version Resource** の、現在 `Read` 可能な全 AUTHORITATIVE item のうち、`Completed + Supported` のいずれか一つの `KnowledgeUnit.text`（`nfc-lf-v1`）に、非空の期待文字列が**同一 Unit 内で連続した UTF-8 literal substring**として存在する述語である。case・幅・かな・空白・句読点を変えない。CRLF/CR→LF、次に NFC という既存正規化を期待文字列にも同じ順で適用する。Unit 間の連結、別 item 間の連結、History、header等の profile 対象外はこの v1 述語に含めない。連結本文に関する強い主張をこの述語の `Absent` として代用しない。

trusted adapter は `BodySearchSpec.query.text`、`ExactTextSelector.expected_exact_text`、request の required claim の値を同じ正規化済み非空bytesに束縛する。`query.field_scope == BodyOnly`、`exact_text_claim == selector.claim_id == required ClaimId`、predicate の allowlist、selector の親 `ResourceId` と required claim の subject、対象 Source/Version を検証する。不一致・未登録・曖昧な親・上限超過では exact absence を実行せず `Unknown` と非開示の blocking gap にする。lexical analyzer が処理した query や `exhausted_matching_units` をこの同一性検証の代わりにしない。

## 4. Source-owned exact negative proof

```rust
struct ExactScanBudget {
    max_visible_items: u64, max_units: u64, max_text_bytes: u64,
    deadline: Instant,
}
struct ExactTextNegativeProof {
    generation: ProjectionGenerationKey,
    bundle_digest: [u8; 32],
    source_snapshot: String,
    claim_id: ClaimId,
    parent: ResourceVersionRef,
    document_revision: i64,
    access_revision: i64,
    predicate: &'static str, // document.body.contains_exact
    exact_text_sha256: [u8; 32],
    visible_item_bindings_digest: [u8; 32],
    scanned_unit_bindings_digest: [u8; 32],
    visible_item_count: u64,
    scanned_unit_count: u64,
}
enum ExactTextAbsenceOutcome {
    ProvenAbsent(ExactTextNegativeProof),
    MatchFound, // 内部結果。これだけで公開 hit/evidence は作らない
    Unknown(InformationGap), // 既存型。blocking == true、非開示の required_fact
}
trait SourceExactTextAbsencePort: Send + Sync {
    fn verify_absence<'a>(&'a self, request: &'a DiscoveryRequest,
        pinned: &'a PinnedBodyBundle, selector: &'a ExactTextSelector,
        budget: ExactScanBudget) -> BoxFuture<'a, ExactTextAbsenceOutcome>;
}
```

`PinnedBodyBundle` は `pin_current_bundle()` で得た同一 immutable Unit manifest・coverage・bundle receipt を参照する trusted handle であり、外部 caller は構築できない。`ExactTextNegativeProof` の field/constructor は実装上 private とし、登録済み Source-owned port の検証済み scan だけが発行する。`Unknown(InformationGap)` は既存型を使い、`blocking=true` と非開示の `required_fact` に固定する。budget は**有限**で、既存 P1 の resource ceilings と採用済み profile の Unit/text 上限の内側にある実行設定から渡す。ここで新しい顧客別閾値や無制限 scan は定義しない。上限に達したら `Unknown` と blocking `Availability`、`Absent` は作らない。

Source-owned port は selector の親について現行 Live `PUBLISHED`/T10、全 AUTHORITATIVE `ContentItem`/representation/FileObject を trusted Source から列挙し、各親への現在 `Read` を評価する。`Denied` は scan、receipt の count/digest、公開 gap/trace から除外し、`Unknown`/error は ID・件数を伏せた blocking gap に畳む。claim 対象の親が `Denied`/`Unknown` なら `ProvenAbsent` にしない。可視 item 集合と pinned manifest/coverage の item identity、Version、Part、representation、raw SHA-256/size/MIME、profile、operation/coverage、Unit数を全件照合し、可視 item はすべて `Completed + Supported` でなければならない。item 0 件の推定、`Partial`/`Unsupported`/`FailedPermanent`、manifest/coverage の欠落・過剰、現行 binding の変化は `Unknown` と対応する blocking gap にする。

各可視 item の **全** Unit を ordinal 順に走査し、同じ pinned manifest の UnitId/parent/part/representation/raw/profile/locatorと `SHA-256(Unit.text)` を検証してから、正規化済み期待bytesの literal substring を `Unit.text` の UTF-8 char boundary で調べる。lexical Unit doc の候補集合や analyzer を scan 対象の決定に使わない。一つでも一致すれば `MatchFound` とし、lexical no-hit との食い違いを内部 integrity/recall signal と blocking `Availability` gap にする。`MatchFound` だけで公開 hit/evidence を作らず、`Absent` も出さない。全可視 item・全 Unit を期限/件数/bytes内で走査し、Source 正本、current Version/T10、Read、全 Part/raw binding と pinned generation/receipt を証明発行直前に再照合できた時だけ、Source port が一時的な `ProvenAbsent` receipt を発行する。receipt の二つの digest は、検証した可視 item binding 列と Unit binding列の決定的順序・長さ付きencodingから計算する。本文そのもの、StorageKey、Denied の ID/件数は receipt、trace、log に含めない。

`BodyOnly` の no-hit と `exhausted_matching_units=true` は scan を起動する契機にすぎず、exact否定の必要十分な証拠ではない。`ProvenAbsent` は当該親・当該 ClaimId・当該期待文字列の述語にだけ使い、Source 全体、別親、連結本文、一般の Discovery 完全性へ拡張しない。scan が未実行、途中終了、timeout/outage、digest不一致、scope不一致、権限再確認不明、公開直前の変更なら `Unknown` と非開示 blocking gap にし、古い receipt を流用しない。結果組立と公開直前にも Source-owned 現行 Read/Version/Part/raw、document/access revision と receipt の binding を照合し、変化なら claim、rank、trace、gap詳細を伏せる。

## 5. 肯定 evidence と claim 組立の親照合

`DocumentExactTextEvidenceCatalog::resolve_hit` は `selector.parent_resource == hit.parent_resource == hit.version.resource_id`、required claim の subject ResourceId、`selector.claim_id`、許可 predicate、正規化期待bytes、hit の span、本書 §3 の request binding をすべて等値検査する。`assemble_verified_unit_text_claim(selector, verified)` も同じ ClaimId/親 Version ResourceId/Source/predicate/valueを required claim と `VerifiedExtractedTextEvidence` の Assertion/ResolvedEvidence に再照合する。異なる親に同じ文字列があっても流用せず、失敗・不明は `Unknown` と非開示 blocking gap にする。`Absent` を組み立てる側も §4 の Source-owned receipt を同じ claim/親/期待文字列/current authority へ束縛し、任意の caller や lexical port は negative proof を mint できない。

## 6. 局所 RED/受入ケース

- seal: `Supported` の Unit doc を1件落とす、重複させる、別 item/親へ差し替える、本文だけを変える、余分な doc を加える。個別 receipt が正しくても publish は失敗し、旧 pointerを保つ。`Partial` の検証済み Unit も全件照合し、`Unsupported`/`FailedPermanent` の doc は0件。
- no-hit: `query.text="甲"` と selector `"乙"` は `Unknown`。`東京都` 内の `京` は tokenizer が0 hitでも `MatchFound` と blocking gap。句読点、CRLF→LF、NFC 合成、日本語部分文字列は normalized literal に従う。`東` と `京` を別 Unit に分割した場合の `東京` は v1 単一Unit述語では非一致とし、連結本文の absence に昇格しない。`Supported` で Unit 0 の item は全件照合後だけ proof 可。
- current authority: 可視 item の Partial/Failed、Unit欠落、manifest/coverage digest不一致、Version/Part/raw差替え、Source outage、Read Unknown/取消、scan 件数/bytes/deadline超過では `Absent` 0件。Denied item の存在/件数/理由は漏らさず、Denied Partial だけで可視親の proof を阻害しない。
- claim: 親 A と親 B に同じ phrase があり、A の selector に B の verified hit を渡しても `Extracted`/`Absent` にしない。required ClaimId/subject/predicate/value の差替えも `Unknown`。公開直前の Read 取消では receipt と claim を破棄する。
