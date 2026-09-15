# リポジトリ初期化・開発基盤設計 v0

- 日付: 2026-09-14
- 対象リポジトリ: `AIrisu-072/knowledge-platform`
- 状態: 承認済み設計 / 実装前
- 対象範囲: リポジトリ構成、規範文書、Rust開発基盤、CI/Lint、OSS取り込み方針
- 前提: 特定企業の固有名詞は使用しない。金融業務での利用を想定した一般化された設計とする。

---

## 1. 目的

`knowledge-platform` を、仕様駆動・Rust優先・ライブラリ優先で開発できるリポジトリとして初期化する。

初期段階から以下を保証する。

- Document Platform / Search Platform の責務境界を維持する
- `spec/` を実装が従う規範文書のSSOTとする
- 調査・説明・意思決定履歴を `docs/` に分離する
- PoCと本番実装を分離する
- Architecture Contract違反をCIで検出できるようにする
- License / Security / Dependency方向を機械検査する
- 採用未確定のライブラリを先走ってproduction dependencyへ入れない
- 既存OSSを最大限再利用し、自作範囲を最小化する

---

## 2. リポジトリ構成

```text
knowledge-platform/
├─ spec/                          # 規範。実装はここに従う
│  ├─ architecture/
│  │  ├─ architecture-contract-v0.md
│  │  ├─ system-architecture-v0.md
│  │  ├─ system-architecture-v0.d2
│  │  └─ dependency-rules.toml
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
│  ├─ operations/
│  │  ├─ error-handling-resilience-requirements-v0.md
│  │  └─ observability-audit-requirements-v0.md
│  │
│  ├─ api/
│  │  └─ README.md
│  │
│  └─ requirements/
│     └─ requirements-v0.md
│
├─ docs/                          # 非規範。説明・調査・意思決定履歴
│  ├─ adr/
│  ├─ research/
│  └─ operations/
│
├─ experiments/                   # PoC。本番コードと隔離
│  └─ README.md
│
├─ crates/                        # Production Rust crates
├─ apps/                          # Human UI等
│
├─ third_party/                   # 必要な場合のみ。小規模vendoring用
│  └─ README.md
│
├─ tools/
│  └─ architecture-lint/
│
├─ .github/
│  └─ workflows/
│     └─ ci.yml
│
├─ Cargo.toml
├─ rust-toolchain.toml
├─ mise.toml
├─ deny.toml
├─ .editorconfig
├─ .gitignore
└─ README.md
```

---

## 3. `spec/` と `docs/` の役割

### `spec/`

規範文書を置く。

例:

- Architecture Contract
- System Architecture
- Logical Data Model
- Transaction & Consistency Requirements
- DB Selection Criteria
- Rust Library Matrix

実装が `spec/` と矛盾する場合、実装側ではなく先に仕様を変更する。

### `docs/`

以下を置く。

- ADR
- 調査結果
- PoC結果
- 運用文書
- なぜその仕様になったかの説明

`docs/` は `spec/` を上書きしない。

---

## 4. OSS再利用・Fork方針

### 4.1 基本原則

本プロジェクトは **library-first / composition-first** とする。

優先順位:

```text
1. 既存Rust crateをそのまま利用
2. 複数の既存Rust crateを組み合わせる
3. 既存OSSを薄く拡張する
4. Rust以外のOSSを利用する
5. 要件を満たすものがない部分だけ自作する
```

### 4.2 「OSSを使う」ことと「このリポジトリへ全コードを入れる」ことは分ける

OSSを採用する場合でも、巨大なupstreamソースを無条件にこのリポジトリへコピーしない。

採用方法は次の順で検討する。

#### A. 通常dependency — 第一選択

```text
Cargo dependency
npm dependency
```

upstreamをそのまま利用できる場合。

#### B. 自社Forkを別リポジトリで維持 — 大規模OSSの推奨

Document Management OSS等を大きく改修する必要がある場合。

```text
upstream OSS
     ↓ fork
AIrisu-072/<fork-repository>
     ↓ pinned dependency / submodule / integration
knowledge-platform
```

メリット:

- upstreamとの差分を追いやすい
- upstream merge / rebaseがしやすい
- 本体リポジトリの履歴を汚しにくい
- OSS固有のCIとPlatform側CIを分離できる

#### C. `third_party/` へvendor

