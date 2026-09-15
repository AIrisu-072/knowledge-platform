# Architecture Contract v0

- Status: v0
- Scope: Knowledge / Document Platform
- Purpose: 実装・PoC・将来拡張に対して、アーキテクチャ上の不変条件・責務境界・依存方向を固定する
- Language policy:
  - Backend / core: Rust優先
  - Human UI: TypeScript / React
  - Rustで要件を満たせない場合のみ他言語OSSを許容
- Library policy: library-first / composition-first
- License policy:
  - 原則許可: Apache-2.0 / MIT / BSD-2-Clause / BSD-3-Clause / PostgreSQL License / Public Domain
  - 原則除外: GPL / AGPL / LGPL / MPL-2.0 / SSPL / BSL / source-available / 独自利用制限
- Deployment policy:
  - 論理分離を優先する
  - 初期は1 Linux Server / 1 Rust applicationへの集約を許容する
  - 論理境界を物理サーバ境界と混同しない

---

# 1. System boundaries

本システムは最低限、以下の論理境界を持つ。

```text
Knowledge Sources
       |
       v
Source Adapters / ETL
       |
       v
Canonical Knowledge Model
       |
       v
Extraction / Normalization
       |
       v
Search Representations
       |
       v
Retrieval
       |
       v
Fusion / Reranking
       |
       v
Search API
      / \
 Human   LLM / Agent


Document Platform
       |
       +----> Knowledge Source


All Components
       |
       +----> OpenTelemetry ----> Observability Platform
       |
       +----> Audit Events -----> Audit Store
```

---

# 2. Core architectural invariants

## AC-01. Document Platform is the source of truth for managed documents

Document Platformは、組織内で管理する業務文書について以下の正本を保持する。

- Document
- DocumentVersion
- FileObject reference
- metadata
- folder / category
- access policy
- read / unread state

Search Index、Extraction結果、Vector表現等を正本として扱ってはならない。

---

## AC-02. Search Platform owns retrieval, not documents

Search Platformは文書そのものを所有しない。

Search Platformが保持してよいものは、Knowledge Sourceから再生成可能な派生データのみとする。

例:

- extracted text
- KnowledgeUnit
- lexical index
- vector index
- metadata index
- temporal representation
- graph representation
- retrieval cache

---

## AC-03. Search Platform is independent from any specific LLM

Search Platformは特定のLLM、LLM Chat、Agent frameworkに依存してはならない。

以下すべてが同じSearch APIを利用できること。

- Human Search UI
- LLM Chat
- Agent
- batch process
- 将来の別クライアント

---

## AC-04. Human and machine clients share the same core APIs

Human用APIとLLM用APIを別実装しない。

Human UIは共通API上のpresentation layerである。

```text
Common API
├─ Human UI
└─ LLM / Agent
```

UI都合の表現変換はFrontendまたは薄いpresentation layerで行い、Core business logicを複製しない。

---

## AC-05. RAG is a usage pattern, not a separate search system

RAG専用検索基盤を別途構築してはならない。

RAGは以下の利用形態と定義する。

```text
Search API
   ↓
SearchResult[]
   ↓
LLM Context
   ↓
Generation
```

Semantic / Vector RetrievalはSearch Platform内のRetrieverの1つとして扱う。

---

## AC-06. Extraction is indexing-time preprocessing

Office / PDF / ZIP等のExtractionは、原則として検索要求時に毎回実行しない。

```text
Source update
   ↓
Extraction
   ↓
Normalization
   ↓
Index update
```

検索時は事前生成された検索表現を利用する。

例外的なon-demand extractionを追加する場合は、明示的な設計判断を必要とする。

---

## AC-07. Search representations are derived projections

Canonical Knowledge Modelから複数の検索用表現を生成可能とする。

```text
Canonical Knowledge Resource
├─ Lexical representation
├─ Vector representation
├─ Structured / Metadata representation
├─ Temporal representation
└─ Graph representation
```

特定の検索方式をCanonical Modelへ埋め込まない。

---

## AC-08. Retrieval, Fusion, and Reranking are replaceable

以下は交換可能な境界として設計する。

- Retriever
- Fusion strategy
- Reranker
- Query planner

Tantivy、Vector engine、特定LLM、特定reranker model等をdomain contractへ直接露出させない。

---

## AC-09. Document consistency and Search consistency are intentionally different

Document Platformの正本はStrong Consistencyを要求する。

Search PlatformはEventual Consistencyを許容する。

Document transactionとSearch Index更新を分散transactionで束ねてはならない。

Transactional Outbox等で同期する。

---

## AC-10. Read state is principal x document_version

既読状態の論理キーは固定する。

```text
principal_id × document_version_id
```

`principal × document` へ簡略化してはならない。

新しいDocumentVersion公開時は、そのVersionにReadStateが存在しないことで自動的に未読となる。

