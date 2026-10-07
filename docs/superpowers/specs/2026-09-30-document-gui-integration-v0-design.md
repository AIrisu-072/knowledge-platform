# Document GUI Integration v0 — Written Design

2026-10-07追補：[文書詳細表示・未読戻し](2026-10-07-document-view-read-state-design.md)を参照。本凍結時の明示確認の意味と旧PUT wireは保持し、新VIEW/RESET・現在projection・専用receiptは追補が規定する。初回日時を現在badgeの根拠にせず、新操作資格hintはfresh整合GET200に従う。


- 状態: **PROPOSED / WRITTEN SPEC REVIEW PENDING**
- 日付: 2026-09-30 JST
- 対象: Document Platform Human GUI / GUI向けRead Model / Revision / Identity Presentation / Diff Display / Client Boundary
- 前提HTTP head: `3f870a92525afb6741e1ee72ee6c932eac0f0511`（Draft PR #32）
- Human GUI Source Design artifact SHA-256: `ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`

## 1. Purpose

Document HTTP/OpenAPI Transport v0で完成した共通APIを、Human GUI v0のSource Designへ安全に接続する。

対象は単なるReact画面実装ではない。Source Design作成で判明したGUI/API Gapを解消し、Documentの内容版・業務metadata改訂・認可・Identity表示・Diff表示・binary transport・Frontend stateを一貫した境界へ固定する。

最終形:

```text
Document authoritative state
        |
        +--> DocumentVersion       authoritative content generation
        +--> DocumentRevision      human-facing issued revision
        +--> Access / ReadState
        |
        v
Document Application
        |
        +--> GUI Read Model
        +--> Action Capability Projection
        +--> Identity Presentation
        +--> Revision Comparison / Diff Display
        |
        v
OpenAPI 3.2.1
        |
        v
Typed Client + Binary Transport Bridge
        |
        v
Document Human GUI
```

## 2. Normative sources and predecessor

Implementation開始時は以下を正本として再取得する。

1. `AGENTS.md`
2. `spec/architecture/architecture-contract-v0.md`
3. `spec/requirements/frontend-ux-requirements-v0.md`
4. `spec/selection/library-tool-selection-v0.md`
5. `spec/api/openapi.yaml`
6. Document Versioning / Management / Diff / HTTP Transportの承認済み仕様・現在実装
7. 本設計
8. 本設計の承認記録
9. 承認済みProduction Implementation Plan

Document HTTP/OpenAPI Transport v0はPR #32 exact head `3f870a92525afb6741e1ee72ee6c932eac0f0511` でStandard CI / Sandbox / DSI PoCがSUCCESSしている。ただしstacked PR #27/#29/#30/#31/#32は本設計作成時点で未mergeであり、旧Active/Statusの最終COMPLETE記録も追随していない。これらはProduction Implementationの最初の統合作業として扱う。

## 3. Human GUI Source Design

レビュー済みSource Design artifact:

`Document-Platform-Human-GUI-v0-Source-Design.zip`

SHA-256:

`ba3c1bba8056f299ac0e91a89f6279a4002b91ffb54d683d9560cad1a8115c86`

内容はMock 1〜7、Design System、Motion System、Accessibility/Keyboard、Error State Mapping、GUI/API Gap、HTML prototype、CSS/JS、verification evidenceを含む107 files。

このZIP自体はrepositoryのnormative SSOTではない。本設計が意味論上の正本であり、Source DesignはVisual / Interaction Referenceである。実装開始時にexact artifactが利用可能ならhashを照合して `docs/design/document-platform-human-gui-v0/` へsource filesをimportしてよい。画像を正本とせず、Design System / Motion / Screen spec / testsへ意味を移す。

## 4. Scope

### 4.1 In scope

