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
- FileObject / ContentItem / ContentRepresentation（旧VersionFileは移行対象）
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
- Typed HyperEdge graph representation

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

Document Management Basics v0 では root の既存固定 ID を維持し、通常操作で移動・改名・削除しない。新規 Folder の名前は前後空白を除き Unicode NFC に正規化する。空名、制御文字、`/`、`\\`、`.`、`..`、255 Unicode scalar values 超を拒否し、同じ親の下で正規化後の名前を大小文字を区別して一意にする。既存名の不正・衝突・孤立・cycle・複数 root は移行前に検出して停止し、自動改名しない。Folder 自身の revision は Document revision と別に保持する。

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
| base_document_version_id | この版を作成した時点の現行公開版。初版は null。明示的な再基準化以外では変更しない |
| version_no | 文書内で単調増加する版番号 |
| lifecycle_state | `WORKING` / `PUBLISHED` / `WITHDRAWN` |
| title | その版のタイトル |
| revision_reason | 改訂理由。任意 |
| created_at | 作成日時 |
| approved_at | 公開可能と判断された日時。承認フローを使う場合 |
| scheduled_publish_at | 有効な公開予約の予定日時を表す投影。予約がなければ null |
| published_at | 実際に公開された日時 |
| withdrawn_at | 公開終了日時。`WITHDRAWN` の場合 |
| effective_from | 適用開始日時。任意 |
| effective_to | 適用終了日時。任意 |
| created_by_principal | 作成者の外部 Principal |

不変条件:

- `(document_id, version_no)` は一意。
- Documentごとの `WORKING` は高々1版。第2版以降は作成時の現行 `PUBLISHED` 版を base とする。
- `PUBLISHED` 後の内容は原則変更せず、新しい版を作る。
- `Document.current_version_id` は `PUBLISHED` の現行版だけを指す。
- `PUBLISHED` なら `published_at` が存在する。
- `WITHDRAWN` なら `withdrawn_at` が存在する。
- 現行版を取下げる場合、直前の base が引き続き `PUBLISHED` で安全に公開できれば、それを現行版へ戻す。戻せなければ `current_version_id` は null。復帰した版の `published_at` は書き換えない。
- 取下げ・旧版復帰は既存の状態と現行版参照、Audit/Outbox履歴で表現し、追加の真偽値を持たない。
- `DRAFT` / `NON_PUBLIC` / `WAITING_FOR_PUBLICATION` / `CURRENT` / `SUPERSEDED` は永続化stateとして持たない。


### 2.4.1 導出UIラベル

UI上の状態名は `lifecycle_state` と属性から導出する。

| 条件 | UI表示 |
|---|---|
| `WORKING` かつ `approved_at IS NULL` | 下書き |
| `WORKING` かつ `approved_at IS NOT NULL` かつ `scheduled_publish_at IS NULL` | 非公開 |
| `WORKING` かつ `approved_at IS NOT NULL` かつ `scheduled_publish_at > now` | 公開待ち |
| `WORKING` かつ有効な予約の `scheduled_publish_at <= now` | 公開遅延（公開実行・再検証待ち） |
| `PUBLISHED` かつ `Document.current_version_id = document_version_id` | 現行版 |
| `PUBLISHED` かつ `Document.current_version_id != document_version_id` | 過去版 |
| `WITHDRAWN` | 公開終了 |

設計原則:

- UI表示上の意味だけを理由にDB stateを増やさない。
- 「過去版」は `SUPERSEDED` stateではなく、`PUBLISHED` だがcurrentでないことから導出する。
- 「非公開」と「公開待ち」は別表示だが、内部stateはどちらも `WORKING` とする。
- `scheduled_publish_at` は予約台帳から同一transactionで更新する投影であり、予約台帳の有効な公開意図だけが期限到達時の公開実行対象となる。

### 2.4.2 削除を通常ライフサイクルに含めない

Document / DocumentVersion は原則永久保存し、通常業務では論理削除・物理削除を行わない。

Versionの取下げでは直前の公開版が現行に戻る場合がある。文書全体を通常検索から外す操作は、単一Versionの取下げと同一視せず、別の公開終了操作・検索条件として扱う。

