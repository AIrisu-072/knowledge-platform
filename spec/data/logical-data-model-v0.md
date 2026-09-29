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
