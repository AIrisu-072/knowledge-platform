# Audit event contract（spec/telemetry）

Audit Infrastructure v1（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §4、[決定記録](../../docs/decisions/2026-10-07-audit-envelope-store-integrity.md)）のevent契約である。

| ファイル | 位置付け |
|---|---|
| `audit-event-catalog.json` | 正本（手で編集する）。event typeごとの形・写像規則 |
| `audit-event.schema.json` | catalogから `crates/audit-core` が生成する。手で編集しない |
| `crates/audit-core` | catalogを `include_str!` で埋め込み、runtime検証・legacy投影・chain計算・export検証を行う |

## catalog

`version` は1。`events` の各entryは次を持つ。未知のmemberと重複keyは読込時に拒否する。

| member | 意味 |
|---|---|
| `type` / `source` | 既存のevent typeとsource。renameしない |
| `origin` | `relay`（外部source。現在はDocumentのみ）、`store` / `relay_control`（control event。relay経路では常に拒否） |
| `event_class` | OA §14の8 class |
| `resources` | 許可する `resource.type` |
| `version_required` | `true`（`resource.version_id` 必須）、`false`（禁止）、`"optional"` |
| `results` | 許可する `result` |
| `subjects` | subject形の一覧。placeholderは `{resource.id}`、`{resource.version_id}`、`{details.<必須uuid field>}`。`{"template", "resource"}` 形は特定のresource種別に限る |
| `fields` / `required` | legacy `data` のallowlistと型（kind）。`reason` はfieldではない |
| `reason` | `absent`（sourceが理由を記録しない）／`caller_text`（理由文を複製せず要約だけを出す） |
| `reason_code_field` | `data.reason_code` に写す閉じたcode field（detailsにも残る） |
| `service_executor_field` | `data.service_executor` へ持ち上げ、detailsから除くprincipal field |
| `duplicated_actor_field` | staging列のactorと一致を検証してから捨てるprincipal field |
| `operation_id_field` / `publish_operation_id_field` | `correlation.operation_id` / `correlation.publish_operation_id` の写像元 |
| `nil_resource_allowed` | nil UUIDの `resource.id` を許す（`authorization.denied` だけ） |
| `bindings` | `details[field]` が `resource.id` / `resource.version_id` / `resource.type` と一致すること。producerが同じ値を書くと確認できたものだけを登録する |

読込時の検査：version、type一意、`required ⊆ fields`、kindが既知、enum系の値が空でない、subjectが空でなくplaceholderが解決できる、bindingが解決できる、control originは `audit.*` typeと専用sourceを持つ。

### kind

自由文字列のkindは無い。JSON整数は `i64`/`u64` で表せる整数のみで、`1.0` や `1e0` は拒否する。

| kind | 内容 |
|---|---|
| `uuid` / `nullable_uuid` | 小文字・hyphen付きのcanonical UUID。nil UUIDは拒否 |
| `counter` / `nullable_counter` | 0以上 `i64::MAX` 以下 |
| `positive_counter` | 1以上 `i64::MAX` 以下 |
| `boolean` | 真偽値 |
| `enum` / `nullable_enum` | `values` のいずれか（nullable はnullも可） |
| `enum_list` | `values` の要素を1個以上、重複なし |
| `digest` / `nullable_digest` | 0–255の整数ちょうど32個 |
| `principal` | `{identityProvider, principalId}` のみ。各部は空でなく256 byte以下、制御文字なし |
| `legacy_time` | 下記 |
| `utc_timestamp` | `YYYY-MM-DDTHH:MM:SS.ffffffZ`（control用） |
| `uuid_list` | `uuid` を最大100件、重複なし |
| `hex_digest` | 小文字hex 64桁 |

### legacy_time

Documentのpayload時刻（`publishedAt`、`scheduledPublishAt`、`endedAt`）は、workspaceの `time` が `serde` featureだけを有効にしているため、`OffsetDateTime` のserde tuple `[year, ordinal, hour, minute, second, nanosecond, offset_h, offset_m, offset_s]` として保存されている。RFC 3339へ変換したとは表記しない。Rustは日付の実在（閏年のordinal 366等）、時刻範囲（秒は0–59）、offset範囲（±25時、±59分、±59秒）と、offset各部の符号が混在しないことを検査する。この形はaudit-coreの試験で `OffsetDateTime` の実serialize結果と照合して固定している。audit系crateでは `time/serde-human-readable` を有効にしない。

## envelope（CloudEvents 1.0.2 structured JSON）

