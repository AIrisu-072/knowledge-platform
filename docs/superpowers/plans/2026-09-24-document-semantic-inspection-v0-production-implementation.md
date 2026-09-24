# Document Semantic Inspection v0 — Production Implementation Plan

- Status: **DRAFT — AWAITING EXPLICIT USER APPROVAL**
- Date: 2026-09-24
- Capability: `Document Semantic Inspection v0`
- Frozen Design: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design.md`
- Design approval: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- Completed PoC report: `docs/superpowers/execution/document-semantic-inspection-v0-poc-report.md`
- Completed PoC plan: `docs/superpowers/plans/2026-09-20-document-semantic-inspection-v0-poc-qualification.md`
- Planning baseline: `test/document-semantic-inspection-poc-v0@245a3bad41f92d5a992560f60e3c26826abc20fb`
- Planned implementation branch: `feat/document-semantic-inspection-v0`
- Planned implementation baseline: **the exact `main` head after PR #8 is merged**
- Execution mode after approval: **task-by-task TDD**
- Production dependency promotion: **NOT YET PERFORMED**

> **Approval gate:** Writing this plan does not authorize production implementation. Do not create the implementation branch, add production parser/sandbox dependencies, or create production Semantic Inspection crates until this plan is explicitly approved by the user and PR #8 has been merged into `main`.

> **Merge gate:** PR #8 is currently Ready for review but unmerged. This plan does not authorize merging PR #8. Merge remains a separate explicit user action.

## 0. Goal

Promote the PoC-qualified Document Semantic Inspection v0 semantics into production while preserving the frozen trust boundary:

```text
authoritative FileObject
        |
        v
Document Application (trusted)
        |
        +-- cache lookup / raw-binding validation
        +-- read-only storage open
        +-- worker-result validation
        +-- immutable persistence
        |
        v
Production Sandbox Runner (trusted infrastructure adapter)
        |
        +-- fresh process
        +-- no credentials
        +-- no network
        +-- read-only input FD
        +-- private temp workspace
        +-- resource limits / timeout
        |
        v
Semantic Inspection Worker (untrusted parser zone)
        |
        +-- format detection
        +-- format-native parser/oracle composition
        +-- deterministic semantic projection
        +-- fingerprint / capabilities
        +-- editorial / external / signature evidence
        |
        v
structured worker result
        |
        v
Application validation
        |
        v
immutable SemanticInspectionRecord
```

The implementation must not create a durable cross-format content IR, must not depend on Search Extraction, and must not relax any PoC qualification or repository security/license gate.

## 1. Preconditions and hard invariants

### 1.1 Preconditions before Task 1 execution

All must be true:

1. Frozen Design remains `APPROVED — design freeze active`.
2. PoC Qualification remains PASS for all eight required formats.
3. PR #8 is merged into `main`.
4. This Production Implementation Plan is explicitly approved by the user.
5. `feat/document-semantic-inspection-v0` is created from the **exact merged `main` head**, not from this planning branch.
6. Fresh baseline CI on that exact head is green.

If any precondition is false, stop before production code or dependency promotion.

### 1.2 Frozen scope

Production v0 supports exactly:

- DOCX
- XLSX
- XLSM
- PPTX
- native-text PDF
- TXT
- CSV
- HTML

Still out of scope:

- DOC / XLS / PPT
- DOCM / PPTM
- OCR
- Search Extraction / Search Canonical Model / indexing
- detailed Document Diff
- macro execution
- external-reference dereference
- conversion/rendition generation
- Authority Migration execution
- antivirus product selection
- HTTP/OpenAPI/UI
- approval workflow

### 1.3 No semantic redesign during promotion

Production code must preserve the qualified `dsi-v0` semantic contract. The PoC fixture corpus is the compatibility oracle.

A proposed change that alters any of the following requires Design/profile review before implementation continues:

- version-significant semantics;
- noise invariance;
- editorial separation;
- capability meaning;
- cross-format equivalence semantics;
- VBA normalization;
- formula semantics;
- visual-content normalization;
- failure classification for a frozen case.

Parser implementation refactoring that preserves those outputs does not require a profile increment.

### 1.4 Dependency direction

Target dependency graph:

```text
document-domain
        ^
        |