---

# 3. DocumentVersion lifecycle contract

## 3.1 Persisted lifecycle state

永続化する状態は最小限とする。

```text
WORKING
PUBLISHED
WITHDRAWN
```

以下を永続化stateとして追加してはならない。

- DRAFT
- NON_PUBLIC
- WAITING_FOR_PUBLICATION
- CURRENT
- SUPERSEDED
- ARCHIVED
- DELETED

これらが必要な場合は、既存stateと属性から導出する。

---

## 3.2 Derived UI labels

表示ラベルは以下の情報から導出する。

- lifecycle_state
- approved_at
- scheduled_publish_at
- published_at
- effective_from
- effective_to
- withdrawn_at
- Document.current_version_id
- current time

例:

```text
WORKING + 未承認
→ 下書き

WORKING + 承認済み + scheduled_publish_atなし
→ 非公開

WORKING + 承認済み + scheduled_publish_atが未来
→ 公開待ち

PUBLISHED + current_version_id == self
→ 現行版

PUBLISHED + current_version_id != self
→ 過去版

WITHDRAWN
→ 公開終了
```

表示上の意味だけを理由にDB stateを増やしてはならない。

---

## 3.3 Published versions are immutable

公開済みDocumentVersionの本文・ファイル・版固有metadataを直接上書きしない。

変更は新しいDocumentVersionとして作成する。

---

## 3.4 No ordinary delete lifecycle

通常業務のDocument lifecycleに論理削除・物理削除を含めない。

検索対象外、公開終了、過去版化は状態・検索条件で表現する。

物理削除は以下の例外に限定し、通常APIから分離する。

- staging / orphan cleanup
- 法令・契約上の削除要求
- 誤登録された機密情報への管理対応
- 保守上の例外操作

---

# 4. Data ownership contract

## 4.1 Document Platform owns

- Document identity
- DocumentVersion identity
- current_version reference
- file reference
- document metadata
- version metadata
- folder / category
- read state
- access policy
- publication lifecycle

## 4.2 Search Platform owns

- KnowledgeSource configuration
- ETL state
- Canonical snapshots used for indexing
- KnowledgeUnit
- lexical representation
- vector representation
- metadata / structured representation
- temporal representation
- graph representation
- retrieval execution state
- fusion / reranking results
- search caches

ただし、これらはすべて正本ではない。

## 4.3 Identity system owns

- authentication
- Windows principal identity
- group identity

Document / Search domainはWindows統合認証の詳細を直接知らない。

Identity Adapterを介してstable principalを受け取る。

## 4.4 Audit system owns

- immutable / append-oriented audit records
- actor / action / resource / result / trace correlation

## 4.5 Observability system owns

- telemetry logs
- traces
- metrics

Observability dataを業務正本として使用しない。

---

# 5. Dependency direction

## 5.1 Domain must not depend on infrastructure

禁止例:

```text
document-domain -> sqlx
document-domain -> axum
document-domain -> tantivy
document-domain -> sspi-rs

search-core -> sqlx
search-core -> axum
search-core -> document-repository implementation
```

Domain / Coreはtrait・value object・domain typesのみを知る。

---

## 5.2 Infrastructure implements core contracts

許可例:

```text
document-repository-postgres
    -> document-domain

search-tantivy
    -> search-core

extractor-office
    -> extraction-core

identity-sspi
    -> identity-core
```

---

## 5.3 API depends on application services, not implementation internals

```text
api
  -> document-service
  -> search-service
```

API handlerからSQL、Tantivy、SSPI、filesystemを直接呼ばない。

---

# 6. Recommended module boundaries

初期候補:

```text
crates/
├─ document-domain
├─ document-application
├─ document-repository
├─ document-storage
│
├─ extraction-core
├─ extractor-office
├─ extractor-pdf
├─ extractor-archive
│
├─ search-core
├─ search-application
├─ search-tantivy
│
├─ identity-core
├─ identity-sspi
│
├─ audit-core
├─ observability
│
└─ api
```

必要になるまでcrateを無制限に細分化しない。

境界は責務・依存方向を守るために存在する。

---

# 7. Database contract

Transactional metadata DBは以下の責務に限定する。

- Document
- DocumentVersion
- Metadata
- Folder
- AccessPolicy
- ReadState
- Transactional Outbox
- transactional Audit event staging

DBへ原則として持たせないもの:

- full document binary
- lexical search index
- vector index
- telemetry logs

DB選定はDB Selection Criteria v0に従う。

---

# 8. File storage contract

Document binaryはDB metadataと分離する。

要求:

- immutable version fileを保持可能
- content hashを持てる
- staging / available / orphan reconciliationが可能
- DB transaction失敗時のorphan recoveryが可能

File Storage固有APIをDocument Domainへ露出させない。

---

# 9. Search / ETL contract

