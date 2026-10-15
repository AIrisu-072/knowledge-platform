# Organization Client v0 — 複数担当者・役割・委任の実装追補（U1）

状態：実装前の追補。凍結済みの[Product/UX](2026-10-02-organization-client-v0-product-ux-design.md)、
[Domain/API/Auth](2026-10-02-organization-client-v0-domain-api-design.md)、
[UI](2026-10-02-organization-client-v0-ui-design.md) の意味を変えず、§5・§6・§13・§16 を
既存の固定2名実装へ接続するための最小の具体化だけを記録する。新しい業務判断は §8 に分けて明示し、
承認済みとは扱わない。

## 1. 基点と範囲

- 基点：main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（PR43受入済み `6103e4d4` を祖先に含む）
- 既存の固定2工程（営業内容整理 → 事務内容確認）、private文案、提出・差戻・完了・保留再開、
  Evidence/Finding/HumanDecision、合成Agent、Document参照は再実装しない
- 本単位で置き換えるのは「担当可否の固定判定」だけである。`sales-01 → 営業工程`、
  `office-01 → 事務工程` というcode内の対応を、Organization policyと工程の責任区分による判定へ移す
- WorkContextの複数化・Attention・WorkViewProfile（U2）、添付（U3）、Agent Chat（U4）は含めない

## 2. 合成Identity（§16の具体化）

| principal | 主な正式割当（役割@組織単位） | 用途 |
|---|---|---|
| sales-01 | 営業@営業店 | 既存。文脈継続・営業工程 |
| office-01 | 事務処理@事務 | 既存。事務工程 |
| review-01 | 審査@融資審査 | 審査queueの利用者（U2以降の工程で使用） |
| approver-01 | 承認@承認、業務管理@承認 | 担当変更・割当/委任の管理 |
| multi-role-01 | 事務処理@事務、審査@融資審査 | 兼務・同時claim |
| delegate-01 | なし | 期限付き委任の受任者 |

- principalは起動時の固定profileだけで決まる（既存と同じ）。header/body/query/toolでは変更できない
- `agent-01` はHuman profileではない。既存の合成Agent識別を変更しない
- 既存2名の割当ID（`SALES_ASSIGNMENT_ID` / `OFFICE_ASSIGNMENT_ID`）、操作digest、保存済みJSONの表記を保持する

## 3. Organization policyのモデル

- `OrganizationalUnit { id, label, defaultArchetype(context|queue), roleIds[] }`：候補pool・既定表示・利用可能役割。直接のTask認可ではない
- `BusinessRole { id, key, label, actions[] }`：§5の操作名だけを持つ
- `RoleAssignment { id, principal, roleId, unitId, validFrom, validUntil?, revokedAt?, reason, createdBy? }`
  - `validUntil` は排他的終端、nullは終端予定なし。逆転は拒否。取消は `revokedAt` を記録し、過去の行為を書き換えない
- `Delegation { id, sourceAssignmentId, delegator, recipient, actions[], validFrom, validUntil, reason, revokedAt? }`
  - 委任元の正式割当より広い操作、委任元より長い期限、再委任を拒否する
  - 委任元の取消・期限切れ、委任自体の取消・期限切れで、次の認可判定から即時に無効
- policyはWork集約とは別の集約（同じ `work` schemaの別table、別revision）として保存する
- 有効性はserverのUTC時刻で評価する。Work操作はpolicy行をshare lockしてから業務行をupdate lockし、
  commit時の時刻で再評価する。policy更新はpolicy行をupdate lockする（§14のfencing）

## 4. 工程と責任区分

| 工程 | 必要な役割（ResponsibilitySegment） |
|---|---|
| 営業内容整理 | 営業 |
| 事務内容確認 | 事務処理 |

`WorkAssignment` は試行ごとに principal と acting responsibility（正式割当または委任のID）を記録する。
以後のその試行への操作は、記録したacting responsibilityが現在も有効で、工程の役割と操作を含む場合だけ許可する。

## 5. 認可（§6の順序を既存実装へ適用）

1. 起動時固定のprincipal
2. 要求された `actingAssignmentId` が本人の現在有効な正式割当・委任であること
3. 工程の役割と操作（例：`work.claim`、`work.edit`、`work.submit`）を含むこと
4. 割当済み試行は担当者本人かつ記録したacting responsibilityであること
5. 既存のartifact visibility・Document provider現在認可（Evidence/Agent/Document）

- 担当変更（`work.assign`）は管理担当のacting responsibilityが必要。工程の役割は不要だが、
  新担当者のresponsibilityは工程の役割と `work.claim` を含み現在有効であること
- 職務分離：管理担当は自分自身への担当変更・自分への正式割当の作成をできない（403）。
  自分が工程の役割を持つ場合は通常の引受を使う。委任は委任元の本人だけが作成し、管理担当が他人の名義で作成しない
- 差戻し後の新しい試行の担当者（委任・担当変更による担当者を含む）は、その試行の差戻指示と差戻し前の提出を読める。
  提出者本人は、その工程の責任を保持している間だけ自分の提出を読める（既存規則）。担当可能なだけの利用者・管理担当・
  文脈継続の閲覧者の一覧には提出ID・差戻指示IDを含めない
