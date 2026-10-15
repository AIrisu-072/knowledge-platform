# 文書作成日時範囲：実行状況

## 2026-10-06 05:58 UTC

- 基点main `dc04ba4af896fd8c5f2e23438cf264e054341f81`、branch `feat/document-created-date-range-20261006`。[小計画](../plans/2026-10-06-document-created-range.md)で既存日時入力・JST helper・GETだけの配線を固定する。製品実装と今回資格はまだ未取得
- 未読PR82は公開 `c8eb4800` / tree `1f05f2bf` で全18checks（15成功/既存skip3）、Document18+5・GUI828/39・Organization・cleanup・全4run artifact0成功。main `ed72abeb` 自身のCI `37418384758` も全13jobs・同実受入・指定DB36・artifact0成功。旧a218のpersistence失敗は未再現で原因未特定のまま。失敗優先診断は上限/公開項目を維持した純粋41件の資格であり、製品障害修正とは扱わない
- 手動導入pinのPR83は `41a848f9` / tree `d7f98cff` で全15checks（13成功/既存skip2）、CI `37418943211` 全13jobs・Document18+5・Organization・DB36・cleanup・適用2run artifact0成功。4文書のみをmain dc04へ統合済み。固定導入対象は資格済み0801のままである。旧248c0bf2のOrganization503原因は未特定で、新headの成功へ付け替えない
- main dc04自身のpush CI `37420977657` も全13jobs/checks成功・failure/skip0・artifact0。今回mainの別ログでGUI828/39、Document18+5、Organization build/2+2、Agent9/provenance・再起動・cleanup・指定DB36/36を確認した。日時機能の資格へ付け替えない
- 06:26 UTC追補：製品source `787c8817` は初期20RED、A→B→Aの旧draft復活1REDから補正し、同HEADで全GUI870/39・schema/type/build成功。独立spec/品質レビューGO、対象183件の独立再実行成功。既存webpack警告3件を保持した
- 受入source `3f586d43` は既存metadata-editorの2case内で、通常カレンダーの実GET、元createdAtの開始包含・終了除外、精密原文/正確に可逆な分表示、詳細往復・HTTP再起動を検査する構成。既存3gotoのreturnToを試験seedに使い、helper用途・goto回数・fixtureを増やしていない。実createdAtの表記が偶然分入力へ完全可逆でも正しいUIを検査し、readonly必須とは推測しない
- 同受入HEADでruntime型、用途guard3・純粋診断37、MCP compile、collection18+5が成功。実browser/DB/通信の資格はまだ未取得であり、原文を取得済みとするsource上のassertionだけで合格とはしない
- 次の操作：この製品・受入・日本語文書の組合せを独立最終レビューし、同じ機能のDraft PRで既存hosted全CI・実受入・再起動・cleanup・artifact0を確認する。公開後の結果を同じPRへ記録し、結果だけの別PRは作らない