document-application ----------------------+
        ^                                  |
        |                                  v
document-repository-postgres     document-semantic-inspection-core
        ^                                  ^
        |                                  |
        +----------------------+-----------+
                               |
             +-----------------+------------------+
             |                                    |
document-semantic-inspection-runner   document-semantic-inspection-worker
             |                                    |
             |                                    +--> qualified parser libraries
             |                                    +--> verified PDFium native binary
             |                                    '- vendored generated VBA parser
             |
             '--> Linux sandbox substrate
```

Rules:

- `document-semantic-inspection-core` contains protocol/value types only and has no parser, storage, SQL, or sandbox dependency.
- Worker must not depend on `document-domain`, `document-application`, repository, or storage crates.
- Worker protocol must not contain FileId, DocumentId, DocumentVersionId, Principal, StorageKey, DB credentials, or storage credentials.
- Runner is an infrastructure adapter and may depend on Application/core contracts.
- Application never imports a concrete parser library.
- Worker never persists results directly.

## 2. Production crate and file layout

Planned additions:

```text
crates/
├─ document-semantic-inspection-core/
│  └─ src/
│     ├─ lib.rs
│     ├─ profile.rs
│     ├─ format.rs
│     ├─ fingerprint.rs
│     ├─ evidence.rs
│     ├─ protocol.rs
│     ├─ error.rs
│     └─ canonical.rs
│
├─ document-semantic-inspection-worker/
│  ├─ build.rs
│  ├─ src/
│  │  ├─ lib.rs
│  │  ├─ main.rs
│  │  ├─ detect.rs
│  │  ├─ limits.rs
│  │  ├─ signatures.rs
│  │  ├─ vba_language.rs
│  │  └─ adapters/
│  │     ├─ text.rs
│  │     ├─ csv.rs
│  │     ├─ html.rs
│  │     ├─ docx.rs
│  │     ├─ spreadsheet.rs
│  │     ├─ vba.rs
│  │     ├─ pptx.rs
│  │     └─ pdf.rs
│  ├─ tests/
│  └─ testdata/
│
└─ document-semantic-inspection-runner/
   └─ src/
      ├─ lib.rs
      ├─ runner.rs
      ├─ sandbox.rs
      ├─ limits.rs
      └─ protocol.rs

crates/document-application/
  src/
    semantic_inspection.rs
    ports.rs
    error.rs
    service.rs
  tests/
    semantic_inspection_contract.rs
    semantic_inspection_vertical_slice.rs

crates/document-repository-postgres/
  migrations/
    0003_document_semantic_inspection_v0.sql
  src/
    semantic_inspection.rs
    semantic_inspection_rows.rs
  tests/
    semantic_inspection_schema.rs
    semantic_inspection_repository.rs
    semantic_inspection_concurrency.rs

third_party/
  document-semantic-inspection/
    tree-sitter-vba/
    pdfium/
    README.md
```

Keep `experiments/document-semantic-inspection/` as historical qualification evidence. Production crates must not depend on the PoC crate.

## 3. Production contract to implement

### 3.1 Public Application operation

```text
EnsureSemanticInspection
|- file_id
'- inspection_profile_version = "dsi-v0"
        |
        v
SemanticInspectionRecord
```

Application behavior:

```text
1. validate profile
2. load authoritative FileObject by file_id
3. lookup immutable (file_id, profile) record
4. cache hit:
   - compare FileObject hash/size to stored raw binding
   - mismatch -> IntegrityViolation
   - match -> return record
5. cache miss:
   - open authoritative file read-only
   - launch fresh sandbox worker
   - validate worker result
   - persist immutable result
   - return persisted/converged result
