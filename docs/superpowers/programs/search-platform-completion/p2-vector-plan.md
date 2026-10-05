# P2 Vector / Embedding / Hybrid Retrieval v0 — 実装・実測計画

> 実行者: 各 Task は `orchestrate` → `toolbox-context` の独立した責務・書込範囲・receipt で進める。完了済み worker を別 Task に再利用しない。各実装 Task は意味のある RED → GREEN と独立した read-only review を通す。

- **Status:** 計画案。P2 設計の独立 architecture 判定は **GO**、既存 L/LG baseline を dense の比較基準にする判定は F1–F3 のため **NO-GO**。モデル、runtime、engine、production dependency、配備は未選定・未検証。
- **Goal:** 凍結 P1 `KnowledgeUnit` から、Source 正本に従う optional Vector 候補を安全に生成する provider-neutral 契約を完成させ、実モデル・実 runtime・実検索 engine を同一 baseline で測って採否を記録する。
- **Spec:** `p2-vector-design.md` SHA-256 `1e28dd210009f133a0d8a92167cee6b78516008884814f688995f97fe7b691a5`、`p2-vector-architecture-review.md`、`p2-harness-first-contract.md`、`p1-knowledgeunit-freeze.md`、`p1-extraction-freeze.md`、`p1-extraction-plan.md`。`spec/selection/library-tool-selection-v0.md` §13.1/13.3、承認済み Search 設計 §§21,27,38,50–54,62 に従う。
- **現行 seam:** `search-core/src/knowledge_unit.rs` に `EmbeddingCacheKey` / `VectorHitRef` / `VectorAuthorityInput` と binding 比較 helper がある。`search-application/src/ports.rs::VectorRetrieverPort` は `DiscoveryRequest` と generation しか受けず、`retrieval_execution.rs` の Vector arm は未接続。`retrieval.rs::RetrieverKind::Vector` と `federation.rs::PriorityConcat` は存在する。この最小型を破壊せず拡張する。P1 本文 manifest・exact resolver・実 Unit index は別の未完了 gate である。

## 共通契約と所有ファイル

| 所有する Task | 書込範囲 | 役割 |
| --- | --- | --- |
| 既存 baseline 修正担当 | `experiments/search-vector-poc/**` | F1–F3 を閉じた L/LG frozen baseline。P2 model worker は読取のみ。 |
| P2-M 実モデル担当 | `experiments/search-vector-model-poc/**` | 独立 `[workspace]`、asset pin、2モデル×CPU runtime、exact/ANN、paired report。root Cargo は変更しない。 |
| P2-C core 契約担当 | `crates/search-core/src/vector.rs`, `crates/search-core/src/lib.rs`, `crates/search-core/tests/vector_contract.rs` | model ID、manifest/receipt、bound vector と validation、activation の型。既存 `knowledge_unit.rs` は読取のみ。 |
| P2-A application 契約担当 | `crates/search-application/src/vector.rs`, `src/ports.rs`, `src/retrieval_execution.rs`, `src/retrieval.rs`, `src/discovery_service.rs`, `src/lib.rs`, `tests/vector_contract.rs`, `tests/vector_lifecycle_contract.rs` | trusted query、Source resolver、候補 folding、S1・failure、stage/pin/CAS/restart/retention の backend-neutral port。P1/P4 実装との同時 writer を置かない。 |
| 条件付き P2-E adapter 担当 | `crates/search-vector-adapter/**`, `crates/search-source-document/**` の P2 専用追加ファイルと `tests/vector_vertical.rs` | 採択された **1** 組の model/runtime/engine に限る。既存 Source/P1 ファイル変更は P1 owner と直列化。 |
| 親の単一 bootstrap 担当 | root `Cargo.toml` / `Cargo.lock`、dependency catalog、P7 bundle/receipt namespace | PoC qualification と採択判断後だけ production dependency / optional versioned receipt を追加。P2 worker はこれらを触らない。 |

共通 test fixture は凍結 P1 の九 field/長さ frame `ku1:` ID、`ResourceVersionRef`（Version `ResourceId`）、`ContentPartRef`、native locator、`RawBinding`、`ExtractionProfileId`、`text_sha256`、`ProjectionGenerationKey` を使う。Unit hit は親 Version に最初の公開 rank より前に一度だけ折り畳み、同じ Version の複数 Part/Unit を独立 Resource または証拠票にしない。S1 の routed order と hard eligibility を保ち、異種 raw score は加算・大小比較せず retriever 内 trace にだけ残す。Vector hit/cache/ANN miss は `BodyRequired`、exact text、Source absence、`Read` または authority の証拠にならない。

