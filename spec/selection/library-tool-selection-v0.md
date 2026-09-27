# Library / Tool Selection v0

- 状態: v0
- 確認日: 2026-09-24
- 対象: Knowledge Platform
- 目的: 既に確定したArchitecture / Error / Observability / Frontend UX / Development・Container・CI要件から、実装ライブラリと開発ツールを選定する
- 原則:
  - ライブラリありきでArchitectureを変更しない
  - permissive licenseを必須とする
  - OSS / library / standard toolingを優先し、自作は最後の手段とする
  - Cargo / npmのfeatureは必要最小限のみ有効化する
  - 「選定済み」と「PoCが必要」を混同しない
  - versionはbootstrap時にlockfile / mise / toolchainへpinする
  - 本文中の一般候補versionは2026-09-14時点の評価基準。Document Semantic Inspection v0の選定は2026-09-24 PoC evidenceを正本とする

---

## 1. Decision status

| Status | 意味 |
|---|---|
| **SELECTED** | v0の標準として採用。bootstrap / implementation planへ進めてよい |
| **POC REQUIRED** | Architecture適合性は高いが、実データ・互換性・性能・成熟度をPoCで確認してから採用 |
| **DEFERRED** | 現段階では必要性が確定していない。Feature / workloadが具体化してから選定 |
| **REJECTED** | license、Architecture不適合、成熟度等の理由でv0では採用しない |

---

# 2. Cross-cutting selection constraints

## 2.1 License

許可:

```text
MIT
Apache-2.0
BSD-2-Clause
BSD-3-Clause
PostgreSQL License
Public Domain相当のpermissive license
```

原則不許可:

```text
MPL-2.0
GPL
AGPL
LGPL
SSPL
BSL / source-available
restrictive custom license
```

例外はArchitecture Decision Recordによる明示的承認を必要とする。

## 2.2 Dependency policy

Rust / TypeScript双方で:

- lockfile必須
- transitive licenseをCIで検査
- known vulnerabilityをCIで検査
- dependency sourceを検査
- default featureを無条件に受け入れない
- optional featureは利用要件がある場合のみ有効化
- production dependency追加は選定文書またはFeature PoCに根拠を持つ

## 2.3 Technology boundary

ライブラリはDomain / Applicationへ直接浸透させない。

```text
Domain / Application
      ↑ Port
Infrastructure Adapter
      ↓
Library / External Tool
```

Frontendも同様に:

```text
Application / View Model
      ↓
Presentation Adapter
      ↓
React / UI Primitive / Motion
```

---

# 3. Backend / Runtime / DB

## 3.1 Decision table

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Primary DB | PostgreSQL 18.x | **SELECTED** | PostgreSQL License | source-of-truth DB |
| Async runtime | Tokio 1.x | **SELECTED** | MIT | Rust async runtime |
| HTTP API | axum 0.8.x | **SELECTED** | MIT | Rust HTTP API |
| HTTP middleware | tower-http 0.7.x | **SELECTED** | MIT | CORS/trace/compression等、必要featureのみ |
| DB access | SQLx 0.9.x | **SELECTED** | MIT OR Apache-2.0 | PostgreSQL access / migrations |
| Serialization | serde / serde_json | **SELECTED** | MIT OR Apache-2.0 | transport / persisted JSON |
| Typed errors | thiserror 2.x | **SELECTED** | MIT OR Apache-2.0 | Domain/Application error型 |
| UUID | uuid 1.x | **SELECTED** | MIT OR Apache-2.0 | stable ID representation |
| Content hashing | sha2 0.11.x | **SELECTED** | MIT OR Apache-2.0 | FileObject等のSHA-256 |
| Windows auth adapter | sspi-rs (`sspi`) | **POC REQUIRED** | MIT OR Apache-2.0 | Negotiate / Kerberos / NTLM adapter |

PostgreSQL 18.6は2026-08-13公開の現行18系。PoC/implementationでは18.xの最新security patchを利用する。

### SQLx feature policy

初期候補:

```text
default-features = false
runtime-tokio
postgres
migrate
```

`uuid` / date-time / json等はDomain modelが必要とするものだけ追加する。

DB migrationはSQLx migrationsを第一候補とし、別migration frameworkを初期導入しない。

## 3.2 REJECTED / DEFERRED

| Candidate | Status | 理由 |
|---|---|---|
| Diesel | DEFERRED | SQLxで要件を満たす。ORM abstractionを先に増やさない |
| SQLite as production DB | REJECTED | ReadState / Outbox / Audit等を含むmulti-writer server workloadに不適 |
| YugabyteDB | DEFERRED | multi-node active HA / horizontal write要件が出た場合 |
| Turso Database | DEFERRED | mission-critical採用判断には成熟度確認が必要 |
| CockroachDB | REJECTED | 現行license policyに不適 |

---

# 4. Windows Integrated Authentication PoC

`sspi-rs` はcross-platform SSP実装とWindows native SSPI利用を提供するためArchitectureとの整合性は高いが、実環境検証を必須とする。

## PoC acceptance

実際のAD / browser / Linux server経路で:

- Edge / ChromiumからNegotiateが成立
- 正しいWindows principalを一意に取得
- 同一browser session内のidentity混線がない
- Kerberos優先 / NTLM fallbackの挙動を記録
- proxy / reverse proxy有無による差を確認
- failed authを別利用者へ誤帰属しない
- Linux deploymentで必要なKDC / SPN / keytab条件を文書化
- load / keep-alive時にconnection単位認証の副作用がないことを確認

PoC通過前にCore DomainへSSPI固有型を導入しない。

---

# 5. OpenAPI / JSON Schema / Codegen

## 5.1 Decision table

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| API Contract | OpenAPI 3.2.1 | **SELECTED** | specification | API SSOT |
| Structural Schema | JSON Schema 2020-12 | **SELECTED** | specification | validation/data schema SSOT |
| OAS lint/bundle | Redocly CLI 2.x | **SELECTED** | MIT | OpenAPI 3.2対応 |
| Rust runtime validation | `jsonschema` 0.55.x | **SELECTED** | permissive | Draft 2020-12 validator |
| TypeScript runtime validation | Ajv 8.x | **SELECTED** | MIT | Draft 2020-12 validator |
| JSON Schema → Rust | `typify` 0.7.x | **POC REQUIRED** | permissive | generated transport types candidate |
| JSON Schema → TypeScript | `json-schema-to-typescript` | **POC REQUIRED** | MIT | generated TS types candidate |
| OpenAPI → TS operations/types | `openapi-typescript` | **POC REQUIRED** | MIT | OAS 3.2使用subset互換性確認 |
| OpenAPI Generator | OpenAPI Generator | **REJECTED** | Apache-2.0 | 3.2非対応。3.1もbeta表記 |

## 5.2 Codegen policy

Canonical relation:

```text
OpenAPI 3.2.1
├─ operation / transport contract
└─ references
      ↓
JSON Schema 2020-12
├─ shared data model
└─ structural validation
```

生成対象候補:

```text
Rust transport DTO
TypeScript transport types
TypeScript API bindings
Error Code types
contract fixtures
```

Runtime structural validationは生成型だけに依存せず:

```text
Rust -> jsonschema
TS   -> Ajv
```

を正本とする。

JSON Schemaはconstraint languageなので、type generatorが全constraintをRust/TS type systemへ埋め込めないことを前提にする。

## 5.3 OpenAPI 3.2 Codegen PoC

PoC fixtureには最低限:

- `$ref`
- discriminated union / `oneOf`
- nullable semantics
- JSON Schema 2020-12 keywords
- RFC 9457 Problem Details
- JSON Pointer field errors
- streaming `itemSchema`
- `text/event-stream`
- `application/jsonl`
- `in: querystring`
- recursive / nested schemas

を含める。

Acceptance:

- deterministic codegen
- clean checkoutから生成可能
- Rust/TS双方がcompile
- runtime validatorとgenerated typeが矛盾しない
- OpenAPI 3.2 fieldをsilent dropしない
- generator update時のdiffをCIで検査可能

PoCに失敗した場合もOpenAPI 3.2.1を3.1へ落とさず、利用可能subset / generator境界を見直す。

自作generatorは最後の手段とする。

---

# 6. Observability / Audit

## 6.1 Decision table

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Rust structured instrumentation | `tracing` 0.1.x | **SELECTED** | MIT | Application instrumentation |
| Subscriber/filter/format | `tracing-subscriber` | **SELECTED** | MIT | structured logs / layers |
| OTel API/SDK | `opentelemetry` / `opentelemetry_sdk` 0.32.x | **SELECTED** | Apache-2.0 | Adapter境界内で利用 |
| OTLP exporter | `opentelemetry-otlp` 0.32.x | **SELECTED** | Apache-2.0 | OTLP export |
| tracing↔OTel bridge | `tracing-opentelemetry` 0.33.x | **SELECTED** | MIT | tracing span correlation |
| OTLP transport | HTTP/protobuf vs gRPC | **POC REQUIRED** | - | dependency量・運用性・性能比較 |
| CloudEvents Rust SDK | `cloudevents-sdk` 0.9.x | **POC REQUIRED** | Apache-2.0 | Audit envelope candidate |
| OpenTelemetry Collector distribution | - | **DEFERRED** | - | deployment/backend要件確定後 |
| Observability backend | - | **DEFERRED** | - | product selection later |
| Audit Store | - | **DEFERRED** | - | retention/tamper-evidence要件確定後 |

OpenTelemetry Rustは2026-09時点で公式statusがTraces / Metrics / LogsともBetaのため、Infrastructure Adapterに封じ込めversion pinを行う。

## 6.2 OTel PoC acceptance

- Trace ContextがHTTP → async task → outbox workerへ伝播
- RFC 9457 `trace_id`とspanが相関
- Collector停止時に業務transactionをblockしない
- sensitive fieldが自動instrumentationから漏れない
- bounded queue / shutdown flush挙動を確認
- selected transportでLinux OCI runtimeが安定

## 6.3 CloudEvents PoC acceptance

公式Rust SDKはCloudEvents 1.0を扱えるがAPIをWIP/unstableとしているため、DomainへSDK型を漏らさない。

Acceptance:

- v1.0 JSON Event Format round-trip
- Audit Event JSON Schemaとの整合
- stable `id/source/type/subject/time`
- duplicate/idempotency keyとしてevent idを利用可能
- SDK updateをAdapter内だけで吸収可能

