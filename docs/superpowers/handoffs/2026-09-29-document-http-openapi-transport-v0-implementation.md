# Document HTTP/OpenAPI Transport v0 — Implementation Session Handoff

対象repository: **`AIrisu-072/knowledge-platform`**

このsessionでは、Document HTTP/OpenAPI Transport v0 のProduction Implementationを実行してください。

## Source of Truth

会話履歴や記憶から状態を再構成せず、**GitHub/repositoryの現在状態を正本**として扱ってください。

最初に必ず以下の順で確認してください。

1. `AGENTS.md`
2. `docs/superpowers/execution/active.md`
3. `docs/superpowers/execution/document-http-openapi-transport-v0-status.md`
4. `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design.md`
5. `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design-approval.md`
6. `docs/superpowers/plans/2026-09-29-document-http-openapi-transport-v0-production-implementation.md`
7. `docs/superpowers/plans/2026-09-29-document-http-openapi-transport-v0-production-implementation-approval.md`
8. Draft PR #27、最新main、対象branch、exact-head CI、Cargo/pnpm lock、migration番号、architecture rules

期待する凍結値:

- Frozen Design blob: `88f7046a5d14a77f4091df0c92691f6634dd57d7`
- Production Implementation Plan blob: `111914181143672d3fec901dae75fc2af0256b15`

## Mandatory approval gate

**Production Implementation Plan Approvalが存在しない、状態が承認済みでない、または承認対象blobが `111914181143672d3fec901dae75fc2af0256b15` と一致しない場合は、製品実装を開始しないでください。**

その場合は、現在状態と不足している承認だけを報告して停止してください。

承認記録が存在し、上記blobと一致し、実装開始が明示されている場合のみ以下へ進んでください。

## Starting state

- Document PlatformのAuthoritative Core、Versioning、Publication、Publication End、Document Management Basics、Document Semantic Inspection、Document Diffは既存実装を正本として再利用する。
- Document Diff設計PR #23 / 実装PR #24はmain統合済み。
- Document HTTP/OpenAPI Transport v0 の書面設計は承認済み。
- Design / Approval / Production PlanはDraft PR #27で管理している。
- 本Capabilityの製品実装はまだ開始していない前提だが、必ずGitHubの現在状態で再確認する。
- 本番deploy、Windows/AD/SSPI実接続、GUI、CLI/MCP/Agent Tool、Search APIはこの実装範囲外。

## Implementation rules

承認済みProduction Implementation Planの **HAPI-01〜12をA→B→C→Dの依存順に実行**してください。

- Unit A: HAPI-01〜03
- Unit B: HAPI-04〜05
- Unit C: HAPI-06〜09
- Unit D: HAPI-10〜12

各Taskは計画に記載されたRED→GREEN、focused tests、commit、verificationを守ってください。

### Branch / PR

実装開始時のGitHub状態を確認し、以下に従ってください。

- PR #27が未mergeなら、承認済みplanを含むexact headを基点に `feat/document-http-openapi-transport-v0-a` を作り、Unit AのDraft PRをPR #27へstackする。
- PR #27がmainへmerge済みなら、そのmergeを含む最新mainを基点にUnit A branchを作る。
- mainへ直接pushしない。
- B/C/Dは計画どおりstacked Draft PRにしてよい。
- commit/push/Draft PR更新は実装に含む。
- **PR merge、本番deploy、本番migration、AD接続は行わない。**

### Frozen invariants

以下を実装都合で変えないでください。

