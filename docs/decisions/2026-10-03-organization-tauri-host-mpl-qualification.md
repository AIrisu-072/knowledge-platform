# ADR: bounded Tauri host-build MPL qualification

Date:2026-10-03 UTC. Status: **ORIGINAL THREE-PACKAGE SCOPE APPROVED; ADDITIONAL SCOPE PENDING**.
This is a newly reconstructed document. It is not a recovered byte-identical copy
of the unpublished packet lost when the execution filesystem reverted.

## Exact original authority

Assistant question `Sentinel_e9d65db3a324819189a253db3c5fe965`,
2026-10-02T17:46:14Z, asked to qualify a build-use license exception limited to
cssparser0.37.0, selectors0.38.0 and cssparser-macros0.7.0. It required full-graph,
output-inclusion, notices/source-obligation examination and another STOP if scope
increased. Production adoption and distribution were expressly excluded.

Owner reply `Sentinel_ed91310d31588191bdac163df3512ec7`,2026-10-03 00:01UTC:

> Tauriライセンスは限定許可します

This is the explicit narrow ADR authority required by
[selection policy§2.1](../../spec/selection/library-tool-selection-v0.md).
The exception is the intersection of these exact package versions with Phase4
host build/proc-macro qualification. No global MPL allowance, other version,
normal tool/runtime use, production adoption or distribution is approved.

| Package | Exact version | Scope |
|---|---|---|
| cssparser |0.37.0|host build/proc-macro qualification only |
| selectors |0.38.0|same |
| cssparser-macros |0.7.0|same |

The existing [preflight](../superpowers/execution/organization-client-tauri-v2-phase4-preflight.md)
remains unchanged historical pending-state evidence. Its required Tauri2.12.1
build-2→dom_query branch is not a complete application inventory.

## Additional question is pending, not permission

The parent reports sending question `Sentinel_59383b2800ac8191a2fe76e8e0e22d0a`
at00:56UTC,2026-10-03: dtoa-short0.3.5 host-only and option-ext0.2.0 host plus normal
target qualification; recommended Windows-first/direct-Cargo route, excluding
Linux exceptions, production and distribution. **No reply has been received in
this checkpoint.** No action is authorized from that pending question.

## Preserved gates

No repository deny.toml/license allowance, advisory severity, native-Windows flag,
CSP, command capability or production dependency changes are made. Unknown/new
package/version/use, unverified source, failed advisory or native license gate
returns execution to STOP. The original§49 conditions remain; original§50 faithful
formalization authority does not waive them.

Before any later permitted build, restore/recreate and independently review the
exact selected host/target/tool/native inventory with lock checksums, source and
notice hashes, target/feature/kind classification and current advisories. The old
unpublished inventory is unavailable and must not be treated as recovered proof.
Actual generated-file/linkage/artifact classification remains outstanding.

Preserve all applicable notices and identify covered-source/source-availability
obligations before any separately proposed distribution. See Mozilla's
[MPL text](https://www.mozilla.org/en-US/MPL/2.0/) and
[FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/). This ADR does not supply legal
approval, accept third-party terms or grant production/distribution permission.

Current next action: review and durably publish the documentation-only
[recovery/STOP record](../research/organization-tauri-qualification-recovery.md),
then await the exact pending decision. No build or runtime work resumes here.
