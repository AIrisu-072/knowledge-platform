# P5 Common Search / Discovery API — 設計案

- 状態: **DRAFT / P1〜P4 の契約凍結前**。本書は API の実装・配備・公開を示さない。
- 対象: Human / LLM / Agent が同じ Search Application を使う HTTP 境界。Graph 専用 API と実行権限の発行は対象外。
- 基点: Phase D の受入記録 `docs/superpowers/execution/search-discovery-platform-v0-acceptance.md`。PR #33 の `4892ba5` は hosted gate PASS だが、公開 transport は deferred、local full gate は容量不足で未完了。
- 規範: `spec/` が正本。既存 `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md` §4, §41, §62–64 の意味を維持し、P5 freeze 時に `spec/api/openapi.yaml` と関連 `spec/` を先に整合させる。

## 1. 調査した実装境界

| 現状 | 根拠 | P5 で必要な差分 |
|---|---|---|
| `DiscoveryService::discover` は Need / evidence / gap / trace を返す。`DiscoveryRequest` に `access_context: String` があり、必須 Claim の空集合は拒否する | `crates/search-core/src/discovery.rs`、`crates/search-application/src/discovery_service.rs:1058` | HTTP DTO から actor を受け取らず、verified actor から内部 request を組み立てる。Search を空 Claim の偽 Discovery に変換しない |
| Retriever は候補と Graph path の current access を結果に入れる前に確認する | `crates/search-application/src/retrieval_execution.rs:181`、`crates/search-application/src/ports.rs:312` | Source ごとの current access を HTTP でも維持し、開示直前に再確認する |
| `SourceRegistryPort::list_sources` 自体は actor を受けない | `crates/search-application/src/ports.rs:39` | Source 存在も保護する `CurrentSourceVisibilityPort` が必要 |
| Retriever cursor は `PlanningOnly`。既存 port は全リストを返す | `crates/search-application/src/retrieval.rs:102`、`crates/search-application/src/ports.rs:181` | 本物の page continuation と安定した順序を追加するまで `nextCursor` を作らない |
| Document は title / 許可 metadata のみ。`BodyRequired` は検索せず `UnsupportedCoverage` gap | `crates/search-source-document/src/coverage.rs:14` | HTTP も本文検索・snippet の coverage を偽装しない。P1 完了後に coverage を再定義する |
| Document の過去版は ID を二つ明示し、`Read` と `ReadHistory` を再評価する別経路 | `crates/search-source-document/src/history.rs:40` | live Search / resource GET は history fallback をしない |
| Document adapter は actor と内部 binding が一致した場合に現行公開版・policy を再確認する | `crates/search-source-document/src/postgres.rs:536` | HTTP request の principal / role / group / `accessContext` からこの binding を作らない |
| 現ブランチの `spec/api/openapi.yaml` は `paths: {}`。HTTP stack は別の未統合 ref `origin/feat/document-http-openapi-transport-v0-d@3f870a9` にある | `spec/api/openapi.yaml`、当該 ref の `crates/document-api-http/src/{identity,router,error,limits,trace}.rs` | P5 を独立設計する。HTTP branch 全体の取り込みや merge を前提条件として隠さない |

HTTP ref では `IdentityAdapter::resolve` が request headers を trusted adapter に渡し、`VerifiedActorContext` を extension に置く。`protect_routes` は adapter 未設定なら起動拒否、`ApiProblem` は `application/problem+json`、JSON body 1 MiB・header 32 KiB・response 32 MiB、`Cache-Control: private, no-store` を実装している。これは**別 ref の確認結果**であり、P5 branch に実装済みという意味ではない。`VerifiedActorContext::from_trusted_adapter` は値の整合性を検査するだけで認証をしない (`crates/document-application/src/access_context.rs:35`)。

## 2. 選択と構成

**選択: `search-api-http` を新設し、Search Application の service にだけ接続する。** Human UI、LLM tool、Agent、CLI は同じ四つの HTTP operation と同じ認可・資格判定を利用する。`InvocationKind` は trusted actor の観測・監査属性であり、別 business logic や権限昇格スイッチではない。