```

### 3.2 Worker request

The structured request contains only:

```text
inspection_profile_version
declared_media_type
expected_raw_content_hash
expected_size_bytes
trace_context
```

The authoritative bytes are supplied through a dedicated read-only inherited file descriptor/handle, not by a storage path.

### 3.3 Worker response

The response contains:

```text
profile_version
observed_raw_content_hash
observed_size_bytes
detected_format
semantic_fingerprint
semantic_capabilities[]
editorial_provenance
external_dependencies[]
digital_signature_evidence[]
extractor_provenance
diagnostics[]
```

No partial success response is valid.

### 3.4 Persisted record

Use immutable storage keyed by:

```text
(file_id, inspection_profile_version)
```

Persist:

- raw binding;
- detected format;
- SHA-256 semantic fingerprint;
- capability evidence;
- editorial provenance;
- external-dependency definitions;
- signature evidence;
- extractor provenance;
- diagnostics;
- inspected_at.

The record is derived data and may be rebuilt from authoritative FileObject.

## 4. Qualified production parser composition

The following parser composition has already passed the frozen PoC promotion gate and may be promoted **only after the Preconditions above are satisfied**:

| Area | Qualified composition |
|---|---|
| TXT | `encoding_rs 0.8.41` + `unicode-normalization` |
| CSV | `csv 1.4.0` |
| HTML | `html5ever 0.39.0` + `markup5ever_rcdom 0.39.0` |
| DOCX | `office_oxide 0.1.11` + `zip 8.6.0` deflate-only + `quick-xml 0.42.0` |
| XLSX/XLSM | `rxls 0.1.3` + `calamine 0.36.1` + raw SpreadsheetML oracle |
| VBA | `ovba 0.7.1` + `tree-sitter 0.25.10` + `tree-sitter-language 0.1.8` + generated VBA parser from `c691f237b2a703732d4b6a1f01d5b4f73f94d41e` |
| PPTX | `office_oxide 0.1.11` + raw PresentationML oracle/sentinel |
| PDF | `pdfium-render 0.9.4` + PDFium `151.0.7881.0` + `lopdf 0.45.0` |
| XMLDSig | `xml-sec 0.1.16` |
| CMS/X.509 | `cms 0.2.3` + `x509-cert 0.2.5` + vendored `openssl 0.10.81` |

Do not reintroduce any rejected candidate from the PoC report.

### 4.1 PDFium native identity

Production must pin and verify the same qualified native identity:

```text
Release: chromium/7881
PDFium: 151.0.7881.0

Linux x64:
1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d

macOS arm64:
52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40

macOS x64:
6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b
```

The installer/build helper must fail closed on hash mismatch or `VERSION BUILD != 7881`.

## 5. Sandbox gap identified after PoC

The PoC proved parser/resource behavior with child processes and OS limits, but its wrapper only applied:

- CPU: 8 seconds;
- output file limit: 2048 blocks;
- Linux virtual memory: 2,097,152 KiB;
- controller-side timeout tests.

That evidence is sufficient for the completed PoC gate, but **production still must implement the full frozen trust boundary**:

- no network;
- no inherited credentials;
- no arbitrary filesystem access;
- read-only authoritative input;
- private temporary workspace;
- fresh process;
- all mandatory resource-limit classes.

Therefore Linux sandbox substrate qualification is the first production task. Do not treat the PoC `ulimit` wrapper alone as a production sandbox.

## Task 1 — Sandbox substrate preflight and ProductionResourceProfile freeze

**Purpose:** close the production trust-boundary gap before adding sandbox libraries to production crates.

**Create initially under experiments only:**
- `experiments/document-semantic-inspection-sandbox/`
- `docs/superpowers/execution/document-semantic-inspection-v0-sandbox-preflight.md`

Candidate classes may include permissively licensed Linux primitives such as Landlock, seccomp, `no_new_privs`, rlimit/process APIs, and process supervision. A candidate name/version is not selected merely by appearing in this plan.

### Step 1 — RED sandbox contract

Create an isolated child that attempts each forbidden action:

- TCP/UDP socket creation/connect;
- DNS/network access;
- reading a file outside allowed runtime paths;
- writing outside private temp;
- reading a synthetic credential environment variable;
- reading a synthetic storage/database credential;
- exceeding CPU;
- exceeding memory;
- exceeding temp/output disk;
- exceeding wall-clock timeout;
- leaving child/grandchild processes after timeout.

Expected: current preflight fails until the sandbox actually denies them.

### Step 2 — Candidate dependency gate

For every sandbox dependency candidate:

```bash
cargo deny check advisories bans licenses sources
```

Require:

- repository-approved license;
- no advisory exception;
- no source-policy exception;
- no privileged daemon/service;
- no network service;
- auditable fail-closed behavior.

If no permissive composition satisfies the frozen boundary, **STOP** and return for a selection/design decision. Do not silently weaken the sandbox.

### Step 3 — Freeze `ProductionResourceProfile::DSI_V0`

Carry forward qualified values:

```text
CPU time                 = 8 s
output file blocks       = 2048
Linux virtual memory     = 2,097,152 KiB
normal worker timeout    = 10 s maximum unless a measured tighter value is selected
```

Also define explicit finite values for every mandatory frozen Design class:

- input bytes;
- decompressed OOXML total bytes;
- per-entry bytes;
- archive entry count;
- XML depth/node count;
- sheets/cells;
- slides/shapes;
- images/decoded pixels;
- embedded objects;
- VBA module count/source bytes;
- structured result bytes;
- stderr bytes;
- temp disk bytes;
- child process count.

Existing qualified PoC parser caps are starting evidence:

```text
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

