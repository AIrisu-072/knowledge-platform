# Repository Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `AIrisu-072/knowledge-platform` を、仕様駆動・高速なnative inner loop・GitHub-hosted CI・machine-enforced Architecture・最小のDevelopment Assurance vertical sliceを備えた開発可能なrepositoryへbootstrapする。

**Architecture:** `spec/` をnormative SSOTとして維持し、production feature codeはまだ作らない。Root toolingは`mise`へ集約し、Rust workspaceにはbootstrapに必要な `tools/architecture-lint` と `tools/assurance` だけを置く。Development Assuranceはこのplanでは Requirement extraction → Level 0/1 Code Graph → impact plan → command Provider → Evidence の最小vertical sliceまでとし、Context Compiler / Agent Gateway / Symbol Graphは別planへ分離する。

**Tech Stack:** Rust 1.98.1, Node.js 24.21.0 LTS, pnpm 12.4.1, mise, cargo-nextest, cargo-deny, Gitleaks CLI, OSV-Scanner, Syft, actionlint, zizmor, Redocly CLI 2.52.1, GitHub-hosted Actions, Docker BuildKit.

**Spec:**  
- `docs/design/repository-bootstrap-design-v0.md`
- `spec/architecture/architecture-contract-v0.md`
- `spec/architecture/development-container-ci-architecture-v0.md`
- `spec/architecture/development-assurance-architecture-v0.md`
- `spec/selection/library-tool-selection-v0.md`

## Global Constraints

- 顧客・金融機関の固有名詞、実顧客データ、実金融データ、個人情報、credential、token、private keyをrepositoryへ入れない。
- 日本語を設計・運用文書の第一言語とする。
- Development supportは Linux / macOS / Windows via WSL2 only。Native Windowsは正式サポートしない。
- Production targetは `linux/amd64` OCI image。multi-archはv0非要件。
- CIはGitHub-hosted runnerのみ。Linuxをauthoritative、macOSをportability smoke testとする。
- Local / CIのtask entrypointは`mise`へ集約する。
- `POC REQUIRED` libraryをproduction dependencyへ追加しない。
- production用の空crateを大量に作らない。
- third-party GitHub Actionは最終commit前にfull commit SHAへpinする。
- generated Assurance IR / EvidenceはGit管理しない。
- Development AssuranceはLLMをnormal verification pathのOracleにしない。
- Repository license自体はこのplanでは決めない。Dependency license policyだけを`cargo-deny`等で強制する。
- このplanではApplication API / Document / Search / UI production implementationを開始しない。

---

# File Structure After This Plan

```text
knowledge-platform/
├─ .github/
│  └─ workflows/
│     └─ ci.yml
├─ .githooks/
│  ├─ pre-commit
│  └─ pre-push
├─ docs/
│  └─ superpowers/
│     └─ plans/
│        └─ 2026-09-14-repository-bootstrap-implementation.md
├─ experiments/
│  └─ README.md
├─ spec/
│  ├─ api/
│  │  ├─ README.md
│  │  └─ openapi.yaml
│  ├─ architecture/
│  │  ├─ dependency-rules.toml
│  │  └─ ...
│  ├─ assurance/
│  │  ├─ README.md
│  │  ├─ control-policy.toml
│  │  └─ capabilities/
│  │     └─ bootstrap.toml
│  └─ ...
├─ third_party/
│  └─ README.md
├─ tools/
│  ├─ architecture-lint/
│  │  ├─ Cargo.toml
│  │  ├─ src/
│  │  │  ├─ lib.rs
│  │  │  ├─ config.rs
│  │  │  ├─ checks.rs
│  │  │  ├─ report.rs
│  │  │  └─ main.rs
│  │  └─ tests/
│  │     └─ policy.rs
│  └─ assurance/
│     ├─ Cargo.toml
│     ├─ src/
│     │  ├─ lib.rs
│     │  ├─ requirement.rs
│     │  ├─ graph.rs
│     │  ├─ capability.rs
│     │  ├─ planner.rs
│     │  ├─ provider.rs
│     │  ├─ evidence.rs
│     │  └─ main.rs
│     └─ tests/
│        ├─ requirements.rs
│        ├─ graph.rs
│        └─ plan.rs
├─ .editorconfig
├─ .gitignore
├─ .gitleaks.toml
├─ AGENTS.md
├─ Cargo.lock
├─ Cargo.toml
├─ Dockerfile
├─ README.md
├─ deny.toml
├─ mise.lock
├─ mise.toml
├─ package.json
├─ pnpm-lock.yaml
├─ pnpm-workspace.yaml
└─ rust-toolchain.toml
```

---

### Task 1: Root toolchain and task contract

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `package.json`
- Create: `pnpm-workspace.yaml`
- Create: `mise.toml`
- Create: `mise.lock`
- Create: `.editorconfig`
- Create: `.gitignore`
- Create: `README.md`
- Create: `AGENTS.md`
- Create: `experiments/README.md`
- Create: `third_party/README.md`

