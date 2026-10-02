# Organization D2 hosted source and visual qualification

## Parent activation sequence

1. The parent published the reviewed source-only [D2 DraftPR48](https://github.com/AIrisu-072/knowledge-platform/pull/48)
   on remote D1 `13c1292c806c9179be0a444ef2b4be8234e00bf4`. Source-only head
   `0dcaeea6d1dce37f3833f5c8ec972ebf9368bac1`, tree
   `a7517c356b1809dda8da7faccf9894c73ea2563f`, matches reviewed locald37bb7ab.
   `REVIEW.pr` now binds48. No capture activation or integrated-head qualification
   is implied by this allocation.
2. Integrate only reviewed D2 source and this harness on
   `design/organization-client-v0-ui`. Re-run Node unit/static/architecture and
   source tests; independently review the complete source/privacy/workflow delta.
   Final review includes the actual PR binding and finite UTC window. Preserve
   PR43 guard/workflow behavior, frozen D1 designs, locks and product source.
3. Publish the reviewed combined exact head. Wait for that head's ordinary CI
   including policy/security/required-check, DSI PoC `qualification`, DSI
   Sandbox `preflight`, and the separate `qualify-source` job to succeed.
   Its `capture-review` job must be skipped. A source-only earlier
   head's success is not qualification for the combined head. Preserve these
   normal subjects and inspect the source blob/SHA256 console receipt.
4. Verify the exact head is still current, no prior D2 capture attempt exists,
   the currentreview window is still open, and all independent findings are
   closed. Parent then applies `d2-visual-<40-character-exact-head>` once.
   The explicit owner-sender guard is `AIrisu-072`; an event emitted by another
   identity fails closed. Do not weaken the actor check to repair a denial.
5. The labeled first attempt runs `capture-review`: it independently verifies
   the prior normal runs and live head/base/label, repeats all source/browser checks,
   captures the20 approved states and exports only after success/cleanup/source
   recheck. No automatic push/rerun/remove-and-reapply activation is allowed.
   Missing/failed API evidence, expiry or earlier same-head source-run ambiguity
   blocks export. Diagnose a fixed failure category, create/review a corrected
   new head if needed, then obtain fresh normal qualification. Do not loosen a
   gate just to obtain pixels or use another browser to bypass a denial.
6. Record separate normal run IDs, capture head/tree/run/attempt, upload action
   SHA, artifact ID/digest, actual creation/expiry,20 filenames and dimensions.
   Download only the permitted artifact for review within its one-day lifetime.
   Inspect every actual pixel image for hierarchy, glyphs, density, clipping,
   action/context visibility and scenario correctness. Parser/DOM/helper passes
   cannot replace this visual judgment. Record defects and actual review verdict
   in a separate report subject; do not retroactively change the source subject.

## Current PR48 prerequisite subject and DSI applicability

The reviewed base is `design/organization-client-v0` at exact
`13c1292c806c9179be0a444ef2b4be8234e00bf4`. Event, ordinary D2 source run, all
selected prerequisite associations, and live PR must agree with it. A retarget
or base advance requires fresh review and qualification; head equality alone is
insufficient because ordinary CI and DSI default checkout tests the PR merge.

The integrated delta includes `docs/superpowers/execution/active.md`, which
matches both existing DSI workflow path filters, and `mise.toml`, which also
matches DSI PoC. These files remain in the current PR48 diff. Therefore this
bounded guard always requires `.github/workflows/dsi-poc.yml` with mandatory
`qualification` success and `.github/workflows/dsi-sandbox-preflight.yml` with
mandatory `preflight` success, in addition to CI and normal D2. Each run must be
completed/successful, exact-head, first attempt and unambiguously associated
with the current PR48/base. Optional Draft macOS DSI jobs can be skipped. No
path-filter parser or generic policy exception is introduced.

The source-only0dcaeea6 subject had CI only; its DSI path filters were inapplicable.
That historical result is neither a missing-run error for that old subject nor
qualification for this new integrated source. Do not label the integrated head
until all four applicable normal workflow subjects are confirmed.

The real schema was verified through the read-only [run37016358837 API](https://api.github.com/repos/AIrisu-072/knowledge-platform/actions/runs/37016358837)
and [PR48 API](https://api.github.com/repos/AIrisu-072/knowledge-platform/pulls/48).
The unit fixture retains only public run/PR/repository identity fields. The run
association uses head/base `{ref,sha,repo:{id,url,name}}`; full PR/run repository
objects separately use their full identity. Missing or multiple associations
fail closed. Count all same-head/path/branch source runs before validating their
associations, preserving denial after a wrong-PR run or prior capture attempt.

Workflow metadata is read through GitHub's documented [public workflow-run API](https://docs.github.com/en/rest/actions/workflow-runs).
Its optional `path@ref` display suffix is normalized only for the exact workflow
filename; repository, branch, head SHA, event, first attempt and job outcomes
remain independently checked.

## Commands and evidence

- Unit/static only, no browser: pinned Node24.21.0
  `node --test tools/organization-d2/test/*.test.mjs`, syntax checks,
  actionlint, architecture-lint and `git diff --check`.
- Existing qualified jsdom can run the source packet's `source-design.test.mjs`
  and `interaction.test.mjs` with `ORG_DESIGN_JSDOM` pointing to its installed
  entrypoint. This is DOM-unit evidence only.
- Hosted normal: `mise run organization:d2:normal`.
- Hosted approved capture: `mise run organization:d2:capture`, invoked only by
  the guarded labeled job. These entrypoints reject local/non-Actions execution
  before importing or launching a browser. Do not download a local browser.
- The workflow reuses existing frozen pnpm dependencies with scripts disabled,
  qualified runner font pins, and pinned Chromium from Playwright1.63.0. It adds
  no application build or Cargo work. Dependency/font/browser installation
  failures emit only a fixed setup category; no setup log is uploaded.

The source server serves only `sales.html`, `office.html`, `prototype.css`,
`prototype.js`, `scenarios.json`, `scenarios.js`; it hashes those and the two
source unit tests as an eight-file review packet. Sources are not copied into
this harness commit. The integrated source is checked against Git blob identity
and rechecked after browser shutdown. The immutable snapshot is the served
subject. No live backend, native filesystem/provider, real Agent or real data is
connected.

## Exact visual scope

Two pages × these ten states:
`normal`, `newly_assigned`, `returned`, `working_draft`, `handed_off`, `due_soon`,
`blocked`, `agent_active`, `evidence_review`, `document_compare`.

Each export is1440px wide,900–2400px high, full-page, at most8MiB, named
`sales-<state>.png` or `office-<state>.png`. Actual geometry also runs at1280×900,
but no1280px images are captured or exported. The fresh-page capture fixtures
are isolated from keyboard/confirmation/selection test mutations. All20 images
must be inspected; count and parser checks alone cannot establish legibility.

Font evidence is exact Kosugi-Regular CDP selection for the unchanged real
source's Japanese heading and body nodes, with exact glyph counts and synthesis
disabled. This does not establish a separate bold face, punctuation/symbol
coverage, ideal typography, or complete layout accessibility. Those remain pixel
review matters. Network proof covers requests observed within the fresh browser
contexts, not all operating-system traffic.

## Review expiry and failure handling

Current authority window ends2026-10-03T13:19:00Z. It does not renew itself.
Extending it requires a newly reviewed current-review decision; this record does
not grant a standing upload exception. Artifact retention is separately requested
as one day from creation and must be verified from the real receipt.

Raw failures stay private to the disposable runner or in memory. Public browser
failure categories are environment/source/gate/prerequisites/browser/network/
geometry/keyboard/font/pixels/export/cleanup/internal. No failed or partial
capture exports. Read the exact source to diagnose; preserve the failure and
normal/capture identities without publishing raw DOM/console/trace bodies.
