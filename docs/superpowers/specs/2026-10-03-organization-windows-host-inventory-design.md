# Organization Phase4 Windows host inventory design

Date: 2026-10-03 UTC. Status: **DESIGN REVIEW PENDING; NO IMPLEMENTATION OR ACTIVATION**.

## 1. Purpose, authority and baseline

Obtain concrete offered/allocated Windows environment facts before proposing
acceptance of proprietary Runtime/tool terms for an unspecified host. Parent
requested this strictly read-only preflight after the [Runtime agreement STOP](../../research/organization-tauri-windows-runtime-gates.md).
Original§50 permits faithful formalization, not Runtime use or license waivers.
The five exact MPL scopes remain unchanged. No Windows10Pro/Phase4 GO follows.

Local design base is reviewed Runtime STOP60834e1/tree01406bb2, above reviewed
metadata4164683/tree9d364531. Parent publication assigns different remote commits;
verify exact mapped remote trees and current gates before implementation/activation.
No remote publication identity or hosted pass for these local commits is invented.

## 2. Offered host versus observed host

The [official catalog at d5e6cc837b0c17eb6cb339fff5067f73fc75323c](https://github.com/actions/runner-images/blob/d5e6cc837b0c17eb6cb339fff5067f73fc75323c/images/windows/Windows2022-Readme.md)
reports windows-2022 / Server2022 OS10.0.20348 build5622, image20260927.320.1,
VS Enterprise2022 17.14.37710.0, Rust/Cargo1.98.1 and PowerShell7.6.6. It lists
Edge152.0.4191.66 and EdgeDriver152.0.4191.100, but not WebView2 Runtime.
These are static catalog claims, not observations from our allocated runner.

The literal windows-2022 label floats across image revisions. Actual ImageOS,
ImageVersion and GitHub's Set up job log must be retained as run-specific evidence.
A later job gets another ephemeral VM and must revalidate actual image/Runtime/tool
identity before any separately authorized use. No environment persistence claim.
Server2022 cannot satisfy actual Windows10Pro edition/build/ESU or desktop proof.

## 3. Chosen minimal scope and rejected alternatives

One hosted Windows OS-metadata job, no checkout or third-party Action, using only
preinstalled PowerShell for bounded registry/file metadata reads and JSON encoding.
It does not invoke loader APIs, Runtime/browser/compiler/driver/vswhere/rustc/cargo/
node/project tooling, install/update/download, build, run broker, alter settings,
accept terms, capture pixels or upload artifacts. No network request from collector.
No scan of user data or the whole machine; absence/unknown is an honest result.

Use exact canonical **block-style YAML** generated from a reviewed full template.
A complete byte match binds one known job and one collector, without implementing
a YAML parser. JSON-syntax YAML was considered but would introduce quoted runner
keys missed by the existing line heuristic and require broader syntax changes.
A new workflow_dispatch-only file cannot first run while confined to an unmerged
Draft because [GitHub requires it on the default branch](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch).
No merge is used to solve activation.

## 4. Narrow policy profile, all global rules preserved

Keep production_target=linux/amd64, platform.native_windows_supported=false,
ci.allow_native_windows=false, ci.allow_self_hosted=false,
ci.require_mise_entrypoint=true and ci.require_workflow_permissions=true.

Proposed single profile organization-phase4-host-metadata-v1 is absent/disabled by
default. It identifies only .github/workflows/organization-phase4-host-inventory.yml,
job host_inventory, literal windows-2022 and the exact collector. No extensible
allowlist, runner/path wildcard, arbitrary script or broad platform support.

An explicit normative exception is necessary in development-container-ci architecture
§§2/17 and its mise§6, architecture-contract§24 and development-assurance§42. Only
this non-project OS-metadata collector may omit mise: installing mise violates
this no-install slice, while a fake comment containing mise run is not compliance.
All other project tasks/workflows retain existing mise and Linux/backend rules.
No assurance capability or root/fixture dependency/lock/license-policy change.

Architecture-lint validates the exact profile and preserved global invariants
before granting any exception. At the designated path, every mismatch fails even
if the altered text no longer looks like a Windows job. Only a full exact template
match earns the job-specific native-Windows and inventory-only mise treatment;
self-hosted denial and explicit empty permissions remain checked. A copied normal
block-YAML Windows workflow at another path remains subject to legacy rejection.
Existing heuristic checks are not described as complete arbitrary-YAML analysis.

Template inputs are closed types: disabled, or an armed positive PR number with
lowercase40-hex nonzero base SHA, lowercase5-hex nonce and unsigned epoch window
whose duration is positive and at most24h. Repository/actor IDs, head/base branch
names, runner, permissions, event, shell, step count and collector are fixed
reviewed constants. No arbitrary string interpolation into YAML or PowerShell.
Reject unknown profile/fields, invalid parameter bounds, CRLF/BOM/trailing content,
extra job/step/event/env/permission, altered shell/collector and noncanonical syntax.
Use existing Rust dependencies only. Bind collector bytes with include_str! and
deterministic indentation; actionlint and zizmor independently validate actual YAML.

## 5. Workflow shape and deliberate activation

Sole event: pull_request types:[labeled]. Sole job host_inventory, timeout3 minutes,
top-level permissions:{}, no uses/services/container/matrix/reusable workflow/cache,
one inline step. Explicit shell: pwsh -NoLogo -NoProfile -NonInteractive -File {0};
no execution-policy override. If host policy prevents execution, STOP, never bypass.

Fresh verified repository ID1369120817 and owner ID179456049 (AIrisu-072) are fixed
proposed actor/repository guards; reverify the actual activating identity before
arming. Fixed head branch: qualify/organization-phase4-host-inventory. Fixed base
branch: docs/organization-tauri-runtime-license-stop, subject to exact published
base binding before implementation. Naming adjustments are reviewed changes, not
permission for a wildcard. Source is same-repository, open Draft PR only.

Initial implementation is disabled. After parent creates the real Draft, bind its
exact PR number/base SHA and finite window in a reviewed armed source revision.
Arming changes source: obtain new exact-head normal checks/review before activation.
No job can run on Draft creation, synchronize, reopen, push or a schedule.

Activation label: w4-<current full40-hex head SHA>-<reviewed5-hex nonce>, exactly49
ASCII characters. Its nonce is a public identifier, not an authentication secret.
Require exact equality, verified actor/repository/PR/head/base, first run attempt,
and bounded time checked before collection. Do not embed the workflow's own full
commit SHA in its own contents; the parent's explicit head-bound label avoids that
circularity. Record PR head SHA separately from effective workflow/merge SHA.
No event text enters executable script. The only declared bindings are
HOST_INV_REPOSITORY_ID, HOST_INV_ACTOR_ID, HOST_INV_PR_NUMBER, HOST_INV_HEAD_SHA,
HOST_INV_BASE_SHA, HOST_INV_HEAD_BRANCH, HOST_INV_BASE_BRANCH,
HOST_INV_HEAD_REPOSITORY_ID, HOST_INV_PR_STATE, HOST_INV_PR_DRAFT,
HOST_INV_EVENT_ACTION, HOST_INV_LABEL, HOST_INV_RUN_ID, HOST_INV_RUN_ATTEMPT and
HOST_INV_WORKFLOW_SHA. Map them from the corresponding GitHub/event scalar fields;
validate every type/value before collection. Read only provider ImageOS/ImageVersion
and the interpreter's own version as additional environment/runtime metadata. Do
not expose a GitHub token, interpolate event text into code or dump environment.

Existing ci.yml also handles labeled events; this action reruns its ordinary Linux
CI. Disclose and obtain applicable activation authority for that collateral existing
work. Do not suppress/modify existing CI. First-attempt/window/concurrency guards
are not durable one-shot storage: parent must inspect prior runs, consume the
approval once and refuse replay/relabel without fresh authority. Revalidate live
PR/source before applying the label and before consuming the resulting receipt.

## 6. Collection and bounded receipt

Read only Microsoft's two documented x64 Evergreen pv (REG_SZ) locations:

- `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`
- `HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`

Use explicit registry views/read-only handles, read only pv and its value kind,
and dispose handles in finally. Accept bounded four-part numeric versions greater
than0.0.0.0; distinguish absent/null/zero from unreadable, wrong type and malformed.
Registry evidence is registered Evergreen presence/version, not loaded version,
health, architecture or entitlement. No GetAvailableCoreWebView2BrowserVersionString
or Fixed Runtime search. [Microsoft detection source](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

OS: allowlisted Windows CurrentVersion product/edition/build/UBR and architecture.
Image: only ImageOS/ImageVersion. Tools: fixed VS Enterprise2022 root and devenv.exe
file-version metadata; bounded Microsoft.VCToolsVersion.default.txt, then fixed
Hostx64/x64 cl.exe/link.exe metadata; validated KitsRoot10 and at most8 immediate
numeric SDK directories with fixed x64 rc.exe/mt.exe metadata. For Rust/Cargo,
only trusted image-script-established installed-toolchain paths may be inspected;
if actual relocation or version resources are unknown, report unknown. Never run
proxies/commands. No standalone Build Tools entitlement is inferred from Enterprise.

Reject UNC/device/unexpected roots, reparse entries, control characters, excessive
counts and reads. Each text read≤4KiB, directory entries≤8 per approved root, total
read budget≤1MiB excluding OS-managed version-resource access; no binary hashing
or signature/network validation is part of this minimal slice. Metadata strings
≤128 characters; no absolute user paths, account/computer names, tokens or raw
exceptions. Fixed component IDs/relative locators only. Pure helper self-tests run
after the activation guard and before any registry/file read, covering bounds,
version parsing and safe serialization; failed tests emit guard_rejected only.

Emit exactly one escaped JSON record≤16KiB plus a fixed status code:
provenance(run/attempt/PR/head/effective-workflow), static_catalog(source URL/pin),
observed_host(image/OS/architecture), webview2(per-key status/version), tools(fixed
ID/presence/version/relative locator), terms(candidate official URL/product match,
entitlement unknown/acceptance not established), result(complete/partial/guard_rejected),
runtime_qualification:not_performed. loaded_version:null and loader_query:not_run.
Record collection start/end UTC; the fields are observations over that interval,
not an atomic installation snapshot. Provider/background updates remain possible.
Normal GitHub setup/job logs remain hosted; no artifact upload does not mean no
metadata log transmission. Null/unknown values cannot be used as positive proof.

## 7. Remaining gates and completion

This document changes no policy/source/workflow and does not activate a job.
Independent design/policy/privacy review precedes implementation. Exact collector,
profile/template/linter/negative tests plus unchanged normal exact-head gates must
pass before separately authorized parent activation. Inventory absence is a valid
observation; unreadable/invalid/budget failure is partial and leaves affected facts
unqualified. Capture run/job/head/log provenance, verify bounded actual output and
reconcile actual products/terms before any new use/installation decision.

Runtime custom-license STOP, compiler/SDK/driver entitlement, privacy, native picker,
closed Document transport, broker/security/generated output and Windows10Pro/ESU
remain. No production, redistribution, userMac, denied probe retry, screenshot or
Phase5/6 scope is created. No result from this preflight accepts a proprietary term.
