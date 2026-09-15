# Transaction & Consistency Requirements v0

## 0. 文書情報

- 文書名: Transaction & Consistency Requirements v0
- 対象:
  - Document Platform
  - Search Platform との整合境界
  - Audit / Observability との整合境界
- 目的:
  - DB製品を選定する前に、必要なトランザクション特性・整合性保証・競合制御をDB非依存で定義する
- 前提:
  - 利用対象人数は約1,000人
  - 文書は原則永久保存
  - 文書数は100万件未満を想定するが継続増加する
  - 文書本体はDBと分離したFile Storageに保持する前提
  - Search Indexは正本ではなく再生成可能な派生データ
  - 既読状態は **principal × document_version** で保持する
  - DocumentVersionの永続化するライフサイクル状態は **WORKING / PUBLISHED / WITHDRAWN** の3状態に限定する
  - 「下書き / 非公開 / 公開待ち / 現行版 / 過去版 / 公開終了」は永続化状態ではなく、状態・属性・`Document.current_version_id`・現在時刻から導出する
  - 通常業務では文書の論理削除・物理削除を基本操作としない
  - 人間とLLM / Agentは共通APIを利用する
  - 初期構成は1台への集約も許容するが、論理境界は分離する

---

# 1. 基本方針

## 1.1 Strong Consistencyを要求する領域

以下はDocument Platformの正本であり、強い整合性を要求する。

- Document
- DocumentVersion
- FileObjectの論理参照
- Folder / Category
- Metadata
- AccessPolicy
- ReadState
- Documentのcurrent version状態
- Transactional Outbox
- 業務上必須のAudit Event

これらについては、途中状態を外部から観測できないことを原則とする。

---

## 1.2 Eventual Consistencyを許容する領域

以下は正本から再構築可能な派生データとする。

- Extraction結果
- Canonical Knowledge Resource Snapshot
- KnowledgeUnit
- Lexical Index
- Vector Index
- Metadata Index
- Temporal Index
- Graph表現
- Search Cache
- Reranking用派生特徴量

Document Platformの正本更新とSearch Platformの更新は分散トランザクションにしない。

---

## 1.3 ObservabilityとAuditを分離する

### Observability

障害調査・性能監視・デバッグ用途。

- logs
- traces
- metrics
- OpenTelemetry

Observabilityの送信失敗を理由に業務トランザクションをrollbackしない。

### Audit

業務上の追跡・監査証跡。

例:

- document.created
- document.version.created
- document.version.published
- document.read
- access_policy.changed
- search.executed

監査上必須と定義されたAudit Eventについては、業務トランザクションと同一commit境界で失われないことを要求する。

---

# 2. 整合性クラス

## C0: Authoritative Strong Consistency

正本の一意性・参照整合性を保証する。

対象例:

- current DocumentVersion
- DocumentVersion状態
- Folder構造
- AccessPolicy
- ReadState
- Outbox Event

要件:

- ACID transaction
- referential integrity
- unique constraints
- atomic commit / rollback

---

## C1: Concurrency-Safe Consistency

複数利用者が同時操作してもlost updateや重複生成を防ぐ。

対象例:

- 同一Documentへの同時改訂
- current version切替
- Folder移動
- Metadata更新
- ReadState upsert

要件:

- optimistic concurrency controlを基本とする
- 必要箇所でrow-level lockingを利用可能であること
- compare-and-swap相当の更新条件を表現できること

---

## C2: Eventual Derived Consistency

正本更新後、一定時間差でSearch Platformへ反映される。

対象:

- Search Index
- Vector Representation
- Extracted Text
- KnowledgeUnit

要件:

- 冪等な再処理
- 再試行
- replay
- rebuild
- index lagの観測

---

## C3: Best-Effort Observability

対象:

- telemetry
- debug logs
- non-critical metrics

要件:

- 本体処理を阻害しない
- バックプレッシャー時に業務処理と分離できる

---

# 3. グローバルInvariant

以下は常に成立していなければならない。

## INV-01: Document current versionの一意性

1つのDocumentに対して、currentとして扱われるDocumentVersionは高々1つ。

