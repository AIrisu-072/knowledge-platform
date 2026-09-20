# Document Semantic Inspection v0 — Execution Status

- Capability: `Document Semantic Inspection v0`
- Execution mode: **Inline Execution**
- Overall phase: **DESIGN REVIEW**
- Design path: **Architectural**
- Design branch: `design/document-semantic-inspection-v0`
- Baseline: `main@73492983dd324fcd53d4b485719e5c31048f9335`
- Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`

## Current state

The architecture, data contract, processing flow, failure/security boundary, format-specific acceptance criteria, library-selection policy, and PoC Coverage Matrix are written into the Design Spec.

No production implementation has started.

No PoC-only parser dependency has been promoted to production.

## Frozen-in-chat decisions represented in the Design

- Document Semantic Inspection is distinct from Search Extraction.
- No durable common cross-format content IR is used for Versioning.
- Format-native parse models are ephemeral.
- FileObject remains authoritative.
- Semantic inspection results are immutable, reproducible derived data keyed by `file_id + inspection_profile_version`.
- Versioning consumes semantic fingerprints/capability evidence, not Search Index data.
- Cross-format equivalence uses Semantic Capability Contract rather than generic content equality.
- one ContentItem has one authoritative representation and zero or more renditions.
- XLSM is a required v0 format because VBA carries business logic.
- VBA is statically extracted and never executed.
- Track Changes/comments are allowed in WORKING but block Publish according to the frozen rules.
- invalid/unverifiable existing signatures block Publish, while absence of signatures does not.
- worker runs out-of-process, without network/credentials/DB/Storage access.
- success is all-or-nothing for Versioning use.
- ZIP is transport, not authoritative semantic content.
- OCR and encrypted/password-protected documents are out of v0 semantic inspection.
- detailed diff is a separate `Document Diff v0` capability.
- parser/library selection is based on code-level determinism, fail-closed behavior, verification evidence, auditability, and security boundaries—not age or popularity.

## PoC candidate status

All parser/tool candidates in the Design remain **PoC candidates only**.

Production promotion requires the full format gate:

- semantic-change fixtures: 100% PASS;
- noise-invariance fixtures: 100% PASS;
- editorial separation: 100% PASS;
- fail-closed fixtures: 100% PASS;
- determinism: 100% PASS;
- resource/sandbox tests: PASS;
- no silent unsupported semantics: PASS;
- license/dependency gate: PASS.

## Current gate

The written Design has not yet been formally approved/frozen.

Required next order:

1. user reviews the written Design Spec;
2. requested corrections, if any, are applied;
3. user explicitly approves the written Spec;
4. write Design approval record and freeze the Spec;
5. invoke the implementation-planning workflow;
6. only after the Plan gate may PoC/implementation work begin.

## Resume order

1. `AGENTS.md`
2. `docs/superpowers/execution/active.md`
3. this status file
4. `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
5. current GitHub state of `design/document-semantic-inspection-v0`

Repository/GitHub state overrides chat memory.
