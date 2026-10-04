# P3-G01 登録参照・read lease interface 改訂 1

Status: **pre-code design proposal / 独立再審査待ち**。`p3-g01-issuer-review.md` の NO-GO を閉じるため、`p3-g01-issuer-refinement.md` の issuer 案を本書で置き換える。P3 Freeze と build guard、P7 Shared Durable Freeze/Plan、P4 `AuthorizedSourceScope` を優先し、Graph backend 選定、実装、READY、資格判定は主張しない。P3 plan の旧 autonomous `stage_full` / `validate_ready` は isolated PoC/fixture 限定である。

## 1. 生成境界と唯一の admission

`TrustedGraphRegistrationHostPort`、`GraphRegistrationIssuer<P>`、`GraphReadLeaseIssuer<P>`、`CommittedFullGraphTarget`、`CommittedIncrementalGraphTarget` は production G01 公開 API に採用しない。公開 trait の実装や public constructor に cross-crate seal の効力はない。P7 の private `EventCandidateHandle` / `ManualBuildHandle` / `FullBuildHandle` / `PinnedBundleLease` と G01 の参照値は異なる型・異なる責務である。P7 coordinator が同一接続の登録 commit 後または pin commit 後に識別値を渡せるよう、G01 参照には public `from_identifiers` を置く。任意 crate が同じ値を作れることを API docs に明記する。private field と accessor は layout の安定性だけを守り、偽造防止・認可・READY 証明を意味しない。public trait 実装を「trusted host」と呼んで authority にしない。

P7-07 の登録 transaction は EVENT なら outbox→Source、MANUAL なら Source から始め、ownership ACTIVE・tenant・activation・現行 retention、Source snapshot/manifest、恒久 key、P7 target、Graph BUILDING parent、対応 guard を同じ接続で commit する。incremental は同一 Source の保存 READY base と新規 BUILDING target、両 key を守る guard を最初の copy より前に同じ登録 transaction に束縛する。Source の `build_fence_seq` は full/incremental 共通で増分する。P3 Graph repository の親登録は private transaction-bound method のみで、通常 builder role に親・guard INSERT を許さない。Graph が選定・接続されていない枝では Graph parent/READY/publish を開かない。

G03/G04/G06 の **毎 batch** は、参照値の一致だけで許可せず、実 builder DB role、保存 P7 target、Graph parent、build kind、Source/key、snapshot/manifest、activation、guard 行と保存 token/fence、`expires_at > clock_timestamp()`、親 BUILDING を検査する。full は P7 full guard、incremental は base READY の凍結 receipt、現在の Source `build_fence_seq`、target/guard/cursor も検査する。Source-less batch は generation→guard の lock 順とし、後から Source lock を取らない。DB trigger は参照値なしの直接 child DML にも親・guard・expiry fence を強制し、commit 直前に DB clock を再検査する。欠行・失効・不一致なら子 DML はゼロで transaction 全体を rollback する。DB 行、role、fence が唯一の child DML admission である。P7-08 だけが同じ接続で Graph/P7 実体を再検証して双方の READY を commit する。G01 参照、stage report、Source mapping receipt、Graph 単独 READY のいずれも publish/pin 証明ではない。

## 2. SQLx-free の正確な公開形

以下は G01 に置く目標 signature（`impl` の関数本体は省略）である。`BoxFuture<'a,T>` は既存 `search-application::ports` と同様 `Result<T,SearchError>` を内包する。`ProjectionGenerationKey` の Source と generation、`DiscoveryEvaluationId`、`SourceId`、`Uuid` は既存 Core 型を使う。SQLx/PgPool/PgConnection/DB role 型は App export に含めない。

