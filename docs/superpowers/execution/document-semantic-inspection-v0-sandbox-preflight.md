# Document Semantic Inspection v0 — Sandbox Preflight

- Status: **TASK 1 RED CONTRACT**
- Date: 2026-09-25
- Production Implementation Plan: `docs/superpowers/plans/2026-09-24-document-semantic-inspection-v0-production-implementation.md`
- Production baseline: `main@48045768d1d026eb785ee065877e401bbafd97ca`
- Baseline CI: `36079233862` — **SUCCESS**
- Implementation branch: `feat/document-semantic-inspection-v0`
- Implementation PR: **#10 (Draft)**
- Sandbox candidate selection: **NOT YET SELECTED**
- Production dependency promotion: **NOT STARTED**

## RED contract

The first Task 1 commit intentionally defines the hostile sandbox contract before any sandbox implementation or candidate dependency is added.

Required proofs:

- TCP socket creation denied;
- UDP socket creation denied;
- outside-filesystem read denied;
- outside-private-temp write denied;
- explicit allowed read/write surface remains usable;
- database/storage/credential-like environment variables are not inherited;
- CPU limit terminates the child;
- address-space limit prevents excessive allocation;
- output-file limit prevents excessive writes;
- wall timeout terminates the worker;
- production profile denies child-process creation;
- supervision test kills the whole process group.

The first hosted run is expected to fail at the unresolved sandbox launcher contract. That failure is the Task 1 RED evidence, not a production defect.

## Candidate gate

No candidate is selected by this RED commit. Candidate composition must pass:

- repository license allowlist;
- cargo-deny advisories/bans/licenses/sources;
- no policy exception;
- no privileged daemon/service;
- no network service dependency;
- fail-closed behavior on unsupported kernel/security capability.

## Resource-profile evidence carried from PoC

Already qualified:

```text
CPU time                 = 8 s
output file blocks       = 2048
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

Task 1 must still freeze finite production values for every remaining Design-required class before it can complete.

## Next exact action

Run the hosted Ubuntu RED contract on the exact Task 1 RED head, record the expected failure, then add candidate dependencies only inside this experiment and qualify them against the same contract.