**Interfaces:**
- Produces: `mise run bootstrap`, `mise run fmt`, `mise run check:rust`, `mise run test:rust`, `mise run verify:fast`, `mise run verify`, `mise run verify:full`.
- Produces: Rust workspace containing only `tools/architecture-lint` and `tools/assurance`.
- Consumes: approved `spec/` and `docs/` content already imported.

- [ ] **Step 1: Add a failing repository bootstrap smoke check**

Create a temporary shell command in the implementation session, without committing it:

```bash
test -f Cargo.toml \
  && test -f rust-toolchain.toml \
  && test -f mise.toml \
  && test -f package.json
```

Run from repository root.

Expected: FAIL because bootstrap files do not yet exist.

- [ ] **Step 2: Create the Rust workspace contract**

Create `Cargo.toml`:

```toml
[workspace]
members = [
  "tools/architecture-lint",
  "tools/assurance",
]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.98"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

Do **not** add production crates.

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.98.1"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 3: Create Node/pnpm tooling-only root**

Create `package.json`:

```json
{
  "name": "knowledge-platform-tooling",
  "private": true,
  "packageManager": "pnpm@12.4.1",
  "devDependencies": {
    "@redocly/cli": "2.52.1"
  },
  "scripts": {
    "api:lint": "redocly lint spec/api/openapi.yaml"
  }
}
```

Create `pnpm-workspace.yaml`:

```yaml
packages:
  - "apps/*"
  - "experiments/*"
```

No React/Vite app is created in this plan. TypeScript 7 compatibility remains P0 PoC.

- [ ] **Step 4: Create the initial mise tool lock contract**

Create `mise.toml` with exact project runtime pins and selected CLI tools:

```toml
[tools]
rust = "1.98.1"
node = "24.21.0"
pnpm = "12.4.1"
"cargo:cargo-deny" = "0.20.2"
"cargo:cargo-nextest" = "0.9.144"
"npm:@redocly/cli" = "2.52.1"
"github:gitleaks/gitleaks" = "8.30.1"
"github:google/osv-scanner" = "2.5.1"
"github:anchore/syft" = "1.51.1"
"github:rhysd/actionlint" = "1.7.12"
"github:zizmorcore/zizmor" = "1.29.0"

[env]
REDOCLY_TELEMETRY = "off"
REDOCLY_SUPPRESS_UPDATE_NOTICE = "true"

[tasks.bootstrap]
description = "Install locked tools, dependencies, and local git hooks"
run = [
  "mise install",
  "pnpm install --frozen-lockfile",
  "git config core.hooksPath .githooks",
]

[tasks.fmt]
description = "Check source formatting"
run = [
  "cargo fmt --all -- --check",
]

[tasks.check:rust]
description = "Run Rust static checks"
run = [
  "cargo check --workspace --locked",
  "cargo clippy --workspace --all-targets --locked -- -D warnings",
]

[tasks.test:rust]
description = "Run Rust tests"
run = [
  "cargo nextest run --workspace",
]

[tasks.api:check]
description = "Validate the OpenAPI contract"
run = [
  "pnpm api:lint",
]

[tasks.arch:check]
description = "Run machine-enforced architecture policy"
run = [
  "cargo run --quiet --locked -p architecture-lint -- check",
]

[tasks.assure:plan]
description = "Compile assurance impact plan"
run = [
  "cargo run --quiet --locked -p assurance-cli -- plan",
]

[tasks.assure:run]
description = "Run planned assurance capabilities"
run = [
  "cargo run --quiet --locked -p assurance-cli -- run",
]

[tasks.assure:report]
description = "Render assurance evidence and gaps"
run = [
  "cargo run --quiet --locked -p assurance-cli -- report",
]

[tasks.verify:fast]
description = "Fast local feedback gate"
depends = ["fmt", "check:rust", "arch:check", "api:check", "test:rust"]

[tasks.verify]
description = "Standard pre-PR verification gate"
depends = ["verify:fast"]

[tasks.verify:full]
description = "Full reproducibility/integration gate"
depends = ["verify"]
```

Do not yet add security and container tasks; Tasks 4–6 extend these definitions.

Run:

```bash
mise lock
```

Commit the generated `mise.lock`.

- [ ] **Step 5: Add repository hygiene**

Create `.gitignore`:

```gitignore
/target/
/node_modules/
/dist/
/coverage/
/tmp/
/.env
.env.*
!.env.example
.DS_Store
*.db
*.sqlite
*.sqlite3
*.pem
*.key
*.p12
*.pfx
target/assurance/
```

Create `.editorconfig`:

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
indent_style = space
indent_size = 2
trim_trailing_whitespace = true

[*.rs]
indent_size = 4

[*.md]
trim_trailing_whitespace = false
```

Create `experiments/README.md` stating that PoCs are non-production and `POC REQUIRED` dependencies stay here until accepted.