- 担当変更は旧割当を終了し、新割当を作り、試行のrevisionを進める。同じ試行のprivate文案は新担当者が読める。
  旧担当者は次の読取から非公開内容を読めない。過去の操作者記録は残す
- 割当の取消・期限切れで担当が無効になっても試行を自動で手放さない（自動再割当・自動claimは作らない）。
  管理担当の一覧に「担当の責任が終了」と表示し、担当変更で解消する
- 同時claimは同じrevisionに対して1件だけ成功し、他方は `WORK_ASSIGNMENT_CONFLICT` または `REVISION_CONFLICT`
- 一覧の最小projection：担当可能（eligible）なだけの利用者には汎用の工程名・状態・claim可否だけを返す。
  担当者識別は本人と管理担当にだけ返す
- 一覧件数・状態からprivate本文は推測できない（既存projectionを維持）

## 6. API（§13のうち本単位で実装する範囲）

| Method / path（`/v1/organization`） | 認可 | 画面 |
|---|---|---|
| GET `/session` | 本人。現在有効な責任の一覧を追加。既存 `actingAssignmentId` は既定値として維持 | ヘッダーの実行担当切替 |
| GET `/units`、GET `/roles` | 合成organizationの全principal（ラベルのみ） | 担当・委任画面 |
| GET `/role-assignments` | 本人分。現在有効な `organization.manage` があれば全件。状態判定用のserver評価時刻 `evaluatedAt` を返す | 担当・委任画面 |
| POST `/role-assignments` | acting=管理担当の正式割当 | 管理：割当の追加 |
| POST `/role-assignments/{id}/revoke` | 同上。現在使用中のacting割当自身は取消不可 | 管理：割当の取消 |
| GET `/delegations` | 委任者・受任者本人。管理担当は全件。`evaluatedAt` を返す | 担当・委任画面 |
| POST `/delegations` | acting=委任元の正式割当（本人）だけ | 自分の委任の作成 |
| POST `/delegations/{id}/revoke` | 委任者本人または管理担当 | 委任の取消 |
| GET `/tasks?view=&actingAssignmentId=` | 指定responsibilityで見える範囲。省略時は全有効responsibilityの和 | 一覧 |
| POST `/tasks/{id}/assignment` | `work.assign` | 担当変更ダイアログ |
| GET `/operations/{id}` | 既存。policy操作も同じ操作IDで回復 | 結果不明時の確認 |

理由（割当・委任・取消・担当変更）は空白のみ不可、UTF-8で1024 bytes以内、改行・タブ以外の制御文字を拒否する。
`validFrom` はserver時刻より5分を超えて過去を指定できず、過去側の指定は現在時刻から開始する（履歴を遡及しない）。
記録は削除しないため、件数上限は取消・期限切れを含めて数える（正式割当は全体96件、委任は委任者ごとに16件）。

policy操作は既存の `operationId` / `expectedRevision`（policyのrevision）/ `actingAssignmentId` を使い、
同一digestの再送は同じ結果、異なるdigestは `OPERATION_CONFLICT`、revision不一致は `REVISION_CONFLICT` とする。
policy操作と担当変更はevent stagingへ実principal・acting responsibility・委任IDを同じtransactionで記録する
（Audit配送の資格は主張しない。Audit担当へのhandoffは状況記録に分ける）。

## 7. 画面

- Primary Navigationは「タスク／文書／検索」のまま。担当・委任画面はヘッダーの担当表示からの導線で開き、主ナビゲーションへ追加しない
- ヘッダー：実principalと実行中の担当（役割@組織単位、委任の場合は委任元と期限）を分けて表示し、兼務者は切り替えられる
- 一覧：選択した担当の範囲を表示する。切替時は旧担当の非公開cacheを破棄する
- 作業面：担当者本人には記録されたacting responsibilityを表示し、その責任で操作する
- 操作面：管理担当には「担当変更」を表示する。候補は工程の役割を持つ現在有効な割当・委任。理由必須。確定はserver成功後のみ
- 担当・委任画面：自分の責任一覧、自分の委任の作成・取消、管理担当の割当追加・取消と全委任の取消
- 認可失効後は既存の非開示処理（内容を隠して理由を表示）を使う

## 8. 新しい判断（承認済みとは扱わない）

1. 合成fixtureに「業務管理」役割（`organization.manage`、`work.assign`）を追加し、approver-01へ割り当てる。
   設計の「汎用administratorなし、現在の管理責任のみ」を満たす合成policyであり、実組織の管理権限規則ではない
2. 委任は委任元の正式割当の本人だけが作成できる（管理担当による代理作成なし）。`organization.manage` と `work.assign` は委任できない。再委任は不可
3. 割当取消・委任失効による担当無効時は自動で解放せず、管理担当の担当変更で解消する
4. 管理担当の範囲は合成organization全体（組織単位による管理範囲の分割はしない）
5. 複数principalのbrowser利用は「principalごとに1 server process」の既存方式を維持する（profile追加のみ）
6. 職務分離：管理担当は自分への正式割当の作成と自分への担当変更をできない
7. 件数・理由の上限（正式割当96件、委任は委任者ごと16件、理由1024 bytesで制御文字不可、担当期間は試行ごと16件）。
   記録の保持期間・削除方針は未決定（PoCでは削除しない）
