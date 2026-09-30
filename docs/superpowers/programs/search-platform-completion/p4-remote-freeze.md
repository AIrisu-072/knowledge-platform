# P4 Remote Source — Design Freeze

- Status: FROZEN architecture contract; implementation plan is being prepared. No production/PoC/CI acceptance claim.
- Requester authority: 2026-09-30 Completion Program prompt explicitly approves autonomous design/freeze/plan/selection/implementation.
- Exact design: `p4-remote-design-revision-1.md`, SHA-256 `f4112c7aa0cf7cbf61dca19c4beebcfb14a6578432ac45861644ab8ea616f9e9`.
- Independent GO: `p4-remote-architecture-recheck.md`, SHA-256 `698511291043276d86687c203ef1e319e09755f0f53064f38528f0760456a635`, reviewer `/root/completion_p4_architecture_recheck`. Original P1 2/P2 3 findings closed; no new related P1/P2.
- Preserved frozen semantics: Source-owned truth; one immutable generation per Source/evaluation; no query-miss absence; trusted tenant/actor binding before routing/gaps/trace; globally unique server-owned SourceId; strict five retention modes; evidence role/origin verified by adapter; existing S1 and business logic.
- Plan: `p4-remote-plan.md`. Library HTTP-client promotion requires credential-free real-transport PoC and dependency/license qualification before production addition.
- Acceptance still pending: scoped TDD, concrete local TCP provider E2E through DiscoveryService, all retention and authority races, SSRF/limits, independent code review, exact-head hosted gates and receipt.
