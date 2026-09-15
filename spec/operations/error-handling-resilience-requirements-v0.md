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

検索処理では以下を区別する。

```text
Query invalid
Source unavailable
Retriever failed
Reranker failed
Partial source failure
No result
Search timeout
Internal error
```

`No result` はエラーではない。

Rerankerが失敗しても、policy上許可する場合はretrieval / fusion結果をfallbackとして返却可能にする。
Fallback発生はresponse metadataへ明示する。

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
