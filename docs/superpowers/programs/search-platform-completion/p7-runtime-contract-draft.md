# P7 Production Runtime — 早期 composition / coordination 契約案

Status: **DRAFT / reconcile required / 未凍結**（2026-09-30）。これは P1〜P6 の改訂設計を接続する初期契約であり、実装・backend 採用・production readiness・SLO・配備の証拠ではない。`spec/` を規範正本とし、P1〜P6 の Freeze と実測 receipt を受けて差分を照合する。

参照: [Search v0 承認設計](../../specs/2026-09-28-search-discovery-platform-v0-design.md) §§3,5–6,20–21,27,49,62–64、[P1 改訂](p1-extraction-design-revision-1.md) §§3–4、[P3 改訂](p3-graph-design-revision-1.md) §§4–5、[P3 build guard 追補](p3-graph-build-guard-amendment.md) §§1–2 と [FK-safe cleanup 修正](p3-graph-cleanup-order-correction.md)、[P4 改訂](p4-remote-design-revision-1.md) §§2,7、[P5 改訂](p5-api-design-revision-1.md) §§1–4、[P6 改訂](p6-outbox-design-revision-1.md) §§1,4–5、[環境 inventory](environment.md)。P7 要求は依頼文 §14–18。監査・観測は `spec/operations/observability-audit-requirements-v0.md` §§23–24 と Search amendment に従う。

## 1. 一つの Rust composition root と port

- 論理分離を維持し、初期は一つの Rust application/process に組み立ててよい。物理 microservice 分割、Graph/Vector 製品、Search API transport はこの案で選ばない。別 process 化する場合も以下の durable fence と request scope を再現してから別設計として検証する。
- composition root は typed config 検証、DB migration/schema/role 検査、Source registry invariant 検査、必須 adapter 接続、artifact recovery、API accept、outbox poll の順に進む。停止時は新規 API/claim と Source lease 取得を止め、deadline 内の request と二つの worker lease を drain し、未確認の event を ack しない。
- 接続する論理 port は `ScopedSourceRegistryPort` / actor-visible registry、`AccessContextAuthorityPort` / trusted identity、`CurrentSourceVisibilityPort`、Source 別 current item/field access、Document Source snapshot/storage、Remote Source adapter、Projection / Unit coverage store、Lexical、選定時のみ Vector、Durable Graph、generation coordinator、Search/Discovery Application、resource locator、outbox delivery/bridge、Audit、OpenTelemetry exporter。Graph は内部 retriever として federation に戻し、独立公開 API を必須にしない。
- `search-core` / `search-application` の公開 port に SQLx `Transaction`/`PgConnection`、provider credential、HTTP header 由来の権限を流さない。PostgreSQL の同一 transaction 操作は infrastructure 内の private transaction-bound port に閉じる。Source 正本と Search 派生 state の間に分散 transaction は作らず、query 最終 gate で現在の Source access を再判定する。
- P4 §2 の**概念型** `TrustedSearchScope {tenant, principal, session, access_handle, access_revision, issued_at, deadline}`、`TrustedDiscoveryBinding {actor, evaluation}`、`AuthorizedSourceScope {actor, source, registration_revision, visibility_revision}` を共有最小契約とする。constructor は trusted in-process resolver/visibility adapter 限定。P5 の `TrustedSearchContext` / `AuthorizedSourceSet` はこの**同じ型を保持する alias/adapter と集合 view**に限定し、別の trusted minting path、tenant/principal 正本、Source visibility 判定系を作らない。P5 の四 operation は identity→可視 Source snapshot→routing/pin/provider→最終 field gate→safe DTO の順を守る。公開 API の port signature と DTO は Freeze 前に照合する。

## 2. Typed config、secret 参照、Source registry

