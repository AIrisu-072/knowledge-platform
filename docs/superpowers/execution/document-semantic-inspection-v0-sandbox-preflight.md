# Document Semantic Inspection v0 — Sandbox Preflight

- Status: **TASK 1 COMPLETE / PASS**
- Date: 2026-09-25
- Production Implementation Plan: `docs/superpowers/plans/2026-09-24-document-semantic-inspection-v0-production-implementation.md`
- Production baseline: `main@48045768d1d026eb785ee065877e401bbafd97ca`
- Baseline CI: `36079233862` — **SUCCESS**
- Implementation branch: `feat/document-semantic-inspection-v0`
- Implementation PR: **#10 (Draft)**
- RED contract head: `343aa9072da19da471a31b96e05eb92d80784820`
- RED hosted preflight: `36080157697` / job `107900137601` — **FAIL as expected**
- Qualified sandbox code/build head: `0cd3345a12f53f30068c72e56ea8aead367cd0ff`
- Last pre-lock GREEN evidence: `36084114757` — **SUCCESS**
- Sandbox candidate selection: **SELECTED FOR DSI v0 SANDBOX**
- Production parser dependency promotion: **NOT STARTED**
- Sandbox composition remains isolated until the production runner task.

## 1. RED evidence

The Task 1 contract was introduced before the sandbox launcher implementation.

Hosted Ubuntu RED run `36080157697` failed at compile time with unresolved imports for:

- `SandboxDisposition`;
- `SandboxLaunch`;
- `SandboxPolicy`;
- `run_sandboxed`.

That is the expected TDD RED boundary: the hostile sandbox contract existed before the launcher/enforcement implementation.

## 2. Selected sandbox composition

| Component | Version | Qualified role |
|---|---:|---|
| `landlock` | `0.4.7` | filesystem read/write confinement, ABI V3 hard requirement |
| `seccompiler` | `0.5.0` | seccomp-BPF denial of network and worker child-process syscalls |
| `libc` | `0.2.189` | RLIMIT, process groups, kill/wait primitives |
| `thiserror` | `2.0.21` | typed fail-closed error contract |

The experiment has its own committed `Cargo.lock`. Direct dependencies are exact-pinned and sandbox tests run with `--locked`.

No advisory/license/source exception was introduced.

## 3. Hosted GREEN evidence

Hosted Ubuntu run `36084114757` passed the Task 1 contract before the final lock/documentation refinements:

- ProductionResourceProfile tests: **4/4 PASS**;
- sandbox contract: **9/9 PASS**;
- cargo-deny: advisories / bans / licenses / sources **PASS**;
- `landlock 0.4.7` and `seccompiler 0.5.0` compiled and executed on Ubuntu 24.04.

The exact locked Task 1 tree is reverified by the final branch-head workflows after this report update.

## 4. Trust-boundary result

Qualified behavior:

- fresh process per inspection — PASS;
- TCP/UDP socket use — DENIED;
- DNS/network-dependent probe — DENIED;
- filesystem reads outside explicit read surface — DENIED;
- filesystem writes outside explicit private write surface — DENIED;
- explicit allowed read/write surface remains usable — PASS;
- credential-like environment variables — NOT INHERITED;
- production child-process creation — DENIED;
- controller wall timeout — KILLS process group;
- supervision test — GRANDCHILD DOES NOT SURVIVE;
- CPU limit — ENFORCED;
- address-space limit — ENFORCED;
- per-file output limit — ENFORCED;
- aggregate private-temp disk limit — ENFORCED;
- unsupported mandatory Landlock enforcement — FAIL CLOSED.

There is no insecure fallback.

## 5. Frozen `ProductionResourceProfile::DSI_V0`

### Carried forward exactly from PoC

```text
CPU time                 = 8 s
output file blocks       = 2,048
output file bytes        = 2 MiB
Linux virtual memory     = 2,097,152 KiB

DOCX archive entries     = 256
DOCX per-entry bytes     = 8 MiB
DOCX total uncompressed  = 32 MiB
DOCX XML depth           = 64

XLSX/XLSM entries        = 20,000
XLSX/XLSM uncompressed   = 512 MiB

PPTX entries             = 20,000
PPTX uncompressed        = 512 MiB
PPTX XML depth           = 256

PDF decompressed stream  = 64 MiB
```

### Additional finite production ceilings

```text
wall timeout             = 10,000 ms
input bytes              = 256 MiB
OOXML total uncompressed = 512 MiB
OOXML per-entry bytes    = 64 MiB
archive entries          = 20,000
XML depth                = 256
XML nodes                = 2,000,000
sheets                   = 1,024
cells                    = 1,000,000
slides                   = 4,096
shapes                   = 100,000
images                   = 4,096
decoded pixels           = 67,108,864
embedded objects         = 1,024
VBA modules              = 1,024
VBA source bytes         = 16 MiB
structured result bytes  = 16 MiB
stderr bytes             = 1 MiB
temp disk bytes          = 1 GiB
child processes          = 0
```

All 23 generic resource classes are tested for an explicit finite boundary. The synthetic boundary contract accepts the exact limit and rejects one-over.

## 6. Selection decision

Selection documents now record the sandbox composition:

- `spec/selection/library-tool-selection-v0.md`;
- `spec/selection/rust-library-matrix-v0.md`.

This is selection for the DSI v0 production sandbox composition, not yet promotion into a production runtime crate.

## 7. Task 1 decision

> **TASK 1 COMPLETE / PASS**

The production sandbox substrate and finite resource profile are qualified strongly enough to proceed to Task 2.

Next gate:

> **Task 2 — Production core contract and deterministic wire model**

Production parser dependencies remain unpromoted. Task 2 creates only the infrastructure-free semantic-inspection core contract.
