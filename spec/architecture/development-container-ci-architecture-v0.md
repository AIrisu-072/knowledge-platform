# Development / Container / CI Architecture v0

- 状態: v0
- 対象: Knowledge / Document Platform 全体
- 位置づけ: 開発環境・コンテナ・CI・Releaseの横断Architecture Contract
- 目的:
  - 開発者OSに依存しないsource構成
  - 高速なlocal inner loop
  - GitHub-hosted CIによる再現可能な検証
  - Linux OCI Containerをcanonical deployment artifactとする
  - CI/Container最適化を開発速度要件として最初から設計する
- 正式サポート開発環境:
  - Linux
  - macOS
  - Windows via WSL2 only
- Native Windows development: 非対応
- Production target: `linux/amd64` OCI image
- Production multi-arch: v0では非要件
- CI Runner: GitHub-hosted runners only
- Development task interface: mise

---

## 1. Core principles

本Architectureの最上位原則:

```text
Correctness
Reproducibility
Fast Feedback
Security
Deployment Portability
```

高速化のためにCorrectness / Security / Reproducibilityを外さない。

ContainerをOS portabilityの代替にはしない。

> **Source自体をportableに保ち、Containerは再現性・統合試験・配布境界として利用する。**

---

## 2. Supported development platforms

正式サポート:

```text
Development
├─ Linux
├─ macOS
└─ Windows
   └─ WSL2 only
```

Production:

```text
Linux OCI Container
└─ linux/amd64
```

Native Windows上での以下は正式サポート対象外:

- Rust build
- Frontend build
- mise task execution
- canonical shell/tooling workflow

Windows固有機能はOS-specific Adapter境界へ隔離する。

---

## 3. WSL2 policy

WSL2はArchitecture上Linux development environmentとして扱う。

```text
Linux Development
├─ Native Linux
└─ WSL2
```

Repositoryは原則としてWSL2のLinux filesystem上へ配置する。

推奨:

```text
~/src/knowledge-platform
```

原則避ける:

```text
/mnt/c/...
```

理由:

- filesystem performance
- permission semantics
- file watching
- Rust incremental build
- Frontend HMR

の差異を減らすため。

---

## 4. Native inner loop

日常開発はContainer内へ閉じ込めない。

```text
Linux / macOS / WSL2
        │
        ▼
       mise
        │
 ┌──────┼────────┐
 ▼      ▼        ▼
Rust   Web     project tools
native native     native
```

Canonical inner loop:

- `cargo check`
- `cargo test`
- Frontend HMR / build
- architecture lint
- OpenAPI validation
- codegen
- unit tests

Containerは主として外部依存と再現性境界に利用する。

---

## 5. OS-specific code boundary

以下のCore領域からOS固有APIへ直接依存しない。

```text
Domain
Application
Search Core
Document Core
API Contract
```

OS依存は明示的Adapterへ隔離する。

例:

```text
Identity Port
       ↑
Windows / Negotiate Adapter
```

Architecture lintでOS固有依存の越境を検査可能にする。

---

## 6. mise as development task SSOT

```requirement
id = "REQ-DEV-MISE-TASK-SSOT"
kind = "architecture"
criticality = "medium"
domain = "development"
```

`mise` をtool version / task entrypointのSSOTとする。

```text
Developer
    │
    └─ mise run <task>

GitHub Actions
    │
    └─ mise run <same task>
```

GitHub Actions YAMLへbuild/test logicを重複させない。

CI YAMLの責務:

```text
checkout
↓
mise setup
↓
cache restore
↓
mise task execution
↓
artifact/result publication
```

---

## 7. Developer bootstrap

Canonical workflow:

```text
clone
  ↓
mise install
  ↓
mise run bootstrap
  ↓
development
  ↓
mise run verify:fast
  ↓
commit
  ↓
mise run verify
  ↓
push / PR
```

`bootstrap` は環境を無制限に変更するものではなく、最低限以下を扱う。

- tool prerequisite確認
- version確認
- generated state確認
- local dependency準備
- development prerequisite検査

---

## 8. Toolchain pinning

以下はversionを固定可能にする。

