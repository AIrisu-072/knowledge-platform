# Document Semantic Inspection v0 — Design

- Status: **APPROVED — design freeze active**
- Date: 2026-09-20
- Capability: `Document Semantic Inspection v0`
- Approval record: `docs/superpowers/specs/2026-09-20-document-semantic-inspection-v0-design-approval.md`
- Design branch: `design/document-semantic-inspection-v0`
- Baseline: `main@73492983dd324fcd53d4b485719e5c31048f9335`

## 0. Purpose

Document Semantic Inspection v0 determines, for one authoritative file, the version-significant semantics needed by Document Versioning without coupling Document Platform to Search indexing.

It is deliberately distinct from Search Extraction.

```text
FileObject (authoritative original)
        |
        +--> Document Semantic Inspection
        |      |- format-native semantic inspection
        |      |- semantic fingerprint
        |      |- semantic capability evidence
        |      |- editorial provenance
        |      |- external dependencies
        |      '- digital-signature evidence
        |
        +--> Search Extraction
               '- Search Canonical Model -> KnowledgeUnit -> Search Index
```

Document Semantic Inspection does **not** create a common cross-format content IR for storage, search, or reconstruction. Format-native parse models are temporary worker-local state.

The capability exists first to support `Document Versioning v0`; later `Document Diff v0`, Authority Migration, Publish quality gates, and evidence presentation may reuse its persisted derived evidence.

## 1. Architectural boundary

### 1.1 Capability graph

```text
                     FileObject
                         |
                         v
          Document Semantic Inspection v0
          |- format-native parsing
          |- format-native normalization
          |- semantic fingerprint
          |- semantic capabilities
          |- editorial provenance
          |- external dependencies
          |- digital-signature evidence
          '- cross-format equivalence support
                         |
             +-----------+-----------+
             v                       v
   Document Versioning v0       Document Diff v0
   |- Version identity          |- base / target reparse
   |- WORKING management        |- format-native comparator
   |- Publish gates             '- Common ChangeSet
   '- Rendition / Authority

Separate path:

FileObject
   -> Search Extraction
   -> Search Canonical Model
   -> KnowledgeUnit
   -> Search Index
```

### 1.2 Responsibilities

Document Semantic Inspection v0 owns:

- inspection of exactly one file at a time;
- format detection and declared-format consistency checks;
- format-native semantic normalization;
- one deterministic semantic fingerprint per file/profile;
- semantic capability evidence used by cross-format equivalence;
- editorial provenance extraction;
- external-dependency extraction without dereference;
- digital-signature evidence extraction and verification status;
- deterministic, immutable derived inspection records;
- fail-closed classification when version-significant semantics cannot be completely inspected.

It does not own:

- DocumentVersion creation, update, publication, or OCC;
- Version-level manifest/fingerprint construction;
- detailed base-vs-target diff generation;
- Search Canonical Model or Search Index;
- OCR;
- macro execution;
- external-reference dereference;
- attachment management;
- approval workflow;
- file conversion / rendition generation;
- Authority Migration execution;
- antivirus / malware verdict policy beyond the sandbox boundary defined here.

## 2. Versioning semantics carried into this capability

Document Versioning v0 is designed around these frozen principles:

- one Document has at most one WORKING Version;
- a new Version represents reader-relevant content change, not management-metadata change;
- Version identity includes normalized title plus the ordered ContentItem manifest;
- one DocumentVersion may contain one or more ContentItems;
- each ContentItem has exactly one authoritative representation and zero or more renditions;
- rendition addition/re-generation does not by itself create a new Version;
- each authoritative file contributes its format-native semantic fingerprint;
- `logical_path + ordinal` are version-significant;
- raw SHA-256 is binary identity/integrity, not semantic identity;
- same raw bytes are not required for semantic equality;
- a file that cannot be semantically inspected under the selected profile cannot participate in Version create/update;
- cross-format equivalence does not use Search indexing;
- authoritative representation migration is permitted only when all source version-significant semantic capabilities are preserved and verifiable by the target representation.

## 3. No common content IR

The design explicitly rejects a single durable cross-format intermediate representation for Document Versioning.

