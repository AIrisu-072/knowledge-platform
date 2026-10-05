# P1 KnowledgeUnit Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the frozen, provider-neutral KnowledgeUnit types and canonical codecs in `search-core`, so P2 can build against actual typed input before P1 body integration.

**Architecture:** One public `knowledge_unit` module owns the DTOs and validation entrypoint; small private submodules own locator, profile, and identity codecs. Source adapters remain responsible for trusted current authority, raw-byte round trips, reader registry, and generation publication.

**Tech Stack:** Rust, existing `serde`, `uuid`, `time`; add `sha2.workspace = true` (`0.11`) and `unicode-normalization.workspace = true` (`=0.1.25`) only to `crates/search-core/Cargo.toml`.

**Spec:** `spec/data/logical-data-model-v0.md` §4.4 and appended Search KnowledgeUnit clause; frozen `p1-knowledgeunit-freeze.md`, exact `p1-knowledgeunit-contract.md` SHA-256 `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe` plus overriding `p1-knowledgeunit-amendment.md` SHA-256 `0d44f5dfed72360bafc56f19c2bf72b1d8b9f278c7ea3247261e1af040a06a00`.

## Global constraints and file map

- Keep `search-core` free of Document, parser, Tantivy, Vector engine, and storage dependencies. Reuse `crate::id::{SourceId,ResourceId}` and `crate::projection::ProjectionGenerationKey`; `ResourceId` is the Version Resource, not DocumentId.
- Create `crates/search-core/src/knowledge_unit.rs` for public DTOs, errors, text normalization, `TextSpan`, part sequence and authority-input validation; create `knowledge_unit/{locator,profile,identity}.rs` for their respective binary codecs. Modify `src/lib.rs` only to export `pub mod knowledge_unit;`.
- `src/knowledge_unit.rs` is the sole public re-export owner. Private submodules cannot define competing DTOs or serialization paths. `serde` serialization is a transport shape; deserialization alone never establishes a trusted Unit.
- Test only in `crates/search-core/tests/knowledge_unit_contract.rs`. No Source adapter, parser, full-body port, `DocSource`, cache store, manifest, or publication changes in this plan.

## Review focus

- Noncanonical NFC path or archive member could alias another Unit: Task 2 rejects it.
- Truncated, unknown-tag, trailing, or oversized frame could hash differently across runtimes: Tasks 2–4 reject it.
- Changed inner reader settings could reuse a ZIP UnitId: Task 3 proves composite profile and ID change.
- Same FileObject in two parts could collapse into one Unit: Task 4 uses both golden IDs.
- Stale parent, lease, or generation could appear current to P2: Task 5 pins equality inputs and leaves current-source checks with the trusted caller.

### Task 1: Canonical DTOs and normalized text

**Files:** `crates/search-core/src/{knowledge_unit.rs,lib.rs,Cargo.toml}`; `crates/search-core/tests/knowledge_unit_contract.rs`.

