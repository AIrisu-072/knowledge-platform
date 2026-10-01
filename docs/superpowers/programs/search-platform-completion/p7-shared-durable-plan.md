# P7 Shared Durable Generation — Production Implementation Plan

> **実行者:** Toolbox v8 の `orchestrate` → `toolbox-context` で、各 task を一責務・限定書込範囲の managed worker に渡す。各 task の RED、GREEN、独立 read-only review を記録する。native `spawn_agent` に黙って切り替えない。

**Goal:** P1 の完全な generation bundle、P3 の選定済み Graph、P6 の fenced delivery が、同一 PostgreSQL の Source/current/READY/pin/guard/receipt と復元可能な索引実体を安全に共有する。

**Architecture:** P6 Search `0001` を一つの Source 行と migration ledger の正本とし、P7 `0002` が全 tenant/Source kind の ownership、`0003` が generation/guard/pin の拘束を追加する。SQLx-free application port の背後で `search-runtime` が一接続の短い transaction を所有し、純粋な canonical codec、DB に保存した全 payload、実 lexical file、条件付き P3 Graph を別々に検証する。P3 production 資格が成立するまで、Graph 非依存の schema/port だけを局所受入とし、READY と公開は閉じる。

**Tech stack:** 既存 Rust workspace、SQLx `0.9.0`、PostgreSQL `18.6`、`testcontainers`、`sha2`、`serde_json`、既存 Tantivy。新しい未資格 dependency は導入しない。

**Spec:** `spec/data/logical-data-model-v0.md` P1-L1〜L3、`spec/data/transaction-consistency-requirements-v0.md` SD-T11〜T13/P6 outbox、`spec/selection/library-tool-selection-v0.md`。実装入力は [元設計](p7-shared-durable-design.md) SHA-256 `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` と [改訂 1](p7-shared-durable-revision-1.md) SHA-256 `2e2f1a24972f2020278cfe7fe37c1726ae280e936685ef74299c1c5c2a8641fe` の合成で、抵触する五件は改訂を優先する。[独立再審査 GO](p7-shared-durable-recheck.md) と [freeze](p7-shared-durable-freeze.md) は**設計判定だけ**である。P1 Extraction/KnowledgeUnit、P3 Graph/build guard/FK-safe cleanup、P5 source-neutral 改訂 2、P6 Outbox の各 freeze/plan も拘束する。

## Global constraints and gates

- P6-I03 が `crates/search-runtime/migrations/0001_search_source_coordination_v0.sql` と `search_runtime_sqlx_migrations` の唯一の writer。P7 は `0001` を改名・再採番せず、Search `0002` 以降だけを書き、`search-runtime::migrate(&PgPool)` の既存単一入口を使う。Domain `0009` → Search `0001` → P7 `0002+`。Graph は別 `search_graph` schema/ledger。SQLx `Migrator::dangerous_set_table_name` の固定名称を途中で変更しない。
- `outbox_events`、Source、ownership、generation、guard、pin、Search receipt は同一 DB。P3 `source_control` は `search_source_coordination` の別名であり、第二の pointer/epoch、別 pool での擬似原子操作、Graph 側の Source mint を作らない。Search receipt は歴史的 metadata で generation FK/GC pin にしない。
- P3-P04 の native backend 選定/独立再判定と、P1 staged-input・P3 content の**別々の** `GraphReceiptMappingV1` canonical encoder/golden vector の GO 以前は Graph production migration、Graph READY、P7 READY、publish、durable pin 成功を許さない。非 PG が選ばれたら同等の atomicity/fence protocol の設計・独立審査まで Graph 接続 task を停止する。孤立 PoC の PASS は資格でない。
- P5 改訂 2 の `SourceRegistration::{Document,Remote}`、`RegistrationNamespace`、`CompleteDesiredRegistrations` と既存 P4 `TrustedSearchScope` / `AuthorizedSourceScope` を採用する。P4-02 の source-neutral implementation amendment と独立 code review を port 差替え前に満たす。二つ目の actor/Source mint や tenant 別 Remote reconcile 正本を作らない。
- P1 の `ProjectionGenerationManifest.digest` は projection-only v1 のまま。P1 composite v1 は別 32-byte digest と `bundle_version` を持ち、Source current と Search receipt の `sha256:` lowercase hex に一致させる。Vector 採用なら v1 へ暗黙追加せず新 bundle version/golden vector を先に凍結する。`Retryable`、欠損、未知 DTO/version/field、retention 不明は READY を拒否する。
- NoRetention/SessionOnly の remote 由来 bytes、Unit、Graph、索引、receipt、backup を永続化しない。`PersistentDiscoveryMetadata` は本文 Unit/embedding の保持許可ではない。P4 RAM evaluation lease は PG pin と別 lifecycle。
- 共有 root `Cargo.toml` / `Cargo.lock` は P6-I01・P1-I01・P3 writer と直列化した一 writer window でのみ変更し、本計画の既存 workspace dependency を再利用する。P6 generic outbox crate は Search/P7 crate を import せず、`delivered_at` は generic worker の別 transaction が所有する。P6-S02/S03、P3-C01/C02/C04、P1-B01/B02、P4-02 の同名ファイル writer は順番を固定する。
- `40001`、`40P01`、lock timeout は同じ expected state/idempotency key で transaction 全体を有限再試行し、毎回 DB 条件を再評価する。counter overflow、DB/commit 応答不明、canonical codec 未対応は fail closed。長い Source read、parser、file sync/scan、Graph copy を Source/outbox lock の中に置かない。
- SQL role は cluster 側で `sql/roles.sql` を限定管理者が作成・grant し、Search migration の後に権限を適用、startup が role/schema/trigger と実接続 role を検査する。欠落・権限過大なら API/claim を開かない。P6 Search completion が outbox を `FOR UPDATE` するための権限は P6-I04 の `UPDATE(lease_token)` 限定に合わせ、`delivered_at`/Audit/Document の UPDATE を与えない。