Create `third_party/README.md` stating dependency → composition → thin extension → separate fork → vendoring order.

- [ ] **Step 6: Add human/agent entrypoint without duplicating normative rules**

Create `AGENTS.md`:

```markdown
# Agent entrypoint

このrepositoryでは `spec/` がnormative SSOTです。

作業開始時に全文を読み込まず、対象変更に関係する仕様だけを参照してください。
最終的な制約判定はDevelopment Assurance / CIに従います。

基本コマンド:

- `mise run verify:fast`
- `mise run verify`
- `mise run verify:full`

重要:
- 顧客固有名詞・実データ・秘密情報をrepositoryへ入れない。
- `POC REQUIRED` dependencyをproductionへ追加しない。
- production実装を始める前に該当specとimplementation planを確認する。
```

Create `README.md` containing only generic project purpose, `spec/` location, supported development OS, and the three `mise` verification commands.

- [ ] **Step 7: Generate lockfiles and verify bootstrap**

Run:

```bash
mise install
pnpm install
cargo metadata --format-version 1 --no-deps
mise lock
```

Expected:
- tool installation succeeds on supported host;
- `pnpm-lock.yaml`, `Cargo.lock`, `mise.lock` exist;
- no application crate exists.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml package.json pnpm-lock.yaml pnpm-workspace.yaml mise.toml mise.lock .editorconfig .gitignore README.md AGENTS.md experiments/README.md third_party/README.md
git commit -m "chore: bootstrap reproducible development toolchain"
```

---

### Task 2: Machine-readable Architecture Contract and custom linter

**Files:**
- Create: `spec/architecture/dependency-rules.toml`
- Create: `tools/architecture-lint/Cargo.toml`
- Create: `tools/architecture-lint/src/lib.rs`
- Create: `tools/architecture-lint/src/config.rs`
- Create: `tools/architecture-lint/src/checks.rs`
- Create: `tools/architecture-lint/src/report.rs`
- Create: `tools/architecture-lint/src/main.rs`
- Create: `tools/architecture-lint/tests/policy.rs`

**Interfaces:**
- Consumes: `spec/architecture/dependency-rules.toml`.
- Produces: CLI `architecture-lint check [--format human|json]`.
- Produces JSON for the later generic Assurance command Provider.
- Must finish in a few seconds on bootstrap repository.

- [ ] **Step 1: Write failing architecture-policy tests**

Create `tools/architecture-lint/tests/policy.rs` with tests equivalent to:

```rust
use architecture_lint::{check_repository, Config};

