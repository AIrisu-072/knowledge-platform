# 文書管理残タスク 2026-10-08：状況

状態: IN PROGRESS / 未統合 / 未配備。main基点 `2a37d35cd228344f98e0194de16d5336fa786e3c`、branch `feat/document-remaining-20261008`。

保存先は同一の **[Draft PR108](https://github.com/AIrisu-072/knowledge-platform/pull/108)**。製品commit `ee8b98f04fa012481cafa755add8db86c00666ef` / tree `3837d48cd4bbe1fd3cfcf52d763a82f2af016f45` で独立レビューGOを再確認した。本節のPR記録追記は文書のみで、製品source/試験/画像は変えない。最新保存headとhosted資格はPRのChecksと説明を参照し、読み直した正確なheadで判定する。

PR作成直後の製品headでは通常CI `37731617010`、DSI PoC `37731617017`、Sandbox `37731617158` の開始を確認した。これは完了・合格の記録ではなく、文書追記後のheadへ成功を転用しない。hostedの最終結果はheadを変えないPR説明にも記録する。統合順とmain統合・統合後CIはクラウド親が担当する。

文書head `d99d2f54e3e768b7b3219bb8993bb43ac5a79844` の通常CI `37731740750` は `document-poc-runtime` がFAIL。CI内Jest74 suites1740件とmock Chromiumは成功したが、実runtimeのmetadata-editor移動後read刷新が失敗した。完了を成功と取り違えた途中報告は訂正済み。原因は通常版queryでもAbortSignalを消費し、文書reset中のobserver一時脱落が版refetchをcancelしていたこと。遅延した文書/版の両GETでREDを再現し、signal消費をhistory用途だけへ限定する最小修正でGREENを確認した。版ID一致検査とhistory取消・認可失効保護は保持する。修正後headの独立レビューと全CIは別途資格を取る。旧headのDSI PoC `37731740780` とSandbox `37731740772` はSUCCESSであるが、新headへ成功を転用しない。

CI修正は独立レビューGO、独立7境界case成功。関連6 suites318件、全体74 suites **1741件成功 / 0失敗 / 0skip**、TypeScript、production build、diff check成功。新main `e67aaccc87644896ea1a58823dafadb6bf0b12d5` はPR107のDocument検査修正2filesと状況文書だけで、GUIと担当file重複なし。これを同じPR108に取り込んで最終headの全gateを取り直し、全成功と独立レビュー後のexpected-head guard付き統合は親から承認済み。統合後CIまで確認する。

統合head `196f5e04f850c399a171fe7c3b1975b29c82e931` は独立レビューGO。CI `37733569481` 初回はDocument実metadata-editor（移動後刷新を含む）成功、Organization再起動後persistenceのfinding GETが503でruntimeとrequired-checkがFAIL。他12通常jobsはSUCCESS。独立読取調査ではOrganization/backend/harness差分なし、readiness成功後の503でDB/provider依存または5秒の鮮度制約の可能性があるが根因未確定。同時期のmain runtime `37733428177` はSUCCESS。同じheadで失敗job全体を一度再試験し、通常14jobsすべてSUCCESS。DSI `37733569479`、Sandbox `37733569495` もSUCCESS（19checks中16成功・既存skip3）。初回503は解決済みとせず根因未確定の過去失敗として保持する。

統合直前にmainがTauri PR103の `8d4b94a912f9ac2e8079fc1c13b4efa4ce6d7538` へ進んだため、上記成功を新組合せへ転用せず追従した。40 upstream filesを競合なく取り込み、双方のactive pointerを保持。製品の文書履歴/改訂featureは変更しない。新headのreviewとGUI全体/型/buildおよび全hosted gatesを取り直す。最新資格はheadを変えないPR説明/Checksへ記録する。

Tauri取り込み製品head `82a25431b8c1f23908fbbb340823b880d6bdf7d5` の独立review GO。上流39files（active除外）はmainと、PR製品22files（active除外）は旧レビュー済みheadと完全一致。activeは双方の全pointerを保持。固定Node24.21.0でGUI **74 suites1756件成功 / 0失敗 / 0skip**、型、build、diff成功。最後の追記は文書だけでsource/試験を変えず、最新保存headの全hosted結果をPR説明/Checksで確認する。

## 現在の境界

元checkoutにはactive/statusの未コミット変更があり、そのまま保全した。別コピー `/Users/airisu/Documents/Codex/2026-10-08/task/knowledge-platform` で最新mainを取得した。repositoryに `.agents/skills` は存在しない。ユーザーのsession保存領域は参照していない。

最新mainのCI [37703216247](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37703216247) は14 jobs成功。PR106はmerged、merge commitは上記main。これは新変更の資格ではない。

項目2は既存ボタンのpublished/authoring用途では過去版を取得できない不備を確認し、明示history表示と既存ページ列へ接続した。指定版が未取得の場合の現行版へのfallbackを廃止した。項目3は既存getDocumentRevisionへ接続し、保存時snapshot/actor/reasonを表示する。focused REDは2 suites、4失敗/67成功/71件。

製品の独立レビューはGO。文書間選択残留、非表示/復帰、古い応答、履歴認可失効、一覧/単体詳細の再読取り、UNKNOWN固定要求再送、2拒否案内のaccessible nameを確認した。全体試験で見つかった通常選択保持/移動fixture/共有拒否の6失敗は修正し、最終の固定Node24.21.0 Jestは **74 suites / 1740件成功 / 0失敗 / 0skip**。関連4 suites191件、回帰3 suites181件、独立関連8 suites370件＋最終34件も成功。中断した編集中runや旧失敗runを資格へ含めない。

schema生成照合、TypeScript、Webpack production build、diff checkが成功。ビルドは3件の性能advisory（main JS709 KiB / entrypoint738 KiB）。承認済みの容量SLOとして扱わない。

最終ChromiumはAPI mockの機能試験 **8/8成功**。旧版detailはhistory以外404として試験し、取下げの対象URI/body/current保持を確認した。キーボード/画面遷移/1280・1440幅/reduced motionも検査。8080は別OrbStack作業が使用していたため停止せず、同じpreviewのPORTだけを8181にした一時configで再検証し、作成した一時filesは削除した。**golden画像比較は `--ignore-snapshots` により未資格**。追加ボタンを含む2画像を目視し、見出し/UUID/メタデータの折返し、確認対象/理由欄に重なり・欠けがないことを確認した。[改訂詳細](evidence/document-remaining-20261008/revision-detail.png)、[過去版取下げ確認](evidence/document-remaining-20261008/past-withdrawal.png)は合成データの画面証拠でありgolden更新ではない。

項目4と6は親のクラウドワークスペース担当。Macはschedulerの合成6利用者接続案を調査したが製品未編集。PR85元commit `53f08ac7537f9378b4d90c4810350471ff0d0f50` のdetect.rs/worker_contract.rsのみを一時抽出して `cargo test -p document-semantic-inspection-worker --test worker_contract` 13/13成功、fmt成功を確認し、差分は戻した。検索変更は取り込んでいない。

項目1/7/8/9の設計案を作成し確認待ち。製品実装はしていない。既読は現行の詳細正常表示契機を保持する。対象PC、認証/TLS、本番directory/権限、実backup/restoreは未実施・確認待ち。運用追補と大量測定計画を作成したが、新規1000件以上の実測はない。

## 検証環境

Rust 1.98.1、固定Node24.21.0、pnpm12.4.1/lockfileで依存準備。通常pnpm shimはENOEXECのためJest/tscは固定Nodeから直接実行する。観測した空きは約8 GiB→試験準備後約6 GiB、Docker socket権限拒否。新機能の実Linux DB/server結合、DSI/scheduler、対象PC、実復元、公式PDF大量投入の合格は記録しない。

次のexact action: PR108の現在headとChecksを読み直し、通常CI・DSI/Sandboxの結果をPR説明へ記録する。親が統合直前にscheduler/DSI変更との順序・最新mainを再確認する。main統合と統合後CIはまだ実施していない。原本構成/初回複数/主体検索/viewerは設計確認後に続行する。