## File and sole-writer map

| Owner task | File / responsibility |
| --- | --- |
| P7-01 | `crates/search-runtime/migrations/0002_search_source_ownership_v1.sql`, `tests/source_ownership_migration.rs`: global ownership/namespace、既存行監査、role の第一段。P6-I03 の `0001` writer と直列。 |
| P7-02 | `crates/search-runtime/src/{source_registration,source_lease}.rs`, `tests/source_registration.rs`: P5 source-neutral desired 全体の atomic reconcile、P6-S02 lease の active/activation 条件。`crates/search-application/src/{remote_registration,scoped,ports}.rs` の型変更は P4-02/P6-S01 writer の後の専用 window。 |
| P7-03 | `crates/search-runtime/migrations/0003_search_generation_v1.sql`, `sql/roles.sql`, `tests/generation_schema.rs`: identity/generation/payload/receipt/lexical/guard/pin、複合 FK、immutability、実 role。Graph DDL は含めない。 |
| P7-04 | `crates/search-core/{Cargo.toml,src/{projection_bundle,lib}.rs}`, `crates/search-application/src/ports.rs` の Core 型 re-export、`crates/search-projection-memory/src/store.rs`, `crates/search-runtime/src/{payload,bundle_codec}.rs`, `tests/bundle_durability.rs`: 純粋 canonical 計算と typed DTO round-trip。P1-B01/B02 の型/codec writer と直列。 |
| P7-05 | `crates/search-runtime/{Cargo.toml,src/lexical_artifact.rs}`, `tests/lexical_artifact.rs`: staging/finalize/reopen/seal/tree digest と外部 bytes。P1-E01 の実 Unit doc 列挙を消費する。 |
| P7-06 | `crates/search-runtime/src/{generation_registration,full_guard}.rs`, `tests/full_guard.rs`: private EVENT/MANUAL 登録、恒久 identity、full token/fence と失効拒否。P6-S03/P3-C01 の同じ Source writer と直列。 |
| P7-07 | `crates/search-runtime/{Cargo.toml,src/graph_registration.rs}`, `crates/search-graph/src/{repository,stage}.rs`, `tests/graph_registration.rs`: **P3 条件付き**一接続 Graph parent 登録/子 batch port。Graph DDLは未適用なら P3-G03 の `0001` 単独 writer、適用済みなら Graph ledger の新しい additive migration 単独 writer が担当し、既適用 fileを書き換えない。P3-G01/G03/G04/C01 writer と共同編集せずその専用 window で改訂を反映。 |
| P7-08 | `crates/search-runtime/src/ready.rs`, `crates/search-graph/src/{canonical,recovery}.rs`, `tests/ready_bundle.rs`: **P3 条件付き**二 encoder mapping、Graph/lexical/全 payload READY。P1-B02/P3-G02/G08 の writer と直列。 |
| P7-09 | `crates/search-application/src/{ports,indexing_service}.rs`, `crates/search-runtime/src/{event_completion,manual_publication}.rs`, `tests/event_completion.rs`: **P3/P6 条件付き** event origin と pointer+receipt commit。P6-S01/S03/S04 の型/bridge writer と直列。 |
| P7-10 | `crates/search-runtime/src/pin.rs`, `tests/pin_scope.rs`: **P3 条件付き** actor scope pin/renew/return/release。P3-C04 writer と直列。 |
| P7-11 | `crates/search-runtime/src/gc.rs`, `tests/gc_races.rs`: **P3 条件付き** current/pin/guard を守る FK-safe GC。P3-C02/C03 writer と直列。 |
| P7-12 | `crates/search-runtime/src/recovery.rs`, `tests/process_restore.rs`: **全 fan-in 後** restart/kill/別 DB restore/corruption。root/Cargo と他の実装範囲を触らない。 |

`crates/search-runtime/src/lib.rs` の module 登録と `Cargo.toml` の既存 workspace edge 追加は P7-01→12 の当該 writer が順に一回ずつ行う。P7 は `crates/search-source-document/src/outbox.rs` を単独で上書きしない。P1-B02、P6-S04、P3-D01 が合流した後、P7-09 の専用統合 window で production path を接続する。`search-runtime/src/lib.rs` は現状 `migrate` だけであり、READY 実装済みとは扱わない。

## Interface contract to reconcile before code

`search-application::ports::BoxFuture<'a,T>` は `Result<T,SearchError>` を内包する。下記は production 目標で、現行の単一 `CompleteEventRequest.candidate` や Remote-only `SourceRegistrationLedgerPort` が実装済みという意味ではない。P4-02/P6-S01 の既存テストと呼出しを同じ writer window で移行し、公開 port に SQLx/PgPool を出さない。

