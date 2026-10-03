# P7 Production Runtime — design revision 1

Status: **REVISED PROPOSAL / independent re-review 待ち / Freeze 前**（2026-09-30）。[元設計](p7-runtime-design-completion.md) SHA-256 `df0f2ba4e91526a24567addfac3777a7d7219ae296d5a3dc4b0dcc0c51737f3e` に対する差分であり、[NO-GO review](p7-runtime-architecture-review.md) の5件だけを閉じる提案である。抵触箇所は本改訂を優先する。`spec/` が規範正本であり、本書の作成は設計GO、production code、最終受入、SLO保証、merge、live deployを意味しない。

## 1. 変更しない境界

単一 Rust composition root、P7-01〜12 の同一 PostgreSQL Source/current/READY/pin/guard、P1 Document/Unit/lexical、採択済み P3 Graph、P4 Remote RAM evaluation、P2 neutral core、P5 四routeと二lease、P6 Search receiptとgeneric ackの分離を維持する。P7-Rxx はこれらの accepted producer を接続し、第二の Source pointer、actor mint、Search receipt、Domain outbox ack を作らない。P3-P04 が未採択なら Graph production READY/publish と P7 runtime READY は閉じ、非PG採択なら共有publication protocolの別設計・独立reviewを先に要する。Vector `Disabled` もP1 lexical/P3 Graph/current/final accessを省略しない。最終 P1〜P6 receipt は R09 にだけ fan-in する。

## 2. Trusted host inventory → PostgreSQL ledger → listener

`HostRegistrationSnapshotPort` の production adapter は trusted host の**実際の登録正本**を読む。物理入力は、host登録writerが全tenant・両namespaceを一つのauthority revisionとしてatomic publishするversioned `HostRegistrationInventory` とし、R02のread-only adapterがhost設定のinventory referenceから読み取る。独立したtenant roster、登録0件のtenant、Document/Remote双方のnamespace、host deployment epoch、namespace別revision・canonical digest、各tenantの完全性証明、発行元とatomic publish証拠を含む。任意の手編集mapやSearch DB queryをこのmanifestに昇格しない。configの型名、`from_complete_host_inventory` というconstructor名、候補mapの存在やdigest自己一致だけを完全性証明としない。request/provider、Source自身、Search DBからhost authorityを逆算しない。hostの実writerがこの完全inventoryを発行できなければ起動拒否する。

R02 は host adapterから二namespaceを `CompleteDesiredRegistrations::capture(...).await` で取得し、hostの全tenant集合・epoch・revision/digestと照合する。P7-02 の唯一の `PgPool` backed `SourceRegistrationLedgerPort` を同一PG Source ledgerに構築し、両集合をそのportでreconcileした `SourceRegistrationCatalog::try_new(ledger, &document, &remote).await` を完了させる。commit直前/直後のhost revision・digest再読、DBのowner/kind/activation/currentとcatalogの照合を通してからだけ P5-08 の四route factory と P6-I04 claim pathを開く。async portを同期shimや `block_on` で包まない。

二namespaceの片方が失敗しても、途中のDB更新をactor向けcatalogとして公開しない。部分tenant、片namespace、stale revision、同revision別digest、source collision、commit応答不明はAPI/listenerとclaimを閉じ、host正本とPGを別接続で再読して再reconcileする。restart時も同じ順序を繰り返す。reload時は新catalogを完全検証後に切り替え、旧catalogのcurrent gateを不確定状態で開放しない。P7-12の実index/current scanはこの後、listenerより前に置く。

| sole owner | 実装責務 | 受け渡し |
| --- | --- | --- |
| P7-02 | `search-runtime` のSQL ledger adapter、Source ownership/activation/currentのatomic reconcileと実role | SQLx-free `SourceRegistrationLedgerPort` と実PG receiptをR02へ渡す |
| P7-R02 | host authority adapter/全tenant証明、async capture、ledger/catalog組立て、起動admission | 検証済み単一catalog/portsをP5-08、P6-I04へ渡す |
| P5-08 | 既存の四route factoryと実HTTP横断 | R02の検証済みcatalogを消費し、host inventory/第二ledgerを作らない |

既存P5-08のstartup named testsはR02の実PG試験を再利用する。`search-runtime/src/api.rs` のroute factoryと `composition.rs` のhost admissionは共有root writer windowで直列化する。

## 3. Audit production path はP6 generic配送と独立

規範 `spec/operations/observability-audit-requirements-v0.md` §14–15の必須classをaudit policy表で列挙する。Document create/version/publish/withdraw、file access、policy/role変更等の**既存Document transaction producer**は `document-application` の `AuditEventRecord` と `document-repository-postgres` の同一transaction `audit_outbox_events` INSERTを消費する。実装済みかは各classの実DB試験で照合し、欠落はそのDocument operationのownerへ戻す。P7 runtimeが所有するprivileged Search管理、audit設定変更、integrity violation、destructive maintenance、重要な認証/認可拒否は、その操作のtransaction ownerが同じDB transaction（拒否は独立の拒否記録transaction）でtyped audit outbox rowを作り、INSERT失敗時はcommit/許可しない。Auditに決めたeventはsamplingしない。

