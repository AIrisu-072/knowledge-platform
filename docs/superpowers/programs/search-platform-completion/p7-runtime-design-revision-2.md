# P7 Production Runtime — design revision 2

Status: **REVISED PROPOSAL / independent architecture・security re-review 待ち / Freeze 前**（2026-10-01）。[revision 1](p7-runtime-design-revision-1.md) のうち独立再審査で残った host inventory publisher と Search/system Audit producer の2件を具体化する差分である。抵触箇所は本書を優先し、それ以外は revision 1 と[元設計](p7-runtime-design-completion.md)を維持する。設計GO、実装、production資格、SLO保証、merge、live deploy の記録ではない。`spec/` が規範正本である。

## 1. 固定入力と現在の欠落

作業木は `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff`。下表は執筆時の SHA-256 であり、未commit文書の内容同一性を示すもので、承認済み freeze を意味しない。

| 入力 | SHA-256 |
| --- | --- |
| `p7-runtime-design-revision-1.md` | `422e00dcb0c685feb2fc9f81755e25bdd555d2e4b7602ccadb85e4c281a9d035` |
| `p7-runtime-plan-revision-1.md` | `d24cba464e40f9d07d2baf9095d4ece1bb099f0f8b06cfaca6eee2e2982e3d57` |
| `p7-runtime-architecture-review-revision-1.md` | `a6f91b67ecfd220191486aa2cdd6715967a8115f9e0b9dc6aee8ecaee6c19e29` |
| `p7-runtime-architecture-review.md` | `0bf9ce336e664a3eb4a5e14c7f37ec9a351bc4edbfa9d6b8b6435f002088f793` |
| `p7-shared-durable-freeze.md` | `20b5b64ac6c8e6209a3618e1c8f4577f1f48333991df0cffe9af96e2cbd5e110` |
| `p7-shared-durable-plan.md` | `97c122bf4447dd63caeac24930503247c11d52e533e1825d913f737162812517` |

現行 `crates/search-application/src/source_registration.rs:675–813` の host port は complete と名付けた constructor と synthetic fixture までで、production publisher はない。同 `ServerDocumentRegistrationConfig` と `crates/search-application/src/remote_registration.rs::ServerRemoteRegistrationConfig` は既存の server-owned 入力型である。現行 Document `audit_outbox_events` は `resource_id UUID NOT NULL`、`resource_type` は `Document|Folder|AccessPolicy` に限定される（`0001_document_authoritative_core.sql:86–100`、`0006_document_management_access_v0.sql:55–57`）。Search Source や system を `AccessPolicy`/nil UUID に偽装せず、専用の型付き source を追加する。

## 2. Host registration SSOT と atomic publisher

**発行元を P7-R01P の `search-runtime` host registration publisher に固定する。** R01 の trusted operator registration input を唯一の authoring input とし、`ServerDocumentRegistrationConfig`、`ServerRemoteRegistrationConfig`、登録0件の tenant を含む独立 `tenant_roster` を一つの versioned `HostRegistrationInputV1` として受ける。これは既存 Search application の server-owned DTO と runtime composition 境界を使う新規実装責務であり、現行 repository に production host publisher が実在するという主張ではない。request/provider、Search Source/ownership/current 行、tenant 別 query、手編集済み部分 map、synthetic host は入力・完全性証明に使わない。host が全 tenant を列挙する正本を提供できない配備は fail closed とする。外部 identity service や live host/credential は選ばない。

P7-R01P は `crates/search-runtime/src/host_inventory_publish.rs` と Search migration `0004_host_registration_inventory_v1.sql` の sole writer とする。host-owned inventory の immutable revision 行と singleton current head を P7 Search Source ledger とは別の表・別 role に置く。各 revision は deployment epoch、単調増加する authority revision、独立 tenant roster、Document/Remote の tenant 別完全集合（空集合を明示）、各 namespace の registration revision・canonical digest、roster digest、全体 digest、writer provenance、schema version を含む。Document は実接続済み adapter capability witness、Remote は server-owned registration validation を通す。全 entry の tenant が roster に属すること、各 tenant の宣言数と実 row 集合、全 DTO field、global SourceId 一意性を検査する。credential/SecretRef 値は inventory に保存しない。roster の完全性は登録集合自身や digest 自己一致から導かず、host authoring input 内の独立 tenant roster を規範集合として検査する。host 入力がその規範集合を提供できない場合は publish を拒否する。

