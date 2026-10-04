# P1 Search Extraction — Full Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 現行 Live Document Version の全 AUTHORITATIVE ContentItem を検証可能な `KnowledgeUnit` に変換し、同一 generation の本文索引・coverage・限定 exact-text 証拠と有限の否定証明を公開する。

**Architecture:** Document の一つの read-only snapshot と FileStorage が raw/Part/Version の正本を所有する。資格済み reader だけを Linux fresh process sandbox で動かし、host が locator、text、profile、raw binding を再検証する。Projection-only digest は維持し、Unit/coverage/lexical/Graph を別の immutable bundle として seal 後に pointer CAS する。検索と claim は同じ pinned bundle、現行 `Read`、Source-owned 再読取で評価する。

**Tech Stack:** Rust 1.98 / edition 2024、既存 PostgreSQL/SQLx、FileStorage、Tantivy 0.26.2、既存 DSI の Linux sandbox 技術基盤。reader 候補は既存 workspace pin の `office_oxide 0.1.11`、`calamine 0.36.1`、`rxls 0.1.3`、`pdfium-render 0.9.4` + PDFium `151.0.7881.0`、`lopdf 0.45.0`、`encoding_rs 0.8.41`、`csv 1.4.0`、`html5ever/markup5ever_rcdom 0.39.0`、`zip 8.6.0`。いずれも Search 本文用途の採用は P1-Q01 で資格判定する。

**Spec:** `p1-extraction-freeze.md` SHA-256 `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d`。合成順は `p1-extraction-design-revision-1.md` `b1e84e476f614eb30e3bd1c5982e7e0ff2a35e33aef9cddd069618315dcbe742` + `p1-knowledgeunit-freeze.md` `0fe9ceb84472633d06c52d84f2f62c86bc783f880751c935a7d773b1c8fdfc84` + `p1-body-absence-amendment.md` `49838833ff293cf20032e46dbe9429847a975fad8fcb40f21fd977eaa8b69ae1` + `p1-partial-positive-correction.md` `b15c20beed37d0f8a569799fa8f41a555e9843f5dc13a47b6c320942130f5db8`。最後の二つが衝突部分を優先する。原案 `p1-extraction-design.md` `ff5a2649f668b623eaee7de7ecdfb16de3f48f629470554dbb7dd92b99722977` §4/6/7 の format、資源、安全、性能条件も適用する。規範は `spec/data/logical-data-model-v0.md` の KnowledgeUnit 節、Search v0 承認済み設計、architecture contract。本文統合の規範追記は別の専任 writer の成果を production 着手前に照合する。

## Global Constraints and file map

- **別 lane:** `p1-knowledgeunit-implementation-plan.md` の Task 1–5 が `search-core::knowledge_unit::{ResourceVersionRef,ContentPartRef,RawBinding,FormatId,UnitKind,NativeLocator,TextSpan,ExtractionProfileId,UnitId,KnowledgeUnit,UnitAuthorityBinding,validate_part_units}` を提供する。本計画は同じ型・codec、`search-core/src/{knowledge_unit.rs,lib.rs}`、その contract test を再実装しない。P2 はその限定契約で先行でき、P1 統合完了とは数えない。
- **PoC と本番の境界:** P1-Q01 の manifest/golden/資源/ライセンス・security 判定が GO するまで、新 reader library を production Cargo に追加しない。DSI で既に pin 済みの依存も Search 本文の fidelity 資格を自動取得しない。DOCX/XLSX/XLSM/PPTX/PDF/Text/CSV/HTML/ZIP のいずれかで候補が NG なら、その reader を production に入れず、同じ隔離 PoC で代替候補を資格判定する。全対象形式の reader が合格するまで P1 全体を完了扱いしない。旧 DOC/XLS/PPT は明示 `Unsupported` のままでよい。
- **production 着手条件:** 上記 exact input の hash、最小 Unit code、本文統合の専任 normative writer の追記を再照合する。設計凍結の範囲で Program の自律承認は済んでおり、工程間の追加承認待ちは置かない。merge/deploy/本番 migration はこの計画の操作に含めない。
- **責務/ファイル:** `experiments/search-extraction-poc/` は fixture・独立 locator oracle・資格報告。`crates/search-extraction-core/src/{protocol,coverage,budget,validation}.rs` は worker/host の型と純粋検証。`crates/document-sandbox-runner/` は DSI/Search 共通 process isolation。`crates/search-extraction-{runner,worker}/` は Search 専用 protocol と format reader。`crates/search-source-document/src/{postgres,model,extraction,body_manifest,body_bundle,body_absence,body_evidence,outbox,coverage,evidence}.rs` は Source 正本・bundle・access。`crates/search-tantivy/src/{schema,index,query}.rs` は schema 2 と検索可能 doc 列挙。`crates/search-application/src/{content_scope,body_ports,ports,retrieval_execution,discovery_service,evidence_resolution}.rs` は request 評価と限定 claim。
- **共有 writer:** root `Cargo.toml`/`Cargo.lock` は P1-I01 だけが編集し、P2/P3/P6/P7 の root 変更と直列化。`search-core/lib.rs` は最小 Unit lane が先。`search-application/{ports.rs,lib.rs,retrieval_execution.rs,discovery_service.rs}` は P4 が `ScopedDiscoveryExecution`/登録/`scoped.rs` の writer、P6 が ports を後で使うため P1-A02/A03 は両者の該当編集と直列化する。P1 は P4 の `discover_scoped(request, ScopedDiscoveryExecution)` を変更せず、`discover_with_content_scope(request, DiscoveryScope)` を同一内部 loop に薄く接続する。`search-source-document/outbox.rs` は bundle owner 1名に限定する。`search-source-document/src/lib.rs` の新 module 登録は S01→S02→B01→A03→A04 が順に一回ずつ行い、同時編集しない。`search-extraction-worker/src/readers/mod.rs` は I03→I04→I05 の登録時のみ直列化し、reader 本体だけを並列化できる。Tantivy schema/index/query、DSI runner の各統合編集も単独 writer とする。
- **既存の意味:** `ProjectionGenerationManifest.digest` と `generation_digest()` は projection-only v1、`resource_count` は Resource 数、`Coverage` は Source enumeration。Live は現行 `PUBLISHED`/非 T10 だけ。History と `ReadHistory` は現行 lookup。DSI output/fingerprint、旧 `version_files`、rendition、macro、OCR、外部 HTML/JS は本文入力ではない。S1 は routed retriever 順 `PriorityConcat`。
- **上限と環境:** input 256 MiB、ZIP entry 20,000、展開 1 entry 64 MiB / total 512 MiB、worker AS 2 GiB、scratch 1 GiB、structured result 16 MiB、wall 10 s / CPU 8 s を超えない。P1-Q01 の初期候補 `BudgetKey` は順に `InputBytes=268435456, ZipEntries=20000, ZipEntryBytes=67108864, ZipTotalBytes=536870912, ZipDepth=3, Units=100000, UnitUtf8Bytes=1048576, WorkerOutputBytes=16777216, XmlDepth=256, XmlNodes=2000000, PdfPages=1024, PdfOperations=1000000, HtmlNodes=2000000, CsvRecords=1000000, CsvFieldBytes=1048576`。Q01 は実測でこれ以下の値を format profile に固定し、非適用 key は 0 とする。値の引上げは profile 変更と再資格を要する。local 約6 GiB free を前提に `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2`、小さい合成 fixture と既存 Docker image を使う。local Docker の Landlock が `NotEnforced` なら隔離 PASS に数えず、hosted Linux 実 canary を必須とする。root の `mise run verify:full` は最終統合だけ。
- **各 Task 共通:** 指定 RED を実行して期待通り失敗を保存 → 最小実装 → 同じ focused command の GREEN → 差分と receipt を独立 read-only reviewer へ渡す。読取不能・証明不能・権限不明・上限到達は `Unknown`/blocking gap または旧 pointer 維持に倒す。本文、ファイル名、StorageKey、snippet、Denied の ID/件数は通常ログ/traceに出さない。