- DocumentRevision Major/Minor model
- GUI向けDocument/Version/Revision Read Model
- lifecycle presentation inputs
- Version file summary
- action capability projection
- Identity presentation resolverとcurrent session presentation
- Revision history / metadata comparison
- authorized Diff display fragments
- OpenAPI 3.2.1のGUI向け拡張
- production typed client qualification
- binary upload/download bridge
- React GUI v0 Mock 1〜7
- Motion System v0
- Keyboard / Focus / WCAG 2.2 AA
- Conflict / Partial / Stale / Unauthorized / Error presentation
- visual regression / E2E / performance qualification

### 4.2 Out of scope

- Search Platform全文検索GUI
- LLM Chat / Agent Tool / CLI
- 本番Windows/AD/SSPI Identity Adapter接続
- general approval workflow新設
- Audit Store viewer
- Observability UI
- Mobile / Tablet
- production deployment
- PR mergeは実装計画の明示範囲に限り、deployは別指示

## 5. Core invariants

1. Document Platformが正本所有者である。
2. Human GUIとAgent/CLIでbusiness logicを複製しない。
3. FrontendはACL/lifecycle/business invariantを再実装しない。
4. `Document.revision` はOCC用であり、人間向け改訂番号に使わない。
5. `DocumentVersion.version_no` はauthoritative content generationの内部番号であり、人間向けMajorと同一である必要はない。
6. `DocumentRevision` は発行済み・表示用の業務改訂であり、Major.Minorを持つ。
7. WORKING Versionは正式Revisionではない。
8. Published content / Document metadataの履歴を捏造しない。
9. Identity display nameをDocument DBの正本として複製しない。
10. capability projectionはUI hintでありauthorizationの代替ではない。
11. Diff DisplayはDiff semantic verdict/coverageを変更しない。
12. OpenAPI 3.2.1をclient tooling都合でdowngradeしない。
13. Server StateをFrontend global storeへ複製しない。
14. Motion completionをbusiness completionに利用しない。

## 6. Three different revision identities

以下を型・命名・UIで分離する。

```text
DocumentVersion.version_no
= content object creation generation

DocumentRevision.major.minor
= human-facing issued revision

Document.revision
= optimistic concurrency / mutation revision
```

例:

```text
DocumentVersion #5
DocumentRevision 7.2
Document.revision = 41
```

は有効である。

内部Version番号と表示Majorを直結しない。理由はCurrent Version withdrawalで過去DocumentVersionを復元する場合でも、人間向けRevision番号を逆行させないためである。

## 7. DocumentRevision model

### 7.1 Identity

```text
DocumentRevision
├─ revision_id              UUIDv7
├─ document_id
├─ document_version_id      authoritative content参照
├─ major_no                 >= 1
├─ minor_no                 >= 0
├─ metadata_snapshot
├─ metadata_snapshot_status
├─ source_kind
├─ operation_id             nullable for legacy
├─ created_at
├─ actor_identity_provider
├─ actor_principal_id
└─ reason
```

一意性:

```text
(document_id, major_no, minor_no)
```

`document_version_id` は複数Revisionから参照され得る。Withdraw fallbackで同じ過去Versionを新しいMajorとして再有効化できるため、`(document_version_id, minor_no)` をidentityにしない。

### 7.2 Metadata snapshot

対象はDocument Management Basics T5のDocument-level業務metadata:

- `document_type`
- `owning_department`
- `category`
- `extensions`

Folder、AccessPolicy、ReadState、publication stateはsnapshot対象外。

Title、authoritative file/content、Version metadataはDocumentVersion側のcontent generationに属する。

新規Revisionは必ず完全なmetadata snapshotを持つ。

Legacy migrationで歴史的metadataを復元不能な場合のみ:

```text
metadata_snapshot_status = unavailable_legacy
metadata_snapshot = null
```

を許容する。過去metadataを現在値や空objectで捏造しない。

### 7.3 Source kind

最低限:

```text
initialPublication
contentPublication
metadataRevision
withdrawFallback
legacyBackfill
```

## 8. Revision allocation rules

Document row lock下でMajor/Minorを採番する。

### 8.1 Initial publication

初回WORKING中は正式Revisionを発行しない。

```text
Version 1 / 下書き
    |
    | Publish
    v
Revision 1.0
```