```rust
// P5 改訂 2 の SourceRegistration/CompleteDesiredRegistrations を使う。
trait SourceRegistrationLedgerPort: Send + Sync {
    fn reconcile<'a>(&'a self, desired: &'a CompleteDesiredRegistrations)
        -> BoxFuture<'a, BTreeMap<SourceId, RegistrationActivation>>;
    fn is_current<'a>(&'a self, registration: &'a SourceRegistration,
        activation: RegistrationActivation) -> BoxFuture<'a, bool>;
}
enum CompleteEventRequest {
    PublishCandidate { fence: SearchDeliveryFence,
        expected_current: CurrentGenerationSnapshot,
        event_candidate: EventCandidateHandle, verified_bundle: VerifiedBundle },
    ReuseCurrent { fence: SearchDeliveryFence,
        expected_current: CurrentGenerationSnapshot,
        expected_manifest_digest: String, expected_bundle_digest: String },
}
trait DurableGenerationCoordinator: Send + Sync {
    fn begin_event_full<'a>(&'a self, fence: SearchDeliveryFence,
        manifest: PersistableGenerationManifest, ttl: BoundedTtl)
        -> BoxFuture<'a, EventCandidateHandle>;
    fn begin_manual_full<'a>(&'a self, source: SourceFence,
        manifest: PersistableGenerationManifest, ttl: BoundedTtl)
        -> BoxFuture<'a, ManualBuildHandle>;
    fn renew_full_guard<'a>(&'a self, handle: &'a FullBuildHandle,
        ttl: BoundedTtl) -> BoxFuture<'a, ()>;
    fn pin_current<'a>(&'a self, binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope, ttl: BoundedTtl)
        -> BoxFuture<'a, PinnedBundleLease>;
    fn renew_pin<'a>(&'a self, pin: &'a PinnedBundleLease,
        binding: &'a TrustedDiscoveryBinding, scope: &'a AuthorizedSourceScope,
        ttl: BoundedTtl) -> BoxFuture<'a, ()>;
    fn verify_pin_before_return<'a>(&'a self, pin: &'a PinnedBundleLease,
        binding: &'a TrustedDiscoveryBinding, scope: &'a AuthorizedSourceScope)
        -> BoxFuture<'a, ()>;
    fn release_pin<'a>(&'a self, pin: PinnedBundleLease,
        binding: &'a TrustedDiscoveryBinding, scope: &'a AuthorizedSourceScope)
        -> BoxFuture<'a, ()>;
}
trait HostScopeReferencePort: Send + Sync {
    fn issue<'a>(&'a self, binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, ActorScopeRef>;
    fn current<'a>(&'a self, reference: &'a ActorScopeRef,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessBindingState>;
}
```

`BoundedTtl` は host policy で min/max 検査済みの duration、`BuildHandle` は `Full(FullBuildHandle)|Incremental(BuildGuardHandle)`、`ActorScopeRef` は非秘密・非空・長さ上限付きで restart 後も host が再解決できる reference とする。`HostScopeReferencePort` は既存 trusted actor/source/evaluation の保存用 reference を発行・再照合する adapterであり、新しい actor mint ではない。`EventCandidateHandle`、`ManualBuildHandle`、`FullBuildHandle`、`VerifiedBundle`、`PinnedBundleLease` の field/constructor は private。P7 adapter は DB 行と host current gate を必ず再読し、handle 所持、external generation/evaluation ID、JSON DTO、preflight `ReadyEvidence` を権限にしない。manual は event handle に変換不可。incremental は P3 `BuildGuardHandle` の base/target 双方と同じ Source `build_fence_seq` を使い、P7 target identity の登録だけを P7 transaction に合流する。

P1-B01 の `BodyUnitManifest` / `BodyCoverageArtifact` / `ArtifactReceipt` / `GenerationBundleReceipt` は**未実装なら実装待ち**とし、P1 writer と共に次の一点へ収束させる。`search-core::projection_bundle::{SemanticRegistrySnapshot, ProjectionPayloadV1, BundleComponentsV1, composite_digest_v1}` が純粋な provider-neutral codec を所有し、既存 `search-projection-memory::generation_digest` は同じ canonical projection-only v1 関数を呼ぶ互換 wrapper にする。`ProjectionPayloadV1 { resources: Vec<CompiledResourceProjection>, registry: SemanticRegistrySnapshot }`、`ArtifactDigestCount { digest:[u8;32], count:u64 }`、`BundleComponentsV1 { source_id:SourceId, projection_digest:[u8;32], unit_manifest:ArtifactDigestCount, body_coverage:ArtifactDigestCount, lexical:ArtifactDigestCount, graph:ArtifactDigestCount, profile_set_digest:[u8;32], lexical_schema_version:String }` とし、`composite_digest_v1(&BundleComponentsV1)->Result<[u8;32],BundleCodecError>` は P1 の exact formula/golden vector を使う。`SemanticRegistrySnapshot` は現行 application 定義を一つだけ Core に移し re-export する。P1 Unit/coverage の canonical encoder は P1-B01 が提供する typed method を再利用し、P7 が同名の第二実装を作らない。P1-B01 実装順と module path が異なれば、その時点でこの contract と P1 plan を同一型に明示的に再照合してから P7-04 の RED に入る。raw `serde_json::Value`、JSONB byte hash、既存 memory store の成功値だけを永続 READY 証明へ流用しない。

