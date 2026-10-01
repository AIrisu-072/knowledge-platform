# P2-05 provider-neutral Core F1/F2 独立再確認

- 判定: **GO — `NEUTRAL_CONTRACT_PASS`（pure Core 契約に限る）**。先行 `p2-neutral-core-review.md` の F1/F2 は、下記の固定入力で解消を確認した。これは P2 全体、P1 本文、実 index、model/runtime/engine、application/lifecycle、production Vector READY の判定ではない。
- 対象: `feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff` の未コミット差分。2026-09-30 JST。独立 read-only reviewer による native fallback。`toolbox-context status --workspace "$PWD"` に P2-05 同目的の未完了 managed run はなく、D1 translation と旧 Vector port inspection の run は再開していない。
- 判断基準: frozen P2 design SHA-256 `1e28dd210009f133a0d8a92167cee6b78516008884814f688995f97fe7b691a5`、P2 plan `6cc75565eb9c27176fd02a451816d9b0c5f8748fbf1c227a3eafeb380f251c3b`、P1 Unit contract `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe` と優先 amendment `0d44f5dfed72360bafc56f19c2bf72b1d8b9f278c7ea3247261e1af040a06a00`。

| 固定入力 | SHA-256 |
| --- | --- |
| `crates/search-core/src/vector.rs` | `254c1c7dc2834cf2ff84037d7871e98c5dc6685a1621ca81ba35170b97aa8fdc` |
| `crates/search-core/src/knowledge_unit.rs` | `626310e6626deb912eefba1f2b9b487583c31e01c414e55acc68dfc8ede6f638` |
| `crates/search-core/tests/vector_contract.rs` | `12abff1af5fcba1040222fcb48532d047e28e8e26f5b26702406fe29f26606be` |
| `crates/search-core/tests/knowledge_unit_contract.rs` | `466c0f897f9866a34cea438fb562f31722291098cf40a80003c48b4cf1ee8fd4` |
| `target/debug/deps/vector_contract-c0d82766d40eb305` | `e35de1120fe2b2fb7cc41a69cbd9b7ab2907a6d22b41462304cb87dd113160c7` |
| `target/debug/deps/knowledge_unit_contract-79101362c0e92ca7` | `34b4843557bdea9c0c2426098ca2fbe9a2cbaa63f4f2bfc06a9ec909df8cda62` |

## F1 — Unit seal と親関係

`vector.rs:674-722` の `hash_unit` は `KnowledgeUnit` の UnitId、Version、Part、親、ordinal、kind、text、locator、text digest と provenance の全 field、および authority/generation/coverage を固定順 frame に入れる。`validate_input` は親 ID の存在と同じ Version/Part の小さい ordinal を確認する（`:786-798`）。`validate_unit` は P1 の `compatible_kind`（`knowledge_unit.rs:238`、`pub(crate)`）を共有して通常 format と Archive leaf の format/locator/kind を照合し、非 Archive の inner format、Archive の leaf 不在・Zip leaf、空または非 ASCII graphic の parser build ID を拒否する（`:327-395`）。

既存 seal に対する有効な親・parser build の変更は digest が変わり、別 Part・未来・不存在の親と誤った kind/format は stage で拒否される（`vector_contract.rs:440-694`）。Archive の実 reader plan、member chain と native pin の真正性は、この DTO が plan を持たないため P1 `validate_part_units` / trusted manifest の確認事項である。

## F2 — retention 非索引集合

`vector.rs:772-806` は storage、各 Unit の retention mode、Source の明示的な embedding 永続許可から禁止集合を再計算し、申告された `nonindexed_retention_unit_ids` と**双方向一致**を要求する。`Persistent` で索引可能なのは `PersistentResource` かつ明示許可ありの場合だけ。`Volatile` では `PersistentDiscoveryMetadata` を本文 embedding 許可と扱わず禁止し、`PersistentResource` の永続許可 bool が false でも揮発索引を禁止しない。`CacheWithExpiry` は有限期限、`SessionOnly` / `NoRetention` は非空の lifetime scope、manifest 作成時の期限内という局所条件も維持する（`:385-394,730-740`）。

`validate_entries` は禁止 Unit の entry、余分・重複・欠落 entry を拒否する（`:825-861`）。索引可能な Unit を任意に非索引集合へ移す旧 F2 経路は `vector_contract.rs:696-817` の禁止集合・mixed retention 回帰で拒否される。禁止 Unit の集合は manifest と receipt に別記録される（`vector.rs:881-960`）。Source の現在の本文利用許可、owner/lease 失効確認、実 disk/backup/queue 残留は trusted port 側の判定である。

## Fresh verification と残る境界

- `target/debug/deps/vector_contract-c0d82766d40eb305 --nocapture`: **11 passed / 0 failed**。binary 更新時刻 2026-09-30 21:19:21 +0900 は上記 source/test 更新後。F1/F2 追加回帰を含む。
- `target/debug/deps/knowledge_unit_contract-79101362c0e92ca7 --nocapture`: **6 passed / 0 failed**。binary 更新時刻 2026-09-30 21:17:24 +0900 は `knowledge_unit.rs` 更新後。
- 対象 3 source/test の SHA は検査開始と終了時に一致した。P4 が共用 build slot を使用するため、この再確認では Cargo build / Clippy / CI を実行していない。修正 receipt の strict Clippy PASS は独立再実行結果として扱わない。cached binary 実行は、実 index bytes や本番接続を検証しない。
- `VectorManifestInput` と `VectorEntryRef` は呼出元の宣言集合。P1 bundle/receipt 真正性、Source の現在 Read・Version/Part/raw・retention、lease 取消、実 index enumeration と receipt、CAS/purge/restart は P2-06/07 の trusted application/adapter gate に残る。`VectorActivationPolicy::default()` は `Disabled`（`vector.rs:521-526`）。

次の exact action: 親がこの pure Core 判定を program status に取り込み、P2-06 の trusted application/lifecycle を別 gate で実装・レビューする。model/runtime/engine の採択は実測後に判定する。
