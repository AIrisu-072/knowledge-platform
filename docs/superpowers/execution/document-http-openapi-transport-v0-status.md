# Document HTTP/OpenAPI Transport v0 — Capability Execution Status

## 2026-09-29 JST — 設計開始

- 状態: **DESIGN ACTIVE / WRITTEN SPEC REVIEW PENDING**。製品コード、OpenAPI paths、production dependency、本番deployは未変更。
- 基準: `main@77b13a1d35d15eea0112ca2d73f8cbd3dfffe1c9`。Document Diff v0はPR #23/#24ともmain統合済みで、main CI `36533301309` はexact merge commitでSUCCESS。
- branch: `design/document-http-openapi-transport-v0`。
- 設計SSOT候補: `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design.md`。
- 固定した主要境界: OpenAPI 3.2.1 contract-first、Human/Agent共通API、VerifiedActorContextの自己申告禁止、Application入口以外からRepository/Storageへ接続禁止、既存UUIDv7 operation ID + revisionを再実行/OCCの正本に利用、file Audit確定前のbyte送信禁止、Diffのverdict/coverage分離維持。
- 追加で必要と判断したApplication補完は、initial createのcommit outcomeを安全に照会する認可済みlookupと、既存serviceを同一verified actorで束ねる薄いcompositionだけ。AccessPolicy read等の新しい業務意味は便乗させない。
- IdentityのWindows/AD/SSPI実接続、GUI、Agent Tool/CLI、Search API、deployは別工程。
- blocker: 書面設計の依頼者承認。承認前にProduction Implementation Planや製品コードへ進まない。
- 次のexact action: Draft設計PRをmain向けに作成し、設計本文をレビューする。依頼者が設計を明示承認した後にapproval recordとProduction Implementation Planを作成する。
