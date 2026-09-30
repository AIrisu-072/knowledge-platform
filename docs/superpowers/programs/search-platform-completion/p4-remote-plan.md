# P4 Remote Source Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task by task. In this repository, dispatch and checkpoint are controlled by `orchestrate` → `toolbox-context`; this document supplies the bounded task contracts, not a hand-written wave schedule. Mark steps with `- [ ]` as evidence is produced.

**Goal:** Four provider-neutral remote discovery modes reach the existing `DiscoveryService` qualification/evidence path through one trusted, access-checked Source evaluation and a real local TCP synthetic provider.

**Architecture:** A request-scoped service binds a server-issued actor and visible Source catalog before routing. It prepares at most one sealed immutable RAM generation per Source/evaluation, then gives the existing router, executor, federation, hard gates, Claim resolver, and final access pass a composite read view keyed by the exact generation. Owned leases gate every derived store and response disclosure; the HTTP adapter is below application ports.

**Tech Stack:** Rust 2024 / 1.98, existing `search-core` and `search-application`, Tokio, serde/sha2; the HTTP client is selected only after an isolated credential-free PoC and dependency/license check.

**Spec:** `docs/superpowers/programs/search-platform-completion/p4-remote-freeze.md`; exact design `p4-remote-design-revision-1.md` SHA-256 `f4112c7aa0cf7cbf61dca19c4beebcfb14a6578432ac45861644ab8ea616f9e9` (independent GO). Normative inputs: `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md` §§5–6, 17–18, 42; `spec/data/transaction-consistency-requirements-v0.md` SD-T2/3/4/7; Search amendments in `spec/data/data-characteristics-v0.md` and `spec/operations/observability-audit-requirements-v0.md`.

## Global Constraints

- The freeze supersedes the design revision's earlier “review pending” header. No change to Source-owned truth, S1 retriever-order `PriorityConcat`, local Discovery business logic, current authorization, Source-local identity, or Observation/Resource lifecycle separation.
- One Source/evaluation has **one** sealed generation key. Never stage incompatible snapshot, ACL revision, Resource version/digest, or field provenance under one key; never write an evaluation generation to `ProjectionGenerationStore`.
- `REMOTE_QUERY`, incomplete enumeration, ordinary 403/404, `LIVE_ONLY` miss, timeout and outage are `Unknown`/gap, never Resource deletion. Only verified complete enumeration or registered authoritative unmasked direct absence creates `Absent`.
- Globally unique server-owned `SourceId`; tenant, principal, session, handle, evaluation, Source visibility and revisions come only from trusted in-process authority/registry, never HTTP/request/provider fields.
- All five `RetentionMode` variants must gate derived bytes, metadata, selectors, assertions/evidence, Graph/probe/receipt, cache and output; nonpersistent modes cannot enter durable conversion. `NO_RETENTION` evaluation data closes at return/error/cancel/deadline and its bounded disclosure closes at send completion/error/disconnect/cancel/deadline.
- Production transport requires HTTPS, fixed operator endpoint, no userinfo/ambient proxy/redirect/provider URL, validated and pinned A/AAAA on every connection and pool reuse, hostname TLS verification, bounded decoded bytes/deadlines, and no automatic retry. A loopback exception exists only in a test-only adapter constructor.
- Synthetic canary limits: call 2 s; evaluation 5 s; request 16 KiB; decoded response 1 MiB; 100 hits/page; 32 pages or HTTP requests; 3,200 hits; 4 remote actions; native ID 512 bytes; cursor 1 KiB; JSON depth 32. These are canary limits, not production SLO measurements.
- No external credentials, customer data, live endpoints, merge or deploy. Do not promote a `POC REQUIRED` library to production before its PoC, permissive transitive-license/source/advisory gate, and explicit selection record. No intermediate human approval is required by this plan.
- Focused test commands below use `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0` to avoid large debug builds. A task records its RED output, focused GREEN output and changed-file review; full repo gates are reserved for final integration.

## Review Focus

Each risk below has a named owning test in the tasks, in addition to the principal acceptance tests.

| Likely input/condition | Expected behavior | Owning test |
| --- | --- | --- |
| Required Source ID belongs to another tenant or is invisible | Same generic gap/result/trace as nonexistent ID, without the hidden ID | P4-03 `hidden_required_is_indistinguishable_from_missing` |
| Two responses share a native ID/version but disagree on digest or a same-name field | Discard the Source batch before federation/Claim/binding | P4-06 `conflicting_projection_aborts_source_batch` |
| ACL changes while an async evidence read is suspended | Remove every derived candidate/Claim/rank/trace/locator before return | P4-13 `revoke_during_evidence_read_removes_source` |
| A saved handle is read after owner expiry/close | No raw `&`/`Arc` path returns data; read and late async completion deny | P4-07 `held_handle_cannot_read_after_expiry` |
| Provider supplies `Authoritative`, `Primary`, and two origin labels | No self-elevation; one canonical upstream counts once | P4-10 `provider_labels_cannot_create_two_direct_origins` |

## File ownership and integration seams

The following names are **new contracts**, not presumed existing APIs. Existing signatures are called out in tasks. Keep single-purpose modules; production dependency direction is `search-source-http` → `search-application` → `search-core`.

