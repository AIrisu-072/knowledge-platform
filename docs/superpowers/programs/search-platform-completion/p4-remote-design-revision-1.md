# P4 Remote Source — 設計改訂 1

- 状態: **設計改訂案 / 独立再レビューと Design Freeze 待ち**。`p4-remote-design.md` は原案として保存し、本書を P4 の実装計画入力とする。実装、実通信試験、本番接続、資格取得を示すものではない。
- 対象: provider-neutral な四つの remote mode、観測・現在認可・保持期間、credential-free の合成 HTTP provider を実 TCP で `DiscoveryService::discover` まで通す P4。外部 credential、顧客データ、live endpoint、merge、deploy は含まない。
- 規範: 承認済み `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md` §§5–6, 17–18, 42、`spec/data/transaction-consistency-requirements-v0.md` SD-T2/3/4/7、`spec/data/data-characteristics-v0.md` の Search amendment、`spec/operations/observability-audit-requirements-v0.md` の Search audit privacy。Source 正本、S1 の retriever 順 priority concat、current authorization、Source-local identity、観測と Resource lifecycle の分離を維持する。

## 1. 改訂判断と現行境界

現行 `CandidateFederator::validate_lists` は同一 Source の異なる generation を `MixedSourceGenerations` として拒否する。`DiscoveryService::evaluate` は Source ごとの `PinnedSource.key` を list、hard gate、重複排除、Claim 解決に用いる。従って原案の「remote action ごとに generation key」を採用しない。**一回の `DiscoveryRequest.temporal_context.evaluation_id` につき一 Source には一つの sealed immutable generation** を与える。複数 remote action の応答は seal 前の Source batch に集約し、同一の Source snapshot・認可 scope・ACL revision と Resource version/digest の整合を証明できた場合だけ同じ generation に入れる。証明できなければ混ぜない。既存の durable Source pin は一つのまま保つ。

`DiscoveryService::discover` は今、引数なしの `SourceRegistryPort::list_sources()`、設定済み `RoutingConstraints`、`DiscoveryRequest.access_context: String` を別々に受ける。P4 は transport を待たず、これらを一つの trusted binding から構築する request-scoped entrypoint を追加する。既存の裸の `DiscoveryService` を remote の公開入口にしない。Source の存在は routing 前と出力直前の両方で current visibility を判定する。

現行 `QualifiedResource` は `ResourceId` 必須で、`resource_ref: None` の hit は qualification 前に飛ばされる。P4 では stable native ID を持つ remote hit のみ共通 qualification に入れる。ID のない hit は **明示的な `UnsupportedCoverage` gap** とし、qualified result、probe、binding を主張しない。四 mode の E2E 資格は stable ID を持つ合成 provider で検証する。

原案の「evaluation/session expiry」は曖昧なので、evaluation RAM、session RAM、expiry cache、durable projection を異なる owner と lease で扱う。`SessionWorkingSet` は現状 owner と TTL を持たず receipt bytes を保持するため、改修前には `SESSION_ONLY` の保管先にも `NO_RETENTION` の保管先にも使えない。provider JSON の evidence role/origin も `ResolvedAssertionEvidence` に直接コピーしない。

## 2. Trusted binding と Source visibility

以下は実装対象の概念 interface であり、既存型の存在を主張しない。`TrustedSearchScope`、`TrustedDiscoveryBinding`、`AuthorizedSourceScope` の constructor は trusted in-process resolver/visibility adapter だけに公開する。三型と port は HTTP に依存しない最小共通契約として `search-application` 側に置き、P5 の Search/Discover/GET/Source 一覧と P7 composition root でも同じ actor/source scope を使う。P4 の合成 harness は同じ resolver を in-process で実装し、P5 は後に認証済み actor の供給元を接続する。HTTP header、`DiscoveryRequest.access_context`、provider JSON を parse して tenant/principal/session を作らない。

