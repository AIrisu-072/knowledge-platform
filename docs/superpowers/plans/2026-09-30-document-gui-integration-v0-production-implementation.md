# Document GUI Integration v0 — Production Implementation Plan

> **For agentic workers:** REQUIRED: repository/GitHub current stateを正本として、RED→GREENとexact-head evidenceを残しながらG0〜G9を依存順に実行する。

**状態:** **PROPOSED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**

**Goal:** 承認済みDocument GUI Integration v0 Designを、DocumentRevision / GUI API gaps / typed client / Human GUIまで一括でproduction実装し、Mock 1〜7の主要業務経路を実Backendへ接続したreview-ready状態にする。

**Frozen Design:** `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design.md` blob `f132910ca5d3e638502f0b38447d9a1ec4020f24`。

## 1. Global constraints

- 本Planの明示承認前にproduct implementationを開始しない。
- 会話履歴ではなくGitHub/repositoryを正本とする。
- `Document.revision`、`DocumentVersion.version_no`、`DocumentRevision.major.minor`を混同しない。
- Human-facing Revision allocationはDocument row lock下でauthoritative mutationと同一transactionにする。
- historical metadataを推測・捏造しない。
- capability projectionはauthorizationの代替にしない。
- Identity display nameをDocument DBへ正本コピーしない。
- FrontendでOffice/PDF parserを持たない。
- Diff verdict/coverageをdisplay都合で変更しない。
- OpenAPI 3.2.1をdowngradeしない。
- TanStack Query server stateを別global storeへコピーしない。
- Motion completionをbusiness completion判定に使わない。
- main直接push禁止。product PRはDraftで積み上げ、deployは別指示。

## 2. Delivery units

| Unit | Tasks | 主目的 |
|---|---|---|
| A | G0〜G2 | predecessor closure / Revision / GUI Read Model |
| B | G3〜G5 | Capability / Identity / Revision+Diff Display |
| C | G6〜G7 | OpenAPI/client / Frontend foundation |
| D | G8〜G9 | Mock 1〜7 production GUI / cross-cutting acceptance |

## G0 — predecessor closure

**Purpose:** GUI implementationの基点を1本のmainへ確定する。

- [ ] PR #32 final head `3f870a92525afb6741e1ee72ee6c932eac0f0511` のStandard CI `36662871915`、Sandbox `36662871921`、DSI PoC `36662871930` を再確認する。
- [ ] HTTP acceptance/status/activeをHAPI-01〜12 COMPLETEへ更新し、docs-only exact-head gateを確認する。
- [ ] stacked PR #27 → #29 → #30 → #31 → #32 の順にbaseをmainへ繋ぎ直し、必要なexact-head checksを確認してmergeする。
- [ ] main merge commitのStandard CI / Sandbox / DSI PoCを確認する。
- [ ] Document HTTP/OpenAPI Transport v0 statusをCLOSED / MERGED / NOT DEPLOYEDとして記録する。
- [ ] GUI Design PR #35をmainへretargetし、Frozen Design / Design Approval / approved Production Plan / handoffだけのexact-head差分を確認する。
- [ ] PR #35のStandard CI / Sandbox / DSI PoCがSUCCESSしたらmainへmergeし、merge commitのgateを確認する。Frozen Design blobは変更しない。
- [ ] product implementation branchは、PR #35まで統合された最新mainから作る。

**STOP:** predecessor merge conflictが設計意味変更を要求する、またはmain gate原因不明FAIL。

## G1 — DocumentRevision schema / domain / transactions

**Files:** new migration, document-domain revision types, Application ports/services, Postgres transaction implementations, history tests/spec updates.

- [ ] RED: WORKING no revision、initial publish 1.0、new content publish Major+1.0、metadata update Minor+1、no-op no revision、non-revision operations unchanged、withdraw fallback next Major、withdraw no fallback no revision、T10 metadata Minor+1、OCC independence、concurrency allocation。
- [ ] migrationでappend-only `document_revisions` を追加。
- [ ] UUIDv7 revision ID、document/version refs、major/minor、metadata snapshot/status、source kind、actor、reason、operation ID、created_atを保存。
- [ ] historical legacy metadataを証明できない場合は `unavailable_legacy`。現在値で埋めない。
- [ ] Publish / Metadata mutation / Withdraw fallbackの既存transactionへRevision insertを原子的に組み込む。
- [ ] Domain/Audit Outbox semanticsを維持し、replay/no-opで重複Revisionを作らない。
- [ ] migration/repository/domain/application focused tests GREEN。