| 規範§14.1の必須class | business/denial transaction writer | typed Audit INSERTの所有・判定 |
| --- | --- | --- |
| 重要なauthentication/authorization failure | P5 identity/routeまたはDocument認可を実行したownerの独立拒否記録transaction | 対象存在を漏らさないreason enum。監査INSERT不能ならアクセス拒否を維持 |
| privilege/role/access policy変更 | Document access-policy transactionまたはP7管理設定transactionのowner | 変更と同じtransaction。既存producerを実DB照合 |
| document create/version create/publish/withdraw | 該当Document repository transactionのowner | 既存`AuditEventRecord`/`audit_outbox_events` producerを消費し、欠落classはそのownerへ返す |
| document export/download | 原本byte開示前のDocument file-access transactionのowner | `document.file.access_granted`等の実producerを照合し、Audit失敗時に開示しない |
| privileged management / audit configuration change | P7管理commandまたは該当Document管理transactionのowner | 管理変更と同一transaction。R04Aはtyped port/配送を提供 |
| integrity violation / destructive maintenance | 検出componentの隔離記録transaction、またはR05管理command transactionのowner | 安全状態を維持したうえで必須Audit rowを作り、失敗を黙殺しない |

`document.read`、`search.execute`、`search.result.open` の高量event採否は規範writerとsecurity/product ownerが保持量・閲覧権とともに事前決定する。未決定classをOTel eventで代替したり、必須扱いを黙って省いたりしない。採用したclassは全件Audit、未採用はAudit対象外と明示する。retentionとfield許可は§4のtyped policyを適用する。

P7-R04A がAudit専用のadditive SQL migration、typed append-only `audit_outbox_events` source projection、独立Audit配送worker、Audit Store向け `AuditSinkPort` を所有する。既存sourceの`attempt_count`/`delivered_at`を維持し、`available_at`、`lease_token`、`lease_owner`、`lease_expires_at`、`dead_lettered_at`、閉じた`last_error_code`と試行上限をadditiveに持たせる。sourceのevent列は不変で、配送状態列だけをlease/fence付きで更新する。sinkは別のappend-only relational `audit_store_events` 行（`event_id` unique、schema version、typed event class/result/reason/time、origin component enum、retention上許されたstable actor/subject reference）として実DBで資格を取る。既存source `data JSONB`をsinkに丸ごと渡さず、versioned decoderとclass別allowlistから列へ投影する。sink roleはINSERT/重複照会のみ、UPDATE/DELETE不可。Audit配送roleはsourceのSELECTとAudit配送状態列の限定UPDATEのみで、Domain `outbox_events.delivered_at`、Search receipt、business rowには書けない。business writerはAudit INSERTのみ、Audit配送ack権を持たない。閲覧は別の監査reader roleと保持policyで制御する。

配送はat-least-once、event_id冪等、duplicate detection、bounded retry/lease、failed/DLQ可視性を持つ。sink commit後にsource ackが不明ならsinkをevent_idで照会し、存在と内容一致を確認してからAudit側だけをsettleする。source claim/settleのcommit応答不明もDB再読とfenceで確定し、推測でackしない。Audit Store停止ではbusiness commit済みsource rowを保持し、別経路の再送で復旧する。P6のgeneric `audit_delivery` 非対象を変更せず、P6 Domain ackをAudit ackとみなさない。live Audit Store製品・credentialは別選定であり、ここではdisposableな別PG sinkでprotocolを証明する。

## 4. Retention-aware output policy と二lease

出力は生の任意key/valueではなく、`SinkKind × RetentionMode × VisibilityClass` で選ぶclosed typed allowlistから作る。未列挙fieldはdefault deny。全sinkでquery/content/raw provider response、provider native ID/locator、candidate ID、具体gap、Graph path/probe/receipt、digest、response bytes、SecretRefおよび秘密値、hidden SourceのID/存在/countを禁止する。秘密値はpanic/Debug/error/config dumpにも現れない。高cardinality IDをmetric labelにしない。actor-visible判定前のSource由来情報は内部sinkにも流さない。必要な運用相関IDはretention modeと権限ごとに下表の最小集合だけを使う。