文書全体の公開終了（T10）は、現行 `PUBLISHED` Version への参照を null にする。元の現行 Version は `PUBLISHED` の過去版として、原本・`published_at` とともに保持する。新しい Document フラグや Version lifecycle state は追加しない。現行版参照が null だけでは未公開・取下げ後・公開終了を区別できないため、意図的な公開終了は永続的な操作台帳から判定する。Document 単位の「公開終了」表示は操作台帳と null の現行版参照から導出し、Version 単位の表示とは区別する。

T10 後は別途設計された再公開操作がない限り、通常の Version 作成・更新・再基準化・予約・公開から現行版を設定できない。通常公開用の読み取りは現行 `PUBLISHED` Version のみを返し、過去版や `WORKING` Version にフォールバックしない。既存の編集・authoritative 読み取りは T10 未終了の `WORKING` 初版を扱えるが、T10 終了後は旧版へフォールバックしない。過去資料へのアクセスは AccessPolicy に従う別経路とする。

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

## 2.6 ContentItem / ContentRepresentation

DocumentVersion は1個以上の順序付きContentItemを持つ。各ContentItemはちょうど1個のauthoritative representationと0個以上のrenditionを持ち、representationはimmutableなFileObjectを参照する。

| Entity | Field | Meaning |
|---|---|---|
| ContentItem | content_item_id | Version内で安定するitem ID。Versionを跨ぐ意味上の同一性はこのIDから推測しない |
| ContentItem | document_version_id | 所属Version |
| ContentItem | logical_path | 版内の論理path。Unicode NFC、case-sensitive、`/` 区切りの相対path |
| ContentItem | ordinal | 版内の非負の順序番号 |
| ContentRepresentation | content_representation_id | representation ID |
| ContentRepresentation | content_item_id | 所属ContentItem |
| ContentRepresentation | file_id | 参照するFileObject |
| ContentRepresentation | role | AUTHORITATIVE または RENDITION |
| ContentRepresentation | original_filename | 利用者に見せる元ファイル名 |

`(document_version_id, logical_path, ordinal)` は一意。空のpath要素、先頭 `/`、`.` / `..`、曖昧な正規化結果は拒否する。Version identityは正規化したtitleと、`logical_path + ordinal` とauthoritative FileObjectのformat-native意味情報からなる順序付きmanifestで決める。renditionの追加・再生成はVersionを増やさない。ZIPは搬送手段でありauthoritative contentではない。Document Semantic InspectionはFileObjectから再生成可能な派生証拠であり、Search Extractionとは分離する。

既存のVersionFile（PRIMARY / ATTACHMENT）は旧表現である。単一PRIMARYでATTACHMENTのない初版のみ `logical_path = "primary"`, `ordinal = 0` のContentItemへ確定的に移行できる。ATTACHMENTがある場合はauthoritative itemかrenditionかを推測せず、分類されるまでVersioning操作を拒否する。新しいContentItem表現を唯一の編集可能な正本とし、旧VersionFileを並行した正本にしない。

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

Document Management Basics v0 の共通属性の編集対象は `document_type`、`owning_department`、`category`、JSON object の `extensions` に限定する。set/unset の部分更新では対象外の既存キーを保持し、同一キーの set/unset を同時指定しない。Version 固有 metadata、title、本文、原本、公開状態はこの操作では変えない。同値更新は Document revision と mutation event を増やさない。

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
| last_read_at | v0では保持しない。将来の拡張点 |

論理主キー:

`(identity_provider, principal_id, document_version_id)`

重要な性質:

- 新版 `v5` が公開されても `v4` の ReadState は変更しない。
- `v5` 用 ReadState が存在しないため、自動的に未読として扱える。
- 「全員を未読に戻す」ための一括更新が不要。
- 端末を変えても Principal が同一なら既読状態を維持できる。

Document Management Basics v0 では、信頼済み HumanInteractive 本人による現行 `PUBLISHED` Version の明示確認でのみ `first_read_at` を作る。通常参照、Agent/Service、プレビュー先読みは既読にしない。重複確認は自然キーで冪等に扱い、初回の必須 Audit と同一 transaction にする。旧版を後から既読にしたり、新版へ既読を自動継承したりしない。