## Task P2-00 — frozen baseline gate（既存担当の receipt を消費）

**Files:** 読取 `experiments/search-vector-poc/{Cargo.toml,Cargo.lock,src/**,tests/**,report.md,baseline-run.log}` と P2 architecture review。ここでは書き換えない。

- [ ] **RED/修正の照合:** F1 は PoC source、path crate、patched Tantivy、port code の exact tree/hash を run manifest に記録し、その snapshot で focused L/LG test と一回の run を再実行。F2 は同じ Version の異なる Part に関連 Unit を複数置き、Unit→親一回計上と Part/representation/raw mismatch 拒否を test/trace で示す。F3 は正例 0 の visible false-positive と unauthorized disclosure を別々に計数し、denied/unknown にだけ正解がある actor fixture と全 query の重複 rank 拒否を示す。
- [ ] **GREEN gate:** reviewer が F1–F3 の exact artifacts/hash、qrels 固定、`cargo test --manifest-path experiments/search-vector-poc/Cargo.toml --offline --locked --tests`、新 L/LG receipt を確認する。旧 `report.md` の 4 query/arm の p99 は SLO とせず、新 baseline に置換する。L/LG の実 Tantivy・typed Graph・planner/executor/S1 経路と P1 本文未接続の境界を維持する。
- [ ] P2-M の protocol/asset 調査と P2-C の pure contract は先行可能。**paired dense 採点、holdout、採択判断はこの gate の GO 後**にのみ始める。

## Task P2-01 — 資産 pin と測定 protocol（P2-M、重み取得前）

**Files:** Create `experiments/search-vector-model-poc/{Cargo.toml,Cargo.lock,.gitignore,README.md,assets-manifest.json,run-manifest.json,src/{lib,assets,embed,exact,ann,arms,measure}.rs,tests/{asset_contract,embedding_parity,ranking,security,engine}.rs,report.md}`。`search-vector-poc` を path dependency として参照し、baseline code を複製しない。asset bytes と disposable `target/` は commit しない。

**Interfaces:** `verify_assets(manifest, local_root) -> Result<VerifiedAssets, AssetError>`、`freeze_run(baseline_receipt, assets, policy, budgets) -> Result<RunPin, RunError>`。`RunPin` は corpus/qrel/label split、Source generation、全 path crate digest、model/runtime/engine code digest、host、asset SHA-256、tokenizer/pooling/truncation/precision/normalization、S1 順序、window、seed、期限、disk/RSS ceiling を持つ。

- [ ] RED `asset_contract::{reject_main_alias,changed_tokenizer_or_weight_changes_id,missing_hash_or_license_fails,no_network_after_pin}`。固定 revision と file list を取得前に記入し、`main` を identity に使わない。未許可の public corpus bytes は repository へ入れず、MIRACL 日本語小 slice の revision/選択 ID/hash/qrel/license/attribution だけを固定する。
- [ ] GREEN は manifest の schema/checksum 検証、qrels を rank 閲覧前に凍結し、query family と親で development/untouched holdout を分割。closed judgment pool の未判定率を保存し、unjudged を grade 0 にしない。合成 corpus は finance 固有語、paraphrase、和英混在、表/slide/Archive、長文、Graph-only、同一親複数 Part、denied/Unknown/no-positive を保持する。
- [ ] 各取得/build 前に `df -h .` と実 RSS/空きメモリを再計測し、**remote 実 file sizes + 一時変換 + Rust build + model resident + index/cache** の peak と OS/並行作業 reserve を run manifest に先に記す。2026-09-30 の観測空き約 2.0 GiB は実行時保証ではない。容量/期限超過は `ENVIRONMENT_UNAVAILABLE` として中止し、品質不合格と混ぜない。一候補ずつ必要 file のみ取得し、PyTorch bin と safetensors/ONNX を無条件で重複取得しない。

## Task P2-02 — 二つの実モデルと CPU runtime の数値一致（P2-M）

**Files:** `experiments/search-vector-model-poc/src/{assets,embed}.rs`, `tests/embedding_parity.rs`, asset/run manifest、report。root dependency は変更しない。

