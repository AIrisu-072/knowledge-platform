# 文書属性編集と公開状態操作のmain統合状況

## 2026-10-05 05:55 UTC — 公開後の受入read用途を修正

- 公開PR71 `5ed279fac43c187421f626a24aec85a97b023a6c` / tree `7091ac4f31fdbb230fef4fcf0cbe269326a83eaa` の[CI37268915967](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37268915967)、runtime job `111631545451` はjourney14件成功/metadata1件失敗。属性入力のexact label問題は先へ進み、取消・未公開metadata保存の確認後、`gui-metadata-working-verified` を最後に `DOCUMENT_VERSION_NOT_FOUND` で停止した。agent・再起動・persistence・Organizationは未実行。最初のPR71失敗と今回の失敗をともに保持する
- 受入specの `version()` が `purpose: authoring` 固定で、公開後にも同じhelperを呼んでいた。既存repositoryの `document_history.rs` はauthoringをWORKINGに限定し、公開済み版のauthoring readを404にする。公開失敗と断定せず、公開後のread契約との不整合を修正する
- `detail(view)` / `version(purpose)` を既定値のない必須引数にし、公開前はauthoring、公開後はpublishedを明示する。公開後と再起動後のGUIもpublishedへ明示遷移する。既存のpublished/history snapshot、原本・Version・read-state・metadata・正式改訂・no-opの検査は保持する
- 純粋source契約3件でhelper引数・公開境界前後の呼出し・GUI用途をRED3→GREEN3で確認。これはTypeScript ASTと既存repository sourceとの対応を検査するもので、Rust/API/browserを実行した証拠ではない
- 最終検証：全GUI315件/24 suites、application/runtime型、schema freshness、production build、純粋runner34件、collection-only journey15件/persistence3件がPASS。Webpack advisory3件は継続
- 作者以外の主担当による独立限定レビューGO、Critical/Importantなし。新source契約3件とdiff検査を別途実行してPASS。review対象spec blob `7e97e7ab2bc3a8503b7fd1195a9cf3f74a3c0509`、test blob `dffba9c518a444ed019d32c4c31c7b98197d9956`
- 製品source、backend/API/認可、既存runtime locator/timeout/retry、有限診断、依存/lock/workflowは不変。ローカルDB/socket/listener/browser/Cargoは実行していない

次の操作：公開5edを祖先に保持する小さい差分を親へ渡し、新exact headの既存hosted受入と全CIを確認する。今回の純粋成功を実DB/browser・再起動・cleanup・artifact公開0の成功へ読み替えない。

以下は統合候補の履歴。

---

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
