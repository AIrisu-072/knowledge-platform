# Fresh Windows candidate inventory receipt

Date: 2026-10-03 UTC. Status: **CAPTURE VERIFIED; PUBLIC PR52; INACTIVE-ADVISORY POLICY STOP; BUILD/RUNTIME HELD**.

This is new reproducible evidence above actual PR51, not recovery of unpublished
b4a bytes. The [exact approval amendment](../../decisions/2026-10-03-organization-tauri-windows-mpl-amendment.md)
and independently reviewed b33c0f2/tree265bcb29 authorize this first slice only.
All underlying raw data are available locally for review; durable receipts/lock/
validation code permit reproduction without publishing third-party binaries/source
or raw logs. [Reproduction instructions](../../../tools/organization-tauri-qualification/README.md)
use official pinned tools and explicit inert manifest paths.

Historical publication note: the three original local packets were superseded
without individual remote publication. Public PR52 now preserves their combined
evidence and STOPs; the [repack record](../../superpowers/execution/organization-tauri-sharded-publication-review.md)
records exact superseded local identities and the new lossless storage verification.
The historical active pointer remains byte-identical; its earlier publication steps
are superseded by this single combined publication, not by new runtime authority.

## Advisory coverage correction, 2026-10-03 UTC

Public PR52 now includes the exact owner-approved four checksum fingerprints.
Hosted full-history Gitleaks and root cargo-deny pass, but recursive OSV2.5.1
reports glib0.18.5 RUSTSEC-2024-0429 and proc-macro-error1.0.4 RUSTSEC-2024-0370
in the inert lock. Both are inactive in the recorded Windows projection; neither
is thereby globally safe or approved for Linux/native execution. No advisory
exception is applied. See the [pending decision packet](../organization-tauri-inactive-advisory-decision.md).

The original all-target cargo-deny result below remains unchanged. Its target
coverage was broad, but its default informational-unsound scope was only direct
workspace dependencies; glib is transitive. Unmaintained defaults to all. Thus
“all-target” was not exhaustive transitive-unsound coverage, and zero diagnostics
under the scoped policy were not proof that no other advisories existed. The
original scan receipts/logs and package coverage stay intact; this correction
records the previously unstated reporting limit rather than rewriting evidence.

## Fresh results

Current official stable core remains Tauri2.12.1, released2026-09-30, upstream
30da1fd6e17de6107ecc850c95dfb16b5729f2dd, freshly checked02:20UTC against
[official release metadata](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.1).
SemVer3 alpha entries are not stable v2 even when a release metadata flag says otherwise.

- Final lock SHA256 d47b06511f830c3bd1ef4f0ce828db9162c1807a6d3563887c4f3c8d2073bcef,
  with406 registry entries. Initial1bb00cc4 was preserved. The only corrective
  lock delta pins approved cssparser-macros0.7.0 instead of unapproved0.7.1 and
  updates its checksum/syn edge; neither version was executed.
- All406 official registry archive hashes matched before Cargo extraction. All
  22,054 extracted regular files matched archive bytes. The216 selected registry
  packages account for8,552 regular files. Per-package member-map digests and
  notice hashes are in [lossless inventory index](inventory/index.json), not archive/source mirrors.
- Actual host is Linux x86_64; target projection is x86_64-pc-windows-msvc. Selected
  roles:91 host-only,88 both,37 target-only and190 inactive/other-target registry
  entries. Thus179 host and125 target registry entries overlap. The inert fixture
  adds one target node. Its independently captured normal/no-proc set matches
  exactly; every selected node has metadata/source evidence.
- Approved cssparser0.37.0/selectors0.38.0/cssparser-macros0.7.0/dtoa-short0.3.5
  are host-only. option-ext0.2.0 is both host and target. No new exception was
  added. Metadata union features and scanner inclusion diagrams are not role proof.
- Windows scanner covers222 registry packages, including all216 selected and six
  extra host-conditional candidates. Conservative all-target scanner covers378,
  also including every selected node. [Coverage reconciliation](scanner-coverage.json)
  leaves no selected host gap; the extra nodes do not establish native-Windows proof.
