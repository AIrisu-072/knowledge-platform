<a id="search--discovery-platform-v0--phase-d-受入証拠"></a>
# Search / Discoveryプラットフォーム v0 — Phase D 受入証拠

この文書は[公開原文（固定版）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/execution/search-discovery-platform-v0-acceptance.md)の意味保存訳です。原設計の再承認や資格の追加ではありません。既存のハッシュは当時の原文・証拠のものであり、訳文のハッシュではありません。以下の状態と「次の作業」は当時の記録であり、現在の実行指示ではありません。

<a id="判定境界"></a>
## 判定境界

承認済み設計仕様 `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md` とPhase D計画 `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-d-integration.md` のD1〜D9を対象とする。実装ブランチは `feat/search-discovery-platform-v0-d`、確認済みPhase C head `a8df10d7582f9d2947b0b58cfbd4713281fca912` の直上である。D8のコードheadは `5480ea4d48a737b520db7ceacac84ffb9fad2744`、ブランチ全体のレビュー修正後のコードheadは `4578a62d16aa8ef726892b9627678179922ec430` である。

この受入はリポジトリ内のDocument Source/Search実行環境と契約を対象にする。Draft PRのマージ、サービスのデプロイ、汎用outbox配送ワーカー、公開通信層/API、本番のID・秘密情報・運用接続は別の操作であり、この記録だけでは実施済みにならない。

<a id="実装済みの範囲"></a>
## 実装済みの範囲

| タスク | コミット | 実装・証拠境界 |
| --- | --- | --- |
| D1 | `3e4566d` | DocumentのライフサイクルをLive/Historical/Authoringへ型付き変換。字句検索の対象はタイトルとLensで許可されたメタデータだけで、本文と未選択メタデータを入れない。 |
| D2 | `e4cc452` | 既存Document認可へ委譲する現在のアクセス確認。`Read`と`ReadHistory`を分離し、期限切れactorをリポジトリ呼出し前に拒否する。 |
| D3 | `45177db` | PostgreSQLの一つの `REPEATABLE READ, READ ONLY` スナップショットでDocument、Version、T10、アクセス改訂番号、表現、DSI状態を読む。 |
| D4 | `f875324` | Domainイベントをトリガーとして正本を再読する冪等なコンシューマー。汎用 `outbox_events.delivered_at`を所有せず、同一実行環境がProjectionと字句検索成果物を所有する。 |
| D5 | `25a913b` | 正本で確認できるDocument–Versionと現在のDocument/Version/Folderの型付きn項関係だけを投影する。推測した関係は作らない。 |
| D6 | `99acfa7` | 取消直後の呼出し時再認可、評価中のgeneration pin、古いトリガーからの最新スナップショット読取り、Source停止時の旧generation保持を実DB経路で固定する。 |
| D7 | `b46865b`, `4578a62` | 7カテゴリの合成評価と段階別の要因特定。誤った複合適合、厳格条件の誤受入、誤拒否、coverageの欠如、Graphパス内の未認可参加者の露出を別集計する。 |
| D8 | `5480ea4` | Version、Documentに範囲を限定したDocument/FolderPlacement、Tantivy、型付きn項Graphを同一generationで構築し、Projection pointerのCASを最後に実行する。正本のclaim証拠、明示IDの履歴参照、本文coverageの事前確認を接続する。 |

D8のGraph補助Resourceは `(Source, Document, Folder)` に範囲を限定し、補助ResourceIdから所有Documentへの対応をgeneration別に保持する。全Graph参加者は現在のDocument `Read`を再確認するため、同じFolderにある別Documentの権限を流用しない。失敗/CAS敗北のgenerationはGraph、字句検索、Projectionをすべて破棄する。公開成功後に証拠記録の保存だけ失敗した場合は公開成果物を保持し、同じイベントの再試行が `Unchanged` と証拠記録の保存へ収束する。

<a id="local-verification"></a>
## ローカル検証

