# Audit Infrastructure v1：Organization・Search・Documentへの引継ぎ

Status: 2026-10-08。基点はmain `dba8168`（単位A PR #98＋単位B PR #113）。単位C（`crates/audit-acceptance`）と本書は同じ統合単位で、本書の作成時点ではmain未統合。設計§13が参照する引継ぎ文書である。

- 本書はOrganizationのevent type・schemaを定義しない（設計§13）。Organizationが決める問いと、Auditが既に持つ接続点を、repositoryの事実（path:line）とともに並べる。
- 凡例：**main実装済み**（main `dba8168` のcodeと試験）／**単位C試験済み**（実Document producer経由の受入試験。PostgreSQL 18.6・合成データ）／**設計のみ**／**未実装**。いずれも本番導入（deploy・本番migration・credential）は未実施である。
- 正本：[設計](../specs/2026-10-07-audit-infrastructure-v1-delivery-design.md)（改訂4）、[決定記録](../../decisions/2026-10-07-audit-envelope-store-integrity.md)、[運用手順](../../operations/audit-delivery-store.md)、[受入試験](../../../crates/audit-acceptance/README.md)、[telemetry仕様](../../../spec/telemetry/README.md)、`spec/telemetry/audit-event-catalog.json`。本書と食い違えば正本が優先する。
- 略記：`AC/` = `crates/audit-core/`、`ST/` = `crates/audit-store-postgres/`、`RL/` = `crates/audit-relay/`、`WD/` = `crates/work-domain/src/`、`WR/` = `crates/work-repository-postgres/`、`DR/` = `crates/document-repository-postgres/`、`SR/` = `crates/search-runtime/`。`catalog:N` は上記catalog、`README:N` は `spec/telemetry/README.md`、`決定:N` は決定記録、`TC:N` は `spec/data/transaction-consistency-requirements-v0.md`、`org-design:N` は `docs/superpowers/specs/2026-10-02-organization-client-v0-domain-api-design.md`、`手順§N` は運用手順の節である。

## 1. 現在の提供物と範囲

### 1.1 Auditが提供するもの

| 提供物 | 根拠 | 状態 |
|---|---|---|
| catalog：Document 23種（origin `relay`。PR #106のVIEW/RESET 2種を含む）＋control 14種（`store` 11、`relay_control` 3）、adapter 3件 | catalog:3-31（adapter）、catalog:33-743（Document） | main実装済み |
| envelope（CloudEvents 1.0.2 structured JSON、閉じた属性集合）とpayload v1の検証、schema生成、(source_format, adapter_version) ごとのgolden pin | `AC/src/envelope.rs`、`AC/tests/schema_contract.rs`、`AC/tests/golden_projection.rs`、README §adapter_versionの規律とgolden pin | main実装済み |
| Document staging行の投影（理由文を複製しない） | `AC/src/legacy.rs:115` | main実装済み |
| relay：登録trigger、staging guard、`BEGIN ATOMIC` digest、claim/lease/ack、circuit breaker、外部障害の試行返却、quarantine・replay・reconcile・repair、health、bin `audit-relay` | `RL/migrations/0001_audit_relay_v1.sql`、`RL/src`、`RL/README.md` | main実装済み |
| Store：別DB、idempotentなingest、hash chain、`registered_types`・`source_services`、2段階開示とaudit-of-audit、verify・checkpoint、retention・purge、recovery epoch、bin `audit-admin`（DB外の判定 `assess` を含む） | `ST/migrations/0001_audit_store_v1.sql`、`ST/src`、`ST/README.md` | main実装済み |
| 運用手順（導入、health、配備順、backup・restore、retention） | [運用手順](../../operations/audit-delivery-store.md) | main実装済み（文書） |
| 実Document producer→relay→StoreのE2E、staging失敗、Store停止、relay強制終了、Store restore、Document migration互換 | `crates/audit-acceptance/tests/acceptance/`（T1–T5、`document_migration`） | 単位C試験済み |

### 1.2 まだ無いもの

