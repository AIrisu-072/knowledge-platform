# Search / Discovery Platform v0 — Design Approval

- Approval status: **APPROVED**
- Approved by: requester
- Approval date: 2026-09-28 JST
- Design branch: `design/search-discovery-platform-v0`
- Approved written design: `docs/superpowers/specs/2026-09-28-search-discovery-platform-v0-design.md`
- Approved design commit: `665ecd5a62f641e364e0e4940845aeb7b98eff52`
- Review/status commit before approval: `56a5fc138f47403eac8ad684edfcf89f0a22b78f`
- Baseline main at design start: `6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`

## Approval scope

The requester explicitly approved the written Search / Discovery Platform v0 design after the D1〜D9 conversational design sections were individually fixed.

The Design Freeze covers, at minimum:

- Search Platform as a logical subsystem inside Knowledge Platform, with Discovery as its higher-level capability;
- Federated Source Registry / source-local discovery rather than a mandatory single global resource index;
- typed DiscoverableResource families and UsageProfile / DiscoveryProfile / DiscoveryLens;
- IntentSignature, typed Applicability predicates, UNKNOWN semantics, Contrastive Resolution, InformationGap;
- Assertion / Authority / Logical Identity / Observation / Temporal models;
- derived Projection architecture with Directory / Structured / Lexical / Vector / Temporal / Access / first-class HyperGraph projections;
- canonical Typed N-ary Relation / HyperEdge semantics and non-canonical status of any future binary shortcut;
- adaptive evidence-driven Retrieval / Probe / Materialization / Source expansion;
- Evidence Requirement / Claim / Evidence Sufficiency / Failure Attribution;
- Session-stable Binding and Task-scoped Context Compiler boundary;
- separation from Execution Control Plane / Task DAG ownership / credential handling / Human Oversight;
- correctness-first Cost / Performance architecture;
- stage-wise Evaluation / Assurance including SOURCE_KNOWLEDGE_ABSENT.

## Deferred decisions preserved

Approval does **not** select or authorize a concrete:

- graph database/backend;
- vector engine / ANN implementation;
- embedding model;
- reranker model;
- fusion library;
- predicate runtime backend;
- transport/API framework;
- physical service topology;
- production dependency;
- production SLO / Recall threshold.

Those require the implementation plan and, where marked POC REQUIRED, qualification evidence.

## Implementation gate

This approval permits:

1. explicit reconciliation of existing normative `spec/` documents with the approved design;
2. writing a Production Implementation Plan.

It does **not** by itself authorize Production code implementation, dependency promotion, merge, deployment, or production migration.

Production implementation starts only after the requester approves the written implementation plan in a separate step.
