# P1 Search Extraction — 改訂案の独立architecture recheck

- 判定: **NO-GO（このexact改訂案の本文経路freeze）**。元レビューの主要な型・経路の欠落は大きく改善されたが、`BodyRequired` の否定判定を誤らせるP1が2件、exact-text claimの結合にP2が1件残る。別のKnowledgeUnit最小契約freezeも独立したgateであり、この判定に含めない。
- 対象: `p1-extraction-design-revision-1.md` SHA-256 `b1e84e476f614eb30e3bd1c5982e7e0ff2a35e33aef9cddd069618315dcbe742`、元の `p1-extraction-architecture-review.md`、`p1-knowledgeunit-contract.md`、承認済みSearch設計とPhase D受入、関連する現行実装。設計・ソースの読取監査であり、実装・parser資格・build・DB/CI・配備の検証ではない。

## 残るblocking findings

### [P1] lexical Unit docとauthoritative Unit manifestの全件対応が検証条件にない

改訂案の `BodyUnitManifest` は全itemとUnitを持ち、lexical receiptは別途Unit doc入力から計算する（改訂案:67-94）。`validate_bundle` は各receiptのkey/count/digestを再計算するが、**各検索可能Unitが同じ本文・parent・part・locator・digestでちょうど1件のlexical Unit docになったこと**、余分なdocがないことを要求していない（改訂案:94-98）。同一keyの二つの正しいdigestを束ねても、両artifactの意味上の一致は証明されない。現行Tantivy builderもLexical入力をSource snapshotのsubsetにできる（`crates/search-tantivy/src/index.rs:181-187`）。

反例: `Supported` itemのmanifestに本文「東京」のUnitを残し、そのUnit docだけをlexical stageから落とす。両receiptのdigest/countは各自の入力と一致するので現在の記述だけならpublishできる。query「東京」でindexの全matching Unitを走査して `exhausted_matching_units=true` となり、現行Readとcoverageも一致して、実在する文字列を `Absent` にできる（改訂案:47）。

**必要な修正:** publish前のbundle validationに、`Completed + Supported/Partial` の全Unitとlexical Unit docの一対一照合を加える。generation、parent Version、part、UnitId、locator、profile、raw/text digest、実際に索引へ渡した正規化本文を比較し、`Unsupported/FailedPermanent` はUnit docゼロとする。索引の構築結果が入力を欠落させた場合もpublish不可にする。Unit doc欠落・重複・別item差替え・本文差替えと、全item `Supported` 下の偽のno-hitをREDで固定する。

### [P1] `contains_exact` の否定に必要な検索完全性とclaim文字列の結合がない

`BodySearchSpec.query.text` と `exact_text_claim` は独立した値で、`ExactTextSelector.expected_exact_text` との同一性検査が規定されていない（改訂案:21-29,41,133-150）。`BodyOnly` はTantivy Unit docのbody tierを検索し、返ったhitについてだけUnit.text上のexact spanを確認する（改訂案:43,154）。しかし `exhausted_matching_units` は**索引が列挙したmatch**の走査完了であって、`nfc-lf-v1` 本文中の全exact文字列を取りこぼさず候補化した証明ではない。現行schemaはdefault tokenizerのbody field、現行queryはTantivy phrase queryである（`crates/search-tantivy/src/schema.rs:58-73`、`query.rs:46-66`）。schema v2がliteral substringを完全に拾う契約は改訂案にない。

反例: trusted callerがquery「甲」、claim期待値「乙」を結合しても型上受理でき、本文に「乙」があって「甲」のhitがなければ§2の条件で「乙」を `Absent` にできる。またUnit.text内の「東京都」に対する部分文字列「京」のように、token/phrase索引が候補を出さない場合もexact containmentの否定にはならない。Unit境界を跨ぐ文字列を述語の対象とするかも未指定である。