## 9.1 Knowledge Source Adapter

各Sourceは共通contractへ変換する。

例:

- Document Platform
- e-Gov
- internal DB / API
- future data source

Source自身の検索UI・検索順位へ依存しない。

必要なのは検索可能な元データとprovenanceである。

---

## 9.2 Canonical Knowledge Model

共通部分だけを比較的厳格にする。

最低限候補:

```text
source_id
resource_id
version_id

title
content
content_type
language

created_at
updated_at
effective_from
effective_to

provenance
locator
access_scope

metadata {}
```

Source固有情報を無理に共通schemaへ押し込まず、`metadata`等で拡張可能にする。

---

## 9.3 Query planning

LLMをQuery Plannerとして利用可能にする。

LLMが決定してよい候補:

- retriever selection
- retrieval profile
- filters
- lexical / semantic / temporal emphasis
- top-k parameters
- fusion strategy候補

LLMが検索基盤内部状態を直接変更してはならない。

生成されたRetrievalPlanはvalidator / policyを通す。

正常な探索に固定回数上限を設けない。

---

# 10. API contract

## 10.1 Common API

HumanとLLM / Agentで同じAPIを使用する。

主なAPI群:

```text
Document API
Search API
```

## 10.2 Search result

検索結果は特定UIに依存しないstructured responseを返す。

概念例:

```text
SearchResult
├─ source
├─ resource_id
├─ version_id
├─ title
├─ snippet
├─ highlights
├─ score / ranking metadata
├─ locator
├─ provenance
└─ metadata
```

Human UIはこれを視覚化する。

LLM / Agentは直接利用する。

---

# 11. UI contract

Human UIはTypeScript / Reactを基本とする。

責務:

- Human-friendly interaction
- document management UI
- search UI
- highlight / snippet visualization
- read / unread visualization
- version visualization

禁止:

- business invariantをFrontendだけで保証する
- UIだけに存在するDocument lifecycle rule
- UI側でSearch ranking truthを再実装する

Backend APIが唯一の業務契約となる。

---

# 12. Observability contract

各主要処理にtrace correlationを持たせる。

共通候補:

```text
trace_id
request_id
actor_id
source_id
resource_id
resource_version
component
operation
```

Search pipelineでは追加で以下を観測可能にする。

```text
query_id
retrieval_plan_id
retriever
candidate_count
rank
score
fusion method
reranker
```

検索精度評価・デバッグ時に、正解Resourceがどの段階で失われたか追跡可能であること。

---

# 13. Audit contract

Audit EventはObservability Logと分離する。

最低限:

```text
event_id
timestamp
actor
action
resource
resource_version
result
trace_id
source_system
metadata
```

機密本文・query本文を無条件でAudit / telemetryへ複製しない。

---

# 14. Library-first contract

実装前に必ず既存libraryを評価する。

優先順位:

```text
1. Existing Rust library
2. Composition of Rust libraries
3. Other-language OSS
4. Minimal custom implementation
```

自作を選択する場合は、なぜ既存libraryでは要件を満たせないかをADRまたはselection documentへ記録する。

---

# 15. Rust dependency policy

各crateで以下を確認する。

- license
- maintenance
- security advisory
- Cargo features
- transitive dependencies
- native dependency
- network/cloud dependency

可能な場合:

```toml
default-features = false
```

とし、必要featureのみ有効化する。

不要機能を利用しないという理由だけで多機能crateを排除しない。

---

# 16. Architecture lint requirements

CIで最低限、以下を機械検査する。

## LINT-01. Forbidden dependency direction

例:

- domain -> infrastructure禁止
- search-core -> document DB implementation禁止
- document-domain -> Tantivy禁止
- core -> Axum禁止

## LINT-02. License gate

Dependency treeに許可されていないlicenseが入っていないこと。

## LINT-03. Security advisory gate

RustSec等のadvisoryを検査する。

## LINT-04. Feature gate

意図しないdefault feature / cloud integrationが有効になっていないことを検査可能にする。

## LINT-05. Spec presence

Architecture Contract / Transaction Requirements / Logical Data Model等の規範文書をCI管理対象とする。

---

# 17. Repository structure contract

推奨:

```text
project-root/
├─ spec/
│  ├─ architecture/
│  │  ├─ architecture-contract-v0.md
│  │  ├─ system-architecture-v0.md
│  │  └─ system-architecture-v0.d2
│  │
│  ├─ data/
│  │  ├─ logical-data-model-v0.md
│  │  ├─ data-characteristics-v0.md
│  │  └─ transaction-consistency-requirements-v0.md
│  │
│  ├─ selection/
│  │  ├─ db-selection-criteria-v0.md
│  │  └─ rust-library-matrix-v0.md
│  │
│  └─ requirements/
│     └─ requirements-v0.md
│
├─ docs/
│  ├─ adr/
│  ├─ research/
│  └─ operations/
│
├─ experiments/
│  ├─ postgres-transaction-poc/
│  ├─ search-poc/
│  ├─ extraction-poc/
│  └─ identity-poc/
│
├─ crates/
├─ apps/
├─ tools/
├─ mise.toml
└─ Cargo.toml
```

