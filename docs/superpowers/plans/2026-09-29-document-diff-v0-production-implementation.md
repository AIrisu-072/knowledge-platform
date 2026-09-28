# Document Diff v0 Production Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development` または `superpowers:executing-plans` を使い、Taskを順に実装する。以下の `- [ ]` は将来の実行欄であり、現時点の完了記録ではない。

**状態:** **PROPOSED / PLAN REVIEW PENDING / IMPLEMENTATION BLOCKED**。

**Goal:** 同じDocumentの2つのDocumentVersionについて、形式固有の意味・構造差分、原本位置、未比較範囲を持つ認可済みDiffResultと新旧対照表Projectionを提供する。

**Architecture:** Applicationが現在認可、両版のauthoritative snapshot、DSI証拠、監査付き原本取得、最終認可と開示監査を組み立てる。独立したDiff workerが一時的な形式固有parse modelから変更を返し、結果だけをsnapshot key付きの容量制限cacheへ置く。Search、DSIの永続意味証拠、Documentの正本は変更しない。

**Tech Stack:** Rust 1.98 / edition 2024、既存のPostgreSQL/sqlx・testcontainers、`sha2`、`serde_json`、既存workspaceでpin済みのDSI形式parserとLinux `landlock 0.4.7` / `seccompiler 0.5.0`。新しいparser libraryやlicense例外は本計画で自動承認しない。

**Spec:** `docs/superpowers/specs/2026-09-28-document-diff-v0-design.md`。依頼者承認対象blob `afee4e9351c5027b1252e8c7b74e542a295f0e67`、承認記録 `docs/superpowers/specs/2026-09-28-document-diff-v0-design-approval.md`。基準は `main@6ea29e1ceea82bb0e20b195890b7c0e7efc85f68`。2026-09-29の確認時、main標準CI `36419479837` はsecurity setup失敗であり、Diff実装のGREEN根拠にはならない。

## Global Constraints

- 本計画はレビュー待ち。計画と実行方法の承認前に製品コード、migration、dependency、実装PRを変更・作成しない。実装開始時はmain、設計blob、PR、CI、既存migration番号を再取得する。
- `spec/`が規範の正本。新しいDiff開示監査eventは実装工程で規範へ反映し、承認設計の意味を変える必要があればamendment gateへ戻る。
- 対象は同じDocumentの異なる2つのVersion ID。正規化titleと順序付きauthoritative manifestに従い、Search chunk、rendition、raw SHAだけから意味同一性を判断しない。
- 現行DSIのproduction対象8形式を同形式Diffの対象とし、異形式の詳細内容は未比較とする。各形式のDiff資格試験に合格した範囲だけ `Full` と呼ぶ。
- `Same` は必要な意味範囲が全域で比較済みの場合だけ。曖昧な対応、parser問題、資源上限は範囲付き `Partial/Unknown` とし、権限・WORKING鮮度・raw binding・必須Audit失敗は結果を返さない。
- PUBLISHED/WORKING/WITHDRAWN/T10の現在認可を両版に適用し、cache hitも再確認する。原本の各openは既存の事前監査、Diff結果開示は新しい必須Auditを通す。actorの有効期限も最終確認する。
- workerにはFileId、DocumentId、Principal、Storage key、DB・Storage credentialsを渡さない。Diff用isolated runnerはLinuxでLandlock/seccomp/rlimit/timeoutを必須とし、非Linux production実行を拒否する。
- 新しいproduction parser dependencyの追加は、個別のPoC・license/security選定・依頼者承認なしに行わない。DSI選定済みlibraryのDiff適合も形式別fixtureで検証する。
- 必須Audit本文に原文・抜粋・Storage locatorを入れない。cacheは正本でなく、直接公開APIを持たない。LLMをDiff correctnessのruntime依存にしない。
- Taskごとに焦点RED/GREENを残す。hosted CIは各レビュー単位の最終headで一度確認し、Taskごとには起動しない。失敗時は原因のあるgateだけを再確認する。PR作成はDraft、mergeは別の明示指示を要する。
- 実行方法は依頼者の先の希望に合わせNative/inlineを予定する。Document Diffの実装開始は別指示を必要とし、workerへ切り替える場合はそのTaskのmodel/effort指定を確認する。