属性は `specversion`（`"1.0"`）、`id`、`source`、`type`、`subject`、`time`（`utc_timestamp`）、`datacontenttype`（`"application/json"`）、`dataschema`（`"urn:knowledge-platform:audit:payload:v1"`）、`data` だけである。未知の属性・extension attributeは拒否する。

payload v1（`data`）は `schema_version`、`event_class`、`action`（= `type`）、`actor {issuer, principal_id}`、`service_executor`（任意）、`resource {type, id, version_id?}`、`result`、`reason_code`（任意）、`reason`（任意）、`correlation`、`details`、`extensions`（v1では `{}` のみ）、`provenance` である。

- 大きさ：Rustはcompactなserializeで24 KiB以下を要求する。StoreはPostgreSQLの `jsonb::text` で32 KiB以下を要求する。したがってRustが受理したものはStoreでも受理される。
- 文字列：すべて512 byte以下、制御文字なし。principalの各部は256 byte以下。
- `correlation`：`operation_id` と `publish_operation_id` はcatalogの写像元fieldと一致しなければならない（`document.version.publication.cancelled` は `publish_operation_id` だけ）。`source_correlation_id` はstagingの `trace_id` 列で、canonicalな小文字UUIDに限る（それ以外は `invalid_source_correlation`）。`trace_id`（W3C、32桁hex、非ゼロ）は予約であり、legacy投影は設定しない。
- `provenance`：Document adapterは `{source_format: "document-audit-outbox-v0", adapter_version, source_commitment, registration}` を必須とする。control eventは `{source_format: "audit-store-control-v1" | "audit-relay-control-v1", adapter_version}` だけを持つ。

## 自由記述reasonを複製しない規則

`caller_text` のtype（`document.version.withdrawn`、`document.publication.ended`、`document.metadata.changed`、`document.moved`、`folder.created`、`folder.renamed`、`folder.moved`）では、claim関数（SQL）が `data - 'reason'` を返し、`reason_kind`（`jsonb_typeof`）と `reason_bytes` だけを渡す。投影は `reason: {provided: true, utf8_bytes, text_retained: "source_systems"}` だけを作る。

- `data` に `reason` が残っていれば `unknown_field` で拒否する（理由文はどこにも複製しない）。
- 理由が文字列でなければ `reason_not_string`、`caller_text` なのに理由が無ければ `invalid_reason`。
- `absent` のtype（通常の `access_policy.changed` を含む）は `reason` を出さない。`provided: false` とも書かない。
- withdrawn・endedの `data.actor` は列のactorと一致を検証し（不一致は `actor_mismatch`）、detailsから除く。
- `serviceExecutor`（published・terminal）は `service_executor {issuer, principal_id}` へ持ち上げ、detailsから除く。

## schema生成とRust⊂schema

```
AUDIT_SCHEMA_BLESS=1 cargo test -p audit-core --test schema_contract   # 再生成
cargo test -p audit-core --test schema_contract                        # byte一致の確認
```

生成schemaはJSON Schema 2020-12で、共有部分を `$defs` に1回だけ定義し、typeごとに `if`/`then` で `$defs/event.<type>` を適用する。

関係は「Rustが受理するものはschemaも受理する」である。試験は、全acceptance fixtureがschemaに適合すること、拒否表のうちschemaも拒否するものには分類を付けず、schemaが受理してしまうものには必ず `rust_only:<category>` を付けることを確認する。

| `rust_only` 分類 | Rustだけが検査する制約 |
|---|---|
| `float_integer` | `2.0`・`2e0` の拒否（JSON Schemaでは整数とみなされる） |
| `utf8_bytes` | byte単位の上限（schemaの `maxLength` は文字数） |
| `subject_binding` | subjectのplaceholderと `resource` / `details` の一致 |
| `resource_binding` | `bindings` の一致 |
| `correlation_binding` | `correlation` と写像元fieldの一致 |
| `reason_code_binding` | `reason_code` と写像元fieldの一致 |
| `calendar` | 実在しない日付、`legacy_time` のoffset符号混在 |
| `duplicate_key` | 重複key（schemaは解析後の値しか見ない） |
| `origin_path` | 提出経路（relay/store/relay_control）とcatalogのoriginの一致 |

envelope全体の24 KiB上限もRustだけが検査する。

## control event（単位B予約）

