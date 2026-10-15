# Document local 10万件検証の状況

Status: ACTIVE / local実装・独立レビュー完了、正確headのCI準備中

- branch: `test/document-load-hundred-thousand-20261009`
- 基点: `d996f8012ef7ebe57e5fc47b49bd85d26d02441a`（PR118統合済みmain、統合後CI全14成功）
- 設計: [Ubuntuでの10万件検証](../plans/2026-10-09-document-load-local-scale.md)
- 承認: 1万/10万件の実施、候補Ubuntuへの専用ツール導入と長時間試験、キャンセル扱いだったbranch/Draft作成再開について、2026-10-09「両方進めて問題ないです。」を確認。

2026-10-09 03:26 UTC: 4段階chain、専用ext4 DB、秘密値を除去する上限付きserver log、disk上の全観測とonline集約、compact receipt、排他起動とackをTDDで実装した。Cloud Node24.19.0でハーネス452件、既存runtime190件成功。288,000資源標本＋120万タイミングのstressは測定器RSS約130MiB、全111,002正例IDを検査するcompact receiptは約10KBで128MiB heap下でも成功した。これは合成fixtureによる測定器検査であり実10万件の成功ではない。

独立レビューはGO。watchdogが起動準備時間を差し引かない指摘をRED→GREENで修正し、最後のTERM→KILL120秒も当初80時間以内へ収めた。独立した143件・44件・期限修正後25件と10万通りの秘密値/chunk境界検査が成功。製品PDF/APIと既定workflowは変更していない。次はPR118統合後の正確mainを基点にDraft PRへ保存し、正確headのCIを通す。

2026-10-09 03:29 UTC: PR118のCI `37877521859` は全14成功。DraftからReadyへの変更とmain統合の明示確認待ちのため、後続の保存/統合順を保留している。local runnerの最終452件/190件と独立GOは完了しており、次はPR118の統合後に正確mainを基点として保存・CIを実施する。

Ubuntu側は固定版の専用環境準備を完了し、既存設定を変更していない。資格済みmainによる現地事前検証はGUI登録の初期操作で停止し、公式PDF small・Agent・再起動まで到達していない。失敗をsandbox/PDF全資格の成功として扱わず、ログに基づき診断する。新local runnerの実負荷は未開始。

2026-10-09 05:11 UTC: PR118はmain `d996f8012ef7ebe57e5fc47b49bd85d26d02441a` へ統合され、統合後CI `37885326309` 全14成功。CI済みPR118とmergeのtreeが一致するため、local変更を保持してこのmainへ基点を更新した。1万件の手動開始は別途進行し、本変更は10万件local modeのDraft保存・正確head CIを先に行う。Ubuntuで確認したGUIの遅延一覧応答によるfocus移動は別修正で資格を取り、その統合後clean sourceで実負荷を始める。現時点で実10万件は未実行。

2026-10-09 05:15 UTC: 保存前の再検証で、started.jsonの作成と書込みの間に読取が起きる競合を1件検出した（451/452成功、runtime190成功）。部分書込みを意図的に停止する決定的REDを追加し、privateな排他publication directory内のready markerを本文の書込み・close後に原子的に作成する方式へ修正した。未完ackは待機、公開後の不正/欠落JSONは拒否し、既存証拠の上書き・二重publisher・非private/symlink markerも拒否する。独立したlauncher11件が成功。最終buildとハーネス455/455成功、既存runtime190/190成功、差分検査成功。修正差分の独立確認もGO。正確headのCIへ進む。
