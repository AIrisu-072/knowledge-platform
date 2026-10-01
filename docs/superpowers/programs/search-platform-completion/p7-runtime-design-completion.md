# P7 Production Runtime / Deployment / SLO — completion 設計提案

Status: **PROPOSAL / 独立 architecture・security review 前 / Freeze 前**（2026-09-30）。本書は runtime 組み立てと運用資格の設計案であり、production 実装、Graph/Vector 採用、SLO 保証、live deployment の証拠ではない。規範正本は `spec/`。独立 review の指摘を解消した後だけ runtime freeze と code task に進む。

## 1. 固定入力と責務境界

- 依頼文 §§14–18,20、Search v0 承認設計 §§61–64、`spec/operations/error-handling-resilience-requirements-v0.md` §§11–15、`spec/operations/observability-audit-requirements-v0.md` §§22–26 と Search/P4 追補を守る。論理分離を維持し、初期は単一 Rust application に組み立てる。物理 microservice 分割は行わない。
- [P7 早期契約](p7-runtime-contract-draft.md) §§1–7 の composition/config/health/retention と、独立 GO 後の [共有 durable freeze](p7-shared-durable-freeze.md)（[元設計](p7-shared-durable-design.md)＋[改訂 1](p7-shared-durable-revision-1.md)）を合成する。後者の P7-01〜12 が global Source ownership、同一 PostgreSQL Source/current、generation READY、event origin、guard、durable pin、GC、別 DB restore を所有する。本書は第二の pointer、ledger、actor mint、Graph READY や outbox ack を設計しない。
- P1 の実 Document snapshot/reader/Unit/coverage/lexical seal、P2 の neutral core と測定選定、P3 の backend 選定/Graph content、P4 の source-neutral registry・Remote TCP/保持、P5 の四 route/実送出、P6 の generic delivery/Search bridge の **accepted code slice** を組み立てる。各 lane の Freeze/plan と独立 receipt を実装時に照合する。未完了 lane を mock で production-ready と呼ばない。
- P2 は測定後 `Disabled` または `Selected` を明示する。`Disabled` でも P2 neutral contract、P1 lexical、P3 Graph、Source current/final access、retention は必須。`Selected` の時だけ P2-07 の採択済み adapter と versioned P7 vector receipt を要求し、v1 bundle を暗黙変更しない。P3 は P3-P04 の独立採択前に Graph production READY/publish を開かない。非 PostgreSQL Graph 採択なら共有 atomicity/fence の別設計・独立審査を先に要する。

| 既存 producer | runtime が消費するもの | 重複しない責務 |
| --- | --- | --- |
| P7-01〜11、P7-12 | complete desired ledger、Source/current、READY/pin/GC、復元検証 | SQL migration、CAS、guard、lease、Search receipt、低層 restore を作り直さない。 |
| P5-01〜08 | Search API の四 route、trusted Bearer→scope、DTO、二 lease と socket 送出、四 route factory | P7 は同じ factory を host process に載せる。別 HTTP handler/可視 catalog/credential verifier を作らない。 |
| P6-G/S と I04 | generic runner、Search bridge、worker wiring/role split、fenced ack | P7 は同じ delivery instance を lifecycle 管理する。`delivered_at` は generic role だけが更新する。 |
| P1/P3/P4 | Source-owned current read、実索引/Graph、Remote RAM evaluation | 新しい正本・永続 Remote generation・第二 Discovery loop を作らない。 |

## 2. Composition root と起動・停止

`search-runtime` の一つの `ProductionRuntimeFactory`（概念名）が host 注入 port、typed config snapshot、選定 receipt を入力し、`SearchRuntime` を一度だけ構築する。P5-08 の四 route factory と P6-I04 の worker wiring はこの factory に接続する既存 producer とする。同じ artifact は API と Search worker を同一 process で走らせられる。既存 P6 worker entrypoint を資格試験や worker-only 運転に使う場合も同じ factory/Source row を使用し、別 service 正本は設けない。Graph は内部 retriever から既存 federation/Discovery pipeline に戻す。