---

# 7. Security / CI / Supply Chain

## 7.1 Decision table

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Tool/version/task manager | mise | **SELECTED** | MIT | local/CI task SSOT |
| Rust license/advisory/source | cargo-deny 0.20.x | **SELECTED** | MIT OR Apache-2.0 | Rust dependency policy |
| Cross-ecosystem vuln/license | OSV-Scanner | **SELECTED** | Apache-2.0 | repo/image dependency scanning |
| Secret scanner | Gitleaks CLI | **SELECTED** | MIT | pre-commit / pre-push / CI |
| Gitleaks GitHub Action | gitleaks-action | **REJECTED** | restrictive EULA | CLIを直接実行 |
| SBOM | Syft | **SELECTED** | Apache-2.0 | CycloneDX / SPDX |
| GitHub Actions syntax | actionlint | **SELECTED** | MIT | workflow lint |
| GitHub Actions security | zizmor | **SELECTED** | MIT | permission/injection/ref等 |
| Dependency update | Dependabot | **SELECTED** | GitHub service | Cargo/npm/Docker/Actions update PR |
| Artifact signing | Cosign | **DEFERRED** | Apache-2.0 | release signing要件時 |
| OCI build | Docker BuildKit / Buildx | **SELECTED** | permissive OSS components | canonical multi-stage build |

## 7.2 Gitleaks policy

利用するのは**Gitleaks CLI本体**。

```text
pre-commit
→ staged changes

pre-push
→ relevant history/diff

GitHub-hosted CI
→ authoritative scan
```

`gitleaks-action`はCLI本体とは別のrestrictive EULAを持つため使用しない。

## 7.3 GitHub Actions policy

- GitHub-hosted runner only
- default `permissions: contents: read`
- third-party Actionはfull-length commit SHA pin
- Actionを使う必要がないCLIはmise経由で直接実行
- `actionlint` + `zizmor` を両方実行
  - actionlint: syntax / expression / workflow correctness
  - zizmor: security-specific static analysis

## 7.4 License scanning

Rust:

```text
cargo-deny
```

Cross-ecosystem:

```text
OSV-Scanner --licenses=<SPDX allowlist>
```

を併用する。

許可listはproject license policyから生成/共有可能な構成にする。

## 7.5 SBOM

Syftで最低限:

```text
CycloneDX JSON
SPDX JSON
```

を生成可能にする。

PR毎の永続保存を必須にはせず、release artifactでは生成可能にする。

---

# 8. Frontend / UX

## 8.1 Core stack

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Runtime | Node.js 24 LTS | **SELECTED** | permissive | frontend build/test runtime |
| Package manager | pnpm 12.x core | **SELECTED** | MIT | frontend dependency management |
| Language | TypeScript 7.x | **POC REQUIRED** | Apache-2.0 | native compiler移行の互換性確認 |
| TypeScript fallback | TypeScript 6.x | **SELECTED fallback** | Apache-2.0 | TS7 PoC不成立時 |
| Renderer | React 19.x | **SELECTED** | MIT | Human UI |
| Build tool | Vite 8.x | **SELECTED** | MIT | SPA build/HMR |
| Router | TanStack Router | **SELECTED** | MIT | typed route/search params |
| Server state | TanStack Query | **SELECTED** | MIT | backend authoritative server cache |
| Table logic | TanStack Table | **SELECTED** | MIT | headless high-density tables |
| Virtualization | TanStack Virtual | **SELECTED** | MIT | large lists/tables |
| Motion | Motion | **SELECTED** | MIT | non-blocking perceptual motion |
| General client state | TanStack Store | **POC REQUIRED** | MIT | framework-agnostic state boundary |
| Workflow machine | XState | **DEFERRED** | MIT | complex workflow Featureのみ |
| UI primitive | Base UI | **POC REQUIRED** | MIT | React Ariaと比較 |
| UI primitive | React Aria Components | **POC REQUIRED** | Apache-2.0 | Base UIと比較 |
| Form library | - | **DEFERRED** | - | Feature要件が出てから |
| SSR/full-stack JS framework | Next.js / TanStack Start等 | **REJECTED v0** | varies | Rust Backendを唯一のserverとする |
| Styling framework | Tailwind等 | **DEFERRED** | - | v0で必須にしない |

### pnpm license boundary

pnpm coreはMITだが、同monorepoの`pnpr/`はPolyForm Shield source-available。
本projectでは**pnpm package manager coreのみ**利用し、`pnpr`は利用しない。

## 8.2 TypeScript 7 PoC

TypeScript 7.0は2026-07にstableになったnative Go portで大幅なtypecheck高速化を目的としているが、移行直後のため互換性PoCを行う。

Acceptance:

- React 19
- Vite 8
- TanStack Router/Query/Table/Virtual
- Motion
- selected UI primitive
- Vitest
- generated API types

がcompile/typecheckする。

既存tool/pluginがTS7非対応の場合、v0はTypeScript 6.xをpinし、Architectureは変更しない。

---

# 9. Frontend state boundary

## 9.1 Selected responsibility split