```rust
struct TrustedSearchScope {
    tenant: TenantId,
    principal: PrincipalRef,
    session: Option<SessionId>,
    access_handle: AccessContextHandle, // server-owned opaque handle
    access_revision: AccessRevision,
    issued_at: Instant,
    deadline: Instant,
}
struct TrustedDiscoveryBinding {
    actor: TrustedSearchScope,
    evaluation: DiscoveryEvaluationId,
}
struct AuthorizedSourceScope {
    actor: TrustedSearchScope,
    source: SourceId,
    registration_revision: RegistrationRevision,
    visibility_revision: VisibilityRevision,
}
struct VisibleSourceRegistration {
    scope: AuthorizedSourceScope,
    registration: RemoteSourceRegistration,
}
trait AccessContextAuthorityPort: Send + Sync {
    fn resolve<'a>(&'a self, handle: &'a AccessContextHandle)
        -> BoxFuture<'a, Option<TrustedSearchScope>>;
    fn bind_discovery<'a>(&'a self, actor: &'a TrustedSearchScope,
        evaluation: DiscoveryEvaluationId)
        -> BoxFuture<'a, Option<TrustedDiscoveryBinding>>;
    fn current<'a>(&'a self, actor: &'a TrustedSearchScope)
        -> BoxFuture<'a, AccessBindingState>;
}
trait CurrentSourceVisibilityPort: Send + Sync {
    fn bind_source<'a>(&'a self, actor: &'a TrustedSearchScope, source: SourceId)
        -> BoxFuture<'a, Option<AuthorizedSourceScope>>;
    fn current<'a>(&'a self, scope: &'a AuthorizedSourceScope)
        -> BoxFuture<'a, AccessDecision>;
}
trait ScopedSourceRegistryPort: Send + Sync {
    fn visible_sources<'a>(&'a self, actor: &'a TrustedSearchScope)
        -> BoxFuture<'a, Vec<VisibleSourceRegistration>>;
}
```

`ScopedDiscoveryService::discover(binding, request, trusted_routing)` は **registry/routing/pin/network の前**に、(1) handle の現行解決結果と `binding.actor` の tenant、principal、session、revision、deadline の完全一致、(2) `bind_discovery` が発行した evaluation と `request.temporal_context.evaluation_id` の一致、(3) `request.access_context` と server-owned handle の一致、(4) 有効 session、(5) current actor/access revision を検証する。失敗時は Source に一切触れず、同じ外部エラーへ閉じる。handle の別 tenant/principal への差替え、期限切れ、revision 変更は拒否する。typed 値だけで真正性を推定しない。

registry は `SourceId` を server-owned **全 tenant 横断で一意**に割り当てる。現行 `ProjectionGenerationKey` と Graph generation key は SourceId+generation で tenant を含まないため、異なる tenant に同じ SourceId を再利用する設定、同一 tenant の重複 registration、provider が選んだ SourceId を P4/P5/P7 の composition root で起動時に拒否する。再登録/registry revision 更新時も同じ invariant を検査し、違反があれば既存 key へ混入させず runtime を fail closed にする。これは global SourceId だけで認可する意味ではない。各 request と各 read/write で `TrustedSearchScope.tenant` と registration の tenant、`AuthorizedSourceScope` の SourceId/revision を突き合わせる。visible/allowed な Source だけをこの scope と組にして `DiscoverableSource` へ投影し、任意の重複勝者を選ばない。`trusted_routing.required_source_ids` / `preferred_source_ids` はこの可視集合と交差させてから `SourceRouter::plan` へ渡す。未知・別 tenant・不可視の Required ID は同じ Source ID を含まない `required_source_unavailable` gap に正規化し、Preferred ID は黙って除外する。数も request に既知の明示 ID の範囲を超えて内部 registry 状態を示さない。`SourceRouter` が作る `source:{id}:...` gap、`source_trace`、retrieval/qualification trace、rejected candidates、locator、count は可視 Source に限る。Source scope が評価途中で Denied/Unknown/error へ変わったら、その Source 由来の candidate、claim、rank、gap、trace、Graph path、locator を一括除去し、Required なら同じ汎用 gap を返す。存在しない ID と不可視 ID の出力形・failure class は同一とする。

`SourceRegistryPort`、`CurrentCandidateAccessEvaluatorPort`、`CurrentSourcePolicyPort`、`ProbeCapabilityCatalogPort`、`ClaimSelectorPort`、`AssertionStorePort`、`EvidenceResolverPort`、`ConceptRegistryPort`、remote adapter は request-scoped wrapper を通る。現在の `&str` access_context を受ける port には `TrustedSearchScope` に対応する opaque handle だけを渡し、wrapper は呼出し時に actor と `AuthorizedSourceScope` の tenant/source/registration・visibility revision/owner/lease を再検証する。P5 の resource lookup も `(AuthorizedSourceScope, ResourceId)` を入力とし、裸の global ID lookup に戻さない。P7 は adapter 未配線時に起動拒否する。provider 側 `current_access` と application 側の最終 access pass が異なる actor を見ないよう、すべて同じ scope から派生する。Claim selector の不可視/別 tenant ID は unknown と同一に扱い、存在だけで `Sufficient` にしない。Source の public access contract を例外とする場合も registration が許可する非機密 field に限り、Source visibility と item access を省かない。