## 2.10 AccessPolicy

認証実装は後続でも、データモデル上は行き止まりを作らない。

想定:

- principal / AD group / role を subject とする
- folder / document を resource とする
- read / write / publish / administer 等の action を定義可能

Search Platform には権限判定用の派生 access scope を同期可能とする。

Document Management Basics v0 では Folder または Document に安定した Policy ID と policy revision を持つ明示 policy を binding できる。主体は issuer 付き Principal / Group / Role、操作は `read`、`read_history`、`write`、`publish`、`administer` とし、操作間の暗黙の包含はない。allow-only とし、Document から祖先 Folder へたどった最も近い明示 policy が policy 全体を置換する。親子 policy を和集合にしない。明示空 policy は不正で、継承は明示 policy の解除として表す。root policy 未設定なら一般操作は拒否する。root の初期 policy は信頼済み bootstrap 専用操作で登録し、通常 T8 で回復・置換しない。

単一の `access-state` revision/guard により policy 変更と Folder/Document 移動を、認可付き操作と直列化する。Policy、access-state、Folder、Document の revision はそれぞれ別である。管理操作台帳は操作 ID、型付き対象、期待 revision、actor、正規化 digest、changed/unchanged 結果、UTC 時刻を保持し、v0 では TTL を設けない。既存 Document に全員向け policy を推定付与しない。

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

### HyperGraphRepresentation

Search / Discovery Platform v0ではfirst-classな派生Projectionとして扱う。物理Graph backendは別途選定する。

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
12. Versionのcontentは1個以上のContentItemで表し、各itemにauthoritative representationをちょうど1個持つ
13. Audit と Observability は別責務
14. DB 製品はこの論理モデル確定後に選定する

---

# 11. 次の設計ステップ

1. 承認済みDocument Versioning v0設計に従い、公開予約・Version取下げのtransactionを実装する
2. Metadata v0 の必須共通項目を決める
3. Canonical Knowledge Model v0 の型を具体化する
4. KnowledgeUnit / chunking policy を決める
5. AccessPolicy の最低限の将来互換性を決める
6. DB / Storage 候補を比較する
7. Rust crate / library 対応表へ落とす

---

# 12. Document Diff v0 の派生データ

Document Diff v0 の `DiffResult` と新旧対照表は、二つの `DocumentVersion` snapshot と原本 `FileObject` から要求時に再生成する派生結果である。Document、DocumentVersion、ContentItem、authoritative ContentRepresentation、FileObject、DSI の正本関係を変更しない。Search Index や検索用 chunk を比較の正本にしない。

cache key は方向付きの両 snapshot digest と比較・資源 profile を結合する。cache entry は主体別権限を保存せず、読み出し時に結果 digest と source binding を検証する。cache hit と新旧対照表の取得でも現在権限・入力鮮度の再確認と必須 Audit を省略しない。未比較範囲と原本参照は投影後も保持する。


---

# Search / Discovery Platform v0 logical model amendment

既存KnowledgeResourceSnapshot / KnowledgeUnitはKnowledgeResource系の一部として維持し、Search canonical modelを以下へ拡張する。

## DiscoverableSource

```text
DiscoverableSource
- source_id
- source_type
- resource_types[]
- business_domains[]
- concept_refs[]
- discovery_modes[]
- discovery_capabilities[]
- enumeration_semantics
- authority_scope
- provenance
- access_model
- retention_mode
- freshness_policy
```

## DiscoverableResource

```text
DiscoverableResource
├─ ResourceIdentity
├─ typed ResourceBody
├─ UsageProfile[]
├─ DiscoveryProfile
├─ TemporalDiscoveryProfile
└─ ResourceRelations[]
```

Resource family:

- KnowledgeResource
- SemanticResource
- CapabilityResource
- AgentSkillResource
- WorkflowResource
- PolicyResource

KnowledgeResourceへ他Resource型を無理に畳み込まない。

## Assertion

```text
Assertion
- assertion_id
- subject_ref
- predicate
- value
- source_ref
- origin
- authority_scope
- evidence_refs[]
- observed_at
- effective_from?
- effective_to?
- derived_by?
```