```text
URL / navigation state
= TanStack Router

Server authoritative state
= TanStack Query

General application/client state
= TanStack Store candidate (PoC)

Complex workflow state machine
= XState only when a Feature justifies it

Pure presentation state
= React-local state where appropriate
```

TanStack QueryのdataをTanStack Storeへコピーしない。

## 9.2 TanStack Store PoC

TanStack Storeはframework-agnostic adapter modelがFrontend UX Requirementに適合する一方、現時点でalpha表記のためPoC必須。

Acceptance:

- Store coreがReact importなしで成立
- React adapterだけでsubscription
- selector単位のrender範囲を制御可能
- unit testをDOMなしで実行可能
- Server Stateを重複保持しない
- 1,000+ rowのselection/state操作でperformance budgetを満たす
- alpha API変更をAdapterで吸収可能

不成立時は成熟したclient-state libraryを再比較する。

---

# 10. UI Primitive PoC: Base UI vs React Aria Components

どちらもheadless/unstyled寄りで、WCAG 2.2 AA・Keyboard/Focus要件に適合しやすい。

## PoC components

同一のOperational Design Systemで以下を両方実装する。

- Dialog / Modal
- Menu
- Combobox
- Select
- Tooltip
- Popover
- Tabs
- form field / error
- focus restoration
- disabled/read-only semantics

TanStack Tableは独立して利用し、primitive libraryのtable実装へ依存しない。

## Evaluation

- keyboard behavior
- focus management
- WAI-ARIA semantics
- React stateとの分離
- Motion integration
- styling freedom
- CSS overhead
- bundle cost
- API stability
- custom composition
- reduced motion
- testability

WCAG適合のための独自patchが少ない方を採用する。

---

# 11. Frontend testing / accessibility

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Unit/component test | Vitest 4.1.x | **SELECTED** | MIT | Vite 8対応済みの成熟minorを優先 |
| DOM/component interaction | React Testing Library | **SELECTED** | MIT | user-centric interaction testing |
| Browser E2E | Playwright | **SELECTED** | Apache-2.0 | keyboard/E2E/performance/visual |
| JSX static a11y | eslint-plugin-jsx-a11y | **SELECTED** | MIT | static checks |
| Automated WCAG checker | IBM Equal Access `accessibility-checker` | **POC REQUIRED** | Apache-2.0 | WCAG 2.2 A/AA、Playwright integration |
| axe-core / @axe-core/playwright | - | **REJECTED** | MPL-2.0 | project license policy不適合 |

## IBM Equal Access PoC

採用する場合、packageのIBM TelemetryはCI / developmentとも**明示的にopt-out**する。

Acceptance:

- WCAG 2.2 A/AA ruleset
- Playwright page integration
- deterministic CI failure
- no source/customer data送信
- telemetry opt-out確認
- manual accessibility testingと併用可能

Automated a11y testだけでWCAG適合を主張しない。

---

# 12. Styling / Design Tokens

v0では大規模CSS frameworkを必須にしない。

初期Preferred:

```text
CSS Custom Properties
+
CSS Modules / scoped component CSS
+
Design Tokens
```

理由:

- Operational Design Systemのsemantic tokenを直接表現
- framework lock-inを抑制
- Runtime JS依存を増やさない
- component primitiveを自由にstyling可能

具体的Token値はFeature/UI PoC後に確定する。

---

# 13. Search / Retrieval

## 13.1 Decision table

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Lexical index/search | Tantivy 0.26.x | **SELECTED** | MIT | in-process BM25/search |
| Japanese tokenizer | Lindera + lindera-tantivy | **POC REQUIRED** | MIT | Japanese corpus評価 |
| Vector retrieval | - | **DEFERRED** | - | evaluation後 |
| Embedding model/runtime | - | **DEFERRED** | - | evaluation後 |
| Reranker | - | **DEFERRED** | - | candidate recall評価後 |
| Graph retrieval | - | **DEFERRED** | - | concrete use case後 |
| Fusion | library/custom thin algorithm | **DEFERRED** | - | retrieval evaluationで決定 |

Searchの初期PoCはまずlexical retrievalを基準線とし、vector等を先に必須化しない。

---

# 14. Japanese tokenization PoC

比較:

- IPADIC
- IPADIC NEologd
- UniDic
- 必要ならuser dictionary

Corpus:

- 金融一般語彙を含むsynthetic / public documents
- 固有名詞
- 数値/日付
- カタカナ
- 英数字混在
- Office抽出後テキスト

測定:

- Recall@K
- MRR
- nDCG
- index size
- indexing throughput
- query latency
- dictionary binary size

辞書は「一般に良い」ではなくproject corpusで選ぶ。

---

# 15. Extraction

## 15.1 Document Semantic Inspection v0 — Office

2026-09-24のPoC qualificationで、以下を **Document Semantic Inspection v0用途に限定してSELECTED** とする。

