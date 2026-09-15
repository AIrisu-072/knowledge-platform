# Observability & Audit Requirements v0

- 状態: v0
- 対象: Knowledge / Document Platform 全体
- 目的: 実装言語・DB・検索方式・Telemetry Backend・Audit Storeに依存しない、観測・監査・相関・機密情報保護の共通契約を定義する
- 基本原則:
  - **Observability と Audit は別責務**
  - Observability は障害調査・性能改善・検索品質分析のための運用データ
  - Audit は「誰が・何を・いつ・どの対象に対して行い、結果がどうなったか」を追跡する業務・セキュリティ証跡
  - Observability はsampling可能、Auditはsamplingしない
  - Observability障害は原則として業務transactionを失敗させない
  - 必須Audit Eventの生成失敗は、対象業務transactionと同一commit境界で扱う
  - 機密本文・資格情報・個人情報を無条件にTelemetry/Auditへ複製しない

---

## 1. Normative / reference standards

### 1.1 Normative technical standards

| 領域 | 規格 |
|---|---|
| Telemetry model / API / SDK | **OpenTelemetry Specification** |
| Telemetry transport | **OTLP** |
| Trace propagation | **W3C Trace Context** |
| Audit event envelope | **CloudEvents 1.0.x stable specification** |
| HTTP semantics | **RFC 9110** |
| Timestamps in audit interchange | **RFC 3339 / ISO 8601互換表現** |

OpenTelemetryのSemantic Conventionsは、**Stableな領域を優先して採用**する。
Development / Mixed stabilityのConventionを利用する場合は、その依存を明示し、破壊的変更へ追従可能なAdapter境界を持つ。

### 1.2 Operational / security guidance

以下は実装規格というより、ログ管理・セキュリティ記録の設計ガイダンスとして利用する。

- OWASP Logging Cheat Sheet
- NIST SP 800-92 / Cybersecurity Log Management guidance

国内固有の業界ガイドラインは、導入時の適合確認に利用してよいが、本仕様のTelemetry/Auditデータモデルの規範源にはしない。

---

# 2. Signal separation

本システムは最低限、以下の信号を区別する。

```text
Observability
├─ Traces
├─ Metrics
└─ Logs

Audit
└─ Audit Events

Evaluation
└─ Search / Retrieval evaluation artifacts
```

## 2.1 Traces

用途:

- request処理の因果関係
- Search pipelineのstage追跡
- Extraction / Indexing処理
- DB / File Storage / Source Adapter依存
- Error correlation
- latency analysis

## 2.2 Metrics

用途:

- SLI / SLO
- throughput
- latency
- saturation
- queue / outbox lag
- error rate
- indexing freshness
- extraction success rate
- partial search rate

## 2.3 Logs

用途:

- structured diagnostic events
- process / component状態
- troubleshooting
- error context

## 2.4 Audit Events

用途:

- actor / action / resource / resultの永続証跡
- 特権操作追跡
- 文書公開・権限変更・データアクセス等の追跡
- incident investigation

## 2.5 Evaluation artifacts

検索品質分析では、通常Telemetryへ大量の候補document IDや全文を流さない。

詳細な候補順位・score・gold answer等は、明示的なEvaluation Runとして別artifactに保存可能にする。

---

# 3. OpenTelemetry boundary

## 3.1 Application instrumentation

各主要componentはOpenTelemetry互換instrumentationを持つ。

```text
Rust Application
├─ API
├─ Document Platform
├─ Search Platform
├─ Extraction
├─ Source Adapters
├─ Outbox Worker
└─ Identity Adapter
        │
        ▼
      OTLP
        │
        ▼
OpenTelemetry Collector
        │
        ▼
Observability Backend
```

Backend固有SDKをDomain / Application Layerへ直接持ち込まない。

## 3.2 Preferred export boundary

Applicationからは原則としてOTLPを利用し、OpenTelemetry CollectorをObservability Backendとの境界にする。