複数Assertionを保持し、Authority Resolution後のDiscovery Projectionから元Assertionへ追跡可能にする。

## Logical resource identity

```text
LogicalResource
    ↓
ResourceRepresentation[]
    ↓
ResourceVersion[]
    ↓
DiscoveryProjection[]
```

SimilarityだけでLogical identityを確定しない。RESOLVED / PROVISIONAL / UNRESOLVED / CONFLICTを区別する。

## UsageProfile / DiscoveryProfile

UsageProfileはapplicable_when / not_applicable_whenを含み、1 Resourceに複数用途を許可する。
DiscoveryProfileはcanonical_name / aliases / concept_refs / intents / high_signal_facets / confusable_with / distinguished_by等を持てる。

## TypedRelationInstance

Canonical Graph relation:

```text
TypedRelationInstance
- relation_id
- relation_type
- participants[] { role, resource_ref }
- qualifiers
- temporal_scope
- authority
- provenance
- evidence_refs[]
```

二項関係も同じn-ary modelで表現する。Canonical relationをlossy binary edgeへ変換しない。
将来のbinary shortcutはderived acceleration artifactに限り、元RelationInstanceへ逆参照可能にする。

## Discovery execution types

最低限以下の型をDomain / Core側で表現可能にする。

- IntentSignature
- ApplicabilityResult
- InformationGap
- QualifiedResource
- FederatedCandidate
- EvidenceRequirement
- Claim
- EvidenceSet / EvidenceSufficiency
- DiscoveryResult
- LogicalResourceBinding
- RepresentationBinding

具体的なIndex / Graph / Vector backend型をこのlogical modelへ露出させない。

## Search KnowledgeUnit v1 — P1→P2 最小入力契約（追補）

この節は §4.4 の Search 向け具体化である。規範入力は `docs/superpowers/programs/search-platform-completion/p1-knowledgeunit-freeze.md` が固定した `p1-knowledgeunit-contract.md`（SHA-256 `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe`）と `p1-knowledgeunit-amendment.md`（SHA-256 `0d44f5dfed72360bafc56f19c2bf72b1d8b9f278c7ea3247261e1af040a06a00`）であり、Archive provenance / profile と Vector cache / hit は後者を優先する。これは provider-neutral な入力型の規範であり、parser、本文索引、exact evidence、Source 権限判定の完成を意味しない。

- `ResourceVersionRef` は `SourceId`、**Version の** `ResourceId`、Source native Version ID、`ContentPartRef` は Source native Part ID、NFC・case-sensitive・相対 `/` 区切りの `logical_path`、part ordinalを保持する。`RawBinding` は immutable authoritative bytes の SHA-256、size、lowercase `type/subtype` MIME essenceを保持する。`UnitProvenance` は Source snapshot、authoritative representation ID、raw binding、outer `FormatId`、profile ID、parser build IDを保持し、Archive だけ leaf formatを別 fieldに保持する。同一 FileObjectでも別 Partの Unit は別IDになる。
- `NativeLocator` は frozen contract §2 の tag 1–8（Docx、Spreadsheet、Pptx、Pdf、Text、Csv、Html、Archive）と物理/native座標を使う。codecは `native-locator:v1\0`、tag、固定幅BE整数、u32-count列、u32長の `frame` を用い、unknown tag、trailing bytes、非NFC文字列、曖昧path、空の必須path、Archive再帰を拒否する。Archive member は厳密decode後のNFC相対pathであり、ZIP raw名との一対一、衝突、symlink、暗号化の検査は trusted hostが担う。locatorは同一raw/profileの再解析で唯一のnative要素に戻り、textを再構成できなければ受理しない。
- 正規化 `nfc-lf-v1` は厳密decode後に CRLF→LF、CR→LF、NFCの順で行い、case・幅・かな・空白・句読点を変えない。`text_sha256` はその UTF-8 bytes の SHA-256。`TextSpan` は正規化textのUTF-8 byte境界の非空半開区間であり、PDF native character indexとは別である。
- `ExtractionProfileId` は固定順binary定義の SHA-256 を `sha256:` + 64小文字hexで表す。非Archiveは `extraction-profile:v1\0` と frozen contract §3 の format、parser artifact/native pin、revision、effective settings、全15 budget keyを符号化する。Archive itemは追補の `extraction-profile:archive:v2\0` と reader node chain全体の composite IDを**item内の全Unit**に使う。inner charset/dialect、nested decoder、parser build、PDFium pin、budgetの変更は別IDを要する。未登録・不完全・曖昧なreader planは Supported としない。
- `UnitId` は `knowledge-unit:v1\0` の後に Source UUID bytes、Version Resource UUID bytes、native Version、native Part、logical path、part ordinal、profile ID、locator bytes、Unit ordinalの**9 fieldを個別にframe**した SHA-256であり、外部表記は `ku1:` + 64小文字hex。generation、raw/text hash、parent IDはID入力ではない。hostは同一Part内の0始まり連続ordinal、重複なしlocator、同一Partの前出Unitへの親参照、kind/format/locator、text digest/ID、raw/native round-tripを検証する。
- P2 embedding cache のkeyは `(embedding_model_id, UnitId, text_sha256, ExtractionProfileId, SourceId, authority_scope_key, retention_lease_id, lifetime_scope_id)` を含む。entry/hitは元Version/Part/raw/representationとgenerationを保持し、Sourceの現在 Read・Live Version/T10・retention許可・lease・scope・pin済みmanifestを候補化前と公開直前に照合する。`SESSION_ONLY` と `NO_RETENTION` の永続embedding/index/backup/queueは禁止する。cache/similarityはexact evidence、lexical `BodyRequired`、absenceの証明にならない。