```text
trusted IdentityAdapter ──> VerifiedActorContext
                              │
                         search-api-http (DTO / limits / Problem / trace)
                              │ server-owned request assembly
            SearchQueryService / DiscoveryService / ResourceReadService / SourceBrowseService
                              │
          Source ports + generation pin + current Source-owned access + evidence
```

`DiscoveryService` の既存 `access_context: String` は認証子ではない。transport の内部 assembly が request ごとに推測不能な binding を作り、同じ verified actor を保持する Source adapter に渡す。外部 JSON に binding を認めず、ログ・cursor にも含めない。各 adapter は actor 有効期限と Source-owned current policy を確認し、`Denied` / `Unknown` / error を開示不可として扱う。将来の内部型強化では文字列を非公開 constructor の opaque 型に置き換えるが、Document / Search の Domain に HTTP または特定 IdP 型を入れない。

`document-api-http` を P5 の依存 crate にする案は、未統合 branch を暗黙に取り込むため採らない。Document API crate に Search route を直接追加する案も、Search の独立境界を弱める。P5 実装時に HTTP ref が統合されていれば `IdentityAdapter`、Problem、trace、limits を小さな共通 transport module に抽出する。未統合なら `search-api-http` に同じ契約の薄い adapter を実装し、composition root で**同一 trusted resolver**を注入する。どちらの場合も接続テストと dependency diff を残し、単に trait 名を揃えただけで同一認証と見なさない。

`SearchQueryService` は独立の application facade とする。既存 `RetrievalExecutor`、Source routing、federation、current access を再利用・抽出するが、必須 Claim を捏造して `DiscoveryService` を呼ばない。`ResourceReadService` は新しい trusted `ResourceLocatorPort` で canonical `ResourceId` から Source を一意に解決し、pin された projection と Source の現在状態を照合する。`SourceBrowseService` は Source registry の列挙結果を current Source visibility で絞る。新 port の不在時は endpoint を公開しない。

## 3. 暫定 HTTP 契約

JSON は `camelCase`、ID は canonical UUID 表示、日時は RFC 3339。未知の request field は拒否し、response では宣言した safe DTO のみを serialize する。OpenAPI 3.2.1 / JSON Schema 2020-12 を契約正本とし、3.2 固有機能を追加する場合は既存の tooling compatibility gate を通す。HTTP ref の route と衝突しない。以下は P1〜P4 freeze 後に確定する**暫定 operation**である。

| Operation | 入力 | 成功 | 意味 |
|---|---|---|---|
| `POST /v1/search` | `SearchQuery` | 200 `SearchPage` | 許可された Source の現行 Resource を検索。検索結果は資格判定や evidence sufficiency の代用ではない |
| `POST /v1/discover` | `DiscoveryInput` | 200 `DiscoveryEvaluation` | 同じ Core で Need、applicability、evidence、gap、contrast を評価。未解決も正常結果 |
| `GET /v1/resources/{resourceId}` | canonical platform ID | 200 `ResourceDetail` | 現行の許可された durable Resource の安全な表示。旧版・provider native locator・ephemeral candidate は解決しない |
| `GET /v1/sources` | `pageSize`, `cursor` | 200 `SourcePage` | その actor に存在開示が許された Source の capability summary のみ |

### 3.1 Request DTO と server-owned 項目

`SearchQuery` の公開 field は `query` (空白のみ不可、UTF-8 2,048 bytes 以下)、`resourceTypes` (最大 8)、`sourceIds` (最大 16、optional で絞り込みのみ)、`coverage` (`titleAndPermittedMetadata` / `bodyRequired`)、`pageSize` (既定 20、最大 100)、`cursor` (opaque) に限定する。初期公開は lexical / directory / structured の**接続済み mode のみ**を申告し、vector、body、remote、Graph の未接続 mode を自動選択したと装わない。`sourceIds` は認可要求ではなく候補の絞り込みであり、未知・不可視 ID を応答で区別しない。