以下の場合のみ許可する。

- 小規模ライブラリ
- upstream追従頻度が低い
- 数ファイル程度の変更
- build reproducibilityのためsourceを固定したい
- fork repositoryを別管理するほどではない

#### D. OSSのコードを参考に再実装

ライセンス条件・アーキテクチャ不整合・過剰依存等によりそのまま利用できない場合のみ検討する。

---

## 5. Document Management OSSをForkする場合

Document Platformとして利用できる高品質なOSSが見つかった場合、

**そのOSSを利用・Forkして統合すること自体は本プロジェクトの範囲内**とする。

ただし原則として、

```text
knowledge-platform/
```

へ巨大なupstream全部を直接vendorするのではなく、

```text
別Fork repository
        +
knowledge-platform側のAdapter / Integration
```

を優先する。

`knowledge-platform` は全体アーキテクチャ・API・統合・契約のSSOTであり、
OSS本体のupstream historyまで抱えることを目的にしない。

### 例

```text
knowledge-platform
├─ Document API
├─ Search API
├─ Integration Adapter
└─ Architecture Contract

forked-document-core
└─ upstream Document OSS + 必要な改修
```

もしOSSがcrateとして十分分割されているならFork自体を行わず、必要なcrateだけ利用する。

---

## 6. Rust Workspace方針

初期化時点で、本番用の空crateを大量に作らない。

最初に必要なのは:

```text
tools/architecture-lint
```

のみ。

以下のようなproduction crateは、実装計画で必要性が確定してから追加する。

```text
document-domain
document-application
search-core
search-application
...
```

---

## 7. Architecture Contractの機械検査

### 7.1 Machine-readable SSOT

`spec/architecture/dependency-rules.toml` を依存方向の機械可読SSOTとする。

将来的な例:

```text
document-domain -> sqlx       禁止
document-domain -> axum       禁止
document-domain -> tantivy    禁止

search-core -> tantivy        禁止
search-tantivy -> search-core 許可
```

### 7.2 Bootstrap時に検査するもの

- 必須specファイルが存在する
- `spec/` と `docs/` の重複規範がない
- dependency-rulesがparse可能
- experimentsがproduction dependencyになっていない
- architecture lintが空workspaceでも成功する

---

## 8. `mise` と開発コマンド

人間・CI共通の入口を `mise` に統一する。

初期task:

```text
mise run fmt
mise run lint
mise run test
mise run arch:check
mise run deny
mise run verify
```

### `verify`

以下を順番に実行する。

1. format
2. clippy
3. tests
4. architecture lint
5. dependency / license / advisory check

ローカルとCIで同じ `verify` を使用する。

---

## 9. Rust Quality Gate

### Format

```bash
cargo fmt --all -- --check
```

### Clippy

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Test

```bash
cargo test --workspace
```

### Dependency / License

`cargo-deny` を利用する。

対象:

- License allowlist
- RustSec advisory
- source restrictions
- duplicate dependencies（必要な範囲）

---

## 10. CI

初期GitHub Actionsは小さく保つ。

```text
checkout
toolchain setup
mise setup
mise run verify
```

required check名は安定した単一名:

```text
verify
```

とする。

PostgreSQL、Browser、Search Index等の専用CIは、PoCまたは本番実装が始まった時点で追加する。

---

## 11. PoC隔離

`experiments/` のコードは本番コードではない。

ルール:

- production crateから依存禁止
- 成功したPoCは先にselection文書 / ADRへ結果を反映
- 採用決定後にproduction contractに沿って実装する
- PoCコードをそのままコピーしてproduction化しない

今後の候補:

```text
postgres-transaction-poc
tantivy-lindera-poc
office-extraction-poc
pdf-extraction-poc
sspi-auth-poc
```

---

## 12. Branch / PR方針

既存の厳格な開発運用を参考にするが、機械的にコピーしない。

基本:

- focused feature / chore branch
- Draft PRで開始可能
- acceptance + CI greenでReady
- PRに以下を記録:
  - 目的
  - 対象範囲
  - 変更したspec
  - 検証結果
  - 監査引渡し時のexact HEAD

新規repoで `main` しかない場合、
`development` branchを導入するかは明示的に決める。
既存repoの慣習だけを理由に自動追加しない。

---

## 13. README

初期READMEは以下のみを扱う。

