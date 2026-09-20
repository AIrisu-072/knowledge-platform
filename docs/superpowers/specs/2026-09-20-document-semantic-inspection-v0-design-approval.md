# Document Semantic Inspection v0 — Design Approval

- Capability: `Document Semantic Inspection v0`
- Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Approval date: 2026-09-20
- Approval source: explicit user instruction — “Design Specを承認して続けて”
- Status: **APPROVED — DESIGN FREEZE ACTIVE**

## Frozen scope

The approved Design freezes:

- separation of Document Semantic Inspection from Search Extraction;
- no durable common cross-format content IR for Versioning;
- FileObject as authoritative original;
- immutable derived inspection records keyed by `file_id + inspection_profile_version`;
- format-native semantic fingerprinting;
- Semantic Capability Contract for cross-format equivalence;
- editorial provenance / signature evidence / external dependency separation;
- proposed-final projection for WORKING Track Changes;
- Publish fail-closed rules for unresolved Track Changes, comments, and invalid/unverifiable existing signatures;
- out-of-process stateless sandbox worker with no network/credentials/DB/Storage access;
- all-or-nothing semantic success for Versioning;
- required v0 formats and explicit unsupported classes;
- XLSM/VBA static semantic inspection with no macro execution;
- detailed Diff as a separate future capability;
- library qualification based on determinism, fail-closed behavior, verification evidence, auditability, and security boundaries;
- PoC Coverage Matrix and 100% required-fixture promotion gates.

## Change control

Any change to a frozen semantic rule, trust boundary, supported-format requirement, success/failure meaning, or production-promotion gate requires an explicit Design amendment and user approval before implementation.

PoC results may select/reject libraries or add supplemental adapters **within** the frozen contracts without a Design amendment. If PoC evidence shows the frozen contract itself is infeasible, implementation stops and returns to Design review.
