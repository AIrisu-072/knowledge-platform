# 文書属性編集と公開状態操作のmain統合状況

## 2026-10-05 UTC — 属性編集の実受入完了と予約取消mainとの統合

### 統合前の確定資格

公開PR71 `8eaa3942984c240678a8b2a018557575a8fbe91c` / tree `ea7b1336ed0a0e4f8848afb8e79570d421abe7c0` は、[CI37270156571](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37270156571)の全13jobs、[DSI37270156572](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37270156572)、[Sandbox37270156595](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37270156595)が終端SUCCESS。Document実journey15件/再起動後3件、Organization、所有cleanup、外部artifact公開0を主担当が確認した。以下の2回の失敗記録を残し、この成功は8eaaの資格に限定する。

主担当がこの終端を確認した後、公開[PR72](https://github.com/AIrisu-072/knowledge-platform/pull/72)統合済みmain `c4388433e97e239057921f2412d1ac3e00900de4` / tree `e5515dbc66c6c2d8e889c696a8147faba9a683f0` と両public履歴を保持して合流する。branchは `integrate/document-metadata-schedule-20261005`、第一parentは8eaa、第二parentはc438とする。元branchと失敗証拠を保持する。

### 合流の範囲

- 概要の属性編集、版タブの予約取消、既存の現行版取下げ・公開終了と初回登録を保持する。3操作の一時状態・固定payload再送・現在認可・遷移guardを混ぜない
- currentPublicationScheduleIdを含むbackend/read/API schema/生成型は受入済み取消mainのbytesをそのまま保持する。この統合ではbackendの追加変更・新しい認可・WORKING更新/rebaseを行わない
- Detail配線、受入testMatch、有限診断のsource/stage/stack許可リスト、双方の診断試験、Active記録を和集合にする。metadata/取消/lifecycleの専用受入と別sidecar、画像/trace/video offを維持する
- 配線試験の旧3spec期待はRED1後、persistenceの正確な4spec配列へ更新した。metadataの縮小Version fixtureにはrequired nullable currentPublicationScheduleId:nullを明示した。製品のguardやassertionを弱めて回避しない

### 新しい組合せの純粋検証

- 全GUI338件/25 suites、application/runtime型、schema freshness、production build、MCP受入bundle buildがPASS。Webpack advisory3件は継続
- 純粋runner36件、API契約16件PASS。API契約の初回試行は既定telemetryを伴うため確認を中断し、成功証拠には数えない。公式 `REDOCLY_TELEMETRY=off` を子processまで継承した別試行で16件を確認した。安全検査・registry・依存/lockを変更せず、telemetryの送信許可を広げていない
- collection-only：journey16件/8files、persistence4件/4files。実browserは起動していない
- 作者以外の主担当による独立共存レビューGO、Critical/Importantなし。metadata/取消/lifecycle/APIの79件、有限診断/read用途/配線の32件、diff検査を独立実行してPASS。Detail blob `6bd28d1e4fa612ce1c65456611382824dba53c10`、runtime config `872966f4905ba6c5bea648b4140b3d0148534882`、診断 `1aac0c96cf946b9edd5d235a10ac97276b1facb5`、配線試験 `8ad63ce50d0d1fa11cddcab1c61aa5ac20ccc70f`
- 既存のmetadata/取消/lifecycle component/application/runtime source、受入済みmainのbackend/schema/生成型、workflow/lockをbyte照合した。ローカルDB/socket/listener/browser/Cargoは実行していない

次の操作：両public parentを持つ正確なcommit/treeとpacketを親へ渡す。親が同PR71を更新し、新exact headの全CIと既存hosted実journey16件/再起動後4件、Organization、所有cleanup、外部artifact公開0を再確認する。統合前8eaaの実受入成功を新しい組合せの資格へ付け替えない。実サーバー反映は所有者が手動で行う。

以下はそれぞれの候補時点の履歴。

---

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
