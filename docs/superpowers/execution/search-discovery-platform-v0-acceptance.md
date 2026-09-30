# Search / Discovery Platform v0 — Phase D 受入証拠

## 判定境界

承認済みDesign Spec `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md` とPhase D計画 `docs/superpowers/plans/2026-09-28-search-discovery-platform-v0-phase-d-integration.md` のD1〜D9を対象とする。実装branchは `feat/search-discovery-platform-v0-d`、確認済みPhase C head `a8df10d7582f9d2947b0b58cfbd4713281fca912` の直上である。D8 code headは `5480ea4d48a737b520db7ceacac84ffb9fad2744`、whole-branch review repair後のcode headは `4578a62d16aa8ef726892b9627678179922ec430` である。

この受入はrepository内のDocument Source/Search runtimeと契約を対象にする。Draft PRのmerge、service deployment、generic outbox配送worker、公開transport/API、本番identity・secret・運用接続は別の操作であり、この記録だけでは実施済みにならない。

## 実装済みの範囲

| Task | Commit | 実装・証拠境界 |
| --- | --- | --- |
| D1 | `3e4566d` | Document lifecycleをLive/Historical/Authoringへ型付き変換。lexical対象はtitleとLensで許可されたmetadataだけで、本文と未選択metadataを入れない。 |
| D2 | `e4cc452` | 既存Document認可へ委譲するcurrent access check。`Read`と`ReadHistory`を分離し、期限切れactorをrepository呼出し前に拒否する。 |
| D3 | `45177db` | PostgreSQLの一つの `REPEATABLE READ, READ ONLY` snapshotでDocument、Version、T10、access revision、representation、DSI stateを読む。 |
| D4 | `f875324` | Domain eventをtriggerとして正本を再読するidempotent consumer。generic `outbox_events.delivered_at`を所有せず、同一runtimeがProjectionとlexical artifactを所有する。 |
| D5 | `25a913b` | 正本で確認できるDocument–Versionとcurrent Document/Version/Folderのtyped n-ary relationだけを投影する。推測relationは作らない。 |
| D6 | `99acfa7` | 取消直後のcall-time再認可、評価中generation pin、古いtriggerからの最新snapshot読取り、Source outage時の旧generation保持を実DB経路で固定する。 |
| D7 | `b46865b`, `4578a62` | 7 categoryの合成evaluationとstage attribution。false composite、hard false accept、false reject、coverage absence、Graph path内の未認可participant exposureを別集計する。 |
| D8 | `5480ea4` | Version、document-scoped Document/FolderPlacement、Tantivy、typed n-ary Graphを同一generationで構築し、Projection pointer CASを最後に実行する。正本claim evidence、明示IDのhistory lookup、body coverage preflightを接続する。 |

D8のGraph補助Resourceは `(Source, Document, Folder)` にscopeし、auxiliary ResourceIdから所有Documentへの対応をgeneration別に保持する。全Graph参加者は現在のDocument `Read`を再確認するため、同じFolderにある別Documentの権限を流用しない。failed/CAS敗北generationはGraph、lexical、Projectionをすべて破棄する。公開成功後にreceipt保存だけ失敗した場合は公開artifactを保持し、同じeventの再試行が `Unchanged` とreceipt保存へ収束する。

## Local verification

