# P2 Vector architecture / actual L・LG baseline — independent review

- 判定日: 2026-09-30 JST。対象 branch/head: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff`、未 commit の共有作業ツリー。
- **設計アーキテクチャ: GO。** 凍結済み P1 Unit 入力、Source 正本、現行 Read、retention、世代 pin、S1 を変更する blocking P1/P2 矛盾は見つからない。
- **最初の L/LG 実経路の機能確認: GO（合成・限定）。** 保存ログは実 Tantivy と typed Graph、既存 planner/executor/federator を通り、qrels から主要指標を再計算できる。
- **dense 比較への投入: NO-GO。** 下記 F1–F3 を閉じてから、この L/LG を再現可能な比較基準として固定する。これは設計の採択、モデル・engine の選定、P1 本文実装の資格判定ではない。

## 判定対象と照合方法

| 入力 | SHA-256 / 状態 |
| --- | --- |
| `p2-vector-design.md` | `1e28dd210009f133a0d8a92167cee6b78516008884814f688995f97fe7b691a5`。指定 hash と一致。 |
| `p2-harness-first-contract.md` | `e8705697d4d10b2ad8a8f467cde1b380d6263792bf8d7dd7eb70ede67cc0150e`。 |
| P1 最小契約と追補 | `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe` / `0d44f5dfed72360bafc56f19c2bf72b1d8b9f278c7ea3247261e1af040a06a00`。freeze:5–9 の値と一致。 |
| `baseline-run.log` / isolated `Cargo.lock` | `208982f08c2f100c3e929d83c742119e468c50d330a617169142866a6a3d553a` / `8d5bdb829c0d5cd322c730d169087cad1f76cefb80edc83c0b7a18660705c81f`。`report.md`:3 の値と一致。 |

参照した契約は承認済み Search 設計 §§21,27,35,38,50–54,62、`p1-knowledgeunit-freeze.md`、`p1-extraction-freeze.md`、`p1-body-absence-amendment.md`、`p1-partial-positive-correction.md`。Source と実装は現在のファイルを独立に読んだ。`baseline-run.log` の 32 件の qrels と rank を別計算し、L/LG の `Recall@5=0.5/1.0`、`nDCG@10=0.666267/0.960762` が `report.md`:13–14 と一致した。ビルドや 10 テストの再実行は行っていない。`report.md`:45–56 の 10/10 と Clippy PASS は作成側の過去のローカル receipt として扱う。

## アーキテクチャ確認

- **Unit と authority:** 設計:12–16,74–114 は Version `ResourceId` / Part / `UnitId` / locator / raw / text / profile / generation を Vector hit から pinned P1 manifest と現行 Source-owned Read・Live Version・Part に照合する。類似度や cache hit を exact evidence・`BodyRequired`・absence に昇格しない。P1 追補:46 と exact-negative:68–78、Partial-positive:4–7 に整合する。実装時は trusted Source resolver を通す必要がある。
- **世代と cache:** 設計:74,106–112 は model weights/tokenizer/前処理/runtime の content-addressed ID、scope/lease/lifetime を含む cache key、双方向 manifest 列挙、staging → validate → current P1 との CAS、失敗時の旧 pointer 維持、再起動回復、full/incremental の論理同値を要求する。`UnitId` だけや旧 generation の hit binding を流用しない。これは設計条件であり、PoC では未検証。
- **retention:** 設計:108,114 は embedding を本文由来 artifact とし、明示的な永続許可のない metadata mode を永続化の根拠にしない。`CACHE_WITH_EXPIRY` の有限 lease、`SESSION_ONLY` の owner-gated RAM、`NO_RETENTION` の request 内 RAM、失効時の handle/cache/index 破棄を規定する。現 PoC の `validate_mock_vector_hit` は永続 artifact の拒否だけを試す契約 probe（`validate.rs`:177–230）で、ephemeral 実装の証拠ではない。
- **S1 と比較:** 設計:12–13,35–53,106,118 は hard eligibility を先に適用し、Lexical/Graph の順序を保つ `PriorityConcat`、親 Version dedup、異種 raw score を trace のみに限定する。RRF 等は別 arm として測定し、採択には別の契約変更を要求する。モデルは E5-small と MiniLM の二候補、exact scan を oracle として ANN family を後から比較する手順（設計:57–70,120–126）で、pgvector を自動的に production backend にしない。モデルカードの prefix・pooling・token limit と engine の存在・license 表示は各[公式 E5](https://huggingface.co/intfloat/multilingual-e5-small/blob/main/README.md)、[公式 MiniLM](https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2)、[hnsw-rs](https://github.com/jean-pierreBoth/hnswlib-rs)、[pgvector](https://github.com/pgvector/pgvector)、[Qdrant](https://github.com/qdrant/qdrant)で概ね照合した。pin 済み資産や transitive license の資格判定は未実施。

## 実 L/LG baseline と不足条件

- 実行経路: `run.rs`:323–355 が patched Tantivy 0.26.2 と `MemoryGraphRetriever` を同じ synthetic Source/generation から構築する。`run.rs`:538–669 は `SourceRouter`、`RetrieverPlanner`、`RetrievalExecutor`、実 retriever、`CandidateFederator::merge(PriorityConcat)` を順に実行する。Graph は `run.rs`:434–468 の typed n-ary path と authority/context/budget を通り、単なる事前 rank list ではない。`RetrievalExecutor`:179–245 は現行 Read と Graph path node の Read を通した後の rank だけ返す。`run.rs`:670–686 は公開直前に現行 Read/Version 相当の状態を再確認する。
- qrels と結果: `qrels.jsonl`:1–12 は正の3 query に各4つの eligible parent、うち2つが Graph-only。`baseline-run.log`:2–11 は 32 件で L rank が各2、LG rank が各4、重複 Graph hit は親1件に畳まれ、denied/unknown は authorized raw trace にないことを示す。256/1024 件も同じ30 challenge recordsを保持するため同じ score であり、規模による品質改善の証拠ではない（`report.md`:20）。`qnone` の比率を未定義として除外する実装は `metrics.rs`:54–64 にある。
- 測定: `report.md`:22–41 と `baseline-run.log`:12–37 は 32/256/1024、各 arm 4 query、monotonic `Instant`、macOS `ps` RSS、RAM index の file bytes 0 を明示する。LG の first-in-arm は L 後の共有 index 上であり独立 cold LG ではない。n=4 の p95/p99 は最大標本に過ぎず、SLO、engine 比較、RSS peak の証拠にはならない。更新は `run.rs`:359–401 の一件変更を含む**全世代再構築**1標本であり、incremental 同値や削除・再開の証拠ではない。
- `validate.rs`:107–175 と `tests/frozen_unit_codec.rs`:5–59 は九 field/二 golden vector、Text locator、raw/text digest、Source/Version/Part binding の synthetic 単行テキストを確認する。これは独立 codec 確認で、P1 parser/native format round trip、actual Unit manifest/lexical-doc seal、`Partial` の肯定と負の証明、production current Source の資格を立証しない。P1 composed freeze:3,14 も full body integration を未完了としている。

### 閉鎖が必要な指摘

1. **F1 [P1・dense 比較の再現性] 実行時の path dependency snapshot が固定されていない。** `report.md`:3 は実行 head が汚れた作業ツリーだったと明記する。isolated lock は外部 crate を固定するが、`Cargo.toml`:12–15 の `search-core`/application/Tantivy/Graph は可変の path dependency。現在の `search-core/src/knowledge_unit.rs` と `search-application/src/ports.rs` はログ生成後に更新されている。設計:31 が要求する port code digest も実行 receipt にない。**処置:** L/LG に必要な path crate、patched Tantivy と PoC source の exact tree/digest を run manifest に固定し、その snapshot で focused L/LG test と一回の baseline run を保存する。現在の保存数値を dense の paired comparison に流用しない。全 workspace CI の再実行はこの限定 gate に不要。
2. **F2 [P2・Unit→親 folding の被覆] baseline は常に一 Resource=一 Unit=一 Part。** `corpus.rs`:94–156 と `run.rs`:302–320 は各 Resource に単一 Text Unit と単一 lexical doc を作る。Graph/lexical の同一親 dedup（`security.rs`:9–40）は確認できるが、設計:12,20,33,49 が求める異なる Part の複数 Unit を同一 Version に畳む経路、Part/raw/locator 差替え、複数 Unit を独立した証拠票にしない条件は未検証。**処置:** dense arm を採点する前に少なくとも一つの Version に別 Part の関連 Unit を複数置き、Unit hit→親 Version の一回計上と Part/representation/raw mismatch の拒否を同じ harness のテスト・trace で確認する。P1 本文実接続は別 gate のまま残す。
3. **F3 [P2・無正例と access 評価] 無正例の誤検出数が指標から消える。** `metrics.rs`:61–74 は正例0なら ranked list の dedup 検証より先に `continue` し、`Score` は query ID だけを保持する。現 `qnone`（`queries.jsonl`:4）は単なる無関連 query で、設計:20,49 の「正解は unauthorized resource のみ」という access stratum がない。**処置:** 無正例 query の visible false-positive 件数と unauthorized disclosure 件数を分離し、重複 rank も常に拒否する。eligible positive が0の actor/snapshot で、検索語が denied/unknown Resource にだけ存在する fixture を追加し、raw/final trace と count を検証する。比率は引き続き未定義のままにする。

### dense 実装まで保留する確認（上記 GO に含めない）

- `run.rs`:629–650 の `raw_score: None` と `StageTrace` は現実行の順位・eligible miss を保存するが、設計:50 の retriever 内 score、gate 別の除外・pending 理由、candidate→authorization→fusion の条件付き recall までは記録しない。dense の attribution 用 trace と access-safe 集計を拡張する。既存 `LexicalRetrieverPort` は score を返さない（`ports.rs`:315–323）ため、L の数値 score を捏造しない。
- `validate_mock_vector_hit` には model ID、authoritative representation、raw size/MIME、Part logical path 等の入力自体がない（`validate.rs`:179–230）。これは明示された fake-unit contract probe に留め、設計:125 の実 `VectorHitRef`/manifest/Source resolver 境界で field ごとの mismatch、stale/Unknown、retention lease、`NO_RETENTION`/`SESSION_ONLY` 永続0件を試験する。
- 2モデルの pinned weights/tokenizer/runtime、Rust 数値 parity、実 embedding、exact/ANN、offline、disk/peak RSS、add/change/delete/restart、license/SBOM、P1 full-body と current Source の E2E は未実行。`report.md`:56 と `baseline-run.log`:38 の `dense_arms=UNRUN` を維持する。現空き disk は監査時 `df -h .` で約3.7 GiB。設計:70 の事前 peak budget と一候補ずつの実行を守る。

**次の exact action:** F1 の snapshot 固定と限定 L/LG 再実行、F2/F3 の fixture・metric contract 追加と focused test を済ませ、その凍結 baseline を model arm の比較入力にする。設計文書の採択と、実装・測定・production/hosted qualification は別 receipt とする。
