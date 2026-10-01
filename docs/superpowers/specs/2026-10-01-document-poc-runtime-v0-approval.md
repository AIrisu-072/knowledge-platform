# Document Platform PoC Runtime v0 — Bounded Authority Record

Date: 2026-10-01 UTC

Status: FIXED REQUIREMENTS AUTHORIZED BY REQUEST §30; scheduler executor decision EXCLUDED / STOP. No implementation, acceptance, merge or deployment approval is inferred from successful documentation publication.

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
- Design Git blob: `2afd3f4d9c126b6820a9d5d630cc0777c7cf91e5`
- Plan: `docs/superpowers/plans/2026-10-01-document-poc-runtime-v0-implementation.md`
- Plan Git blob: `71bf1b6e4b4b3b7a40e9dc35ed4be567be6c07a3`

## Explicit exclusions and prerequisites

- Provider `poc` / principal `poc-scheduler` is a candidate only. No scheduler executor is selected. Task R5 and scheduler acceptance remain STOP until an explicit decision is recorded.
- Unknown identity, arbitrary groups, Agent write tools, production authentication, Search/RAG changes and any separate business API are not authorized.
- Existing frozen Document business/authorization/audit/Revision/Diff contracts and license policy remain authoritative. Qualify any changed dependency graph; STOP rather than silently expanding the GUI-specific exception.
- Baseline PR36 hosted success does not prove current real-backend GUI acceptance. C0 closure is pending. C1 completion requires real runtime and all required acceptance, including the separately blocked scheduler.
- R1 is documentation only. Product implementation belongs to R2 and is subject to the bounds above. Publication is not completion of C0/C1/C2/C3.
