# 文書属性編集と公開状態操作のmain統合状況

## 2026-10-05 05:06 UTC — 両公開履歴を保持した統合候補

- 第一parent：公開PR71 `8d53d7a3931b0e23f7929b85eb6846213985c71e` / tree `f728e6008ad0b892c5caaeab8d46db64f3a35e82`
- 第二parent：PR70統合済みmain `5d262557f1d59ab10db2eeb5d808c19382b2414a` / tree `f6f1db4db052b7e3d152365780f59808840edb8c`。mainの第二parentは公開PR70 `2d2cd3d66d54137b2f3336ff98243b77fe834da5`
- branch：`integrate/document-metadata-main-20261005`。元のmetadata枝・公開履歴を残し、今回の統合を別worktreeで準備する
- 主担当からの両公開履歴統合指示に従う。PR70の受入を新しい組合せへ付け替えず、PR71初回実runtimeの失敗も[属性編集状況](document-metadata-editor-status.md)に保持する

## 合流内容と確認範囲

文書の初回登録、概要の3属性編集、版一覧の現行版取下げ・公開終了を加算的に保持する。新しい業務状態・API・認可・予約取消・WORKING更新/rebaseは追加しない。

1. 競合はDetailの両import、runtimeのtestMatch、有限診断のsource/stage/stack許可リスト、双方の診断試験、Activeの先頭記録を和集合にする。APIの両import/adapterは自動mergeの結果を確認する
2. metadata初回実受入で失敗したexact labelを、[独立確認済みの小さい修正](document-metadata-editor-status.md)で解消する。3項目と理由の可視ラベルに一致するaria-labelのみを追加し、既存runtimeのlocator/timeout/retryは変更しない
3. metadataの縮小Version fixtureへ、実APIで必須のwithdraw capability（disabled）を1行追加する。初回統合GUI315件中29件はこのfixture欠落で失敗した。製品の取下げ判定を弱めて回避しない
4. lifecycleの既存配線試験がpersistence2specだけを期待していた不整合をRED1で確認し、metadataを含む正確な3spec配列へ揃える。任意specを許す正規表現へ緩めない

両操作の一時状態は独立したcacheを使い、各mutationのpayload固定・現在認可・取消/再表示・遷移・遅延応答の境界を維持する。metadataとlifecycleの実受入は別の合成文書と別sidecarを使う。新規3specの画像/trace/videoはoff、既存private raw診断と有限の外部要約を区別し、追加値を外部artifactへ公開しない。

## 検証

- 両GUI focused54件PASS。全GUI315件/24 suites PASS（label回帰2件を含む）
- application/runtime型、schema freshness、production build、既存MCP受入bundle build PASS。Webpack advisory3件は継続
- 純粋runner31件PASS。合流した診断18件と既存要約/配線の確認を含む
- collection-only：journey15件/7files、persistence3件/3files。実browserを起動せず収集だけを確認
- 独立共存レビュー：GO、未解消Critical/Importantなし。GUI/API56件・診断18件・修正配線1件・collection15+3を独立実行してPASS。main専用12path中11pathのbyte一致と配線試験1行だけの整合変更、metadata sourceの加算的保持、操作cache分離、別sidecar、recording offと有限診断を確認した
- backend/API schema/migration/lock/依存/workflowと停止中Search/Audit/Toolboxは変更しない。予約取消の別作業やWORKING未解決仕様は取り込まない

次の操作：独立レビュー後、両public parentを持つ正確なcommit/treeとpacketを親へ渡す。親がPR71を更新し、新exact headの全CIと同じhosted PostgreSQL18.6/固定合成2profiles/Chromium、両HTTP再起動、所有cleanup、外部artifact公開0を確認する。ローカルDB/socket/listener/browserは未実行で、新しい組合せの実受入成功はまだ主張しない。実サーバー反映は所有者が手動で行う。
