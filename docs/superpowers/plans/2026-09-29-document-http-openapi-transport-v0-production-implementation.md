# Document HTTP/OpenAPI Transport v0 — Production Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development` または `superpowers:executing-plans` を使い、Taskを依存順に実装する。以下の `- [ ]` は将来の実行欄であり、現時点の完了記録ではない。

**状態:** **PROPOSED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**

**Goal:** 既存Document Platformの認可済みApplication capabilityを、Human UIとLLM / Agentが共通利用できるOpenAPI 3.2.1準拠HTTP APIとして公開し、GUI実装を安全に開始できる契約・router・縦断検証を完成させる。Windows/AD本番接続とdeployは別工程とする。

**Architecture:** `spec/api/openapi.yaml` とJSON Schemaをtransport contractのSSOTとし、`crates/document-api-http` がaxum/tower-http上でV1 structural validation、trusted Identity Adapter、RFC 9457 mapping、request/response projection、bounded multipart/streaming、trace境界だけを担当する。Domain/Applicationの業務ロジックを複製せず、Repository / SQL / FileStorage / workerの具象依存をHTTP crateへ入れない。認証製品未接続のproduction assemblyはfail closedとし、allow-all runtimeを作らない。

**Tech Stack:** Rust 1.98.1 / edition 2024、Tokio 1.x、axum 0.8.x、tower-http 0.7.x、serde / serde_json、`jsonschema 0.55.x`、OpenAPI 3.2.1、JSON Schema 2020-12、Redocly CLI 2.52.1、Node 24.21.0 / pnpm 12.4.1。OpenAPI/JSON Schema codegen候補 `typify 0.7.x`、`openapi-typescript`、`json-schema-to-typescript` はPOC REQUIREDのまま開始し、実contract subsetで資格を得たものだけpromoteする。

**Frozen Design:** `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design.md` blob `88f7046a5d14a77f4091df0c92691f6634dd57d7`。承認記録 `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design-approval.md`。設計提示head `55ad0e5b005cde93a225f4af564c33cd6bd242ac`、基準main `77b13a1d35d15eea0112ca2d73f8cbd3dfffe1c9`、Draft設計/計画PR #27。

## 1. Global Constraints

- 本計画は依頼者レビュー待ち。**計画の明示承認・実装開始指示前に製品コード、OpenAPI paths、production dependency、実装PRを変更・作成しない。**
- 実装開始時は会話履歴を進捗正本にせず、`AGENTS.md` → `active.md` → capability status → Frozen Design → Approval → Approved Plan → GitHub branch/PR/CIの順に再取得する。
- `spec/` がnormative SSOT。実装で設計本文の意味を変える必要が出た場合は、その場で設計を変更せずSTOPし、Design Amendment gateへ戻る。
- Human UI / Agent / CLIで別のbusiness APIを作らない。UI/Agent固有表現は生成client / View Model / tool adapterで行う。
- HTTP requestのJSON/query/multipart/custom headerからPrincipal、identity provider、Group、Role、InvocationKind、service executor、delegationを自己申告させない。
- `VerifiedActorContext` はtrusted Identity Adapterだけが生成する。test fixture identityはproduction assemblyから到達不能にする。未設定production identityはstartupまたは全requestでfail closed。
- `document-api-http` は `document-repository-postgres`、`document-storage-fs`、`sqlx`、Diff/DSI runner具象へproduction dependencyを持たない。Application port/facadeだけを呼ぶ。
- handler内SQL、ACL評価、Domain transition、operation replay、Audit書込を再実装しない。
- caller-generated operation IDと既存expected revisionをcanonical replay/OCC identityとして保持する。同一operationで独立した汎用Idempotency-Keyを追加しない。
- Version作成の `targetVersionId` とFolder作成の `folderId` はclientがoperation開始前に生成し、exact retryで固定する。
- initial createだけは現行Applicationがresource IDsを内部生成するため、commit outcome unknownをblind POST retryしない。認可済みoutcome recoveryを用いる。
- file downloadは認可・必須Audit commit成功後にのみStorageをopenし、byte送信を開始する。Storage locator / raw pathはAPIへ出さない。
- Document Diffの `verdict` と `coverage` は独立。未比較をSameへ変換しない。Diff correctnessにLLMをruntime依存させない。
- API errorはRFC 9457 + stable code。human-readable文字列、SQLSTATE、ライブラリエラー文字列をmachine branchingへ使わない。
- request/upload/response/timeout/cancellationは有限境界を持つ。既存workerの内側resource profileをHTTP上限で緩和しない。
- credential付きAPIはsame-origin既定、`Access-Control-Allow-Origin: *`は禁止。認証方式確定前にCSRF不要と決めない。
- Document/ACL/read-state/file/Diff responseは原則 `Cache-Control: private, no-store`。HTTP cacheを認可正本にしない。
- telemetry/access logに本文、抜粋、upload byte、credential/token、Storage locator、AccessPolicy全文を出さない。HTTP telemetryは必須業務Auditの代替ではない。
- PRはDraftで作成し、merge・deploy・本番migrationは別の明示指示を必要とする。Taskごとの局所RED/GREENを残し、hosted CIはDelivery Unitの最終headで一度確認する。

