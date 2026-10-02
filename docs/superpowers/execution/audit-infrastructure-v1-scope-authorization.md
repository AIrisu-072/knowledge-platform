# Audit Infrastructure v1 — bounded scope authorization

Date: 2026-10-02 UTC. Source: current requester's explicit request to design and implement Audit Infrastructure v1 in AIrisu-072/knowledge-platform. This records task authorization, not a claim that the requester personally reviewed artifacts created later.

The requester explicitly directed:

> ただし今回のAudit Infrastructure v1は、既存Document Platformですでに存在するAudit契約・Audit Outboxを完成させる横断Infrastructureであり、Organization Client固有Domainを必要としない範囲については **PR #43完了を待たず独立して進めて構いません**。

> Audit作業は、GitHub / repositoryの現在状態を確認した上で、**PR #43 stackに依存しない最新のqualified baseline**から独立branch / Draft PR stackを作成してください。

> STOPがなければ、確認待ちせず、
> A1 Design Freeze
> ↓
> A2 Schema Qualification
> ↓
> A3 Audit Store / Delivery
> ↓
> A4 Producer Integration
> ↓
> A5 Acceptance
> ↓
> A6 Organization Handoff
> まで自律的に進めてください。

> ただしすべてDraftのままとし、merge / deploy / production migration / production Identity接続は実施しないでください。

A0 must precede code. The scope includes independent architecture/security review and resolution of P1/P2 before qualification. Design/plan freeze hashes will be recorded after review; their scope remains bounded by the actual 20-section request and normative repository contracts.

STOP conditions: breaking approved Audit meaning; unavoidable unaccepted PR43 dependency; Organization semantics essential to core; unavoidable breaking event migration; new external operational/paid service; license/security policy violation; production deployment/credentials; incompatible integrity/transaction semantics; major conflict with existing WIP generic outbox architecture. Stop/defer only the affected dependency when independent core can continue.

No permission to edit Document acceptance or Search WIP, define Organization business concepts, merge/close PRs, deploy, perform production migrations or connect production identity is inferred.
