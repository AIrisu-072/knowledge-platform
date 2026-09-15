# Knowledge / Document Architecture v0

## 1. 図の読み方

本図は、構成要素の一覧ではなく **データと問い合わせの流れ** を基準に整理している。

大きく4つの流れに分ける。

1. **文書管理フロー** — 社内文書の正本を管理する
2. **Indexing / ETL フロー** — 各データソースを検索可能な表現へ変換する
3. **Query / Retrieval フロー** — 人間・LLMが同じSearch APIから横断検索する
4. **Observability / Audit** — 全体の監査・デバッグ・性能評価を行う

---

## 2. A. 文書管理フロー

Document Platform は、企業内で業務上作成・発信する文書の **Source of Truth（正本）** を管理する。

主な責務:

- 文書登録
- 版管理
- フォルダ / カテゴリ
- metadata
- 既読 / 未読
- access policy

人間は React / TypeScript のUIを利用するが、UI固有APIは作らず、LLM / Agentと共通のDocument APIを利用する。

初期の物理構成は、別サーバを増やさず以下を想定する。

- Rust backend
- Metadata Store / DB: **未選定**
- filesystem
- React / TypeScript の静的ビルド

---

## 3. データモデルとDB選定

DBは現時点では固定しない。

先に、Document Platform と Search Platform が保持すべきデータ、整合性、更新パターン、検索・監査要件を定義し、その結果から永続化方式を選定する。

特に先に決める対象は以下。

- Document / DocumentVersion の識別と版管理
- Folder / Category / Metadata の持ち方
- Read / Unread の単位（ユーザー × 文書版）
- Access Policy / Windows Identity との紐付け
- Audit Event の保持要件
- Search Index を再生成可能な派生データとして扱う境界
- Knowledge Source / Canonical Knowledge Model の識別子と provenance
- 同時更新、トランザクション、一貫性の必要水準
- 文書数・版数・利用者数・更新頻度・保持期間の概算

その後、例えば次の候補を比較する。

- Embedded / local DB
- PostgreSQL 等のRDBMS
- KV / document-oriented store
- filesystem + DB の組み合わせ

**DB製品を先に決めてデータモデルを合わせるのではなく、データモデルと運用要件からDBを選ぶ。**

---

## 3. B. Indexing / ETL フロー

Search Platform はKnowledge Sourceの正本を所有しない。

Knowledge Sourceの例:

- Document Platform
- e-Gov法令等の外部Knowledge
- 内部DB / API
- 将来追加するデータソース

これらをSource Adapterから取り込み、検索用に変換する。

```text
Knowledge Source
    ↓
Source Adapter / ETL
    ↓
Extraction / Normalization
    ↓
Canonical Knowledge Model
    ↓
Search Representations / Indexes
```

### Extraction

Office / PDF / ZIPなどの内部情報を事前に抽出する。

これは「二段階検索」ではなく、**1回の検索でファイル内部まで検索できるようにするためのIndexing前処理**である。

### Canonical Knowledge Model

検索表現を完全固定しない。

共通で必要な薄い項目だけ固定し、データソース固有項目は拡張metadataとして保持する。

想定する共通項目:

- source_id
- resource_id
- version_id
- title
- content
- language
- content_type
- created / modified / effective time
- provenance
- locator
- access_scope
- metadata

### Search Representations

同じKnowledgeから複数の検索表現を生成できる。

- Lexical
- Vector
- Metadata
- Temporal
- Graph（必要になった場合）

---

## 4. C. Query / Retrieval フロー

人間・LLM・LLM Chatは、すべて同じSearch APIを利用する。

```text
Human / LLM / Agent
    ↓
Search API
    ↓
Retrieval Planner / Policy
    ↓
複数 Retriever
    ↓
Fusion
    ↓
Reranking
    ↓
SearchResult[]
```

### Retrieval Planner / Policy

問い合わせの意図に応じて、以下を決める。

- 使用するRetriever
- top_k
- filter
- lexical / semantic / temporal等の重み
- Fusion strategy

LLMをQuery Plannerとして利用することは可能だが、検索を自由実行させるのではなく、**構造化したRetrieval Planを出力させ、Policy / Validatorで検証してから実行**する。

### Retrieval

候補Recallを担当する。

例:

- Lexical / BM25
- Semantic / Vector
- Structured / Metadata
- Temporal
- Graph（将来）

### Fusion / Reranking

検索精度の中心となる層。

- Fusion: 複数Retrieverの候補を統合
- Reranking: 候補集合を精密に順位付け

ただし、RerankerはRetrieverが取得できなかった正解文書を復活させられないため、評価は各層で分離する。

---

## 5. RAGの位置づけ

RAGは独立した検索システムではない。

```text
Search API
    ↓
SearchResult[]
    ↓
LLM Contextへ投入
    ↓
Generation
```

この利用形態になった場合にRAGとなる。

したがって、人間向け検索とLLM向けRAGで検索基盤を二重化しない。

---

## 6. SearchResultの共通形式

人間UIとLLM / Agentで同じ検索結果形式を利用する。

最低限以下を想定する。

- source
- resource_id
- version_id
- title
- snippet
- highlights
- score
- metadata
- locator
- provenance

UIはこの結果を人間向けに表示するだけであり、別の検索ロジックを持たない。

---

## 7. 評価・デバッグ

検索品質は一つの「検索精度」という数字だけで評価しない。

```text
Document / Source Quality
    ↓
Source Adapter
    ↓
Extraction
    ↓
Indexing / Representation
    ↓
Retrieval
    ↓
Fusion
    ↓
Reranking
```

各段階で正解データが存在するかを追跡可能にする。

例:

```text
Document Platform       PASS
Extraction              PASS
Lexical Top100          rank 14
Vector Top100           rank 62
Fusion Top50            rank 8
Reranking Top10         FAIL
```

これにより、Document Platform側のデータ品質を直すべきか、Search Platformのアルゴリズムを直すべきかを切り分けられる。

---

## 8. Observability / Audit

### Observability

各コンポーネントはOpenTelemetry形式で以下を出力する。

- logs
- traces
- metrics

OpenTelemetry Collectorへ集約し、Observability Platformで分析する。

特にSearch Platformでは、一回のQueryを同一trace_idで追跡できるようにする。

### Audit

監査ログはObservabilityログと責務を分ける。

例:

- document.read
- document.create
- document.publish
- document.version.create
- search.execute
- search.result.open

監査用のAudit Storeへ保存する。

---

## 9. 初期実装の原則

### 論理分離・物理統合

Architectureとしては、Document / Search / Extraction / Identity / Observabilityを分ける。

ただし初期はサーバコストを抑えるため、可能な限り同一Linux Server上のモジュラーモノリスとして配置する。

```text
1 Linux Server
├─ Rust Application
│  ├─ document module
│  ├─ search module
│  ├─ extraction module
│  ├─ identity adapter
│  └─ telemetry
├─ Metadata Store / DB（未選定）
├─ Tantivy等の検索Index候補
├─ Document Files
└─ React / TypeScript static UI
```

負荷や運用上の必要が生じたコンポーネントのみ、後から分離できる構造とする。