For Design-required classes not numerically bounded in the PoC implementation, derive finite production values with synthetic boundary tests and record them in the preflight report before Task 2. Unlimited values are forbidden.

### Step 4 — Update selection evidence

Only after the sandbox contract and dependency gates pass:

- update `spec/selection/library-tool-selection-v0.md`;
- update `spec/selection/rust-library-matrix-v0.md`;
- record selected sandbox primitives and exact versions;
- record rejected candidates and reasons.

### Step 5 — Gate

Task 1 completes only when:

- no-network proof passes;
- filesystem confinement proof passes;
- credential/environment isolation proof passes;
- process/resource termination proof passes;
- every ProductionResourceProfile class has a finite number;
- cargo-deny passes.

## Task 2 — Production core contract and deterministic wire model

**Create:**
- `crates/document-semantic-inspection-core/`

**Modify:**
- root `Cargo.toml`
- root `Cargo.lock`

### Step 1 — RED contract tests

Pin tests for:

- `InspectionProfileVersion("dsi-v0")`;
- `FormatId` required eight-format set;
- `SemanticFingerprint` exactly SHA-256 / 32 bytes;
- `CapabilityState = Present | Absent | NotRepresentable | NotVerifiable`;
- capability evidence shape;
- editorial evidence shape;
- external dependency shape;
- signature evidence shape;
- extractor provenance shape;
- worker request/response serialization;
- canonical map/list ordering where order is not semantically significant;
- rejection of unknown protocol versions;
- bounded decode of worker result.

### Step 2 — GREEN core types

Implement infrastructure-free types only.

No parser or OS sandbox imports are allowed in this crate.

### Step 3 — Golden protocol snapshots

Create versioned JSON/protocol golden snapshots. Reordering serializer maps or changing enum names must not occur accidentally under `dsi-v0`.

### Step 4 — Verify

```bash
cargo test -p document-semantic-inspection-core
cargo deny check
mise run verify:fast
```

## Task 3 — Worker shell, raw binding, format detection, and fail-closed protocol

**Create:**
- `crates/document-semantic-inspection-worker/`
- binary `document-semantic-inspection-worker`

### Step 1 — RED worker protocol tests

Prove:

- worker consumes only the allowed request fields;
- input arrives through inherited read-only FD/handle;
- worker recomputes SHA-256 and byte count;
- wrong expected hash -> `RawBindingMismatch`;
- wrong expected size -> `RawBindingMismatch`;
- declared/detected format mismatch -> `FormatMismatch`;
- unknown format -> `UnsupportedDocumentFormat`;
- malformed request -> controlled non-zero error;
- panic does not produce success output;
- no partial result is emitted.

### Step 2 — GREEN detection

Detect using content/container structure plus declared-media compatibility, never filename extension alone.

### Step 3 — Worker result provenance

Include:

- worker build id;
- adapter id/version;
- exact parser library versions;
- native dependency identity/hash.