`spec/` は規範。
`docs/` は説明・調査・意思決定履歴。

---

# 18. PoC isolation contract

PoCはproduction implementationと分離する。

PoCで検証したコードを無条件でproduction crateへ移植しない。

手順:

```text
PoC
 ↓
Evaluation
 ↓
Selection decision
 ↓
spec / ADR update
 ↓
Production implementation
```

---

# 19. Change control

本Contractに反する変更は、通常の実装変更として入れてはならない。

必要な場合:

1. 変更理由を記録
2. Architecture Contractを更新
3. 関連specを更新
4. Architecture lintを更新
5. その後実装

「実装が先、specが後」を常態化させない。

---

# 20. Non-goals of v0

現時点で固定しない。

- HA topology
- RPO / RTO具体値
- Backup製品
- Vector engine
- Reranker model
- Fusion algorithm
- OCR engine
- AccessPolicy詳細
- 完全なapproval workflow
- 特定UI design systemの最終確定

ただし、後から追加不能になる依存は作らない。

---

# 21. Architecture acceptance checklist

実装・PRレビュー時に確認する。

- [ ] Document Platformが正本所有者のままか
- [ ] Search Indexを正本として扱っていないか
- [ ] Search Platformが特定LLMへ依存していないか
- [ ] Human / LLM用business APIを重複実装していないか
- [ ] RAG専用検索系を作っていないか
- [ ] Extractionをquery-time必須処理にしていないか
- [ ] Search representationをCanonical Modelへ過剰に固定していないか
- [ ] Retriever / Fusion / Rerankerが交換可能か
- [ ] `principal × document_version` が維持されているか
- [ ] derived labelをpersistent stateへ増やしていないか
- [ ] published versionを上書きしていないか
- [ ] ordinary delete lifecycleを追加していないか
- [ ] strong / eventual consistency境界を壊していないか
- [ ] core/domainがinfrastructureへ依存していないか
- [ ] AuditとObservabilityを混同していないか
- [ ] 新規自作機能に既存library不採用理由があるか
- [ ] 初期deployment都合で論理境界を壊していないか

---

# 22. v0 decision summary

本Contractで固定する中心原則:

```text
Document Platform = managed document source of truth
Search Platform   = cross-source retrieval platform
Common API        = Human / LLM shared
RAG               = Search API usage pattern
Extraction        = indexing-time preprocessing
Search Index      = rebuildable derived state
Identity          = adapter boundary
Audit             = business traceability
Observability     = operational diagnostics
Deployment        = initially consolidated, logically separated
Implementation    = Rust-first, library-first
UI                = TypeScript / React
```

---

# 23. 横断標準・運用契約

以下の横断仕様を本Architecture Contractの下位規範として扱う。

```text
Error Handling & Resilience Requirements v0
Observability & Audit Requirements v0
```

規範技術:

```text
API                 = OpenAPI 3.2.1
Structural Schema   = JSON Schema 2020-12
HTTP Errors         = RFC 9457
HTTP Semantics      = RFC 9110
Trace Propagation   = W3C Trace Context
Telemetry           = OpenTelemetry / OTLP
Audit Envelope      = CloudEvents 1.0.x stable
```

Observability Backend、Audit Store、Code Generator、Validator等の具体製品・ライブラリは、
この契約を満たすものを後続Selectionで選定する。

Architecture実装が特定Backend製品のデータモデルへ直接依存してはならない。

---

# 24. 開発・Frontend横断契約

以下を本Architecture Contractの下位規範として扱う。

```text
Frontend / UX Requirements v0
Development / Container / CI Architecture v0
```

開発環境:

```text
Linux
macOS
Windows via WSL2 only
```

Production:

```text
linux/amd64 OCI image
```

CI:

```text
GitHub-hosted runners only
Linux = authoritative
macOS = portability
```

Local / CIのtask entrypointは`mise`へ集約する。

---

# 25. Development Assurance Contract

以下を本Architecture Contractの下位規範として扱う。

```text
Development Assurance Architecture v0
```

開発支援の基本モデル:

```text
Spec Graph
+
Desired Architecture Graph
+
Observed Code Graph
+
Assurance Graph
      ↓
Development Assurance Control Plane
```

LLMへRule全文を常時注入せず、可能な限り外部Provider / Gateway / CIで制約を強制する。
Explorationの自由度を維持し、Violation / Gap / Counterexampleをfeedbackとして返す。

v0 Code GraphはLevel 0-1を対象とし、Symbol/Semantic Graphは将来拡張とする。