1. OpenAPI 3.2.1 + JSON Schema 2020-12がtransport contractのSSOT。
2. Human UI / LLM Agent / CLIは共通API。別business APIを作らない。
3. HTTP payload/query/multipart/custom headerからPrincipal、Group、Role、InvocationKind、delegationを自己申告させない。
4. `VerifiedActorContext` はtrusted Identity Adapterだけが生成する。production allow-all identityは禁止。
5. `document-api-http` からPostgreSQL、FileStorage、SQLx、Diff/DSI runner具象へ直接依存しない。
6. handler内でACL、Domain transition、operation replay、Auditを再実装しない。
7. UUIDv7 operation ID、expected revision、Version createの `targetVersionId`、Folder createの `folderId` をexact retry identityとして維持する。
8. initial Document createのcommit unknownはblind POST retryしない。認可済みoutcome recoveryを使う。
9. file downloadは認可・必須Audit commit成功前にStorage open / byte送信しない。
10. Document Diffの `verdict` / `coverage` / `unverifiedRegions` をlosslessに返し、Partial/UnknownをSameやHTTP errorへ変換しない。
11. RFC 9457 stable error codeを使い、human-readable error文字列、SQLSTATE、library error文字列でmachine branchingしない。
12. finite resource/timeout/cancellation境界を維持し、test通過のために上限を勝手に緩和しない。
13. Document responseは原則private/no-store。credential付きwildcard CORSは禁止。
14. telemetry/access logへ本文、抜粋、upload byte、credential、Storage locator、AccessPolicy全文を出さない。

## Important plan-specific checks

HAPI-01では、`typify` / `openapi-typescript` / `json-schema-to-typescript` をPOC REQUIREDとして実contractで資格判定してください。失敗した候補をproductionへ追加せず、OpenAPIを3.1へdowngradeしないでください。

HAPI-02では以下のApplication gapsをTransportより先に解消してください。

- administer認可付きAccessPolicy read
- root Folder discovery
- authorized initial-create outcome lookup
- typed management error surface

TransportからRepositoryへ直接読みに行く代替は禁止です。

HAPI-06/HAPI-11ではPlan §4.1の初期HTTP resource profile候補を用いて境界/1-over/実測を行ってください。代表文書で不合理なら値を黙って緩和せずSTOP条件として記録してください。

HAPI-09ではaudit失敗時に `storage_open_calls == 0` とresponse body 0 byteを実証してください。

HAPI-12ではOpenAPI operation coverage、real handler schema一致、authorization revoke、exact replay、commit unknown、file audit、Diff Partial、cursor stale、T10、expired/unavailable identityを横断確認してください。

## STOP conditions

Production Plan §7のSTOP Conditionsをすべて適用してください。

特に以下では勝手に回避策を実装しないでください。

- Frozen Designの意味変更が必要
- OpenAPI 3.2.1をdowngradeする必要がある
- allow-all identityまたはpayload自己申告が必要
- HTTP→Repository/Storage直接依存が必要
- 新DB migrationで永続意味変更が必要
- file Audit前byte送信を避けられない
- Diff Partial/Unknownをlosslessに表現できない
- finite resource boundを安全に決められない
- CI/Sandbox/DSI PoC失敗原因が不明
- merge/deploy/AD接続が必要

STOPした場合はCapability statusへ、blocker、証拠、最後のGREEN head、次に必要な判断を記録してください。

## Verification / status updates

- Taskごとのfocused RED/GREENを残す。
- hosted CIをTaskごとに乱発せず、Production PlanのUnit A/B/C/D最終headで確認する。
- Unit Dの最終同一headでは、標準CI、DSI Sandbox Preflight、DSI PoC、HTTP acceptanceをすべて確認する。
- pending/cancelled/failedをSUCCESS扱いしない。
- session終了・context上限前にstatus/activeを更新し、完了Task、現在Task、verification、branch/PR、blocker、次のexact actionを残す。
- fresh evidenceなしに「完了」と宣言しない。

## Completion target

STOP条件に該当しない限り、HAPI-01〜12の予定実装と検証を最後まで進めてください。

最終報告には最低限以下を含めてください。

1. 完了したHAPI Task / Delivery Unit
2. 作成・更新したDraft PR
3. 最終exact head
4. local / hosted verification結果
5. OpenAPI/codegen qualification結果
6. resource/timeout実測
7. Identityが未接続であることとGUIが利用可能な契約範囲
8. 未解決blocker / evidence limits
9. merge / deployを行っていないこと

**実装PRのmergeと本番deployは行わず、レビュー可能な状態で終了してください。**
