# P5-01 Search 規範 / OpenAPI 独立レビュー

- 判定: **GO — P5-01 の規範と OpenAPI の静的契約**。四 route の status、Problem、security、header、公開 DTO と主要 limit に、凍結契約を妨げる不一致は見つからなかった。これは HTTP 実装、実 socket 送出、P1/P3/P4/P7 の Source 正本、production factory、実 DB/TCP、hosted gate の受入ではない。
- 対象: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未 commit 作業版。レビュー用文書以外は変更していない。
- 契約優先順: P5 freeze → revision 2 → reconciliation → 改訂 1、P1/P3/P4 freeze と P1 Archive ruling を継承。旧 reconciliation §4 の一律 504/TIMEOUT は revision 2 の内部 503 / 真の upstream 504 に置換される。

## 確認結果

1. `spec/errors/search-api-error-registry.yaml:1-104` は **14 code / 12 status** と適用 operation を固定し、`spec/operations/error-handling-resilience-requirements-v0.md:505-527` と `spec/api/openapi.yaml:13-165,213-515,1141-1440` の四 route response 集合に一致する。400/401/403/404/409/413/415/422/429/431/503/504 の各 Problem は `application/problem+json`、`about:blank`、status と固定 title/detail、宣言済み `code`、`trace_id`、`instance` 省略。404 は Resource GET、409 は Search/Source cursor のみに現れる。全 200/Problem は `private, no-store` と `nosniff`、401 は固定 `WWW-Authenticate: Bearer realm="search"` を宣言する。既存 Document HTTP §7 の URN 契約は差分に含まれない。[RFC 9457 §4.2.1](https://www.rfc-editor.org/rfc/rfc9457.html#section-4.2.1)、[RFC 9110 §15.5.2](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.2) と整合する。`about:blank` の RFC 上の意味は HTTP status までで、Search 固有 `code` は宣言済み extension として扱う。
2. `spec/data/logical-data-model-v0.md:840-861` は request self-principal を排除し、P4 `TrustedSearchScope` / `AuthorizedSourceScope` と Document/Remote 共通 `VisibleCatalogSnapshot` を一つの境界にする。`spec/api/openapi.yaml:166-171,592-673` は同じ四 route の Bearer scheme と閉じた `SearchQuery` / `DiscoveryInput` を宣言する。Source 個別 Denied/Unknown は他の可視 Source を残し、完全 catalog を作れない基盤障害は四 route の generic 503 とする。SourcePage は stamp 不在で全可視 Source が一 page に収まる場合だけ 200、超過は `DEPENDENCY_UNAVAILABLE` 503 であり、切捨て 200 を許さない。
3. `spec/api/openapi.yaml:747-1122` の SearchPage、DiscoveryEvaluation、ResourceDetail、SourcePage と入れ子 DTO は `additionalProperties: false` と公開 field allowlist に合う。native URL、Graph path、raw score、provider trace、private actor、retention/authority 設定の field はない。`spec/data/logical-data-model-v0.md:853-861` は P1 の親 Version/Part/raw/current Read、Verified Partial positive と blocking gap、有限 exact negative proof、異種 Archive leaf、P3 n-ary 全 participant、P4 二 lease/五 retention mode を規範として保持する。未評価 required Claim は `unresolved` と blocking `REQUIRED_CLAIM_UNRESOLVED`、残りだけで `sufficient` にしない。
4. `spec/operations/error-handling-resilience-requirements-v0.md:517-527` と registry/OAS は内部 deadline を `503 SERVICE_UNAVAILABLE`、必要 upstream response の実 gateway/proxy 待ちだけを `504 UPSTREAM_TIMEOUT` とする。[RFC 9110 §15.6.4–5](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.6.5) の 504 条件と一致する。query/purpose UTF-8 bytes、配列件数、page default/max、snippet、POST/header/success body の公開上限は OAS の schema または `x-` extension と §15 で一致する。具体的な operation 秒数は配備値・測定済み SLO として主張していない。

## 焦点を絞った検証と残る実装 gate

- `mise run api:check`: **PASS**。Redocly は既存の `info.license` 不在と localhost placeholder の警告 2 件を出した。元の OAS も同じ placeholder/情報を持ち、今回の四 route 契約の blocker ではない。
- `python3` の既存 PyYAML/jsonschema による read-only fixture: 四 route の response 集合、14 code/12 status、security/challenge/headers、10 個の主要 DTO field allowlist、全 component schema、14 件の正しい Problem と 55 件の誤 status/title/detail/errors を照合して **PASS**。別 fixture で正常な Search/Discover、self-principal/未知 selector/上限超過/非公開 DTO field など 7 件の負例が期待どおりだった。新しい試験ファイルや dependency は追加していない。
- **Schema 単独では保証しない条件:** `SearchQuery.query = "あ"×1000` は JSON Schema の `maxLength: 2048` を通るが UTF-8 では 3000 bytes であり、`x-utf8-byte-max: 2048` は handler が実行する必要がある。また `DiscoveryEvaluation` の `sufficient` と blocking `REQUIRED_CLAIM_UNRESOLVED` gap の同居も現在の schema では通るが、§15 と logical model は明示的に禁じる。いずれも規範矛盾ではなく、P5-04/P5-06 の実装・wire parity 試験で反例として拒否することが必須。`x-required-response-headers` も実応答の存在証明ではない。
- P4-02 source-neutral catalog/ledger 独立 GO、実 credential verifier/identity/challenge 配線、P1 full body、P3 全 participant、P4 `TransientDisclosureLease` の socket 完了、P7 durable current、四 route 実 HTTP/DB/TCP の試験は未確認。これらが揃うまで P5 production qualified としない。

## Exact input

| 入力 | SHA-256 |
| --- | --- |
| P5 freeze | `26b755774af138f845c755cd91a04a8172819274c65483cc38012151ed5942c9` |
| P5 revision 2 / reconciliation / plan | `81c8be90375bd2461fa8179ef7d5fdb0705c1b02431dee918d99ac26372901d9` / `a17920c81928c638ea11204f6dce5e62a0c72d52a286431c70d743dd62d8c12e` / `fdb44ec8ccf70fa6714bc9b402ae07be8332271b3facf583af2c294a8ff3e8d9` |
| P1 composed / Archive ruling / P3 / P4 freezes | `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d` / `eadaa11a8c02953f48e533c53d549e9b26f346dc862231d3d7622a9ee615599c` / `773be941a35a2984495614ea41db74f70d325f4c034fe576d4d106b4af0e6ca3` / `7b38cae0e4057556970a36c9df4752348c0ad1d7c4cac916eb794275d35bd5c5` |
| Search Error Registry / operations §15 / logical model / OAS | `7d605a27dc41da4f7397eecac3937bf54a38d1dd8f4754968ccbeb8aac748bba` / `8b0ec6a0c1e8faa34bf5d2ccb6953cec2efadaceab27667ab95631b1fd94d1cd` / `c2f9ea7cf657730b215ad7318e44556bd2ea5cde982b9528443110821dec0327` / `803db0d6051860dc92cb3fc1571aeddae3ba36be376dab5e8ef69edda217d144` |

次の exact action: P5-02 以降の実装者は P4-02 の独立 GO を先に確認し、上記 schema 単独では証明できない 2 つの反例と全 response header/Problem の actual handler parity を focused 試験に追加する。P5-09 は実 socket/DB/TCP と exact-head hosted gate を別 receipt で確認する。
