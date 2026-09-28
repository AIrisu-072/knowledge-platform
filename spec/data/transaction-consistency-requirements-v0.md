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
| `WORKING` かつ有効な予約の `scheduled_publish_at <= now` | 公開遅延（公開実行・再検証待ち） |
| `PUBLISHED` かつ `Document.current_version_id = self` | 現行版 |
| `PUBLISHED` かつ `Document.current_version_id != self` | 過去版 |
| `WITHDRAWN` | 公開終了 |

表示上の意味だけを理由に永続化stateを増やさない。Document 単位の T10「公開終了」は、専用の操作記録と null の現行版参照から導出し、Version 単位の `WITHDRAWN` とは区別する。

---

## INV-13: lifecycle属性整合性

最低限、以下を保証する。

- `PUBLISHED` なら `published_at` が存在する
- `WITHDRAWN` なら `withdrawn_at` が存在する
- `scheduled_publish_at` を持つ `WORKING` 版は、公開可能と判断済みであることを表現できる
- `scheduled_publish_at` は有効な予約台帳から同一transactionで更新する投影であり、時刻属性だけでは公開を実行しない
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
- 1個以上のauthoritative ContentItemを持つ新しいmanifest
- metadata変更
- actor principal

### 変更対象

- DocumentVersion
- ContentItem / FileObject reference
- Outbox Event
- Audit Event

### 初期状態

新規Versionは原則 `WORKING` として作成する。

Documentごとの `WORKING` は高々1版とする。第2版以降の新規版は作成時の現行 `PUBLISHED` 版を `base_document_version_id` として記録する。初版のbaseはnull。`version_no` はRepository transaction内で採番する。各authoritative ContentItemはDocument Semantic Inspectionを成功させ、baseとの意味上の差分を確認する。意味上の差分がない場合は新しいVersionを作らない。

公開前の表示ラベルは `approved_at` / `scheduled_publish_at` から「下書き」「非公開」「公開待ち」を導出する。

### 競合

同一Documentに複数利用者が同時Version作成する可能性を許容するかは業務ルールで制御する。

競合する作成要求のうち、同一Documentで成功できる `WORKING` Version作成は高々1件とする。Document revisionのOCCと短い行ロック、部分unique制約で守る。

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

第2版以降はtargetのbaseがtransaction時点のcurrentと一致し、すべてのauthoritative ContentItemのInspection・原本参照・公開品質が有効であることを再検証する。未解決Track Changes、埋込コメント、既存の無効または検証不能な署名は公開しない。初版の通常公開は既存のDocument Publish v0契約を維持し、初版の予約公開は期限到達時に追加のInspection・公開品質確認を行う。公開操作は既存のPublish operation ID/ledgerで冪等化する。

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

## T3a: DocumentVersionの公開を予約・実行・取消する

公開予約は `DocumentVersion` の新たな永続化stateではなく、予約台帳に保持する将来のPublish意図である。初版と第2版以降の `WORKING` Versionを対象にできる。

予約時はcaller UUIDv7のPublish operation ID、対象Version、base/current、Document revision、実行者Principal、未来のUTC日時、authoritative manifest/Inspection同一性を記録する。公開前提と品質を確認し、Document revisionを1増やし、`approved_at` と `scheduled_publish_at` の投影、Domain/Audit Outboxを同一transactionで更新する。`approved_at` は公開可能性を示し、独立した承認workflowを導入しない。同一operation IDの同一要求は結果を再生し、異なる要求はConflictとする。

期限到達後、durable workerが予約台帳の有効な意図を取得し、保存済みPublish operation IDでT3を実行する。DB時刻で期限到達を確認し、早期公開を拒否する。原本、Inspection、公開品質、対象Version、base/current、Document revisionを再検証する。複数workerが同じ予約を取得しても、Publish ledgerとDocument行ロックにより成功は1件だけとなる。Publishと予約完了は同一transactionでcommitする。

