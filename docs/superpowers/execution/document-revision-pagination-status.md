# 正式改訂の続き表示：実行状況

## 2026-10-06 13:05 UTC

- PR88統合main `ea40684833ebcdf945636300b7b22741725fc80c` / tree `cd18f8cfdafeb57bb537df11e46a0e30ba907030` を基点とする。branch `feat/document-revision-pagination-20261006`、[小計画](../plans/2026-10-06-document-revision-pagination.md)に従い既存nextCursorを通常GUIへ配線する。
- PR88公開head `af08e586` は[CI37455031944](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37455031944)の全13jobs・全18checks（15成功/既存skip3）、Document18+5/Agent9・Organization・HTTP再起動・指定DB36・cleanup/artifact0を確認してmainへ統合した。個別移動/replay/Document cleanupは同tree sourceと公開PASSの対応推論。main自身の[push CI `37457937486`](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37457937486)もattempt1の全13jobs/checks・Document18+5/Agent9/22工程・Organization全8工程・HTTP再起動/cleanup・指定DB36/36・artifact0を独立確認した。ref/tree/両parentsも終端後に一致。
- 基点のGUIは正式改訂の先頭100件だけを読み、APIのnextCursorを使っていなかった。今回の候補は「版・改訂」「新旧比較」でページ列を共有し、続き表示と明示した比較IDの未取得時保持を実装した。新backend・権限projection・Version history用途は追加しない。
- 既存Node/pnpmと公式registryの供給元検査724件に合格し、同じ固定lockの687依存を再利用した。offline metadata不足は通常の公式metadata取得で解消し、検査の無効化や別runnerへの変更はしていない。
- GUI初回候補 `9a2e9465` は実DOM21件の欠落反例からページ列・明示比較ID保持を実装した。CURSOR_STALEの旧cursor自動再送と、Document read拒否→回復時の古い履歴復活も追加の実反例から限定補修。初回検証は全GUI1126件/47 suites・focused141件/3 suites・schema/型/build成功だった。

- Task 1初回独立reviewはImportant 2件でNO-GOだった。Document GETの中間403が自動retry成功で隠れた場合と、比較403後に成功済みの別pairへ戻る場合に、旧history/比較cacheがfresh readなしで残る反例を再現した。

- `9fc21a9` は認可拒否をAPI失敗境界から当該ページ列へ保持し、同文書の全pair readを取消/破棄する限定補修。取消済み旧要求の遅延403もAbortSignalで除外した。実RED7件と取消遅延2件を閉じ、全GUI1137/47・focused152/3・schema/型/build成功。独立限定再reviewはI1/I2 ADDRESSED・spec/品質GO、新所見0。global/detail/comparison retryは不変。
- 受入source `d5cc1493` は既存metadata journey/persistenceの2caseへ、正式改訂2件の通常表示、明示した1.1→1.0の比較、先頭再読取後の同pair新POST、既存2 HTTP再起動後の再表示をinline追加した。元のsnapshot/原本/actor別readState・文書移動/replay・日時/未読検査を保持し、新helper/fixture/runner/caseを追加していない。純粋guardの意図したRED3件→GREEN、最終62/62・runtime型・MCP compile・collection18+5成功。
- Task 2の初回組合せreviewはImportant 1件でNO-GOだった。比較専用画面に通常tabがなく、受入の戻り3箇所が到達不能だった。独立DOM反例2 FAILと正しい戻る導線1 PASSを確認し、`06c27907` で既存「← 版・改訂へ戻る」button経由へ限定補修した。製品変更なし、navigation guardのRED1→GREEN、最終純粋63/63・runtime型・collection18+5成功。元の6通信観測・逆向きpair・URL/readState/snapshot検査を保持した。
- Task 2 I1の限定再reviewはADDRESSED、Task 2 spec/品質・全feature組合せGO、新所見0。次のexact actionはGUI・試験・日本語手順を同機能1本のDraft PRへ保存して同head既存hosted CI/実受入・cleanup/artifact0を確認すること。実runtimeは未実行・未資格で、公開PRもまだ作成していない。権限・業務意味の未解決判断はない。
- 100件超の実GUI例は既存fixtureになく、今回の実資格へ含めない。DOM100+1と既存HTTP cursor試験、既存2改訂の画像なし実受入を区別する。macOS golden/画像・本番Identity/対象PC/PGプロセス再起動等の既存未資格を保持する。固定導入版 `cd6aafcc` は文書移動と今回の続き表示を含まず、GUI手順に明記した。結果だけの別PRは作らない。
