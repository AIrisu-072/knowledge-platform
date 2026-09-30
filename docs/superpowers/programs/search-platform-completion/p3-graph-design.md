# P3 Durable HyperGraph — 設計草案

Status: **DRAFT / 未凍結**（2026-09-30）。本書は P3 の設計・PoC・実装境界を提案する。製品 backend の採用結果、ベンチマーク値、復旧成功を既成事実にしない。P3 の完了判定は後続の独立 architecture review、freeze、実測 PoC、実装、独立 code review、qualification receipt に従う。

## 1. 正本と現状

- 規範は [Search v0 Approved Design](../../specs/2026-09-28-search-discovery-platform-v0-design.md) §22–27、§34、§54、§62。Graph は Source ごとの派生 Projection で、canonical relation は `TypedRelationInstance` の typed n-ary HyperEdge。`discovery`／`semantic`／`evidence` namespace、role、qualifier、half-open temporal scope、authority、provenance、evidence refs を維持する。二項 shortcut を正本にしない。
- [Phase D acceptance](../../execution/search-discovery-platform-v0-acceptance.md) D7–D8 は `search-graph-memory` と Document Source の同一 generation vertical slice を確認した。Graph 専用 durable backend は deferred のまま。実 DB vertical は 5/5 PASS、最終 local full gate は disk exhaustion で GREEN 未取得と記録されている。P0 の hosted exact-head qualification は [Completion Program Status](../../execution/search-platform-completion-program-status.md) に別記録されている。
- `search-core/src/relation.rs` の `TypedRelationInstance::validate`、`search-core/src/graph.rs` の `GraphTraversalPlan`／`GraphPathEvidence`、`search-application/src/ports.rs` の `HyperGraphRetrieverPort` が Domain/Application 境界。`search-graph-memory/src/index.rs` は `(SourceId, GenerationId)` ごとの immutable resource/relation/incidence を構築し、`traversal.rs` は全 participant の現在権限と temporal を確認してから path を返す。これを**意味論 oracle**にする。
- `search-source-document/src/outbox.rs` の `MemoryDocumentIndexRuntime` は Projection／Tantivy／Graph を構築し、Projection pointer CAS を最後に行う。Graph auxiliary ownership は現在 `GraphOwnership` のメモリにある。再起動後にこれを失うため、P3 は所有関係も generation ごとに永続化する。P7 の consolidated runtime と pointer/pin の接続まで含めない限り、永続 Graph 単体を稼働済み Search runtime と呼ばない。

## 2. 不変条件と受入境界

1. 物理 key は常に `(source_id, generation_id, relation_id)` と `(source_id, generation_id, resource_id)`。`relation_id` 単独、ResourceId 単独を global identity として検索しない。relation の participant 全員が同じ Source generation の resource row を参照する。
2. `TypedRelationInstance` 一件を一つの relation row と participant 集合として保存する。`from_role`、`to_role`、`required_participants` は**同じ relation_id**に束縛して評価する。別 relation の borrower/product/collateral を接合した false composite、role swap、他 generation との接合はゼロとする。
3. relation identity、namespace/type、繰返し可能な role、`TypedValue` qualifier、`[valid_from, valid_to)`、authority、provenance、evidence refs を lossless に往復する。集合的な participant/evidence の順序だけを canonicalize し、`TypedValue::List` の順序は変えない。Graph path は各 step の relation ID、全 participant、role、evidence を保持する。
4. `GraphTraversalPlan::validate`、seed／hop／relation／branch／path budget、allowed namespace/type、authority、temporal target を oracle と一致させる。未実装の `stop_conditions` は受理しない。budget 超過は partial result でなく明示エラー。access/temporal/authority で不可視となる枝は branch budget の前に除く。
5. seed、target、**全 participant**に現在の Source access を generation-scoped owner mapping で評価する。Denied／Unknown／評価失敗は不可視。Graph row に保存した access projection は最終認可に使わない。欠落 seed と未認可 seed の外部挙動を区別しない。Graph path、relation、件数、timing を通じて未認可 Resource の存在を露出しない。
6. `NO_RETENTION`／session-only Source の relation は永続層へ入れない。Source の outage や remote miss から削除を推定しない。Graph path は retrieval trace であり、それだけで Primary claim evidence を成立させない。
7. 公開済み generation は不変。別 generation の build／incremental update は既存 pin の結果を変えない。Source pointer の CAS が負けた staging を廃棄し、現在 pointer を巻き戻さない。再起動後、同じ key と digest の READY artifact を読める。不一致・破損・欠落時は fail closed と再構築へ進む。

