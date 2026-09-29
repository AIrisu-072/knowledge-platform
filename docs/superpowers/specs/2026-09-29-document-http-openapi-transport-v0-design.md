# Document HTTP/OpenAPI Transport v0 — 設計案

- 状態: **PROPOSED / WRITTEN SPEC REVIEW PENDING**
- 日付: 2026-09-29 JST
- Capability: `Document HTTP/OpenAPI Transport v0`
- 基準: `main@77b13a1d35d15eea0112ca2d73f8cbd3dfffe1c9`
- 区分: Product Capability / Transport Architecture / 設計のみ
- 実行状況: `docs/superpowers/execution/document-http-openapi-transport-v0-status.md`
- 実装開始条件: 本書の明示承認、別途作成するProduction Implementation Planの明示承認、着手時のmain・依存仕様・CI再確認。いずれか未充足なら製品コードを変更しない。

## 1. 目的

Document Platformに既に実装済みのAuthoritative Core、版管理、公開・予約・公開終了、文書管理基本操作、履歴・ファイル参照、既読、Document Diffを再実装せず、Human UI、LLM / Agent、将来のCLIが共通して利用できるHTTP/OpenAPI境界を追加する。

このCapabilityの役割は、HTTPを新しい業務ロジック層にすることではない。既存Application Layerの認可済み入口を、OpenAPI 3.2.1で記述された安定したtransport contractとして公開することである。

```text
Human UI / LLM / Agent / CLI
            |
            | OpenAPI 3.2.1
            v
Document HTTP Transport
            |
            +--> Identity Adapter
            |      |
            |      v
            |  VerifiedActorContext
            |
            v
Authorized Application Entry
            |
            +--> Document / Version / Publish / T10
            +--> Management / Folder / AccessPolicy / ReadState
            +--> Query / History / File Access
            +--> Document Diff
            |
            v
Ports
            |
      PostgreSQL / FileStorage / isolated workers
```

Human向けとAgent向けに別の業務APIを作らない。UI固有の表示変換はFrontend/View Modelで、Agent固有のtool schema変換はAgent側adapterで行う。

## 2. 規範と維持する境界

優先するSSOT:

- `spec/architecture/architecture-contract-v0.md`
- `spec/data/logical-data-model-v0.md`
- `spec/data/transaction-consistency-requirements-v0.md`
- `spec/operations/error-handling-resilience-requirements-v0.md`
- `spec/operations/observability-audit-requirements-v0.md`
- `spec/requirements/frontend-ux-requirements-v0.md`
- `spec/selection/library-tool-selection-v0.md`
- `spec/api/README.md`
- 承認済みDocument Authoritative Core、Publish、Semantic Inspection、Versioning、Publication End、Document Management Basics、Document Diffの各設計・改訂

固定する不変条件:

1. Document Platformの正本をHTTP DTOへ移さない。Document / Version / FileObject / metadata / folder / policy / read stateは既存Domain・Application・DB契約が正本である。
2. TransportからRepository、SQL、FileStorage、Diff workerへ直接接続しない。必要な公開操作がApplicationに不足する場合、狭いApplication契約を追加してからtransportを接続する。
3. Human UIとLLM / Agentは同じoperation contractを使う。
4. AuthorizationはApplication/Repositoryの現在認可を維持し、HTTP middlewareだけの一度限りの判定へ置換しない。
5. 必須業務AuditはApplication transactionが正本であり、HTTP access logやOpenTelemetry spanを代替にしない。
6. Search Index、検索用chunk、renditionをDocument APIの正本にしない。
7. Document Diffの `verdict` と `coverage` を独立して保持し、未比較範囲を「変更なし」に変換しない。
8. 本CapabilityでWindows/AD/SSPI固有型をDomain/Applicationへ持ち込まない。

## 3. 対象と対象外

### 3.1 対象