## Review Focus

| 入力・競合 | 期待する結果 | 所有Task |
|---|---|---|
| 同じ意味でpackage byteだけ異なる原本 | 互換DSI証拠で `Same + Full`。raw hash差を内容変更としない | DIF-01/05/15 |
| 比較中のWORKING更新、T10、権限剥奪、actor期限切れ | 古い結果・cacheを返さない。最終認可と監査が同一確定境界 | DIF-02/03/15 |
| 重複した表行・段落、移動＋修正に見える候補群 | 推測で `Moved` や `Added/Removed` とせず、対応未確定と原本範囲を返す | DIF-05/11/15 |
| 旧VersionのContentItem分類が未確定、DSI証拠欠落 | 正本を推測せず、再検査可能なものだけ再生成。残りは比較済みとしない | DIF-02/05/15 |
| cache結果のdigest不一致、原本raw binding不一致、Audit失敗 | 結果開示なし。cache missや部分結果へ無言で置き換えない | DIF-03/06/15 |

## File Map / インターフェース

新規crateは3つに限定する。`document-diff-core` はIDやStorage keyを含まないworker wire contractと純粋なbounded alignment、`document-diff-worker` は形式固有parser、`document-diff-runner` は二原本の隔離起動を持つ。Repositoryの `DiffCache` port実装がin-process bounded cacheを提供する。DSI workerの永続schemaやparser結果型を変更しない。Diff workerは既存 `document-semantic-inspection-runner::seal_worker_sandbox` をLinuxで再利用し、専用runnerは二つのread-only FD、raw binding確認、process制限を資格試験する。

| 責務 | 新規/変更先 |
|---|---|
| worker protocol・共通ChangeSet・bounded alignment | `crates/document-diff-core/{Cargo.toml,src/lib.rs,src/protocol.rs,src/change.rs,src/alignment.rs}`、root `Cargo.toml` |
| Application result・snapshot identity・port・service・Projection | `crates/document-application/src/document_diff/{mod.rs,model.rs,snapshot.rs,ports.rs,evidence.rs,service.rs,projection.rs}`、既存 `src/{lib.rs,error.rs}` と `Cargo.toml` |
| PostgreSQL pair snapshot・最終認可/Audit・派生cache | `crates/document-repository-postgres/src/{document_diff_snapshot.rs,document_diff_access.rs,document_diff_cache.rs}`、既存 `src/lib.rs`。新しい永続cache tableは作らない |
| isolated runner | `crates/document-diff-runner/{Cargo.toml,src/lib.rs,src/linux.rs,src/executor.rs}` |
| worker shell・各形式 | `crates/document-diff-worker/{Cargo.toml,src/lib.rs,src/main.rs,src/shell.rs,src/adapters/{mod.rs,text.rs,csv.rs,html.rs,docx.rs,spreadsheet.rs,vba.rs,pptx.rs,pdf.rs}}` |
| 規範・資格・受入fixture | `spec/architecture/architecture-contract-v0.md`、`spec/data/{logical-data-model-v0.md,transaction-consistency-requirements-v0.md}`、`spec/operations/{observability-audit-requirements-v0.md,error-handling-resilience-requirements-v0.md}`、`experiments/document-diff/qualification.md`、各crateのfocused `tests/` |

Task間の固定API:

```rust
// document-application::document_diff
pub struct DiffRequest { pub document_id: DocumentId, pub base_version_id: DocumentVersionId,
                         pub target_version_id: DocumentVersionId, pub profile: DiffProfileVersion }
pub struct AuthorizedDiff { pub result: Arc<DiffResult>, pub audit_event_id: Uuid }
pub struct AuthorizedComparisonTable { pub rows: Vec<ComparisonRow>, pub audit_event_id: Uuid }
// DocumentDiffService のimpl内の公開method
pub async fn compare(&self, ctx: &VerifiedActorContext,
                     request: DiffRequest) -> Result<AuthorizedDiff, ApplicationError>;
pub async fn comparison_table(&self, ctx: &VerifiedActorContext,
                     request: DiffRequest) -> Result<AuthorizedComparisonTable, ApplicationError>;
pub trait DocumentDiffRepository {
    async fn capture_pair(&self, ctx: &VerifiedActorContext,
                          request: DiffRequest) -> Result<DiffPairSnapshot, RepositoryError>;
    async fn authorize_and_audit_result(&self, ctx: &VerifiedActorContext,
                                        pair: &DiffPairSnapshot, result_digest: [u8; 32],
                                        verdict: ContentVerdict, coverage: DiffCoverage,
                                        cache_hit: bool) -> Result<Uuid, RepositoryError>;
}
pub trait DiffInspectionEvidence {
    async fn ensure(&self, file_id: FileId,
                    profile: InspectionProfileVersion) -> Result<SemanticInspectionRecord, ApplicationError>;
}
pub trait DiffExecutor {
    async fn compare(&self, request: WorkerDiffRequest, base: ContentReader,
                     target: ContentReader) -> Result<WorkerDiffResponse, DiffExecutionError>;
}
pub trait DiffCache { fn get(&self, key: &DiffCacheKey) -> Result<Option<Arc<DiffResult>>, DiffCacheError>;
                      fn put(&self, key: DiffCacheKey, result: Arc<DiffResult>) -> Result<(), DiffCacheError>; }
// document-diff-worker の全形式adapterが実装する純粋な比較口
pub trait FormatComparator {
    fn compare(&self, base: &[u8], target: &[u8],
               budget: &mut ComparisonBudget) -> Result<WorkerDiffResponse, WorkerDiffFailure>;
}
```

`DiffResult` と `DiffPairSnapshot` はDocument ID、両Version ID、ordered authoritative items、Version revision/manifestとDSI binding、`Same/Different/Unknown`、`Full/Partial/None`、changes/unverified/ancillary、result digestを型で保持する。`Change` はoperation、relocation、形式固有facet、`base: Option<SourceEvidence>`、`target: Option<SourceEvidence>`、reason codeを持つ。`SourceEvidence` はDocument/Version/ContentItem/authoritative Representation/FileObjectのID、raw hash、DSI profile、形式固有locator、locator粒度、parser provenanceを持ち、Storage keyは外へ出さない。`UnverifiedRegion` は両側の示せる範囲、理由code、原本へのnavigation hintを持つ。`DiffExecutionError` は `ResourceLimit / Unsupported / RawBindingMismatch / SandboxUnavailable / InvalidResponse`、`DiffCacheError` は `Unavailable / IntegrityViolation` を少なくとも区別する。worker wireはformat、profile、両raw hash/size、resource profile、形式固有locator/changeだけとし、Document側IDはApplicationで付ける。追加はbase locator、削除はtarget locatorを `None` とする。`AuthorizedDiff.audit_event_id` はcache用canonical resultに含めない。

初期Diff resource profileの**資格試験候補**は原本各256 MiB・合計512 MiB、request 64 KiB、result 16 MiB、stderr 1 MiB、wall 30秒、CPU 25秒、address space 4 GiB、temp 2 GiB、node各100万・depth 256・候補照合800万・change 10万とする。cache候補はprocessあたり128 MiB、entry 16 MiB、64 entry。これらは承認済みのproduction実測値ではない。DIF-04/DIF-15で境界値と1超過・代表大文書を計測し、許容できなければ値を勝手に緩和せず計画改訂へ戻る。限界に達した区画は理由付き未比較となる。

## Delivery / verification units

| Draft unit | Task | 対象gate |
|---|---|---|
| A: core・現在認可・隔離worker・cache | DIF-01〜06 | 焦点Rust/実DB、標準CI、Linux sandboxをAの最終headで一度 |
| B: TXT/CSV/HTML | DIF-07〜09 | 3形式のDiff fixture、標準CIをB最終headで一度 |
| C: DOCX/XLSX/XLSM/PPTX | DIF-10〜13 | Office各資格fixture、標準CIをC最終headで一度 |
| D: PDF・横断受入 | DIF-14〜15 | PDF/Diff全体、標準CI・Sandbox・DSI PoCをD最終headで一度 |