`DiscoveryInput` は `need: {purpose, requiredResourceTypes, requiredClaimIds, temporalTarget?, businessTimezone?}`、`query?`、`coverage` を受ける。`purpose` は非空・512 bytes 以下、Claim ID は 1〜16 個。公開 DTO の値は `IntentFactOrigin::Explicit` としてのみ扱い、Derived / Inferred を caller に指定させない。Claim selector は trusted `ClaimSelectorPort` から generation に pin して解決し、任意 predicate / expected value を caller に注入させない。`needId`、`discoveryEvaluationId`、`evaluatedAt`、`EvidenceRequirement`、temporal / routing / retriever / Probe budget、Graph plan、provider permission は server が構築する。caller の `temporalTarget` は評価対象時刻であり、historical Resource の検索許可ではない。必要な Claim を解決できない場合、Source 存在を漏らさない validation または `Unresolved` gap とする。どちらにするかは P1/P4 の claim catalog 契約とともに freeze する。

四 operation の request、query、header、cursor に `principal`、`role`、`group`、`subjects`、`invocationKind`、`accessContext`、`serviceExecutor`、credential、任意 provider URL / locator を認めない。HTTP header も未検証の入力であり、trusted `IdentityAdapter` が認証・解決した `VerifiedActorContext` だけを使用する。actor 不在・失効時は処理しない。

### 3.2 Success DTO と情報の境界

`SearchPage` は `items[]`, `nextCursor?`, `partial`, `coverage[]`, `gaps[]`, `traceId` を持つ。各 item は `resourceId`、`sourceId`、`resourceType`、`resourceVersionId?`、`title?`、`rank`、許可済み `matchedFields[]`、`snippet?`、`provenance` の safe subset。スコアの絶対値や未校正の cross-source score は出さない。`snippet` は明示的に許可された Source field の plain text だけを最大 320 Unicode code points で返し、field と coverage (`title` / `metadata` / P1 が承認した `body`) を付ける。HTML を返さず、UI は表示時に escape する。Document body extraction が未実装の間、`bodyRequired` は空 result と blocking `UNSUPPORTED_COVERAGE` gap を返し、title hit を body hit に流用しない。

`DiscoveryEvaluation` は `needId`、`discoveryEvaluationId`、`qualifiedResources[]`、`evidenceSufficiency`、`evidence[]`、`gaps[]`、`rejectedCandidates[]`、`evaluationCompleteness`、`trace` を持つ。qualified item は canonical resource identity、usage profile ID、applicability、bounded matched condition code、解決 discriminator code、safe evidence ID のみ。Evidence は claim ID、state、role、許可された source/resource 参照、field 級の citation handle、任意の許可済み typed value を含む。Source adapter が許可しない値は省く。`evidence_ref`、`upstream_origin`、citation chain の内部 locator、Graph path、DSI opaque ref、任意 raw trace string をそのまま JSON に出さない。Primary は同じ generation/resource に束縛した正本 assertion だけとし、Graph path や要約を Primary に昇格しない。`Sufficient` は評価した Need と budget に対する state であり、全 Source を網羅したという主張ではない。

`gaps[]` は `reasonCode` (`MISSING_FACT`、`INSUFFICIENT_EVIDENCE_CLASS`、`AUTHORITY`、`FRESHNESS`、`CORROBORATION`、`CONFLICT`、`AVAILABILITY`、`UNSUPPORTED_COVERAGE` など)、`blocking`、許可された `requiredFactCode` / `acceptableEvidenceCodes[]` を返す。Source の機密名、具体的な missing fact 値、未許可 Resource ID は省く。`rejectedCandidates[]` は現在も見える candidate の reason code のみ。否認された候補の件数・ID・除外理由を通常 response に混ぜない。

