# フォルダー一覧の続き表示

既存APIを通常GUIから使えるようにする所有者の指示に従い、PR76統合main `ce8ed4f15dd564235c4f68a406eec42fd8ca91d4` を基点とする。既存 `GET /v1/folders/{folderId}/children` のopaque cursorを使い、先頭200件で終わるフォルダーツリーを「さらに表示」で継続できるようにする。新backend・権限判断・依存・検証基盤は追加しない。

## 固定する範囲

- 展開した各フォルダーの現在の子一覧だけを対象とする。初回200件とnextCursorを読み、明示操作で次を取得する。cursorを復号・自作せず、選択親を別の親へ取り違えない
- 取得済み行・選択を保ち、読取中の連打で重複取得しない。閉じる/再表示/画面往復、途中の失敗、明示的な最初からの再読取を扱う。再読取は確認されていない書込操作を消さない
- 登録先capability用の既存通常queryとページ列のcache形式を混ぜない。既存QueryClient/現在認可・cursor束縛・型付きclientを使う。無効化の既存prefixを維持し、Root作成後も一覧を再取得できるようにする
- 親のcapabilityを子の認可として使わず、件数や内容から操作可否を推測しない。ページを跨ぐ変更があり得ることを踏まえ、重複表示や見かけの全件保証を避ける。エラーを空一覧や完了へ置き換えない
- 非root配下の作成・改名・移動・ACL・既読は今回追加しない。停止中Search作業にも触れない

## 実装手順

1. 現行FolderNodeとquery利用箇所を確認し、続き表示/二重取得/選択保持/閉再表示/エラー/無効cursor/再読取/cache分離の純粋DOM反例をRED→GREENにする。既存queryとCSSを使う最小変更に限る
2. 既存の画像なしOrganization受入で、Root作成済みフォルダーの配下に合成201件を既存APIから準備し、通常ナビで展開→初回200件→続き1件→選択と、既存HTTP再起動後の表示を確認する。既存Root/Workの不変条件を壊さず、既存fixture/helperを最小再利用する。新runner/proxyや画像を作らない
3. 全GUI・型/build、runtime純粋・型・collection、source保持と独立レビューを確認し、日本語Draftを公開する。同一headの通常CI・実受入・owned cleanup・公開artifact0を確認する。main mergeは親担当、実サーバー反映は所有者の手動操作

ローカルlistener/socket/browser/DB/Cargoを起動しない。実通信は既存hostedの使い捨てPostgreSQL18.6・固定2名・Chromiumだけ。golden/skip/timeoutを緩めず、未実行の画像比較や通信断の資格を拡張しない。
