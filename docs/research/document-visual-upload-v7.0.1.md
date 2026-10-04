# Official upload-artifact v7.0.1: bounded adoption evidence

Observed: 2026-10-02 UTC. Decision: [bounded adoption ADR](../decisions/2026-10-02-document-visual-upload-bounded-adoption.md). **Activation, final CI/runtime qualification and actual pixel review remain pending.**

## Artifact and method

The sole action is `actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` (v7.0.1). Its [metadata](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/action.yml) runs the committed Node 24 bundle, without installing upstream development dependencies. The [root LICENSE](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/LICENSE) is MIT. This review uses the official [manifest](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/package.json), [lock](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/package-lock.json), and [upload bundle](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/dist/upload/index.js).

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Upstream package-lock.json | 351,963 | `3cbe59ae3fe81770fd3d9128093087c5581fff9474b0d0bc6d3d34bda1ff1a7e` |
| Committed dist/upload/index.js | 4,450,352 | `eea594941d8ee535974e0fbc03bbdf567f3abc78194f224b93f2df9a887ee2e9` |

The prior static qualification retained a Node 24/npm 11.19 package-lock-only audit with ignore-scripts and separate empty npmrc files, all 51 primary GHSA records, and an integrity-verified source inventory of all 156 non-development lock nodes / 151 unique packages. Thirty-seven decision-relevant packages received targeted static source/bundle comparison. Shared source can match multiple versions: a positive overlap does not identify a separate bundled version, and no hit does not prove absence. No install, lifecycle hook, upstream build, action execution, exploit probe or complete source-to-bundle rebuild was performed. Preparing this ADR does not rerun the registry audit or upgrade historical observations to new executable evidence.

The [machine receipt](document-visual-upload-v7.0.1/qualification.json) binds the immutable upstream artifacts, complete non-development graph identities, exact exception graph, shipped license notices, all 78 reconciled advisory rows, and buffers archival receipts. Evidence hashes identify the earlier raw audit/inventory/comparison records; primary source URLs and exact package integrity values support reproduction. Any old evidence wording that says no bounded exception was approved or buffers remained unknown describes the earlier pre-decision state; the ADR and the recovered provenance below supersede those two conclusions only.

## License source review

The ADR grants exactly 13 ISC and 5 BlueOak-1.0.0 exceptions in this action graph. Their complete shipped notices are retained in the JSON receipt with their original archive paths and SHA-256 values. Preserve them if redistributing corresponding copies or substantial portions. The action's own license declaration or [reviewed-dependencies configuration](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/.licensed.yml) is supporting evidence, not a substitute for each package's grant.

Known bundled examples include `minimatch@3.1.5` in actual file selection, `minimatch@5.1.8`, `glob@10.5.0` / `minimatch@9.0.8`, `graceful-fs@4.2.11`, `inherits@2.0.4`, `lru-cache@10.4.3`, `universal-user-agent@7.0.3`, `minipass@7.1.3` and `path-scurry@1.11.1`. Some CLI/build helpers are not established as bundled. The conservative lock-graph exception does not claim every entry executes or distinguish minimatch 9 from 10 by common code alone.

`tslib@2.8.1` ships an unconditional 0BSD grant, classified under the existing Public-Domain-equivalent category. `traverse@0.3.9` ships full MIT text under the legacy MIT/X11 label; `chainsaw@0.1.0` declares MIT/X11 in its manifest but omits a standalone notice. `binary@0.3.0` declares MIT in its manifest and README. Other missing standalone notices remain disclosed in the graph inventory, without silently classifying every metadata-only package as unlicensed. No general allowlist is expanded.

## buffers provenance

The earlier unknown-license finding is closed for this exact source by an authoritative retained redistributor record, with these limits:

1. The [official Debian original archive](https://deb.debian.org/debian/pool/main/n/node-buffers/node-buffers_0.1.1.orig.tar.gz) and saved [npm archive](https://registry.npmjs.org/buffers/-/buffers-0.1.1.tgz) are byte-for-byte identical: 4,195 bytes, SHA-256 `f8de49c60e467005182d9687b5d87775bbcec6385ac63220b7741327441deb6d`, SHA-1 `b24579c3bed4d6d396aeee6d9a8ae7f5482ab7bb`. All six unpacked files match.
2. The 7,057-byte `index.js`, SHA-256 `97801e296ba5f3c32242f647e1daac5917e300367fed7935ab1e62f40adb1b8a`, appears verbatim once in official bundled module 6627, byte offset 606901. This is exact source identity, not a partial fingerprint or package-name inference.
3. The [paired 0.1.1-5 Debian packaging archive](https://deb.debian.org/debian/pool/main/n/node-buffers/node-buffers_0.1.1-5.debian.tar.xz), SHA-256 `53a5f11cf2ee466b27f60409caed8fb9c4bbfe5fecac28485d11c675e882089f`, supplies `debian/copyright`. It applies Expat to `Files: *`, attributes James Halliday and includes the full MIT-form permission, notice-retention condition and disclaimer. Its only source patch changes package-main and README command spelling, not runtime `index.js`. The [older 0.1.1-2 copyright record](https://sources.debian.org/copyright/license/node-buffers/0.1.1-2/) calls the same grant MIT.
4. The [official source descriptor](https://deb.debian.org/debian/pool/main/n/node-buffers/node-buffers_0.1.1-5.dsc), SHA-256 `6324b0a7ae1fecead640afadec1ef616e07af0e005a71ae135004ba42c28f6a5`, binds both archives by matching size/hash. Its PGP signature is present but was not cryptographically verified. The provenance basis is official Debian HTTPS retrieval plus matching hashes.
5. The original [upstream licensing commit](https://github.com/substack/node-buffers/commit/1b745ee35d33eb166e15ef1866073a07c6d7de87) and original repository/license reads returned 404 during qualification. The npm/orig archive still lacks a standalone notice and license field. Debian's record is not the absent author's independently authenticated declaration, and an owner policy exception cannot create copyright permission.

The resulting classification is **MIT/Expat, evidenced by Debian's retained source-package record for the exact npm source**. The [complete recovered copyright text](document-visual-upload-v7.0.1/buffers-0.1.1-debian-copyright.txt), SHA-256 `7ed63688e5bc3c442bcebf756f04a4be5337a51ec8d4b51804349977a9667f0a`, is preserved byte-for-byte alongside the machine verification. This corrects the earlier unknown finding without creating a new license family or claiming a legal guarantee.

## Security evidence and limits

Raw npm audit: 15 vulnerable package groups over 703 dependencies (156 production / 547 development), including three critical groups. Correct exact-node/version range matching removes 148 invalid node/advisory combinations and retains 78 rows / 51 unique advisories. Forty-two non-development rows cover 28 unique advisories; 36 development rows cover 30, with seven shared across the sets. The critical `concurrently`, `handlebars` and `shell-quote` groups are upstream development-only, relevant to publisher/build provenance but not evidence that those critical APIs execute during this committed upload.

The affected non-development versions are `brace-expansion@1.1.12`, `2.0.2`, `5.0.3`; `fast-xml-builder@1.0.0`; `fast-xml-parser@5.4.1`; `lodash@4.17.23`; and `undici@6.23.0`. Every primary advisory URL, exact affected range, first patched version, classification and static bundle-line reference is retained in the receipt.

| Component | Bounded source finding | Limit / primary advisory examples |
|---|---|---|
| Brace expansion | Literal approved file paths, `nobrace: true`, and per-file `zip.file` avoid attacker-controlled brace patterns | Scope-dependent trigger exclusion; [GHSA-3jxr-9vmj-r5cp](https://github.com/advisories/GHSA-3jxr-9vmj-r5cp), [GHSA-6j4f-fj2g-mc7p](https://github.com/advisories/GHSA-6j4f-fj2g-mc7p) remain version matches |
| XML builder | Default `processEntities: true`; UUID/counter block IDs rather than user XML/CDATA/comments | Configuration/input mitigation for [GHSA-5wm8-gmm8-39j9](https://github.com/advisories/GHSA-5wm8-gmm8-39j9) |
| XML parser | Azure response parsing remains present; zero-limit advisory configuration is absent and other special input triggers are not supplied | **High [GHSA-8gc5-j5rx-235r](https://github.com/advisories/GHSA-8gc5-j5rx-235r) remains conditionally reachable.** [GHSA-jp2q-39xq-3w4g](https://github.com/advisories/GHSA-jp2q-39xq-3w4g) and [GHSA-gh4j-gqv2-49f6](https://github.com/advisories/GHSA-gh4j-gqv2-49f6) retain separate trigger assessments |
| Lodash | Selected submodules are present; affected template/unset/omit code and distinctive supporting symbols were not identified | Static scope assessment, not proof of package-wide safety; [GHSA-f23m-r3pf-42rh](https://github.com/advisories/GHSA-f23m-r3pf-42rh), [GHSA-r5fr-rjxr-66jc](https://github.com/advisories/GHSA-r5fr-rjxr-66jc) |
| Undici | Selected upload uses Actions Node HTTP and Azure Blob clients; vulnerable WebSocket/cookie/retry/helper APIs are not called, and optional REST get/download/delete paths stay unselected with `overwrite: false` | A proxy dispatcher can initialize; do not claim all Undici is unexecuted. Examples: [GHSA-v9p9-hfj2-hcw8](https://github.com/advisories/GHSA-v9p9-hfj2-hcw8), [GHSA-rfgv-xxqx-mfg5](https://github.com/advisories/GHSA-rfgv-xxqx-mfg5) |

For GHSA-8gc5-j5rx-235r, the retained primary range is `>= 5.0.0, < 5.5.6`; 5.5.6 is first patched. Bundled 5.4.1 reaches XML parsing through Azure response deserialization with `processEntities: true` (bundle lines 91649, 95579, 95601, 98308, 123913). A malicious/compromised service, endpoint or trusted proxy could exhaust CPU/memory. Fixed PNGs constrain uploaded content, not remote XML responses. The bounded ADR accepts that disclosed residual exposure; no exploit, malicious response or dynamic action test was performed. A local change to the upstream bundle would create a different action and is not covered by this decision.

The audit is public-upstream dependency evidence. It does not replace final repository license/advisory/secret checks, `actionlint`, `zizmor`, exact-head runtime acceptance or pixel inspection, and it does not authorize a mandatory-check bypass.

## Fixed visual scope and activation gates

The approved filenames are:

1. `01-list-context-1440.png`
2. `02-list-focus-return-1280.png`
3. `03-detail-overview-1440.png`
4. `04-revision-version-1440.png`
5. `05-comparison-1440.png`
6. `06-version-file-selected-1440.png`
7. `07-publication-ready-1440.png`
8. `08-publication-confirm-focus-1440.png`
9. `09-publication-success-1440.png`
10. `10-access-policy-effective-draft-1440.png`
11. `11-occ-conflict-1440.png`
12. `12-permission-denied-file-retained-1440.png`
13. `13-permission-restored-retry-success-1440.png`

Static qualification inspected capture/export candidate `f4f091225d3dcacc438e6c6140a723453c2d7f4a`: owned disposable DB only, no external DB/prebuilt mode, real-journey checkpoints, clean source head, acceptance and cleanup success, exact file set, private regular single-link files/directories, no symlinks/hardlinks, bounded PNG/pixel-format validation and no ancillary chunks. Validated bytes enter fresh private staging before atomic export. Failed/partial runs export nothing; automatic screenshot/trace/video reports are disabled in visual mode. These are historical source-review findings that must be preserved and rechecked on the final integrated candidate, not a claim that capture is now integrated or executed.

The action can follow symlinks and implicit descendants. Export guards address that risk on a fresh trusted runner; hostile same-UID runner code is outside their boundary. `include-hidden-files: true` is needed only for the fixed `.state` ancestor. The actual workflow must enumerate the 13 literal paths, use `if-no-files-found: error`, `overwrite: false`, and request `retention-days: 1`. A broad directory or glob is not approved.

The earlier inert patch enabled capture/upload on every runtime-job invocation. That would exceed current-review authority: general `pull_request`/push triggers and future reruns must remain default-off. Before activation, independently review a gate restricted to current PR #43, the reviewed exact source and approved run/attempt. It must require successful runtime acceptance, validation/export and cleanup, and fail closed outside that scope. No workflow is changed by this documentation commit.

The action uses existing short-lived `ACTIONS_RUNTIME_TOKEN` / `ACTIONS_RESULTS_URL` support; no new secret, persistent credential or `actions: write` permission is needed. Preserve `contents: read` and checkout `persist-credentials: false`. Retention implementation at bundle lines 84118–84143 sends explicit one-day expiry and only reduces it for a smaller repository maximum. Unknown default retention does not justify omitting the explicit input.

Actions administration/allowlist/default settings remain unknown because the supported connector did not permit that settings endpoint family. No alternate route or settings change is inferred. The allowlist may reject execution; that uncertainty must remain visible and fail closed. This report does not claim it passed or that final CI is qualified.

After activation, retain a text-only receipt identifying exact source/action SHA, run/attempt, artifact ID/digest, validated 13-file list, and actual creation/expiry timestamps. Public-repository artifact access is for signed-in readers, as described by the pinned action metadata. Storage is limited to the approved one day, with no generated PNG commit, no Library mirror and temporary local review copies removed within that period. Inspect the actual pixels of all 13 images and explicitly record defects and limits. DOM checks, PNG parsing and storage success alone cannot close human visual/usability review.

## Current completion boundary

Recorded: authorized pin-specific ISC/BlueOak decision; nonzero-audit and residual-risk disposition; recovered buffers MIT/Expat evidence; retained immutable sources, hashes, grants, findings and current-review constraints. This documentation candidate starts at `34667b891963add0ffebd8be64269137f97ee163` and does not assert a current hosted PR head.

Pending: PDF-display repair and source integration, final default-off current-review gate, independent review, final exact-head required CI/security/runtime gates, allowed-action execution or fail-closed handling, actual one-day artifact receipt, and actual pixel inspection. No package installation, Rust workload, workflow/action execution, artifact upload or publication was performed for this documentation change.