## 2. Review Focus / High-risk invariants

| 入力・競合 | 期待結果 | 所有Task |
|---|---|---|
| HTTP payloadにprincipal/group/role/invocationKindを含める | schema拒否または無視ではなくfield自体を契約に持たない。trusted adapterのみactor生成 | HAPI-03/12 |
| policy read→別主体がpolicy変更→古いexpectedPolicyRevisionでPUT | 409。最新policyを取得して別operationとしてやり直す | HAPI-02/05/12 |
| root Folder探索 | Infrastructure定数をHTTP crateがimport/hard-codeせずApplication readから取得 | HAPI-02/04 |
| Version/Folder作成retry | operationIdだけでなくtargetVersionId/folderIdも同じ。違えばoperation conflict | HAPI-05/07/12 |
| initial create commit outcome unknown | 同じPOSTを自動retryしない。generated IDs付き503 + recovery GET | HAPI-02/06/12 |
| policy剥奪後の一覧/履歴/file/Diff/cache | 古い結果を権限根拠にしない。現在認可で拒否 | HAPI-04/09/10/12 |
| file Audit insert/commit失敗 | Storage open 0、response body 0 byte | HAPI-09/12 |
| Diff `Different + Partial` | 200成功結果としてverdict/coverage/unverifiedを保持。確認完了へ圧縮しない | HAPI-10/12 |
| invalid cursor / stale cursor | validationと409 conflictをstable codeで区別 | HAPI-04/12 |
| client切断とDB commitが競合 | commit済みをrollback済みと偽らない。commit unknown契約を保持 | HAPI-06/07/08/11 |
| Range付きfile request | v0では部分取得しない。明示拒否し、監査意味を曖昧にしない | HAPI-09 |

## 3. File Map / Dependency Boundary

予定する主な変更先:

| 責務 | 新規/変更先 |
|---|---|
| OpenAPI root / schemas / examples | `spec/api/openapi.yaml`, `spec/api/schemas/{common,errors,document}/**` |
| machine-readable error registry | `spec/errors/error-registry.yaml` |
| codegen/tooling qualification | `experiments/document-api-codegen/**`, `package.json`, `pnpm-lock.yaml`, `mise.toml`（資格結果に応じた最小差分） |
| Application補完 | `crates/document-application/src/{access_policy_read.rs,document_query.rs,authorized_document.rs,error.rs,lib.rs}` とfocused tests |
| PostgreSQL補完 | `crates/document-repository-postgres/src/{access_policy.rs,document_query.rs,authorized_repository.rs,error.rs,lib.rs}` とfocused tests。新migrationは原則不要 |
| HTTP transport | `crates/document-api-http/{Cargo.toml,src/**,tests/**}`、root `Cargo.toml` |
| architecture boundary | `spec/architecture/dependency-rules.toml`, `tools/architecture-lint/**`（必要なnegative smokeを追加） |
| contract/e2e evidence | `crates/document-api-http/tests/**`, `docs/superpowers/execution/document-http-openapi-transport-v0-acceptance.md` |
| selection result | `spec/selection/library-tool-selection-v0.md`（POCの事実だけ反映） |

`document-api-http` のproduction dependency方向:

```text
document-api-http
    ├─> document-application
    ├─> document-domain
    ├─> document-diff-core (wire enum/schema projectionに必要な範囲のみ)
    ├─> axum / tower-http / tokio / serde / jsonschema
    └─X document-repository-postgres / document-storage-fs / sqlx
        document-diff-runner / document-semantic-inspection-runner
```

composition rootは本Capabilityで安全に作れる範囲に限定する。production identity adapterが未接続なら、利用可能なallow-all binaryを作らない。HTTP routerはdependency injectionで実DB縦断試験可能にし、将来のAD adapterを差し込める。

## 4. Planned Interfaces

Application補完候補:

```rust
pub struct AccessPolicyView {
    pub target: PolicyTarget,
    pub binding_mode: PolicyModeKind,
    pub policy_id: Option<PolicyId>,
    pub policy_revision: i64,
    pub effective_policy_id: PolicyId,
    pub effective_source: PolicyTarget,
    pub effective_grants: Vec<PolicyGrant>,
}

#[allow(async_fn_in_trait)]
pub trait AccessPolicyReadRepository: Send + Sync {
    async fn get_access_policy(
        &self,
        ctx: &VerifiedActorContext,
        target: PolicyTarget,
    ) -> Result<AccessPolicyView, RepositoryError>;
}

pub struct RootFolderView {
    pub folder_id: FolderId,
    pub name: String,
    pub revision: i64,
}

pub async fn get_root_folder(
    &self,
    ctx: &VerifiedActorContext,
) -> Result<RootFolderView, ApplicationError>;

pub struct CreateOutcomeRequest {
    pub document_id: DocumentId,
    pub document_version_id: DocumentVersionId,
    pub file_id: FileId,
}

pub async fn lookup_create_outcome_authorized(
    &self,
    ctx: &VerifiedActorContext,
    request: CreateOutcomeRequest,
) -> Result<Option<CreateDocumentResult>, ApplicationError>;
```