## Search 本文 P1 — 正本、Unit、coverage、lexical の規範

本節は上の最小 Unit 型を、`p1-extraction-freeze.md`（SHA-256 `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d`）の合成契約に従って本文へ接続する。`p1-body-absence-amendment.md` の否定証明には `p1-partial-positive-correction.md` の肯定側訂正を適用する。Archive の同一 Part に異種 leaf がある場合は `p1-unit-archive-binding-ruling.md` の leaf ごとの照合を適用する。詳細な型、canonical encoding、上限と検証ケースは凍結契約に従い、§4.4 の一般的な `KnowledgeUnit` 説明を本文の正本・完全性証明として扱わない。

### P1-L1: Source-owned authoritative body binding

- 本文の Source 正本は、通常検索では T10 未終了の現行 `PUBLISHED` DocumentVersion の全 `ContentItem` と、各 item の唯一の `AUTHORITATIVE` `ContentRepresentation` が参照する immutable `FileObject` である。単一の `REPEATABLE READ, READ ONLY` snapshot で Version/T10、document/access revision、item、representation、FileObject を結び、item の欠落・重複・不正な path/ordinal/参照を拒否する。`version_files`、rendition、DSI fingerprint/result、macro 実行、外部 HTML/JS、History を Live 本文の代用にしない。History は明示 ID の `Read` + `ReadHistory` 経路に留める。
- trusted host は `FileStorage` から上限付きで開いた raw bytes を FileObject の SHA-256・size・MIME と束縛し、worker 前と応答後に hash/size を再照合する。worker へ Source/actor/StorageKey/FileObject ID を渡さない。host は返却 Unit の Version Resource、Part、representation、raw、profile、ordinal、kind、native locator、正規化 text/`text_sha256`、再解析 round-trip と UnitId を検証する。同一 bytes・同一 text でも親 Part が違えば Unit を混同しない。不一致は integrity incident とし、新 generation を公開しない。
- format reader は固定した合成・公開 corpus で本文範囲、locator、資源、license/security を形式別に資格判定し、登録済み parser/native pin・effective settings と全15 budget key を `ExtractionProfileId` に固定する。DOCX/XLSX/XLSM/PPTX/PDF/Text/CSV/HTML/明示許可 Archive の対象範囲を形式別に定義し、未資格・曖昧・対象外の形式を `Supported` としない。Archive は item 共通 composite profile を使い、各 Unit の member chain、実際の leaf reader/format、inner locator、outer raw/profile と reader-use 全 node を照合する。同一 Archive Part 内に Text と CSV 等の異種 leaf を許し、Part 全体を単一 leaf format と仮定しない。新 reader 依存は隔離 PoC の GO 前に production へ入れない。

