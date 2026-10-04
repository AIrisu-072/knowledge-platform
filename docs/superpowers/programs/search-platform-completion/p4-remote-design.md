# P4 Remote Source — Design Draft

- Status: **DRAFT / independent review and freeze pending**. This document is a proposed extension of the approved Search / Discovery v0 semantics, not implementation or qualification evidence.
- Scope: provider-neutral remote discovery, observation, current access, retention, and a credential-free synthetic HTTP provider exercised over a real local TCP connection through `DiscoveryService`. No live provider, credentials, customer data, production deployment, or merge is part of P4.
- Baseline: `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md` §§5–6, 18, 26, 31–32; `spec/data/transaction-consistency-requirements-v0.md` SD-T2/3/7; `spec/data/data-characteristics-v0.md` Search amendment; `spec/operations/observability-audit-requirements-v0.md` Search audit privacy. This draft preserves Source-owned truth, S1 priority concatenation, current authorization, and source-local identity.

## 1. Existing boundary and decision

`search-core::source::{DiscoveryMode, EnumerationSemantics, RetentionMode}` already names all four remote modes and four coverage classes. `SourceRouter::plan` currently marks remote-only routes `RuntimePortUnavailable`; `RetrieverPlanner` creates unsupported remote actions; `RetrievalExecutor` rejects them; `DiscoveryService` pins a **persistent** projection generation before any action. `PersistableGenerationManifest` and `PersistableResourceProjection` reject `CACHE_WITH_EXPIRY`, `SESSION_ONLY`, and `NO_RETENTION`. Merely adding an HTTP call inside an existing local retriever would either claim a generation that does not exist or bypass retention and current-access checks.

**Decision:** add a `RemoteSourcePort` for the four modes, a per-evaluation `RemoteSnapshotSet` for immutable remote action responses, and a read-only composite view for existing qualification/evidence ports. Keep `ProjectionGenerationStore` and its atomic publication contract for persistent generations. An action result has its own opaque `ProjectionGenerationKey` used **only as an evaluation-local read key**; its manifest records the response snapshot and coverage, and it is never passed to `PersistableGenerationManifest`, durable index, or publication. A required remote route is planned only when its configured adapter and trusted inputs exist. Failures become bounded `InformationGap` entries so independent Sources can continue; structural identity/integrity violations fail the action closed.

This is not a parallel remote Search engine: remote hits enter the same current-access filter, hard gates, S1 `CandidateFederator::merge(PriorityConcat)`, qualification, evidence resolution, and final access recheck as local hits. Provider score is retained only as source-local diagnostic input and never compared across Sources.

Considered paths: (a) mirror every provider into a persistent local generation, (b) call providers directly and return their ranked responses, (c) route provider calls into evaluation-scoped snapshots and use the current core. (a) cannot serve `NO_RETENTION`/`LIVE_ONLY` and makes incomplete enumeration look authoritative; (b) bypasses core access/evidence rules. Choose (c), with an optional persistent collection path **only** when the retention and complete-snapshot contract independently permits it.

## 2. Registration, modes, and coverage

The registry stores a server-owned `RemoteSourceRegistration { tenant_id, source_id, provider_kind, endpoint_ref, supported_modes, enumeration_semantics, authority_scope, allowed_resource_kinds, access_contract, retention_contract, content_permission, freshness_contract, transport_limits }`. `endpoint_ref` resolves to an operator-configured fixed origin, never a URL from a request or provider hit. The existing `DiscoverableSource` is its public routing projection; a request-scoped `SourceRegistryPort` wrapper lists only entries for the trusted tenant. Duplicate `(tenant_id, source_id)` is a configuration error, not an arbitrary winner. A provider response cannot rewrite tenant, Source ID, mode, retention, authority scope, or endpoint.

