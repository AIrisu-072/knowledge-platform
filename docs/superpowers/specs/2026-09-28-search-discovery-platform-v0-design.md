# Search / Discovery Platform v0 — Design Spec

- Status: **PROPOSED / WRITTEN REVIEW PENDING**
- Capability: Search Platform v0 / Discovery
- Repository: `AIrisu-072/knowledge-platform`
- Design branch baseline: `main@6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`
- Design date: 2026-09-28 JST
- Scope class: Architectural
- Implementation status: **NOT STARTED**
- User approval state: D1〜D9の会話上の設計方針は承認済み。本書そのものの承認は未実施。
- Normative status: 本書承認後、既存 `spec/` の矛盾箇所を実装計画で明示的に改訂する。本書単独で既存規範を暗黙変更しない。

---

# 1. Purpose

Search Platform v0 は `knowledge-platform` 内部の論理サブシステムとして、文書検索だけでなく、Human / LLM / Agent が現在のNeedに必要なResourceを発見し、適用可能性・根拠・不足情報まで評価できる共通Discovery基盤を提供する。

対象Resourceは少なくとも以下である。

- KnowledgeResource
- SemanticResource
- CapabilityResource
- AgentSkillResource
- WorkflowResource
- PolicyResource

Search Platformは業務データの正本を所有しない。各Sourceが正本を維持し、Search Platformは再生成可能なDiscovery Projection、Index、検索実行状態、Session Working Setを扱う。

本Capabilityの目標は「類似度の高いTop-Kを返す」ことではない。

```text
Need
 ↓
Source discovery
 ↓
Candidate retrieval
 ↓
Applicability / Contrast
 ↓
Evidence resolution
 ↓
QualifiedResource + Evidence + InformationGap
```

を共通契約として成立させる。

---

# 2. Existing architecture preserved

以下の既存Architecture原則を維持する。

1. Document Platformは管理文書の正本。
2. Search Platformが保持する検索表現は派生データ。
3. Search Platformは特定LLM / Agent frameworkへ依存しない。
4. Human / LLM / Agentは共通Core APIを利用する。
5. RAG専用の別検索基盤を作らない。
6. Retriever / Fusion / Reranker / Query Plannerは交換可能境界。
7. Document transactionとSearch Index更新を分散transactionで結合しない。
8. Domain / CoreはSQLx、Tantivy、Axum、特定Graph DB、特定Vector engine等へ依存しない。
9. 初期Deploymentは1 Rust applicationへ集約可能だが、論理境界と物理境界を混同しない。
10. library-first / Rust-firstを維持する。

本設計で既存Architectureを拡張する主要点は以下。

- Search Platformの上位CapabilityをDiscoveryとして明示する。
- Source Registryをfirst-classにする。
- 文書以外のResource typeを正式に扱う。
- Graph Representationをv0 first-class Projectionへ昇格する。
- GraphのCanonical relation semanticsをTyped N-ary Relation / HyperEdgeとする。
- Retrieval終了条件をTop-KではなくEvidence Sufficiencyへ拡張する。
- Agent利用のためのSession Binding / Context境界を定義する。

---

# 3. System boundary

```text
Knowledge Platform
│
├─ Document Platform
├─ Extraction / DSI
├─ Identity
├─ Audit / Observability
│
└─ Search Platform
    ├─ Source Registry
    ├─ Discovery
    ├─ Projection / Indexing
    ├─ Retrieval
    ├─ HyperGraph Retrieval
    ├─ Qualification
    ├─ Evidence Resolution
    └─ Common Search / Discovery API
```

Search Platformの責務:

- Source discovery / routing
- Resource discovery
- Discovery Projection生成
- Search Index生成
- Candidate federation
- Applicability / Contrast評価
- Evidence Requirement / Sufficiency評価
- Information Gap生成
- Session Working Index
- Binding候補生成
- Discovery trace

Search Platform外の責務:

- Document / CRM / 業務DB等の正本管理
- Credential管理
- Current authorizationの最終判断
- Task DAG ownership
- Scheduler
- Tool execution
- Execution Ledger
- Retry / compensation
- Human oversight orchestration
- LLM conversation state

Search Platformは実行可能Resourceを発見できるが、検索結果をExecution Authorizationとして扱ってはならない。

---

# 4. Search and Discovery

SearchはResourceを取得する機構、DiscoveryはSearchを利用してNeedを解決する上位Capabilityとする。

```text
SearchRequest
→ SearchResult[]

DiscoveryRequest
→ DiscoveryResult
```

Discoveryは最低限以下を含む。

- Source Routing
- Candidate Retrieval
- Qualification
- Contrastive Resolution
- Authority / Temporal Resolution
- Evidence Resolution
- Information Gap Resolution

Graph RAGは独立Platform / 独立利用者向けAPIとして公開しない。

```text
Search Platform
└─ Discovery
   └─ Retrieval
      └─ HyperGraph Retriever
```

内部ではGraph Projection / Store Adapter / Traversal Planner / Retrieverを独立境界にできる。

---

# 5. Discoverable Source model

```text
DiscoverableSource {
    source_id
    source_type

    resource_types[]
    business_domains[]
    concept_refs[]

    discovery_modes[]
    discovery_capabilities[]

    enumeration_semantics

    authority_scope
    provenance

    access_model
    retention_mode
    freshness_policy

    latency_profile?
    cost_profile?
    availability_state?
}
```

Discovery mode:

- LOCAL_DIRECTORY
- LOCAL_CONTENT_SEARCH
- REMOTE_ENUMERATION
- REMOTE_QUERY
- DIRECT_ADDRESS
- LIVE_ONLY

Enumeration semantics:

- COMPLETE
- PARTIAL
- QUERY_ONLY
- NONE

Remote search missをResource absenceとして扱わない。Sourceが提供するcoverage semanticsに基づいてPresence evidenceを評価する。

Retention mode:

- PERSISTENT_RESOURCE
- PERSISTENT_DISCOVERY_METADATA
- CACHE_WITH_EXPIRY
- SESSION_ONLY
- NO_RETENTION

Retentionとruntime materializationは別契約とする。NO_RETENTION SourceでもProvider契約上許可される場合、runtimeで全文を読み、終了後に破棄できる。

---

# 6. Federated Discovery Fabric

単一巨大Global Resource Indexを必須にしない。

```text
L0 Global Source Registry
        ↓
L1 Source-local Resource Directory
        ↓
L2 Source-local Specialized Indexes
        ↓
L3 Authoritative Resource
```

Sourceを初期Primary Shard Boundaryとする。巨大SourceはSource内subshard可能とする。業務domainだけを理由に早期物理shardしない。

Security上、Resourceの存在自体が機密となる場合はSource / Access境界に沿う物理分離を許容する。

Source routingはHard single classifierにしない。Need、Authority、Freshness、Coverage、不確実性に応じてfan-out / expansionできる。

---

# 7. Discoverable Resource model

v0の基本型:

```text
DiscoverableResource
├─ KnowledgeResource
├─ SemanticResource
├─ CapabilityResource
├─ AgentSkillResource
├─ WorkflowResource
└─ PolicyResource
```

Resource共通構造:

```text
DiscoverableResource
├─ ResourceIdentity
├─ ResourceBody
├─ UsageProfile[]
├─ DiscoveryProfile
├─ TemporalDiscoveryProfile
└─ ResourceRelations[]
```

すべてをDocument型、共通body文字列、Embeddingへ潰さない。

SemanticResourceはDataset / Field / Metric / Relationship / Concept等を扱い、Apache Ossie等のSemantic Layer interchangeをAdapterとして接続可能にする。Ossie等をSearch Platform全体のDomain Modelへしない。

---

# 8. Resource Identity

```text
ResourceIdentity {
    resource_id
    resource_type
    resource_version?

    source_id
    source_native_id?

    provenance

    valid_from?
    valid_to?

    access_scope

    integrity_digest?
}
```

名前、title、similarityをidentityとして扱わない。

同一Resourceの意味と具体的提供形態を分離する。

```text
LogicalResource
    ↓
ResourceRepresentation[]
    ↓
ResourceVersion[]
    ↓
DiscoveryProjection
```

PlanningはLogicalResourceを扱える。Execution用Bindingは最終的にRepresentation / Versionへ到達する。

---

# 9. Usage Profile

同じResourceに複数用途を許容する。

```text
UsageProfile {
    usage_profile_id

    purpose

    business_domain?
    business_operation?

    subject?
    object?
    product_scope?

    lifecycle_phase?
    actor?

    trigger?
    action?
    effects?

    applicable_when
    not_applicable_when

    required_context[]
    required_resources[]

    expected_input?
    expected_outcome?
}
```

同一Toolが複数業務へ利用可能であることをResource複製で表現しない。

Negative applicabilityをfirst-classにする。

---

# 10. Discovery Profile and Lens

DiscoveryProfile:

```text
DiscoveryProfile {
    canonical_name
    aliases[]
    concept_refs[]
    intents[]

    high_signal_facets{}

    positive_signals[]
    negative_signals[]

    confusable_with[]
    distinguished_by[]

    projection_version
}
```

`high_signal_facets` は熟練者がResource本体を読む前に確認する識別項目を機械可読にしたものとする。

DiscoveryLens:

```text
DiscoveryLens {
    lens_id
    lens_version

    resource_type
    domain_scope?
    source_scope?

    identity_fields[]
    high_signal_facets[]
    searchable_fields[]
    applicability_fields[]
    temporal_fields[]
    relation_fields[]

    extraction_policy
    projection_policy
}
```

Lens階層:

```text
Base Lens
 ↓
Resource Type Lens
 ↓
Domain Lens
 ↓
Source Extension
```

Concept synonymを各Resourceへ展開複製しない。ResourceはConceptRefを保持し、Semantic Registryがsynonym / hierarchyを解決する。

---

# 11. Intent Signature

自然言語NeedをDiscoveryで比較可能なtyped intentへ変換する。

```text
IntentSignature {
    purpose

    business_domain?
    business_operation?

    subject?
    object?
    product_scope?

    lifecycle_phase?
    actor?

    trigger?
    requested_action?
    expected_effect?

    temporal_target?

    required_authority?
    required_freshness?

    constraints[]
    unresolved_facets[]
}
```

Intentの各Factは出所を保持する。

- EXPLICIT
- DERIVED
- INFERRED

LLM推論とHuman明示Factを同じ強さにしない。

---

# 12. Applicability model

Similarityはcandidate generationに限定する。

Applicability state:

- APPLICABLE
- EXCLUDED
- UNRESOLVED
- INVALID

Predicate state:

- TRUE
- FALSE
- UNKNOWN
- ERROR

MissingをFalseとして扱わない。

評価順序の原則:

```text
Access
 ↓
Temporal validity
 ↓
Hard applicability
 ↓
Hard discriminator
 ↓
Authority requirement
 ↓
Contrast
 ↓
Soft ranking
```

Vector similarity等が高くてもHard mismatchならEXCLUDED。

---

# 13. Typed Predicate IR

Applicability Ruleを任意スクリプトやLLM文章だけで実行しない。

v0 type候補:

- Boolean
- String
- Integer
- Decimal
- Date
- DateTime
- Duration
- ConceptRef
- ResourceRef
- List
- Set
- Money
- Quantity

v0 operator候補:

- AND / OR / NOT
- EQ / NE / LT / LTE / GT / GTE
- IN / CONTAINS / INTERSECTS / SUBSET
- EXISTS / MISSING
- SAME_CONCEPT / IS_A / DESCENDANT_OF

IRはtyped、side-effect-free、terminatingとする。Money等をfloatへ簡略化しない。

Hard discriminatorには最低Evidence classを要求可能にする。INFERREDのみで高保証Hard gateを満たさない構成を可能にする。

---

# 14. Fact and Information Gap