**STOP:** revision allocationに別transactionが必要、または既存OCC semanticsを変更する必要がある。

## G2 — GUI Read Model / Revision APIs / file summary

- [ ] `document_versions.updated_at` を追加し、Version content/lifecycle/schedule projection更新で維持する。
- [ ] GuiVersionSummary: versionNo/baseVersionId/lifecycle timestamps/isCurrent/updatedAt/fileSummary。
- [ ] GuiDocumentListItem: displayVersion/displayRevision/readState/displayTimestamp。
- [ ] Version file summaryをsingle-query/bounded aggregationで返し、N+1を作らない。
- [ ] `GET /v1/documents/{documentId}/revisions`
- [ ] `GET /v1/documents/{documentId}/revisions/{revisionId}`
- [ ] published/authoring/historyの代表Version選択規則をApplication所有のまま維持。
- [ ] keyset pagination、cursor binding、current authorization、T10/history semanticsを固定。
- [ ] OpenAPI schema/examplesとhandler contract tests GREEN。

## G3 — Action Capability Projection

- [ ] Document / Version / Folder detailのcapability contractをREDで固定。
- [ ] `available | disabled(reason)` を返す。
- [ ] reasonはpermission/lifecycle/pendingSchedule/staleBase/notCurrent/notHumanInteractive/unsupported。
- [ ] high-cost DSI/publish qualityは事前成功保証せずmutation時再評価。
- [ ] capability取得後のpolicy revoke/state raceでmutationが正しく拒否されること。
- [ ] list全件へ重いcapability計算を付けずdetail queryで取得。

## G4 — Identity Presentation / session

- [ ] `IdentityPresentationResolver` portを追加。batch resolve。
- [ ] current production Identity adapter未接続時はfail-closed authを維持しつつ、test resolverでpresentationを検証。
- [ ] History / PolicyのIdentityRefをpresentation enrichment。
- [ ] resolver unavailable/notFoundはDocument read全体をfailさせずsubjectId fallback。
- [ ] bounded TTL cacheをadapter内だけに閉じる。
- [ ] `GET /v1/session` を追加し、verified principal/presentation/invocationKind/expiresAtを返す。
- [ ] request body/headerからidentity自己申告不可を維持。

## G5 — Revision comparison / Diff Display Projection

- [ ] metadata snapshot comparison modelをREDで固定。same/different/unavailableLegacyを区別。
- [ ] same DocumentVersion revision pairはcontent Diffを再実行せずmetadata comparisonのみ。
- [ ] different DocumentVersionなら既存DocumentDiffServiceを再利用しmetadata diffとcompose。
- [ ] `POST /v1/documents/{documentId}/revision-comparisons` を実装。
- [ ] existing `POST /v1/documents/{documentId}/comparisons` に `projection=display` + pageSize/cursorを追加。
- [ ] display fragmentはauthoritative bytes + source locatorから生成。Frontend parser/search extraction/rendition禁止。
- [ ] text/table/structural/unavailable fragment union。
- [ ] default page 50、max 100、one-side 16 KiB、one item 32 KiB、one page 1 MiBの境界+1-over test。
- [ ] fragment truncation/unavailableでもDiff coverageを偽らない。
- [ ] current authorization、file access audit、freshness、display disclosure audit correlationを確認。
- [ ] persistent display cacheはv0では作らない。
- [ ] fragment本文をAudit/telemetryへ保存しない。

## G6 — OpenAPI 3.2 / typed client / Binary Bridge

- [ ] G1〜G5の全contractをOpenAPI 3.2.1 + JSON Schema 2020-12へ反映。
- [ ] all request/response examplesをschema検証。
- [ ] `@hey-api/openapi-ts` をexact Document APIでPoC。3.2.1 silent drop、union/nullability、all operations、compile、determinism、license/securityを確認。
- [ ] 不合格ならproductionへpromoteしない。OpenAPI downgrade禁止。
- [ ] generated JSON SDK/types + hand-written BinaryTransportBridgeを実装。
- [ ] Binary bridgeはcreate document/version multipart、Blob/File binding、download Blob/ReadableStream、Problem normalizationだけを所有。
- [ ] JSON DTO/business ruleの手書き複製禁止。
- [ ] TypeScript 7 compatibility gate。主要stack不成立ならTS6 fallback。