| config 群（概念型） | 起動検証と境界 |
| --- | --- |
| `RuntimeConfig { deployment, tenant_mode, db, data_dir, index_dir, graph_store, source_registrations, limits, deadlines, telemetry }` | config file と環境変数の override 優先順位を明示し、parse 後に一つの typed snapshot と revision を作る。path は配置/権限/容量を検査し、production の暗黙 default を置かない。 |
| `DatabaseRef` / `GraphStoreRef` / `IndexDirectoryRef` | DB endpoint、schema/role、durable pointer/lease/receipt、index path を参照。Graph backend は測定後に確定。PG 以外なら §3–4 と同等の atomicity/fence の再設計が必須。 |
| `ProviderRegistrationConfig` | server-owned tenant/`SourceId`、固定 endpoint ref、mode/retention/field/authority grant、timeout、rate/resource limits、revision、credential の `SecretRef` だけを保持。provider response や request は登録値を変更できない。 |
| `SecretRef` | secret manager/keychain/env *name* 等への不透明参照。値は必要時に専用 resolver から adapter に渡し、typed config 表示、Debug、log、panic、trace、audit、manifest に格納しない。未解決の必須 secret は対象 adapter を起動しない。 |

- registry は `SourceId` を server-owned、**全 tenant 横断で一意**に発行する。起動時と registration 更新時に tenant を跨ぐ同一 ID、同一 tenant の重複、provider 指定 ID、既存 `(SourceId,generation)` への再割当てを拒否する。既存世代へ混入させない。単一 tenant 構成では trusted tenant が設定 tenant と違えば routing/locator 前に拒否する。一意性は認可の代用ではなく、全 port 呼出しで trusted tenant、registration revision、`AuthorizedSourceScope` を照合する。
- `ScopedSourceRegistryPort::visible_sources` と P5 の actor-visible snapshot が得られない場合は全公開入口を fail closed。個別 Source の Denied/Unknown は集合に入れず、隠れた Source の存在・障害を公開 health、gap、trace、件数に出さない。request 中に scope が失効したらその Source 由来の candidate/evidence/rank/trace を一体除外する。
- Remote は一 evaluation / 一 Source につき一つの sealed immutable **RAM generation**。共有 snapshot と scope/revision を証明できない複数 action を混ぜず、durable `ProjectionGenerationStore`、Graph、outbox receipt に変換しない。P4 の retention owner/lease と P5 の公開 cursor lifetime は同一 composition root で検証する。

## 3. 共有 generation / publication state（PostgreSQL 候補）