Document Management Basics v0 の認可付き経路では、worker は予約依頼者の現在の Principal・issuer 付き membership・有効期限を信頼済み resolver で再取得し、実行時の `read + publish` を access-state 共有 guard と同一確定 transaction 内で再確認する。service executor の権限で依頼者の不足を迂回しない。権限不足または identity の恒久的不正が確定した場合は既存の監査付き予約終端処理へ渡す。一時的な identity 障害は予約と Publish operation ID を維持して再試行し、公開も終端確定もしない。依頼者と executor を監査で区別する。既存の DB 時刻・DSI・品質・manifest・冪等性条件は保持する。

一時的な基盤障害は同じIDのまま再試行する。永続的な業務・整合性・品質failureでは公開せず、予約を終了して `scheduled_publish_at` を消し、Document revision、Domain/Audit Outboxを同一transactionで更新する。取消はcaller UUIDv7 operation IDと期待revisionを使う冪等・監査対象の操作とする。予約中の通常編集、再基準化、別IDでの手動公開は取消後に行う。Versionの永続化stateは `WORKING` のままである。

---

## T4: DocumentVersionを取下げる

### 前提

公開済みVersionを公開対象から外す操作。データ自体は削除しない。

### 変更対象

- `DocumentVersion.lifecycle_state -> WITHDRAWN`
- `DocumentVersion.withdrawn_at`
- `Document.current_version_id`（対象がcurrentの場合）
- `Document.revision`
- 影響を受ける有効な公開予約
- Audit Event
- Outbox Event

### UI表示

`WITHDRAWN` は UI 上「公開終了」と導出する。

### 確定したcurrent復帰ルール

取下げ対象がcurrentなら、そのVersionの `base_document_version_id` が直前の復帰候補となる。候補が引き続き `PUBLISHED` であり、原本・Inspection・公開品質を安全に確認できる場合に限り `current_version_id` を候補へ戻す。候補がない、すでに `WITHDRAWN`、または検証できない場合は `current_version_id = null` とする。より古い祖先を自動で探して公開しない。対象が過去版ならcurrentは変更しない。

対象Versionを `WITHDRAWN` とし、`withdrawn_at`、Document revisionの加算、復帰先またはnullへのcurrent切替、影響する予約の無効化、Domain/Audit Outbox、caller UUIDv7 operation IDの成功記録を同一transactionでcommitする。取下げ前後のcurrent IDと復帰不可の理由を履歴に残す。`withdrawal/restored` 等の追加フラグやlifecycle stateは作らない。復帰したVersionの本文と元の `published_at` は変更しない。

### 守るInvariant

- 取下げ後もVersionおよびFileObjectは保持する
- `WITHDRAWN` Versionを `current_version_id` が指し続けない
- currentに復帰するVersionは同じDocumentの `PUBLISHED` 版であり、公開できる状態にある
- Search Platformへ除外・再Indexイベントを確実に通知する


---

## T5: Document Metadataを変更する

### 方針

Version固有metadataとDocument共通metadataを分離する。

### 競合

Optimistic Concurrency Controlを基本とする。

### 守るInvariant

更新対象のrevision一致。

Document Management Basics v0 は Document 共通属性 `document_type`、`owning_department`、`category`、`extensions` のみを set/unset する。対象外の既存キーを保持し、Version 固有 metadata・内容・原本は変更しない。期待 revision 不一致は同値要求でも Conflict とし、一致した同値要求は結果台帳へ `unchanged` を記録して revision・mutation event を増やさない。実変更は PENDING 公開予約中に拒否し、T10 後は `read + write + read_history + administer` を要求して公開状態を復帰させない。実変更、管理台帳、DocumentMetadataChanged、必須 Audit を同一 transaction に含める。

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

Document Management Basics v0 の移動は、Document の `read + write + administer` と旧親・新親 Folder の `administer` を要求する。旧親一致、期待 Document revision、両 Folder の利用可否を確認し、継承 policy の変更を access-state 排他 guard 下で確定する。PENDING 予約中の実移動は拒否し、T10 後は `read_history` も要求する。明示 Document policy、Version、原本、ReadState は保持する。同一場所は `unchanged`、実移動は Document revision と access_revision を各 1 増やし、管理台帳・DocumentMoved・必須 Audit と原子的に記録する。

---

## T7: Folderを作成・移動する

### 変更対象

- Folder
- parent_folder_id
- revision

### 守るInvariant