実装時に型名は既存moduleとの整合で調整してよいが、意味は変えない。policy readは対象への `administer`、root readはFolder visibility、create outcomeは現在認可を必須とする。

HTTP境界候補:

```rust
#[allow(async_fn_in_trait)]
pub trait IdentityAdapter: Clone + Send + Sync + 'static {
    async fn authenticate(
        &self,
        request: &IdentityRequestContext<'_>,
    ) -> Result<VerifiedActorContext, AuthenticationError>;
}

pub struct HttpApiState<A, I, V> {
    pub application: A,
    pub identity: I,
    pub schemas: V,
}

pub fn build_router<A, I, V>(state: HttpApiState<A, I, V>) -> axum::Router;
```

`IdentityRequestContext` はmethod、route/path、headers、peer metadata等の認証に必要なtransport情報だけを参照し、business request bodyからidentityを生成しない。

API namespaceはFrozen Designどおり `/v1`。主要operation:

```text
GET  /v1/documents
GET  /v1/documents/{documentId}
POST /v1/documents
GET  /v1/document-creation-outcomes/{documentId}

GET  /v1/folders/root
GET  /v1/folders/{folderId}/children
POST /v1/folders
PATCH /v1/folders/{folderId}
POST /v1/folders/{folderId}:move

GET/PUT /v1/documents/{documentId}/access-policy
GET/PUT /v1/folders/{folderId}/access-policy
PATCH   /v1/documents/{documentId}/metadata
POST    /v1/documents/{documentId}:move

GET  /v1/documents/{documentId}/versions
GET  /v1/documents/{documentId}/versions/{versionId}
POST /v1/documents/{documentId}/versions
PUT  /v1/documents/{documentId}/versions/{versionId}
POST /v1/documents/{documentId}/versions/{versionId}:rebase
POST /v1/documents/{documentId}/versions/{versionId}:publish
POST /v1/documents/{documentId}/versions/{versionId}:withdraw
POST /v1/documents/{documentId}/versions/{versionId}:schedule-publication
POST /v1/documents/{documentId}/versions/{versionId}:cancel-publication-schedule
POST /v1/documents/{documentId}:end-publication

GET /v1/documents/{documentId}/history
GET /v1/documents/{documentId}/versions/{versionId}/files
GET /v1/documents/{documentId}/versions/{versionId}/files/{contentItemId}/{representationId}
PUT /v1/documents/{documentId}/versions/{versionId}/read-state

POST /v1/documents/{documentId}/comparisons
```

実装中にURIを変更したくなった場合、resource/action意味が同じでもOpenAPIと設計の対応を確認する。意味が変わる場合はDesign Amendment。

### 4.1 Initial HTTP resource profile candidate

以下は**実装・資格試験の開始値**であり、未測定のSLOや将来容量保証ではない。HAPI-11で境界値・1-over・代表fixtureを実測し、内側worker/profileとUXの両方に対して妥当と確認できた場合だけv0として固定する。

| 項目 | candidate |
|---|---:|
| JSON request body | 1 MiB |
| multipart JSON metadata part | 1 MiB |
| authoritative file 1件 | 256 MiB |
| multipart request total | 1 GiB |
| multipart parts | 64 |
| filename / display-name input | 1,024 UTF-8 bytes |
| aggregate request headers | 32 KiB |
| non-binary JSON response | 32 MiB |
| ordinary read/mutation operation budget | 30 s |
| create/version multipart operation budget | 120 s |
| Document Diff API operation budget | 45 s |
| streaming download idle budget | 30 s |

- per-file 256 MiBは既存DSI executorとDiff sourceの有限境界に合わせた開始値。HTTPでこれを超えて受け入れても後段が安全に処理できないため、少なくともv0のqualified pathでは上限を一致させる。
- multipart total 1 GiBは複数ContentItem Versionのtransport全体だけの上限であり、各itemの256 MiB上限を緩和しない。
- Diff API 45秒は既存Diff worker wall 30秒より外側に置く開始値。DB/Audit/queue overheadの実測で不足する場合は、依存timeoutとの階層を保った計画差分を提示する。
- downloadは既知Content-Lengthを持つ原本のstreamingを前提とし、全bodyを30秒で終える意味ではない。idle/stallを有限化する候補である。
- exact値をtest通過のためだけに引き上げない。代表業務文書が正当な範囲で失敗する、またはmemory/disk/latencyが危険ならSTOP条件として値を再レビューする。

## 5. Delivery / Verification Units