`trace` は bounded な `{stage, outcomeCode, visibleSourceId?, count?}` の配列に変換し、Source routing / retrieval / qualification の判断を区別する。内部の `DiscoveryResult.source_trace`、`retrieval_trace`、`qualification_trace` は自由文字列であり直列化禁止。`traceId` は HTTP correlation ID で、評価 ID や認可証明ではない。通常 telemetry / audit は本文、query 全文、snippet、全 candidate ID、Graph path を記録しない。

`ResourceDetail` は Search projection そのものではなく、Source が現在許可した `resourceId`、`sourceId`、`resourceType`、`resourceVersionId?`、`title?`、`snippet?`、`coverage`、安全な `provenance`、`traceId` のみ。canonical ID から Source への mapping が一意でない場合は integrity failure とし、任意の Source を選ばない。GET の直前に current access / current publication / Source policy を再確認する。Document T10 後の live resource、旧版、取り消された権限は「存在しない」と同じ 404。Resource GET は full body download、Tool execution、binding、credential の API ではない。

`SourcePage` の item は許可された `sourceId`、`sourceType`、`resourceTypes[]`、`discoveryModes[]`、`enumerationSemantics`、`coverage`、bounded `availabilityCode` だけ。`accessModel`、authority scope の内部値、provider address、cost profile、credential、非公開 Source の存在数は返さない。空の registry と、見える Source が 0 件は同じ成功形にする。

成功形の最小例。ID は合成値で、field 名と safe disclosure の範囲を示す。P1〜P4 freeze で追加・縮小する場合は OpenAPI diff に明記する。

```text
POST /v1/search
{"query":"example","coverage":"titleAndPermittedMetadata","pageSize":20}
200
{"items":[{"resourceId":"00000000-0000-4000-8000-000000000011","sourceId":"00000000-0000-4000-8000-000000000012","resourceType":"knowledge","rank":1,"title":"Example","matchedFields":["title"],"snippet":{"text":"Example","field":"title","coverage":"title"},"provenance":{"sourceId":"00000000-0000-4000-8000-000000000012","resourceVersionId":"00000000-0000-4000-8000-000000000013"}}],"nextCursor":null,"partial":false,"coverage":[{"sourceId":"00000000-0000-4000-8000-000000000012","kind":"titleAndPermittedMetadata"}],"gaps":[],"traceId":"example-trace"}
```

```text
POST /v1/discover
{"need":{"purpose":"Find an applicable resource","requiredResourceTypes":["knowledge"],"requiredClaimIds":["00000000-0000-4000-8000-000000000021","00000000-0000-4000-8000-000000000024"]},"coverage":"titleAndPermittedMetadata"}
200
{"needId":"00000000-0000-4000-8000-000000000022","discoveryEvaluationId":"00000000-0000-4000-8000-000000000023","qualifiedResources":[{"resourceId":"00000000-0000-4000-8000-000000000011","sourceId":"00000000-0000-4000-8000-000000000012","applicability":"applicable","evidenceIds":["00000000-0000-4000-8000-000000000025"]}],"evidenceSufficiency":"unresolved","evidence":[{"id":"00000000-0000-4000-8000-000000000025","claimId":"00000000-0000-4000-8000-000000000021","state":"supported","role":"primary","citation":{"resourceId":"00000000-0000-4000-8000-000000000011","field":"title"}}],"gaps":[{"reasonCode":"MISSING_FACT","blocking":true,"requiredFactCode":"claim-unresolved"}],"rejectedCandidates":[],"evaluationCompleteness":"bounded","trace":[{"stage":"retrieval","outcomeCode":"completed","count":1}],"traceId":"example-trace"}
```

### 3.3 Paging と budget

Search の `nextCursor` は署名付き opaque token とし、normalized request digest、verified actor の principal/subjects fingerprint、可視 Source 集合、各 Source の pin 済み generation、安定した fusion order と last key、期限を束縛する。cursor の中身はクライアント契約にせず、raw query・snippet・credential を入れない。次 page でも actor と各候補の current authorization を再評価し、取消された item をスキップして page を満たす。generation が失われた、query/actor/可視集合が変わった、期限切れの場合は `CURSOR_STALE` 409。全 Source が安定した continuation または完全な bounded snapshot を提供しない場合は `nextCursor` を発行せず、`partial=true` と `PAGINATION_UNAVAILABLE` gap を返す。**現行 `PlanningOnly` を slice して cursor が動くと主張しない。** P4 remote adapter が continuation をどう提供するかは P4 freeze に従う。

