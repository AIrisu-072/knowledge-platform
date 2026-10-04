# DocumentとOrganizationの統合計画

> 実行担当は既存sourceを保全し、この計画を順に実行する。新機能の設計・実装は含めない。

**目的:** 受入済みDocument/Organizationと文書側枝をmainへ統合できる一つのDraft候補にし、正確なcandidateの全CIを確認する。

**構成:** main `d71753d4` の子孫であるPR58 `6118a690` を基点とする。PR43→47→48→49→54→56→57の祖先を維持し、PR36最新、PR39（PR38を包含）、PR46、PR55をmerge parentで保全する。既存branch/mainのrefは移動しない。

**技術:** 既存Git履歴、Rust、React/TypeScript、PostgreSQL、既存CI。製品source・dependency lock・migration・workflowはPR57 `d383bacc` とbyte一致を必須にする。

**規範:** `AGENTS.md`、既存Document GUI計画G9、受入PR43 exact `6103e4d4` と報告PR46 `88628331`、Organization凍結設計/承認記録。過去の受入対象SHAを別対象へ読み替えない。

## 制約と重点レビュー

- 旧PR36単独の未受入記録と、後続のaccepted実runtimeを混同しない。統合candidateのexact CIは別証拠として扱う
- 凍結design/approvalのblobと旧checkpointを保持し、Activeと現在statusにだけ最新の統合事実を追記する
- PR38/39/46/55の文書を落とさず、競合した履歴は意味を変えず統合する
- SearchのDocument migration version9衝突、未資格Tauri PR52/53、Audit、進行中Agent実装を取り込まない
- main mergeはCIを起動するがdeployしない。画像captureの限定条件・permissionsは変えない。対象PCでの導入/backup/restore未実行を明示する

## 作業

### 1 祖先と文書側枝を統合

- [x] mainと対象PRのlive head/baseを確認する
- [x] 独立worktreeでPR58を基点に、PR36・39・46・55の順にmergeする
- [x] 非文書の差分が発生した場合は、accepted sourceとの内容同一性を確認し、意味変更が必要なら停止する
- [x] 全対象headの祖先包含と、PR57に対する非文書blob不変を検証する

### 2 現在状態を正確に記録

- [x] `docs/superpowers/execution/active.md` と `document-gui-integration-v0-status.md` に受入済み対象・統合candidate・未完gateの違いを追記する
- [x] `docs/superpowers/execution/document-organization-integration-status.md` にmerge parent、競合判断、検証結果、次の操作を保存する
- [x] 既存日本語訳が独立した参照文書として存在すれば、凍結原本を変更せず必要な参照だけ保全する。ない場合は作業を止めない

### 3 検証と公開

- [x] 祖先、保護対象blob、相対リンク、diff、repository policyを確認する
- [x] 既存の対象GUI/純粋helper/type/schema/build検査を固定toolで実行する。禁止されたlocal DB/listener/browserは再実行しない
- [x] 別担当の統合保持レビューを受ける。Critical/Important指摘なし、検証結果の文書更新のみを反映する
- [ ] 日本語Draftをmain baseへ公開し、全適用workflowとrequired-checkの終端を確認する
- [ ] exact head/tree、差分、CI、main merge時の副作用を親へ渡す。main実mergeと実本番deployはここでは行わない

G9の統合経路合格はcandidateの実browser/backendを含むexact CI成功を条件とする。PR36旧headへ新しい試験結果を遡及して付け替えない。
