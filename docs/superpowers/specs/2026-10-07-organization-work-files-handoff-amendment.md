# Organization Client v0 — 作業ファイル・共有provider・Handoff・差戻し後の作業の実装追補（U3）

状態：実装前の追補。凍結済みの[Product/UX](2026-10-02-organization-client-v0-product-ux-design.md) §6・§7・§10、
[Domain/API/Auth](2026-10-02-organization-client-v0-domain-api-design.md) §4・§6・§7・§13・§14、
[UI](2026-10-02-organization-client-v0-ui-design.md) §5〜§7 の意味を変えず、U1（[複数担当](2026-10-07-organization-multi-principal-amendment.md)）・
U2（[文脈・注意・表示Profile](2026-10-07-organization-work-context-attention-amendment.md)）の上に、
「個人作業ファイルの添付」「共有providerへの保存」「提出時のHandoff Snapshot」「差戻し後の新しい作業」を接続する最小の具体化だけを記録する。
新しい業務判断は §10 に分けて明示し、承認済みとは扱わない。

## 1. 基点と範囲

- 基点：U2統合後のmain
- 既存の工程（提出・差戻・完了・保留再開・根拠/候補/人間判断・合成Agent・文書参照）、担当判定（U1）、文脈・注意（U2）は再実装しない
- 本単位で変えるのは次の4点だけ
  1. 作業中の成果物に「ファイル」を追加する（既存の文案と同じ `work_item_private`）
  2. ファイルの内容を、Work所有の共有保存領域（Domain §7の最小Shared Authoritative Provider）へ明示操作で保存する
  3. 提出時に、serverが保存領域で確認したファイルの不変世代をHandoff Snapshotへ固定し、次担当が現在の認可で取得する
  4. 差戻しで作られた試行で、前回の提出を明示操作で新しい非公開文案へ取り込み、作業をやり直して再提出する
- Documentへの昇格（Document Platformへの保存）、ローカルWorkspace（Tauri/native）からのファイル選択、保存済み内容の削除・GC、ブラウザ内プレビューは含めない（§11）

## 2. 共有provider（Work成果物保存領域）

Domain §7の「Work authorityが所有する別のmetadataと保存namespace（`WorkArtifactStorage`）」を次のとおり具体化する。

- 保存先：Organization serverの保存root（`KP_STORAGE_ROOT`）の下の別namespace `work-artifacts/`。既存の `FileSystemStorage` adapterをそのまま再利用し、`staging` から不変の `objects` へ移す方式・fsyncも同じ
- Documentの版・改訂・ACL・公開・解析（DSI/Diff）・検索の記録へは何も複製しない。Documentの保存一覧（`staging`/`objects`）とも重ならない
- 保存rootは静的配信しない。内容はWork APIを通してだけ返す。frontend・Agent・nativeへ物理パスを渡さない
- server所有でdeviceに依存しないため、提出後は別のprincipal・別の端末から取得できる。ただし提出までは `work_item_private`（現在の試行の担当者だけ）であり、「共有の保存領域に置いた」ことは「共有して見せた」ことを意味しない
- 世代（generation）：内容を登録するたびに新しい不変の世代を作る。世代は `{id, sizeBytes, sha256, storedAt, providerId}` で、IDは内容登録の操作IDと同じ値。同じ世代IDへ異なる内容は保存しない
- 6 processは同じ保存rootを共有する（既存のDocument保存と同じ前提）

## 3. 作業ファイル（WorkingArtifact）の記録

既存の `WorkingArtifact` に次を追加する（文案の保存JSON・API表現は不変）。

| 項目 | 文案（既存） | ファイル（新規） |
|---|---|---|
| `schemaId` | `organization.text-draft.v1` | `organization.work-file.v1` |
| `value` | `{text}` | 無し |
| `file` | 無し | `{fileName, mediaType, generation?}` |
| `derivedFrom` | 取込み時だけ `{snapshotId, artifactId}` | 同左 |
| `visibility` | `work_item_private` | 同左 |