## 3. backend 候補と暫定推奨

| 候補 | canonical 表現・Rust 接続 | transaction／backup／運用 | 判断 |
| --- | --- | --- | --- |
| **PostgreSQL incidence** | relation table + participant/incidence table。既存 SQLx、PostgreSQL 実 DB fixture と同じ技術境界。`(source,generation,resource,role,relation)` の複合 B-tree を使う。 | FK、transaction、Source pointer CAS を同一 DB transaction に置ける。`pg_dump` は整合 snapshot、WAL/PITR は運用選択肢。DB の運用は既存系に乗るが、join／high-degree fan-out、index 書込み増、Graph と lexical file の非原子境界を実測する。 | **第一候補**。P3 PoC が correctness と容量・復旧 gate を満たした場合に freeze で採用。 |
| **Rust native persistent adjacency/index（redb）** | immutable relation record と `(resource,role)→relation IDs` を copy-on-write B-tree の KV table に置く案。pure Rust、MIT/Apache-2.0、ACID/MVCC。SQLx は不要。 | 単一 write transaction の直列化、別 PG pointer との原子性、ファイル配置／破損／live backup／restore の運用を追加設計する。安全な backup 方法を PoC で確認するまでファイルコピーを有効な live backup と扱わない。 | 同じ oracle fixture で比較する。PostgreSQL が測定上の制約に当たる場合の代替。 |
| **Dedicated graph DB（Neo4j Community）** | relation を **Relation vertex**、participant を role 付き接続として reify する必要がある。native property-graph edge のままでは n-ary identity を失う。Rust は公式 driver ではなく community `neo4rs`。 | Community は GPLv3、offline dump/restore。online backup は Enterprise 機能。別 service、Bolt、障害・認証・backup・version 運用が増え、PG pointer との分散 publish も要る。商用利用条件は採用前に別途確認。 | 比較 PoC のみ。現時点で製品採用しない。 |