P7-04 の `StoredBundleV1 { key, source_snapshot, manifest: ProjectionGenerationManifest, projection: ProjectionPayloadV1, unit_manifest: BodyUnitManifest, coverage: BodyCoverageArtifact, receipt: GenerationBundleReceipt }` は `dto_version='v1'` の tagged/`deny_unknown_fields` DTO として全内容を復元する。`validate_stored_bundle_v1(&StoredBundleV1)->Result<ValidatedPayloadV1,BundleError>` は manifest/source/key/count、全 Unit.text SHA、Unit/coverage/profile/projection/composite の独立再計算を行うが、**Graph/lexical 実体を検証する前に READY handle を発行しない**。`LexicalSealV1 { key, schema_version, analyzer_version, logical_digest, searchable_doc_count, unit_seal_digest, unit_seal_count, tree_digest, index_relpath }` は実 index 再 open/列挙から得る。P7-08 は `GraphReceiptMappingV1::validate(p1_staged_input, p3_rows, p1_graph_receipt, p3_graph_receipt)` を一つの canonical Graph record 集合から二方式で再計算し、P3 `GraphGenerationReceipt` の key/snapshot/mapping/schema/resource/relation count と照合する。P1 input digest に P3 content digest をコピーしない。

### Common task protocol

各 task は指定した named test を**先に**追加し、実 PostgreSQL `postgres:18.6-bookworm`・独立 `PgPool`/必要な別 process・実 builder/coordinator/reader/GC role で意図した RED を保存する。最小実装後に同じ focused command と当該 crate の `cargo clippy --locked -p <crate> --all-targets -- -D warnings`、`cargo fmt --all -- --check` を通し、独立 read-only reviewer が SQL bypass、型、lock 順、失効・scope・retention を判定する。1 task ごとに全 CI を回さない。対象全体が揃った後に `mise run verify:fast`、必要な最後の exact-head hosted gate を一度行い、失敗に対応する範囲だけ追加検証する。各 receipt は branch/head、migration checksum、role、fixture/索引 hash、test command/result、既知未接続 gate を明記する。merge/deploy/本番 migration は別指示の操作である。

## Tasks — Graph 非依存の局所 substrate

### P7-01 — global ownership と legacy 移行拒否

**Files:** `0002_search_source_ownership_v1.sql`; `tests/source_ownership_migration.rs`。`0001` は変更しない。

**Interface:** `search_source_ownership(source_id PK,tenant_owner_key,source_kind DOCUMENT|REMOTE,registration_revision,visibility_revision,activation_epoch,state ACTIVE|TOMBSTONED,registration_dto,registration_digest,created_at,updated_at)` は `(source_id,tenant_owner_key)` にも UNIQUE を持ち、owner/kind/SourceId は tombstone 後も immutable。`search_registration_serial` は一行の global lock と `document_deployment_revision/document_desired_set_digest`、`remote_deployment_revision/remote_desired_set_digest` の二組を持つ。各組は初回 reconcile 前だけ両 NULL、以後は正数 revisionとdigest両 NOT NULLの complete CHECK とし、production factoryは両組の確定まで起動しない。既存 Source へ nullable owner/revisions/activation と `registration_active=false` を追加し、Search receipt に `bundle_version` を追加する。既存 receipt の version を digest から推測しない。ownershipだけ差替え、または Source側active/revision/activationだけ変更する直接DMLは trigger/roleで拒否し、同じregistration transactionの両行一致だけを許す。

- [ ] RED: `cargo test -p search-runtime --locked --test source_ownership_migration -- --test-threads=1` で Domain `0009` と Search `0001` の独立 ledger、`0002` 順序、二 namespace revision、kind CHECK、owner/kind UPDATE/DELETE 拒否、旧 current/ownership 不明行を黙って再割当てしないことを試す。`tenant_owner_key` は既存 `TenantId` と同じ非空・trim済み・制御文字なし・UTF-8 最大256 bytes、registration DTO は version allowlist・最大65536 bytes、digestは `sha256:` + lowercase hexに限定する。legacy owner/kind/revision/全 generation mapping が host durable authority から証明できないケースは startup/backfill 失敗とし、DB の半更新・pointer NULL 化を許さない。
- [ ] GREEN: migration と前提 scan/明示 backfill transaction を実装。host-configured ownership proof がない production は起動拒否し、全既存 Source/key の照合、FK/trigger の検査が済むまで acquisition/read/publish を閉じる。Search `_sqlx_migrations` と Domain ledger の checksum/順序を実 DB で再確認する。

### P7-02 — complete desired 全集合と Source lease current gate

**Files:** `src/source_registration.rs`, `src/source_lease.rs`, `tests/source_registration.rs`; application 型は P4-02 専用 window。

**Interface:** P5 改訂 2 の `reconcile(&CompleteDesiredRegistrations)` / `is_current(&SourceRegistration,RegistrationActivation)`。trusted composition root の host-owned **同 revision・全 tenant・当該 namespace の完全 snapshot** と key/DTO exact equality を二度照合する。`RegistrationSetDigest` は namespace/version separator（Remote は `remote-desired-set:v1`）、UUID 昇順、固定 tag・順・length frame、全 server-owned DTO field から計算する。DB serial 行を先頭に lock → SourceId 順 Source 行 → SourceId 順 ownership 行、commit 直前に host revision/digest を再確認。Document/Remote は同じ serial/ledger を使い、tombstone 対象は namespace で限定する。DTO/visibility/revision 変更・削除・再有効化は activation と `fence_epoch` を overflow 検査して進め、lease token を失効させるが current key/二 digest/revision を触らない。P6-S02 の acquire は ACTIVE・owner/activation binding が成立する行だけを条件付き UPDATE する。