## 3. Remote operation、coverage、absence

server-owned `RemoteSourceRegistration` は tenant/source、provider kind、固定 endpoint ref、supported modes、enumeration semantics、authority/predicate scope、allowed resource kinds、current access contract、retention/content/freshness contract、transport limits、**canonical upstream lineage policy** を持つ。provider response はこれらを書き換えられない。`RemoteQueryInput` は allowlist 済み text/facet と bounded window、`OpaqueNativeId` は URL ではない bounded UTF-8 ID、cursor は adapter 内部値のみ。固定 endpoint 以外を hit や request から fetch しない。

| Mode | 応答/coverage | miss と整合条件 |
| --- | --- | --- |
| `REMOTE_ENUMERATION` | 同じ source snapshot と scope/revision を持つ全 page が terminal cursor へ達した時のみ `CompleteEnumeration`。途中は `PartialEnumeration` | 完全 sweep の既知 ID に限り adapter が `VerifiedAbsence` を出せる。page 欠落、cursor 反復、token 変更、ACL 変更では absent/deletion にしない |
| `REMOTE_QUERY` | `QueryResult`、または登録済み部分列挙としての `PartialEnumeration`。query の全 hit は Source 全体の complete を意味しない | 空結果は `Unknown`。Source の Resource を削除しない |
| `DIRECT_ADDRESS` | source-native ID の exact lookup。`DirectLookup` | 登録済み authoritative・ACL-unmasked absence capability と current scope の証明がある場合だけその ID に `VerifiedAbsence`。通常 404/403 は `Unknown` |
| `LIVE_ONLY` | 一回の query/direct response を評価 RAM の `QueryResult`/`DirectLookup` として扱い、reusable collection を作らない | miss は `Unknown`。後の probe/materialization は fresh read し、評価時 version/digest と照合できなければ再 qualification を要求する |

`VerifiedAbsence` は provider の文字列ではなく、adapter が `(tenant, source, authorized_scope, source_snapshot_or_lookup_token, exact_native_id, coverage, access_revision, observed_at)` を検証して作る非公開 receipt とする。`Presence::Absent` を生成できるのはこれだけ。response status、search total、page estimate、probe `NotFoundByProbe` は単独では absence 証拠にならない。Source outage は availability gap であり Resource lifecycle の一斉更新にしない。同じ native ID/version で異なる digest は `IntegrityConflict` として閉じ、古い binding を暗黙更新しない。

## 4. 一 Source・一 generation の execution contract

```rust
struct TrustedRemoteContext {
    binding: TrustedDiscoveryBinding,
    source_scope: AuthorizedSourceScope,
}
enum RemoteOperation {
    Enumerate { cursor: Option<OpaqueCursor> },
    Query { input: RemoteQueryInput },
    Lookup { native_id: OpaqueNativeId },
    Live { input: LiveInput },
}
struct RemoteActionResponse {
    retriever_id: String,
    operation: RemoteOperationKind,
    source_snapshot_proof: SourceSnapshotProof,
    coverage: Coverage,
    hits: Vec<UntrustedRemoteHit>,
    absence: Vec<VerifiedAbsence>,
}
enum RemoteAccessTarget { SourceScope, Resource(RemoteIdentity) }
trait RemoteSourcePort: Send + Sync {
    fn execute_batch<'a>(&'a self, context: &'a TrustedRemoteContext,
        actions: &'a [PlannedRemoteAction]) -> BoxFuture<'a, Vec<RemoteActionOutcome>>;
    fn current_access<'a>(&'a self, context: &'a TrustedRemoteContext,
        target: &'a RemoteAccessTarget) -> BoxFuture<'a, AccessDecision>;
    fn current_policy<'a>(&'a self, context: &'a TrustedRemoteContext,
        target: &'a RemoteIdentity) -> BoxFuture<'a, CurrentSourcePolicy>;
    fn probe_or_materialize<'a>(&'a self, context: &'a TrustedRemoteContext,
        target: &'a PinnedRemoteTarget, stage: MaterializationState)
        -> BoxFuture<'a, RemoteReadOutcome>;
}
struct RemoteEvaluationGeneration {
    key: ProjectionGenerationKey, // application mints one key per Source/evaluation
    owner: EvaluationLeaseId,
    source_snapshot: VerifiedSourceSnapshot,
    action_receipts: Vec<ActionReceipt>,
    resources: BTreeMap<ResourceId, CompiledResourceProjection>,
    lists: Vec<RetrieverRankList>,
    // selectors, assertions, concept view, evidence, allowed Graph relations
}
```

