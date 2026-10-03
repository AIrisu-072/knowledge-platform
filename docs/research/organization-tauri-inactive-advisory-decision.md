# Windows-only qualification: inactive advisory decision packet

Date: 2026-10-03 UTC. Status: **OWNER DECISION PENDING; POLICY / H1 STOP**.

## 1. Current verified subject

Public DraftPR52 is remote fdaf9a04f4251414a94ddfad89e6b81a6406c83f,
tree53fbaef9898f21881de1c3dc39debd49e0baafb6, matching independently reviewed
local5bf06ec. The exact four approved checksum fingerprints now pass hosted
full-history Gitleaks; its synthetic self-test and root cargo-deny also pass.
CI37124138433 security job111205933594 subsequently fails the unchanged recursive
OSV2.5.1 check on the isolated qualification lock; aggregate required-check also
fails. All remaining CI jobs pass;
DSI37124138415 and Sandbox37124138370 pass, while branch-specific D2 is skipped.
No advisory exception follows from the prior license or checksum approvals.

| Locked package | Advisory | Actual meaning |
| --- | --- | --- |
| glib0.18.5 | RUSTSEC-2024-0429 / GHSA-wrw7-89jp-8q8g | affected unsound iterator implementation; GitHub CVSS4.0 6.9/moderate |
| proc-macro-error1.0.4 | RUSTSEC-2024-0370 | unmaintained; no particular vulnerability/CVSS or patched release asserted |

No configuration, lock, dependency, workflow, scanner rule or H1 implementation is
changed by this documentation packet. The original failures/receipts remain intact.

## 2. Package risk and recorded selection are different facts

