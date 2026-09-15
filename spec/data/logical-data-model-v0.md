# Knowledge / Document Platform 論理データモデル v0

## 0. 前提

本モデルは DB 製品非依存の論理モデルである。DB 選定はこのモデルと非機能要件から後続で行う。

固定原則:

- Document Platform は社内文書の正本を管理する。
- Search Platform は正本を所有せず、検索のための派生データを保持する。
- 人間と LLM / Agent は同一 API を利用する。
- 既読状態は **Principal × DocumentVersion** で保持する。
- DocumentVersionの永続化するライフサイクル状態は **WORKING / PUBLISHED / WITHDRAWN** の3状態に限定する。
- 「下書き / 非公開 / 公開待ち / 現行版 / 過去版 / 公開終了」は状態・属性・current versionから導出し、DBへ重複保存しない。
- 通常業務ではDocument / DocumentVersionを論理削除・物理削除しない。
- Principal は新規ユーザー管理を作らず、Windows 統合認証等の外部 Identity Provider から得る安定識別子を用いる。
- 検索 index は壊れても Knowledge Source から再構築可能でなければならない。
- Audit / Observability は業務データとは別責務とする。

---

# 1. データ分類

## 1.1 Authoritative Data（正本）

Document Platform が所有する。

- Document
- DocumentVersion
- FileObject / VersionFile
- Folder
- Category / Tag
- Document / Version Metadata
- ReadState
- AccessPolicy（将来拡張を含む）

## 1.2 Derived Search Data（再生成可能）

Search Platform が保持する。

- KnowledgeResourceSnapshot
- KnowledgeUnit
- Lexical representation / index
- Vector representation / index
- Metadata / structured index
- Temporal representation
- Graph representation（将来）

## 1.3 Operational Data

- SourceSyncState
- IndexState
- ExtractionState
- RebuildState

## 1.4 Audit / Observability

別基盤で扱う。

- AuditEvent
- OpenTelemetry logs / traces / metrics
- Search pipeline trace

---

# 2. Document Platform

## 2.1 ExternalPrincipalRef

新しいユーザーマスタではなく、外部 Identity Provider のユーザーを参照するための値。

| Field | Meaning |
|---|---|
| identity_provider | 例: `windows-ad` |
| principal_id | 安定した外部識別子。Windows SID 等を優先 |
| display_name | 表示用。正本識別子として使用しない |

論理キー:

`(identity_provider, principal_id)`

> Windows ユーザー名変更に影響されないよう、可能なら SID 等の不変 ID を採用する。

## 2.2 Folder

| Field | Meaning |
|---|---|
| folder_id | 内部 ID |
| parent_folder_id | 親 Folder。root は null |
| name | フォルダ名 |
| status | active / archived 等 |

関係:

- Folder 1 : N Folder
- Folder 1 : N Document

`path` は原則派生値とし、正本 ID として使用しない。

## 2.3 Document

文書という論理的な同一性を表す。版が変わっても Document は同じ。

| Field | Meaning |
|---|---|
| document_id | 文書 ID |
| folder_id | 所属 Folder |
| current_version_id | 現行版への参照。公開中の版がない場合は null を許容可能 |
| revision | Optimistic Concurrency Control用の更新番号 |
| created_at | 文書作成日時 |

Document 自体には、版ごとに変わり得る本文・ファイルを持たせない。

関係:

- Document 1 : N DocumentVersion
- Document N : 1 Folder

## 2.4 DocumentVersion

文書の特定版。公開済み版は原則 immutable とする。

| Field | Meaning |
|---|---|
| document_version_id | 版 ID |
| document_id | 親 Document |
| version_no | 文書内で単調増加する版番号 |
| lifecycle_state | `WORKING` / `PUBLISHED` / `WITHDRAWN` |
| title | その版のタイトル |
| revision_reason | 改訂理由。任意 |
| created_at | 作成日時 |
| approved_at | 公開可能と判断された日時。承認フローを使う場合 |
| scheduled_publish_at | 公開予定日時。未設定なら null |
| published_at | 実際に公開された日時 |
| withdrawn_at | 公開終了日時。`WITHDRAWN` の場合 |
| effective_from | 適用開始日時。任意 |
| effective_to | 適用終了日時。任意 |
| created_by_principal | 作成者の外部 Principal |