```text
Fact {
    fact_id
    value
    type

    origin
    evidence_ref?

    observed_at?
    valid_from?
    valid_to?

    confidence?
}
```

Origin候補:

- EXPLICIT
- AUTHORITATIVE
- OBSERVED
- DERIVED
- INFERRED

UNRESOLVEDはInformationGapへ変換する。

```text
InformationGap {
    gap_id
    required_fact
    reason
    blocking
    acceptable_evidence
}
```

Resolution traceは同一state + 同一Actionのno-progress loopを防止する。

---

# 15. Contrastive Resolution

既知の紛らわしいResource群:

```text
ContrastSet {
    contrast_set_id
    members[]
    discriminators[]
}
```

Discriminator:

```text
Discriminator {
    facet
    importance: HARD | SOFT
    comparison_mode
    missing_behavior
}
```

既知ContrastSetだけでなくruntime candidate groupから差分Facetを抽出してContrast可能にする。

QualifiedResource:

```text
QualifiedResource {
    resource_ref
    usage_profile_ref

    applicability: APPLICABLE

    matched_conditions[]
    resolved_discriminators[]
    remaining_nonblocking_unknowns[]

    contrast_resolution

    evidence_refs[]
    qualification_trace
}
```

RejectedCandidateもreason付きでtrace可能にする。

---

# 16. Assertion and Authority model

Source trustとAuthorityを同一視しない。

```text
Assertion {
    assertion_id

    subject_ref
    predicate
    value

    source_ref

    origin
    authority_scope

    evidence_refs[]

    observed_at
    effective_from?
    effective_to?

    derived_by?
}
```

Origin:

- AUTHORITATIVE
- DECLARED
- CURATED
- DERIVED
- EXTRACTED
- OBSERVED
- INFERRED

AuthorityはResource全体の単一scoreではなくpredicate / domain scope単位で解決する。

```text
Assertions
   ↓
Authority Resolution
   ↓
Resolved Discovery Projection
```

INFERRED assertionはCURATED / AUTHORITATIVE assertionを上書きしない。同Authority scopeの矛盾は多数決等で不可逆に潰さずAuthorityConflictとして残す。

---

# 17. Logical Identity

Identity state:

- RESOLVED
- PROVISIONAL
- UNRESOLVED
- CONFLICT

Identity resolutionの強いEvidence例:

- stable provider ID
- source-native ID
- canonical URI
- integrity digest
- explicit same-resource declaration
- schema/content identity

Similarity / LLM推論のみの場合はPROVISIONALとする。

Relation / equivalence kind候補:

- EXACT_EQUIVALENT
- SAME_LOGICAL_RESOURCE
- ALTERNATIVE_IMPLEMENTATION
- SAME_CONCEPT
- MIRRORS
- DOCUMENTS
- DERIVED_FROM
- SUPERSEDES
- NOT_EQUIVALENT

必要に応じてscopeを持つ。

Capability同一性はschema equalityだけで決めない。

```text
CapabilityContract {
    intent
    inputs
    outputs
    effects
    guarantees
    preconditions
    postconditions
    failure_semantics
}
```

---

# 18. Observation model

Remote Sourceの観測とResource lifecycleを分離する。

```text
ResourceObservation {
    resource_ref

    observed_at
    observation_method

    presence
    reachability
    source_coverage

    remote_version?
    etag?
    digest?
}
```

Presence:

- PRESENT
- ABSENT
- UNKNOWN

Reachability:

- REACHABLE
- UNREACHABLE
- UNKNOWN

Coverage:

- COMPLETE_ENUMERATION
- PARTIAL_ENUMERATION
- QUERY_RESULT
- DIRECT_LOOKUP

Remote search missをabsence / deletionとしない。

Freshness:

- FRESH
- STALE
- UNKNOWN

STALEはcurrent-state guaranteeが失効した意味であり、古い内容であること自体を意味しない。

```text
EffectiveResourceState =
    SourceState
  + RepresentationState
  + VersionState
  + ObservationState
  + EvaluationContext
```

Source outageを全Resourceの書換えで表現しない。

Same version identifier + different digestはIntegrityConflictとする。

---

# 19. Temporal model

Source-native日時を1 timestampへ潰さない。

```text
TemporalDiscoveryProfile {
    freshness_anchor_at
    freshness_basis

    effective_from?
    effective_to?
}
```

`freshness_age` は保存せずQuery時に計算する。

```text
TemporalEvaluationContext {
    evaluation_id
    evaluated_at

    temporal_target
    business_timezone
}
```

`evaluated_at` は現在性評価、`temporal_target` はas-of対象時点に用いる。

---

# 20. Projection architecture

```text
Authoritative Source
        ↓
Observation / Adapter
        ↓
Assertions / Typed Relations
        ↓
Authority Resolution
        ↓
Logical Identity Resolution
        ↓
Discovery Lens
        ↓
Projection Compiler
        ↓
Directory / Structured / Lexical / Vector /
Temporal / HyperGraph / Access
```

Projectionは正本ではない。

Assertion StoreをProjection前の再利用可能中間表現とする。ただしRetention contractに従いSession-only / non-persistentも許可する。

Projection family:

- DirectoryProjection
- StructuredProjection
- LexicalProjection
- VectorProjection
- TemporalProjection
- HyperGraphProjection
- AccessProjection

---

# 21. Directory / Structured / Lexical / Vector

DirectoryProjectionは軽量Resource Cardとし、巨大本文を保持しない。

Facet state:

- KNOWN
- UNKNOWN
- NOT_APPLICABLE
- CONFLICT

Structured Projectionはtyped facetによる高速Filterを担う。

Lexical Projectionはcanonical_name / title / aliases / high_signal / body等をfield分離可能にする。

Vector Projectionは全Resource必須としない。Embeddingを正本・唯一の検索表現にしない。小さいfiltered corpusではexact similarity、大規模ではANNを利用可能とする。ANN missを不存在証拠にしない。

---