catalogは `origin: "store"`（source `urn:knowledge-platform:audit-store`）と `origin: "relay_control"`（source `urn:knowledge-platform:audit-relay`）のentryを読み込める。type名は `audit.` で始め、`AuditStore` resource（id `audit-store`）を使える。現時点ではcontrol entryを登録していない。単位B（Store・relay）が設計§4.5の12種を追加し、SQLが組み立てたcontrol eventをRustのcatalogで検証する試験を同時に追加する。relay経路（`Origin::Relay`）は、`audit.*` type、control source、`AuditStore` resourceを `control_type_forbidden` で常に拒否する。

## hash chain

- `envelope_digest = sha256(jsonb::text のbyte列)`（`kp-audit-jsonb-sha256-v1`）
- `chain = sha256("kp-audit-chain-v1" || prev_chain || int8send(seq) || uuid_send(event_id) || envelope_digest)`
- `GENESIS = sha256("kp-audit-chain-genesis-v1")`。seq 1の `prev_chain` である。

SQL実装と照合するための固定vector（`crates/audit-core/tests/chain_export.rs`）：

| 値 | hex |
|---|---|
| `GENESIS` | `9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344` |
| `envelope_digest('{"example":"envelope"}')` | `c147ac99fc21ba6cc69a47812b1bb2415e353995e22b65d6a2b58a5222dd5f6f` |
| seq 1、event `0199a1b2-0000-7000-8000-000000000001`、上のdigest | `babbd336ec2c8556082424a253afb6d8e88ef5a63c9cbcab6cea8f441ad528e1` |
| seq 2、event `0199a1b2-0000-7000-8000-000000000002`、digest = `0x11` × 32 | `1496cf0e490216c2ca995db8ac0a340562a691cf1d2f955865624900b4856106` |

## export検証

export行は `{"seq","event_id","origin","envelope_digest","prev_chain","chain","recovery_epoch","expired","envelope"}` で、`envelope` はStoreの `jsonb::text` の原文（失効済み・identity chainでは `null`）である。audit-coreはRawValueで原文を保持してhashする。

- `verify_export(text, Anchor)`：genesisまたは信頼済みcheckpointから、seqの連続性、`prev_chain` の連鎖、chainの再計算、本文digest、`envelope.id = event_id`、本文の有無と `expired` の整合、epochの非減少を検査する。
- `verify_identity_chain(text, Anchor)`：本文の無いidentity chainを同様に検査する。
- `verify_export_subset(text)`：filter付きexportの行単位の整合だけを見る。結果は常に `anchored: false` であり、真正性の根拠にならない。
- `compare_checkpoint(report, checkpoint)`：`Match` / `Mismatch` / `StoreBehind` / `Ahead` / `BeforeAnchor` / `Unanchored`。

## 拒否code

`envelope_too_large`、`invalid_json`、`duplicate_key`、`invalid_envelope`、`unknown_event_type`、`control_type_forbidden`、`invalid_source`、`invalid_subject`、`invalid_resource`、`invalid_result`、`invalid_actor`、`invalid_service_executor`、`unknown_field`、`missing_field`、`invalid_field`、`invalid_correlation`、`invalid_source_correlation`、`invalid_reason`、`reason_not_string`、`actor_mismatch`、`source_row_too_large`、`source_digest_mismatch`、`invalid_provenance`、`invalid_extensions`。拒否は、codeと、catalogのfield名または固定の位置名だけを持つ。payloadの値は含めない。

## producerとの対応（main `d515aa3`）

| type | producer |
|---|---|
| `document.created`、初回 `document.version.created` | `document-application/src/service.rs` → `document-repository-postgres/src/repository.rs` |
| `document.version.created/updated/rebased` | `versioning_mutation.rs`（`baseDocumentVersionId` はnull可） |
| `document.version.published` | `versioning_service.rs`・`service.rs` → `publish.rs`（予約公開のみ `serviceExecutor`） |
| `document.version.publication.scheduled/cancelled/terminal` | `schedule.rs`（terminalReasonは `versioning_service.rs` の8種） |
| `document.version.withdrawn` | `withdrawal.rs`（restorationWithheldReasonは5種） |
| `document.publication.ended` | `publication_end.rs` |
| `document.metadata.changed`、`document.moved` | `document_management.rs` → `targeted_events.rs` |
| `folder.created/renamed/moved` | `folder_management.rs` → `targeted_events.rs` |
| `access_policy.changed` | `access_policy.rs`（通常・bootstrap） |
| `authorization.denied` | `targeted_events.rs`（`access_policy.rs`・`read_state.rs` から） |
| `document.version.read_confirmed` | `read_state.rs` |
| `document.file.access_granted` | `file_access.rs` |
| `document.diff.result_access_granted` | `document_diff_access.rs` |
| `document.revision_comparison.result_access_granted` | `document_revision_read.rs` |