| Unit | Tasks | Draft PR / gate |
|---|---|---|
| A: contract・Application gaps・HTTP foundation | HAPI-01〜03 | Unit A最終headで標準CI + API/arch/focused tests |
| B: read・management | HAPI-04〜05 | Unit B最終headで標準CI + 実DB authorization |
| C: create/version/lifecycle/file | HAPI-06〜09 | Unit C最終headで標準CI + multipart/commit/audit streaming |
| D: Diff・hardening・横断受入 | HAPI-10〜12 | 最終同一headで標準CI + Sandbox + DSI PoC + API acceptance |

A→B→C→Dの依存順。stacked Draft PRにしてよい。各Taskで焦点RED/GREENをcommitに残し、hosted CIを毎Task起動しない。mergeは別指示。

---

## HAPI-01: OpenAPI contract skeleton・Error Registry・3.2 tooling qualification

**Files:** Modify `spec/api/openapi.yaml`, create `spec/api/schemas/{common,errors,document}/**`, `spec/errors/error-registry.yaml`, `experiments/document-api-codegen/**`; modify `package.json`, `pnpm-lock.yaml`, `mise.toml`, selection doc only as qualification evidence requires.

**Purpose:** 製品handlerより先に、設計で承認された全operationとmachine contractをOpenAPI/JSON Schemaへ固定し、POC REQUIRED toolingが実contract subsetを扱えるか実証する。

- [ ] **RED:** OpenAPI contract testを追加し、現在の `paths: {}`、error registry不存在、Document schema不存在でFAILさせる。version 3.2.1、operationId一意、全Document operation、RFC 9457 extension、multipart、binary response、`oneOf`、nullable、cursor、examplesを検査する。
- [ ] `spec/errors/error-registry.yaml` にFrozen Designのstable codeを定義する。最低限Validation/Auth/Forbidden/NotFound/RevisionConflict/OperationConflict/CursorStale/StaleVersion/StaleComparisonInput/BusinessRule/ReservedDocument/FolderCycle/RootProtected/IdentityUnavailable/PublishQuality/UnsupportedMedia/DependencyUnavailable/Timeout/CommitOutcomeUnknown/Integrity/Internalを含む。
- [ ] request schemaにはprincipal/group/role/invocation kind/service executor/delegation fieldを作らない。write commandには必要なoperation ID/revision/target ID/reasonだけを定義する。
- [ ] `experiments/document-api-codegen` で実contract fixtureに対し `openapi-typescript`、`json-schema-to-typescript`、Rust `typify` を検証する。3.2、external refs、oneOf、multipart、RFC9457、nullable、formatのloss/誤生成を記録する。
- [ ] **Promotion rule:** candidateが期待shapeをlossなく生成し、license/security gateを通る場合だけselectionを `SELECTED` へ更新しproduction toolingへ追加する。不合格候補はproductionへ追加せず、OpenAPIを3.1へdowngradeしない。Rust側は手書きthin DTO + `jsonschema` runtime validationへfallback可能。
- [ ] `mise run api:check` をparse/lintだけでなくcontract fixture validationまで拡張する。CIは必ずmise entrypointを使う。
- [ ] **GREEN:** `mise run api:check` とcodegen qualification scriptがPASSし、生成物の差分を正本にしない（generated outputはfixture比較または一時出力）。
- [ ] **commit:** `spec: define document HTTP API contract and tooling qualification`。

**STOP:** Tooling都合でOpenAPI 3.2.1の意味を落とす必要がある、または選定済み規格と矛盾する場合は実装へ進まず計画/選定レビューへ戻る。

## HAPI-02: Application補完 — policy read / root discovery / create recovery / typed management errors

**Files:** Application `src/{access_policy_read.rs,document_query.rs,authorized_document.rs,error.rs,lib.rs}`; Postgres `src/{access_policy.rs,document_query.rs,authorized_repository.rs,error.rs,lib.rs}`; focused Application/Postgres tests。原則migrationなし。

- [ ] **RED 1 policy:** `administer` がある主体だけlocal binding mode/revisionとeffective inherited policyを読めること、継承中のlocal revision=0、明示policy、親変更、剥奪、期限切れを実DBで固定する。
- [ ] **RED 2 root:** root UUIDをRepository定数からtransportへ漏らさず、認可済みApplication readからID/name/revisionを取得できる契約を追加する。root不可視の主体へ名称/件数を漏らさない。
- [ ] **RED 3 create outcome:** generated Document/Version/File IDsがすべて一致し、現在authoring認可を持つ場合だけcreate successを回復できる。別Document IDや権限剥奪後は結果を開示しない。
- [ ] **RED 4 typed errors:** reserved document、folder cycle、root protected、identity unavailable等、DMB設計上machine-readableである理由が `ApplicationError::BusinessRule` 一種類に潰れない試験を追加する。文字列解析は禁止。
- [ ] **GREEN:** 最小のApplication ports/servicesとPostgres実装を追加。既存ACL/transaction意味を変えず、policy readはadminister、root discoveryはcurrent visibility、create outcomeはcurrent authを再確認する。
- [ ] 既存DMB/Versioning/T10 testsを回帰。新migrationが必要になった場合はSTOPし、理由を計画差分として提示する。
- [ ] **commit:** `feat: expose transport-safe document application reads`。

