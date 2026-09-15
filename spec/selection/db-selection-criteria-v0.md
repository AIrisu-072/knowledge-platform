# DB Selection Criteria v0

- Project: Knowledge / Document Platform
- Status: v0 / candidate-selection entry
- Research snapshot: 2026-09-14
- Scope: **Document Platform の transactional metadata store**
- Out of scope: Search Index、Vector Index、Document binary storage、Telemetry store
- Inputs:
  - Logical Data Model v0
  - Data Characteristics v0
  - Transaction & Consistency Requirements v0

---

## 1. Selection principle

DBは「利用者1,000人だから」「Rustだから」といった単一条件では選ばない。

以下の順序で評価する。

1. **Hard Gate**: 整合性・トランザクション・ライセンス・運用上の必須条件
2. **Weighted Criteria**: Hard Gate通過候補の相対評価
3. **PoC**: 実際のTransaction Catalogと想定Workloadで実測
4. **採用判断**

Search Platformは独立責務であり、全文検索・Vector Searchの有無をtransactional DBの主選定理由にしない。

---

# 2. Fixed requirements

## 2.1 Workload / data

| 項目 | v0 |
|---|---|
| 利用対象 | 約1,000人 |
| 文書数 | 現時点不明、100万件未満想定 |
| 既存蓄積 | 約12年 |
| 保存 | 原則永久保存 |
| 文書増加 | 継続増加 |
| ファイルサイズ | 概ね1〜10MB中心、ばらつきあり |
| ReadState | **principal × document_version** |
| 閲覧設計値 | 最大30文書/人/日程度 |
| 初期物理構成 | 1 Linux Serverも許容 |
| 将来 | Backup / PITR / replication / HAへ拡張可能であること |

Document binary自体はDB BLOBへ集約せず、File/Object Storageと論理的に分離する。

---

# 3. Hard Gates

候補DBは原則として以下を全て満たすこと。

## G1. License / governance

許容:

- PostgreSQL LicenseのようなBSD/MIT系permissive license
- Apache-2.0
- MIT
- BSD-2-Clause / BSD-3-Clause
- Public Domain

原則除外:

- copyleftを運用上避けたいDB
- source-available / 独自利用制限ライセンス
- 将来のself-host利用条件が不透明なもの

## G2. ACID / integrity

必須:

- multi-statement ACID transaction
- atomic commit / rollback
- Foreign Key
- CHECK constraint相当
- Unique Constraint
- Composite Unique Constraint
- UPSERT相当

## G3. Concurrency model

必須:

- 1,000利用者規模のserver applicationで現実的な並行writer処理
- lost update防止
- Optimistic Concurrency Controlを安全に実装可能
- current-version切替等で局所的な競合制御が可能

「DB全体で常時single writer」は本番候補として重大な減点要因とする。

## G4. Transaction Catalog support

以下を安全に実装できること。

- Document作成
- DocumentVersion作成
- current versionのatomic切替
- ReadState UPSERT
- Folder移動
- Metadata更新
- AccessPolicy更新
- Transactional Outbox
- Audit Eventの同一commit境界記録

## G5. Isolation / locking

必要:

- Read Committed相当以上
- row-levelまたは同等の局所競合制御
- revision条件付きUPDATE
- 同時公開時に1要求だけを成功させられること

## G6. Transactional Outbox

必須:

- business updateとOutbox insertを同一transactionへ含められる
- workerが未処理eventを安全にclaimできる
- retry / at-least-once deliveryを構成できる

`SKIP LOCKED` 相当は強く望ましいが、同等方式があれば可。

## G7. Schema / metadata

必須:

- schema migration
- relational modeling
- recursive hierarchyを現実的に扱える

強く希望:

- JSON型または拡張metadataを効率的に持つ仕組み
- JSON field indexing
- partial / expression index

## G8. Rust production ecosystem

必須:

- 成熟したRust driver/library
- async applicationから利用可能
- connection pooling
- transaction API
- migration tooling

## G9. On-prem operations

必須:

- self-host可能
- internet接続を前提としない
- Linux上で安定運用可能
- vendor cloudを必須としない

## G10. Long-term recovery path

