# P7 Production Runtime Implementation Plan — revision 1

> **For agentic workers:** Toolbox v8 `orchestrate` → `toolbox-context` の一責務workerでtaskごとにRED→GREEN→独立read-only reviewを行う。`superpowers:executing-plans` のstep粒度を用いる。チェック欄は実装時の追跡用であり、本書は実行receiptではない。

**Goal:** [runtime設計改訂1](p7-runtime-design-revision-1.md)のhost authority、Audit、privacy、OTLP transport、capacityの5件を実装可能な責務と受入試験に落とす。

**Architecture:** [旧計画](p7-runtime-plan-proposal.md) SHA-256 `acecf896ad5481955b805728bb8c0d970e154657e1a4d5a89f7c100f3bc0ed3a` のR01〜R09を保ち、本書の変更箇所を優先する。P7-02の単一SQL ledger、R02のhost admission、P5-08の四routeを順に接続する。AuditはP6 generic配送から分け、telemetry transportは隔離PoCで選定する。

**Tech Stack:** 既存Rust/SQLx/PostgreSQL 18.6/Tokio、P5実TCP、隔離PoC内のOpenTelemetry 0.32.x/OTLP Collector、Pythonのcapacity harness。`POC REQUIRED` dependencyは資格前にproduction rootへ追加しない。

**Spec:** [設計改訂1](p7-runtime-design-revision-1.md)と[旧設計](p7-runtime-design-completion.md) SHA-256 `df0f2ba4e91526a24567addfac3777a7d7219ae296d5a3dc4b0dcc0c51737f3e`、[独立NO-GO](p7-runtime-architecture-review.md)、[P7 shared freeze](p7-shared-durable-freeze.md)、`spec/operations/observability-audit-requirements-v0.md` §14–15, SD-O1/O2、`spec/selection/library-tool-selection-v0.md` §6.1/§25。

## Global constraints and file ownership

- **Plan状態:** 5件の独立re-reviewがGOし、parentが合成設計/計画のexact hashをFreeze記録する前にRxx production codeを開始しない。本書は承認済みFreezeではない。P7-01〜12、P5-07/08、P6-I04/S06/S07はaccepted producer aliasで、再実装しない。P3-P04 BlockedならGraph READY/publish/R02/R09 acceptanceを閉じる。最終P1〜P6 receiptはR09だけが受ける。
- **Root writer:** `crates/search-runtime/src/lib.rs`、root `Cargo.toml`/`Cargo.lock`、`Dockerfile`/`mise.toml`、P5 `src/api.rs` はparentが予約する単一integration windowで直列化する。R04PのCargoはisolated PoC内だけ。`spec/`、graph、journal、statusは本plan workerの対象外。
- **Security:** migration、registration、Search builder/coordinator/reader、generic delivery、Audit business writer/dispatcher/reader/sinkを実DB roleで分ける。secret値や顧客データをtest fixture・artifactに保存しない。`NoRetention`の二leaseとsocket終端はP4/P5実装を消費する。
- **Verification:** 各ownerがnamed focused RED→GREEN、実PostgreSQL別接続/実TCP/別processを必要に応じて取得し、対象crate strict Clippy/fmtと独立read-only reviewを記録する。不要なfull CI反復はしない。R09でexact-head hosted gateを一度判定する。空きdiskが不足する現在のdocs作業中にCargo/build/load試験は行わない。

| writer / task | 新規・変更予定file | 境界 |
| --- | --- | --- |
| P7-02（既存） | `crates/search-runtime/src/source_registration.rs`, `tests/source_registration.rs`、既存migration/rolesの専用window | 唯一の`PgPool` backed `SourceRegistrationLedgerPort`。R02はSQL/source pointerを増やさない |
| P7-R01/R02/R03 | `crates/search-runtime/src/{config,host_registration,composition,startup,health,shutdown}.rs`, `tests/{config_contract,runtime_wiring,health_lifecycle}.rs` | R02だけがhost inventory adapterとadmissionを所有。P5-08の`src/api.rs`/四route factoryを消費 |
| P7-R04P | `experiments/search-otlp-transport-poc/{Cargo.toml,Cargo.lock,src/main.rs,collector.yaml,qualification.md}` | production Cargoを触らない。parentが採択しnormative writerがpin記録 |
| P7-R04A | `crates/document-repository-postgres/migrations/0010_audit_delivery_v0.sql`, `crates/search-runtime/src/audit/{model,source_store,sink_pg,worker}.rs`, `crates/search-runtime/sql/{audit_roles,audit_sink}.sql`, `crates/search-runtime/tests/audit_delivery.rs` | Audit専用SQL/role/source/sink/worker。Domain 0009後に単一writer。Document producer欠落は当該operation ownerへ返す |
| P7-R04O | `crates/search-runtime/src/{observability,telemetry_policy}.rs`, `crates/search-runtime/tests/observability_privacy.rs` | typed policy/OTLP adapter。Audit append/ackはR04Aのportを消費 |
| P7-R05/R06 | 旧計画のrebuild/runbook、Dockerfile/mise/config/smoke file | R06はR04P採択pin・R04A/O実資格前にimage GOを出さない |
| P7-R07/R08/R09 | `experiments/search-runtime-capacity/{README.md,run.py,analyze.py,tests/test_manifest.py}`, `docs/superpowers/programs/search-platform-completion/{capacity,p7-operational-slo-proposal,p7-runtime-qualification,p7-runtime-code-review,p7-runtime-receipt}.md` | 測定・提案・最終receipt。production sourceをbenchmark中に編集しない |