不変条件:

- `(document_id, version_no)` は一意。
- `PUBLISHED` 後の内容は原則変更せず、新しい版を作る。
- `Document.current_version_id` は `PUBLISHED` の現行版だけを指す。
- `PUBLISHED` なら `published_at` が存在する。
- `WITHDRAWN` なら `withdrawn_at` が存在する。
- `DRAFT` / `NON_PUBLIC` / `WAITING_FOR_PUBLICATION` / `CURRENT` / `SUPERSEDED` は永続化stateとして持たない。


### 2.4.1 導出UIラベル

UI上の状態名は `lifecycle_state` と属性から導出する。

| 条件 | UI表示 |
|---|---|
| `WORKING` かつ `approved_at IS NULL` | 下書き |
| `WORKING` かつ `approved_at IS NOT NULL` かつ `scheduled_publish_at IS NULL` | 非公開 |
| `WORKING` かつ `approved_at IS NOT NULL` かつ `scheduled_publish_at > now` | 公開待ち |
| `PUBLISHED` かつ `Document.current_version_id = document_version_id` | 現行版 |
| `PUBLISHED` かつ `Document.current_version_id != document_version_id` | 過去版 |
| `WITHDRAWN` | 公開終了 |

設計原則:

- UI表示上の意味だけを理由にDB stateを増やさない。
- 「過去版」は `SUPERSEDED` stateではなく、`PUBLISHED` だがcurrentでないことから導出する。
- 「非公開」と「公開待ち」は別表示だが、内部stateはどちらも `WORKING` とする。
- `scheduled_publish_at` 到達後の自動公開方式はTransaction要件側で後続確定する。

### 2.4.2 削除を通常ライフサイクルに含めない

Document / DocumentVersion は原則永久保存し、通常業務では論理削除・物理削除を行わない。

通常検索から外す必要がある場合は、current Versionの公開終了 (`WITHDRAWN`) と検索条件で表現する。

物理削除は、法令・契約上の削除義務、誤登録機密情報、staging/orphan fileのGC等の例外的管理処理に限定し、通常のDocument lifecycleとは分離する。

## 2.5 FileObject

物理ファイル自体を表す。

| Field | Meaning |
|---|---|
| file_id | ファイル ID |
| content_hash | 内容ハッシュ |
| media_type | MIME type |
| size_bytes | サイズ |
| storage_locator | filesystem 等の物理位置 |
| created_at | 保存日時 |

ファイルパスそのものを外部 API の安定 ID にしない。

## 2.6 VersionFile

DocumentVersion と FileObject の関連。

| Field | Meaning |
|---|---|
| document_version_id | Version |
| file_id | FileObject |
| role | primary / attachment |
| ordinal | 表示順 |
| original_filename | 利用者に見せるファイル名 |

これにより 1 版に複数添付を持てる。

## 2.7 Metadata

v0 では柔軟性を優先し、共通項目 + 拡張 metadata の構成とする。

### Document-level metadata

版を跨いで安定する分類。

例:

- document_type
- owning_department
- category

### Version-level metadata

版ごとに変わり得る情報。

例:

- revision_reason
- effective date
- author / publisher
- source filename

Source 固有項目は拡張 metadata として保持可能にする。

## 2.8 Tag / Category

必要に応じて many-to-many で Document と関連付ける。

Folder は物理的・階層的整理、Tag / Category は横断分類として分離する。

## 2.9 ReadState — 固定要件

**既読状態は Principal × DocumentVersion で保持する。**

| Field | Meaning |
|---|---|
| identity_provider | Principal provider |
| principal_id | 外部 Principal ID |
| document_version_id | 読んだ版 |
| first_read_at | 初回既読日時 |
| last_read_at | 最終閲覧日時。必要なら |

論理主キー:

`(identity_provider, principal_id, document_version_id)`

重要な性質:

- 新版 `v5` が公開されても `v4` の ReadState は変更しない。
- `v5` 用 ReadState が存在しないため、自動的に未読として扱える。
- 「全員を未読に戻す」ための一括更新が不要。
- 端末を変えても Principal が同一なら既読状態を維持できる。

## 2.10 AccessPolicy（v0では拡張点）