### P1-L2: Item coverage と検索可能 Unit doc の seal

- `BodyUnitManifest` と `BodyCoverageArtifact` は同じ現行 Live snapshot の全 AUTHORITATIVE item をそれぞれちょうど一度記録し、Version/Part/representation/raw/profile/operation/coverage/Unit count を一致させる。`Completed + Supported` は登録 profile の reader-visible scope を最後まで列挙し全対象 text と locator を検証した場合だけ許す。対象 text が真に空なら Unit 0 を許す。`Completed + Partial` は traversal 完了、既知の省略範囲・非空理由、少なくとも1件の locator 検証済み Unit が必要で、その Unit は検索可能にするが blocking coverage gap と不完全性を保持する。`Completed + Unsupported` と `FailedPermanent` は Unit 0、`Retryable` は公開 manifest に含めない。途中 kill/panic/output 切断、hard budget 中断、不明な欠落を Partial/Supported に変換しない。body coverage と Source enumeration coverage は別である。
- Resource doc と Unit doc を分け、Unit doc の body field には検証済み `KnowledgeUnit.text` だけを索引する。`Completed + Supported/Partial` の全 Unit と、**構築後に実際に検索可能な** lexical Unit doc を、同一 generation・Source・親 Version Resource・Part・representation・raw・UnitId・ordinal・kind・locator・profile・`text_sha256`・正規化本文の全 bytes で双方向一対一照合する。`Unsupported/FailedPermanent` の doc は0件とする。欠落・余分・重複・本文差替えは個別 receipt の digest が正しくても seal を拒否する。tokenizer の候補化だけで literal substring の完全性は証明しない。
- 既存 `ProjectionGenerationManifest.digest` / `generation_digest()` は projection-only v1、`resource_count` は Resource 数、同 manifest の `coverage` は Source enumeration のまま維持する。Unit manifest、body coverage、実 lexical doc、Graph、profile set、schema version は別の同一 `ProjectionGenerationKey` の immutable `GenerationBundleReceipt` に runtime 再計算 digest/count と composite digest を持たせる。full/incremental は同じ Source binding・順序・profile・正規化から同じ論理 digest を得る。本文だけが変われば composite digest が変わる。Projection digest を本文 digest に置き換えない。
- （2026-10-07）bundle は親 Version×Part×profile/parser build 単位の content-addressed immutable segment と generation ごとの順序付き segment 一覧で保存してよい。一覧から再計算する合成 digest は full rebuild の論理 digest と一致させる。検証の範囲と時点は transaction-consistency-requirements の SD-T11 5 に従う。

### P1-L3: 本文検索と限定 exact-text claim

- `BodyRequired` は trusted adapter が request ごとに非空 `BodyOnly` query と claim/selector binding を渡す。P1 の `discover_with_content_scope`、P4 の `discover_scoped` と既存 `discover` は同一の内部評価 loop に接続する。Unit doc の body tier だけを本文候補とし、title/alias/metadata、Graph、Vector、probe や DSI を本文充足の代わりにしない。通常 Discovery の S1 `PriorityConcat` は維持する。検索・evidence・coverage は同じ pinned bundle を使用し、Unit hit の親 Version Resource/Part/raw/profile/locator/span を保持する。
- `document.body.contains_exact` の肯定は、同一の現行 Live 親 Version の `Completed + Supported` **または** `Completed + Partial` の検証済み Unit 一つの中に、非空の期待文字列が `nfc-lf-v1` 後の連続 UTF-8 literal substring としてある場合だけ作る。case・幅・かな・空白・句読点は変えず、Unit/item 間を連結しない。Partial 肯定の `Extracted` claim と blocking coverage gap は共存させる。trusted query・selector・required ClaimId/subject/predicate/value、実 span、pinned Unit、Source-owned 現行 Version/T10・`Read`・Part/raw、同一 raw の再読取と locator 再構成を照合し、公開直前にも再確認する。一般の本文 hit や Vector similarity は別の事実 claim の primary evidence に昇格しない。
- 同述語の `Absent` は lexical no-hit の意味ではなく、指定した一つの現行 Live 親 Version の可視 AUTHORITATIVE item がすべて `Completed + Supported` で、Source-owned な有限の全 Unit literal scan と最終 authority 照合を終えた時だけの限定証明である。`Partial` の検証済み Unit は肯定に使えても否定には使えない。完全な否定証明・非開示 gap と公開制御は本書の規範と `transaction-consistency-requirements-v0.md` の P1 consistency 追補をともに満たす。