## G7 — Frontend foundation / Operational Design System

**Create:** `apps/document-web`。

- [ ] React 19 / Vite 8 / TanStack Router / Query / Table / Virtual / Motion / Ajv / Vitest / RTL / Playwrightをproduction selectionに従い追加。
- [ ] React Aria Componentsをfocused PoC: Dialog/Menu/Select/ComboBox/Popover/Tooltip/Tabs/Tree/form error/focus restoration。
- [ ] keyboard/a11y/composition/performance/license/security合格ならSELECTEDへ更新。不成立ならBase UI比較へSTOP。
- [ ] CSS Modules + CSS Custom PropertiesでDesign Tokens。
- [ ] Motion tokens: instant 0 / fast 90 / standard 140 / spatial 180 ms。
- [ ] reduced motion、focus ring、semantic state、error mapping、responsive minimum widthを共通化。
- [ ] Router=URL/filter/sort、Query=server state、Table=table state、React local/useReducer=workflow/presentation。TanStack Store/XState追加なし。
- [ ] Presentation componentからraw fetch/API URL/business invariantをarchitecture lint/testで禁止可能にする。

## G8 — Mock 1〜7 production GUI

Source DesignのVisual/Interaction meaningを実装する。

### Mock 1: Document list + Folder Tree + Context Panel
- published/authoring navigation
- filter/sort/pagination
- stable selection
- context panel spatial motion
- revision label / version working label
- Ready/Empty/Loading/Unauthorized
- return navigation context preservation

### Mock 2: Document detail / overview
- Overview / 版 / 新旧比較 / 履歴 / Access
- file download
- metadata summary
- capability-based actions
- Access tab only when appropriate

### Mock 3: Revision / Version management
- WORKING Version separate from issued revisions
- revision timeline
- revision pair selection
- new version action
- metadata revision identity

### Mock 4: New Version
- bounded multipart
- Idle/File selected/Pending/Validation Error/Conflict/Error
- no fake upload progress
- operation IDs/target IDs fixed across exact retry

### Mock 5: Publish / schedule
- Ready/Pending/Success/Conflict/Error
- high-risk confirmation
- authoritative success only after Backend response
- schedule UTC/date-time semantics

### Mock 6: Diff / revision comparison
- diff/comparison table/display
- Full/Partial/Unknown states
- metadata comparison
- bounded fragments/truncation
- source locator navigation
- context panel closed for wide workspace

### Mock 7: AccessPolicy
- inherited/explicit
- identity presentation
- Pending/Conflict/Error
- policy revision internal OCC
- capability-based visibility

## G9 — cross-cutting acceptance

- [ ] real PostgreSQL + FileSystemStorage + production DSI/Diff runners + HTTP + frontend E2E。
- [ ] root→folder→document→working→publish→revision→metadata minor→new major→withdraw fallback→revision monotonicityを縦断。
- [ ] revision comparison content+metadata。
- [ ] identity presentation fail-soft。
- [ ] capability stale/revoke race。
- [ ] file audit 0-byte failure semantics。
- [ ] keyboard-only primary flows。
- [ ] focus restoration。
- [ ] WCAG 2.2 AA automated + manual checklist。
- [ ] prefers-reduced-motion。
- [ ] visual regression major screens/states。
- [ ] T_input / T_usable / motion budget。
- [ ] 1280/1440 horizontal overflow。
- [ ] OpenAPI examples/handlers/generated client consistency。
- [ ] final same-head Standard CI / Sandbox / DSI PoC + frontend E2E。
- [ ] acceptance/status/activeをCOMPLETEへ。
- [ ] implementation PRはreview-ready、merge/deployしない。

## 3. STOP conditions

Frozen DesignのSTOP conditionsをすべて適用する。特に:
- display RevisionとOCC/Version identityを混同する必要が出る
- legacy metadataを推測する必要が出る
- frontend parserが必要
- capabilityをauthorization代替にする必要
- display fragmentをunboundedにする必要
- OpenAPI downgradeが必要
- production identityをallow-allにする必要
- React Aria/client generatorのpolicy不適合
- production deploy/AD接続が必要

## 4. Completion

G0〜G9すべてのfocused RED/GREEN、Unit gate、final exact-head evidenceが揃うまでCOMPLETEとしない。

Production Plan Approval記録が存在し、承認対象blobと一致するまで実装開始禁止。