認証実装は後続でも、データモデル上は行き止まりを作らない。

想定:

- principal / AD group / role を subject とする
- folder / document を resource とする
- read / write / publish / administer 等の action を定義可能

Search Platform には権限判定用の派生 access scope を同期可能とする。

---

# 3. Document Platform のイベント

Search Platform と直接 DB 結合しないため、論理イベント境界を定義する。

最低限:

- DocumentCreated
- DocumentVersionCreated
- DocumentVersionPublished
- DocumentVersionWithdrawn
- DocumentPublicationEnded
- DocumentMoved
- DocumentMetadataChanged

Search Platform はこれらから再 Index / 削除を行える。

初期実装では同一プロセス内の関数呼び出しでもよいが、意味上はイベント境界として扱う。

---

# 4. Search Platform

## 4.1 KnowledgeSource

検索可能な情報源を表す。

例:

- `document-platform`
- `egov-laws`
- `internal-api-x`

| Field | Meaning |
|---|---|
| source_id | 一意な Source ID |
| source_type | document / api / database / external 等 |
| enabled | 検索対象か |
| sync_mode | event / polling / batch 等 |

## 4.2 Resource Identity

Search Platform 内での正本参照キー。

`ResourceKey = (source_id, source_resource_id)`

`ResourceVersionKey = (source_id, source_resource_id, source_version_id)`

Document Platform なら:

- source_resource_id = document_id
- source_version_id = document_version_id

Search Platform 独自 ID だけで元データとの対応が失われないようにする。

## 4.3 CanonicalKnowledgeResourceSnapshot

Knowledge Source から取り込んだ検索用の共通表現。

これは正本ではなく再生成可能な snapshot。

必須 Core Fields:

| Field | Meaning |
|---|---|
| source_id | Source |
| source_resource_id | Source 内 Resource ID |
| source_version_id | Source 内 Version ID |
| resource_type | document / law / record 等 |
| title | 表示・検索用タイトル |
| language | 言語 |
| content_type | 文書種別 |
| created_at | 元データ作成日時 |
| updated_at | 更新日時 |
| effective_from | 有効開始 |
| effective_to | 有効終了 |
| provenance | 出典情報 |
| locator | 元 Resource を再取得するための位置情報 |
| access_scope | 検索時アクセス制御用の派生情報 |
| metadata | Source 固有の拡張属性 |

Core は薄く固定し、Source 固有属性は metadata に残す。

## 4.4 KnowledgeUnit

検索の最小候補単位。

文書全体を 1 件として扱う必要はない。

| Field | Meaning |
|---|---|
| unit_id | 検索単位 ID |
| resource_version_key | 親 Resource Version |
| parent_unit_id | 階層構造を持つ場合 |
| ordinal | 文書内順序 |
| unit_type | document / section / paragraph / table / sheet 等 |
| text | 正規化済み本文 |
| structural_path | 章・節・sheet・ZIP 内 path 等 |
| locator | 元文書内の位置情報 |
| metadata | Unit 固有属性 |

Extraction / chunking の結果として生成される。

## 4.5 Search Representations

KnowledgeUnit から複数の検索表現を派生させる。

### LexicalRepresentation

- token / term
- field
- term position
- analyzer version

想定候補: Tantivy + Lindera。

### VectorRepresentation

- unit_id
- embedding model identifier
- embedding model version
- vector
- generated_at

Embedding は検索アルゴリズムの一表現であり、正本ではない。

### MetadataRepresentation

- field / value
- filter / facet 用

### TemporalRepresentation

最低限、意味を潰さず保持する。

- created_at
- updated_at
- published_at
- effective_from
- effective_to

単一 `timestamp` へ統合しない。

### GraphRepresentation（将来）

必要になった場合のみ追加。

---

# 5. Indexing ETL

基本パイプライン:

```text
Knowledge Source
    ↓ Extract
Source Adapter
    ↓ Transform
Canonical Knowledge Resource
    ↓
Content Extraction / Normalization
    ↓
Knowledge Units
    ↓ Load
Search Representations / Indexes
```

重要:

- Query 時に Office / ZIP を毎回解析しない。
- Source 更新時に Extraction / Indexing を行う。
- Search index は常に再構築可能にする。

---

# 6. Query Runtime Model

