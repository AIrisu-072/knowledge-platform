# P1→P2 KnowledgeUnit 最小契約 — Archive / Vector 追補

- Status: **設計追補 / 最小契約の独立再判定待ち**。`p1-knowledgeunit-review.md` の NO-GO 2件だけを修正する。本文 generation / evidence 全体の受入、parser 資格、production 実装の完了は示さない。
- 適用: `p1-knowledgeunit-contract.md` §3 の `FormatId::Zip` profile 算出と §1 の Archive provenance、および §4 の P2 Vector cache / hit 規則を本書で上書きする。それ以外は同契約と `p1-extraction-design-revision-1.md` を併用する。承認済み Search 設計の Source 正本、retention、現行 Read、generation pin は維持する。

## 1. Archive の composite `ExtractionProfileId`

通常の非 Archive Unit の `ExtractionProfileDefinition` v1 と `ExtractionProfileId` は変更しない。`FormatId::Zip` の authoritative item には **item 単位で一つ**の Archive composite profile v2 を使い、`BodyItemEntry.profile` とその item の全 `KnowledgeUnit.provenance.profile` を同じ ID にする。ZIP 内の一 member だけの設定変更でも、この item の profile と全 UnitId が変わる。

```text
ArchiveReaderNode {
  members: Vec<String>,                   // [] は outer ZIP、以後はその reader を起動する member chain
  parser_build_id: String,                // 登録済み ASCII ID
  definition: ExtractionProfileDefinitionV1,
}
chain_bytes = u32_be(members.len) || concat(frame(member.as_bytes()))
node_bytes  = frame(chain_bytes) || frame(parser_build_id ASCII bytes)
              || frame(definition の §3 v1 binary encoding 全体)
archive_profile_bytes = "extraction-profile:archive:v2\0"
                        || u32_be(nodes.len) || concat(frame(node_bytes))
ExtractionProfileId = "sha256:" + lower_hex(SHA-256(archive_profile_bytes))
```

`frame` は元契約の `u32 BE length || bytes`。`members` は元契約の厳密 decode・NFC・相対 path 検証済み文字列であり、UTF-8 bytes の **成分列**の辞書順（prefix は先）で node を一意に並べる。空 chain の `Zip` node はちょうど一つかつ先頭。重複 chain、余分な node、未知 tag、非 canonical 文字列・encoding、trailing bytes を拒否する。非空 chain の各 proper prefix は `Zip` node であり、終端 node はその member に実際に適用する reader とする。中間 ZIP は自身の `format_settings::Archive.member_decoder` を持ち、leaf は tag 1–7 の reader とその実際の `format_settings` を持つ。1 member に二つの reader を暗黙選択しない。

各 node の v1 definition は `FormatId`、parser name/version/build SHA-256、native binary SHA-256、scope/segmentation/normalization/locator revision、format settings、全 budget を含む。Text charset、CSV charset/delimiter/quote、各階層の ZIP member decoder、PDFium native pin を省略しない。PDF node の native pin は必須。他形式も native binary を使うなら pin 必須で、複数の unpinned native artifact や v1 definition に表せない有効 reader 設定があれば `Supported` にしない。登録済み build ID は node の parser identity/hash と一致させる。reader の暗黙 default は登録済み definition に展開して固定する。

reader plan は最終 Unit を受理する前に固定する。worker は起動した全 reader を `reader_use: Vec<ArchiveReaderNode>` として返し、host は chain / build / native pin / effective settings の node 集合との一対一照合に加え、registry と配備済み parser/native hash を検証する。worker 報告値だけから profile を信用しない。異なる reader を実行した、途中の ZIP node が欠けた、実際の設定を profile へ符号化できない、または plan が非決定的なら、その item を `Supported` として公開しない。build/native pin 不一致は元契約 §4 と P1 revision §6 の integrity/config incident として新 generation を公開しない。

`UnitProvenance.detected_format` と `BodyItemEntry.detected_format` は **outer authoritative bytes** の `Zip` とする。Archive Unit の `UnitProvenance` に `archive_inner_format: Option<FormatId>` を追加し、Archive では `Some(leaf FormatId)`、非 Archive では `None` に固定する。既存 `parser_build_id` は Archive では outer node の ID、各 inner parser/build/native pin は composite profile の対応 node に記録する。host は `NativeLocator::Archive.members` が plan の唯一の leaf chain に一致し、`inner` tag が `archive_inner_format` と leaf definition の `FormatId` に一致し、中間 node が全て `Zip`、outer raw/representation と Unit の profile が item manifest と一致することを検証する。OOXML 内部 ZIP は引き続き Archive chain として扱わない。