- 本番導入：Document PoCのruntime・container・Linux手順はrelayを起動しない（`audit-relay` を参照するのはaudit crate・workspace定義・Audit設計・決定記録・運用手順・telemetry仕様・`dependency-rules.toml` だけ）。本番role分離も未実施（§5.3）。
- Search・Organizationのsource接続（§3、§4）。relayはDocument source固定（`RL/src/breaker.rs:353`、`RL/src/reconcile.rs:474` の `DOCUMENT_SOURCE`）。
- legal holdの作成・解除（`ST/migrations/0001_audit_store_v1.sql:300-308` の予約表だけ。手順§11）。
- 単位Cが通していない経路：DSI・Diff worker binary（process内の合成実装で代替）、`DueScheduler` 本体（`poll_once` と同じ順の呼出しで代替）、HTTP層、`audit-relay`・`audit-admin` のprocess起動（library入口で代替）、別hostのStore停止、実producer行でのDocument DB restore・組restore・in-place restore・replay・quarantine・retention（単位Bが合成行で試験）。詳細は[受入試験](../../../crates/audit-acceptance/README.md)の「含まないもの」。

### 1.3 Auditがしないこと（境界）

- 通常log・trace・metric・Domain Business Event・経営分析の業務正本・Personal Memoryの代わりにならない。Storeは調査証跡である（設計§1の4）。Organization側も同じ線を引く（org-design:30）。
- 本文、検索query全文、credential/token、physical storage locator、ACL全文、顧客データ、Chat transcript、Personal Memory、内部思考、未確定Draft本文を保存しない。payloadはtypeごとのallowlistで、自由記述のkindは無い（設計§1の5、README:71）。
- 既存typeをrenameせず意味を変えず、additiveに進化させる。legacy rowに無い情報を復元したように書かない（設計§1の6）。
- 本trackはDocument producer、`outbox_events`、`crates/outbox-delivery`、Search crate/migration、Work schema、GUI/Tauriを変更しない（設計§3）。

## 2. 現在の帰属とversioned extension hook

### 2.1 帰属の意味（main実装済み、単位C試験済み）

| 項目 | 意味 | 根拠 |
|---|---|---|
| `actor {issuer, principal_id}` | staging列の検証済み主体（Documentでは `VerifiedActorContext.principal`）。`invocation_kind` は行に無いので出さない | `AC/src/legacy.rs:231-234`、`DR/src/file_access.rs:73-74`、設計§4.2 |
| `service_executor` | 予約公開（published）とterminalでだけ、legacy `data.serviceExecutor` を持ち上げる。actorは予約した依頼者のまま | `AC/src/legacy.rs:181-196`、catalog:164,268、`DR/src/publish.rs:22-42`、`DR/src/schedule.rs:581-591` |
| schedulerの値 | `{issuer: "service", principal_id: "scheduler"}`。認証主体・policy subjectではない。T1が依頼者actorと並ぶことを確認 | `crates/document-publication-scheduler/src/identity.rs:1-14`、`crates/document-application/src/access_context.rs:65-69`、TC:484、`crates/audit-acceptance/tests/acceptance/journey.rs:335-387` |
| control eventのactor | 束縛された主体。束縛の無いsession（unboundの拒否、bootstrap）は `{issuer: "db_role", principal_id: session_user}` で `details.session_role` と一致 | README:223、`AC/src/envelope.rs:310-312` |
| correlation | `operation_id` / `publish_operation_id` はcatalogの写像元field、`source_correlation_id` はstaging `trace_id` 列のUUID。W3C `trace_id` は予約で、全adapterが `trace_id: false` | README:112、catalog:9 |
| 信頼境界 | stagingへINSERTできるroleは任意のactorを書ける。relayが検出するのは行内の不整合だけ | 設計§4.6 |

payload v1の上位memberは閉じた集合で、「誰の代理か」を表すmemberは無い（`AC/src/envelope.rs:59-71`）。

### 2.2 versioned extension hook（新しいproducer familyを足す手順）

