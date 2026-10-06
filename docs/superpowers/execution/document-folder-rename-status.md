# フォルダー改名：実行状況

## 2026-10-06 00:28 UTC

- PR79統合main `5d9e3c46c5b8ed1b5dd8cbe32edefb46482f787a`、branch `feat/document-folder-rename-20261006`。[小計画](../plans/2026-10-06-document-folder-rename.md)を固定し、既存APIのGUI配線を開始する
- PR79は公開head `9c1d1f98` で全適用CI・実選択親作成/HTTP再起動・cleanup・全4run公開artifact0を確認してmerge済み。main push CI `37392942272` も全13 jobs/checks・今回実runtime・DB36・cleanup・公開artifact0まで独立に成功
- source `e2a3fb4a`、runtime `114132fb`、private sidecar試験 `a2be0497` で実装を固定。全GUI708件/38 suites・GUI型/build・両runtime型・Organization純粋28件・collection2+2/18+5に成功
- 次の操作：日本語文書との組合せを独立レビューし、同じtreeでDraft公開と既存hosted全CI・実改名/HTTP再起動・cleanup・公開artifact0を確認する。実no-opと実文書folderName更新は今回runtime対象外、純粋/DOM資格と区別する。今回改名のhosted資格は未取得
- 新backend・認可推測・検証基盤・画像・移動/ACL/既読・Search作業は追加しない。既存未資格は維持する