---

## INV-02: current versionは公開可能状態である

`Document.current_version_id` が存在する場合、そのDocumentVersionは少なくとも公開済み状態でなければならない。

---

## INV-03: Published DocumentVersionの不変性

公開済みDocumentVersionの内容本体を直接上書きしない。

変更は新しいDocumentVersionとして作成する。

---

## INV-04: ReadStateの一意性

既読状態の論理キーは以下。

```text
principal_id × document_version_id
```

同一組合せについて複数のReadStateレコードを生成しない。

---

## INV-05: FileObject参照整合性

DocumentVersionから参照されるFileObjectは利用可能状態でなければならない。

DB上でVersionだけ存在し、対応ファイルが利用不能な状態を正常状態としない。

---

## INV-06: Folder循環禁止

Folder階層にcycleを許可しない。

---

## INV-07: DocumentVersion番号の重複禁止

同一Document内でversion sequence / version identifierが重複しない。

---

## INV-08: Search Indexは正本ではない

検索Indexの内容をDocument Platformの正本更新へ逆流させない。

---

## INV-09: Outbox Eventの喪失禁止

Search同期等に必要なイベントは、対応する業務更新と同一transactionで記録される。

---

## INV-10: Audit Eventの追跡性

監査対象イベントは、actor / action / resource / timestamp / result / trace_id等の最低限の追跡情報を持つ。

---

## INV-11: DocumentVersionの永続化状態は最小化する

`DocumentVersion.lifecycle_state` は以下のみを永続化する。

```text
WORKING
PUBLISHED
WITHDRAWN
```

`DRAFT`、`NON_PUBLIC`、`WAITING_FOR_PUBLICATION`、`CURRENT`、`SUPERSEDED` 等を独立した永続化状態として持たない。

---

## INV-12: UI表示ラベルは導出値とする

人間向け表示ラベルは以下の事実から導出する。

- `lifecycle_state`
- `approved_at`
- `scheduled_publish_at`
- `Document.current_version_id`
- 現在時刻

標準導出規則:

| 条件 | UI表示 |
|---|---|
| `WORKING` かつ `approved_at IS NULL` | 下書き |
| `WORKING` かつ `approved_at IS NOT NULL` かつ `scheduled_publish_at IS NULL` | 非公開 |
| `WORKING` かつ `approved_at IS NOT NULL` かつ `scheduled_publish_at > now` | 公開待ち |
| `PUBLISHED` かつ `Document.current_version_id = self` | 現行版 |
| `PUBLISHED` かつ `Document.current_version_id != self` | 過去版 |
| `WITHDRAWN` | 公開終了 |

表示上の意味だけを理由に永続化stateを増やさない。

---

## INV-13: lifecycle属性整合性

最低限、以下を保証する。

- `PUBLISHED` なら `published_at` が存在する
- `WITHDRAWN` なら `withdrawn_at` が存在する
- `scheduled_publish_at` を持つ `WORKING` 版は、公開可能と判断済みであることを表現できる
- `Document.current_version_id` は `PUBLISHED` のVersionだけを指す

---

# 4. 競合制御方針

## 4.1 基本はOptimistic Concurrency Control

Document、Metadata、Folder等の更新対象にrevision相当の値を持つ。

概念例:

```text
Document.revision = 12

UPDATE document
SET ..., revision = 13
WHERE id = ? AND revision = 12
```

0件更新の場合はConflictとして扱う。

### 目的

- lost update防止
- 長時間lockの回避
- Web UI / LLM / APIの並行利用への対応

---

## 4.2 Pessimistic Lockを許容する箇所

以下のような短時間・高整合性操作ではrow-level lock等を許容する。

- current versionの切替
- version sequence採番
- Folder移動での整合チェック
- AccessPolicy変更

ただしアプリケーション全体で長時間transactionを保持しない。

---

# 5. Transaction Catalog

## T1: 文書を新規作成する

### 入力

- title
- folder / category
- metadata
- initial file
- actor principal

### 変更対象

- Document
- DocumentVersion
- FileObject reference
- Metadata
- Outbox Event
- Audit Event

### 守るInvariant