## Review Focus

1. 日本語 1 文字の部分文字列が Tantivy tokenizer で 0 hit でも、全 Unit scan が `MatchFound` とし `Absent` を出さない: P1-A04。
2. 同じ text と同じ FileObject が別 ContentItem/別親にあっても、Unit・lexical doc・selector の親/Part binding を混同しない: P1-S02/E01/A03。
3. `Partial` の検証済み Unit は肯定 claim と blocking coverage gap が共存し、同じ親の否定は `Unknown`: P1-A03/A04。
4. receipt 個別 digest が正しくても、検索可能 lexical doc の欠落/余分/重複/本文差替えを seal が拒否する: P1-E01/B02。
5. 読取中の raw/Read/Version/Part 変更、sandbox 非強制、worker output 切断は、新 generation や claim/absence を公開せず旧 pointerを保つ: P1-I02/S02/B03/A03/A04。

---

### P1-Q01 — Search 本文 reader の隔離資格と固定 corpus

**Files:** Create `experiments/search-extraction-poc/{README.md,Cargo.toml,Cargo.lock,deny.toml,generate.py,manifest.json,run.py,expected.json,qualification.md}`、`experiments/search-extraction-poc/src/{main,readers}.rs` and `experiments/search-extraction-poc/tests/test_manifest.py`。この Cargo は `[workspace]` を持つ隔離 PoC で、既存 production Cargo は変更しない。

**Interfaces:** `manifest.json` は `{id,format,sha256,origin,license,expected_units:[{kind,text,locator,part_ordinal}],coverage,reasons,limits}` の配列。`run.py --verify-manifest` は raw SHA と oracle expectation の重複/欠落を検証し、`--qualify --format {docx,xlsx,xlsm,pptx,pdf,text,csv,html,zip}` は `CARGO_TARGET_DIR="$PWD/target" cargo run --manifest-path experiments/search-extraction-poc/Cargo.toml --locked --bin extraction-qualify` を一つの binary として呼び `{format,parser_build_sha256,native_pin,fixture_id,matched_units,missed_units,locator_roundtrip_failures,coverage,reason,wall_ms,peak_rss_bytes,scratch_bytes,result_bytes,license_security}` を JSONL に出す。`qualification.md` は形式ごとに GO/NG と profile の全15 budget を数値で記録する。

