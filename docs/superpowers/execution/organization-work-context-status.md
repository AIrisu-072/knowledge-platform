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
