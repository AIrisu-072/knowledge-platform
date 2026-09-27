> **Status note (2026-09-24):** この文書は2026-09-14候補調査のスナップショットです。現在の採否判断は `library-tool-selection-v0.md` を正本とします。Document Semantic Inspection v0については末尾のPoC qualification updateを追加しています。

# Rust Library Matrix v0

- Project: Knowledge / Document Platform + Search Platform
- Status: v0 / library-selection entry
- Research snapshot: 2026-09-14
- Principle: **library-first / composition-first**
- Preferred language: Rust
- Fallback: Rustで要件を満たせない場合のみ他言語OSSを許容
- License preference: Apache-2.0 / MIT / BSD系
- Deployment preference: server/processを不必要に増やさない

---

# 1. Selection rules

各機能は以下の順で決める。

1. Rustの既存crateで要件を満たす
2. Rust crateを薄く組み合わせれば満たす
3. 他言語OSSを同一hostのchild process / libraryとして利用
4. 最後に不足部分のみ自作

「多機能だから除外」にはしない。
Cargo Featureで機能分割できる場合は `default-features = false` を基本に、必要機能だけ有効化する。

評価項目:

- License
- Maintenance / recent activity
- API maturity
- Rust MSRV
- Cargo feature granularity
- transitive dependency risk
- unsafe / native dependency
- security advisory履歴
- requirement coverage
- replacement difficulty

---

# 2. Matrix

Legend:

- **Adopt**: v0第一候補
- **PoC**: 有力だが実データ/実形式で検証必須
- **Alternative**: 第一候補が不足した場合
- **Defer**: 要件がまだ足りず選定しない
- **Avoid**: 現時点では採用しない