`SourceSnapshotProof` は登録済み adapter が provider token、固定 Source scope、current ACL revision、観測時点を検証した値。token 文字列だけを比較して保証したことにはしない。`REMOTE_ENUMERATION` の page は全て同じ proof に属する。複数 mode を一 Source batch に含める場合は **すべての action が同じ verified source snapshot と scope/revision を指す**ことが条件。共有 snapshot を証明できない provider（典型的な `LIVE_ONLY`）はその評価で一 action のみ選ぶ。その場合の manifest `source_snapshot` は adapter が計算した単一応答の opaque fingerprint とし、Source 全体の complete/fresh guarantee を意味しない。複数 action が必要でも証明不能なら他 action は `remote_snapshot_incompatible` の `UnsupportedCoverage` gap とし、新しい Discovery evaluation を必要とする。異なる action response を見かけ上同じ key に押し込まない。

実行順は次のとおり。`ScopedDiscoveryService` が可視 Source と必要入力・adapter capability・予算から remote Source ごとの bounded action batch を **最初の federation 前**に確定する。`RetrieverSupport` と `RetrievalInputs` に四 mode の利用可否と typed input を加え、planned/unsupported を実接続に一致させる。Source scope の current access を network 前に確認する。adapter が有限 deadline 内に batch を取り、各 response の tenant/source、registered mode、snapshot proof、cursor、identity、version/digest、retention、field/provenance を検証する。成功 action を `RemoteGenerationBuilder` に stage し、Source 単位で一回だけ `seal()` する。seal 後は resources、selectors、assertions、evidence、Graph、list の追加・置換を禁止する。partial outage は成功 action のみによる generation と action ごとの gap を返せるが、同一 snapshot の証明が失われた場合は Source batch 全体を破棄する。Required Source は planned だけでは executed と数えず、少なくとも一つの有効な access-checked action が完了した時だけ executed とする。残りの失敗 gap は消さない。

同じ ResourceId が複数 action に現れたら、Source-native ID、version、adapter 計算 digest、field ごとの typed value と provenance が一致する canonical projection だけを共用する。部分 projection の補完は同一 snapshot/version と field provenance が検証された場合に限る。version/digest/同名 field が食い違えば Source batch を `IntegrityConflict` として失敗させ、異なる Claim、Fact、identity evidence、binding に合成しない。provider score は Source-local 診断値だけで、S1 の Source 間比較に使わない。

`RemoteEvaluationGeneration.key` は provider が指定せず application が collision check 後に mint する。global SourceId registry invariant と request の tenant/source binding を検証してから key を登録する。durable `ProjectionGenerationStore` へ書かず、`PersistableGenerationManifest` に変換しない。composite read adapter は key の明示 registry から durable pin または評価 RAM を選び、UUID 推測で dispatch しない。ひとつの Source で durable と remote の両方を広告する場合も、同じ評価では trusted plan が **どちらか一つの generation domain** を選ぶ。両方の hit を同じ Source の list に混ぜる能力は P4 に含めず、必要なら gap と新しい評価にする。

全 `RawRetrievalHit.generation`、`RetrieverRankList.generation`、`HitRecord` の read key、`PinnedSource.key`、`assemble_resource_claims` の key は Source の sealed key に一致させる。list/hit の Source と generation の相違は structural error。複数 mode は list を分けても key は一つで、現行 `CandidateFederator::validate_lists` の単一 Source generation 制約を保つ。`RetrieverHitTrace` には同じ key と個別 action receipt を紐付けるが、raw provider token は出力しない。`seen_resources` は `(key, ResourceId)`、probe binding は同一 key・Resource/version/digest に限定する。sealed 後に adaptive selector が追加 remote action を欲した時は現在評価に append せず `remote_expansion_requires_new_evaluation` gap を出す。新しい評価 ID で新しい snapshot を取得する。local Source の既存 adaptive loop と Source 間 S1 順は変えない。

