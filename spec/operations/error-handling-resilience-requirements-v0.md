# Error Handling & Resilience Requirements v0

- 状態: v0
- 対象: Knowledge / Document Platform 全体
- 目的: 実装言語・DB・検索エンジン・UIライブラリに依存しない、共通のエラー処理・バリデーション・回復性・観測性の契約を定義する
- 方針:
  - 世界標準を規範とする
  - 独自規約は世界標準で表現できない業務固有部分に限定する
  - Human UI / LLM / Agent は同一API契約を利用する
  - インフラ固有エラーをAPI境界へ直接漏らさない
  - Validationは可能な限りSchema / Codegenで生成し、業務Invariantだけを手実装する

---

## 1. Normative standards

本システムの横断仕様は以下を規範とする。

| 領域 | 規格 |
|---|---|
| API Description | **OpenAPI 3.2.1** |
| Structural Schema / Validation | **JSON Schema Draft 2020-12** |
| HTTP Error Representation | **RFC 9457 Problem Details for HTTP APIs** |
| HTTP Semantics | **RFC 9110** |
| Distributed Trace Propagation | **W3C Trace Context** |
| Telemetry | **OpenTelemetry** |

国内業界ガイドラインは、導入・運用時の適合確認に利用してよいが、本仕様のAPI/Error/Telemetryの規範源にはしない。

---

## 2. OpenAPI policy

### 2.1 OpenAPI 3.2.1をAPI ContractのSSOTとする

```text
spec/api/
├─ openapi.yaml
└─ schemas/
   ├─ common/
   ├─ document/
   ├─ search/
   └─ errors/
```

以下をOpenAPIから生成可能にする。

```text
OpenAPI 3.2.1
      │
      ├─ Rust transport types / bindings
      ├─ TypeScript API client
      ├─ TypeScript API types
      ├─ Structural validators
      ├─ API documentation
      └─ Contract tests
```

### 2.2 OpenAPI 3.2専用機能

以下の利用を許可する。

- streaming response
- `itemSchema`
- `application/jsonl`
- `application/json-seq`
- `text/event-stream`
- `in: querystring`

ただし、利用するCodegen / Validator / Linterが正しく処理できることをCIで検証する。

```text
OpenAPI Contract = 3.2.1
Tooling          = 検証済みsubset
```

Tooling都合だけで契約全体を3.1へ落とさない。

---

## 3. Validation architecture

Validationは3層に分離する。

```text
Request
  ↓
V1 Structural Validation
  ↓
V2 Application Validation
  ↓
V3 Domain Invariants
  ↓
Business Operation
```

### 3.1 V1: Structural Validation

**Codegen対象。原則として自作しない。**

対象:

- required
- type
- enum
- string length
- numeric range
- pattern
- array cardinality
- object shape
- nested schema
- nullability
- standard formats
- discriminated union等

正本:

```text
OpenAPI 3.2.1
+
JSON Schema 2020-12
```

Frontend / Backendで同じconstraintを二重手書きしない。

### 3.2 V2: Application Validation

複数field間・Use Case単位の条件。

例:

```text
effective_from <= effective_to

scheduled_publish_at != null
なら
approved_at != null
```

JSON Schemaで自然かつ保守可能に表現できる場合はSchemaへ寄せる。
過度に複雑になる場合はApplication Layerで実装する。

### 3.3 V3: Domain Invariants

Schema validationでは表現しない。

例:

- `principal × document_version` の一意性
- 同一Documentのcurrent versionは高々1つ
- PUBLISHED Versionは直接上書き不可
- concurrent publishでは1要求のみ成功
- Search Indexは正本ではない
- strong consistency / eventual consistency境界

これらは:

```text
Rust Domain
+
DB Constraint
+
Transaction
```

で保証する。

---

## 4. Error layering

```text
Infrastructure Error
(PostgreSQL / Tantivy / IO / HTTP / SSPI ...)
          ↓
Infrastructure Adapter
          ↓
Application / Domain Error
          ↓
Error Registry
          ↓
RFC 9457 Problem Details
          ↓
Common API
       /      \
 Human UI    LLM / Agent
```