Publish transaction内でVersion publish/current switch/operation ledger/outbox/auditと同時にRevision 1.0を作成する。

### 8.2 New content publication

新しいWORKING VersionをPublishし、current contentが変化する場合:

```text
latest revision = 5.3
Version #6 publish
        ↓
Revision 6.0   // 次のmajor。Version #6であることは別field
```

MajorはDocument単位で `max(major_no)+1`。

Minorは0へreset。

### 8.3 Metadata mutation

T5 metadataが実変更され、既に少なくとも1件の正式Revisionがある場合:

```text
6.0
 ↓ metadata update
6.1
 ↓ metadata update
6.2
```

同値no-opではRevisionを増やさない。

metadata mutationの既存Management operation ID / reason / actor / occurred_atをRevisionへ接続し、Document mutation・management ledger・Domain/Audit event・DocumentRevisionを同一transactionで確定する。

初回Publish前のmetadata変更ではRevisionを発行しない。Publish時点の最新metadataが1.0 snapshotになる。

### 8.4 Non-revision operations

以下ではMajor/Minorを変更しない。

- Folder move
- AccessPolicy mutation
- ReadState
- schedule / cancel
- plain Publish state transition without content switch
- T10 publication end
- file download/read
- Audit/telemetry

### 8.5 Withdrawal with fallback

current VersionをWithdrawし、以前のeligible PUBLISHED Versionをcurrentへ復元する場合、利用者から見たeffective contentが変わるため新Majorを発行する。

```text
Revision 5.2 -> Version #5 current
Withdraw #5
fallback Version #4
        ↓
Revision 6.0 -> Version #4
```

表示Majorは逆行しない。

snapshotはwithdraw時点の現在Document metadata。

### 8.6 Withdrawal without fallback

current Versionがnullになるだけで新しいcontent revisionは存在しないため、新Revisionは発行しない。

最後に発行済みのRevisionは履歴として残し、UIはpublication statusと分離して「公開なし / 公開終了」を示す。

### 8.7 T10後のmetadata mutation

T10後に許可されたDocument metadata変更では、最後に発行されたRevisionと同一MajorにMinorを積む。

```text
5.2 / 公開終了
 ↓ metadata update
5.3 / 公開終了
```

Revisionはcontent/metadata履歴であり、publication availabilityとは独立する。

## 9. Legacy migration

既存DocumentVersionを新Revision modelへ移行する際、歴史を捏造しない。

- PUBLISHED Versionの順序をcontent historyとして保持する。
- current/last known Versionについて現在Document metadataを完全snapshotとしてbackfill可能。
- historical Versionの過去metadataを証明できない場合は `unavailable_legacy`。
- WORKINGだけのDocumentにはRevisionを作らない。
- migrationはdeterministicで再実行可能、rollback可能であること。
- production実データが未存在でもlegacy test fixturesで上記を証明する。

## 10. GUI Read Model

### 10.1 Revision summary

```ts
type DocumentRevisionSummary = {
  revisionId: string
  major: number
  minor: number
  label: string
  documentVersionId: string
  createdAt: string
  reason: string
  sourceKind:
    | "initialPublication"
    | "contentPublication"
    | "metadataRevision"
    | "withdrawFallback"
    | "legacyBackfill"
  metadataSnapshotStatus: "complete" | "unavailableLegacy"
}
```

`label` はBackend projectionとして返してよいが、machine logicはmajor/minorを使う。

### 10.2 Version summary

```ts
type GuiVersionSummary = {
  versionId: string
  versionNo: number
  baseVersionId: string | null
  lifecycleState: "WORKING" | "PUBLISHED" | "WITHDRAWN"
  isCurrent: boolean
  approvedAt: string | null
  scheduledPublishAt: string | null
  publishedAt: string | null
  withdrawnAt: string | null
  updatedAt: string
  fileSummary: VersionFileSummary
}
```

`document_versions.updated_at` を追加し、WORKING content update/rebaseおよびVersion lifecycle/schedule projection変更時に更新する。これはOCC revisionではない。