| Candidate | Status | License | Qualified role |
|---|---|---|---|
| Office Oxide `office_oxide 0.1.11` | **SELECTED** | MIT OR Apache-2.0 | DOCX/PPTX typed semantic parser |
| `rxls 0.1.3` | **SELECTED** | MIT | XLSX/XLSM typed workbook semantics |
| Calamine `0.36.1` | **SELECTED** | MIT | spreadsheet differential oracle |
| `ovba 0.7.1` | **SELECTED** | MIT | static VBA project/source extraction |
| `tree-sitter 0.25.10` + vendored `tree-sitter-vba@c691f237...` | **SELECTED** | permissive / upstream grammar revision | strict VBA syntax/recovery gate |
| `zip 8.6.0` deflate-only | **SELECTED** | MIT | bounded OOXML package traversal |
| `quick-xml 0.42.0` | **SELECTED** | MIT | independent raw OOXML coverage/semantic oracle |

Selection scope is the frozen semantic-inspection contract. It does not assert that Office Oxide alone is a complete DOC/XLS/PPT legacy extractor.

### Rejected Office/PPTX candidates

| Candidate | Status | Reason |
|---|---|---|
| `stemma 0.5.0` | **REJECTED** | vulnerable Quick-XML line + legacy ZIP license conflict |
| `docx-review-core 0.1.1` | **REJECTED** | vulnerable Quick-XML line |
| `docxml 0.3.1` | **REJECTED** | ZIP codec graph outside project license allowlist |
| direct `tree-sitter-vba` git crate at `c691f237...` | **REJECTED as package** | missing referenced Rust binding build file; exact generated parser revision is vendored instead |
| `pptx 0.1.0` | **REJECTED** | Quick-XML 0.39.4 RustSec findings |
| `powerpoint-ooxml 1.0.0` | **REJECTED** | required OPC/ZIP dependency graph violates license policy |

## 15.2 Document Semantic Inspection v0 — TXT / CSV / HTML

| Candidate | Status | Qualified role |
|---|---|---|
| `encoding_rs 0.8.41` + `unicode-normalization` | **SELECTED** | strict TXT decoding/normalization |
| `csv 1.4.0` | **SELECTED** | explicit-delimiter CSV semantics |
| `html5ever 0.39.0` + `markup5ever_rcdom 0.39.0` | **SELECTED** | non-script HTML DOM semantics |
| `scraper 0.27.0` | **REJECTED** | transitive MPL-2.0 path under current license policy |

## 15.3 Scope boundary

The semantic-inspection PoC does **not** close the separate Search Extraction or legacy DOC/XLS/PPT corpus PoCs. Those workloads keep their own acceptance criteria and may choose different extraction libraries.

---

# 16. PDF / Digital Signature Evidence

## 16.1 Document Semantic Inspection v0 — PDF

| Candidate | Status | License | Qualified role |
|---|---|---|---|
| `pdfium-render 0.9.4` + PDFium `151.0.7881.0` | **SELECTED** | MIT OR Apache-2.0 wrapper; native artifact separately verified | reader-visible PDF semantics |
| `lopdf 0.45.0` | **SELECTED** | MIT | independent structural oracle |
| `pdf-extract 0.12.x` | **POC REQUIRED** | MIT | Search Extraction candidate only; not selected by this DSI PoC |
| OCR | **DEFERRED** | - | scan-only PDF is `RequiresOcr` in v0 |

PDFium artifacts are pinned to release `chromium/7881` and SHA-256 verified per Linux/macOS platform. Required PDFium/lopdf disagreement fails closed; the adapter never selects a winner heuristically.

## 16.2 Digital-signature evidence

| Candidate | Status | Qualified role |
|---|---|---|
| `xml-sec 0.1.16` | **SELECTED** | XMLDSig core/reference verification |
| `cms 0.2.3` | **SELECTED** | structured CMS parsing |
| `x509-cert 0.2.5` | **SELECTED** | Rust X.509 model/cross-check |
| `openssl 0.10.81` vendored | **SELECTED** | explicit X.509 chain verification and offline CRL validation |
| `pkix-chain 0.1.1` | **REJECTED** | yanked |
| `pkix-chain 0.4.1 + pkix-path 0.3.2` | **REJECTED** | `rsa 0.9.10` triggers RUSTSEC-2023-0071 |

Trust anchors/revocation material are caller-supplied and offline. No system trust or network CRL/OCSP/AIA retrieval is enabled.

---

# 17. File / Object Storage

初期source-of-truth binary adapterは:

```text
Domain FileStore Port
        ↑
Local filesystem adapter
(std::fs / tokio::fs)
```

を**SELECTED**とする。

`object_store` 0.14.xはMIT/Apache-2.0でlocal/S3/Azure/GCSを統一できるが、v0でcloud object storage要件がないため:

```text
object_store = DEFERRED
```

とする。

理由:

- Domain Portにより後から交換可能
- initial deploymentは同一Linux server
- 不要なcloud/HTTP/crypto dependencyを先に持ち込まない

S3-compatible storage等が具体化した時点で第一候補として再評価する。

---

# 18. Stable identifiers / content integrity

| Candidate | Status | Role |
|---|---|---|
| `uuid` | **SELECTED** | resource identifier |
| `sha2` / SHA-256 | **SELECTED** | immutable FileObject content hash |

UUID version strategy（v4/v7等）はdata model implementation時に決める。
hash algorithmの変更余地を残すため、persisted modelではalgorithm名を表現可能にすることを推奨する。

---

# 19. OSS Document Platform / Fork Policy

## 19.1 Evaluated full DMS candidates

