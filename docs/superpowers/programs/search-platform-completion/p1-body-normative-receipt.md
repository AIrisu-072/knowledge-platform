# P1 本文 normative reconciliation receipt

- Status: **仕様追記済み・独立レビュー待ち**。production reader、bundle、exact evidence、実 DB/Linux sandbox、CI の完成を示さない。
- Writer scope: `spec/data/logical-data-model-v0.md` の P1-L1〜L3、`spec/data/transaction-consistency-requirements-v0.md` の SD-T11〜SD-T13、本 receipt のみ。既存 Unit 最小契約、P4 consistency、P6 outbox の先行差分は書き換えていない。
- 入力 SHA-256: composed freeze `p1-extraction-freeze.md` `205c5a5ff68843e073da8d87b825a55078dbdb66bd985f22d2044eb888fd406d`; full plan `p1-extraction-plan.md` `8a02c655199b974d8bb3dfe865bc91138908f09aba1f57961c058c1a5cab85ec`; exact absence `p1-body-absence-amendment.md` `49838833ff293cf20032e46dbe9429847a975fad8fcb40f21fd977eaa8b69ae1`; Partial positive `p1-partial-positive-correction.md` `b15c20beed37d0f8a569799fa8f41a555e9843f5dc13a47b6c320942130f5db8`; Archive leaf ruling `p1-unit-archive-binding-ruling.md` `eadaa11a8c02953f48e533c53d549e9b26f346dc862231d3d7622a9ee615599c`。

## 追記した規範

| 節 | 固定した境界 |
| --- | --- |
| P1-L1 | Source-owned 現行 Version/全 AUTHORITATIVE Part/representation/FileObject、raw 前後照合、host Unit 検証、資格済み profile/Archive 異種 leaf。 |
| P1-L2 | item ごとの Supported/Partial/Unsupported/Failed 状態、実検索可能 Unit doc との全 field・本文 bytes の双方向一対一、projection-only digest と別の immutable composite bundle。 |
| P1-L3 | request ごとの BodyOnly、title/Graph/Vector 等の代用禁止、Partial の検証済み Unit による限定肯定と gap、Source-owned 有限否定。 |
| SD-T11 | 同一 snapshot、公開直前の current authority、実 Linux mandatory sandbox、15 budget、同一 key stage/validate/CAS/discard、旧 pointer 保持。 |
| SD-T12 | 同じ pinned bundle と現行 Read/Version/Part/raw、親/ClaimId/期待値の一致、Partial positive + blocking gap、非開示 filter。 |
| SD-T13 | Source-owned 全可視 item・全 Unit の有限 literal scan、`Supported` のみの負証明、`MatchFound`/`Unknown`、限定 receipt と公開直前再照合。 |

## 衝突の解消と保持

- `p1-extraction-design.md` §5.6 の body を既存 projection digest に混ぜる案は composed revision §3 により撤回済み。P1-L2/SD-T11 は `ProjectionGenerationManifest.digest` と `resource_count` を維持し、別の composite bundle を要求する。
- `p1-extraction-design-revision-1.md` §2 の lexical no-hit 中心の否定条件は exact absence 追補 §4 により強化済み。SD-T13 は `exhausted_matching_units` を起動条件に留め、Source-owned 全可視 item・全 Unit の literal scan と current binding を必要とする。
- `p1-body-absence-amendment.md` §3 の Supported 限定肯定は `p1-partial-positive-correction.md` により訂正済み。P1-L3/SD-T12 は `Completed + Partial` の検証済み Unit の肯定を許し、gap を保持する。否定の全可視 `Completed + Supported` 条件は変えない。
- Archive Part の scalar leaf format は異種 member を表せない。P1-L1 は item 共通 composite profile と Unit ごとの実 leaf/chain/locator を要求し、同一 Part の Text + CSV を許す。
- 既存 Unit 最小契約 11 行（現行 `logical-data-model-v0.md` 804–814）の確認時 SHA-256 は `681440eb432217a8357eff175a2e593216196e7e81da9108778d8d91a591b062`。先行 P6 outbox 節（現行 `transaction-consistency-requirements-v0.md` 756–788）の確認時 SHA-256 は `72ecd73799239ad2869ee98a49944d36eac0cf2ce6ead0661caa8362010ed530`。本 writer の差分は両節より後への追記だけである。

## 局所確認

- `shasum -a 256` で上記入力 hash を照合した。
- `git diff --check -- spec/data/logical-data-model-v0.md spec/data/transaction-consistency-requirements-v0.md` は PASS。
- `git diff -U0` で本 writer の P1-L1〜L3、SD-T11〜T13 は末尾への追加だけと確認した。表示される P6 の既存変更は開始時からあり、保持した。
- 次の exact action: 独立 read-only reviewer に凍結入力・本3ファイル・既存 Unit/P4/P6 境界を照合させ、指摘があればこの writer 範囲で狭く訂正する。その後、計画の focused RED/GREEN と実 Linux/DB 縦断を別々に実施する。