publisher の**一つの PostgreSQL transaction**が immutable revision、全 tenant/二 namespace の row、per-tenant count/digest、current head の旧revision条件付き CAS、および §3 の `host.registration.changed` Audit row を書いて commit する。片 namespace・片 tenant の可視更新を許さず、同 revision 異内容、旧 revision、epoch 不整合、SourceId 衝突、Audit INSERT 失敗は全 rollback。commit 応答不明時は再接続した独立 connection で head/全 row/digest/Audit event_id を再読し、一致が証明されるまで listener と claim を閉じる。current head は host inventory の参照であり、P7 Source current pointer や第二 Source ledger ではない。R01 の config はこの host inventory reference と入力 revision を保持し、direct file reload や無監査の hot swap を actor-visible catalog に反映しない。

P7-R02 の `host_registration.rs` は publisher の read-only adapter で、immutable revision と current head を同じ read transaction から取得し、host authoring input の roster/revision と別 connection で再読した revision/全 row を突き合わせる。`CompleteDesiredRegistrations::capture(..., Document/Remote).await` の二結果が同じ epoch・authority revision・roster に属し、namespace digest と DTO exact equality が一致してから P7-02 唯一の `PgPool` backed `SourceRegistrationLedgerPort` へ渡す。`SourceRegistrationCatalog::try_new(...).await` の後、host head と P7 ledger/owner/kind/activation/current を再読し、P7-12 current scan を通して P5-08 四 route と P6-I04 claim を開く。reload は新 catalog 全体の検証後に一度だけ切り替える。途中失敗・restart mismatch・stale revision では旧 catalog の current gate も不確定として閉じる。R02 は publisher/Source ledger/route factory を複製しない。

## 3. Search/system typed Audit source と producer

R04A は既存 Document `audit_outbox_events` の event identity/subject 制約を緩めず、Domain `0010_audit_delivery_v0.sql` に **別の** append-only `search_audit_outbox_events` を追加する。両 source は同じ PostgreSQL 内で別表・別配送状態を持ち、R04A の Audit-only dispatcher が origin-tagged event を versioned decoder と class 別 allowlist で同じ relational Audit sink へ投影する。P6 Domain `outbox_events.delivered_at`、Search receipt、各 Audit `delivered_at` は独立である。R04A の既存 lease/fence・bounded retry/DLQ・sink `event_id` unique・unknown commit 再読契約を両 Audit source に適用する。sink に同 event_id の異内容があれば一致扱いせず停止・調査する。

Search source row は `event_id UUID`、`schema_version`、閉じた `event_class`/`event_type`/`origin_component`/`actor_kind`/`subject_kind`/`result`/`reason_code`、UTC `occurred_at`、必要な場合だけ bounded stable `actor_ref`/`subject_ref`、Audit専用配送状態を型と SQL CHECK で拘束する。任意 `data JSONB`、raw token/query/content/provider locator、SecretRef、具体 gap は持たない。`actor_kind=VerifiedPrincipal` は既存 host verifier の非秘密 reference だけ、`SystemComponent` は固定 enum と actor_ref NULL、未認証拒否は `Unknown` と actor_ref NULL。`subject_kind=SearchSource` は現在の権限で対象を特定できる管理操作だけが restricted Audit に ref を持ち、未知・不可視対象の拒否は `UnknownTarget`/ref NULL とする。`RuntimeConfig`、`AuditPolicy`、`MaintenanceOperation`、`SystemComponent` は各自の typed subject であり、Document UUID や nil UUID を流用しない。event type と class/subject/result/reason の許可組合せは schema version ごとに閉じる。

R04A は `append_search_audit_on(&mut PgConnection, TypedSearchAuditEvent)` の transaction-bound port と INSERT-only role を提供するが、business mutation を代理実行しない。各 producer は同じ connection/transaction で business row と Audit row を commit し、Audit INSERT 失敗なら変更を rollback する。拒否は許可操作を起こさず、P5 edge の独立拒否記録 transaction を試み、記録失敗でも拒否を維持して readiness/運用障害に上げる。外部 file 等を一つの DB commit と偽らず、destructive action は先に同一transactionの durable admission row＋Auditを commit し、それから side effect を実行し、結果も別の同一transactionの state＋Audit に記録する。結果記録失敗は成功扱いにしない。