v0でSLA値は固定しないが、以下へ現実的に拡張できること。

- online backup
- point-in-time recovery
- replication
- standby / HA
- monitoring
- corruption / disaster recovery

---

# 4. Weighted criteria

Hard Gate通過候補を100点で評価する。

| Criteria | Weight | 見るもの |
|---|---:|---|
| Transaction / consistency fit | 25 | MVCC、isolation、row lock、OCC、constraint |
| Operational fit | 20 | 1台開始の容易さ、保守負荷、運用知見 |
| Recovery / future HA | 15 | backup、PITR、replication、failoverへの道 |
| Rust ecosystem | 15 | driver、pool、migration、async、型安全性 |
| Data modeling | 10 | JSON、recursive、index、constraint |
| Outbox / worker ergonomics | 10 | safe claim、locking、notification / CDC |
| License / governance | 5 | permissiveness、将来利用条件 |

Hard Gateに明確に違反する候補は得点に関係なく本番Shortlistから外す。

---

# 5. Candidate longlist

## 5.1 PostgreSQL 18.x — **Primary shortlist**

### Gate assessment

| Gate | Result |
|---|---|
| G1 License | PASS |
| G2 ACID / integrity | PASS |
| G3 concurrency | PASS |
| G4 transaction catalog | PASS |
| G5 isolation / locking | PASS |
| G6 outbox | PASS |
| G7 schema / metadata | PASS |
| G8 Rust ecosystem | PASS |
| G9 on-prem | PASS |
| G10 recovery path | PASS |

### Rationale

- PostgreSQLはMVCCを採用し、read/writeの競合を抑えながらmulti-user workloadを扱える。
- row-level lockingを利用可能。
- `FOR UPDATE ... SKIP LOCKED` をqueue型workerで利用可能。
- WAL archiving + base backupによるPITR経路が標準で存在。
- physical / logical replicationの両経路を持つ。
- PostgreSQL LicenseはBSD/MITに近いpermissive license。
- RustではSQLx、tokio-postgres、Diesel等の成熟した選択肢がある。
- 現行majorは18。2026-08時点で18.6がcurrent minor。

### Risks / costs

- SQLite系よりDB server運用が増える。
- VACUUM、WAL、connection、backup等の運用設計は必要。
- Search IndexをPostgreSQLへ統合し始めると責務分離が崩れるため、使用範囲をtransactional metadataへ限定する。

### v0 status

**PoC対象 A / 第一候補**

---

## 5.2 SQLite 3.x — **Control / local-only candidate**

### Positive

- Public Domain。
- ACID。
- 単一ファイル・zero configuration。
- WAL modeではreaderとwriterを並行可能。
- Rust supportが非常に成熟。
- 小規模・local stateには極めて強い。

### Critical mismatch

SQLite公式は、write transactionをserialiseし、**1 database fileにつき同時writerは1つ**としている。
公式の用途ガイドも「many concurrent writers」が必要ならclient/server DBを推奨している。

今回のDocument Platformは:

- 1,000 users
- ReadState write
- version publication
- audit/outbox write
- future server split / HA

を持つため、single-writerを中心とする設計へ全体を寄せる積極的理由がない。

### v0 status

**本番Shortlistから除外。**
ただし以下には利用可能:

- unit/integration tests
- local developer tooling
- offline temporary state
- lightweight prototype

---

## 5.3 YugabyteDB — **Future distributed option**

### Positive

- Apache-2.0。
- PostgreSQL-compatible YSQL。
- strong ACID distributed transactions。
- row-level locking / Read Committed / Serializable等を提供。
- horizontal scale / multi-zone / HAを主用途とする。

### Mismatch for v0

今回の初期要件は:

- 1台開始可能
- server数・運用コストを増やしたくない
- Document DBの水平write scaleは現時点で要求されていない

YugabyteDBを初期採用すると、現在不要なdistributed-database operational complexityを先払いする。

### v0 status

**初期Shortlistから除外。**
将来、multi-node active HA / horizontal write scalability が必須になった場合の再評価候補。

---

## 5.4 libSQL — **Rejected for production metadata store**

### Positive

- MIT。
- SQLite互換。
- embedded replica / remote access。
- Rust driverあり。