## Search Full API P5 — 公開 DTO と Source 正本の境界

四 operation と wire schema は [`spec/api/search-openapi.yaml`](../api/search-openapi.yaml)、Search 専用 error は [`spec/errors/search-api-error-registry.yaml`](../errors/search-api-error-registry.yaml) を正本とする。本節は P5 composed freeze（revision 2 > reconciliation > revised design）および P1 composed/Archive ruling、P3 typed n-ary Graph、P4 remote freeze を Search 論理モデルへ写像する。P4 source-neutral catalog、P1 full body、P3/P7 durable/current、二段 disclosure lease と実 HTTP の実装・資格を完了済みとは扱わない。

### P5-D1: 同一 trusted actor と完全可視 catalog

v0 transport の Bearer credential は server 設定の `SearchCredentialVerifierPort` が検証し、opaque session handle だけを `VerifiedActorResolverPort` に渡す。四 operation は P4 の一つの `TrustedSearchScope`、`AuthorizedSourceScope`、`ScopedSourceRegistryPort` と `VisibleCatalogSnapshot` を共有する。request の principal/tenant/role/group/subjects/access context、provider URL、native locator、Source grant を認証・可視性の入力にしない。Document と Remote の登録を一つの union catalog に持ち、各 Source の owner、registration/visibility revision、activation、現在の Source 存在許可を検査する。Source 個別 `Denied` / `Unknown`・revision race はその Source だけを除外し、他の可視 Source を保つ。actor/registry/ledger/visibility error、列挙不完全、重複 ID、構造的不一致は完全 snapshot 不成立として四 operation を generic 503 に閉じる。空集合も完全な列挙時だけ正常結果である。production factory は二 namespace の全 tenant 完全集合 reconcile、durable SourceId owner/current ledger、identity verifier/challenge と可視性 port がない場合、四 route を起動しない。

四 route は `trusted identity/operation authorization → 完全な可視 Source snapshot → 可視 selector/locator → routing/pin/Source I/O → actor/Source/item/field/Graph participant final gate → private safe DTO` の順を守る。`sourceIds` は可視集合との intersection のみで、未知・他 tenant・不可視 ID の数や理由を出さない。Resource locator は可視 Source scope 内の Source-owned current locator だけを使い、裸 ID の global lookup、ephemeral/native ID、旧版/history fallback を禁じる。対象の存在を開示できる前の失敗と、未知・不可視・T10 終了・重複 locator は同じ 404 である。

### P5-D2: 閉じた request / response projection

`SearchQuery` は `query,resourceTypes?,sourceIds?,coverage,pageSize?,cursor?`、`DiscoveryInput` は `need.{purpose,requiredResourceTypes,requiredClaimIds,temporalTarget?,businessTimezone?},query?,coverage` だけである。`GET /v1/sources` は `pageSize?,cursor?`、Resource GET は canonical `resourceId` だけを受ける。未知 JSON field を拒否する。公開 `temporalTarget` は履歴 Read 権限を発行しない。上限と不正入力の typed outcome は operations §15 と OpenAPI に固定する。

HTTP は Core 型を直接 serialize しない。`SearchPage` は許可済み `items,nextCursor,partial,coverage,gaps,traceId` だけを持ち、item は `resourceId,sourceId,resourceType,resourceVersionId?,title?,rank,matchedFields,snippet?,provenance` のみ。`DiscoveryEvaluation` は `needId,discoveryEvaluationId,qualifiedResources,evidenceSufficiency,evidence,gaps,rejectedCandidates,evaluationCompleteness,completenessReasonCodes,trace,traceId` に閉じる。`ResourceDetail` は current durable Resource の `resourceId,sourceId,resourceType,resourceVersionId?,title?,snippet?,coverage,provenance,traceId`、`SourcePage` は完全可視集合の `items,nextCursor,traceId` のみで partial field を持たない。Source item は `sourceId,sourceType,resourceTypes,discoveryModes,enumerationSemantics,coverage,availabilityCode?` の安全な接続済み能力だけである。Source endpoint、authority/access model、retention、credential、provider trace/native URL、Graph path、raw score、query/body、private actor はいずれの DTO にも出さない。