| Candidate | Current license | Status |
|---|---|---|
| Mayan EDMS | GPL-2.0 | **REJECTED** |
| Paperless-ngx | GPL-3.0 | **REJECTED** |
| Docspell | AGPL-3.0-or-later | **REJECTED** |
| Papra | AGPL-3.0 | **REJECTED** |

いずれも現在のno-copyleft policyに適合しない。

したがってv0では:

> **Full DMSをforkして土台にする案は採用しない。**

これは「世界中に適合OSSが存在しない」という主張ではない。
今後、要件に近いpermissive OSSが見つかった場合は再評価する。

## 19.2 Reuse / fork order

新規機能では以下の順に検討する。

```text
1. normal dependency
2. 複数libraryのcomposition
3. thin extension / adapter
4. other-language OSS integration
5. custom implementation
```

大規模OSSを変更する必要がある場合:

```text
knowledge-platform repoへupstream全体をコピーしない
```

Separate fork:

```text
AIrisu-072/<upstream-fork>
```

を作成し、`knowledge-platform` から明示的にintegrationする。

`knowledge-platform` は以下のSSOTであり続ける。

- Architecture
- API Contract
- Integration
- business/domain logic
- Human UI
- Search orchestration
- Operational Design System

---

# 20. Version strategy

`SELECTED` は「常にlatestを自動採用する」という意味ではない。

Bootstrap時に:

```text
mise.toml
rust-toolchain.toml
Cargo.lock
pnpm-lock.yaml
packageManager field
Docker base digest
GitHub Action commit SHA
```

へ具体versionをpinする。

Update policy:

```text
Security patch
→ 優先

Patch/minor
→ automated PR可

Major
→ compatibility / architecture review
```

---

# 21. Cargo feature policy

Cargo dependencyは原則:

```toml
default-features = false
```

を起点に検討し、必要なfeatureだけ明示する。

ただし、default feature setが小さく安全で、無効化がmaintenance負担を増やす場合は理由を記録してdefaultを許容する。

Feature選択自体をPoC / build size / dependency treeで検証する。

---

# 22. Frontend package feature policy

npm packageでも同様に:

- package全体を入れる前にsubpath importを確認
- tree-shaking可能性を確認
- optional pluginを一括導入しない
- Runtime dependencyとDev dependencyを分離
- UI primitiveは必要componentから導入
- Motion+等のproprietary/premium packageをproduction dependencyにしない

---

# 23. PoC backlog / execution order

実装を始める前の高優先PoC:

## P0 — TypeScript 7 toolchain compatibility

```text
TypeScript 7
React 19
Vite 8
Vitest 4.1
TanStack stack
Motion
generated types
```

結果:
- PASS → TS7 pin
- FAIL → TS6 pin、Architecture変更なし

## P1 — OpenAPI 3.2 Codegen

Rust / TypeScript generated transport model + operation bindingsのcompatibility。

## P2 — Windows Integrated Authentication

`sspi-rs` を実AD / Linux deployment / browserで検証。

## P3 — UI foundation

- Base UI vs React Aria Components
- TanStack Store
- Motion non-blocking
- WCAG checker integration

## P4 — Japanese lexical search

Tantivy + Lindera辞書比較。

## P5 — Office extraction

Search Extraction / legacy Office corpusとして継続。Document Semantic Inspection v0のDOCX/XLSX/XLSM/PPTX qualificationは2026-09-24に完了済みだが、このPoCとは評価目的が異なる。

## P6 — PDF extraction

Search Extraction向けpdf-extract等のcorpus評価として継続。Document Semantic Inspection v0のPDFium + lopdf dual-engine qualificationは完了済み。

## P7 — Observability/Audit adapters

OTLP transport / CloudEvents SDK Adapter。

各PoCは`experiments/`配下でproduction codeと分離し、採否結果をADRまたはSelection updateへ反映する。

---

# 24. Selected baseline summary

現時点でimplementation planへそのまま持ち込めるbaseline:

```text
Backend
├─ PostgreSQL 18.x
├─ Tokio
├─ axum
├─ tower-http
├─ SQLx
├─ serde / serde_json
├─ thiserror
├─ uuid
└─ sha2

API / Validation
├─ OpenAPI 3.2.1
├─ JSON Schema 2020-12
├─ Redocly CLI
├─ jsonschema (Rust)
└─ Ajv (TypeScript)

Observability
├─ tracing
├─ tracing-subscriber
├─ opentelemetry
├─ opentelemetry_sdk
├─ opentelemetry-otlp
└─ tracing-opentelemetry

Dev / Security / CI
├─ mise
├─ cargo-deny
├─ OSV-Scanner
├─ Gitleaks CLI
├─ Syft
├─ actionlint
├─ zizmor
├─ Dependabot
└─ Docker BuildKit / Buildx

Frontend
├─ Node.js 24 LTS
├─ pnpm core
├─ React 19.x
├─ Vite 8.x
├─ TanStack Router
├─ TanStack Query
├─ TanStack Table
├─ TanStack Virtual
├─ Motion
├─ Vitest 4.1.x
├─ React Testing Library
├─ Playwright
└─ eslint-plugin-jsx-a11y

Document Semantic Inspection v0
├─ office_oxide 0.1.11 + raw OOXML oracle
├─ rxls 0.1.3 + Calamine 0.36.1
├─ ovba 0.7.1 + tree-sitter VBA syntax gate
├─ PDFium 151.0.7881.0 + lopdf 0.45.0
├─ html5ever 0.39.0 / csv 1.4.0 / encoding_rs 0.8.41
└─ xml-sec 0.1.16 + cms 0.2.3 + OpenSSL 0.10.81

Search / Storage
├─ Tantivy
├─ zip
├─ local filesystem adapter
├─ uuid
└─ sha2
```