- [ ] RED: `remote_reconcile_never_tombstones_document`, `partial_tenant_map_cannot_tombstone_foreign_remote`, `document_remote_same_source_id_is_rejected_after_tombstone`, `partial_or_stale_remote_desired_set_is_atomic_failure`。同 revision 別 digest、旧 revision、同 revision 同 map 冪等、別 tenant/kind再利用、二 process 同時 reconcile、lease 失効、acquire/renew commit応答不明時の outbox非claim、counter/BIGINT overflow、owner proof 欠落を同じ command で試す。`cargo test -p search-runtime --locked --test source_registration -- --test-threads=1` はまず RED。
- [ ] GREEN:一 transaction の ledger adapter と `is_current` 全 DTO/current check を実装。P4 actor-visible registry の既存 mintを再利用し、Document/Remote 初期 reconcile 完了まで四 route を起動しない。独立接続で GREEN。

### P7-03 — generation/guard/pin schema と実 role

**Files:** `0003_search_generation_v1.sql`, `sql/roles.sql`, `tests/generation_schema.rs`。

**Interface:** `search_generation_identity` は `(source_id,generation_id)` 恒久 PK、owner/activation と不変で GC 後も残し、ownership `(source_id,tenant_owner_key)` への複合 FKと Source row との一致 triggerで旧owner混入を拒否する（activationは過去値を保存し、再登録後の現行値を FK で上書きしない）。`search_generation` は同じ複合 key、`BUILDING|READY|FAILED|DELETING`、`build_kind FULL|INCREMENTAL`、不変 `stage_origin EVENT|MANUAL` と event ID/Source epoch complete-or-null、FULL token/fence complete-or-null の逆側 INCREMENTAL NULL、source snapshot、versioned `projection_manifest`/digest/resource_count、activation、bundle version、ready_at を持つ。FULL target 4 列に UNIQUE、guard token と `(source_id,build_fence)` も UNIQUE、full guard から `(source_id,target_generation_id,guard_token,build_fence)` の複合 `ON DELETE RESTRICT` FK。`search_generation_payload` は `projection|unit_manifest|body_coverage` 各 key/kind 一行で `dto_version,payload JSONB,logical_digest,logical_count` を持つ。`search_generation_receipt` は P1 projection/Unit/coverage/lexical/Graph input/profile/composite の digest/count、lexical schema/analyzer、Graph backend/schema/mapping/content digestとresource/relation count、versioned receipt DTO、任意の vector receipt を保持する。`search_lexical_artifact` は key-derived相対 path、format/schema、tree/実doc/unit seal digest/count、finalized_at を持つ。`search_evaluation_lease` は `(source_id,lease_id)` PK、generation FK、evaluation ID/二 digest/DB expiry に tenant owner・`actor_scope_ref`・registration/visibility/access revision を保存する。全子行は key 複合 FK `RESTRICT`。Source current は `(source_id,current_generation_id)` FK を持つが historical Search receipt に generation FK を付けない。

- [ ] RED: `cargo test -p search-runtime --locked --test generation_schema -- --test-threads=1` で key/tenant/activation/kind/origin/token 完全拘束、wrong-key FK、current orphan の移行停止、READY→BUILDING/READY child DML、直接 guard 再発行、pin scope field UPDATE、identity DELETE/再利用を拒否する。`actor_scope_ref` は非空・trim済み・制御文字なし・UTF-8 最大256 bytes、revision は正数/BIGINT範囲、二digestは `sha256:` + lowercase hex とする。実 `search_registration`、`search_builder`、`search_coordinator`、`search_reader`、`search_gc` の role を別接続で使い、registration の pointer直接DML、builder の親/guard/pointer直接 DML、reader の全 DML、GC 以外の DELETE を拒否する。PostgreSQL row lock に必要な UPDATE privilege は限定 coordinator/`SECURITY DEFINER` trigger（固定 `search_path`・PUBLIC EXECUTE revoke）で満たし、builder に無制限親 UPDATE を与えない。
- [ ] GREEN: nullable legacy pointer/receipt の監査と必要な明示 backfill を先に完了させ、証明できない旧 pointer は migration/起動を停止。CHECK/FK/immutable trigger/実 grant を実装し、builder の child DML は親 BUILDING + full guard token/fence + `clock_timestamp()` 未失効を trigger でも再検査する。GC role の DELETING child DELETE だけを例外とする。

### P7-04 — typed full payload と純粋 canonical 再計算

**Files:** Core/memory/runtime codec map の P7-04 行と `tests/bundle_durability.rs`。P1-B01 の typed DTO と golden vector が存在することが前提。

**Interface:** `validate_stored_bundle_v1(&StoredBundleV1)->Result<ValidatedPayloadV1,BundleError>`。projection payload は全 `CompiledResourceProjection` と `SemanticRegistrySnapshot`、Unit manifest payload は全 `BodyItemEntry` と Unit.text、coverage payload は同じ authoritative item 集合を格納する。versioned typed DTO を SQL JSONB に保存・復元し、`deny_unknown_fields` と size/count bounds、重複/余分/欠落/非 canonical 値を拒否する。projection-only digest は既存 `search-projection-memory::generation_digest` と byte 同値の純粋 Core codecから、Unit/coverage/profile/composite は P1 encoderから**復元 DTOを入力に**再計算する。`ArtifactReceipt.key`、manifest source/key/snapshot/count、全 Unit.text SHA、Supported/Partial Unit と coverage gap、Unsupported/Failed Unit 0 を照合する。Archive Part の共通 binding は複数 leaf 許容の `archive_inner_format=None` を維持し、各 Unit の `Some(leaf)`/member chain/locator/profileを検証する。Partial の肯定 Unit は保存できるが coverage gapを残し、否定の完全性を主張しない。

