# API contract

`spec/api/openapi.yaml` is the transport contract source of truth. Endpoint and schema changes are contract-first: update the OpenAPI/JSON Schema contract before implementation.

OpenAPI 3.2-only features must be covered by tooling compatibility tests before production use. Code generation compatibility is handled by its separate PoC plan.
