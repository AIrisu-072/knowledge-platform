# Document GUI Integration v0 — Design Amendment 01

- 日付: 2026-10-01 JST
- 状態: **APPROVED — feature-scoped toolchain amendment**
- 対象Frozen Design: `docs/superpowers/specs/2026-09-30-document-gui-integration-v0-design.md`
- Frozen Design blob: `f132910ca5d3e638502f0b38447d9a1ec4020f24`
- 元Production Plan blob: `0830c306ebb38290e4c3dc277f6c97a0759cf912`

## Decision / ADR

G7のfrontend build/test toolchainを次のとおり置き換える。

| Frozen Design selection | G7 implementation selection |
|---|---|
| Vite 8 | Webpack `5.111.1`, webpack-cli `7.2.3`, webpack-dev-server `6.0.0` |
| Vitest 4.1 | Jest `30.5.2`, babel-jest `30.5.2` |
| TypeScript 7 preferred | TypeScript `6.0.3` fallback; generator compatibility check already failed on TS7 |
| unspecified transformer | Babel `7.29.7` presets/core, selected after Jest peer-compatibility check |

The replacement is scoped to Document GUI Integration v0. React 19, TanStack Router/Query/Table/Virtual, Motion, CSS Modules + CSS Custom Properties, Ajv, React Testing Library, Playwright, generated API client, BinaryTransportBridge, and all UI/API/business semantics remain as approved. This amendment does not introduce a shared cross-format content model, frontend document parser, Search Extraction coupling, or changes to authoritative backend behavior.

React Aria Components `1.21.1` remains the preferred primitive after its focused qualification (6/6). G7 still must complete the frontend foundation contracts and focused implementation checks before it is marked complete.

## Candidate-specific license decision

Architecture Contract §5 remains unchanged. The requester explicitly approved individual exceptions for only the licenses present in the current resolved candidate graph, leaving the general policy unchanged. The candidate toolchain lock SHA-256 at approval was `ee2e1430204112a91a31cbfa34a286ab1effca56ac35d918bb3f4df77d05ea16`. G8 then linked the already-selected first-party `@knowledge-platform/document-api-client` workspace package; the current lock SHA-256 is `bb74081c198fcb1c9c8038933c434f0a00b50a2aa7a436c3def15156881d6847`. `pnpm install --filter @knowledge-platform/document-web --frozen-lockfile --ignore-scripts` passed. `pnpm licenses list --json` for the app reports the same third-party package/license graph: 751 package entries and these exact license identifiers:

- ISC (34 packages)
- BlueOak-1.0.0 (8 packages)
- CC-BY-4.0
- Python-2.0
- MIT-0
- Unlicense
- CC0-1.0
- 0BSD
- `(MIT OR CC0-1.0)`

This does not authorize any additional license identifier or third-party package. The first-party workspace link adds no third-party package, and the post-link inventory confirms that the individually approved license IDs remain unchanged. Any added third-party package or new license identifier requires stopping for fresh inventory and approval. The candidate graph contains no GPL, AGPL, LGPL, MPL, SSPL, BSL, or source-available packages. Optional `eslint-plugin-jsx-a11y` and `identity-obj-proxy` remain excluded because their graphs add MPL licensing; equivalent project-specific accessibility coverage is provided by tests and browser checks.

## Qualification evidence and completion boundary

Candidate qualification recorded before implementation: Node `24.21.0` / pnpm `12.4.1` frozen filtered install PASS; peer check PASS; `pnpm audit --audit-level=low` PASS with no known advisories; candidate license inventory contains only the individually approved identifiers above plus the previously allowed identifiers. These checks qualify the candidate selection, not G7 implementation completion.

The original Frozen Design is not edited. This amendment is limited to implementation tooling and the explicitly approved license scope. Product behavior, security boundaries, accessibility/motion acceptance, and G8/G9 acceptance criteria remain unchanged.