Reasons:

- DOCX tracked-change semantics, XLSX formulas, XLSM VBA, PPTX slide/object structure, and PDF page/object structure have different semantic models;
- a common content IR tends to erase format-native meaning that matters to Version identity;
- Search may legitimately use a lossy Search Canonical Model, but Document Versioning may not;
- reconstruction of the source file from an intermediate form is unnecessary because the original FileObject remains authoritative.

The required property is **bidirectional traceability**, not reversible serialization:

```text
semantic evidence / diff / search result
        -> source locator
        -> original authoritative FileObject location
```

## 4. Data contract

### 4.1 Application contract

```text
EnsureSemanticInspection
|- file_id
'- inspection_profile_version
        |
        v
SemanticInspectionRecord
```

The caller does not supply media type, content hash, size, or storage location. The Application obtains those from the authoritative FileObject.

### 4.2 InspectionProfileVersion

`inspection_profile_version` versions the semantic contract, not the parser implementation.

A new profile is required when any version-significant rule changes, including:

- semantic-normalization rules;
- which constructs are version-significant;
- VBA normalization rules;
- visual-content normalization rules;
- formula normalization;
- semantic-capability definitions;
- cross-format equivalence semantics.

A parser/library patch that preserves the same semantic contract does not by itself increment the profile. Concrete parser/build versions are captured in extractor provenance.

### 4.3 Worker input

```text
InspectionJob
|- inspection_profile_version
|- declared_media_type
|- expected_raw_content_hash
|- expected_size_bytes
|- read-only content stream / handle
'- W3C trace context
```

The worker does not receive:

- FileId;
- DocumentId or DocumentVersionId;
- Principal;
- database credentials;
- storage credentials;
- storage paths/keys;
- unrelated document data.

### 4.4 Persisted successful record

Successful records are immutable derived data keyed by:

```text
(file_id, inspection_profile_version)
```

Conceptual schema:

```text
SemanticInspectionRecord
|- file_id
|- inspection_profile_version
|- raw_binding
|  |- raw_content_hash
|  '- size_bytes
|- detected_format
|- semantic_fingerprint
|- semantic_capabilities[]
|- editorial_provenance
|- external_dependencies[]
|- digital_signature_evidence[]
|- extractor_provenance
|- diagnostics[]
'- inspected_at
```

A record is not authoritative. It is reproducible from FileObject.

A record's existence means that inspection is complete enough for Version identity under that profile. Partial semantic results are never persisted as successful records.

### 4.5 SemanticFingerprint

```text
SemanticFingerprint
|- algorithm = SHA-256
'- digest = 32 bytes
```

The digest covers the format-native, version-significant semantic projection, deterministically serialized.

Examples of included semantics:

- reader-visible text and meaningful structure;
- formulas and reference definitions;
- hidden business content where defined as semantic;
- normalized visual content;
- VBA logic/project structure for XLSM.

Excluded from semantic identity:

- save timestamps;
- author/last-modified metadata;
- comments and review metadata;
- Track Changes metadata itself;
- digital-signature wrappers;
- parser-generated IDs;
- serializer/container noise;
- pure decoration such as font/spacing where it does not alter information meaning.

### 4.6 SemanticCapabilityEvidence

Cross-format equivalence uses capability evidence, not direct fingerprint equality.

```text
SemanticCapabilityEvidence
|- capability_id
|- presence
|- version_significant
'- equivalence_fingerprint? 
```

The model must distinguish at least:

- PRESENT;
- ABSENT;
- NOT_REPRESENTABLE / NOT_VERIFIABLE where the format cannot preserve or prove a source capability.

Examples:

XLSM may expose:

- reader_content;
- workbook_structure;
- formula_logic;
- vba_logic;
- hidden_content;
- external_references.

PDF may expose reader content while being unable to represent/verify VBA or spreadsheet formula logic.

Authority Migration may proceed only when every source version-significant capability is preserved and verifiable in the target.

### 4.7 EditorialProvenance

Editorial evidence is separated from Version identity.

