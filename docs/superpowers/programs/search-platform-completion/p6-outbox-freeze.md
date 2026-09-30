# P6 Generic Durable Outbox — Design Freeze

- Status: FROZEN architecture contract; concrete implementation plan pending. No implementation/DB qualification claim.
- Authority: Completion Program explicitly authorizes autonomous freeze and implementation.
- Exact design: `p6-outbox-design-revision-1.md`, SHA-256 `c886a6b0933d6323b0fab7d41f0641e18332c6c74104025442a03609369d5366`.
- Independent GO: `p6-outbox-architecture-recheck.md`, SHA-256 `9728fffa22b501ada2f408735a3a325b592a2ba63b4d808e7ac800a5649be88c`; original four P2 findings closed, no new P1/P2.
- Preserved boundaries: generic delivery owns delivered_at; Search consumer owns graph/projection publication; lease/fence validates dispatch and conditional receipt/publication; Source fence is distributed, not a process mutex.
- Plan: `p6-outbox-plan.md`, scoped generic infrastructure, Search bridge and shared additive migration edits.
- Pending: TDD, real concurrent PostgreSQL/fault tests, independent code review, P7 integration and exact-head hosted acceptance.
