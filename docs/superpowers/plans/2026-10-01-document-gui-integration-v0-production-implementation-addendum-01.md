# Document GUI Integration v0 — Production Plan Addendum 01

- 日付: 2026-10-01 JST
- 状態: **APPROVED — applies to G7 only**
- 元Plan: `docs/superpowers/plans/2026-09-30-document-gui-integration-v0-production-implementation.md`
- Amendment: `docs/superpowers/specs/2026-10-01-document-gui-integration-v0-design-amendment-01.md`

G7 toolchain implementation uses Webpack `5.111.1`, webpack-cli `7.2.3`, webpack-dev-server `6.0.0`, Jest/babel-jest `30.5.2`, Babel `7.29.7`, and TypeScript `6.0.3`. Keep the originally planned React/TanStack/Motion/Ajv/React Aria/RTL/Playwright layers and frontend boundaries.

G7 order:

1. Port the existing G7 contract tests from Vitest to Jest without changing assertions or design semantics; add the minimal Jest/Babel harness required to run them and expose the still-missing frontend contracts/implementation.
2. Complete Webpack/PostCSS configuration, remove stale Vite/Vitest type references, and implement the design tokens, motion tokens, shell, and architecture boundaries covered by those contracts.
3. Run focused Jest, TypeScript, Webpack production-build, and local dev-server smoke checks. G7 is complete only when its contracts pass and the frontend foundation builds.
4. Proceed to G8 Mock 1–7 only after G7 is locally green. Per requester direction, defer hosted CI to the final exact-head G9 gate; do not run CI between G7/G8 tasks.

License approval is limited to the exact resolved candidate lock recorded in Design Amendment 01. Do not add packages or change lock resolution without re-running the dependency inventory and obtaining any newly required individual approval. Do not merge or deploy the product implementation PR.