- Rust toolchain
- Node.js
- Frontend package manager
- OpenAPI tooling
- JSON Schema tooling
- Secret scanner
- Architecture lint tooling
- Container build tooling
- Code generators

主な正本候補:

```text
mise.toml
rust-toolchain.toml
Cargo.lock
frontend lockfile
```

Local / CIで同一tool versionを利用する。

---

## 9. Development container policy

Dev Containerはcanonical development pathにしない。

v0 Non-goal:

- VS Code Dev Container必須化
- Docker-in-Docker development
- all-development-inside-container

利用者が任意で追加する余地は残す。

---

## 10. Container responsibilities

Containerの責務を以下に限定する。

```text
Container
├─ Development dependencies
│  └─ PostgreSQL等
├─ Reproducible verification
│  └─ production image build verification
└─ Deployment artifact
   └─ Linux OCI image
```

---

## 11. Canonical production artifact

```requirement
id = "REQ-DEV-PRODUCTION-LINUX-AMD64"
kind = "architecture"
criticality = "medium"
domain = "development"
```

Production artifactは:

```text
linux/amd64 OCI image
```

をcanonical targetとする。

Production multi-archはv0では要求しない。

`linux/arm64` 等は具体的な運用要件が発生した時点で追加する。

---

## 12. Canonical build pipeline

概念:

```text
Frontend build
TypeScript / React
       ↓
static assets

Rust build
       ↓
release binary

       ↓

Linux OCI runtime image
├─ Rust executable
└─ static assets
```

Production Node runtimeは持ち込まない。

---

## 13. Canonical Dockerfile

単一のcanonical `Dockerfile` を基本とする。

避ける:

```text
Dockerfile.dev
Dockerfile.test
Dockerfile.ci
Dockerfile.prod
```

差分が必要ならmulti-stage target / build argumentで表現する。

概念:

```text
base
├─ frontend-deps
│  └─ frontend-build
├─ rust-deps
│  └─ rust-build
├─ test
└─ runtime
```

ProductionとCIで別build implementationを持たない。

---

## 14. Runtime image requirements

Runtime imageへ含める:

- Rust executable
- Frontend static assets
- 必要最低限のruntime libraries
- CA certificates等の実行必須物

含めない:

- Cargo
- rustc
- Node.js
- npm / package manager
- TypeScript compiler
- Git
- build cache
- source tree
- test data

必須要件:

- non-root user
- explicit working directory
- explicit writable paths
- graceful shutdown
- health endpoint
- no secrets baked into image layers

---

## 15. Filesystem contract

将来的にread-only root filesystemへ対応可能な設計を目指す。

書込みが必要なpathは明示する。

例:

```text
/var/lib/knowledge-platform
/tmp
```

Applicationが暗黙にcurrent working directoryへ永続データを書き込む設計を避ける。

---

## 16. Configuration / secret boundary

OCI imageへenvironment固有configurationを埋め込まない。

```text
OCI Image
      +
Runtime Configuration
      +
Secrets
      =
Running Application
```

原則:

- build-time secretをimage layerへ残さない
- secretをrepositoryへ置かない
- secretをDockerfile `ENV`へ焼き込まない
- secretをlog/errorへ出さない
- configurationとsecretを区別する

具体Secret Store製品は後続選定とする。

---

## 17. CI runner policy

```requirement
id = "REQ-DEV-GITHUB-HOSTED-CI"
kind = "architecture"
criticality = "medium"
domain = "development"
```

CIはGitHub-hosted runnerのみを正式利用する。

Self-hosted runnerはv0で使用しない。

Primary CI:

```text
GitHub-hosted Linux runner
```

Secondary portability CI:

```text
GitHub-hosted macOS runner
```

Native Windows runnerは使用しない。

WSL2はLinux semanticsとして扱う。

---

## 18. CI execution model

PR CI:

```text
GitHub Pull Request
        │
        ▼
GitHub-hosted runner
        │
        ├─ Policy / Spec
        ├─ Security
        ├─ Rust
        ├─ Frontend
        ├─ Codegen
        ├─ Integration
        └─ OCI Build
```

