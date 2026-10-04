# Document Platform PoC Runtime v0 — Bounded Authority Record

Date: 2026-10-01 UTC

Status: FIXED REQUIREMENTS AUTHORIZED BY REQUEST §30; scheduler attribution resolved by the bounded 2026-10-02 naming decision below. No implementation, acceptance, merge or deployment approval is inferred from successful documentation publication.

## 2026-10-02 UTC — Bounded scheduler attribution decision

The requester delegated the shortest comprehensible reusable name after rejecting a PoC-specific name. Exact conversation context:

- Requester: 「秘密情報検査31件の限定除外は承認します。PoCというのは対外的につけるもので実装に含めるべきじゃないのでは、、？本番に移行することを考えた場合、外部の進捗としてPoCとして管理するのはいいが、実装コードの命名に使うものじゃないのでは？」
- Assistant: 「予約公開の実行者名は『document-publication-scheduler』を提案します。監査記録用で権限を追加しない設計はそのままです。この名前で進めてよいですか？」
- Requester: 「その名前だと長すぎるので、理解できる最短の長さの名前にしてください。」
- Assistant: 「『scheduler』にします。文書管理の中では予約公開処理だと分かり、PoCにも本番にも依存しない名前です」

Chosen bounded attribution: `PrincipalRef { identity_provider: "service", principal_id: "scheduler" }`. `service` reuses the existing scheduler test namespace; it is a stored attribution namespace, not a newly implemented authentication provider. The requested short reusable principal is `scheduler`, not `poc-scheduler` or `document-publication-scheduler`. This delegated naming decision resolves STOP-01 only for R5's executor attribution.

The executor has no groups, roles, ACL grants, credentials, login capability or selectable HTTP profile. It is never resolved into an authenticated requester and never added to policy subjects. The scheduler separately re-resolves only the two existing fixed PoC requesters, preserving their exact subjects; existing Application/Repository code checks current ACL at execution and commit. Static resolution requires `KP_RUNTIME_MODE=poc`; production and unknown modes fail closed. No production identity adapter/framework is authorized.

The first quoted sentence about scanner findings is context only: this record does not change or implement scanner exclusions. All original runtime, authorization, sandbox, separate-process, verification and no-merge/no-deploy boundaries remain. Historical statements below reflect the original R1 approval state and are superseded only by this bounded R5 attribution decision.

## Source and scope

The requester's Document Platform PoC Runtime / Server Composition / Agent Integration instruction of 2026-10-01, §§2–15 and §30, expressly fixes the following choices and permits faithful written design/plan transcription followed by implementation without another approval round:

- `document-server` composition root without business logic or reversed dependencies
- `StaticPoCIdentityAdapter`, process-fixed `poc-human` and `poc-agent`, separate instances sharing PostgreSQL/FileStorage
- no request-supplied identity; no production identity, production fallback or production deployment
- loopback by default, explicit warned nonloopback PoC override, fail-fast redacted config
- same-origin built Human GUI, health/readiness and graceful shutdown
- explicit migration, no migration during `serve`, synthetic bootstrap/seed through existing contracts
- scheduler as a separate process and actual composition-root acceptance

The requester also requires capability-separated PRs (§29), RED → GREEN → local → exact-head hosted verification (§32), and no merge/deploy. This record documents that existing authority. It is not a claim that the requester individually reviewed or approved a new scheduler identity, a new dependency/license exception, or every implementation detail as a separate semantic decision.

## Reviewed document identities

The exact design and plan Git blob IDs are recorded below after requirements review. Subsequent semantic changes require renewed review and any required approval; editorial changes require updated blob identities.

- Design: `docs/superpowers/specs/2026-10-01-document-poc-runtime-v0-design.md`
- Design Git blob: `70a1c6766fd7077b2d6099f9c306c01594c79fca`
- Plan: `docs/superpowers/plans/2026-10-01-document-poc-runtime-v0-implementation.md`
- Plan Git blob: `96cc56db813e91963cc325905225c137c92118c3`

## Explicit exclusions and prerequisites

- Original R1 exclusion: provider `poc` / principal `poc-scheduler` was a candidate only. Superseded by the bounded 2026-10-02 `service` / `scheduler` attribution decision above. R5 acceptance still requires fresh real-process evidence.
- Unknown identity, arbitrary groups, Agent write tools, production authentication, Search/RAG changes and any separate business API are not authorized.
- Existing frozen Document business/authorization/audit/Revision/Diff contracts and license policy remain authoritative. Qualify any changed dependency graph; STOP rather than silently expanding the GUI-specific exception.
- Baseline PR36 hosted success does not prove current real-backend GUI acceptance. C0 closure is pending. C1 completion requires real runtime and all required acceptance, including the separately blocked scheduler.
- R1 is documentation only. Product implementation belongs to R2 and is subject to the bounds above. Publication is not completion of C0/C1/C2/C3.
