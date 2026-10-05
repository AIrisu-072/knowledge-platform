# P1→P2 KnowledgeUnit 最小契約 — 独立レビュー

- 判定: **NO-GO（この版の provider-neutral Unit 入力契約の freeze）**。以下の2点を契約へ反映して再判定する。P1 の本文 generation、`BodyRequired`、evidence 接続の実装・受入はこのレビューの対象外であり、合格を示さない。
- 対象: `p1-knowledgeunit-contract.md` SHA-256 `b38cb20b858a9e467908ce33d46ea8d2a1d52c29bccf28ef586c61c6394a6bfe`。`p1-review-unit.json` の最小契約受入条件、`spec/data/logical-data-model-v0.md` §2.6/§4.4、承認済み Search 設計、既存 Document/Search ID・path 実装と照合した。ビルド・parser 実行・DB 試験は行っていない。

## Freeze を止める指摘

1. **Archive Unit の `ExtractionProfileId` が全 reader 条件を固定しない**（契約:78, 70, 102）。`FormatId::Zip` の `format_settings` は member decoder だけで、inner の Text charset、CSV dialect、PDFium native binary pin を表現しない。PDF の native pin 必須条件は `Pdf` profile にだけ掛かる書き方である。同じ outer ZIP bytes と member chain でも、これらを変えたとき profile と `UnitId` が同じままになり得る。`UnitProvenance.detected_format` が outer `Zip` と inner format のどちらかも未指定で、host の format/locator 照合が一意に定まらない。**必要な修正:** Archive の有効 profile が nested chain 内で実際に使う全 reader 設定・native pin を content-addressed に含む規則と、outer/inner format の記録・照合規則を固定する。固定できない inner format は `Supported` にしない。P2 の cache は修正後の profile ID だけを受ける。
2. **P2 cache と query hit の使用可能範囲が不明確**（契約:104）。cache key の4要素と hit の generation/parent binding は指定されるが、`RetentionMode` ごとの embedding 保存・再利用期限、query の同一 pinned generation 照合、返却直前の Source-owned current `Read` と現行 Version/Part 照合が明文では要求されない。承認済み Search 設計 §5/§27 と `p1-extraction-design.md` §1/§5 は retention、generation、current access を区別する。**必要な修正:** cache は Source の retention/lease が許す期間・scope 内だけに置き、`NO_RETENTION` と `SESSION_ONLY` を永続 cache に入れないこと、cache 再利用を候補・evidence の認可証明にしないことを明記する。Vector hit は評価の pinned generation と Source-owned parent binding を照合し、現行権限・Version を再確認してから候補化する。exact text evidence と lexical `BodyRequired` 条件は引き続き別判定とする。

## 確認できた点

- `ResourceVersionRef.resource_id` を Document Version の `ResourceId` とする点は現行 `DocumentSourceTranslator` と一致する（`crates/search-source-document/src/translate.rs:340-345`）。`ContentPartRef` の ContentItem ID、`LogicalPath`、ordinal は正本 §2.6 と `LogicalPath::new` の制約に沿う（`crates/document-domain/src/versioning.rs:9-31`）。同じ FileObject を使う別 ContentItem も ID が分かれる。
- Native locator は8 variant の起点、物理座標、非 canonical 値の拒否、raw からの往復・text 再構成が規定されている（契約:57-72, 102）。ZIP member chain は length-framed な各 path と inner locator で区別され、入れ子でも単一の flat path に潰れない。各 container の正規化後重複、symlink、曖昧 decode の拒否と、OOXML 内 ZIP を Archive としない境界を確認した。
- `UnitId` の domain separator、9個の length-framed field、UUID の16 byte表現、正規化 text digest は明確（契約:74-98）。Python標準ライブラリで独立計算し、locator hex `6e61746976652d6c6f6361746f723a763100050000000000000001`、2つの `ku1:` ベクトル `c6b9bfcc8a9d53ee19966146ccfce5a8b2f6f792f7cab53d4a9154377e867ca1` / `3b01330d059d71802ec8b3bc216ff9739b3765843892fe4b8fa9bdfa987b115e`、`東京\n` の SHA-256 `866bff0df548a00eaad416ca1fc987f20d94dae000fee8abd356dfb29bd15934` が全て一致した。synthetic profile は codec fixture としてのみ扱う。
- trusted host が正本 Version/Part/representation/raw SHA-256・size/profile と bytes を worker 前後に照合し、worker は Source ID・actor・StorageKey を受けず、host が Unit ID/provenance を付け、raw locator/text/digest を再検証する境界は明確（契約:53-55, 102）。Document の `MediaType::new` は trim のみなので、実装時は契約の lowercase essence への正規化後に正本値と照合する必要がある（`crates/document-domain/src/file.rs:65-80`）。

修正後の再判定はこの最小契約に限る。P1 architecture review の generation/evidence 等の未解決事項を、このレビューから完了扱いしない。