- 作成時は `generation` が無い（内容未登録）。内容未登録・登録失敗・結果不明のファイルは提出できない
- `fileName` は表示名だけであり、ローカルのパスではない。パス区切り（`/`、`\`）、制御文字、拡張子を偽装できる双方向・不可視の書式文字（U+061C、U+200B〜U+200F、U+202A〜U+202E、U+2066〜U+2069、U+FEFF）、`.`・`..`、空、255 bytes超を拒否する。ブラウザが渡すのも名前だけで、serverはパスを受け取らない
- `mediaType` は `type/subtype` 形式（127 bytes以内）の申告値として記録するだけで、取得時の応答には使わない（§6）
- 1ファイルは1 byte以上8 MiB以下。1試行の成果物（文案とファイルの合計）は16件以内（既存 `MAX_ARTIFACTS`）
- 内容の再登録は同じ成果物に新しい世代を作る。前の世代・提出済みの固定内容は変更しない

## 4. 操作と認可

すべて現在の試行の担当者本人が、記録したacting responsibilityで `work.edit` を持つときだけ実行できる（既存の文案保存と同じ）。担当可能なだけの利用者、次工程、管理担当、担当変更後の元の担当者は、既知のIDでも作成・登録・取得できない（hidden 404）。

| API | 操作 | 内容 |
|---|---|---|
| `POST /tasks/{id}/working-artifacts`（`file` 指定） | ファイルの作成 | 名前と申告media typeだけを記録。operation ledger・必須staging |
| `PUT /working-artifacts/{id}/content` | 内容の登録 | 8 MiB以内のbinary。操作ID・task revision・acting responsibility・成果物revisionはheaderで渡す。serverが受け取ったbytesから大きさとSHA-256を計算し、保存領域へ保存した後にcommandを確定する |
| `GET /working-artifacts/{id}/content` | 作業中ファイルの取得 | 現在の試行の担当者だけ。保存領域で大きさ・hashを確認してから返す |
| `POST /working-artifacts/{id}/discard` | ファイル・文案を外す | 提出前の現在の試行の成果物を記録から外す。保存済みの内容は削除しない |
| `POST /tasks/{id}/working-artifacts/import` | 前回提出の取込み | §7 |
| `GET /handoff-snapshots/{id}/artifacts/{artifactId}/content` | 提出済みファイルの取得 | §6 |

- 内容の登録は、先に認可（現在の担当者・現在の試行・成果物revision）を確認してからbytesを保存する。保存後にcommandが競合で失敗した内容は参照されないまま残る（Domain §7：非公開のまま、破壊的な削除はしない）
- 同じ操作IDの再送は、同じbytesなら同じ結果を返し、異なるbytesなら `OPERATION_CONFLICT`。記録の有無より先にoperation ledgerを確認する（外した後の再送も保存領域へ書かない）。外す操作の再送も、記録が無くなった後に同じ操作の受領内容から再実行する
- 保存領域の呼出しは1回5秒で打ち切り、`WORK_ARTIFACT_UNAVAILABLE` とする。保存先のI/O失敗は、同じ世代に別の内容が確定している場合だけ `OPERATION_CONFLICT`、それ以外は `WORK_ARTIFACT_UNAVAILABLE`
- 変更操作の503は結果不明として扱い、同じ操作IDで照会する（同じ操作が別のrequestで確定しうるため）。読取りの503は「利用できません」と表示する
- 保存領域が構成されていないserverはファイル作成を `WORK_ARTIFACT_UNAVAILABLE` で拒否し、sessionの `fileUpload` はfalse（画面は追加を出さない）
- 他の画面・タブで外された文案・ファイルへの操作（`WORK_ARTIFACT_NOT_FOUND`）は、タスクが読める限り古い状態として再読込し、閲覧拒否として入力を消さない
- 結果不明（`COMMIT_OUTCOME_UNKNOWN`）は既存どおり同じ操作IDで照会する

## 5. 提出時のHandoff Snapshot

- 提出は文案・ファイルの任意の組合せ（1件以上16件以内）を選ぶ。既存の文案だけの提出は不変
- 提出前（業務行のlock前）に、選択したファイルの世代を保存領域で確認する（存在・大きさ・SHA-256）。確認できた世代の集合だけをserver内部の非永続の受領確認としてaggregateへ渡し、lock中に固定する。frontendの申告・flagは使わない
- 確認できない世代があれば提出しない（`WORK_ARTIFACT_UNAVAILABLE`、503）。内容未登録のファイルは `HANDOFF_NOT_READY`
- 固定する内容：`artifacts[]` に `{artifactId, revision, schemaId, value? , file?}`。`file` は `{fileName, mediaType, generation}` で、作業中の可変な指し先ではなく、その時点の不変世代を固定する
- 提出・source試行の完了・次の試行（ready）・ledger・stagingは既存どおり同じtransaction

## 6. 受領者・担当者の取得

- 提出済みファイルは、既存の `getHandoffSnapshot` と同じ規則で読める利用者（受領した試行の担当者、提出者本人で現在も営業工程の責任を持つ者、差戻しで前回提出を参照する現在の担当者）だけが取得できる。snapshotの所属はproviderの継続的な権限を与えない
- 取得の直前に現在の認可を確認し、保存領域から読んだ内容の大きさ・hashを固定値と照合し、返す直前にもう一度現在の認可を確認する。不一致・欠落は内容を返さず `WORK_ARTIFACT_UNAVAILABLE`（画面では「利用できません」と表示し、空や成功として扱わない）
- 応答は常に `application/octet-stream` の添付（`Content-Disposition: attachment`、RFC 5987のUTF-8名）、`Cache-Control: no-store`、`X-Content-Type-Options: nosniff`、`Content-Security-Policy: sandbox`。ブラウザ内で表示・実行しない

## 7. 差戻し後の新しい作業（取込み）

- 対象：差戻しで作られ、現在activeの営業工程の試行（`returnInstructionId` あり）
- 担当者が明示的に「前回の提出を取り込む」を実行すると、その試行が参照する前回提出（`handoffSnapshotId`）の固定内容から、新しい非公開成果物（新しいID、revision 0、`derivedFrom` 付き）を作る。ファイルは同じ不変世代を参照し、bytesを複製しない
- 前回の提出・差戻指示・完了済みの試行は変更しない。同じ提出からの重複取込みは拒否する。自動では取り込まない
- 取り込んだ後は通常どおり文案を編集・保存し、ファイルを外す・追加して再提出する。再提出は新しいsnapshot（`previousSubmissionId` あり）と次工程の新しい試行を作る

## 8. Audit担当へのhandoff（共通schemaは変更しない）

`work.event_staging` のactionに `artifact_created`、`artifact_content_written`、`artifact_discarded`、`submission_imported` を追加する（migration 0009、語彙のcheckだけ）。payloadは成果物ID・schema・世代ID・大きさ・SHA-256・取込み元snapshot IDだけで、ファイル名・文案本文は入れない。

## 9. 受入

- Domain：作成・内容登録・外す・取込み・提出の固定・受領確認なしの拒否、非開示（次工程・担当可能なだけ・担当変更後の元担当者）、名前規則・上限
- Repository（実PostgreSQL）：ledger・staging・replay、受領確認の失敗で提出しないこと
- HTTP：8 MiB境界、添付応答header、hash不一致で503、header経由の操作ID・revision
- GUI：ファイルの追加・内容登録の失敗/再試行・外す・提出確認への表示・受領側の取得・取込み
- 実browser（既存の6名DB、文脈persistenceの後）：差戻された案件Bを営業担当が引き受け、前回提出を取り込み、ファイルを添付して保存し、次工程から既知IDで読めないことを確認して再提出。審査担当が受領ファイルを取得してbytesを照合し、前回提出が変わっていないことを確認して完了。6 process再起動後も取得できる

## 10. 新しい判断（承認済みとは扱わない）

1. Work成果物の保存先を、Organization serverの保存rootの下の別namespace `work-artifacts/` とする（既存のFileSystemStorage adapterを再利用）
2. 上限：1ファイル1 byte以上8 MiB以下、1試行の成果物16件（文案を含む）、ファイル名255 bytes・media type 127 bytes
3. ファイルは常に `application/octet-stream` の添付として返し、ブラウザ内のプレビューは作らない
4. 世代IDは内容登録の操作IDと同じ値とする（同じ内容の再送は冪等、異なる内容の再送は `OPERATION_CONFLICT`）
5. 差戻し後の試行で、前回提出の文案・ファイル参照を利用者の明示操作で新しい非公開成果物へ取り込めるようにする（自動取込みはしない。ファイルは複製せず同じ不変世代を参照）
6. 提出前の成果物は担当者が記録から外せる。保存済みの内容は削除しない（削除・GCの方針は未決定）
7. 提出時・取得時の受領確認は、保存領域での存在・大きさ・SHA-256の一致とする
8. ファイル名・文案本文はevent stagingに入れない
9. 文案の無い（ファイルだけの）提出を認める
10. 新しいAPI path：`/working-artifacts/{id}/discard`、`/tasks/{id}/working-artifacts/import`、`/handoff-snapshots/{id}/artifacts/{artifactId}/content`（設計§13の表に無いもの）

## 11. 範囲外・残件

- Document Platformへの昇格（提出済みの内容を既存のDocument APIで文書化する明示操作）。Domain §7のとおり別操作であり、作業中の非公開境界を弱めない形で後続に分ける
- ローカルWorkspace（Tauri担当のRuntime Contract・broker）からのファイル選択。本単位はブラウザのファイル選択だけで、Runtime Contractの読取りhandleから同じ内容登録APIへ渡す接続点だけを残す
- 保存済み内容の削除・保持期間・orphanの回収、容量の上限、ウイルス検査、プレビュー。PoCでは外す・差し替えの繰返しや競合で参照されない内容が増え、Documentと共有する保存rootの容量を消費しうる（本番前に上限・回収の判断が必要）
- 書込み途中で停止した場合に残る `staging/{操作ID}.part` があると、同じ操作IDの再送は保存できない（結果不明のまま）。利用者はファイルを選び直して新しい操作で登録する
- 本番の保存基盤・暗号化・backupの選定