- Document系HTTP APIのURI・method・operationId・request / response契約
- OpenAPI 3.2.1 / JSON Schema 2020-12による構造schema
- RFC 9457 Problem Detailsとstable error code
- HTTP requestから検証済み `VerifiedActorContext` へ到達するIdentity Adapter境界
- cursor、revision、operation ID、idempotent replayのtransport表現
- multipart uploadとbinary download/streamingの境界
- Document Diffと新旧対照表projectionのHTTP表現
- W3C Trace Context / OpenTelemetryのtransport境界
- request body / upload / headerの資源上限、timeout、cancellation
- OpenAPI tooling / codegen compatibilityのqualification gate
- axum / tower-httpを利用するRust HTTP adapterの実装境界

### 3.2 対象外

- React GUIそのもの
- Agent Tool / MCP / CLIそのもの
- Windows AD / Kerberos / NTLM / SSPIの本番実接続
- 本番deploy、reverse proxy、TLS終端、DNS、証明書運用
- Search API
- 承認workflow
- 通常削除・物理削除・T10後再公開
- 任意bulk mutation
- Audit Store配送worker
- Document Diffの広い実文書corpus精度評価
- 外部公開APIやInternet exposure

## 4. Transport構成

新規transport adapterは `crates/document-api-http` を第一候補とする。

```text
crates/document-api-http
├─ router
├─ identity
├─ request DTO / schema binding
├─ response projection
├─ RFC 9457 error mapping
├─ multipart / stream adapter
└─ trace / HTTP middleware boundary
        |
        v
crates/document-application
```

`document-api-http` はDomain/Application型を呼び出せるが、`document-repository-postgres` や `document-storage-fs` の具象型へ直接依存しない。実行バイナリのcomposition rootがApplication serviceとInfrastructure adapterを配線する。

初期deploymentが1 Linux server / 1 Rust applicationであっても、この論理境界を崩さない。

## 5. Identity / Authentication境界

### 5.1 ClientはPrincipalを自己申告しない

HTTP JSON、query、multipart metadata、custom headerから以下を信頼してはならない。

- `principal_id`
- `identity_provider`
- group / role
- `InvocationKind`
- service executor
- delegation

request schemaには業務commandのactor fieldを置かない。既存Application commandがactorを要求する場合、transportが検証済み `VerifiedActorContext` のprincipalから構築する。

### 5.2 Identity Adapter

```text
HTTP connection / trusted auth material
            |
            v
Identity Adapter
            |
            +-- authentication
            +-- provider validation
            +-- group / role resolution
            +-- expiry
            +-- invocation kind
            |
            v
VerifiedActorContext
```

productionでは未検証identityを許すallow-all adapterを持たない。Identity Adapterが構成されていない、provider不明、期限切れ、membership検証不能の場合はfail closedとする。

Windows Integrated Authenticationの実接続は既存 `sspi` PoCの合格後に別工程で行う。本CapabilityのHTTP contractは特定認証製品に依存しない。

テストfixture用adapterはproduction assemblyから到達不能にする。

### 5.3 HTTP status

- 認証情報がない/無効: `401`
- 認証済みだが操作不可: `403`
- resource可視性を隠す必要がある既存Application契約は、その既存NotFound/Forbidden意味をtransportが変更しない。

## 6. API namespaceと表現規則

v0のDocument API namespaceは `/v1` とする。API semantic versionとOpenAPI文書の `info.version` は別に管理する。

外部JSONの規則:

- field名: `lowerCamelCase`
- resource ID: canonical UUID string
- caller-generated operation ID: UUIDv7
- revision: 0以上のinteger
- timestamp: RFC 3339 / UTC
- SHA-256等のdigest: lowercase hexadecimal 64文字
- cursor: opaque string。内部構造を公開しない
- enum: lowerCamelCaseのstable wire value。Rust variant名をそのまま外部契約にしない
- unknown request field: schema上rejectする方向を原則とする

Application内部のRust名やDB column名はwire contractではない。

## 7. Query API

一覧は既存 `DocumentQueryService` の3つの意味を混ぜない。

### 7.1 Document list

```http
GET /v1/documents?view=published
GET /v1/documents?view=authoring
GET /v1/documents?view=history
```

`view` により返却schemaを明示的に分ける。共通query候補:

- `titleContains`
- `folderId`
- `includeDescendants`
- `documentType`
- `owningDepartment`
- `category`
- `createdFrom`
- `createdBefore`
- `sort`
- `pageSize`
- `cursor`

publishedだけ `unreadOnly` を許可する。

`pageSize` は省略時50、1〜200。cursorはprincipal・view・filter・sortにbindingされる。syntax不正はValidation、正しい形式だが現在queryと整合しない/staleなcursorはConflictとして扱う。

### 7.2 Document detail

```http
GET /v1/documents/{documentId}?view=published
GET /v1/documents/{documentId}?view=authoring
```

publishedは現行 `PUBLISHED` のみ。authoringは既存編集用契約に従う。T10後に旧版へfallbackしない。

### 7.3 Version / history

```http
GET /v1/documents/{documentId}/versions?purpose=published|authoring|history
GET /v1/documents/{documentId}/versions/{versionId}?purpose=published|authoring|history
GET /v1/documents/{documentId}/history
GET /v1/documents/{documentId}/versions/{versionId}/files?purpose=published|authoring|history
```

`purpose` は既存 `VersionPurpose` の認可意味を保持し、同じVersion IDでも用途を省略して権限を広げない。

### 7.4 Folder root / children

```http
GET /v1/folders/root
GET /v1/folders/{folderId}/children
```

`/folders/root` は論理的なroot discovery endpointであり、transportが `document-repository-postgres::SYSTEM_ROOT_FOLDER_ID` をimportまたはhard-codeすることを禁止する。Application Layerが現在認可を評価したうえでrootの `folderId` と `revision` を返す狭いread contractを持つ。

これによりGUI/AgentはInfrastructure定数を知らずにFolder treeを開始でき、root直下へのFolder作成で必要な `expectedParentRevision` も取得できる。

読めないFolder名、祖先名、子件数を漏らさない。

## 8. Mutation API

状態遷移は一般CRUDへ無理に押し込まず、通常resource更新と明示actionを分ける。

### 8.1 Document create / upload

```http
POST /v1/documents
Content-Type: multipart/form-data
```

multipartは最低限:

- `request`: JSON metadata part
- `file`: authoritative primary file

v0の初版作成は既存Application契約どおりprimary fileを1つ扱う。複数ContentItemを初版作成へ拡張する場合は先にApplication/Domain契約を別途変更する。

media typeやoriginal filenameは表示・transport metadataであり、semantic formatの信頼根拠にしない。

### 8.2 Version lifecycle

```http
POST /v1/documents/{documentId}/versions
PUT  /v1/documents/{documentId}/versions/{versionId}
POST /v1/documents/{documentId}/versions/{versionId}:rebase
POST /v1/documents/{documentId}/versions/{versionId}:publish
POST /v1/documents/{documentId}/versions/{versionId}:withdraw
POST /v1/documents/{documentId}/versions/{versionId}:schedule-publication
POST /v1/documents/{documentId}/versions/{versionId}:cancel-publication-schedule
POST /v1/documents/{documentId}:end-publication
```

create/updateのcontent payloadは既存 `VersioningPreflight` が扱うmanifest意味を壊さないmultipart契約にする。binary partとmanifest itemの対応を明示IDで固定し、part順序へ意味を持たせない。

### 8.3 Management

```http
PATCH /v1/documents/{documentId}/metadata
POST  /v1/documents/{documentId}:move

POST  /v1/folders
PATCH /v1/folders/{folderId}
POST  /v1/folders/{folderId}:move

GET /v1/documents/{documentId}/access-policy
PUT /v1/documents/{documentId}/access-policy
GET /v1/folders/{folderId}/access-policy
PUT /v1/folders/{folderId}/access-policy
```

metadata PATCHはJSON Merge Patchの一般意味を採用せず、既存commandと一致する `set` / `unset` を明示する。これによりnull値と削除を混同しない。

AccessPolicyは `inherit` または明示grant集合を表すdiscriminated unionにする。明示空grantは既存Domain契約どおり拒否する。