## Review Focus

1. host inventoryが片namespaceまたは一tenantを落とす → R02 `startup_rejects_partial_tenant_or_namespace`。
2. Audit sink commit後のack応答不明とP6 Domain ack混同 → R04A `audit_unknown_commit_idempotent_settle` / `audit_roles_enforce_separate_acks`。
3. `NoRetention` sentinelがdisconnectまたはexporter dropのhandleに残る → R04O `no_retention_sentinel_all_paths_and_sinks`。
4. transportのdefault Cargo featureが未資格依存を混入させる → R04P `transport_feature_closure_matches_pins`。
5. peak RSS/low diskで3,000群を走らせる → R07 `budget_pre_admission_and_midrun_abort`。

## Tasks and order

### P7-R01 — configのhost inventory referenceとsecret境界（旧R01を補足）

**Files:** `crates/search-runtime/src/{config,secrets}.rs`, `tests/config_contract.rs`。**Produces:** `HostRegistrationInventoryRef`（trusted hostがatomic publishしたmanifestへの参照のみ）、typed `RuntimeConfigSnapshot`、redacted `SecretRef`。host writer/provenance/tenant rosterのない静的mapをproduction設定として受けない。

- [ ] `config_rejects_unowned_or_partial_inventory_ref` と `secret_value_never_enters_config_or_diagnostics` をREDで固定する。`cargo test --locked -p search-runtime --test config_contract`。
- [ ] immutable設定検証を実装し同testをGREENにする。R02が実host authorityを解決できない場合はR03へ進めない。既存R01のpath/deadline/policy試験を維持する。

### P7-R02 — trusted host authorityとsingle catalog起動（旧R02を置換）

**Files:** `crates/search-runtime/src/{host_registration,composition}.rs`, `tests/runtime_wiring.rs`。**Consumes:** R01のref、host登録writerが全tenant・両namespaceを一つのauthority revisionでatomic publishしたversioned `HostRegistrationInventory`、P7-02実PG ledger、P5-08四route factory、P6-I04 worker path、P7-12 current verifier。**Produces:** 検証済み一個の`Arc<SourceRegistrationCatalog>`とP5/P6へ渡すports。host manifestは独立tenant roster（0件tenantを含む）、二namespace、同一epoch、各revision/digest、writer/provenanceを必須にする。

- [ ] 実PG・別接続の `startup_rejects_partial_tenant_or_namespace`、`startup_rejects_stale_revision_and_same_revision_different_digest`、`startup_unknown_commit_closes_listener_and_claim_until_reread`、`restart_rejects_host_pg_inventory_mismatch` を先にREDにする。Document/Remoteの逆順、同SourceId衝突、0件tenant、host publish中断も含める。`cargo test --locked -p search-runtime --test runtime_wiring -- --test-threads=1`。
- [ ] R02のread-only host adapterを実装し、`CompleteDesiredRegistrations::capture(host, Document).await` と `capture(host, Remote).await` を同じhost epoch/tenant rosterに照合する。P7-02の`PgPool` adapterを一つだけ注入し、`SourceRegistrationCatalog::try_new(ledger, &document, &remote).await`、host/DB再読、P7-12 scanの順にawaitする。成功前はlistenerとclaimを閉じる。同期shim/`block_on`、synthetic authority、memory ledgerをproduction constructorから除く。
- [ ] 同じ実PG試験をGREENにし、P5-08 `cargo test --locked -p search-runtime --test search_api_runtime` の既存startup/four-route casesと照合する。P5-08は検証済みcatalogを引数で消費するだけとし、`src/api.rs`の変更はP5 writerに返す。hostの実writer/atomic publishを接続できない場合はR02 production資格を未達と記録する。