Collector / Backend製品は後から変更可能であること。

## 3.3 Collector failure

Collectorが停止しても、通常のDocument/Search業務を停止させない。

必要に応じて:

- bounded queue
- batch export
- drop policy
- exporter retry

を利用するが、Telemetry保持のために業務処理を無制限blockしない。

---

# 4. Trace context

W3C Trace Contextを利用する。

標準伝播:

```text
traceparent
tracestate
```

独自Trace Headerを正本として設計しない。

以下の境界でcontextを維持する。

- HTTP request
- async task
- background worker
- Transactional Outbox consumer
- Source Adapter
- Search pipeline
- future service split

単一process構成でも将来のprocess分離を阻害しない。

---

# 5. Baggage policy

OpenTelemetry Baggageは自動的に下流へ伝播し得るため、**機密情報・個人情報・安定した利用者識別子を原則入れない**。

禁止例:

- credential
- token
- private key
- document body
- search query本文
- customer data
- account number
- raw principal name
- confidential metadata

利用する場合は明示的allowlistとする。

Baggageは信頼境界・認可判断の入力に使用しない。

---

# 6. Resource / service identity

Telemetryにはcomponentを識別できるResource属性を持たせる。

候補:

```text
service.name
service.version
service.instance.id
deployment.environment.name
```

将来的に単一binaryから複数serviceへ分離しても同一命名規約を利用する。

---

# 7. Common correlation identifiers

必要な場所で以下を相関可能にする。

```text
trace_id
span_id
request_id
operation_id
audit_event_id
actor_principal_id
resource_id
resource_version
query_id
retrieval_plan_id
source_id
outbox_event_id
```

ただし、Signalごとに必要最小限だけ記録する。

## 7.1 Telemetry

原則として高cardinality識別子をMetric labelへ入れない。

`resource_id`、`principal_id`、`query_id`等はTrace / Logで必要時に利用し、Metric dimensionには原則使用しない。

## 7.2 Audit

actor / resourceの一意識別はAudit上必要なため保持可能。

表示名ではなくstable identifierを優先する。

---

# 8. Structured logging contract

Application logは構造化形式を基本とする。

最低限候補:

```text
timestamp
severity
event_name
component
operation
trace_id
span_id
error.code
message
attributes
```

「when / where / who / what」を後から復元できる設計を目指す。

自由文だけのログを主要な機械分析インターフェースにしない。

---

# 9. Severity policy

SeverityはOpenTelemetry Logs Data Modelへ自然にmapping可能な形にする。

概念:

```text
TRACE
DEBUG
INFO
WARN
ERROR
FATAL
```

運用環境でDEBUG/TRACEを恒常的に大量出力しない。

業務上期待されるValidation Error等をすべてERRORとして記録しない。

例:

```text
Validation failure        -> INFO / WARN候補
Authorization denial      -> WARN
Transient dependency fail -> WARN / ERROR
Unhandled internal error  -> ERROR
Process cannot continue   -> FATAL
```

具体mappingはError Registryと連携する。

---

# 10. Trace design

Traceは「何が遅いか」「どこで失敗したか」を段階的に追跡可能にする。

## 10.1 API trace

```text
HTTP Request
└─ Application Use Case
   ├─ DB operation
   ├─ File operation
   └─ downstream dependency
```

## 10.2 Search trace

```text
Search Request
├─ Query Planning
├─ Retriever: lexical
├─ Retriever: semantic
├─ Retriever: structured
├─ Retriever: temporal
├─ Fusion
├─ Reranking
└─ Response serialization
```

最低限観測する候補:

```text
query_id
retrieval_plan_id
retriever.type
candidate_count
duration
partial
fallback_used
error.code
```

通常Traceに全candidate ID / scoreを大量記録しない。

## 10.3 Extraction trace