**Interfaces:** `ResourceVersionRef {source_id:SourceId,resource_id:ResourceId,source_native_version:String}`; `ContentPartRef {source_native_part_id:String,logical_path:String,ordinal:u32}`; `RawBinding {sha256:[u8;32],size_bytes:u64,media_type:String}`. `FormatId` tags 1–9 = Docx/Xlsx/Xlsm/Pptx/Pdf/Text/Csv/Html/Zip; `UnitKind` is Heading/Paragraph/TableCell/SpreadsheetCell/SlideText/PdfText/PlainText/CsvField/HtmlText. Base DTOs derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`; later ID types use validated string serde (`ku1:` / `sha256:`), not array/debug serialization.

- [ ] Add RED `normalization_and_span`: `normalize_unit_text("東京\r\n") == "東京\n"`; SHA-256 hex is `866bff0df548a00eaad416ca1fc987f20d94dae000fee8abd356dfb29bd15934`; decomposed accent composes, case/width/kana/punctuation remain unchanged. `TextSpan::new(&text,start_byte,end_byte)` accepts only nonempty UTF-8 boundary-aligned half-open byte ranges within normalized text and rejects non-normalized input.
- [ ] Run `cargo test -p search-core --test knowledge_unit_contract normalization_and_span`; expect RED from missing public API.
- [ ] Implement `pub fn normalize_unit_text(&str)->String`, `pub fn text_sha256(&str)->[u8;32]`, `pub struct TextSpan {start_byte:u32,end_byte:u32}`, and `TextSpan::new(text:&str,start_byte:u32,end_byte:u32)->Result<Self,UnitCodecError>`. Apply CRLF→LF, then CR→LF, then NFC. Add only the two existing workspace dependencies and single module export.
- [ ] Run the same focused test; expect PASS.

### Task 2: Native locator codec and canonical paths

**Files:** `crates/search-core/src/knowledge_unit/locator.rs`; same contract test.

**Interfaces:** `NativeLocator::{Docx{steps},Spreadsheet{sheet_ordinal,row,col},Pptx{slide_ordinal,shape_path,text_slot},Pdf{page_index,char_start,char_end},Text{line_start,line_end},Csv{record,field},Html{text_node_path},Archive{members,inner:Box<NativeLocator>}}`; `DocxStep::{BodyBlock,Row,Cell,CellBlock}(u32)`; `PptxTextSlot::{ShapeParagraph{paragraph},TableCellParagraph{row,col,paragraph}}`. `NativeLocator::encode(&self)->Result<Vec<u8>,UnitCodecError>` and `NativeLocator::decode(&[u8])->Result<Self,UnitCodecError>`; `validate_logical_path(&str)->Result<(),UnitCodecError>` and `validate_archive_member(&str)->Result<(),UnitCodecError>`.

Codec payloads by tag: 1=`count(u32)||[(step_tag 1–4,u32)]`; 2=`sheet_ordinal||row||col`; 3=`slide_ordinal||count||shape_path(u32)*||slot_tag(1/2)||slot fields(u32)*`; 4=`page_index||char_start||char_end`; 5=`line_start||line_end`; 6=`record||field`; 7=`count||text_node_path(u32)*`; 8=`count||frame(member UTF-8)*||frame(inner full locator bytes)`. All `u32` are BE, positions zero-based, ranges half-open.

- [ ] Add RED `locator_golden_and_rejections`: `Text{0,1}` hex equals `6e61746976652d6c6f6361746f723a763100050000000000000001`; all eight variants round-trip. Reject empty/invalid Docx step grammar, empty PPTX/HTML/Archive paths, PDF/Text reversed ranges, recursive Archive, unknown tag, truncation, trailing bytes, non-NFC, absolute/empty/dot/dotdot/backslash/control path, and a declared `u32::MAX` frame with a short body without allocating that size.
- [ ] Run `cargo test -p search-core --test knowledge_unit_contract locator_golden_and_rejections`; expect RED.
- [ ] Implement `b"native-locator:v1\0" || tag:u8 || payload`, fixed-width BE integers, `u32`-counted vectors, `frame(bytes)=u32_be(len)||bytes`, and length-framed inner locator. Decode strictly, consume all bytes, enforce bounds before allocation; encode rejects noncanonical input rather than silently normalizing identity strings. Archive members are NFC relative ZIP paths; outer `ContentPartRef.logical_path` follows `document-domain::LogicalPath` rules without depending on that crate. Raw ZIP flag/decoder collision and native-element round-trip remain host responsibilities.
- [ ] Run the same focused test; expect PASS.

### Task 3: Profile v1 and Archive composite v2

**Files:** `crates/search-core/src/knowledge_unit/profile.rs`; same contract test.

**Interfaces:** `ExtractionProfileId::parse(&str)->Result<Self,UnitCodecError>`, `as_str(&self)->&str`; `ExtractionProfileDefinitionV1 {format:FormatId,parser_name:String,parser_version:String,parser_build_sha256:[u8;32],native_binary_sha256:Option<[u8;32]>,scope_revision:u32,segmentation_revision:u32,normalization_revision:u32,locator_revision:u32,format_settings:FormatSettings,limits:BTreeMap<BudgetKey,u64>}` with `encode(&self)->Result<Vec<u8>,UnitCodecError>` and `decode(&[u8])->Result<Self,UnitCodecError>`; `ExtractionProfileId::for_definition(&ExtractionProfileDefinitionV1)->Result<Self,UnitCodecError>` and `for_archive(&ArchiveProfilePlan)->Result<Self,UnitCodecError>`. `FormatSettings::{None,Text{charset},Csv{charset,delimiter,quote},Archive{member_decoder}}`; `BudgetKey` exactly tags 1–15, all keys present once including zero for non-applicable. `ArchiveReaderNode {members:Vec<String>,parser_build_id:String,definition:ExtractionProfileDefinitionV1}`; `ArchiveProfilePlan {nodes:Vec<ArchiveReaderNode>,used_leaf_chains:Vec<Vec<String>>}` with `decode(bytes:&[u8],used_leaf_chains:Vec<Vec<String>>)->Result<Self,UnitCodecError>`.

V1 definition bytes: domain prefix, then `frame` of each field in the order above. `format` payload is one tag byte; hash payload is 32 bytes; native option payload is `0` or `1||32 bytes`; revision payloads are u32 BE. Settings payloads are `0`, `1||frame(charset)`, `2||frame(charset)||delimiter||quote`, or `3||frame(member_decoder)`; map payload is `u32_be(15)||[(BudgetKey tag:u8,value:u64 BE)]` in tag order. Reject normalization/locator revision other than 1 in v1.

- [ ] Add RED `profile_codec_and_archive`: hand-authored v1 binary fixture starts `extraction-profile:v1\0` and round-trips exactly; mutation of parser build hash, PDF pin, scope/segmentation/revisions, settings, or any budget changes ID. Reject duplicate/missing budget tag, unknown tag, malformed option, non-ASCII parser ID, trailing bytes, PDF without native pin. Archive fixtures cover Text/CSV/nested ZIP/PDF leaves; changing inner charset/dialect/decoder/build/PDF pin changes composite ID. Reject unsorted/duplicate/extra/missing node, missing ZIP prefix, unmatched leaf, noncanonical member, wrong root or leaf format, unknown/trailing v2 bytes.
- [ ] Run `cargo test -p search-core --test knowledge_unit_contract profile_codec_and_archive`; expect RED.
- [ ] Implement v1 field-order framing above, no JSON/hash-map iteration; `b"extraction-profile:archive:v2\0" || u32_be(nodes.len()) || concat(frame(node_bytes))`, with node bytes exactly `frame(chain_bytes)||frame(parser_build_id)||frame(v1_definition_bytes)` and `chain_bytes=u32_be(members.len())||concat(frame(member))`. Sort/check nodes by their UTF-8 member-component sequences lexicographically, prefix before child; require one empty-chain Zip root and every used nonempty leaf once, with proper Zip prefixes and no unused node. `FormatId::Zip` authoritative items use this one composite ID across all Units. Registry/deployed artifact equality and actual reader-use matching remain host responsibilities.
- [ ] Run the same focused test; expect PASS.

### Task 4: Unit identity and local sequence validation

**Files:** `crates/search-core/src/knowledge_unit/identity.rs`, `knowledge_unit.rs`; same contract test.

**Interfaces:** `UnitId::derive(version:&ResourceVersionRef,part:&ContentPartRef,profile:&ExtractionProfileId,locator:&NativeLocator,ordinal:u32)->Result<Self,UnitCodecError>`; `UnitId::parse(&str)->Result<Self,UnitCodecError>` / `Display` emits `ku1:` plus 64 lowercase hex. `UnitProvenance {source_snapshot:String,authoritative_representation_ref:String,raw:RawBinding,detected_format:FormatId,archive_inner_format:Option<FormatId>,profile:ExtractionProfileId,parser_build_id:String}`; `KnowledgeUnit {unit_id:UnitId,version:ResourceVersionRef,part:ContentPartRef,parent_unit_id:Option<UnitId>,ordinal:u32,kind:UnitKind,text:String,locator:NativeLocator,text_sha256:[u8;32],provenance:UnitProvenance}`. `UnitAuthorityBinding {version:ResourceVersionRef,part:ContentPartRef,source_snapshot:String,authoritative_representation_ref:String,raw:RawBinding,detected_format:FormatId,archive_inner_format:Option<FormatId>,profile:ExtractionProfileId,parser_build_id:String,archive_plan:Option<ArchiveProfilePlan>}`; `validate_part_units(binding:&UnitAuthorityBinding,units:&[KnowledgeUnit])->Result<(),UnitValidationError>`.

- [ ] Add RED `unit_id_golden_and_invalid`: Source `00000000-0000-0000-0000-000000000001`, Version Resource `00000000-0000-0000-0000-000000000002`, native Version `00000000-0000-0000-0000-000000000003`, synthetic profile `sha256:` + 64 zeroes, Text locator `{0,1}`, ordinal 0. Part `00000000-0000-0000-0000-000000000004`/`primary`/0 yields `ku1:c6b9bfcc8a9d53ee19966146ccfce5a8b2f6f792f7cab53d4a9154377e867ca1`; part `00000000-0000-0000-0000-000000000005`/`attachment`/1 yields `ku1:3b01330d059d71802ec8b3bc216ff9739b3765843892fe4b8fa9bdfa987b115e`. Reject wrong prefix/case/length/hex, invalid frame, non-NFC native ID, and altered field order; inner reader setting change also changes Archive UnitId. Document UUID spelling is an adapter-only check.
- [ ] Add RED `unit_sequence_rejections`: out-of-order or duplicate ordinal/locator, missing/forward/self/cross-part parent, non-NFC text/path, noncanonical MIME essence, wrong text digest/ID, mismatched Version/Part/representation/raw/profile/outer-inner format/parser build, wrong UnitKind-locator pair, Archive leaf-chain mismatch against `archive_plan`. Empty `units` is locally valid only as a sequence; caller must separately prove `Supported` coverage. No API here proves raw locator round-trip, complete coverage, or current Read.
- [ ] Run `cargo test -p search-core --test knowledge_unit_contract unit_`; expect RED.
- [ ] Implement SHA-256 of `b"knowledge-unit:v1\0"` plus the nine individually framed fields in frozen order; UUIDs use `as_uuid().as_bytes()` (16 bytes), integer payloads are u32 BE. Validate parent links only against earlier Units of the same Version/Part; compare all authority binding fields and recomputed text hash/ID. Allow Docx Heading/Paragraph/TableCell, spreadsheet cell, slide text, PDF text, plain text, CSV field, and HTML text/Heading with their corresponding locator (Archive uses validated leaf). Archive requires `detected_format=Zip`, `archive_inner_format=Some(leaf)`, and matching `archive_plan`; non-Archive requires `None` for both archive fields. Never put generation, raw/text hash, parent ID, or OS path into ID.
- [ ] Run both focused tests; expect PASS.

### Task 5: P2 authority and cache input types, no cache behavior

**Files:** `crates/search-core/src/knowledge_unit.rs`; same contract test.

**Interfaces:** `EmbeddingCacheKey {embedding_model_id:String,unit_id:UnitId,text_sha256:[u8;32],profile:ExtractionProfileId,source_id:SourceId,authority_scope_key:String,retention_lease_id:String,lifetime_scope_id:String}`; `VectorHitRef {generation:ProjectionGenerationKey,unit_id:UnitId,version:ResourceVersionRef,part:ContentPartRef,authoritative_representation_ref:String,raw:RawBinding,profile:ExtractionProfileId,text_sha256:[u8;32]}`; `VectorAuthorityInput {generation:ProjectionGenerationKey,version:ResourceVersionRef,part:ContentPartRef,authoritative_representation_ref:String,raw:RawBinding,profile:ExtractionProfileId,authority_scope_key:String,retention_lease_id:String,lifetime_scope_id:String,retention_mode:RetentionMode,lease_expires_at:Option<OffsetDateTime>}`. `matches_pinned_unit(hit:&VectorHitRef,unit:&KnowledgeUnit,pinned:&VectorAuthorityInput)->bool` and `cache_key_matches_authority(key:&EmbeddingCacheKey,unit:&KnowledgeUnit,pinned:&VectorAuthorityInput)->bool` compare only stored identity/binding fields. Reuse `crate::source::RetentionMode` and `time::OffsetDateTime`.

- [ ] Add RED `vector_binding_inputs`: DTO serde round-trip preserves each field; two cache keys differing by model, Source, authority scope, lease, lifetime, text digest, profile, or UnitId are unequal. Generation/parent/part/raw/profile/text mismatch fails `matches_pinned_unit`; scope/lease/lifetime mismatch fails `cache_key_matches_authority`. No helper turns a cache hit into evidence or `BodyRequired` qualification.
- [ ] Run `cargo test -p search-core --test knowledge_unit_contract vector_binding_inputs`; expect RED.
- [ ] Add only these typed input records with derived `Debug,Clone,PartialEq,Eq,Serialize,Deserialize` and `Hash` on `EmbeddingCacheKey` if all fields allow it, plus the two equality helpers. Current Read/Version/Part/retention permission, expiry, lease cancellation, lifetime cleanup, pinned manifest equality, and cross-generation rebinding are checked by future trusted Source/P2 code; this module does not mint current-authority proof or implement persistence.
- [ ] Run the focused test, then `cargo test -p search-core --test knowledge_unit_contract` and `cargo fmt --all -- --check`; expect PASS. Record exact test head/output before any completion claim.

## Handoff check

Self-review each frozen §1–4 and amendment §1–2 against the five tasks: all core fields, tags, codecs, golden bytes, local rejections, Archive profile, and P2 binding inputs have an owning RED. Host-only raw/registry/Source authority, manifest seal, parser qualification, full-body ports, and production cache remain explicitly outside this minimum. No implementation, test, or CI result is claimed by this plan.