probe、detail、evidence を seal 後に remote から再取得する必要がある場合は、current access と `PinnedRemoteTarget` の snapshot/version/digest を再検証する。provider が同一 snapshot を指定して返せない時は当該 fact を `Unknown` とし、元の immutable projection へ追記しない。`LIVE_ONLY` の fresh read が違う内容なら現在評価の candidate に結合せず、new evaluation/rebind を要求する。開示直前に actor/source/item/field access を再検証し、取消を観測した Source の全派生結果を捨てて再評価する。

## 5. Identity、qualification、binding

登録済み stable native ID は bounded canonical form に正規化し、length-framed `(tenant, source, provider kind, native ID)` に versioned SHA-256 namespace を適用して Source-local `ResourceId` と candidate ID を adapter が生成する。provider は `resource_ref`、`candidate_id`、`source_ref` を選べない。stable ID の `RemoteStableReference` は `resource_ref: Some(id)`、sealed generation の projection、current item access、hard gates を満たした時だけ既存 `QualifiedResource` へ進む。synthetic HTTP provider は全四 mode でこの経路を使い、`DiscoveryService::discover` の本物の qualification/evidence/sufficiency まで検査する。

native stable ID のない response は `EphemeralCandidate` として評価 RAM 内で識別してよいが、P4 では federation list に入れず、認可済み Source に対し `ephemeral_identity_not_qualifiable` の `UnsupportedCoverage` gap を出す。`resource_ref: None` を現行 `DiscoveryService::evaluate` に黙って通して `continue` させない。`ProbeExecutionService`、`SessionWorkingSet::bind_resource`、`RepresentationBinding`、durable GET link に渡さない。後続設計が evaluation-only ResourceId/projection と current target access を規範化しない限り、ID なし候補の qualified E2E は未対応と明記する。

stable `LIVE_ONLY` candidate は評価中の immutable response projection に対してのみ qualify する。`LiveReference` binding を作る場合は `RevalidationMarker::Required`、stable native ID と version/digest を含む `PinnedRemoteTarget` を必要とする。実行時は Source の current access/policy/representation を再取得し、同じ version/digest と確認できた場合だけ使用する。version/digest がない、または変わった場合は binding を暗黙更新せず新しい Discovery/qualification を求める。remote version pin は version identity、snapshot pin は version または digest の既存 validation を守る。provider locator は表示用の不信データで fetch destination ではない。

Graph へ入るのは explicit typed n-ary relation、validated participant、同じ generation と可視 scope のものだけ。`SESSION_ONLY` は session Graph、`NO_RETENTION` は evaluation-call Graph に限り、persistent Graph へ昇格しない。Graph path の全 participant と relation metadata も current access で検査する。

## 6. Evidence role と upstream independence

`ResolvedAssertionEvidence` の `role`、`is_summary`、`upstream_origin` が `search-core::evidence::is_verified_direct_claim_evidence` と independent-origin count を決める。remote JSON の同名 field は**証拠ではない**。adapter は provider payload をまず `UntrustedEvidenceHint` として保持し、server-owned registration の predicate/authority grant、固定 Source の検証済み provenance lookup、対象 Resource/version/digest、directness、citation chain の整合を検証した場合だけ private constructor の `VerifiedProvenance` を作る。`EvidenceResolverPort` はこの値からのみ `ResolvedAssertionEvidence` を構築し、generation/source/resource/ref の一致も既存 `assemble_resource_claims` に検査させる。

`AssertionOrigin::Authoritative` は registration がその tenant/source/predicate/authority scope を明示的に grant し、該当 assertion の version と evidence provenance が検証された時だけ付与する。`Primary` はその Source が直接作成した、summary ではない一次証拠に限る。`Corroborating`/`Contradicting` も同じ directness 検証が必要。引用・要約・転載・モデル推定・未検証の provider claim は `Contextual`/`Derived` または `is_summary=true` とし、unknown Claim を direct support に昇格させない。provider の自己申告だけで role を変更できない。

`upstream_origin` は provider 表示ラベルでなく、登録済み canonical lineage group の opaque ID とする。同じ upstream をミラーする二 Source、同一 provider の別ラベル、同じ文書の別引用は同じ group へ畳む。独立性を証明できない lineage はその provider の単一 group へ畳むか direct evidence として採用しない。異なる verified lineage group の時だけ `minimum_independent_sources` の複数個に数える。合成 provider では catalog 自体を一つの canonical origin とし、別の JSON `origin` ラベル二つで二票にしない。`UntrustedEvidenceHint` は audit/telemetry や persistent projection に丸ごと保存しない。