A→B→C→Dの依存順とし、必要ならstacked Draft PRにする。各Taskの局所RED/GREENはcommitと焦点commandで残し、hosted CIを毎Taskで繰り返さない。最終の成功判定はDの**同一head**で標準CI、Sandbox、DSI PoC、Diff専用受入fixtureがすべて成功した場合だけとする。現在のmain security setup失敗が続く場合は、原因を別記録し、GREENと偽らない。未承認のmerge・deployは行わない。

---

### DIF-01: Diff型・worker protocol・snapshot key

**Files:** Create `crates/document-diff-core/src/{lib.rs,protocol.rs,change.rs}` と `crates/document-application/src/document_diff/{mod.rs,model.rs,snapshot.rs}`、各Cargo設定。Modify root `Cargo.toml`、Application `src/lib.rs`、`spec/architecture/architecture-contract-v0.md` と `spec/operations/error-handling-resilience-requirements-v0.md`。

**Interfaces:** `DiffProfileVersion::V0`、`DiffRequest`、`DiffResult`、`DiffCacheKey::from_pair(&DiffPairSnapshot, DiffProfileVersion, ResourceProfileVersion)`、`WorkerDiffRequest/Response`、`ComparisonBudget` と `WorkerDiffFailure`。wireにDocument/Principal/Storage keyを入れない。

- [ ] **RED:** 空のcore crate/Cargo登録を作ったうえで、`crates/document-application/tests/document_diff_identity.rs` にtitle CRLF/NFC invariant、manifest順・format/profile・raw binding・版固有metadata変更でkeyが変わる、Document revision単体ではkeyを決めない、同一意味/別package bytesでは意味判定を変えない試験を追加する。`crates/document-diff-core/tests/protocol.rs` でID/credential fieldのないwire、未知version・過大response・不正locator、追加/削除で存在しない側の架空locatorを拒否する。
- [ ] **RED確認:** `cargo test -p document-application --test document_diff_identity` と `cargo test -p document-diff-core --test protocol` が未定義API・契約不一致のみでFAIL。
- [ ] **GREEN:** 上記型、canonical digest/serializationとresponse validatorを実装。Version identity digestとraw bindingを別fieldに保つ。規範へDiffの派生境界とhard error/部分結果の区別を設計どおり追記する。
- [ ] **確認:** 同じ焦点commandがPASSし、`cargo fmt --all -- --check` がPASS。
- [ ] **commit:** 上記ファイルをcommitし、snapshot/protocol contractを固定する。

### DIF-02: 認可付き両版snapshotとDSI証拠

**Files:** Create Application `document_diff/{ports.rs,evidence.rs}`、Postgres `document_diff_snapshot.rs`、`crates/document-repository-postgres/tests/document_diff_snapshot.rs`。Modify各 `lib.rs`。

**Interfaces:** `DocumentDiffRepository::capture_pair`、`DiffInspectionEvidence::ensure`。既存 `authorize_version_in_tx`、`begin_snapshot`、`EnsureSemanticInspection::ensure` を再利用する。

- [ ] **RED:** 実DB試験で異Document・同一Version拒否、現行PUBLISHED、過去PUBLISHED/WITHDRAWN、WORKING、T10後、分類未確定legacy、DSI record欠落、同時manifest更新を固定する。欠落時は保存済みmanifestを証拠済みと偽らない。DSI再生成の前後でmanifestが変わった場合は古いsnapshotと新しいevidenceを混ぜない。
- [ ] **RED確認:** `cargo test -p document-repository-postgres --test document_diff_snapshot` が新API不在だけでFAIL。
- [ ] **GREEN:** 両Version・ordered ContentItem・authoritative representation・FileObject・DSI bindingを単一consistent snapshotで取得し、現在状態から参照purposeを導く。`evidence.rs` で既存 `EnsureSemanticInspection::ensure` をportへ接続し、欠落DSIだけを安全に再生成する。再生成後は両版を再取得してraw/manifest bindingを照合し、最終snapshot identityへ確定したevidence digestを含める。
- [ ] **確認:** 同焦点試験と既存 `cargo test -p document-repository-postgres --test document_history_projection` がPASS。
- [ ] **commit:** snapshot/port/試験のみをcommitする。

### DIF-03: 原本byte取得と最終認可・必須Audit