- [ ] RED: `cargo test -p search-runtime --locked --test bundle_durability -- --test-threads=1` は P1 composite v1 golden、body-only change で projection-only digest 不変/composite 変化、全 payload 保存→新 process/別接続で同 key/digest、JSONB 物理 byte 並替え非依存、未知 version/field・欠落・余分・重複・不正 count/Unit.text の fail closed。memory projection v1 との同値と `Retryable`/retention拒否を確認する。
- [ ] GREEN:純粋 codec と typed runtime round-trip を実装。`search-generation` READY/publish はまだ呼ばず `ValidatedPayloadV1` だけを返す。P1-B01/B02 と型の一致を reviewer が確認する。

### P7-05 — file-backed lexical の durability と双方向 Unit seal

**Files:** `src/lexical_artifact.rs`, `tests/lexical_artifact.rs`。P1-E01 の実検索可能 Unit doc 列挙が前提。

**Interface:** `finalize_lexical(key, staged_dir, expected_p1_lexical_receipt, unit_manifest)->Result<LexicalSealV1,...>` と `reopen_and_validate_lexical(key, saved_row, unit_manifest)->Result<LexicalSealV1,...>`。trusted index root + key からのみ final directory を導き、staging files と directory を fsync→immutable final path へ atomic rename→親 directory fsync。再 open した実 index の全 Resource/Unit doc から P1 logical lexical digest/count/schema/analyzer、sorted file-name+size+SHA-256 tree digest を算出。全 Supported/Partial Unit と実検索可能 Unit doc の親/Part/raw/locator/textを双方向一対一で照合し、Unsupported/Failed の doc は 0。

- [ ] RED: `cargo test -p search-runtime --locked --test lexical_artifact -- --test-threads=1` は builder 自己申告だけの一致、Unit doc 欠落/余分/重複/本文差替え、別 key path、rename 前/後 crash、tree file 破損を拒否する。P1 の実 index APIを使い、metadata-only mock で seal 成功にしない。
- [ ] GREEN: immutable file lifecycle と DB artifact rowを実装。file/DB は原子 commit 不可なので READY 前・CAS 前・pin 後/返却前に再確認し、commit 前 unlink をしない。orphan sweep は DB state/key/guard を確認し commit 後のみ行う。

### P7-06 — EVENT/MANUAL target 登録と full guard fence

**Files:** `src/{generation_registration,full_guard}.rs`, `tests/full_guard.rs`。

**Interface:** EVENT は locked outbox row→Source rowから route/event ID/lease token/Source epoch/expiry を取得し、MANUAL は Source から開始。ownership ACTIVE・activation/retention/snapshot を照合し、恒久 identity INSERT→P7 BUILDING target INSERT→保存 target token/fence と一致する full guard INSERT を一 transaction で commitする。`build_fence_seq` は Source row下で一度だけ増やし overflow拒否。private handle の field は外へ発行しない。`renew_full_guard`、子 batch、READY は保存 binding と DB clock未失効を毎回再検査し、失効 guard を同 targetへ再発行しない。incremental は target identity と P3 base/target guard を最初の copy より前に同一登録 transaction に結ぶ。

- [ ] RED: `expired_full_guard_rejects_late_child_write_and_ready`, `full_target_guard_cannot_be_reissued`、wrong event/route、MANUAL→EVENT handle 偽装、guard INSERT failure/key collision/fence overflow の全 rollback。`cargo test -p search-runtime --locked --test full_guard -- --test-threads=1`、独立接続と期限境界 barrier で RED。
- [ ] GREEN: Graph 非接続時にも target+guard の登録/失効/cleanup 候補検査まで実装するが、Graph READY と publish は閉じる。Graph 接続時の一 commit parent 登録は P7-07 で拡張する。

## Tasks — P3 native qualification 後だけ着手

### P7-07 — Graph parent の唯一の一接続登録入口

**Files:** map の P7-07 行。P3-P04 GO、選定済み PG backend、P3-G01/G03/G04/C01 の production signature/role 改訂が前提。別 backendなら本 task を実行せず protocol を再設計・独立 reviewする。

**Interface:** private `GraphRepository::register_full_on(&mut PgConnection, &RegisteredTarget)->Result<RegisteredFullBuildHandle,GraphError>` は P7 coordinator の EVENT または MANUAL の**同じ** transaction 内だけで Graph BUILDING parent を登録し、Source/key/snapshot/activation/token/fence を P7 保存 target と一致させて opaque handle を返す。Graph parent→P7 target の複合 FK・immutable binding/child mutation triggerは P3-G03 の未適用 migrationで一度に確定するか、既適用なら新しい Graph additive migrationで加える。`DurableGraphGenerationPort::stage_full_registered(handle,resources,relations)->BoxFuture<GraphStage>` は登録済み parent の**子 batch だけ**を書き、各 batch が P7/Graph parent/guard/expiry を再読。旧 `stage_full(manifest,...)->GraphStage` の parent自律 INSERT は isolated PoC/fixture専用で production composition root/role に出さない。incremental は P3 frozen base/target guard と同じ target registration/connectionを使う。

