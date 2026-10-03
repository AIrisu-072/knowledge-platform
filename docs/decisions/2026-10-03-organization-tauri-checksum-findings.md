# Exact Tauri checksum-finding exceptions

Date: 2026-10-03 UTC. Status: **OWNER APPROVED; PATCH REVIEW / HOSTED REQUALIFICATION PENDING**.

## Authority and exact scope

Assistant question Sentinel_9ad83992c9c08191a0ac482c945a6f8e,
2026-10-03T10:12:48Z:

> TauriのCIで、秘密情報検査が5箇所を検出しました。確認したところ、すべて公開ライセンス文書や依存ファイルのSHA-256値で、認証情報ではありませんでした
>
> 今回のコミット・ファイル・行に固定した4つの識別子だけを、検査の限定除外へ追加してよいですか？ ファイル全体や検出ルール全体は除外しません

Owner reply Sentinel_4746e698b9d88191a6303c512aaa0807,
2026-10-03T12:14 UTC:

> 除外していいです、

Apply only four immutable commit/path/rule/line fingerprints in .gitleaksignore,
covering the five independently verified data digests introduced by public PR52
commit0802fe6de30c9491c0fe459c57b2c56641f027d9/tree17f35a26b3aea994af6860eccc296cc7ce3f88e3.
The existing31 entries remain byte-for-byte unchanged; .gitleaks.toml, detector
rules, workflow/task commands, source/dependency/license policy and locks remain
unchanged. No whole commit/file/path/rule exclusion or future finding permission.

The exact four entries are the three generic-api-key fingerprints for
`docs/research/organization-tauri-windows-inventory/runtime-terms-receipt.json`
lines10/21/32 and the generic-api-key fingerprint for
`docs/research/organization-tauri-windows-inventory/inventory/inactive_or_other_target-007.json`
line1, all bound to the complete introducing commit above. Machine-readable exact
entries and regression outcomes are in the [scope receipt](../research/organization-tauri-windows-inventory/gitleaks-scope-receipt.json).
A fingerprint does not include a value/column; the last entry covers the two
reviewed matches on that single immutable line. A new commit at the same location
is not covered. No credential was identified among these five findings.

## Independent provenance

The pinned official Gitleaks8.30.1 report maps the three Runtime findings to
agreement-response digest fields. Captured raw responses and fresh official
Microsoft readbacks recompute identically; their decoded HTML digests also agree.
These are public document hashes, not API credentials or Runtime acceptance:

- [Evergreen agreement response](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us)
- [Fixed agreement response](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us&fixed=true)
- [Consumer agreement response](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us&consumer=true)

The other two findings are member hashes for lib/libwinapi_oemlicense.a in
winapi-i686-pc-windows-gnu0.4.0 and winapi-x86_64-pc-windows-gnu0.4.0. Both official
registry archive SHA256 values match the committed Cargo.lock before member hashes
are recomputed. The members are GNU ar binaries,3322/3328 bytes, in inactive/other-
target packages with no selected role or occurrence. The existing notice_hashes
field is a filename-regex inventory label, not proof that these binaries contain
license-notice prose. No package/native code was executed. See the bounded
[provenance receipt](../research/organization-tauri-windows-inventory/gitleaks-checksum-provenance.json).

Independent review at10:16UTC concluded GO for this bounded decision scope only;
all five spans matched the verified public-data digests. It did not itself grant
suppression authority. The owner approval above subsequently granted only these
four entries. The former failure and original public inventory remain intact;
no history rewriting, hiding, renaming or deletion is used to clear the check.

## Verification and remaining gates

[Reproduction helper](../../tools/organization-tauri-qualification/verify_gitleaks_scope.py)
requires the exact official prebuilt binary SHA256 and checks31+4 entries plus
unchanged rule bytes before scanning. The self-test source is bound to the exact
subject task, and its executable name must match the verified binary; inherited
shell startup/function overrides are not used in the isolated test process. It
runs no compiler or Tauri code. It verifies
original5 → approved0 → restored5 on the exact public subject delta, new-path and
same-path/line new-commit generic-key negatives, a distinct PAT-shaped negative,
and the unchanged repository scanner self-test. Output/report paths never print
candidate values, matches or synthetic fixture contents.

Gitleaks reads both its explicit ignore input and the source's implicit ignore
file. Therefore the reversal uses an isolated Git object view with controlled
ignore bytes, not an override argument that still silently loads the approved
working-tree entries. Source objects are reused read-only, with no remote or lazy
fetching; unrelated unpublished local branches are not imported into that view.

The earlier optional full-ancestry attempt reached missing historical objects in
the shared partial clone; Git HTTPS lazy retrieval failed with Proxy CONNECT
aborted. It was interrupted, not counted as passing. The helper retries full
subject ancestry without network and explicitly records failure/incompletion,
never treats an empty report after a scanner error as success. Fresh hosted full-
repository history and downstream security/CI-lint gates remain mandatory before
H1 implementation. A local subject-only result does not replace other public refs.

Original PR52 CI37114872963 failed security and aggregate required-check; every
other CI job succeeded. DSI37114872993 and Sandbox37114873016 succeeded; D2 skipped.
The security task stopped before downstream dependency and CI-lint stages. This
patch requires independent review, parent/worker connector publication to the same
Draft, remote-tree equality and fresh exact-head hosted checks. Runtime agreement
acceptance/use, Windows H1 activation and all broader native/license/security STOPs
remain unchanged.

Initial scope regression failed before the approved four entries existed. Review
also found that an unbound dynamic self-test could incorrectly report unchanged
PASS after replacement with true, or select a sibling executable after renaming
the verified input. Two focused tests failed before those guards were added; all
31 stdlib tests now pass. The guarded detector replay, negative controls and
unchanged self-test also pass; full local history remains explicitly blocked and
requires the hosted gate. No additional exception followed from these test fixes.