### 10.3 File summary

```ts
type VersionFileSummary = {
  authoritativeItemCount: number
  totalSizeBytes: number
  primary: {
    displayName: string
    mediaType: string
    sizeBytes: number
  } | null
}
```

一覧/Version historyでN+1 file listを行わない。

### 10.4 Document list item

published/authoring/historyの代表Version選択規則は既存Applicationが所有する。

```ts
type GuiDocumentListItem = {
  documentId: string
  title: string
  folderId: string
  displayVersion: GuiVersionSummary
  displayRevision: DocumentRevisionSummary | null
  readState: {
    isRead: boolean
    firstReadAt: string | null
  }
  displayTimestamp: {
    kind: "revisionCreatedAt" | "workingUpdatedAt"
    value: string
  }
}
```

published viewでは `displayRevision.createdAt`。
初回未公開WORKING等、正式Revisionがないauthoring viewではVersion `updatedAt`。

UIは必要に応じてラベルを「改訂日時」「更新日時」と分ける。

## 11. Revision history API

Document Workspaceの「版」タブはContent Versionと正式Revisionを区別して表示する。

新規read:

```http
GET /v1/documents/{documentId}/revisions
GET /v1/documents/{documentId}/revisions/{revisionId}
```

Revision historyは新しい順のkeyset pagination。

WORKING VersionはRevision listとは別にVersion readから表示する。

概念UI:

```text
Version 7 / 下書き

Revision 6.1  metadata改訂
Revision 6.0  旧内容へ復元
Revision 5.2  metadata改訂
Revision 5.0  本文改訂
```

## 12. Metadata comparison

正式Revision間ではmetadata snapshot同士を比較可能にする。

```text
MetadataChange
├─ field
├─ before
├─ after
└─ operation
```

対象fieldはT5管理対象だけ。

legacy snapshot unavailableの場合はUnknown/Unavailableを明示し、空値や変更なしへ変換しない。

## 13. Action Capability Projection

FrontendはACL/lifecycleを推測しない。

### 13.1 Meaning

Capability projectionは:

> 現在のVerifiedActorContextと現在authoritative stateに基づき、GUIがそのActionを提示してよいかを示すderived hint。

Mutation時のserver-side authorization/preflightを代替しない。

### 13.2 Availability

```ts
type ActionAvailability =
  | { status: "available" }
  | {
      status: "disabled"
      reason:
        | "permission"
        | "lifecycle"
        | "pendingSchedule"
        | "staleBase"
        | "notCurrent"
        | "notHumanInteractive"
        | "unsupported"
    }
```

`available` は成功保証ではない。Publish quality / DSI等の高コストpreflightは実Mutation時に行う。

### 13.3 Document / Version capability

Document detail responseにDocument capabilitiesを含める。

- createVersion
- updateMetadata
- moveDocument
- endPublication
- manageAccess
- compareVersions

Version detail/summaryには必要な範囲で:

- edit
- rebase
- publish
- withdraw
- schedulePublication
- cancelPublicationSchedule
- download

Folder readでは:

- createDocument
- createFolder
- renameFolder
- moveFolder
- manageAccess

一覧全件へ重いCapability評価を付けない。Mock 1 Context Panelで必要な操作は選択時のdetail queryから取得する。

## 14. Identity Presentation

### 14.1 Ownership

Identity System owns:

- authentication identity
- principal/group/role identity
- display name / directory presentation

Document DBはdisplay nameを正本としてコピーしない。

### 14.2 Contract

```ts
type IdentityRef = {
  provider: string
  kind: "principal" | "group" | "role"
  subjectId: string
}

type IdentityPresentation = {
  ref: IdentityRef
  displayName: string | null
  secondaryText: string | null
  resolution: "resolved" | "notFound" | "unavailable"
}
```

新しいApplication port:

```text
IdentityPresentationResolver
```

batch resolveをサポートし、History/PolicyでN+1 directory lookupを避ける。

### 14.3 Failure semantics