Do not include parser provenance in semantic fingerprint.

## Task 4 — Promote TXT / CSV / HTML semantics

Port qualified logic with minimal semantic change.

### RED

Use promoted synthetic fixtures and assert exact parity with the PoC-qualified expected relations:

TXT:
- CRLF/LF invariant;
- Unicode normalization invariant;
- content change differs;
- ambiguous decode fails closed.

CSV:
- quoted syntax noise invariant;
- row/cell changes differ;
- inconsistent structure fails;
- ambiguous delimiter fails.

HTML:
- whitespace/decorative noise invariant;
- visible text/link/image changes differ;
- script-required semantics fail without script execution.

### GREEN

Promote only:

- `encoding_rs 0.8.41`;
- `unicode-normalization`;
- `csv 1.4.0`;
- `html5ever 0.39.0`;
- `markup5ever_rcdom 0.39.0`.

Do not add `scraper`.

## Task 5 — Promote DOCX semantics and OOXML coverage sentinel

### RED

Require qualified fixtures for:

- body/heading/list order;
- table structure/merge;
- header/footer;
- footnote/endnote;
- hyperlink target;
- image semantics;
- sections;
- proposed-final tracked-change projection;
- comments/editorial evidence;
- serialization/relationship/package-order noise;
- malformed/deep/oversized package;
- unknown potentially semantic part.

### GREEN

Promote:

- `office_oxide 0.1.11`;
- `zip 8.6.0` with `default-features = false`, deflate only;
- `quick-xml 0.42.0`;
- independent project-owned raw OOXML oracle/sentinel.

Unknown potentially semantic content/relationship must fail closed.

## Task 6 — Promote XLSX / XLSM / VBA semantics

### RED

Require:

- sheet add/remove/order/visibility;
- cell value/type;
- formula source;
- cached-result noise;
- defined names;
- merges/tables;
- links;
- chart/image semantics;
- external workbook/ODBC definition semantics without dereference;
- parser/oracle disagreement handling;
- VBA logic change differs;
- VBA whitespace/comment/case-equivalent noise stays equal;
- invalid/incomplete VBA fails closed.

### GREEN

Promote:

- `rxls 0.1.3`;
- `calamine 0.36.1` with picture support;
- `ovba 0.7.1`;
- `tree-sitter 0.25.10`;
- `tree-sitter-language 0.1.8`;
- vendored generated parser from exact VBA grammar revision `c691f237b2a703732d4b6a1f01d5b4f73f94d41e`.

VBA is static-only and never executable.

## Task 7 — Promote PPTX semantics

### RED

Require:

- slide add/remove/order;
- text/shape association;
- table;
- chart;
- SmartArt;
- image;
- hyperlink;
- speaker note;
- grouping/object relationship;
- comment-only editorial change;
- theme/font/background/internal-ID/package-order noise;
- unknown semantic package part fail-closed.

### GREEN

Use `office_oxide 0.1.11` plus independent raw PresentationML oracle/sentinel.

The raw oracle remains authoritative for qualified chart/SmartArt/package coverage that the typed library does not fully expose.

## Task 8 — Promote PDF and digital-signature evidence

### Step 1 — RED PDF semantics

Require:

- text;
- page order;
- link;
- visible form value;
- image;
- annotation as editorial evidence;
- producer/object-id noise;
- scan-only -> `RequiresOcr`;
- encrypted -> `EncryptedContentUnsupported`;
- broken xref -> controlled extraction failure;
- PDFium/lopdf disagreement -> `ParserDisagreement`;
- ambiguous read order -> fail closed.

### Step 2 — GREEN PDF composition

Promote:

- `pdfium-render 0.9.4`, `pdfium_7881`, `thread_safe`;
- exact PDFium `151.0.7881.0`;
- `lopdf 0.45.0`, default features disabled.

No “trust one parser” fallback is allowed on required semantic disagreement.

### Step 3 — RED signatures

Cover:

- CMS valid/tampered/digest/expired/revoked/unknown issuer/broken chain/unsupported algorithm/malformed;
- XMLDSig valid/invalid/unverifiable;
- PDF ByteRange exact covered-byte reconstruction;
- OOXML OPC signature origin traversal for DOCX/XLSX/PPTX.

