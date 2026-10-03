# Windows-first Tauri inventory implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans task-by-task. Independent review precedes metadata/tool restoration and follows the final evidence.

**Goal:** Recreate truthful, reproducible, role-scoped Windows candidate inventory after the exact two-package approval.
**Architecture:** Private inert nested workspace plus data-only inventory/scanning. Root policy and application remain untouched; later builds are separately gated.
**Tech Stack:** Official Rust/Cargo1.98.1, cargo-deny0.20.2 prebuilt, Python standard library, Tauri2.12.1/tauri-build2.7.1.
**Spec:** [Windows inventory design](../specs/2026-10-03-organization-tauri-windows-inventory-design.md).

## Global constraints

- Approved host-only cssparser0.37.0/selectors0.38.0/cssparser-macros0.7.0/dtoa-short0.3.5; option-ext0.2.0 host plus Windows target
- No other exception, Linux build/runtime, package code execution, global policy change or production/distribution
- Disk floor1536MiB on every used filesystem, combined downloaded+expanded tool/cache/evidence budget4GiB, serialized Cargo, one shared cache/toolchain
- Actual PR51 base629822a4/treea85096b2; frozen Phase1/2/3, capturedPR48 and existing Draft heads preserved

## Review focus

- A flattened dependency graph hides a host-only exception leaking into normal target use: compare expanded ancestry and independent normal/no-proc projection
- Windows target projection on Linux misrepresented as Windows-native graph: record actual host and unresolved host-conditioned edges
- New transitive version or advisory slips behind five exceptions: exact-name/version/use predicates and unchanged raw scan
- Archive/source mismatch or escaped extraction: hash every selected archive/source and reject unsafe members
- Recursive CI discovery executes fixture code: prove own workspace and unchanged command/capability/OSV roots before publication

## W0. Review exact authority and first-slice design

Files: this plan, its spec, scope ADR, additive active/status and review record.
- [x] Fetch actual PR51 and verify tree/current exact-head gates; check clean baseline and available disk
- [x] Record exact owner question/reply and strict five-package scope
- [x] Independently review first-slice design, tool restoration, role semantics, no-execution and later platform gates
- [x] Close all Important/Critical findings before W1; reviewed b33c0f2/tree265bcb29, independent GO02:25UTC

## W1. Restore selected tools and fresh isolated dependency data

Files: tools/organization-tauri-qualification/{Cargo.toml,Cargo.lock,src/lib.rs}; docs/research/organization-tauri-windows-inventory/ evidence; external shared cache/toolchain.
- [x] Check disk and no active Cargo; obtain official Rust1.98.1 minimal host and cargo-deny0.20.2 prebuilt/checksums, record exact provenance/license/version
- [x] Write inert manifest as designed; static-test own workspace/publish=false/no build.rs/no executable/no root membership
- [x] Run explicit cargo generate-lockfile --manifest-path tools/organization-tauri-qualification/Cargo.toml; preserve initial lock
- [x] Only if needed, cargo update --manifest-path ... -p cssparser-macros --precise 0.7.0; preserve exact delta and final lock
- [x] Download every locked official crates.io archive as data to the single Cargo cache, verify lock checksum and confined regular/directory members before any Cargo extraction; abort on unsafe, duplicate, symlink, hardlink or special members
- [x] Retain cargo metadata --locked --offline --format-version 1 --manifest-path tools/organization-tauri-qualification/Cargo.toml as complete package/source lookup only, with inactive/other-target status retained
- [x] cargo metadata --locked --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc --manifest-path ...; preserve fresh output and source provenance
- [x] cargo tree --locked --offline --target x86_64-pc-windows-msvc --manifest-path ... with normal/build edges, no dedupe, proc-macro annotations/features; independently capture normal/no-proc projection

## W2. Verify inventory and scoped policy without package execution

Files: per-package inventory, fresh provenance manifest, lossless raw evidence, fixture-local scoped deny config and stdlib inventory/check tests if needed.
- [x] For any new inventory code, write failing synthetic tests first: both-role node, proc-macro host ancestry, build descendants, inactive package, wrong approved version/use, unsafe archive member and checksum mismatch
- [x] Implement minimal deterministic parser/checker; pass synthetic tests and actual cross-projection consistency checks
- [x] Match every selected crate archive to lock checksum; compare cache regular-file bytes against verified archive members, reject missing/extra/changed source and unsafe/duplicate/link/special members, identify Cargo bookkeeping separately; hash all regular sources/notices and retain unknown/native/tool exclusions
- [x] Fetch official current RustSec DB and record immutable SHA; scan unchanged root deny policy using explicit fixture manifest/target and preserve truthful raw result
- [x] Retain a conservative all-target unchanged-policy scan and machine-readable covered-package list; compare selected occurrence IDs against filtered/all-target scanner coverage and keep any uncovered selected host node unqualified/STOP; inactive failures stay separate
- [x] After exact role predicate passes, scan derived five-exception-only policy using exact version = "=x.y.z" and independent role equality; compare config against root and fail any new advisory/source/license scope
- [x] Cross-check current official stable2.x source, exact manifests and advisories; compare new findings against historical summary without identity claims

## W3. Independent evidence/CI review and durable handoff

Files: execution receipt, additive active/status, exact publication manifest generated outside repo.
- [x] Record passed/failed/not-run gates and exact fresh output hashes; no runtime or Windows-native claim
- [ ] Recheck current CI paths cannot execute new fixture dependencies merely on Draft creation
- [ ] Independent review of exact bytes, role scope, scans, source hashes, source/lock/policy preservation and all unresolved gates
- [ ] Parent publishes a distinct Draft and verifies exact tree/changed file hashes/current-head hosted gates; remain active until verified

W4 (build/runtime) is **NOT EXECUTABLE** from this plan. It requires the separate
gates in the design. If W1–W3 reveals prohibited or unverifiable scope, preserve
results, STOP dependent execution and report the exact blocker rather than bypassing.

Publication packaging amendment: raw metadata/tree/scanner/archive-member records
remain external and available to the reviewer. Durable bounded receipts, exact lock,
source validation scripts and upstream references are committed instead; no dependency
archive/source/SDK payload or raw log enters the publication tree.
