# P7 Production Runtime Implementation Plan — revision 2

Status: **PLAN PROPOSAL / independent architecture・security re-review 待ち / Freeze 前**（2026-10-01）。[設計改訂2](p7-runtime-design-revision-2.md) SHA-256 `415d5a1648702670eb5737eecfe16581b7565191f2e64409c143236a00e95461` の host publisher と typed Search/system Audit producer を実装責務・受入試験へ割り当てる。[計画改訂1](p7-runtime-plan-revision-1.md) の R01〜R09 を維持し、抵触する R01/R02/R03/R04A/R05/R09 と作業順のみ本書を優先する。チェック欄は将来の作業であり、code/SQL/試験 PASS receipt ではない。

## 1. Exact inputs と境界

作業木 `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` で執筆時に確認した SHA-256:

| 入力 | SHA-256 |
| --- | --- |
| `p7-runtime-design-revision-1.md` | `422e00dcb0c685feb2fc9f81755e25bdd555d2e4b7602ccadb85e4c281a9d035` |
| `p7-runtime-plan-revision-1.md` | `d24cba464e40f9d07d2baf9095d4ece1bb099f0f8b06cfaca6eee2e2982e3d57` |
| `p7-runtime-architecture-review-revision-1.md` | `a6f91b67ecfd220191486aa2cdd6715967a8115f9e0b9dc6aee8ecaee6c19e29` |
| `p7-runtime-architecture-review.md` | `0bf9ce336e664a3eb4a5e14c7f37ec9a351bc4edbfa9d6b8b6435f002088f793` |
| `p7-shared-durable-freeze.md` | `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110` |
| `p7-shared-durable-plan.md` | `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517` |

`spec/`、P7 shared freeze、P5/P6 owner、Graph P3-P04 gate が優先する。P7-Rxx は P7-01〜12 Source/current/READY/pin/guard/Search receipt、P5-08四route、P6-I04 generic workerを alias として消費し、第二 Source pointer、actor mint、Domain outbox ack を作らない。Graph未採択なら R02/R09 production acceptance を閉じ、P2 Vector `Disabled` は lexical/Graph/current/final access の省略ではない。P1〜P6 final receipt は R09 のみが fan-in する。共有 `lib.rs`、root Cargo、`src/api.rs`、Domain/Search migration は parent が一つずつ writer window を予約する。現行 production host publisher、Search/system Audit source、P5 rejection hook は未実装である。

## 2. Sole writer と順序