| File scope | Responsibility | Shared edit owner |
| --- | --- | --- |
| `spec/data/transaction-consistency-requirements-v0.md`, `spec/data/data-characteristics-v0.md`, `spec/operations/observability-audit-requirements-v0.md` | Frozen remote semantics made normative | P4-01 only |
| `crates/search-application/src/{scoped,remote_registration,remote_identity,remote_observation,remote_generation,remote_lease,remote_evidence,remote_read_view,remote_cache,remote_session,remote_disclosure}.rs` | Separate trusted binding, registration, identity/absence, sealed batch, owner/lease, evidence, composite view and stores | Each named task owns its module |
| `crates/search-application/src/{routing,retrieval}.rs` | Visible routing and actual four-mode plan/support/input | P4-03/P4-04; serialize their edits |
| `crates/search-application/src/{ports,retrieval_execution,discovery_service,lib,projection}.rs` | Common port interfaces, executor, service integration, exports, metadata persistence guard | P4-02/05/09/11/12/13/14; serialize each edit, including each new module's `lib.rs` export |
| `experiments/search-http-client-poc/**` and `spec/selection/library-tool-selection-v0.md` | Isolated HTTP library qualification and explicit selection | P4-15 only, before root Cargo edits |
| `crates/search-source-http/**` | Fixed-origin transport, provider protocol/adapter, synthetic TCP tests | P4-16–P4-19 own disjoint source/test files |
| Root `Cargo.toml`, `Cargo.lock`, adapter `Cargo.toml`, architecture policy/CI port lists if needed | Production promotion and new crate wiring | P4-16 only; serialize with other program Cargo/architecture edits |

The task graph should derive dependencies from the **Consumes/Produces** lines. No worker may edit another task's module or concurrent shared Cargo/lock/port files. Review the actual worktree/HEAD before execution because other completion-program tasks share this checkout.

### P4-01 — Normative remote contract

**Files:** Modify the three `spec/` files named above; test by reviewing their exact diff against the frozen design.

**Consumes:** frozen P4 design and SD-T2/3/4/7. **Produces:** normative Source visibility, one-generation, verified absence, ephemeral gap, evidence provenance and owner/lease clauses. Do not reopen approved business semantics.

- [ ] Add narrowly scoped Search amendments: trusted actor/Source binding before route and each read; one Source/evaluation sealed key; only verified absence; stable-ID qualification and explicit ID-less gap; provenance proof; five retention owners and two-stage `NO_RETENTION` disposal.
- [ ] Check `git diff --check -- spec/data/transaction-consistency-requirements-v0.md spec/data/data-characteristics-v0.md spec/operations/observability-audit-requirements-v0.md`; compare each new clause with design §§2–7 and record clause-to-task mapping. Expected: no conflicting pre-existing clauses or whitespace errors.

### P4-02 — Trusted actor and globally unique visible catalog

**Files:** Create `crates/search-application/src/scoped.rs`, `remote_registration.rs`; test `crates/search-application/tests/scoped_catalog_contract.rs`; modify `src/lib.rs` only for exports at the integration seam.

**Consumes:** P4-01. **Produces:** search-local opaque `TenantId`, `PrincipalRef`, `AccessContextHandle`, `AccessRevision`, `RegistrationRevision`, `VisibilityRevision` (do not import a Document Domain principal into Search); `TrustedSearchScope`, `TrustedDiscoveryBinding`, `AuthorizedSourceScope`, `VisibleSourceRegistration`, `RegisteredEndpoint { scheme, host, port, base_path }`, `RemoteSourceRegistration`, `AccessContextAuthorityPort`, `CurrentSourceVisibilityPort`, `ScopedSourceRegistryPort`, and `RemoteRegistrationCatalog::{try_new(registrations: Vec<RemoteSourceRegistration>) -> Result<Self, SearchError>, replace_checked(&mut self, registrations: Vec<RemoteSourceRegistration>) -> Result<(), SearchError>}`. Use the design §2 port signatures and `crate::ports::BoxFuture`; private fields/constructors, with an in-process issuer owned by the authority/visibility adapter for the synthetic harness. Registration carries tenant/source/provider kind/fixed endpoint/modes/authority predicates/current-access/retention/freshness/canonical lineage/limits; provider cannot choose `SourceId`.

- [ ] RED: `scoped_catalog_contract::{cross_tenant_source_id_collision_rejected_on_start_and_update,foreign_handle_and_expired_revision_rejected_before_registry,visible_sources_only_return_bound_scopes}`. Assert no Source/remote port call on bad handle or revision, and no arbitrary duplicate winner.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test scoped_catalog_contract`; expected compile/behavioral failure solely from absent contracts.
- [ ] Implement sealed scope issuance/current checks and atomic catalog replacement; validate all tenant IDs, unique SourceId, registration/source revision and visible Source's tenant before projection to `DiscoverableSource`.
- [ ] Rerun the same command; expected all named tests PASS. Commit this task's files after review.

### P4-03 — Visibility-safe SourceRouter input

**Files:** Create `crates/search-application/src/visible_routing.rs`; test `tests/visible_routing_contract.rs`; modify `routing.rs` only where the trusted route interface requires it.

**Consumes:** P4-02 catalog/scopes and existing `SourceRouter::plan(&DiscoveryNeed, &[DiscoverableSource], &RoutingConstraints) -> SourceRoutePlan`. **Produces:** `VisibleRouting::prepare(actor: &TrustedSearchScope, visible: &[VisibleSourceRegistration], requested: &RoutingConstraints) -> Result<(Vec<DiscoverableSource>, RoutingConstraints, Vec<InformationGap>), SearchError>`; the Source list and required/preferred IDs are intersected before calling the existing router. Missing and invisible Required IDs use one ID-free `required_source_unavailable` gap; invisible Preferred IDs vanish.

- [ ] RED: `visible_routing_contract::{hidden_required_is_indistinguishable_from_missing,foreign_preferred_never_appears_in_route_or_trace,duplicate_registration_never_routes}`. Compare full externally visible results, not just a boolean.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test visible_routing_contract`; expected RED.
- [ ] Implement the wrapper and keep `SourceRouter`'s local ordering/role behavior for visible IDs; do not feed hidden IDs to `SourceRouter::plan` where it emits `source:{id}`.
- [ ] Rerun the focused command plus `cargo test -p search-application --locked --test routing_contract`; expected PASS.

