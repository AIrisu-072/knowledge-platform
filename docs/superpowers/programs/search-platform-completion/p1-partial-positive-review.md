# P1 exact-text predicate — Partial positive 独立レビュー

- 判定: **GO（前回の Partial-positive blocking finding の閉鎖）**。`p1-partial-positive-correction.md` を `p1-body-absence-amendment.md` §3 と P1 の resolver/assembler に適用した合成契約では、以前の「Partial の検証済み Unit から肯定 claim が作れるのに述語は Supported のみに限定」という矛盾は残らない。これは P1 本文設計を実装計画へ渡すための当該限定ゲートの判定である。
- 判定した exact input（SHA-256）: `p1-extraction-design.md` `ff5a2649f668b623eaee7de7ecdfb16de3f48f629470554dbb7dd92b99722977`、`p1-extraction-design-revision-1.md` `b1e84e476f614eb30e3bd1c5982e7e0ff2a35e33aef9cddd069618315dcbe742`、`p1-knowledgeunit-freeze.md` `0fe9ceb84472633d06c52d84f2f62c86bc783f880751c935a7d773b1c8fdfc84`、`p1-body-absence-amendment.md` `49838833ff293cf20032e46dbe9429847a975fad8fcb40f21fd977eaa8b69ae1`、`p1-body-absence-review.md` `9599765ab2ad923e15b4e3c87855d1cf92ddd64e883698edd63a320f535cf9bd`、`p1-partial-positive-correction.md` `b15c20beed37d0f8a569799fa8f41a555e9843f5dc13a47b6c320942130f5db8`。

## 確認結果

1. **肯定側:** 補正:3–5 が追補:30 の Supported 限定を訂正し、同一の現行 Live 親 Version の `Completed + Supported` **または** `Completed + Partial` の検証済み Unit 内に正規化済み非空 literal が連続出現するときだけ、`document.body.contains_exact` を真とする。これは原案:81–85 と改訂案:43–45,154–156,184,188–190 の Partial Unit 検索・限定 `Extracted` claim 経路と一致する。Partial の肯定後も blocking coverage gap を保持し、全体の completeness は false のまま（補正:5）。
2. **evidence と親の binding:** 補正:4,7 は raw/locator/text/profile/親/ClaimId と現行 Source-owned Read/Version/Part の一致を要求する。追補:32,78 および改訂案:154–156 の trusted query/selector/required claim、span、再読取、resolver/assembler の照合を外していない。別親・別 locator・別 raw・失効 Read から肯定 claim を作れない。
3. **否定側:** 補正:6–7 は `ProvenAbsent` を全可視 authoritative item が `Completed + Supported` であり、pinned bundle に対する全 Unit の literal scan と最終 authority 照合が完了した場合だけに保つ。追補:68–74 の有限 budget、Source-owned receipt、現行 Read/Version/Part/raw 再照合も維持する。Partial Unit に `東京` が見つかれば限定肯定 claim と gap が併存し、同じ親で `大阪` が見つからなくても否定は `Unknown` になる。未検証範囲を検索済みと扱わない。

前回レビュー:15–21 の blocking finding は補正で閉じる。前回レビュー:8–11 の既閉鎖事項と `p1-knowledgeunit-freeze.md` の限定 GO はそのまま採用した。Archive、Vector、production code、parser 資格、build、DB/CI、配備は再審査・検証していない。次の exact action は、上記 SHA-256 の合成契約と Partial 肯定／否定 Unknown の局所ケースを full P1 implementation plan の入力に固定すること。