- [ ] RED: `python3 -m unittest discover -s experiments/search-extraction-poc/tests` の `test_manifest` で未生成 fixture/hash、曖昧 locator、同一 format の未判定ケースが失敗する。生成対象は DOCX 見出し/段落/入れ子表、XLSX と XLSM 日本語・絶対 cell/欠落 formula cache、PPTX 複数 slide/群化 shape/table、PDF 2 page/縦組・image-only、Text UTF-8 BOM/明示 CP932、CSV quote 内改行、HTML heading/hidden/script、nested ZIP と path/symlink/暗号/duplicate/bomb を含む。旧 DOC/XLS/PPT は magic-only 非対応 fixture で `Unsupported` を固定する。全角/半角、濁点結合、CRLF、句読点、同文面の複数 Part を固定する。
- [ ] GREEN: Python stdlib による小さい OOXML/ZIP/PDF byte generator と独立 XML/CSV/DOM/ZIP locator oracle を実装し、各 raw SHA/expected text/kind/locator/coverage を照合する。Search 用の `plain_text()` 一括結果から locator を逆算しない。`office_oxide::Document::from_reader(..., explicit_format)`、`calamine::Range::start()+used_cells()` 等の pinned API は公式 docs と現行 source で照合してから harness に使う。
- [ ] `run.py --qualify` を各 format で一度実行し、`Supported` では対象 text/locator omission 0、`Partial/Unsupported` は期待 reason 一致、raw locator 一意往復、10 s/2 GiB 等の ceiling 内であることを report に記録する。`cargo deny --manifest-path experiments/search-extraction-poc/Cargo.toml --config experiments/search-extraction-poc/deny.toml check` は既存 PoC と同じ permissive-license allowlist/security policy で PASS とし、public fixture の license/hash も manifest へ記録する。NG の形式を fail closed とし、未測定 library を選定しない。`git diff --check` と独立 reviewer で資格を gate する。

### P1-I01 — Core wire/coverage contract と単一 root workspace 登録

**Files:** Create `crates/search-extraction-core/{Cargo.toml,src/lib.rs,src/{protocol,coverage,budget,validation}.rs,tests/extraction_contract.rs}`、`crates/document-sandbox-runner/{Cargo.toml,src/lib.rs}`、`crates/search-extraction-{runner,worker}/{Cargo.toml,src/lib.rs}`。Modify root `Cargo.toml`/`Cargo.lock` はこの Task だけ。`search-core` の最小 Unit Task 完了と P1-Q01 GO が前提。

**Interfaces:** `WorkerOperation::{Extract,ResolveLocators(Vec<NativeLocator>)}`、`WorkerRequest {operation,format:FormatId,profile:ExtractionProfileId,profile_bytes:Vec<u8>,expected_raw:RawBinding,budgets:ExtractionBudgets}`、`WorkerFragment {ordinal:u32,parent_ordinal:Option<u32>,kind:UnitKind,text:String,locator:NativeLocator}`、`NativeOmission {package_path:Option<String>,physical_child_path:Vec<u32>,reason:CoverageReason}`、`WorkerReport {coverage:BodyCoverage,fragments:Vec<WorkerFragment>,reader_use:Vec<ArchiveReaderNode>,scope_items:u32,known_omissions:Vec<NativeOmission>}`。`RegisteredProfile {id:ExtractionProfileId,definition:ExtractionProfileDefinitionV1,parser_build_sha256:[u8;32],native_binary_sha256:Option<[u8;32]>,budgets:ExtractionBudgets}` は trusted registry からのみ作り worker 自己申告からは作らない。`BodyCoverage`、`CoverageReason`、`PermanentFailureCode`、`RetryableFailureCode`、`ItemOperationState` は revision §6 の variant をそのまま使用。`ExtractionBudgets::validate(&self)->Result<(),ExtractionError>`、`BudgetMeter::charge(key:BudgetKey,delta:u64)->Result<(),ReaderFailure>`、`validate_worker_report(report:&WorkerReport,profile:&RegisteredProfile)->Result<(),ExtractionError>` を export。`ReaderFailure` と `ExtractionError` は revision §6 の typed outcome と incident を区別する。worker request に Source/actor/StorageKey/FileObject ID を入れない。

- [ ] RED: `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test -p search-extraction-core --locked --test extraction_contract` は未登録 crate で失敗。contract tests は全 failure matrix、`Partial` 非空理由・検証 Unit・既知 omission、`Supported` 空本文許可、`Unsupported/Failed` Unit 0、途中 output/kill/panic の publish 禁止、15 budget key 全部、未知 wire tag/trailing/oversize を検査する。
- [ ] GREEN: 各 crate manifest/entrypoint と core DTO/validation を実装し、root workspace/lock を一回で登録する。worker protocol は `search-extraction:v1` の bounded 長さ付き request/response、数値・enum code のみを受理する。`cargo metadata --locked --format-version 1` と上記 focused test を PASS。P1-Q01 が NG とした reader への production 依存は追加しない。

### P1-I02 — 共通 Linux process seal と Search runner

**Files:** Implement `crates/document-sandbox-runner/src/{process,linux,sandbox}.rs`、`crates/search-extraction-runner/src/{lib,executor}.rs`、`crates/search-extraction-worker/src/main.rs`; adjust `crates/document-semantic-inspection-runner/src/{lib,linux,sandbox}.rs` only to delegate shared mechanics。Test `crates/search-extraction-runner/tests/{runner_isolation,runner_failure_matrix}.rs` and existing DSI `runner_baseline/runner_isolation`。

**Interfaces:** `SandboxProcessRunner::run(&self, worker:&Path, raw:&[u8], request:&[u8])->Result<Vec<u8>,SandboxRunError>` owns fresh process, read-only FD, private temp, FD close, rlimits, bounded stdout/stderr and timeout; `seal_worker_sandbox()->Result<(),SandboxRunError>` applies mandatory Linux Landlock/seccomp before parsing. `SearchExtractionRunner::extract(&self,raw:&[u8],request:WorkerRequest)->Result<WorkerReport,ExtractionError>` and `resolve_locators(&self,raw:&[u8],request:WorkerRequest)->Result<Vec<WorkerFragment>,ExtractionError>` call that runner; no DSI `WorkerResponse` or fingerprint is reused。

