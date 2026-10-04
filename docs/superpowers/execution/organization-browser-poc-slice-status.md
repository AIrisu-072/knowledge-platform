# Organization Browser PoC 最小slice — 実行状況

## 2026-10-04 07:17 UTC — 最小Browser PoCの実動作検証完了

状態: **実DB・2名実browser・再起動後復元・cleanupと、exact-head通常CIがPASS**。以下の未実行・許可待ち記録は各時点の履歴として保持する。

所有者は06:56:07 UTC、GitHub ActionsのUbuntuで一時PostgreSQL・模擬2名を使い保存/提出/引継ぎ/再起動後復元を試し、画像を公開せず一時DBを削除する質問に「実行して」と回答した（質問 `Sentinel_220853533a048191b3550a8c06b873bc`、回答 `Sentinel_03cf97bd75ec8191a10be526fd032e5c`）。ローカル拒否を再試行する許可ではない。

検証済みsourceは [PR54](https://github.com/AIrisu-072/knowledge-platform/pull/54) のremote `44e1b41219a77809f82fe22045cb4fceaf0c1ed8` / tree `daafe0ad864d883f3a72db99f491b43085bfdae9`（local `357e1e2a04c42640ea7fb17319f205432c31a80b` と同内容）。

- [実runtime job111383088007](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37184350050/job/111383088007): 07:05:09 UTCにOrganizationのbuild/database/transaction/initialize/journey/restart/persistence/shutdownがすべてpassed
- 実PostgreSQLでmigration再適用、private非開示、staging失敗時のaggregate/ledger/history/event rollback、operation再送/reconnect、claim競合を確認。通常suiteでignoredの1件を別使い捨てDBへ明示実行した
- 実React/Chromiumで営業のDocument往復・未保存保持・private保存・提出確認/cancel/確定、事務の引受け・固定snapshot閲覧を確認。2つのHTTP serverを停止/再起動し、同じDBから状態/operation/snapshotが復元され、private非開示が続くことを確認した
- 所有processのgraceful終了と、所有label照合後の一時container削除を完了。cleanup失敗は最終passedを拒否する実装で、最終passed・job成功を確認。画像upload stepはskipped
- [CI37184350050](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37184350050)、[DSI37184350057](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37184350057)、[Sandbox37184350046](https://github.com/AIrisu-072/knowledge-platform/actions/runs/37184350046) はすべてSUCCESS。D2は対象branch外でskipped。Rust825件PASS/7件skip、GUI88件/18 suites、既存mock browser6件、既存Document実runtime回帰もPASS

これは承認済み最小Browser PoCの完了であり、PostgreSQL process再起動、Tauri/Windows/WebView2、Phase5/6全範囲、production資格は含まない。PR54はDraft、merge/deployなし。次のexact actionは親によるこの実証結果の報告と日本語記録の保存。追加機能・native資格は別scopeとして扱う。

2026-10-04 05:55 UTC 初回実装時点。状態: **実装・限定独立レビューGO、local最小検証PASS／実DB・browser統合は未実行**。

## 2026-10-04 06:18 UTC — 実DB・browser確認の追加sourceを準備

所有者は06:10 UTCに「解決して続けて、完了までしてください。何かできたら都度報告してください。」と指示した。初回実装commit `44fbd7d900aa56c5260efd9bd3933d91a3c32d0a` の次commitとして、既存Document CIの後段に最小Organization検証を追加する。

既存Documentのprocess・PostgreSQL readiness・停止処理を再利用し、別の一時container内にbrowser用DBとtransaction試験用DBを分離する。2名の実UI操作と同DB/storageでの2process再起動、private非開示、固定snapshot、staging失敗時rollbackを確認する。依存追加・画像/trace/video公開・Document資格条件の変更はない。

source準備のみ承認済み。hosted Organization実行の個別許可は質問中で、このcommitをpushして自動起動する操作も保留。実DB/browser結果はまだ **NOT RUN**。

追加sourceの静的確認: Organization runtime型検査PASS、Playwrightはjourney/persistence各1件の収集のみPASS、listener-free設定/終了契約3件PASS、既存GUI17 suites/74件PASS、対象Clippy・fmt・構文・差分・追加差分Gitleaks PASS。実DB試験はcompile成功、0 passed / 1 ignoredのまま。独立レビューで停止確認logの欠落を検出し、OrganizationのRuntimeとWork poolが終了した後だけ既存harnessと同じdrain確認を出す最小修正を加えた。追加13コード/設定ファイルの限定独立再レビューはGO、追加blockingなし。

### ローカル検証範囲の逸脱と停止

06:18–06:19 UTC、既存Document Node試験を純粋suiteと誤認し、`node --test tools/document-poc-runtime/test/*.test.mjs` を実行した。129件が10.612秒で成功してexit0で終了したが、そのうち次の6試験は合成HTTP/TCPのloopback listenerを実際に起動した。実行前のsource確認が不足していた。

- `harness.test.mjs`: HTTP readiness、合成TCP echoを使うdatabase proxy、paused download、100-continue JSON request
- `response-loss.test.mjs`: 合成HTTP upstreamの応答喪失、no-dispatch/stalled-upstreamの終了確認

いずれも試験専用の合成listenerであり、実PostgreSQL、Organization server、Chromium、本番接続は起動していない。試験sourceのfinally/afterでcloseを待機し、応答喪失のchildではTCPServerWrapが0であることも確認している。親runnerのexit0と、その後の該当試験process検索で残存なしを確認した。全system socketの不存在までは主張しない。

この事実は直ちに親へ報告して追加runtimeを停止した。129件の成功はPostgreSQL EPERMの解消証拠でも、別経路を使う許可でもない。以後は内容確認済みのlistener-free純粋試験・型/構文検査のみ実行する。保存した試験出力13396 bytesのSHA256は `6c878e854aeab0f846c4105d1898a33aaae3c49d9ba12bf8d689d32798488340`。

## 承認と基点

所有者は2026-10-04 05:20:11 UTC、Tauri実機検証を後に回し、ブラウザー版で「模擬ユーザー2名によるタスク一覧→文書参照→提出→次担当への引継ぎ」を先行する質問に「先行してください。許可します」と回答した。質問 `Sentinel_270b5242718c8191affc55be5d5a011c`、回答 `Sentinel_239f81edcca88191a9ab7aae859f8a85`。

基点は凍結PR49 `0860e34ebd2c2353f6528423a6ebf52cfa70751d`。Phase1/2/3設計本文のblobはそのまま。承認済みpreview修正 `b4c221a6` の9 source/configファイルを再利用し、Tauri fixture/guard/sourceは含めない。branchは `feat/organization-poc-slice`。push/PR/merge/deployはこの作業では行っていない。

## 完了した実装

- 固定 `organization-synthetic/sales-01` と `office-01`。リクエストはprincipalを切り替えない
- Reactの `/tasks`、context/queueの2投影、private文案保存、共有Document往復、提出確認、snapshot、次担当claim
- Work domain/application/PostgreSQL/HTTPとOrganization composition。Work専用schema・別migration ledger、OCC・operation/digest照合、提出・次task・history・必須stagingの単一transaction
- 既存Document routerは信頼済みidentityをcompositionで注入して再利用。Document既存2profile・API・ACL・migrationを変更しない
- 最小OpenAPI3.2.1 subsetと、選定済みgeneratorによるTypeScript型。transportは境界内に限定
- 結果不明は同一ID/同一payloadで確認・回復。operation未登録404で文案やIDを捨てない

操作・起動手順は [Browser PoC運用](../../operations/organization-browser-poc.md)、範囲は [最小計画](../plans/2026-10-04-organization-browser-poc-slice.md)。

## 独立レビュー

最初のsourceレビュー: Criticalなし、Important2件。

1. operation recoveryがworkflowとoutcomeを別時点で読む競合 → 単一SQL JOINへ修正
2. recovery404をtask認可失効と混同してID/文案を破棄 → 元commandを保持し、再確認・同一再送へ修正

限定再レビューはGO、追加blockingなし。修正した5ファイルのSHA256を別レビューと照合した。レビューはsourceのみで、実DB/browserの資格取得を代替しない。

## 実行した検証

- Organization/Work Rust: **21 PASS**。DB transaction試験 **1件は明示ignored／未実行**
- 既存Document config/identity回帰: **5 PASS**。新composition純粋試験: **1 PASS**
- GUI: **17 suites / 74 PASS**。既存Document57件を保持
- TypeScript・生成済みSearch schema freshness・production Webpack build: **PASS**。既存bundle-size/runtimeChunk advisory **3件**あり
- OpenAPI lint: **PASS、警告なし**。architecture-lint、cargo fmt、対象全target Clippy `-D warnings`、diff check: **PASS**
- Rust registry dependency521件のname/version/source/checksum集合は基点と完全一致。新規workspace packageのみ追加。pnpmはレビュー済みlock SHA256 `fbe0cb60b77539e53fb3e9236af728c6fe4b70a4680d6d45441a9490c04596e8` のまま
- 固定pnpm offline/frozen installは687件再利用、download0、supply-chain policy維持。初回のcache-path不足と親shellのPATH不足はenvironment prerequisite failureとして残し、正しい既存cache/PATHで再実行した
- 公開152262b5の承認済みGitleaks4 fingerprintを同期。既存31件・rules・対象commit/path/lineはそのまま、新しい除外なし

## 保留と次のexact action

実PostgreSQLのrollback/競合/restart、listener起動、browser実操作、実Document組合せは**未実行**。既知のsocket/browser拒否を再試行したり、別経路へ迂回したりしていない。実DB試験は使い捨てDBでの実行権限が整ってから行う。local純粋テストだけで「画面から一連の操作を実証済み」とはしない。

次のexact actionは、親がこの差分のexact commit/treeとローカル検証限界を確認し、許可されたDraft公開範囲で引き渡すこと。その後、許可された実行環境の条件を満たした時に運用手順の2名実操作と実DB試験を実施する。Tauri Phase4の資格取得やPhase5/6全体の完了を宣言しない。

## 2026-10-04 06:29 UTC — PR54通常CIのproduction CSS検査を修正

親が日本語Draft [PR54](https://github.com/AIrisu-072/knowledge-platform/pull/54) を公開した。remote `92584f5ade09ecee3c13d153069acd2d4edc0d74` / tree `1026ff75263de0b602ec71233d862581d018af49` は初回local実装 `44fbd7d900aa56c5260efd9bd3933d91a3c32d0a` と同じ内容。

既存Document CI job `111378164718` はJest74件とproduction buildが成功し、既存mock browser6件中1件のcontrast検査で失敗した。productionのcssnanoが `#ffffff` を等価な `#fff` に短縮する一方、既存試験parserが2桁ずつしか読まず、1channelからNaNを算出していた。生成CSSと試験の純粋な再計算で原因を確認した。正しい3桁展開では7組すべて4.5以上であり、製品のpalette・閾値・minify設定は変更しない。

この修正は試験の色解析のみ。承認待ちのOrganization専用実DB/browser CI追加とは別commit・別worktreeで準備した。新しい実DB/browser実行結果はまだなく、Document jobもmock preview検証の時点で停止して実compositionまで進んでいない。次は限定レビュー・純粋回帰を通ったexact sourceを親がPR54へ反映し、新headの通常CIを確認する。

修正後のlistener-free検証は、旧parser反例14件RED→新helper14件GREEN、GUI18 suites/88件、schema freshness、型検査、production build、既存Playwright6件の収集、差分検査がPASS。生成CSS実bytesを同じhelperで計算した7組は5.0486〜15.9247。実browser再実行は次のhosted headを待つ。pnpm実行時にregistry metadataの再検証と既存modules metadata時刻更新があったが、package download/addはなく、lock・supply-chain設定は不変。以後は固定Nodeから既存toolを直接実行した。

コード3ファイルの限定独立レビューはGO。短縮/長形式等価、黒白21、同色1、低contrast、不正入力の両引数拒否を別の純粋実行でも確認した。Critical/Importantの追加指摘なし。