- [ ] RED: `full_registration_is_one_commit_with_guard_and_graph_key`, `graph_stage_failure_leaves_no_visible_unprotected_target`, `isolated_stage_full_cannot_publish_without_p7_guard` を `cargo test -p search-runtime --locked --test graph_registration -- --test-threads=1` で実 role/別接続/fault injection。Graph INSERT/guard INSERT fault のあと P7/Graph/identity の可視状態を照合する（identity は transaction rollbackなら未作成）。
- [ ] GREEN:一接続 API、Graph parent の複合 FK/immutable binding と DML trigger、production role の parent INSERT 禁止を実装。handle のみの認可を拒否し GREEN。

### P7-08 — 実 payload/lexical/Graph から READY を一 commit

**Files:** map の P7-08 行。P1-B01/B02、P7-04/05、P3-G02/G08 の receipt/encoderが前提。

**Interface:** `validate_ready(handle:&BuildHandle)->BoxFuture<VerifiedBundle>` は事前に Source snapshot・保持 grant・全 DTO・immutable lexical final directory/再 openを検証し、短い transaction で target→guard を lock、token/fence/DB expiry、P7 payload/receipt と P3 Graph row/receiptを同じ接続で再検証する。P1-B02 の `canonical_graph_staged_input_v1(typed_relations,document_owner_mapping)` と P3-G02 の `canonical_graph_digest(source,schema,resources,relations)` は同じ復元済み typed record 集合から**別々**に計算し、それぞれ仕様化した raw bytesと固定 golden vectorを P7-08 着手前に揃える。P1 encoderは typed n-ary+Document owner mapping、P3 encoderは Source/schema/全 Graph resource/relationを扱い、P3 digestを P1 欄へ転記しない。`GraphReceiptMappingV1` は key/snapshot/mapping/schema/resource/relation countを照合し、`graph_input_count` は attachment重複除去後の一意 relation数とする。P1 composite v1 を Core encoderで再計算し、P3 Graph READYの実体を確認した同じ接続でP7 READY+receiptを一 commitで確定する。READY後の全子行は不変、READY単独はcurrentでない。

- [ ] RED: `cargo test -p search-runtime --locked --test ready_bundle -- --test-threads=1` は P1 composite golden、P1 Graph input/P3 content **両** golden、P3 digestをP1欄へコピーした値、key/snapshot/count/mapping不一致、lexical消失、Unit text破損、READY中 child更新/guard期限切れを拒否する。未資格Graphでは positive READY testが通らないことも assertion。
- [ ] GREEN: typed `GraphReceiptMappingV1` と同一 connectionのREADY、外部 file事前/再確認を実装。PGとfileの原子性を捏造せず、後続 pin/readも再検証する。

### P7-09 — event origin、manual CAS と Search receipt

**Files:** map の P7-09 行。P6-S03/S04 の production型/bridgeを本 taskと直列化。

**Interface:** `PublishCandidate` は outbox→Source→P7/Graph generation key順→guard→lease ID順→Search receiptで lockし、DB行からevent/Source epochを再取得。candidate の `stage_origin='EVENT'`、保存 event ID/epoch/snapshot/activation/guard、同 key READY/二 digestとexpected pointer/revisionを再照合する。pointer + `pointer_revision` + `last_published_epoch` CAS、`(source_id,event_id)` receipt（bundle versionと両 digest、epoch単調）とguard DELETEは一 commit。`ReuseCurrent` は候補 handleを取らず、current key/二 digest/revisionと実READY/authority/retentionのみを再検証。manualはSourceから始め同じCAS/READY条件で公開し、pending eventをackしない。CAS敗北はguardを保持。generic ackは別 fenced transaction。commit応答不明は `CompletionUnknown`、reconnect後のcurrent/receipt/fence再読と後続配送だけで収束。

- [ ] RED: `wrong_event_candidate_cannot_publish_or_write_receipt`, `old_epoch_or_manual_candidate_cannot_complete_event`, `reuse_current_requires_current_ready_after_gc`。同 epoch別 key/digest/version、旧 epoch、ack応答不明、publish commit応答不明、CAS敗北を `cargo test -p search-runtime --locked --test event_completion -- --test-threads=1` の別接続/barrierで検査。historical receiptのみ/GC済み keyは成功にならない。
- [ ] GREEN: `CompleteEventRequest` sum typeへP6-S01とindexer呼出しを一緒に移し、P6-S03がP7 private transaction-bound APIだけを使う。再配送で二重 publishを作らず、Search側が`delivered_at`を更新しないことを確認。

### P7-10 — actor scope pin、renew/return/release

**Files:** map の P7-10 行。

**Interface:** `pin_current` はSource row lock前にlexicalを暫定preflightし、Source→generation→guard→leaseの短いtransactionで**その時の**current/activation/ownership ACTIVE/二digest/P3 READYを確認して lease INSERT。保存欄は tenant owner、host-issued `actor_scope_ref`、registration/visibility/access revision、evaluation、key/manifest/bundle、DB expiryで immutable。host referenceはrestart後も同じ actor/session/evaluation scopeに再解決できるか、不明ならfail closed。renew/return/actor releaseはpinと現在の`TrustedDiscoveryBinding`+`AuthorizedSourceScope`の双方を要求し、DB rowとP4/P5 current gateを再照合。Graph readは `REPEATABLE READ READ ONLY` snapshotを用い、return前は新しいDB-clock transactionを開く。returnはitem/field/Graph participant、Document current Version/Part/raw/Readまで再判定し、結果全体を閉じる。旧 key pinはpointer前進だけでは失効しない。