AccessPolicyのGETは既存Document Management Basics設計どおり対象への `administer` を要求する。Transport実装前に、現在Application Layerで不足している認可済みpolicy-read contractを追加する。TransportからRepositoryの `read_grants` やpolicy tableを直接読んではならない。

policy read responseは、更新に必要なlocal bindingと利用者が理解するためのeffective policyを区別する。

- `target`
- `bindingMode = inherit | explicit`
- `policyId`（local bindingが存在する場合）
- `policyRevision`（local binding未作成なら0。PUTの `expectedPolicyRevision` に利用）
- `effectivePolicyId`
- `effectiveSource`（target自身または継承元Folder）
- `effectiveGrants`

継承中にeffective grantsだけを返してlocal bindingの有無を隠すと、PUT時のrevision競合制御ができないため、この二つを混同しない。

### 8.4 Read state

```http
PUT /v1/documents/{documentId}/versions/{versionId}/read-state
```

このoperationは `HumanInteractive` の明示確認のみ成功する。Agent/Serviceの先読み、一覧表示、ファイルprefetch、Diff実行で暗黙既読化しない。

## 9. Operation ID / Idempotency / OCC

### 9.1 Existing operation IDs are canonical

Version、Publish、Schedule、Publication End、Managementで既に定義済みのcaller-generated UUIDv7 operation IDをそのまま利用する。

別の汎用 `Idempotency-Key` を同じoperationに重ね、二つのidempotency identityを作らない。

Clientはwrite開始前にoperation IDを生成し、commit結果不明またはretryable transport failure時は**同一operation ID・同一payload**でのみ再試行する。異なるpayloadで同じIDを使った場合はConflict。

### 9.2 Optimistic concurrency

既存commandの:

- `expectedRevision`
- `expectedDocumentRevision`
- `expectedFolderRevision`
- `expectedParentRevision`
- `expectedPolicyRevision`
- `expectedCurrentVersionId`

をAPI上でも明示する。

HTTP ETagを業務revisionの別正本として導入しない。必要なら将来、同じrevisionのpresentationとしてETagを追加できる。

### 9.3 Initial create unknown outcome

現行初版Createはcaller-generated operation IDを持たず、serviceがDocument/Version/File IDを生成してからcommitする。そのため `CommitOutcomeUnknown` で同じPOSTをblind retryしてはならない。

Transport公開前に、既存 `lookup_create_outcome` を認可済みApplication入口へ昇格させる狭い変更を行う。

commit outcome unknownのProblemには最低限:

- generated `documentId`
- generated `documentVersionId`
- generated `fileId`
- recovery endpoint
- `retryable: false`

を含め、Clientはrecovery GETで結果を確認する。TransportがRepositoryを直接照会してはならない。

## 10. File access / download

```http
GET /v1/documents/{documentId}/versions/{versionId}/files/{contentItemId}/{representationId}?purpose=...
```

固定条件:

1. `VersionFileAccessService` が現在認可と必須file-access Auditをcommitする。
2. commit成功後にだけFileStorageをopenする。
3. HTTP status/header/bodyを確定してbyte送信を始めるのは上記成功後。
4. Audit commit結果不明時は0 byteのままProblemを返す。
5. responseにStorageKeyや内部pathを含めない。

header:

- `Content-Type`: 保存済みmedia type
- `Content-Disposition`: sanitized `safe_display_name`
- `Content-Length`:既知なら設定
- `Cache-Control: private, no-store`

v0ではHTTP Rangeを提供しない。Range対応には部分取得時のAudit・整合性意味を別途設計する。

## 11. Document Diff API

```http
POST /v1/documents/{documentId}/comparisons
```

request:

- `baseVersionId`
- `targetVersionId`
- `profile`
- `projection = diff | comparisonTable`

transportはDocument Diff Application serviceの認可・鮮度・Audit・cache境界を迂回しない。

responseは最低限:

- `verdict`
- `coverage`
- `resultDigest`
- `changes` または `rows`
- `unverifiedRegions`
- `ancillaryChanges`（diff projection）
- 原本へ戻るsource locator
- `auditEventId`