- [ ] RED: `cargo test -p search-extraction-runner --locked --test runner_isolation` injects open forbidden path, spawn/network, inherited FD, no Landlock/seccomp, timeout, >16 MiB output, >1 GiB scratch, wrong PDFium hash and worker kill; all fail closed without a partial successful report。`runner_failure_matrix` asserts typed permanent/retryable/config mapping and no unbounded retry。
- [ ] GREEN: factor only DSI-tested process controls into `document-sandbox-runner`; Search worker seals before parse, PDFium exact binary SHA pin is checked before seal。Run the two Search tests and `cargo test -p document-semantic-inspection-runner --locked --test runner_baseline --test runner_isolation` on hosted Linux。macOS の非強制/skip は PASS にしない。DSI protocol、error map、behavior の回帰を保存する。

### P1-I03 — Text/CSV/HTML の実 reader と native round-trip

**Files:** Create `crates/search-extraction-worker/src/readers/{mod,text,csv,html}.rs` and `tests/readers_textual.rs`; modify `src/lib.rs` to register `readers`。

**Interfaces:** `FormatExtractor::extract(raw:&[u8],profile:&RegisteredProfile,budget:&mut BudgetMeter)->Result<WorkerReport,ReaderFailure>` / `resolve(raw,profile,locators)->Result<Vec<WorkerFragment>,ReaderFailure>`。Text は厳密 charset/BOM decode と物理 line 半開区間、CSV は quote 内改行を含む論理 `(record,field)`、HTML は全 child（text/comment/空白を含む）index の `text_node_path`。各 reader は `nfc-lf-v1` を使い、`Supported` は scope を列挙しきったときだけ。

- [ ] RED: `cargo test -p search-extraction-worker --locked --test readers_textual` の `text_bom_cp932_and_line_boundaries`、`csv_quoted_newline_and_ambiguous_dialect`、`html_dom_path_hidden_and_script_scope`、`utf8_nfc_literal_not_casefolded` は golden Unit text/kind/locator と再解析一意性を検査する。巨大 CSV field、HTML node/depth、invalid encoding、外部 URL/JS は非 `Supported`。
- [ ] GREEN: P1-Q01 GO の pin だけで実装し、同じ command PASS。host が見える output に source filename/path を含めない。

### P1-I04 — DOCX/XLSX/XLSM/PPTX の実 reader

**Files:** Create `crates/search-extraction-worker/src/readers/{docx,spreadsheet,pptx,ooxml_package}.rs` and `tests/readers_ooxml.rs`; modify `src/readers/mod.rs` after I03。

**Interfaces:** 同じ `FormatExtractor`。DOCX は `/word/document.xml` main body の物理 `BodyBlock→Row→Cell→CellBlock`、XLSX/XLSM は workbook `<sheet>` 順 ordinal と `Range::start()` を加えた絶対 `(row,col)`、PPTX は presentation 順 slide と `<p:spTree>` 物理 shape path/text slot。XLSM VBA は実行せず本文 scope に含めない。OOXML 内部 ZIP は `NativeLocator::Archive` にしない。

- [ ] RED: `cargo test -p search-extraction-worker --locked --test readers_ooxml` は DOCX 入れ子 table/heading/style、XLSX 非A1起点・日本語 cell・cache欠落、XLSM macro除外、PPTX group/table/複数 slide の固定 locator 往復と順序を検査。MIME/拡張子/magic/OPC の不一致、暗号化、XML node/depth、ZIP宣言値と実展開値超過は Unit を全破棄し非 `Supported`。
- [ ] GREEN: Q01 の GO reader を使用し、`plain_text()` から locator を復元しない。Known omitted chart/notes/visibility/formula cache は `Partial` と理由を付ける。同じ command PASS。

### P1-I05 — PDF/明示 ZIP container と複合 reader plan

**Files:** Create `crates/search-extraction-worker/src/readers/{pdf,archive}.rs` and `tests/readers_pdf_archive.rs`; modify `src/readers/mod.rs` after I04。

**Interfaces:** PDF は pinned PDFium `PdfPageText` character index 半開区間と `lopdf` 構造 guard、ZIP は明示許可された container のみ member chain + inner tag 1–7。`ArchiveReaderNode` の全実行 reader と registered composite v2 profile を一対一照合し、outer `Zip` / inner leaf format を分離する。

- [ ] RED: `cargo test -p search-extraction-worker --locked --test readers_pdf_archive` は複数 page、縦組/曖昧順、image-only `RequiresOcr`、文字 index≠UTF-8 byte、nested ZIP/CSV charset、inner dialect/native pin 変更で profile と全 UnitId 変更を検査。ZIP raw 名/NFC 衝突、path traversal、symlink、暗号、unsupported codec、overlap、entry/total/深さ bomb は Unit 0 の typed non-Supported。
- [ ] GREEN: Q01 GO の pin と bounded reader plan で実装。`resolve_locators` の一意な text 再構成、全 reader-use、予算の admission/解析中/host response 三層一致を検査して同じ command PASS。

### P1-S01 — 同一 PostgreSQL snapshot の全 authoritative binding

**Files:** Modify `crates/search-source-document/src/{postgres,model,lib}.rs`; create `tests/body_snapshot.rs`。