**Candidate contract:** E5-small は `query: ` / `passage: `、attention-mask mean pooling、L2 正規化、512-token 上限。MiniLM-L12-v2 は prefix なし、model の `1_Pooling/config.json` に従う mean pooling、`max_seq_length=128`。両者は 384 次元の独立学習済みモデル。重み精度・tokenizer/special token/Unicode/truncation/出力 normalization はモデル別に pin する。[E5 card](https://huggingface.co/intfloat/multilingual-e5-small/blob/main/README.md)、[MiniLM card](https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2)。

- [ ] RED `embedding_parity::{query_passage_prefix,mask_and_padding,token_limit_boundary,japanese_mixed_token_ids,dimension_nonfinite_zero_norm,reference_vector_and_rank}`。参照実装の固定短文 query/Unit vector を **test-only 数値 oracle** とし、cosine 誤差許容と rank 逆転許容を事前に記す。reference 成功を Rust 実装成功にしない。
- [ ] CPU 候補 1 は pinned Candle/safetensors。公式 config は E5・MiniLM とも `architectures=BertModel` / `model_type=bert` で、E5 tokenizer は XLM-R 系である。Candle に `XLMRobertaModel` は存在するが、重み名/shape を検証せず選ばない。まず `bert::BertModel` と tokenizer 入力・mask/position/token-type の互換性を実測し、合わなければ `RUNTIME_NO_GO` とする。[E5 config](https://huggingface.co/intfloat/multilingual-e5-small/blob/main/config.json)、[MiniLM config](https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2/blob/main/config.json)、[Candle Bert API](https://docs.rs/candle-transformers/latest/candle_transformers/models/bert/struct.BertModel.html)、[Candle XLM-R API](https://docs.rs/candle-transformers/latest/candle_transformers/models/xlm_roberta/struct.XLMRobertaModel.html)。
- [ ] CPU 候補 2 は pinned `ort` + ONNX Runtime default CPU、model repo の ONNX artifact を一つずつ検査する。E5 の `onnx/model.onnx` は約 470 MB、MiniLM は同約 470 MB；MiniLM の arm64 quantized は約 118 MB だが **別 model variant/precision ID** として別 arm に置き、float baseline と黙って同一視しない。ONNX graph が pooling を含むか実 output shape で確認する。[E5 assets](https://huggingface.co/intfloat/multilingual-e5-small/tree/main/onnx)、[MiniLM assets](https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2/tree/main/onnx)、[ort CPU API](https://docs.rs/ort/latest/ort/ep/index.html)。CoreML は別測定 arm であり CPU receipt に混ぜない。
- [ ] GREEN は **二つの実モデルの Rust CPU embeddings** と、少なくとも重なる一モデルで Candle/ort の実出力・rank・load/query RSS/時間を同じ fixture で比較し、可能な範囲の差を説明する。両 runtime を同時 resident にしない。pin 後に outbound network を遮断して load/query/rebuild を再実行。失敗した runtime/precision はそのまま記録し、toy hash・固定 vector・単なる HNSW specification で代用しない。

## Task P2-03 — real exact D/LD/LDG と paired 測定（P2-M）

**Files:** `experiments/search-vector-model-poc/src/{exact,arms,measure}.rs`, `tests/{ranking,security}.rs`, `report.md`。P2-00 の frozen baseline を読取。

**Interfaces:** PoC 内の `embed_units(&RunPin, &[KnowledgeUnit]) -> Result<Vec<PoCBoundEmbedding>, ModelError>`、`exact_search(&RunPin, query, scope, window) -> Result<Vec<PoCRankedVectorHit>, EngineError>`、`run_arm(Arm::{L,D,LD,LG,LDG}, pin, actor, window)`。モデル別 exact dot/cosine は同じ正規化・filter・親 folding を使う。`PoCRankedVectorHit` は P1 `VectorHitRef` と model ID / 1-based rank / retriever 内 score を組にし、`(score desc, UnitId bytes)` で tie break。P2-C の production 型完成は PoC の開始条件ではない。

- [ ] RED `ranking::{multi_part_one_parent,graph_only_lg_rescue,hard_mismatch_excluded,stable_tie,model_generation_profile_raw_mismatch,no_positive_counts_and_no_disclosure,body_required_vector_only_not_evidence}`。L/LG は実 adapter のまま。D/LD/LDG も実 embedding と similarity を実行し、fake rank list は contract test に限定する。
- [ ] GREEN は同一 corpus/qrels/Source pin/window/S1 order で L・D・LD・LG・LDG を再実行。親 Version `Recall@1/5/10/20`（分母は eligible positive 全件）、`MRR@10`、gain `2^grade-1` の `nDCG@10`、Unit/論理 Resource recall、Graph-only rescue と false-composite を出す。正例 0 の比率は未定義、visible false-positive と unauthorized disclosure は別 count。小 fixture は case 別、十分な件数では query bootstrap interval を付す。
- [ ] Trace は route → retriever raw rank/score → current access/Version/Part → applicability/temporal → parent dedup → S1 → 公開直前再確認を ID-free denied/unknown 理由で残し、段階別 conditional recall と欠損原因を出す。既存 Lexical port の score が無ければ `None` のままにする。32/256/1024 の bounded scale は品質 corpus と容量用 distractor を分ける。cold/warm、embed/search/auth/fusion、build/update、実 process RSS/peak、index/cache/disk、n と host contention を記録し、独立 timed request 100 未満の p99 を安定値と呼ばない。
- [ ] Holdout 前に優先 query 層、意味ある quality 差、許容 p95/p99/RSS/disk/build/update、L と LG の現在 baseline を decision sheet に固定する。LD/LDG が L と relevant な LG の両方に反復可能な改善を示さない、重要層で退化する、または不確実性が広い場合は **`Disabled` を既定**にする。S1 は維持し、RRF/score calibration は development で固定した別 arm を holdout 一度だけ測り、採用する場合は別の契約変更にする。

## Task P2-04 — exact 対実 ANN engine（P2-M）

**Files:** `experiments/search-vector-model-poc/src/ann.rs`, `tests/engine.rs`, manifest/report。P2-03 と同じ immutable vector と可視 filter を使う。

- [ ] RED `engine::{same_vectors_and_filters,ann_recall_vs_exact,filtered_underfill,add_change_delete,purge_lease,full_incremental_equivalence,reload_or_restart,stale_generation_and_model_rejected}`。ANN `Recall@K` の oracle は同 vector・同 filter の exact scan であり、qrel recall とは別。
- [ ] まず in-process exact を測り、**実 ANN 一系統**を同じ 32/256/1024 および無理のない拡張規模で測る。第一候補 `hnsw_rs` は search-time filter と dump/reload を実 API で検証する。[hnsw-rs](https://github.com/jean-pierreBoth/hnswlib-rs)。PostgreSQL extension が利用可能なら [pgvector](https://github.com/pgvector/pgvector) exact/HNSW/IVFFlat の build・filter 後 underfill・iterative scan・delete/rebuild と運用費を比較する。Qdrant は規模/運用の実利益が必要な場合だけ別 service arm とする。既存 Postgres image に pgvector があると推定せず、比較のためだけに新 image を pull しない。
- [ ] GREEN は **exact と実 ANN** の Recall/latency/RSS/peak/build/add/change/delete/lease purge/restart/disk を同一 pin で報告。ANN が改善しないなら exact 選択が可能で、ANN を production 必須にしない。engine の未実測/環境不能は `ENGINE_PASS` にしない。

## Task P2-05 — provider-neutral core（P2-C、P1 最小 core 受入後・モデル採否から独立）

**Files:** `crates/search-core/src/vector.rs`, `src/lib.rs`, `tests/vector_contract.rs`。既存 P1 core の `KnowledgeUnit`、`EmbeddingCacheKey`、`VectorHitRef`、`VectorAuthorityInput` を再利用する。新しい runtime/engine crate は依存させない。

**Interfaces:** `EmbeddingModelSpec::validate_and_id() -> Result<EmbeddingModelId, VectorContractError>`、`BoundEmbedding::new(spec, unit, pinned, values) -> Result<Self,...>`、`QueryEmbedding::new(spec, values) -> Result<Self,...>`、`RankedVectorHit::validate(spec, pinned_unit) -> Result<...>`、`VectorProjectionManifest::validate_against(&VectorManifestInput, &[VectorEntryRef]) -> Result<VectorStageReceipt,...>`、`VectorActivationPolicy::{Disabled,EligibleOptIn}`。`VectorManifestInput` は P1 bundle key/receipt digest、Source snapshot、検証済み `KnowledgeUnit` と対応する `VectorAuthorityInput`、retention により非索引となった Unit ID を運ぶ **core DTO** で、trusted `PinnedBodyBundle` の代替権限ではない。`VectorEntryRef` は P1 `VectorHitRef`、model ID、vector digest、`EmbeddingCacheKey` を持ち、locator は照合対象の `KnowledgeUnit` から検証する。model ID は固定順長さ frame の SHA-256（`sha256:`）に exact revision/weights/tokenizer/config、query/passage template、pooling/mask、truncation、dimension/metric、precision/normalization、runtime build/native binary を含む。値変更は必ず別 ID。

- [ ] RED `vector_contract::{model_id_changes_for_every_semantic_input,wrong_dimension_nan_inf_zero_norm_rejected,one_unit_two_parts_and_wrong_parent_rejected,manifest_missing_extra_duplicate_or_stale_entry_rejected,cache_key_scope_lease_lifetime_isolation,partial_validated_units_only,disabled_default}`。`manifest` は P1 body bundle key/receipt、Source snapshot/owner/lease、model、schema/metric/precision、engine/build/params、profile 集合、lexical/Graph 比較 schema、全 Unit binding/count/digest、index receipt、readiness を保持する。lexical analyzer は比較記録であり embedding/cache-key 入力ではない。
- [ ] GREEN は manifest と実 index entry 集合を **双方向**照合する純粋 validator、versioned receipt digest、nonindexed retention 禁止集合の別記録。`cargo test -p search-core --locked --test vector_contract` と `cargo fmt --all -- --check` を対象 head で PASS。P1 `matches_pinned_unit` は stored field 一致だけで、現行 Read/retention 許可を発行しない。

## Task P2-06 — trusted application・lifecycle 契約（P2-A、P1 full body API 受入後）

**Files:** `crates/search-application/src/{vector,ports,retrieval_execution,retrieval,discovery_service,lib}.rs`, `tests/{vector_contract,vector_lifecycle_contract}.rs`。P1 `PinnedBodyBundle`/`BodyUnitManifest`/`KnowledgeUnitHitRef`/Source-owned exact resolver、P4 `AuthorizedSourceScope` と final disclosure gate の**実際に受入済みの型**を読み、名称の差のみ調整する。別の trust issuer や Discovery loop は作らない。

**Interfaces:** `TrustedVectorQuery::compile(scope, request, model, window) -> Result<Self, SearchError>` は private constructor、bounded text/token/window、actor/Source/model を caller が差替不可。`EmbeddingProvider::{spec,embed_units,embed_query}`、`VectorIndexPort::{stage_full,stage_incremental,validate_stage,search,discard,purge_scope}`、`VectorGenerationPort::{publish_if_current,pin_current,recover}`、`VectorSourceResolverPort::resolve_hit(pin, hit, scope) -> BoxFuture<'_, VectorResolution>`、`VectorRetrieverPort::retrieve(pin, trusted_query, window) -> BoxFuture<'_, VectorRetrievalBatch>` を SQLx/vendor-free port とする。`VectorResolution::{Visible(SourceValidatedVectorCandidate),Suppressed,Unavailable}` は private candidate constructor を持ち、Denied と Unknown の公開情報を伏せつつ、Unknown を blocking gap にする。`VectorRetrievalBatch` は Source resolver 後の親 candidate と内部 `VectorHitRef`/rank/score trace を分離し、未検証 hit を直接 serialize しない。

- [ ] RED `vector_contract::{forged_query_or_model_rejected,read_unknown_denied_id_free,stale_version_t10_part_representation_raw_profile_text_model_or_generation_rejected,multi_unit_parent_once,body_required_and_absence_not_minted,lexical_graph_order_unchanged,unavailable_does_not_assert_absence}`。候補作成**前**、fusion 前、公開直前に trusted Source から現在 Live Version/T10、Part/representation/raw、Read、retention と同一 P1 manifest/receipt を照合。`Partial` は検証済み Unit の candidate のみ許し、complete body とは呼ばない。History は明示 Version/`ReadHistory` route 外で混入させない。
- [ ] RED `vector_lifecycle_contract::{stage_failure_and_cas_loss_preserve_old_pointer,queries_pin_same_p1_vector_key,restart_orphan_unready,full_incremental_add_change_delete_equivalent,model_profile_change_reembeds,reused_bytes_rebound_after_permission,revocation_expiry_scope_change_and_cancel_purge}`。fake index/clock/Source は境界強制用で品質証拠ではない。staging → 全 entry/digest/permission 検証 → P1 current key/model/profile/lease の CAS → immutable pin とし、敗北時は未公開 Vector だけ破棄。restart は validated ready receipt だけを復元する。
- [ ] GREEN は既存 `RetrieverRankList`/`CandidateFederator::merge(PriorityConcat)` に hard-gated 親候補を返し、raw score は retriever 内 trace のみ。Vector unavailable は bounded `Unknown`/非開示 gap、既存 Lexical/Graph が十分ならその route を継続。`cargo test -p search-application --locked --test vector_contract --test vector_lifecycle_contract` と既存 `discovery_loop`/P1 body focused suite を PASS。default config は `Disabled`、採択時のみ明示 opt-in。
- [ ] Retention: embedding は本文由来。`PersistentResource` でも Source の明示 embedding 永続許可が必要、`PersistentDiscoveryMetadata` は許可ではない。`CacheWithExpiry` は最短 Source/actor/session lease 以内、P4 remote は owner-gated RAM のみ。`SessionOnly` は session RAM、`NoRetention` は evaluation RAM のみ。後二者は永続 index/cache/backup/queue/spill 0 件を test し、owner/lease 未実装 Source では Vector を起動しない。cache key は P1 の model/Unit/text/profile/Source/scope/lease/lifetime を全照合し、cross-generation bytes reuse は新権限を確認して再 bind する。

## Task P2-07 — 採択時だけ実 adapter と P7 receipt を接続（P2-E、単一 bootstrap）

**Gate:** P2-00–04 の実測、P2-05–06 の contract、独立 architecture/security review、model/runtime/engine の license・SBOM・advisory・macOS arm64/Linux packaging/offline receipt が揃う。採択しない場合はこの Task を `SKIPPED_DISABLED` と記録し、P2 neutral contract と default `Disabled` を完成とする。

**Files:** `crates/search-vector-adapter/**`, `crates/search-source-document/tests/vector_vertical.rs` と P2 専用 adapter file。root Cargo/lock/selection catalog/P7 DB migration は親の唯一の writer が資格済み版と exact artifact digest を記録して追加する。

- [ ] RED real storage/Source vertical: 実 P1 multi-Part Unit bundle → model embed → exact/選択 engine stage → READY receipt → CAS → pin → ANN/exact candidate → Source resolver/current Read → S1 → final disclosure。add/change/delete、stale Version/T10、Read 取消、lease expiry、CAS 競合、crash/restart、orphan cleanup、full/incremental 同値、model/profile 交換、remote `SESSION_ONLY`/`NO_RETENTION` の永続0件を含める。
- [ ] GREEN は選定済み adapter のみを production dependency に昇格させ、model weights/tokenizer/runtime native binary の checksum と SBOM/license/notice を packaging に固定。P7 の既存 `(SourceId,generation_id)` と P1 composite READY を共有し、**optional Vector receipt の有無を明示する新 bundle version** を親/P7 owner が定義する。`vector_receipt=None` の既存 route を壊さず、`Some` の時だけ同 key/Source snapshot/lease/model/index digest を READY と CAS で照合する。P2 の neutral receipt は P7 最終 capability fan-in を待たず定義でき、P7 最終 receipt はこの実装結果を後で消費する。
- [ ] Focused 実 DB/Linux sandbox canary、対象 strict Clippy/fmt、`mise run verify:fast`、必要な hosted exact-head gate は**最終統合 head で一回**確認する。CI/local/PoC/production/live の状態を分け、採択判断・merge・deploy は既存 program の権限境界で扱う。

## 最終判定と reviewer focus

Decision record は `SOURCE_INSPECTED` → `BASELINE_FROZEN` → `HARNESS_PASS` → `MODEL_QUALITY_MEASURED`（実二モデル）→ `RUST_PARITY_PASS`（実 CPU）→ `ENGINE_PASS`（exact と実 ANN）→ `NEUTRAL_CONTRACT_PASS` → 条件付き `PRODUCTION_INTEGRATED` / `HOSTED_PASS` / `SELECTED`、または `DISABLED` を区別する。途中失敗・disk/time 不足は理由と最後に完了した size を残し、次段階へ昇格しない。No gain でも neutral types/ports/lifecycle/failure contract は完了し、Vector は default disabled の optional flag に留まる。

独立 reviewer は特に (1) no-positive/denied の raw/final trace 漏えい、(2) 同一 Version 複数 Part と stale raw の親 binding、(3) index/manifest の部分一致による偽 READY、(4) `SESSION_ONLY`/`NO_RETENTION` の disk/backup/telemetry 残留、(5) model/tokenizer/precision/retention 変更後の cache reuse を再試験する。各 Task receipt は branch/HEAD、入力 SHA、実行 command/result、未測定項目と次の exact action を記載する。