- INV-05
- INV-07
- INV-09
- INV-10

### Transaction境界

DB上の正本登録は単一transactionでcommitする。

File Storageへの書込みはDB transactionと同一ACID境界にできないため、別途File Commit Protocolを適用する。

### Commit後Side Effect

- Extraction要求
- Search Index更新要求

---

## T2: 新しいDocumentVersionを作成する

### 入力

- document_id
- base_revision
- new file
- metadata変更
- actor principal

### 変更対象

- DocumentVersion
- FileObject reference
- Outbox Event
- Audit Event

### 初期状態

新規Versionは原則 `WORKING` として作成する。

公開前の表示ラベルは `approved_at` / `scheduled_publish_at` から「下書き」「非公開」「公開待ち」を導出する。

### 競合

同一Documentに複数利用者が同時Version作成する可能性を許容するかは業務ルールで制御する。

Version sequence採番は重複を許可しない。

### 守るInvariant

- INV-03
- INV-05
- INV-07
- INV-09

---

## T3: DocumentVersionを公開する

最重要transactionの1つ。

### 変更対象

- target DocumentVersion
- Document.current_version_id
- previous current DocumentVersion
- Document.revision
- Outbox Event
- Audit Event

### 原子的に行う操作

```text
target version.lifecycle_state -> PUBLISHED
target version.published_at -> now
Document.current_version_id -> target version
previous current.lifecycle_state -> PUBLISHED のまま保持
（previous current は current_version_id から外れるため UI 上「過去版」と導出）
Document.revision -> +1
OutboxEvent(document.version.published)
AuditEvent(document.version.published)
```

### 守るInvariant

- INV-01
- INV-02
- INV-03
- INV-09
- INV-10

### 競合時

同時公開が発生した場合、1つのみ成功させる。

後続要求はConflictとして再読込を要求する。

---

## T4: DocumentVersionを取下げる

### 前提

公開済みVersionを公開対象から外す操作。データ自体は削除しない。

### 変更対象

- `DocumentVersion.lifecycle_state -> WITHDRAWN`
- `DocumentVersion.withdrawn_at`
- `Document.current_version_id`（対象がcurrentの場合）
- Audit Event
- Outbox Event

### UI表示

`WITHDRAWN` は UI 上「公開終了」と導出する。

### 要検討

current Versionを取下げる場合に、

- 直前の `PUBLISHED` Versionへcurrentを戻すか
- `current_version_id = null` を許容するか

は業務ルールとして後続確定する。

### 守るInvariant

- 取下げ後もVersionおよびFileObjectは保持する
- `WITHDRAWN` Versionを `current_version_id` が指し続けない
- Search Platformへ除外・再Indexイベントを確実に通知する


---

## T5: Document Metadataを変更する

### 方針

Version固有metadataとDocument共通metadataを分離する。

### 競合

Optimistic Concurrency Controlを基本とする。

### 守るInvariant

更新対象のrevision一致。

検索用metadata更新はOutbox経由で非同期反映する。

---

## T6: 文書をFolder間で移動する

### 変更対象

- Document.folder_id
- Document.revision
- Audit Event

### 守るInvariant

- 移動先Folderが存在する
- 移動権限がある
- Folder構造自体のcycleを作らない

---

## T7: Folderを作成・移動する

### 変更対象

- Folder
- parent_folder_id
- revision

### 守るInvariant

- Folder cycle禁止
- parent存在保証
- 同一parent下での名前重複ルールは要件次第

### 必要機能

recursive query / hierarchical queryが有用。

---

## T8: AccessPolicyを変更する

### 変更対象

- AccessPolicy
- policy binding
- revision
- Audit Event

### 要件

権限変更は監査対象。

### 将来要件

Windows Identity / group情報との同期方式に依存するため、policy modelの詳細は後続で定義する。

---

## T9: 文書を既読にする

### 論理キー

```text
principal_id × document_version_id
```

### 操作

```text
未読 -> INSERT
既読 -> 必要ならread_at更新
```

### 要件

- idempotent
- concurrent-safe
- UPSERT可能
- unique constraint必須

### 性能想定