hookは「catalog entry ＋ adapter/source_format ＋ golden ＋ Store `registered_types` ＋ relayのsource登録」の組である（設計§13の3接続点：source adapter、catalogに登録した `extensions` 名前空間、source所有のmigrationによる配送登録）。

| 手順 | repositoryにあるもの | 状態 |
|---|---|---|
| 1. adapter | catalog `adapters` に1件（`urn:` source、一意な `source_format` `[a-z0-9-]{1,64}`、`adapter_version ≥ 1`、commitment/registration、`trace_id`）。読込時に一意性・未使用を検査 | main実装済み（`AC/src/catalog.rs:133-149,564-618`、README:16-37） |
| 2. type | event entry（dotted type `[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+`、class、resources、results、subject形、fields/kind、reason扱い、写像field） | main実装済み（`AC/src/kinds.rs:264-275`、README:41-67） |
| 3. resource種別・kind | `ResourceType` は4値（Document/Folder/AccessPolicy/AuditStore）、`Kind` は閉じたenum、`resource.id` はUUID、enum値は `[A-Za-z0-9_-]{1,64}`（`.` 不可） | 新しい種別・kindには**audit-coreのcode変更**が要る（`AC/src/catalog.rs:92-97,633-639`、`AC/src/envelope.rs:390-400`） |
| 4. payloadの版 | `schema_version` 1、`dataschema` `...:payload:v1` | main実装済み（`AC/src/envelope.rs:30-31,285-287`） |
| 5. `extensions` 名前空間 | v1は空objectだけを許し、catalogに名前空間を登録するmemberは無い（catalogは未知memberを拒否） | **未実装**（`AC/src/envelope.rs:349-351`、`AC/src/legacy.rs:252`、`AC/src/catalog.rs:374-379`） |
| 6. 投影code | Document用の `project` だけ（source_format・adapter_versionは定数） | 他sourceは**未実装**（`AC/src/envelope.rs:40`、`AC/src/legacy.rs:27,115`） |
| 7. golden | type追加・enum値追加ではadapter_versionを上げず、現在のsectionへentryを追記。出力変更時だけ版を上げ旧sectionを凍結 | 規則はmain実装済み（README:159-184）。golden fileは単一source_formatで、**2つ目のadapterの形式は未実装**（`AC/tests/golden_projection.rs:56`） |
| 8. Store `registered_types` | (source, type, adapter_version, source_format)。migrationでだけ変更でき、ingestで照合する。commitmentの無いadapterのduplicateはenvelopeの完全一致だけ | main実装済み（`ST/migrations/0001_audit_store_v1.sql:310-326`、同:1549）。後続migrationでの追加は未検証（手順§12） |
| 9. Store `source_services` | ingestできる主体を (issuer, principal_id, source) で登録。v1は `service/audit-relay` → Document sourceだけ。追加は `register_source_service`（ownerのmember、記録付き） | main実装済み（`ST/migrations/0001_audit_store_v1.sql:228-249`） |
| 10. relayのsource登録 | Documentは `AFTER INSERT` triggerで同一transactionに配送登録する。他sourceの登録・配送経路は無い | Documentはmain実装済み（`RL/migrations/0001_audit_relay_v1.sql:1599-1604`）、他sourceは**未実装** |
| 11. review | CODEOWNERSは無い。catalog変更は配備前にAuditのreviewを受ける（手順§7）。Organizationは「Phase1/2凍結後に閉じた版付きOrganization metadata catalogを足し、Audit trackでsource→sink配送を独立に資格確認する」（org-design:667-669） | 機械的なreview強制は**未実装** |

## 3. Organization向け（DECISIONS / QUESTIONS）

AuditはRole/Delegation/WorkItem/Workflowの意味もschemaも定義しない。現在の帰属（§2.1）を保持し、接続は§2.2のhookで行う。以下はOrganizationが決める問いと判断材料である。

### 3.1 main上のOrganization/Workモデル（事実）

