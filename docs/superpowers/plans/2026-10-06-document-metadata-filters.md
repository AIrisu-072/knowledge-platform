# 文書一覧の属性3項目による絞り込み

## 目的と承認範囲

既存内部処理を通常GUIから使いたいという所有者の指示（`Sentinel_cef3cbfde698819198d9ed687212a610`）に沿い、既存listDocumentsのdocumentType / owningDepartment / categoryを通常一覧へ接続する。基点はPR80統合main `1fe1b011e31477cd7a4de1b7facdcef56a97612d`。既存Frozen管理設計§11の属性絞り込み、Frozen GUI§19.2のURL正本と§25の一覧・詳細往復を再利用する。新backend・認可・検索基盤・属性マスタは作らない。

## 小さい追加設計

- 一覧の「文書種別」「所管部署」「カテゴリ」の3入力を、既存タイトルと同じ明示「絞り込む」で適用する。3属性の解除はタイトル・folder・view・sort・pageSizeを保持する。入力中は新条件GETを発行しない
- 条件はANDの完全一致。空文字だけ未指定として省略し、空白・大文字小文字・Unicodeをtrim/NFCせず保持する。backendの既存契約は非空、1024 UTF-8 bytes以内、制御文字なし。新しいfacet/候補一覧・未知属性推測・欠損/空値専用フィルターは追加しない
- ListSearch/schemaと生成validator、query key、SDK queryを一対一に接続する。条件適用/解除で旧cursorを破棄し、page/view/folder/sortの既存操作、ブラウザー戻る/進む、詳細往復に3条件を保持する。APIの認可済み集合とcursor認可をそのまま使い、クライアント側だけで行を除外しない
- フォームは不正値を理由付きで止めて入力を保持する。GET失敗と0件を区別し、別条件の遅延応答で現在の結果を置き換えない。create/rename/metadata等の未解決mutation storeを変更/clearしない
- 詳細の戻り先は既存の同originかつ正確な/documentsパス制限を維持する。router自身のdefaultParseSearchで復号し、JSONらしい文字列の引用を値へ混ぜない。URIエンコードされたreturnToの有限長だけ81920文字（80 KiB相当）へ拡張する。旧title/cursorのschema許容文字列と新3属性のpercent/JSON引用展開の有限上界を含め、外部URL・別パス・上限超を受理しない。非string/孤立surrogateの属性URLは日本語の限定error面でGETを止め、同アプリ内の解除Linkで戻る。不正returnTo内の属性もcatchで未指定へ落とさず理由付きで止める。新しいURL/状態管理基盤は作らない

## 実装と検証

1. 実route/URL/APIの反例をREDで確認し、3項目と限定した詳細復帰補修を実装する。完全一致のspace/非NFC/JSON-like/特殊文字、UTF-8境界/制御文字、空欄省略、cursorリセット、他条件/選択保持、往復/履歴、エラー/遅延応答を確認する。全GUI708件を保持し、型/schema/buildを検証する
2. 既存metadata-editorのjourney/persistenceの中へread-only確認だけを足す。初回3属性での一致/1条件不一致/解除、再起動後の空白属性一致・削除category不一致、通常画面からの操作と詳細往復を確認する。元metadata変更・原本・正式Minor/no-op・private sidecarの証拠を変えない。新fixture/case/sidecar/runnerを作らない
3. 独立レビュー、日本語Draft、同一headの既存hosted全CI、HTTP再起動、owned cleanup、公開artifact0で資格化する。main mergeは親、実サーバー反映は所有者の手動操作

## 今回の対象外と既存限界

Folder移動は配下のWrite/Share認可・継承影響と移動先の現在認可の境界が広いため別slice。日時・未読条件のGUIも今回は含めない。移動/ACL/既読記録、新backend、Search/Audit/Toolbox作業は追加しない。GUI内のsearch-stateは文書URL条件であり、独立Search機能の変更ではない。

実通信は既存hostedの使い捨てPostgreSQL18.6・固定2名・Chromiumだけ。ローカルlistener/socket/browser/DB/Cargo・画像保存を行わず、120秒/test・retries0・画像/trace/video off・既存有限診断とcleanupを維持する。golden/full visual・全headers喪失・対象PC導入/backup/restore/PostgreSQLプロセス再起動・本番認証の未資格は維持する。