## HAPI-03: `document-api-http` scaffold・Identity / RFC9457 / schema validation / architecture boundary

**Files:** create `crates/document-api-http/{Cargo.toml,src/{lib.rs,state.rs,router.rs,identity.rs,error.rs,validation.rs,trace.rs},tests/**}`; modify root Cargo, dependency rules, architecture-lint negative tests。

**Dependencies:** selected `axum 0.8.x`, `tower-http 0.7.x`, `jsonschema 0.55.x`。必要featureのみ。新しいauth libraryは追加しない。

- [ ] **RED architecture:** HTTP crateから `sqlx`、Postgres/Storage/runner具象を依存させたfixtureをarchitecture lintが拒否するnegative testを追加する。
- [ ] **RED identity:** no adapter、invalid identity、expired identity、payloadにactor-like fieldを含むrequest、test identityのproduction constructionを拒否する。
- [ ] `IdentityAdapter` と `IdentityRequestContext` をtransport layerに実装。business bodyからidentityを生成しない。test-only fixture adapterは `#[cfg(test)]` またはtests crate内に閉じる。
- [ ] RFC 9457 `ApiProblem` をError Registry由来のstable codeへ写像する。Application/Repository/library error文字列をそのままdetailへ出さない。
- [ ] JSON Schema 2020-12 runtime validatorをcompile/cacheし、V1 structural validationをhandler前またはmapper境界で適用する。Schema compile失敗はstartup/config error。
- [ ] W3C traceparent / OpenTelemetry既定契約に沿うcorrelation boundaryを作る。credential/bodyをspan attributeに入れない。
- [ ] CORSはsame-origin既定。wildcard credential CORSを作らない。
- [ ] **GREEN:** router smoke、401/403/422/409/500 Problem shape、trace ID、architecture negative/positive testがPASS。
- [ ] **commit:** `feat: add secure document HTTP transport foundation`。
- [ ] Unit A最終headで `mise run verify:fast`、対象integration tests、標準CIを一度確認する。CI pending/failedをGREEN扱いしない。

## HAPI-04: 認可付きread API — documents / folders / versions / history / policy

**Files:** HTTP `src/{documents.rs,folders.rs,history.rs,policy.rs,dto/**}`, tests。OpenAPIはHAPI-01 contractから意味を変えず必要なexample補足だけ。

- [ ] **RED:** published/authoring/history view、filters、sort、pageSize 1/50/200/201、cursor invalid/stale/principal mismatch、T10、WORKING、WITHDRAWN、Folder不可視、policy readのadminister不足をHTTP縦断で固定する。
- [ ] `GET /v1/documents` はviewごとに既存 `DocumentQueryService` を使い、取得後のHTTP filterで認可を補わない。
- [ ] `GET /v1/documents/{id}` はpublished/authoringを区別し、T10後に旧版fallbackしない。
- [ ] versions/history/version detail/file-listは `purpose` を明示し、用途省略で権限を拡張しない。
- [ ] root / childrenはApplication readを使う。HTTP crateが `SYSTEM_ROOT_FOLDER_ID` を知ることをarchitecture testで禁止する。
- [ ] policy GETはlocal bindingとeffective policyを区別し、revision/grantsをDTOへ投影。hidden ancestor名を漏らさない。
- [ ] responseをOpenAPI schema validatorへ通すcontract testを追加する。
- [ ] **GREEN:** HTTP focused tests +既存query/history/policy tests PASS。
- [ ] **commit:** `feat: expose authorized document read APIs`。

## HAPI-05: Management / ReadState mutation API

**Files:** HTTP `src/{management.rs,read_state.rs,dto/**}`, tests。

- [ ] **RED:** metadata set/unset、document move、Folder create/rename/move、policy PUT、HumanInteractive read-state、Agent/Service read-state拒否、operation replay、revision conflict、reserved/folder-cycle/root-protected typed Problemを固定。
- [ ] create Folder requestはclient-generated `operationId` と `folderId` を必須にする。同一operation retryでどちらも固定。再生成禁止。
- [ ] metadata PATCHはJSON Merge Patchへ変換せず `set` / `unset` の既存意味を保持する。
- [ ] policy PUTはGETで得たlocal `policyRevision` を `expectedPolicyRevision` として要求する。inherit / explicitをdiscriminated unionで扱い、空explicitを拒否。
- [ ] ReadStateはexplicit HumanInteractive requestだけを既読化する。list/prefetch/file/Diffでは呼ばない。
- [ ] **GREEN:** HTTP focused +DMB transaction/concurrency tests PASS。
- [ ] **commit:** `feat: expose document management mutation APIs`。
- [ ] Unit B最終headで標準CIと実DB authorization testsを確認。