---

## 19. mise task hierarchy

### 19.1 `verify:fast`

日常のinner loop。

含む候補:

- formatting
- static analysis
- architecture checks
- fast unit tests
- spec validation

Performance target:

```text
< 60 sec
```

### 19.2 `verify`

PR前の標準確認。

```text
verify:fast
+
full unit tests
+
frontend verification
+
codegen consistency
+
dependency / license / secret checks
```

### 19.3 `verify:full`

重い境界。

```text
verify
+
integration tests
+
ephemeral dependencies
+
production OCI image build
+
container smoke tests
```

---

## 20. CI DAG

単一巨大Jobにしない。

```text
                    ┌─ policy
                    ├─ security
changes ────────────┼─ rust-static
                    ├─ rust-test
                    ├─ frontend
                    └─ portability-macos
                           │
               ┌───────────┴──────────┐
               ▼                      ▼
          integration             OCI build
```

独立Jobを並列化する。

---

## 21. Path-aware CI

変更されたSubsystemに応じて重い検査を選択できる構造にする。

例:

```text
spec/** only
→ policy / codegen / architecture

apps/web/**
→ frontend / API contract compatibility

crates/extraction/**
→ extraction-specific verification
```

Branch ProtectionがJob構成変更で壊れないよう、最終集約Gateを持つ。

---

## 22. Merge gate

個別Jobではなく最終集約GateをBranch Protectionのrequired checkとする。

```text
PR Jobs
├─ policy
├─ security
├─ rust-static
├─ rust-test
├─ frontend
├─ codegen
├─ integration
├─ container-build
└─ portability-macos
        │
        ▼
   required-check
```

重要Gateを`continue-on-error`で無効化しない。

---

## 23. Superseded run cancellation

同一PRへ新commitがpushされた場合、古いrunをcancelできる構造とする。

目的:

- CI cost削減
- 古い結果待ちの防止
- latest SHAへのfeedback集中

---

## 24. Cache architecture

Cacheは最初から設計対象とする。

### Rust

分離候補:

- Cargo registry
- Cargo git
- compiler/build cache

Cache keyに考慮:

- OS
- Rust toolchain
- `Cargo.lock`
- target triple

### Frontend

package manager storeをlockfile単位でcacheする。

`node_modules`そのものを無条件共有する設計を避ける。

### OCI build

BuildKit cacheを利用可能な構造にする。

```text
dependency layer
↓
application source layer
```

を分離し、source変更で全dependency rebuildになりにくい構成を要求する。

---

## 25. Cache correctness

Cacheはbuild correctnessへ影響してはならない。

```text
cache miss
→ slower
→ same result

cache hit
→ faster
→ same result
```

Cacheがないとbuildできない構成は禁止する。

---

## 26. CI performance budget

初期Target:

```text
Local verify:fast
< 60 sec

PR critical path
< 5 min

PR full verification
< 10 min
```

これは絶対SLAではなくArchitecture Performance Budgetとする。

恒常超過時は以下を改善する。

- test splitting
- affected test selection
- cache
- parallelism
- test boundary

Gate削除による単純高速化を原則禁止する。

---

## 27. Secret detection architecture

Secret Detectionは必須だが、具体scannerは後続選定とする。

```text
Local
├─ pre-commit
└─ pre-push

Remote
└─ GitHub Actions
```

検査対象:

- private keys
- API tokens
- credentials
- known secret patterns
- high-confidence entropy findings

Git history / PR diffをscan可能にする。

---

## 28. Secret scan output policy

Secret scan結果で秘密情報そのものをCI logへ再出力しない。

False positive allowlistは:

```text
repository managed
+
reason required
+
narrow scope
```

を原則とする。

---

## 29. Local hooks

### pre-commit

数秒で完了する軽量Gate。

候補:

- format
- staged secret scan
- lightweight policy check

Workspace全体compile等の重い処理は原則入れない。

### pre-push

候補:

- secret scan
- `verify:fast`
- generated consistency軽量確認

Full integration / OCI buildは通常入れない。

原則:

```text
Local Hook
= early feedback

CI
= authoritative gate
```