### Step 4 — GREEN signatures

Promote qualified signature stack.

Trust inputs are explicit caller/configuration data only. Do not use system trust and do not perform network CRL/OCSP/AIA retrieval.

Invalid/unverifiable signatures remain evidence and do not change the semantic fingerprint.

## Task 9 — Production Linux sandbox runner

**Create:**
- `crates/document-semantic-inspection-runner/`

Production deployment sandbox support is Linux-first. Worker semantic code remains portable for cross-host parity testing.

### Step 1 — RED process isolation

Use a synthetic malicious/self-test worker to prove:

- fresh process per inspection;
- no inherited environment except explicit allowlist;
- no credential variables;
- network syscalls denied;
- input available only through read-only FD;
- no authoritative storage path present in request/env/argv;
- private temp workspace;
- arbitrary filesystem reads/writes denied after sandbox sealing;
- child process creation denied or bounded by the frozen profile;
- timeout kills the whole process tree;
- stdout/stderr/result size limits enforced.

### Step 2 — GREEN sandbox bootstrap

Required order:

```text
trusted runner
  -> create private temp
  -> stream authoritative bytes to private read-only input
  -> verify/carry expected raw binding from FileObject
  -> construct minimal request
  -> spawn worker with sanitized environment
  -> apply OS process/resource restrictions
  -> worker initializes required native runtime
  -> seal filesystem access to the allowed private runtime surface
  -> run exactly one inspection
  -> capture bounded structured result
  -> kill/reap on timeout/failure
  -> remove private temp
```

There is no insecure fallback. If mandatory sandbox controls cannot be installed, return `ExtractorUnavailable`.

### Step 3 — Error mapping

Map:

- wall timeout -> `InspectionTimeout`;
- CPU/memory/disk/object bound -> `InspectionResourceLimitExceeded`;
- worker unavailable/sandbox unavailable -> `ExtractorUnavailable`;
- malformed/oversized worker response -> `InvalidWorkerResult`;
- raw mismatch -> `RawBindingMismatch`.

### Step 4 — log leakage test

Generic stderr/telemetry must not contain representative body text, comments, VBA source, cell values, or signature payload bytes.

## Task 10 — Application ports and EnsureSemanticInspection orchestration

**Modify:**
- `crates/document-application/src/ports.rs`
- `crates/document-application/src/error.rs`
- `crates/document-application/src/service.rs`
- `crates/document-application/src/lib.rs`

**Create:**
- `crates/document-application/src/semantic_inspection.rs`
- `crates/document-application/tests/semantic_inspection_contract.rs`

### Step 1 — RED ports

Define production contracts equivalent to:

```text
SemanticInspectionRepository
  get_file_object(file_id)
  get_semantic_inspection(file_id, profile)
  insert_or_converge_semantic_inspection(record)

SemanticInspectionExecutor
  inspect(request, content_reader) -> WorkerResult
```

Repository returns trusted authoritative FileObject metadata. Add a validated restoration path in `document-domain` if required; do not expose SQL rows or storage paths to the worker.

### Step 2 — RED service ordering

Prove exact order:

```text
validate profile
load FileObject
lookup cache
cache hit -> verify raw binding -> return
cache miss -> storage.open
executor.inspect
validate worker result
repository.insert_or_converge
return persisted result
```

### Step 3 — GREEN result validation

Application must reject:

- profile mismatch;
- observed hash/size mismatch;
- unknown detected format;
- incompatible declared/detected format;
- non-SHA-256 or non-32-byte fingerprint;
- malformed capability contract;
- duplicate/conflicting capability ids;
- over-limit evidence/diagnostic output;
- invalid provenance shape.

Worker cannot persist its own result.

### Step 4 — Failure mapping

Expose frozen inspection failures distinctly enough for Document Versioning to fail closed.

Integrity-significant failures map to the existing authoritative `IntegrityViolation`/explicit inspection error boundary without being silently retried.

## Task 11 — PostgreSQL immutable record persistence and concurrency convergence