**Files:** Create Postgres `document_diff_access.rs`、`tests/document_diff_access.rs`。Modify Application `document_diff/ports.rs`、`src/error.rs` と規範 `spec/data/transaction-consistency-requirements-v0.md`、`spec/operations/observability-audit-requirements-v0.md`。Application `document_diff/service.rs` はDIF-05で作る。

**Interfaces:** `DocumentDiffRepository::authorize_and_audit_result`。原本openは既存 `VersionFileAccessService::open_version_file` の事前Auditを通す。結果開示の線形化点は最終認可・鮮度確認・`document.diff.result_access_granted` 作成の同一短いDB transaction。

- [ ] **RED:** 実DB試験で計算中のpolicy剥奪・T10・WORKING更新・actor期限切れ・audit INSERT失敗を注入し、いずれも結果未開示をassertする。cache hitでもAudit 1件、本文/Storage locatorなし、送信完了を示すeventなしをassertする。
- [ ] **RED確認:** `cargo test -p document-repository-postgres --test document_diff_access` が新finalization API不在でFAIL。
- [ ] **GREEN:** 同一transaction内に既存access guard→両版現在認可→WORKING digest/revision照合→必須Audit INSERTを実装。commit結果不明を開示成功にしない。規範へ読取確定境界、event意味・payload最小化・sampling禁止を追記する。
- [ ] **確認:** 焦点試験、既存 `cargo test -p document-repository-postgres --test version_file_access` と `git diff --check` がPASS。
- [ ] **commit:** 最終認可・監査契約をcommitする。

### DIF-04: 二原本worker shellと隔離runner

**Files:** Create `crates/document-diff-worker/src/{lib.rs,main.rs,shell.rs,adapters/mod.rs}`、`crates/document-diff-runner/src/{lib.rs,linux.rs,executor.rs}` と各Cargo設定、worker `tests/worker_shell.rs`、runner `tests/runner_isolation.rs`。Modify root `Cargo.toml`。

**Interfaces:** `DiffExecutor::compare(WorkerDiffRequest, ContentReader, ContentReader)`。workerへ二つのread-only FDとbounded requestだけを渡し、DSI runnerのLinux sealを再利用する。