### P4-04 — Four-mode planner support and typed inputs

**Files:** Modify `crates/search-application/src/retrieval.rs` and `routing.rs`; test `tests/remote_planner_contract.rs`.

**Consumes:** P4-02 `RemoteSourceRegistration`, P4-03 visible routing, existing `RetrieverPlanner::plan`, `RetrieverSupport`, `RetrievalInputs`. **Produces:** `RemoteQueryInput { text, facets, window }`, `OpaqueNativeId`, `LiveInput` with bounded constructors, four explicit support flags and source-local typed inputs; `RetrievalPlan` may schedule only registered, visible, currently wired modes. A Required remote route is Initial; planned state never claims execution. Unsupported/missing input is `ActionState::Unsupported/Unresolved`, not a fake `Planned` action.

- [ ] RED: `remote_planner_contract::{registered_remote_modes_plan_only_with_port_and_input,required_remote_has_initial_action,remote_order_preserves_s1_local_order,missing_live_or_native_id_is_unresolved}`. Include a local-only profile regression.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_planner_contract`; expected RED.
- [ ] Extend `RetrieverSupport::contains` and `RetrievalInputs`; replace the current unconditional `NoExecutionPort` remote branch without changing existing local order/`PriorityConcat` order.
- [ ] Rerun focused test plus `cargo test -p search-application --locked --test routing_contract`; expected PASS.

### P4-05 — Provider-neutral remote operation and observation contract

**Files:** Create `crates/search-application/src/remote.rs`, `remote_observation.rs`; test `tests/remote_observation_contract.rs`; modify `src/ports.rs` only to export the new port if required.

**Consumes:** P4-02 trusted context/registration, P4-04 typed inputs, existing `Coverage`, `Presence`, `CurrentSourcePolicy`. **Produces:** the design §4 `RemoteSourcePort::{execute_batch,current_access,current_policy,probe_or_materialize}` signatures, `TrustedRemoteContext`, `PlannedRemoteAction`, `RemoteActionOutcome`, `RemoteActionResponse`, `SourceSnapshotProof`, `VerifiedAbsence` (private receipt constructor), `PinnedRemoteTarget`, and opaque `EvaluationLeaseId`. Use `BoxFuture<'a,T>` and no HTTP type in application. `verify_absence(registration: &RemoteSourceRegistration, context: &TrustedRemoteContext, pages: &[RemoteActionResponse], native_id: &OpaqueNativeId) -> Option<VerifiedAbsence>` checks complete terminal page sequence or a registered authoritative unmasked direct lookup for the exact native ID and current scope/revision.

- [ ] RED: `remote_observation_contract::{query_and_live_miss_are_unknown,only_terminal_consistent_enumeration_proves_absence,plain_404_403_and_partial_page_do_not_prove_absence,outage_does_not_mutate_resource_state}`.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_observation_contract`; expected RED.
- [ ] Implement typed operation/outcome/coverage validation; keep `ResourceObservation` separate from Source business lifecycle and prevent provider strings from constructing absence receipts.
- [ ] Rerun focused test plus `cargo test -p search-core --locked --test resource_contract`; expected PASS.

### P4-06 — Stable identity and one sealed Source batch

**Files:** Create `crates/search-application/src/remote_identity.rs`, `remote_generation.rs`; test `tests/remote_generation_contract.rs`.

**Consumes:** P4-02 registration, P4-05 actions/proofs/`EvaluationLeaseId`, existing `ProjectionGenerationKey`, `CompiledResourceProjection`, `FederatedCandidate`. **Produces:** `remote_resource_id(tenant: &TenantId, source: SourceId, provider_kind: &str, native_id: &OpaqueNativeId) -> Result<ResourceId, SearchError>` and corresponding candidate ID using bounded canonical UTF-8 plus length-framed versioned SHA-256; `RemoteGenerationBuilder::new(context: TrustedRemoteContext, evaluation: DiscoveryEvaluationId, lease_id: EvaluationLeaseId)`, `stage(&mut self, response: RemoteActionResponse) -> Result<(), SearchError>`, `seal(self) -> Result<RemoteEvaluationGeneration, SearchError>`. The builder mints one collision-checked key in application, stores action receipts/list identities and Resource projections in RAM, and never implements `PersistableGenerationManifest` conversion.

- [ ] RED: `remote_generation_contract::{query_and_lookup_share_one_key_when_snapshot_matches,conflicting_projection_aborts_source_batch,different_snapshot_or_acl_revision_aborts_batch,unproven_second_live_action_yields_explicit_gap,idless_hit_yields_ephemeral_gap_without_federation,provider_candidate_id_and_locator_cannot_choose_identity}`. The conflict cases must fail before any list/Claim/binding escapes.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_generation_contract`; expected RED.
- [ ] Implement canonical identity and seal rules: same Source/snapshot/scope/revision; same ID/version/digest/typed field/provenance or verified compatible partial merge; no append after seal; action receipt carries only low-cardinality status. An ID-less hit yields `ephemeral_identity_not_qualifiable` (`UnsupportedCoverage`) and no `resource_ref`.
- [ ] Rerun focused test plus `cargo test -p search-application --locked --test federation_contract`; expected PASS, including existing mixed durable-key rejection.

