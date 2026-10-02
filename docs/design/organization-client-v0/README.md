# Organization Client v0 — two source designs

This packet formalizes the frozen Product/UX and Domain/API contracts. It is
synthetic, offline, inspectable source design, not the product frontend, backend
authorization proof, live Agent integration, Tauri application or Search rollout.

Open `sales.html` or `office.html` in a permitted local/hosted preview. Both use
the same CSS/JS and the same ten scenario definitions. Top review controls are
outside the proposed product shell. State URLs can use `?scenario=returned` or
another value from `scenarios.json`, with optional `module=evidence|agent|document|
search|return|history|resources`. They contain no private data.

The app's primary navigation is exactlyタスク/文書/検索; Agent/Evidence/Workspace
remain Context Modules. A submitted state renders immutable snapshot membership,
not a live working editor. Eligible-only queues omit private contents. Changed
task identity clears candidate/Agent previews and retains draft text only under
its original stable item ID. Every preview explicitly states that no real action
was sent. The source does not access network, storage, OS, credentials or a model.

## Files and verification

- `sales.html`, `office.html`: two archetypes
- `prototype.css`, `prototype.js`: shared visual/interaction source
- `scenarios.json`: authoritative ten-state fixture list
- `scenarios.js`: `window.ORG_SCENARIOS = <the same JSON>;`, generated without a
  build tool; the values must match the JSON exactly
- `source-design.test.mjs`: four dependency-free source/shape checks
- `interaction.test.mjs`: twenty-four optional DOM unit cases, not browser rendering

Run `node --test docs/design/organization-client-v0/source-design.test.mjs` and
`node --check docs/design/organization-client-v0/prototype.js` from repo root.
For the DOM suite set `ORG_DESIGN_JSDOM` to the `lib/api.js` of an existing
qualified jsdom26.1.0 installation, then run `node --test
docs/design/organization-client-v0/interaction.test.mjs`. With no such module it
explicitly skips; do not call that PASS. No dependency installation is required
or implied. The suite's dialog shim only tests our handlers/return-focus branch,
not real browser focus trapping, geometry or screenshots.

## Actual visual proof

Local cloud-browser preview returned ERR_BLOCKED_BY_CLIENT. Do not bypass it.
The separately owner-approved D2 hosted harness is being prepared and reviewed;
capture/export is not active by merely serving these files. Only20 approved
synthetic PNGs may be uploaded, at1440px/full-page≤2400px, retained1day.1280/1440
geometry checks produce no additional images. No source/log/trace/video/secret
artifact. The old PR43 approval/guard is unchanged and grants no standing upload.

See [Phase3 UI design](../../superpowers/specs/2026-10-02-organization-client-v0-ui-design.md)
for API/state/accessibility mappings and the pending qualification limits.
