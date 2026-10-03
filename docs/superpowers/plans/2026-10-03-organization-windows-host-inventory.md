# Windows host inventory implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans task-by-task after independent design approval. No implementation or activation is authorized by this pending plan alone.

**Goal:** Obtain bounded actual hosted-Windows metadata without using WebView2 or other unqualified tools.
**Architecture:** One exact-template inventory-only job and narrow machine-policy profile, disabled until head-bound parent activation. Existing global Windows/backend/mise rules remain.
**Tech Stack:** Existing architecture-lint Rust/serde/TOML dependencies, canonical block YAML, preinstalled PowerShell metadata APIs.
**Spec:** [Host inventory design](../specs/2026-10-03-organization-windows-host-inventory-design.md).

## Global constraints

- All six global platform/CI values remain as specified; no dependency/lock/deny/CSP/auth changes
- No Runtime/loader/browser/compiler/driver/project-tool invocation, install/download/build, artifact upload or screenshot in the Windows job
- Sole windows-2022 job, permissions:{},3-minute timeout, exact fixed source/activation tuple
- Record≤16KiB, strings≤128chars, reads≤4KiB each/1MiB aggregate, directories≤8, activation≤24h
- Existing labeled-event CI rerun is disclosed; do not disable it
- Current metadata/Runtime STOP checkpoint publication and exact remote-base verification precede implementation

## Review focus

- Template/profile changes accidentally enable another Windows or project-execution path
- Activation replays, stale heads, partial source identity or floating images presented as approval
- Metadata access failure/absence wrongly treated as installed/loaded/entitled Runtime
- Registry/file text escaping exposes raw host/user data or executable input
- Version discovery invokes a proprietary binary or changes host execution/security policy

## H0. Design gate

Files: this plan, design and additive execution status only.
- [x] Independent review of exact policy/mise exception, activation, resource/privacy and no-use semantics
- [x] Close Important/Critical findings; independent GO40c91eb/treed11acb71 at03:26UTC, no new semantics or Runtime authority
- [ ] Parent confirms implementation boundary and actual published predecessor identity; no Runtime acceptance inferred

## H1. Fail-closed profile/template enforcement

Conditional files: tools/architecture-lint/src/{config.rs,checks.rs,windows_host_inventory.rs}, tests/policy.rs;
spec/architecture/{dependency-rules.toml,development-container-ci-architecture-v0.md,architecture-contract-v0.md,development-assurance-architecture-v0.md};
Template/collector paths under tools/organization-windows-host-inventory/;
.github/workflows/organization-phase4-host-inventory.yml; mise.toml negative-smoke only if necessary.

Interfaces: optional closed profile and disabled/armed typed activation; render the complete expected workflow from fixed constants and collector bytes; validate exact candidate bytes and return a specific finding on any mismatch. Absent profile retains existing rejection. No arbitrary parser dependency.

- [ ] RED tests: absent/unknown profile, changed preserved flags, invalid PR/SHA/nonce/window, designated-path mismatch even without Windows text, copied workflow at wrong path
- [ ] Implement closed types/template equality and the exact per-job Windows/mise handling; preserve self-hosted/permissions and ordinary checks
- [ ] RED→GREEN mutations: extra job/step/event/env/action/checkout/upload, wrong runner/expression/matrix/shell/permissions, altered collector/guard, CRLF/BOM/trailing content, comment-only mise
- [ ] Update only explicit normative sections; retain root/fixture source/locks/dependency policies and old workflow bytes
- [ ] Run cargo test --locked -p architecture-lint and existing architecture checks/negative-smoke; any environment/tool restore outside reviewed scope stops before installation

## H2. Pure metadata collector and disabled workflow

Conditional files: tools/organization-windows-host-inventory/collector.ps1 and canonical workflow template, embedded as source in the H1 verifier; no Windows application artifacts.

Interfaces: fixed collector reads the design's allowlisted registry/file metadata and emits the one bounded record. Context enters only through enumerated scalar environment bindings. No raw errors/text or dynamic command strings.

- [ ] Add pure-helper self-tests for malformed/zero/oversized versions, absent/unreadable/type mismatch, parameter bounds, escaping and16KiB limit; structure them before collection after guard
- [ ] Implement bounded read-only providers, deterministic safe statuses, fixed tool roots and complete handle disposal; unknown paths/resources remain unknown
- [ ] Verify source contains no invocation/install/network/policy/capture/upload route; do not invoke binaries to learn their versions
- [ ] Generate canonical block YAML, initially disabled, exactly one step/job and explicit non-profile noninteractive shell without policy override
- [ ] Run actionlint/zizmor via existing ci:lint; no waiver if canonical workflow is unsupported. Run relevant unchanged assurance/policy checks and independent source/privacy review

## H3. Publish, bind and deliberately activate

- [ ] Parent publishes reviewed disabled source as a new Draft, preserving existing PR heads; new Draft creation cannot run the Windows collector
- [ ] Resolve exact PR/base/actor IDs, window and nonce; create a reviewed armed revision, then requalify normal exact-head gates
- [ ] Parent verifies live head and workflow/collector/profile identities, prior-run history, unconsumed activation authority and disclosed bounded logs/collateral existing CI
- [ ] Parent alone applies the exact49-character w4-<full-head-SHA>-<five-hex-nonce> label; no auto label, workflow dispatch, merge or retry
- [ ] Inspect actual run/job and one bounded JSON record; reject stale subject, unexpected code/fields/size, setup-policy bypass, unexpected tool execution or upload

## H4. Record available environment and stop at use boundary

- [ ] Reconcile catalog versus actual image/OS/registry/tool facts; record absence/partial states and immutable run/job/log links
- [ ] Check correct product-specific terms source, including Enterprise versus standalone Build Tools; entitlement/acceptance remain unknown unless established
- [ ] Independently review metadata evidence and parent-publish a bounded receipt, never raw logs/binaries
- [ ] Only then formulate the smallest environment/product-specific owner decision. No Runtime use/install or later-VM equivalence follows automatically

Current execution stops at H0 design review. H1–H4 are conditional implementation,
qualification and activation steps, not a claim of authority already exercised.