# 22. HyperGraph canonical model

Graph Representationはv0 first-class Projectionとする。

Canonical relation semantics:

```text
TypedRelationInstance {
    relation_id
    relation_type

    participants[] {
        role
        resource_ref
    }

    qualifiers{}

    temporal_scope

    authority
    provenance
    evidence_refs[]
}
```

二項Relationも同じTyped N-ary Relation modelで表現する。

Canonical relationをlossy binary edgeへ変換しない。

例:

```text
LoanRelation R1
├─ borrower   → Company A
├─ product    → Product B
├─ collateral → Property C
└─ branch     → Branch D
```

をA→B、A→C、A→Dへ潰してrelation identityを失わない。

Binary shortcutは将来、実測で必要性が確認された場合のみ任意Acceleration Artifactとして追加可能とする。必ず元RelationInstanceへ追跡可能とする。

---

# 23. Graph namespaces

Graph RAGを外部システムとして分離しないが、内部relation namespaceは分ける。

- `discovery:*`
- `semantic:*`
- `evidence:*`

例:

- `discovery:requires`
- `semantic:is_a`
- `evidence:supported_by`

必要なQueryではnamespace横断Traversalを許可する。

Task DAGはRetrieval Graphとは別モデルである。

---

# 24. HyperGraph index and backend boundary

高速探索用のIndex候補:

- resource_id → relation_id[]
- (resource_id, participant_role) → relation_id[]
- relation_type → relation_id[]
- (relation_type, participant_role) → relation_id[]
- (resource_id, relation_type) → relation_id[]
- concept_id
- temporal bucket
- authority scope

Graph Domain Contract:

- GraphProjection
- GraphTraversalPlan
- HyperGraphRetriever

物理Backend選定はDeferredとする。

候補例:

- PostgreSQL
- Rust adjacency/index
- dedicated graph database

特定製品をDomainへ露出させない。

---

# 25. Relation acquisition policy

Relation生成の優先順位:

### Tier A — authoritative / explicit

- Tool / Skill / Workflow宣言
- Document supersedes relation
- Dataset / Field relation
- Provider schema relation

### Tier B — deterministic structural extraction

- hyperlink
- DB foreign key
- structured document reference
- spreadsheet structural relation

### Tier C — semantic inference

LLM等による抽出。

Tier Cは必要なResource / Relation typeに限定し、origin=INFERREDとして扱う。Hard applicabilityをINFERREDだけで満たさない。

Graph自体をProgressiveに構築可能とする。

```text
Persistent Base Graph
= explicit + authoritative + deterministic

Information Gap
 ↓
Targeted Probe / Extraction
 ↓
Session Graph
```

---

# 26. Session Working Index

Remote / temporary data用にSession Working Indexをfirst-classとする。

```text
Session Working Index
├─ Directory
├─ Structured
├─ Lexical?
├─ Vector?
└─ HyperGraph
```

Retention contractに応じ必要なProjectionだけ生成する。

NO_RETENTION Source由来relationはPersistent Graphへ移さずSession Graphで扱える。

---

# 27. Projection generation

Index更新はGeneration単位で扱う。

```text
Generation N
 ↓
Generation N+1 build
 ↓
Validate
 ↓
Atomic publish
```

```text
ProjectionGenerationManifest {
    generation_id

    projection_schema_version
    lens_version
    semantic_registry_version

    analyzer_version?
    embedding_model_version?
    graph_schema_version?

    source_snapshot

    resource_count
    relation_count?

    coverage
    digest

    built_at
}
```

Discovery Evaluationは利用するGenerationをpin可能とする。

Incremental rebuildとFull rebuildは同じSource snapshot / version群から論理的に同じ結果を生成することを要求する。

変更依存に応じて対象Projectionだけ再生成可能にする。

---

# 28. Live / Historical discovery

```text
Live Discovery Tier
Historical Discovery Tier
```

通常QueryはLiveを基本とし、as-of query等でHistoricalを追加する。履歴を削除しない。

---

# 29. Retrieval execution model

Discoveryは1回の検索ではなくEvidence-driven loopとする。

```text
Need
 ↓
IntentSignature
 ↓
Source Routing
 ↓
Retriever Planning
 ↓
Candidate Generation
 ↓
HyperGraph Expansion / Probe
 ↓
Qualification
 ↓
Evidence Update
 ↓
Evidence Sufficiency
 ├─ SUFFICIENT → STOP
 └─ 不足 → InformationGap
                ↓
          次の探索Action
```

---

# 30. Discovery Need / NeedGraph

```text
DiscoveryNeed {
    need_id

    intent_signature

    required_resource_types[]
    required_claims[]

    authority_requirements[]
    freshness_requirements[]

    constraints[]

    completion_requirement
}
```

複雑なNeedはNeedGraphへ分解可能とする。

NeedGraphは「何が必要か」であり、Task DAG「どう達成するか」と分離する。

---

# 31. Source routing

```text
SourceRoutePlan {
    routes[] {
        source_ref
        role
        discovery_mode

        required_facts[]
        authority_requirement?
        freshness_requirement?

        initial_materialization_policy
    }

    expansion_policy
}
```

Source role:

- REQUIRED
- PREFERRED
- EXPANSION

内部→外部という固定順にしない。NeedとAuthority / Freshness / Coverageに応じる。

---

# 32. Retriever profiles

Initial strategy例:

### Identity

- Exact
- Structured
- Concept

### Capability

- Structured
- Concept
- HyperGraph
- Lexical
- Vector fallback

### Knowledge

- Structured
- Lexical
- Temporal
- HyperGraph
- Vector

### Evidence Investigation

- Entity / Claim seed
- HyperGraph
- KnowledgeUnit
- Probe
- Lexical / Vector fallback

### Exploratory

- Lexical
- Vector
- HyperGraph
- Remote / Web

Profileは探索を閉じるHard routeではなく初期戦略とする。

---

# 33. Adaptive Cascaded Retrieval

全Retrieverを常時実行しない。