- [ ] **RED:** 最小crate/Cargo登録と空のshell/runner APIを作ったうえで、不正raw hash/size、余分なFD・環境変数、network/fork/exec試行、Landlock未強制、timeout、stdout/stderr肥大、片側input欠落、worker panic、macOS production起動を拒否する試験を置く。無資格adapterは `UnsupportedSemanticConstruct` を返す。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test worker_shell` とLinux `cargo test -p document-diff-runner --test runner_isolation` が新shell/runner不在でFAIL。macOSではLinux専用実行試験をhosted gateへ委ねる。
- [ ] **GREEN:** DSIの公開sandbox sealを変更せず再利用し、二つのinputのraw bindingを親runnerとworkerで確認する。PDFium等のnative初期化後、helper threadが残っていない状態でsealし、未強制なら拒否する。上記候補resource profileをversion付きで実装し、境界値・1超過で確定的な失敗codeを返す。
- [ ] **確認:** 焦点試験PASS。Linux強制canaryはhosted Sandbox gateで確認する。共有DSI sourceを変更した場合だけ、その対象回帰を追加する。
- [ ] **commit:** worker/runner/試験と資格メモ `experiments/document-diff/qualification.md` をcommitする。

### DIF-05: ContentItem対応付け・bounded内部alignment・service合成

**Files:** Create core `src/alignment.rs`、Application `document_diff/service.rs`、`crates/document-diff-core/tests/alignment.rs`、`crates/document-application/tests/document_diff_contract.rs`。

**Interfaces:** `align_items(base: &[ItemAnchor], target: &[ItemAnchor], budget: &mut AlignmentBudget) -> AlignmentOutcome`、`DocumentDiffService::compare`。`ItemAnchor` はpath/ordinal/format/profile/fingerprint、`AlignmentBudget` は候補照合残数、`AlignmentOutcome` は確定pairと未確定clusterを持つ。Task 01〜04のport/worker responseを接続する。

- [ ] **RED:** exact path+ordinal、unique pathでのreorder、一意な同profile fingerprintでの配置変更、重複fingerprint・重複行の曖昧性、移動＋修正候補、異形式、同fingerprint別raw bytes、fingerprint不一致だが局所位置不明、版固有metadataだけの差を試験する。未比較がある結果は `Same` にならず、metadataだけで内容を `Different` とせず、曖昧候補を確定 `Moved/Added/Removed` にしない。
- [ ] **RED確認:** `cargo test -p document-diff-core --test alignment` と `cargo test -p document-application --test document_diff_contract` が不足APIのみでFAIL。
- [ ] **GREEN:** 階層区画・一意anchor・bounded候補で対応付け、確定差分と未比較範囲を合成する。比較器にIDを渡さず、戻りlocatorへApplicationでsource bindingを付ける。
- [ ] **確認:** 同焦点試験がPASSし、候補照合数が資源profile内であることをassertする。
- [ ] **commit:** alignment/service契約をcommitする。

### DIF-06: 容量制限cacheと新旧対照表Projection

**Files:** Create Postgres Repository `src/document_diff_cache.rs`、Application `document_diff/projection.rs`、`crates/document-repository-postgres/tests/document_diff_cache.rs`、`crates/document-application/tests/document_diff_projection.rs`。Modify `spec/data/logical-data-model-v0.md`。

**Interfaces:** `DiffCache::{get,put}` と内部純粋関数 `project_comparison_table(&DiffResult) -> Vec<ComparisonRow>`。公開入口は `DocumentDiffService::comparison_table` のみで、Task 03の最終認可/Auditを経由する。cache keyはTask 01。

- [ ] **RED:** 同snapshot再利用、WORKING変更後miss、結果digest不一致のhard error、64 entry/128 MiBの候補上限、超大entry非保存、部分結果の理由保持、未比較範囲を表から消さないことを試験する。非表示sheet・VBAのnavigation hintとitem全体へのfallbackも表に残す。cache hitとProjection再取得の両方で現在認可・最終Auditが増えることを実DB試験へ追加する。
- [ ] **RED確認:** `cargo test -p document-repository-postgres --test document_diff_cache` と `cargo test -p document-application --test document_diff_projection` が新API不在でFAIL。
- [ ] **GREEN:** Repositoryの `DiffCache` port実装としてin-process bounded cacheを作り、cache取得の一時失敗は再計算、cache保存の一時失敗は認可済みの計算結果を返す。digest不一致はintegrity failureとする。Projectionはcanonical結果を変更せず、旧/新/変更理由/原本位置/比較状態を出す。公開入口は毎回 `compare` の現在認可・Auditを実行し、古い `AuthorizedDiff` を再利用しない。規範にDiffResult/cacheが派生であり新しいDocument正本でないことを追記する。
- [ ] **確認:** 焦点試験PASS。A headで `mise run verify:fast` とhosted標準CI/Sandboxを各一度確認する。
- [ ] **commit:** Aの最終headをcommitし、結果をstatusへ記録する。

### DIF-07: TXT形式比較

**Files:** Create worker `src/adapters/text.rs` と `tests/text_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `TextComparator::compare(base, target, budget) -> WorkerDiffResponse`。既存pin `encoding_rs 0.8.41` と `unicode-normalization 0.1.25` のみを使用。

- [ ] **RED:** CRLF/LF・NFC差は `Same`、本文変更は旧新行/span付き `Modified`、追加/削除、曖昧decodeは原本item未比較をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test text_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** decodeとraw offset mappingを保持して意味textを比較し、normalize後の位置を原本位置と取り違えない。
- [ ] **確認:** `cargo test -p document-diff-worker --test text_diff` PASS。共有DSI sourceに変更がある場合だけ対象回帰を追加する。
- [ ] **commit:** TXT adapterをcommitする。

### DIF-08: CSV形式比較