```text
Extraction Job
├─ container traversal
├─ parser selection
├─ extraction
├─ normalization
└─ indexing handoff
```

候補属性:

```text
format
file_size
container_depth
extracted_unit_count
duration
outcome
error.code
```

文書本文は記録しない。

---

# 11. Metrics requirements

初期Metricsは「意思決定に使えるもの」に限定する。

## 11.1 API

- request count
- request latency histogram
- error count / rate
- active request count

## 11.2 Document Platform

- document create count
- version publish count
- read-state update count
- transaction conflict count
- DB pool utilization
- DB transaction latency

## 11.3 Search Platform

- search request count
- search latency
- partial result rate
- no-result rate
- retriever latency
- retriever candidate count distribution
- fusion latency
- rerank latency
- fallback rate

## 11.4 Index / ETL

- extraction success / failure count
- extraction latency
- outbox pending count
- outbox oldest age
- index update lag
- failed indexing jobs
- rebuild progress

## 11.5 Cardinality rule

以下をMetric labelへ原則入れない。

```text
principal_id
document_id
document_version_id
query_id
trace_id
raw URI
search query
filename
```

高cardinalityによるMetrics backendの劣化を防ぐ。

---

# 12. Sampling policy

## 12.1 Metrics

原則としてsamplingしない。

## 12.2 Traces

sampling可能。

ただし以下は優先的に保持可能なpolicyを設計する。

- error trace
- high latency trace
- critical operation
- explicit diagnostic session

Sampling方式・rateは後続で選定する。

## 12.3 Logs

severity / event classに応じてvolume control可能。

## 12.4 Audit

**sampling禁止。**

Audit対象に指定されたEventは全件記録する。

---

# 13. Audit event standard

Audit Eventのtransport-neutral envelopeとして **CloudEvents 1.0.x stable** を採用する。

概念例:

```json
{
  "specversion": "1.0",
  "id": "01K...",
  "source": "urn:knowledge-platform:document",
  "type": "com.knowledge-platform.document.version.published.v1",
  "subject": "document/abc/version/4",
  "time": "2026-09-14T10:00:00Z",
  "datacontenttype": "application/json",
  "data": {
    "actor": "...",
    "result": "success",
    "trace_id": "...",
    "reason_code": null
  }
}
```

CloudEventsを採用する理由:

- event envelopeを独自発明しない
- producer / consumer / storage実装を分離できる
- id / source / type / subject / timeを標準化できる
- 将来event transportを変更しやすい

Audit固有payloadはJSON Schemaで別途定義可能にする。

---

# 14. Audit event classes

最低限以下のClassを持つ。

```text
SECURITY
PRIVILEGED_OPERATION
CONTENT_LIFECYCLE
ACCESS_POLICY
DATA_ACCESS
SEARCH_ACCESS
CONFIGURATION
SYSTEM_AUDIT
```

## 14.1 必須監査候補

- authentication / authorization failureで重要なもの
- privilege / role / access policy変更
- document create
- document version create
- document publish
- document withdraw
- document export / download
- privileged management operation
- audit configuration change
- integrity violation
- destructive maintenance operation

## 14.2 高量イベント

以下は保存量・監査要件を確認してpolicy化する。

- document.read
- search.execute
- search.result.open

ただし、**Audit Eventとして扱うと決めた場合はsamplingしない**。

`ReadState` は現在状態であり、履歴Auditの代替ではない。

---

# 15. Audit durability

Audit EventはObservability Logより強い耐久性を要求する。

必須Eventについては:

```text
Business Transaction
├─ business state update
└─ Audit / Outbox Event creation
       ↓ atomic commit
```

とする。

Audit Storeへの配送自体はEventual Consistencyを許容する。

配送要件:

- at-least-once
- retry
- idempotent consumer
- duplicate detection
- failed / dead-letter状態の観測

Audit delivery失敗でbusiness event自体を「発生していなかったこと」にしない。

