# P1 KnowledgeUnit core — 独立コードレビュー

- 判定: **GO（最小 provider-neutral Unit core に限る）**。P1→P2 の codec、provenance、locator の局所検証を止める具体的な欠陥は確認しなかった。
- 対象: `feat/search-platform-completion-core`、base HEAD `80a47960d025e4dfdea1eacade28b15d218725ff` 上の未コミット `crates/search-core` 実装。レビュー時の `knowledge_unit.rs` SHA-256 は `e1154b5082ed74cc92df26c452396004e66faba7ab7474103a2edb0ef74588d2`、`locator.rs` は `8d6759b6c12a5e6c73cf7727b4c333e3c9b29f4deaeb800c36c15fb802b8b3c3`、`profile.rs` は `369c48bfa1c6cd73ac52ed2e10f18b08dfb1fff0f63613522c7fef1c36cd6aef`、`identity.rs` は `6bda40833978d9578a79aed070d82e50d05a08da22757c0585a0a9afa47898fc`。テストの SHA-256 は `466c0f897f9866a34cea438fb562f31722291098cf40a80003c48b4cf1ee8fd4`。
- 規範: `p1-knowledgeunit-contract.md` SHA-256 `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe` と優先追補 `p1-knowledgeunit-amendment.md` SHA-256 `0d44f5dfed72360bafc56f19c2bf72b1d8b9f278c7ea3247261e1af040a06a00`。`p1-unit-archive-binding-ruling.md` の同一 Part 内 Text/CSV 混在許可も適用した。

## 照合結果

1. `identity.rs:15-87` は UUID 16 bytes、native Version/Part、path、part ordinal、profile、locator、Unit ordinal の9要素を規定順に個別 frame し、`ku1:` の小文字 hex のみを parse/serde で受理する。`knowledge_unit.rs:119-153` は CRLF→LF、CR→LF、NFC の順で text を正規化し、UTF-8 byte span を検証する。`profile.rs:16-69` の `sha256:` string codec も canonical な入力に限定される。二つの UnitId、locator hex、text digest は契約 fixture と独立 Python 計算で一致した。
2. `locator.rs:58-94,96-339` は8 tag、Docx physical step grammar、PPTX/HTML の必須 path、PDF/Text の非空半開区間、NFC 相対 path、truncation/unknown/trailing bytes を検証する。`Reader::frame` (`knowledge_unit.rs:505-532`) は宣言長が残り bytes を超える場合に allocation 前に拒否する。Archive decode は内側 tag 8 を再帰 decode 前に拒否する (`locator.rs:310-329`)。
3. `profile.rs:80-145,204-383` は15 budget key の全件昇順、format 対応 settings、PDF native pin、revision 1、field frame を検証する。`profile.rs:399-525` は Archive node の成分列順、唯一の Zip root、Zip proper prefix、使用済み leaf 全件を強制し、outer と各 leaf の definition を composite profile に含める。Text charset、CSV dialect、nested decoder、parser build ID、PDF pin の変更で ID が変わるテストを確認した。
4. `knowledge_unit.rs:274-392` は trusted binding と各 Unit の Version/Part、snapshot、representation、raw、outer format、profile、outer parser build、連続 ordinal、locator 一意性、text digest、再導出 ID、前出 parent を照合する。Archive の `binding.archive_inner_format=None` は Part 全体の leaf format 非固定として扱い、各 Unit の `Some(leaf)` を plan の同一 member chain・inner locator・UnitKind に照合する (`knowledge_unit.rs:319-360`)。Text/CSV 混在の正例と leaf 偽装の負例は `knowledge_unit_contract.rs:797-861` にある。
5. `EmbeddingCacheKey`、`VectorHitRef`、`VectorAuthorityInput` は契約上の typed field を持つ (`knowledge_unit.rs:394-433`)。二つの helper は generation/parent/Part/raw/profile/text と scope/lease/lifetime の保存値の一致だけを返す (`knowledge_unit.rs:435-478`)。cache hit を Read、retention、evidence、`BodyRequired` の証明に昇格させる API はない。

## 実行した検証

- `cargo test -p search-core --test knowledge_unit_contract`: **6 passed, 0 failed**。
- `cargo clippy -p search-core --all-targets -- -D warnings`: **exit 0**。
- `cargo fmt -p search-core -- --check`: **exit 0**。
- Python 標準ライブラリによる独立 fixture 計算: `ku1:c6b9...67ca1`、`ku1:3b013...b115e`、locator `6e6174...0001`、`東京\n` digest `866bff...15934` は frozen 値と一致。

## 受入境界

`validate_part_units` の binding は trusted host が確定済みという前提であり、serde deserialize 単独では Unit、locator、profile、span の有効性を証明しない。raw bytes との native locator/text 往復、ZIP raw 名衝突・symlink・暗号化、実 reader-use と registry/配備 pin、profile budget、coverage/manifest seal、現在 Read・Version/Part・retention/lease はこの core の外側で検証する。P2 はこの typed input を harness/design に使えるが、P1 本文生成、Source current authority、Vector cache persistence、exact evidence の合格判定には転用しない。
