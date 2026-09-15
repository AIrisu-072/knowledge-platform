# Development Assurance Architecture v0

- 状態: v0
- 対象: Knowledge Platform 開発プロセス全体
- 位置づけ: Agent-first Development / Architecture / Verification / Context Control Plane
- 主目的:
  - LLM / Coding Agentがrepository全体・全仕様・全testを毎回読まずに開発できること
  - LLMの発想・探索の自由度を維持したまま、実装段階でArchitecture / Security / API / UX / Build等の契約へ収束させること
  - 仕様・実装・検証Evidenceの整合を宣言的に管理すること
  - 人間も同じControl Planeを観測・利用できること
- 基本原則:
  - Agentは自由に考える
  - Hard Ruleは可能な限りAgentの外側で強制する
  - Agentへ常時ルール全文を注入しない
  - 違反・Gap・Counterexampleだけを必要時に返す
  - LLMはnormal verification pathのOracleにしない
  - deterministic CIを最終correctness gateとする
  - generated stateはSSOTにしない
  - v0では独自DSLを作らない

---

## 1. Core idea

本Architectureは、LLMに大量の規則を記憶・参照させる設計ではない。

```text
Agent
  ↓
自由な探索・設計・実装案
  ↓
Development Assurance
  ↓
Violation / Gap / Counterexample
  ↓
Agentによる修正
```

目的は、**LLMの創造性を制約することではなく、発想後の実装を機械的な契約へ収束させること**である。

Architecture / Security / API / UX / Testing / Build等の規則の大部分は、prompt contextではなく外部Environment / Provider / CI Gateへ配置する。

---

## 2. Agent cognitive-load principle

AssuranceはAgentのContext Windowを支配してはならない。

1. Rules SHOULD be enforced externally where possible.
2. Full normative specifications MUST NOT be injected by default.
3. Exploration receives goals + hard business constraints, not full implementation detail.
4. Current implementation is observed state, not automatically immutable design.
5. Hard constraints and advisory preferences are distinct.
6. Assurance feedback is violation/gap-oriented, not full-rule repetition.
7. Spec / Architecture / Code Graph context is progressively expanded.
8. Context Compiler enforces a bounded Assurance Context Budget.
9. Exploration and implementation use different context profiles.
10. Context capacity must remain available for novel reasoning.

---

## 3. Reasoning phases

### 3.1 Exploration

与えるもの:

```text
Problem
Goal
Hard business constraints
Relevant domain concepts
```

原則として与えないもの:

- repository全文
- Architecture rule全文
- test suite全文
- current implementation detailの大量注入
- soft preferenceをhard constraintとして扱う情報

### 3.2 Grounding

```text
Candidate Idea
   ↓
Desired Architecture
Observed Architecture
Relevant Requirements
Known Evidence / Gaps
   ↓
Compatible / Violation / New Capability Required
```

### 3.3 Implementation

採用案が決まった後に必要なContextだけを狭く深く渡す。

```text
Relevant symbols
Relevant API/schema
Relevant requirements
Relevant architecture boundaries
Relevant verification
Known gaps
```

原則:

```text
探索時 = 広く薄く
実装時 = 狭く深く
```

---

## 4. Hard constraints vs advisories

### 4.1 Hard constraint

違反した成果物はmerge不可。

例:

- credential / secretをcommitしない
- PUBLISHED DocumentVersionを直接変更しない
- DomainからInfrastructure実装へ直接依存しない
- required API error contractに違反しない
- prohibited licenseを導入しない

### 4.2 Advisory

Architecture上の推奨。理由があれば変更可能。

すべてをHard Errorにせず、既存Architectureを絶対視してAgentの探索空間を不必要に狭めない。

---

## 5. Logical graph model

