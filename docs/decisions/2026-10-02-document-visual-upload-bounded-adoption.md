# ADR: bounded official uploader adoption for the PR #43 visual review

Date: 2026-10-02 UTC

Status: **BOUNDED ADOPTION APPROVED; ACTIVATION AND FINAL QUALIFICATION PENDING**

## Decision

The authorized decision permits the official `actions/upload-artifact` **v7.0.1 only**, pinned to `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`, for the remaining [PR #43](https://github.com/AIrisu-072/knowledge-platform/pull/43) visual review. It approves the exact 13 ISC and 5 BlueOak-1.0.0 packages below and accepts the disclosed residual malicious-storage-response denial-of-service risk for this bounded use. It does not change the general license allowlist, waive required scanning, or qualify a final CI run.

This is the explicit, graph-specific ADR exception required by [Selection §2.1–2.2](../../spec/selection/library-tool-selection-v0.md#21-license). [Development/Container/CI §37–39](../../spec/architecture/development-container-ci-architecture-v0.md#37-supply-chain-security) and [Architecture LINT-02/LINT-03](../../spec/architecture/architecture-contract-v0.md#lint-02-license-gate) continue to apply. The earlier MCP client exception supplies no authority for this different graph.

The immutable [action metadata](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/action.yml), [MIT license](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/LICENSE), [upstream lock](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/package-lock.json), and [committed upload bundle](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/dist/upload/index.js) define the approved artifact. Lock SHA-256: `3cbe59ae3fe81770fd3d9128093087c5581fff9474b0d0bc6d3d34bda1ff1a7e`. Bundle SHA-256: `eea594941d8ee535974e0fbc03bbdf567f3abc78194f224b93f2df9a887ee2e9`. A mutable version tag, changed bundle, dependency upgrade, alternative uploader, or additional non-allowlisted package requires renewed qualification and an applicable decision.

## Exact license exception

These are exact package/version entries from the pinned action's non-development lock graph. The entire set is a conservative upper bound; membership does not assert that every package is bundled or executes. Lock node paths, registry sources, integrity values, full shipped notices and their hashes are retained in the [qualification receipt](../research/document-visual-upload-v7.0.1/qualification.json).

| License | Exact package versions |
|---|---|
| ISC (13) | `@isaacs/cliui@8.0.2`, `foreground-child@3.3.1`, `glob@10.5.0`, `graceful-fs@4.2.11`, `inherits@2.0.4`, `isexe@2.0.0`, `lru-cache@10.4.3`, `minimatch@3.1.5`, `minimatch@5.1.8`, `minimatch@9.0.8`, `signal-exit@4.1.0`, `universal-user-agent@7.0.3`, `which@2.0.2` |
| BlueOak-1.0.0 (5) | `jackspeak@3.4.3`, `minimatch@10.2.4`, `minipass@7.1.3`, `package-json-from-dist@1.0.1`, `path-scurry@1.11.1` |

Preserve copyright, permission and disclaimer notices. This exception applies only within the identified action graph and current review. It does not permit the same packages in product/runtime dependencies or other workflows. `tslib@2.8.1` ships 0BSD, classified within the existing Public-Domain-equivalent category; it is disclosed separately and adds no general allowlist entry. The legacy MIT/X11 declarations in `traverse@0.3.9` and `chainsaw@0.1.0`, including the latter's missing standalone notice, remain recorded in the receipt.

## Recovered buffers provenance

The earlier unknown-license finding for `buffers@0.1.1` is superseded by exact-byte archival evidence. [Debian's upstream archive](https://deb.debian.org/debian/pool/main/n/node-buffers/node-buffers_0.1.1.orig.tar.gz) is byte-identical to the npm archive, and all six unpacked files match. Its entire 7,057-byte `index.js` occurs verbatim once in bundled module 6627. The paired [Debian packaging archive](https://deb.debian.org/debian/pool/main/n/node-buffers/node-buffers_0.1.1-5.debian.tar.xz) retains a version-specific MIT/Expat grant and upstream attribution for `Files: *`. The complete [recovered copyright notice](../research/document-visual-upload-v7.0.1/buffers-0.1.1-debian-copyright.txt) is retained unchanged.

Classification: **MIT/Expat, evidenced by Debian's retained source-package record for the identical source**. MIT is already allowed; no new buffers license exception is created. The npm/original archive still has neither a license field nor standalone notice. The original GitHub repository/licensing-commit reads returned 404; the author's original declaration was not independently recovered. The [Debian source descriptor](https://deb.debian.org/debian/pool/main/n/node-buffers/node-buffers_0.1.1-5.dsc) hashes match both archives, but its PGP signature was **not cryptographically verified**. Provenance rests on official Debian HTTPS retrieval, descriptor hashes and exact source identity; it does not authenticate the unavailable original author declaration. [Detailed evidence and limits](../research/document-visual-upload-v7.0.1.md#buffers-provenance).

## Security disposition

The scan is nonzero. Exact-version reconciliation retained 78 package/advisory rows: 42 non-development rows across 28 advisories and 36 development rows. Static source review assessed the bounded upload's inputs and selected APIs. Development-only critical groups do not establish critical vulnerabilities executed by the committed uploader. All findings remain visible in the receipt; no scanner exclusions, dependency overrides or modified upstream bundle are introduced.

**Residual high risk is accepted for this bounded review:** `fast-xml-parser@5.4.1` is affected by [GHSA-8gc5-j5rx-235r / CVE-2026-33036](https://github.com/advisories/GHSA-8gc5-j5rx-235r), CVSS 7.5, first patched in 5.5.6. Azure upload-response deserialization reaches XML parsing with entity processing. A malicious or compromised service, endpoint or trusted proxy could cause CPU/memory exhaustion through numeric entity references. Thirteen fixed PNG inputs do not remove the Azure XML response parser. No attacker control, exploit or resulting impact was demonstrated, and this decision does not call the risk eliminated.

Other scoped mitigations are fixed literal paths with brace expansion disabled; generated block-list IDs; existing XML-builder/parser settings; selected Lodash submodules; and avoidance of the advisory-specific Undici WebSocket/cookie/retry APIs in the chosen upload flow. Those are static applicability findings with limitations, not package-wide security guarantees. See the [source-audit report](../research/document-visual-upload-v7.0.1.md#security-evidence-and-limits). The bounded decision does not authorize bypassing any mandatory final repository security gate.

## Storage, execution and review boundary

- Current PR #43 review only, using exactly the 13 named synthetic PNG checkpoints in the report. Capture remains default-off. A separately reviewed activation gate must bind the approved source and run/attempt, fail closed outside that scope, and require successful acceptance/export. Ordinary future pull-request, push or rerun activity must not enable storage automatically. This ADR does not implement that gate.
- Use GitHub-hosted execution and the official pinned action's existing short-lived runtime credentials. Preserve `contents: read` and checkout `persist-credentials: false`; add no permissions, persistent credentials or secrets. No custom uploader or administrative-settings workaround is approved.
- Request `retention-days: 1`, `overwrite: false` and an exact literal file list. GitHub Actions is the approved storage destination; signed-in readers of the public repository can access the artifact. No generated image commits or long-lived Library mirror. Temporary local review copies stay within the approved one-day period.
- Preserve the owned disposable database, trusted fresh runner, private regular single-link export files, no symlinks, exact filename/count/PNG validation, successful cleanup, and atomic export guards. Validate all 13 files; `if-no-files-found: error` alone is insufficient. Hidden-file inclusion is only for the fixed `.state` ancestor and must not broaden paths to a directory or wildcard.
- Repository Actions administration/allowlist settings remain **unknown** after the supported settings-read boundary. This decision does not assert administrative allowlisting, change settings, or treat a rejection as permission to bypass it. Execution may fail closed on those settings.
- After reviewed activation and execution, record exact source/action pins, workflow run/attempt, artifact ID/digest, 13-file validation, and actual `created_at`/`expires_at`. Inspect the actual pixels for every checkpoint and report usability/visual defects before claiming the visual review complete. Passing DOM assertions, PNG structure checks or artifact creation is insufficient.

## Remaining gates and handoff

This ADR records the authorized bounded decision only. Capture/export implementation, its current-review-only activation gate, independent review, required exact-head CI/runtime/security checks, actual storage receipt and actual pixel inspection remain separately evidenced in C3 Status. The record itself changes no general policy allowlist and establishes no action execution, upload or runtime acceptance result.

The parent's next action is to review the final bounded activation candidate on the reviewed PDF-display repair and verify its exact-head gates. Any changed audience, selected files, retention, permission, action pin, dependency exception graph or residual-risk premise requires a new applicable decision before proceeding.
