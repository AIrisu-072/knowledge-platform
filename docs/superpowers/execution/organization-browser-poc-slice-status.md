# Organization Browser PoC 最小slice — 実行状況

2026-10-04 05:55 UTC。状態: **実装・限定独立レビューGO、local最小検証PASS／実DB・browser統合は未実行**。

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