起動 admission は次の順で fail closed とする。各段階は診断 code と対象 component category のみを記録し、tenant/Source/credential 等を public health に載せない。

1. host-supplied config を parse/validate し、配置・容量・期限階層・retention・logging/dump policy と必須 `SecretRef` を検証する。値を config snapshot/log に保持しない。
2. privileged migration phase の完了を確認する。Domain `0009` → Search `0001` → P7 `0002+`、選定後の Graph ledger を checksum と順序で検証し、serve/claim 用実 DB role の grant、trigger、RLS/権限境界を別接続で確認する。migration 実行権と通常 API/worker 権を同じ接続 role に与えない。未証明の legacy Source backfill は拒否する。
3. trusted host の Document/Remote **全 tenant・namespace ごと完全 desired snapshot**を同じ P7-02 ledger に reconcile し、global SourceId/tenant/kind/activation/current invariant を確認する。P4 `TrustedSearchScope`/`AuthorizedSourceScope`、P5 credential verifier/identity/current visibility と P7 host scope reference を一つの trusted chain に接続する。
4. P7-12 の current bundle 検証を通す。実 lexical directory と P3 Graph、採択時 Vector の key/digestを再 open し、失われた Source は unavailable にして再構築待ちにする。Remote RAM generation、session/cursor は restart 後復元しない。
5. Document Source/実 extraction runner、Projection/lexical、Graph、Remote adapter、P2 mode、Search/Discovery application、P5 API router、P6 generic+Search delivery、Audit、OTel を接続する。必須 port 未配線、test fake/MemoryDocumentIndexRuntime、未資格 Graph/Vector、identity なしでは API accept と claim を開かない。
6. readiness が安全な応答を確認してから listener と claim poll を開く。shutdown は新規 HTTP/claim/Source lease を止め、進行 request と outbox/Source lease の bounded drain・cancel を行う。未確認 event を ack せず、pin/disclosure lease を閉じる。期限内に終わらなければ fence に回収を任せ、成功に変換しない。

## 3. Typed config と secret・権限

`RuntimeConfig` は config file の versioned schema に host environment の **参照値** override を適用した immutable snapshot とする。優先順位、override allowlist、設定 revision、unknown field 拒否を固定し、再読込は新 snapshot の validation→complete desired reconcile→段階的切替の transaction として扱う。provider/request から構成値を変更できない。具体秒数と production capacity は測定後に決める。

| typed 群 | 必須検証 |
| --- | --- |
| `DeploymentMode`, `TenantMode`, `RuntimePaths` | trusted tenant 一致、data/index/staging root の所有/書込権、同 filesystem の atomic rename、空き容量と mount、外部向け bind/listener。production に暗黙 path/default を置かない。 |
| `DatabaseRef`, `GraphStoreRef`, `SearchRoleRefs` | endpoint 参照、schema/ledger、migrator・registration・builder・coordinator・reader・GC・generic delivery の分離。P7-03 と P6-I04 の実 grant を検査。Graph backend は P3 receipt に従う。 |
| `CompleteDesiredRegistrations`, `ProviderConfig` | Document/Remote 各 namespace 全 tenant snapshot revision/digest、固定 origin/許可 transport、rate/response/resource/deadline/retention、grant proof。host だけが発行し、Remote 失敗で登録正本を消さない。 |
| `RuntimeLimits`, `DeadlinePolicy`, `WorkerPolicy` | P5 公開 hard limit と P1 parser/ZIP resource limit、P6 DB policy の範囲に収める。`client > operation > dependency` と残余 budget、bounded queue/in-flight/lease/drainを検証。初期安全上限を SLO と呼ばない。 |
| `TelemetryConfig`, `AuditConfig`, `SecurityConfig` | exporter/collector 参照、別の保持・閲覧権限、bounded buffer/drop、request/body debug log 無効、core dump policy、NO_RETENTION の漏出防止。 |

