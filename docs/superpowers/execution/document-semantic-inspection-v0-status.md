# Document Semantic Inspection v0 — Execution Status

- Capability: `Document Semantic Inspection v0`
- Execution mode: **Inline Execution**
- Overall phase: **POC QUALIFICATION EXECUTION / TASK 2 COMPLETE / TASK 3 NEXT**
- Design path: **Architectural**
- Frozen Design merged: PR #7
- Execution branch: `test/document-semantic-inspection-poc-v0`
- Execution PR: **#8 (Draft)**
- Execution baseline: `main@5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`
- Last qualified code head: `b9da1bfa5fdf07f1b99a13248fe3233fae1082c9`
- Design Spec: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Design approval: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- PoC Qualification Plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`

## Approval state

- Frozen Design: **APPROVED / FROZEN**
- PoC Qualification Plan: **APPROVED 2026-09-21**
- Production dependency promotion: **NOT AUTHORIZED** by this approval; PoC qualification only.

PR #7 was advanced from review to approved state and merged after explicit user approval. The execution baseline is the resulting main merge commit `5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`.

## Task 1 — COMPLETE

Isolated workspace, deterministic harness, manifest validation, report output, root mise entrypoints, and path-scoped hosted DSI PoC CI are implemented.

TDD / CI evidence:

- RED commit: `0650625db9ad0fd89f6292b6eddce3e4f6b48ac0`
- RED DSI run: `35520524705` — FAIL as expected; the first observed failure exposed an invalid independently-copied PoC lockfile before compile.
- Harness implementation commit: `1980d03b18ca90f4896ef17b2206f1013cd452e6`
- Exact locked Task 1 head: `c9ad1c9f5ec6e47c53f0409027bf98e3886b8252`
- DSI PoC run: `35520894240` — **SUCCESS**
- Standard CI run: `35520894277` — **SUCCESS**

The lockfile issue was repaired by generating the isolated workspace's own lock and committing it. The PoC crate is marked `publish = false`; the private experiment itself is therefore excluded from third-party license evaluation while dependencies remain enforced.

## Task 2 — COMPLETE

TXT / CSV / HTML semantic adapters and 17 synthetic qualification cases are implemented.

Behavior pinned by tests:

- TXT: CRLF and canonical Unicode noise are invariant; content changes differ; ambiguous encoding fails closed.
- CSV: quote syntax noise is invariant; row/cell changes differ; inconsistent columns and missing explicit delimiter fail closed.
- HTML: whitespace/decorative attributes are invariant; visible text/link/image changes differ; script-required semantics fail closed without script execution.

TDD / qualification evidence:

- RED commit: `f3b62f9ac413658ece3d80e5843f25009c1db0f4`
- RED DSI run: `35521340386` — FAIL as expected with unresolved `TextAdapter`, `CsvAdapter`, and `HtmlAdapter`.
- Initial GREEN candidate: `d412dc1c0a53f4d7647c394b1bf24814bbe1c6e3`
- Initial GREEN DSI run: `35521588349` — semantic tests and `dsi-poc verify` passed all 17 cases, but `cargo-deny` rejected `scraper 0.27.0` because its transitive `cssparser/selectors` graph contains MPL-2.0.
- Replacement commit: `b14b7954cf64295ac808b96e3ef65c698ad9def2`
- Replacement DSI run: `35521818109` — **SUCCESS** using direct `html5ever 0.39.0 + markup5ever_rcdom 0.39.0`.
- Exact locked Task 2 head: `b9da1bfa5fdf07f1b99a13248fe3233fae1082c9`
- Exact locked DSI run: `35521964417` — **SUCCESS**

### HTML candidate decision

`scraper 0.27.0` is **REJECTED for this repository** under the existing dependency-license policy. This is a qualification result, not a Design semantic change.

The accepted Task 2 HTML PoC substrate is:

- `html5ever = "=0.39.0"`
- `markup5ever_rcdom = "=0.39.0"`

The same frozen HTML semantic fixtures were retained across the candidate swap.

## Current gate / next exact action

Proceed to **Task 3 — DOCX qualification**:

1. add the Task 3 candidate dependencies only inside the isolated PoC workspace;
2. independently generate minimal OOXML/DOCX fixtures;
3. write DOCX RED tests before the adapter;
4. run RED and record hosted evidence;
5. implement `DocxAdapter` plus raw OOXML coverage sentinel;
6. require unknown potentially semantic package parts to fail closed;
7. run the determinism repetition and `mise run poc:dsi:verify`.

Do not promote any qualified candidate into production crates during this Plan.

## Resume order

1. `AGENTS.md`
2. `docs/superpowers/execution/active.md`
3. this status file
4. frozen Design Spec
5. Design approval record
6. PoC Qualification Plan
7. current GitHub state of branch `test/document-semantic-inspection-poc-v0`, PR #8, and exact-head CI

Repository/GitHub state overrides chat memory.