を保持する。

`Different + Partial`、`Unknown + Partial`、`Unknown + None`を有効な第一級状態として表現する。UI/Agentが `coverage != full` を無視して確認完了扱いできるschemaにしない。

Diff resultに原文抜粋を新規追加しない。v0は現在の原本locator/evidenceをtransportする。

## 12. Error contract

API errorは原則 `application/problem+json`。

共通field:

- `type`
- `title`
- `status`
- `detail`
- `instance`
- `code`
- `traceId`
- `retryable`
- `errors[]`

機械判定は `status`、`code`、`retryable`、field error code / JSON Pointerだけを使う。

初期stable code:

| code | HTTP | retryable | 意味 |
|---|---:|---:|---|
| `VALIDATION_FAILED` | 422 | false | schema / application入力不正 |
| `AUTHENTICATION_REQUIRED` | 401 | false | identity未確立 |
| `FORBIDDEN` | 403 | false | 現在認可なし |
| `DOCUMENT_NOT_FOUND` | 404 | false | 文書なし/不可視 |
| `DOCUMENT_VERSION_NOT_FOUND` | 404 | false | 版なし/不可視 |
| `FOLDER_NOT_FOUND` | 404 | false | Folderなし/不可視 |
| `REVISION_CONFLICT` | 409 | false | OCC競合 |
| `OPERATION_CONFLICT` | 409 | false | 同一operation IDの内容不一致等 |
| `CURSOR_STALE` | 409 | false | cursor binding失効 |
| `STALE_VERSION` | 409 | false | Version前提が古い |
| `STALE_COMPARISON_INPUT` | 409 | false | Diff中に入力前提変更 |
| `BUSINESS_RULE_REJECTED` | 422 | false | 型付き理由へ分解できない残余の業務拒否 |
| `RESERVED_DOCUMENT` | 409 | false | 予約中等の文書変更制約 |
| `FOLDER_CYCLE` | 409 | false | Folder移動でcycle発生 |
| `ROOT_PROTECTED` | 409 | false | rootへの禁止操作 |
| `IDENTITY_UNAVAILABLE` | 503 | true | identity resolution一時失敗 |
| `PUBLISH_QUALITY_REJECTED` | 422 | false | 公開品質条件未達 |
| `UNSUPPORTED_MEDIA_TYPE` | 415 | false | transport media type非対応 |
| `DEPENDENCY_UNAVAILABLE` | 503 | true | 一時依存障害 |
| `TIMEOUT` | 504 | true | operation budget超過 |
| `COMMIT_OUTCOME_UNKNOWN` | 503 | operation-specific | commit結果不明 |
| `INTEGRITY_VIOLATION` | 500 | false | 正本整合性違反 |
| `INTERNAL` | 500 | false | 非公開内部失敗 |

operation IDを持つmutationの `COMMIT_OUTCOME_UNKNOWN` は「同一ID・同一payloadの再実行だけ許可」をProblem extensionで示す。initial createだけは§9.3のrecoveryを使い、自動再POSTしない。

ProblemにSQL、Storage locator、credential、原文、stack traceを含めない。

Document Management Basicsの設計・計画では `ManagementErrorCode` として `ReservedDocument`、`FolderCycle`、`RootProtected`、`IdentityUnavailable` 等をmachine-readableに区別する方針が既にある。一方、現在の一部実装経路はこれらを `ApplicationError::BusinessRule` へ集約している。HTTP adapterがerror文字列やSQL状態から原因を推測することは禁止する。Transport実装前に、必要な区別をApplicationの型付きerror surfaceへ復元し、その型からError Registryへ写像する。

実装時に `spec/errors/error-registry.yaml` を作成し、OpenAPI schemaとRust mappingの双方から参照する。

## 13. Request body / upload / streaming

### 13.1 Limits

HTTP layerは無制限bodyをApplicationへ渡さない。

- JSON body上限
- multipart全体上限
- part数上限
- filename/header上限
- file size上限
- request timeout

