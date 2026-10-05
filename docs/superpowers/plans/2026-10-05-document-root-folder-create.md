# System Root直下のフォルダー作成GUI

## 目的と今回の範囲

所有者の既存内部処理を通常の画面から使う指示に従い、既存フォルダー一覧へ作成入口だけを追加する。基点はPR75統合main `495dedb3945678bf70ff1a4a640843893814e2d3`。対象はSystem Rootの直下で、非rootの作成・改名・移動・ACL・既読更新は含めない。新backend/API/依存/権限・新しい永続化基盤は追加しない。

承認根拠は所有者 `Sentinel_cef3cbfde698819198d9ed687212a610` の「GUI v0ではまずは画面操作のみで大半のことができるように内部処理があるものを実装して使えるようにして欲しい。新規作成する必要があるものは後回しでいいです。」である。既存Frozen設計のサーバーcapabilityと最終現在認可を維持する小さいUI追加で、業務状態・権限判断を変更しない。

## 固定する設計

- 一覧のフォルダー欄から「System Rootにフォルダーを作成」を開き、名前と理由を入力する。別フォルダー選択中でも登録先をSystem Rootと明示する。GET `/v1/folders/root` のID/revision/createFolder capabilityだけを使用し、固定IDやACL/lifecycleをGUIで推測しない
- 初回送信前にoperationId・新folderId・parentFolderId・expectedParentRevision・name・reasonを一つの要求として固定する。既存typed SDKのPOST `/v1/folders` を使う
- QueryClientに紐づく既存操作保持方式にならい、pending/unknownの要求をタブ内で保持する。戻る/再表示/別画面往復で未解決要求を消さず、二重送信・別IDでの再作成を禁止する。ページ再読込/タブ終了では保持が失われるため離脱警告と管理者への結果照会を表示する。ブラウザー永続storageへpayloadを保存しない
- unknown後は同じ要求だけを明示再送する。後続403/404等は初回未実行の証明ではない。新規入力やrootの新revisionで要求を作り直さない。最終再認可・OCC・名前正規化/重複制約・冪等性はbackendが判定する
- 確定成功はoperationIdと新folderIdの一致を確認し、作成通知とroot/子一覧の再取得へ進む。返るresultingRevisionは子のrevisionなので親に代入しない。再取得の失敗を作成失敗へ変換しない。遅延完了で別画面へ強制移動しない
- 初回の確定拒否だけは内容見直しへ戻せる。再取得成功後の新しい明示操作は新revisionと新IDを使い、旧unknown要求の復旧と混ぜない

## 実装と検証

### Task 1: 純粋GUIと既存API配線

`apps/document-web/src/api/document-api.ts` のSDK wrapper、限定application操作状態、作成dialog、`DocumentHomePage` の入口を実装する。既存React Aria/問題表示/UUIDv7/QueryClientの様式とCSSを再利用する。最初に単体/DOM反例をREDにし、root正本、disabled理由、名前/理由、pending連打、キャンセル、再表示、戻る/進む、別navigation、unknown固定要求、403/404、競合、結果不一致、再取得失敗、成功後の新規操作をGREENにする。全GUI・schema/型・production buildを確認し、限定独立レビューを行う。

### Task 2: 既存hosted受入への追加

既存Organizationのjourney/persistence specへ独立ケースを各1件追加する。同じ固定2合成profile・使い捨てPostgreSQL18.6・Chromium・既存runnerで、通常ナビからroot直下作成、表示再取得、同じ要求の冪等性、HTTP再起動後の保持を確認する。新specだけscreenshot/trace/videoをoffにし、既存有限診断へ必要な固定case/stageだけを追加する。新検証基盤や新画像は作らない。同要求replayはbackendの実冪等性確認で、GUI unknown回復はTask 1の純粋DOM資格と区別する。ローカルは純粋試験・型・collectionのみでlistener/socket/browser/DB/Cargoを実行しない。

### Task 3: 最終確認と公開

Task 1/2のレビューを閉じ、全GUI・型/build・安全なruntime純粋試験・collectionの最終組合せを検証する。日本語GUI手順・実行状況を更新し、小さいDraft PRを公開、同一headの全適用CI・実受入・owned cleanup・公開artifact0を確認する。main mergeは親担当、実サーバー反映は所有者の手動操作とする。

## 全体の制約

既存未検証のmacOS goldenと全応答喪失faultを合格へ付け替えず、golden/skip変更なし。停止中Search/Audit/Toolboxは変更しない。元branch/packet/証拠を保持する。対象PCの手順全文・backup/restore・PostgreSQLプロセス再起動の未検証も維持する。