- Folder cycle禁止
- parent存在保証
- 同一parent下での正規化済み名前重複は禁止（大文字小文字は区別）

Document Management Basics v0 では、正規化済み Folder 名を同一親の下で大小文字を区別して一意にする。root は通常操作で変更しない。作成・改名は対象 revision と管理台帳、FolderCreated/FolderRenamed、必須 Audit を原子的に記録する。移動は対象・旧親・新親と、実効 policy が変わる継承対象の変更前 `administer` を必要とし、cycle、PENDING 予約を持つ配下 Document、期待 revision を排他 access guard 下で検査する。実変更時だけ対象 Folder revision と access_revision を増やし、FolderMoved と必須 Audit を同時 commit する。子孫 Document revision は一括加算しない。無検査の部分移行や既存 Folder の自動改名はしない。

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

### Document Management Basics v0 の追加契約

信頼済み identity adapter が本人 Principal、issuer 付き group/role、有効期限、実行種別を検証する。未検証、期限切れ、root policy 未設定は fail closed とし、認証なし allow-all は置かない。v0 は Folder/Document の allow-only policy とし、最も近い明示 policy が全操作を置換する。`read`、`read_history`、`write`、`publish`、`administer` は独立で、明示空 policy は拒否する。初回 root policy は信頼済み bootstrap 専用操作とし、通常 T8 は変更前 `administer` を必要とする。

通常の認可付き変更は単一 access-state 行を共有ロック、T8 と移動は排他ロックし、対象 Folder ID 順、Document ID 順、policy/操作台帳の順でロックする。確定 transaction 内で現在 policy と identity 有効期限を再検査する。T8 実変更は policy revision と access_revision を増やし、Document revision は増やさない。操作 ID と正規化 digest による完全再実行を記録し、再生結果の開示にも現在認可を要求する。異要求の同 ID は Conflict、同値 no-op は台帳だけを記録する。変更と AccessPolicyChanged、必須 Audit の原子性を保証する。権限変更は PENDING 予約があっても可能で、予約の期待 revision を書き換えない。

Windows/AD 等の実接続と membership 反映遅延は後続設計とする。

---

## T9: 文書を既読にする

### 論理キー

```text
identity_provider × principal_id × document_version_id
```

### 操作

```text
未読 -> INSERT
既読 -> 保存済みfirst_read_atを再生し、更新しない
```

### 要件

- idempotent
- concurrent-safe
- UPSERT可能
- unique constraint必須

Document Management Basics v0 では、信頼済み HumanInteractive 本人による現行 `PUBLISHED` Version の明示確認だけを登録する。access-state 共有 guard、Document lock、現在の `read`、Version 一致、T10 未終了を確認する。参照だけで既読にせず、過去版の新規確認も拒否する。初回 INSERT と `document.version.read_confirmed` 必須 Audit は同一 transaction、重複時は追加 event なし。Document revision は増やさない。

### 性能想定

設計値として約1,000利用者 × 最大30閲覧/日なら、最大約30,000 ReadState更新/日程度を初期負荷想定に利用可能。

---

## T10: 文書全体の公開を終了する（Versioning v0のT4とは別操作）

通常業務では論理削除・物理削除を行わない。文書全体を今後の通常利用・通常検索対象から外す必要がある場合は、T4 の Version 取下げではなく独立した公開終了 transaction を使う。T4 は直前の公開版を current に戻し得るため、T10 の代用にならない。

### 前提と操作 ID

T10 の対象は現行 `PUBLISHED` Version を持つ Document に限る。呼出側の UUIDv7 操作 ID、Document ID、期待 Document revision、期待現行 Version ID、実行者、空でない理由を要求する。期待 revision・現行版の不一致は Conflict、現行版がない場合は業務上の拒否とする。同じ ID・同じコマンドは保存済み結果を再生し、異なるコマンドは Conflict とする。別 ID による二度目の公開終了は拒否する。commit 結果が不明なら同じ ID で照会・再試行する。

### 同一 transaction に含むもの

