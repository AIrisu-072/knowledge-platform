# P7 共有 durable generation 基盤 — 設計案

Status: **DRAFT / 実装・backend 選定・実 DB 検証前**（2026-09-30）。これは P3 Graph と P6 Search delivery が共用する基盤の実装入力である。P3 の三候補の測定・選定、P6 の全縦断 receipt、P7 最終 composition/HTTP/運用の完了を主張しない。規範は `spec/`、P1/P3/P6 の Freeze と各 implementation plan、[P7 早期契約](p7-runtime-contract-draft.md) とする。抵触時は P3 の build guard 追補・FK-safe cleanup 修正、P6 改訂 1、P1 の本文追補・Partial 肯定修正を優先する。

## 1. 実装を先行できる境界

- P6-I03 が `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql` の **唯一の writer** となり、物理表 `search_source_coordination` と `search_index_receipts`、独立 ledger `search_runtime_sqlx_migrations` を作る。P3 の `source_control` はこの物理 Source 行の別名であり、二つ目の pointer/table/epoch を作らない。P7 は Search migration `0002` 以降を追加する。Domain `0009` → Search `0001` → P7 `0002+` を確認し、P3 Graph migration は別 schema/ledger とする。`search-runtime::migrate(&PgPool)` が Search 連番の単一入口である。
- `outbox_events`、Source 行、P7 READY/pin/guard、Search receipt は **同一 PostgreSQL database** で transaction を共有する。これは Graph 製品の最終採用ではない。[P3 isolated PoC 報告](../../../../experiments/search-graph-poc/report.md) は PostgreSQL 18.6 incidence を実装候補にしたが、[独立監査](p3-graph-poc-review.md) は P3-P04 の受入と production backend 昇格を **NO-GO** とした。PG は継続評価の第一候補である。Graph は `DurableGraphGenerationPort` と private transaction-bound Graph repository から接続する。非 PG 候補が選ばれたら、Graph READY・pin・GC と pointer CAS の同等 crash/fence protocol を別途凍結・審査するまで、その候補を production publish に接続しない。共有 PG coordination の schema/port を P3 全統合 receipt や P6 全統合 receipt 待ちにしない。Graph production migration/READY 接続は P3-P04 の再判定後に行う。最終 P7 は両 receipt と P1/P4/P5 を fan-in する。
- 依存方向は `search-core` 型 → `search-application` の SQLx-free port ← `search-runtime` adapter とし、`search-runtime` は Graph/storage・Document bridge・generic delivery を組み立てる。`search-source-document`、`search-graph`、`outbox-delivery` から `search-runtime` へ依存させない。`PgConnection` / `sqlx::Transaction` は runtime 内 private transaction-bound interface のみに現れる。Graph PG adapter を選ぶ場合も READY 確認、pointer、pin、GC は**同じ接続**で行い、別 pool の読み合わせを原子性と呼ばない。
- SQLx は workspace/lock の `0.9.0` を用いる。同版の [`Migrator::dangerous_set_table_name`](https://docs.rs/sqlx/0.9.0/sqlx/migrate/struct.Migrator.html#method.dangerous_set_table_name) で `search_runtime_sqlx_migrations` を指定できるが、この名称は最初の適用前に固定し、既適用 ledger を後から空の表に切り替えない。`migrate!` の埋込 source、独立 ledger の checksum/順序、Domain `_sqlx_migrations` と Graph ledger の非干渉を実 DB で検証する。第三者 `sqlx_migrator` の API を SQLx 本体の API と取り違えない。

## 2. 物理状態と移行 `0002+`

P6-I03 の Source 行は `source_id`、`fence_epoch`、`owner_token`/`lease_expires_at`、`current_generation_id`/`current_manifest_digest`/`current_bundle_digest`、`pointer_revision`、`last_published_epoch`、`build_fence_seq` を持つ。nullable 組は全て complete または全て NULL、counter は非負で overflow を拒否する。登録済み `SourceId` は全 tenant で一意、Source 行は取得前に一意 INSERT する。current key は `(source_id,current_generation_id)` であり `current_generation_id` 単独では参照しない。

`0002` 以降に以下を **additive** に置く。表名は Search namespace の提案で、`0001` の列・receipt PK・ledger を改名しない。`search_index_receipts` へ `bundle_version` を追加し、以後の書込み/同 epoch 照合に必須とする。既存行がある移行では version を digest から推定して自動 backfill せず、検証できない receipt を historical metadata として隔離し、現行 READY で再処理する。全子表の `(source_id,generation_id)` FK は `ON DELETE RESTRICT`、payload 書込 role と coordinator GC role を分離する。

P4 の in-process catalog は SourceId の過去の tenant 所有を restart 後に証明できない。`search_source_ownership` を **全 tenant・Document/Remote 共通の durable ledger** として同じ coordination DB に置く。`source_id UUID PRIMARY KEY`、host-issued bounded opaque `tenant_owner_key TEXT NOT NULL`、`source_kind TEXT NOT NULL`、`registration_revision BIGINT NOT NULL`、`activation_epoch BIGINT NOT NULL`、`state ACTIVE|TOMBSTONED`、allowlisted nonsecret versioned `registration_dto JSONB` とその digest、作成/更新時刻を持つ。tenant owner と SourceId は変更・削除不可で、tombstone を残す。別 tenant が使用済み SourceId を登録する試みは、現行 ACTIVE 行がなくても拒否する。全 tenant は同一 ledger を用い、別 DB ごとの独立採番を production へ持ち込まない。同一 tenant の明示的再有効化だけは新 activation epoch と新 generation key を要求する。

`0002` は既存 Source 行に nullable `tenant_owner_key`/`registration_revision`/`activation_epoch` と `registration_active BOOLEAN NOT NULL DEFAULT false` を追加する。P4 の SQLx-free `SourceRegistrationLedgerPort` は `reconcile`/`is_current` を `BoxFuture` で返す。P7 実 DB adapter の `reconcile(desired)` は **desired 全体で一原子決定**であり、`search_registration_serial` の一行を最初に lock し、既存 Source 行を SourceId 順、ownership 行を SourceId 順に lock して一 transaction で比較・更新する。Remote desired にない既存 Remote は tombstone とし、Document 行は消さない。初回 Source は global serial lock の下で ownership→Source を同 transaction に INSERT して commit まで見せない。Document 登録も同じ ledger/serial lock を使う。tenant owner は不変、registration revision と activation epoch は非負・単調で overflow fail closed。`RegistrationActivation(u64)` が PG `BIGINT` の上限を超える値も拒否する。登録 DTO/visibility revision の変更・削除・再有効化では activation epoch を進め、Source owner token を失効させ、`fence_epoch` も overflow を確認して進める。**ledger reconciliation は current key/manifest/bundle と `pointer_revision` を変更しない**。旧 current は activation mismatch で pin/read/reuse 不可、明示的な新 generation publish CAS でのみ置換する。tombstone 後の pointer 解除/旧 artifact 退役も別の coordinator transaction とし、current/pin/guard を検査する。DB role/trigger は ownership row だけの差替えや Source row の active/revision 単独変更を拒む。P6 Source lease の条件付き UPDATE は active・owner/activation binding が欠けた行を取得しない。actor read/pin は global serial/ownership row の lock を先に取らず、Source lock 下で current epoch を確認し、registration 更新と逆順 lock を作らない。`activation_epoch` は登録の有効化世代、`fence_epoch` は各 Source lease 取得の所有 fence であり混同しない。

移行済み `search_source_coordination` 行の tenant を SQL から推定しない。host-configured durable ownership port で全既存 SourceId と generation の owner/kind/revision を確認して ledger と Source 行を原子的に結合し、未対応・矛盾・重複・別 tenant 再利用が一件でもあれば startup/registration を拒否する。適用後は `(source_id,tenant_owner_key)` の一致と ledger 存在を FK/trigger と起動時 scan で検証する。production にこの port がなければ fail closed。P4 の `SourceRegistrationLedgerPort::is_current` は保存 DTO を復元して登録全体と activation を照合し、単なる process-local cache または digest 単独で current としない。P4 の actor-visible registry と `AuthorizedSourceScope` は ledger の ACTIVE・tenant・registration/activation revision を current check で照合し、Graph/Projection key だけから owner を推定しない。Remote RAM generation にも同じ global ownership gate を掛ける。Remote `desired` は全 tenant の一つの集合として渡し、tenant 別 catalog が他 tenant の行を tombstone にしない。複数 replica は同じ host-configured deployment revision と remote desired-set digest を用い、同 revision で異なる set、旧 revision からの再 reconcile を拒否する。`search_registration_serial` は migration/startup 時に一行を初期化し、この比較を DB 内で行う。

| 表 | 列・拘束と所有者 |
| --- | --- |
| `search_generation_identity` | `(source_id,generation_id)` PK、tenant owner key、activation epoch、作成時刻。全 durable build の最初に同じ transaction で一度だけ INSERT し、GC 後も**削除しない**。同 key の再割当て、別 tenant/activation への混入を DB で拒否する。NoRetention/SessionOnly の RAM generation は記録しない。 |
| `search_generation` | `(source_id,generation_id)` PK、Source FK、`state ∈ {BUILDING,READY,FAILED,DELETING}`、`source_snapshot`、versioned `projection_manifest`、`projection_manifest_digest`、`projection_resource_count`、`bundle_version`、`activation_epoch`、`ready_at`、`stage_event_id?`/`stage_source_epoch?`。P1 manifest の source/key/snapshot/schema/count/digest と行を一致させる。generation key は再利用しない。 |
| `search_generation_payload` | `(source_id,generation_id,kind)` PK、`kind ∈ {projection,unit_manifest,body_coverage}`、`dto_version`、`payload JSONB`、検証済み logical digest/count。provider-neutral DTO として全 projection+semantic registry、全 `BodyItemEntry`/`KnowledgeUnit.text`、全 coverage entry をそれぞれ保存する。未知 DTO version/field、欠落、余分、重複、count 超過を拒否する。JSONB の物理 byte hash を P1 canonical digest とみなさず、復元 DTO から P1 の encoder で再計算する。 |
| `search_generation_receipt` | key PK、source snapshot、P1 `projection_digest`、`unit_manifest_digest/count`、`body_coverage_digest/count`、`lexical_digest/count/schema/analyzer`、`graph_input_digest/count`、`profile_set_digest`、P1 `composite_digest`、Graph backend/schema/source mapping/content digest/resource/relation counts、`vector_receipt?`、versioned receipt DTO。各 component key と source snapshot を揃える。`composite_digest` は `current_bundle_digest` と同じ 32-byte 値で、`0001` の Search receipt `bundle_digest` にも同じ version を明示して書く。 |
| `search_lexical_artifact` | key PK、trusted index root 内の key から導く相対 directory、index format/schema、sorted file-name+size+SHA-256 の tree digest、actual searchable Resource/Unit doc の logical digest/count、Unit seal digest/count、`finalized_at`。index bytes は versioned immutable directory に置く。外部 path を request/provider から受けない。Vector 採用時は同型の別 artifact row/receipt と bundle version を追加する。 |
| `search_generation_full_guard` | `(source_id,target_generation_id)` PK、推測不能 token、Source 内単調 `build_fence`、DB clock expiry、target FK `RESTRICT`。full build の target を Stage→READY→CAS の間保護する。incremental は P3 `search_graph.build_guard` が base と target の双方を保護し、同じ `build_fence_seq` namespace を使う。 |
| `search_evaluation_lease` | `(source_id,lease_id)` PK、server-issued evaluation ID、generation FK `RESTRICT`、manifest/bundle digest、DB clock `expires_at`、owner scope reference。external generation/evaluation ID は発行権限にならない。Remote evaluation RAM lease は別 lifecycle であり、この表に入れない。 |

物理 migration の最小 SQL 形は次のとおり。長さ上限、digest の `sha256:` lowercase hex、version allowlist、immutable trigger と role grant は migration で明示し、`CHECK` だけで READY 意味論を証明したと扱わない。`current_bundle_digest` と receipt の 32-byte digest は `sha256:` + 64 lowercase hex で表し、decode した P1 bytes と比較する。

```sql
CREATE TABLE search_source_ownership (
  source_id uuid PRIMARY KEY,
  tenant_owner_key text NOT NULL,
  source_kind text NOT NULL,
  registration_revision bigint NOT NULL CHECK (registration_revision > 0),
  activation_epoch bigint NOT NULL CHECK (activation_epoch > 0),
  state text NOT NULL CHECK (state IN ('ACTIVE','TOMBSTONED')),
  registration_dto jsonb NOT NULL,
  registration_digest text NOT NULL,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);
CREATE TABLE search_registration_serial (
  singleton boolean PRIMARY KEY CHECK (singleton),
  deployment_revision bigint NOT NULL CHECK (deployment_revision > 0),
  remote_desired_set_digest text NOT NULL
);
ALTER TABLE search_source_coordination
  ADD COLUMN tenant_owner_key text,
  ADD COLUMN registration_revision bigint,
  ADD COLUMN activation_epoch bigint,
  ADD COLUMN registration_active boolean NOT NULL DEFAULT false;
ALTER TABLE search_index_receipts ADD COLUMN bundle_version text;

CREATE TABLE search_generation_identity (
  source_id uuid NOT NULL REFERENCES search_source_ownership(source_id) ON DELETE RESTRICT,
  generation_id uuid NOT NULL,
  tenant_owner_key text NOT NULL,
  activation_epoch bigint NOT NULL CHECK (activation_epoch > 0),
  created_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,generation_id)
);
CREATE TABLE search_generation (
  source_id uuid NOT NULL REFERENCES search_source_coordination(source_id) ON DELETE RESTRICT,
  generation_id uuid NOT NULL,
  activation_epoch bigint NOT NULL CHECK (activation_epoch > 0),
  state text NOT NULL CHECK (state IN ('BUILDING','READY','FAILED','DELETING')),
  source_snapshot text NOT NULL,
  projection_manifest jsonb NOT NULL,
  projection_manifest_digest text NOT NULL,
  projection_resource_count bigint NOT NULL CHECK (projection_resource_count >= 0),
  bundle_version text,
  stage_event_id uuid,
  stage_source_epoch bigint,
  ready_at timestamptz,
  PRIMARY KEY (source_id,generation_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation_identity ON DELETE RESTRICT
);
CREATE TABLE search_generation_payload (
  source_id uuid NOT NULL, generation_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('projection','unit_manifest','body_coverage')),
  dto_version text NOT NULL, payload jsonb NOT NULL,
  logical_digest text NOT NULL, logical_count bigint NOT NULL CHECK (logical_count >= 0),
  PRIMARY KEY (source_id,generation_id,kind),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_generation_receipt (
  source_id uuid NOT NULL, generation_id uuid NOT NULL,
  source_snapshot text NOT NULL, receipt_version text NOT NULL,
  projection_digest text NOT NULL,
  unit_manifest_digest text NOT NULL, unit_count bigint NOT NULL CHECK (unit_count >= 0),
  body_coverage_digest text NOT NULL, body_item_count bigint NOT NULL CHECK (body_item_count >= 0),
  lexical_digest text NOT NULL, lexical_count bigint NOT NULL CHECK (lexical_count >= 0),
  lexical_schema_version text NOT NULL, lexical_analyzer_version text NOT NULL,
  graph_input_digest text NOT NULL, graph_input_count bigint NOT NULL CHECK (graph_input_count >= 0),
  profile_set_digest text NOT NULL, composite_digest text NOT NULL,
  graph_backend text NOT NULL, graph_schema_version text NOT NULL,
  graph_mapping_digest text NOT NULL, graph_content_digest text NOT NULL,
  graph_resource_count bigint NOT NULL CHECK (graph_resource_count >= 0),
  graph_relation_count bigint NOT NULL CHECK (graph_relation_count >= 0),
  vector_receipt jsonb,
  receipt_dto jsonb NOT NULL,
  PRIMARY KEY (source_id,generation_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_lexical_artifact (
  source_id uuid NOT NULL, generation_id uuid NOT NULL,
  index_relpath text NOT NULL, index_format_version text NOT NULL,
  tree_digest text NOT NULL, logical_digest text NOT NULL,
  searchable_doc_count bigint NOT NULL CHECK (searchable_doc_count >= 0),
  unit_seal_digest text NOT NULL, unit_seal_count bigint NOT NULL CHECK (unit_seal_count >= 0),
  finalized_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,generation_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_generation_full_guard (
  source_id uuid NOT NULL, target_generation_id uuid NOT NULL,
  guard_token uuid NOT NULL UNIQUE,
  build_fence bigint NOT NULL CHECK (build_fence > 0),
  expires_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,target_generation_id), UNIQUE (source_id,build_fence),
  FOREIGN KEY (source_id,target_generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE TABLE search_evaluation_lease (
  source_id uuid NOT NULL, lease_id uuid NOT NULL, evaluation_id uuid NOT NULL,
  generation_id uuid NOT NULL, activation_epoch bigint NOT NULL,
  manifest_digest text NOT NULL, bundle_digest text NOT NULL,
  expires_at timestamptz NOT NULL,
  PRIMARY KEY (source_id,lease_id),
  FOREIGN KEY (source_id,generation_id) REFERENCES search_generation ON DELETE RESTRICT
);
CREATE INDEX search_evaluation_lease_by_generation
  ON search_evaluation_lease(source_id,generation_id,expires_at);
```

`search_source_coordination` の nullable current key には `(source_id,current_generation_id) → search_generation` の複合 `ON DELETE RESTRICT` FK を追加する。移行前に非 NULL pointer の orphan を検出したら移行を停止し、既存行を黙って NULL にしない。公開・pin・readiness は Source 行/ledger が ACTIVE で tenant と activation epoch が一致し、candidate `search_generation.activation_epoch` および恒久 identity も同じ場合だけ通す。起動時には identity ledger の全 key を ownership ledger と照合し、異なる tenant owner、未知の legacy generation、key 再割当てを拒否する。歴史的 `search_index_receipts.generation_id` は **FK/pin にしない**ので、旧 generation の GC 後も event metadata を保持できる。古い expired evaluation lease row は同じ GC transaction で削除してから generation を消す。

`search_generation` と payload/receipt/lexical rows は READY 後に不変とする。全子表の INSERT/UPDATE/DELETE trigger は親 generation を lock して BUILDING のみ通常変更可とし、READY→BUILDING、READY 内容の mutation を DB grants/trigger で拒否する。`DELETING` と子 DELETE は coordinator GC role の限定 transaction のみ許す。P3 Graph 側も凍結 §4 の row-level fence を維持する。DB 管理者の trigger 無効化は trust boundary 外の監査対象である。

## 3. READY の意味と同一 key の証明

1. full build は Source row →恒久 identity の新規 INSERT →新規 `search_generation`/Graph generation → full guard を同じ短い transaction で登録する。incremental は Source row → target identity の新規 INSERT → **base/target key 順の両 generation** → P3 guard を最初の copy より前に登録し、Graph target と P7 target を同じ key/snapshot に束縛する。target を guard なしで他 process に可視化しない。identity PK conflict と `build_fence_seq` overflow は失敗し、別 key を発行する。Source snapshot の長時間 read、parser/lexical file I/O、Graph copy は Source row lock 外で行う。
2. Projection/Unit/coverage は上記 payload として DB に全内容を保存する。`ProjectionGenerationManifest.digest` は既存 projection-only v1 の `sha256:` 値のままとし、body を混ぜない。P1 Unit manifest は **Unit text を格納した DTO**から `text_sha256` を再計算して canonical digest、coverage は同じ authoritative item 集合から digest/count を再計算する。`Retryable` item、全 authoritative item の列挙・raw binding 不一致、profile pin 不一致なら READY にしない。`Completed+Partial` の検証済み Unit は肯定証拠になり得るが、coverage gap を残し、negative absence の完全性には使わない。
3. Lexical は staging directory を作り、全 file と directory を durable sync して immutable final path へ atomic rename し、親 directory も sync する。完成 index を新たに開いて全 searchable doc を列挙し、P1 logical lexical digest/count/schema/analyzer と tree digest を確認する。P1 本文追補どおり Unit manifest の全 Supported/Partial Unit と検索可能 Unit doc を**双方向に一対一**で seal し、Unsupported/FailedPermanent の doc は 0 とする。builder 入力 receipt だけでは READY にしない。ファイル消失・破損・別 key の流用は不可用とする。
4. Graph は P3 `recover_ready`/`validate_ready` が typed n-ary row、participant/owner/source mapping、temporal、count、Graph content digest を実体から再計算する。P1 `GenerationBundleReceipt.graph.digest` は **Graph staged input + Document owner mapping** の digest、P3 `GraphGenerationReceipt.graph_content_digest` は **Source ID/schema/Graph resource と relation 全体**の digest であり、同一値と仮定しない。`graph_input_count` は attachment 重複除去後の一意な typed relation 数と定め、P3 `relation_count` と照合する。両 receipt を一つの復元済み canonical Graph record 集合から独立計算し、key、source snapshot、mapping、schema、resource/relation count の対応を `GraphReceiptMappingV1` で検査する。P1 Graph input digest の exact encoder/test vector が固定・照合されるまで publish を拒否する。P3 content digest を P1 欄へコピーしない。
5. `GenerationBundleReceipt` v1 の `composite_digest` は P1 §3 の domain separator・field 順・count の式をそのまま再計算する。各 `ArtifactReceipt.key` は同じ `(SourceId,generation_id)`。Graph count は P1 v1 composite の入力外でも別 receipt の検査を省かない。Vector を production retrieval に選ぶ場合は vector artifact/key/count/digest/モデル/retention を含む **新しい bundle version と golden vector** を先に凍結し、v1 digest を黙って拡張しない。Vector 未採用なら v1 を維持する。
6. `validate_ready` は target row lock 下で保存 payload、index の実体、Graph receipt、retention grant、Source snapshot、上記対応を再検証し、最後に `search_generation` READY と receipt を一つの PG transaction で確定する。外部 index I/O は Source lock 中に行わず、事前検証後に immutable directory と DB binding を再確認する。READY は「公開可能な候補」であり current ではない。file-backed artifact は PG と原子的に保存できないため、CAS 前検証と後述の読取時 fail closed が保証境界となる。index/Graph だけ READY、または DB receipt だけ READY は不可。

## 4. SQLx-free scoped port と公開の線形化点

概念 signature は以下とする。Rust 実装時の既存 `SourceId`/`ProjectionGenerationKey`/P4 trusted 型を再利用し、SQLx 型や `PgPool` を application に出さない。`ReadyEvidence` は preflight 結果であり、publish 権限でも永続 pin でもない。

```rust
enum GenerationDomain { Durable, RemoteEvaluation }
enum ScopedExecution<'a> {
    Actor(&'a AuthorizedSourceScope),          // trusted resolver が発行
    System(&'a SourceFence),                   // P6 の DB lease を再検証
}
struct VerifiedBundle { key: ProjectionGenerationKey, manifest: ProjectionGenerationManifest,
                        receipt: GenerationBundleReceipt, activation_epoch: u64 }
enum BuildHandle { Full(FullBuildHandle), Incremental(BuildGuardHandle) }
enum ReadyEvidence { Durable(VerifiedBundle), Remote(SealedRamGeneration) }
enum ScopedPin { Durable(PinnedBundleLease), Remote(EvaluationRamLease) }
trait ScopedGenerationGate: Send + Sync {
    fn check_ready<'a>(&'a self, scope: ScopedExecution<'a>, key: ProjectionGenerationKey,
                       domain: GenerationDomain) -> BoxFuture<'a, ReadyEvidence>;
    fn pin_current<'a>(&'a self, scope: ScopedExecution<'a>, evaluation: DiscoveryEvaluationId,
                       domain: GenerationDomain, ttl: BoundedTtl) -> BoxFuture<'a, ScopedPin>;
}
trait DurableGenerationCoordinator: Send + Sync {
    fn begin_full<'a>(&'a self, scope: SourceFence, manifest: PersistableGenerationManifest)
        -> BoxFuture<'a, FullBuildHandle>;
    fn begin_incremental<'a>(&'a self, scope: SourceFence, base: VerifiedBundle,
                             manifest: PersistableGenerationManifest)
        -> BoxFuture<'a, BuildGuardHandle>;
    fn publish_if_current<'a>(&'a self, scope: SourceFence, expected: CurrentGenerationSnapshot,
                              candidate: VerifiedBundle, guard: BuildHandle)
        -> BoxFuture<'a, CasOutcome>;
    fn renew_pin<'a>(&'a self, pin: &'a PinnedBundleLease, ttl: BoundedTtl)
        -> BoxFuture<'a, ()>;
    fn verify_pin_before_return<'a>(&'a self, pin: &'a PinnedBundleLease)
        -> BoxFuture<'a, ()>;
    fn release_pin<'a>(&'a self, pin: PinnedBundleLease) -> BoxFuture<'a, ()>;
}
```

`ScopedExecution::Actor` は tenant、Source registration/visibility revision と現在 access を照合する。`System` は登録 Source、retention、Source token/epoch/DB expiry を照合し、外部 request から構築できない。P4 `RemoteOperation` は一評価・一 Source の sealed RAM generation と RAM lease だけを返し、`Durable` variant に変換しない。P6 staging は `System + Durable` の preflight を使い、event completion は同じ検証を **transaction 内に再実行**する。外部が supplied key を `pin_current` に渡して任意の未公開 READY を読む API は作らない。

`pin_current` の durable 枝は current pointer を lock なしで暫定読取して lexical 実体を先に preflight し、短い transaction で Source row を lock して **その時点の current pointer** と revision を再取得する。変わっていれば bounded 再試行、同じなら P7 READY/bundle と P3 Graph READY を generation lock 下で検査し、DB clock 期限付き lease を同 transaction に INSERT する。Source lock 中に lexical file I/O を行わない。commit 後に lexical を再確認し、欠損なら lease を release して失敗する。検証済み key/receipt/lease のみ返す。旧 activation の current pointer は CAS の expected state には使えるが `ReuseCurrent`/pin 成功にはならない。lexical file/Graph read はこの key を使い、結果返却前に新 transaction で lease、same key/digests、file/index の可用性、actor/source/item/field の現在権限を再判定する。Graph の `REPEATABLE READ READ ONLY` snapshot と P3 read verifier を通す。失効や破損で別 key に暗黙切替えず、結果全体を閉じる。**現行 Version/T10/Read/part/raw は常に Source 正本で再判定**し、Projection/Graph の保存済み AccessProjection や READY は現行権限を代替しない。

event completion の lock 順は `outbox_events` 一行 → Source 一行 → `(source_id,generation_id)` 昇順の P7/Graph generation（同 key 内は P7→Graph）→ target key 順の full/P3 build guard → lease ID 順 → Search receipt とする。manual rebuild は outbox row を取らず Source から開始する。Source-less copy/delta/validate batch は generation→guard のみで、後から Source を取らない。複数 Source は Source ID 順に別 transaction とする。Read Committed で連続 SELECT の snapshot 同一性を仮定せず、row lock と条件付き UPDATE で守る。

`complete_event_if_current` は P6 outbox token と DB expiry、Source token/epoch/DB expiry、`last_published_epoch <= epoch`、expected `(current key, manifest digest, bundle digest, pointer_revision)`、candidate の same-key READY と guard を同一短期 transaction で再検査する。pointer/revision/last epoch の CAS と `(source_id,event_id)` receipt の epoch 単調 upsert を同じ commit に置く。同 epoch は key・projection digest・bundle version/digest の完全一致時だけ冪等、古い epoch は `Lost`。`ReuseCurrent` / `Unchanged` / `Duplicate` も historical receipt 単独では成功にせず、**今の current READY 実体**を照合する。成功した incremental publish は同 commit で P3 guard を DELETE、full は full guard を DELETE。CAS 敗北時は pointer/receipt を書かず guard を保って明示 abort へ渡す。generic outbox `delivered_at` は別 transaction の P6 worker が token-fenced で更新する。manual rebuild は pending event を ack しない。

PG commit の応答を失った呼出しは `CompletionUnknown` とし、その呼出しに成功を返さない。DB 再接続後の current READY、pointer revision、receipt、fence を再読し、後続の fenced 再配送/明示 reconciliation で収束させる。Source lease acquire/renew の応答不明も所有を推測せず claim しない。lock timeout、`40001`、`40P01` は transaction 全体を rollback して同じ expected state/idempotency key で有限再試行し、毎回 DB 条件を再評価する。上限・counter overflow・DB 不明は明示 error、CAS 敗北を成功へ変換しない。

## 5. pin・guard・GC と crash recovery

- `retire_unpinned` / `discard_unpublished` / expired guard cleanup は Source lock の下で current、DB clock 時点の有効 evaluation lease、full guard、および P3 guard の **base と target の双方**を調べる。current、有効 pin、有効 guard のある key は削除しない。guard 失効を見た場合も単に無視せず、同じ lock 順の cleanup を完了してから GC を再判定する。Search receipt は GC pin ではない。
- 増分 copy は P3 凍結どおり sorted base `FOR SHARE` / target `FOR UPDATE` → guard lock、token/fence/DB expiry/base immutable receipt/target BUILDING/committed cursor を batch 開始・commit 直前に確認する。Source pointer revision の前進は有効 guard の copy を壊さない。full/incremental の target READY 後も guard は publish/abort まで保持し、期限切れ token は renew・copy・READY・publish で再利用不可。
- 期限切れ/明示 abort の target が unpublished かつ unpinned と確認できたら、一つの transaction で **P7 と Graph target を `DELETING` → 該当 guard DELETE → expired lease DELETE → target child DELETE → target generation DELETE** とする。実際の FK-safe 順は P3 participant→relation→resource→Graph generation、P7 payload/lexical/receipt→P7 generation であり、DB 外 index の unlink は commit 後である。base/target generation locks は commit まで保持する。`ON DELETE RESTRICT` の guard を残して親を消さない。途中 error は全 rollback で guard/target が両方残る。current/pin と expired guard の予想外の共存は integrity failure として削除しない。READY base は guard が消えた後、別 GC transaction でのみ退役候補になる。
- DB 外 index は **DB commit 後**に idempotent orphan sweep で削除する。commit 前 unlink による rollback 後の current 破壊を禁止する。削除失敗は保護されない余剰 bytes として観測し、current 再公開の理由にしない。staging directory の crash 残骸も key/DB state/guard を照合してから掃除する。外部 file の消失は DB READY を成功扱いする理由にならず、当該 key を quarantine/unavailable として新 key へ Source 正本から full rebuild する。
- restart は migration/role/registry を検査してから current の P1 manifest・全 payload・Lexical reopen/seal/tree digest・P3 Graph receipt・bundle version と composite digest を復元検証する。source row/READY/Graph/ファイルのどれかが不一致ならその Source の query/publish を fail closed にする。中断 BUILDING、失効 guard/lease、ack 不明は DB clock と fence に従って回収する。有効 guard を勝手に消さず、進行 cursor が検証不能なら別 key から再構築する。RAM remote generation/cursor/session は復元しない。backup→別 DB restore でも同じ key/digest と外部 index bytes の復元を個別に検査し、DB restore だけで「復旧」としない。

## 6. 保持条件と後続実装の切り分け

durable stage は `PersistableGenerationManifest` / `PersistableResourceProjection` と登録 Source の retention/field proof の双方を確認する。`NoRetention`・`SessionOnly` の remote 由来 bytes、Unit、Graph、index、receipt を PG、disk、outbox、backup に永続化しない。`PersistentDiscoveryMetadata` は本文由来 Unit/embedding の保持許可ではない。entry の Source/owner/scope/revision/lease が不明・失効なら拒否する。P4 の `NO_RETENTION` は評価 RAM と短命 disclosure に閉じ、P7 最終 composition は log/telemetry/spool/core dump の漏出 gate を別途満たす。typed config、secret resolver、Audit/OTel、HTTP/send lifetime、deployment と SLO は [P7 早期契約](p7-runtime-contract-draft.md) §§2,5–8 と最終 P7 設計で扱い、この共有基盤の READY だけでそれらを完了扱いしない。

| 先行 task / writer | 完了判定に必要な局所証拠 |
| --- | --- |
| P6-I03 → P7 `0002+` migration writer | 独立 SQLx ledger、Source 行一つ、複合 FK、READY/receipt/payload/lease/guard の CHECK・FK・trigger・role を実 PG で確認。既存 Domain 行と ledger 不変。 |
| P7 durable artifact writer | kill/reopen 後の Projection/Unit/coverage DTO 再計算、実 lexical doc seal/tree digest、Graph mapping、duplicate bundle/corrupt row/file の fail closed、v1 composite golden vector。 |
| P3/P7 coordination writer | full と incremental guard の登録・fence・copy/GC 競合、CAS/pin/expiry、`DELETING→guard DELETE→children DELETE→generation DELETE` と中間 fault rollback を独立 process/PG で確認。 |
| P6-S03 と bridge writer | outbox→Source→generation→guard→lease→receipt の commit/rollback、stale epoch、同 epoch digest conflict、publish commit unknown、別 ack unknown、GC 済み historical receipt の再配送を実 PG で確認。 |
| 最終 P7 assembly writer | P3 測定選定 receipt、P6 generic+Search 縦断 receipt、P1 body seal/coverage、P4 scoped RAM、P5 最終 access/API を fan-in。再起動/別 DB restore/複数 process、read/publish/GC、保持・観測・health、exact-head gate を別途記録。 |

**公開前の未確定条件:** P3-P04 の独立監査 NO-GO を追加 PoC と再審査で閉じる必要がある。P1 Graph input digest と P3 Graph content digest の再計算可能な `GraphReceiptMappingV1` encoder/test vector も実装・検証を要する。非 PG Graph が選ばれた場合は上記 atomicity に代わる審査済み protocol が必要である。これらが未成立の枝は READY/publish を停止する。共有 PG coordination schema/port、global ownership ledger、P6 Source lease、P1 durable payload、pin/guard/GC のうち Graph 非依存の局所実装はその間も進められる。
