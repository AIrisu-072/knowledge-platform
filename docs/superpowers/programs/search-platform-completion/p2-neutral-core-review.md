# P2-05 provider-neutral Core 独立レビュー

- 判定: **NO-GO — `NEUTRAL_CONTRACT_PASS` は未成立**。下記 F1/F2 の修正と独立再確認が必要。対象は純粋な Core 契約だけで、モデル・index engine の採択判定ではない。
- 対象: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット差分。`vector.rs` SHA-256 `dbf58e427cbe11013d9d547af5ee4af4178a3ebf418ce6e8312e80dc1a993014`、`lib.rs` `07e1e100569b316afb160056ea03f846e0238cdc6fc01b05c606416590ab2c15`、`vector_contract.rs` `7dc4deceff474d85e25c51f875cae949d9e365f829120b5499f62f898b231133`。設計 `1e28dd210009f133a0d8a92167cee6b78516008884814f688995f97fe7b691a5`、計画 `6cc75565eb9c27176fd02a451816d9b0c5f8748fbf1c227a3eafeb380f251c3b` は freeze 記載値と一致。P1 最小 KnowledgeUnit は別途 GO、P1 full body は別ゲート。
- 実行: `target/debug/deps/vector_contract-bf334899af724c8d --nocapture` は **7/7 PASS**。実行 binary SHA-256 `6c1c8d9531e159a737f68f7416d0e28a4db47b1abc5f94250fd70a8936ab5e11`、更新時刻 2026-09-30 19:52:38 +0900（対象 source より後）。これは cached binary の実行結果であり、このレビューで Cargo 再ビルド、CI、実 index 照合は行っていない。実装 receipt の RED `E0432` は記述を確認したが、RED binary/log は独立再実行していない。
- 実行基盤: この独立監査は native read-only reviewer fallback。`toolbox-context status --workspace "$PWD"` には P2-05 と同目的の未完了 run がなく、表示された D1 translation と旧 Vector port inspection は再開していない。managed worker の receipt と取り違えないこと。

## 阻止事項

### F1 — Unit の親・種別・provenance を変えても同じ manifest seal が通る

`validate_unit` は `parent_unit_id == unit_id` だけを拒否し、親が同一 Version/Part の前出 Unit かを確認しない（`crates/search-core/src/vector.rs:327-345`）。`hash_unit` は `parent_unit_id`、`UnitKind`、`ordinal`、`detected_format`、`archive_inner_format`、`parser_build_id` を直接符号化しない（同 `:635-669`）。`UnitId` に含まれない親・種別・一部 provenance を変えても `unit_bindings_digest` は同じままになる。`validate_against` はその digest を照合するだけなので（同 `:857-866`）、stage 後に入力 Unit の親を別 Part の UnitId に変えた場合も、同じ entries に対して READY receipt を返せる。P1 の `validate_part_units` は同一 Part・親の前出・kind/format を検証するが（`crates/search-core/src/knowledge_unit.rs:309-386`）、この Core validator は呼んでいない。

計画の `one_unit_two_parts_and_wrong_parent_rejected` と、設計の「全 Unit binding」「wrong parent」の双方向 seal に未達。現行 test は `VectorHitRef.version/part` の改変のみを試し、`KnowledgeUnit.parent_unit_id` の差替えを試さない（`crates/search-core/tests/vector_contract.rs:239-307`）。少なくとも seal に全 immutable Unit field を含め、同一 Part の親関係を検証するか、trusted P1 manifest の Unit と全 field を照合する入力境界を型で固定してから再試験すること。

### F2 — retention 非索引集合へ任意の索引可能 Unit を移せる

`validate_input` は `nonindexed_retention_unit_ids` の存在と重複だけを検証し、retention がその Unit の索引を禁止するかを検証しない（`crates/search-core/src/vector.rs:722-727`）。`validate_entries` はこの集合を欠落許容集合として差し引く（同 `:750-770`）。従って `PersistentResource`、明示永続許可あり、`Persistent` storage の Unit をこの集合へ入れて entries を空にしても、`stage` は `indexed_unit_count=0`、`readiness=Ready` の manifest を作れる。これは索引欠落を retention 禁止として分類する。

設計 `p2-vector-design.md:110` の「retention prohibition による欠落だけを別記録」に未達。現行 `partial_validated_units_only` test は集合の削除・indexed との重複を試すが、索引可能 Unit を retention 除外へ移す経路は試さない（`crates/search-core/tests/vector_contract.rs:389-418`）。非索引理由を trusted Source/P1 の per-Unit 判定に結び、当該 storage/許可/lease と一致しない除外を拒否して再試験すること。

## 確認できた契約と境界

- `EmbeddingModelSpec::validate_and_id` は固定順の長さ付き frame、domain と SHA-256 で名称、revision、weights/tokenizer、前処理、query/passage template、pooling/mask、token/chunk/truncation、dimension/metric/precision/normalization、runtime/build/native/config を含む（`vector.rs:20,78-117,166-280`）。全 field 差替えの 1 test は通過。ただし ID の独立 golden 値は test に固定されておらず、codec の将来 drift 検出は追加が望ましい。
- `BoundEmbedding` / `QueryEmbedding` は dimension、NaN/Inf、指定 UnitL2 norm を検証する（同 `:283-297,369-443`）。P1 cache key の model/Unit/text/profile/Source/scope/lease/lifetime と hit の generation/Version/Part/representation/raw/profile/text は既存 helper で照合する（同 `:404-429,745-767`、`knowledge_unit.rs:395-478`）。`RankedVectorHit` は照合済み Unit への参照と有限 score/1-based rank を検証するだけで、Source Read や本文 evidence を発行しない（`vector.rs:455-479`）。
- `VectorActivationPolicy::default()` は `Disabled`（`vector.rs:482-487`）。新しい model/runtime/index crate の Core 依存はない。no-gain 時もこの既定値を維持するという設計と整合。
- この pure validator の entries は**呼出元が渡した宣言集合**であり、実 index bytes の列挙・index receipt 真正性は確認できない（`vector.rs:833-839`）。現行 Source Read、retention lease の現在有効性、Live Version/T10、P1 body evidence、Unit→親 Version rank folding、S1 の順序、CAS/purge/restart、disk/backup 残留も P2-06/07 の trusted port/実 adapter 側のゲート。ここでの 7/7 PASS を production Vector READY や model/engine 適格と扱わない。

次の exact action: F1/F2 の focused RED を追加し、Core の照合・seal を修正、同じ対象 test と cache/multi-Part/retention 回帰を GREEN にした source SHA と実行 receipt を提出する。その後、別 reviewer が新しい exact inputs で再判定する。