| task / sole writer | 予定 file・操作 | 消費/受け渡し |
| --- | --- | --- |
| P7-R01（既存） | `crates/search-runtime/src/{config,secrets}.rs`, `tests/config_contract.rs` | versioned `HostRegistrationInputV1`/inventory reference、redacted SecretRef。tenant roster と両 namespace 入力を検証するが publish はしない |
| P7-R04A-S（R04Aのschema/append範囲） | Domain `crates/document-repository-postgres/migrations/0010_audit_delivery_v0.sql`, `crates/search-runtime/src/audit/{model,source_store}.rs`, `sql/audit_roles.sql`, `tests/audit_source_schema.rs` | Document source配送列の additive 変更、別 `search_audit_outbox_events` とversioned `audit_policy_revision`、typed `append_search_audit_on`、実 role。R01P/P5/R05に同一transaction portを渡す |
| P7-R01P（新規、host publisherのみ） | Search `crates/search-runtime/migrations/0004_host_registration_inventory_v1.sql`, `src/host_inventory_publish.rs`, `tests/host_inventory_publish.rs` | R01の trusted input と既存 server-owned Document/Remote DTOを一つのhost SSOT revisionとしてatomic publish。R04A-SのAudit appendを同じtransactionで呼ぶ。`0003`の後の専用 Search migration writer |
| P7-R02（既存） | `src/{host_registration,composition}.rs`, `tests/runtime_wiring.rs` | R01Pのread-only inventory port→P7-02唯一のPG ledger→単一catalog→P5-08/P6-I04。publisher/第二 ledger/route handler を作らない |
| P7-R04A-C（R04Aのaudit policy command範囲） | `src/audit/admin.rs`, `tests/audit_policy_transaction.rs` | DB-backed policy revision変更とtyped Audit INSERTを同一transactionで行う。file/env直変更をactive policyにしない |
| P5-06/08既存 ownerの直列 integration window | `crates/search-api-http/src/auth.rs`, `crates/search-runtime/src/api.rs`, P5 HTTP/Runtime tests | R04A-Sのdenial portを認証・認可 rejection pointへ接続。R04A workerはP5 routerを編集しない |
| P7-R03 / P7-R05既存 owner | R03 `src/{startup,health,shutdown}.rs`/`tests/health_lifecycle.rs`、R05 Search `migrations/0005_search_maintenance_v1.sql` と `src/rebuild.rs`/`src/bin/search_admin.rs`/`tests/runtime_recovery.rs` | R05がversioned `search_maintenance_operation`/`search_integrity_quarantine` のsole schema・transaction writer。R03は検出してそのportを呼び、未記録時もunreadyにする。P7-11 GCやP7-12 restoreを書き直さない |
| P7-R04A-D（R04Aの配送範囲） | `src/audit/{sink_pg,worker}.rs`, `sql/audit_sink.sql`, `tests/audit_delivery.rs` | Document/Search両Audit source→別PG sink、Audit-only lease/fence/retry/DLQ/ack。P6 generic workerを変更しない |

Domain `0009` → Search `0001`〜`0003` → Domain `0010`/R04A-S → Search `0004`/R01P → R02 → Search `0005`/R05 の schema/host順を維持する。`0003`がまだ適用されていなければR01Pは migration番号を横取りしない。R01 typed parser は独立に先行可能だが、R01Pのpublisher GREENは R04A-S と P7-03 の accepted schema/role を待つ。R02はR01P実 publisher、P7-02実PG ledger、P5-08/P6-I04 code slice、P7-12 scanを待つ。R03のstartup骨格は先行可能だが、最終readinessは `0005`、Audit source/producer role の実照合を待ち、R06 artifact はR04A-Dを含む旧改訂1の全条件を待つ。R09だけが最終資格を判定する。

## 3. P7-R01P — 実 host inventory publisher

**Input:** R01のtrusted host `HostRegistrationInputV1`（独立tenant roster、0件tenant、Document/Remoteの全設定と宣言件数、deployment epoch、単調 authority revision）、`ServerDocumentRegistrationConfig`＋接続済み capability witness、`ServerRemoteRegistrationConfig`。config authoring input自体が不完全/証明不能なら実配備を起動しない。Search Source/ownership/current DBを roster の出典にしない。

**Transaction:** 一つのPG connectionで immutable inventory revision/tenant/両namespace/per-tenant count/digestをINSERTし、current headを旧revision条件付きCASし、`host.registration.changed` typed Audit rowをINSERTしてcommitする。host inventoryはP7 Source ledgerとは別表・別role、credential値なし。partial write・Audit failure・同revision異digest・旧revision・SourceId衝突は全rollback。commit応答不明は独立接続でhead/全row/Audit event_idを再読し、未確定のまま publish再試行やlistener/claim解放をしない。R02 reader用には旧/new headと全rowを単一read transactionで得るportだけを渡す。

- [ ] `host_inventory_publish_atomic_all_tenants_and_namespaces`、`host_inventory_rejects_missing_roster_tenant_or_namespace`、`host_inventory_rejects_stale_or_same_revision_new_digest` を実PG・別readerでRED→GREEN。0件tenant、逆順入力、Document witness欠落、Remote DTO不正、二writer競合、partial INSERT fault、source ID衝突を含める。予定 focused command は `cargo test --locked -p search-runtime --test host_inventory_publish -- --test-threads=1`。
- [ ] `host_inventory_publish_audit_failure_rolls_back_head_and_rows` と `host_inventory_unknown_commit_requires_reread` を同じ実PGでRED→GREEN。Audit INSERT failure injection後、別接続で旧head/旧二namespace/旧Audit状態だけが可視なことを検査する。unknown commitでは再読一致までR02 admissionが閉じることを検査する。実writer/read-only/registration/dispatcher roleのgrant超過を拒否する。

