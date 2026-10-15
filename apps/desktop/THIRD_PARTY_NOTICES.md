# Third-party notices: desktop shell

The desktop shell (`apps/desktop/src-tauri`) is built from unmodified upstream
crates. Most are MIT and/or Apache-2.0 licensed; the full list with versions is
in `apps/desktop/src-tauri/Cargo.lock`. The following components are licensed
under the Mozilla Public License 2.0 (MPL-2.0) and were approved for this
lockfile only, at these exact versions (owner decision 2026-10-07,
`docs/decisions/2026-10-07-tauri-v2-desktop-qualification.md`).

| Crate | Version | Role | Source Code Form |
|---|---|---|---|
| option-ext | 0.2.0 | in the executable (via `dirs`) | https://crates.io/crates/option-ext/0.2.0 |
| cssparser | 0.37.0 | build time only (tauri-macros/tauri-codegen) | https://crates.io/crates/cssparser/0.37.0 |
| cssparser-macros | 0.7.1 | build time only | https://crates.io/crates/cssparser-macros/0.7.1 |
| selectors | 0.38.0 | build time only (tauri-macros/tauri-codegen) | https://crates.io/crates/selectors/0.38.0 |
| dtoa-short | 0.3.5 | build time only | https://crates.io/crates/dtoa-short/0.3.5 |

These files are used without modification. Under MPL-2.0 their Source Code
Form is available from the locations above; the license text is at
https://mozilla.org/MPL/2.0/.

This file covers only the MPL-2.0 obligations. It is **not** a complete notice
for a distributed executable: the executable also contains MIT, Apache-2.0,
Unicode-3.0, BSD-3-Clause and Zlib licensed crates whose licence texts and
copyright notices must accompany binary copies. No desktop executable is
distributed today; before the first distribution, generate the full
third-party licence bundle for the executable's normal dependency graph (as
`apps/document-mcp/third-party-notices/` does for that artifact) and ship it
together with this file.

`target-lexicon 0.12.16` (Apache-2.0 WITH LLVM-exception) is a Linux build-time
dependency only and is not part of the built executable.

The Windows build uses the Microsoft Edge WebView2 Runtime already installed on
the device (Evergreen). The Runtime is not bundled or redistributed.