#[test]
fn valid_bootstrap_repository_passes() {
    let fixture = architecture_lint::test_support::Fixture::valid();
    let report = check_repository(fixture.root(), &Config::for_fixture()).unwrap();
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

#[test]
fn self_hosted_runner_is_rejected() {
    let fixture = architecture_lint::test_support::Fixture::valid()
        .with_file(".github/workflows/ci.yml", "jobs:\n  test:\n    runs-on: self-hosted\n");
    let report = check_repository(fixture.root(), &Config::for_fixture()).unwrap();
    assert!(report.findings.iter().any(|f| f.code == "ARCH_CI_SELF_HOSTED"));
}

#[test]
fn native_windows_runner_is_rejected() {
    let fixture = architecture_lint::test_support::Fixture::valid()
        .with_file(".github/workflows/ci.yml", "jobs:\n  test:\n    runs-on: windows-latest\n");
    let report = check_repository(fixture.root(), &Config::for_fixture()).unwrap();
    assert!(report.findings.iter().any(|f| f.code == "ARCH_CI_NATIVE_WINDOWS"));
}

#[test]
fn dockerfile_variants_are_rejected() {
    let fixture = architecture_lint::test_support::Fixture::valid()
        .with_file("Dockerfile.prod", "FROM scratch\n");
    let report = check_repository(fixture.root(), &Config::for_fixture()).unwrap();
    assert!(report.findings.iter().any(|f| f.code == "ARCH_DOCKERFILE_VARIANT"));
}
```

Use a deterministic temporary fixture helper inside `#[cfg(test)]`; do not depend on real repository contents for unit tests.

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p architecture-lint --test policy
```

Expected: FAIL because the crate/API does not exist.

- [ ] **Step 3: Define machine-readable rules**

Create `spec/architecture/dependency-rules.toml`:

```toml
version = 1

[repository]
required_files = [
  "spec/architecture/architecture-contract-v0.md",
  "spec/architecture/development-container-ci-architecture-v0.md",
  "spec/architecture/development-assurance-architecture-v0.md",
  "spec/operations/error-handling-resilience-requirements-v0.md",
  "spec/operations/observability-audit-requirements-v0.md",
  "spec/requirements/frontend-ux-requirements-v0.md",
  "spec/selection/library-tool-selection-v0.md",
  "mise.toml",
  "rust-toolchain.toml",
]
forbidden_top_level = [
  "Dockerfile.dev",
  "Dockerfile.test",
  "Dockerfile.ci",
  "Dockerfile.prod",
]

[platform]
production_target = "linux/amd64"
native_windows_supported = false

[ci]
allow_self_hosted = false
allow_native_windows = false
require_mise_entrypoint = true

[workspace]
allowed_production_roots = ["crates", "apps"]
tooling_root = "tools"
```

- [ ] **Step 4: Implement the minimal linter library**

Create package `architecture-lint` with `publish = false`. Use only tooling dependencies required to parse TOML and emit JSON.

Core public types:

```rust
pub struct Finding {
    pub code: String,
    pub path: String,
    pub message: String,
}

pub struct Report {
    pub findings: Vec<Finding>,
}

pub fn check_repository(root: &Path, config: &Config) -> Result<Report, Error>;
```

Checks in v0:

```text
ARCH_REQUIRED_FILE_MISSING
ARCH_DOCKERFILE_VARIANT
ARCH_CI_SELF_HOSTED
ARCH_CI_NATIVE_WINDOWS
ARCH_CI_BYPASSES_MISE
```

Do not implement TypeScript AST or symbol-call analysis here.

- [ ] **Step 5: Add stable JSON output**

`architecture-lint check --format json` must emit:

```json
{
  "status": "pass",
  "findings": []
}
```

or:

```json
{
  "status": "fail",
  "findings": [
    {
      "code": "ARCH_CI_SELF_HOSTED",
      "path": ".github/workflows/ci.yml",
      "message": "self-hosted runner is forbidden by architecture contract"
    }
  ]
}
```

Exit `0` for pass, non-zero for violations or tool failure.

- [ ] **Step 6: Run focused tests**

```bash
cargo nextest run -p architecture-lint
cargo clippy -p architecture-lint --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 7: Run against the real repository**

```bash
mise run arch:check
```

Expected: PASS before CI exists; the linter must treat absent `.github/workflows/*.yml` as acceptable until Task 6 creates CI, while required normative files must already exist.

- [ ] **Step 8: Commit**

```bash
git add spec/architecture/dependency-rules.toml tools/architecture-lint Cargo.toml Cargo.lock
git commit -m "feat: add machine-enforced architecture policy"
```

---

### Task 3: Minimal Development Assurance vertical slice

**Files:**
- Create: `spec/assurance/README.md`
- Create: `spec/assurance/control-policy.toml`
- Create: `spec/assurance/capabilities/bootstrap.toml`
- Modify: `spec/architecture/development-container-ci-architecture-v0.md`
- Create: `tools/assurance/Cargo.toml`
- Create: `tools/assurance/src/lib.rs`
- Create: `tools/assurance/src/requirement.rs`
- Create: `tools/assurance/src/graph.rs`
- Create: `tools/assurance/src/capability.rs`
- Create: `tools/assurance/src/planner.rs`
- Create: `tools/assurance/src/provider.rs`
- Create: `tools/assurance/src/evidence.rs`
- Create: `tools/assurance/src/main.rs`
- Create: `tools/assurance/tests/requirements.rs`
- Create: `tools/assurance/tests/graph.rs`
- Create: `tools/assurance/tests/plan.rs`

**Interfaces:**
- Produces binary name: `assure`.
- Produces subcommands: `scan`, `plan`, `run`, `report`.
- `plan` accepts `--changed-from <git-sha>` and `--all`.
- v0 graph: repository/files/workspace packages plus Cargo workspace dependency edges.
- v0 Provider: generic command Provider.
- Evidence path: `target/assurance/evidence/`.
- This task does **not** implement Context Compiler, Agent Gateway, MCP, Symbol Graph, or LLM invocation.

- [ ] **Step 1: Define a minimal requirement metadata convention**

Create `spec/assurance/README.md` specifying that normative Markdown can include a fenced TOML block:

````markdown
```requirement
id = "REQ-DEV-GITHUB-HOSTED-CI"
kind = "architecture"
criticality = "medium"
domain = "development"
```
````

The prose around the block remains the normative meaning. The block only supplies machine-readable identity/classification; it is not a new DSL.

- [ ] **Step 2: Add metadata to three already-approved requirements**

In `spec/architecture/development-container-ci-architecture-v0.md`, add metadata blocks immediately under the relevant existing sections without changing their prose:

```toml
id = "REQ-DEV-GITHUB-HOSTED-CI"
kind = "architecture"
criticality = "medium"
domain = "development"
```

```toml
id = "REQ-DEV-MISE-TASK-SSOT"
kind = "architecture"
criticality = "medium"
domain = "development"
```

```toml
id = "REQ-DEV-PRODUCTION-LINUX-AMD64"
kind = "architecture"
criticality = "medium"
domain = "development"
```

- [ ] **Step 3: Write failing requirement-extraction tests**

Create a test that parses Markdown containing the fenced block:

```rust
#[test]
fn extracts_requirement_metadata_from_normative_markdown() {
    let markdown = r#"
## CI runner policy

```requirement
id = "REQ-DEV-GITHUB-HOSTED-CI"
kind = "architecture"
criticality = "medium"
domain = "development"
```

CIはGitHub-hosted runnerのみを正式利用する。
"#;

    let reqs = assurance_cli::requirement::extract(markdown, "spec/example.md").unwrap();

    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].id, "REQ-DEV-GITHUB-HOSTED-CI");
    assert_eq!(reqs[0].domain, "development");
}
```

- [ ] **Step 4: Run the test and verify failure**

```bash
cargo test -p assurance-cli --test requirements
```

Expected: FAIL because the parser is missing.

- [ ] **Step 5: Implement Requirement extraction**

Use a Markdown parser to inspect fenced code blocks whose info string is exactly `requirement`, then parse the block body as TOML.

Output:

```rust
pub struct Requirement {
    pub id: String,
    pub kind: String,
    pub criticality: Criticality,
    pub domain: String,
    pub source: PathBuf,
    pub section: Option<String>,
}
```

Reject duplicate requirement IDs during `scan`.

- [ ] **Step 6: Add Control policy and bootstrap Capability declaration**

Create `spec/assurance/control-policy.toml`:

```toml
version = 1

[criticality.low]
minimum_capabilities = 1

[criticality.medium]
minimum_capabilities = 1

[criticality.high]
minimum_capabilities = 2
```

Create `spec/assurance/capabilities/bootstrap.toml`:

```toml
version = 1

[[capability]]
id = "CAP-ARCH-BOOTSTRAP-POLICY"
controls = [
  "CTRL-DEV-GITHUB-HOSTED-CI",
  "CTRL-DEV-MISE-TASK-SSOT",
  "CTRL-DEV-PRODUCTION-LINUX-AMD64",
]
provider = "command"
mechanism = "static"
oracle = "architecture-contract"
cost = "fast"
command = [
  "cargo",
  "run",
  "--quiet",
  "--locked",
  "-p",
  "architecture-lint",
  "--",
  "check",
  "--format",
  "json",
]
scope_paths = [
  ".github/",
  "mise.toml",
  "Dockerfile",
  "spec/architecture/",
]
```

Control IDs are mechanically derived as `REQ-*` → `CTRL-*`; do not duplicate a hand-written control registry.

- [ ] **Step 7: Write failing Level 0/1 graph tests**

The graph test should construct a fixture Cargo workspace and assert:

```rust
assert!(graph.component("tools/architecture-lint").is_some());
assert!(graph.component("tools/assurance").is_some());
assert!(graph.files().any(|p| p.ends_with("mise.toml")));
```

For Level 1, inject or execute `cargo metadata --format-version 1 --no-deps` and verify workspace package dependency edges are represented.

- [ ] **Step 8: Implement Level 0/1 Code Graph**

v0 graph model:

```rust
pub enum NodeKind {
    Repository,
    Component,
    File,
    Spec,
}

pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub path: Option<PathBuf>,
}

pub enum EdgeKind {
    Contains,
    DependsOn,
    GovernedBy,
}
```

Populate:
- repository tree;
- workspace packages from Cargo metadata;
- workspace package dependency edges;
- normative spec file nodes.

Do not parse Rust symbols.

- [ ] **Step 9: Write failing impact-plan tests**

Given:

```text
changed path = ".github/workflows/ci.yml"
```

expected plan:

```text
affected requirement:
  REQ-DEV-GITHUB-HOSTED-CI

planned capability:
  CAP-ARCH-BOOTSTRAP-POLICY
```

Given:

```text
changed path = "docs/research/note.md"
```

expected plan: no bootstrap architecture capability unless a declared scope matches.

- [ ] **Step 10: Implement Planner**

Public API:

```rust
pub struct Plan {
    pub requirements: Vec<String>,
    pub controls: Vec<String>,
    pub capabilities: Vec<String>,
    pub gaps: Vec<Gap>,
}

pub fn plan(
    graph: &Graph,
    requirements: &[Requirement],
    capabilities: &[Capability],
    changed_paths: &[PathBuf],
) -> Plan;
```

If an affected medium/high requirement has no capability, emit:

```text
NO_CAPABILITY
```

Do not invoke an LLM.

- [ ] **Step 11: Implement generic command Provider and Evidence**

Provider contract:

```rust
pub trait Provider {
    fn id(&self) -> &'static str;
    fn run(&self, capability: &Capability) -> Result<Evidence, ProviderError>;
}
```

`CommandProvider`:
- executes the declared argv without shell expansion;
- captures exit status/stdout/stderr;
- treats output as provider artifact;
- never logs secret environment values;
- records commit SHA and provider/capability identity.

Evidence:

```rust
pub struct Evidence {
    pub capability_id: String,
    pub outcome: Outcome,
    pub commit: String,
    pub duration_ms: u64,
    pub artifact_path: Option<PathBuf>,
}
```

Write generated evidence below `target/assurance/evidence/`.

- [ ] **Step 12: Implement CLI vertical slice**

Commands:

```bash
assure scan
assure plan --all
assure plan --changed-from <sha>
assure run --all
assure report
```

`report` prints only compact status/gaps by default. Provider raw output is available through artifact paths for drill-down.

- [ ] **Step 13: Run focused verification**

```bash
cargo nextest run -p assurance-cli
cargo clippy -p assurance-cli --all-targets -- -D warnings
cargo run --quiet -p assurance-cli -- scan
cargo run --quiet -p assurance-cli -- plan --all
cargo run --quiet -p assurance-cli -- run --all
cargo run --quiet -p assurance-cli -- report
```

Expected:
- requirements extracted;
- architecture capability planned;
- architecture Provider passes;
- evidence file generated under ignored `target/assurance/`;
- no LLM call.

- [ ] **Step 14: Commit**

```bash
git add spec/assurance spec/architecture/development-container-ci-architecture-v0.md tools/assurance Cargo.toml Cargo.lock mise.toml
git commit -m "feat: add development assurance vertical slice"
```

---

### Task 4: API, dependency, secret, and workflow policy gates

**Files:**
- Create: `spec/api/README.md`
- Create: `spec/api/openapi.yaml`
- Create: `deny.toml`
- Create: `.gitleaks.toml`
- Modify: `mise.toml`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`

**Interfaces:**
- Produces: `mise run api:check`, `mise run security:secrets`, `mise run security:deps`, `mise run ci:lint`.
- Consumes: OpenAPI 3.2.1, permissive dependency policy.

- [ ] **Step 1: Create a minimal OpenAPI 3.2.1 root contract**

Create `spec/api/openapi.yaml`:

```yaml
openapi: 3.2.1
info:
  title: Knowledge Platform API
  version: 0.0.0
paths: {}
```

Create `spec/api/README.md` stating that endpoint/schema additions must be contract-first and that 3.2-only features require tooling compatibility tests.

Run:

```bash
pnpm api:lint
```

Expected: PASS.

- [ ] **Step 2: Configure strict Rust dependency policy**

Create `deny.toml` with only the approved license set from the normative selection policy:

```toml
[graph]
all-features = false

[advisories]
yanked = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"

[licenses]
allow = [
  "MIT",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "PostgreSQL",
]
confidence-threshold = 0.8

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

Run:

```bash
cargo deny check
```

If a selected tooling dependency uses another permissive license, **do not silently broaden the allowlist**. Stop and update `library-tool-selection-v0.md`/ADR first.

- [ ] **Step 3: Configure Gitleaks without custom secret copies**

Create `.gitleaks.toml`:

```toml
[extend]
useDefault = true
```

No secrets or sample real credentials in fixtures.

- [ ] **Step 4: Add security and workflow tasks**

Extend `mise.toml`:

```toml
[tasks.security:secrets]
run = [
  "gitleaks git --redact --exit-code 1",
]

[tasks.security:deps]
run = [
  "cargo deny check",
  "osv-scanner scan source -r .",
]

[tasks.ci:lint]
run = [
  "actionlint",
  "zizmor .github/workflows",
]

[tasks.security]
depends = ["security:secrets", "security:deps", "ci:lint"]

[tasks.verify]
depends = ["verify:fast", "security"]
```

Do not add Syft to the fast path; use it in Task 7/full verification.

- [ ] **Step 5: Run gates locally**

```bash
mise run api:check
mise run security:secrets
mise run security:deps
```

Expected: PASS, or a dependency-license failure that must be explicitly resolved in policy before proceeding.

- [ ] **Step 6: Commit**

```bash
git add spec/api deny.toml .gitleaks.toml mise.toml package.json pnpm-lock.yaml
git commit -m "chore: add contract and supply-chain policy gates"
```

---

### Task 5: Local pre-commit / pre-push feedback

**Files:**
- Create: `.githooks/pre-commit`
- Create: `.githooks/pre-push`
- Modify: `mise.toml`

**Interfaces:**
- `pre-commit`: staged secret scan + formatting/policy only.
- `pre-push`: secret scan + `verify:fast`.
- CI remains authoritative.

- [ ] **Step 1: Create lightweight pre-commit hook**

```sh
#!/bin/sh
set -eu

exec mise run hook:pre-commit
```

- [ ] **Step 2: Create pre-push hook**

```sh
#!/bin/sh
set -eu

exec mise run hook:pre-push
```

Make both executable.

- [ ] **Step 3: Add hook tasks**

Add to `mise.toml`:

```toml
[tasks.hook:pre-commit]
run = [
  "gitleaks git --staged --redact --exit-code 1",
  "cargo fmt --all -- --check",
  "mise run arch:check",
]

[tasks.hook:pre-push]
run = [
  "gitleaks git --redact --exit-code 1",
  "mise run verify:fast",
]
```

If the pinned Gitleaks version changes the staged-scan syntax, update the task based on that pinned CLI's help output and add a hook smoke test; do not use a floating command.

- [ ] **Step 4: Install and smoke-test hooks**

```bash
git config core.hooksPath .githooks
mise run hook:pre-commit
mise run hook:pre-push
```

Expected: PASS in clean repository.

- [ ] **Step 5: Commit**

```bash
git add .githooks mise.toml
git commit -m "chore: add fast local verification hooks"
```

---

### Task 6: GitHub-hosted CI DAG and merge gate

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `spec/architecture/dependency-rules.toml`
- Modify: `mise.toml`

**Interfaces:**
- GitHub-hosted Linux authoritative gate.
- macOS portability smoke gate.
- Stable required job: `required-check`.
- Superseded PR runs cancelled.
- `permissions: contents: read`.
- No production secrets.

- [ ] **Step 1: Write an intentionally unpinned first workflow and prove security lint catches it**

Create the initial workflow using version tags only in the working tree:

```yaml
name: CI

on:
  pull_request:
  push:

concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

permissions:
  contents: read

jobs:
  policy:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v7.0.1
        with:
          fetch-depth: 0
      - uses: jdx/mise-action@v4
      - run: mise run verify:fast

  portability-macos:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v7.0.1
      - uses: jdx/mise-action@v4
      - run: mise run portability:macos
```

Run:

```bash
zizmor .github/workflows
```

Expected: FAIL because third-party actions are not immutable-SHA pinned.

Do not commit this state.

- [ ] **Step 2: Resolve immutable action SHAs**

Resolve the exact release commits:

```bash
gh api repos/actions/checkout/git/ref/tags/v7.0.1 --jq '.object.sha'
gh api repos/jdx/mise-action/git/ref/tags/v4.0.0 --jq '.object.sha'
```

If the tag object is annotated, dereference it to the commit object before use.

Replace every `uses:` reference with the resulting full commit SHA and retain a trailing version comment, e.g. `# v7.0.1`.

Run:

```bash
zizmor .github/workflows
```

Expected: no `unpinned-uses` finding.

- [ ] **Step 3: Complete the CI DAG**

Final jobs:

```text
policy
rust-static
rust-test
security
portability-macos
container-build
required-check
```

Responsibilities:

```text
policy
→ arch:check + api:check + assurance scan/plan

rust-static
→ fmt + cargo check + clippy

rust-test
→ cargo nextest

security
→ gitleaks + cargo-deny + OSV + actionlint + zizmor

portability-macos
→ tool setup + cargo check + pnpm install/api lint

container-build
→ canonical Dockerfile verification

required-check
→ needs all jobs; fail if any required predecessor failed/cancelled
```

All jobs use GitHub-hosted runners only.

- [ ] **Step 4: Add macOS portability task**

Add:

```toml
[tasks.portability:macos]
run = [
  "cargo check --workspace --locked",
  "pnpm install --frozen-lockfile",
  "pnpm api:lint",
]
```

No full PostgreSQL/integration workload is required in this bootstrap plan.

- [ ] **Step 5: Strengthen architecture rules now that CI exists**

Change `architecture-lint` real-repository behavior so that `.github/workflows/ci.yml` is now required.

Also validate:
- no `self-hosted`;
- no native Windows runner;
- at least one `mise run` entrypoint;
- `permissions:` exists at workflow level.

Add focused failing/passing tests before the implementation change.

- [ ] **Step 6: Run CI linters locally**

```bash
mise run ci:lint
mise run verify:fast
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add .github/workflows/ci.yml spec/architecture/dependency-rules.toml tools/architecture-lint mise.toml
git commit -m "ci: add github-hosted verification gates"
```

---

### Task 7: Canonical Dockerfile, OCI verification, and SBOM-ready full gate

**Files:**
- Create: `Dockerfile`
- Modify: `mise.toml`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- One canonical root Dockerfile.
- Bootstrap image contains only bootstrap Rust tools, not application runtime.
- This plan does not claim this image is the final production application image.
- Full gate can build `linux/amd64` and generate an SBOM.

- [ ] **Step 1: Add a failing architecture test for Dockerfile proliferation**

Create a fixture with both `Dockerfile` and `Dockerfile.prod`.

Expected: `ARCH_DOCKERFILE_VARIANT`.

Run:

```bash
cargo nextest run -p architecture-lint
```

Expected: PASS after existing rule handles it.

- [ ] **Step 2: Create the canonical multi-stage Dockerfile**

Use a bootstrap tooling target only:

```dockerfile
# syntax=docker/dockerfile:1

FROM rust:1.98.1-bookworm AS rust-build
WORKDIR /src

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY tools ./tools
COPY spec ./spec

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --workspace --locked --release \
 && mkdir -p /out \
 && cp /src/target/release/architecture-lint /out/architecture-lint \
 && cp /src/target/release/assure /out/assure

FROM debian:bookworm-slim AS bootstrap-tools
RUN useradd --system --uid 10001 --create-home app
COPY --from=rust-build /out/architecture-lint /usr/local/bin/architecture-lint
COPY --from=rust-build /out/assure /usr/local/bin/assure
USER 10001
ENTRYPOINT ["assure"]
```

This is a bootstrap verification artifact. When the first application runtime is introduced, add its `runtime` stage to this same Dockerfile rather than creating a second Dockerfile.

- [ ] **Step 3: Add container tasks**

```toml
[tasks.container:verify]
run = [
  "docker buildx build --platform linux/amd64 --target bootstrap-tools --load -t knowledge-platform-bootstrap:local .",
  "docker run --rm knowledge-platform-bootstrap:local --help",
]

[tasks.sbom]
run = [
  "syft knowledge-platform-bootstrap:local -o cyclonedx-json=target/sbom.cdx.json",
  "syft knowledge-platform-bootstrap:local -o spdx-json=target/sbom.spdx.json",
]

[tasks.verify:full]
depends = ["verify", "container:verify", "sbom"]
```

Create `target/` output only; SBOMs are generated artifacts, not committed.

- [ ] **Step 4: Run full local verification**

```bash
mise run verify:full
```

Expected:
- OCI build succeeds;
- container starts as non-root and prints `assure --help`;
- CycloneDX/SPDX files are generated below ignored `target/`.

- [ ] **Step 5: Wire container-build CI job**

The GitHub-hosted Linux job runs:

```bash
mise run container:verify
```

The `required-check` must depend on it.

Do not publish the bootstrap image to a registry.

- [ ] **Step 6: Commit**

```bash
git add Dockerfile mise.toml .github/workflows/ci.yml
git commit -m "chore: add canonical oci verification path"
```

---

### Task 8: End-to-end bootstrap verification and documentation reconciliation

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `docs/design/repository-bootstrap-design-v0.md` only if implementation reality requires a non-semantic clarification
- Inspect: all root/tool/spec files created by Tasks 1–7

**Interfaces:**
- Produces a repository that is ready for separate PoC implementation plans.
- Does not start P0–P7 PoCs.

- [ ] **Step 1: Verify the complete local developer path from clean state**

From a fresh worktree/clone:

```bash
mise install
mise run bootstrap
mise run verify:fast
mise run verify
mise run verify:full
```

Expected: all PASS.

Record wall-clock durations for:
- `verify:fast`;
- standard `verify`;
- `verify:full`.

Do not fail the bootstrap solely because the target budgets are exceeded once; record the gap and optimize if the overage is structural.

- [ ] **Step 2: Verify Development Assurance vertical slice**

```bash
mise run assure:plan
mise run assure:run
mise run assure:report
```

Expected:
- requirements are extracted;
- architecture capability is selected;
- deterministic evidence is written;
- no LLM is invoked;
- generated state stays under ignored `target/assurance/`.

- [ ] **Step 3: Verify architecture-negative cases**

Temporarily introduce each violation in the worktree, verify `mise run arch:check` fails, then revert it:

```text
runs-on: self-hosted
runs-on: windows-latest
Dockerfile.prod
CI command bypassing mise where the policy requires mise
```

Do not commit the negative fixtures outside unit tests.

- [ ] **Step 4: Verify secret scanning with a synthetic fake pattern**

Use only a scanner-provided synthetic/test secret pattern or an obviously non-live fixture approved by Gitleaks documentation.

Expected: scan fails and output is redacted.

Remove the fixture immediately after the test.

- [ ] **Step 5: Verify repository policy**

Run the prohibited customer/institution proper-name scan defined by the external handoff manifest, then:

```bash
git status --short
```

Expected:
- zero prohibited proper-name matches;
- only intended files staged/changed.

Also verify no `.env`, key, DB, generated Assurance evidence, SBOM, or build output is tracked.

- [ ] **Step 6: Verify GitHub workflow static checks**

```bash
actionlint
zizmor .github/workflows
```

Expected: PASS.

- [ ] **Step 7: Final consolidated gate**

```bash
mise run verify:full
```

Expected: PASS.

- [ ] **Step 8: Commit final bootstrap reconciliation**

```bash
git add README.md AGENTS.md docs/design/repository-bootstrap-design-v0.md
git commit -m "docs: finalize repository bootstrap workflow"
```

Skip this commit if there are no documentation changes.

---

# Follow-up Plans — intentionally out of scope

The following each require their own spec-linked implementation plan after this bootstrap is merged/reviewed:

```text
P0 TypeScript 7 compatibility
P1 OpenAPI 3.2 codegen
P2 Windows Integrated Authentication
P3 UI foundation
P4 Japanese lexical search
P5 Office extraction
P6 PDF extraction
P7 Observability/Audit adapters

Development Assurance:
- Bounded Context Compiler
- Context Lease
- Agent Gateway
- MCP/Hook adapters
- Symbol-level Code Graph
```

This separation prevents Repository Bootstrap from turning into the application or the full Assurance platform.