### Critical mismatch

libSQL自身がSQLite由来の**single-writer model**を継承すると明記している。

### v0 status

**除外。**

---

## 5.5 Turso Database — **Watchlist**

### Positive

- MIT。
- Rustでfrom-scratch実装。
- async。
- MVCC / concurrent writersを志向。
- Rust-firstとして非常に魅力的。

### Critical issue

2026年現在もプロジェクト自身がBETAと明記し、mission-critical用途ではcautionを推奨している。
金融機関向けDocument Platformの正本DBとして、v0でこの成熟度リスクを取る理由がない。

### v0 status

**Watchlist。PoC研究対象には可、本番候補にはまだ入れない。**

---

## 5.6 CockroachDB — **License gate reject**

現在のCommunity Licenseは今回のpermissive-license policyと一致しない。

### v0 status

**G1 FAIL / 除外。**

---

# 6. Provisional shortlist result

## Production PoC

1. **PostgreSQL 18.x**

## Comparative baseline

2. SQLite 3.x — performance / implementation baselineのみ

## Future re-evaluation

- YugabyteDB — distributed HA / horizontal scale requirement発生時
- Turso Database — production maturityが十分になった場合

現時点では、Hard Gateをすべて満たし、初期運用コストと将来拡張のバランスが取れている候補は **PostgreSQLが明確に最有力**。

これはまだ「採用決定」ではなく、次のPoCでTransaction Catalogを実装し検証するためのShortlist判断である。

---

# 7. PostgreSQL PoC acceptance criteria

以下をPoCで実装・計測する。

## P1. current version atomicity

並行して2つのVersion publishを実行し:

- currentは必ず1つ
- lost updateなし
- loserはConflictとして検出

## P2. ReadState

`principal × document_version` に対して大量並行UPSERT。

確認:

- duplicateなし
- deadlock / latency
- composite index性能

## P3. Transactional Outbox

business transactionとOutbox eventを同時commit。

複数workerによるclaimを試す。

## P4. Folder hierarchy

recursive queryおよび移動時cycle validation。

## P5. Metadata

Document共通metadata / Version metadata / extensible metadataのquery/index性能。

## P6. Recovery readiness

PoCでは本番SLAを決めないが:

- backup
- restore
- WAL archiving / PITR構成可否

を手順レベルで確認する。

## P7. Rust integration

SQLxを第一候補として:

- transaction
- compile-time checked query
- migrations
- pool
- Postgres notification（必要なら）

を確認する。

---

# 8. Decision rule

PostgreSQL PoCが以下を満たした場合、Document Platform transactional metadata DBとして採用候補を確定できる。

- Transaction & Consistency Requirements v0の全Invariantを表現可能
- 1,000-user想定でDBがボトルネックにならない
- ReadState / Outboxの並行writeが安定
- Rust integrationに重大な欠点がない
- backup / future HAへの移行経路が確認できる

満たさない要件が発見された場合のみ、Longlistへ戻る。

---

# 9. Sources

- PostgreSQL License: https://www.postgresql.org/about/licence/
- PostgreSQL 18 documentation: https://www.postgresql.org/docs/18/
- PostgreSQL MVCC: https://www.postgresql.org/docs/18/mvcc-intro.html
- PostgreSQL PITR: https://www.postgresql.org/docs/18/continuous-archiving.html
- PostgreSQL Logical Replication: https://www.postgresql.org/docs/18/logical-replication.html
- PostgreSQL versioning/support: https://www.postgresql.org/support/versioning/
- SQLite isolation/concurrency: https://www.sqlite.org/isolation.html
- SQLite appropriate uses: https://www.sqlite.org/whentouse.html
- SQLite license/public domain: https://www.sqlite.org/copyright.html
- YugabyteDB FAQ: https://docs.yugabyte.com/stable/faq/general/
- YugabyteDB transactions: https://docs.yugabyte.com/stable/explore/transactions/
- libSQL: https://github.com/tursodatabase/libsql
- Turso Database: https://github.com/tursodatabase/turso
- CockroachDB Community License: https://www.cockroachlabs.com/cockroachdb-community-license/