---

## 30. Codegen as first-class CI gate

以下をCodegen sourceとする。

```text
OpenAPI 3.2.1
JSON Schema
Error Registry
Audit Event Schema
```

概念:

```text
source specs
    ↓
codegen
    ↓
Rust / TypeScript artifacts
```

CIでは以下相当を検証する。

```text
mise run generate
git diff --exit-code
```

Codegenは:

```text
same input
+
same generator version
=
same output
```

を要求する。

---

## 31. Generated artifact policy

GeneratorごとにGit管理するかを選択可能とする。

Git管理する場合:

```text
generate
↓
git diff --exit-code
```

Git管理しない場合:

```text
clean checkout
↓
generate
↓
build/test
```

必須:

- source specが明確
- generator versionが固定
- generation commandが一意
- generated fileを手修正しない

---

## 32. Integration test isolation

Integration Testはephemeralとする。

```text
CI Job
  │
  ├─ PostgreSQL container
  ├─ temporary file storage
  ├─ temporary search index
  └─ application under test
        ↓
      Tests
        ↓
      Destroy
```

禁止:

- shared development DB
- shared search index
- previous CI state
- manual pre-created environment

---

## 33. Database test lifecycle

例:

```text
start container
↓
health ready
↓
migration from zero
↓
seed synthetic fixture
↓
test
↓
destroy
```

Migrationがclean environmentから成立することを検査する。

---

## 34. Test data policy

CI / repository / local fixtureで以下を使用しない。

- 実顧客データ
- 実個人情報
- 実金融データ
- 実credential

原則:

```text
Synthetic data
```

明示的にsanitizedされたdata利用は例外扱いとする。

---

## 35. Test boundaries

Test Pyramid比率は先に固定しない。

責務境界ごとに試験する。

```text
Domain
→ pure tests

Application
→ port / mock contract tests

Infrastructure Adapter
→ integration tests

OpenAPI boundary
→ contract tests

Frontend
→ component / interaction tests

Full system
→ selected E2E flows
```

E2Eへ全責務を押し込まない。

---

## 36. Hermetic test policy

可能な試験は外部Internetへ依存させない。

PR CIで以下へ実アクセスしないことを原則とする。

- external Web API
- external LLM
- external document source

Adapter Contractをfixture / local mockで検証する。

実接続検証は:

```text
scheduled
manual
environment-specific
```

な別workflowへ分離する。

---

## 37. Supply-chain security

対象:

```text
Supply Chain
├─ Rust crates
├─ JS packages
├─ GitHub Actions
├─ OCI base images
└─ build tools
```

最低要件:

- lockfile
- license policy
- known vulnerability scan
- dependency source検査
- secret scan
- GitHub Actions最小権限
- third-party Action immutable pinning
- base image policy

---

## 38. GitHub Actions permissions

Default:

```yaml
permissions:
  contents: read
```

必要Jobだけ追加権限を持つ。

将来Release等で必要な場合:

```text
packages: write
id-token: write
```

等をJob限定で利用する。

対応サービスではlong-lived secretよりOIDC / short-lived credentialを優先する。

---

## 39. Third-party GitHub Actions

原則:

- GitHub official
- well-maintained / trusted vendor

を優先する。

Supply-chain保護のためimmutable commit SHA pinを基本方針とする。

```text
uses: vendor/action@<commit-sha>
```

Update automationは後続tool選定とする。

---

## 40. SBOM / provenance readiness

Production artifactについて以下を生成可能なpipelineとする。

```text
OCI Image
├─ SBOM
├─ dependency inventory
└─ build provenance
```

v0では特定generator / signing productは固定しない。

後続候補:

- CycloneDX
- SPDX
- SLSA provenance
- OCI signing

---

## 41. Base image policy

最小サイズだけでbase imageを選ばない。

評価項目:

- security updates
- CVE visibility
- compatibility
- debuggability
- size
- license
- maintenance

Distroless / static binary等の具体構成はnative dependency選定後に決める。

---

## 42. Release artifact immutability

同一version tagの上書きを前提にしない。

Release識別:

```text
version
+
git commit SHA
+
image digest
```

Deployment時にOCI digestまで追跡可能にする。

---

## 43. Build metadata

Runtimeから最低限以下を相関可能にする。

- source revision
- release version

build timestampをartifact contentへ含めるかはreproducibility要件と合わせて後続決定する。

---

## 44. PR CI / Release CI separation

PR:

```text
build
test
OCI build verification
```

Release:

```text
immutable artifact publication
```

Deployment:

```text
environment-specific operation
```

PR jobへ本番credentialを渡さない。

---

## 45. Release gate

正式Releaseは保護されたsource revisionからのみ作成する。

```text
PR
↓
validation

protected revision
↓
release trigger

reverification / artifact build
↓
OCI publish
↓
digest / SBOM / provenance record
```

具体的branch model / tag policyはRepository Governanceで決定する。

---

## 46. Environment separation

Environment-specific値をsource/build definitionへ埋め込まない。

```text
Same OCI Artifact
      │
      ├─ Development config
      ├─ Test config
      └─ Production config
```

同一image digestを環境間でpromote可能な構造を目指す。

---

## 47. Configuration validation

Applicationはstartup時にrequired configurationを検証する。

```text
startup
↓
configuration validation
↓
ready
```

Secret値そのものをerrorへ出さない。

例:

```text
CONFIG_REQUIRED_FIELD_MISSING
```

のようなstable error codeへmapping可能にする。

---

## 48. Migration gate

CIでは必ず:

```text
empty DB
↓
all migrations
↓
current schema
```

を検証する。

可能なら将来:

```text
N-1 schema → N
```

のupgrade pathも検証可能にする。

Migration toolingはDB selection後に決める。

---

## 49. Container performance budget

追跡候補:

- OCI image size
- cold build duration
- cached build duration
- runtime startup time
- memory baseline

優先順位:

```text
Correctness
Security
Reproducibility
Build cache efficiency
Startup / runtime efficiency
Image size
```

数MB削減のためにsecurity updateability / debuggabilityを犠牲にしない。

---

## 50. CI optimization lifecycle

CIも継続改善対象とする。

追跡候補:

- job duration
- queue duration
- cache hit rate
- test duration
- failure frequency
- flaky / quarantine count

Performance Budget超過が恒常化した場合にCI Architectureを改善する。

---

## 51. Dependency update policy

原則:

```text
Security update
→ 優先

Patch / minor
→ automation可能

Major
→ architecture / compatibility review
```

SemVerだけを盲信せず必要に応じて以下を確認する。

- generated diff
- API compatibility
- benchmark
- license
- security
- container impact

---

## 52. Flaky test policy

Flaky testをretryだけで正常扱いしない。

禁止:

```text
1st FAIL
2nd PASS
→ permanent CI PASS
```

発見時:

```text
fix root cause
or
explicit quarantine
```

Quarantine数を追跡可能にする。

---

## 53. CI failure ergonomics

Job責務を小さく保つ。

```text
policy
rust-static
rust-test
frontend
security
integration
container-build
```

Failure時に以下を特定しやすくする。

- failing command
- failing test
- generated diff
- contract mismatch
- relevant artifact

巨大wrapper scriptで失敗理由を隠さない。

---

## 54. Failure artifacts

保存候補:

- test report
- OpenAPI diff
- generated diff
- container build metadata
- integration log
- visual regression diff

無条件保存禁止:

- database dump
- document body
- raw financial data
- credentials
- secret-containing environment

ArtifactにもSensitive Data Policyを適用する。

---

## 55. Build / test reproducibility

同一commitについて以下で同じbuild definitionを使う。

```text
Linux local
GitHub-hosted Linux runner
Production build
```

理想:

```text
Source + Lockfiles + Toolchain Definition
                    │
          ┌─────────┼──────────┐
          ▼         ▼          ▼
        Local       CI      OCI Build
```

v0でbit-for-bit reproducible binaryは必須にしない。

必須:

- dependency version reproducibility
- toolchain version reproducibility
- codegen reproducibility
- build step reproducibility
- source revision traceability

---

