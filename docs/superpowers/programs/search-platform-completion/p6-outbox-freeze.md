# P6 Generic Durable Outbox — Design Freeze

- Status: FROZEN architecture contract and bounded implementation plan. No implementation/DB qualification claim.
- Authority: Completion Program explicitly authorizes autonomous freeze and implementation.
- Exact design: `p6-outbox-design-revision-1.md`, SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`.
- Independent GO: `p6-outbox-architecture-recheck.md`, SHA-256 `9728fffa22b501ada2f408735a3a325b592a2ba63b4d808e7ac800a5649be88c`; original four P2 findings closed, no new P1/P2.
- Preserved boundaries: generic delivery owns delivered_at; Search consumer owns graph/projection publication; lease/fence validates dispatch and conditional receipt/publication; Source fence is distributed, not a process mutex.
- Plan: `p6-outbox-plan.md`, SHA-256 `29638c077a29aebd007be4a4fd7c5fb7f7bfbf4a785ba0540782f98795811a80`; 21 bounded tasks, scoped generic infrastructure, Search bridge and shared additive migration edits.
- Execution: generic receipt and full integrated receipt are separate; shared durable substrate is implemented before final P7 assembly to avoid cyclic qualification.
- Pending: TDD, real concurrent PostgreSQL/fault tests, independent code review, P7 integration and exact-head hosted acceptance.
