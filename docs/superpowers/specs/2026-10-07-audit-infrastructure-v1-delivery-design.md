# Audit Infrastructure v1：監査Outboxから監査Storeまでの配送・保存・検証 設計

Status: DESIGN CANDIDATE（独立review前）。2026-10-07、基点main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（push CI 37562024089 SUCCESS）。

本書は未統合Draft [PR44](https://github.com/AIrisu-072/knowledge-platform/pull/44)（設計）と [PR45](https://github.com/AIrisu-072/knowledge-platform/pull/45)（schema/legacy contract）を置き換える。旧設計の脅威分析・失敗モデルは入力として尊重するが、旧基点 `d71753d` は30 merge古く、旧方針「reasonを持つevent・通常ACL変更を配送しない」は今回の要求（取下げ・metadata/ACL変更をStoreまで届ける）と衝突する。旧PRのstackには依存しない。

規範入力：`spec/operations/observability-audit-requirements-v0.md`（以下OA）§§2.4, 7.2, 12.4, 13–17, 19–22, 26–33、`spec/data/transaction-consistency-requirements-v0.md`（TC）INV-09/10, T1–T10, §7（:797）、`spec/architecture/architecture-contract-v0.md`（AC）§§4.4, 7, 13、`spec/data/logical-data-model-v0.md`、`spec/selection/library-tool-selection-v0.md`（LTS）。

## 1. 不変条件

1. 必須AuditのstagingはDocumentの業務mutationと同一transactionで確定する。stagingまたは同時に行う配送登録が失敗した場合、業務はcommitしない（OA §22.2, TC INV-09）。
2. Storeが停止していても業務は継続し、staged eventは配送待ちとして残る。staging失敗（業務rollback）とStore停止（配送遅延）を状態・health・試験で区別する。
3. Auditはsamplingしない。配送はat-least-once、ingestはevent IDでidempotentに行う。
4. Auditを通常log・trace・metric・Domain Business Event・経営分析の業務正本・Personal Memoryの代わりにしない。Storeは調査証跡であり、業務集計の正本ではない。
5. 文書本文、検索query全文、credential/token、physical storage locator、ACL全文、顧客データ、Chat transcript、Personal Memory、内部思考、未確定Draft本文をStoreへ無条件に保存しない。payloadはevent種別ごとのallowlistであり、自由記述fieldは持たない（OA:646, §19）。
6. 既存event typeとその意味を変えない。renameせずadditiveに進化させる。legacy rowに無い情報（理由文、actor種別、W3C trace等）を復元したかのように表現しない。
7. produced（staged）／delivered（ack済み）／stored（Store受理）／verified（integrity検査済み）を別の証拠として扱う。
8. 本番credential・実データ・本番migration・deployは扱わない。

## 2. Capability matrix（main `d515aa3` 時点）

凡例：実装・検証済み／実装不完全／仕様のみ／欠落／他担当待ち。「本設計」列は本trackで埋める範囲を示す。

| Capability | main現状 | 根拠 | 本設計 |
|---|---|---|---|
| 生成（Document 21種） | 実装・検証済み（一部試験欠落） | 13箇所のINSERT（`document-repository-postgres/src`）。`authorization.denied` の試験は0件。`version.created/updated/rebased`、schedule系、`withdrawn`、`document.moved`、`folder.renamed`、`revision_comparison` のaudit失敗試験は無い | Producerは変更しない。E2E受入で代表operationを実証する |
| atomic staging | 実装・検証済み（未試験の種別あり） | 業務tx内でINSERTし、失敗時はrollback。failure trigger試験は8系統 | 配送登録をstagingと同一txで行い（§5）、失敗時に業務rollbackすることを試験する |
| schema | 実装不完全 | SQL列は固定だが、`data` はshapeもsizeも検証していない。`spec/telemetry/` は存在しない | catalogからschemaを生成し、runtimeで検証する（§4） |
| CloudEvents | 仕様のみ | OA §13。SDKはPOC REQUIRED | 自前のstructured JSON envelope（ADR、§4.1） |
| actor/resource attribution | 実装・検証済み（Document内） | issuer/principal、resource_typeの3値、scheduler serviceExecutor（`service/scheduler`） | そのまま保持し、偽装を拒否する |
| correlation | 実装不完全 | `trace_id` 列はNULLまたは別audit IDで、W3C traceparentは保存されない。operation IDはpayloadに散在 | `correlation` へ明示的に写像し、推測しない |
| 配送（dispatcher） | 欠落 | 読み手が無い | Audit専用の配送状態と既存runnerで実装（§6） |
| retry/backoff | 欠落（列のみ） | `attempt_count` は常に0 | lease・指数backoffを実装する |
| idempotency | 欠落 | source側のPKのみ | Storeの永続identity registryで担保（§7） |
| terminal failure | 欠落 | — | quarantineと監査付きreplay |
| Store | 仕様のみ（DEFERRED） | LTS:268 | 既存PostgreSQL/SQLxで別schema・別ledger・別DBに対応したbackend（§7） |
| append-only | 欠落 | stagingにtrigger・grant・TRUNCATE防止が無い | staging guard、Store guard、権限templateを追加（§5, §7） |
| integrity | 欠落 | — | event digest、write-time hash chain、外部checkpoint（ADR、§8） |
| retention | 仕様のみ | 年数は固定しない（OA §26） | 方針データ化。既定は失効なし。期限到達は特権maintenance（§9） |
| access/export | 仕様のみ | OA §27 | Audit専用の権限とDB関数経由のread/export（§10） |
| 閲覧の監査（audit-of-audit） | 仕様のみ | OA:842 | 開示と同一txでcontrol eventを記録（§10） |
| backup/restore | 仕様のみ | Linux guideは単一DBの `pg_dump` のみ | 2DBのbackup契約、recovery epoch、reconcile（§11） |
| minimization | 実装不完全 | 7種が自由記述 `reason` を保持し、withdraw/endには上限が無い | 理由文は複製せず、提供有無とbyte数のみ記録（§4.4） |
| health/reconciliation/restart | 欠落 | — | produced/delivered/stored/verifiedを分けたhealthとreconcile（§12） |
| Search audit配送 | 他担当待ち | `search_audit_outbox_events` はSearch所有で、R04Aが未実装 | source adapterの契約をhandoffする |
| Organization attribution | 他担当待ち | `work.event_staging` にissuer・role・delegationが無い | versioned extensionの接続点とhandoff（§13） |

## 3. 構成とcrate

| 構成要素 | 置き場所 | 責務 |
|---|---|---|
| `crates/audit-core` | pure（sqlx/tokio/fs非依存） | catalog、envelope、検証、legacy投影、chain計算、port trait、export検証 |
| `crates/audit-store-postgres` | Store DB（`audit_store` schema、ledger `audit_store_sqlx_migrations`） | `AuditStore` portのPostgreSQL実装、ingest、調査・export・検証・retention・権限のSQL関数、bin `audit-admin` |
| `crates/audit-relay` | Document DB（`audit_relay` schema、ledger `audit_relay_sqlx_migrations`） | 配送登録trigger、staging guard、配送状態、`outbox-delivery` runnerへのadapter、reconcile、replay、health、bin `audit-relay` |

- Store DBは `AUDIT_STORE_DATABASE_URL` で別DB・別serverを指せる。PoCでは同じclusterの別databaseでもよい。試験はsourceとStoreを別databaseに分け、cross-database transactionが無いことを前提に検証する。
- `_sqlx_migrations`（Document）へはAudit行を書かない。document-server/organization-serverの厳格なcompatibility checkを壊さないためである。Search/Graphと同じ別ledger方式をとる。
- Document producer、`outbox_events`、`crates/outbox-delivery`、Search crate/migration、Work schema、GUI/Tauriは変更しない。

## 4. Event contract

### 4.1 CloudEvents envelope（ADR）

`cloudevents-sdk` はPOC REQUIRED（LTS:265）なので、productionには入れない。CloudEvents 1.0.2 structured JSON formatに従う閉じたenvelopeを `audit-core` で実装する。SDK型をDomainへ漏らさず、将来SDKを採用する場合はadapterで置き換える。

| 属性 | 値 |
|---|---|
| `specversion` | `"1.0"` |
| `id` | 既存の監査event UUID（変更しない） |
| `source` | 既存値 `urn:knowledge-platform:document-platform`、Storeのcontrol eventは `urn:knowledge-platform:audit-store` |
| `type` | 既存の短いdotted type（例 `document.version.published`）。OA §13の逆DNS例は例示であり、OA:646「既存Document eventの型と意味を維持」を優先する |
| `subject` | 既存のsubject |
| `time` | `occurred_at` をUTC RFC3339、マイクロ秒、末尾 `Z` で表す |
| `datacontenttype` | `"application/json"` |
| `dataschema` | `"urn:knowledge-platform:audit:payload:v1"` |
| `data` | payload v1（§4.2） |

未知の属性・extension attributeは拒否する。重複keyは拒否する（PR45のparserを流用）。envelopeはserialize後32 KiB以下とする。

### 4.2 payload v1

```json
{
  "schema_version": 1,
  "event_class": "CONTENT_LIFECYCLE",
  "action": "document.version.withdrawn",
  "actor": {"issuer": "poc", "principal_id": "poc-human"},
  "service_executor": {"issuer": "service", "principal_id": "scheduler"},
  "resource": {"type": "Document", "id": "<uuid>", "version_id": "<uuid>"},
  "result": "success",
  "reason_code": "<closed code>",
  "reason": {"provided": true, "utf8_bytes": 42, "text_retained": "source_staging_only"},
  "correlation": {"operation_id": "<uuid>", "source_correlation_id": "<verbatim>"},
  "details": {"<allowlisted legacy key>": "<typed value>"},
  "extensions": {},
  "provenance": {"source_format": "document-audit-outbox-v0", "adapter_version": 1, "source_digest": "sha256:<hex>"}
}
```

- `event_class` はOA §14の8 classのいずれか。種別ごとの割当はcatalogが決める。
- `actor` はstaging列のissuer/principalである。`invocation_kind` はlegacy rowに保存されていないため出さない。名前から推測しない。
- `service_executor` は、legacy `details.serviceExecutor`（published/terminal）がある場合だけ持ち上げる。requester（`actor`）と区別する（TC:484）。
- `resource.type` は `Document` / `Folder` / `AccessPolicy` / `AuditStore`。`resource.id` はUUID。ただし `AuditStore` は固定文字列 `audit-store` とする。
- `reason_code` はcatalogが閉じたcode fieldを指定する種別（`terminalReason`、denied `reason_code`）だけで持つ。
- `correlation.operation_id` は `operation_id` / `operationId` / `publishOperationId` からの明示写像である。`source_correlation_id` はstagingの `trace_id` 列の値をそのまま入れる（128 byte以下のASCII可視文字に限る）。W3C traceとは名乗らない。
- `details` は種別ごとのallowlistであり、legacy key名を変えない。型はcatalogのkindで検証する。
- `extensions` はv1では空objectのみを許す。Organization等の名前空間はcatalog登録後に許可する（§13）。
- `provenance.source_digest` はsource receipt digest（§5.2）である。

### 4.3 catalog・schema・検証

- 正本：`spec/telemetry/audit-event-catalog.json`（type、source、event_class、許可resource、version要否、result、subject pattern、fields{name:{kind, values?}}、required、reason扱い、reason_code_field、service_executor_field、operation_id_field）。
- kind：`uuid`、`nullable_uuid`、`counter`（0以上のi64）、`positive_counter`、`boolean`、`enum`、`nullable_enum`、`enum_list`、`digest`（0–255の整数32個）、`nullable_digest`、`principal`、`legacy_time`（整数9要素の旧time serde配列。RFC3339へ変換したとは表記しない）。自由文字列のkindは無い。
- `spec/telemetry/audit-event.schema.json` はcatalogからRustで生成する（`$defs`/`$ref` で共有部分を1回だけ定義）。差分があれば試験が失敗する（生成の再現性。OA §29）。jsonschemaはdev-dependencyとしてのみ使い、runtimeの正本はRust validatorとする。両者がconformance fixtureで一致することを試験する。
- 上限：envelope 32 KiB、文字列512 byte、ID 128 byte、principalの各部256 byte、details key 32個、深さ6。

### 4.4 自由記述reasonの扱い（PR44/45からの変更点）

対象は `document.version.withdrawn`、`document.publication.ended`、`document.metadata.changed`、`document.moved`、`folder.created`、`folder.renamed`、`folder.moved`。

- 理由文はStoreへ複製しない。staging projectionのSQL側で `data - 'reason'` を取り、`reason: {provided, utf8_bytes, text_retained: "source_staging_only"}` だけを作る。理由文はstaging（append-only化する。§5.3）と、Documentの業務ledger（`document_revisions.reason`、withdrawal ledger、publication end table）に残る。これはOA:646「許可された理由分類」と、「無制限の入力を複製しない」を満たすための意図的な最小化である。
- `reason` が非文字列なら `reason_not_string` としてquarantineする。
- 通常の `access_policy.changed` はsourceがreasonを記録しないため `reason` を出さない（`provided: false` とも書かない）。「無い」を「空」と誤表現しない。
- 取下げ・公開終了で重複している `data.actor` はstaging列のactorと一致することを検証し、一致しなければ `actor_mismatch` でquarantineする。detailsからは除く。
- 将来、上限付きの理由文保存を有効化する場合は、別policy・別承認とする（OA:668の扱いに準じる）。

## 5. Source側（Document DB）：配送登録とstaging保護

### 5.1 配送登録

- `audit_relay.deliveries`（event_id PK、FKで `audit_outbox_events` へ。registration_kind、source_digest、legacy_attempt_count、legacy_delivered_at、配送状態列、store receipt列、quarantine列、replay_count）と `audit_relay.delivery_policy`（singleton、revision付き）を置く。
- `AFTER INSERT ON public.audit_outbox_events FOR EACH ROW` のtrigger（`SECURITY DEFINER`、`search_path` 固定）が、同一transaction内でdeliveries行を作る。登録が失敗すると元のINSERTが失敗し、producerは既存どおりrollbackする。これにより業務だけがcommitされることは無い。producer roleには新しい権限を要しない。
- 既存行は、migration transaction内で `LOCK TABLE audit_outbox_events IN SHARE ROW EXCLUSIVE MODE` を取ってからbackfillし、そのままtriggerを作成する。lockはcommitまで同時INSERTを待たせるので、backfillとtrigger作成の間に漏れは生じない。全件の完全性をmigration transactionで確定でき、旧設計の多段bootstrap barrierは不要になる。その代わりmigration中は業務INSERTが待機する（運用手順に明記する）。
- 既存の `attempt_count>0` / `delivered_at` 非NULL行は、配送済みとは見なさない。値を `legacy_*` 列に記録したうえでpendingにする（受領証の無い配送主張を信用しない）。
- 運用中にtriggerが失われた場合は、reconcileが未登録行（anti-join）を検出する。`--repair` で `registration_kind='repair'` として登録し、件数をcontrol eventに記録する。

### 5.2 source receipt digest

`audit_relay.source_digest(o public.audit_outbox_events)` は `sha256(convert_to(jsonb_build_array('kp-audit-source-v1', event_id, event_type, source, subject, actor_identity_provider, actor_principal_id, resource_type, resource_id, resource_version_id, result, trace_id, data, to_char(occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'))::text, 'UTF8'))` で計算する。session TimeZoneには依存しない。jsonbのtext出力は決定的である。登録時に保存し、claim時にserver側で再計算・比較する（`source_intact`）。不一致なら `source_digest_mismatch` でquarantineする。理由文を含む全列が対象なので、理由文をclientへ取り出さずに改変を検出できる。

### 5.3 staging保護

`BEFORE UPDATE OR DELETE ... FOR EACH ROW` と `BEFORE TRUNCATE ... FOR EACH STATEMENT` で拒否する（SQLSTATE 55000、既存 `document_revisions_append_only` と同じ方針）。productionコードにUPDATE/DELETEは無い。v1はsource cleanupを提供しない（Store backupの復元可能範囲を確立していないため）。保護はAudit ledgerのmigrationで導入するので、Document単体の既存試験には影響しない。DB ownerはtriggerを無効化できる。この限界は外部checkpointとreconcileで補う（§8）。

### 5.4 projection（claim時、SQL側）

stagingの各text列は1024 byteを超えると全体をNULLにし、`source_row_too_large` とする。`data` はobjectであることを確認し、`data - 'reason'` が16 KiB以下である場合だけ返す。`reason_kind`、`reason_bytes`、`source_intact` を同時に返す。理由文や大きな値はclientへ出さない。

## 6. 配送（dispatcher）

### 6.1 既存 `outbox-delivery` の再利用と責務差

`DeliveryRunner`、`DeliveryPolicy`、`DeliveryConfig`、`retry_delay_within`、`ClaimAdmission` を、変更せずにlibraryとして使う。`PostgresOutboxStore`（`outbox_events` 固定）・`ErrorCode` の拡張・`DeliveryRoute` の追加は行わない。

| 観点 | 汎用P6（`outbox_events`） | Audit relay |
|---|---|---|
| source | Domain `outbox_events` | `audit_outbox_events`（将来、他sourceのadapter） |
| 配送状態の所有者 | 汎用delivery role | `audit_relay.deliveries`（Audit専用role）。汎用roleは引き続き42501 |
| 宛先 | Search bridge | Audit Store（別DB可）、idempotent ingest |
| 完了の証拠 | `outbox_events.delivered_at` | `deliveries.delivered_at` と、Store receipt（seq、digest、outcome） |
| 終端 | `dead_lettered_at` と7種のcode | quarantineと、Audit固有の詳細code（`quarantine_code`） |
| 投入前の関門 | Source lease | Store可用性admission（Store停止中はclaimせず、試行回数を消費しない） |

`ErrorCode` は閉じた7種なので、Audit固有の詳細code（`source_digest_mismatch`、`store_identity_conflict` 等）は、同一process内の有界な `DeliveryLedger`（`(event_id, lease_token)` をkeyにする）でhandlerからstore実装へ渡す。crashで失われた場合は汎用codeになる（安全側）。

### 6.2 状態と手順

pending → leased → delivered。leasedからretry待ち（pending + `available_at`）へ。pending/leasedからquarantinedへ。

1. admission：Store pingに失敗したらclaimしない（Store停止＝配送遅延）。
2. claim：DB clock、`FOR UPDATE SKIP LOCKED`、新しいlease token、attempt上限を初回claim時に固定する。
3. handler：`source_intact` を確認し、legacy投影・検証、`AuditStore::ingest`（timeoutはlease/3未満）を行う。
4. receipt：Stored/Duplicateならledgerへ記録し、Applied/KnownNoopを返す。
5. ack：fenced（token一致・lease未失効・未終端）で `delivered_at`、`store_seq`、`store_digest`、`store_outcome` を記録する。receiptの無いackは拒否する。
6. 失敗：Store不通・結果不明・timeoutはRetryable（同じevent IDで再送）。検証不正・identity conflictはTerminal（quarantine）。上限に達した結果不明は、reaperが `delivery_unknown_at_limit` としてquarantineする。

Store保存後・ack前にcrashした場合は、lease失効後の再配送でStoreがDuplicate（同digest）を返し、ackへ収束する。結果不明のcommitも同様である。staleなtokenはrenew・ack・failのいずれもできない。policyは既定 attempt 16、lease 30 s、backoff 1–300 s、batch 32、in-flight 4とする（`DeliveryConfig::validate` の範囲内）。

### 6.3 quarantineとreplay

quarantineは削除でも成功でもない。保持される終端証拠である。`audit-relay replay --event-id`（特権）は、Storeへ `audit.delivery.replay_requested` control eventを記録してからpendingへ戻し、`replay_count` を加算する。control eventを記録できなければ戻さない。

## 7. Audit Store（`audit_store` schema）

### 7.1 表

- `publication_head`（singleton：last_seq、last_chain、recovery_epoch）
- `events`（永続identity。seq PK、event_id UNIQUE、source、type、class、subject、occurred_at、stored_at、actor、resource、result、envelope_digest、digest_algorithm、source_digest、adapter_version、prev_chain、chain、recovery_epoch、expired_at、expired_by_seq。削除しない）
- `event_bodies`（seq PK FK、envelope jsonb。retentionでのみ削除する）
- `access_grants`、`retention_policies`、`legal_holds`（予約）

### 7.2 ingest（`audit_store.ingest(envelope jsonb, source_digest bytea, adapter_version int)`、SECURITY DEFINER）

1. 構造検査（specversion、id、type、32 KiB）。正規の検証はRust portで事前に行う（schema検証してからappendする）。
2. `envelope_digest = sha256(convert_to(envelope::text,'UTF8'))`（`kp-audit-jsonb-sha256-v1`）。PostgreSQLのjsonb正規化を正とする。export行はこのtextそのものなので、外部検証者は行のbyte列をhashするだけで済む。
3. `publication_head` を `FOR UPDATE` でlockする。全publication（ingest・control event・expire）が同じlockを通る。
4. event_idが既存の場合、同じdigestなら `duplicate`（期限到達済みなら `duplicate_expired`。本文は復活させない）。digestが違えば `conflict` とし、挿入せずに `audit.integrity.conflict_detected` を記録する。
5. 新規なら `seq = last_seq+1`、`chain = sha256('kp-audit-chain-v1' || prev_chain || int8send(seq) || uuid_send(event_id) || envelope_digest)` とし、events/bodies/headを同一transactionで更新する。

head lockをcommitまで保持するので、seqの公開はcommit順になり、可視のhead値は「それ以下が全てcommit済み」の閉じたwatermarkになる。直列化は意図したtrade-offである（Auditの量では許容範囲。試験で競合を確認する）。seqはStoreへのcommit順であり、業務の因果順ではない。`occurred_at` は保持する。

### 7.3 append-only

eventsのUPDATEは、retention関数がtransaction-local GUCを設定した場合に限り、`expired_at`/`expired_by_seq` をNULLから設定することだけを許す。eventsのDELETEとTRUNCATEは常に拒否する。bodiesはretention関数経由のDELETEだけを許し、UPDATEとTRUNCATEは拒否する。表への直接DML権限は誰にも与えず、すべての変更はSECURITY DEFINER関数を通す。通常業務API・Document role・relay roleからの変更・削除はできない（OA §16）。

## 8. Integrity（ADR：方式比較と選択）

| 方式 | 検出できるもの | 限界・運用 |
|---|---|---|
| A. event digestのみ | 偶発的な改変（期待digestを信頼できる場合） | 削除・digest同時改変・順序入替を検出できない |
| B. write-time hash chain＋event digest＋外部checkpoint | 改変、削除（seq欠番・chain断）、順序入替、checkpoint以前の全体書換え | DB ownerはchainを全体再計算できるため、外部保管checkpointとの照合が必要。restoreではrecovery epochを記録する |
| C. 検証時Merkle/rolling checkpoint | Bと同等（checkpoint時点） | checkpointの作成がO(n)になる。行単位の自己検証性が無い |
| D. 署名checkpoint・WORM・外部anchor | DB ownerへの耐性 | 鍵管理・新しい運用基盤が要る（v1範囲外。将来拡張） |

選択：B。

- 理由：ingestはcommit順の確定のためにhead lockで既に直列化されているので、chainのcostはinsertごとのsha256 1回で済む。checkpointはhead（seq, chain）の1組になる。
- retention：tombstone（identity行）がdigest/chainを保持するので、本文を削除してもchainは検証可能なままである。
- migration：chainは本文ではなくdigestに依存するので、既存行は再計算不要である。
- 業務transactionをまたぐchainは作らない。

検証（`audit_store.verify`）：開始時のheadをW（上限）に固定し、範囲内のseq連続性、prev_chainの連鎖、chainの再計算、本文digest、本文欠落=expired整合、envelope.id=event_id、head整合を検査する。結果は1件のcontrol event（`audit.integrity.verified`、成否、件数、head chain）に記録する。この記録はWより後のseqになるため、検証が自分の結果を無限に追いかけることは無い。外部checkpoint照合はCLI側で行い、seqの不一致を書換え、seq>headをtruncation/巻戻しと判定する。

## 9. Retention

- `retention_policies`（policy_id、revision、selector：event_class/type、retain_days、NULL=無期限）。既定では行が無いので失効しない。年数は固定しない（OA §26）。
- `expire(actor, policy_id, expected_revision, cutoff, limit≤1000)` には `maintain` 権限が要る。
  - head → policy → identityの順にlockし、revisionが一致しなければstaleとして削除しない。
  - legal holdが1件でも有効なら削除しない（v1の拡張境界。hold内容の判定規則は将来の拡張）。
  - 対象は `occurred_at < least(cutoff, now()-retain_days)` で、本文があり未失効のものに限る。
  - control event `audit.retention.expired`（件数、seq範囲、policy、revision）を先に記録し、そのseqで対象を印付けてから本文を削除する。identity・digest・chainは残す。
  - 失効済みeventの再配送は `duplicate_expired` を返し、本文は復活しない。
- source stagingの削除（破壊的cleanup）はv1では提供しない。Storeの失効は、source側のcopyを消したことを意味しない。

## 10. 調査・export・設定変更の認可と監査

- 権限（Organization roleではなくAuditの責務）：`investigate`、`export`、`verify`、`administer`（grant・retention policy）、`maintain`（expire・recovery epoch・replay）。Document ACLは流用しない。
- 主体は（issuer, principal_id）とする。呼出側service（CLI等の信頼済みadapter）が主体を渡し、DBはDB roleで呼出可否を、grantで主体の権限を判定する。本番identity連携は後続（§13）。
- 最初の管理者は `bootstrap_administrator`（schema ownerのみ実行可、administerが0件の場合だけ成功）で作り、control eventに記録する。
- `investigate` / `export_page` / `verify` / `change_access` / `set_retention_policy` / `expire` / `begin_recovery_epoch` / `lookup_receipts` はすべてSECURITY DEFINER関数とし、同一transactionで次を行う。
  1. 権限を判定する。拒否なら `audit.access.denied` を記録してdeniedを返し、何も開示しない。
  2. 入力を検証する（filter allowlist：event_types≤16、source、actor、resource、occurred範囲、event_ids≤100。limitは1–100、exportは1–1000）。
  3. watermarkを固定する（clientは現在headを超える値を指定できない）。
  4. 結果を取得する。
  5. control eventを記録する（呼出者、操作、bounded query shape、件数、watermark、page digest）。
  6. 返す。

  control eventの記録に失敗すればtransactionごと失敗し、何も開示しない。
- control eventの記録は内部関数で行い、調査APIを再帰呼出ししない。
- export行は `{"seq":…,"event_id":…,"envelope_digest":…,"prev_chain":…,"chain":…,"recovery_epoch":…,"expired":…,"envelope":<jsonb text原文>}` とする。`audit-core` のexport検証（RawValueでenvelope原文を保持し、sha256とchainを再計算）で、DB外で検証できる。manifestには件数、seq範囲、watermark、各pageのdigestとcontrol event IDを入れる。
- `lookup_receipts(event_ids)`（relay用）はdigest/seq/expiredだけを返し、本文は返さない。

## 11. Backup / restore 契約

- 対象は2つある。Document DB（staging＋`audit_relay`）とStore DBである。いずれも既存PostgreSQLの `pg_dump -Fc` / `pg_restore` を使い、新しい基盤は要らない。
- 外部checkpoint（`audit-admin checkpoint` の出力JSON：epoch、seq、chain、時刻）は、DBとは別の場所に保管する。同じ管理者が持つbackupは独立anchorではない。
- Store restore手順：
  1. 新しいDBへrestoreする。
  2. `audit-admin verify` で内部整合を確認する。
  3. 最新の外部checkpointと照合する。truncationを検出したら記録する。
  4. `begin_recovery_epoch`（control event）を実行する。
  5. `audit-relay reconcile --repair` で、delivered-missing（relayはackしたがStoreに無い）をpendingへ戻す。
  6. 再配送する（idempotent）。
  7. 再verifyと新しいcheckpointを取る。
- 復旧後に再配送されたeventは新しいseqを得る。旧checkpoint（restore点より後）とは一致しないので、新epochとして扱う。
- 失効より前の古いbackupをrestoreすると、失効済みの本文が戻り得る。手順上、restore後に現行retentionを再適用し、記録する。これをv1の限界として明記する。
- source（Document DB）を古いbackupへ戻した場合、Storeにあるがsourceに無いeventが生じ得る。reconcileはこれを `store_only` として報告し、自動では削除しない。

## 12. Health・reconciliation・restart

- `audit-relay health`（JSON）：
  - produced：staging件数、登録件数、未登録件数
  - pending / leased / retry待ち / quarantined（code別）
  - 最古pendingの経過時間、delivered件数
  - Store可用性、Store head seq、最終 `audit.integrity.verified` のseq・時刻（verified）、verification lag
  - trigger導入状態、policy revision

  labelは固定codeのみで、principal・resource・payloadを出さない。
- `audit-relay reconcile`：deliveredとStore receiptを照合し、次に分類する。
  - ok
  - delivered_missing
  - digest_mismatch
  - pending
  - quarantined
  - unregistered
  - source_tampered（digest再計算の不一致）

  `--repair` を付けた場合だけ、delivered_missingをpendingへ戻し、unregisteredを登録する。結果を `audit.reconciliation.completed` に記録する。新しいIDの生成や事実の書換えはしない。
- restartは試行履歴をresetしない。lease失効後に再claimする。shutdownではclaimを止め、有界にdrainし、未完了分はlease失効に任せる。

## 13. 境界とhandoff

- Organization：Role/Delegation/WorkItem/Workflowの意味をここで定義しない。現在の帰属情報（staging列のissuer/principal、`serviceExecutor`）を保持する。将来の接続点は次の3つである。
  1. source adapter trait（source_format/adapter_versionで識別）
  2. catalogに登録した `extensions` 名前空間（例 `org.work.v1`）
  3. source側の配送登録（Organization所有のmigrationで追加する）

  詳細は `docs/superpowers/handoffs/audit-infrastructure-v1-organization-handoff.md` に記す。
- Search：`search_audit_outbox_events` とR04AはSearch担当の領域である。adapter契約（配送登録またはSearch所有の未配送index＋grant、class/resultの写像、NO_RETENTIONの優先）をhandoffに記す。
- Document：producerの欠落（`authorization.denied` の範囲・試験、取下げ・公開終了時のschedule terminal audit、withdraw/endの理由文上限、通常ACLのreason未保存）は変更せず、Document担当への引継ぎ事項とする。

## 14. 試験と受入

合成データのみを使う。PostgreSQL 18.6（testcontainers。sourceとStoreは別database）で行う。

1. schema互換：main producerのpayload全形（nullable base、revision_comparison、bootstrap/通常ACL、denied、scheduler）がacceptされること。機微key・未知key・型違反・上限超過・重複key・不正correlationが拒否されること。生成schemaの再現性。
2. Store：idempotent（同digestはduplicate）、conflict、失効済みreplay、append-only（UPDATE/DELETE/TRUNCATE/直接INSERTの拒否）、head直列化と遅延commit、chain検証、改変・削除・順序入替の検出、外部checkpoint照合、権限なしread/exportのdenied記録と非開示、export検証、retention（cutoff前後、stale revision、hold、tombstone）、検証の非再帰。
3. 配送：登録trigger失敗時に業務rollback、Store停止中は業務継続でpendingが増え、復旧後に排出されること。不正schemaのquarantine、source改変の検出、staleなlease、結果不明のcommit、保存後・ack前のcrash（子processをkill -9してrestart）、同時重複、replay、reconcile/repair、backup→restore→epoch→再配送。
4. Document E2E：作成、公開、予約公開（scheduler attribution）、取下げ、metadata変更、folder操作、ACL変更、初回既読、原本アクセス、Diffアクセス、拒否を、実producer経由でStoreまで届け、actor/resource/correlation・理由文の非複製・chain検証を確認する。staging失敗時に業務rollbackすること。