- `mise run eval:search`: **24/24 PASS**。出力は合成シナリオのID、カテゴリ、段階別件数だけで、顧客コンテンツを含まない。
- D8の実PostgreSQL縦断テスト: **5/5 PASS**。Graphのみの再現率、厳格なゲート後の2候補のS1順序、空でないPrimary証拠のSufficient、取消後の全結果面の秘匿処理、同一Folderの認可分離、T10/履歴、本文の `UnsupportedCoverage`を確認した。
- D8統合後の `search-source-document`: 最終のテスト専用追加前に **51/51 PASS**。Core/Projection/Graph/Tantivyの関連回帰は **102/102 PASS**。後始末強化後と証拠記録保存失敗の回帰は、最終編集後にそれぞれ対象を限定して **1/1 PASS**。
- 変更crateのstrict Clippy、`cargo fmt --all -- --check`、アーキテクチャ、ロック済みメタデータ、`git diff --check` はPASS。D1〜D7の各タスクも対象限定RED/GREENと独立した読み取り専用レビューを通過した。
- 最終 `mise run verify` は、秘密情報、依存関係/セキュリティ、ワークフロー、fmt、ワークスペースの `cargo check --locked`、ワークスペースのstrict Clippy、アーキテクチャ、APIをPASSした。Rustワークスペーステストはリンク中に空き容量が約300 MiBとなり、`search-graph-memory` のテストバイナリで `errno=28` (`No space left on device`) により停止した。これは **ローカル標準ゲートのGREENではなく、Searchアサーションの失敗でもない**。再構築可能な当該worktreeの `target` は診断保存後に削除した。
- ブランチ全体の独立レビューは、Graphパス内の未認可参加者をD7セキュリティ指標が数えないP2を1件検出した。n項関係の参加者と `resource_path` メンバーの2ケースをREDで再現し、重複除去した露出集計へ修正した。対象限定2/2、全評価24/24、strict Clippy/fmt/diffがPASSし、別の読み取り専用レビュアーがGOを返した。D1〜D8の本番経路に追加P1/P2はなかった。

<a id="coveragehistoryevidenceの境界"></a>
## 対応範囲・履歴・証拠の境界

`DocumentCoveragePreflight::discover` がDocumentアダプターの型付き入口である。`TitleAndPermittedMetadata`だけが通常の `DiscoveryService` を実行し、`BodyRequired` は検索器を呼ばず、処理を阻止する `UnsupportedCoverage` と空の適合結果を返す。D8縦断テストは、実際のタイトルに一致するリクエストでも字句検索ポートが呼ばれないことを固定する。公開通信層/APIは未実装なので、外部リクエストからcoverage種別を決める接続はまだ存在しない。

履歴参照はLive検索の代替経路ではない。`DocumentHistoricalLookup` は明示されたDocument IDとVersion IDを既存 `DocumentHistoryService` へ渡し、`Read`と`ReadHistory`の両方を要求する。T10は公開終了台帳を含む正本スナップショットから判定し、`current_version_id = NULL` だけから推測しない。

Primary証拠へ昇格できるのは、同じgeneration/resourceに束縛した正本のタイトル・許可メタデータのassertionだけである。Graphパス、DSIの不透明参照、一般パス文字列は検索traceには残せるが、Primary claim証拠にはしない。

<a id="hosted-verification"></a>
## ホスト環境での検証

Phase DのbaseであるPhase C Draft PR #28 head `a8df10d7582f9d2947b0b58cfbd4713281fca912` は、CI `36580213345`、DSI Sandbox `36580213402`、DSI PoC `36580213444` がSUCCESSしている。これはPhase D headのホスト環境での証拠ではない。

2026-09-30に実際のGitHub状態を再確認した。Draft PR #33はOPEN、baseはPhase Cであり、head `4892ba5d2736b35bf95de25f834f016609d2e0d4` に対する [標準CI 36665497017](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36665497017)、[DSI PoC 36665497015](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36665497015)、[DSI Sandbox 36665497041](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36665497041) は全てcompleted/SUCCESSである。Phase Dのホスト環境での適合確認は **PASS**。この完了記録はCompletion Programブランチ上に置き、検証済みのPR #33 headを動かさない。

<a id="deferred-capabilities"></a>
## 後続に繰り延べた機能

- 全文Search Extractionと本文の索引化
- vector/embedding、スコア較正、再ランキング
- 永続化された専用Graphバックエンド
- リモートプロバイダーアダプターとリモート実体化の通信層
- 公開Search/Discovery通信層/API
- 汎用の永続outbox配送ワーカーと本番スケジューラーの接続
- 本番デプロイ、ID/秘密情報の接続、運用SLO・負荷容量の認定

当該Graph/Projectionの参照実行環境、合成評価、HTTP未接続の型付きアダプターを、上記の繰り延べた機能や配備済みサービスとして扱わない。

<a id="受入状態"></a>
## 受入状態

ローカルの対象限定/評価証拠は **PASS**。ローカル標準ゲートは容量上の阻害要因のため **INCOMPLETE**。ブランチ全体の独立レビューは修正後 **GO**。Phase Dの正確なheadに対するホスト環境ゲートは **PASS**（`4892ba5`、上記3実行）。Draft PR #33はOPENであり、マージ/デプロイは未実施。