| Mode | Operation and input | Permitted coverage | Negative result |
| --- | --- | --- | --- |
| `REMOTE_ENUMERATION` | Cursor pages within a declared tenant/source/principal scope | `COMPLETE` only after every page shares one snapshot and terminal cursor is reached; otherwise `PARTIAL` | `ABSENT` only for a known ID within a verified complete scope/snapshot; incomplete pages remain `UNKNOWN` |
| `REMOTE_QUERY` | Typed query text/filter and bounded window from trusted planner | `QUERY_ONLY` or `PARTIAL`; even a provider's “all query hits” is complete **for that query**, not the Source | Empty result is `UNKNOWN`, never deletion or Source knowledge absence |
| `DIRECT_ADDRESS` | Opaque source-native ID resolved under fixed provider origin | `NONE`, `PARTIAL`, or `COMPLETE` | Only a provider contract with authoritative, ACL-unmasked lookup may attest `ABSENT` for that exact ID after source-scope authorization; otherwise `UNKNOWN` |
| `LIVE_ONLY` | Query/direct read with no reusable collection | `NONE` or `QUERY_ONLY` | Miss is `UNKNOWN`; every access/materialization uses a fresh provider call |

`COMPLETE`, `PARTIAL`, `QUERY_ONLY`, `NONE` describe the registered Source's enumeration guarantee, not a rank or trust score. A Source may advertise multiple modes, but mode-specific observations retain their own coverage. `COMPLETE` never follows from an HTTP 200, a page count estimate, a search total, or an empty response. A failed page, changed snapshot token, repeated cursor, rate limit, timeout, or scope/ACL change downgrades the batch to incomplete and yields an availability/coverage gap. Do not retire Resource IDs or publish a deletion from it. Outage changes Source availability, not all Resource states.

`ResourceObservation` stays separate from Resource lifecycle. The adapter is the only producer of an opaque `VerifiedAbsence` receipt containing `(tenant, source, authorized_scope, snapshot_or_lookup_token, exact_resource_id, coverage_kind, observed_at)`. A complete enumeration receipt requires all pages and a stable source snapshot; a direct lookup receipt requires the explicit ACL-unmasked absence capability. The application can construct `Presence::Absent` only from that receipt. `QueryResult`, `PartialEnumeration`, ordinary HTTP 404/403, and probe `NotFoundByProbe` produce `Presence::Unknown`. Same provider version with a different digest is an `IntegrityConflict` and never silently replaces a binding.

## 3. Provider-neutral application contracts

Proposed interfaces; names and signatures are intended implementation targets, not claims that these types already exist:

```rust
struct TenantSourceKey { tenant_id: TenantId, source_id: SourceId }
struct TrustedRemoteContext {
    tenant_source: TenantSourceKey,
    principal_ref: PrincipalRef,
    access_context_binding: AccessContextBinding,
    evaluation_id: DiscoveryEvaluationId,
    deadline: Instant,
}
enum RemoteOperation {
    Enumerate { cursor: Option<OpaqueCursor> },
    Query { query: RemoteQueryInput, limit: NonZeroUsize },
    Lookup { native_id: OpaqueNativeId },
    Live { input: LiveInput },
}
trait RemoteSourcePort: Send + Sync {
    fn execute<'a>(&'a self, ctx: &'a TrustedRemoteContext,
                   op: &'a RemoteOperation) -> BoxFuture<'a, RemoteActionOutcome>;
    fn current_access<'a>(&'a self, ctx: &'a TrustedRemoteContext,
                          id: &'a RemoteIdentity) -> BoxFuture<'a, AccessDecision>;
    fn current_policy<'a>(&'a self, ctx: &'a TrustedRemoteContext,
                          id: &'a RemoteIdentity) -> BoxFuture<'a, CurrentSourcePolicy>;
    fn materialize<'a>(&'a self, ctx: &'a TrustedRemoteContext,
                       request: &'a MaterializationRequest)
                       -> BoxFuture<'a, MaterializationReceipt>;
}
enum RemoteActionOutcome {
    Snapshot(RemoteResponseSnapshot),
    Unavailable(RemoteFailureCode),
    Unsupported(RemoteFailureCode),
}
struct RemoteResponseSnapshot {
    key: ProjectionGenerationKey,
    source_snapshot: OpaqueSnapshotToken,
    coverage: Coverage,
    hits: Vec<RemoteHitBundle>,
    absence: Vec<VerifiedAbsence>,
    observed_at: OffsetDateTime,
}
struct RemoteHitBundle {
    candidate: FederatedCandidate,
    projection: Option<CompiledResourceProjection>,
    assertions: Vec<Assertion>,
    evidence: Vec<ResolvedAssertionEvidence>,
}
```

