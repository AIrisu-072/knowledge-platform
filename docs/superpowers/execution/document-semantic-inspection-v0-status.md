# Document Semantic Inspection v0 — Execution Status

- Capability: `Document Semantic Inspection v0`
- Execution mode: **Inline Execution**
- Overall phase: **POC QUALIFICATION EXECUTION / TASK 4 RED COMPLETE / XLSX-XLSM GREEN NEXT**
- Design path: **Architectural**
- Frozen Design merged: PR #7
- Execution branch: `test/document-semantic-inspection-poc-v0`
- Execution PR: **#8 (Draft)**
- Execution baseline: `main@5cfe6cefebc1e695b04cd0dc4c19707aeb8b4eab`
- Last qualified code head: `b4dae3c89fa84ce50deada7f268aa5b04830da5d`
- Task 3 dependency-preflight candidate head: `ec532ec12d89352d83dc9a85ae68a3da583c0ebb`
- Task 3 dependency-preflight DSI run: `35545142423` — **SUCCESS**
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

## Task 3 — DEPENDENCY PREFLIGHT COMPLETE

Candidate selection evidence:

- `stemma 0.5.0` — **REJECTED**: vulnerable transitive Quick-XML line; legacy ZIP graph also violates the repository license gate.
- `docx-review-core 0.1.1` — **REJECTED**: vulnerable transitive Quick-XML line.
- `docxml 0.3.1` — **REJECTED**: default ZIP codec graph includes license expressions outside the current allowlist.
- `office_oxide 0.1.11` + direct `zip 8.6.0` deflate-only + `quick-xml 0.42.0` — **PREFLIGHT PASS**.
- DSI PoC run `35545142423`: existing 17 semantic cases, CLI verification, advisories, license and source gates all passed with this candidate graph.

Ruling: do not weaken security/advisory/license policy to preserve a planned parser name. Keep the frozen semantic contract and qualify the replacement candidate against the same DOCX fixtures.

## Task 3 RED evidence

- RED contract head: `21fea55cffebcb53dac5886ffedcbb923bc19cd5`
- DSI PoC run: `35546037242` — **FAIL as expected**
- Exact failure: unresolved import `document_semantic_inspection_poc::DocxAdapter`; fixture/manifest/raw-binding validation introduced no earlier failure.

## Task 3 — COMPLETE

Fresh exact-head qualification evidence:

- Qualified head: `b4dae3c89fa84ce50deada7f268aa5b04830da5d`
- DSI PoC run: `35551781760` — **SUCCESS**
- Standard CI: **SUCCESS** at the same head
- DOCX tests: **13/13 PASS**
- Manifest verification: **38 cases PASS**
- Dependency gate: advisories/bans/licenses/sources **PASS**
- Determinism: 20 in-process repetitions per successful DOCX case plus 5 fresh-process snapshots **PASS**
- Hostile OOXML: relationship cycles, traversal, duplicate entries, archive-bomb/resource cases fail closed as required
- Editorial evidence: tracked-change details and resolved/unresolved comment state preserved separately from semantic identity

No Design amendment was required. `office_oxide 0.1.11` remains a PoC-qualified candidate only; no production dependency promotion has occurred.

## Task 4 — DEPENDENCY PREFLIGHT COMPLETE

Dependency evidence:

- `rxls = 0.1.3`
- `calamine = 0.36.1` with picture support
- `ovba = 0.7.1`
- `tree-sitter = 0.25.10`
- VBA grammar: exact upstream revision `c691f237b2a703732d4b6a1f01d5b4f73f94d41e`, vendored generated parser because the upstream Rust-package bindings are incomplete
- DSI PoC run `35606038480` — **SUCCESS**
- Existing DOCX/TXT/CSV/HTML suite remained green: manifest verification **38 cases PASS**
- cargo-deny: advisories/bans/licenses/sources **PASS**

The initial direct git-crate attempt failed at compile time only because the upstream commit references a missing `bindings/rust/build.rs`; this is recorded as packaging failure, not a grammar semantic failure.

## Task 4 — RED COMPLETE

Fixture / RED evidence:

- Independent raw SpreadsheetML XLSX corpus added without using rxls or Calamine serialization.
- Calamine synthetic `tests/vba.xlsm` imported from commit `0af05f4f6030351e3b8a999ea0810c8618368776` with MIT provenance.
- Local XLSM seed SHA-256: `2fe9f89f4a969658c1e3f9b0e8c70ccb155840aa6ee1bad6b15df603f41da1c0`.
- RED head: `fe1abea83d2669c1a847203cdcfe84926acbdb4f`
- DSI PoC run: `35607642860` — **FAIL as expected**
- Exact failure: unresolved imports `SpreadsheetAdapter` and `VbaAdapter`; dependency graph and existing suites compiled before the RED failure.

Ruling: represent format-specific instances as `SpreadsheetAdapter::XLSX` and `SpreadsheetAdapter::XLSM`, because the shared `InspectionAdapter` trait exposes exactly one `FormatId` per instance.

## Current gate / next exact action

Proceed to **Task 4 — XLSX/XLSM/VBA GREEN implementation**:

1. adjust RED tests to the explicit XLSX/XLSM adapter-instance API and re-confirm RED;
2. implement spreadsheet projection plus raw OOXML coverage sentinel;
3. implement strict Tree-sitter VBA canonicalizer and ovba full-module extraction;
4. resolve the required XLSM VBA source-mutation/noise fixture gate without skipping it;
5. run Task 4 verification and qualification evidence.

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
