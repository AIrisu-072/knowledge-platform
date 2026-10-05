# P7 runtime revision 1 — independent architecture/security re-review

**FAIL / NO-GO — design/plan Freeze only**（2026-09-30）。先行 review の5件のうち3件は設計・計画上閉じたが、host inventory の発行者と Audit source schema/producer の2件は P2 として残る。これは P7-Rxx 実装、P3 backend 採択、P1〜P6 最終資格、SLO、merge、live deployment の判定ではない。

## Findings（優先順）

1. **[P2] host の完全 inventory を atomic publish する実 owner と正本入力がない。** 改訂設計 [§2:11–21](p7-runtime-design-revision-1.md) は `HostRegistrationInventory` の独立 tenant roster・二 namespace・epoch・revision/digest と「host登録writer」を要求し、R02 を *read-only* adapter としている。改訂計画 [R01/R02:42,49–53](p7-runtime-plan-revision-1.md) も inventory reference の読取と `CompleteDesiredRegistrations::capture(...).await` → P7-02 `PgPool` ledger → `SourceRegistrationCatalog::try_new(...).await` の組立ては割り当てたが、**実登録正本から manifest を生成・atomic publish する writer の file/task/受入試験は割り当てていない**。現行 port は constructor 名・candidate digest で全 tenant 完全性を証明できず（`crates/search-application/src/source_registration.rs:675–709`）、実装は synthetic host だけ（同:764–813）。`startup_rejects_partial_tenant_or_namespace` を合成 manifest だけで GREEN にしても、実 host tenant roster の欠落を検出した証拠にならない。host SSOT の具体的な登録 writer/source、atomic publish 境界、R02 が独立に照合する roster 証拠、production 用の別接続/restart/revision 変更試験の sole owner を計画に追加する。発行できない配備の起動拒否は安全だが、実 production 組立てを完成させる計画の代わりにはならない。
2. **[P2] Search/runtime 必須 Audit class の同一 transaction producer が既存 source schema に接続できていない。** 改訂設計 [§3:27–40](p7-runtime-design-revision-1.md) は Search privileged management、audit configuration、integrity/destructive operation、重要な auth failure を `audit_outbox_events` に typed INSERT するとし、改訂計画 [R04A:75–81](p7-runtime-plan-revision-1.md) は `0010` で配送状態列を加え、source event 列を不変にするとする。しかし既存 `audit_outbox_events` は `resource_id UUID NOT NULL`（`0001_document_authoritative_core.sql:86–100`）、`resource_type` は `Document|Folder|AccessPolicy` だけ（`0006_document_management_access_v0.sql:55–57`）。現行 Document denial は `AccessPolicy` と nil UUID を明示的に使う（`targeted_events.rs:64–84`）が、Search Source、runtime 設定、system integrity の class/subject をその値へ詰める契約はない。R04A の書込範囲にも P5 identity/route や P7 管理 command の producer 呼出し箇所がなく、`audit_delivery` 試験だけでは各 business mutation と Audit INSERT の同一 transaction を証明できない。各必須 class の source-row 型/制約拡張または別の typed Audit source、system actor/subject 表現、transaction owner の具体 file/task と class 別 INSERT 失敗 rollback/拒否記録試験を固定する。既存 Document producer の流用と P6 Domain ack 分離は妥当だが、この欠落を埋めないまま Audit 経路完了とは判定できない。

## 先行5件の再判定