PostgreSQL の multicolumn index の先頭列制約、transaction isolation と backup は [公式 Index](https://www.postgresql.org/docs/current/indexes-multicolumn.html)、[Isolation](https://www.postgresql.org/docs/current/transaction-iso.html)、[Backup](https://www.postgresql.org/docs/current/backup.html) による。PostgreSQL 本体の license は [PostgreSQL License](https://www.postgresql.org/about/licence/)。redb の性質と dual license は [開発元 README](https://github.com/cberner/redb)、transaction API は [redb API](https://docs.rs/redb/latest/redb/) による。Neo4j の model/運用判断は [edition と license](https://neo4j.com/pricing/)、[community Rust driver](https://neo4j.com/docs/getting-started/languages-guides/community-drivers/)、[backup matrix](https://neo4j.com/docs/operations-manual/current/backup-restore/) による。これらは機能・制約の資料であり、この repository における性能の証拠ではない。

**選定規則:** correctness／権限非漏洩／同一 generation 復旧／retention を必須 gate とし、その後に実測 latency、build 時間、書込み増、disk、運用手数を比較する。具体的 SLO を未測定のまま Freeze しない。PostgreSQL が gate を通れば単一 transaction 境界と既存運用の少なさを優先して採用する。通らなければ欠陥と再現条件を記録して redb／専用 DB を再評価する。製品 dependency は PoC と選定 receipt 以前に追加しない。

## 4. 提案する PostgreSQL 物理契約

Search 所有の `crates/search-graph/migrations/0001_search_graph_v1.sql` を**追加 migration**とする。Document 正本の `crates/document-repository-postgres/migrations` は変更しない。同じ PostgreSQL database 内の `search_graph` schema を使い、Search 専用 DB role にのみ table 権限を与える。既存 Domain/Application に SQLx 型を持ち込まない。

| table | key と主要列 | 制約・index |
| --- | --- | --- |
| `search_graph.generation` | `(source_id,generation_id)` PK、`graph_schema_version`、`projection_manifest_digest`、`source_snapshot`、`resource_count`、`relation_count`、`graph_content_digest`、`state`、timestamps | `state ∈ {BUILDING,READY,FAILED}`。`READY` は graph artifact の準備完了だけを意味し、公開 pointer ではない。count 非負、schema=`typed-nary-v1`。 |
| `search_graph.resource` | `(source_id,generation_id,resource_id)` PK、`valid_from/to`、`effective_from/to`、`access_subject_ref` | generation FK、半開区間の整合性。`access_subject_ref` は Source adapter 専用の opaque owner key、外部 candidate に返さない。Document/FolderPlacement は DocumentId に戻せる scoped key を保存する。 |
| `search_graph.relation` | `(source_id,generation_id,relation_id)` PK、namespace、type、qualifiers の versioned typed JSONB、temporal、authority、provenance、evidence refs、canonical payload digest | generation FK、非空 type、half-open interval。JSONB の型 roundtrip と canonical bytes の digest は adapter validation で確認。任意の自由文を evidence ref として無審査永続化しない。`(source_id,generation_id,namespace,relation_type,relation_id)` index。 |
| `search_graph.participant` | `(source_id,generation_id,relation_id,ordinal)` PK、`role`、`resource_id` | 同じ generation の relation と resource へ FK。`UNIQUE(source_id,generation_id,relation_id,role,resource_id)`。role 非空。`(source_id,generation_id,resource_id,role,relation_id)` incidence index。role は重複可能でも `(role,resource)` 重複は禁止。 |

Relation の属性だけを JSONB に保存して participant の index 列と矛盾させない。`stage`／`validate` 時に JSONB→`TypedRelationInstance` を再構成し、全 typed fields、参加者、canonical digest の一致を確認する。各 relation の participant がその generation の resource に存在することを FK と application 検証の両方で確認する。全 relation の attachment が同一 canonical definition であることを確認する。任意の `(resource,role)`→relation index は derived acceleration であり、canonical relation row を差し替えない。

Retrieval は seed と role で incidence を引き、候補の relation row と**その relation_id の participant 全集合**を読む。`RelationPathPattern` と `GraphTraversalPlan::allows` 相当を同一 relation へ評価し、generation-scoped current access を全員に確認してから target を列挙する。出力の並び・重複除去・candidate identity・path evidence は memory oracle と一致させる。SQL の `LIMIT` を認可・temporal・authority filter より前に掛け、正しい候補を隠したり budget 判定を変えたりしない。DB 上の scan cap が必要なら明示 `budget exceeded` を返し、結果を部分成功にしない。

## 5. port、generation と runtime 統合

`HyperGraphRetrieverPort::retrieve(generation, plan)` と `GraphRetrievalResult` は維持する。提案する追加 port は SQLx 非依存の `search-application` に置き、実装を `crates/search-graph` に閉じる。

```text
GraphResourceRecord { resource_ref, temporal: TemporalProjection,
                      access_subject_ref: Option<OpaqueSourceKey> }
GraphGenerationReceipt { key, projection_manifest_digest, graph_content_digest,
                         resource_count, relation_count, graph_schema_version }
GraphIncrementalDelta { changed_resources, retired_resources,
                        retired_relation_ids, replacement_relations }

DurableGraphGenerationPort:
  stage_full(manifest, source_retention, resources, relations) -> key
  stage_incremental(manifest, source_retention, base_key, delta) -> key
  validate_ready(key) -> GraphGenerationReceipt
  recover_ready(key, expected_manifest_digest) -> GraphGenerationReceipt
  discard_unpublished(key) -> bool
  retire_unpinned(key) -> bool

GenerationScopedGraphAccessPort:
  evaluate(key, resource_ref, access_context, access_subject_ref) -> AccessDecision
```

`stage_full` と `stage_incremental` は同じ canonical normalization を使い、`TypedRelationInstance::validate`、Source／retention／count／schema／role／resource closure を検査する。`validate_ready` は counts と graph digest を再読して READY にする。`graph_content_digest` は Source ID、schema version、sorted resource temporal/owner metadata、sorted relation payload/participant を含み、generation ID／build time を除く。同一 Source snapshot の full／incremental が同じ digest となる。既存 `ProjectionGenerationManifest.digest`（Projection 全体）とは別値として混同せず、receipt に両方を保存する。

Incremental は base READY の行を新 key へ複製してから、`changed_resources ∪ retired_resources` に接する旧 relation を全て除去する。これは意味論上の差分更新で、初期 PostgreSQL 案の物理コピー量は O(base rows) になりうるため測定対象にする。Source adapter は新 authoritative snapshot からその**影響 closure 全体**の `replacement_relations` を供給する。`retired_relation_ids` は明示削除も含む。新 relation の participant 全員が新 generation resource に揃わない場合、又は closure を証明できない partial Source では READY にせず full rebuild へ切り替える。同一 snapshot の full rebuild との digest／query parity を定期・qualification で照合する。単に旧 incidence をコピーして changed resource だけ上書きする方式は採らない。コピー費用が許容値を超えた場合は immutable segment 共有と generation-scoped delta を別 PoC で測る。

現在 pointer は Graph table に重複作成しない。P7 の consolidated `SearchGenerationCoordinator` が Source ごとの durable current pointer と evaluation pin lease を所有する契約にする。publish 順は (1) Projection／lexical／Graph の同一 manifest で staging、(2) 各 artifact の検証と durable readiness、(3) pointer CAS を最後に一回、(4) receipt 保存。PG pointer と Graph READY を同一 DB transaction で確認する。lexical 等の外部 artifact は CAS 前に存在・digest 検証し、CAS 後の欠落は generation を無言で rebinding せず fail closed／rebuild とする。CAS 敗北・build failure では Graph と他 staging を全て廃棄。公開後に receipt 保存だけ失敗した場合は公開 artifact を保持し、同じ event が `Unchanged` と receipt 保存へ収束する既存 D8 semantics を維持する。

`pin_current(source)` は durable pointer と同じ key の manifest／Graph receipt を検証し、evaluation lease を記録する。Query は lease の key を使い、後続の新公開を見ても旧 generation を読み続ける。GC は current または有効 lease のある generation を削除しない。`discard_unpublished`／`retire_unpinned` は同じ DB transaction で pointer と lease を確認し、coordinator が未接続なら READY の削除を拒否する。lease 期限切れ後の Query は別 key に静かに移らず明示失効。crash/restart 時は PostgreSQL READY row、pointer、lease、全 artifact digest を検査し、BUILDING／FAILED／CAS 敗北 artifact を cleanup、current artifact 欠落は Source snapshot から再構築する。Graph の restart 復元は P3 単体で試験し、全 index 間の公開・復元は P7 で試験する。

Document adapter は `MemoryDocumentIndexRuntime::build_graph_generation`／`discard_graph_generation` を durable adapter に置換する。`DocumentGraphAccessReader` の現在の process-local `GraphOwnership` は、`(source,generation,resource)`→DocumentId を `access_subject_ref` に永続化して読み直す。`DocumentCurrentAccessAdapter::evaluate_owned_document` に query ごとに現在認可を委譲し、同一 Folder の別 Document 権限を流用しない。Version／Document／FolderPlacement の scope と `validate_document_graph` の relation attachment 検査を保つ。Source adapter がない一般 Graph resource では現在 access を解決できない限り不可視にする。Graph path を Primary evidence へ昇格しない。

## 6. PoC と選定証拠

後続 PoC は production `Cargo.toml`／lockfile を変更しない隔離実験とし、三候補を同じ fixture generator、同じ `GraphTraversalPlan`、同じ oracle output で比較する。Docker は現端末で `docker info --format '{{.ServerVersion}} {{.Driver}}'` が `29.4.0 overlayfs` で exit 0。これは container daemon の到達性のみで、PostgreSQL PoC／backup／復旧の成功を示さない。

| fixture／注入 | 必須観測 |
| --- | --- |
| 3〜5 role の n-ary、同じ borrower の異なる product/collateral、role swap、繰返し role、同じ relation を二度使う path、複数 Source／同じ裸 ID | oracle と hit/path/evidence の完全一致、false composite 0、cross-generation/cross-Source join 0 |
| Authority、valid-from/to 境界、resource temporal、Denied/Unknown/error participant、Document/FolderPlacement owner、取消直後 | current access と temporal の一致、未認可 participant exposure 0、access error の存在秘匿、false accept 0 |
| 高 degree、seed/hop/relation/branch/path の境界値と超過、無効 stop condition | oracle と同じ complete result または同じ error class、partial success 0 |
| full→incremental（追加／変更／削除／relation closure）→同一 snapshot full、並行 build、CAS 敗北、build 中 crash | full/incremental digest・query parity、旧 pin 不変、未公開 artifact 不可視、敗北 artifact cleanup |
| kill -9／DB restart／backup→別環境 restore／Graph row・index 欠損／manifest digest mismatch | READY の再読、current/lease の整合、欠損時 fail closed と再構築、復旧手順の再現 |
| 合成 corpus の小・中・大、skew/high-degree、1/2/4/8 concurrent readers と writer、cold/warm | build・incremental・traversal p50/p95/p99、rows/relations/sec、peak RSS、disk/WAL、backup/restore 所要、index plan と I/O。実測環境、seed、commit、container image を記録 |

母集団・負荷と許容値は代表的 Source 契約に基づく PoC plan で固定する。客先実データは使わない。PG は `EXPLAIN (ANALYZE, BUFFERS)`、redb は transaction 待機／ファイルサイズ、Neo4j は query profile／Bolt 往復と別 service 障害を記録する。比較表は correctness gate の PASS/FAIL、測定値、Rust dependency/license、運用／backup 制約を同列に記載する。任意の一回の latency 測定や HTTP 200 を製品選定・復旧の根拠にしない。選定後は採用版の license/dependency check と production の実 DB canary を別途通す。

## 7. 後続の実装・検証範囲

1. **Freeze 前:** 独立 read-only architecture review が §2 の意味論、SQL schema、P7 pointer との境界、license を監査し、指摘を本草案に反映する。`p3-graph-freeze.md` は意味論と PoC 選定規則を凍結し、`p3-graph-plan.md` は実測手順と write scope を記録する。この段階で PostgreSQL の製品採用を宣言しない。
2. **P3 PoC→選定→isolated backend:** 凍結後、隔離 PoC で §6 を測り、同じ plan の選定記録に理由・版・実測・license/運用条件を追記する。その後 `crates/search-graph/**` に採用候補の migration、repository、`DurableGraphGenerationPort`／`HyperGraphRetrieverPort` adapter、full/incremental stage、digest、recovery、oracle parity tests を置く。既存 `search-core` の型を変えず、必要な port 追加のみ `search-application/src/ports.rs` に限定する。現 DAG の `p3-implement` write scope は `crates/search-graph` のみなので、port 変更が必要なら freeze/plan で scope を明記して controller が割当を更新してから着手する。
3. **Runtime wiring:** `search-source-document/src/outbox.rs` の Document graph writer/reader と ownership を durable adapter に接続する。P7 は source current pointer／pin lease／全 artifact publish-recovery の coordinator を担当する。P3 の scoped worker がこれら別 crate を暗黙編集しない。P3 receipt には isolated backend と runtime-connected の達成状態を別記し、P7 接続前に E2E 稼働を主張しない。
4. **Qualification:** 既存 `search-graph-memory/tests/hypergraph_contract.rs`、`search-source-document/tests/relation_projection.rs`／`generation_races.rs`／`vertical_slice.rs` 相当を durable adapter で再実行する。実 PostgreSQL の restart・restore・fault 注入、focused strict Clippy/fmt、architecture/dependency checks、独立 code review を行う。最終統合は実行 head の CI/Sandbox/PoC 結果を一度確認し、Draft PR のまま記録する。

本草案は backend 製品や SLO を Freeze した文書ではない。Design Freeze を破る必要、不可逆 migration、live deployment／credentials／支払契約、法務上の license blocker、重大な security tradeoff、業務意味論を変える代替案が判明したときだけ Completion Program の Hard Stop として親へ上げる。通常の schema/index/adapter 選択と PoC 修正は承認済みの自律作業範囲で進める。