実装ライブラリ固有エラーをAPIへ直接漏らさない。

---

## 5. Error taxonomy v0

初期分類:

```text
Validation
Authentication
Authorization
NotFound
Conflict
BusinessRule
Unsupported
Integrity
DependencyUnavailable
Timeout
TemporaryFailure
RateLimited
Internal
```

各エラーに最低限以下を定義する。

```text
stable code
category
HTTP status
retryable
user visibility
audit policy
telemetry policy
```

---

## 6. Error Registry

machine-readableなRegistryを持つ。

推奨配置:

```text
spec/errors/error-registry.yaml
```

概念例:

```yaml
DOCUMENT_VERSION_CONFLICT:
  category: conflict
  http_status: 409
  retryable: false
  audit: true
  telemetry: warning

VALIDATION_FAILED:
  category: validation
  http_status: 422
  retryable: false
  audit: false
  telemetry: info

SEARCH_DEPENDENCY_UNAVAILABLE:
  category: dependency_unavailable
  http_status: 503
  retryable: true
  audit: false
  telemetry: error
```

このRegistryから以下を生成可能にする。

- Rust error code enum
- TypeScript error code union
- RFC 9457 schema補助
- Error documentation
- Contract tests

---

## 7. RFC 9457 API Error Contract

APIエラーは原則:

```text
Content-Type: application/problem+json
```

基本field:

```text
type
title
status
detail
instance
```

Extension field候補:

```text
code
trace_id
retryable
errors
source_status
```

例:

```json
{
  "type": "urn:knowledge-platform:problem:validation",
  "title": "入力内容が不正です",
  "status": 422,
  "code": "VALIDATION_FAILED",
  "trace_id": "01K...",
  "retryable": false,
  "errors": [
    {
      "code": "REQUIRED",
      "pointer": "/title",
      "message": "必須項目です"
    }
  ]
}
```

機械判定に利用してよい:

- HTTP status
- `code`
- `retryable`
- `errors[].code`
- `errors[].pointer`

機械判定に利用しない:

- `title`
- `detail`
- `message`

---

## 8. Field-level validation errors

field位置は可能な限り **JSON Pointer** で示す。

例:

```text
/title
/filters/0/value
/document/effective_from
```

Backend / Frontendで独自field path記法を作らない。

---

## 9. HTTP semantics

RFC 9110の意味を優先する。

| Category | Typical status |
|---|---:|
| Validation | 400 / 422 |
| Authentication | 401 |
| Authorization | 403 |
| NotFound | 404 |
| Conflict | 409 |
| Unsupported | 415 / 422 |
| RateLimited | 429 |
| DependencyUnavailable | 503 |
| Timeout | 504相当またはoperation-specific |
| Internal | 500 |

具体的なstatus選択はAPI operationごとにOpenAPI Contractで確定する。
独自HTTP status codeは原則作らない。

---

## 10. Retry policy

### 10.1 原則retryしない

- Validation
- Authentication
- Authorization
- NotFound
- BusinessRule
- Unsupported
- Integrity
- optimistic concurrency conflict
  - 最新状態取得後の再操作は別operationとして扱う

### 10.2 Retry可能候補

- Timeout
- TemporaryFailure
- DependencyUnavailable
- 一時的DB接続障害
- 一時的Extraction failure
- 一時的Search Source failure
- RateLimited

### 10.3 Retry algorithm

自動retryには原則:

```text
exponential backoff
+
jitter
```

を使用する。

### 10.4 Idempotency

自動retry可能なwrite operationは、必要に応じてidempotency設計を持つ。
Idempotency Key導入要否はoperation単位で決める。

---

## 11. Timeout policy

Timeoutは階層化する。

```text
Client timeout
    >
API operation budget
    >
dependency timeout
```

内側のdependency timeoutが外側operation timeoutを超えない。

具体時間は実測前に一律固定しない。
UX SLO・Search workload・Extraction workload・依存先特性から導出する。

---