高コスト処理へ進む候補数を構造的に減らす。

```text
Exact / Structured
        ↓
Lexical / Concept
        ↓
HyperGraph Expansion
        ↓
Vector
        ↓
Probe / Fragment
        ↓
Full Materialization
        ↓
Remote / Web expansion
```

これは固定一本道ではない。NeedによってGraph-first / Remote-first等を許可する。

最適化原則:

> Performance optimization must reduce the amount of work performed, not reduce the semantic information required for correctness.

---

# 34. Graph traversal

Graphは固定TierではなくSeedからの横断Expansion Operatorとする。

```text
Seed Generation
├─ Exact
├─ Structured
├─ Lexical
├─ Vector
└─ Direct ResourceRef
        ↓
Typed HyperEdge Expansion
```

```text
GraphTraversalPlan {
    seed_nodes[]

    path_patterns[]

    allowed_relation_types[]
    allowed_namespaces[]

    participant_role_constraints[]

    authority_requirement?
    temporal_context
    access_context

    expansion_budget
    stop_conditions[]
}
```

無制限自由Traversalを許可しない。

Graph爆発はhop数だけでなくbranchingを制御する。High-degree nodeは追加relation / role / facet制約なしで自由展開しない。

---

# 35. Federated Candidate

```text
FederatedCandidate {
    candidate_id

    resource_ref?
    logical_resource_ref?

    source_ref

    retrieval_method
    locator?

    matched_signals[]

    materialization_state

    provenance

    retrieval_trace_ref
}
```

Candidate identity class:

- DURABLE_RESOURCE
- REMOTE_STABLE_REFERENCE
- EPHEMERAL_CANDIDATE

異なるSource / Retrieverのraw scoreを直接比較しない。

高価処理前にLogicalResource groupingを行う。

---

# 36. Progressive materialization

Materialization state:

- REFERENCE_ONLY
- METADATA
- PROBED
- FRAGMENT
- FULL_CONTENT

Materialization policy:

- INLINE_FULL
- DISCRIMINATIVE_FIRST
- TARGETED_FRAGMENT
- REFERENCE_ONLY

Resource size、latency、provider cost、rate limit、retention、unresolved discriminatorを考慮する。

小Resourceは中間段階を飛ばしてFULL_CONTENTへ進める。

---

# 37. Probe

```text
ProbeCapability {
    probe_type
    supported_resource_types[]
    query_mode
    return_types[]
    completeness_semantics
    cost_profile
}
```

Probe execution location:

- LOCAL
- PROVIDER
- NONE

Public Webは取得可能・許可される場合LOCAL Probe可能。Third-party管理corpusはProviderが公開した検索 / MCP / API境界内だけを利用する。

Probe missはNegative FactではなくNOT_FOUND_BY_PROBEとして扱い、Completeness semanticsと組み合わせる。

---

# 38. Lexical / Vector / Fusion / Reranking

Lexicalは安価な主Candidate Generatorとして利用可能。Field-aware retrievalを許可する。

Top-Kは固定せず段階的window expansion可能とする。

Retriever cursor/stateを保持し、拡張時に全検索をやり直さない。

Vectorは必要Queryのみ。filtered corpusが小さければExact vector、大規模ならANNを選択可能。ANN depthはEvidence不足に応じて拡張可能。

異種Retrieverのraw scoreを直接加算しない。初期Fusion候補はrank-based方式とするが、Strategyは交換可能。

Hard eligibilityはFusion / Rankingより優先する。

高価なCross Encoder / LLM rerankingは小さい最終候補集合に限定する。

Late-interaction等の大規模Index方式はbenchmarkで必要性が実証されるまで必須にしない。

---

# 39. Evidence model

```text
EvidenceRequirement {
    required_claims[]

    authority_requirements[]
    freshness_requirements[]
    corroboration_requirements[]

    contradiction_policy
    completion_policy
}
```

```text
Claim {
    claim_id
    subject
    predicate
    value
    temporal_scope
    evidence_refs[]
    state
}
```

Claim requirement:

- REQUIRED
- PREFERRED
- OPTIONAL

Evidence role:

- PRIMARY
- CORROBORATING
- CONTRADICTING
- CONTEXTUAL
- DERIVED

同一upstream sourceの転載・派生を独立Evidenceとして数えない。

追跡候補:

- publisher
- upstream_origin
- citation_chain
- content_digest

LLM summaryをPRIMARY Evidenceとして扱わない。

---

# 40. Evidence sufficiency and gaps

Sufficiency state:

- SUFFICIENT
- INSUFFICIENT
- UNRESOLVED
- CONFLICTED
- INVALID

Gap type:

- FactGap
- AuthorityGap
- FreshnessGap
- CorroborationGap
- ConflictGap
- AvailabilityGap

Gap typeによって次のSearch Actionを変える。

Conflict時に無条件Broad Searchをせず、primary authority / source lineage / direct evidenceを優先する。

次Actionの基本優先順位:

1. Blocking requirement
2. Authority / Freshness necessity
3. Expected gap resolution
4. Critical Need impact
5. Cost

単一weighted scoreを必須にしない。

Evidenceが十分なら残り探索をcancel / not-start可能にする。

適切なSource / Probeを試しても新しいFactが得られない場合、UNRESOLVEDとして正常終了可能とする。

---

# 41. Discovery result

```text
DiscoveryResult {
    discovery_evaluation_id

    need

    qualified_resources[]

    evidence_set

    unresolved_gaps[]
    rejected_candidates[]

    source_trace
    retrieval_trace
    qualification_trace
}
```

Resource一覧だけではなく、Evidence、Gap、選択・除外理由を返す。

---

# 42. Session binding

原則:

```text
Discovery = dynamic
Binding = session-stable
```

Planning用:

```text
LogicalResourceBinding {
    binding_id
    logical_resource_ref
    usage_profile_ref
    qualification_evidence_ref
    bound_at
}
```

Execution用:

```text
RepresentationBinding {
    binding_id

    logical_resource_ref
    representation_ref
    resource_version_ref?

    source_ref
    provider_ref?

    schema_digest?
    content_digest?

    binding_mode
    bound_at
}
```

Binding stability:

- SNAPSHOT_PINNED
- REMOTE_VERSION_PINNED
- SESSION_SNAPSHOT
- LIVE_REFERENCE

Binding済みResourceを新しいDiscovery結果で暗黙置換しない。Rebindは明示的な再Discovery / 再判断として扱う。

Current authorization、availability、policy、temporal applicabilityは実行時に再評価する。

---

# 43. Agent boundary

Agentは全Resource catalogを初期Contextへ持たない。

基本構造:

```text
Agent Runtime Contract
+
Discovery Capability
        ↓
Need
        ↓
Qualified Resources
        ↓
Binding
        ↓
Task-specific Context
```

Task DAGはSearch Platform外。

NeedGraphとTask DAGを分ける。

- NeedGraph = 何が必要か
- Task DAG = どう達成するか

Task Plannerは具体Tool IDではなくCapabilityRequirementを利用可能とする。

Workflowは再利用template、Task DAGはSession instance。

Skillはreasoning / execution guidanceでありToolやWorkflowと同一視しない。

---

# 44. Context Compiler boundary

```text
TaskContextManifest {
    task_id
    task_graph_revision

    skills[]
    workflow_fragments[]

    knowledge_fragments[]
    evidence_refs[]

    semantic_resources[]

    capability_contracts[]
    concrete_tool_schemas[]

    policy_guidance[]

    facts[]

    context_budget
}
```

SessionにBindingされた全ResourceをLLMへ渡さない。Taskごとに必要Contextのみcompileする。

Context Segment:

```text
ContextSegment {
    segment_id
    segment_type

    resource_ref?
    evidence_ref?

    purpose

    trust_class
    provenance

    content_digest
    content
}
```

Remote contentはUNTRUSTED_CONTENTとして扱い、system instructionへ昇格しない。

Planning ContextとExecution Contextを分ける。

Tool Schemaは必要Taskにだけ公開する。

Stable segment順序 / digest再利用を可能にし、特定Providerのprefix cache等はoptional optimizationとして利用可能にする。

---

# 45. Tool result normalization

Tool execution自体はSearch Platform外だが、Agent integrationではraw resultを以下へ正規化可能とする。

- Fact
- Evidence
- InformationGap
- ResourceRef

Tool call成功とTask成功を同一視しない。Task completionはEvidence / Completion Contractで評価する。

Execution中にInformationGapが発生した場合、再Discoveryは正常経路。

---

# 46. Execution Control Plane boundary

Search Platformが提供可能:

- QualifiedResource
- Evidence
- InformationGap
- Safety metadata
- Binding candidate

Search Platform外:

- final authorization
- execution
- retry
- idempotency ledger
- sandbox
- scheduler
- compensation
- human gate

Execution直前にbinding validity / authorization / provider availability / policy / temporal applicability / schema consistency等を再validationする。

Capability Discovery Projectionには以下のSafety metadataを持てる。

- mutation_scope
- reversibility
- idempotency
- atomicity
- external_effect
- data_sensitivity
- required_permission

単一risk scoreへ潰さない。

---

# 47. Human oversight boundary

Humanへの問い合わせ判断はTop-level Orchestrator側。

Search PlatformはInformationGapを返す。

Subagent / Taskが直接Humanへ質問する前提にしない。

OrchestratorのSelf-resolution順序:

1. Session facts / bindings
2. Session Working Set
3. Internal Discovery
4. Capability / Tool
5. Contracted remote source
6. Web / external source
7. Policy / Workflow / Skill safe default
8. Reversible exploration / verification
9. Human

Human Oversight reason:

- IRREDUCIBLE_AMBIGUITY
- MATERIAL_GOAL_CHANGE
- MATERIAL_TARGET_CHANGE
- NORMATIVE_HUMAN_CHOICE
- POLICY_MANDATED
- UNRESOLVED_HIGH_IMPACT_CONFLICT
- EXPLICIT_APPROVAL_REQUIRED

uncertaintyだけをHuman Gate理由にしない。

Human instructionからOrchestratorがIntentContract / Assumption Ledgerを導出できる設計を想定するが、それらの所有者はSearch Platformではない。

---

# 48. Cost / performance architecture

各処理のCostを単一scalarへ潰さない。

```text
CostProfile {
    expected_latency
    cpu
    peak_memory
    io_read
    io_write
    network_bytes

    llm_input_tokens
    llm_output_tokens

    remote_calls
    monetary_cost
}
```

主要最適化原則:

1. Cheap deterministic filterを高価処理より前へ。
2. UNKNOWNだけ追加Probe / Materializationへ昇格。
3. Typed PredicateをLLMではなく決定的評価。
4. Predicate compile / memoizationを可能にする。
5. Concept synonymをResourceへ重複展開しない。
6. Source共通属性を全Resourceへ複製しない。
7. Rule / Evidenceは参照中心。
8. Logical identity resolution / Contrastで全ペアO(N²)を避けblockingする。
9. Vectorを全Resource必須にしない。
10. Embedding / extractionはcontent-addressed cache可能にする。
11. Generationはimmutable segment再利用可能にする。
12. Incremental Projection dependencyを追跡する。
13. Live / Historicalを分離可能にする。
14. Remote fan-outをNeed / Gapに応じて拡張。
15. Remote query request coalescingを可能にする。
16. ProbeとFull materializationのbreak-evenを実測で調整可能にする。
17. Evidence Sufficiencyを探索Loop内部で再評価。
18. Conflictはtargeted search。
19. Evidence origin dedupを早期に行う。
20. Context CompilerでLLM tokenを削減。
21. Task DAG側では同一pure read等の重複抑制を可能にするがside-effect dedupはExecution契約へ委ねる。
22. Speculative workはPURE / READ_ONLY / LOW_COST等へ限定し、v0必須にしない。