## 7. Retention owner と無効化 state machine

全 store の entry は `Owner {TrustedSearchScope, AuthorizedSourceScope, evaluation?, retention_mode}` と `Lease {absolute_deadline, idle_deadline?, provider_expiry?}` を持つ。response body、metadata、assertion、evidence、claim selector、Graph、probe、receipt、materialization bytes、query/result trace、cache key、derived index を同じ owner/lease の管理対象にする。`Open` の read/write ごとに current actor/source/item/field policy、retention、lease、revision を確認し、いずれか不一致なら entry を `Revoked`/`Expired` へ単調遷移させる。`Building -> Open -> Closing -> Closed`、または任意時点から `Revoked`/`Expired` へ遷移し、後二者は再開しない。close、absolute/idle expiry、ACL/retention revision 変更、取消、error、cancel では Source owner の全 projection/Graph/probe/receipt/materialization bytes と参照 handle を一括無効化する。非同期 read の終了直前にも lease/current access を検査し、失効した値を返さない。raw `&`/`Arc` や iterator が lease を迂回して読める API は公開しない。

| Retention | 所有と期限 | 許す store / 禁止事項 |
| --- | --- | --- |
| `PERSISTENT_RESOURCE` | Source grant と field allowlist の範囲で durable generation。current access は各 read で再検査 | 再生成可能 projection のみ。credential と raw provider response は保存しない |
| `PERSISTENT_DISCOVERY_METADATA` | 同上、metadata field proof 必須 | identity/title/facet/provenance の許可 field のみ。body、fragment、content-bearing assertion/evidence、禁止 relation は typed constructor が拒否する。現在の粗い `PersistableResourceProjection` 判定だけでは write しない |
| `CACHE_WITH_EXPIRY` | tenant/source/principal/access revision/normalized operation ごとの bounded RAM cache。`min(provider TTL, registration ceiling, actor/session absolute expiry)` | read/write 時に provider expiry と current policy を再確認。revision/retention 変更で全派生を invalid。durable conversion と principal 間再利用は禁止。cache hit も新 evaluation generation へ seal し直す |
| `SESSION_ONLY` | trusted session owner、absolute deadline と idle deadline の両方。session close で即 invalid | session RAM working index/Graph/receipt/bytes のみ。同一 session の再読でも gate を通す。disk spill、serialize、persistent projection は禁止 |
| `NO_RETENTION` | `EvaluationLease` は `discover`/materialization の success/error/cancel/deadline で即終了。許可済み返却 field だけ別の短命 `TransientDisclosureLease` で送出完了まで所有 | evaluation RAM の projection/Graph/probe/bytes だけ。session、cache、persistent index、audit/telemetry payload、fixture、log、temp file、dump に由来データを残さない。評価内 store は呼出し終了で再読不能、返却 buffer も送出後に再読不能 |

`NO_RETENTION` は二段階で release する。(1) `DiscoveryService::discover`/materialization の返却、error、cancel、deadline 時点で `EvaluationLease` を必ず close し、remote generation、projection、Graph、probe、receipt、raw response とその全 handle を破棄する。(2) 最終 access/field gate を通った返却 field だけを裸の owned `DiscoveryResult` でなく `TransientDisclosure<T>` に移し、`ScopedDiscoveryService` が actor/source と `TransientDisclosureLease` に結び付ける。P4 harness はこの短命 lease 内でのみ結果を検査する。P5 HTTP handler は同じ lease 内で最終 actor/source/item/field access を再検査し、許可 field だけを bounded response stream に直列化する。stream への書込み完了、body 生成/送信 error、client disconnect、cancel、deadline のどの経路でも `TransientDisclosureLease` を close する。送出中の buffer は transport の有限 deadline と backpressure の下だけに存在し、queue、retry spool、server-side cursor/cache、background task へ渡さない。lease close 時に未送出 buffer を破棄し、送出済み bytes も server に保持しない。`TransientDisclosure<T>` は clone/serialize/保存用変換を提供せず、scope 外に raw `T`/`Arc` を取り出せない。HTTP を持たない P4 harness でも明示 `with_disclosure` callback の return/error/cancel 後に同じ drop を検査する。これを P5/P7 の共通 response lifetime contract とし、P7 composition root は stream/ログ/エラー処理がその契約を保てない場合に起動拒否する。

