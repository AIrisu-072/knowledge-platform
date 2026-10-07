# Organization Client v0 — WorkContext・Attention・WorkViewProfileの実装追補（U2）

状態：実装前の追補。凍結済みの[Product/UX](2026-10-02-organization-client-v0-product-ux-design.md) §4・§5・§7、
[Domain/API/Auth](2026-10-02-organization-client-v0-domain-api-design.md) §3・§10・§13、
[UI](2026-10-02-organization-client-v0-ui-design.md) §3・§4・§6 の意味を変えず、U1（[複数担当の追補](2026-10-07-organization-multi-principal-amendment.md)）の
上に「複数の文脈（案件）」「注意（Attention）」「営業型／事務型の表示Profile」を接続する最小の具体化だけを記録する。
新しい業務判断は §9 に分けて明示し、承認済みとは扱わない。

## 1. 基点と範囲

- 基点：U1統合後のmain
- 既存の工程（提出・差戻・完了・保留再開・根拠/候補/人間判断・合成Agent・文書参照）と担当判定（U1）は再実装しない
- 本単位で変えるのは次の3点だけ
  1. 単一の固定workflow（`WORKFLOW_ID`）を、合成fixtureで定義した複数のWorkflowInstance（各1つのWorkContext）へ一般化
  2. Attention projection（派生。lifecycleではない）と、本人の割当に対する確認済み記録
  3. WorkViewProfile（営業型＝context、事務型＝queue、審査＝queueのProfile）と、それに沿った一覧・詳細画面
- 添付（U3）、Agent Chat（U4）、Workspace/ResourceBinding（Tauri担当）は含めない

## 2. 合成WorkContextとWorkflowInstance（§3の具体化）

文脈と工程の生成はv0では版付きfixture/configであり、作成APIや汎用workflow designerは作らない（Domain §13）。

| 文脈 | kind | 表示名（合成） | 定義 | 初期状態 |
|---|---|---|---|---|
| C1（既存 `CONTEXT_ID`） | case | 合成案件A・設備更新相談 | 営業内容整理 → 事務内容確認（既存定義） | 営業工程 active、sales-01（既存のまま） |
| C2 | case | 合成案件B・運転資金相談 | 営業内容整理 → 審査内容確認（新定義） | 営業工程 ready・未割当、期限あり |
| C3 | request | 合成依頼C・住所変更届 | 営業内容整理 → 事務内容確認（既存定義と同じ工程） | 営業工程 ready・未割当、期限あり |

- 既存C1のID・定義・保存JSONと `seed-work` は変えない。C2・C3は明示コマンド `seed-contexts` が `ON CONFLICT DO NOTHING` で追加する（既存DBにも追加できる。既存の進捗は変えない）。受入済みの2名journeyは単一文脈のDBのまま実行する
- 各instanceは2工程（営業→次工程）で、次工程のtask IDと最初の試行IDはfixtureで固定する
- 審査内容確認の責任区分は「審査」役割。審査は事務型（queue）のProfileで表示する。差戻しはworkflow状態であり別画面を作らない
- WorkContextは `owner unit`（合成fixtureでは全文脈が「営業店」）を持つ。文脈そのものの読取は §4 の規則に従う

## 3. 期限（dueAt）

- 期限は試行（WorkAttempt）の明示的な `dueAt` だけを使う。fixtureはC2・C3の最初の営業試行に、seed時刻からの相対で期限を与える（C2：6時間後、C3：1時間前）
- 提出・差戻しで作られる試行には期限を自動で付けない（期限の業務規則は未定義のため作らない）

## 4. 認可と開示（U1の判定の上に追加する規則）

| 対象 | 開示する相手 | 開示しない相手 |
|---|---|---|
| 文脈の一覧・識別（表示名・kind・不透明な文脈ID） | 文脈のowner unitで `context.read` を持つ現在有効な責任の保有者、またはその文脈の未完了の試行の担当者本人 | 担当可能なだけの利用者・管理担当・完了した試行だけの担当者（queueの行には汎用の工程名・状態・期限・注意だけで、`contextId` も `null`） |
| 文脈の進捗（工程ごとの状態・試行番号・期限・割当済みか） | owner unitで `context.progress.read` を持つ責任 | 上記以外。進捗に担当者の識別・理由・本文・根拠は含めない |
| 文脈の履歴（業務状態の推移） | owner unitで `context.history.read` を持つ責任 | 上記以外。理由・本文・識別は含めない |
| Attention | 一覧の行を見られる利用者（行と同じ範囲）。ただし `newly_assigned` は担当者本人だけ。出典ID（差戻指示ID・割当期間ID）は担当者本人だけ | — |
| 確認済み記録 | 担当者本人が自分の現在の割当期間に対してだけ作成 | 管理担当・他人 |

- 文脈・Attention・Profileは認可を広げない。Search projection・UI非表示で代替しない
- WorkContext×WorkItemは同じ記録で、context表示とqueue表示は同じIDの別projectionである

## 5. Attention（Domain §10の具体化）

| 種類 | 根拠となる記録 | 成立条件 |
|---|---|---|
| `newly_assigned` | 現在の試行の割当期間（WorkAssignmentRecord） | 他人（管理担当）が割り当て、担当者本人がまだ確認していない |
| `returned` | 現在の試行のReturnInstruction | 現在の試行が差戻しで作られ、未完了 |
| `due_soon` | 試行の `dueAt` とWorkTypeの事前通知幅 | 未完了で、`dueAt − 通知幅 ≤ 現在 < dueAt` |
| `overdue` | 試行の `dueAt` | 未完了で、`現在 ≥ dueAt` |

