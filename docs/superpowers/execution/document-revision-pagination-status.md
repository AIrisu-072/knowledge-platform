# 正式改訂の続き表示：実行状況

## 2026-10-06 13:47 UTC

- PR88統合main `ea40684833ebcdf945636300b7b22741725fc80c` / tree `cd18f8cfdafeb57bb537df11e46a0e30ba907030` を基点とする。branch `feat/document-revision-pagination-20261006`、[小計画](../plans/2026-10-06-document-revision-pagination.md)に従い既存nextCursorを通常GUIへ配線する。
- PR88公開head `af08e586` は[CI37455031944](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37455031944)の全13jobs・全18checks（15成功/既存skip3）、Document18+5/Agent9・Organization・HTTP再起動・指定DB36・cleanup/artifact0を確認してmainへ統合した。個別移動/replay/Document cleanupは同tree sourceと公開PASSの対応推論。main自身の[push CI `37457937486`](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37457937486)もattempt1の全13jobs/checks・Document18+5/Agent9/22工程・Organization全8工程・HTTP再起動/cleanup・指定DB36/36・artifact0を独立確認した。ref/tree/両parentsも終端後に一致。
- 基点のGUIは正式改訂の先頭100件だけを読み、APIのnextCursorを使っていなかった。今回の候補は「版・改訂」「新旧比較」でページ列を共有し、続き表示と明示した比較IDの未取得時保持を実装した。新backend・権限projection・Version history用途は追加しない。
- 既存Node/pnpmと公式registryの供給元検査724件に合格し、同じ固定lockの687依存を再利用した。offline metadata不足は通常の公式metadata取得で解消し、検査の無効化や別runnerへの変更はしていない。
- GUI初回候補 `9a2e9465` は実DOM21件の欠落反例からページ列・明示比較ID保持を実装した。CURSOR_STALEの旧cursor自動再送と、Document read拒否→回復時の古い履歴復活も追加の実反例から限定補修。初回検証は全GUI1126件/47 suites・focused141件/3 suites・schema/型/build成功だった。

- Task 1初回独立reviewはImportant 2件でNO-GOだった。Document GETの中間403が自動retry成功で隠れた場合と、比較403後に成功済みの別pairへ戻る場合に、旧history/比較cacheがfresh readなしで残る反例を再現した。

- `9fc21a9` は認可拒否をAPI失敗境界から当該ページ列へ保持し、同文書の全pair readを取消/破棄する限定補修。取消済み旧要求の遅延403もAbortSignalで除外した。実RED7件と取消遅延2件を閉じ、全GUI1137/47・focused152/3・schema/型/build成功。独立限定再reviewはI1/I2 ADDRESSED・spec/品質GO、新所見0。global/detail/comparison retryは不変。
- 受入source `d5cc1493` は既存metadata journey/persistenceの2caseへ、正式改訂2件の通常表示、明示した1.1→1.0の比較、先頭再読取後の同pair新POST、既存2 HTTP再起動後の再表示をinline追加した。元のsnapshot/原本/actor別readState・文書移動/replay・日時/未読検査を保持し、新helper/fixture/runner/caseを追加していない。純粋guardの意図したRED3件→GREEN、最終62/62・runtime型・MCP compile・collection18+5成功。
- Task 2の初回組合せreviewはImportant 1件でNO-GOだった。比較専用画面に通常tabがなく、受入の戻り3箇所が到達不能だった。独立DOM反例2 FAILと正しい戻る導線1 PASSを確認し、`06c27907` で既存「← 版・改訂へ戻る」button経由へ限定補修した。製品変更なし、navigation guardのRED1→GREEN、最終純粋63/63・runtime型・collection18+5成功。元の6通信観測・逆向きpair・URL/readState/snapshot検査を保持した。
- Task 2 I1の限定再reviewはADDRESSED、Task 2 spec/品質・全feature組合せGO、新所見0。GUI・試験・日本語手順13filesを[Draft PR89](https://github.com/AIrisu-072/knowledge-platform/pull/89)へ保存した。初回公開head `de849c92830536616f92418ff6421f0840e81f02` / tree `d35e9f6dca3d2ca243177bdc009248c294d2b65f` は凍結ローカル候補と全blobが一致し、baseは `ea406848` のまま。
- 初回[CI37469083623](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37469083623)の画像なし実受入はjourney16成功/2失敗で不合格。actual head一致・cleanだがqualified=false。予約取消は `gui-schedule-dismissed` 後のstrict-locator/toContainText、metadataは `gui-metadata-noop-verified` 後のtimeout。Agent・HTTP再起動・persistence・Organizationは未到達である。有限公開診断に失敗node本文やtimeoutの正確な待機行はない。apiEventsは先頭12件で、停止直前の通信ではない。
- 初回CIは全終端し、通常13jobsは11成功/実受入と集約checkの2失敗、全18checksは13成功/2失敗/既存skip3。Rustと指定DB36/36・Folder回帰4/4は成功、全4workflowはattempt1・artifact0だった。失敗runのcleanup成功は有限stdoutから確認できず、成功扱いしていない。
- 予約取消は同treeの実DOMで、成功statusと正式改訂の背景再読取statusの同居を再現した。`04a393f9` は既存の名前付き取消操作region内で単一成功statusを完全一致で確認する試験だけの限定補修。通知・aria-liveと独立readは保持する。新2反例RED→既存を含む25 PASS、Web schema/型・runtime型・collection18+5、独立spec/品質GO。遅いrefresh成功/403でも通知と取消POST1回を検査する。
- metadataのtimeout根因・停止awaitは有限診断から未確定。固定Nodeの実routeでは、日時開始・未読ON・分範囲の3条件は初回の異なるGETであり、overview中の改訂GETは0だった。cache再利用や新改訂readを根因と断定しない。一方、閉じた編集dialogの遅いfocus復帰が別の操作へ移したfocusを奪う具体反例は成立した。
- `31aed928` はこの独立したUI欠陥だけを補修した。close時の元triggerと既存opening世代を捕捉し、同世代・接続中・有効・focusがbodyの場合だけ復帰する。実RED2→metadata39 PASS、予約取消補修を含む全GUI1144/47・schema/型/build成功。既存build性能warning3件を保持。実CIのtimeout根因を特定・解消したという主張ではない。Homeの選択行focusにも別の競合反例があり、今回は残件として保持する。
- metadata focus補修と予約取消fixの組合せは限定独立spec/品質GO、新所見0。次のexact actionは同PRの新headで既存hosted CI/実受入・cleanup/artifact0を確認すること。初回失敗の証拠を保持し、原因未特定を修正済みとは扱わない。timeout延長・skip・根拠のない繰返し実行はせず、業務意味・権限モデルは変えない。
- 100件超の実GUI例は既存fixtureになく、今回の実資格へ含めない。DOM100+1と既存HTTP cursor試験、既存2改訂の画像なし実受入を区別する。macOS golden/画像・本番Identity/対象PC/PGプロセス再起動等の既存未資格を保持する。固定導入版 `cd6aafcc` は文書移動と今回の続き表示を含まず、GUI手順に明記した。結果だけの別PRは作らない。