## 12. Cancellation propagation

```text
Human / LLM cancels request
        ↓
HTTP request cancellation
        ↓
Application task cancellation
        ↓
Search / Retrieval cancellation
        ↓
Dependency cancellation
```

不要になった処理は可能な限り停止する。

---

## 13. Partial failure

Search Platformは複数Knowledge Sourceを扱うため、Partial Failureを第一級状態として扱う。

例:

```text
Document Platform  OK
External Source    OK
Internal Source    ERROR
```

全検索を必ず500にしてはならない。

Response概念:

```json
{
  "results": [],
  "partial": true,
  "source_status": {
    "documents": "ok",
    "external": "ok",
    "internal": "unavailable"
  }
}
```

Human UIは「一部の情報源を検索できませんでした」と表示可能にする。
LLM / Agentも`partial = true`を判断材料として利用可能にする。

---

## 14. Streaming failure semantics

OpenAPI 3.2.1のstreaming表現を利用する場合、stream開始後の失敗を通常HTTP error responseだけで表現できない点を考慮する。

Streaming protocolには少なくとも:

```text
data item
progress item
warning item
terminal error item
completion item
```

等のevent contractを設計する。

---

## 15. Search-specific error semantics

### 15.1 P5 Search HTTP の適用範囲と Error Registry

`POST /v1/search`、`POST /v1/discover`、`GET /v1/resources/{resourceId}`、`GET /v1/sources` に限る。status、`code`、固定 `title`/`detail`、適用 operation の唯一の対応表は [`spec/errors/search-api-error-registry.yaml`](../errors/search-api-error-registry.yaml) とする。既存 Document HTTP の §7 の URN 型とその handler は、この Search 専用規範では変更しない。

全 P5 error は RFC 9457 `application/problem+json` とし、明示的な `type: "about:blank"`、実 HTTP status と一致する body `status`、その status の固定英語 reason phrase `title`、registry の固定 `detail`、宣言済み extension `code` と無作為な相関 ID `trace_id` を持つ。`about:blank` 自体は HTTP status を超える意味を持たない。機械判定に必要な Search 固有区別は `code` を使い、`title`/`detail` を分岐に使わない。`instance` は省略する。`errors[]` は 422 の公開 request field の pointer と固定 code のみで、Source、Claim、resource ID、query、SQL、locator、内部 trace、秘密情報を含めない。認可前 parse error を含む**全** Problem と全 200 に `Cache-Control: private, no-store` と `X-Content-Type-Options: nosniff` を付ける。

v0 の実 transport は `Authorization: Bearer <opaque token>` である。`SearchCredentialVerifierPort` が credential を検証し、server 内だけで既知の session descriptor に解決する。`VerifiedActorResolverPort` と P4 の `CheckedAuthorityAdapter` が同じ opaque session を `TrustedSearchScope` に結ぶ。request body/header の自己申告 principal、tenant、role、group、Source grant は使わない。401 `AUTHENTICATION_REQUIRED` には四 operation とも `WWW-Authenticate: Bearer realm="search"` を必ず付ける。auth scheme/challenge/verifier/resolver が未配線、空、または不正な場合は route の起動を拒否し、header のない 401 を送らない。認証済みの operation 全体の拒否は 403、trusted identity 基盤障害は 503 `IDENTITY_UNAVAILABLE`。scheme 変更には本節、OpenAPI、handler の明示差分と再試験を要する。

Resource GET の未知・他 tenant・不可視・旧版・T10 終了・取消・重複 locator と、存在開示確定前の target 固有障害は同一 `404 RESOURCE_NOT_FOUND` とする。cursor の不明・失効・別 actor/session・request/可視集合/generation/retention 不一致は同一 `409 CURSOR_STALE` とする。両者の応答は原因別の body、status、固定 detail、header、retry policy を変えず、相関 ID だけを発生ごとに新しくする。隠れた個別 Source の障害を 503 の分類に使わない。完全な actor-visible catalog を作れない registry/ledger/visibility 障害は四 operation とも generic 503 `DEPENDENCY_UNAVAILABLE` とし、途中の Source 集合を 200 として返さない。

