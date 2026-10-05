# 文書初回登録GUIの状況

## 2026-10-05 03:12 UTC — 最小実装と純粋検証

- 基点: main `d20f2c1c47b5dcbebdfd1901e3ce7d433fbd58eb` / tree `4a1ea931a8ac610ac33531d18be32b5050584555`。独立branch `feat/document-initial-registration-20261005`
- 範囲: 文書一覧/編集作業から登録先・文書名・原本1件を確認し、既存初回createでWORKINGを作成する。成功後はauthoringの版一覧へ移動し、公開は従来の別操作で行う
- 認可: root IDは既存APIで取得し、子FolderはそのFolderのchildren応答のcreateDocument capabilityを使う。capabilityは表示用であり、mutation/回復GETの現在認可が正本
- 結果不明: 初回createはserver生成IDで、operationIdや同一POSTの冪等性がない。送信前に未完了markerを同一originのsessionStorageへ保存し、二重click/再表示/同タブ再読込で再POSTしない。File、タイトル、本文、principalは保存しない。サーバーから3 IDsを受け取れた場合だけ既存typed GETで結果を照会する。404や通信断を未作成と断定しない。タブを閉じた後・別タブでの照合を保証する新backendは追加しない
- 純粋検証: 初期8件は登録UI未実装を理由にRED、最小実装後13件GREEN、独立レビューの遷移・422拒否・旧入力の追加RED5件を修正して計18件GREEN、capability未取得の非表示1件を加えて計19件GREEN。取消/再表示/フォルダー遷移後の遅延応答、未完了markerの保存失敗も含む。元GUI240件を含む最終全259件/21 suites PASS。型とschema freshness、production build PASS、既存Webpack advisory3件
- DOM試験では既存と同じjsdomのrequired-file制約によりform submitを使う。実browserではボタン/Enter操作で確認する。純粋試験を実ブラウザー合格として扱わない
- 既存hosted受入へ合成文書のGUI初回作成→未公開WORKING→原本bytes→確認付き公開→agent同一read→両HTTP server再起動後復元を追加する。既存runnerの固定到達段階6つだけを追加し、純粋診断はRED1件後16件PASS。新runner、画像/trace/video公開、外部サービス送信は追加しない
- ローカルDB/socket/listener/browserの実行はしていない。依存は既存固定lockのまま公式registryの供給元checkに合格し、既存storeから設置した。Search/Audit/Toolboxの停止作業や実サーバー反映は行わない

独立レビュー: 製品sourceのCritical/Importantは全て解消。追加ケースだけの画像/trace/videoを明示offにし、既存全体の設定は維持した。runtime型とdiff検査も成功。

最終確認: GUI259件、application/runtime型、schema freshness、production build、MCP受入bundleのbuild、journey12件/persistence1件のcollection-onlyが成功。新ケースだけを専用specに分け、top-levelで記録offにした。元document-runtime.spec.tsは変更していない。実browser/DBは未実行。

次の操作: 日本語Draftを公開し、そのexact headの全CIと既存使い捨てPostgreSQL/合成profile/Chromiumで実受入を終端まで確認する。main mergeは親へ引き渡す。