`NO_RETENTION` は session 内で再利用せず、後続呼出しは新しい evaluation と fresh provider read を必要とする。`SessionWorkingSet` の現行 public `bound`/`pinned_generation`/`state` などをそのまま lease 境界として使わない。session wrapper または同等の内部改修で全 read/write、receipt、materialization を owner gate の下へ入れるまで `SESSION_ONLY` を有効化しない。`NO_RETENTION` は `SessionWorkingSet` に保存しない。既存 `MaterializationReceipt::to_session_store_record` と `PersistableGenerationManifest`/`PersistableResourceProjection` の非永続 mode 拒否は維持する。retention を緩める変更で旧 entry を昇格しない。厳格化時は即 invalid。`NO_RETENTION` の telemetry/audit は provider 内容、query、candidate ID、locator、具体 gap、body、digest を持たず、この mode 由来の per-call payload を作らない。runtime の request/body debug log、temporary response spool、core dump を無効にした構成を受入条件にする。合成試験 data は実行時生成し、終了後に fixture として保存しない。

## 8. Synthetic HTTP と SSRF 境界

実通信資格には OS 割当 port の `127.0.0.1` 合成 knowledge catalog を使う。credential-free、二 tenant、stable native ID、version/digest、ACL revision、canonical upstream、transient body を runtime seed から生成する。`GET /v1/catalog?cursor=`、`POST /v1/search`、`POST /v1/lookup`、`POST /v1/live`、`POST /v1/authorize`、`GET /v1/content/{encoded-native-id}` を登録済み adapter が呼ぶ。これはテスト provider protocol であり production endpoint 選定ではない。`/authorize` は source/item current access と ACL revision、content は binding の version/digest を検査する。provider 側 filtering だけを唯一の認可とは扱わない。

transport policy は production で HTTPS、operator-configured の exact origin/port/path、userinfo 禁止、ambient proxy 禁止、自動 redirect 禁止、caller/provider URL 禁止とする。接続ごとに A/AAAA 全件を検査し、loopback/private/link-local/multicast/unspecified/metadata/reserved と IPv4-mapped IPv6 を拒否する。検査済み address に接続を pin し、元の hostname で TLS/SNI/certificate を検証する。再解決と connection-pool reuse でも同じ allowlist を適用し、DNS preflight 後の通常 resolver による rebinding を許さない。テスト専用 adapter instance にだけ明示的な loopback 例外を注入し、production config には表現できない型/構成にする。HTTP client library は公式資料と隔離 PoC で機能・依存を確認してから selection 記録を更新し、`POC REQUIRED` のものを未資格で production crate に入れない。

初期合成 canary の上限は call 2 秒、evaluation 5 秒、request 16 KiB、decoded response 1 MiB、100 hits/page、32 pages/HTTP requests、3,200 hits、4 remote actions、native ID 512 bytes、cursor 1 KiB、JSON depth 32。これらは試験値であり production SLO ではない。`Content-Length` と実際の decoded stream bytes の両方を数え、decompression 後上限を超えない。全 string/array、並行数、総 RAM、connect/read/total deadline、cancel を制限する。初期 adapter に automatic retry はない。timeout、429、5xx、malformed/truncated body、oversize、redirect、rebind は低 cardinality の availability/coverage gap へ変換し、`Absent` にしない。cancel は body を閉じ、disk に書かない。remote text と tool 風文字列は不信データであり instruction、tool dispatch、URL fetch、authority に昇格しない。

## 9. 実装段階と受入証拠

