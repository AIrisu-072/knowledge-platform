# フォルダー改名：実行状況

## 2026-10-06 00:28 UTC

- PR79統合main `5d9e3c46c5b8ed1b5dd8cbe32edefb46482f787a`、branch `feat/document-folder-rename-20261006`。[小計画](../plans/2026-10-06-document-folder-rename.md)を固定し、既存APIのGUI配線を開始する
- PR79は公開head `9c1d1f98` で全適用CI・実選択親作成/HTTP再起動・cleanup・全4run公開artifact0を確認してmerge済み。main push CI `37392942272` も全13 jobs/checks・今回実runtime・DB36・cleanup・公開artifact0まで独立に成功
- source `e2a3fb4a`、runtime `114132fb`、private sidecar試験 `a2be0497` で実装を固定。全GUI708件/38 suites・GUI型/build・両runtime型・Organization純粋28件・collection2+2/18+5に成功
- 次の操作：日本語文書との組合せを独立レビューし、同じtreeでDraft公開と既存hosted全CI・実改名/HTTP再起動・cleanup・公開artifact0を確認する。実no-opと実文書folderName更新は今回runtime対象外、純粋/DOM資格と区別する。今回改名のhosted資格は未取得
- 新backend・認可推測・検証基盤・画像・移動/ACL/既読・Search作業は追加しない。既存未資格は維持する

## 2026-10-06 02:15 UTC — PR80とmainの統合資格

- [PR80](https://github.com/AIrisu-072/knowledge-platform/pull/80)の公開head `57e5d1213fbaf898b092c41a81034f2a3f856979` / tree `9a80a475bff783807446039e65805d8959166c02` は独立レビューGO。全18 checks（15成功・既存skip3）、通常CI `37399045746` 全13jobs、全4run公開artifact0を確認した
- main `1fe1b011e31477cd7a4de1b7facdcef56a97612d` は同tree・parents `[5d9e3c46,57e5d121]`。[main自身のpush CI](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37401300371)も全13jobs/checks成功、GUI708/38 suites、Document18+5、Agent9、Organization2+2/全8段階、Rust1831・既存別21/7、DB36/36、owned cleanup、公開artifact0まで新しい実ログで確認した
- 実改名の資格はGUI作成済み子QのPATCH0→1、sales同要求replay、office現在403、両HTTPサーバー再起動後の同ID/新名。Root・201件・Work・元create receiptを保持した。実GUI no-op、通信断fault、実文書folderName更新、golden/full visual等の未資格は維持する
- このsliceの統合後確認は完了。次は別branchの既存属性フィルターGUI。実サーバーへの反映は所有者の手動操作で、導入済みとは扱わない