**Files:** Create worker `src/adapters/csv.rs` と `tests/csv_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `CsvComparator::compare(base, target, budget) -> WorkerDiffResponse`。既存pin `csv 1.4.0` を使用。

- [ ] **RED:** quote構文noiseは `Same`、cell値・列・行変更はrow/cell locator、unique row reorderは `Reordered`、重複行は対応未確定、不整合構造・delimiter曖昧性は未比較をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test csv_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** 表の構造とcell型を保持し、行全体の総当たりをしないbounded alignmentを接続する。
- [ ] **確認:** `cargo test -p document-diff-worker --test csv_diff` PASS。共有DSI sourceに変更がある場合だけ対象回帰を追加する。
- [ ] **commit:** CSV adapterをcommitする。

### DIF-09: HTML形式比較

**Files:** Create worker `src/adapters/html.rs` と `tests/html_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `HtmlComparator::compare(base, target, budget) -> WorkerDiffResponse`。既存pin `html5ever 0.39.0` / `markup5ever_rcdom 0.39.0` を使用。

- [ ] **RED:** 空白/CSS装飾noiseは `Same`、visible text・link・image・見出し/表構造は位置付き差分、JavaScript必須の意味は未比較、script非実行をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test html_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** 意味DOMとraw sourceへのlocatorを構成し、node位置が不確かなら親DOM/itemへ広げる。
- [ ] **確認:** `cargo test -p document-diff-worker --test html_diff` PASS。B headで `mise run verify:fast` とhosted標準CIを一度確認する。
- [ ] **commit:** HTML adapterとB最終記録をcommitする。

### DIF-10: DOCX形式比較

**Files:** Create worker `src/adapters/docx.rs` と `tests/docx_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `DocxComparator::compare(base, target, budget) -> WorkerDiffResponse`。既存pin `office_oxide 0.1.11`、`quick-xml 0.42.0`、`zip 8.6.0` の範囲を資格確認する。

- [ ] **RED:** paragraph/heading/list、table cell、header/footer、footnote/endnote、link/image、sectionとtracked-change確定投影を対象に、移動＋本文変更、コメントだけの付随差、ZIP noise等価、未対応OOXMLの範囲付き未比較をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test docx_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** OOXMLの親pathと局所anchorを保持し、コメント・編集由来を内容判定から分ける。
- [ ] **確認:** `cargo test -p document-diff-worker --test docx_diff` PASS。共有DSI sourceに変更がある場合だけ対象回帰を追加する。
- [ ] **commit:** DOCX adapterをcommitする。

### DIF-11: XLSX形式比較

**Files:** Create worker `src/adapters/spreadsheet.rs` と `tests/xlsx_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `SpreadsheetComparator::xlsx(base, target, budget) -> WorkerDiffResponse`。既存pin `rxls 0.1.3`、`calamine 0.36.1`、`quick-xml 0.42.0` を資格確認する。

- [ ] **RED:** sheet順/hidden状態、cell valueとformulaを別facet、named range、table/merge、chart source/data、external referenceを差分にする。計算cacheだけの変更は `Same`、重複行移動は未確定をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test xlsx_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** workbook→sheet→range/cellの区画とsource locatorを保ち、巨大sheetの候補上限を守る。
- [ ] **確認:** `cargo test -p document-diff-worker --test xlsx_diff` PASS。共有DSI sourceに変更がある場合だけ対象回帰を追加する。
- [ ] **commit:** XLSX adapterをcommitする。

### DIF-12: XLSM/VBA形式比較

**Files:** Create worker `src/adapters/vba.rs` と `tests/xlsm_diff.rs`。Modify `src/adapters/spreadsheet.rs`、`mod.rs`、`shell.rs`。

**Interfaces:** `SpreadsheetComparator::xlsm(base, target, budget) -> WorkerDiffResponse`。XLSX差分にVBA project/module/procedure facetを合成する。既存pin `ovba 0.7.1`、`cfb 0.10.0`、`tree-sitter 0.25.10` を資格確認する。

- [ ] **RED:** module/procedure/宣言・参照変更は差分、VBAの意味を変えない空白/コメント/大小文字noiseは `Same`、未知必須構文・解析不能moduleはその範囲未比較、macro非実行をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test xlsm_diff` がVBA comparator不在でFAIL。
- [ ] **GREEN:** DSIのVBA意味規則をfixtureで一致させ、worksheet差分とmodule差分を混同しない。
- [ ] **確認:** `cargo test -p document-diff-worker --test xlsm_diff` PASS。共有DSI sourceに変更がある場合だけ対象回帰を追加する。
- [ ] **commit:** XLSM adapterをcommitする。

### DIF-13: PPTX形式比較

**Files:** Create worker `src/adapters/pptx.rs` と `tests/pptx_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `PptxComparator::compare(base, target, budget) -> WorkerDiffResponse`。既存pin `office_oxide 0.1.11`、`quick-xml 0.42.0`、`zip 8.6.0` を資格確認する。