設計値として約1,000利用者 × 最大30閲覧/日なら、最大約30,000 ReadState更新/日程度を初期負荷想定に利用可能。

---

## T10: 文書全体の公開を終了する

通常業務では論理削除・物理削除を行わない。

文書を今後の通常利用・通常検索対象から外す必要がある場合は、削除ではなく公開終了として扱う。

### 基本動作

- current DocumentVersionを `WITHDRAWN` とする
- `withdrawn_at` と理由を記録する
- `Document.current_version_id` を業務ルールに従って更新する
- 原本・過去Version・Auditは保持する
- Search Platformへ通常検索対象から外すためのイベントを送る

### 方針

```text
検索対象から外す
!=
データを削除する
```

過去資料としての参照可否はAccessPolicy / 検索条件で制御する。

### 物理削除

通常のDocument lifecycleには含めない。

誤登録した機密情報、法令・契約上の削除義務、staging/orphan fileのGC等、例外的な管理処理のみ後続要件で定義する。


---

## T11: Search Index更新イベントを登録する

### 方針

Transactional Outbox Patternを採用可能であることを必須要件とする。

### 同一transactionに含むもの

- 業務データ変更
- Outbox Event追加

### transaction外

- Extraction
- Search Index更新
- Vector生成
- Cache invalidation

---

## T12: Audit Eventを登録する

### 方針

業務上必須のAudit Eventは対象transactionと同時に失われないこと。

### 実装選択肢

1. Audit Event自体をtransactional tableへ書く
2. Outboxへaudit eventを登録し専用Audit Storeへ配送

v0では方式を固定しない。

---

# 6. File Storage Commit Protocol

DBとFile Storageは単一ACID transactionを構成できないことを前提とする。

## 6.1 必須特性

- incomplete uploadを公開しない
- orphan fileを検出可能
- missing fileを検出可能
- retry可能
- idempotentであること
- content hashを保持可能

---

## 6.2 推奨状態モデル

```text
FileObject
├─ staging
├─ available
├─ failed
└─ orphaned
```

### 概念フロー

```text
1. staging領域へUpload
2. hash / size / MIME等を検証
3. DB transaction開始
4. FileObject論理レコード作成
5. DocumentVersion作成
6. Outbox / Audit追加
7. DB commit
8. ファイルをavailableへ確定
9. 失敗時はreconciliation対象へ
```

### 注意

7と8の間で障害が起こり得るため、reconciliation worker等で回復可能にする。

別案として、先にcontent-addressed storageへimmutableに格納し、その参照だけをtransactionで確定する方式も候補とする。

DB選定時にはどちらも実現可能であることを確認する。

---

# 7. Transactional Outbox Requirements

## 必須要件

Outbox Eventは以下を持つ。

```text
event_id
event_type
aggregate_type
aggregate_id
aggregate_version
occurred_at
payload / reference
processing_state
attempt_count
```

## 配送要件

- at-least-once deliveryを基本とする
- consumer側をidempotentにする
- duplicate eventを許容しても結果が壊れない
- retry可能
- dead-letter / failed stateを観測可能

## Search Platform側

`document.version.published` 等を受けて、

```text
Extract
-> Normalize
-> Generate Search Representations
-> Index
```

を実行する。

---

# 8. Search Consistency Requirements

## 8.1 正本とIndexの整合

Search IndexはDocument Platformより遅延してよい。

ただし以下は観測可能でなければならない。

- source version
- indexed version
- index lag
- failed indexing
- last successful indexing time

---

## 8.2 Stale Result

検索結果が旧Versionを返した場合、Document API側で `lifecycle_state` と `Document.current_version_id` を用いて現行性を検証可能であること。

UI / LLM側へ検索結果を返す際にversion / source revisionを保持する。

---

## 8.3 Rebuild

Search Index全損時にDocument Platformおよび他Knowledge Sourceから再構築可能であること。

---

# 9. Isolation Requirements

DB製品選定前の論理要件として以下を要求する。

## 9.1 通常CRUD

Read Committed相当以上で成立可能であること。

## 9.2 current version切替