- 方針aggregate `OrganizationPolicy { units, roles, role_assignments, delegations }` はrevision付きで `work.organization_policies` に1行ある（`WD/organization.rs:178-185`、`WR/migrations/0007_organization.sql:4-8`）。
- 正式割当 `RoleAssignment` と委任 `Delegation`（必須の `valid_until`、理由、取消）（`WD/organization.rs:104-141`）。`organization.manage` と `work.assign` は委任できない（`WD/organization.rs:74-79`）。
- 責任は評価時刻ごとに導出し、委任は委任元の割当が有効な間だけ有効（`WD/organization.rs:463-491,528-561`）。`describe()` は効力を問わずラベルを返し、stagingの説明に使われる（`WD/organization.rs:562-571`、`WD/lib.rs:894-897`）。
- 操作文脈 `CommandContext { operation_id, expected_revision, acting_assignment_id }`（`WD/lib.rs:466-470`）。主体は合成6名の閉じた集合（`WD/lib.rs:53-60`）。Organization serverがDocumentを呼ぶissuerは `organization-synthetic`（`crates/organization-server/src/identity.rs:64`）。
- AgentExecution：`requested_by`、`requester_responsibility`、`executed_by`（合成executor `organization-synthetic/agent-01`）、`executor_invocation_kind`、`provider_principal_bindings`、`purpose`（`WD/agent.rs:4,58-75`）。Organization設計は「requester、acting responsibility、実executor、実provider identity」の保持を求める（org-design:397-398）。
- 割当・委任の理由文は1024 bytes以内でpolicy本体にあり、stagingへは入れない（`WD/organization.rs:276-287`、`WR/src/lib.rs:541`）。

### 3.2 `work.event_staging` の現状（事実）

- 列：`id`、`operation_id`（UNIQUE、0004以降NULL可）、`workflow_id` か `policy_id` の一方、`principal_id`、`acting_assignment_id`、`task_id`、`action`（閉じた語彙、0009で26種）、`occurred_at`、`payload`（`WR/migrations/0001_work.sql:22-32`、`0004_agent.sql:3`、`0007_organization.sql:17-37`、`0009_work_files.sql:4-16`）。issuer列、配送状態列、append-only guardは無い。
- 書込み：Work操作 `WR/src/lib.rs:428-429`、policy操作 `WR/src/lib.rs:532-534`（ledgerと同一transaction）、Agent遷移 `WR/src/agent.rs:445-446`（`operation_id` はNULL、principalは要求者）。
- payload：Work操作は責任種別・role・委任ID・委任者（`WR/src/lib.rs:649-663`）、policy操作は対象・期間・`policyRevision`（`WR/src/lib.rs:542-553`）、作業ファイルはID・世代・大きさ・SHA-256（`WR/src/files.rs:165-169`）。題名・本文・理由・依頼目的は入れない（organization-multi-principal-status・work-context-status・work-files-status・agent-chat-statusの「Audit担当へのhandoff」節）。
- consumerはproduction codeに無い（readiness検査 `WR/src/lib.rs:789` だけ）。migration ledger `work.schema_migrations` は件数とchecksumの完全一致を起動時に検査するので、Auditがこのledgerへ行を足すと起動が失敗する（`WR/src/lib.rs:738-748,782-788`）。
- 確認済み（attention-seen）はWork mutationではなく、stagingに入らない（`WR/migrations/0008_attention.sql:1-3`）。

### 3.3 Organizationが決める事項

