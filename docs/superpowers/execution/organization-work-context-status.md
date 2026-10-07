# Organization 複数文脈・注意・表示Profile（U2）— Capability Execution Status

## 2026-10-07 — 実装・ローカル検証・独立review修正完了、PR/hosted CIへ

### 位置付け

- U1（複数担当・役割・委任、[PR #96](https://github.com/AIrisu-072/knowledge-platform/pull/96)）はmain `04076b1` へ統合済み。U2はその上に、凍結設計のWorkContext・Attention・WorkViewProfileを接続する
- 設計の具体化：[実装追補](../specs/2026-10-07-organization-work-context-attention-amendment.md)、手順：[小計画](../plans/2026-10-07-organization-work-context.md)、利用手順：[Organization Browser PoC](../../operations/organization-browser-poc.md#複数の文脈案件注意表示profileを使う)
- 既存の工程・提出・差戻・完了・保留再開・根拠/判断・合成Agent・U1の担当判定は再実装しない
- branch：`claude/trusting-knuth-dn5cx4`（main `04076b1` から作り直し）

### 実装した内容

- Domain：合成文脈fixture 3件（既存の案件Aは保存JSONまで不変）。固定のworkflow/task IDへの依存を、各instanceの計画（次工程ID・最初の試行ID・工程・WorkType）へ置換。審査工程（審査役割）。試行の明示的な期限。Attention（記録からの導出、lifecycleではない）。確認済みの検証（本人の現在の割当期間だけ）。文脈projection（表示名はowner unitの `context.read` 保有者と現在の担当者だけ、進捗・履歴は別権限）。表示Profile 3種と選択規則。policy未添付は何も許可しない（U1）
- Repository：migration 0008（確認済み記録。Work revision・ledger・stagingに入れない）。全instanceの有界な読取り（最大32）、record IDから所有instanceの特定、操作はそのinstanceの行をlockしてledger・staging・historyへ記録。Agentのprovider確認・回復もinstance単位
- HTTP/OpenAPI：`work-view-profiles`、`work-contexts`（一覧・詳細・履歴）、`tasks/{id}/attention`、`attention-seen`、task一覧の `contextId`・`workTypeId`、責任の `workViewProfileId`。生成型を再生成
- Server：明示コマンド `seed-contexts`（`seed-work` は既存どおり単一文脈）
- GUI：既定Profile、営業型の文脈一覧・概要・履歴、事務型のWorkType別キュー、注意の文字表示と「確認済みにする」、審査Profileの初期module「根拠」
- 受入：6名policyの後、同じDBで `seed-contexts`（再実行で無変化）→ 文脈journey → 再起動 → 文脈persistence

### 独立review（NO-GO → 修正）

審査を含む文脈が差戻し後に停止する不具合と、注意APIの閲覧範囲が一覧より広い点を含む次を修正した（修正commit `3a309f3`）。

| 指摘 | 修正 |
|---|---|
| 審査定義（案件B）で差戻し後の再提出が拒否され、文脈が停止する | 再提出の可否を「差戻し可能な定義」で判定。差戻し→再提出→新しい審査試行→完了の試験 |
| 再提出で作られた次工程の試行にも「差戻し」の注意が出る | 差戻しで作られた試行だけに出す。対象試行の試験 |
| 注意APIの閲覧範囲が一覧より広い | 一覧の行（文脈・キューどちらか）に出る場合だけ返す |
| 確認済み記録の読取りが行lock中 | instance単位でlock前に読む。中断sweepも文脈数の上限で閉じる |
| 表示指定があるとProfileを読み込まず初期moduleが効かない | 常に読み込み、初期moduleを適用 |
| 一覧の並び | 期限の早い順 |
| decoder・境界の試験不足 | 期限境界・owner unit以外の文脈閲覧・decoderの試験を追加 |

### 独立再review（GO）と軽微事項の対応

同じreviewerの再reviewで、上記6件の修正はすべて正しいと確認された（GO、Critical/Importantなし）。軽微事項のうち開示に関わるものと小さなものはこのPRで直した。

| 軽微事項 | 対応 |
|---|---|
| 完了した試行の担当者に文脈の表示名・文脈一覧が残る | 未完了の試行の担当者だけに限定（追補§4・§9.6） |
| 担当可能なだけの行・管理担当の行に不透明な `contextId` が載り、文脈ごとにまとめられる | 表示名と同じ相手だけに開示し、それ以外は `null`（OpenAPI・decoder・GUIを追従） |
| 再照会の受領内容に確定時の文脈表示名が残る | 受入済み2名journeyが受領内容の一致を前提にしているため変更せず、新しい判断として明記（追補§9.10）。対象記録の現在の閲覧可否は従来どおり確認する |
| instance別lockで同じ操作IDが別文脈へ同時に送られると503になる | ledgerの一意制約違反を `OPERATION_CONFLICT` へ対応付け |
| 文脈の注意件数が選択中の責任を無視する | 一覧の行と同じ責任の範囲で数える |
| 一覧の既定件数50とclientの未指定 | clientが `limit=100` を明示（追補§9.11） |
| 表示指定時のProfile・期限順の試験が無い／文脈の無い行が未整列／古いfixture | 試験を追加（修正を外すとRED）、文脈の無い行も期限順、fixtureを現行の規則へ更新 |

### 検証（軽微事項の修正後 `d14fa76`、ローカル）

| 区分 | 結果 |
|---|---|
| Domain・HTTP・server・application | 全pass（文脈試験11件。新規1件は3つの修正をそれぞれ外すとRED） |
| 実PostgreSQL 16 | 3件pass |
| GUI | 1563/1563（65 suites）、型検査。1回目の全体実行で1件失敗（対象は記録できず）、その後の全体3回はすべてpass、新規・変更した2 filesは単独6回すべてpass |
| fmt・clippy（変更crate、`-D warnings`）・organization OpenAPI lint | pass |
| 実browser（ローカル） | 17 stageすべてpassed（受入済み2名journeyは不変のまま成功） |

### hosted CI

- 修正前head `8df98de`：全job成功（`document-poc-runtime` のOrganization受入stepを含む）
- 修正後headはこのpushで実行する

### 検証（ローカル、修正後 `3a309f3`）

| 区分 | 結果 |
|---|---|
| Domain | 全pass。文脈試験10件（fixture・審査工程・差戻し→再提出→完了・Attention・差戻し注意の対象・一覧と同じ閲覧範囲・期限境界・開示・Profile・所有instance） |
| HTTP | 全pass。新しい試験3件（Profile/session、文脈の開示とfilter、確認済みの冪等性） |
| 実PostgreSQL 16 | 3件pass（既存2件＋新規：instanceごとのledger・revision、受理の回復、審査queue、確認済みがWork mutationでないこと、再seedで無変化） |
| GUI | 全pass。新しい試験5件＋decoder。変異確認：既定Profileを外すと2件、確認済みボタンを外すと1件がRED |
| runner node試験 | pass |
| 実browser（ローカル、PostgreSQL 18.6公式image、system Chromium） | 17 stageすべてpassed（既存2名journey/restart/persistence、6名policy journey/restart/persistence、context-seed／context-journey／context-restart／context-persistence、cleanupでowned container削除） |

ローカルbrowserは固定のbundled Chromiumではなくsystem Chromiumを使った確認であり、資格はhosted CIの同一headで取る。

### 新しい判断（承認済みとは扱わない）

[実装追補§9](../specs/2026-10-07-organization-work-context-attention-amendment.md#9-新しい判断承認済みとは扱わない)。

### Audit担当へのhandoff（共通schemaは変更していない）

- 確認済み（`attention-seen`）はWork mutationではないため、Work revision・ledger・`work.event_staging` に記録しない。必要ならAudit側で別の観測種別として扱う判断が要る
- 文脈ごとのWork操作のstagingは既存の形式のまま（`workflow_id` が文脈ごとのinstanceを示す）

### main push CI（U1統合 `04076b1`）

- run `37578317667`：結果は次回更新で記録（作成時点で実行中）

### 次のexact action

1. branchをpushし、U2のPRを作成（Draft）、PR activityを購読
2. exact-head CIを確認し、失敗は根因を直す。独立reviewで修正の再確認
3. 合格後mainへ統合し、main push CIを確認
4. 同名branchを最新mainから作り直し、U3（個人作業ファイル・共有provider・Handoff Snapshot・差戻し後の作業）へ