```rust
pub struct RegisteredFullBuildHandle { // fields private; identity only
    key: ProjectionGenerationKey,
    guard_token: Uuid,
    build_fence: i64,
}
impl RegisteredFullBuildHandle {
    pub const fn from_identifiers(key: ProjectionGenerationKey,
        guard_token: Uuid, build_fence: i64) -> Self;
    pub const fn key(&self) -> ProjectionGenerationKey;
    pub const fn guard_token(&self) -> Uuid;
    pub const fn build_fence(&self) -> i64;
}
pub struct BuildGuardHandle { // fields private; identity only
    base_key: ProjectionGenerationKey,
    target_key: ProjectionGenerationKey,
    guard_token: Uuid,
    build_fence: i64,
}
impl BuildGuardHandle {
    pub const fn from_identifiers(base_key: ProjectionGenerationKey,
        target_key: ProjectionGenerationKey,
        guard_token: Uuid, build_fence: i64) -> Self;
    pub const fn base_key(&self) -> ProjectionGenerationKey;
    pub const fn target_key(&self) -> ProjectionGenerationKey;
    pub const fn guard_token(&self) -> Uuid;
    pub const fn build_fence(&self) -> i64;
}
pub enum GraphBuildRef { Full(RegisteredFullBuildHandle), Incremental(BuildGuardHandle) }
pub struct GraphReadLease { // fields private; identity only
    key: ProjectionGenerationKey,
    evaluation_id: DiscoveryEvaluationId,
    lease_id: Uuid,
}
impl GraphReadLease {
    pub const fn from_identifiers(key: ProjectionGenerationKey,
        evaluation_id: DiscoveryEvaluationId, lease_id: Uuid) -> Self;
    pub const fn key(&self) -> ProjectionGenerationKey;
    pub const fn evaluation_id(&self) -> DiscoveryEvaluationId;
    pub const fn lease_id(&self) -> Uuid;
}
pub enum GraphBatchPhase { Copy, Delta }
pub struct GraphBatchCursor { // public fields, not a committed checkpoint
    pub target_key: ProjectionGenerationKey,
    pub phase: GraphBatchPhase,
    pub committed_sequence: u64,
}
pub struct GraphStage { pub key: ProjectionGenerationKey } // no READY flag
pub struct GraphStageReport { // physical pre-READY comparison data, no authority
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    pub projection_manifest_digest: String,
    pub source_mapping_digest: String,
    pub graph_content_digest: String,
    pub resource_count: u64,
    pub relation_count: u64,
    pub graph_schema_version: String,
}
pub struct GraphSourceMappingReceipt { // untrusted comparison data
    pub key: ProjectionGenerationKey,
    pub source_snapshot: String,
    pub mapping_digest: String,
}
pub trait DurableGraphGenerationPort: Send + Sync {
    fn stage_full_registered<'a>(&'a self, target: &'a RegisteredFullBuildHandle,
        resources: &'a [GraphResourceRecord], relations: &'a [TypedRelationInstance])
        -> BoxFuture<'a, GraphStage>;
    fn copy_batch<'a>(&'a self, target: &'a BuildGuardHandle,
        expected: &'a GraphBatchCursor, limit: u32) -> BoxFuture<'a, GraphBatchCursor>;
    fn verify_copy<'a>(&'a self, target: &'a BuildGuardHandle) -> BoxFuture<'a, ()>;
    fn apply_delta_batch<'a>(&'a self, target: &'a BuildGuardHandle,
        delta: &'a GraphIncrementalDelta, expected: &'a GraphBatchCursor, limit: u32)
        -> BoxFuture<'a, GraphBatchCursor>;
    fn validate_staged<'a>(&'a self, target: &'a GraphBuildRef)
        -> BoxFuture<'a, GraphStageReport>;
    fn recover_ready<'a>(&'a self, key: &'a ProjectionGenerationKey,
        expected_manifest_digest: &'a str) -> BoxFuture<'a, GraphGenerationReceipt>;
}
pub trait GraphSourceMappingValidatorPort: Send + Sync {
    fn validate_authoritative<'a>(&'a self,
        manifest: &'a ProjectionGenerationManifest,
        records: &'a [GraphResourceRecord])
        -> BoxFuture<'a, GraphSourceMappingReceipt>;
}
pub trait GraphLeaseVerifierPort: Send + Sync {
    fn verify<'a>(&'a self, lease: &'a GraphReadLease,
        binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, ()>;
}
pub trait GenerationScopedGraphAccessPort: Send + Sync {
    fn evaluate<'a>(&'a self, key: &'a ProjectionGenerationKey,
        resource_ref: ResourceId, binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope) -> BoxFuture<'a, AccessDecision>;
}
pub trait PinnedGraphRetrievalPort: Send + Sync {
    fn retrieve_pinned<'a>(&'a self, lease: &'a GraphReadLease,
        plan: &'a GraphTraversalPlan, binding: &'a TrustedDiscoveryBinding,
        scope: &'a AuthorizedSourceScope)
        -> BoxFuture<'a, GraphRetrievalResult>;
}
```

`GraphBatchCursor`、`GraphStage`、`GraphStageReport`、`GraphSourceMappingReceipt` の public fields は adapter 間の比較データを渡すためであり、偽造可能である。commit 済み checkpoint は DB row だけにある。各 batch は期待 phase/sequence と保存 cursor を比較し、処理後に同じ transaction で cursor を進める。`limit=0` または上限超過は caller 入力の `InvalidRequest`、別 target/phase/sequence は `OperationFailed` として rollback する。cursor 不明なら同 target を推測再開せず、新 key で再構築する。`verify_copy` は delta 前に target 全行の count/content digest を凍結 base receipt と照合して `copy_verified_at` を同 transaction で保存する。

