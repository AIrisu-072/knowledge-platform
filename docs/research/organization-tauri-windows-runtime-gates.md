# Windows native qualification gates after the scoped MPL approval

## Current Runtime agreement STOP, 2026-10-03 UTC

The [current legal-source receipt](organization-tauri-windows-inventory/runtime-terms-receipt.json)
now verifies the official developer Runtime agreements, separately from SDK loader
BSD evidence. [Evergreen terms](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us),
[Fixed Version terms](https://developer.microsoft.com/microsoft-edge/api/eula/webview2?locale=en-us&fixed=true)
and the consumer variant are distinct. The official download page selects developer
Evergreen terms for bootstrapper/standalone downloads and Fixed terms for fixed
packages. No overall effective date/revision or Runtime build is specified; a GDPR
reference date is not an effective-date assertion. These current sources do not
prove historical acquisition terms or entitlement for an installed host.

Evergreen expressly permits development/testing, but use constitutes acceptance.
Its custom terms include diagnostics/SmartScreen notice requirements, automatic
updates and use/liability restrictions. Runtime object-code redistribution has
additional obligations and is not proposed. Its download-link restriction versus
Microsoft deployment guidance is a further unresolved distribution issue, not a
reason to assume permission. Repository restrictive-custom-license policy therefore
keeps **Runtime use blocked pending a separate bounded ADR/owner decision**. The
five MPL approvals and the BSD-form SDK license grant no such exception or agreement
acceptance. No Runtime download/install/launch/use or security setting change occurred.

An adequate existing Runtime can avoid a fresh installer/download acceptance step;
it does not establish exemption from terms or this host's prior entitlement. Edge
Stable alone is not a supported WebView2 backing Runtime. A new installer flow has
an explicit acceptance step and requires its own linked agreement/action permission;
a programmatic URL does not erase it. Fixed Version is not an assumed workaround.

Next: design/review a minimal GitHub-hosted Windows **inventory-only** preflight to
identify actual official image/OS, installed Runtime presence/version and toolchain/
terms provenance. It must not launch Runtime/loader/browser/broker/compiler, install
anything, build, capture or upload artifacts. Its narrow policy amendment cannot
weaken existing backend/native-Windows flags or unrelated-job rejection. No Windows
job is added or activated by this record. Obtain concrete environment evidence before
asking acceptance for an unspecified host; actual Windows10Pro/ESU remains separate.

Fingerprint distinction: raw Evergreen API JSON SHA256
e15b53f476b66f8335c18436998256dc9862b210242a8e4c7f7e14d2de53591d;
decoded evergreenHtml field UTF-8, without normalization, SHA256
ce6fa83e57c338256e5cabe9e1eea83076c271b0fdb253408213eeb08859d7b6.
Independent reads match raw and decoded bytes for all three variants. No legal HTML
or raw API logs are committed; the receipt records source URLs and exact fingerprints.

---

Date: 2026-10-03 UTC. Status: **READ-ONLY PREREQUISITES; NATIVE EXECUTION NOT QUALIFIED**.
The five exact MPL uses are approved by the [ADR amendment](../decisions/2026-10-03-organization-tauri-windows-mpl-amendment.md).
They do not approve new tools, native terms, privacy changes or distribution.

## Available host evidence

This executor is Linux x86_64. A worker environment-list action returned “invoking
thread is not attached to an Aeon,” which was not an empty-catalog result. The
root then successfully enumerated its complete current catalog: one connected and
authorized Mac, no saved coding environments, nextCursor null. The Mac is outside
the cloud-only instruction and is not used. No suitable Windows executor is
presently offered by that catalog; no global claim about cloud Windows availability.

GitHub-hosted Windows is a credible conditional supplemental route. Current
[official runner images](https://github.com/actions/runner-images/blob/main/README.md)
list x64 Windows Server2022/2025 and Windows11 ARM64, not Windows10Pro. A hosted
Server run cannot establish the requested Windows10Pro edition/build/ESU result.
No new workflow or environment was created or dispatched during this investigation.

## Smallest platform amendment, still unimplemented

Keep dependency-rules.toml production_target=linux/amd64,
platform.native_windows_supported=false, ci.allow_native_windows=false and
ci.allow_self_hosted=false. Current development-container-ci architecture lines63–70/§17,
architecture-contract§24 and development-assurance architecture§42 must be reconciled
consistently for any narrow exception. Do not claim broad native Windows support.

A later explicit normative carve-out may allow only the approved isolated Phase4
manifest, adapter and test harness on one named approved Windows qualification
host, retaining every dependency/advisory/security/privacy gate and Linux/backend
production authority. An authorized Windows10Pro cloud Remote outside CI would
not need a CI runner exception, but still needs that native desktop scope.

A supplemental GitHub job additionally needs one exact workflow path, job ID and
literal runner label (for example a reviewed windows-2022 tuple, not an open-ended
windows-latest permission). Current architecture-lint checks.rs39–81 reject Windows
runner text file-wide. A future change must validate per job, not skip the entire
workflow. Negative tests must reject wrong workflow/job/runner, an extra Windows
job in the allowed file, unresolved expressions/matrices and self-hosted labels;
retain existing Linux/macOS, mise-entrypoint and permissions enforcement. No flag,
linter, normative policy, workflow or security setting is changed in this packet.

## Windows10Pro and native toolchain

- [Windows10 Home/Pro lifecycle](https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro)
  ended ordinary support2025-10-14;22H2 is final. Current [release information](https://learn.microsoft.com/en-us/windows/release-health/release-information)
  lists22H2 ESU19045.7727 (2026-09-14 OOB). At actual testing, recheck baseline and
  record real edition/build/KB/security servicing and applicable ESU entitlement.
- [Edge/WebView2 support](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-supported-operating-systems)
  continues Windows10 22H2 updates until at least October2028 without ESU. Browser
  engine support does not maintain the underlying OS or prove ESU eligibility.
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) require Microsoft
  C++ Build Tools/Desktop development with C++, MSVC Rust and WebView2. Frontend
  needs the existing Node toolchain. VBScript/MSI prerequisites are excluded by
  the no-installer route, rather than installed speculatively.
- Do not choose obsolete SDK19041/22621 solely because the target is Windows10.
  Microsoft currently marks newer26100+/28000+ [SDKs supported](https://learn.microsoft.com/en-us/windows/apps/windows-sdk/).
  Pin actual compiler/linker/SDK, inspect terms and test API compatibility.

## Distinct native and driver terms

The Rust wrapper license does not qualify its embedded SDK or loader. Fresh
[exact loader receipt](organization-tauri-windows-inventory/native-loader-receipt.json)
now matches all nine webview2-com-sys0.39.1 payloads byte-for-byte to official
Microsoft.Web.WebView2 1.0.3800.47, archive SHA256
56c9f26bdd07916a2d1949fb58a5c7e434dfa1173577dca879206050c4e718db.
The pinned updater selects this SDK; its exact NuGet license is BSD-3-Clause-form
and requireLicenseAcceptance=false. Full LICENSE/NOTICE hashes are retained; the
crate omits both. NOTICE names two BSD third-party components and contains generic
LGPL-debugging boilerplate, without mapping those components to individual loader
files. No new LGPL use is inferred or approved; preserve the complete official
notices conservatively for any later approved artifact. Package/Authenticode trust
and native vulnerability coverage remain unverified. This is source/license
provenance, not acceptance of broader Runtime/compiler/SDK/driver terms.

For comparison only, current
[Microsoft.Web.WebView2 SDK1.0.4258.31 license](https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.4258.31/License)
is BSD-style three-clause; the eventual resolved wrapper's exact embedded archive
differs from the exact1.0.3800.47 package above; newer terms do not substitute for
that pinned evidence. The [distribution guide](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)
distinguishes statically linked WebView2Loader and architecture-matched loader DLL.
Actual linked outputs remain uninspected.

Microsoft publishes separate [2022 Build Tools terms](https://visualstudio.microsoft.com/license-terms/vs2022-ga-diagnosticbuildtools/)
and [2026 terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-diagnostic-buildtools/).
Read-only review of their linked agreement text found a qualifying Visual Studio
license route and a separate bounded third-party open-source C++ dependency route;
2026 also changes outsourced Build Device conditions. Eligibility for this exact
Rust application/toolchain/host must be established before use. A free download
button is not evidence of eligibility. Installed Windows SDK terms and the actual host
Runtime acquisition/entitlement remain unverified. Current developer Runtime terms
were later recovered in the checkpoint above; no agreement was accepted. The earlier
localization503 was an availability failure, not proof that terms were absent.

[EdgeDriver terms](https://developer.microsoft.com/en-us/microsoft-edge/tools/webdriver/eula)
are Microsoft-specific and include telemetry/update and redistribution restrictions.
Do not upload a driver binary as an ordinary project artifact. Pin and inventory
both tauri-driver and a WebDriver client independently before use; neither is part
of this first metadata-only framework fixture.

## Privacy and proof limitations

[WebView2 privacy documentation](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy)
describes required diagnostics even with optional Windows diagnostics disabled,
optional API/SDK usage data, default SmartScreen/user-notice obligations and crash
dumps ordinarily sent to Microsoft. A documented custom crash-reporting API can
prevent automatic dump submission, but selecting it requires reviewed application
privacy/error handling. Do not silently change OS, diagnostic or security settings.
Synthetic fixtures, isolated profile, safe logs/dumps and notices need review before
execution; absence of screenshot capture does not prevent diagnostic transmission.

Tauri [officially documents Windows WebDriver CI](https://v2.tauri.app/develop/tests/webdriver/ci/).
Its example's unpinned installation commands are not an approved recipe here. Match
EdgeDriver to the actual loaded WebView2 Runtime, not merely installed Edge; record
runtime, executable/source/frontend hashes, compiler/SDK and image identity.

Microsoft's [WebView2 WebDriver guide](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/webdriver)
expressly says native UI cannot be automated by Edge WebDriver. Therefore real
folder choose/cancel requires a separately reviewed native UI Automation or desktop
interaction method and interactive session. Mocked selections and injected paths
cannot close the original native-picker gate. The method must not introduce new
privileges, Developer Mode, security changes or an unqualified tool graph silently.

Actual confinement/reparse/junction/symlink/race/exclusive-create/stable-snapshot
and restart tests remain required under frozen Phase2§12. No Windows policy/host,
picker, transport, native security, frontend/runtime or Phase4 completion is claimed.

## Next bounded route

Finish fresh role-scoped metadata/source/advisory inventory first. Independently
review its exact evidence, then prepare the concrete Windows policy/native terms/
driver/picker/closed Document transport/privacy subdesign. If a GitHub Server job
is subsequently authorized and qualified, label its results supplemental. A serviced,
authorized Windows10Pro cloud host and every original runtime case remain necessary
for Phase4 GO. No arbitrary future dependency approval is requested here.