**Interfaces:** `AuthoritativeItemBinding {content_item_id:Uuid,part:ContentPartRef,representation_id:Uuid,file_id:FileId,raw:RawBinding,storage_key:StorageKey}` を trusted read model とし、`VersionSnapshotRecord.authoritative_items:Vec<AuthoritativeItemBinding>` に保持。Document Domain に `ContentItemId` wrapper は現状ないため UUID を snapshot 境界で検証し、`ContentPartRef.source_native_part_id` に canonical UUID 文字列を入れる。`PostgresDocumentSnapshotReader::read` の既存 `REPEATABLE READ, READ ONLY` transaction で `content_items`→`content_representations` role `AUTHORITATIVE`→`file_objects` を `(ordinal,logical_path,content_item_id)` 順に読む。

- [ ] RED: `cargo test -p search-source-document --locked --test body_snapshot` は複数 Part/同一 FileObject、authoritative representation が0件・欠落 FileObject・不正 path/ordinal/hash/MIME、Dsi/旧 version_files/rendition にしか本文がない場合を検査。2件は DB unique 制約の拒否を検査し、reader にも duplicate row の fail-closed guard を置く。live/historical を混在させない。
- [ ] GREEN: 一 snapshot 内の結合と UUID/path/size/hash/lowercase MIME essence 検証を追加し、同じ command PASS。既存 `postgres_snapshot` と `translation_contract` を対象回帰する。

### P1-S02 — Raw read、profile registry、host Unit 検証

**Files:** Create `crates/search-source-document/src/extraction.rs` and `tests/body_extraction.rs`; modify `crates/search-source-document/{Cargo.toml,src/lib.rs}` only for qualified internal crate edges and module export。

**Interfaces:** `ExtractedItemResult {operation:ItemOperationState,coverage:Option<BodyCoverage>,units:Vec<KnowledgeUnit>,detected_format:FormatId,profile:ExtractionProfileId}` と `BodyBuildError::{Retryable,Integrity,Configuration}`。`DocumentBodyExtractor<F:FileStorage,E:ContentExtractor>::extract_item(&self,snapshot:&VersionSnapshotRecord,item:&AuthoritativeItemBinding)->Result<ExtractedItemResult,BodyBuildError>`。`ContentExtractor::{extract(raw:&[u8],profile:&RegisteredProfile),resolve_locators(raw:&[u8],profile:&RegisteredProfile,locators:&[NativeLocator])}` は Source ID を持たない port として `WorkerReport` / `Vec<WorkerFragment>` を返す。host は `FileStorage::open(&StorageKey)` を上限付きで読み、前後 SHA-256/size、registered profile definition/build/native pin と worker reader-use、`validate_part_units`、native round-trip と full-scope/known omission を再検査してから Version/Part/provenance/UnitId を付ける。P1-B01 が `ExtractedItemResult` と authoritative binding から `BodyItemEntry` を作る。

- [ ] RED: `cargo test -p search-source-document --locked --test body_extraction` は raw 前後の入替え、同 hash 文面の別 Part、profile/build/inner reader差、locator改変/重複/親循環、応答切断、format spoof、coverage 不整合を拒否する。worker へ StorageKey/actor/Source ID が渡らない assertion を含む。
- [ ] GREEN: Source-owned target binding、bounded byte read と host validator を実装して同じ command PASS。raw mismatch/config/sandbox 失敗は item failure へ偽装せず generation build error。既知 permanent と retryable の revision §6 matrix をコードで照合する。

### P1-B01 — Canonical Unit manifest、coverage artifact と receipt

**Files:** Create `crates/search-source-document/src/{body_manifest,body_bundle}.rs` and `tests/body_manifest.rs`; modify `src/lib.rs` after S02。

**Interfaces:** `BodyItemEntry`、`BodyUnitManifest {key,source_snapshot,entries}`、`BodyCoverageArtifact {key,items}`、`ArtifactReceipt {key,digest,count}`、`GenerationBundleReceipt` は revision §3 の field/順序。`validate_manifest(&BodyUnitManifest,source_snapshot:&DocumentOutboxSnapshot)->Result<BodyCoverageArtifact,BodyBuildError>` と `compute_bundle_receipt(...)->Result<GenerationBundleReceipt,BodyBuildError>` は canonical UUID bytes/BE/frame/option tag で runtime 再計算する。projection digest は既存 `manifest.digest` を hex decode して入れる。

- [ ] RED: `cargo test -p search-source-document --locked --test body_manifest` は全 Live authoritative item が厳密に一度、規定 sort、`Completed+Supported/Partial/Unsupported` と `FailedPermanent` の合法組合せ、`Retryable` の publish 禁止、wrong key/count/text digest、未知 tag/trailing/non-NFC を検査。body のみ変更で composite digest が変わり projection-only digest が不変、snapshot/generation ID 差を除いた rebuild equivalence が同値。
- [ ] GREEN: `body-unit-manifest:v1\0`、`body-coverage:v1\0`、`body-profile-set:v1\0`、`document-generation-bundle:v1\0` の exact canonical encoding を実装し、同じ command PASS。

### P1-E01 — Tantivy schema 2、実 doc seal と bounded unique-parent query

**Files:** Modify `crates/search-tantivy/src/{schema,index,lib}.rs`; create `tests/body_lexical.rs`。P1-S02/B01 の検証済み Unit だけを入力にする。`query.rs` の変更は P1-E02 が所有する。