| Area | Requirement | Primary candidate | Lang | License | Status | Notes |
|---|---|---|---|---|---|---|
| Async runtime | async I/O / tasks | Tokio | Rust | MIT | **Adopt** | Rust server ecosystemの標準的基盤 |
| HTTP API | Document/Search common API | axum 0.8.9 | Rust | MIT | **Adopt** | modular、Tokio/Tower統合 |
| DB access | PostgreSQL transaction / pool / migrations | SQLx 0.9 | Rust | MIT / Apache-2.0 | **Adopt** | async、compile-time checked query、Postgres featureのみ有効化可能 |
| DB alternative | typed ORM/query builder | Diesel 2.3.x | Rust | MIT / Apache-2.0 | Alternative | ORMが必要になった場合。v0ではSQL visibilityを優先しSQLx |
| Serialization | API / event / metadata | serde / serde_json | Rust | MIT / Apache-2.0 | **Adopt** | de facto standard |
| File I/O | document binary storage | std / tokio::fs | Rust | Rust std / MIT ecosystem | **Adopt** | 初期は余計なstorage serverを導入しない |
| ZIP | recursive archive extraction | zip (zip-rs) | Rust | MIT | **Adopt** | featureを絞れる。nested archive防御はアプリ側policy必要 |
| Unified Office extraction | DOCX/XLSX/PPTX + DOC/XLS/PPT | office_oxide 0.1.8 | Rust | MIT / Apache-2.0 | **PoC priority** | 6形式を1crateで扱える。2026年登場の若いcrateなので実文書corpusで検証 |
| DOCX fallback | DOCX parsing | docx-rs 0.4.22 | Rust | MIT | Alternative | read/write対応。Office Oxide不足時の専用fallback |
| XLS/XLSX fallback | spreadsheet read | calamine 0.36.1 | Rust | MIT | Alternative | pure Rust、read-only用途に強い |
| OOXML low-level | custom extraction fallback | quick-xml 0.41.x + zip | Rust | MIT | Alternative | DOCX/PPTXの必要XMLだけ抽出する最終fallback |
| PPTX fallback | PPTX read | rust-pptx | Rust | MIT | PoC / Alternative | 軽量だがproject maturityがまだ低い |
| PDF | PDF parse / text extraction | lopdf 0.44.0 | Rust | MIT | **PoC** | active。抽出品質はreal-world corpusで評価必須 |
| Text search | lexical index / BM25 / snippet | Tantivy 0.26.2 | Rust | MIT | **Adopt candidate** | in-process、別search server不要 |
| Japanese analysis | Japanese tokenization for Tantivy | lindera-tantivy 5.0.1 | Rust | MIT | **Adopt candidate** | IPADIC / UniDic等をfeature選択可能 |
| Search fusion | RRF / weighted fusion | thin domain module | Rust | internal | **PoC** | algorithmic core。既存crateより交換可能traitを優先 |
| Reranking | cross-encoder / LLM rerank | 未選定 | 未選定 | 未選定 | **Defer** | model/latency/precision evaluation前に固定しない |
| Vector representation | embedding storage/index | 未選定 | 未選定 | 未選定 | **Defer** | Search representation v0/benchmark後に選定 |
| Windows auth | SSPI / Negotiate adapter | sspi-rs | Rust | MIT / Apache-2.0 | **PoC** | Kerberos/NTLM/Negotiate。Linux server + AD実環境で要検証 |
| Windows native fallback | Windows API bindings | windows-rs | Rust | MIT / Apache-2.0 | Alternative | server-side architecture次第 |
| App diagnostics | structured spans/events | tracing | Rust | MIT | **Adopt** | OpenTelemetry bridgeの入口 |
| Telemetry | OTLP traces/metrics/logs | opentelemetry-rust 0.32.x | Rust | Apache-2.0 | **Adopt** | signalsの成熟度差に注意 |
| Hash / integrity | file content hash | sha2 | Rust | MIT / Apache-2.0 | Adopt candidate | Content-addressing / integrity用。algorithmは後で固定 |
| UUID | resource/event IDs | uuid | Rust | MIT / Apache-2.0 | Adopt candidate | UUIDv7利用可否はmodelで決定 |
| API schema | OpenAPI generation | 未選定 | Rust | permissive only | Defer | API shape確定後 |
| Audit events | append-only audit contract | thin domain module | Rust | internal | **Adopt design** | audit semanticsは業務固有。transport/storeは別選定 |
| Outbox worker | claim/retry | SQLx + PostgreSQL primitives | Rust | MIT/Apache + PostgreSQL | **Adopt design** | generic queue productを追加せず開始 |

---

# 3. Recommended initial dependency direction

## 3.1 Backend core

```toml
# conceptual only

tokio
axum
serde
serde_json
sqlx        # postgres + runtime-tokio + migrate + uuid/json等必要分のみ
tracing
opentelemetry
```

方針:

- SQLxは `default-features = false`
- PostgreSQLのみ有効化
- DB抽象化のために「複数DBを同時サポート」はしない
- transaction invariantをSQLレベルで明示できることを重視する

---

## 3.2 Extraction

第一案:

```text
office_oxide
lopdf
zip
```

ただし **office_oxideはPoC Gateを通るまで採用確定しない**。

### Office Oxide PoC Gate

実際に想定される匿名化 / synthetic corpusで:

- DOCX
- DOC
- XLSX
- XLS
- PPTX
- PPT

について評価する。

指標:

- extraction success rate
- text fidelity
- table/sheet/slide structure preservation
- Japanese text correctness
- malformed file behavior
- decompression bomb / pathological input resistance
- memory peak
- processing latency
- container path / position情報をどこまで作れるか

不足した形式だけ専用crateへfallbackする。

例:

```text
office_oxide
   ├─ DOCX OK
   ├─ XLSX OK
   ├─ PPTX NG
   └─ PPTX -> rust-pptx / quick-xml fallback
```

「全形式を1ライブラリへ依存」ではなくExtractor traitで交換可能にする。

---

## 3.3 Search

初期案:

```text
Tantivy
  +
lindera-tantivy
```

理由:

- Rust in-process
- MIT
- 別search server不要
- BM25 / lexical retrieval
- search snippet/highlightを実装可能
- 日本語tokenizationを組み込める

固定しないもの:

- Vector index
- Fusion algorithm
- Reranker model

これらは検索評価基盤を作ってから選ぶ。

---

# 4. Cargo feature policy

Rust crate採用時は必ず以下を確認する。

1. `default-features` の内容
2. optional dependency
3. native dependency
4. network/cloud integrationが自動で入らないか
5. OCR / image / write support等の不要機能を切れるか
6. dependency license
7. RustSec advisories

基本形:

```toml
some_crate = {
    version = "...",
    default-features = false,
    features = ["required-only"]
}
```

ただしcrate側がfeature splitしていない場合は、利用側から任意moduleをcompile対象外にはできない。

---

# 5. License gate

Direct dependencyだけでなくtransitive dependencyも監査する。

許容方針:

- Apache-2.0
- MIT
- BSD-2-Clause / BSD-3-Clause
- PostgreSQL License
- Public Domain
- その他については個別承認

原則として避ける:

- GPL / AGPL / LGPL
- MPL-2.0
- SSPL
- BSL / source-available
- 独自利用制限

CIで `cargo-deny` 等によるdependency license / advisory gateを検討する。

---

# 6. Key uncertainties

## U1. Office extraction

最も大きい未確定要素。

`office_oxide` は要件への適合度が非常に高い一方、2026年に登場した若いライブラリであるため、金融機関の12年分文書へ適用する前にcorpus testが必須。

## U2. PDF extraction

PDFは生成元により内部構造差が大きい。

単一crateの「成功/失敗」ではなく:

```text
Native text extraction
 -> fallback
 -> OCR（将来）
```

というpipelineとして扱う。

OCRは現在のv0 mandatory requirementではない。

## U3. Windows Integrated Authentication

`sspi-rs` は有力だが、実際の:

- AD domain
- Kerberos / NTLM
- SPN
- Linux server
- browser

条件でPoCが必要。

Document/Search coreとはIdentityProvider traitで切り離す。

## U4. Vector / reranking

検索精度に大きく関わるが、先に特定libraryへ固定しない。

Evaluation Harnessで:

- lexical only
- vector
- hybrid
- fusion
- reranking

を比較して選ぶ。

---

# 7. Proposed module boundaries

```text
crates/
├─ document-domain
├─ document-storage
├─ document-repository
├─ extraction-core
├─ extractor-office
├─ extractor-pdf
├─ extractor-archive
├─ search-core
├─ search-tantivy
├─ identity-core
├─ identity-sspi
├─ audit-core
├─ observability
└─ api
```

これはdeployment分離を意味しない。

初期は1 Rust binaryへlinkしてよい。

```text
1 Linux Server
└─ 1 Rust Application
   ├─ Document Platform
   ├─ Search Platform
   ├─ Extraction
   ├─ Identity adapter
   └─ API
```

---

# 8. Immediate PoC order

1. **PostgreSQL + SQLx transaction PoC**
2. **Tantivy + Lindera Japanese search PoC**
3. **Office Oxide extraction corpus PoC**
4. **PDF extraction corpus PoC**
5. **SSPI / Windows Integrated Authentication PoC**
6. その後 Vector / Fusion / Reranking

この順序なら、確定済み要件を先に潰し、まだアルゴリズム要件が固まっていない領域へ早期にロックインしない。

---

# 9. Sources