- Document 行をロックして現行版・revision・Version の所属と `PUBLISHED` 状態を再確認する。
- `Document.current_version_id = null` とし、revision を 1 増やす。元の現行 Version は `PUBLISHED` のまま、`published_at`・原本・過去記録を保持する。`WITHDRAWN` にしない。
- その Document の有効な公開予約を終端化し、対応する `scheduled_publish_at` 投影を消す。予約履歴は保持する。
- 冪等な公開終了の操作結果、検索対象から Document 全体を外す Domain Outbox Event、必須の Audit Outbox Event を記録する。いずれかの生成失敗では transaction 全体を commit しない。

操作記録は現行版参照が null になった理由を区別する永続証跡となる。新しい Document フラグや Version lifecycle state は追加しない。T10 後、通常の Version 作成・更新・再基準化、予約・期限到達・手動 Publish は、新しい現行版を設定できない。各変更 transaction はロック下で T10 記録を確認し、事前検査だけに依存しない。再公開は別操作として後続設計する。T10 は内容を復帰させないため、Storage・DSI の障害を理由に妨げない。

### 読み取りと Search

通常公開用の Document・ファイル読み取りは、同じ Document の現行 `PUBLISHED` Version のみを返す。現行版参照が null なら結果を返さず、過去版や `WORKING` Version へフォールバックしない。既存の編集・authoritative 読み取りは T10 未終了の `WORKING` 初版を扱えるが、T10 終了後は旧版を返さない。終了記録の確認と取得は同じ DB statement で行う。権限に基づく過去資料の参照は AccessPolicy を使う別経路とする。

Document Management Basics v0 の認可付き履歴経路は、明示 Document/Version ID と `read + read_history` を要求し、残存 `WORKING` の内容にはさらに `write` を要求する。この経路は通常公開用の取得へフォールバックせず、T10 の `current_version_id = null` や Version の内容・状態を変更しない。

Search Index からの除外配送は遅延してよい。ただし検索結果を表示・利用する前に、Document 側で結果の Version が現行 `PUBLISHED` 版か確認し、T10 後の古い結果を抑止する。再構築元も現行 `PUBLISHED` 版だけを列挙する。Search consumer と再構築処理そのものは別機能とする。

```text
検索対象から外す
!=
データを削除する
```

物理削除は通常の Document lifecycle に含めず、例外的な管理処理として別に扱う。Document Versioning v0 は T10 を実装しない。


---

## T11: Search Index更新イベントを登録する

### 方針

Transactional Outbox Patternを採用可能であることを必須要件とする。

### 同一transactionに含むもの

- 業務データ変更
- Outbox Event追加

Document Management Basics v0 の T5/T6/T7/T8 実変更では、型付き対象参照・対象 revision・操作 ID を持つ Domain Outbox を業務データと同時に記録する。T9 の初回既読確認や単純参照には Search Index 更新イベントを要求しない。Search consumer と配送 worker は本機能の実装対象外とする。

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

Document Management Basics v0 では既存の Audit Outbox staging を Document / Folder / AccessPolicy の型付き対象へ拡張する。T5/T6/T7/T8 の実変更、T9 の初回既読確認、原本バイト開示前の `document.file.access_granted` は必須 Audit を業務 transaction と同時に生成する。完全再実行、同値 no-op、既読重複では mutation Audit を増やさない。認可拒否の `authorization.denied` は独立 transaction に記録し、記録失敗でも拒否を許可へ反転しない。通常一覧・metadata・履歴の単純参照は v0 では全件 Audit 対象にしない。

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

検索結果が旧Versionを返した場合、Document API側で `lifecycle_state` と `Document.current_version_id` を用いて現行性を検証可能であること。T10 の公開終了後は現行版参照が null となるため、Index からの除外配送を待たずにその Document の通常検索結果を抑止する。

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

Document Versioning v0の承認済み設計は、Version取下げ時のcurrent挙動と `scheduled_publish_at` 到達時の公開方式を確定した。文書全体の公開終了は、承認済みの Document Publication End v0 設計に従う別の T10 操作であり、本番実装は別途計画する。

1. Document / Version metadataの境界
2. AccessPolicy model
3. Folder名重複ルール
4. FileObjectのcommit protocol詳細
5. Audit Eventの永久保存要否
6. Audit Storeへの配送方式
7. Outbox Event保持期間
8. 例外的な物理削除を認める条件と管理手順
9. SLA / RPO / RTO / HA要件

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