**Interfaces:** `LexicalBuildInput` に `units:Vec<KnowledgeUnit>` を別 collection として加え、`TantivyLexicalIndex::enumerate_unit_docs(key)->Result<Vec<IndexedUnitDoc>,LexicalIndexError>` で構築後の検索可能 doc を列挙する。`IndexedUnitDoc {generation:ProjectionGenerationKey,parent_resource:ResourceId,version:ResourceVersionRef,part:ContentPartRef,authoritative_representation_ref:String,raw:RawBinding,unit_id:UnitId,ordinal:u32,kind:UnitKind,locator:NativeLocator,profile:ExtractionProfileId,text_sha256:[u8;32],text:String}` の field を stored value から再構成する。Resource doc と Unit doc は型tagで分け、Unit doc は body field だけに text を入れる。body query の application port への接続は P1-E02 が所有する。

- [ ] RED: `cargo test -p search-tantivy --locked --test body_lexical` の `schema2_resource_and_unit_fields` と `enumerated_docs_reflect_actual_index_not_builder_inputs` を追加。欠落/重複/本文差替えした実 index からは builder 入力と異なる doc 集合が返る。既存 schema 1 generation の read は維持するが body-ready にしない。
- [ ] GREEN: schema 2 index、実 doc 列挙、deterministic lexical staged input receipt を実装して同じ command と `lexical_contract` PASS。`Supported/Partial` 全 Unit と実 doc の双方向 seal、Unsupported/Failed の doc 0 は P1-B02 で検証する。Graph receipt は P1-B02 が既存 Graph staged input から独立に再計算する。

### P1-B02 — 同一 key runtime stage、seal、CAS、cleanup、pin

**Files:** Modify `crates/search-source-document/src/outbox.rs`; create `tests/body_bundle_runtime.rs`。`search-projection-memory/src/store.rs` は projection-only digest を維持し、必要な pointer 操作だけを単独 writer で追加する。

**Interfaces:** `DocumentIndexRuntime::{stage_body_unit_manifest,stage_body_coverage,validate_bundle,pin_current_bundle,discard_body_generation}` を revision §3 の引数/返値で追加。`MemoryDocumentIndexRuntime` は一つの共有 lock で `Staging→Validated→Published|Discarded`、同 key の projection/Unit/coverage/lexical/Graph artifact と seal を検証し、projection pointer CAS を最後に行う。`PinnedBodyBundle` は外部から構築不可の immutable handle。

- [ ] RED: `cargo test -p search-source-document --locked --test body_bundle_runtime` は各 artifact の missing/wrong key/digest/count、lexical 実 doc seal の失敗、validated 後変更、同時 publish CAS 敗北、stage/cleanup 障害、旧 in-flight pin、公開後 receipt 保存失敗を注入する。どれも未公開 artifact を query に見せず旧 pointer を保つ。
- [ ] GREEN: revision §3 の全 stage/validate/discard と lock 内 CAS を実装し、同じ command と既存 `outbox_indexing` の対象ケースを PASS。cleanup 失敗は incident、公開後 receipt 保存失敗は公開 bundle を維持して再試行で補完。

### P1-B03 — Outbox full/incremental body rebuild と current Source seal

**Files:** Modify `crates/search-source-document/src/outbox.rs`; create `tests/body_reconciliation.rs`。

**Interfaces:** `DocumentOutboxIndexer::reconcile_once` は S01/S02/B01 の同一 snapshot から全 item の body manifest と lexical Unit input を作り、B02 の validate 後、公開直前の新 Source read で current Version/T10、document/access revision、全 Part/representation/raw を再照合する。`IndexingReceipt` は projection と composite digest/schema/profile を比較し、旧 projection-only generation を body-ready とみなさない。

- [ ] RED: `cargo test -p search-source-document --locked --test body_reconciliation` は body-only 変更、同 raw/profile unchanged、Part 追加/削除、Version/T10/Read 差替え、CAS 敗北、Source outage、legacy bundle 欠落、permanent failure、retryable timeout、full/incremental 同値と再 bind を検査。新 generation 失敗時は旧 pointer/readを保持。
- [ ] GREEN: same key の stage順、bounded Source reread retry、失敗全 artifact discard、receipt/no-op 条件を実装し、同じ command と `generation_races` の対象ケース PASS。DB/Search 分散 transaction を作らない。

### P1-A02 — Request 単位 `BodyOnly` と共通 Discovery loop

**Files:** Create `crates/search-application/src/{content_scope,body_ports}.rs`; modify `src/{ports,lib,retrieval_execution,discovery_service}.rs`、`crates/search-source-document/src/coverage.rs`。Test `crates/search-application/tests/body_discovery_contract.rs` and `crates/search-source-document/tests/body_preflight.rs`。P4 service/ports の該当 writer 完了後に統合する。

**Interfaces:** `BodySearchSpec {query:LexicalQuery,exact_text_claim:Option<ClaimId>}`、`DiscoveryScope::{Normal,BodyRequired(BodySearchSpec)}`。`DocumentCoveragePreflight::discover(service,request,requirement,body:Option<BodySearchSpec>)`。`DiscoveryService::discover_with_content_scope(request,scope)` と P4 `discover_scoped(request,ScopedDiscoveryExecution{content_scope,...})` は同じ内部 evaluator を呼ぶ。`LexicalFieldScope::{ExistingFields,BodyOnly}`、`LexicalQuery.field_scope`、`LexicalRetrieverPort::retrieve(...)->BoxFuture<'a,LexicalRetrievalBatch>`、`LexicalHit {candidate:FederatedCandidate,unit_hit:Option<KnowledgeUnitHitRef>}`、`LexicalRetrievalBatch {hits:Vec<LexicalHit>,exhausted_matching_units:bool}`。`KnowledgeUnitHitRef {generation:ProjectionGenerationKey,parent_resource:ResourceId,version:ResourceVersionRef,part:ContentPartRef,authoritative_representation_ref:String,unit_id:UnitId,span:TextSpan,text_sha256:[u8;32],raw:RawBinding,profile:ExtractionProfileId,opaque_locator:String}` を `body_ports.rs` に定義し、`RawRetrievalHit.unit_hit` と `HitRecord` に同じ ref を保持。`ExactTextSelector` と `VerifiedExtractedTextEvidence` の field も revision §5 どおり同 file に定義し、A03 が trusted 実装を追加する。