## 56. Repository hygiene

最低限以下が誤commitされない構造を持つ。

- `.env*`
- private keys
- local DB files
- build outputs
- coverage outputs
- temporary indexes
- IDE-local files
- production/customer fixtures

`.gitignore`だけに依存せずSecret Scannerを第二防衛線とする。

---

## 57. Architecture enforcement

可能なものはmachine-enforceする。

候補:

- GitHub-hosted runner only
- build logicはmise taskへ集約
- Native Windows supportを要求しない
- production target = `linux/amd64`
- lockfile存在
- generated diff clean
- OS-specific dependency boundary
- canonical Dockerfile policy
- repository hygiene
- license policy

文書を人手運用だけにしない。

---

## 58. macOS portability gate

macOSでは毎PR full integrationを必須にしない。

Portability smoke test候補:

- mise setup
- Rust workspace compile/check
- Frontend install/typecheck/build
- architecture tooling

Production semanticsのfull verificationはLinux authoritative gateで行う。

---

## 59. Non-goals v0

以下はv0で要求しない。

```text
Kubernetes
Service Mesh
self-hosted GitHub Runner
Dev Container必須化
Docker-in-Docker development
Native Windows development
production multi-arch
remote build farm
distributed compilation
every-PR macOS full integration
bit-for-bit reproducible binary
environment-specific Dockerfile
```

必要性が具体化した時点で追加する。

---

## 60. Acceptance criteria

本Architectureに適合する開発基盤は最低限以下を満たす。

- [ ] Linux / macOS / Windows via WSL2 を正式開発環境とする
- [ ] Native Windows developmentを正式サポートしない
- [ ] WSL2ではLinux filesystem上でのrepository配置を標準とする
- [ ] miseをtool/task SSOTとする
- [ ] native inner loopをcanonical development pathとする
- [ ] Containerを外部依存・再現性・Deployment境界として利用する
- [ ] Production artifactを`linux/amd64` OCI imageとする
- [ ] canonical multi-stage Dockerfileを用いる
- [ ] Runtime imageへbuild toolchainを持ち込まない
- [ ] GitHub-hosted runnerのみをCIで利用する
- [ ] Linux CIをauthoritative gateとする
- [ ] macOS portability gateを持つ
- [ ] `verify:fast` / `verify` / `verify:full` のtask hierarchyを持つ
- [ ] CIを並列DAGとして構成する
- [ ] superseded run cancellationを利用可能にする
- [ ] cache correctnessがbuild correctnessへ影響しない
- [ ] Codegen consistencyをCI gateとする
- [ ] Secret detectionをlocal + CIで実施する
- [ ] Integration Testをephemeralにする
- [ ] Synthetic test dataを原則とする
- [ ] PR CIを外部Internet依存から可能な限り隔離する
- [ ] GitHub Actionsをleast privilegeで運用する
- [ ] third-party Actionをimmutable pin可能にする
- [ ] SBOM / provenanceを後から追加可能にする
- [ ] PR CIとRelease CIを分離する
- [ ] Release artifactをsource revision / digestへ相関可能にする
- [ ] CI Performance Budgetを持つ
- [ ] Flaky testをretryだけで正常扱いしない
- [ ] Architecture Contractをmachine-enforce可能にする

---

## 61. Related specifications

- Architecture Contract v0
- Transaction & Consistency Requirements v0
- Error Handling & Resilience Requirements v0
- Observability & Audit Requirements v0
- Frontend / UX Requirements v0
- DB Selection Criteria v0
- Rust Library Matrix v0
- Repository Bootstrap Design v0

---

## 62. Next selection stage

本Architecture確定後に、具体tool/libraryを選定する。

最低限の選定対象:

- Secret scanner
- OpenAPI 3.2 tooling
- JSON Schema validator / generator
- Rust OpenTelemetry integration
- Frontend build / router / state libraries
- GitHub Actions helper tools
- Container base image
- SBOM / provenance tooling
- dependency update automation
- vulnerability / license tooling

**ToolありきでArchitectureを変更しない。**
要件を満たさないtoolは候補から外すかAdapterで吸収する。