- [ ] RED: `pin_cannot_transfer_between_actor_scopes`, `registration_or_visibility_change_revokes_old_pin_read`, `foreign_tenant_cannot_renew_same_source_pin`。`cargo test -p search-runtime --locked --test pin_scope -- --test-threads=1` に加え、expiry中/旧pointerpin、別 process pin↔publish、scope reference再起動解決不能、reader role直接DML拒否を試す。revoked actor releaseは成功を装わず、coordinator GCだけが失効leaseを掃除する。
- [ ] GREEN:同一接続のpin/renew/verify/releaseとP4/P5 current gateを実装。pinは旧 key を保存し、返却直前まで同じ key/receiptと現在 authorityを再確認する。

### P7-11 — guarded GC と FK-safe cleanup

**Files:** `src/gc.rs`, `tests/gc_races.rs`。

**Interface:** `retire_unpinned`、`discard_unpublished`、expired guard cleanup はcurrent、有効pin、full/P3 base/target guardを確認する。Source→sorted generation→guard→lease lock後、unpublished・unpin済み targetを一 transactionでDELETING→**guard DELETE**→expired lease DELETE→Graph participant/relation/resource→P7 child→Graph/P7 parent DELETE。恒久 identityとhistorical receiptは残す。fullとincrementalは同じ Source `build_fence_seq` namespace、token/DB expiryと保存bindingで判定し、失効 targetへ guardを再発行しない。file unlinkはcommit後のidempotent orphan sweepだけで行う。

- [ ] RED: `guard_delete_then_child_failure_rolls_back_full_target` と current/pin/guardを持つ keyの GC拒否、別process pin↔publish↔GC、base/target copy↔GC、失効guard再発行拒否、実FK `23503`のない正常cleanupを `cargo test -p search-runtime --locked --test gc_races -- --test-threads=1` で試す。guard DELETE後のchild faultは同じ transactionをrollbackし、別接続からguard/target双方が残る。
- [ ] GREEN: P3 transaction-bound child cleanupを同一接続で実装。Source-less copy/validate batchはgeneration→guardのみをlockし後からSourceを取らない。READY baseはtarget cleanup後の別GC transactionでだけ退役させる。

### P7-12 — restart、corruption、別 DB restore と統合受入

**Files:** `src/recovery.rs`, `tests/process_restore.rs`。本 plan の最後にだけ実施。

**Interface:** startup はmigration/role/complete registry/ownership scan→current の全P1 payload/manifest、実 lexical tree/doc、P3 Graph mapping/content、bundle version/compositeを再検証→API/claimを開く順。中断BUILDING、expired guard/lease、unknown ack/commitはDB clockとfenceで回収し、有効guardを消さない。incremental のcommit済み cursor/対象全体 digestが証明不能なら同keyを再開せず、新keyで full rebuildする。破損や外部bytes喪失は当該keyをunavailableにして新keyでSource正本からrebuildし、旧pinを別keyへ暗黙転送しない。`pg_dump -Fc`/`pg_restore` は**別の disposable DB**、lexical immutable directoryも別復元し、同 key/digest/実 queryを確認する。RAM Remote generation/session/cursorは復元しない。

- [ ] RED: `cargo test -p search-runtime --locked --test process_restore -- --test-threads=1` でkill/restart（build、publish commit前後、ack前）、missing/corrupt row/lexical file/digest、unknown commit、stale generation、別DB restoreで索引bytes欠落/復元、current/pin/guard/GC競合を独立process/connectionで注入。DBだけ復元してindexが無い場合はreadyでない。旧ownership行のbackfill失敗と同tenant再有効化時の旧scope失効も回帰する。
- [ ] GREEN:recovery/掃除と新key再構築を最小実装。P6 generic receipt、P3 native qualification、P1 body/lexical、P4/P5 current authority の各独立 receiptを照合してから共有 durable acceptanceを判定する。最終 `mise run verify:fast` と必要な exact-head hosted gateの結果を別に記録し、最終 P7 HTTP/運用/SLO/保持・送出 lifetime の完了とは分ける。

## Review focus and acceptance ledger

| 失敗しやすい入力・競合 | 所有 test |
| --- | --- |
| 全 tenant 未満の Remote desired map、同 revision異DTO、Document/Remote SourceId衝突 | P7-02 の4 named ownership tests |
| 別 event/旧 Source epoch/MANUAL候補と GC済み historical receipt | P7-09 の3 named event tests |
| full guard期限境界、別target token再利用、cleanup途中 fault | P7-06 の2 named guard tests、P7-11 の1 named cleanup test |
| 別接続 Graph parent、Graph/guard片側だけ可視、isolated `stage_full` | P7-07 の3 named Graph tests |
| pin IDを別actor/tenantへ移す、visibility変更、restart後scope参照不能 | P7-10 の3 named pin tests と P7-12 |

上の **16 named real-PG cases** は改訂 1 §6 の名前を変更せず割当てた。各 positive gate は実 role・独立接続を必要とし、型だけ/memory fake/Graph孤立PoCを代用しない。全task完了後の独立 read-only reviewer は migration checksum/legacy失敗、SQL role/trigger bypass、canonical golden、P1 lexical双方向seal、P3二digest対応、P6 pointer+receipt/別ack、pin/current/guard/GC race、restart/別DB restoreを照合する。共有PG局所受入、Graph production資格、P6縦断、最終P7 runtime/HTTP、exact-head CIを別欄の証拠として報告する。