`POC REQUIRED`の依存をproduction baselineへ追加してはならない。
PoC workspace / experimentに隔離する。

---

# 25. Rejected summary

```text
OpenAPI Generator
  → OAS 3.2未対応

gitleaks-action
  → restrictive EULA
  → MITのGitleaks CLIを直接実行

axe-core / @axe-core/playwright
  → MPL-2.0

Mayan EDMS
  → GPL-2.0

Paperless-ngx
  → GPL-3.0

Docspell
  → AGPL-3.0-or-later

Papra
  → AGPL-3.0

Document Semantic Inspection rejected candidates
  → scraper 0.27.0: MPL-2.0 transitive path
  → stemma 0.5.0 / docx-review-core 0.1.1: vulnerable Quick-XML
  → docxml 0.3.1 / powerpoint-ooxml 1.0.0: dependency license graph
  → pptx 0.1.0: vulnerable Quick-XML
  → pkix-chain 0.1.1: yanked
  → pkix-chain 0.4.1 + pkix-path 0.3.2: RUSTSEC-2023-0071 path

Native Windows dev
  → Architecture上非対応

JS SSR/full-stack server framework
  → Rust Backendを唯一のserverとするためv0では不採用
```

---

# 26. References

確認日: 2026-09-14

## Backend

- PostgreSQL 18.6 release notes  
  https://www.postgresql.org/docs/release/18.6/
- Tokio  
  https://docs.rs/tokio/
- axum  
  https://docs.rs/axum/
- SQLx  
  https://docs.rs/sqlx/
- tower-http  
  https://docs.rs/tower-http/
- thiserror  
  https://docs.rs/thiserror/
- sspi-rs  
  https://github.com/Devolutions/sspi-rs

## OpenAPI / Schema

- Redocly CLI  
  https://github.com/Redocly/redocly-cli
- jsonschema  
  https://docs.rs/jsonschema/
- Ajv  
  https://ajv.js.org/json-schema.html
- Typify  
  https://docs.rs/typify/
- openapi-typescript roadmap  
  https://github.com/openapi-ts/openapi-typescript/discussions/2559
- OpenAPI Generator  
  https://github.com/OpenAPITools/openapi-generator

## Observability

- OpenTelemetry Rust  
  https://opentelemetry.io/docs/languages/rust/
- tracing  
  https://docs.rs/tracing/
- opentelemetry-otlp  
  https://docs.rs/opentelemetry-otlp/
- tracing-opentelemetry  
  https://docs.rs/tracing-opentelemetry/
- CloudEvents Rust SDK  
  https://github.com/cloudevents/sdk-rust

## Security / CI

- mise  
  https://github.com/jdx/mise
- Gitleaks  
  https://github.com/gitleaks/gitleaks
- cargo-deny  
  https://github.com/EmbarkStudios/cargo-deny
- OSV-Scanner  
  https://github.com/google/osv-scanner
- Syft  
  https://github.com/anchore/syft
- actionlint  
  https://github.com/rhysd/actionlint
- zizmor  
  https://github.com/zizmorcore/zizmor
- Dependabot  
  https://docs.github.com/en/code-security/dependabot/

## Frontend

- React 19.3  
  https://react.dev/blog/2026/09/09/react-19-3
- TypeScript 7.0  
  https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/
- Node releases  
  https://nodejs.org/en/about/previous-releases
- pnpm  
  https://github.com/pnpm/pnpm
- Vite 8  
  https://vite.dev/blog/announcing-vite8
- TanStack Router  
  https://tanstack.com/router/latest
- TanStack Query  
  https://tanstack.com/query/latest
- TanStack Table  
  https://tanstack.com/table/latest
- TanStack Virtual  
  https://tanstack.com/virtual/latest
- TanStack Store  
  https://tanstack.com/store/latest
- Base UI  
  https://base-ui.com/react/overview/about
- React Aria Components  
  https://react-spectrum.adobe.com/react-aria/
- Motion  
  https://motion.dev/docs/react
- Vitest 4.1  
  https://vitest.dev/blog/vitest-4-1
- React Testing Library  
  https://github.com/testing-library/react-testing-library
- Playwright  
  https://playwright.dev/
- eslint-plugin-jsx-a11y  
  https://github.com/jsx-eslint/eslint-plugin-jsx-a11y
- IBM Equal Access  
  https://github.com/IBMa/equal-access

## Search / Extraction / Storage

- Tantivy  
  https://docs.rs/tantivy/
- Lindera Tantivy  
  https://docs.rs/lindera-tantivy/
- Office Oxide  
  https://docs.rs/office_oxide/
- Calamine  
  https://docs.rs/calamine/
- zip  
  https://docs.rs/zip/