- P3 の PostgreSQL 第一候補を**条件付き統合契約**として記す。P6 の Domain `outbox_events`、P7 の Source control/current pointer・Search receipt・evaluation lease、P3 Graph generation/guard は v0 では**同一 PostgreSQL database**に置く。Projection READY metadata も同じ transaction で検証できる配置にする。Graph/index file の bytes は別媒体でもよいが、READY 証拠と digest を事前検証し、失われたら query を fail closed にする。別 backend を選ぶなら publication/pin/GC の同等 protocol と crash proof を先に設計・審査する。
- Source ごとの durable control は**一行、一つの current pointer**。P3 §5 の `source_control` と P6 §2 の仮称 `search_source_coordination` は別々の pointer にせず、一つの schema/adapter へ統合する。概念 field は `source_id`, `current_generation_id?`, `current_manifest_digest?`, `current_bundle_digest?`, `pointer_revision`, `fence_epoch`, `owner_token?`, `lease_expires_at?`, `last_published_epoch`, `build_fence_seq`。各単調 counter の overflow は fail closed。登録済み Source の行を先に一意 INSERT してから lock する。実列名・migration/role は Freeze/plan で決める。
- `(SourceId, generation_id)` を全 artifact の共通 key とする。既存 `ProjectionGenerationManifest.digest` は P1 §3 の **projection-only v1** のまま。P1 `GenerationBundleReceipt` の Unit/coverage/lexical/Graph/profile/composite digest と P3 `GraphGenerationReceipt` の source snapshot/mapping/graph content digest/count/schema を別々に保持し、key、source snapshot、receipt 内容、schema、count を照合する。P1 Graph receipt と P3 Graph content digest の同一性または検証可能な対応は未確定であり、曖昧なまま publish しない。Vector 採用時は versioned bundle/receipt への追加規則を別途確定する。
- `BUILDING → READY` は P3 の DB-side row/child mutation fence と validation transaction を通す。READY は immutable artifact であり、current pointer ではない。P1 body Unit/coverage の全 authoritative item と lexical の検証、P3 Graph の READY/owner/relation/digest 検証、optional Vector の選定済み receipt を済ませる。`Retryable` item、欠損/不一致、Source snapshot 変化なら新 key は非公開のまま、旧 pointer を維持する。
- `publish_if_current` は expected `(current key, manifest/bundle digest, pointer_revision)` を Source lock 下で再照合し、同じ key の READY Projection/lexical/Graph 等の receipt を検証して**一回の CAS**で pointer と revision を確定する。候補は transaction 終了まで staging lease/guard で GC から保護する。CAS 敗北は旧 pointer を戻さず、敗北 artifact は guarded discard へ。Source I/O、lexical file I/O、provider call を DB row lock 中に行わない。
- `pin_current` は Source→generation lock と同じ DB transaction で current READY key/manifest/Graph receipt を確認し、server-issued evaluation lease を挿入して commit してから key を返す。外部指定 generation/evaluation ID は pin 権限にしない。旧 key は新 publish 後も lease expiry/release まで immutable。Graph read は P3 の read-only snapshot と return 前の DB-clock lease 再検査を通し、途中で欠損/失効すれば別 key へ移らず結果全体を fail closed にする。P4 の一時 `EvaluationLease` と durable PG pin lease は別 lifecycle。
- `retire_unpinned` / `discard_unpublished` は current key、有効 evaluation lease、**base/target 双方の有効 build guard**を再確認し、どれかがあれば物理削除しない。期限切れ guard の target cleanup は Source→sorted generation→guard→lease の lock と検証を保ち、同一 transaction で target `DELETING`、**guard DELETE を先に**、target 子行→target generation の順に削除する（guard FK は `ON DELETE RESTRICT`）。失敗時は全 rollback で guard と target を保持する。Search receipt は過去の event metadata であって GC pin ではない。

## 4. Outbox と build guard を含む lock / fence 順

1. 単一の DB transaction が双方へ触れる場合の順序は **outbox event row → Source control row → generation rows (`SourceId`, key 順) → build guard rows (`source_id,target_generation_id` 順) → evaluation lease rows (ID 順) → Search receipt**。Source だけの操作は Source から開始し、複数 Source は ID 順に処理する。P3 stage/copy/validate の Source-less batch は generation→guard の順だけを取り、後から Source lock を取らない。全経路で逆順取得を禁止する。
2. Search bridge は **別の短い transaction**で Source lease を先に取得し、次に outbox event を 1 件だけ claim する。長い build 中は Source (`owner_token`,`fence_epoch`) と outbox (`lease_token`) の双方を bounded heartbeat し、片方の失効/不明で協調 cancel。generic P6 worker の batch / 複数 in-flight 能力を Search route に流用しない。Source lease は event ごとに解放し、次の取得は epoch を増やす。
3. publish 直前に双方を renew し、P6 `complete_event_if_current` は上記順の**短い同一 PG transaction**で outbox token・未完了・DB clock expiry、Source token/epoch/expiry、`last_published_epoch <= epoch`、expected pointer/revision、同一 generation の READY receipt を再検証する。pointer CAS と `(source_id,event_id)` の epoch 単調 Search receipt 書込みを同じ commit に含める。同 epoch は key/digest 完全一致のときのみ idempotent。P6 receipt の `digest` は P1 の projection-only 値だけでは body 変更を識別できないため、versioned composite digest の併記または同等の照合を Freeze で決め、no-op 判定でも両方を確認する。`Published`/`Unchanged`/`Duplicate` はこの境界の確認後だけ成功。outbox `delivered_at` ack は**別 transaction**で token-fenced に行い、ack 不明なら再配送時に current pointer + READY + receipt を再読して収束する。manual rebuild は outbox row を持たないが同じ Source fence/pointer 条件を使い、pending event を ack しない。
4. P3 追補の incremental build は copy 前に Source→sorted(base READY,target BUILDING)→guard で `BuildGuardHandle {source,base,target,token,fence}` を登録し、`build_fence_seq` を増す。guard は base と target 両方を保護し、pointer revision が進んでも有効。各 copy/delta batch は sorted generation→guard lock、同じ token/fence/DB expiry、base immutable receipt、target BUILDING、copy verified state を検査し、commit 直前にも expiry を確認する。`validate_ready` は guard を解除しない。CAS 成功と guard DELETE は同じ publish commit、CAS 敗北は guard を保持して明示 abort。abort/expiry cleanup は §3 の FK-safe 削除順を使う。失効後は target を復活させず新 key から再試行する。
5. lock wait、deadlock、serialization failure は transaction 全体を rollback し、同じ expected key/revision と idempotency key で bounded retry して全条件を再評価する。上限後は明示 failure。DB lock、Source lease、outbox lease を Source 読取・抽出・Graph/lexical build の長時間 I/O の代わりにしない。