**Create:**
- `crates/document-repository-postgres/migrations/0003_document_semantic_inspection_v0.sql`
- `crates/document-repository-postgres/src/semantic_inspection.rs`
- `crates/document-repository-postgres/src/semantic_inspection_rows.rs`
- `crates/document-repository-postgres/tests/semantic_inspection_schema.rs`
- `crates/document-repository-postgres/tests/semantic_inspection_repository.rs`
- `crates/document-repository-postgres/tests/semantic_inspection_concurrency.rs`

### Step 1 — RED schema contract

Require:

- FK to authoritative `file_objects`;
- unique `(file_id, inspection_profile_version)`;
- exact raw hash length;
- non-negative size;
- fingerprint algorithm constrained to SHA-256 for v0;
- fingerprint digest exactly 32 bytes;
- immutable record behavior through repository API;
- no successful partial row.

### Step 2 — GREEN migration

Use typed scalar columns for identity/raw binding/fingerprint/provenance identifiers and JSONB for structured evidence where appropriate.

Do not store the format-native parse model or common durable content IR.

### Step 3 — Concurrent convergence

Run two inspections for the same `(file_id, profile)`.

Allowed:

- duplicate worker computation.

Required persistence behavior:

```text
same raw binding + meaningfully identical result
    -> one row, both callers receive the same persisted result

same raw binding + meaningfully different result
    -> SemanticInspectionDeterminismViolation
    -> do not pick a winner
```

Comparison for convergence includes the complete deterministic result contract required by the frozen Design, not merely semantic fingerprint.

### Step 4 — Cache integrity

If stored raw hash/size no longer matches authoritative FileObject metadata, return IntegrityViolation. Do not convert it into a cache miss/reinspection.

## Task 12 — Production vertical slice

**Create:**
- `crates/document-application/tests/semantic_inspection_vertical_slice.rs`

Use real:

- `FileSystemStorage`;
- `PostgresDocumentRepository`;
- production Application service;
- production Linux sandbox runner where host supports it;
- production worker;
- synthetic fixture only.

Prove:

```text
create authoritative file
  -> ensure dsi-v0
  -> fresh worker
  -> validated result
  -> immutable database record
  -> second ensure is cache hit
  -> no second worker launch
```

Also prove:

- raw binding mismatch -> IntegrityViolation;
- corrupt/unsupported document -> no successful row;
- timeout/resource breach -> no successful row;
- worker malformed result -> no successful row;
- scan-only PDF -> RequiresOcr;
- encrypted content -> correct unsupported error;
- invalid signature can persist as explicit evidence;
- external references are not dereferenced;
- VBA is not executed.

## Task 13 — PoC-to-production parity gate

Production promotion is not complete merely because production unit tests pass.

### Step 1 — Promote qualification corpus

Copy the synthetic fixture corpus and immutable expectations from the qualified PoC head into production testdata or a neutral repository testdata location.

Record source provenance:

```text
PoC qualification code head:
a4fcef1cb5cac5672199165f433bd303c25135a6

PoC final documentation head:
245a3bad41f92d5a992560f60e3c26826abc20fb
```

Do not introduce customer/institution documents.

### Step 2 — Exact parity

All 91 manifest cases must preserve the qualified relation/error expectation.

Additionally require the qualified supplemental:

- XLSM/VBA cases;
- signature vectors;
- cross-format capability tests;
- determinism/security tests.

### Step 3 — Repeat determinism

For every successful production fixture:

- 20 in-process/worker-level repeated inspections;
- 5 fresh worker process snapshots under locale/timezone variation;
- exact deterministic result equivalence.

### Step 4 — Cross-host semantic parity

Run worker semantic tests on:

- Ubuntu;
- macOS Intel;
- macOS arm64.

Linux-only OS sandbox enforcement tests run on Ubuntu. macOS remains a semantic/parser portability gate, not the production sandbox deployment target.

## Task 14 — CI, assurance, self-review, and merge gate