内部 final gate、local read、projection、serialization の operation deadline、または gateway 条件を証明できない timeout は 503 `SERVICE_UNAVAILABLE`。実際に gateway/proxy として必要な upstream response を待ち、時間内に受け取れなかったと型で確定した場合だけ 504 `UPSTREAM_TIMEOUT` とする。自由文字列や provider URL から 503/504 を推測しない。認可済みの独立結果と全 final gate と bounded DTO を期限内に確定できた Search/Discover だけ不完全性を明示した 200 が可能で、Search は `partial=true`、Discover は `evaluationCompleteness=bounded|interrupted` と typed gap を使う。header 送出後の失敗は status を変えず接続と disclosure lease を閉じる。`Retry-After` と `retryable` は公開 retry policy が安全に確定した場合にだけ出す。

### 15.2 Search の部分結果と有限予算

`No result` は正常な 200 の空配列である。optional Source/Probe、retriever/reranker の失敗時も、安全な独立結果と最終 gate がある場合だけ Search は `partial=true`、Discover は `evaluationCompleteness=interrupted` と型付き gap を返せる。required evidence が未評価なら `sufficient` にしない。未知・不可視 Source/Claim を absent と推論しない。Discovery の未知・他 tenant・不可視・失効 required Claim は `unresolved` と blocking `REQUIRED_CLAIM_UNRESOLVED` に統一し、Claim の存在差を trace/validation/error class に出さない。

公開 gap reason は既存の `MISSING_FACT`、`INSUFFICIENT_EVIDENCE_CLASS`、`AUTHORITY`、`FRESHNESS`、`CORROBORATION`、`CONFLICT`、`AVAILABILITY`、`UNSUPPORTED_COVERAGE` に `PAGINATION_UNAVAILABLE`、`REQUIRED_CLAIM_UNRESOLVED`、`REQUIRED_SOURCE_UNAVAILABLE`、`BUDGET_EXHAUSTED` を加えた閉じた enum とする。後者四つは Application の `PublicGapReasonCode` であり、Core の `MissingFact` に読み替えない。`evaluationCompleteness` は `complete` / `bounded` / `interrupted`、`completenessReasonCodes[]` は `BUDGET_EXHAUSTED`、`SOURCE_INTERRUPTED`、`REQUIRED_EVIDENCE_UNEVALUATED`、`BODY_COVERAGE_INCOMPLETE` に限る。上限到達時に安全な独立 200 を返すなら Search は `partial=true` と `BUDGET_EXHAUSTED` gap、Discover は `bounded`、同名 reason、同 gap を必須とする。required evidence 未評価には blocking gap と `REQUIRED_EVIDENCE_UNEVALUATED` を付け、本文 Partial の verified positive にも blocking body coverage gap を残す。negative `Absent` は Source 正本の全対象 item の `Completed + Supported` と有限 exact literal scan の証明時だけである。

v0 の**安全側 hard limit**は query 2,048 UTF-8 bytes、purpose 512 UTF-8 bytes、`sourceIds` 16、`requiredClaimIds` 1〜16、`resourceTypes` 8、page size 既定 20/最大 100、Discovery action 16、optional initial Source 8、evaluated candidate 200、qualified resource 50、evidence 64、gap 64、public trace 64、rejected visible candidate 200、snippet 320 Unicode code points、POST JSON body 16 KiB、request headers 総量 16 KiB、serialized success body 1 MiB とする。未知 JSON field と公開 input の形・範囲違反は 422、JSON/encoding 破損は 400、POST body bytes 超過は 413、header bytes 超過は 431、承認外 POST media type は 415。response 上限で安全な typed outcome に再構成できないときは 503 `SERVICE_UNAVAILABLE` とし、黙った切捨て 200 を禁じる。SourcePage は部分成功を持たず、可視集合の authoritative continuation stamp がなければ**全可視 Source が一つの上限内 page に収まる場合だけ** 200 とする。超過時は cursor なしの切捨てをせず 503 `DEPENDENCY_UNAVAILABLE` とする。

