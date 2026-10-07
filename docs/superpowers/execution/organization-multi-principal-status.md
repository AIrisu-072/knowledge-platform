# Organization 複数担当・役割・委任（U1）— Capability Execution Status

## 2026-10-07 — U1実装・ローカル検証完了、Draft PR/hosted CIへ

### 位置付け

- 基点：main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（引継ぎ値と一致を確認）。main push CI `37562024089` は success
- PR43受入点：exact head `6103e4d4e3bb0d45ba03e1d2935492de7f11394a` / tree `f2e13eee0d7e1bfa71952c1da52a72cecb65fc9e` がmainの祖先で、[依頼者の受入記録](https://github.com/AIrisu-072/knowledge-platform/pull/43#issuecomment-5952167525)と一致。Phase0→1→2→3の凍結を保持し、やり直さない
- PR62（合成Agent）とPR67（完了・保留再開・Document参照）はmainへ統合済み。この単位はそれらを再実装せず、担当可否の固定判定だけを置き換える
- 設計の具体化：[実装追補](../specs/2026-10-07-organization-multi-principal-amendment.md)、手順：[小計画](../plans/2026-10-07-organization-multi-principal.md)、利用手順：[Organization Browser PoC](../../operations/organization-browser-poc.md#複数の担当者役割委任を使う)
- branch：`claude/trusting-knuth-dn5cx4`

### 実装した内容

- Domain（`work-domain`）：`OrganizationalUnit`／`BusinessRole`／`RoleAssignment`（有効期間・取消）／`Delegation`（期限・取消・縮小のみ・再委任不可）と、評価時刻つきの責任解決。工程ごとの責任区分（営業工程→営業、事務工程→事務処理）。担当可否・閲覧・各操作を「担当者本人＋記録したacting responsibilityが現在有効＋役割が操作を含む」で判定。`Assign` command、試行ごとの割当期間記録。合成principalを6名へ拡張（既存2名の割当ID・digest・保存JSON表記は不変）
- Repository：migration 0007（`work.organization_policies`、ledger/stagingの対象列・語彙・principal）。Work操作はpolicy行をshare lock、policy操作はupdate lock。policy操作もledger・必須stagingと同一transaction。seedはpolicyを上書きしない
- HTTP/OpenAPI：session（責任一覧・管理可否・policy revision）、units/roles、割当・委任の一覧/作成/取消、`tasks?actingAssignmentId`、`tasks/{id}/assignment`、`ORGANIZATION_RECORD_NOT_FOUND`。TS型を再生成
- Server：6 profile（8090〜8095）。新DBのDocument bootstrapで追加4名へ共有入力の閲覧のみ付与。旧2名のgrantで初期化済みのDBも受理する（追加4名は文書参照が利用不可と表示）
- GUI：ヘッダーの実行担当表示と範囲切替（URLの範囲は本人の有効責任に一致する場合だけ使用）、担当の管理（非公開本文を取得しない）と担当変更ダイアログ、「担当と委任」画面（自分の責任、委任の作成・取消、管理担当の割当追加・取消）。結果不明は同一操作IDで回復し、記録が無い場合だけ同一payloadを再送
- 受入：既存の2名journey/persistenceは変更せず、別の新DBで6 processを起動するpolicy journey/restart/persistenceをrunnerへ追加

### 検証（ローカル）

| 区分 | 結果 |
|---|---|
| Domain（work-domain/application） | 全pass。新organization試験7件。既存試験4件は意味変更に伴い期待値を更新（役割に無い操作は状態より先に403、他principalの責任・別工程は非開示404） |
| 変異確認 | 委任期限・取消判定を外すと新試験2件がRED、管理者表示の本文取得・URL範囲検証を外すとGUI試験2件がRED |
| 実PostgreSQL 16（ローカル） | `postgres_transaction` 2件pass（既存journey＋新規：同時claimで1件のみ成立、委任replay/OPERATION_CONFLICT/回復、担当変更、policy writer保持中のWork操作待機と取消後拒否）。3回反復で安定 |
| HTTP | 既存14件＋新4件pass |
| organization-server | 全pass（6 profile、bootstrap grant） |
| clippy（変更crate, -D warnings）／fmt／architecture-lint／assurance／api lint・contract／organization OpenAPI lint | pass |
| GUI全体 | 1427/1427（57 suites）、型検査、production build、organization runtime型検査 pass |
| runtime診断・設定のnode試験 | pass（policy phaseの閉じた診断を追加） |
| 実browser（ローカル、PostgreSQL 18.6公式image、Chromium 141） | 既存2名journey/restart/persistence pass。6名policy journey/restart/persistence pass |

ローカルbrowserは固定のbundled Chromiumではなくsystem Chromiumを使い、未commit sourceの確認である。資格はhosted CIの同一headで取る。

### 実装中に見つけて直したもの

- 担当変更の確定通知が一覧の再評価で消える（管理パネルをrevision付きkeyで再生成していた）。実browserで発見し、GUI回帰試験でRED→GREENを確認してから修正
- 管理者表示の「現在の担当」が領域（region）として公開されていなかった

### 新しい判断（承認済みとは扱わない）

[実装追補§8](../specs/2026-10-07-organization-multi-principal-amendment.md#8-新しい判断承認済みとは扱わない)の5点（合成「業務管理」役割、委任の作成者と委任不可の操作、自動解放なし、管理範囲は合成organization全体、principalごとの1 process）。policy理由の上限は応答サイズの都合で1024 bytes。

### Audit担当へのhandoff（共通schemaは変更していない）

Work側の必須local staging（`work.event_staging`）に次を記録する。Audit配送・保持・検証の資格は主張しない。

- 共通：`principal_id`（実principal）、`acting_assignment_id`（acting responsibility）、`operation_id`、`occurred_at`、`workflow_id`か`policy_id`のどちらか一方
- Work操作payload：`actingResponsibilityKind`（role_assignment|delegation）、`actingRoleId`、`delegationId`、`delegatorPrincipalId`。担当変更は `assigneePrincipalId`、`assigneeResponsibilityId`、`attemptId`
- policy操作payload：`resourceType`（role_assignment|delegation）、`recordId`、`subjectPrincipalId`／`delegatorPrincipalId`・`recipientPrincipalId`、`roleId`、`unitId`、`actions`、`validFrom`、`validUntil`、`policyRevision`
- 利用者が書いた理由文はstagingへ入れない（Work/policyの記録本体にだけ保持）

最終Domain/API/Auth設計の確認後、versioned extensionとして統合する前提で整理した。

### Tauri担当との接続点

この単位はRuntime Contract・native broker・local bindingに触れていない。policy由来のresource bindingはU2以降でWorkspace APIへ接続する。

### 次のexact action

1. 独立review（subagent）の指摘を処理する
2. commit・push・Draft PR作成・PR購読
3. exact-head CI（特に `document-poc-runtime` のOrganization stage 13件）を確認し、失敗は根因を直す
4. 合格後mainへ統合し、main push CIを確認
5. 同名branchを最新mainから作り直し、U2（WorkContext複数化・Attention・WorkViewProfile）へ