- quick-xml  
  https://docs.rs/quick-xml/
- pdf-extract  
  https://docs.rs/pdf-extract/
- lopdf  
  https://docs.rs/lopdf/
- object_store  
  https://docs.rs/object_store/
- sha2  
  https://docs.rs/sha2/
- uuid  
  https://docs.rs/uuid/

## DMS license evidence

- Mayan EDMS LICENSE  
  https://gitlab.com/mayan-edms/mayan-edms/blob/master/LICENSE
- Paperless-ngx  
  https://github.com/paperless-ngx/paperless-ngx
- Docspell LICENSE  
  https://github.com/eikek/docspell/blob/master/LICENSE.txt
- Papra  
  https://github.com/papra-hq/papra

# 27. Development Assurance / Verification Tooling

本projectではDevelopment AssuranceをLLM-first Control Planeとして採用する。

| 領域 | Candidate | Status | License | Decision |
|---|---|---|---|---|
| Custom Assurance Core | Rust CLI / crates | **SELECTED** | project code | Compiler / IR / Planner / Evidence / Gap / Context Compiler |
| Custom Architecture Linter | Rust CLI | **SELECTED** | project code | Assurance Providerとして実装 |
| Rust test runner | cargo-nextest | **SELECTED** | Apache-2.0 / MIT | fast deterministic Rust test execution |
| Property testing | proptest | **SELECTED** | MIT OR Apache-2.0 | generated test cases |
| State-machine testing | proptest-state-machine | **POC REQUIRED** | permissive | Domain lifecycle / workflow |
| Rust fuzzing | cargo-fuzz / libFuzzer | **SELECTED** | MIT OR Apache-2.0 | parser/extractor/API fuzzing |
| Structured fuzz input | arbitrary | **SELECTED** | MIT OR Apache-2.0 | typed fuzz input generation |
| Model checking | Kani | **POC REQUIRED** | MIT OR Apache-2.0 | bounded formal verification |
| Mutation testing | cargo-mutants | **SELECTED** | MIT | scheduled/affected critical crates |
| TS property testing | fast-check | **DEFERRED** | MIT | frontend/domain TS property need発生時 |
| Code Graph L0-1 | repository tree + Cargo metadata + TS/build metadata | **SELECTED** | project code / existing metadata | v0 observed implementation graph |
| Symbol Code Graph | rust-analyzer/rustdoc/tree-sitter等 | **DEFERRED** | varies | v1以降、必要性/精度評価後 |
| Graph DB | - | **REJECTED v0** | - | generated index/IRで開始 |

## 27.1 LLM test generation policy

LLM-generated test codeはnormal CI pathへ入れない。

```text
static/formal
→ spec-driven generated cases
→ property
→ state-machine
→ fuzz
→ minimal regression
→ LLM-generated test code
```

LLMはGap / Counterexample分析に利用し、deterministic verifierをOracleとする。

## 27.2 Architecture lint policy

Custom Architecture LinterはRust CLIとして実装し、Assurance Providerとして接続する。

Rule sourceはmachine-readable Architecture Contractとし、lint本体へproject policyを無秩序にhard-codeしない。



---

# 28. Document Semantic Inspection v0 Production Sandbox

Qualification evidence: `docs/superpowers/execution/document-semantic-inspection-v0-sandbox-preflight.md`.

Scope is the Linux production sandbox substrate for the frozen `Document Semantic Inspection v0` trust boundary only. Selection here does not add these crates to a production crate yet; Task 1 qualification remains isolated under `experiments/document-semantic-inspection-sandbox/` until the approved implementation sequence promotes the composition.

| Candidate | Status | Qualified role | Evidence |
|---|---|---|---|
| `landlock 0.4.7` | **SELECTED for DSI v0 sandbox** | filesystem read/write confinement with ABI V3 hard requirement | hosted Ubuntu filesystem allow/deny contract PASS; fail-closed when required Landlock enforcement is unavailable |
| `seccompiler 0.5.0` | **SELECTED for DSI v0 sandbox** | seccomp-BPF denial of network syscalls and production child-process creation | hosted Ubuntu TCP/UDP/DNS and fork/clone contract PASS |
| `libc 0.2.189` | **SELECTED for DSI v0 sandbox support** | RLIMIT CPU/address-space/file-size, process groups, kill/wait primitives | hosted resource-limit and whole-process-group termination contract PASS |
| `thiserror 2.0.21` | **SELECTED for preflight error contract** | typed fail-closed sandbox/preflight errors | dependency gate PASS |

Task 1 acceptance evidence:

- fresh process per inspection: PASS;
- credential-like environment inheritance: denied;
- network socket access: denied;
- filesystem access outside explicit read/write surface: denied;
- production child-process creation: denied;
- wall timeout kills the process group;
- aggregate private-temp disk monitoring: PASS;
- CPU / address-space / per-file output limits: PASS;
- every frozen `ProductionResourceProfile::DSI_V0` class has an explicit finite boundary;
- independent Cargo lock is committed for reproducible preflight resolution;
- cargo-deny advisories / bans / licenses / sources: PASS with no policy exception.

Production support is Linux-first. macOS remains a semantic/parser portability target and is not selected as the v0 production sandbox substrate.