## 4. P7-R02 — roster/revision照合とrestart admission

R02は host inventory current headのepoch/revision、独立tenant roster、両namespaceのtenant別宣言件数/実件数とcanonical digest、raw typed DTOをread-only snapshotから得る。host authoring inputのrevision/rosterと照合し、`CompleteDesiredRegistrations::capture(..., Document/Remote).await` を同じauthority revisionに束縛する。P7-02の`PgPool` ledgerで両setをreconcileし、`SourceRegistrationCatalog::try_new(...).await`、別接続でhost head/PG serial/owner/kind/activation/currentを再読、P7-12 scan、その後にだけP5 listener/P6 claimを開く。`HostRegistrationSnapshot::from_complete_host_inventory` の名前とself-digestを完全性証明にしない。reconcile途中のDB更新をcatalogに出さず、restart/reloadは全体再検証後に一度だけ切替える。

- [ ] revision 1 の四 named test `startup_rejects_partial_tenant_or_namespace`、`startup_rejects_stale_revision_and_same_revision_different_digest`、`startup_unknown_commit_closes_listener_and_claim_until_reread`、`restart_rejects_host_pg_inventory_mismatch` を**R01P実publisherから作った inventory**でRED→GREENにする。合成manifest単独GREENは不可。別PG connection、別process restart、publisherの途中 fault、publish直後のrevision変更、raw config/host head/ledgerの三者不一致、0件tenant、一namespaceだけ先行、同SourceId衝突を含める。予定 focused commandは `cargo test --locked -p search-runtime --test runtime_wiring -- --test-threads=1`。
- [ ] P5-08 `search_api_runtime` の四route startup試験とP6-I04 claim閉鎖を実接続で照合する。`src/api.rs`はP5-08 ownerの予約windowでのみ変更する。host/ledger re-readが不明ならpublic healthは固定unready codeとし、Source/tenant情報を出さない。

## 5. P7-R04A-S/C/D と各 business producer — typed Audit

**Schema-first R04A-S:** Domain `0010`は既存 Document `audit_outbox_events` のevent列/`resource_type` CHECK/`resource_id NOT NULL`を維持して配送状態列だけをadditiveに増やし、別の append-only `search_audit_outbox_events` とversioned `audit_policy_revision` を作る。`search_audit_outbox_events` は schema version、閉じた class/type/origin/actor-kind/subject-kind/result/reason、UTC時刻、許された bounded actor/subject ref と配送状態のみ。System/UnknownTarget はUUID捏造やDocument型流用なし。任意JSONB payload、raw token/query/content/provider ID/secretは禁止。event列UPDATE/DELETE、無権限INSERT、Audit dispatcher以外のsettle、P6 genericによるAudit ackを実role/triggerで拒否する。`append_search_audit_on(&mut PgConnection, TypedSearchAuditEvent)` はtransaction-boundで、pool内部取得・独立commitをしない。予定 focused command は `cargo test --locked -p search-runtime --test audit_source_schema -- --test-threads=1`。

