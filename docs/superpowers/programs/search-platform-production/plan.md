# Search Platform本番化プログラム — 計画

状態：**ACTIVE**（2026-10-06）。基点はmain `ce8ed4f`（PR #66統合後）。branch `feat/search-platform-production-20261006`、Draft PR 1本でmainへ。CIがすべて成功したら所有者の確認後にmergeし、deployはしない。進捗は[状態記録](../../execution/search-platform-production-program-status.md)に残す。

## 所有者の判断（2026-10-06）

- **A1 複数Sourceの充足**：Claimを判定できるSourceの候補だけで判定する。判定できない候補は検索結果には残すが、充足の判定には含めない。Source間の食い違いは「矛盾」のまま示す。範囲内に判定できるSourceが一つも無ければ「未解決」とし、その理由（`no_evaluating_source`）を付ける。
- **A2 本文抽出のkill・timeout**：自動の再試行は上限回数までとし、手動の再試行には上限を設けない。取り込む形式は決まっているので、失敗を前提にした設計にはしない。優先度は基本機能と設計済みの作業より低い、品質向上の項目とする。
- **E Vector**：本番で使えるようにする。融合順（Graphの後）と類似度の下限を直したうえで測り直し、合格すれば既定で有効にする。不合格なら理由を記録し、Sourceごとに有効化できる状態にする。
- **範囲外**：Search coordinatorの `UPDATE (lease_token)` の置換（旧A3）と、本番前の作業（Graph保存先の再選定、SLOの再計測、旧Search9履歴の確認、本番資格の宣言とdeploy）。

## 作業

| ID | 内容 | 主な変更先 | 受入 |
| --- | --- | --- | --- |
| A1 | 判定できないSourceの候補をClaimの判定から外す。範囲内に判定できるSourceが無いときは理由を付ける | `search-application` の `evidence_resolution.rs`、`discovery_service.rs` | `search-application` 全試験、`server_e2e` の複数Source Discoverが `sufficient` |
| B4 | HTTP APIが、P7で公開した世代（READY・pin・受領記録）を読む本番用のactor port factory | `search-runtime` の新しい `durable_read.rs` | P7で永続化したDocument世代を、HTTP API 4 routeで読む実DB/FS試験 |
| B5 | ホスト登録一覧の原子的な公開処理（P7-R01P）、読取り専用アダプターと起動時照合（P7-R02）、そのためのSearch Audit生成元の表と追記ポート（R04Aの生成元側） | `search-runtime` の `host_inventory_publish.rs`、`host_registration.rs`、`audit/`、新しいmigration | 部分的なテナント・名前空間・古い改訂・コミット不明を拒否する実PG試験 |
| B6 | 差分世代を、基準世代から引き継いだ自己完結のP7バンドルとしてREADY・公開する（Graphの差分構築と同じ方式） | 新しいmigrationとP7のREADY・公開処理、`PgDocumentIndexRuntime` | 全件構築と差分構築の結果が一致し、競合・失敗時は旧世代を保つ実PG試験 |
| B7 | 公開DiscoverにGraph計画の入力を足し、session単位のDocument Graph権限readerで、第三の参加者の権限取消を確かめる | `search-application`、`search-api-http`（OASへの追加のみ）、`search-runtime` | `server_e2e::graph_nary_third_participant_revocation` |
| E | Vectorの本番実装（P2-07）。Candleの埋込み、完全走査、P7受領記録、S1の融合順をL・G・Dに変更、類似度の下限。公開データ（MIRACL日本語の固定部分集合）と既存の合成基準で再計測し、既定値を決める | 新しい `search-vector-adapter`、`spec/selection` への選定追加、`search-runtime` | 選定・SBOM・数値一致、実DBの縦断試験、再計測の報告 |
| D | 完成プログラムのG2レビューで残したP3の改善（`g2-review-*.md`）。ただし旧A3は除く | 各crate | 項目ごとの試験と再レビュー |
| A2 | 自動の再試行は上限まで、上限なしの手動再試行を運用操作として追加する | `search-source-document`、`search-runtime` | 自動の上限と手動の再実行の試験 |

順序はA1 → B4 → B6 → B5 → B7 → E → D → A2。B4がB6・B7・Eの読取り経路の前提になる。仕様の追加（OAS、選定の追加、migration）は、その作業の中で一度だけ行う。

## 検証

各作業では、対象crateの試験・clippy・fmtを確認する。実PostgreSQLが必要な試験はOrbStackで実行する。独立レビューは、管理worker（Claude Opus 5.5、high）で領域ごとに行う。hosted CI（CI、DSI PoC、DSI Sandbox Preflight）は、最終headで成功させる。過剰な証跡は作らない。
