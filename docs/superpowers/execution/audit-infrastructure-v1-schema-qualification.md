# Audit Infrastructure v1 — A2 schema qualification

Status: INDEPENDENT CODE RE-REVIEW GO at `dd17021ec0938fe77430769d3ee5a2c1d2b6d97a`; exact-head hosted qualification is pending. Backend/source SQL admission/delivery/access/retention/recovery remain unimplemented. This is not whole-track qualification.

## Scope

A-AUD1 adds pure `audit-core`, existing-taxonomy catalog and generated JSON Schema2020-12, synthetic conformance vectors and additive contract CI. It does not alter Document producers, existing migrations, generic Search outbox, PoC acceptance code or Organization semantics.

The20 existing types have13 schema-eligible variants and7 explicitly deferred reason-bearing types.67 synthetic vectors cover every catalog type, unknown-field/result/source rejection and signed64 counter boundaries. Actual Diff digest arrays and legacy timestamp tuples are preserved. The bounded compatibility addendum is independently approved at blob `c932988f7d9aadd8d1d77271fe184a9ca250026a`;512/513 source-admission qualification is still a future real-DB test, not a result claimed here.

The selected Rust jsonschema0.55.0 engine validates structure with required UTF-8/wire/calendar keywords. Runtime semantic validation additionally binds existing action/result/source/resource, metadata IDs, service executor and correlations. Actor kind stays unknown for un-attested legacy provenance. Free-text reason cannot become a lossy delivered projection. `audit-json-v1` canonicalization preserves integer/array values and sorts object keys.

## Test-first receipts

- Initial Node RED: schema artifact absent. Initial Rust RED: E0432 for missing AuditEnvelope/LegacyAuditRow/ValidationError/canonical functions. Test/scaffold commit `2ead72be` preceded implementation.
- Schema engine API RED: missing exported `schema_validate`; then existing selected jsonschema0.55.0 engine and keywords implemented.
- Generator RED: missing `renderSchema`; then reproducibility, duplicate taxonomy and unknown-kind checks GREEN4/4.
- Attribution repair RED: legacy provenance could assert `human_interactive`, and subject could identify a different resource. Two failing tests were observed before restricting legacy kind and binding subject to resource. Audit-core suite15/15 and strict Clippy passed afterward.
-67 conformance vectors include15 expected qualified and52 rejected/deferred variants. They are synthetic contract evidence, not integration with a real source database or endpoint.
- Correlation RED: invented request/operation IDs and simultaneous trace/legacy labels were accepted; cancellation had mislabeled the Publish intent reference. Both regressions were observed, then fixed with exact metadata-to-correlation binding and a distinct `publish_operation_id`. Final audit-core17/17 tests (including67 conformance vectors), strict Clippy, workspace fmt and Node4/4 generator tests passed with the yoke0.8.4 baseline hygiene fix. Independent code review and exact-head hosted gates remain pending.

## Independent code review1 and test-first repairs

Reviewer inspected `c177ca0c3dcc3f5fdbf9265a3e5af0392f12a2ef` and independently reran17 tests. Verdict: NO-GO for two Important/P2 findings: ACL target metadata could contradict resource evidence (including unsupported AccessPolicy resource), and last-wins duplicate raw JSON members could silently discard required/private evidence before canonicalization. A3 remains gated on independent re-review GO.

- ACL RED covered nine legacy/envelope target-ID/type/unsupported-resource cases, all incorrectly accepted. Source `PolicyTarget` permits Document/Folder only; `access_policy.rs` emits target metadata from the same resource. GREEN restricts the catalog accordingly and requires exact target/resource equality.
- Duplicate RED covered first/last conflicting values, identical duplicates and escaped decoded-key aliases in envelope metadata and nested legacy data. GREEN uses a recursive unique-key visitor before any map conversion; all errors remain fixed and redacted. LegacyAuditRow no longer exposes a derived Deserialize path bypassing its bounded raw constructor.
- An additional source-backed edge had explicit RED: `UtcOffset::from_hms` normalizes mixed-sign components. Validation now requires an exact offset-component round trip; valid UTC/positive/negative tuples remain unchanged. This tightens malformed input admission without changing serialized producer evidence.
- Fresh local suite21/21 (including67 existing conformance vectors), strict Clippy, Node4/4, schema regeneration and workspace fmt pass. Independent re-review GO at `dd17021ec0938fe77430769d3ee5a2c1d2b6d97a` confirmed these repairs, reran21/21 and4/4, and passed seven additional parser edge probes. This is not whole-track or hosted qualification.

## Dependency and verification boundary

External Cargo package `(name,version,source,checksum)` set exactly equals D-AUD1 hygiene head `3ccd86dd4592dfffbc9baca735f3e76b82417dbf`. Relative to initial d71753d4, the sole external package change is the separately reviewed yoke-derive0.8.3→0.8.4 version/checksum fix. The new crate reuses already-selected/locked serde/JSON, UUID, time, SHA256, error and jsonschema libraries; no SDK/service selection is implied.

Completed local checks: workspace `cargo fmt --all --check`, locked offline metadata and actionlint passed. `mise run verify:fast` was attempted and exited127 because mise is absent from this cloud toolchain. Full workspace tests/standard security/aggregate gate have not been claimed. Shared Cargo work is serialized and disk guard preserved; bounded Audit/shared consumer-check target was506MiB after the final checks, with about2.2GiB free and the1.5GiB floor preserved.

D-AUD1 PR44's scanner-only exact31 correction is a separate two-file change. Its hosted corrected scan found zero findings with the unchanged control finding one; local all-ref history contains additional mapped commits and is not represented as hosted parity. Parent publishes/reviews the Draft stack and remote tree mapping separately.

## Required next qualification

The independent code review gate is GO with no remaining Critical/Important finding. Publish the reviewed Draft schema layer and obtain exact-head hosted CI; resolve any new failures with scoped RED→GREEN. Only after A-AUD1 qualification may A3 claim source/store/delivery behavior; no Rust schema test substitutes for PostgreSQL pre-materialization, atomic receipt, commit watermark, privilege, lease, retention, restore or failure tests.