```text
┌──────────────────────────────┐
│ Spec / Normative Graph       │
│ Requirement / Policy / API   │
└──────────────┬───────────────┘
               │ expected_by
               ▼
┌──────────────────────────────┐
│ Architecture Graph           │
│ desired boundaries/relations │
└──────────────┬───────────────┘
               │ implemented_by
               ▼
┌──────────────────────────────┐
│ Code Graph                   │
│ repository/module/symbol     │
│ import/call/implements       │
└──────────────┬───────────────┘
               │ verified_by
               ▼
┌──────────────────────────────┐
│ Assurance Graph              │
│ capability/evidence/gap      │
└──────────────────────────────┘
```

論理的には1つのDevelopment Knowledge Graphとして扱えるが、v0で巨大Graph DBを導入しない。

---

## 6. Spec / Normative Graph

対象:

- Requirement
- Error / Resilience Contract
- Observability / Audit Contract
- Frontend / UX Requirements
- Development / CI Contract
- API Contract
- JSON Schema
- License / Security policy

Requirementは既存Markdownを正本とする。Machine-readable metadataだけを同じsectionへ付与可能にする。

意味ベースIDを原則とする。

```text
REQ-DOC-IMMUTABLE-PUBLISHED
REQ-SEC-NO-SECRETS
REQ-UI-MOTION-NONBLOCKING
```

---

## 7. Desired Architecture Graph

仕様から生成する「あるべきArchitecture」。

```text
Domain
  ↓ allowed
Application
  ↓ allowed
Infrastructure

Domain → SQLx
= forbidden
```

Architecture Contract / dependency-rules等から生成する。

---

## 8. Observed Architecture Graph

実装から生成する現在のArchitecture。

```text
document-domain
  imports
sqlx
```

DesiredとObservedを比較しArchitecture Driftを検出する。

---

## 9. Code Graph

Code Graphは、**現在のコードが実際にどのようにつながっているか**を表す。

### Level 0 — Repository Graph

```text
repository
package/crate
file
spec
```

### Level 1 — Dependency Graph

```text
crate → crate
TS module → module
API → schema
```

### Level 2 — Symbol Graph

```text
function
type
trait
impl
call
reference
```

### Level 3 — Semantic Graph

```text
reads
writes
publishes
authorizes
emits audit event
changes state
```

v0 implementation targetはLevel 0〜1。
IRはLevel 2〜3を将来追加可能な形にする。

---

## 10. Code Graph purpose

Code GraphはLLMにrepository全体を読ませないためのIndexでもある。

必要なsourceだけsource locationから取得し、Code Graph nodeへsource全文をコピーしない。

---

## 11. Assurance control model

```text
Requirement
    ↓
Control
    ↓
Capability
    ↓
Provider
    ↓
Evidence
    ↓
Gap
```

---

## 12. Requirement

「何が正しくなければならないか」という規範。本文はNormative MarkdownがSSOT。

---

## 13. Control

Requirementを機械判定可能な保証条件へ変換したもの。

Requirementと現在のTool implementationを分離する。
Toolを交換してもRequirementは変わらない。

---

## 14. Control generation

通常ControlはRequirement metadata + global Assurance Policyから自動生成する。

特殊なRequirementのみdeclarative overrideを許可する。

```text
control-policy.toml
```

v0では独自DSLを作らない。

---

## 15. Capability

ControlへEvidenceを提供する検査能力。

Capabilityはtest function単位ではなく意味単位。

例:

```text
CAP-SECRET-GITLEAKS
CAP-ARCH-DEPENDENCY-BOUNDARY
CAP-DOC-LIFECYCLE
CAP-UI-KEYBOARD-E2E
```

---

## 16. Capability metadata

TOML等で最小metadataを宣言する。

```text
id = "CAP-DOC-LIFECYCLE"
controls = ["CTRL-DOC-IMMUTABLE-PUBLISHED"]
provider = "proptest-state-machine"
mechanism = "state-machine"
oracle = "domain-invariant"
cost = "fast"

scope.crates = ["document-domain"]

assumptions = ["single transactional source of truth"]
exclusions = ["database failover"]
```

file path / git revision / provider version等は自動導出する。

---

## 17. Provider