**必要な修正:** `document.body.contains_exact` の比較対象を明示する（1 Unit内か、連続するUnitを結合した本文か）。claim用の正規化済みexact文字列をrequest queryへ同一値として束縛し、不一致なら `Absent` を禁止する。その比較対象に対して漏れのない検索手段または全Unitのexact走査を定め、`exhausted_matching_units` がその検査全体の終了を意味する場合に限り、現在Readで絞った全item `Supported` と合わせて `Absent` にする。日本語部分文字列、句読点・改行・NFC、Unit境界、query/selector不一致を試験する。

### [P2] exact-text selectorの親Resource照合をresolver契約へ明記する

`ExactTextSelector` は `parent_resource` を持つが（改訂案:133-138）、`resolve_hit` の列挙された検証はhitとmanifestの同一parent、spanと期待textの一致、現行Version/Readであり、**`selector.parent_resource == hit.parent_resource`** を明記していない（改訂案:144-156）。Resource Aのrequired claimに、同じ文面を持つResource Bの検証済みhitを結び付けないことが契約から一意に分かる必要がある。

**必要な修正:** resolverと `assemble_verified_unit_text_claim` の双方でselectorのClaimId、親Version ResourceId、許可述語、正規化期待値をrequestのrequired claimおよびhit/verified evidenceと照合し、不一致は `Unknown` と非開示gapへ落とす。別Resourceに同じtextがあるfixtureとclaim/hit差替えを追加する。

## 改訂案で解消を確認した範囲

- `BodyRequired` はtrusted request単位の`BodyOnly`をpreflight→planner→executor→lexicalへ渡し、title/Graphだけのhitでbody qualificationを満たさず、現在Readにより可視itemのgapとtraceを絞る（改訂案:17-49）。この方向は元レビューの実行経路・非開示指摘に合う。上記の否定判定を修正するまで、`Absent` のfreezeには使えない。
- 既存 `ProjectionGenerationManifest.digest` と`resource_count`をprojection-onlyに保ち、Unit/coverage/lexical/Graphの別receiptを同一keyでstage/validate/CAS/discard/pinする。これは現行storeのprojection-only再計算と矛盾しない（改訂案:51-98、`crates/search-projection-memory/src/store.rs:203-269`）。上記のartifact間の全件対応が追加条件となる。
- `KnowledgeUnitHitRef` をretrieval/dedup後まで保持し、Source-owned resolverがUnit、current Version/part/raw、locator再読取、現行Readを照合してから限定的な`Extracted` evidenceにする設計は、元レビューの汎用candidate locator問題を閉じる方向である（改訂案:108-156）。上記selectorの親照合は残る。
- `Completed + Supported/Partial/Unsupported`、`FailedPermanent`、非公開`Retryable`、integrity/config failureを分け、全authoritative itemの記録、途中Unit破棄、旧pointer保持とgapを表で固定した（改訂案:158-198）。元レビューのfailure/coverage/P2写像はこの範囲で解消している。

## 独立gateとinterface調整

- `p1-knowledgeunit-review.md` は当時の最小契約SHA-256 `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe` にArchive profileとVector cacheの2件でNO-GOを出している（同レビュー:3-9）。この修正と再レビューは別担当のgateとし、本書はUnit最小契約のfreezeを代行しない。P1全体freezeは本文経路とUnit契約の両方を消費する。
- P1改訂案:35-38の`DiscoveryService::discover_scoped(request, DiscoveryScope)`は、P4安定planの`discover_scoped(request, ScopedDiscoveryExecution<'_>)`（`p4-remote-plan.md:172-176`）とRustの同名overloadになる。本文側を例えば`discover_with_content_scope(request, DiscoveryScope)`へ改名し、同じ内部evaluation loopへ渡すよう、実装前にinterfaceを一致させる。これは狭い名前・統合契約の調整であり、上記P1/P2の代替gateではない。

次のexact actionは改訂案へ上記の検証不変条件・exact否定条件・selector照合と対応試験を追記し、新しいSHA-256で再度独立architecture reviewを行うこと。実装着手・production完了・CI GREENは本書から推論しない。