これらの limit は公開契約の初期安全上限であり、測定済み SLO・production latency 値ではない。§11 の `Client timeout > API operation budget > dependency timeout` に従い、具体的な秒数と配備値は P1/P4/P7 workload と実 transport の資格試験で固定する。deadline 未設定や残余時間超過の dependency call は許さない。cursor は同じ trusted session に束縛した RAM-only UUID v4 handle とし、絶対 5 分、idle 1 分、actor/provider/retention のより短い期限に従う。`NO_RETENTION` や必要 Source の非継続性があれば Search は `nextCursor=null`、`partial=true`、`PAGINATION_UNAVAILABLE` gap とし、provider cursor を保存・公開しない。SourcePage の stamp が不安定なら cursor を発行しない。ResourceDetail と SourcePage に partial 成功はない。

---

## 16. Extraction-specific error semantics

最低限:

```text
UnsupportedFormat
CorruptDocument
EncryptedDocument
MalformedArchive
ArchiveLimitExceeded
TextExtractionFailed
TemporaryExtractorFailure
```

を区別可能にする。

未知形式は:

```text
保存・版管理は可能
全文検索対象外
```

を許容できる。

Extraction失敗によってDocument正本を失敗扱いにしない。

---

## 17. Concurrency errors

optimistic concurrency失敗はInternal Errorにしない。

例:

```text
DOCUMENT_VERSION_CONFLICT
HTTP 409
retryable = false
```

Human UIは最新状態の再取得を促す。
LLM / Agentは最新状態を読み直して再計画可能にする。

---

## 18. UI error handling

Human UIは最低限以下を区別する。

```text
Pending
Success
Recoverable Error
Conflict
Partial Result
Fatal Error
```

### 可逆・冪等操作

例: ReadState

```text
optimistic update
↓
request
↓
failure
↓
rollback + error indication
```

### Critical operation

例: Document publish / AccessPolicy

```text
action
↓
即座にpending表示
↓
server commit
↓
successなら確定
failureなら未完了を明示
```

成功前に「成功済み」と表示してはならない。

---

## 19. Motion and error feedback

```text
T_state
= user action → usable state

T_motion
= visual transition completion
```

必須:

```text
T_motion completionを待たず
error / success / pending状態を操作可能にする
```

Motionはpresentation aidであり、業務状態のsource of truthではない。

---

## 20. Observability integration

Application ErrorはOpenTelemetryと連携する。

最低限候補:

```text
trace_id
error.code
error.category
component
operation
resource_id
resource_version
retryable
```

Validation等の期待されるclient errorとsystem failureを区別する。

---

## 21. W3C Trace Context

サービス境界では:

```text
traceparent
tracestate
```

を標準的に伝播する。

独自trace headerを正本にしない。

単一プロセス構成でも将来分離可能なように、HTTP / worker / async boundaryでtrace contextを維持する。

---

## 22. Audit integration

Audit対象error例:

- authorization denied
- access policy change failure
- document publish failure
- privileged management operation
- integrity violation

すべてのvalidation errorをAudit Storeへ保存する必要はない。

Audit policyはError Registryで指定可能にする。

---

## 23. Sensitive data policy

Error / Telemetry / Auditへ以下を無条件に入れない。

- document body
- search query全文
- customer information
- personal information
- credentials
- tokens
- private keys
- authentication headers
- raw uploaded files

代わりに可能な限り:

```text
resource_id
version_id
query_id
hash
error_code
trace_id
```

等で追跡する。

---

## 24. Error message localization

Error Registryのstable codeは言語非依存。

Human-readable messageはUI側でlocalize可能にする。

```text
code = DOCUMENT_VERSION_CONFLICT
```

は不変。

表示文は変更可能。

LLM / Agentも文言ではなくcodeを優先して判断する。

---

## 25. Code generation policy

原則:

```text
Spec first
   ↓
Codegen
   ↓
Thin custom logic
```

生成対象候補:

