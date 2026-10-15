# 読み取れる文書を既存APIで移動する小計画

> 実装はTDD・独立spec/品質レビューで進め、機能・試験・日本語手順を1つのPRへ収める。

## 目的と承認済みの境界

通常GUIから既存内部処理を使う所有者の指示 `Sentinel_cef3cbfde698819198d9ed687212a610` と、既存GUI・付随試験・文書の保存継続指示に基づく限定追加。PR87統合main `3448c51de9ceac4f74531c5ca841c0842d09d725` を基点にする。Frozen管理v0 §5/6/8.2、Frozen GUI §13の認可hint/現在read/UNKNOWNを維持する。新backendや権限契約は追加しない。

通常published/authoring詳細を読めて、現在の `folderId` が非nullの文書を対象とする。詳細の `capabilities.moveDocument` を提示hintとして使う。非表示の元FolderをURL・一覧・Rootから推測しない。終了後T10・非表示元Folder・読めない移動先は今回の入口の対象外。APIが持つ能力すべてのGUI化完了とは扱わない。

## 共通制約

- 既存typed SDK、React Query、React Aria、FolderNode、readSelectedFolder、global access_revision後のread再取得を再利用する。汎用mutation基盤・新依存・新runnerは追加しない。
- 文書の正式Major/Minor、Version/原本/manifest/既読/明示policyを変えない。移動時のDocument OCC revisionとaccess_revisionの加算、同Folder no-op、現在認可付きreplayは既存serverが正本。
- 送信bodyは `operationId/fromFolderId/toFolderId/expectedDocumentRevision/reason` の5項目だけ。移動先Folderのmove/create hintを認可の代わりにしない。Rootも読める移動先候補。
- 日本語PR/docs。画像・trace/video・新sidecar・DB/ブラウザーのローカル実行なし。Node24.21.0/pnpm12.4.1の既存固定lockで純粋GUI試験・型/buildを実行する。実DB/ブラウザーは既存hostedだけ。
- Search/Audit/Toolboxの停止作業、新ACL preview、新権限モデルを含めない。main mergeは親担当、実サーバー導入は所有者の手動。

## Task 1: 文書移動の固定操作と通常GUI

対象: `apps/document-web/src/api/document-api.ts`、新application `document-move.ts`、新component `DocumentMove.tsx`、DocumentDetailPage/DocumentHomePage、必要な既存Folder操作の未確定guardと対応test。

- [x] 既存API facadeへmoveDocumentを配線する反例を先に書き、欠落による実REDを確認する。
- [x] 詳細入口と一覧の保持要求入口を加える。新規は詳細を開く時/送信直前にfresh GETし、Document ID/title/folderId/folderName/revision/current hintを検査する。移動先もRoot readまたは選択行のparent/page provenanceからfresh readする。 現概要のfolderName nullを「ルート」へ置換する表示も、未確認の所属と分かる文言へ限定補正する。
- [x] 対象名/ID・元所属名/ID・移動先名/ID・理由を表示し、「明示アクセス設定は保持。継承中は移動先の設定が適用され、自分を含む閲覧・編集権限が変わり得る」ことを明示確認する。ACL差分や人数を捏造しない。
- [x] Close/再表示/Back/別navigation・遅延応答・同batch移動先変更を検査し、古い非同期結果から送信しない。送信前の状態変化は入力保持と明示見直しへ止める。
- [x] QueryClient所有の小さいDocument専用storeでpath/body/表示contextを固定する。新しい移動は未解決の同storeを上書きしない。Folder作成/改名/移動との未確定は相互保持し、新規開始だけ止める。既存他操作store/providerは消さない。
- [x] UNKNOWNは同一要求だけを再送する。再送後403/404/409でも未実行と認定しない。初回の既知Problem code/status拒否と、通信断/503/不正receiptを分ける。receiptのoperation/resource/changed/revision/日時を厳密照合し、+1/据置のsafe integer上限を確認する。
- [x] 成功後は既存global read resetで現在認可に依存するreadのみ再取得し、操作cache・providerを保持する。旧receiptで現在所属へ書き戻さない。権限喪失/GET失敗でも保持結果が一覧から再表示できることを確認する。別navigationやURLのopaque cursorを無言で書き換えない。
- [x] focused RED→GREENと全GUI・schema/型/buildを確認し、小commitの独立spec/品質レビューを受ける。

## Task 2: 既存の画像なし実受入へ最小追加

対象: `apps/document-web/e2e-runtime/metadata-editor.spec.ts`、必要な既存support/純粋検査/有限診断。既存Document18+5内で実施する。

- [x] 既存metadata合成文書をSharedからSandboxへ1回移動する。普通の詳細/ツリー操作で到達し、元/先fresh GET・正確な5項目POST/receipt・現在詳細を照合する。
- [x] 正式Revision/全原本hashと本人/Agent readStateを前後比較する。private状態に固定要求・receipt・移動先だけを最小保存する。
- [x] 既存2 HTTP再起動のpersistenceで現在所属と元receipt再送を確認する。replayでrevision/公開状態/正式版/原本/既読を追加変更しない。既存metadata・未読・日時ケースの検査を保持する。
- [ ] 純粋検査の反例→GREEN、runtime型、既存collectionを確認し、独立レビューを受ける。新fixture/runner/timeout延長/skip/画像/artifact公開は追加しない。


## Task 3: 同PRの手順と公開資格

- [ ] 日本語GUI手順・今回status/Activeを更新し、対象外と未知結果の回復限界を明示する。導入pinが未収録ならその範囲を記す。
- [ ] 独立した組合せレビュー、通常CI/実受入/cleanup/artifact0を同じ公開headで確認してから親へmerge-readyを返す。旧ローカルログが環境から失われた事実と、現在新たに取得した証拠を区別する。
- [ ] macOS golden、全status/headers喪失、本番Identity/TLS、対象PC導入、backup/restore、PGプロセス再起動は今回も未資格。同権限Shared→Sandboxを実ACL変化の資格とは扱わない。

## レビュー観点

null folderIdの補完禁止、移動後read拒否でも元要求回復可能、同batchの旧移動先誤送信なし、stale/new navigationで自動再送なし、global resetによる既存未確定操作消失なしをTask 1の実DOM/純粋試験で確認する。Task 2の実受入成功をGUI通信断や実ACL差分の資格へ拡大しない。