---

# 16. Audit store requirements

具体製品は後続選定とする。

最低要件:

- append-oriented
- immutable / tamper-evident構成へ拡張可能
- retention policy
- access control
- export
- search / investigation
- event ID一意性
- ordering / timestamp保持
- backup / restore
- integrity verification

通常業務APIからAudit Eventの変更・削除を行えないこと。

---

# 17. Audit event schema

Audit payload候補:

```text
actor
actor_type
action
resource_type
resource_id
resource_version
result
reason_code
trace_id
request_id
source_system
client_context
metadata
```

機械判定はstable code / identifierで行う。

Human-readable messageは補助情報とする。

---

# 18. Search audit privacy

検索Queryは高度に機密性が高い可能性がある。

Audit / TelemetryへQuery全文をデフォルト保存しない。

代替:

```text
query_id
query_hash
query_length
source set
retrieval profile
result count
partial
```

Query本文の保存が必要になった場合は:

- 目的
- retention
- access control
- masking
- legal / compliance requirement

を別途承認する。

---

# 19. Sensitive data classification

Telemetry / Auditで以下を区別する。

```text
PUBLIC
INTERNAL
CONFIDENTIAL
RESTRICTED
SECRET
```

名称は後続Data Classification仕様で変更可能。

最低原則:

- credential / secretは記録禁止
- document bodyは通常記録禁止
- raw financial/customer dataは通常記録禁止
- stable IDsを利用し、表示名・本文を減らす
- 必要なattributeはallowlist型を優先

---

# 20. Redaction / filtering

Telemetry pipelineには必要に応じてCollector側のfilter / transform / redactionを利用可能にする。

ただし最善策は**Applicationから最初から機密情報を出さないこと**。

自動instrumentationが収集するattributeもレビュー対象にする。

---

# 21. Clock / time requirements

AuditではUTCを基準に時刻を保持する。

外部表現はRFC 3339互換形式を使用可能にする。

Duration計測はwall clockではなくmonotonic clockを利用する。

Host / VM / containerの時刻同期を運用要件とする。

大きなclock skewを検知可能にする。

---

# 22. Reliability semantics

## 22.1 Observability

Telemetry送信失敗:

```text
business operation -> 継続
telemetry           -> retry / bounded buffer / drop policy
```

## 22.2 Audit

必須Audit Eventのtransactional creation失敗:

```text
business transaction -> commitしない
```

Audit Event作成成功後のremote Audit Store delivery失敗:

```text
business state       -> committed
audit outbox         -> retry
```

---

# 23. Health / diagnostics

最低限以下を区別する。

```text
liveness
readiness
dependency health
degraded state
```

Readinessは「processが生きている」だけでなく、必要な依存へ業務要求を安全に受け付けられる状態かを表す。

Search Platformの一部Sourceが利用不能でも、partial searchを提供可能ならsystem全体をunreadyにする必要はない。

---

# 24. Observability for partial failure

Partial Failureはtrace / metric / logへ反映する。

例:

```text
search.partial = true
search.source.failed_count = 1
```

ただし失敗Source名をMetric labelへ大量展開しない。

少数固定Source categoryならlabel利用を検討可能。

---

# 25. Search quality observability

オンラインTelemetryで最低限:

- candidate count
- retriever latency
- fusion latency
- rerank latency
- partial / fallback
- no result

を観測する。

検索精度そのもの:

```text
Recall@K
MRR
nDCG
Precision@K
```

は通常運用Metricsだけでなく、Evaluation Harnessの測定値として管理する。

Telemetry Backendを検索品質評価DBとして濫用しない。

---

# 26. Retention

具体的な保存期間はv0では固定しない。

Signalごとに別policyを持てること。

```text
Trace retention
Log retention
Metric retention
Audit retention
Evaluation artifact retention
```

AuditとTelemetryを同一保持期間にしない。

---

# 27. Access control