public `resourceType` は `knowledge,document,folderPlacement,semantic,capability,agentSkill,workflow,policy`、request coverage は `titleAndPermittedMetadata,bodyRequired`、Source/search response capability は `titleAndPermittedMetadata,bodySearchWithPerItemCoverage` の閉じた enum。`bodySearchWithPerItemCoverage` は実接続済み body search 能力であり、全 item の完全性ではない。`ResourceDetail.coverage` は item ごとの `titleAndPermittedMetadata,bodySupported,bodyPartial,bodyUnsupported,bodyUnknown` で、後四者は P1 の現在の item coverage による。`bodyPartial` は ResourceDetail という DTO の部分成功ではなく、本文 coverage の不完全性を示す。snippet は現在の field grant のある plain text 最大 320 Unicode code points と `title,metadata,body` の field/coverage code のみ。`provenance` は可視 Source/Version の公開 canonical ID だけ、citation は許可済み resource と field だけ。`value` と condition/evidence/rejection code は Source が現在許可した field と server 登録済みの非データ由来 code に限る。free-form qualification/source/retrieval trace、selector/expected value、Unit locator/Part/raw、internal citation chain、native provider role/origin は遮断する。rank/count/trace は final gate 後の可視項目から再計算し、S1 `PriorityConcat` の順序を守る。

Discover の `requiredClaimIds` は UUID 形式と 1〜16 個だけを公開 validation し、Source-owned claim catalog が**同じ** actor/Source scope と pinned generation で subject/field/selector を照合する。未知・他 tenant・不可視・失効 Claim は `unresolved` と blocking `REQUIRED_CLAIM_UNRESOLVED` に統一し、残りの Claim だけで `sufficient` にしない。P1 `bodyRequired` は `BodyOnly` と同じ pinned Unit/doc seal、親 Version/Part/raw、現行 `Read` に束縛する。`Completed + Partial` Unit の exact positive は blocking coverage gap と共存し、negative `Absent` は全可視 authoritative item の `Completed + Supported` と Source-owned finite exact scan proof に限る。P3 typed n-ary relation は全 participant と metadata の同一 Source/generation/current grant が必要で、一人でも不可視なら派生 item/evidence/rank/count/trace を一体除去する。Graph path を citation/Primary/trace にしない。

### P5-D3: cursor、retention、開示寿命

公開 cursor は CSPRNG の衝突検査付き UUID v4 RAM handle だけで、actor/tenant/session、request digest、可視集合 stamp、Source generation/snapshot、retention、S1 last key、期限に束縛する。raw query/body、provider cursor、credential、個人情報を handle/保存 state/log に入れない。別 session、可視集合変化、取消、失効、restart、generation/retention/request 変更は同一 `409 CURSOR_STALE`。`NO_RETENTION` や完全性に必要な非継続 Source があれば Search は cursor なし・partial と `PAGINATION_UNAVAILABLE` gap。SourcePage は stable visibility stamp/keyset がある場合だけ cursor を発行し、stamp 不在で全可視 Source が一 page に収まらなければ 503 とし、切捨て 200 を返さない。

P4 の `PERSISTENT_RESOURCE`、`PERSISTENT_DISCOVERY_METADATA`、`CACHE_WITH_EXPIRY`、`SESSION_ONLY`、`NO_RETENTION` の制約を継承する。`NO_RETENTION` の provider response/evaluation は `EvaluationLease` が終了時に閉じ、許可された field だけを `TransientDisclosureLease` に渡す。全 final gate 後、bounded JSON を private buffer に確定し、socket 送信完了・error・disconnect・cancel・deadline 時に disclosure と buffer を閉じる。handler return や body EOF だけを socket 完了と見なさない。header 前には body を streaming しない。`NO_RETENTION` の content/candidate/provider cursor/trace を cursor、Projection、Graph、cache、Audit、telemetry、test fixture に再保持しない。