## HAPI-06: Initial Document multipart create + authorized outcome recovery

**Files:** HTTP `src/{multipart.rs,create.rs,limits.rs}`, ApplicationのHAPI-02 recovery入口、tests。

- [ ] **RED:** valid multipart、request part欠落、file欠落、duplicate part、unknown part、oversized metadata/header/filename/file、invalid media type、client disconnect before commit、commit outcome unknownを固定する。
- [ ] `POST /v1/documents` は `request` JSON part + primary `file` part。filename/media typeはsemantic trust根拠にしない。
- [ ] uploadを無制限RAM bufferせず、bounded stream/spoolを既存 `ContentReader` へ渡す。temp strategyを使う場合もHTTP crateがauthoritative Storageを直接管理しない。
- [ ] §4.1 candidate（per-file 256 MiB / multipart total 1 GiB等）を実装し、境界/1-over試験を作る。値を緩和してtestを通さない。
- [ ] `CommitOutcomeUnknown` は503 + `retryable=false` + generated IDs + recovery locationを返し、POST自動retryを禁止する。
- [ ] `GET /v1/document-creation-outcomes/{documentId}` はversion/file IDsを照合し、現在認可で結果を開示する。
- [ ] **GREEN:** failure時に重複Documentを作らず、blind retry pathが存在しないことをassert。
- [ ] **commit:** `feat: create documents over bounded multipart HTTP`。

## HAPI-07: Version create/update/rebase multipart API

**Files:** HTTP `src/versioning.rs`, multipart manifest DTO、tests。

- [ ] **RED:** Version create/update/rebase、operation replay、targetVersionId mismatch、expectedRevision conflict、manifest/part対応不明、duplicate/missing binary part、DSI/publish preflight不正を固定する。
- [ ] Version createはclient-generated `operationId` と `targetVersionId` を必須にし、retryで固定する。
- [ ] create/update multipartはcomplete VersioningPreflight inputを表現し、part順序へ意味を持たせず、manifest itemとbinary partを明示ID/nameでbindingする。
- [ ] path `versionId` とcommand targetが一致しないrequestをV1/V2 validationで拒否する。
- [ ] Rebaseはbinaryを受けず既存Application commandを呼ぶ。
- [ ] client disconnect / timeout時もcommit outcomeを誤判定しない。
- [ ] **GREEN:** Versioning existing transaction/vertical-slice + HTTP contract tests PASS。
- [ ] **commit:** `feat: expose version mutations over HTTP`。

## HAPI-08: Publish / withdraw / schedule / cancel / T10 action API

**Files:** HTTP `src/publication.rs`, DTO/tests。

- [ ] **RED:** manual publish、withdraw、UTC schedule、cancel、T10、exact replay、operation conflict、stale revision/current version、quality reject、policy剥奪をHTTPで固定する。
- [ ] 各requestは既存UUIDv7 operation IDとexpected revisionをそのままApplication commandへmappingする。actorはbodyに持たない。
- [ ] scheduleの時刻はUTC契約を維持し、timezoneを黙って変換しない。
- [ ] T10 reasonを必須化し、currentVersion ID/revisionを保持する。T10後の通常readは404/既存契約どおりで、HTTPが過去版fallbackしない。
- [ ] `CommitOutcomeUnknown` はoperation-specific retry instructionsをstable Problem extensionへ出す。異payload同IDは409。
- [ ] **GREEN:** publish/version/schedule/T10/scheduler authorization regressions PASS。
- [ ] **commit:** `feat: expose document publication lifecycle APIs`。

## HAPI-09: Audited file download streaming

**Files:** HTTP `src/file_download.rs`, headers/content-disposition helper、tests。

- [ ] **RED:** no read/read_history、T10/history purpose、他Document/item/representation、audit failure、commit unknown、Storage open failure、unsafe filename、Range headerを固定する。
- [ ] 必ず `VersionFileAccessService::open_version_file` を通し、authorize+Audit commit成功後にStorageがopenされる既存順序を維持する。
- [ ] HTTP response body stream開始前にgrant成功済みであることをtest doubleで証明する。audit失敗時 `storage_open_calls == 0`。
- [ ] `Content-Disposition: attachment` + sanitized safe display name、`X-Content-Type-Options: nosniff`、`Cache-Control: private, no-store`。Storage locator/pathはheader/bodyに含めない。
- [ ] v0ではRangeを受けない。Range requestは明示的な416等のcontract-defined responseとし、部分byteを返さない。
- [ ] stream中に長いDB lockを保持しない。client disconnect時に送信終了をAudit success意味へ書き換えない。
- [ ] **GREEN:** file access existing DB tests + HTTP byte-order tests PASS。
- [ ] **commit:** `feat: stream audited document files over HTTP`。
- [ ] Unit C最終headで標準CI、multipart/resource/audit focused testsを確認。

## HAPI-10: Document Diff HTTP projection

**Files:** HTTP `src/diff.rs,dto/diff.rs`, tests。