| 必須 class / event | 同一transaction producer owner | subject と失敗時の判定 |
| --- | --- | --- |
| Document create/version/publish/withdraw、policy/role、file access | 既存 `document-application` command と `document-repository-postgres` の該当 repository transaction。`AuditEventRecord`、`targeted_events.rs`、`file_access.rs` 等を実DBで照合し、欠落は該当 Document owner に返す | 既存 Document typed subject。Audit INSERT 失敗なら mutation/原本開示をしない |
| host registration/privileged Search config change | P7-R01P `host_inventory_publish.rs` の head CAS transaction | `CONFIGURATION`/`HostInventory`。inventory/head と Search Audit row の同時 commit。不明 commit は再読まで admission 閉鎖 |
| Audit configuration change | P7-R04A `audit/admin.rs` の versioned Audit policy transaction | `CONFIGURATION`/`AuditPolicy`。file/env 直変更を有効化せず、Audit row と policy revision を同時 commit |
| Search rebuild・privileged management・destructive maintenance | P7-R05 `rebuild.rs`/`search_admin.rs` が所有する management/admission/result transaction。P7-11 GC は既存 guard/current/pin 境界を維持 | `PRIVILEGED_OPERATION` または `SYSTEM_AUDIT`/`MaintenanceOperation`。Audit失敗なら DB mutation rollback、外部作用前 admission 失敗なら実行しない |
| integrity violation | P7-R05 が所有する隔離状態 transaction（R03 は検出して同 port を呼ぶ） | `SYSTEM_AUDIT`/`SystemComponent`。記録失敗でも安全隔離を解除せず運用障害を明示 |
| 重要な authentication/authorization failure | P5-06 `search-api-http/src/auth.rs` と P5-08 `search-runtime/src/api.rs` の rejection hook が、R04A denial portを独立 transactionで呼ぶ | `SECURITY`/`UnknownTarget`、閉じた reason。対象存在・tokenを含めず、INSERT失敗でも拒否 |

既存 Document denial `targeted_events.rs::record_authorization_denied` の `AccessPolicy`/nil UUID は Document 専用の既存表現であり、Search/system event の型にはしない。高量 `document.read`、`search.execute`、`search.result.open` の採否は revision 1 §3どおり規範 writer と security/product owner が先に決め、採用 class は全件記録し sampling しない。R04A は source/sink の role・retention・reader grant を分離し、Search publisher/P5/R05 の business role は Audit INSERT のみ、Audit dispatcher は配送状態列のみ、P6 generic role は Audit ack 不可とする。

## 4. 維持する閉鎖と未了 gate

revision 1 §4 の `SinkKind × RetentionMode × VisibilityClass` の closed allowlist、全 sink/二 lease/実 socket/exporter/保持 handle の `NoRetention` sentinel、§5 の隔離 OTLP HTTP/protobuf 対 gRPC PoC・exact pin・規範選定前の production dependency 禁止、§6 の immutable workload manifest・bounded pilot・budget admission/abort・`NOT_ADMITTED` と測定範囲内だけの SLO 提案は**変更しない**。review revision 1 でこの3件は設計上 CLOSED と判定されたので、実装 PASS とは呼ばない。

P7-01〜12 の同一PG Source/current/READY/pin/guard とP6 Search receipt/generic ack、P1 Document/Unit/lexical、P3条件付きGraph、P4 Remote RAM、P5四route/二leaseはそのまま。P3-P04 未採択なら Graph/P7 READY・publish/R02/R09 production acceptance は閉じる。P2 Vector `Disabled` でも neutral core・lexical・Graph/current/final gate は省かない。最終 P1〜P6 receipt は R09 のみに fan-in する。

次は[改訂計画2](p7-runtime-plan-revision-2.md)と本書を独立 architecture/security reviewer が再判定すること。host publisher/typed Audit source と各 producer の実PG別接続・restart・rollback、P5 denial hook、実role、P3資格、P5 socket、P6 unknown COMMIT、P7-12別DB restore、OTLP採択、capacity/SLO は未実施の実装・資格 gate である。設計だけで production readiness は宣言しない。
