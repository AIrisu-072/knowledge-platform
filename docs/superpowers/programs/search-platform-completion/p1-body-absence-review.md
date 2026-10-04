# P1 本文 exact absence 追補 — 独立最終レビュー

- 判定: **NO-GO（この合成設計の P1 本文 freeze）**。前回の P1 2件、P2 1件と P1/P4 の名称衝突は追補で閉じた。ただし、`Partial` の検証済み Unit からの肯定 claim と、新しい `document.body.contains_exact` 述語の対象に、実装時に真偽を誤らせる P1 の矛盾が1件残る。
- 判定対象: `p1-extraction-design-revision-1.md` SHA-256 `b1e84e476f614eb30e3bd1c5982e7e0ff2a35e33aef9cddd069618315dcbe742`、`p1-knowledgeunit-freeze.md` SHA-256 `0fe9ceb84472633d06c52d84f2f62c86bc783f880751c935a7d773b1c8fdfc84`、`p1-body-absence-amendment.md` SHA-256 `49838833ff293cf20032e46dbe9429847a975fad8fcb40f21fd977eaa8b69ae1`、前回独立再審査 `p1-extraction-architecture-recheck.md` SHA-256 `b7f6f4c5b044fd3bcbcd459819d9b9e5791939d213b858e2f613b6b56099c519`。承認済み Search 設計と P4 plan の接点も照合した。Unit 最小契約は別の独立 GO と freeze をそのまま採用し、Archive・Vector・golden vector を再審査していない。

## 前回指摘の閉鎖

1. **P1 lexical 実 doc ↔ Unit manifest: 閉鎖。** 追補:22–26 は同一 key の構築後に実際に検索可能な lexical Unit doc を列挙し、`Supported/Partial` の全 Unit と Source、親 Version、Part、representation、raw、UnitId、ordinal、kind、locator、profile、text digest、正規化本文 bytes を双方向で一対一照合する。`Unsupported/FailedPermanent` の doc は0件。receipt 個別一致だけでは publish できず、欠落・余分・重複・差替えを拒否する。局所 RED も追補:82 にある。前回再審査:8–14 の反例はこの条件では成立しない。
2. **P1 exact 否定の完全性: 閉鎖。** 追補:28–74 は v1 を単一 Unit 内の正規化済み連続 literal に限定し、trusted adapter が query、selector、required claim の同一 bytes/ClaimId/親を束縛する。lexical no-hit/exhaustion は Source-owned scan の起点に留め、有限 budget 下で現行 Live/Read/全 authoritative item、manifest/coverage/current raw と全可視 Unit の本文を検査する。`ProvenAbsent` は private な Source-owned port の一時 receipt に限定され、`Partial` 等、変更、上限到達、権限不明、lexical recall 不一致は `Unknown` と blocking gap になる。前回再審査:16–22 の query/selector 差替えと日本語部分文字列 miss は、偽の `Absent` に進めない。追補:83–84 に対応ケースがある。
3. **P2 selector の親照合: 閉鎖。** 追補:76–78 は resolver と claim assembler の双方で selector、hit、required claim、verified evidence の ClaimId・親 Version ResourceId・Source・predicate・正規化値を照合し、同文面の別親を `Unknown`/非開示 gap にする。追補:85 に差替えケースがある。
4. **P1/P4 interface: 閉鎖。** 追補:6–20 は P1 を `discover_with_content_scope(request, DiscoveryScope)`、P4 を `discover_scoped(request, ScopedDiscoveryExecution)` として分け、`content_scope` を同じ内部 evaluation loop に渡す。P4 plan:172–176 と同じ署名であり、Rust の同名 overload はない。

## 残る blocking finding

### [P1] `Partial` の肯定 body claim と述語定義が一致しない

追補:30 は `document.body.contains_exact` の対象を **`Completed + Supported` の Unit** に限定して定義する。一方、改訂案:43–45,154–156,184,188–190 は、`Completed + Partial` の検証済み Unit を検索可能にし、同一 manifest/current Version/Read/raw/locator を検証した body hit を qualified として扱い、`resolve_hit` からその限定述語の `AssertionOrigin::Extracted` claim を組み立てる経路を残す。追補:78 の resolver/assembler の追加照合にも item coverage の `Supported` 条件はない。元案:81–85 も `Partial` の検証済み Unit の肯定 hit を認める。

再現条件は、一つの現行親 Version の `Partial` item に位置と raw が検証済みの Unit `東京` があり、他の全 `Supported` Unit には `東京` がない場合である。`BodyOnly` はその `Partial` Unit を hit にし、exact resolver は `Extracted(document.body.contains_exact, "東京")` を作れる。しかし追補:30 の述語は `Supported` Unit の存在だけで真になるため、この claim は定義上真ではない。否定 proof が `Partial` を拒否する条件（追補:70）は、この肯定側の矛盾を解消しない。

**必要な修正:** v1 述語の肯定対象を、現在 Read と Source/raw/locator が検証できた `Completed + Supported/Partial` の Unit と明記し、否定は従来どおり全可視 item `Completed + Supported` の時だけ許す。あるいは `Partial` hit を candidate のみに留め、同述語の `Extracted` claim を禁止する。採用する一方を resolver/assembler の条件と局所ケースに固定し、`Partial` 内の検証済み一致で肯定 claim、同 item の未読範囲による否定禁止を同時に検証する。

本判定は設計の文面と署名の整合性に限る。production 実装、parser 資格、build、DB/CI、配備の合格は主張しない。次の exact action は上の肯定対象と claim 組立条件を狭く訂正し、その新しい SHA-256 に対して独立再判定すること。
