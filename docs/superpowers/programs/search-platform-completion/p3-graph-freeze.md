# P3 Durable Typed HyperGraph — Design Freeze

- Status: FROZEN semantic/coordination contract, concrete measured PoC and implementation plan pending. PostgreSQL remains a provisional candidate, not an adopted production backend.
- Authority: autonomous Completion Program design/freeze approval.
- Exact input: `p3-graph-design-revision-1.md`, SHA-256 `ba9e8616d9dd8fc569280f20956c841929155366fa30246dadee0f0c73e8aa59`.
- Exact input: `p3-graph-build-guard-amendment.md`, SHA-256 `4f4597622f2053a7a782c5f0bf7704681c5542ed589d0b4fc3612ffa01595ab0`.
- Exact input: `p3-graph-cleanup-order-correction.md`, SHA-256 `e0754819b37b4fc7eaf9cf1104ec634ed15549f520df2e1f25d45b1d8951d691`.
- Exact input: `p3-graph-build-guard-review.md`, SHA-256 `afe2fcd86688c741098480c72c44e7463217d509ae90da28743d5c9153cfc783`.
- Composition precedence: revised design, build guard, FK-safe cleanup correction. Independent GO closes all original five P2 findings and the cleanup-order finding.
- Preserved typed n-ary/Source authority/current access/temporal/generation semantics and in-memory semantic oracle. Durable READY immutable, atomic pointer/pin/GC/fence and protected base/target builds must hold.
- Required measured backend comparison: PostgreSQL incidence, Rust persistent adjacency and dedicated graph DB, real data/query/update/recovery/space/license evidence. Selection before production backend promotion.
- Plan: `p3-graph-plan.md`; independent bounded PoC, shared coordination, durable adapter and multi-process recovery tests. P7 composition and hosted qualification remain pending.