- [ ] **RED:** diff/table projection、Same+Full、Different+Full、Different+Partial、Unknown+Partial/None、stale comparison、policy剥奪、cache hit Audit、invalid same-version requestを固定する。
- [ ] `POST /v1/documents/{documentId}/comparisons` はpath Document IDとrequest version pair/profile/projectionを `DocumentDiffService` へmappingする。
- [ ] responseは `verdict`、`coverage`、result digest、changes/rows、unverified regions、ancillary、source evidence locator、auditEventIdをlossなく表現する。
- [ ] Storage locator、credential、原文抜粋を新規追加しない。
- [ ] Partial/UnknownをHTTP errorへ変換しない。Application hard errorだけRFC9457へmappingする。
- [ ] **GREEN:** Diff acceptance/regression + HTTP schema tests PASS。
- [ ] **commit:** `feat: expose document comparisons over HTTP`。

## HAPI-11: Resource limits / timeout / cancellation / browser / observability hardening

**Files:** HTTP `src/{limits.rs,timeout.rs,security_headers.rs,trace.rs}`, `mise.toml`/tests/acceptance evidence as needed.

- [ ] §4.1のJSON、multipart total、part count、per-file、header/filename、response JSON、operation/idle timeout候補についてexact boundaryと1-over試験を追加する。既存Diff/DSI source 256 MiB等の内側profileを超えて意味安全性を緩和しない。
- [ ] 代表small/large fixtureでLinux CIまたは同等環境のwall/memoryを測定し、候補上限が不合理なら勝手に緩和せずSTOPして計画差分を提示する。
- [ ] timeout階層 `client > API operation > dependency` を満たす。Diff worker wall 30秒よりAPI operation budgetが短くならない。
- [ ] client cancellationを可能な範囲で伝播するが、DB commit結果を推測しない。commit start後のunknown outcome testを追加。
- [ ] same-origin既定、wildcard credential CORSなし、security headers、no-storeを全Document routesへ適用する。Cookie/session CSRFはauth方式未決定のため「不要」と実装しない。
- [ ] traceparent propagation、route template/status/duration/error codeを記録し、raw URL/query/body/filename/token/policy全文を記録しないtestを追加。
- [ ] rate limitは設計で必須値が未定のため、勝手にgeneric limiterを入れない。必要性が実測で出た場合は別判断。
- [ ] **commit:** `test: harden document HTTP resource and observability boundaries`。

## HAPI-12: Cross-cutting contract/e2e acceptance and GUI/Agent readiness

**Files:** `crates/document-api-http/tests/{contract,authorization,e2e,commit_unknown,cancellation,security}.rs`、`docs/superpowers/execution/document-http-openapi-transport-v0-acceptance.md`、selection/tooling evidence。

- [ ] PostgreSQL 18.6 + FileSystemStorage + production DSI/Diff executorsを可能な範囲で実配線し、HTTP routerをtest identity adapterから縦断する。production allow-all binaryは作らない。
- [ ] 一連のscenario: root discovery → Folder create → Document create → authoring list → Version create/update → publish → published read → mark read → metadata/move/policy → history/file → Diff → policy revoke → same calls denied。
- [ ] exact replayでoperation result/revision/event identityが変わらないこと、異payload同IDが409、target resource ID変化が409を確認する。
- [ ] initial create commit unknownはrecovery GETだけで復旧し、Document重複0。
- [ ] file Audit failureで0 byte、Diff Partialを保持、cursor stale、T10、expired identity、identity unavailableを確認する。
- [ ] OpenAPI request/response exampleと実handler responseのschema一致を全operationで検証する。operationId coverageが100%であること。
- [ ] TypeScript codegen資格済み候補がある場合、temporary/generated clientで全operation型が生成可能であることを確認し、GUI側がraw URL/fetchを必要としない証拠を残す。候補不合格なら手作業で結果を捏造せず、API contract自体の完成とは分けてgap記録する。
- [ ] `mise run verify:fast`、`mise run verify`。差分またはrepository policyが要求するなら `mise run verify:full`。pinned PDFiumを必要とする既存testsを正しく設定する。
- [ ] 最終exact headで標準CI、DSI Sandbox Preflight、DSI PoCを確認する。全てSUCCESSになるまで完了扱いしない。新HTTP差分がSandbox/PoCを意味上変更しない場合も回帰gateとして確認する。
- [ ] architecture negative smokeでHTTP crate→DB/Storage direct dependency、Domain/Application→axum、CI bypassが拒否されることを確認。
- [ ] Acceptance文書に、実測値、未接続Identity、未deploy、codegen資格結果、GUI開始可能範囲、残るblockerを記録する。
- [ ] **commit:** `test: qualify Document HTTP/OpenAPI Transport v0`。
- [ ] Draft実装PRをレビュー可能状態にし、**merge・deployは行わず**依頼者の明示指示を待つ。

## 6. Acceptance Criteria ↔ Task mapping

Frozen Design §20の受入条件を次のTaskで満たす。