実際の検査toolを共通interfaceへ接続するAdapter。

例:

```text
ArchitectureLint
Gitleaks
cargo-deny
OSV-Scanner
Redocly
JSON Schema Validator
proptest
Kani
cargo-fuzz
cargo-mutants
Playwright
Accessibility checker
OCI inspection
```

Provider自身で既存tool機能を再実装しない。

---

## 18. Provider interface

概念interface:

```text
discover()
plan()
run()
collect()
```

Providerは薄いAdapterとする。

---

## 19. Evidence

Evidenceは完全自動生成。

```text
capability_id
commit
outcome
provider
provider_version
scope
parameters
assumptions
bounds
duration
artifact reference
```

人間はEvidenceを編集しない。

Evidence identity:

```text
commit
+ capability
+ provider version
+ relevant configuration
```

---

## 20. Gap

Desired assuranceとObserved Evidenceとの差。

```text
NO_CAPABILITY
STALE_EVIDENCE
EXCLUSION_HIT
ASSUMPTION_CHANGED
INSUFFICIENT_MECHANISM_DIVERSITY
MUTANT_SURVIVED
FUZZ_COUNTEREXAMPLE
MODEL_COUNTEREXAMPLE
UNBOUND_CHANGE
POLICY_UNSATISFIED
CRITICALITY_INCREASED
CONTEXT_SCOPE_DRIFT
```

Gapは原則自動生成。

---

## 21. Assurance Compiler / IR

入力:

```text
Markdown requirements
control-policy.toml
capability manifests
OpenAPI
JSON Schema
Cargo metadata
GitHub workflow YAML
repository tree
```

を共通IRへcompileする。

```text
Many declarative sources
        ↓
Assurance Compiler
        ↓
Canonical Assurance IR
```

Generated IRはGit管理せず、人間が編集しない。

---

## 22. IR relationships

最低限:

```text
Requirement
  └─ satisfied_by → Control

Control
  └─ evidenced_by → Capability

Capability
  └─ executed_by → Provider

Capability
  └─ applies_to → Component

Component
  └─ depends_on → Component
```

将来Symbol edgeを追加可能にする。

---

## 23. Assurance Planner

```text
git diff
   ↓
Code / Component Graph
   ↓
Affected Requirement / Control
   ↓
Available Capability
   ↓
Execution Plan
```

Plan段階ではまだVerifierを実行しない。

---

## 24. Plan / Run / Report

Developer-facing workflow:

```text
assure plan
assure run
assure report
assure gaps
```

入口はmiseへ統合する。

```text
mise run assure:plan
mise run assure:run
mise run assure:report
mise run verify:fast
mise run verify
mise run verify:full
```

miseは「何を実行するか」の共通入口。
Assuranceは「なぜ必要か / 何を保証するか / 何が不足か」をplanするControl Plane。

---

## 25. Assurance profiles

### fast

```text
architecture
static
schema
fast property
affected unit
```

### standard

```text
fast
security
broader tests
codegen
```

### full

```text
standard
integration
OCI verification
selected E2E
broader verification
```

Mutation / Fuzz / Model Checking等はControl / Gap / scheduleに応じて追加する。

---

## 26. Verification architecture

TestingはAssuranceの一部として扱う。

優先順位:

```text
1. Static / Formal Verification
2. Spec-driven generated test cases
3. Property testing
4. State-machine testing
5. Fuzzing
6. Persistent regression tests
7. LLM-generated test code
```

LLM-generated test codeは最後の手段。

---

## 27. Generated cases over generated test code

大量のtest codeを保守するのではなく、少数のpersistent harnessから大量caseを生成する。

```text
JSON Schema
→ valid/invalid data generation

Property
→ generated inputs

State Model
→ generated transition sequences

Fuzzer
→ generated inputs
```

---

## 28. Persistent tests

恒久的に残す候補:

- Architecture lint
- API / Schema contract
- Domain invariant / property
- Critical state-machine
- Security-critical regression
- Production bugのminimal repro
- Critical workflow E2E

