# Document HTTP/OpenAPI Transport v0 — Capability Execution Status

## 2026-09-29 JST — 設計承認・Production Implementation Planレビュー待ち

- 状態: **DESIGN APPROVED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**。製品コード、OpenAPI paths、production dependency、本番deployは未変更。
- 基準: `main@77b13a1d35d15eea0112ca2d73f8cbd3dfffe1c9`。Document Diff v0はPR #23/#24ともmain統合済みで、main CI `36533301309` はexact merge commitでSUCCESS。
- branch: `design/document-http-openapi-transport-v0`。Draft PR #27。初回設計commit `e56d50b95f22841e4ea05580ab1515bb45f3b0d7`。
- Frozen Design: `docs/superpowers/specs/2026-09-29-document-http-openapi-transport-v0-design.md` blob `88f7046a5d14a77f4091df0c92691f6634dd57d7`。依頼者が2026-09-29に明示承認。承認記録を追加。
- 固定した主要境界: OpenAPI 3.2.1 contract-first、Human/Agent共通API、VerifiedActorContextの自己申告禁止、Application入口以外からRepository/Storageへ接続禁止、既存UUIDv7 operation ID + revisionを再実行/OCCの正本に利用、file Audit確定前のbyte送信禁止、Diffのverdict/coverage分離維持。
- 自己レビューでUI実利用上の不足を確認し、Application補完を明確化: (1) initial createの認可済みoutcome lookup、(2) DMBで既定済みadminister認可のAccessPolicy read（local revision + effective policy）、(3) Repository定数を漏らさないroot Folder discovery、(4) 現在BusinessRuleへ集約される管理理由の型付きerror surface、(5) 既存serviceの薄いcomposition。いずれも既存業務意味のtransport公開であり、新しいACL/workflowを追加しない。
- Version/Folder作成ではApplication commandにtarget resource IDが含まれるため、Clientがresource IDをoperation開始前に生成しretryでも固定する。Transport側でretryごとに再生成しない。
- IdentityのWindows/AD/SSPI実接続、GUI、Agent Tool/CLI、Search API、deployは別工程。
- Production Implementation Plan: `docs/superpowers/plans/2026-09-29-document-http-openapi-transport-v0-production-implementation.md`。HAPI-01〜12、A→B→C→D。現時点では未承認。
- Plan self-review: UI実利用に必要なpolicy read/root discovery/create recovery/typed management errorをApplication補完として先行し、HTTP resource profile candidate（JSON 1 MiB、file 256 MiB、multipart total 1 GiB、Diff 45 s等）を有限境界として追加。設計意味の変更なし。
- Implementation session handoff: `docs/superpowers/handoffs/2026-09-29-document-http-openapi-transport-v0-implementation.md` を作成。Frozen Design `88f7046a5d14a77f4091df0c92691f6634dd57d7` / Plan `111914181143672d3fec901dae75fc2af0256b15` を再取得し、plan approvalが一致する場合だけHAPI-01へ進む。
- blocker: Production Implementation Planの依頼者承認。承認前に製品実装へ進まない。
- 次のexact action: Production Implementation Planを自己レビューして依頼者へ提示する。承認されたらplan approval recordを作り、別session向け実装開始promptからGitHub正本を再取得してHAPI-01 REDへ進む。