- [ ] RED: `cargo test -p search-application --locked --test body_discovery_contract` は constructor の固定 `config.lexical_query` を使わない request 別 query、title-only/Graph-only/Vector-only を body qualified 0、missing port/bundle、action limit、scope swap、Unit ref dedup後保持、P4 の scoped 共通 loop と S1 順序を検査。`cargo test -p search-source-document --locked --test body_preflight` は `BodyRequired` の body 引数なしと forged selector を blocking gap にする。
- [ ] GREEN: P4 の API 名と共有 evaluator を保持して typed scope を planner→executor→port→qualification へ貫通。`BodyRequired` は Document Source Lexical action の verified Unit hit 以外を数えない。query は非空・上限内・`BodyOnly`、claim/selector/required 値は同じ normalized bytes/親/ClaimId に bind。全可視 Live item の coverage gap は現行 Read で絞り、Denied 側の ID/件数/理由を消し、Unknown は非開示の単一 blocking gap にする。両 command と既存 `discovery_loop` PASS。

### P1-E02 — BodyOnly の unique Resource refill と exact span

**Files:** Modify `crates/search-tantivy/src/query.rs`; test `crates/search-tantivy/tests/body_query.rs`。P1-A02 の typed port と P1-E01 の schema 2 を消費する。

**Interfaces:** `TantivyLexicalIndex` implements `LexicalRetrieverPort::retrieve(...)->LexicalRetrievalBatch`。`BodyOnly` は Unit doc の body field のみ、`ExistingFields` は従来の canonical name→aliases→title→high signal→body tier と S1 順を維持する。返す `KnowledgeUnitHitRef` の candidate ID は `"{source_id}:{parent ResourceId}"`、`resource_ref` は親 Version ResourceId。

- [ ] RED: `cargo test -p search-tantivy --locked --test body_query` の `body_only_excludes_title_and_resource_doc`、`unique_parent_refill_and_underfill`、`same_text_separate_part_keeps_ref`、`token_hit_without_literal_span_is_not_qualified`。同親の大量 Unit 後に別親の可視候補が補充され、window 不足では `exhausted_matching_units=false`。Tantivy の token no-hit を exact 不存在へ使わない。
- [ ] GREEN: `(body tier,score desc,Unit ordinal,UnitId)` で親代表、unique parent に `limit`、有界 refill と underfill trace を実装。実 Unit.text 上で同じ正規化 query literal の UTF-8 `TextSpan` を確認できる hit だけ `unit_hit` を持つ。上記と既存 `lexical_contract` PASS。

### P1-A03 — Source-owned exact positive evidence と parent-bound claim

**Files:** Create `crates/search-source-document/src/body_evidence.rs`; modify `src/{evidence,lib}.rs` and `crates/search-application/src/{body_ports,evidence_resolution,discovery_service}.rs`; test `crates/search-source-document/tests/body_evidence.rs` and `crates/search-application/tests/body_claim.rs`。

**Interfaces:** A02 の `KnowledgeUnitHitRef`、`ExactTextSelector {claim_id:ClaimId,parent_resource:ResourceId,predicate:String,expected_exact_text:String}`、`VerifiedExtractedTextEvidence {assertion:Assertion,resolved:ResolvedAssertionEvidence,matched_span:TextSpan}` を消費する。`ExactTextEvidencePort::{selector_for,resolve_hit}` は revision §5 の signature。`DocumentExactTextEvidenceCatalog::resolve_hit` は pinned manifest Unit、selector/required ClaimId・parent/Source/predicate/value、span、current Live/Read/Part/raw、FileStorage raw の同一 profile/locator 再解析を検証。`assemble_verified_unit_text_claim(required,selector,verified)->Claim` は限定述語 `document.body.contains_exact` のみを `Extracted`/direct `Primary` にする。

- [ ] RED: `cargo test -p search-source-document --locked --test body_evidence` と `cargo test -p search-application --locked --test body_claim` は同 phrase の別 parent/Part、forged UnitId/locator/span/raw/profile、wrong required ClaimId/subject/predicate/value、Read Unknown/取消、途中 raw変更、公開直前取消を `Unknown` と非開示 gap にする。`Completed+Partial` の verified Unit は肯定 claim と blocking coverage gap が共存する。汎用 body hit、Vector similarity、title/policy claim は Primary にしない。
- [ ] GREEN: Source re-read と `TextSpan` UTF-8 boundary/期待 literal 一致、現行 access 再照合、`ResolvedAssertionEvidence {content_digest:text_sha256,is_summary:false}` と同 generation/Source/parent の citation chain を実装。両 command と既存 `evidence_resolution_contract` PASS。

### P1-A04 — 有限 Source-owned exact negative proof