## 5. Health / degraded / admission

| signal | 判定 |
| --- | --- |
| liveness | process/event loop が応答し停止処理が可能。DB/Remote 障害だけで process を再起動ループにしない。 |
| global readiness | trusted identity/tenant/visibility、registry invariant、coordinator DB/schema/roles、current pointer 検査、必須 Audit 生成経路、API final gate と選定済み port が安全に応答できること。未配線 port、schema 不一致、fence 不可、全 Source の安全な処理不能は unready。 |
| Source readiness | 内部管理面で Source ごとに `ready / rebuilding / degraded / unavailable`、current generation/key/digest と Projection・body coverage・Lexical・Graph・選定時 Vector の検証結果、remote expiry、outbox backlog/oldest age/DLQ を分ける。stale/corrupt artifact を ready としない。 |

- 一部 Remote Source の outage/timeout はその可視 Source の degraded とし、安全な他 Source の Search/Discovery は P5 の `partial`/required evidence 規則で継続可能。hidden Source の状態を公開応答や public health に漏らさず、actor-visible Source のみ固定 code の gap とする。registry/visibility 全体または final gate 不可は partial 200 にしない。
- ready は freshness の保証ではない。outbox lag、Source snapshot と current pointer の差、build/receipt/Graph/index 欠損を別 signal にする。具体的閾値と alert/SLO は小・中・大の合成/public corpus 測定後に提案し、benchmark 観測と保証 SLO を分ける。

## 6. Restart / corruption / rebuild の判断

| 故障 | recovery 契約 |
| --- | --- |
| process restart / `kill -9` | 新規 claim/publish 前に registry・schema・pointer/READY/bundle を再検証。期限切れ Source/outbox/evaluation lease は DB clock と fence に従い回収。未確認 ack は event と current receipt を再読し、RAM cursor/session/remote generation は復元せず stale 扱い。 |
| interrupted BUILDING / incremental guard | commit 済み batch だけを検証可能な cursor とし、有効 guard は勝手に削除しない。guard 失効なら Source→両 generation→guard の guarded cleanup で未公開 target を terminal/delete し、旧 handle を拒否。証明不能なら別 key の full rebuild。 |
| stale generation / source revision change | 新 Source snapshot または current Version/access と不一致なら旧 artifact を結果へ流さず blocking gap / fail closed。新 key を Source 正本から再構築して CAS、旧 current/pin を保持。remote miss/outage を削除・absence とみなさない。 |
| corrupt/missing Projection・Unit・Lexical・Graph・Vector | key/receipt/digest を quarantine/unready にして query へ出さない。Source 正本と承認済み profile/schema から full index/Graph を別 key へ rebuild、full↔incremental digest・result parity を検査して公開。Graph だけ READY や HTTP 200 を runtime 復旧と数えない。 |
| provider outage / outbox backlog | bounded retry/backoff と Source-specific alarm。既存許可済み結果だけを retention/current access の範囲で返す。復帰後に正本再読、fenced event replay/receipt 修復を行い、event payload を索引正本としない。DLQ は原 event を保持し結果不明と失敗確定を区別する。 |