**Modify as required:**
- `.github/workflows/ci.yml`
- add a production DSI workflow only if standard CI cannot express the cross-host/native matrix cleanly;
- `mise.toml`;
- assurance metadata/controls required by repository policy;
- `docs/superpowers/execution/document-semantic-inspection-v0-status.md`;
- `docs/superpowers/execution/active.md`.

### Step 1 — Repository gates

Run:

```bash
mise run verify:fast
mise run verify
mise run verify:full
cargo deny check
```

All must PASS.

### Step 2 — Explicit production DSI evidence

At minimum:

```bash
cargo test -p document-semantic-inspection-core
cargo test -p document-semantic-inspection-worker
cargo test -p document-semantic-inspection-runner
cargo test -p document-application --test semantic_inspection_contract -- --nocapture
cargo test -p document-application --test semantic_inspection_vertical_slice -- --nocapture
cargo test -p document-repository-postgres --test semantic_inspection_schema -- --nocapture
cargo test -p document-repository-postgres --test semantic_inspection_repository -- --nocapture
cargo test -p document-repository-postgres --test semantic_inspection_concurrency -- --nocapture
```

### Step 3 — Architecture self-review

Explicitly verify:

```text
Search Extraction dependency                         absent
durable common cross-format content IR              absent
worker receives FileId/DocumentId/StorageKey        absent
worker DB/storage credentials                        absent
macro execution                                      absent
external-reference dereference                       absent
network access from worker                           denied
fresh process per inspection                         implemented
private temp/read-only input                         implemented
finite resource classes                              implemented
raw-binding recomputation                            implemented
format mismatch fail-closed                          implemented
partial-success persistence                          absent
cache-hit raw-binding integrity check                implemented
immutable (file_id, profile) record                  implemented
concurrent determinism violation                     implemented
qualified parser composition only                    implemented
rejected PoC dependencies                            absent
PDFium exact native identity/hash                    implemented
signature evidence outside semantic fingerprint      implemented
invalid/unverifiable signature evidence              preserved
91-case PoC parity                                   PASS
cross-host semantic parity                           PASS
```

Any frozen-contract mismatch requires an explicit Design/profile amendment before continuing.

### Step 4 — Exact-head hosted CI

After every final branch-tree change, require hosted CI on that exact head.

Do not claim completion from local results only.

### Step 5 — Review PR feedback

Fetch:

- all required workflow jobs for exact head;
- all submitted reviews;
- all inline review threads.

Fix blocking findings with RED -> GREEN evidence and rerun exact-head CI.

### Step 6 — Stop at merge gate

When:

- all Tasks 1–14 are complete;
- exact-head CI is green;
- production DSI cross-host gate is green;
- no blocking review thread remains;
- execution status contains exact evidence;

mark the implementation PR Ready for review.

**STOP. Do not merge the production implementation PR without an explicit user merge instruction.**

## 6. Production completion criteria

Document Semantic Inspection v0 is production-complete only when all are true:

```text
Frozen Design preserved                          PASS
PoC-qualified 8-format composition promoted     PASS
91/91 production parity cases                    PASS
supplemental VBA/signature gates                 PASS
cross-format capability gate                     PASS
determinism gate                                 PASS
fresh-process sandbox                            PASS
no-network / no-credential sandbox               PASS
filesystem/resource containment                  PASS
raw binding / format detection                   PASS
Application result validation                    PASS
immutable PostgreSQL persistence                 PASS
concurrent convergence/determinism violation     PASS
Ubuntu production sandbox CI                     PASS
Ubuntu/macOS Intel/macOS arm64 semantic CI       PASS
cargo-deny                                       PASS
standard repository required-check               PASS
blocking review findings                         0
```

## 7. Execution handoff after approval

After explicit approval, the exact next sequence is:

```text
1. verify PR #8 is merged
2. fetch exact merged main SHA
3. verify main exact-head CI
4. create feat/document-semantic-inspection-v0 from that SHA
5. update Active/Status with implementation baseline
6. execute Task 1 only
7. do not advance past a failed hard gate
8. continue task-by-task with TDD and exact evidence
```

Until approval, the repository phase remains:

> **PRODUCTION IMPLEMENTATION PLAN REVIEW — NO PRODUCTION CODE AUTHORIZED**