| sink | 許可する最小field | 禁止/追加条件 |
| --- | --- | --- |
| public health / Problem | 固定status・reason code、全体readinessのみ | tenant/Source ID、可視・不可視別件数、具体gap、内部revisionなし |
| authenticated admin diagnostics | 対象actorに可視なSourceのtyped status、許可されたcurrent keyの存在bit、集計lag bucket | 原始ID、digest、payload、hidden countなし。権限を再確認し、監査閲覧roleと分離 |
| config / CLI / debug / panic | schema version、component category、validation code、参照の存在bit | `SecretRef`表現・secret値・endpoint credential・tenant/Source固有値なし |
| structured logs | route enum、stage enum、status/error code enum、bounded duration/size bucket | freeform error文字列、per-call provider属性、raw IDsなし |
| metrics | stage/status/route/source-kind enumとbounded count/duration/size bucket | IDラベル、hidden Source別count、query/payloadなし |
| traces | stage/status/route enum、bounded duration/count/size bucket。持続保持モードでのみ権限制限付きopaque operation correlation ID | `NO_RETENTION`/`SessionOnly`のprovider由来per-call属性・IDなし。raw native IDなし |
| Audit source/sink | §3でpolicy採用したtyped class/result/reason、必要なstable subject/actor referenceを監査roleに限定 | Remote内容/具体gap/provider IDなし。`NO_RETENTION`はprovider由来per-call payload/IDを一切生成しない |
| exporter buffer / error / shutdown report | 上記telemetry typed envelopeとdrop/flush count・status enumのみ | retry spool/未送信buffer/error chainに禁止値なし。shutdown後のhandleは再読不能 |

`PersistentResource` と `PersistentDiscoveryMetadata` はそのSourceで保持が明示許可されたstable identifierだけを認証済みAudit/管理面に記録できる。後者は本文Unitやembeddingの許可ではない。`CacheWithExpiry` はTTLを越える相関IDを持たず、`SessionOnly` はsession終了後のper-call IDを持たない。`NoRetention` はper-call provider-derived attributeを生成せず、共通のstage/status/count/size bucketだけを許す。count bucketもhidden Source数を逆算できる場合は抑止する。これらは公開DTOの許可ではなくsink側の上限である。

P4 `EvaluationLease` をevaluation success/error/cancel/deadlineで閉じ、P5-07 の `TransientDisclosure<T>` を実socket送出完了、write error、disconnect、cancel、deadlineで閉じる。handler returnやbody EOFのみを送出完了と扱わない。R04Oのsentinel試験は全sink、exporter queue/flush/drop、error chain、buffer、保持handleとshutdown後を捕捉する。合成sentinelは実行時に作り、fixtureへ保存しない。

## 5. OTLP transport は隔離PoCで選定

`opentelemetry`/`opentelemetry_sdk`/`opentelemetry-otlp` の選定と、HTTP/protobuf対gRPCのtransport `POC REQUIRED` を分ける。P7-R04P はproduction workspace外の隔離PoCで双方の正確なcrate version・feature set・Cargo.lock dependency closureを固定し、公式API、license/advisory/source、macOS/Linux cross-compile、local Collector interop、trace context、停止信号下のbounded queue/flush/drop、必要性能を同一条件で比較する。独立review後、parent backend decision と規範writerのselection記録・version/feature pinが揃うまでR04Oにtransport dependencyを追加しない。選定前はportとlocal in-memory captureだけでprivacy契約を実装する。R06 imageは未資格transportを含めない。

## 6. Capacity manifest、測定予算、SLO境界

R07のimmutable workload manifestは各runでtenant数、Document/Remote Source数とtenant別分布、Document/Version/Resource/Unit数と分布、Graph relation/participant/degree数と分布、synthetic request/provider payload bytes分布、Remote fanout、HTTP同時数、worker in-flight/backlog、cold/warm定義、固定seed・fixture/asset hash・candidate build hash、failure schedule、繰返しを宣言する。coldは新process・既存immutable indexを再openしてquery cacheを空にした最初の測定、warmは同一processで宣言済みwarmup後の測定とし、DB/OS cacheを消したという主張は別証拠がある場合だけ付ける。合成payload本文だけを生成し、顧客データを入れない。少なくとも複数tenant・両Source kind・Document→Unit→GraphとRemote fanout・HTTP/worker併走を含む代表shapeを依頼文§14のruntime要件に結び、P3-P04の100/1,000/3,000 relation group資格を縮小しない。

先に小さなbounded pilotを行い、実測wallclock、peak RSS、DB/index/Graph disk増分、free-space reserve、backlog drainから**有効な最大run時間・資源予算**とpre-admission式を固定する。各runは空きdisk/reserve、available memory/予測peak、wallclock、low-space/memory hard stopを実行前に判定し、途中も監視して停止・checkpoint・安全なresumeを行う。100/1,000/3,000 envelopeは各段階がこの予算に収まる場合だけ進め、未実行は理由付き `NOT_ADMITTED` とする。欠測をPASSや外挿値にしない。raw/aggregateはp50/p95/p99、throughput、RSS peak、disk、fanout/payload実績、cold/warm、fault結果と測定不能理由を残す。R08はmanifestと想定業務量が一致する範囲だけproposed operational SLOを作り、観測値・提案値・対外保証値を区別する。

## 7. 再審査条件

本改訂と[改訂計画](p7-runtime-plan-revision-1.md)を旧2文書に合成して独立architecture/security reviewerが5件を再判定する。GO後にparentがexact hashのruntime freezeを別記録し、production taskを始める。P7-01〜12/P5/P6の既承認semanticsを本改訂だけで変更せず、P5実socket、P6 unknown COMMIT/fenced ack、P7-12別DB restoreの実receiptはR09で照合する。specific cloud、live credential、merge/deploy、本番migrationはこの設計資格に含めない。