- Axum: https://github.com/tokio-rs/axum
- Tokio: https://github.com/tokio-rs/tokio
- SQLx: https://github.com/transact-rs/sqlx
- Diesel: https://github.com/diesel-rs/diesel
- zip-rs: https://github.com/zip-rs/zip2
- Office Oxide: https://github.com/yfedoseev/office_oxide
- docx-rs: https://github.com/bokuweb/docx-rs
- Calamine: https://github.com/tafia/calamine
- quick-xml: https://github.com/tafia/quick-xml
- rust-pptx: https://github.com/hidemi-ito/rust-pptx
- lopdf: https://github.com/J-F-Liu/lopdf
- Tantivy: https://github.com/quickwit-oss/tantivy
- Lindera: https://github.com/lindera/lindera
- Lindera-Tantivy: https://github.com/lindera/lindera-tantivy
- sspi-rs: https://github.com/Devolutions/sspi-rs
- windows-rs: https://github.com/microsoft/windows-rs
- tracing: https://github.com/tokio-rs/tracing
- OpenTelemetry Rust: https://github.com/open-telemetry/opentelemetry-rust


---

# 10. Document Semantic Inspection v0 qualification update

Qualification evidence: `docs/superpowers/execution/document-semantic-inspection-v0-poc-report.md`.

| Area | Candidate / composition | Result | Scope |
|---|---|---|---|
| TXT | encoding_rs 0.8.41 + Unicode normalization | **Adopt for DSI v0** | strict text semantics |
| CSV | csv 1.4.0 | **Adopt for DSI v0** | explicit-delimiter table semantics |
| HTML | html5ever 0.39.0 + markup5ever_rcdom 0.39.0 | **Adopt for DSI v0** | script-free DOM semantics |
| DOCX | office_oxide 0.1.11 + raw OOXML sentinel | **Adopt for DSI v0** | format-native semantics/editorial evidence |
| XLSX/XLSM | rxls 0.1.3 + Calamine 0.36.1 + raw SpreadsheetML | **Adopt for DSI v0** | workbook semantics/differential oracle |
| VBA | ovba 0.7.1 + tree-sitter 0.25.10 + vendored VBA grammar `c691f237...` | **Adopt for DSI v0** | static inspection only |
| PPTX | office_oxide 0.1.11 + raw PresentationML oracle | **Adopt for DSI v0** | slide semantics/package coverage |
| PDF | pdfium-render 0.9.4/PDFium 7881 + lopdf 0.45.0 | **Adopt for DSI v0** | dual-engine semantic/structural inspection |
| Signatures | xml-sec 0.1.16 + cms 0.2.3 + x509-cert 0.2.5 + OpenSSL 0.10.81 | **Adopt for DSI v0** | explicit-trust/offline evidence |
| scraper 0.27.0 | wrapper candidate | **Avoid for this project** | MPL-2.0 transitive path |
| stemma/docx-review-core | DOCX candidates | **Avoid** | current RustSec findings |
| pptx 0.1.0 | PPTX candidate | **Avoid** | current RustSec findings |
| pkix-chain/path line | signature-chain candidate | **Avoid** | yanked/vulnerable dependency line |

This update does not qualify legacy DOC/XLS/PPT, Search Extraction, OCR, or conversion/rendition generation. Those remain separate PoCs/workloads.


---

# 11. Document Semantic Inspection v0 production sandbox update

Qualification evidence: `docs/superpowers/execution/document-semantic-inspection-v0-sandbox-preflight.md`.

| Area | Candidate / composition | Result | Scope |
|---|---|---|---|
| Filesystem confinement | landlock 0.4.7 | **Adopt for DSI v0 production sandbox** | ABI V3 hard-required read/write confinement |
| Syscall confinement | seccompiler 0.5.0 | **Adopt for DSI v0 production sandbox** | deny network and worker child-process syscalls |
| Resource/process primitives | libc 0.2.189 | **Adopt for DSI v0 production sandbox support** | RLIMIT + process-group supervision |
| Error contract | thiserror 2.0.21 | **Adopt for sandbox adapter/preflight** | typed fail-closed errors |

The selected composition passed the Ubuntu 24.04 sandbox contract and the repository cargo-deny advisory/license/source gate without exceptions. It remains isolated in the Task 1 experiment until promotion into the production sandbox runner task. This selection does not broaden Document Semantic Inspection semantics and does not qualify a non-Linux production sandbox.
