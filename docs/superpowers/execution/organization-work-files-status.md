# Organization 作業ファイル・共有provider・Handoff・差戻し後の作業（U3）— Capability Execution Status

## 2026-10-07 — 実装・ローカル検証中

### 位置付け

- U2（複数文脈・注意・表示Profile、[PR #101](https://github.com/AIrisu-072/knowledge-platform/pull/101)）の上に、凍結設計のWorkingArtifact（ファイル）・Work所有の共有保存領域・Handoff Snapshotのファイル固定・差戻し後の新しい作業を接続する
- 設計の具体化：[実装追補](../specs/2026-10-07-organization-work-files-handoff-amendment.md)、手順：[小計画](../plans/2026-10-07-organization-work-files.md)、利用手順：[Organization Browser PoC](../../operations/organization-browser-poc.md#作業ファイルを添付して提出し差戻し後にやり直す)
- 既存の工程・提出・差戻・完了・保留再開・根拠/判断・合成Agent・U1の担当判定・U2の文脈と注意は再実装しない
- 作業branch：ローカル `u3-files`（U2統合後に `claude/trusting-knuth-dn5cx4` を最新mainから作り直して移す。U2統合前にDraft PRを重ねない）

### 実装した内容

- Domain（`work-domain`）：`WorkFile`・不変の `FileGeneration`（IDは内容登録の操作ID、大きさ・SHA-256はserverが計算）・`DerivedFrom`。commandは作成・内容登録・外す・前回提出の取込み（すべて現在の試行の担当者の `work.edit`）。提出はファイルの世代とserverの受領確認（非永続）を要求し、`PinnedArtifact` に世代を固定。受領者・担当者の取得は既存のsnapshot・成果物の閲覧規則。文案の保存JSONは不変
- Application：`WorkArtifactStore` port（put・read・verify）、`content_identity`、`WorkRepository` の内容登録・取得・store有無
- Repository：store注入。内容登録は認可→preview→保存→command（同じ操作IDの再送はstoreへ書かない）。提出はpreview後・lock前に選択世代をstoreで確認し、lock中に受領確認を添付。取得は認可→読取り・照合→再認可。migration 0009（stagingのaction語彙）。stagingに名前・本文は入れない
- Server：既存の `FileSystemStorage` を保存rootの下の `work-artifacts/` で使うadapter（既存世代の一致確認、読取り時の大きさ・hash照合、8 MiB上限）。sessionの `fileUpload` はstoreがあるときだけtrue
- HTTP/OpenAPI：ファイル作成（`file`）、`PUT/GET /working-artifacts/{id}/content`（8 MiB、headerで操作ID等、添付応答・`sandbox`）、`discard`、`import`、`/handoff-snapshots/{id}/artifacts/{artifactId}/content`、`WORK_ARTIFACT_UNAVAILABLE`（503）。生成型を再生成
- GUI：作業ファイル欄（追加・内容登録・取得・外す、8 MiB・名前の事前確認）、提出確認と受領・差戻前のスナップショットのファイル表示と取得（添付として保存、画面に内容を表示しない）、差戻し後の「前回の提出内容を取り込む」。保存領域の拒否は確定した失敗として扱う（結果不明にしない）
- 受入：文脈persistenceの後、同じDBで files-journey → 6 process再起動 → files-persistence

### 検証（ローカル）

| 区分 | 結果 |
|---|---|
| Domain | 全pass。作業ファイル試験5件（変異確認：受領確認・名前規則・重複取込み・text保存のschema・世代ID・取込み対象の各ガードを外すとRED） |
| Application | 全pass（content identityの試験） |
| 実PostgreSQL 16 | 4件pass（新規：store呼出しがlock外、再送でstoreへ書かない、異なるbytesの同一操作IDはOPERATION_CONFLICT、古いrevision・非担当者はbytes保存前に拒否、受領確認なしで提出しない、改ざん後は取得不可、stagingにファイル名なし）。受領確認を常に成功させる変異でRED |
| Storage adapter | pass（世代の不変・一致時の冪等・Documentの領域と分離・欠落/不一致で取得不可・8 MiB境界） |
| HTTP | 全pass（新規3件：8 MiB境界と1 byte超過・header・media type、添付応答header、503と404、作成/外す/取込みのcommand） |
| GUI | 1572/1572（66 suites）、型検査、organization runtime型検査。新規：画面5件・client 4件（変異確認：未登録ファイルでの提出不可・保存領域拒否の確定失敗扱い・固定ファイルの世代必須・取込みボタンの条件を外すとRED） |
| runner node試験・API contract・OpenAPI lint | pass |
| 実browser（ローカル、PostgreSQL 18.6公式image、system Chromium） | 20 stageすべてpassed（既存2名・6名policy・文脈の各stageに加え files-journey／files-restart／files-persistence、cleanup完了） |

### 実装中に見つけて直したもの

- 内容登録を作成の直後に続けて開始すると、結果の照合が前のrenderの操作（作成）を参照して「応答と操作が一致しません」になる。照合を各requestに束縛して修正し、GUI試験で確認
- 受入済み2名journeyのsession確認が `fileUpload: false` を前提にしていた。保存領域を構成したため期待値を `true` に更新（hintであり操作ごとにserverが再確認する）
- 実browserで、Playwrightの既定（`journey` 以外は添付の保存を許可しない）により取得が保存されなかった。審査担当のcontextで明示的に許可
- この実行環境（`LANG` 未設定）のChromiumは日本語の保存名を `download` に置き換える（`C.UTF-8` では元の名前）。受入はserverの添付header（RFC 5987の名前）とbytes・SHA-256で確認する

### 新しい判断（承認済みとは扱わない）

[実装追補§10](../specs/2026-10-07-organization-work-files-handoff-amendment.md#10-新しい判断承認済みとは扱わない)の10点。

### Audit担当へのhandoff（共通schemaは変更していない）

- `work.event_staging` に `artifact_created`・`artifact_content_written`・`artifact_discarded`・`submission_imported` を追加（migration 0009）。payloadは成果物ID・schema・試行ID・世代ID・大きさ・SHA-256・取込み元snapshot ID
- 提出（`submitted`）は、ファイルを含む場合だけ `pinnedGenerationIds` を追加（文案だけの提出のpayloadは不変）
- ファイル名・文案本文は入れない

### Tauri担当との接続点

- ブラウザのファイル選択だけを実装した。Runtime Contractの読取りhandle（8 MiBの不変snapshot）から得たbytesを同じ `PUT /working-artifacts/{id}/content` へ渡せば、ローカルWorkspaceのファイルも同じ経路で添付できる。物理パス・binding IDはWork APIへ送らない

### 次のexact action

1. 独立reviewの結果を確認し、指摘を修正
3. U2統合後、同名branchを最新mainから作り直してU3 commitを移し、PR作成・exact-head CI