- プロジェクトの一般的な目的
- Document Platform / Search Platformの概要
- `spec/` が規範であること
- `mise` の導入方法
- `mise run verify`
- 実装はまだ選定・PoC段階であること

顧客固有名詞・内部情報は記載しない。

---

## 14. 金融用途を想定した初期セキュリティ方針

Bootstrapで最終セキュリティ設計までは行わない。

ただし最初から:

- secretをcommitしない
- `.env*` をignore
- real customer / financial dataをfixtureに使わない
- test dataはsynthetic / sanitized
- dependency advisoryをCI検査
- AuditとObservabilityを分離
- permissive-license gateをCIに入れる

---

## 15. Bootstrapでは行わないこと

まだ行わない:

- PostgreSQLの本採用確定
- SQLxをproduction dependencyへ追加
- Tantivy / Linderaの本採用
- Office extractorの本採用
- Windows Integrated Authentication実装
- React / TypeScript UI構築
- 空のproduction crate大量作成
- DB migration
- Document/Search API実装
- Docker / Compose導入
- 大規模OSSの無条件vendor

---

## 16. 完了条件

Repository Bootstrap完了条件:

1. 承認済みv0仕様が `spec/` に格納済み
2. 顧客固有名詞が存在しない
3. Cargo workspaceがvalid
4. Rust toolchainが固定済み
5. `mise run verify` が成功
6. fmt / clippy / test / cargo-deny / architecture lintがverifyに含まれる
7. architecture lintにmachine-readable rulesが存在
8. CIが同一 `verify` を実行
9. `experiments/` がproductionから隔離される
10. OSS fork / vendor方針が明文化済み
11. READMEに開発入口が記載済み
12. verification後にworking treeがclean
13. exact HEADで監査引渡し可能

---

## 17. 実装順序

```text
1. 承認済みspecを配置
2. root governance / toolingを追加
3. 最小Cargo workspaceを作成
4. architecture-lintのstructural check実装
5. cargo-deny設定
6. mise task設定
7. GitHub Actions設定
8. local verify
9. initialization branch push
10. CI確認
11. exact HEAD / verification evidenceで引渡し
```

## 18. 横断仕様を先に固定する順序

実装ライブラリ選定より前に、実装非依存の横断仕様を固定する。

```text
Architecture / Data / Transaction
        ↓
Error Handling & Resilience
        ↓
Observability & Audit
        ↓
Frontend / UX
        ↓
Development / Container / CI
        ↓
Library / Tool Selection
        ↓
PoC
```

この順序により、OpenTelemetry SDK、Audit Store、Frontend State Library、
Container tool等を「使いたいライブラリ」ではなく要求仕様から評価する。

## 19. Development / Container / CI v0 確定

開発基盤は以下を前提とする。

```text
Development:
  Linux / macOS / Windows via WSL2

Task SSOT:
  mise

CI:
  GitHub-hosted runners only

Production:
  linux/amd64 OCI image

Container:
  external dependencies
  reproducible verification
  deployment artifact
```

ライブラリ・tool選定は横断仕様確定後に行う。

## 20. Library / Tool Selection v0

ライブラリ・ツールの現在の採否判断は以下を正本とする。

```text
spec/selection/library-tool-selection-v0.md
```

`SELECTED` のみがproduction bootstrapへ導入可能。
`POC REQUIRED` は `experiments/` 配下のPoCを通過するまでproduction dependencyへ追加しない。

高優先PoC:

```text
P0 TypeScript 7 compatibility
P1 OpenAPI 3.2 Codegen
P2 Windows Integrated Authentication
P3 UI foundation
P4 Japanese lexical search
P5 Office extraction
P6 PDF extraction
P7 Observability/Audit adapters
```

## 21. Development Assurance v0

Repository Bootstrapでは、将来のDevelopment Assuranceを阻害しない構成を採用する。

初期配置候補:

```text
spec/
  assurance/
  architecture/

tools/
  architecture-lint/
  assurance/

target/assurance/
  generated only
```

v0 bootstrapで最初から巨大なAssurance Engineを完成させない。
まずArchitecture Provider / Requirement extraction / Level 0-1 Code Graph / Planの最小vertical sliceを作り、実projectで育てる。

Development Assuranceの規範:

```text
spec/architecture/development-assurance-architecture-v0.md
```