Source 一覧は SourceId の安定 keyset、同じ actor fingerprint と registry snapshot/revision に束縛した cursor を使い、各 page で可視性を再評価する。既定 pageSize は 20、最大 100。registry snapshot/revision を提供できなければ cursor を発行しない。Discovery は `maxActions`、fan-out、candidate/claim/result count、Probe bytes/calls を server configuration で制限する**一回の bounded evaluation**であり、初期契約では page cursor を持たない。response 上限で評価結果を黙って切らず、`evaluationCompleteness=bounded` と明示 gap を返す。暫定 ceiling は 16 actions、8 optional initial Sources、200 evaluated candidates、50 qualified items、64 evidence items、64 gaps、64 public trace items、Probe は未接続なら 0 calls。limit に達した評価は `Sufficient` と全件網羅を混同せず、該当 completeness reason を返す。これらの値は P1〜P4 の benchmark と PoC 後に freeze する。HTTP 全体の body/header/response/timeout 上限は既存 HTTP 契約と整合させ、単純な size cap だけで高価な fan-out を制限したことにしない。

`PAGINATION_UNAVAILABLE` と budget 到達を表す code は現行 `search_core::GapReason` に存在しない。P5 では `EvaluationCompletenessReason` 等の型付き API / Application 契約を追加するか、規範レビューを経て `GapReason` を拡張する。既存の `MissingFact` に読み替えたり、HTTP handler が任意文字列を足したりしない。

### 3.4 History / remote / retention

通常 Search、Discovery、Resource GET は **live current** のみで、T10 終了済み Document や過去版へ fallback しない。Document history は既存の明示 `documentId` + `versionId` と `Read` + `ReadHistory` の別境界を利用し、P5 で履歴検索 endpoint を足さない。`temporalTarget` も history 権限を作らない。RemoteStableReference / EphemeralCandidate は P4 で durable platform `ResourceId` への Source-owned 解決と current access ができるまで `GET /v1/resources/{id}` の link に変換しない。

`NO_RETENTION` Source の body、snippet、evidence は、P4 がその call に対する live disclosure を許した場合だけ短命の応答 buffer に置く。server cache、cursor、trace、Audit、Projection へ保存しない。許可されなければ safe metadata のみ、または blocking gap。P1 の KnowledgeUnit / snippet coverage と P4 の provider grant が凍結されるまでは body snippet を有効にしない。

## 4. Error / security / observability 契約