- `mise run eval:search`: **24/24 PASS**。出力は合成scenarioのID、category、stage countだけで、顧客contentを含まない。
- D8 real PostgreSQL vertical: **5/5 PASS**。Graph-only recall、hard gate後の2候補S1順序、非空Primary evidenceのSufficient、取消後の全結果面redaction、同一Folderの認可分離、T10/history、body `UnsupportedCoverage`を確認した。
- D8統合後の `search-source-document`: 最終test-only追加前に **51/51 PASS**。Core/Projection/Graph/Tantivyの関連回帰は **102/102 PASS**。cleanup強化後とreceipt失敗回帰は最終編集後にそれぞれfocused **1/1 PASS**。
- 変更crateのstrict Clippy、`cargo fmt --all -- --check`、architecture、locked metadata、`git diff --check` はPASS。D1〜D7の各Taskもfocused RED/GREENと独立read-only reviewを通過した。
- 最終 `mise run verify` は、secrets、dependency/security、workflow、fmt、workspace `cargo check --locked`、workspace strict Clippy、architecture、APIをPASSした。Rust workspace testはlink中に空き容量が約300 MiBとなり、`search-graph-memory` のtest binaryで `errno=28` (`No space left on device`) により停止した。これは **local standard gateのGREENではなく、Search assertionの失敗でもない**。再構築可能な当該worktreeの `target` は診断保存後に削除した。
- Whole-branch independent reviewは、Graph path内の未認可participantをD7 security metricが数えないP2を1件検出した。n-ary participantと `resource_path` memberの2ケースをREDで再現し、deduplicated exposure集計へ修正した。focused 2/2、全evaluation 24/24、strict Clippy/fmt/diffがPASSし、別のread-only reviewerがGOを返した。D1〜D8のproduction経路に追加P1/P2はなかった。

## Coverage・history・evidenceの境界

`DocumentCoveragePreflight::discover` がDocument adapterのtyped入口である。`TitleAndPermittedMetadata`だけが通常の `DiscoveryService` を実行し、`BodyRequired` はretrieverを呼ばず、blocking `UnsupportedCoverage` と空のqualified resultを返す。D8 verticalは実titleに一致するrequestでもlexical portが呼ばれないことを固定する。公開transport/APIは未実装なので、外部requestからcoverage種別を決める接続はまだ存在しない。

HistoryはLive searchのfallbackではない。`DocumentHistoricalLookup` は明示されたDocument IDとVersion IDを既存 `DocumentHistoryService` へ渡し、`Read`と`ReadHistory`の両方を要求する。T10は公開終了台帳を含む正本snapshotから判定し、`current_version_id = NULL` だけから推測しない。

Primary evidenceへ昇格できるのは同じgeneration/resourceに束縛した正本title・許可metadata assertionだけである。Graph path、DSI opaque reference、一般path文字列はretrieval traceには残せるが、Primary claim evidenceにはしない。

## Hosted verification

Phase DのbaseであるPhase C Draft PR #28 head `a8df10d7582f9d2947b0b58cfbd4713281fca912` は、CI `36580213345`、DSI Sandbox `36580213402`、DSI PoC `36580213444` がSUCCESSしている。これはPhase D headのhosted証拠ではない。

2026-09-30にlive GitHub状態を再確認した。Draft PR #33はOPEN、baseはPhase Cであり、head `4892ba5d2736b35bf95de25f834f016609d2e0d4` に対する [標準CI 36665497017](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36665497017)、[DSI PoC 36665497015](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36665497015)、[DSI Sandbox 36665497041](https://github.com/AIrisu-072/knowledge-platform/actions/runs/36665497041) は全てcompleted/SUCCESSである。Phase Dのhosted qualificationは **PASS**。このclosure記録はCompletion Program branch上に置き、検証済みのPR #33 headを動かさない。

## Deferred capabilities

- full-body Search Extractionと本文indexing
- vector/embedding、score calibration、reranker
- durable dedicated Graph backend
- remote provider adapterとremote materialization transport
- public Search/Discovery transport/API
- generic durable outbox delivery workerとproduction scheduler wiring
- production deployment、identity/secret接続、運用SLO・負荷容量の認定

現在のGraph/Projection reference runtime、合成evaluation、HTTP未接続のtyped adapterを、上記のdeferred capabilityや配備済みserviceとして扱わない。

## 受入状態

Local focused/evaluation evidenceは **PASS**。local standard gateは容量blockerのため **INCOMPLETE**。whole-branch independent reviewは修正後 **GO**。Phase D exact-head hosted gatesは **PASS**（`4892ba5`、上記3 run）。Draft PR #33はOPENであり、merge/deployは未実施。