Query 処理中だけ存在する論理オブジェクト。

## 6.1 SearchRequest

- query text
- requested sources
- metadata filters
- temporal condition
- top_k
- principal context

## 6.2 RetrievalPlan

LLM Query Planner を使用する場合も、自由形式ではなく構造化する。

例:

- enabled retrievers
- lexical top_k
- semantic top_k
- metadata filters
- temporal policy
- fusion strategy
- weight profile
- rerank strategy

RetrievalPlan は Policy / Validator を通してから実行する。

## 6.3 Candidate

Retriever が返す一時候補。

- unit_id
- retriever type
- raw score
- rank
- retrieval metadata

## 6.4 FusedCandidate

複数 Retriever の候補統合後。

- unit_id
- constituent ranks / scores
- fusion score
- fusion method

## 6.5 RankedResult

Reranking 後の最終結果。

- unit_id
- final rank
- final score
- snippet / highlights
- provenance
- locator
- resource/version identifiers

Human UI と LLM / Agent は同じ SearchResult を利用する。

---

# 7. Search Quality / Traceability

検索精度の原因をレイヤー分解できるよう、ID を end-to-end で保持する。

最低限追跡したい ID:

- source_id
- source_resource_id
- source_version_id
- unit_id
- extraction_version
- representation_version
- index_version
- query_id
- retrieval_plan_id
- trace_id

評価可能にする対象:

1. Source coverage
2. Extraction quality
3. Index coverage / freshness
4. Retriever Recall@K
5. Fusion の gain / loss
6. Reranking の nDCG / MRR 等
7. End-to-end task success

正解文書がどの段階で失われたか追跡可能にする。

---

# 8. Audit / Observability との境界

## Observability

OpenTelemetry を使用する。

- logs
- traces
- metrics

検索パイプラインの span 例:

- query planning
- lexical retrieval
- semantic retrieval
- fusion
- reranking
- response

## Audit

業務監査イベントは別 schema / store とする。

例:

- document.read
- document.create
- document.version.publish
- search.execute
- search.result.open

Query 本文や文書本文を一般ログへ無制限に残さない。

---

# 9. DB / Storage 選定へ導出される要件

この論理モデルから DB 選定時に評価すべき要件を導出する。

## Document Platform Metadata Store

必要特性:

- transaction
- referential integrity
- unique constraints
- recursive folder hierarchy
- concurrent multi-user access
- composite key (`Principal × DocumentVersion`)
- version publish 時の atomic update
- backup / restore
- metadata extensibility
- Rust driver maturity

SQLite / PostgreSQL 等はこの要件をもとに後続比較する。

## File Storage

必要特性:

- immutable version file
- content hash
- backup / restore
- large file handling
- local filesystem から開始可能

## Search Storage

Document DB と同じ製品にする必要はない。

- Lexical index
- Vector index
- Metadata / temporal index

は再生成可能な派生データとして扱う。

---

# 10. v0 で固定する事項

1. **ReadState = Principal × DocumentVersion**
2. Document と DocumentVersion は別エンティティ
3. DocumentVersionの永続化stateは **WORKING / PUBLISHED / WITHDRAWN** のみ
4. 下書き / 非公開 / 公開待ち / 現行版 / 過去版 / 公開終了は導出UIラベル
5. 公開済み DocumentVersion は原則 immutable
6. 通常Document lifecycleに論理削除・物理削除を含めない
7. Search Platform は authoritative data を所有しない
8. Source / Resource / Version / Unit の ID 対応を失わない
9. Canonical Model は薄く固定し、Source 固有属性は拡張 metadata とする
10. Query 時の Office / ZIP 再解析は行わず、Indexing ETL で処理する
11. Human UI と LLM / Agent は同じ API / SearchResult を利用する
12. Audit と Observability は別責務
13. DB 製品はこの論理モデル確定後に選定する

---

# 11. 次の設計ステップ

1. 公開予約・公開終了時のtransaction挙動を確定する
2. Metadata v0 の必須共通項目を決める
3. Canonical Knowledge Model v0 の型を具体化する
4. KnowledgeUnit / chunking policy を決める
5. AccessPolicy の最低限の将来互換性を決める
6. DB / Storage 候補を比較する
7. Rust crate / library 対応表へ落とす