| 先行指摘 | 判定 | 根拠・残る境界 |
| --- | --- | --- |
| 1. host inventory → PG ledger → P5 composition | **OPEN / P2** | R02 の async `capture`/`try_new` と sole adapter/route 分担、unknown commit・restart 時の fail closed は明記（設計:11–23、計画:47–53）。上記の host manifest *publisher* が未割当。 |
| 2. Audit producer・別配送 | **OPEN / P2** | R04A の source/sink 別 role、lease/fence、retry/DLQ、event_id unique、unknown commit 再読、P6 ack 分離は明記（設計:27–42、計画:75–81）。上記の Search/system class schema と transaction producer が未接続。 |
| 3. sink 別 NO_RETENTION policy | **CLOSED at design level** | health/admin/config/log/metric/trace/Audit/exporter を列挙し、`SinkKind × RetentionMode × VisibilityClass` default deny、secret/hidden Source 禁止、二 lease、success/error/cancel/disconnect/deadline/socket/exporter flush/drop/shutdown/保持 handle の sentinel を指定（設計:44–61、計画:83–88）。`count/size bucket` が provider 由来 per-call 値でないことは R04O の実装・sentinel で証明する残余 risk。 |
| 4. OTLP transport POC REQUIRED | **CLOSED at design level** | isolated R04P が HTTP/protobuf と gRPC を exact version/features/lock、license/advisory/source、Collector、停止時 buffer で比較し、独立 review・parent 採択・normative pin 前の production Cargo/R06 image を拒否（設計:63–65、計画:61–73,90–92）。提案 feature/API は [`opentelemetry-otlp` 0.32.0 公式 crate docs](https://docs.rs/opentelemetry-otlp/0.32.0/opentelemetry_otlp/) の `with_http`/`with_tonic` と各 feature に一致。PoC/採択は未実行。 |
| 5. workload 軸・測定予算 | **CLOSED at design level** | tenant/Source/Document/Unit/Graph/payload/fanout/HTTP-worker concurrency/backlog/cold-warm/fault の immutable manifest、pilot に基づく walltime/RSS/disk reserve、pre-admission と途中 abort、欠測 `NOT_ADMITTED`、R08 の観測範囲内 SLO 提案を明記（設計:67–71、計画:94–104）。100/1,000/3,000 relation group の P3-P04 資格は縮小しない。実測・SLO 提案は未実行。 |

## 横断境界・検証方法

- P3 `source_control` は P7 `search_source_coordination` の alias で、第二 pointer は置かない（`p7-shared-durable-freeze.md:11`、`p7-shared-durable-plan.md:15–17`）。Graph READY/publish/pin/GC は P3-P04 採択と同一 DB の transaction-bound coordinator を要する（`p3-graph-design-revision-1.md:50–66`）。改訂は P7-01〜12、P5-07/08、P6-I04/S06/S07 の alias を消費し、P1〜P6 最終 receipt を R09 でのみ fan-in する。Graph が未採択/Blocked なら R02/R09 production acceptance は閉じる。
- 手法は taskgraph `p7-review-runtime-revision1`（`task-graph.yaml:6999–7014`）に対する fresh static review。旧提案/先行 review、改訂2文書、共有/P3/P5/P6 契約、規範 `spec/`、port と Audit migration/producer を照合した。Cargo、DB、container、性能測定は実行していない。よって named tests は**計画**であり PASS receipt ではない。現 worktree は `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未 commit 状態。
- runtime route: parent 指定は `gpt-6-sol / max`。`toolbox-context status --workspace "$PWD"` は別目的の2 run だけを `inspect-before-resume` と返し、この監査の managed run は無い。Active Pointer は managed inspection の uncertain operation と native scoped continuation を記録する。従ってこの監査から managed health gate/実 worker model attribution は独立には証明しない。既存 run の replay は行っていない。

## Exact inputs

監査開始時と執筆直前の SHA-256 は同一。主要5入力:

| file | SHA-256 |
| --- | --- |
| `p7-runtime-architecture-review.md` | `0bf9ce336e664a3eb4a5e14c7f37ec9a351bc4edbfa9d6b8b6435f002088f793` |
| `p7-runtime-design-completion.md` | `df0f2ba4e91526a24567addfac3777a7d7219ae296d5a3dc4b0dcc0c51737f3e` |
| `p7-runtime-plan-proposal.md` | `acecf896ad5481955b805728bb8c0d970e154657e1a4d5a89f7c100f3bc0ed3a` |
| `p7-runtime-design-revision-1.md` | `422e00dcb0c685feb2fc9f81755e25bdd555d2e4b7602ccadb85e4c281a9d035` |
| `p7-runtime-plan-revision-1.md` | `d24cba464e40f9d07d2baf9095d4ece1bb099f0f8b06cfaca6eee2e2982e3d57` |

補助入力の SHA-256: `p7-shared-durable-freeze.md` `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110`、`p7-shared-durable-plan.md` `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517`、`p3-graph-design-revision-1.md` `ba9e8616d9dd8fc569280f20956c841929155366fa30246dadee0f0c73e8aa59`、`p5-api-plan.md` `fdb44ec8ccf70fa6714bc9b402ae07be8332271b3facf583af2c294a8ff3e8d9`、`p6-outbox-plan.md` `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`、`observability-audit-requirements-v0.md` `d20dfac30fda3c826746e8f1814a844fe3d5578f10299f2f79c97a28afeb868d`、`library-tool-selection-v0.md` `b12dc484fe8b596a924c06abdcce92dc39dffcff1ea562ca74c42875638656ba`、`source_registration.rs` `60975ee86938f3a3f59b18d0945ca10385eb5374419bd5c0d51cbd88963cc17f`、`0001_document_authoritative_core.sql` `45822f609f23685819bd33090bf848c97d9f7f50b795e9048eb9a5ba06a58b35`、`0006_document_management_access_v0.sql` `cab4a7332ebb9b8602be379ec56709f686f1b5a6755f33a530e93788583179fa`。

**Exact next action:** host manifest publisher/authority の sole owner と実入力を計画へ追加し、Search/system Audit class の source schema・transaction producer/試験を設計と R04A に追加する。改訂後の exact hash で別の独立 reviewer が5件を再判定するまで runtime design/plan Freeze をしない。