[glib's captured-revision advisory](https://github.com/rustsec/advisory-db/blob/f8dee89e1b2f2f1eaf548312df7655fe5202a302/crates/glib/RUSTSEC-2024-0429.md)
affects versions >=0.15.0,<0.20.0; patched versions begin at0.20.0. It has no
withdrawal or OS exemption. The affected VariantStrIter next/nth/last/next_back/
nth_back functions let C mutate pointer storage created immutable in Rust, yielding
undefined behavior. The [captured0.18.5 source](https://github.com/gtk-rs/gtk-rs-core/blob/42b9caf98e03ded086362d9653ca58fe94dc8658/glib/src/variant_iter.rs#L118-L130)
still contains that implementation. This is not an already-backported fix or a
mistaken affected range. [Upstream fix](https://github.com/gtk-rs/gtk-rs-core/pull/1343)
and [GitHub's reviewed severity](https://github.com/advisories/GHSA-wrw7-89jp-8q8g)
are separate evidence from any application's exposure.

[proc-macro-error's advisory](https://github.com/rustsec/advisory-db/blob/f8dee89e1b2f2f1eaf548312df7655fe5202a302/crates/proc-macro-error/RUSTSEC-2024-0370.md)
is a maintenance warning with no patched version. The suggested maintained
alternatives require upstream source/API migration. proc-macro-error2 is not a
clean replacement: [RustSec removed that suggestion](https://github.com/rustsec/advisory-db/commit/dfd3306e15d42c3de5bb972b2f68181a7cf83819)
after that fork also became unmaintained.

Both locked packages have roles=[], occurrences=0, no selected paths/features,
and inactive_or_other_target classification in the verified inventory. They are
absent from the captured Windows metadata/scanner projection and present in the
all-target package list. This proves inactivity only in the recorded Linux-x64
host to x86_64-pc-windows-msvc target projection. It does not make these package
versions safe, establish native-Windows-host selection, prove function-level
unreachability in a future binary or qualify Linux use. No Tauri application or
these packages' code has been built/executed in this track.

## 3. Correction: the old all-target scan was not exhaustive for unsoundness

Original scan-all-raw.jsonl, scan-receipt.json and advisory DB revision remain
unchanged and genuine. Their all-target label describes target/package coverage.
It must not be read as exhaustive transitive-unsound advisory coverage.

Exact cargo-deny0.20.2 source [defaults](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/advisories/cfg.rs#L99-L106)
and [filtering](https://github.com/EmbarkStudios/cargo-deny/blob/bca0dde53651ee946720e4540b5ce2610bec8f06/src/advisories.rs#L105-L139)
explain the difference: informational-unmaintained defaults to all dependencies;
informational-unsound defaults to direct workspace dependencies. Both captured
policies set only yanked="deny" in their advisory sections. glib was transitive,
present in the scanner graph and DB, but outside that default reporting scope.
proc-macro-error was reported. There was no package omission or advisory withdrawal.
A zero-error scoped scan is an outcome under those settings, not a general absence
of vulnerabilities. OSV's independent full-lock finding must remain visible.

## 4. No compatible supported update removes these nodes today

Current official stable Tauri remains2.12.1; its [exact source](https://github.com/tauri-apps/tauri/blob/30da1fd6e17de6107ecc850c95dfb16b5729f2dd/crates/tauri/Cargo.toml)
and locked runtime/Wry manifests require GTK^0.18 on Linux/BSD targets. The lock
selects GTK0.18.2, GLib0.18.5 and glib-macros0.18.5/gtk3-macros0.18.2; both macro
crates require proc-macro-error^1.0. Current official registry data lists no newer
compatible0.18 versions and proc-macro-error still ends at1.0.4.

GTK0.19.0 is now officially published and uses GLib^0.22 plus gtk3-macros^0.19.0,
but it does not satisfy Tauri's^0.18 requirement. GLib0.20+ likewise cannot replace
the required^0.18 node through a normal compatible update. Adding a newer GLib
alongside the old one does not remove the affected package. No Cargo resolution,
update, local fork, replacement crate or source patch was attempted.

Registry sources: [GTK](https://raw.githubusercontent.com/rust-lang/crates.io-index/master/3/g/gtk),
[GLib](https://raw.githubusercontent.com/rust-lang/crates.io-index/master/gl/ib/glib),
[GLib macros](https://raw.githubusercontent.com/rust-lang/crates.io-index/master/gl/ib/glib-macros),
[GTK macros](https://raw.githubusercontent.com/rust-lang/crates.io-index/master/gt/k3/gtk3-macros),
[proc-macro-error](https://raw.githubusercontent.com/rust-lang/crates.io-index/master/pr/oc/proc-macro-error).
These current checks are dated2026-10-03; registry records are observations, not
permission for a future transitive graph.

[Cargo's resolver](https://doc.rust-lang.org/cargo/reference/resolver.html)
resolves platform-specific dependency tables across platforms when creating a lock.
A Windows target/table does not lawfully prune the Linux entries out of Cargo.lock.
Removing Wry would not remove Tauri core/runtime's GTK target dependency and would
also abandon the required existing-React desktop runtime proof. Tauri3 alpha, a
custom runtime/fork, manually edited locks or renamed/hidden lockfiles are not
compatible fixes within this approved scope.

## 5. Native OSV configuration alone cannot express the requested boundary

Pinned OSV2.5.1 is commitc84fa4568f2526d0333e9a914ea8a0a5f74ad68b.
Its [configuration contract](https://github.com/google/osv-scanner/blob/c84fa4568f2526d0333e9a914ea8a0a5f74ad68b/docs/configuration.md)
places config beside a scanned file and does not inherit it into child directories.
A global --config override instead affects every parsed input; it is unsuitable
for applying this fixture exception to a recursive repository scan.

The [actual types](https://github.com/google/osv-scanner/blob/c84fa4568f2526d0333e9a914ea8a0a5f74ad68b/internal/config/config.go#L16-L49)
provide ID/alias-based IgnoredVulns with optional expiry/reason, but no package or
version conjunction. PackageOverrides can match name/version/ecosystem, yet its
vulnerability action ignores all vulnerabilities for that package, including new
ones. Neither alone expresses exactly two advisory/package/version combinations
plus lock identity and inactive host/target roles. Do not invent unsupported fields
or use a package-wide ignore as if it were ID-specific.

## 6. Smallest proposed guarded qualification treatment

**Proposal only; no implementation or approval inferred.** Prefer an explicit,
mandatory qualification-policy guard and an inert, reviewed native config template,
not a permanently auto-loaded osv-scanner.toml. Standalone recursive OSV must keep
reporting the findings unless the reviewed guard deliberately evaluates this exact
profile. No .gitignore/exclusion/renaming/path-hiding change is proposed.

The only proposed input is `tools/organization-tauri-qualification/Cargo.lock`,
with its unchanged Cargo.toml and inert src/lib.rs at the verified subject above.
Proposed permission is confined to glib0.18.5 / RUSTSEC-2024-0429 (and its existing
GHSA alias) plus proc-macro-error1.0.4 / RUSTSEC-2024-0370 in this unchanged inert
fixture. Recommend expiry2026-10-10T00:00:00Z with no automatic renewal; the owner
may instead retain STOP or set a different explicit deadline. No date is applied
by this packet.

Before any qualified-result treatment, a required guard must:

1. Match the exact fixture path, manifest/source/lock bytes and approved tools.
   Lock SHA256 is d47b06511f830c3bd1ef4f0ce828db9162c1807a6d3563887c4f3c8d2073bcef.
   Confirm its own workspace, publish=false, no executable/build.rs/capability or
   runtime configuration, and no root Cargo/pnpm/build-task activation. Reject a
   new lock/manifest/package/version, extra scanned input, symlink, unknown profile,
   expired permission or config drift. Existing execution-root isolation must be
   checked, not assumed from the directory name.
2. Reconstruct the reviewed host/target selection using qualified data-only helpers,
   verified archives, explicit host/Windows target and locked/offline metadata/tree.
   Independently reconcile normal/no-proc target selection and complete host lookup.
   Both packages must remain absent from every selected host/target occurrence,
   including proc-macro ancestry. Any selected use, unknown role, changed source or
   new native-Windows-host graph fails and reopens STOP. No package/build-script/
   proc-macro compilation, Rust call analysis or runtime is permitted.
3. Preserve an unfiltered recursive/full-lock OSV JSON result, exit status, scanner
   and database/advisory provenance before applying this profile. Preserve complete findings and
   raw-result hashes in bounded receipts; raw reports remain external or in the
   existing CI evidence surface, with no new artifact upload or source/binary mirror.
   Use the native
   scanner, not a reimplementation of vulnerability/range matching. Validate source
   path, exact package versions and the two known advisory groups/aliases. Reject
   any additional finding, different affected input/version, changed advisory
   semantics, incomplete scan, network/parse/tool error, missing coverage or empty
   result that cannot prove complete scanning. Raw findings remain two unresolved
   advisory groups, not relabelled zero vulnerabilities.
4. If native ID suppression is used to cross-check the qualified fixture result,
   generate its reviewed config only for an explicit single-lock invocation after
   those guards pass; never pass it to repository-wide input. Do not commit an
   always-active same-directory ignore file or use PackageOverrides. Preserve
   raw-versus-qualified results and the source/permission tuple in a bounded receipt.
5. Keep root cargo-deny and all unrelated repository scans/rules intact. Integrate
   the guard into the required mise security entrypoint so the only accepted raw
   OSV failure is this fully validated profile. A profile-qualified gate must say
   it accepted known inactive qualification data; it is not a global scanner pass,
   a package repair or application/native-runtime security qualification.

The exact implementation and error/schema/coverage handling require independent
review and negative tests after owner approval. No current code is claimed to
provide these predicates. If they cannot be enforced without weakening another
boundary, remain STOP. New interpreter/tool installation, if needed, requires its
own previously qualified official provenance; no implicit restoration or package
execution follows this proposal.

## 7. Required negative evidence before publication of an implementation

- Change each package/version/advisory/source/lock/manifest/expiry independently:
  fail, even if native ID filtering would otherwise hide the result
- Move either package into a host, target or proc-macro role: fail; unknown host or
  native-Windows graph is not substituted for the recorded projection
- Add another affected package/advisory or matching ID in another lock/directory:
  fail; no root or child-directory exemption
- Unknown native config field/ID, package-wide ignore, modified template, extra
  input, malformed/truncated/empty JSON, scan error or partial coverage: fail
- Full raw scan continues to report both known groups; raw failure is retained.
  Direct unguarded recursive OSV still reports them; expired/removed profile
  restores unconditional failure. No output is labelled vulnerability-free
- Normal root/workflow/license/security gates remain mandatory. No deployment,
  Linux package use, Runtime acceptance or Windows job activation is triggered

## 8. Owner choice and remaining STOPs

The bounded choice is whether to authorize this guarded, expiring Windows-only **qualification-data** exception
for these exact two tuples, conditional on independent implementation review and
negative tests,
or to keep STOP pending a supported compatible upstream migration. A bare ignore,
blanket Linux advisory waiver, package-wide rule, fork or false unaffected claim
is not offered as an approved alternative.

Even if approved and implemented, actual native-Windows host/tool/Runtime terms,
Windows10Pro/ESU, picker/Document transport/security, generated-output/source-notice
obligations and all runtime requirements remain unqualified. The proposed treatment
does not authorize Linux use, application build/runtime, production, distribution,
security-setting changes beyond the separately reviewed exact profile, screenshots
or H1 activation. H1 inventory-only implementation remains held until the owner
judgment, profile review and applicable required-gate qualification.