| Design AC | Primary | Cross-check |
|---|---|---|
| 1 OpenAPI全operation | HAPI-01 | HAPI-12 |
| 2 Human/Agent共通API | HAPI-01/03 | HAPI-12 |
| 3 identity自己申告禁止 | HAPI-01/03 | HAPI-12 |
| 4 operation/revision/cursor意味維持 | HAPI-04〜08 | HAPI-12 |
| 5 RFC9457 stable code | HAPI-01/02/03 | HAPI-12 |
| 6 Diff lossless | HAPI-10 | HAPI-12 |
| 7 Storage locator非公開 | HAPI-09/10 | HAPI-12 |
| 8 VerifiedActorContext必須 | HAPI-03 | HAPI-12 |
| 9 transport→Infra直結禁止 | HAPI-03 | HAPI-12 |
| 10 file Audit先行 | HAPI-09 | HAPI-12 |
| 11 Agent/Service既読禁止 | HAPI-05 | HAPI-12 |
| 12 revoke/cache非復活 | HAPI-04/05/09/10 | HAPI-12 |
| 13 exact operation replay | HAPI-05/07/08 | HAPI-12 |
| 14 revision 409 | HAPI-05/07/08 | HAPI-12 |
| 15 create recovery | HAPI-02/06 | HAPI-12 |
| 16 target/folder ID固定retry | HAPI-05/07 | HAPI-12 |
| 17 finite resource/cancel | HAPI-06/11 | HAPI-12 |
| 18 cursor invalid/stale区別 | HAPI-04 | HAPI-12 |
| 19 Redocly/Schema CI | HAPI-01 | HAPI-12 |
| 20 codegen POC gate | HAPI-01 | HAPI-12 |
| 21 typed client boundary | HAPI-01 | HAPI-12 |
| 22 GUIなし縦断 | HAPI-03〜10 | HAPI-12 |
| 23 policy read | HAPI-02/04 | HAPI-12 |
| 24 root discovery | HAPI-02/04 | HAPI-12 |
| 25 management typed errors | HAPI-02/05 | HAPI-12 |

## 7. STOP Conditions

以下では推測で続行せず、statusへblockerを記録して設計/計画レビューへ戻る。

1. Frozen Design blobの意味変更が必要。
2. OpenAPI 3.2.1をtooling都合で3.1以下へ落とす必要が出る。
3. Identityをpayload自己申告またはallow-all productionで代替しないと動かせない。
4. HTTP crateがRepository/Storage具象へ直接依存しないと実装できない。
5. AccessPolicy read/root discovery/create recoveryで既存ACL意味を変更する必要がある。
6. 新DB migrationが必要になり、単なるtransport公開を超える永続意味変更が発生する。
7. codegen candidateのlicense/securityがpolicy不適合。
8. finite body/timeout/resource boundを安全に決められず、無制限化が必要になる。
9. file body送信前Auditを維持できない。
10. Document DiffのPartial/Unknownをlosslessに表現できない。
11. 標準CI / Sandbox / DSI PoCの失敗原因が不明のまま。
12. 本番deploy、AD接続、PR mergeが必要になる。これらは別指示。

## 8. Git / PR strategy

- Design/Approval/PlanはDraft PR #27上で管理し、Frozen Design blobを変更しない。
- 計画承認後の実装は `feat/document-http-openapi-transport-v0-a` から開始。
- Unit A完了後、Draft PR Aを作成。B/C/DはAへstackしてよい。
- mainへ直接pushしない。
- TaskごとにRED→GREENの意味が追えるcommitを作る。
- CI記録だけのcommitでheadが変わった場合、計画が要求するexact-head gateを再確認する。
- 実装PR merge、本番deploy、AD integrationは別指示。

## 9. 実装開始時のexact action

計画承認・実装開始指示が別途記録された後、新しいsessionは次の順で実行する。

1. `AGENTS.md`
2. `docs/superpowers/execution/active.md`
3. `docs/superpowers/execution/document-http-openapi-transport-v0-status.md`
4. Frozen Design `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design.md`
5. Design Approval
6. 本Production Implementation Plan
7. Production Implementation Plan Approval（存在・blob一致を確認）
8. 最新main、PR #27、branch、CI、Cargo/pnpm lock、migration番号、architecture rules
9. 計画前提に影響するmain差分があれば先に差分レビュー
10. `feat/document-http-openapi-transport-v0-a` を最新の承認済み基点から作成し、HAPI-01のREDから開始

Production Implementation Plan Approvalが存在しない、またはblobが一致しない場合は**実装を開始しない**。

## 10. Current handoff state

この計画作成時点ではHAPI-01〜12はすべて未実装。製品コード、`spec/api/openapi.yaml` paths、production dependency、HTTP crateは変更していない。

次は本計画の書面レビュー。依頼者が明示的に `Production Implementation Planを承認します` 等と指示したら、計画blobを固定したapproval recordを作成できる。その後、別sessionで上記exact actionに従って実装を開始する。