失敗は [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457.html) の `application/problem+json` と既存の `type/title/status/detail/instance/code/traceId/retryable/errors[]` へ統一する。機械判定は `code` と HTTP status。`detail` や field error に内部 Source 名、SQL、locator、query 本文を入れない。OpenAPI は [公式 OAS 3.2.1](https://spec.openapis.org/oas/v3.2.1.html) と `spec/operations/error-handling-resilience-requirements-v0.md` に従う。

| 状況 | status / code | 開示条件 |
|---|---|---|
| 未認証・actor 失効 | 401 `AUTHENTICATION_REQUIRED` | adapter 診断を返さない |
| Search operation 自体の拒否 | 403 `FORBIDDEN` | target Source / Resource を特定しない |
| Resource 不在・不可視・旧版・T10 終了 | 404 `RESOURCE_NOT_FOUND` | 同じ形・cache policy・経路で応答 |
| JSON / field / budget の違反 | 400/422 `VALIDATION_FAILED`、過大 body は 413 | JSON Pointer は公開 field のみ |
| cursor の不一致・失効 | 409 `CURSOR_STALE` | token 内部状態を返さない |
| rate limit | 429 `RATE_LIMITED` | retry 指示は policy が許す場合のみ |
| Identity / 必須 Source の障害、timeout | 503 `IDENTITY_UNAVAILABLE` / `DEPENDENCY_UNAVAILABLE`、504 `TIMEOUT` | partial で安全に返せる場合は 200 + gap。必須証拠欠落を `Sufficient` にしない |

認可前の schema parse や error 経路も `no-store`、`nosniff`、bounded response を適用する。Source ごとの失敗・未対応は許可された Source に限って低 cardinality の gap として示す。Source の存在そのものが機密なら Source ID や failure count を出さない。request/response の `principal` 注入、他 actor の cursor、URL native locator、Graph path の未許可 participant、Document 権限取消と T10 の競合を negative test で固定する。

## 5. 実装対象と qualification

P5 freeze 後の差分を次の範囲に限定する。これは実装計画の候補であり、現時点の完了一覧ではない。

1. `spec/api/openapi.yaml` と `spec/api/schemas/search/` に四 operation、DTO、Problem code、paging/coverage、閉じた request schema を契約先行で記述。必要な Search 規範だけ `spec/` へ反映し、既存 Document API との OpenAPI diff を検査。
2. `crates/search-application/` に `SearchQueryService`、source visibility、resource read、実 cursor-aware retrieval port / stable fusion order、safe result projection を実装。Domain/Core に Axum・SQLx・Tantivy・HTTP DTO を入れない。
3. `crates/search-api-http/` に identity bridge、四 handler、Problem/trace/limits、OpenAPI DTO mapping、起動時の必須 adapter 確認を実装。runtime composition root で Search/Document の trusted resolver、Source adapter、generation store を接続。HTTP ref の統合状況に応じて共通 module を抽出するか互換層を置き、別 ref のコードを無断で取り込まない。
4. `crates/search-source-document/` と P1/P3/P4 adapter に current access・coverage・provenance・snippet disclosure・remote continuation の契約を実装。P6 outbox に依存する freshness は readiness / gap として示し、delivery 未接続を「最新」と表示しない。
5. 契約試験: OpenAPI parse/lint、schema/handler parity、Problem mapping、認証と actor 注入拒否、Source 存在秘匿、候補/Graph path の取消、T10/live/history 分離、`BodyRequired`、evidence attribution、NO_RETENTION、cursor 固定/失効、response budget。実 DB + HTTP の一つの end-to-end canary、focused Search 回帰、architecture/strict Clippy/fmt、最後に対象 head の hosted gate を確認する。

P5 完了判定は、四 operation が実 HTTP で同じ verified actor と Search Application に接続し、OpenAPI と一致し、上記 security/coverage/paging の反例に通ること。`200` だけ、HTTP route 定義だけ、mock Source だけでは達成しない。Production identity、credential、scheduler、deployment、merge は別状態として記録する。

## 6. P1〜P4 から受け取る凍結契約と未決事項

| 依存 | P5 が必要とする確定入力 | 未確定時の扱い |
|---|---|---|
| P1 Extraction | KnowledgeUnit ID・field-level coverage・snippet 生成と権限・evidence provenance | body snippet と `bodyRequired` の成功を公開しない |
| P2 Vector | vector mode の利用可能性、score calibration / fusion の資格 | vector field や未校正 score を API 成功契約に入れない |
| P3 Graph | graph candidate/path の認可と safe trace の境界 | Graph path を直接 expose せず、共通 candidate/evidence に正規化 |
| P4 Remote Source | stable/ephemeral identity、live disclosure、NO_RETENTION、Source visibility、continuation | durable GET link と page cursor を発行しない |
| P6 Outbox | index freshness の運用保証、停止・障害時の既知状態 | freshness unknown/availability gap を残し、最新性を保証しない |

次の正確な設計作業は P1〜P4 の freeze を読み、ここに記した暫定 field、coverage、cursor と actor bridge を差分レビューして P5 freeze に確定すること。その後に OpenAPI と implementation plan を作る。現時点で P5 の code、API test、hosted qualification はない。
