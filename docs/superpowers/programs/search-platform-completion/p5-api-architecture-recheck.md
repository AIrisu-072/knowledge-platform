# P5 Common Search / Discovery API — 改訂 1 独立アーキテクチャ再レビュー

- 判定: **改訂 1 の設計・セキュリティ境界は GO**。先行レビューの P1/P2 は、この改訂案の意味上は閉じている。これは実装、OpenAPI、実 HTTP、P5 全体の Design Freeze の GO ではない。
- 対象: `p5-api-design-revision-1.md`、先行 `p5-api-architecture-review.md`、承認済み Search / Discovery v0 design §§4, 5, 41, 62–64、現行の Search / Document 型・port、P4 改訂案、および別 ref の HTTP Error 実装。別 ref と未凍結の P1〜P4 は、P5 に接続済みの証拠とはしない。

## 先行 P1/P2 の再判定

| 先行指摘 | 改訂 1 で閉じた境界と確認結果 |
| --- | --- |
| P1 trusted tenant / Source 存在可視性 | 同一の trusted actor・tenant・session を四 operation の入口で作り、actor-visible Source snapshot を `sourceIds`、Claim/Resource 解決、routing、pin、remote call より前に置く。不可視 Source の gap、trace、count、`partial`、障害分類を遮断し、必須 port 未配線なら起動拒否する（改訂 `:9-29`）。現行 `DiscoveryRequest.access_context: String` と actor-less `SourceRegistryPort::list_sources()` を公開入口から直接使わない型境界が必要であることも正しく特定している（`crates/search-core/src/discovery.rs:142-146`、`crates/search-application/src/ports.rs:39-42`、`discovery_service.rs:167-173`）。`SourceId` の全 tenant 横断一意性を起動時に検証しつつ tenant-scoped locator を維持する（改訂 `:25,31`）。 |
| P1 cursor 秘匿 / retention | 外部 cursor は意味を持たない乱数 handle のみ。RAM state、session・actor・visibility・generation・request 束縛、絶対/idle/provider TTL、失効・取消時の消去、content/trace の非保存を規定し、`NO_RETENTION` を継続 state から除外する（改訂 `:45-58`）。完全性に必要な非継続 Source があれば `nextCursor=null` と typed `PAGINATION_UNAVAILABLE` の partial response とする。これは署名だけの可読 payload と、現行 `PlanningOnly` の slice を継続とみなす抜け道を閉じる。 |
| P2 Claim visibility | `requiredClaimIds` の公開構文検査と、tenant/source/actor/current field visibility を伴う非公開 binding、pin 後の selector 検証を分ける。未知・他 tenant・不可視・失効 ID は同じ unresolved outcome とし、残りだけで `Sufficient` にしない（改訂 `:35-37`）。現行 `ClaimSelectorPort::selector_for(generation, claim_id)` に actor/tenant がないため、この追加境界は必要（`crates/search-application/src/ports.rs:53-69`）。 |
| P2 最終開示 / SourcePage cursor | item、field、citation/evidence、Graph participant の current access を出力確定直前に再検査し、失効した item と派生 rank/count/trace を一体で破棄する。actor/session 失効時は buffer 全体を 401 とし、gate 前の streaming を禁じる。SourcePage は visibility stamp/digest 変化で `CURSOR_STALE` とし、小さい key の飛ばしを防ぐ（改訂 `:39-41,47`）。将来の取消との原子的保証までは主張していない。 |
| P2 deadline / partial / Problem / OAS | operation deadline を routing、Source、Probe、evidence、最終 gate、serialization に伝播し、安全な確定結果がある場合のみ 200 partial とする truth table を置いた。required evidence 未評価を `Sufficient` にせず、最終確定不能なら 503/504 Problem にする（改訂 `:62-71`）。RFC 9457 の `type` 主識別子、`instance` 省略、HTTP/body status 一致、OAS 3.2.1 埋込 dialect と standalone JSON Schema の区別、四 operation の security/Problem/no-store/schema parity を明記した（改訂 `:73-92`）。 |

## P5 Design Freeze までの正確な残件

1. **P1〜P4 の exact contract 照合は未完了。** P4 改訂案は `TrustedSearchScope`、`AuthorizedSourceScope`、`ScopedSourceRegistryPort` を P5/P7 と共通にする（`p4-remote-design-revision-1.md:19-70`）。P5 の `TrustedSearchContext`、`AuthorizedSourceSet`、`ActorVisibleSourceRegistryPort`（改訂 `:9,23,27`）は、その共通 scope を包むか集約する概念として一本化し、別の resolver、constructor、Source visibility 判定系を増やさない。P4 の `EvaluationLease` から送出完了までの `TransientDisclosureLease`（同 `:163-173`）と、P5 の private response buffer・非 streaming final gate（改訂 `:39,56,58,62`）を同じ `NO_RETENTION` lifetime として固定する。Claim catalog / KnowledgeUnit coverage / Graph participant / remote continuation / stable fusion と型付き gap・completeness wire enum も P1〜P4 の凍結結果に照合してから `spec/`、OpenAPI、implementation plan を確定する。この照合は architecture semantics の再判定と別の、**Full API Freeze の必須 gate** である。
2. **Problem `type` の具体 URI を wire freeze 前に修正する。** 改訂 `:73` の `urn:knowledge-platform:problem:<CODE>` は、現行 `spec/operations/error-handling-resilience-requirements-v0.md:304` と別 ref `document-api-http/src/error.rs::ApiProblem::new` の慣行を引き継ぐが、`knowledge-platform` は [IANA URN Namespaces](https://www.iana.org/assignments/urn-namespaces) に登録されていない。[RFC 8141 §1](https://www.rfc-editor.org/rfc/rfc8141.html#section-1) は NID 登録を有効な URN の条件とする。既存慣行を無検証で RFC 準拠の type URI として固定しない。登録済み namespace の安定した URI を用いるか、個別 type を未確定とする間は `about:blank` と registry-defined `code` extension を用いて `type`/`code` の役割と一対一記述を修正する。選んだ値を Error Registry、P5 design、`spec/`、OpenAPI、HTTP mapping で一致させる。公開 documentation host の選定はこの gate に要らない。

上記 2 件を閉じる前に P5 の full Design Freeze / 公開資格を宣言しない。設計・セキュリティ architecture gate の GO は、未実装 port、未作成の四 operation、未実施の HTTP/DB/CI 試験を完了扱いにしない。

## 確認範囲

関連文書・型・port・別 ref の Error 実装、および RFC 9457 / RFC 8141 / IANA / OAS 3.2.1 の公式資料を read-only で確認した。build、CI、実 HTTP 試験は実施していない。このレビュー以外のファイルは変更していない。