`TenantId`, `PrincipalRef`, and bindings are internal trusted types, not strings parsed from `DiscoveryRequest.access_context` or HTTP headers. P5's authenticated transport will construct them; P4's local harness constructs them in process. `RemoteQueryInput` is a typed allowlisted text/facet query, `OpaqueNativeId` is a bounded UTF-8 identifier with no URL syntax, and `OpaqueCursor` is never accepted from an external Search client. All four operations are dispatched by registry mode and adapter capability, with explicit `RetrieverSupport` bits and required inputs in `RetrievalInputs`; no provider URL, raw credential, or arbitrary HTTP method enters the planner.

`RemoteResponseSnapshot` is staged atomically into `RemoteSnapshotSet` **after** Source/tenant/snapshot/identity validation and **before** any hit is visible. The set owns in-memory projection, assertion, claim-selector, concept-view, and evidence lookup keyed by the action's `ProjectionGenerationKey`; earlier action snapshots are immutable, and its entire content is dropped on evaluation/session expiry. Each ephemeral generation ID is fresh for the evaluation, checked against every active durable and ephemeral key, and a collision fails closed. `DiscoveryService` records the per-hit key rather than looking up one `PinnedSource` key per Source; local hits continue using the existing pinned durable generation. The composite read adapters dispatch through an explicit durable/ephemeral key registry, never by guessing from the UUID; `ProjectionGenerationStore` receives no ephemeral write. A later remote call makes a new snapshot key and cannot mutate earlier evidence. Required Source execution counts only a completed, access-checked action, not a planned action or transport error.

`RemoteHitBundle.candidate.source_ref` must equal the registered Source; resource and candidate IDs are derived from length-framed `(tenant_id, source_id, provider_kind, source_native_id)` with a versioned SHA-256 namespace. A provider without stable IDs gets `EphemeralCandidate` with an evaluation-scoped ID and cannot create a durable `RepresentationBinding`. Stable IDs use `RemoteStableReference`; a live reference binds only with `BindingMode::LiveReference` and `RevalidationMarker::Required`. Provider locators are opaque display data, not fetch destinations. Resource type, title, facets, claims, and relations are translated with a declared `DiscoveryLens`; no provider-supplied assertion becomes `Authoritative` unless the registration grants that predicate/authority scope and evidence provenance. Unknown values remain `UNKNOWN`, not false. Only explicit typed n-ary relations with validated participants enter an allowed session Graph.

The existing `CurrentCandidateAccessEvaluatorPort`, `CurrentSourcePolicyPort`, `ProbePort`, and `MaterializerPort` remain the core boundary. The remote adapter checks source-scope access **before network discovery**, item access after each returned hit and again before detail/probe/materialization; the existing Discovery final pass rechecks before any candidate, claim, rank, trace, or locator leaves. `Denied`, `Unknown`, and access errors all suppress hit identity. A source with no authoritative current item-access operation is limited to non-sensitive public data with an explicit public access contract; otherwise it cannot qualify candidates. A provider's own filtering is insufficient as the sole check. A policy/tenant/snapshot mismatch invalidates the response. Revocation during a slow evidence read removes the entire hit and derived evidence.

## 4. Retention and materialization matrix

Retention applies to candidate metadata, assertions, relations, content, query/result trace, cache, index, audit, telemetry, and test artifacts. Runtime materialization is separately limited by `ProviderContentPermission`, `MaterializationBudget`, current access, stage, and response byte limit; a permitted `NO_RETENTION` full read is transient and is not permission to persist it.