| # | 問い | 判断材料（事実） |
|---|---|---|
| O1 | 委任下の `actor` は受任者か委任者か | stagingは受任者を `principal_id`、委任IDを `acting_assignment_id`、委任者をpayloadに持つ（`WR/src/lib.rs:654-662`）。Auditの `actor` は「検証済みの実行主体」の意味（§2.1） |
| O2 | 「代理で行った」をどう表すか（details、`extensions` 名前空間、payloadの新版） | payload v1に代理用memberは無い（`AC/src/envelope.rs:59-71`）。名前空間は未実装（§2.2 手順5） |
| O3 | issuerの出所 | staging列に無い。Document呼出しでは `organization-synthetic`。catalogの `principal` kindは `{identityProvider, principalId}` を要する（README:83）が、payloadの主体は裸の文字列 |
| O4 | 認可時点のpolicy revision・評価時刻を残すか | Work操作のpayloadに `policyRevision` は無く、policy操作には有る（`WR/src/lib.rs:552`） |
| O5 | Agentの帰属（要求者／実executor／provider principal） | Documentの前例は「actor=依頼者、service_executor=実行者」。持ち上げるのは `principal` kindだけ（`AC/src/legacy.rs:181-196`）。`executedBy` は文字列 |
| O6 | 理由の扱い | 理由文はstagingに無い。Auditの選択肢は `absent` か要約 `{provided, utf8_bytes}` だけ（README §自由記述reasonを複製しない規則） |
| O7 | どのactionを監査sourceにし、配送登録をどこが持つか | `work.event_staging` は業務と監査の兼用で、guardも配送状態も無く、ledgerは厳格（§3.2）。設計§13の案はOrganization所有のmigrationによる配送登録 |
| O8 | idempotency key | 行の `id`（UUIDv7、`WR/src/lib.rs:429`）。`operation_id` はAgent遷移でNULL。commitmentを持たないadapterではduplicateはenvelopeの完全一致だけ（§2.2 手順8） |
| O9 | 語彙のcatalog適合 | `PolicyAction` 名は `.` を含み（`WD/organization.rs:36-73`）enum値に使えない。`work_item`・`role_assignment`・`delegation`・`agent_execution` は `ResourceType` に無い（§2.2 手順3） |
| O10 | 作業ファイルのSHA-256をStoreへ運ぶか | staging payloadに有る（`WR/src/files.rs:168`）。Documentの理由文は推測確認を防ぐためsalt付きcommitmentにした（設計§5.2） |
| O11 | attention-seenを観測eventとして監査するか | Work mutationではない（§3.2） |
| O12 | Organization経由のDocument操作とWork operationの相関 | Documentの `audit_outbox_events` にWork operationの列は無い（`DR/migrations/0001_document_authoritative_core.sql:86-101`、`0006_document_management_access_v0.sql:55-57`） |

Organization専用のtype・field・resource種別は、Organizationの判断と独立reviewの後に、Audit reviewを伴う追加変更として§2.2の手順で入れる。

## 4. Search向け

### 4.1 既存のSearch audit outbox（事実。Auditは変更を提案しない）

- 表 `search_audit_outbox_events`（`SR/migrations/0007_host_registration_inventory_v1.sql:61-95`）：`event_id`、`schema_version`、`event_class`、`event_type`（`host.registration.changed` のみ）、`origin_component`、`actor_kind`/`actor_ref`、`subject_kind`/`subject_ref`、`result`、`reason_code`、`occurred_at`、`attempt_count`、`delivered_at`。
- guard：DELETE拒否、内容列不変、`delivered_at` は一度だけ。row triggerだけでTRUNCATE用は無い（同:122-147）。未配送index（同:149-151）。
- 権限：PUBLICから剥奪し、INSERTだけを `search_host_publisher` に与える。SELECT・UPDATEを持つ配送roleは無い（`SR/sql/roles.sql:115-121`）。
- 書込みは `append_search_audit_on`（`SR/src/audit.rs:51`）で、呼出しはhost inventoryのpublishだけ（`SR/src/host_inventory.rs:554-561`）。`delivered_at` を設定するcodeは無く、未配送である。
- Search P7計画（Freeze前のPLAN PROPOSAL）のR04A-Dは、Document・Search両sourceを別PG sinkへ配送し、Document `audit_outbox_events` に配送状態列を足す（`docs/superpowers/programs/search-platform-completion/p7-runtime-plan-revision-2.md:3,31,53`）。これは決定D4（配送状態は `audit_relay.deliveries` が持ち、Documentの列を変えない。決定:51-63、TC:807）と重なる（§6 S1）。

### 4.2 接続にSearchが用意するもの（Search所有の判断）