```text
EditorialProvenance
|- tracked_changes[]
|  |- kind
|  |- author_label
|  |- timestamp
|  |- source_locator
|  '- unresolved
|- comments[]
|  |- author_label
|  |- timestamp
|  |- resolved_state
|  |- source_locator
|  '- content
|- document_author_labels
|- last_modified_by
'- modification_metadata
```

Office author labels are evidence claims, not authenticated identity.

System-level "who changed what" remains based on Document Audit + later Document Diff. Approval identity/timestamp/content binding belongs to system-side Approval Records.

### 4.8 Track Changes semantics

WORKING files may contain unresolved tracked changes.

For semantic comparison, inspection computes a proposed-final projection equivalent to:

- insertion -> included;
- deletion -> excluded.

The source file is never modified or auto-accepted.

Tracked-change evidence remains in EditorialProvenance.

Publish later fails closed while unresolved tracked changes remain.

### 4.9 Comments

Comments are allowed in WORKING files and extracted into EditorialProvenance.

Publish later requires zero embedded comments, resolved or unresolved. The platform does not auto-delete them.

### 4.10 DigitalSignatureEvidence

```text
DigitalSignatureEvidence
|- signature_type
|- signer_claim
|- certificate_subject
|- certificate_issuer
|- certificate_fingerprint
|- signed_at
|- cryptographic_validity
|- covered_content
'- validation_diagnostics
```

Digital signatures are not Version identity.

A file without a digital signature may be published in v0.

If a signature exists but is invalid or unverifiable, inspection may still succeed with explicit evidence; Publish must fail closed.

System-side Approval Records remain the authoritative approval mechanism.

### 4.11 ExternalDependency

```text
ExternalDependency
|- dependency_kind
|- normalized_reference
|- source_locator
'- version_significant
```

External references are parsed as definitions only. They are never dereferenced by inspection.

Changing an external-reference definition is a semantic change. Changing the external resource contents without changing the document does not alter that document's Version.

### 4.12 ExtractorProvenance

```text
ExtractorProvenance
|- worker_build_id
|- adapter_id
|- adapter_version
|- parser_libraries[]
|- parser_library_versions[]
'- native_dependency_identity[]
```

For native engines such as PDFium, exact binary/build identity and hash must be recorded.

Extractor provenance is not part of semantic fingerprint.

### 4.13 Diagnostics

Diagnostics record non-fatal facts such as:

- hidden sheets;
- external references;
- tracked changes;
- comments;
- macros;
- signatures.

Diagnostics cannot be used to represent partial semantic success.

## 5. Processing flow

```text
EnsureSemanticInspection(file_id, profile)
        |
        v
1. validate profile
2. load authoritative FileObject
3. lookup (file_id, profile)
   |- hit -> verify raw binding -> return
   '- miss
4. open FileObject read-only
5. launch fresh sandbox worker
6. worker recomputes raw hash/size
7. detect format and validate declared format
8. run format-native semantic inspection
9. produce fingerprint/evidence
10. Application validates worker result
11. insert immutable record
12. return record
```

### 5.1 Cache hit

A cache hit must verify:

- FileObject content hash equals recorded raw hash;
- FileObject size equals recorded size.

Mismatch is an IntegrityViolation, not a cache miss.

### 5.2 Format detection

Adapter selection must not trust extension or declared media type alone.

Detection combines:

- container/magic structure;
- format-native structural checks;
- compatibility with declared media type.

Mismatch fails closed.

### 5.3 Application validation of worker result

The Application validates at least:

- observed hash and size;
- requested/returned profile identity;
- detected format compatibility;
- semantic-fingerprint algorithm/length;
- semantic-capability contract shape;
- configured output limits.

A worker cannot directly persist its own output.

### 5.4 Concurrent ensure

Concurrent inspection of the same `(file_id, profile)` may perform duplicate work, but persistence converges via a unique key.

If two executions over the same raw file/profile produce meaningfully different semantic results, the system does not pick a winner. It raises `SemanticInspectionDeterminismViolation`.

### 5.5 Parser update

Implementation/library updates may remain under the same profile only if corpus evidence shows semantic-contract compatibility.

If a parser defect invalidates prior derived records, an explicit derived-cache rebuild/repair operation is used. Authoritative FileObject is not mutated.