- `waiting_for_confirmation` と `blocked` は、v0に根拠となる記録（確認依頼・前提不足の記録）が無いため出さない（推測しない）
- 複数のAttentionは同時に成立しうる。確認済みにしても作業は完了しない。分析・人事評価の目的は持たない
- 確認済み記録はWork集約のrevisionを進めない別記録（`work.attention_acknowledgements`）。同じ割当期間への再確認は冪等

## 6. WorkViewProfile（Product §5の具体化）

| Profile | archetype | 一次grouping | 既定の並び | 文脈moduleの優先 |
|---|---|---|---|---|
| 営業・文脈（sales-context） | context | 文脈 | 期限 | 履歴・文書をprominent、根拠available |
| 事務・キュー（office-queue） | queue | WorkType | 期限 | 文書をvisible、根拠available |
| 審査・キュー（review-queue） | queue | WorkType | 期限 | 根拠・文書をprominent |

- 選択：役割の上書き（審査→review-queue）があればそれ、無ければ組織単位の既定（営業店→sales-context、事務・承認→office-queue、融資審査→review-queue）
- Profileは表示だけを変え、データの権限・操作を変えない。利用者は既存の「営業型・文脈／事務型・キュー」切替で別projectionを見られる（権限は同じ）

## 7. API（§13のうち本単位で実装する範囲）

| Method / path（`/v1/organization`） | 認可 | 画面 |
|---|---|---|
| GET `/session` | 既存。各責任に `workViewProfileId` を追加 | 既定の表示Profile |
| GET `/work-view-profiles` | 全principal（定義のみ） | 表示Profile |
| GET `/tasks?view=&actingAssignmentId=&contextId=&workTypeId=` | 既存（U1）。各行に `workTypeId`・`workTypeLabel`・`dueAt`・`attention[]`・`contextTitle`（§4で開示できる場合だけ） | 一覧 |
| GET `/tasks/{id}/attention` | 行が見える利用者 | 注意の詳細 |
| POST `/tasks/{id}/attention-seen` | 担当者本人の現在の割当期間 | 「確認済みにする」 |
| GET `/work-contexts?actingAssignmentId=` | §4 | 営業型の文脈一覧 |
| GET `/work-contexts/{id}` | §4（進捗は別条件） | 文脈の概要・進捗 |
| GET `/work-contexts/{id}/history` | §4 | 文脈の履歴 |

- 文脈が見えない場合は存在を明かさず404（`WORK_CONTEXT_NOT_FOUND`）
- 一覧は合成fixtureの範囲（文脈は最大32、各2工程）なので既存の上限（100件）内に収まる。cursorは使わない

## 8. 画面

- Primary Navigationは「タスク／文書／検索」のまま、Landingはタスク
- 営業型（context）：左の一覧に文脈（表示名・kind・注意件数）。選ぶと主作業に文脈の概要・工程の進捗・自分の次の作業を表示し、関連タスクを選ぶと既存の作業面を開く
- 事務型（queue）：左の一覧で先にWorkTypeを選び、そのWorkTypeの行を「自分の担当／引受可能」で分けて表示。行には工程名・状態・期限・注意だけを出す（担当可能なだけの行に顧客名＝文脈表示名を出さない）
- 審査Profileは文脈moduleの初期表示を「根拠」にする
- 注意は文字のラベル（新しい割当／差戻し／期限間近／期限超過）と期限の日時で表し、色だけに頼らない。`newly_assigned` には「確認済みにする」
- 選択は安定IDで保持し、再読込・並び替え・他者の引受で別の対象へ自動で切り替えない

## 9. 新しい判断（承認済みとは扱わない）

1. 合成文脈C2・C3の表示名・kind・owner unit・工程構成、期限をseed時刻からの相対で与えること
2. 合成WorkTypeの事前通知幅は24時間（全WorkType共通の合成値。実業務の規則ではない）
3. Profileの選択規則（役割の上書き：審査、組織単位の既定：営業店/事務/承認/融資審査）
4. 確認済み記録はWork操作ではない（operation ledger・event stagingに入れない）。冪等なupsert
5. 文脈の表示名は顧客名に相当しうるため、担当可能なだけの利用者には開示しない
6. 文脈の一覧・識別は、owner unitの `context.read` 保有者に加えて、その文脈の未完了の試行の担当者本人にも開示する。試行が完了（提出・差戻・完了）した後は、その担当者の行・文脈一覧から表示名と文脈IDを外す
7. 文脈の一覧・行の取得は、PoCの有界な件数（最大32文脈）を前提に全instanceを読む実装とする（本番の索引設計ではない）
8. 事務型の一覧はWorkTypeを選ぶ前に「すべての種類」を明示の選択肢として表示する（WorkTypeが1種類だけなら選択欄を出さない）。並びは期限の早い順（期限の無い行はserverの順序のまま）
9. 差戻しで作られた試行だけを「差戻し」の注意とする。差戻し後の再提出で作られる次工程の試行は、比較のため差戻指示を参照しても「差戻し」とはしない
10. 操作結果の再照会（同じ操作IDのreplay・`/operations/{id}`）は、確定時の受領内容をそのまま返す。再照会のたびに対象記録の現在の閲覧可否は確認するが、受領内容に含まれる文脈の表示名・IDは確定時の値であり、現在の開示規則で書き換えない（受入済みの2名journeyが受領内容の一致を前提にしているため。現在の開示に合わせて削る方式は、受入条件の変更を伴うので別途判断する）
11. 担当一覧の取得は1回100件まで（clientは `limit=100` を明示）。PoCの上限32文脈×2工程に収まる