- 配送登録の方式（Search所有のmigrationによる登録、またはSearchが付与する未配送index＋grant）と、source URN・`source_format`・commitmentの要否（Searchにはsaltが無い。§2.2 手順8）。
- `actor_kind`/`actor_ref` から `actor {issuer, principal_id}` への写像（`SystemComponent` で `actor_ref` がNULLの扱い。actorは必須：`AC/src/envelope.rs:294-296`）。
- `subject_kind`/`subject_ref`：`resource.id` はUUIDに限り `ResourceType` は閉じている（§2.2 手順3）。**audit-coreの変更**が要る（Audit・Searchの合同判断）。
- class・resultの写像。NO_RETENTION由来の値のdigest・長さ・hidden Source IDを計算せず保持しない（設計§13）。

## 5. Document向け

### 5.1 決定D4の規則（main `dba8168` のcode。`audit-relay migrate` を実行したDBで効く）

- `audit_relay` ledgerが `public.audit_outbox_events` に、登録trigger、append-only guard（UPDATE・DELETE・TRUNCATEを55000で拒否）、deliveriesからのFK、`BEGIN ATOMIC` digest関数を追加する（決定:51-63、`RL/migrations/0001_audit_relay_v1.sql:130-131,269-279,1599-1604`）。
- migrate順はDocument ledger → `audit-relay migrate`（preflightで列・型とDocument ledgerを確認）。Documentの `_sqlx_migrations` へAudit行は書かない。
- この表に触れるDocument migrationにはAuditのreviewが要る。`DROP COLUMN ... CASCADE` は禁止。digest対象列（`event_id, event_type, source, subject, actor_identity_provider, actor_principal_id, resource_type, resource_id, resource_version_id, result, trace_id, data, occurred_at`）のDROP・型変更はmigrate時に失敗する。nullable列のADDはproducer・登録・digestを変えない（単位C `document_migration` で確認）。
- owner DELETEを前提とする既存試験（`DR/tests/document_history_projection.rs:88`）は、Document単体のDBでだけ有効（決定:62）。

### 5.2 新しいaudit typeとenum値の追加（手順§7、README:159-184）

- **新しいDocument audit type**は、catalog entry・audit-coreの試験fixture・golden entry・Store `registered_types`（後続migration）を揃え、**producerのdeployより前に**Store → relayの順で配備する。producerが先に出ると、relayはそのtypeの行を `relay_catalog_skew` として保留する：行はstagingと配送登録に残り、試行を消費せず、quarantineされず、何も失われないが、そのtypeの配送は遅れ、healthは `catalog_skew_held`・`relay_held` を警報し続ける。relayがStoreより先だと、probeが `store_unregistered_type` で全配送を止める。
- PR #106の `document.version.detail_viewed`・`document.version.marked_unread` はこの規則でcatalogとStoreへ登録済み（catalog:553-614）。
- **既存typeのenum値の追加**（例：`authorization.denied` の新しい `action_code`）は保留されず `invalid_field` でquarantineされるので、relay（新catalog）→ producerの順に配備し、先に書かれた行はrelay更新後に `audit-relay replay` で戻す。main `6a34de3` の `get_current_read_state`・`mutate_read_state` はcatalogに登録済み（catalog:722-736）。CIは、Documentのsourceにある `record_authorization_denied` の全呼出しの `action_code` がcatalogに無ければ失敗する（`AC/tests/catalog_contract.rs:1297-1326`）。新しいtype名そのものはCIでは検出せず、relayの保留とhealthで検出する。

### 5.3 stagingのowner（単位Cの観察）

- 非superuserの `public.audit_outbox_events` のOWNERは、自分の表に付いたrelayのtriggerを無効化・削除できる。relayのpostureは表のownerを違反として報告しない（単位Cは非superuserのowner `document_app` でpostureが空であることを前提に動く：`crates/audit-acceptance/tests/acceptance/support.rs:1019-1028`）。
- 事後の検出：未登録行はhealthの `unregistered_rows` 警報と `reconcile --repair` の登録で拾われる（repair登録のdigestはrepair時点の値。設計§5.1）。登録済み行はFKで削除できず、改変は `source_digest` で検出する（設計§5.2–5.3）。
- 現在のDocument PoCは単一superuserでmigrateとserveを行い、role分離を満たさない（設計§5.3、手順§12）。**Documentと依頼者への引継ぎ：本番のruntime loginはstaging表のownerであってはならない**（ownerとruntimeを分け、ownerのloginは配備時だけ使う）。