### P4-07 — Retention owner, lease and guarded handle

**Files:** Create `crates/search-application/src/remote_lease.rs`; test `tests/remote_lease_contract.rs`.

**Consumes:** P4-02 trusted scopes and P4-05 current source policy. **Produces:** `RemoteOwner { actor, source, evaluation, retention_mode }`, `RemoteLease { absolute_deadline, idle_deadline, provider_expiry }`, monotone `LeaseState::{Building,Open,Closing,Closed,Revoked,Expired}`, `GuardedRemoteStore<T>` with `read(&scope, key, gate)` and `write(...)` returning owned/bounded values only. Every operation checks current actor/source/item/field policy, retention/revision and lease; post-await read gate repeats immediately before returning. `invalidate_owner(...)` clears projection/selector/assertion/evidence/Graph/probe/receipt/content/trace handles together.

- [ ] RED: `remote_lease_contract::{held_handle_cannot_read_after_expiry,async_read_finishing_after_revocation_returns_nothing,retention_revision_change_invalidates_all_derived_entries,closed_or_revoked_lease_never_reopens}`.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_lease_contract`; expected RED.
- [ ] Implement owner and clock-injected lease state; do not expose `&T`, `Arc<T>`, iterator, `Deref` or cloneable raw payload outside the guard. Expiry/cancel/error/close must invalidate every handle.
- [ ] Rerun focused command; expected PASS.

### P4-08 — Expiry cache and session owner

**Files:** Create `crates/search-application/src/remote_cache.rs`, `remote_session.rs`; test `tests/remote_store_contract.rs`; modify `session.rs` only for the scoped wrapper's private integration, preserving existing local API behavior.

**Consumes:** P4-07 guarded store, P4-02 actor/session scope, existing `SessionWorkingSet::{bind_resource,bound,pinned_generation,state,durable_record,probe_and_record,materialize}`. **Produces:** `RemoteResultCache::get(&mut self, context: &TrustedRemoteContext, operation: &RemoteOperation, now: Instant) -> Result<Option<RemoteActionResponse>, SearchError>`, `put(&mut self, context: &TrustedRemoteContext, operation: &RemoteOperation, response: RemoteActionResponse, provider_expiry: Instant) -> Result<(), SearchError>` and `invalidate_owner(&mut self, owner: &RemoteOwner)` keyed by tenant/source/principal/access revision/normalized operation, with deadline `min(provider TTL, registration ceiling, actor/session absolute expiry)`; `ScopedSessionWorkingSet::{bind,read,probe,materialize,close}` with full owner gate and no borrowed raw return. Cache hit is raw input to a **new** evaluation seal, never an old generation reuse.

- [ ] RED: `remote_store_contract::{cache_ttl_and_acl_revision_gate_both_reads_and_writes,cache_cannot_cross_principal_or_promote_to_durable,session_owner_idle_absolute_and_close_expire_all_handles,no_retention_never_enters_session,materialization_rechecks_access_budget_and_version_digest}`.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_store_contract`; expected RED.
- [ ] Implement bounded RAM stores using P4-07 gates. The scoped session wrapper must gate every read/write/receipt/materialization; never treat the old public `SessionWorkingSet::bound` reference as a remote lease. A changed `LIVE_ONLY` version/digest requires new discovery/rebind.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test session_context`; expected PASS.

### P4-09 — Metadata-only durable field proof

**Files:** Modify `crates/search-application/src/projection.rs`; create `tests/remote_persistence_contract.rs`.

**Consumes:** P4-02 registration field allowlist, P4-07 retention, existing `PersistableGenerationManifest` and `PersistableResourceProjection::try_from`. **Produces:** `VerifiedPersistentProjection::try_from_remote(projection, registration, current_policy, field_proofs) -> Result<PersistableResourceProjection, ProjectionError>`; explicit metadata-only typed validation for `PersistentDiscoveryMetadata`. Existing local conversion may continue but remote writes must use this constructor.

- [ ] RED: `remote_persistence_contract::{metadata_only_rejects_body_fragment_content_assertion_evidence_and_relation,permitted_identity_title_facet_provenance_persists,credential_and_raw_provider_response_never_persist,session_cache_no_retention_modes_still_rejected}`.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_persistence_contract`; expected RED.
- [ ] Validate each field's registration permission/proof and current field policy before durable conversion. Do not weaken the current nonpersistent rejection or store raw provider response.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test projection_contract`; expected PASS.

### P4-10 — Verified evidence and canonical lineage

