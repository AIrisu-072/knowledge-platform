# Document API codegen qualification — HAPI-01

- Date: 2026-09-29 JST
- Contract: `spec/api/openapi.yaml` at OpenAPI 3.2.1, plus its actual external JSON Schema references.
- Reproduction: `mise exec -- bash experiments/document-api-codegen/qualify.sh`
- Outcome: screening script PASS. **No candidate is promoted to production tooling or dependencies.** Generated files are temporary; the OpenAPI and JSON Schema files remain the only transport contract source.

## Actual-contract fixture

`prepare-fixture.mjs` bundles the current contract with Redocly CLI 2.52.1, then extracts `SetAccessPolicy`, `ComparisonResponse`, both multipart request schemas and the RFC 9457 `Problem`. It retains nested `$ref`, `oneOf`, nullable fields, format and 2020-12 schema structure. The entire OpenAPI document is separately used for `openapi-typescript`. A successful command alone is not qualification; `check-shapes.mjs` inspects emitted types and records loss.

| Candidate | Version / license | Observed result | Decision |
|---|---|---|---|
| `openapi-typescript` | 7.13.0 / MIT | Accepted the 3.2.1 document and produced all 30 operation types. Explicit policy discriminator mapping preserved `inherit` / `explicit`, comparison union and nullable policy ID. Both multipart binary parts became `string` / `string[]`; this does not type a browser `Blob` upload. Its [official README](https://github.com/openapi-ts/openapi-typescript/blob/main/packages/openapi-typescript/README.md) advertises OpenAPI 3.0/3.1 and the maintainer [roadmap](https://github.com/openapi-ts/openapi-typescript/discussions/2559) notes incomplete 3.2 coverage. | POC REQUIRED; no production promotion. |
| `json-schema-to-typescript` | 16.0.0 / MIT | Actual schema fixture compiled to TypeScript with policy discriminant, comparison coverage and nullable fields. Multipart binary parts became `string` / `string[]`. The [upstream feature table](https://github.com/bcherny/json-schema-to-typescript/blob/master/README.md) documents 2020-12 gaps and that `oneOf` exclusive semantics are not expressible by its emitted TypeScript. | POC REQUIRED; no production promotion. |
| `typify` | 0.7.0 / Apache-2.0 | `cargo check` against the actual-contract fixture failed: generated `FieldError` was defined twice / recursively, and `OperationId` conversion impls conflicted. Missing generated helper crates were also reported. The [official crate documentation](https://docs.rs/typify/0.7.0/typify/) describes JSON Schema type generation, but this fixture does not compile. | REJECTED for this contract; no production promotion. |

The two TypeScript outputs passed `tsc --noEmit --strict`; that does not repair binary typing or prove all OpenAPI 3.2 fields. The experiment did not run a production security gate because no candidate met the shape gate. Root `Cargo.lock` and `pnpm-lock.yaml` remain untouched by these candidates. HAPI-03 uses hand-written thin Rust DTOs plus the already selected `jsonschema` runtime structural validator. A future typed client must requalify its generator or use a verified explicit binary wrapper before production use; the contract stays at 3.2.1.

Redocly recommended lint's `security-defined` rule is disabled in `redocly.yaml` because declaring bearer, cookie, mTLS or custom-header authentication before selecting the trusted Identity Adapter would falsely define an identity mechanism. The contract test instead checks that request bodies contain no actor self-assertion fields. The `info-license` rule is disabled because the repository has no declared API license.
