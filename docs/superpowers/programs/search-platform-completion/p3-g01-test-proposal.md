# P3-G01 focused TDD proposal

Target module: `crates/search-application/tests/graph_generation_port.rs`, after the App/Cargo slot is released and the issuer interface review passes.

1. `graph_generation_port_rejects_ephemeral_retention_and_unscoped_key`
   - A synthetic `TrustedGraphRegistrationHostPort` has an associated opaque registration marker and returns a committed-row DTO. This is a host-level fixture, not a provider/request-derived registration.
   - For `SessionOnly`, `NoRetention`, and `CacheWithExpiry`, `GraphRegistrationIssuer::issue_full` returns `SearchError::InvalidRequest` before any storage-port call.
   - Reject when the DTO `source_id` differs from `ProjectionGenerationKey.source_id`.
   - Reject a nil generation UUID, empty snapshot/schema/manifest digest, zero activation/fence, or nil guard token. Exact checks must match the reviewed interface and P7 target schema.
   - A valid persistent DTO yields a private-field `RegisteredFullBuildHandle`; its scoped key and snapshot accessor round-trip. The test makes no READY assertion.
2. `ports_have_no_sqlx_types`
   - Implement every G01 production trait with a pure in-test adapter using only `BoxFuture`, `SearchError`, Core IDs/records, and `std`/`uuid`. The adapter returns `OperationFailed` to prove the type boundary without storage.
   - Assert staging accepts `RegisteredFullBuildHandle` plus records/relations and has no autonomous manifest or mapping-digest argument.
   - The package already has no SQLx dependency; the locked App test is the compile gate.
3. Add targeted tests only if implementation reveals a material risk: empty Source-owned native mapping, mismatched `TemporalProjection.resource_ref`, relation participant validation without flattening n-ary relations, or invalid relation closure proof.

Required first run after review and exclusive slot:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-application --locked --test graph_generation_port -- --test-threads=1
```

Do not run this draft before P4 releases App/Cargo. This is a proposed test contract, not evidence of implementation or qualification.