## 6. Sandbox and trust boundary

The parser layer is treated as untrusted.

```text
TRUSTED
+--------------------------------+
| Document Application           |
| PostgreSQL                     |
| File Storage                   |
| result validation              |
+----------------+---------------+
                 | one file
=================|================
                 v
UNTRUSTED PARSER ZONE
+--------------------------------+
| fresh sandbox process          |
| no network                     |
| no credentials                 |
| read-only input                |
| private temporary workspace    |
| CPU / memory / time limits     |
| expansion / object limits      |
| never execute macros           |
+----------------+---------------+
                 | structured result
=================|================
                 v
       Application validation
                 |
                 v
       immutable derived record
```

v0 default is one fresh process per inspection. Worker pooling is a later optimization requiring its own safety validation.

The worker has no DB or File Storage credentials and cannot read arbitrary documents.

### 6.1 Mandatory resource-limit classes

Numeric values are set by PoC, but unlimited processing is prohibited. Limits must exist for:

- wall-clock time;
- CPU time;
- memory;
- temp disk;
- input size;
- decompressed OOXML size;
- XML nesting/node counts;
- sheets/cells;
- slides/shapes;
- images/decoded pixels;
- embedded objects;
- VBA modules/source size.

### 6.2 OOXML/container safety

Adapters must defend against:

- archive bombs;
- path traversal;
- oversized entries;
- conflicting/duplicate entries;
- deep XML nesting;
- malformed relationship graphs.

VBA is statically inspected and never executed.

## 7. Failure model

### 7.1 Inspection-failure errors

No successful SemanticInspectionRecord is created for:

- `UnsupportedDocumentFormat`;
- `RequiresOcr`;
- `EncryptedContentUnsupported`;
- `FormatMismatch`;
- `RawBindingMismatch`;
- `SemanticExtractionFailed`;
- `InspectionTimeout`;
- `InspectionResourceLimitExceeded`;
- `ExtractorUnavailable`;
- `InvalidWorkerResult`;
- `SemanticInspectionDeterminismViolation`.

Integrity/security-significant failures include at least:

- RawBindingMismatch;
- InvalidWorkerResult;
- SemanticInspectionDeterminismViolation.

### 7.2 Publish-quality failures are not inspection failures

Inspection may succeed while Publish later fails due to:

- unresolved tracked changes;
- embedded comments;
- existing invalid/unverifiable digital signatures;
- approval conditions.

### 7.3 Versioning business failures are not inspection failures

Examples:

- no semantic content change;
- existing WORKING Version;
- stale Document revision;
- changed base/current Version;
- invalid ContentItem manifest.

### 7.4 Retry

Automatic retry candidates are limited to transient infrastructure conditions such as:

- ExtractorUnavailable;
- timeout where policy permits;
- explicitly classified temporary extraction failure.

Deterministic unsupported/integrity/resource-limit errors are not automatically retried.

### 7.5 Failure persistence

v0 does not persist negative results as successful cache records.

Failure is surfaced through the Application error contract and telemetry. Integrity/security failures may additionally produce Audit according to Error Registry policy.

## 8. Supported-format policy

### 8.1 v0 required semantic-inspection formats

Subject to PoC gates, production Versioning support targets:

- DOCX;
- XLSX;
- XLSM;
- PPTX;
- native-text PDF;
- TXT;
- CSV;
- HTML.

XLSM is required because macro-enabled Excel already carries business logic in the target environment.

### 8.2 Legacy Office

DOC / XLS / PPT remain PoC-gated. They are promoted only if format-native semantics can be inspected deterministically and fail-closed under the same contract.

### 8.3 Explicit v0 rejection

- scan-only PDF -> `RequiresOcr`;
- encrypted/password-protected Office/PDF -> `EncryptedContentUnsupported`;
- unknown formats;
- corrupt documents;
- documents with partially inspectable required semantics;
- resource-limit violations.

### 8.4 ZIP

ZIP is not a document semantic format.

It is an upload/download transport container:

- upload ZIP may be expanded into multiple ContentItems/representations under separate archive-intake rules;
- ZIP itself is not authoritative Version content;
- batch/version download may generate a temporary ZIP;
- nested/hostile archive policy belongs to archive intake.

## 9. Format-specific semantic acceptance

### 9.1 DOCX

Must account for:

- paragraph / heading / list order;
- tables and cell structure;
- headers/footers;
- footnotes/endnotes;
- links;
- images/meaningful visual content;
- section semantics;
- tracked changes proposed-final projection;
- comments/provenance;
- digital-signature evidence.

Pure formatting and serialization noise must not alter fingerprint.

### 9.2 XLSX

Must account for:

- workbook/sheet structure and order;
- visible/hidden/very-hidden sheets;
- cell values/types;
- formulas themselves;
- named ranges;
- table/merged-cell structure;
- external-reference definitions;
- links;
- meaningful images/charts;
- editorial provenance;
- signature evidence.

Cached calculation results are not authoritative semantic identity.

### 9.3 XLSM

All XLSX requirements plus:

- VBA project structure;
- modules and names;
- procedures/functions;
- executable source;
- declarations/constants;
- references;
- relevant project settings.

VBA whitespace, indentation, line-ending, comment-only, and meaning-neutral case differences must not alter semantic identity.

VBA semantic canonicalization is strict: unrecognized required syntax fails closed.

### 9.4 PPTX

Must account for:

- slide existence/order;
- text and shape association;
- tables;
- chart data/series/labels;
- SmartArt meaning;
- images/diagrams;
- hyperlinks;
- speaker notes;
- meaningful object relationships;
- comments/provenance;
- signatures.

Pure theme/font/background decoration is not Version identity.

### 9.5 native-text PDF

Must account for:

- page order;
- stable text/read order;
- tables where deterministically recoverable;
- images;
- links;
- visible form-field values;
- annotations/comments as editorial provenance;
- digital signatures.

Ambiguous semantic interpretation fails closed instead of guessing.

### 9.6 TXT

- deterministic decode;
- Unicode normalization;
- line-ending normalization;
- ambiguous encoding fails closed.

### 9.7 CSV

CSV is treated as tabular semantics rather than raw text.

- deterministic encoding/delimiter;
- row/column structure;
- quoted-field normalization;
- cell values;
- inconsistent structure fails closed.

### 9.8 HTML

- visible text;
- heading/list/table semantics;
- links;
- images;
- meaningful DOM order;
- script never executed;
- pure CSS decoration excluded.

Content whose meaning cannot be determined without JavaScript execution is unsupported in v0.

## 10. Cross-format representation model

Version and file format are distinct concepts.

A DocumentVersion contains one or more ContentItems.

Each ContentItem has:

- exactly one authoritative representation;
- zero or more renditions.

Adding/re-generating an equivalent rendition does not create a new Version.

Authority Migration is an explicit operation, never an automatic side effect.

Cross-format equivalence uses Semantic Capability Contract:

```text
source version-significant capabilities
        |
        v
can target represent + verify all?
        |- no -> authority migration denied
        '- yes -> capability-level equivalence checks
```

Example:

XLSM -> PDF may be a rendition but cannot be authority-equivalent when formula/VBA/hidden-content semantics are not representable.

## 11. Detailed diff boundary

Detailed diff is explicitly deferred to `Document Diff v0`.

Document Semantic Inspection persists semantic fingerprints/evidence only.

When a detailed diff is requested later:

```text
base authoritative FileObject
+
target authoritative FileObject
        |
        v
format-native comparator
        |
        v
Common ChangeSet
```

Format-native parse models are reconstructed on demand and not stored as durable platform state.

## 12. Library selection policy

Library "maturity" is not judged by age, stars, or user count.

Primary candidates are evaluated by:

1. semantic coverage;
2. fail-closed behavior;
3. determinism;
4. fixture/golden/differential/fuzz/corpus evidence;
5. unsafe/FFI/security boundary;
6. code auditability;
7. dependency/license reproducibility.

A feature-rich parser that silently ignores required semantics is not acceptable as Primary.

### 12.1 PoC primary candidates

These are **PoC primary candidates, not production dependencies yet**:

- DOCX: `stemma`;
- XLSX/XLSM: `rxls`;
- VBA extraction: `ovba`;
- VBA semantic canonicalization: strict internal adapter over extracted source;
- PPTX: `pptx` / rust-pptx;
- PDF: `pdfium-render` plus independent `lopdf` structural inspection;
- OOXML XMLDSig: `xml-sec`;
- CMS/X.509 path: RustCrypto `cms`, `x509-cert`, and permissive PKIX validation components;
- HTML: `html5ever`;
- CSV: `csv` behind strict project policy;
- text encoding: `encoding_rs`.

### 12.2 Independent oracles / supplements

Examples:

- DOCX: `docx-review-core`, raw OOXML checks;
- spreadsheet: `calamine`, `excel-ooxml`, raw OOXML;
- PPTX: `powerpoint-ooxml`, raw OOXML;
- PDF: dual PDFium/lopdf interpretation plus format-specific fixtures;
- signature validation: independent known-good/known-bad vectors.

Production promotion is prohibited until the PoC gate passes.

## 13. PoC Coverage Matrix

### 13.1 Fixture classes

Every format uses:

- BASE;
- SEMANTIC;
- NOISE;
- EDITORIAL;
- HOSTILE.

Every fixture also participates in determinism testing.

### 13.2 DOCX fixtures

Semantic-different:

- body text;
- heading/list structure;
- table value/merge;
- header/footer;
- footnote/endnote;
- hyperlink target;
- image content;
- section order.

Noise-same:

- save timestamp;
- lastModifiedBy;
- relationship IDs;
- XML serialization differences;
- font/margin-only differences;
- meaning-equivalent image re-encoding.

Editorial:

- insertion/deletion/replacement/move;
- format-only tracked change;
- resolved/unresolved comments.

Hostile:

- malformed OOXML;
- unknown required OOXML construct;
- relationship cycle;
- archive bomb;
- deep/oversized XML;
- traversal attempts.

### 13.3 XLSX fixtures

Semantic-different:

- cell value/type;
- formula;
- sheet add/remove/order;
- hidden/very-hidden content;
- named range;
- merged cells;
- table ranges;
- hyperlinks;
- external-reference definitions;
- chart data/series;
- image content.

Noise-same:

- cached formula result;
- calculation timestamp;
- XML ordering;
- formatting-only differences.

### 13.4 XLSM fixtures

All XLSX fixtures plus VBA:

Different:

- procedure/function body;
- call target;
- constant/declaration;
- module add/remove/rename;
- reference;
- project structure.

Same:

- whitespace;
- indentation;
- line endings;
- comments;
- meaning-neutral case differences.

Failure:

- incomplete VBA extraction;
- unknown strict-canonicalizer syntax;
- partial VBA project visibility.

### 13.5 PPTX fixtures

Semantic-different:

- slide add/remove/order;
- text;
- table;
- chart data;
- SmartArt;
- image;
- hyperlink;
- notes;
- meaningful shape/object relationship.

Noise-same:

- theme/font/background-only;
- internal shape IDs;
- serialization differences.

Unknown package parts must be detectable by project coverage sentinel.

### 13.6 PDF fixtures

Semantic-different:

- visible text;
- page order;
- images;
- links;
- form-field values.

Editorial/evidence:

- annotation/comment;
- digital signature.

Failure:

- scan-only;
- encrypted;
- broken xref;
- parser disagreement on required semantics;
- ambiguous read structure.

### 13.7 TXT / CSV / HTML fixtures

TXT:
- CRLF/LF same;
- Unicode-normalization equivalent same;
- text change different;
- ambiguous decode failure.

CSV:
- syntax variants representing same table same;
- row/column/cell changes different;
- inconsistent structure failure.

HTML:
- visible/structural/link/image changes different;
- whitespace/attribute-order/pure-CSS changes same;
- JS-required semantic construction unsupported.

### 13.8 Signature negative vectors

Required cases include:

- valid signature;
- tampered signed content;
- invalid digest;
- expired certificate;
- revoked certificate;
- unknown issuer;
- broken chain;
- unsupported algorithm;
- malformed signature.