**Files:** Create `crates/search-application/src/remote_evidence.rs`; test `tests/remote_evidence_contract.rs`.

**Consumes:** P4-02 registration grants/lineage, P4-06 sealed Resource/version/digest, existing `ResolvedAssertionEvidence` and `assemble_resource_claims`. **Produces:** private `VerifiedProvenance` constructor `verify_provenance(registration, scope, target, hint, lookup) -> Option<VerifiedProvenance>` and `resolved_evidence(key, resource, verified) -> ResolvedAssertionEvidence`. `UntrustedEvidenceHint` is not evidence or a persistable/auditable payload.

- [ ] RED: `remote_evidence_contract::{provider_labels_cannot_create_two_direct_origins,authoritative_requires_predicate_grant_and_matching_version,unknown_lineage_keeps_claim_unknown,two_registered_independent_groups_can_satisfy_threshold,quoted_or_summary_claim_is_not_direct}`. Test both assertion origin and Claim sufficiency through existing resolver functions.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_evidence_contract`; expected RED.
- [ ] Derive role, directness and opaque canonical upstream only from registration/provenance lookup, resource version/digest and citation chain; provider `role`/`origin` text has no authority. Keep unverified refs contextual/summary or absent so unknown Claim stays unknown.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test evidence_resolution_contract`; expected PASS.

### P4-11 — Explicit-key composite read view and remote executor

**Files:** Create `crates/search-application/src/remote_read_view.rs`; modify `src/ports.rs` and `retrieval_execution.rs`; test `tests/remote_read_view_contract.rs`.

**Consumes:** P4-06 sealed generation, P4-07 guard, P4-10 verified evidence; existing `ProjectionGenerationStore::{pin_current,resource_at}`, `ClaimSelectorPort`, `AssertionStorePort`, `EvidenceResolverPort`, `ConceptRegistryPort`, `RetrievalExecutor::execute`. **Produces:** `GenerationReadPort::{pin_current,resource_at}` extracted from the existing store for `DiscoveryPorts.generations` (durable adapter forwards reads; write trait remains separate); `CompositeEvaluationReadView` keyed by an explicit `ProjectionGenerationKey -> {durable | remote lease}` registry, implementing the read port plus selector/assertion/evidence/concept ports; `SealedRemoteRetrieverPort::retrieve(action: &RetrievalAction, key: ProjectionGenerationKey) -> BoxFuture<'_, Vec<FederatedCandidate>>`; `RetrievalExecutionPorts.remote: Option<&dyn SealedRemoteRetrieverPort>`. Remote executor arms produce existing `RawRetrievalHit` with the sealed key and still apply `CurrentCandidateAccessEvaluatorPort`. For remote `ConceptRegistryPort::pin_view`, return only a non-sensitive server-owned concept view (unknown for unproven relations), never an `Arc` to lease-owned provider data.

- [ ] RED: `remote_read_view_contract::{sealed_key_reads_all_five_ports_without_durable_write,unknown_key_never_falls_back_by_uuid,remote_executor_ranks_only_accessible_hits,all_remote_hit_and_list_keys_match_pin,remote_graph_participant_access_is_current}`.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_read_view_contract`; expected RED.
- [ ] Implement explicit dispatch and guarded copy-out for projection/selector/assertion/evidence/concept view. Change remote `RetrievalExecutor` branches from unconditional unsupported to the sealed port, retaining local branches and Graph path checks.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test retrieval_execution_contract`; expected PASS.

### P4-12 — One common Discovery execution path

**Files:** Modify `crates/search-application/src/discovery_service.rs`; test `tests/remote_discovery_contract.rs`.

