# P3-G01 issuer refinement

## Cross-crate private constructor

`RegisteredFullBuildHandle`, `BuildGuardHandle`, and `GraphReadLease` live in `search-application::graph_generation` with private fields and no public raw-field constructor. An application-owned `GraphRegistrationIssuer<P>` is the only code path that constructs full or incremental handles. `P: TrustedGraphRegistrationHostPort` has associated opaque registration types; these may be P7 runtime's opaque full and incremental build handles without a reverse dependency from application to runtime. The host port's async `committed_full(&P::FullRegistration)` returns a typed `CommittedFullGraphTarget` receipt derived from committed P7/Graph registration rows. It does not accept a request-supplied mapping digest, parent state, or a bare global token. The incremental method analogously resolves base, target, and guard. `GraphRegistrationIssuer::issue_full(&P::FullRegistration)` checks the Source-scoped key, retention eligibility, nonempty snapshot/manifest/schema digests, and nonzero activation/fence before constructing the opaque handle. Only the host port is implemented at the trusted composition root. An arbitrary Rust implementation of that trait is code-level host trust, not a user-data grant.

`CommittedFullGraphTarget` is a transport value between the trusted host adapter and issuer, not a READY certificate. G03/P7-07 must re-read the P7 target, Graph parent, full guard, database expiry, token/fence, activation, and role on every child batch. P7-08 alone validates both physical bundles and commits READY on one connection. The handle carries Source/key/snapshot/activation/token/fence identity for matching; it has no `is_ready` or `authorized` flag. `stage_full_registered(handle, resources, relations)` has no manifest, mapping-digest, or caller-selected-retention argument. Staging cannot insert a parent. The receipt's mapping digest is read or recomputed from the Source-owned physical mapping at validation/recovery, not asserted by the builder. `GraphSourceMappingValidatorPort::validate_authoritative(manifest, records)` returns a typed Source-owned mapping receipt and takes no `expected_digest` from an untrusted caller.

`GraphReadLease` is emitted through an analogous `GraphReadLeaseIssuer<P>` where `P::Pinned` is P7's opaque pinned-bundle type. The lease struct has private fields; its issuer obtains the key/evaluation/lease tuple from the trusted P7 host port. G07 must still verify the saved row and database-clock expiry before reading and before return. The tuple itself is not authority.

## Proposed shape

```rust
pub trait TrustedGraphRegistrationHostPort: Send + Sync {
    type FullRegistration: Send + Sync;
    type IncrementalRegistration: Send + Sync;
    fn committed_full<'a>(&'a self, registration: &'a Self::FullRegistration)
        -> BoxFuture<'a, CommittedFullGraphTarget>;
    fn committed_incremental<'a>(&'a self, registration: &'a Self::IncrementalRegistration)
        -> BoxFuture<'a, CommittedIncrementalGraphTarget>;
}

pub struct GraphRegistrationIssuer<P> { host: P }
impl<P: TrustedGraphRegistrationHostPort> GraphRegistrationIssuer<P> {
    pub async fn issue_full(&self, registration: &P::FullRegistration)
        -> Result<RegisteredFullBuildHandle, SearchError>;
    pub async fn issue_incremental(&self, registration: &P::IncrementalRegistration)
        -> Result<BuildGuardHandle, SearchError>;
}

pub trait DurableGraphGenerationPort: Send + Sync {
    fn stage_full_registered<'a>(&'a self, handle: &'a RegisteredFullBuildHandle,
        resources: &'a [GraphResourceRecord], relations: &'a [TypedRelationInstance])
        -> BoxFuture<'a, GraphStage>;
    // validate_staged returns a non-READY report. P7-08 performs the final
    // same-connection Graph/P7 READY validation and commit. recover_ready reads
    // an existing READY row but cannot create or promote one.
    // Incremental staging consumes the registered target and frozen guard.
}
```

## TDD implications

The initial named test uses a synthetic trusted host port returning a committed receipt with `SessionOnly`/`NoRetention`, and another with `key.source_id != receipt.source_id`; the issuer rejects each before any storage call. A valid persistent receipt returns an opaque handle whose key matches the scoped key. A pure Rust fake implementation of the production port type-checks without SQLx. Source-text or Cargo-absence checks are supplementary evidence, not a substitute for compile. The first Cargo run must show unresolved Graph modules/types as RED after P4 releases the application/Cargo slot.

Do not run tests from this proposal yet. It is a design refinement only: P7 registration, database roles, READY, current access, and production Graph storage are not established here.