**Files:** Create `crates/search-source-document/src/body_absence.rs`; modify `crates/search-source-document/src/lib.rs` and `crates/search-application/src/{body_ports,discovery_service}.rs`; test `crates/search-source-document/tests/body_absence.rs` and `crates/search-application/tests/body_absence_contract.rs`。

**Interfaces:** private `ExactTextNegativeProof`、`ExactScanBudget {max_visible_items,max_units,max_text_bytes,deadline}`、`ExactTextAbsenceOutcome::{ProvenAbsent,MatchFound,Unknown}`、`SourceExactTextAbsencePort::verify_absence(request,pinned,selector,budget)` は amendment §4。初期 finite request 上限は `max_visible_items=1024, max_units=100000, max_text_bytes=67108864, deadline=2s`、各 profile のより小さい ceiling が優先する。`ProvenAbsent` constructor は Source port 専有、receipt は generation/bundle digest/snapshot/ClaimId/parent/revisions/predicate/text SHA/binding列 digest/count に限定。

- [ ] RED: `cargo test -p search-source-document --locked --test body_absence` は selector が指定した一つの現行 Live 親 Version の全可視 AUTHORITATIVE item の Source-owned enumeration、Read Denied 除外・Unknown 非開示、全 Part/profile/raw/coverage/Unit 集合一致、全 Unit の literal scan、空本文 `Supported`、件数/bytes/deadline、最後の authority 再照合を検査。`Partial`/Failed/Unsupported/欠落 Unit/新 Part/old Version は `Absent` 0件。
- [ ] RED: `cargo test -p search-application --locked --test body_absence_contract` は `BodyOnly` no-hit/exhaustion だけで `Absent` にならず、query `甲` 対 selector `乙`、`東京都` 内の `京` token miss、句読点/NFC/CRLF、別 Unit 間 `東`+`京`、別 parent receipt、Read 取消を `Unknown`/blocking gap にする。`Partial` に `東京` 肯定がある親の `大阪` 否定も `Unknown`。
- [ ] GREEN: lexical no-hit/exhaustion を scan の起点にだけ使い、pinned manifest の全文を正規化 literal で走査。`MatchFound` は integrity/recall signal とし hit/evidence は mint しない。proof 発行直前と disclosure 直前に Source Read/Version/Part/raw/revision/bundle を再確認する。両 command PASS。

### P1-V01 — 実 DB/FS/Linux 縦断、回帰、容量測定と独立判定

**Files:** Create `crates/search-source-document/tests/body_vertical.rs` and `experiments/search-extraction-poc/{measure.py,report.json,report.md}`; modify only fixture/test wiring needed by previous Tasks。P4/P6/P7 の共有 integration head を pin する。

- [ ] RED: `cargo test -p search-source-document --locked --test body_vertical` は実 PostgreSQL/FS と hosted Linux Search sandbox で、現行 Version 複数 Part→raw read→reader→manifest→Tantivy実doc seal→Graph/lexical→CAS→BodyOnly→肯定/否定/coverage を一周させる。title-only、Graph-only、同一 FileObject の別 Part、Partial/Denied Partial、Read Unknown/取消、T10/History、新 Version、body-only変更、full/incremental、CAS敗北、stage障害、raw mismatch、failure matrix を assertion する。旧 DSI の semantic output 文字列は body hit 0。
- [ ] GREEN: 必要な integration wiring のみ修正し、上記 command、`cargo test -p search-tantivy --locked --test lexical_contract`、`cargo test -p document-semantic-inspection-runner --locked --test runner_baseline --test runner_isolation`、`cargo clippy -p document-sandbox-runner -p search-extraction-core -p search-extraction-runner -p search-extraction-worker -p search-source-document -p search-tantivy -p search-application --all-targets --locked -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` を PASS。実 Linux sandbox canary で Landlock/seccomp の拒否を観測し、local 非強制結果を混ぜない。最終統合 head でのみ `mise run verify:fast`、必要な hosted exact-head gate を実行する。
- [ ] `measure.py` は 1/10/50 MiB 代表（format別）、最大 Unit/多数小 Part/高圧縮 ZIP を cold/warm で実行し、wall/CPU、peak RSS または cgroup high-water、scratch/output bytes、Unit数、index build、query p50/p95/p99、unique Resource 充足率を `report.json` に記録する。未実施/上限で拒否した case を明示し、未測定の本番 SLO を宣言しない。corpus/lock/parser/PDFium/環境 hash、format別 fidelity、Partial/Unsupported、最悪case、既存 DSI 回帰、独立 read-only review と exact-head CI receipt を別欄に記録する。

## Self-review and handoff

- Freeze の §1–6 と revision §2–7 は、最小 Unit 契約（別 lane）→Q01→I01–05→S01–02→B01–03/E01→A02/E02/A03–04→V01 の所有 Task に対応。特に amendment の実 lexical doc seal、有限 Source-owned negative proof、Partial positive correction を個別 RED に置いた。
- 実装順の並列余地は Q01 と最小 Unit core の独立作業、I03/I04/I05 の**別 reader 本体ファイル**、S01 と reader 資格の非競合部分に限る。root Cargo/lock、reader `mod.rs`、Source `lib.rs`、DSI runner refactor、Tantivy schema、Source outbox、application 共通 loop は各々単独 writer とする。P4 の scoped service が先、P6 の ports 編集と P1-A02/A03 は直列化する。
- 各 GREEN は局所主張だけを証明する。P1 完了は production code、実形式 fidelity、安全な Linux process、実 DB/FS、独立 review、exact-head hosted gate の結果を揃えて判断する。merge/deploy は別操作。