**Consumes:** P4-03 visible sources, P4-04 plan, P4-05 remote port, P4-06 builder, P4-11 composite view, and P1's `DiscoveryScope::{Normal,BodyRequired(BodySearchSpec)}` contract. **Produces:** `DiscoveryService::discover_scoped(request: DiscoveryRequest, context: ScopedDiscoveryExecution<'_>) -> Result<DiscoveryResult, SearchError>`; `ScopedDiscoveryExecution` carries `content_scope: DiscoveryScope`, visible sources/routing/binding/source scopes/remote port and guarded read view. Legacy `discover(request)` and P1's **renamed** `discover_with_content_scope(request: DiscoveryRequest, scope: DiscoveryScope) -> Result<DiscoveryResult, SearchError>` are thin adapters into the same internal Discovery evaluation loop. The P1 design revision currently calls its method `discover_scoped(request, DiscoveryScope)`; amend that **binding name only** before either service integration task to avoid Rust overload, without changing P1 `BodyRequired` preflight, lexical or qualification semantics. Preselect bounded remote Source actions and seal their batch **before first federation**, choosing either durable or remote generation for a Source in one evaluation. Required execution counts only a completed access-checked action.

- [ ] RED: `remote_discovery_contract::{two_remote_actions_one_key_pass_common_federation_and_claims,required_remote_planned_but_failed_is_not_executed,remote_expansion_after_seal_requires_new_evaluation,local_discovery_and_s1_priority_unchanged}`. Assert qualified Resource and `EvidenceSufficiency`, not just remote hit count.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_discovery_contract`; expected RED.
- [ ] Refactor the existing `discover` setup minimally: select visible registry/routes, run remote batch/access before `evaluate`, supply exactly one pin per Source, read remote records via P4-11. Keep `CandidateFederator::merge(PriorityConcat)`, hard gates, `seen_resources`, `assemble_resource_claims`, local adaptive loop and existing trace behavior as the one shared implementation. An incompatible extra action emits `remote_snapshot_incompatible`/`remote_expansion_requires_new_evaluation` without appending the seal.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test discovery_loop`; expected PASS.

### P4-13 — Scoped entrypoint and revocation before disclosure

**Files:** Create `crates/search-application/src/remote_disclosure.rs`; modify `src/scoped.rs`, `src/discovery_service.rs`, `src/lib.rs`; test `tests/scoped_discovery_contract.rs`.

**Consumes:** P4-02 authority/visibility, P4-07 leases, P4-12 shared execution. **Produces:** `ScopedDiscoveryService::discover(binding: &TrustedDiscoveryBinding, request: DiscoveryRequest, trusted_routing: RoutingConstraints) -> Result<TransientDisclosure<DiscoveryResult>, SearchError>` and `TransientDisclosure::with_disclosure(&mut self, gate: &dyn CurrentDisclosureAccessPort, inspect: impl FnOnce(DisclosureView<'_>) -> Result<(), SearchError>) -> Result<(), SearchError>` as an async method. The `DisclosureView` exposes only allowed qualified IDs, Claim states/evidence roles, sufficiency and generic gap/trace fields through borrowed accessors; no raw `DiscoveryResult`/`Arc`, `Deref`, `AsRef`, `Clone` or `Serialize` escape. `CurrentDisclosureAccessPort::authorize(owner, disclosed_fields) -> BoxFuture<'_, ()>` rechecks actor/Source/item/field before the callback. For all retention modes this is the final output gate; `NO_RETENTION` transfers allowed fields into a separate short disclosure lease and closes the evaluation lease at inner discover return.

- [ ] RED: `scoped_discovery_contract::{foreign_or_expired_handle_stops_before_route_and_network,revoke_during_evidence_read_removes_source,source_visibility_revoked_removes_claim_rank_trace_locator,no_retention_callback_success_error_cancel_closes_both_leases}`. Use a blocked async evidence read to revoke Source/item/field and compare missing vs invisible Required outputs.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test scoped_discovery_contract`; expected RED.
- [ ] Bind request `access_context` and `temporal_context.evaluation_id` to the server-issued handle/evaluation; wrap every `&str` port in the same actor/Source owner gate. On Source revoke, invalidate its whole derived state and recompute from remaining visible Sources; return an ID-free generic Required gap. Close disclosure on callback return/error/cancel; test absence of Graph/probe/receipt/response buffers and forbidden log/audit payload.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test discovery_loop`; expected PASS.

### P4-14 — Remote probe/materialization pin

**Files:** Create `crates/search-application/src/remote_binding.rs`; modify `src/materialization.rs` and `src/ports.rs` only for the checked remote target adapter; test `tests/remote_binding_contract.rs`.

**Consumes:** P4-05 `PinnedRemoteTarget`, P4-07 guarded owner, P4-13 scoped actor, existing `RepresentationBinding::validate` and `MaterializationService::{probe,materialize}`. **Produces:** `RemoteBindingService::bind_live(scope, generation, candidate, target) -> Result<RepresentationBinding, SearchError>` and `revalidate_target(scope, target, requested_state, budget) -> BoxFuture<RemoteReadOutcome>`; stable ID and version/digest required, `LiveReference` uses `RevalidationMarker::Required`, and content is read only from registered endpoint.

- [ ] RED: `remote_binding_contract::{changed_live_digest_requires_new_qualification,missing_version_and_digest_refuses_live_bind,remote_version_pin_requires_version,snapshot_pin_requires_version_or_digest,revoked_policy_or_budget_denies_materialization,provider_locator_is_never_fetch_target}`.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked --test remote_binding_contract`; expected RED.
- [ ] Recheck actor/Source/item/field/current policy and version/digest before remote read and again before returning receipt. Do not append fresh data to a sealed projection or silently rebind old session state.
- [ ] Rerun focused command plus `cargo test -p search-application --locked --test materialization_contract`; expected PASS.

### P4-15 — Isolated HTTP client qualification and selection

**Files:** Create `experiments/search-http-client-poc/{Cargo.toml,Cargo.lock,deny.toml,src/lib.rs,tests/transport.rs,report.md}`; modify `spec/selection/library-tool-selection-v0.md` only after PoC evidence. This manifest has its own `[workspace]`, so no root production Cargo/lock change occurs here.

**Consumes:** P4 freeze transport policy. **Produces:** exact pinned client version/features, official API links, credential-free executable PoC evidence, `cargo deny` transitive license/source/advisory result, and a `SELECTED` or `REJECTED` decision. Start with `reqwest = "=0.13.5"` (async, `default-features = false`, then only PoC-required features), the current official [ClientBuilder API](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html) and [crate features](https://docs.rs/reqwest/0.13.5/reqwest/). Its documented knobs are candidates, not qualification; pinning, TLS/host verification and no hidden retry must be proven on real local TCP before selection.

- [ ] RED: local TCP tests `redirect_never_followed`, `proxy_environment_ignored`, `all_a_aaaa_checked_and_pinned_against_rebind`, `hostname_tls_validation_preserved`, `decoded_stream_limit_and_content_length`, `connect_read_total_deadline_and_cancel`, `no_automatic_retry_or_disk_spool`. Runtime-generated synthetic payloads only; include a resolver that changes answer after preflight and a denied private/metadata address. Record expected failures before the guarded configuration.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --manifest-path experiments/search-http-client-poc/Cargo.toml --locked`; expected RED for missing guard/negative cases, then GREEN after isolated implementation.
- [ ] Run `cargo deny --manifest-path experiments/search-http-client-poc/Cargo.toml --config experiments/search-http-client-poc/deny.toml check`; inspect official license/source and feature tree, note the exact version/API/date and PoC receipt in `report.md`. If the candidate cannot satisfy pinned connection/TLS/retry/decompression requirements, record `REJECTED` and qualify a replacement in the same isolated scope before production promotion; do not silently relax transport policy.
- [ ] Update the selection table with exact decision and qualified features. Expected: no production dependency added before this task's GREEN and selection record.

### P4-16 — Qualified HTTP dependency and fixed-origin transport

**Files:** Create `crates/search-source-http/{Cargo.toml,src/lib.rs,src/transport.rs,tests/transport_policy.rs}`; modify root `Cargo.toml`/`Cargo.lock` and architecture policy/port list only if the qualified client and new adapter require it. `crates/search-application/Cargo.toml` must not gain an HTTP client.

**Consumes:** P4-02 fixed server registration, P4-15 `SELECTED` PoC. **Produces:** `GuardedHttpTransport::new_production(endpoint: RegisteredEndpoint, resolver: Arc<dyn AddressResolver>, limits: TransportLimits) -> Result<Self, SearchError>` and `request(&self, path: RegisteredPath, body: &[u8], deadline: Instant) -> BoxFuture<'_, BoundedResponse>`. Define `AddressResolver`, `TransportLimits`, `RegisteredPath` here. A separate `new_loopback_for_test(...)` is compiled only with the `synthetic-loopback-test-only` Cargo feature; release builds with that feature fail at compile time and production configuration cannot select it. Integration tests explicitly enable the feature. Production constructor enforces HTTPS exact origin/port/path and cannot encode loopback allowance; only registered paths, never provider/caller URLs, reach transport.

- [ ] RED: `transport_policy::{private_and_mapped_ip_rejected,rebinding_and_pool_reuse_rechecked,redirect_and_proxy_denied,caller_or_provider_url_never_fetched,decoded_oversize_and_timeout_close_body,no_automatic_retry}`. Include exact canary transport limits from Global Constraints.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-source-http --locked --features synthetic-loopback-test-only --test transport_policy`; expected RED before adapter implementation.
- [ ] Promote only selected pinned client/features into adapter Cargo and root lock, implement guarded DNS/connect/TLS and bounded response stream (check `Content-Length` plus actual decoded bytes, connect/read/total deadlines, cancellation, no retry/log/spool). Revalidate address allowlist on every connection and pool reuse.
- [ ] Rerun focused command, `cargo deny check`, `cargo test -p architecture-lint --locked` and `cargo fmt --all -- --check`; expected PASS. Record selected dependency/version and exact root-lock diff.

### P4-17 — Provider protocol and RemoteSourcePort adapter

**Files:** Create `crates/search-source-http/src/{protocol,adapter}.rs`, `tests/protocol_contract.rs`; modify `src/lib.rs` for exports only.

**Consumes:** P4-05 `RemoteSourcePort`, P4-06 stable identity/batch contract, P4-10 verified provenance, P4-14 remote target and P4-16 guarded transport. **Produces:** `HttpRemoteSourceAdapter::new(registration: RemoteSourceRegistration, transport: GuardedHttpTransport) -> Result<Self, SearchError>` implementing all `RemoteSourcePort` methods; `decode_response(mode: RemoteOperationKind, bytes: BoundedResponse, registration: &RemoteSourceRegistration) -> Result<RemoteActionResponse, RemoteProtocolError>`. The adapter uses fixed `/v1/catalog?cursor=`, `/v1/search`, `/v1/lookup`, `/v1/live`, `/v1/authorize`, `/v1/content/{encoded-native-id}` paths and treats all provider JSON fields as untrusted. It checks tenant/source/snapshot/cursor/identity/version/digest/field/provenance shape before returning to P4-06/P4-10.

- [ ] RED: `protocol_contract::{all_four_modes_decode_bounded_inputs,partial_page_and_looping_cursor_are_not_complete,malformed_or_truncated_json_and_429_5xx_become_low_cardinality_gaps,provider_tool_text_and_role_origin_labels_never_control_execution,authorize_and_content_require_current_scope_and_version}`. Check depth 32, 100 hits/page, 32 pages/requests, 3,200 hits, four actions, 512-byte ID, 1 KiB cursor and 16 KiB request limits.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-source-http --locked --features synthetic-loopback-test-only --test protocol_contract`; expected RED.
- [ ] Implement the registered path encoder and bounded parser; map timeout/status/invalid body to availability/coverage gaps, never `Absent`. `/authorize` checks current Source/item ACL revision; content checks binding version/digest; do not copy provider role/origin into verified evidence.
- [ ] Rerun focused command; expected PASS.

### P4-18 — Real TCP four-mode qualification

**Files:** Create `crates/search-source-http/tests/support/{mod,synthetic_catalog}.rs`, `tests/scoped_tcp_e2e.rs` (runtime-generated data only; no saved fixture).

**Consumes:** P4-02/03 trusted scope and visible catalog, P4-12/13 common/scoped Discovery, P4-17 adapter. **Produces:** real `127.0.0.1:0` synthetic catalog and four-mode end-to-end receipt through `ScopedDiscoveryService` → `DiscoveryService::discover_scoped` → `SourceRouter`/`RetrieverPlanner`/`RetrievalExecutor` → sealed generation → `CandidateFederator`/hard gates → qualified Resource, Claim/evidence and sufficiency. Two tenants, stable native ID, version/digest, ACL revision, canonical upstream, transient body; `/authorize` is current.

- [ ] RED: `scoped_tcp_e2e::{enumeration_qualifies_and_only_terminal_sweep_proves_absence,query_qualifies_but_query_miss_stays_unknown,direct_lookup_qualifies_but_unmasked_absence_needs_grant,live_qualifies_and_changed_content_requires_new_evaluation,two_actions_share_one_snapshot_and_key}`. Verify TCP request count, sealed generation key and final `qualified_resources`, `evidence_set`, `evidence_sufficiency`; an HTTP 200 alone fails acceptance.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-source-http --locked --features synthetic-loopback-test-only --test scoped_tcp_e2e`; expected RED from missing integration and then GREEN. Assert an unrelated local Source and S1 result survive one remote outage.
- [ ] Record exact HEAD, case count, request count and duration. No synthetic fixture or credentials are persisted.

### P4-19 — Real TCP revocation and retention canary

**Files:** Create `crates/search-source-http/tests/retention_tcp_e2e.rs`; reuse P4-18 test support read-only.

**Consumes:** P4-07/08/09 owner stores, P4-13 disclosure, P4-18 synthetic catalog. **Produces:** real TCP canary for Source/item/field ACL revocation and all five retention modes with actual response body lifetime.

- [ ] RED: `retention_tcp_e2e::{revoked_acl_during_response_leaks_no_source_fields,all_five_modes_store_only_allowed_projection,no_retention_success_error_cancel_disconnect_deadline_leave_no_derived_bytes,held_handle_and_response_buffer_fail_after_close,cache_expiry_and_session_close_force_fresh_provider_read}`. Instrument cache/session/Graph/probe/receipt/materialization/log/audit/telemetry sinks; assert no forbidden payload or fixture remains. Run harness with core dumps disabled and body debug logging off.
- [ ] Run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-source-http --locked --features synthetic-loopback-test-only --test retention_tcp_e2e`; expected RED and then GREEN. A test-only sink can inspect lease-scoped allowed fields, but no raw `DiscoveryResult` or buffer survives the callback.
- [ ] Record exact HEAD, case count, closed-handle assertions and request counts for fresh rereads.

### P4-20 — Independent review, final gates and receipt

**Files:** Add the P4 implementation receipt under `docs/superpowers/programs/search-platform-completion/` at execution time; update the active Capability Execution Status and Draft PR only through the controller/parent. No production code in the reviewer task.

**Consumes:** P4-01 through P4-19 and their RED/GREEN receipts. **Produces:** independent read-only code review, focused regression evidence, exact-head hosted CI/Sandbox/PoC evidence and a state report separating implemented, measured, hosted and live. Reviewer checks each Review Focus counterexample, all five retention modes, real TCP service path, transport decision, Source leakage, dependency direction and no `NO_RETENTION` payload stores.

- [ ] Run focused crates/tests once after material change; run `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p search-application --locked`, `cargo test -p search-source-http --locked --features synthetic-loopback-test-only`, `cargo fmt --all -- --check`, `cargo clippy -p search-application -p search-source-http --all-targets --locked --features synthetic-loopback-test-only -- -D warnings`, `mise run arch:check`, `cargo deny check`, then the relevant repository `mise run verify:fast` gate once. Record exact outputs and HEAD; do not keep replaying full CI for each small task.
- [ ] Have a different read-only reviewer inspect the final diff and actual RED/GREEN evidence; fix actionable findings and rerun only affected tests plus the final gate if code changed.
- [ ] Create/update the Draft implementation PR, attach it to the task, and check its exact-head hosted gates once. Record any missing external credentials/production identity resolver and deploy as **unverified**, not as P4 synthetic failure or live delivery.

## Self-review / handoff

- [x] Coverage: every frozen design §2–§8 requirement is owned above; especially one key, hidden Source normalization, ID-less gap, verified origin, owner expiry, all four real TCP modes, and SSRF/rebinding.
- [x] Interfaces: `RemoteSourcePort` (P4-05) feeds builder (P4-06); lease (P4-07) gates read view (P4-11); planner (P4-04) and visible catalog (P4-03) feed the one `DiscoveryService` path (P4-12); scoped binding/disclosure (P4-13) wraps that path; transport (P4-16) consumes qualified selection (P4-15), adapter (P4-17) consumes transport, real TCP canaries (P4-18/19) consume all.
- [x] Parallel work may use disjoint newly created module files after their contracts exist. `routing.rs`, `retrieval.rs`, `ports.rs`, `discovery_service.rs`, `lib.rs`, `session.rs`, `projection.rs`, root Cargo/lock, architecture policy and selection record are serialization points; the controller derives artifact edges and schedules them, not this document.