- Rust transport DTO
- TypeScript request / response types
- TypeScript API client
- structural validators
- Error Code types
- JSON Pointer field mapping
- contract tests
- documentation

生成しないもの:

- Domain invariant
- transaction logic
- business rule
- search ranking
- retry business policy
- UI behavior semantics

Generated codeとmanual codeをディレクトリ境界で分離する。
生成物を直接編集しない。

---

## 26. CI requirements

CIで最低限検証する。

```text
OpenAPI parse / lint
OpenAPI 3.2.1 version
JSON Schema validation
Schema reference resolution
Codegen reproducibility
generated diff clean
Error Registry uniqueness
Error code ↔ OpenAPI mapping
contract tests
```

3.2専用機能を追加した場合、Codegen / Validatorがその機能を扱えることをtestする。

---

## 27. Compatibility policy

API変更はOpenAPI diffで検査可能にする。

破壊的変更候補:

- required field追加
- enum縮小
- type変更
- response削除
- error code semantics変更

API Versioning方式は後続仕様で決定する。

---

## 28. Resilience non-goals v0

現時点で固定しない。

- Circuit Breaker library
- Service Mesh
- distributed rate limiter
- retry回数の一律固定
- timeout秒数の一律固定
- specific OpenTelemetry backend
- specific API code generator
- specific JSON Schema validator
- specific error handling Rust crate

要件からライブラリを選定する。

---

## 29. Acceptance criteria

- [ ] OpenAPI 3.2.1をAPI契約SSOTとして扱う
- [ ] JSON Schema 2020-12でStructural Validationを定義する
- [ ] Structural ValidationをFrontend / Backendで手書き重複しない
- [ ] Domain InvariantをSchemaだけに依存しない
- [ ] RFC 9457 Problem Detailsを共通error envelopeにする
- [ ] stable error codeを持つ
- [ ] Error Registryがmachine-readable
- [ ] infrastructure errorをAPIへ直接漏らさない
- [ ] retryable / non-retryableを区別する
- [ ] cancellationを可能な限り伝播する
- [ ] partial failureを表現できる
- [ ] SearchのNo Resultをerror扱いしない
- [ ] W3C Trace Contextを伝播する
- [ ] OpenTelemetryとError Registryをcorrelateできる
- [ ] AuditとTelemetryを分離する
- [ ] sensitive dataをerror/logへ無条件出力しない
- [ ] UI animationがerror state反映をblockしない
- [ ] LLM / Human双方が同じerror contractを利用できる

---

## 30. References

- OpenAPI Specification 3.2.1
  - https://spec.openapis.org/oas/v3.2.1.html
- JSON Schema Draft 2020-12
  - https://json-schema.org/draft/2020-12
- RFC 9457: Problem Details for HTTP APIs
  - https://www.rfc-editor.org/rfc/rfc9457.html
- RFC 9110: HTTP Semantics
  - https://www.rfc-editor.org/rfc/rfc9110.html
- W3C Trace Context
  - https://www.w3.org/TR/trace-context/
- OpenTelemetry
  - https://opentelemetry.io/docs/specs/

---

## 31. Document Diff v0 failure boundary

Document Diffの `ContentVerdict` と `DiffCoverage` は独立した軸である。原本位置を伴う確定変更があり、他に未比較範囲が残る場合は `Different + Partial` を許す。未比較範囲がある結果を `Same` や人間の確認完了に変換してはならない。

未対応の意味構造、局所的な原本破損、曖昧な対応、比較資源上限、再生成できなかったDSI証拠は、影響範囲を明示した部分結果にする。安全な局所位置を示せない場合はContentItem全体を未比較とする。これらをDSIの成功済み完全検査へ書き戻してはならない。

比較中のWORKING更新、現在認可失敗、FileObject raw hash・size不一致、cache/source binding不整合、必須Diff開示監査の失敗は結果を返さないhard errorである。cache hitでも同じhard error境界を適用し、失敗した旧結果を無言で返さない。API/Telemetryに原文・抜粋・Storage locator・credentialsを含めない。