| class / producer責務 | named 実PG試験と失敗時判定 |
| --- | --- |
| Document create/version/publish/withdraw、role/policy、download | Document既存ownerが `document-application::AuditEventRecord` と `document-repository-postgres` の各 transaction の実INSERTを class 別に照合。`document_required_class_insert_failure_rolls_back_business_or_file_grant`。欠落は該当Document ownerに返し、R04Aがbusiness writeを複製しない |
| host registration・Search config | R01P `host_inventory_publish_audit_failure_rolls_back_head_and_rows`。head CASとAudit INSERTは一接続一commit |
| Audit config | R04A-C `audit_policy_change_insert_failure_rolls_back_revision`。versioned policy rowとAudit rowを一commit。未監査file/env reloadを有効化しない |
| privileged Search rebuild/management・destructive maintenance | R05 `management_audit_failure_rolls_back_admission_and_prevents_side_effect`、`maintenance_result_audit_failure_never_reports_success`。DB状態変更＋Auditを一commit、外部作用の前にdurable admission＋Auditをcommit |
| integrity violation | R03/R05 `integrity_audit_failure_keeps_quarantine_and_unready`。検出・隔離状態＋Auditは一transactionを試み、INSERT失敗でも外部公開を再開しない |
| 重要なSearch authentication/authorization failure | P5-06/08 `search_denial_audit_failure_still_denies_without_target_or_token`。R04A denial portで独立拒否記録transaction、失敗でも401/403を維持し運用障害を出す。Search/system rowはDocument `AccessPolicy`/nil UUIDを使わない |

R04A-Sは `audit_required_class_has_no_sampling` と class/type/subject/resultの未知値、hidden Source ref、NoRetention payload拒否をschema/portでRED→GREENにする。R04A-CとP5/R05/R03は上表の**自分の**mutation/rejection pointの試験を所有し、R04Aのdelivery試験だけで同一transaction producer済みと数えない。高量`document.read/search.execute/search.result.open`のpolicy採否は規範writer/security-product ownerの別記録を待ち、採用時は全件Audit。未決定classを必須class完了と数えない。

**Delivery R04A-D:** revision 1 の別PG relational `audit_store_events`、event_id unique/content一致、両sourceのorigin-tagged versioned decoder、Audit-only fenced claim/retry/DLQ/ackを実装する。`audit_sink_outage_replays_without_domain_ack`、`audit_unknown_commit_idempotent_settle`、`audit_roles_enforce_separate_acks` をDocument/Search source両方で別接続・別sink PGでRED→GREENにする。source claim/settle不明もfence付き再読、sink commit後ack不明もsink event_id/content照合後だけAudit側settle。予定 focused commandは `cargo test --locked -p search-runtime --test audit_delivery -- --test-threads=1`。Audit Store停止はbusiness committed rowを保持し、P6 Domain ackやSearch receiptへ波及させない。R03 readinessはschema、writer role、mandatory producer hookの配線を確認し、sink停止を既commit業務の取消しとみなさない。

## 6. 維持する3件と資格 gate

revision 1 §4/R04Oの sink別 `SinkKind × RetentionMode × VisibilityClass` default denyと `NoRetention` 全sink/success/error/cancel/disconnect/deadline/exporter/shutdown/保持handle sentinel、§5/R04Pの隔離 HTTP/protobuf 対 gRPC OTLP transport PoC・exact version/features/license/Collector/stop比較・独立採択前production Cargo/R06 image拒否、§6/R07-R08の immutable Source/tenant/Document/Unit/Graph/payload/fanout/concurrency/backlog/cold-warm/fault manifest、pilot budget、pre-admission/途中停止/`NOT_ADMITTED`、測定範囲内だけのSLO提案は変更しない。revision 1 reviewerの設計上 CLOSED の判定を維持し、実装結果へ読み替えない。

R09 は R01P inventory publish/restart、R02 host→P7 ledger→P5/P6、R04A-S/C/DとDocument/P5/R03/R05 producerの全class別実PG/role試験、R04O sentinel、R04P選定pin、R07予算付き実測を集める。P1〜P6 final receipt、P5実socket、P6 unknown COMMIT/fenced ack、P7-12別DB restore、P3 native qualification、exact-head hosted CI、live接続は各独立欄にする。次に本書と設計改訂2のexact hashで独立architecture/security再審査を行い、GO後にだけparentが別の合成Freezeを記録する。現時点で implementation/qualification/production readiness、merge、本番migration、live deployは未了である。