Observability BackendとAudit Storeの閲覧権限を分離可能にする。

Audit Storeはより強いaccess controlを要求できること。

Audit閲覧・export自体も監査対象にできる構造とする。

---

# 28. Development / test policy

テスト環境で実データをTelemetry/Audit fixtureに使わない。

Synthetic dataを使用する。

Contract testでは:

- trace context propagation
- error ↔ telemetry correlation
- audit event schema
- CloudEvents envelope
- sensitive field absence
- metric cardinality policy

を検証可能にする。

---

# 29. CI requirements

最低限以下を検証対象にする。

```text
OpenTelemetry instrumentation contract tests
CloudEvents Audit schema validation
Audit event type uniqueness
Sensitive attribute denylist / allowlist checks
Metric naming / label policy
Trace propagation test
Audit ↔ transaction test
Generated schema reproducibility
```

具体的なlint / generatorはライブラリ選定後に決める。

---

# 30. Repository placement

推奨:

```text
spec/
├─ operations/
│  ├─ error-handling-resilience-requirements-v0.md
│  └─ observability-audit-requirements-v0.md
│
├─ telemetry/
│  ├─ audit-event.schema.json        # 後続
│  └─ semantic-conventions.yaml      # 独自拡張が必要な場合のみ
│
└─ errors/
   └─ error-registry.yaml            # 後続
```

OpenTelemetry標準Semantic Conventionを再定義しない。

独自属性が必要な場合のみproject namespaceで追加する。

---

# 31. Library selection requirements

本仕様を固定した後で以下を選定する。

- Rust OpenTelemetry SDK / integration
- `tracing` bridge
- OTLP exporter
- OpenTelemetry Collector distribution / configuration
- Observability Backend
- Audit Event library / CloudEvents implementation
- Audit Store
- Redaction / policy tooling

**ライブラリありきで本仕様を変更しない。**

候補ライブラリが本仕様を満たさない場合は、候補側を落とすかAdapterで吸収する。

---

# 32. Acceptance criteria

- [ ] ObservabilityとAuditが別責務である
- [ ] Trace / Metric / Log / Audit / Evaluationを区別する
- [ ] OpenTelemetry / OTLPをobservability境界に利用する
- [ ] W3C Trace Contextを利用する
- [ ] Audit EventはCloudEvents 1.0.x stable envelopeを利用する
- [ ] Audit Eventはsamplingしない
- [ ] 必須Audit Event作成はbusiness transactionと整合する
- [ ] Audit配送はat-least-once + idempotentにできる
- [ ] Telemetry障害で通常業務を停止させない
- [ ] 機密情報をBaggageへ入れない
- [ ] document/query本文をTelemetry/Auditへデフォルト保存しない
- [ ] Metric labelのcardinalityを制御する
- [ ] Search pipelineのstageごとのlatency/失敗を追跡可能
- [ ] Partial Failureを観測可能
- [ ] Search品質評価を通常Telemetryと分離できる
- [ ] Audit Storeをappend-oriented / tamper-evidentへ拡張可能
- [ ] Audit閲覧自体を監査可能にできる
- [ ] Backend製品を変更してもApplication契約が変わらない

---

# 33. References

- OpenTelemetry Specification
  - https://opentelemetry.io/docs/specs/otel/
- OpenTelemetry Semantic Conventions
  - https://opentelemetry.io/docs/specs/semconv/
- OpenTelemetry Logs Data Model
  - https://opentelemetry.io/docs/specs/otel/logs/data-model/
- OTLP Specification
  - https://opentelemetry.io/docs/specs/otlp/
- W3C Trace Context
  - https://www.w3.org/TR/trace-context/
- CloudEvents Specification
  - https://github.com/cloudevents/spec
- OWASP Logging Cheat Sheet
  - https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html
- NIST SP 800-92
  - https://csrc.nist.gov/pubs/sp/800/92/final