Correctness / Retention / Evidence RequirementをCost削減のため弱めてはならない。

---

# 49. Observability for optimization

最低限、以下を計測可能にする。

- source_route_latency
- sources_fanned_out
- directory_candidates
- structured_filtered
- lexical_candidates
- vector_candidates
- graph_seed_count
- graph_nodes_expanded
- graph_relations_expanded
- probe_count
- probe_bytes
- full_materialization_count
- materialized_bytes
- remote_calls
- remote_latency
- llm_calls
- llm_input_tokens
- llm_output_tokens
- context_tokens
- discovery_completion_state

本文や巨大Tool SchemaをTelemetryへ複製しない。ID / count / timing / reason code / digest中心とする。

---

# 50. Evaluation architecture

Pipeline stage別に評価する。

- E0 Contract / Data Integrity
- E1 Source Routing
- E2 Candidate Retrieval
- E3 HyperGraph Retrieval
- E4 Applicability / Contrast
- E5 Evidence
- E6 Discovery Completion
- E7 Session Binding / Context Compiler
- E8 End-to-End integration

最終LLM回答accuracyだけで評価しない。

---

# 51. E0 — Contract / data integrity

最低限:

- Resource ID / Version / Digest consistency
- Assertion provenance completeness
- TypedRelation participant role validity
- Graph → Evidence reverse traceability
- Projection generation reproducibility
- incremental / full rebuild logical equivalence
- Retention contract compliance
- Access visibility isolation

---

# 52. E1 — Source routing

Metrics候補:

- Required Source Recall
- Required Source Miss Rate
- Unnecessary Source Fan-out Rate
- Remote Escalation Rate
- Source Routing Latency

Required Source missを後段retrieval失敗と区別する。

---

# 53. E2 — Candidate retrieval

一般指標:

- Recall@K
- Precision@K
- MRR
- nDCG

追加:

- LogicalResource Recall
- Representation Recall

Logical Resourceを候補集合へ入れられたかをRepresentation数より重視する。

---

# 54. E3 — HyperGraph retrieval

Metrics候補:

- Relation Recall / Precision
- Participant Role Accuracy
- Valid Path Rate
- False Path Rate
- Required Path Recall
- Path-to-Evidence Trace Accuracy
- Nodes Expanded
- Relations Expanded
- Branching Factor
- High-degree Expansion Rate
- Traversal Latency

特に `False Composite Relation Rate` を重大指標とする。

n-ary relationを誤ったparticipant組合せへ平坦化したFalse Pathを重大Graph Errorとして扱う。

---

# 55. E4 — Applicability / contrast

Metrics候補:

- Applicability Precision / Recall
- Exclusion Precision / Recall
- Contrast Resolution Accuracy
- Hard Discriminator False Accept
- Hard Discriminator False Reject
- Unknown Detection Accuracy
- Unknown Resolution Rate
- Information Gap Accuracy
- False Exclusion from Missing Fact

Hard False Acceptを重大エラーとして扱う。

---

# 56. E5 — Evidence

Metrics候補:

- Claim Coverage
- Evidence Attribution Accuracy
- Authority Requirement Satisfaction
- Freshness Requirement Satisfaction
- Contradiction Recall
- False Corroboration Rate
- Sufficiency Precision / Recall

Evidence不足をSUFFICIENTと判断するFalse Sufficientを重大エラーとして扱う。

---

# 57. E6 — Discovery completion

Metrics候補:

- Need Completion Rate
- Required Claim Completion Rate
- Unresolved Gap Accuracy
- No-progress Detection Accuracy

Failure attributionを必須にする。

例:

```text
Failure
├─ SOURCE_KNOWLEDGE_ABSENT
├─ SOURCE_ROUTING_MISS
├─ RETRIEVAL_MISS
├─ GRAPH_PATH_MISS
├─ APPLICABILITY_ERROR
├─ EVIDENCE_LOCATOR_ERROR
└─ ...
```

`SOURCE_KNOWLEDGE_ABSENT` を検索アルゴリズム失敗と区別する。

これは将来CRM / SFA等によるKnowledge Coverage改善へ接続する。

---

# 58. E7 — Binding / context

Metrics候補:

- Correct Logical Binding Rate
- Correct Representation Binding Rate
- Stale Binding Detection Rate
- Silent Rebinding = 0
- Required Context Recall
- Irrelevant Context Rate
- Tool Schema Exposure Count
- Context Tokens
- Context Compilation Latency

Context token削減だけを成功とせず、Task success / required fact recall / tool selectionと同時評価する。

---

# 59. E8 — End-to-End

End-to-End successは最低限以下を満たす。

```text
Goal achieved
AND Required evidence satisfied
AND No hard applicability violation
AND No access violation
AND No unresolved blocking gap hidden
```

複数の正しいRetrieval / Planning経路を許容し、exact trace一致を要求しない。

---

# 60. Evaluation Scenario

```text
EvaluationScenario {
    user_instruction
    initial_session_state

    available_sources[]
    hidden_source_truth

    resources[]
    relations[]

    required_claims[]
    required_authority[]
    required_freshness[]

    allowed_resources[]
    invalid_resources[]

    expected_gaps[]

    acceptable_plans[]
    forbidden_actions[]

    fault_injections[]

    completion_contract
}
```

Scenario分類:

- Confusable Resource
- Temporal
- HyperGraph
- Remote / rights
- Evidence conflict
- Security / access
- Prompt injection
- Source coverage absent
- Fault injection

Fault候補:

- Provider timeout
- Schema change
- Index stale
- Graph projection stale
- Resource disappearance
- Authorization revoked
- Source unavailable
- Digest mismatch

---

# 61. Evaluation priority

評価優先順位の基本:

1. Correctness / Safety
2. Coverage
3. Evidence quality
4. Latency
5. Resource Cost

PoC Evidenceなしに具体的SLO / Recall thresholdを先にFreezeしない。

Production Gate前に少なくとも以下を評価する。