| `RetentionMode` | Allowed server-side state | Enforcement |
| --- | --- | --- |
| `PERSISTENT_RESOURCE` | Source-permitted resource/metadata/body projection and durable generation | Field allowlist and Source grant checked at ingestion and publish; current access checked again when read; no credentials or raw provider response in index |
| `PERSISTENT_DISCOVERY_METADATA` | Only explicitly allowed identity/title/facet metadata and provenance; no body/fragments | A typed metadata-only projection constructor rejects body-bearing assertions, evidence text, content and disallowed relations before `PersistableResourceProjection`; field-level contract controls sensitive titles/IDs |
| `CACHE_WITH_EXPIRY` | Bounded in-memory cache keyed by tenant, Source, principal/access revision and normalized operation, with provider TTL ceiling | Expiry checked on every read, eviction on revocation, no reuse across principals; **never** passed to persistent generation conversion; separate expiry-aware store required before any durable cache is considered |
| `SESSION_ONLY` | Session-scoped RAM working index/Graph and materialization bytes | No cross-session reuse; bound lifetime/idle TTL, drop on close/expiry; no serialization, disk spill, fixture snapshot, or durable projection |
| `NO_RETENTION` | Only evaluation-call RAM needed for authorized result and optional transient full content | No persistent index, audit, telemetry payload, cache, session store record, fixture, Graph, trace, dump, temp file, or raw HTTP logging; no provider-derived per-call telemetry. Generate synthetic test data at runtime and discard it with the evaluation |

The existing `MaterializationReceipt::to_session_store_record` already denies durable conversion for the last three modes; keep that gate. `PersistableGenerationManifest`/`PersistableResourceProjection` continue to reject them. For `PERSISTENT_DISCOVERY_METADATA`, their current coarse allow decision is insufficient: add a field-level proof type and tests before any remote metadata write. Cache and session stores are separate adapters with no `Persistable*` conversion. `NO_RETENTION` may return a transient result to the authorized caller, but the server does not retain its candidate IDs, body, query, locator, assertion, or response bytes. Disable debug/request body traces and crash dumps in the PoC process; aggregate fixed operational counters may be measured only from non-content-bearing local test runs and are not populated from `NO_RETENTION` traffic.

## 5. Concrete credential-free HTTP provider and transport

Use a **local synthetic knowledge catalog** (`127.0.0.1` on an OS-assigned port) whose resources are generated in process from deterministic seeds. It models a multi-tenant authoritative catalog with current ACL revision, stable native IDs, version/digest, policy and knowledge resource kinds, and an optional transient body. It is credential-free and contains no customer text. The same provider-neutral adapter is exercised through actual HTTP request/response bytes:

| Endpoint | Contract |
| --- | --- |
| `GET /v1/catalog?cursor=` | Fixed-scope pages `{snapshot, items, next_cursor, terminal}`; enumerate only under registered tenant/principal. Test changed snapshot, duplicate cursor, missing page, partial results |
| `POST /v1/search` | `{query, limit}` to `{snapshot, hits}`; empty is QueryResult/UNKNOWN; no arbitrary URL or provider score authority |
| `POST /v1/lookup` | `{native_id}` to present, denied, or contract-qualified absent; lookup ID is not a URL |
| `POST /v1/live` | Current query/lookup result with no cacheable collection state |
| `POST /v1/authorize` | Source/item current access and ACL revision for the trusted local principal; a later revision can revoke a previously returned hit |
| `GET /v1/content/{encoded-native-id}` | Bounded representation read after current policy/access and binding revalidation; response version/digest checked against binding |

These paths define only the synthetic provider protocol. The application `RemoteSourcePort` remains transport-neutral; a later MCP or other provider implements the same operations and must qualify its own coverage/access/retention semantics. P4 does not select a live endpoint or require credentials.

