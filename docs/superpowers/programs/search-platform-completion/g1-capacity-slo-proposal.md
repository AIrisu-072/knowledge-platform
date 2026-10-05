# G1: 容量の実測とSLO案（2026-10-05）

この文書は、Search Platform Completion ProgramのG1「実測した容量と運用SLO案が再現できること」の記録です。**本番SLOの宣言ではありません。** すべて開発機（Apple M5、macOS 26.5.2、16 GiB、Rust 1.98.1）での単一プロセス・逐次の計測で、本番相当の負荷試験、Linuxサンドボックス、同時接続、ネットワーク越しの遅延は含みません。本番前に、配置先の構成と想定規模で同じ手順を再計測してから値を確定します。

## 実測値

### HTTP API（P5、4 route）

`crates/search-runtime/tests/server_e2e.rs` の `measure_route_latency`（`#[ignore]`、CIでは走らない）。本番factoryを `127.0.0.1:0` の実socketで起動し、実PostgreSQL・実ファイルのDocument 24件（本文Partあり）と、実TCPの合成remote catalog（2件）に対して、route ごとに逐次200回要求した。最適化build（`debug-assertions` は loopback 機能の制約で有効）。

| route | 条件 | p50 | p95 | p99 | 最大 |
| --- | --- | ---: | ---: | ---: | ---: |
| `POST /v1/search`（title/metadata） | 24件全件が一致 | 55.6 ms | 57.4 ms | 59.0 ms | 74.7 ms |
| `POST /v1/search`（bodyRequired） | 24件全件の本文が一致 | 55.2 ms | 67.4 ms | 85.2 ms | 96.0 ms |
| `POST /v1/discover`（remote Claim） | remote 1 Source＋Document | 177.2 ms | 185.2 ms | 198.0 ms | 238.2 ms |
| `GET /v1/resources/{id}` | Document現在版 | 3.7 ms | 4.0 ms | 4.2 ms | 4.8 ms |
| `GET /v1/sources` | 2 Source | 1.1 ms | 1.2 ms | 1.3 ms | 1.7 ms |

計測head：`c2280a6`（G2で最終開示gateに項目・Claimの再確認を加えた後）。加える前（`837b5f3`）はSearch p50 39.9 ms、Discover p50 157.9 msで、差は開示直前の再確認分。

所見：Searchの時間の大半は、ヒットごとのDocument現在権限確認（PostgreSQLへの往復）で、評価時と最終開示gateの二度行うため、返す件数にほぼ比例する。Discoverはremote providerとの往復（列挙・`/authorize`・`/content`）が支配的で、remote登録の `call_millis`・`evaluation_millis` に従う。

再現：

```sh
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release -p search-runtime \
  --features synthetic-loopback-test-only --test server_e2e -- --ignored --nocapture measure_route_latency
```

### 本文抽出と本文検索（P1）

[P1容量報告](../../../../experiments/search-extraction-poc/report.md)（in-process reader、macOS）。1 MiBのtext・HTML・DOCXはSupported（1.6〜3.1万Unit、索引構築0.8〜2.5秒、BodyOnly検索 p50 0.14〜0.25 ms・p95 約4 ms）。10 MiB以上と1 MiBのCSVは、1 itemあたり10万Unit・出力16 MiBの絶対上限でResourceLimitのUnsupportedになる。再現は `python3 experiments/search-extraction-poc/measure.py`（`search-source-document` の `body_measure` exampleを1 caseずつ起動し、CPU時間とpeak RSSを加える）。

### 永続Graph（P3）

[判断記録](../../../decisions/2026-10-05-search-graph-store-and-db-placement.md)、[P3計測ワークフロー](../../../../.github/workflows/search-p3-qualification.yml) run `37255132277`（100/1000/3000）。PostgreSQLはp50（1 reader）約22〜24 ms、p95（8 readers）約250〜295 ms。再起動・別環境への復元・故障注入はPASS。保存先は暫定で、本番前に再選定する。再現は `mise run poc:search-graph:qualify`。

### Vector（P2）

[P2報告](../../../../experiments/search-vector-model-poc/report.md)。採否は `DISABLED`。参考値：モデル常駐 約0.9 GiB、1,025 Unitの埋め込み 4.6〜5.4秒、query埋め込み＋全件走査 p50 10〜12 ms。

## SLO案（v0、単一ノード、本番前に再計測して確定）

| 指標 | 案 | 根拠 |
| --- | --- | --- |
| Search（pageSize 20以下）p95 | 250 ms以下 | 実測p95 57 ms（24件）。ヒット数に比例する権限確認を見込み約4倍の余裕 |
| Search（bodyRequired）p95 | 300 ms以下 | 実測p95 67 ms＋本文索引の規模依存（P1のBodyOnly p95 約4 ms/Source） |
| Discover（remote 1 Source）p95 | 1秒以下、かつremote登録の `evaluation_millis` 以内 | 実測p95 185 ms。provider遅延に依存 |
| Resource取得 p95 | 50 ms以下 | 実測p95 4.0 ms |
| Source一覧 p95 | 50 ms以下 | 実測p95 1.2 ms |
| 5xx率（依存障害を除く） | 0.1%以下 | 依存障害は `DEPENDENCY_UNAVAILABLE`/`IDENTITY_UNAVAILABLE` として別集計 |
| Graph探索 p95（8並行） | 500 ms以下 | PostgreSQL実測p95 250〜295 ms |

運用上の値：操作timeoutはhost設定（計測時20秒、起動時に0秒超・5分以下を強制）、開示lease 30秒（上限60秒）、cursor 絶対5分・無操作1分、要求header・JSON本文 16 KiB、成功応答 1 MiB、本文抽出 1 item 10万Unit・16 MiB。

## 未計測（宣言しない）

- 同時接続・持続負荷（すべて逐次計測）と、本番ハードウェア・ネットワーク越しの遅延。
- Linuxサンドボックスでの本文抽出（本番経路）、outbox配送の処理量と遅延（P6）、GC・起動時復旧の規模依存時間（P7）。
- 1,000件を超えるDocument・Unit規模でのSearch。