原則残さない:

- 一時探索用generated test code
- successful fuzz inputs全件
- redundant example test
- transient mutation probe

---

## 29. Verification holes

### Spec error
間違ったRequirementを正確に検証してもsystemは間違う。

### Vacuous property
Generatorが意味あるbranchを通らない可能性がある。

### Unrealistic input distribution
Synthetic/Fuzzだけでは実業務分布と異なる。

### Bounded proof misunderstanding
Model checkerのbound / assumptionをEvidenceへ必ず含める。

### Concurrency / temporal behavior
Propertyだけに依存せずintegration / deterministic concurrency / fault injectionを利用。

### Mutation false signal
Survivorはautomatic defectではなくGap signal。

### Affected-test misclassification
Impact selectionはoptimizationであり唯一のcorrectness gateではない。

```text
PR fast path
→ affected assurance

merge/scheduled broader gate
→ wider assurance
```

### Verification tool bugs
Assurance tool自身へgolden fixtures / self-test / version pinを要求する。

---

## 30. Context Compiler

入力:

```text
Task
Git diff
Spec Graph
Desired Architecture Graph
Observed Code Graph
Assurance Graph
```

出力:

```text
Bounded Context Envelope
```

LLMへGraph全体を渡さない。

---

## 31. Progressive context loading

```text
Global Graph
    ↓
Relevant Slice
    ↓
Specific Node / Harness
```

最初はsummary + handleだけ渡し、必要な場合のみdrill-downする。

---

## 32. Assurance Context Budget

Assurance ContextがContext Windowを支配してはならない。

Compilerへbounded selectionを要求する。

制限候補:

```text
max_requirements
max_controls
max_capabilities
max_symbols
max_evidence_items
```

上限超過時は全文を注入せずhandle / countだけ提示する。

---

## 33. LLM dossier

LLMがGap分析を必要とする場合だけCompact Dossierを生成する。

full test suite / full spec / full code graphを含めない。

---

## 34. LLM usage policy

LLMはnormal verification pathには入れない。

呼び出し候補:

```text
NO_CAPABILITY
EXCLUSION_HIT
ASSUMPTION_CHANGED
MUTANT_SURVIVED
FUZZ_COUNTEREXAMPLE
MODEL_COUNTEREXAMPLE
PRODUCTION_ESCAPE
NEW_HIGH_CRITICALITY_REQUIREMENT
```

LLMをfinal Oracleにしない。

---

## 35. Counterexample-driven feedback

```text
Generate
   ↓
Check
   ↓
Violation / Counterexample
   ↓
Refine
```

Agentへ返すのは違反したRule / Gapだけ。

これによりrule context常時注入を削減し、current architectureへのanchorを抑制する。

---

## 36. Agent integration principle

LLMがAssuranceを自発的に利用することを前提としない。

```text
User Task
   ↓
Agent Runtime / Gateway
   ↓
Assurance Plan
   ↓
Context Compiler
   ↓
Minimal Context
   ↓
LLM
```

---

## 37. Agent Gateway

Repository write / tool executionの前後へ外部Gateを置ける設計にする。

```text
LLM
 ↓
Tool request
 ↓
Agent Gateway
 ↓
Allowed?
 ├─ yes → execute
 └─ no  → re-plan / context refresh
```

モデル固有実装はAdapterへ隔離する。

---

## 38. Context Lease

Agent session / taskにActive Context Leaseを持たせる。

```text
base_revision
plan_id
context_generation
allowed_scope
affected_requirements
active_controls
```

scope外編集では`CONTEXT_SCOPE_DRIFT`を発生させる。

---

## 39. Scope drift lifecycle

```text
Agent attempts out-of-scope edit
        ↓
Gateway detects drift
        ↓
re-plan
        ↓
Code Graph / Requirement impact refresh
        ↓
Context Package refresh
        ↓
new Context Lease
        ↓
continue
```

---

## 40. Agent adapters

### MCP Adapter

