# API contract

Documentのtransport契約は [`spec/api/openapi.yaml`](openapi.yaml)、Searchの4操作は [`spec/api/search-openapi.yaml`](search-openapi.yaml) をそれぞれ正本とする。両方を既存の `api:lint` と `api:contract` で検査する。endpoint/schemaは実装に先立って対応するOpenAPI/JSON Schemaを更新する。

Search契約の配置変更は[統合判断](../../docs/decisions/2026-10-04-search-main-migration-integration.md)に従う。両契約のwire意味と既存の凍結原文hashを統一・置換しない。

OpenAPI 3.2-only features must be covered by tooling compatibility tests before production use. Code generation compatibility is handled by its separate PoC plan.