- Root raw Windows scan exits4 for exactly the five approved MPL names, with zero
  advisory/source errors. Scoped exact-version Windows scan exits0, with zero
  advisory/license/source errors; nine duplicate-version and six unused policy
  warnings remain. Conservative root all-target scan exits5: six license errors
  (five MPL plus target-lexicon0.12.16) and proc-macro-error1.0.4 unmaintained
  RUSTSEC-2024-0370. Those two additional packages are inactive in this projection.
  They remain unapproved; no Linux/security exception or ignore was introduced.
- [Scan receipt](scan-receipt.json) uses cargo-deny0.20.2 and fresh RustSec
  f8dee89e1b2f2f1eaf548312df7655fe5202a302, dated2026-10-02T20:27:46Z. Preserve
  failed raw-policy outcomes; scoped success is not global policy success.
- The standard-library inventory test suite passes18 tests after observed RED
  cases. A real version_check package exposed a parser-prefix bug; its regression
  failed then passed. No dependency build script/proc macro/application ran.

## Native and emitted-artifact classification

[Loader receipt](native-loader-receipt.json) binds webview2-com-sys0.39.1's nine
prebuilt loader files to Microsoft.Web.WebView2 1.0.3800.47, all byte-identical.
NuGet SHA256 and official CDN SHA512 match; exact package LICENSE is Microsoft
BSD-3-Clause-form and requires notice retention/no endorsement. The wrapper omits
LICENSE/NOTICE, so retain the full official package notices for any later approved
artifact. NOTICE's generic LGPL-debugging language is not proof of a newly selected
LGPL component; named components are BSD, and individual loader attribution is
not supplied. No blanket LGPL permission or absence-of-LGPL binary claim is made.

MSVC source selects WebView2LoaderStatic; build.rs copies all nine files first.
Neither script nor native file was executed. Signature/Authenticode, native
vulnerabilities, actual linking and final-output inclusion remain unverified.
A Wry example wasm is merely packaged example data; inactive GNU/system-deps/
libloading native fixtures are not selected runtime artifacts. Inventory records
both selected payload hashes and inactive payload counts/member-map digests.

All five MPL packages' code/generated-output contribution remains unqualified.
Host-only ancestry is not evidence of no copied/generated covered code. Preserve
existing source/notices and examine actual outputs before any future distribution
proposal under [MPL§3](https://www.mozilla.org/en-US/MPL/2.0/) and
[Mozilla FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/). No production or distribution
permission is conferred, and this receipt is not legal approval.

## CI execution safety

Actual changed fixture has its own workspace, no root member/dependency edge,
no build.rs/executable/capability/workflow/task hook and no pnpm match. Root Cargo,
lock, deny, source, workflows and assurance capability manifest are byte-unchanged.
Existing root/named Rust tasks cannot select this graph. Assurance only executes
existing spec/assurance/capabilities manifests; recursive data discovery does not
promote manifests into build roots. Docker copies tools as data, then builds only
the root workspace. Existing Node test roots exclude this stdlib suite.

Existing recursive OSV may inspect Cargo.lock as data and report inactive advisory
entries; that truthful failure is not permission to disable security checks or
rename the lock to evade scanning. PinnedOSV2.5.1 has Rust call analysis off in the
unchanged command. Draft creation still runs normal CI. A final independent review
must recheck these claims against the exact candidate; no Tauri execution workflow
is added and no future workflow success is promised.

## Exact remaining scope

This fixture contains the framework base graph, not a complete eventual app graph.
Native Windows-host dependencies/features, the actual frontend/API/picker/transport/
test-driver graph, WebView2 Runtime/WindowsSDK/MSVC/driver terms and servicing,
authorized Windows10Pro cloud host/ESU, privacy/native interaction, emitted output
and all fifteen runtime cases remain separate gates. The catalog currently offers
only an excluded Mac and no saved coding environment. Supplemental GitHub Windows
Server could provide some actual evidence after a narrowly reviewed policy/terms/
workflow change, but cannot close Windows10Pro proof. [Detailed gates](../organization-tauri-windows-runtime-gates.md).

No global Windows flag/CSP/auth/CORS/security policy change, native installation,
package compilation, screenshot/upload, userMac, denied probe retry, Search WIP,
Audit import, merge/deploy or Phase5/6 implementation occurred. Parent must durably
publish the independently reviewed exact bytes and verify hosted outcomes. Any
new prohibited package/version/use, advisory or unverifiable native artifact stops
its affected scope before execution.