`GraphSourceMappingReceipt` の `mapping_digest` は caller の expected digest ではなく Source snapshot の canonical mapping から得る比較値で、`graph_schema_version` や任意の `schema digest` と別物である。validator trait も誰でも実装可能なので、この戻り値だけで Source 正本とは呼ばない。G03/G04/G08 は登録済み Source adapter と保存 Graph row の owner/kind/native mapping を同一 Source/key/snapshot に対して再計算し、保存 mapping digest、manifest、P7 receipt に照合する。`GenerationScopedGraphAccessPort::evaluate` も保存済み resource row の kind/owner を再読し、caller の owner 引数や Graph 内 `AccessProjection` を最終権限にしない。validator 不在、snapshot 不可、wrong owner/Source、version 対応不能なら fail closed。Document/FolderPlacement の一対一 owner、Knowledge Version の直接評価、n-ary attachment/closure は P3 凍結 §6–7 のままである。

## 3. Read lease と現在 scope

P7 `pin_current(binding: &TrustedDiscoveryBinding, scope: &AuthorizedSourceScope, ttl)` が保存 current READY と P3 READY/二 digest を同じ transaction で確認し、`PinnedBundleLease` を作る。G01 `GraphReadLease` はそこから渡す key/evaluation/lease **識別子だけ**で、public `from_identifiers` は pin を作らない。G07 の production `retrieve_pinned(lease, plan, binding, scope)` は P7 wrapper が保持する concrete verifier と現在 Source access port を使い、verifier を read 前と return 前に呼ぶ。caller が fake verifier/access port を引数で差し込める production method を設けない。P7 verifier は毎回保存 `search_evaluation_lease` と DB clock expiry、tenant owner、Source/key/evaluation/actor scope reference、registration/visibility/access revision、activation、manifest/bundle digest、P7/P3 READY と Source ownership ACTIVE を照合し、host reference を現在の actor/session/evaluation に再解決する。P4 `CurrentSourceVisibilityPort` と P5 current actor/Source gate を再実行する。既存 P4 `AuthorizedSourceScope` / `TrustedDiscoveryBinding` をそのまま受け取り、第二の actor/Source mint を作らない。read は `REPEATABLE READ READ ONLY`、return 前は新しい DB-clock transaction で同じ lease を検査し、item/field/participant と Document current Read/Version/Part/raw を再判定する。旧 key pin は pointer 前進だけでは失効しないが、保存 key/receipt と現在 authority は必ず一致させる。失効・不一致は結果全体を閉じ、別 key へ暗黙 rebind しない。

## 4. `SearchError` 分類と境界

| 条件 | App error / 後段の扱い |
| --- | --- |
| 外部 `GraphTraversalPlan`/公開 budget または batch `limit` の形式不正 | `InvalidRequest`。内部登録参照の不整合をこれに偽装しない。 |
| 現在 Source が利用不可、retention が durable Graph を許さない、現在 actor/Source gate が失効・Denied/Unknown | `SourceUnavailable`、ID-free の一般文言。個別 seed の Denied/Unknown は既存の不可視結果規則に従う。 |
| target/guard/lease 欠行、DB clock 期限切れ、保存 token/fence 不一致、旧 activation の build、FAILED/DELETING/READY target への batch | `FenceLost`。全 transaction rollback、read は結果を返さない。 |
| nil/wrong Source の内部参照、base/target Source 相違、保存 row 間の snapshot/manifest/schema/mapping/receipt 不整合、偽 validator/不完全 closure、wrong cursor、role/trigger 拒否など integrity/protocol fault | `OperationFailed`、ID-free の一般文言。既に保存値との単なる stale mismatch と判定できる場合のみ `FenceLost`。 |
| host/Source/visibility/validator port が `SearchError` を返す | その variant を改変せず伝播。接続不能など分類不能な host failure は adapter が `OperationFailed` にする。P4 の bind 後構造的不一致も `OperationFailed("trusted scope unavailable")`。 |
| commit 応答不明 | `CompletionUnknown`。再読前に成功を返さない。 |

P7/G03/G07 の role・row・scope checks が通るまでは public constructor、synthetic port、unit test の成功を admission と呼ばない。`PersistentResource` は該当 Source field grant を満たす場合に、`PersistentDiscoveryMetadata` は許された metadata だけに限定して候補になれる。後者から本文 Unit/embedding の保存許可を推論しない。`CacheWithExpiry` / `SessionOnly` / `NoRetention` は durable stage の対象外である。