1. 関連 `spec/` の remote outcome、Source visibility、single-generation、ephemeral gap、provenance、retention lease を規範化してから、契約 RED を作る。既存 frozen semantics を弱める変更はしない。
2. `search-application` の scoped entrypoint、access binding/visibility wrapper、planner support/typed inputs、`RemoteSourcePort`、Source batch builder/seal、composite read ports、`DiscoveryService` への単一 pin 配線を実装する。Remote hit も現在の hard gates、`CandidateFederator::merge(PriorityConcat)`、qualification、Claim resolution、最終 access pass を通る。local path と S1 順を回帰確認する。
3. metadata-only field proof、cache/session/evaluation lease、close/revocation/expiry/cancel の全-store invalidation を作る。無効化前に取得した handle から expiry 後に再読できない試験を先に通す。`NO_RETENTION` を `SessionWorkingSet` と `Persistable*` に入れない。
4. `search-source-http` と runtime 生成の合成 provider を実 TCP で接続する。HTTP client は別 PoC/selection を経る。Source current access、SSRF、bytes、deadline、provenance を adapter 単体と service の両方で試験する。
5. 一つの `DiscoveryService::discover` E2E で、trusted tenant/principal/session → visible registry → remote route → 実 HTTP → sealed generation → common federation → current access → qualified resource と Claim/evidence/sufficiency まで確認する。200 や in-memory fake retriever のみでは P4 達成としない。
6. 独立 read-only review、focused contract/E2E、strict Clippy/fmt、関係 repo gate、実装 Draft PR の exact-head hosted gate を記録する。性能・運用値は実測と提案を分ける。

最低限の反例 matrix:

- 同一 Source の query と direct lookup が同一 verified snapshot/version/digest を返すと、二 list は同じ generation で S1 と qualification を通る。異なる snapshot、同一 ID/version の異なる digest、同名 field の矛盾は merge/Claim/binding 前に Source batch を失敗させる。異なる durable key の二 list は現行通り拒否する。
- `REMOTE_ENUMERATION` 完全 terminal sweep だけが既知 ID の absence receipt を作る。page 失敗/ACL 変更/partial/query miss/通常 404/403/`LIVE_ONLY` miss は `Unknown`。outage は既存 durable Resource state を変更しない。
- access handle を tenant B/principal B のものへ置換、期限切れ handle、別 session、別 evaluation、別 revision を拒否する。異なる tenant へ同一 SourceId を再利用する登録は P7 startup と registry update の両方で拒否する。Required/Preferred に別 tenant/不可視 Source ID を渡し、登録なしと同じ gap/trace/result になる。途中 Source/item/field ACL 取消では候補・claim・rank・trace・locator が一つも漏れない。
- stable ID の四 mode は実 HTTP から qualified result へ到達する。ID なし `EphemeralCandidate` は明示 gap、qualified result なし、probe/binding/GET なし。`LIVE_ONLY` 再取得内容変更は古い candidate/binding に結合しない。
- provider JSON の `Primary`/`Authoritative`/異なる `origin` ラベルを自己申告させても昇格しない。同一 canonical upstream の二ラベルは一票。provenance 不明は `Unknown`。独立二 origin は登録済み lineage 証拠がある時だけ sufficiency に数える。
- 五つの retention mode で許可 store だけを使う。`NO_RETENTION` は response stream success/error/disconnect/cancel/deadline 後の全 server-side store、Graph、probe、receipt、response buffer、log、audit/telemetry payload、fixture が空で、保持済み handle からも再読できない。`SESSION_ONLY` は別 owner・idle/absolute expiry・close、cache は provider TTL/access revision/retention change で読み書きとも拒否する。materialization は current permission/budget と version/digest を再検証する。
- redirect、DNS rebinding、private/metadata address、proxy、oversize decoded body、timeout、malformed response を fail closed にし、独立可視 Source の結果は保持する。

## 10. 独立 review 指摘との対応

| Review | 本書で固定した修正 |
| --- | --- |
| P1: action ごとの generation が Federation と衝突 | §4: seal 前 Source batch と一 Source/evaluation 一 key、全 list/hit/Claim/read に同 key、異 snapshot の混合禁止 |
| P1: trusted actor と access_context/Source trace の断絶 | §2: P4 entrypoint で binding 検証、可視 registry/routing、全 port の同一 binding、不可視 Source gap/trace の正規化 |
| P2: ephemeral qualified path の空白 | §5: stable ID のみ qualified、ID なしは明示 gap と probe/binding 禁止、LIVE_ONLY 内容再結合禁止 |
| P2: NO_RETENTION/session/cache owner と expiry の空白 | §7: owner/lease/state machine、全 read/write gate、一括 invalidation、成功/error/cancel と保持参照の試験 |
| P2: provider 自己申告 evidence role/origin | §6: private verified provenance、registration grant と canonical lineage、同一 upstream の false corroboration 拒否 |

未凍結事項は実装時の HTTP library selection と具体的な `spec/`/Rust 型配置であり、上記の意味境界を変更しない。これらを理由に P4 の review 指摘を P5 transport へ先送りしない。