### 13.9 Cross-format fixtures

Examples:

- DOCX -> PDF;
- XLSX -> PDF;
- XLSM -> PDF.

The test asserts capability preservation/loss rather than generic cross-format equality.

XLSM -> PDF must permit rendition use where appropriate but deny authority migration when formula/VBA/hidden semantics are lost.

### 13.10 Differential oracle policy

Primary parser output is compared against independent implementations and golden expectations.

No majority vote is used.

Disagreement requires analysis and an explicit fixture expectation correction or adapter/library change.

## 14. Reproducibility and security gates

For every fixture:

```text
same bytes
+ same inspection profile
-> same semantic fingerprint
-> same capability evidence
-> same editorial provenance
-> same external dependencies
-> same signature evidence
```

Where supported, semantic results must remain equivalent across Linux/macOS.

Primary parsers also receive malformed/truncated/oversized/deeply nested/adversarial inputs.

Acceptance requires:

- no panic escaping sandbox;
- bounded resources;
- no partial-success record;
- no document-body leakage to generic logs/errors;
- no silent unsupported semantics.

## 15. PoC repository layout

PoC remains isolated under `experiments/`.

Suggested layout:

```text
experiments/document-semantic-inspection/
|- fixtures/
|  |- docx/
|  |- xlsx/
|  |- xlsm/
|  |- pptx/
|  |- pdf/
|  |- txt/
|  |- csv/
|  '- html/
|- expectations/
|- adapters/
'- reports/
```

Repository fixtures are synthetic only.

Customer/financial-institution documents are never committed. A later internal real-corpus run uses the same harness in the protected environment.

## 16. Production promotion gate

A format is promoted to production only when all required gates pass:

```text
semantic-change fixtures        100% PASS
noise-invariance fixtures       100% PASS
editorial separation            100% PASS
fail-closed fixtures            100% PASS
determinism                     100% PASS
resource/sandbox tests          PASS
no silent unsupported semantics PASS
license/dependency gate         PASS
```

"Most documents work" is not an acceptance criterion.

An unsupported construct is either:

1. supported by a supplemental adapter;
2. explicitly rejected;
3. grounds for selecting another parser.

## 17. Out of scope

This design does not add:

- production parser dependencies before PoC;
- Search indexing;
- Search Canonical Model;
- Document Diff implementation;
- OCR;
- DOCM/PPTM production support;
- macro execution;
- antivirus product selection;
- archive-intake implementation;
- Document Versioning implementation;
- Approval workflow;
- HTTP/OpenAPI/UI;
- Authority Migration execution;
- backup/ransomware infrastructure.

Backup / Recovery & Ransomware Resilience remains a later capability. This design intentionally preserves separable authoritative File Storage, Metadata Store, and derived inspection/search data so production can evolve from one Linux server to isolated recovery tiers.

## 18. Design decisions carried forward to Document Versioning v0

After this capability is implemented and its format gates pass, Document Versioning v0 resumes with the previously approved decisions:

- one WORKING Version per Document;
- new WORKING based only on current PUBLISHED Version;
- Repository-transaction version_no allocation;
- caller UUIDv7 operation IDs for create/update;
- Document revision increments on create/update/publish;
- WORKING mutable, PUBLISHED immutable;
- WORKING is reusable rather than deleted;
- multiple ContentItems per Version;
- ZIP as transport, not authoritative content;
- `logical_path + ordinal` version-significant;
- exactly one authoritative representation per ContentItem;
- renditions do not create a Version;
- semantic equality is determined through Document Semantic Inspection, not raw SHA-256;
- all authoritative ContentItems must inspect successfully;
- detailed Diff remains separate;
- current replacement keeps prior current Version PUBLISHED but non-current/historical;
- Publish reuses the existing Publish operation ledger/API and extends it to Version #2+.

## 19. Design approval gate

No production implementation of Document Semantic Inspection v0 begins until:

1. this written Design is reviewed and explicitly approved;
2. the Design is frozen;
3. an Implementation Plan is written from the frozen Design;
4. PoC dependencies remain isolated under `experiments/` until their promotion gates pass.