復旧資格は restart、kill、publish↔pin/GC、guard copy↔GC、corrupt row/index/digest、full rebuild、Graph rebuild、stale generation、outbox replay/ack 不明、provider outage を fault injection と実 DB で確認する。環境 inventory は空き 12 GiB・98% 使用、PDFium の pinned dylib は別 worktree にのみ存在し、DB image は既存だが container/test 未実行という時点観測である。P7 の容量・性能・復旧測定は実行直前に容量を再計測し、固定 SLO を先に置かない。

## 7. Audit / OpenTelemetry / retention

- Audit と OpenTelemetry trace/metric/log は別 port/保持・権限。必須 Audit event の生成は業務 transaction の規範に従い、OTel collector の停止だけで通常 Search/Document を停止しない。outbox Search receipt と Audit outbox ack を混同しない。
- trace は許可された境界で `discovery_evaluation_id`、`need_id`、generation、Source routing、candidate/Graph/probe/materialization/evidence/remote/extraction/outbox stage の**固定 code・count・duration**を相関する。ID は必要な trace/audit の管理境界に置き、Source/Resource/Query/Evaluation ID を metric label にせず、bounded low-cardinality の stage/status/source category だけを metric dimension にする。本文、query 全文、credential、provider native locator、Graph path、無制限 candidate/Claim 値を通常 telemetry/audit に載せない。
- P4 §7 の `Owner {TrustedSearchScope, AuthorizedSourceScope, evaluation?, retention_mode}` と bounded `Lease` を全 store/read handle に適用する。`SESSION_ONLY` は session RAM のみ、`CACHE_WITH_EXPIRY` は provider TTL 等の短い期限、persistent mode は field proof の範囲のみ。`NO_RETENTION` は evaluation 終了時に全 remote-derived RAM/Graph/probe/receipt を閉じ、許可済み field の `TransientDisclosure<T>` だけを送出期限まで保持し、送出成功・error・cancel・disconnect 後に破棄する。cursor/cache/disk/spool/audit/telemetry/fixture へ複製せず、request/body debug log と core dump を無効にできない構成は起動拒否。通常 trace に provider 由来の per-call payload を作らない。
- API response は P5 の最終 actor/Source/item/field gate が済むまで送出しない。`NO_RETENTION` の短命 disclosure はその gate 後に bounded stream へ渡し、backpressure 中も lease/deadline を守る。RAM cursor は P5 の session-bound opaque handle のみで、`NO_RETENTION` の continuation を発行しない。

## 8. Freeze 前の照合条件

1. P1 の `GenerationBundleReceipt` と P3 Graph receipt/guard、P2 Vector 採否・versioned receipt、P6 event receipt の key/digest/expiry を一つの実 DB + file crash protocol に照合し、DB migration、role、trigger、transaction-bound port の実装境界を固定する。
2. P4 `TrustedSearchScope`/`AuthorizedSourceScope` と P5 `TrustedSearchContext`/`AuthorizedSourceSet`、Claim/locator final gate、`NO_RETENTION` response lifetime、public cursor/Problem/OpenAPI を同一実 HTTP/application 経路で照合する。P5 の transport/security scheme と wire enum は revised design のままで未決定。
3. Graph/Vector backend と Vector 採用可否、embedding/fusion、物理配置、timeout/lease TTL、capacity と proposed SLO は PoC/benchmark/安全性/ライセンス/復旧 receipt を得てから選ぶ。P3 の PostgreSQL は第一候補で確定ではない。production 接続の承認は P1〜P6 Freeze、独立 architecture/security review、rebuild・多 process fence・NO_RETENTION・actual API E2E と final exact-head hosted gate の結果を要する。