- Contract tests
- Retriever benchmark
- HyperGraph benchmark
- Applicability benchmark
- Evidence benchmark
- Security fixtures
- Fault injection
- Performance baseline

---

# 62. Security and access invariants

1. Access filteringをCandidate visibilityより前に適用可能にする。
2. Search IndexのAccess Projectionだけで最終Execution authorizationを確定しない。
3. Remote contentをinstructionとして信頼しない。
4. Secret / credentialをLLM Contextへ入れない。
5. NO_RETENTION等のSource契約をCache / Graph都合で破らない。
6. Unauthorized Resourceの存在自体を漏らさない構成を可能にする。
7. Audit / telemetryへ本文やqueryを無条件複製しない。

---

# 63. API conceptual boundary

Human / LLM / AgentでCore business logicを複製しない。

最低限の概念API:

```text
SearchRequest
→ SearchResult[]

DiscoveryRequest
→ DiscoveryResult
```

Graph専用利用者向けAPIをSystem boundaryとして必須にしない。

内部Graph-specific contract:

- GraphProjection
- GraphTraversalPlan
- RelationPathPattern
- GraphCandidate
- GraphPathEvidence
- HyperGraphRetriever

Graph-specific resultはFederatedCandidate / Evidenceへ正規化して共通Discovery Pipelineへ戻す。

---

# 64. Deferred physical decisions

本設計で意図的に固定しないもの:

- Graph storage/backend製品
- Vector engine
- Exact ANN implementation
- Embedding model
- Reranker model
- Fusion implementation library
- Predicate execution backend
- Search API transport
- physical service split
- exact sharding topology
- exact cache TTL
- exact SLO / Recall target
- Community detection algorithm
- Late interaction index
- LLM model / provider

これらはPoC / benchmark / Production Planで選定する。

---

# 65. Explicit non-goals for v0 design

本CapabilityのDesign Freezeは以下を直接実装対象にしない。

- CRM / SFAそのものの構築
- Tool Execution Control Planeの本番実装
- Task DAG scheduler実装
- Human UI
- Human approval workflow
- credential broker
- global knowledge ingestion of the public Web
- third-party provider corpusの無断mirror
- arbitrary script-based applicability
- Graph community summaryを正本とする方式
- full autonomous Agent product

ただし後続Capabilityが利用できるContractを壊さない。

---

# 66. Required normative reconciliation after written approval

本書承認後、Production Implementation Plan作成時に少なくとも以下の既存規範を差分確認し、必要箇所を明示改訂する。

1. `spec/architecture/architecture-contract-v0.md`
   - Search Platform = 文書retrievalだけではなくDiscovery capabilityを持つこと
   - Graph Representationをv0 first-classとすること
   - RAGを独立systemにしない原則は維持
2. `spec/architecture/system-architecture-v0.md` / D2
   - Source Registry / Federated Discovery
   - HyperGraph Projection
3. `spec/data/logical-data-model-v0.md` / D2
   - CanonicalKnowledgeResource中心モデルからtyped DiscoverableResource / Assertion / Relation semanticsへの拡張
   - `Graph Representation（将来）` 表現の改訂
4. `spec/data/data-characteristics-v0.md`
   - Graph Representationのfuture扱い改訂
   - Session-only / remote retention特性
5. `spec/data/transaction-consistency-requirements-v0.md`
   - Search projection generation / outbox / eventual consistency境界の必要追記
6. `spec/selection/library-tool-selection-v0.md`
   - `Graph retrieval — DEFERRED` を
     - Graph retrieval contract: REQUIRED
     - Graph backend selection: DEFERRED
     に分離
7. Observability / Audit requirements
   - Discovery evaluation / candidate stage / graph traversal / evidence trace

承認前にこれら既存規範を暗黙変更しない。

---

# 67. Design invariants summary

1. Search PlatformはKnowledge Platform内部の論理サブシステム。
2. DiscoveryはSearch Platformの上位Capability。
3. Source / Resourceを分離する。
4. 巨大Global Resource Indexを必須にしない。
5. Resourceはtypedで、すべてをDocument / Textへ潰さない。
6. SimilarityはCandidate Generationに限定する。
7. Applicability / Contrast / Authority / TemporalをRankingより優先する。
8. MissingをFalse扱いしない。
9. Assertion / provenance / authority conflictを不可逆に潰さない。
10. LogicalResource / Representation / Versionを分離する。
11. Remote observationとResource lifecycleを分離する。
12. Projectionは全て派生データ。
13. HyperGraph Representationをv0 first-classにする。
14. Canonical relationはTyped N-ary Relation / HyperEdge。
15. Lossy binary graphをCanonicalにしない。
16. Graph RAGを外部独立Systemにしないが内部境界は分離する。
17. Graph traversalはtyped / bounded。
18. RetentionとMaterializationを分離する。
19. UNKNOWNだけ追加探索へ昇格できる。
20. Evidence SufficiencyをDiscovery停止条件にする。
21. Discoveryはdynamic、Bindingはsession-stable。
22. Agent ContextはTask単位でcompileする。
23. Search PlatformとExecution Control Planeを分離する。
24. Human escalation判断は上位Orchestrator。
25. CorrectnessをPerformance Costより優先する。
26. Source Knowledge AbsenceをSearch failureと区別する。
27. Pipeline stage別にEvaluation / Failure Attribution可能にする。
28. 物理Backend選定をDomain Contractへ固定しない。

---

# 68. Written design review gate

本書の承認は以下を意味する。

- D1〜D9の会話上の合意を、本書の書面ContractとしてFreezeしてよい。
- 次にImplementation Planを作成してよい。
- 承認後に既存normative `spec/` との差分をImplementation Plan上で明示的に反映してよい。

本書承認だけでは以下を意味しない。

- Production code実装承認
- dependency採用承認
- Graph / Vector backend製品の採用承認
- merge承認
- deploy承認

次工程は、本書レビュー・明示承認後にProduction Implementation Planを作成することである。
