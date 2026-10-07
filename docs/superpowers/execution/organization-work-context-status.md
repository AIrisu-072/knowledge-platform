# Organization 複数文脈・注意・表示Profile（U2）— Capability Execution Status

## 2026-10-07 — 実装・ローカル検証中

### 位置付け

- U1（複数担当・役割・委任、[PR #96](https://github.com/AIrisu-072/knowledge-platform/pull/96)）の上に、凍結設計のWorkContext・Attention・WorkViewProfileを接続する
- 設計の具体化：[実装追補](../specs/2026-10-07-organization-work-context-attention-amendment.md)、手順：[小計画](../plans/2026-10-07-organization-work-context.md)、利用手順：[Organization Browser PoC](../../operations/organization-browser-poc.md#複数の文脈案件注意表示profileを使う)
- 既存の工程・提出・差戻・完了・保留再開・根拠/判断・合成Agent・U1の担当判定は再実装しない

### 実装した内容

- Domain：合成文脈fixture 3件（既存の案件Aは保存JSONまで不変）。固定のworkflow/task IDへの依存を、各instanceの計画（次工程ID・最初の試行ID・工程・WorkType）へ置換。審査工程（審査役割）。試行の明示的な期限。Attention（記録からの導出、lifecycleではない）。確認済みの検証（本人の現在の割当期間だけ）。文脈projection（表示名はowner unitの `context.read` 保有者と現在の担当者だけ、進捗・履歴は別権限）。表示Profile 3種と選択規則。policy未添付は何も許可しない（U1）
- Repository：migration 0008（確認済み記録。Work revision・ledger・stagingに入れない）。全instanceの有界な読取り（最大32）、record IDから所有instanceの特定、操作はそのinstanceの行をlockしてledger・staging・historyへ記録。Agentのprovider確認・回復もinstance単位
- HTTP/OpenAPI：`work-view-profiles`、`work-contexts`（一覧・詳細・履歴）、`tasks/{id}/attention`、`attention-seen`、task一覧の `contextId`・`workTypeId`、責任の `workViewProfileId`。生成型を再生成
- Server：明示コマンド `seed-contexts`（`seed-work` は既存どおり単一文脈）
- GUI：既定Profile、営業型の文脈一覧・概要・履歴、事務型のWorkType別キュー、注意の文字表示と「確認済みにする」、審査Profileの初期module「根拠」
- 受入：6名policyの後、同じDBで `seed-contexts`（再実行で無変化）→ 文脈journey → 再起動 → 文脈persistence

### 検証（ローカル）

| 区分 | 結果 |
|---|---|
| Domain | 全pass。新しい文脈試験6件（fixture・審査工程・Attention・開示・Profile・所有instance） |
| HTTP | 全pass。新しい試験3件（Profile/session、文脈の開示とfilter、確認済みの冪等性） |
| 実PostgreSQL 16 | 3件pass（既存2件＋新規：instanceごとのledger・revision、受理の回復、審査queue、確認済みがWork mutationでないこと、再seedで無変化） |
| GUI | 全pass。新しい試験5件。変異確認：既定Profileを外すと2件、確認済みボタンを外すと1件がRED |
| runner node試験 | 29件pass |

### 新しい判断（承認済みとは扱わない）

[実装追補§9](../specs/2026-10-07-organization-work-context-attention-amendment.md#9-新しい判断承認済みとは扱わない)の7点。

### 次のexact action

1. 実browser受入（ローカル、17 stage）の結果確認
2. 独立review
3. U1統合後に同名branchを最新mainから作り直し、U2 commitを移してPR作成・exact-head CI