```text
assurance.plan
assurance.context
assurance.gaps
assurance.verify
codegraph.inspect
```

MCPはInterfaceでありEnforcementではない。

### Hook Adapter

```text
session start
pre read
pre write
post write
pre commit
```

### Wrapper Adapter

Hook/MCPが不十分なAgent向けにGateway越しで起動する。

---

## 41. Enforcement layers

```text
Agent Gateway
   ↓
Local Assurance
   ↓
Git hooks
   ↓
GitHub CI
   ↓
Merge Gate
```

最終correctness gateはdeterministic CI。

---

## 42. Custom Architecture Lint

Custom Architecture LinterはAssurance Providerの1つ。

```text
Assurance Engine
      ↓
Architecture Provider
      ↓
dependency-rules.toml
cargo metadata
repository structure
workflow config
```

初期対象:

- repository structure
- required normative files
- canonical Dockerfile policy
- no self-hosted runner
- no native Windows CI
- mise task contract
- basic crate dependency direction

---

## 43. Developer UX

開発者/Agentは個別toolを覚えなくてよい。

```text
mise run verify:fast
mise run verify
mise run verify:full
```

内部:

```text
assure plan
↓
required Provider selection
↓
parallel run
↓
Evidence normalization
↓
Gap detection
↓
compact report
```

---

## 44. Incubation / future extraction

v0は`knowledge-platform`内でincubateする。

generic kernelへproject固有概念を入れない。

将来以下が成立した時点で`rust-build-standards`等へ抽出を検討する。

```text
1. knowledge-platformで実運用
2. 2種類以上のProviderが動作
3. plan → run → evidence → gap が一周
4. generic / project-specific境界が明確
5. 別Rust projectでの具体的利用要求が存在
```

---

## 45. v0 Kernel

```text
1. Requirement extraction
2. Control generation
3. Capability manifests
4. Provider registry
5. Assurance IR
6. Level 0-1 Code Graph
7. Diff → impact
8. Plan
9. Run
10. Evidence normalization
11. Gap detection
12. Report
13. Bounded Context Compiler
14. LLM dossier generation
```

v0 Non-goal:

```text
custom DSL
graph database
symbol-level full call graph
semantic full-program analysis
remote state
distributed scheduler
plugin marketplace
web management UI
automatic LLM test-code generation
automatic PR merge
```

---

## 46. Acceptance criteria

- [ ] LLMへfull normative specsをdefault注入しない
- [ ] ExplorationとImplementationでContext profileを分ける
- [ ] Hard Constraint / Advisoryを区別する
- [ ] Spec / Architecture / Code / Assurance Graphを論理的に分離する
- [ ] v0 Code GraphはLevel 0-1を生成できる
- [ ] Requirement → Control → Capability → Provider → Evidence → Gapを表現できる
- [ ] Evidence / Gap / IRは再生成可能で人間編集しない
- [ ] Assurance Plannerがdiffから実行Capabilityを導出できる
- [ ] mise経由でplan/run/reportを実行できる
- [ ] Generated test codeよりgenerated test casesを優先する
- [ ] Persistent test suiteを高価値testへ限定できる
- [ ] LLMをnormal verification pathに置かない
- [ ] Gap時だけbounded dossierを生成できる
- [ ] Assurance Context Budgetを持つ
- [ ] Code GraphをProgressive Context Loadingに利用できる
- [ ] Agent RuntimeがAssurance利用を外側から支援/強制できる
- [ ] Scope Driftでre-planできる
- [ ] MCPをInterface、Gateway/Hook/CIをEnforcementとして区別する
- [ ] Final correctness gateはdeterministic CI
- [ ] Custom Architecture LintをProviderとして扱う
- [ ] generic kernelを将来別projectへ抽出可能

---

## 47. Related specifications

- Architecture Contract v0
- Development / Container / CI Architecture v0
- Error Handling & Resilience Requirements v0
- Observability & Audit Requirements v0
- Frontend / UX Requirements v0
- Library / Tool Selection v0