Transport policy is server-owned and deny-by-default. Production registrations require HTTPS, exact allowed origin/port/path prefix, no userinfo, no URL supplied by caller/provider, no automatic redirects, no ambient proxy, and no cross-origin fetch. At each connection, resolve every A/AAAA address, reject loopback/private/link-local/multicast/unspecified/metadata/reserved ranges (including IPv4-mapped IPv6), and connect only to a validated pinned address while preserving original hostname for TLS/SNI and certificate validation. Re-resolution or pool reuse must pass the same allowlist; DNS preflight followed by an ordinary resolver is insufficient against rebinding. No redirect may bypass this check. The local PoC has an **explicit test-only loopback origin** injected into its adapter instance; this exception is unavailable in production configuration. [`reqwest` `ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html) documents `no_proxy`, timeout and DNS override hooks, and its [redirect policy](https://docs.rs/reqwest/latest/reqwest/redirect/struct.Policy.html) supports `none`; these are candidate mechanisms to qualify, not an unqualified production dependency. HTTP `Location` can designate a different target under [RFC 9110 §15.4](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.4), hence explicit redirect denial.

Set finite per-call connect, read, and total deadlines, plus an evaluation deadline and cancellation. Initial local PoC limits: at most 2 seconds per call, 5 seconds per evaluation, 16 KiB request, 1 MiB decoded response, 100 hits/page, 32 pages/HTTP requests total, 3,200 total hits, 4 remote actions, 512-byte native IDs, 1 KiB cursors, and JSON depth 32. These are testable starting caps, not measured production SLOs. Reject too-large `Content-Length` early and count actual decoded streamed bytes because length may be absent or compression may expand. Disable automatic decompression or enforce the post-decompression cap. Bound all other strings/arrays, concurrent calls, and total memory. No automatic retry in the initial adapter; timeout, 429, 5xx, malformed response, and truncated body map to low-cardinality failure codes and a required/optional Source gap, never `ABSENT`. Cancellation closes the body without writing it to disk. Source text, metadata, and tool-like strings remain **untrusted data**: no instruction execution, tool dispatch, URL fetch, or authority promotion from their wording.

## 6. Execution, failure, and qualification

Implementation sequence and reviewable outputs:

1. **Contract tests and normative amendment:** add a focused `spec/` Search amendment for remote outcomes, verified absence, all five retention paths, tenant/source scope, and session snapshots. Add failing tests for query miss, partial/complete enumeration, direct lookup, `LIVE_ONLY`, cross-tenant identity, and retention conversions. This draft itself does not change normative files.
2. **Application integration:** extend `SourceRouter`, `RetrieverSupport`, `RetrievalInputs`, `RetrieverPlanner`, `RetrievalExecutor`, and `DiscoveryService` with remote actions and `RemoteSnapshotSet`. Keep local generation pinning and S1 order unchanged. A remote action stages a validated immutable snapshot, then shared current-access filtering and qualification run. On transport failure, other routed Sources continue; a required Source gets a blocking Availability gap. No action is called executed merely because it was planned.
3. **Persistence guard:** add metadata-only proof and expiry/session stores as above; assert `NO_RETENTION` cannot reach `ProjectionGenerationStore`, Tantivy, durable Graph, Audit, Telemetry payload, serialized working set, or test fixtures. Include crash/timeout cleanup and retention-change invalidation.
4. **Synthetic provider adapter:** create a small `search-source-http` crate with the provider-neutral adapter implementation and a local TCP HTTP server in integration-test support. Qualify any proposed HTTP library via `spec/selection/library-tool-selection-v0.md` and PoC before adding it to production dependencies or `Cargo.lock`. Verify pinned DNS/address, redirect denial, TLS configuration path, proxy denial, decoded byte cap, deadlines, and no raw logging. The local provider is never a production service.
5. **Real E2E canary:** trusted tenant/principal → Source registry → remote route/action → real loopback HTTP provider → `RemoteSnapshotSet` → `DiscoveryService::discover` → current access/evidence/qualification. Test all four modes and four coverage classes; for `NO_RETENTION` test a transient full read plus zero retained state after return. Mutate ACL between retrieval and final evidence pass; verify no candidate, locator, trace, rank, or claim leaks. Verify provider outage preserves prior durable Source state and independent local Source results.
6. **Independent review and qualification:** read-only security/correctness review of Source scope, ACL races, SSRF/DNS/rebinding, untrusted content, absence, retention, and S1. Run focused contract/E2E tests, strict Clippy/fmt and relevant repo gates, then exact-head hosted checks for the implementation Draft PR. Record outcomes in the program status/receipt; this draft is not a PASS claim.

Acceptance requires a real transport response to become a qualified Search result through core; a 200-only PoC, an in-memory fake retriever, or a schema-only test does not close P4. The acceptance matrix must include: query miss never absent; complete enumeration absent only after terminal same-snapshot sweep; partial failure never delete; direct 404 with hidden ACL unknown; current revocation redacts every output surface; Source A/tenant A cannot inject Source B/tenant B; timeout/oversize/redirect/rebinding fail closed; each retention mode uses only its permitted store; `NO_RETENTION` yields no persistent index/audit/telemetry/fixture artifact; materialization obeys provider permission and budget; equal provider version/different digest conflicts. Live external credentials, a production endpoint, or a material business/security tradeoff would invoke the program's Hard Stop rather than being inferred from this local qualification.