- [ ] **RED:** slide順、shape/text、chart data、SmartArt、image、link、notes、意味を持つ重なり/配置は差分。theme/font/background noiseは内容差分にせず、object対応が曖昧ならslide範囲未比較をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test pptx_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** slide→shape/objectの階層で比較し、位置だけの見た目差と意味を持つ関係差を分ける。
- [ ] **確認:** `cargo test -p document-diff-worker --test pptx_diff` PASS。C headで `mise run verify:fast` とhosted標準CIを一度確認する。
- [ ] **commit:** PPTX adapterとC最終記録をcommitする。

### DIF-14: native-text PDF形式比較

**Files:** Create worker `src/adapters/pdf.rs` と `tests/pdf_diff.rs`。Modify `adapters/mod.rs`、`shell.rs`。

**Interfaces:** `PdfComparator::compare(base, target, budget) -> WorkerDiffResponse`。既存pin `pdfium-render 0.9.4` / PDFium 7881 と `lopdf 0.45.0` を資格確認する。

- [ ] **RED:** page順、安定したtext/read order、image/visual region、link/form値、PDFのpaint順差を検出し、同意味のpackage noiseを内容差分にしない。scan/OCR、曖昧な読取順、region特定不可はpage/item未比較をassertする。
- [ ] **RED確認:** `cargo test -p document-diff-worker --test pdf_diff` がadapter未実装でFAIL。
- [ ] **GREEN:** pinned PDFiumの描画とlopdfの構造確認を用い、Office段落同一性を推測せず、page/region locatorの根拠を保つ。
- [ ] **確認:** `cargo test -p document-diff-worker --test pdf_diff` PASS。共有DSI sourceに変更がある場合だけ対象回帰を追加する。
- [ ] **commit:** PDF adapterをcommitする。

### DIF-15: 横断評価・資源qualification・最終gate

**Files:** Modify `experiments/document-diff/qualification.md`。Create `crates/document-application/tests/document_diff_vertical_slice.rs`、`crates/document-repository-postgres/tests/document_diff_concurrency.rs`、worker `tests/diff_acceptance.rs`。受入fixtureを各 `tests/fixtures/` に追加する。実装意味の変更はこのTaskへ先送りしない。

**Interfaces:** Task 01〜14の公開契約をそのまま使用。`DiffResult`→Projection→原本locatorの縦断と、既存Versioning/DSI/T10/DMB回帰を確認する。

- [ ] **契約fixture:** 全8形式と異形式、exact unchanged、単独/複合変更、add/remove/move/reorder、意味等価/別byte、見た目同じ/意味差、metadata-only、部分結果、巨大文書、WORKING更新、剥奪/T10、audit/cache/source破損をgold fixtureへ固定する。各expected verdict・coverage・変更facet・base/target locatorを記録する。
- [ ] **初回確認:** 新fixtureを焦点commandで実行し、未充足の契約だけがFAILした場合はREDとして記録する。初回からPASSしたものを強制的なREDと呼ばない。
- [ ] **GREEN:** 必須fixtureのfalse unchanged、未比較隠蔽、無権限開示、曖昧対応の確定移動、誤locatorをすべて0にする。precision/recall、alignment、false change、locator、candidate count、peak memory、wall timeを測定して記録する。候補resource profileの境界値と1超過を検証し、不適合なら計画改訂へ戻る。
- [ ] **確認:** `cargo test -p document-diff-worker --test diff_acceptance`、Application/Postgres縦断、`mise run verify:fast` をPASS。Dの同一headで標準CI、DSI Sandbox Preflight、DSI PoC regression、Diff受入を一度確認する。基準CIの外因性security失敗は成功に読み替えない。
- [ ] **commit:** Dの実装・評価記録をcommit/pushし、そのexact headのCI runをCapability Statusに記録する。記録commitでheadが変わった場合は新headの必要gateを確認してから完了を宣言する。PRはDraftのままレビュー待ちにする。