`SecretRef` は DB/provider/identity/Audit/OTel 用の不透明参照で、host `SecretResolverPort` が adapter 構築時だけ値を解決する。参照は config に置けるが、秘密値は serialized config、`Debug`、panic、trace、audit、manifest、CLI error に出さない。必須 secret が未解決なら該当 adapter を起動しない。権限不要の component に secret を渡さない。secret provider/credential 実値は本提案で選ばず読まない。

## 4. Health、degraded、診断

| signal | 判定と公開境界 |
| --- | --- |
| liveness | event loop と停止受付の応答。DB/Remote 一時障害で無条件に再起動ループにしない。公開応答は固定 code のみ。 |
| global readiness | trusted identity/credential/visibility、完全 registry、DB schema/grants、Source/current gate、選定済み Graph・P1 index・P5 API final gate、必須 Audit event 生成経路、P6 claim/fenceが安全に使えること。未配線/不一致なら listener は 503、claim 停止。 |
| Source readiness | 管理用・認証済み診断で `ready/rebuilding/degraded/unavailable`、current key/pointer revision、P1 Unit/coverage/lexical、P3 Graph、選定時 Vector、Remote expiry、outbox backlog/oldest age/DLQ を別 signal として示す。 |

一部 Remote outage だけなら、その Source を degraded にし、安全な他の actor-visible Source を P5 の partial/evidence 規則で扱う。Source に依存する Required evidence は sufficient にしない。registry/visibility 全体、current final gate、必須 local Source の安全性を失った場合は partial 200 を作らない。hidden Source の ID、存在、障害、count は public health/Problem/gap/trace に出さない。readiness は freshness 保証ではなく、commit-to-visible lag と Source snapshot 差分は別に測る。alert 閾値は実測後に設定する。

## 5. Recovery、backup、運用操作

P7-12 の key/receipt/file/Graph 検証と P6-S07 の二重 fence/outbox replay は**既存の資格**として再利用する。runtime はそれらを呼ぶ起動手順、operator command、runbook と HTTP/worker 再開判定を所有する。

- full index rebuild と Graph rebuild は Source 正本の current Version/Part/raw/retention を再読し、新しい `(SourceId,generation)` の BUILDING→READY→guarded CAS を使う。古い current/pin を上書きせず、未完了 outbox row を manual rebuild で ack しない。部分失敗・`Retryable`・外部 file 不足は公開しない。incremental cursor が証明不能なら別 key の full rebuild に戻す。
- restart/kill/interrupted generation/stale generation/commit 応答不明では migration/role/registry と current 実体を再検査する。current、pin、guard を一つの DB clock/fence で管理し、remote evaluation/cursor/session RAM は stale にする。corrupt derived row/file を quarantine/unavailable とし、別 key へ暗黙切替えず再構築する。
- backup は PostgreSQL、immutable lexical bytes、選定 Graph の必要 bytes/schema、artifact version と digest を一組の manifest にする。復元試験は disposable な**別 DB と別 index root**で実行し、同 key/receipt と実 query を検証してから accept する。DB のみ戻して file 不在の状態を ready としない。secret 値を backup に含めない。
- runbook は safe stop、lease expiry/unknown commit、provider outage、DLQ、lag、stale/corrupt key、rebuild、backup/restore、role/schema 不一致、rollback（immutable artifact/旧 current 保護）を command と観測証拠つきで記す。不可逆 migration や live 切替は別の Hard Stop 判定に送る。

## 6. Audit・OpenTelemetry・保持