Identity presentation取得失敗でauthorized Document read全体を失敗させない。

fallback:

```text
resolved
→ displayName

notFound/unavailable
→ subjectId + availability marker
```

bounded TTL cacheはIdentity adapter側で許容するが、Document DBへ永続コピーしない。

### 14.4 Current session

GUI user menu / actor indication用:

```http
GET /v1/session
```

verified principal、presentation、invocationKind、expiresAtを返す。Client request body/headerからidentityを自己申告させない。

## 15. AccessPolicy presentation

既存PolicyのsubjectはIdentityPresentationをinline enrichmentする。

Machine identity:

```text
kind/provider/subjectId
```

は保持し、GUI display nameだけでPolicy mutation targetを指定しない。

## 16. Diff Display Projection

### 16.1 Separation

```text
DiffResult
= semantic comparison truth

DiffDisplayProjection
= authorized bounded human-readable representation
```

Display ProjectionはDiff result digestやSame/Different/Unknown、Full/Partial/Noneを変更しない。

### 16.2 DisplayFragment

```ts
type DisplayFragment =
  | {
      kind: "text"
      text: string
      truncated: boolean
      locator: SourceLocator
    }
  | {
      kind: "table"
      cells: Array<{
        row: number | null
        column: number | null
        label: string | null
        value: string
      }>
      truncated: boolean
      locator: SourceLocator
    }
  | {
      kind: "structural"
      summary: string
      locator: SourceLocator
    }
  | {
      kind: "unavailable"
      reason:
        | "nonTextual"
        | "unverified"
        | "resourceLimit"
        | "unsupported"
    }
```

FrontendにDOCX/XLSX/PPTX/PDF parserを置かない。

原本表示用内容はauthoritative file bytesとDiffのsource locatorからBackendで作る。Search extraction / rendition / browser-side parserを表示本文の正本にしない。

### 16.3 Execution / authorization boundary

Version同士の表示本文は既存Comparison endpointを拡張する。

```http
POST /v1/documents/{documentId}/comparisons
```

既存 `projection=diff|comparisonTable` に `projection=display` を追加する。

`display` requestは少なくとも:

- baseVersionId
- targetVersionId
- profile
- pageSize
- cursor

を持つ。

Display Projectionは以下を必須とする。

1. 現在のVerifiedActorContextで両Versionの現在認可を確認する。
2. semantic DiffResultを既存DocumentDiffServiceの意味で確定する。
3. display fragment用authoritative bytesを必要な範囲だけ既存の監査付きfile access境界から取得する。
4. source locatorとraw bindingを再確認する。
5. bounded format-specific display executorでfragmentを生成する。
6. 結果開示直前に現在認可・input freshnessを再確認し、Diff結果開示Auditと相関可能なdisplay access evidenceを残す。
7. Audit/authorization/commit outcome unknownではfragmentを返さない。

実装最適化でDiff計算とdisplay生成のfile openを共有してよいが、必須Auditと現在認可の意味を弱めない。

Display fragmentはcanonical DiffResult / result digestへ含めない。永続保存しない。v0ではcross-requestのpersistent display cacheを作らない。bounded in-process ephemeral cacheを導入する場合も、keyをresult digest + display profileへbindingし、cache hitごとに現在認可を再確認する。

### 16.4 Resource profile

開始値:

- page size default 50
- max page size 100
- one-side serialized fragment max 16 KiB
- one display item max 32 KiB
- one JSON display page max 1 MiB

超過時は `truncated=true` またはUnavailableを返し、coverageをFullへ偽装しない。

全文が必要なら既存の監査済みfile downloadを使う。

本文fragmentをAudit/telemetryへ複製しない。

## 17. Revision comparison

正式Revision同士を比較する新しいApplication compositionを追加する。

HTTP endpoint:

```http
POST /v1/documents/{documentId}/revision-comparisons
```

Request:

```text
baseRevisionId
targetRevisionId
projection = diff | comparisonTable | display
pageSize / cursor  // display時
```

同一Documentの異なるRevisionだけを許可する。