## 2. Vector の retention、cache、candidate 境界

Embedding cache と Vector projection は再生成可能な **本文由来 artifact** であり、Source の現在の retention 許可を超えて保存・複製・再利用しない。cache entry は元契約の4要素 `(embedding_model_id, UnitId, text_sha256, ExtractionProfileId)` に加え、`source_id`、Source が発行した `authority_scope_key`、`retention_lease_id`、`lifetime_scope_id` を key とし、entry に `retention_mode`、`lease_expires_at: Option<UTC timestamp>`、作成元 `ResourceVersionRef` / `ContentPartRef` / raw binding、作成元 generation key を保持する。`authority_scope_key` は Source の tenant / service / owner / access 境界を潰さない opaque ID で、別 scope へ共有しない。`lifetime_scope_id` は `SESSION_ONLY` では session ID、`NO_RETENTION` では request ID、それ以外では空文字に固定する。lease は Source 側で有効性を再確認できる非 secret ID とし、期限切れ・取消・scope 変更時は entry と対応 Vector artifact を失効・削除する。

| Source retention | embedding の保持・再利用上限 |
| --- | --- |
| `PERSISTENT_RESOURCE` | Source が本文由来 embedding の永続保持を明示許可した scope に限る。期限があれば lease 内だけ。 |
| `PERSISTENT_DISCOVERY_METADATA` | metadata 保持の許可を本文由来 embedding に拡大しない。別の明示許可がない限り永続化しない。 |
| `CACHE_WITH_EXPIRY` | 明示許可された scope と有限 `lease_expires_at` まで。期限後の cache / Vector artifact は使わない。 |
| `SESSION_ONLY` | 許可された session 内の揮発的保持のみ。session 終了または lease 失効で破棄し、永続 embedding / index / backup / queue へ書かない。 |
| `NO_RETENTION` | Source が許す request 中の一時 materialization のみ。request 終了時に破棄し、永続 embedding / index / backup / queue へ書かない。 |

retention mode だけから許可を推定しない。lease / scope / 現行 Source 許可を lookup と再利用の時点で照合し、不明・期限切れ・取消なら miss とする。cross-generation で同じ4要素の embedding bytes を再利用する場合も、再利用先の許可と manifest を検証して **新しい generation / parent binding を作り直す**。古い hit binding をコピーしない。cache の存在、miss、embedding 値は Read 許可・現行 Version・exact evidence・absence の証明にならない。

Vector hit は `VectorHitRef { generation: ProjectionGenerationKey, unit_id: UnitId, version: ResourceVersionRef, part: ContentPartRef, authoritative_representation_ref: String, raw: RawBinding, profile: ExtractionProfileId, text_sha256: [u8;32] }` を内部で保持する。candidate 化の前に (1) 評価中の `pin_current_bundle()` の key と `hit.generation` の一致、(2) 同じ pinned `BodyUnitManifest` の Unit / Version / Part / representation / raw / profile / text digest の完全一致、(3) Source-owned 現行 `Read`、現行 Live Version / T10、authoritative Part / representation / raw binding と retention 許可を確認する。不一致・Unknown は候補にしない。公開直前にも現在 Read と Version / Part binding を再確認し、失効した候補・rank・trace は伏せる。Vector similarity は P1 revision §5 の exact text resolver を代替せず、`BodyRequired` の lexical `BodyOnly` qualified hit / absence 条件を満たさない。S1 の既存 retrieval / Fusion 順は変更しない。

## 再判定用の局所チェック

- 同一 ZIP bytes / locator でも、inner charset・CSV dialect・nested decoder・parser build・PDFium pin のいずれかが違えば composite profile と UnitId が違う。欠けた reader node、outer/inner format 不一致、実行 pin 不一致を受理しない。
- `NO_RETENTION` / `SESSION_ONLY` の永続 embedding は 0 件。scope / lease / generation が違う cache hit はそのまま候補にしない。Read 取消・Version/Part 差替え後の Vector hit は返さず、exact evidence / lexical `BodyRequired` に昇格しない。
- `UnitId` の domain separator、9 field の順序・length framing、外部 `ku1:` codec、元契約の2つの golden vector は**変更しない**。変わるのは Archive に入力する `profile_id` の値だけである。
