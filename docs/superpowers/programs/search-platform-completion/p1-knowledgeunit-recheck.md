# P1→P2 KnowledgeUnit 最小契約 — 独立再審査

- 判定: **GO — 下記 SHA の合成最小契約を P2 入力として freeze 可能**。`p1-knowledgeunit-review.md` の NO-GO 2件は閉じた。最小契約内に新たな具体的矛盾は確認しなかった。
- 判定対象: `p1-knowledgeunit-contract.md` SHA-256 `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe` と、§1 Archive provenance / §3 `FormatId::Zip` profile / §4 P2 Vector 規則を上書きする `p1-knowledgeunit-amendment.md` SHA-256 `0d44f5dfed72360bafc56f19c2bf72b1d8b9f278c7ea3247261e1af040a06a00`。元レビュー SHA-256 `adecb0d3ffc634f17abd832d1693ce5bd93d2af2fb982dcb19a5410dd6705e27` の指摘 1–2 に限って再判定した。照合した承認済み Search 設計 SHA-256 `78b0a66bf372ec27cedea8ac153a4c2a7bebb34f8c46effc70d9496797cf19b4`、P1 revision 1 SHA-256 `b1e84e476f614eb30e3bd1c5982e7e0ff2a35e33aef9cddd069618315dcbe742`。

## 指摘の閉鎖

1. **Archive profile / provenance: 閉鎖。** 追補:8–30 は authoritative ZIP item 全体に一つの content-addressed composite profile v2 を定め、outer・各 nested ZIP・leaf の reader node を canonical な member chain 順に符号化する。各 node の v1 definition に実効 charset、CSV dialect、member decoder、parser build、使用する native binary pin と budget が含まれる。PDF pin は必須で、表せない設定・未 pin の native artifact・plan と実行 reader の不一致は `Supported` として受理しない。outer `Zip` と leaf `archive_inner_format` を分け、locator の chain / inner tag、manifest、registry、配備 hash を host が照合する。したがって元レビュー:8 の「同じ ZIP / locator でも inner reader の変更で UnitId が不変」と「outer/inner の曖昧さ」は解消する。
2. **Vector cache / hit authority: 閉鎖。** 追補:34–46 は4要素 key に Source、authority scope、lease、lifetime scope を加え、retention mode ごとの保持上限と lookup / 再利用時の現行 Source 許可を規定する。`SESSION_ONLY` と `NO_RETENTION` の永続 cache・index・backup・queue は禁止され、期限切れ・取消・scope 変更で失効する。cross-generation 再利用は新しい generation / parent binding を作り直す。Vector hit は評価の pinned key と同じ `BodyUnitManifest` の Unit / Version / Part / representation / raw / profile / text digest に一致し、候補化前および公開直前に現行 Source-owned `Read`、Live Version / T10、Part binding を再確認する。cache や similarity は exact evidence、lexical `BodyRequired`、absence の証明にならない。元レビュー:9 の authority / lifetime の欠落は解消する。

元契約:82–98 の `UnitId` domain separator、9 field の順序・framing、`ku1:` codec は追補:52 で不変。元レビュー:15 で二つの golden vector は独立照合済みであり、今回 corpus / codec を再実行していない。変化するのは Archive の `profile_id` 入力値だけである。

この GO は **P2 の provider-neutral Unit / cache 入力契約**に限る。P1 本文 generation・coverage・exact evidence・no-hit semantics の統合レビュー、parser 採用、production 実装、build / CI の合格は含まない。実装時の局所試験は追補:48–52 の差分・拒否・失効ケースを使う（freeze blocker の追加ではない）。