```text
Revision A
├─ DocumentVersion A
└─ MetadataSnapshot A

Revision B
├─ DocumentVersion B
└─ MetadataSnapshot B
```

同一DocumentVersionならcontent Diffを再計算せずmetadata diffのみ。

異なるDocumentVersionなら既存DocumentDiffServiceを再利用し、metadata diffを合成する。

Responseは:

- base/target revision
- content comparison status
- content verdict / coverage（異なるDocumentVersionの場合）
- metadata comparison status
- metadata changes
- display items（projection=display）
- unverified regions
- content result digest（存在する場合）
- metadata snapshot digests
- audit event

を分離して持つ。

同一DocumentVersionのRevision比較ではcontent comparison statusを `sameAuthoritativeVersion` とし、Document Diffを再計算しない。metadata snapshotだけを比較する。

Revision comparison cursorはprincipal、documentId、baseRevisionId、targetRevisionId、metadata snapshot digest、content result digest（存在時）、projection、page positionへbindingする。

WORKING Version対正式Revisionの内容比較は既存Version comparison APIを使用する。WORKINGを正式Revisionとして捏造しない。

## 18. Typed client boundary

### 18.1 OpenAPI remains SSOT

OpenAPI 3.2.1をdowngradeしない。

既存:

- `openapi-typescript`: binary multipart縮退
- `json-schema-to-typescript`: binary縮退
- `typify`: Document API fixture compile failure

### 18.2 Preferred qualification candidate

`@hey-api/openapi-ts` を次のproduction client generator候補としてPoCする。

採用条件:

- exact OpenAPI 3.2.1 contractをsilent dropせず処理
- all operations/types deterministic generation
- unions/nullability/error types維持
- TypeScript compile
- generated JSON SDKとruntime validationの整合
- license/security gate
- generator version exact pin

PoC不成立でもOpenAPIをdowngradeしない。

### 18.3 Binary Transport Bridge

binary upload/downloadは明示的なhand-written boundaryを許可する。

```text
Generated JSON SDK / Types
        +
BinaryTransportBridge
```

責務:

- multipart FormData
- Blob/File binding
- Version manifest part mapping
- download Blob/ReadableStream
- RFC9457 error normalization

JSON business DTOを手書き複製しない。

## 19. Frontend stack

既存selectionを基本とする。

- React 19
- Vite 8
- TanStack Router
- TanStack Query
- TanStack Table
- TanStack Virtual
- Motion
- CSS Modules + CSS Custom Properties
- Ajv
- Vitest
- React Testing Library
- Playwright

### 19.1 UI primitive decision procedure

React Aria Componentsをpreferred candidateとする。

Production dependency promotion前にMock 1〜7で必要な:

- Dialog
- Menu
- Select/ComboBox
- Tooltip/Popover
- Tabs
- Tree
- form error / focus restoration

をfocused PoCする。

合格ならSELECTEDへ更新。

重大なcomposition/performance/API stability問題があればBase UI比較へ戻る。custom primitiveを先に作らない。

### 19.2 Client state

TanStack Storeはv0 productionへ追加しない。

```text
URL/filter/sort
= TanStack Router

Server state
= TanStack Query

Table state
= TanStack Table

Workflow/local interaction
= React state / useReducer

Presentation state
= React local state
```

TanStack Query dataを別global storeへコピーしない。

XStateはv0で使わない。

## 20. TypeScript version gate

TypeScript 7 compatibilityをimplementation前半で確認する。

React 19 / Vite 8 / TanStack family / Motion / selected primitive / generated client / Vitestがcompile/typecheckすればTS7を採用。

不成立なら既存selectionどおりTypeScript 6.x fallbackをpinする。

ArchitectureをTS version都合で変更しない。

## 21. Frontend architecture

候補:

```text
apps/document-web/
├─ src/
│  ├─ api/
│  │  ├─ generated/
│  │  ├─ binary-transport/
│  │  ├─ validation/
│  │  └─ problem-mapping/
│  ├─ application/
│  │  ├─ documents/
│  │  ├─ revisions/
│  │  ├─ versions/
│  │  ├─ diff/
│  │  ├─ policy/
│  │  └─ session/
│  ├─ view-model/
│  ├─ components/
│  │  ├─ primitives/
│  │  ├─ document/
│  │  ├─ version/
│  │  ├─ diff/
│  │  └─ policy/
│  └─ routes/
```

Presentation Componentは禁止:

- raw fetch
- API URL
- ACL rule
- lifecycle invariant
- revision allocation
- RFC9457文字列判定

```text
Component
  ↓
View Model / Action
  ↓
Typed Client / Binary Bridge
```

## 22. GUI visual / interaction contract

Source Designの骨格を維持する。

- Desktop 1440×900基準
- Light neutral UI
- 3 pane document explorer
- left navigation + Folder Tree
- central high-density document table
- right Context Panel
- Document Workspace tabs: 概要 / 版 / 新旧比較 / 履歴 / アクセス権
- card乱用なし
-状態を色だけで示さない
- WCAG 2.2 AA
- Keyboard主要業務完結

正式Revisionは「改訂 5.2」。

WORKINGは「Version 7 / 下書き」。

正式RevisionとWORKING Versionを同じラベルで表示しない。

## 23. Motion System v0

固定token:

```text
instant   0 ms
fast      90 ms
standard  140 ms
spatial   180 ms
```

- row selection: fast
- status / tab: standard
- disclosure: standard
- Context Panel / spatial layout: spatial

`prefers-reduced-motion: reduce` では意味を維持してinstantへ縮退。

Motionはnext operationをblockしない。

## 24. Complete UI states

各Featureで必要な状態を設計・試験する。

```text
Initial
Empty
Loading
Pending
Ready
Partial
Stale
Conflict
Error
Unauthorized
Unavailable
Disabled
```

0件と取得失敗を混同しない。

ConflictはToastだけで終わらせず対象面へ表示。

## 25. Data refresh / navigation

- list → detail → backでfolder/filter/sort/scroll/selectionを保持
- selectionはstable ID
- background refreshで読んでいる対象を突然差し替えない
- stale/conflictを明示してreconcile
- high-risk mutationはoptimistic success禁止

## 26. OpenAPI additions

設計上必要な変更:

1. Document list/detailへRevision / Version read modelを追加
2. Version readへ`versionNo`, `baseVersionId`, lifecycle timestamps, `updatedAt`, file summary
3. `GET /v1/documents/{documentId}/revisions`
4. `GET /v1/documents/{documentId}/revisions/{revisionId}`
5. Document/Version/Folder detailへAction Capability Projection
6. History / Policy responsesへIdentityPresentation enrichment
7. `GET /v1/session`
8. existing `POST /v1/documents/{documentId}/comparisons` へ `projection=display` とbounded pagingを追加
9. `POST /v1/documents/{documentId}/revision-comparisons`
10. machine-readable metadata comparison
11. exact OpenAPI examples + schema tests

OpenAPI 3.2.1が引き続きtransport SSOT。

## 27. Error / Conflict

既存RFC9457 stable codeへ新しいmachine reasonが必要ならError Registryへ追加する。

候補:

- REVISION_NOT_FOUND
- REVISION_COMPARISON_STALE
- IDENTITY_PRESENTATION_UNAVAILABLE は通常partial presentationであり全面errorにしない
- DIFF_DISPLAY_RESOURCE_LIMIT はfragment-level unavailable/truncatedを優先

既存409 OCC/OperationConflictを再利用可能な場合は新codeを増やさない。

## 28. Transaction / audit boundaries

DocumentRevision発行を必要とするoperationは、既存authoritative mutationと同一transactionで確定する。

- Publish new content → Revision Major.0
- Metadata mutation → same Major + next Minor
- Withdraw fallback → next Major.0

Audit/Outboxの原子性を維持。

Revision display read自体を全件Audit対象にしない。