P5/P4 の final actor/Source/item/field/Graph participant gate 後だけ公開 DTO を送出する。`NO_RETENTION` は P4 evaluation RAM lease と P5 `TransientDisclosure<T>` の二段階寿命を同じ send completion/error/cancel/disconnect/deadline で閉じ、cache/cursor/disk/spool/dump/fixture/trace/audit に payload を残さない。復旧・benchmark の synthetic data もこの retention 契約を破らない。

trace は `discovery_evaluation_id`、`need_id`、`projection_generation`、route、candidate、Graph expansion、probe、materialization、evidence sufficiency、Remote call、outbox lag、extraction state を固定 stage/code/count/duration で相関する。ただし ID は適切な限定 trace/evaluation artifact に留め、metric label は stage/status/source category など低 cardinality の allowlist のみ。本文、query 全文、provider native locator、Graph path、秘密値は通常 log/trace/audit に複製しない。隠れた Source の count も外に出さない。

必須 Audit event の transactional creation 失敗は対応する business transaction を commit しない。作成後の Audit Store 配送失敗は audit outbox の retry、OTel collector 停止は bounded buffer/drop とし、Audit と OTel の役割・retention・閲覧権を分ける。Search receipt、generic outbox ack、Audit outbox を混同しない。

## 7. 容量・負荷・SLO と deployment-ready 判定

load harness は実 runtime の synthetic/public corpus を小・中・大の**複数規模**で走らせる。件数・同時数・試行時間は host の空き disk/RAM、P1/P3 index bytes、P6 backlog と実行前 reserve を測り、実行可能な envelope を先に書いて決める。測定は indexing/extraction/outbox throughput、HTTP Search/Discover p50/p95/p99、Graph traversal、Remote fanout、memory/disk、full/Graph rebuild、restart recovery、Vector 選定時の index/query、Agent 連携時の context tokens を含む。選定外 Vector や Agent 不在は `N/A` と原因を記す。paired baseline、seed、code SHA、config/role、fixture hash、hardware、warm/cold、fault schedule、repeat と分布を保存し、quality stage 別 evaluation と接続する。

観測 capacity envelope、想定業務 traffic/concurrency と失敗許容を照合した**後**に `proposed operational SLO` を別文書で提案する。benchmark 観測値、運用提案値、対外保証値は別欄とし、対外保証はこの task で設定しない。閾値が決まらない場合は追加測定/業務前提収集の bounded task とし、数値を先に発明しない。

deployment-ready artifact は既存 `Dockerfile`/`mise.toml` の拡張による reproducible Linux OCI/binary、必要 worker/parser pin、non-root filesystem/mount・health/signal、config schema/example（参照のみ）、one-shot migration/role check、runbook、artifact digest/SBOM と local disposable DB/Remote TCP の実 smoke で判定する。specific cloud/orchestrator や live production target はここで選ばない。資格試験用 fixture identity/provider は host real credential の代替証拠にしない。merge、live deploy、本番 migration は行わない。

## 8. 未解決の技術判定と review gate

1. P3-P04 の Graph 採択と `GraphReceiptMappingV1` 二 encoder、P2 Disabled/Selected と採択時 vector receipt。これらは lane の実測・独立判定を消費する。非 PG Graph なら共有 publication protocol の再設計・独立 review が必要。
2. P5/P4 の実 host identity/Source authority、P7 `HostScopeReferencePort` の restart 後再解決、provider secret resolver 実接続、P6/P7 実 DB role。port/fixture だけでは live authority を主張しない。
3. request/dependency/lease/drain timeout、capacity reserve、alert、operational SLO は P5/P6 安全上限と実負荷・故障測定から決める。Audit policy・各 signal retention の数値も既存規範と運用要件に照合する。

独立 architecture/security review は、P1〜P6 freeze と P7 shared freeze の意味を再審議せず、組立て時の権限・保持・復旧・NO_RETENTION・役割/秘密・degraded/partial・循環依存・測定可能性を確認する。P1/P2 blocker は修正→別 reviewer 再審査を経てから Freeze とする。