は既存Application/worker/resource profileより外側かつ矛盾しない有限値としてImplementation Planで固定する。外側HTTP上限が内側のsemantic安全上限を無効化してはならない。

### 13.2 Streaming upload

大きなfileをtransportが無条件に全量RAM bufferingしない。Applicationの `ContentReader` / preflight境界へbounded streamingで接続する。

client切断時は可能な範囲でApplication taskへcancellationを伝播する。ただし、commitが開始済みのtransactionを「clientが切れたから失敗した」と偽って扱わない。commit結果不明の既存契約を維持する。

### 13.3 Download

binary downloadは§10の監査確定後streamingを開始する。

## 14. Browser boundary / CORS / CSRF

v0はsame-origin UIを優先する。

- credential付きrequestに `Access-Control-Allow-Origin: *` を使わない。
- cross-originが必要になった場合は明示allowlist。
- Browser auth方式確定前にCSRF不要と決め打ちしない。
- Cookie/session方式を採用する場合はstate-changing requestへCSRF protectionを必須化する。
- Negotiate等のconnection/browser authを採用する場合もOrigin/CORSを認証の代替にしない。

## 15. Cache / freshness

Document/ACL/read-stateは権限変更と競合し得るため、v0の業務API responseは原則:

```http
Cache-Control: private, no-store
```

とする。HTTP shared cacheを権限付きDocument responseの正本にしない。

FrontendのTanStack Query等のserver-state cacheは許容するが、mutation後のinvalidate/revalidationとConflict表示はFrontend UX契約に従う。

## 16. Observability

HTTP transportはW3C `traceparent` を受理し、OpenTelemetry spanへ接続する。

記録してよいtransport属性候補:

- stable `operationId`
- route template
- HTTP method/status
- request/response size bucket
- duration
- authenticated invocation kind
- error stable code
- trace/span IDs

記録しない:

- document本文/抜粋
- upload byte
- Storage locator
- credential/token
- AccessPolicy grantの全文
- arbitrary query value
- full filenameを無制限にlog

HTTP access logは業務Auditの代替ではない。

## 17. OpenAPI / JSON Schema / codegen gate

`spec/api/openapi.yaml` をcontract SSOTとする。実装時はcontract-firstで更新する。

使用するOpenAPI 3.2 subsetを実際のDocument API fixtureで検証し、以下をCIへ入れる。

- Redocly parse / lint / bundle
- OpenAPI version 3.2.1維持
- JSON Schema 2020-12 validation
- RFC 9457 schema
- multipart request
- binary response
- `oneOf` / discriminated union
- nullable semantics
- cursor/query parameter
- operationId uniqueness
- request/response examples

`typify`、`openapi-typescript`、`json-schema-to-typescript` は現在POC REQUIREDである。production依存へ追加する前に、実際のv0 contract subsetでcompatibility PoCを通す。PoC不合格でもOpenAPI 3.2.1 contractを3.1へ落とさず、手書きの薄いtransport DTOまたは別のpermissive toolingを評価する。

## 18. Implementation boundary

Production Implementationでは少なくとも以下を分離する。

```text
OpenAPI / JSON Schema
        |
        v
Transport DTO / V1 validation
        |
        v
Request mapper
        |
        +--> VerifiedActorContext (trusted adapter only)
        |
        v
Application command/query
        |
        v
Response projection
```

禁止:

- handler内SQL
- handlerからFileStorage key直接open
- handler内AccessPolicy評価の再実装
- handlerでDomain transitionを再実装
- human用とagent用の別handler business logic
- human message文字列によるmachine error判定
- transport DTOのDeserializeから `VerifiedActorContext` を生成

## 19. Application Layerへの許容される狭い追加

Transport実装のためにApplicationへ追加してよいのは、既存Domain意味を変更しない公開入口の補完のみ。

現時点で必要と判断するもの:

1. initial createのcommit outcomeを安全に照会する**認可済みcreate outcome lookup**。
2. Document Management Basicsで既に定義済みの「policy設定の参照にはadministerが必要」という意味を公開する**認可済みAccessPolicy read contract**。local bindingのmode/revisionとeffective inherited policyを区別して返す。
3. Repository具象の `SYSTEM_ROOT_FOLDER_ID` をtransportへ漏らさないための**認可済みroot Folder discovery/read contract**。
4. 既存設計上はmachine-readableである管理エラー理由が現在 `ApplicationError::BusinessRule` に集約される箇所について、HTTPが文字列解析せず扱える**型付きApplication error surfaceの補完**。
5. Transport assemblyが既存Query/Management/History/Diff serviceを同じverified actorで利用するための薄いfacadeまたは明示的なservice composition。

これらは既存Document Management Basicsの意味をtransportから利用可能にする補完であり、新しいACLモデルや新しいFolder業務規則を導入しない。新しいworkflow・policy言語・権限意味は本Capabilityへ便乗させない。

## 20. Acceptance criteria

### API contract

1. OpenAPI 3.2.1がDocument APIの全公開operationを記述する。
2. Human UIとAgentが同一operationを利用可能。
3. requestからprincipal/group/role/invocation kindを自己申告できない。
4. operation ID / revision / cursorの既存意味が失われない。
5. RFC 9457 + stable codeでApplication errorを表現できる。
6. Diffのverdict/coverage/unverifiedを完全に表現できる。
7. file responseにStorage locatorを含めない。

### Authorization / audit

8. 全handlerがVerifiedActorContextなしではApplicationへ入れない。
9. Repository / Storageへのtransport直結がarchitecture lint/testで禁止される。
10. file downloadはAudit commit成功前にbodyを開始しない。
11. Agent/Service requestでReadStateが暗黙更新されない。
12. policy変更後の古いHTTP response/cacheから権限を復活させない。

### Reliability

13. caller-generated operation IDを同一payloadで再実行できる。
14. revision conflictを409としてmachine-readableに返す。
15. initial createのcommit unknownはblind retryせずrecovery lookupへ誘導する。
16. request size / timeout / cancellationに有限境界がある。
17. cursor invalidとcursor staleを区別する。

### Tooling / UI readiness

18. Redocly / JSON Schema validationがCIでPASS。
19. OpenAPI 3.2 codegen候補はPOC REQUIRED gateを通るまでproductionへ入らない。
20. TypeScript側がraw fetch/API URLをPresentationへ漏らさずGenerated/typed client境界を構成可能。
21. GUIを作らなくてもcontract testで全operationを縦断検証できる。
22. AccessPolicy管理Clientが `administer` 認可済みGETからlocal policy revisionとeffective grantsを取得でき、Repositoryへ直接接続しない。
23. Folder Clientが `/v1/folders/root` からtree探索を開始でき、Infrastructureのroot UUIDを知る必要がない。
24. Management errorのmachine判定がhuman-readable error文字列に依存しない。

## 21. 実装順序の方針

書面設計承認後に別のProduction Implementation PlanでTaskを固定する。現時点の分割候補:

1. API contract / schemas / error registry / OpenAPI 3.2 tooling PoC
2. `document-api-http` scaffold、identity/error/trace boundary
3. query / detail / history / folder read
4. management / read-state mutation
5. create / version / publish / schedule / T10 multipart + operation replay
6. audited file download
7. Document Diff
8. end-to-end contract / authorization / cancellation / resource-bound tests
9. exact-head hosted CIとUI/Agent client compatibility evidence

この順序は承認前の実装指示ではない。Taskの正確なファイル・RED→GREEN・gateはProduction Implementation Planで確定する。

## 22. 承認境界

本書の承認で承認されるのは、Document HTTP/OpenAPI Transport v0の責務、公開operationの意味、Identity/authorization/error/idempotency/file/Diff境界である。

承認に含まれないもの:

- Production Implementation Plan
- 製品コード変更
- `spec/api/openapi.yaml` の実operation追加
- production dependency追加
- GUI / CLI / Agent Tool実装
- Windows/AD接続
- deploy
- PR merge

設計承認後、実装計画を別途作成してレビューする。
