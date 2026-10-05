# P2-05 neutral Core F1/F2 correction receipt

- Scope: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット差分。2026-09-30 21:20 JST。独立レビュー `p2-neutral-core-review.md` の F1/F2 に限る。
- F1: `vector.rs::hash_unit` は `KnowledgeUnit` の parent、ordinal、kind、text、outer/inner format、parser build を含む全 immutable field を固定順 frame に封印する。`validate_input` は parent ID が入力 Unit 集合に存在し、同じ Version/Part の小さい ordinal を指すことを検証する。`validate_unit` は P1 `compatible_kind` を再利用し、非 Archive と Archive leaf の format/locator/kind、parser build の局所整合を拒否側で検証する。`knowledge_unit.rs::compatible_kind` の変更は `pub(crate)` 可視化 1 箇所のみ。
- F2: `validate_input` は `storage × authority.retention_mode × Source の明示 embedding 永続許可` から禁止 Unit 集合を再計算し、呼出元の `nonindexed_retention_unit_ids` と双方向一致させる。`Persistent` は `PersistentResource` かつ明示許可の Unit だけ索引可能。`Volatile` は `PersistentDiscoveryMetadata` の metadata 保持を本文 embedding grant に拡大せず、`PersistentResource` の永続許可 bool を揮発利用の追加条件にしない。`CacheWithExpiry` の有限期限、`SessionOnly`/`NoRetention` の lifetime scope と manifest 作成時の期限確認は既存検証を維持する。禁止 Unit は manifest の別集合に入り、index entry と両立しない。

## RED / GREEN

- 旧 Core、追加 test の `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_TARGET_DIR="$PWD/target" cargo test -p search-core --locked --test vector_contract -- --nocapture`: **7 passed / 4 failed**。失敗は valid parent の変更でも同じ seal、foreign parent stage 受理、許可済み `PersistentResource` を任意に非索引化、禁止 mode を含む正しい persistent manifest の拒否。いずれも期待した F1/F2 の挙動差だった。最初の compile は test 内の関数名 shadow を直してからこの RED を実行した。
- 修正後 `cargo test -p search-core --locked --test vector_contract --test knowledge_unit_contract`: **11/11 + 6/6 PASS**。親/Part、valid Docx kind と XLSX/XLSM format の seal 差分、Archive leaf 整合、retention の揮発/永続 matrix、既存 cache/multi-Part/partial 回帰を含む。上記の Cargo 環境変数を使用。
- `cargo clippy -p search-core --locked --all-targets -- -D warnings`: **PASS**。上記の Cargo 環境変数を使用。`rustfmt --edition 2024 --check` は `vector.rs`、`knowledge_unit.rs`、`vector_contract.rs` の 3 ファイルで **PASS**。

## Exact inputs and boundary

| File / executable | SHA-256 |
| --- | --- |
| `crates/search-core/src/vector.rs` | `254c1c7dc2834cf2ff84037d7871e98c5dc6685a1621ca81ba35170b97aa8fdc` |
| `crates/search-core/src/knowledge_unit.rs` | `626310e6626deb912eefba1f2b9b487583c31e01c414e55acc68dfc8ede6f638` |
| `crates/search-core/tests/vector_contract.rs` | `12abff1af5fcba1040222fcb48532d047e28e8e26f5b26702406fe29f26606be` |
| `target/debug/deps/vector_contract-c0d82766d40eb305` | `e35de1120fe2b2fb7cc41a69cbd9b7ab2907a6d22b41462304cb87dd113160c7` |

この pure validator は caller が渡す P1 validated Unit と authority DTO の内部整合だけを証明する。Archive member chain と reader plan の真正性は Vector DTO に plan がないため P1 `validate_part_units` / trusted manifest の gate に残る。Source の現行 Read、body embedding の借用許可、lease/revocation の現在有効性、P1 receipt 真正性、実 index bytes、CAS/公開/復旧は P2-06/07 の trusted port と adapter の gate。今回の PASS は production Vector READY または model/engine 適格の証明ではない。

次の exact action: 独立 read-only reviewer が上記 SHA の F1/F2 と focused suite を再確認し、`NEUTRAL_CONTRACT_PASS` を再判定する。