### P7-R03 — startup/health/shutdown（旧R03を補足）

**Files:** 旧R03の `src/{startup,health,shutdown}.rs`, `src/bin/search_service.rs`, `tests/health_lifecycle.rs`。**Consumes:** R02のcomplete catalogとP7-12 recovery、P5/P6 lease/claim drain。

- [ ] `host_or_ledger_mismatch_blocks_listen_and_claim`、`unknown_commit_restart_never_exposes_partial_catalog`、`audit_migration_or_writer_role_missing_blocks_ready`、既存Remote degraded/public health/signal testsをRED→GREENにする。`cargo test --locked -p search-runtime --test health_lifecycle`。R04AのDomain `0010`/Audit writer資格を最終readinessへ組み込み、listener/claimが開く時点、hidden countの不在、shutdown時の未確認ack不在を実processで検査する。R03はR04Aの実装と直列統合し、相互の実装前提を循環させない。

### P7-R04P — OTLP transportの隔離PoC（R04O exporter接続/R06の必須前提）

**Files:** 上表の`experiments/search-otlp-transport-poc/`だけ。**Produces:** 両候補のexact Cargo version/feature/lock/tree、license/advisory/source、Collector interop、bounded stop、cross-compile、比較表と独立review receipt。公式[`opentelemetry-otlp` 0.32.0 feature/API](https://docs.rs/opentelemetry-otlp/0.32.0/opentelemetry_otlp/)を基準に、最初の二候補をそれぞれ`default-features = false`で固定する。

| candidate | PoC `opentelemetry-otlp` pin / features | transport |
| --- | --- | --- |
| HTTP/protobuf | `=0.32.0`, `trace,metrics,logs,http-proto,reqwest-client,reqwest-rustls` | `with_http()` / `Protocol::HttpBinary` |
| gRPC | `=0.32.0`, `trace,metrics,logs,grpc-tonic,tls-ring,tls-roots` | `with_tonic()` |

`opentelemetry`/`opentelemetry_sdk`もPoC内で互換なexact `=0.32.0`とし、個別Cargo.lockを保存する。TLS/root有無のfeature closureを`cargo tree -e features`とmetadataで照合する。patch修正が必要なら両候補のpinを同時に改め、新manifest/hashで再測定する。

- [ ] `transport_feature_closure_matches_pins` をRED→GREENにし、各候補をmacOS/Linuxでcross-compile、`cargo deny` license/advisory/sourceとSBOM差分を記録する。local Collectorでtraces/metrics/logs、HTTP→async→worker context、collector停止/復帰、SIGTERM/CTRL-C下のqueue limit、flush deadline、drop count、error redactionを同一条件で反復する。停止がdeadlineを越える/秘密をerrorへ出す候補は失格とする。
- [ ] 独立review後にparentがtransportを明示採択/不採択し、normative writerが`spec/selection/library-tool-selection-v0.md`の`POC REQUIRED`判定・exact version/features/lock evidenceを更新する。採択記録前にproduction Cargo/rootやR06 imageへ昇格しない。双方不合格ならR04Oのport-only実装に留める。

### P7-R04A — Audit source、独立配送、relational sink（新しい専任task）

**Files:** 上表のDomain `0010` migration、`audit/{model,source_store,sink_pg,worker}.rs`、`sql/{audit_roles,audit_sink}.sql`、`tests/audit_delivery.rs`。**Consumes:** 既存Document `AuditEventRecord`/同一transaction producerと`audit_outbox_events`、規範§14–15、P6のgeneric非対象境界。**Produces:** typed mandatory-class policy、append-only source event列、Audit-only lease/settle、別PGのunique `event_id` append-only `audit_store_events` sink、独立role/retention/reader grant。Domain `0009`後にDomain migration writerを予約する。`0010`は既存`attempt_count`/`delivered_at`を保持し、`available_at`、`lease_token`/`lease_owner`/`lease_expires_at`、`dead_lettered_at`、閉じた`last_error_code`、`attempt_limit`とpartial-lease/double-terminal CHECKを加える。sink tableは`event_id` unique、schema version、event class/result/reason/time、origin component enum、許可されたstable actor/subject referenceを別列にし、sourceの`data JSONB`はversioned decoderでallowlist投影する。

- [ ] 設計改訂§3の全必須classを一行一classのpolicy表に写し、transaction owner、出所port、保持と閲覧権、producer実DB testを指定する。高量`document.read/search.execute/search.result.open`の採否はnormative writer/security-product ownerの明示記録を待ち、採用時samplingゼロ。未決定classを必須classの完了と数えない。
- [ ] `audit_insert_failure_rolls_back_business_transaction`、`audit_sink_outage_replays_without_domain_ack`、`audit_unknown_commit_idempotent_settle`、`audit_roles_enforce_separate_acks`、`audit_required_class_has_no_sampling`を別接続のsource PG＋別sink PGでREDにする。`cargo test --locked -p search-runtime --test audit_delivery -- --test-threads=1`。
- [ ] source event列UPDATE/DELETEを拒否するtrigger/roleとtyped validation、Audit専用claim/renew/retry/DLQ/settleを実装する。sinkはevent_id uniqueと同一内容照合で冪等化し、sink commit後ack不明ではsink/source双方を再読しfence一致時だけAudit `delivered_at`をsettleする。Audit writerはINSERTのみ、dispatcherはAudit配送列のみ、sink writerはINSERT/duplicate SELECTのみ、readerは権限付きSELECTのみ。P6 generic roleはAudit更新不可、Audit roleはDomain ack/Search receipt更新不可とする。GREEN後、Document producer欠落は当該operation ownerへ返し、R04AがDocument business mutationを複製しない。

### P7-R04O — sink別typed privacyと選定済みOTLP（旧R04を置換）

**Files:** `crates/search-runtime/src/{observability,telemetry_policy}.rs`, `tests/observability_privacy.rs`。**Consumes:** 設計改訂§4のsink表、R04A Audit port、P4/P5二leaseとP5-07実socket送出。transport接続時だけR04Pの採択pinを追加で消費する。**Produces:** `SinkKind × RetentionMode × VisibilityClass` closed enum policy、bounded stage/status/count/size bucket、default-deny exporter envelope。OTLP選定前はcapture adapterのみで試験する。

- [ ] `no_retention_sentinel_all_paths_and_sinks`をREDにする。runtime生成sentinelをquery、provider response/ID、candidate、gap、Graph/receipt/digest、SecretRef/secret、hidden Source ID/countへ配置し、success/error/cancel/disconnect/deadline、P5実socket write error、exporter queue flush/drop、shutdown後の全health/admin/config/debug/log/metric/trace/Audit/sink/buffer/error/保持handleを走査する。sentinelは保存fixtureにしない。`cargo test --locked -p search-runtime --test observability_privacy`とP5-07 `send_lifetime`を使う。
- [ ] typed allowlistを実装し全pathをGREENにする。`NoRetention`/`SessionOnly`のprovider由来per-call attributeを作らず、全modeで内容/query/raw/native ID/hidden count/secretを拒否する。R04P採択後だけexact pinでOTLP adapterをrootの予約windowに追加し、Collector停止でbusiness commit/Domain ack/Audit ackを変えないことを実processで確認する。

### P7-R05/R06 — recoveryとartifact（旧taskを維持しgateを追加）

R05は旧計画のP7-12別DB/別index root restore、manual rebuild、unknown ackのnamed testsを維持する。R06はR01〜R05に加え、R04A Audit role/worker/sink接続、R04Oのprivacy試験、R04P採択とnormative exact pinを入力にする。`image_rejects_unqualified_otlp_transport`を`crates/search-runtime/tests/container_smoke.rs`に追加し、未資格transport/secret混入でimage GOを拒否する。既存 `mise run container:search-smoke` はdisposable PG/local TCP/実role/SIGTERMで実行し、live deployは行わない。

### P7-R07 — immutable manifestと測定可能なenvelope（旧R07を置換）

**Files:** `experiments/search-runtime-capacity/{README.md,run.py,analyze.py,tests/test_manifest.py}`, `docs/superpowers/programs/search-platform-completion/capacity.md`。**Consumes:** R02〜06の同一candidate build、P3-P04測定契約、実CPU/RAM/disk inventory。**Produces:** immutable workload manifest、pilot budget receipt、admitted raw runs、集計と`NOT_ADMITTED`理由。

- [ ] `manifest_requires_complete_workload_axes`をREDにする。tenant別Document/Remote Source数、Document/Version/Resource/UnitとGraph relation/participant/degreeの数・分布、synthetic request/provider payload byte分布、provider fanout、HTTP concurrency、worker in-flight/backlog、cold/warm定義、seed、fixture/asset/build hash、fault schedule、repeat、role/hardwareを欠くmanifestは拒否する。coldは新process/既存index再open/空のquery cache、warmは同processで宣言済みwarmup後と固定し、DB/OS cache coldは実際に制御した場合だけ記す。実payload本文だけsynthetic生成。`python3 -m unittest discover -s experiments/search-runtime-capacity/tests`。
- [ ] bounded pilotでwallclock、peak RSS、DB/index/Graph disk増分、free-space reserve、backlog drainを実測し、最大run時間/RSS/disk reserve/available-memory hard stopと推定誤差をmanifestへ固定する。`budget_pre_admission_and_midrun_abort`をRED→GREENにし、開始前不適合を`NOT_ADMITTED`、途中low-space/memory/deadlineを停止・checkpoint、同seed/build/manifestのresumeだけ許す。実行済みと未実行の軸を混ぜない。
- [ ] 複数tenant・Document/Remote両kind・Document→Unit→Graph、fanout、HTTP/worker併走の代表shapeを実OCI/PG/FS/Graph/TCPで走らせる。100→1,000→3,000 relation groupは各段階がpilot予算に収まる時だけ進む。これはP3-P04固有のrequired qualificationを縮小しない。各runのp50/p95/p99、throughput、RSS peak、DB/index/Graph disk、fanout/payload実績、fault/coldwarm、欠測理由をraw/aggregate両方に残し、独立reviewで外挿範囲を確認する。SLO数値を先に置かない。

### P7-R08/R09 — SLO提案と最終fan-in（旧taskを維持）

R08はR07 immutable matrixと依頼者の想定業務量が対応する範囲だけproposed operational SLO/alertを作り、観測値・提案値・対外保証値を別欄にする。業務量/分布または測定が足りなければ提案未確定と追加測定を記す。R09は旧計画のfull runtime qualificationにR02全tenant startup、R04A Audit source/sink/fence、R04O全sink sentinel、R04P採択pin、R07 admission matrixを加える。P1〜P6最終receipt、P5 socket送出、P6 unknown COMMIT/epoch/fenced ack、P7-12別DB restoreは**R09だけ**で照合し、hosted exact-head/local/運用/未接続/liveを分ける。

## NO-GO five-finding closure map

| review finding | design paragraph | task / sole writer | file | named acceptance |
| --- | --- | --- | --- | --- |
| 1. host全tenant/二namespace配線 | 改訂§2 | P7-02 SQL、R02 host/composition、P5-08 route | `host_registration.rs`, `composition.rs`, `source_registration.rs`, `api.rs` | `startup_rejects_partial_tenant_or_namespace`, `startup_rejects_stale_revision_and_same_revision_different_digest`, `startup_unknown_commit_closes_listener_and_claim_until_reread`, `restart_rejects_host_pg_inventory_mismatch` |
| 2. Audit producer/配送断絶 | 改訂§3 | R04A Audit専任、Document operation producer owner | `0010_audit_delivery_v0.sql`, `audit/{source_store,sink_pg,worker}.rs`, `audit_roles.sql` | `audit_insert_failure_rolls_back_business_transaction`, `audit_sink_outage_replays_without_domain_ack`, `audit_unknown_commit_idempotent_settle`, `audit_roles_enforce_separate_acks` |
| 3. sink/retention漏出 | 改訂§4 | R04O privacy、P5-07 socket alias | `telemetry_policy.rs`, `observability_privacy.rs`, P5 `send.rs` | `no_retention_sentinel_all_paths_and_sinks`, P5 `send_lifetime` |
| 4. OTLP未資格昇格 | 改訂§5 | R04P隔離PoC、parent採択、normative writer pin、R06 image | `experiments/search-otlp-transport-poc/`, `container_smoke.rs` | `transport_feature_closure_matches_pins`, `image_rejects_unqualified_otlp_transport` |
| 5. workload軸/予算欠落 | 改訂§6 | R07 manifest/pilot、R08提案 | `experiments/search-runtime-capacity/{run.py,analyze.py,tests/test_manifest.py}`, `capacity.md` | `manifest_requires_complete_workload_axes`, `budget_pre_admission_and_midrun_abort` |

**Diff summary:** 旧R02へhost physical owner/async catalog/startup PG gateを追加し、旧R04をtransport PoC・Audit専任・privacy実装へ分割し、旧R06のdependency gateと旧R07のmanifest/pilot budgetを具体化した。R01/R03/R05/R08/R09の旧計画の非抵触条件は維持する。次はこの合成計画の独立architecture/security re-reviewであり、GOとparent freeze後だけcode taskをdispatchする。