同時公開によるwrite skew / lost updateを防止できること。

方式は以下いずれかを許容。

- row lock
- optimistic revision check
- stronger isolation level
- unique / exclusion constraint等

## 9.3 ReadState

Unique Constraint + UPSERTにより並行書込みを安全に処理できること。

## 9.4 Folder更新

parent / child整合性をtransaction内で検証可能であること。

---

# 10. DBに要求する機能

## 必須

- ACID transactions
- atomic commit / rollback
- Foreign Key
- Unique Constraint
- composite unique key
- UPSERT相当
- concurrent writersへの対応
- optimistic concurrencyを実装可能
- row-levelまたは同等の競合制御
- schema migration可能
- transaction内でOutboxを書ける
- Rustから成熟したdriver / libraryで利用可能

## 強く希望

- MVCC
- recursive CTE / hierarchical query
- partial index
- expression index
- JSON / extensible metadata support
- transaction isolation levelを選択可能
- efficient composite indexes
- background worker向け安全なclaim処理

## 将来拡張として評価

- replication
- PITR
- online backup
- read replica
- HA
- logical replication / CDC
- failover tooling

これらはv0で必須SLAを定めないが、将来必要になった際に移行不能にならないことを評価する。

---

# 11. DB選定で比較すべき固有機能

DB候補比較では、単なる性能ベンチマークではなく以下を評価する。

## Transaction / Concurrency

- 同時writerモデル
- lock粒度
- MVCCの有無
- isolation level
- deadlock handling
- long transaction影響

## Integrity

- FK
- deferred constraint
- partial unique constraint
- check constraint
- recursive hierarchy表現

## Outbox / Worker

- `SELECT ... FOR UPDATE`相当
- `SKIP LOCKED`相当
- notify / change notification
- CDC

## Metadata

- JSON型
- JSON field indexing
- schema evolution

## Operations

- migration
- backup
- restore
- corruption recovery
- replication
- monitoring
- tooling maturity

## Rust Ecosystem

- async driver
- connection pool
- migration library
- compile-time query validation等
- maintenance状況

---

# 12. 非要件

v0では以下をDB要件として固定しない。

- Search全文検索をRDBで実行すること
- Vector SearchをRDBで実行すること
- File binaryをDB BLOBとして保持すること
- Observability LogをDocument DBへ保存すること
- HA構成を初期導入時から必須とすること
- 特定DB固有機能への依存

Search Platformの検索Indexは別責務とする。

---

# 13. 未確定事項

以下は後続要件として確定する。

1. Version取下げ時のcurrent version挙動
2. `scheduled_publish_at` 到達時の自動公開実行方式
3. Document / Version metadataの境界
4. AccessPolicy model
5. Folder名重複ルール
6. FileObjectのcommit protocol詳細
7. Audit Eventの永久保存要否
8. Audit Storeへの配送方式
9. Outbox Event保持期間
10. 例外的な物理削除を認める条件と管理手順
11. SLA / RPO / RTO / HA要件

---

# 14. DB選定Gate

DB製品を採用候補とするためには、少なくとも以下を満たすこと。

```text
G1 ACID transactionを満たす
G2 DocumentVersion公開Invariantを安全に実装できる
G3 principal × document_versionの一意ReadStateを安全に管理できる
G4 Optimistic Concurrency Controlを実装できる
G5 Transactional Outboxを実装できる
G6 1,000利用者規模の並行アクセスに対応できる
G7 永続的に増加するmetadata / version情報を扱える
G8 Rust ecosystemが実運用に耐える
G9 Schema migrationが可能
G10 将来のbackup / replication / HA要件へ移行可能、または現実的な移行経路を持つ
```

---

# 15. 次工程

本v0を基に次の順で進める。

1. Transaction Catalog / Invariantのレビュー
2. 導出UIラベルと公開・取下げ処理のレビュー
3. AccessPolicyの最低限モデル確定
4. DB候補のlonglist作成
5. Gate評価
6. 固有機能比較
7. 想定WorkloadでPoC
8. DB選定

DB製品名を先に固定せず、**本書のtransaction・consistency要件を満たすかを基準として比較する。**