### 5.4 既知の欠け（Documentへの引継ぎ、設計§13）

- `authorization.denied`：producerは業務transactionの外（pool）でINSERTする（`DR/src/targeted_events.rs:64-85`）。呼出しは `DR/src/access_policy.rs:544,584`、`DR/src/read_state.rs:183`、`DR/src/current_read_state.rs:334,361`。対象範囲はDocument担当が決める。
- `trace_id` 列は `TEXT NULL`（`DR/migrations/0001_document_authoritative_core.sql:96`）でW3C traceparentは保存されない（TC INV-10・AC §13は未充足）。
- 理由文：caller_textの7種はStoreへ `{provided, utf8_bytes, text_retained}` だけを送る（`AC/src/legacy.rs:144-155`、決定:65-71）。withdraw/endの理由文に上限が無い。通常のACL変更はreasonを記録しない。
- そのほか：client指定IDのnil UUID（quarantine。README §nil UUIDのclient指定ID）、principalの文字種と長さ上限（README §principalの文字規則）、取下げ・公開終了時のschedule terminal audit、理由文を開示する機能の要否。

## 6. 未決事項と所有者

| # | 未決事項 | 所有者 | 前提・期限 |
|---|---|---|---|
| A1 | 単位C（`audit-acceptance`・本書・capability matrix最終版）のmain統合とmain CI | Audit | [状況](../execution/audit-infrastructure-v1-status.md)の先頭 |
| A2 | `extensions` 名前空間の登録方式（catalog形式、schema生成、Storeでの照合） | Audit（Organizationの要求を受けて） | O2の結論の後 |
| A3 | 2つ目のadapterのgolden形式と投影code、非Document sourceの配送経路（relayへ足すか、sourceのadapterがStore portを直接使うか） | Audit＋Organization／Search | 最初の非Document adapterの前 |
| A4 | 新しい `ResourceType`・kind（UUIDでないid、裸のprincipal文字列、`.` を含むenum値）を許すか | Audit＋Organization／Search | O9・§4.2 |
| A5 | relay postureでstaging表のownerを報告するか（§5.3） | Audit | D5と合わせて |
| O1–O12 | §3.3の各問い | Organization | Organization Domain/API/Auth設計の確定後 |
| S1 | Search R04A-D（両sourceの配送、Document列の追加）と決定D4の調整 | Search＋Audit（依頼者の裁定が要る場合あり） | Search P7計画のFreeze前 |
| S2 | §4.2の写像と配送登録の方式 | Search | S1の後 |
| D1 | `authorization.denied` の対象範囲と試験 | Document | — |
| D2 | W3C traceparent列の追加 | Document | — |
| D3 | withdraw/end理由文の上限、通常ACLのreason、理由文開示機能の要否 | Document（開示はAuditと合同） | — |
| D4 | 新しいaudit type・enum値の配備順の遵守（§5.2） | Document＋Audit | 各Document PRの統合前 |
| D5 | 本番のrole分離（serveを非superuserで行い、runtime loginをstaging表のownerにしない。`audit_relay_owner` を分ける） | Document＋依頼者（Auditがreview） | 本番化の前 |
| R1 | purgeとlegal holdの優先関係（holdの作成・解除はv1に無い） | 依頼者 | legal hold実装の前 |
| R2 | `register_source_service` の記録へのsource fieldの追加（kindの判断） | 依頼者 | 2つ目のsourceの前 |
| R3 | ackに記録するStore epoch（probe時か、commit時のreceiptか） | 依頼者 | — |
| R4 | reconcileの `repaired_*` を計画件数として記録すること（実適用件数はCLI出力だけ） | 依頼者 | — |
| R5 | Audit Storeの本番再選定（PostgreSQLは暫定採用、決定D2）と本番導入 | 依頼者 | 本番化の前 |