Diff/revision comparisonの原文display開示は既存Diff結果開示Auditと整合し、原文fragmentをAudit本文に保存しない。

## 29. Implementation ordering

Design承認後のProduction Planは最低限以下の依存順を持つ。

```text
G0 predecessor closure
  HTTP status COMPLETE
  stacked PR #27/#29/#30/#31/#32 integration + main gate

G1 DocumentRevision schema/domain/transactions/migration

G2 GUI Read Model / revision APIs / file summary / lifecycle projection

G3 Action Capability Projection

G4 Identity Presentation / session

G5 Revision Comparison / Diff Display Projection

G6 OpenAPI + client generator PoC + Binary Bridge

G7 Frontend foundation / Design System / Motion / accessibility

G8 Mock 1–7 production GUI

G9 cross-cutting E2E / keyboard / a11y / visual / performance
```

全Taskを一つのProduction Implementation Planで実行可能にする。

## 30. STOP conditions

以下は書面Design Amendmentなしに進めない。

1. `Document.revision` を表示Revisionへ流用する必要が出る。
2. DocumentVersionそのものをMajor.Minorへ変更する必要が出る。
3. metadata historyを証拠なしにbackfillする必要が出る。
4. GUIがACL/lifecycleを独自実装する必要が出る。
5. Identity display nameをDocument DB正本へコピーする必要が出る。
6. Frontend document parserが必要になる。
7. Diff Partial/Unknownを表示の都合で縮退する必要が出る。
8. OpenAPI 3.2.1 downgradeが必要になる。
9. production allow-all identityが必要になる。
10. unbounded Diff display/request/responseが必要になる。
11. UI primitive/client generatorがproject license/security policyへ不適合。
12. deploy/本番AD接続が必要になる。

## 31. Acceptance criteria

### Revision

- REV-01: WORKINGは正式Revisionを発行しない。
- REV-02: initial Publish = 1.0。
- REV-03: effective content switchはMajor+1 / Minor=0。
- REV-04: T5 metadata実変更は同Major Minor+1。
- REV-05: T5 no-opはRevisionを増やさない。
- REV-06: Folder/ACL/ReadState/schedule/T10だけではRevisionを増やさない。
- REV-07: Withdraw fallbackでもMajorが逆行しない。
- REV-08: historical metadata unavailableを捏造しない。
- REV-09: Document.revision/OCCと表示Revisionが独立。

### GUI/API

- GUI-01: list/detailでN+1なくversionNo/revision/file summaryを表示可能。
- GUI-02: lifecycle label導出に必要な入力が取得可能。
- GUI-03: Backend capabilityでAction表示を制御し、mutationは再認可。
- GUI-04: History/Policyにidentity displayをfail-softで表示。
- GUI-05: current session presentationを取得可能。
- GUI-06: Diff displayにbounded old/new fragmentを表示可能。
- GUI-07: Same/Different/Unknown × Full/Partial/Noneをlosslessに保持。
- GUI-08: revision metadata diffが可能。
- GUI-09: binary upload/downloadをtyped boundaryで扱う。
- GUI-10: raw fetch/API URLをPresentation componentが持たない。

### UX

- UX-01: Mock 1〜7の主要Ready/Conflict/Partial/Pending stateを実装。
- UX-02: Keyboardだけで主要業務経路を完結。
- UX-03: WCAG 2.2 AA自動+手動検査。
- UX-04: Motion tokenとReduced Motionを維持。
- UX-05: Context Panel spatial transitionはnext interactionをblockしない。
- UX-06: list/detail navigation contextを維持。
- UX-07: high-risk actionをBackend成功前にSuccess表示しない。
- UX-08: visual regressionで重要情報破壊を検知。

## 32. Approval boundary

本書はWritten Design候補。

依頼者の明示承認前に:

- DB migration
- Domain/Application product code
- OpenAPI product changes
- frontend app
- production dependency promotion
- predecessor stacked PR merge

へ進まない。

承認後にDesign Approval recordを作成し、G0〜G9を含む単一Production Implementation Planを作成・レビューする。
