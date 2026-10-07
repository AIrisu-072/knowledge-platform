# Audit event contract（spec/telemetry）

Audit Infrastructure v1（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §4、[決定記録](../../docs/decisions/2026-10-07-audit-envelope-store-integrity.md)）のevent契約である。

| ファイル | 位置付け |
|---|---|
| `audit-event-catalog.json` | 正本（手で編集する）。sourceごとのadapter契約と、event typeごとの形・写像規則 |
| `audit-event.schema.json` | catalogから `crates/audit-core` が生成する。手で編集しない |
| `audit-adapter-golden.json` | (source_format, adapter_version) ごとの投影結果のdigest。entryは `<fixture>@<入力行hash>`、sectionとentryは追加のみ（旧sectionはdigestで凍結） |
| `crates/audit-core` | catalogを `include_str!` で埋め込み、runtime検証・legacy投影・chain計算・export検証・Store portを提供する |

## catalog

`version` は1。トップレベルは `version`、`adapters`、`events` の3つである。未知のmemberと重複keyは読込時に拒否する。

### adapters

sourceごとに1件。envelopeの `provenance` と `correlation.trace_id` の検証は、ここから読む（Rustにsourceごとの値を直書きしない）。

| member | 意味 |
|---|---|
| `source` | 対象のsource。各eventの `source` は、同じoriginのadapterをちょうど1件持つ |
| `origin` | `relay` / `store` / `relay_control` |
| `source_format` | `provenance.source_format` の値（`[a-z0-9-]{1,64}`、adapter間で一意） |
| `adapter_version` | `provenance.adapter_version` の値。現在の投影版で、envelopeはこの値と一致しなければならない |
| `commitment` / `registration` | `provenance.source_commitment` / `provenance.registration` の要否（`required` / `optional` / `forbidden`） |
| `trace_id` | `correlation.trace_id` を設定できるか。予約fieldであり、専用のW3C trace列を持つadapterだけが `true` にできる |

現在のadapter：

| source | origin | source_format | adapter_version | commitment | registration | trace_id |
|---|---|---|---|---|---|---|
| `urn:knowledge-platform:document-platform` | relay | `document-audit-outbox-v0` | 1 | required | required | false |
| `urn:knowledge-platform:audit-store` | store | `audit-store-control-v1` | 1 | forbidden | forbidden | false |
| `urn:knowledge-platform:audit-relay` | relay_control | `audit-relay-control-v1` | 1 | forbidden | forbidden | false |

読込時の検査：adapterが1件以上、sourceとsource_formatが一意、`adapter_version ≥ 1`、control originのadapterはcommitment・registrationが `forbidden` で `trace_id: false`、どのeventにも使われないadapterは拒否、eventのsourceにadapterが無ければ拒否。

`Catalog::registered_types()` は、origin=relayのtypeについて `(source, type, adapter_version)` を返す。Storeの `registered_types` 表はこれから投入し、試験で一致を確認する。control typeはingestされないので含めない。relayの `probe(expected)` も同じ一覧（`ProbeExpectation::from_catalog`）を送る。

### events

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
| `client_chosen_resource_ids` | `resource.id` をclientが選ぶresource種別（relay entryのみ。現在は `Folder`）。nil UUIDは `nil_client_id`（下記） |
| `client_chosen_version_id` | `resource.version_id` をclientが選び得る（relay entryでversionを持つものだけ）。nil UUIDは `nil_client_id` |

`fields` の各fieldは `kind`、enum系の `values`、`client_chosen`（clientが選ぶID。relay entryのuuid系kindだけ）を持つ。

読込時の検査：version、type一意、`required ⊆ fields`、kindが既知、enum系の値が空でなく一意で `[A-Za-z0-9_-]{1,64}`、subjectが空でなくplaceholderが解決できる、bindingが解決できる、control originは `audit.*` typeと専用sourceを持つ、`AuditStore` に適用され得るsubject（`resource` が `AuditStore`、または種別指定なしで `resources` に `AuditStore` を含む）は `{resource.id}` を使わない（Rustは `audit-store` を描画し、生成schemaはUUIDを要求するため）、`client_chosen` 系はrelay entryのuuid系kind・entryのresource（`AuditStore` 以外）・versionを持つentryに限る、`source_urn` / `source_list` は `values` を書かない（読込時にcatalogのsourceから補う）。

`Catalog::validate(&'static self, value, origin)` は、埋め込み以外のcatalog（試験用。`Box::leak` で `'static` にする）でenvelopeを検証する。拒否はcatalogのfield名を `&'static str` で持つので、catalogは `'static` でなければならない。

### kind

自由記述用のkindは無い。文字列のkindは、閉じた集合（enum、source、resource ref、db_role等）か、文法で制限した値である。ただし、文法だけで制限するkind（`event_type`：catalogの文法・最大128 B、`principal_ref`：principal文字種・各256 B）には、読める文字列が入り得る。Store（unit B）は、filter・selectorのevent typeを登録済みtypeとcontrol typeだけに限定する。actorのfilter値は、上限付きのprincipal文字列として記録する（この残余は受容する）。JSON整数は `i64`/`u64` で表せる整数のみで、`1.0` や `1e0` は拒否する。

| kind | 内容 |
|---|---|
| `uuid` / `nullable_uuid` | 小文字・hyphen付きのcanonical UUID（nil不可。nilの扱いは下記 `nil_client_id`）。値は約16 byteの不透明な値で、versionやvariantは検査しない |
| `counter` / `nullable_counter` | 0以上 `i64::MAX` 以下 |
| `positive_counter` / `nullable_positive_counter` | 1以上 `i64::MAX` 以下（nullable はnullも可） |
| `boolean` | 真偽値 |
| `enum` / `nullable_enum` | `values` のいずれか（nullable はnullも可） |
| `enum_list` | `values` の要素を1個以上、重複なし |
| `digest` / `nullable_digest` | 0–255の整数ちょうど32個（32 byteの不透明な値） |
| `principal` | `{identityProvider, principalId}` のみ。各部はprincipal文字規則（下記）に従う |
| `legacy_time` | 下記 |
| `utc_timestamp` / `nullable_utc_timestamp` | `YYYY-MM-DDTHH:MM:SS.ffffffZ`（control用） |
| `uuid_list` | `uuid` を最大100件、重複なし |
| `hex_digest` / `nullable_hex_digest` | 小文字hex 64桁 |
| `resource_ref` | control event専用。envelopeに現れるresource id：小文字canonical UUID（`authorization.denied` の行があるのでnilを含む）か、ちょうど `audit-store` |
| `event_type` / `event_type_list` | control event専用。catalogの文法 `[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+`、128 byte以下（版の食い違いに備えて文法だけを見る）。listは1–16件、重複なし |
| `source_urn` / `source_list` | control event専用。catalogのsource（全adapterのsourceと2つのcontrol source）のいずれか。値の集合は読込時にcatalogから作り、entryには書かない。listは1–16件、重複なし |
| `db_role` / `nullable_db_role` | control event専用。引用符の要らないPostgreSQLのrole名 `[a-z_][a-z0-9_$]{0,62}` |
| `principal_ref` | control event専用。principalの1部分（issuerまたはprincipal id）。principal文字規則（下記）に従う |
| `int8_text` | control event専用。PostgreSQL int8のcanonicalな10進表記（`0` または `-?[1-9][0-9]*`、i64の範囲内）。fingerprintの各部 |
| `code` | control event専用。`[a-z0-9_]{1,64}` |

control event専用のkindは、`origin: relay` のentryでは読込時に拒否する（relay entryの語彙を変えない）。list系の上限16（`MAX_CONTROL_LIST`）は設計§10.3のfilter event_types≤16と同じで、retentionのselector（`selector_event_types`、`selector_sources`）も最大16件である（relay typeが16を超える場合、1つのpolicyで全typeを列挙できない。event_classで選ぶ）。上限内の最大envelopeは試験で32 KiB以内（2倍の余裕）に収まることを確認する。

### legacy_time

Documentのpayload時刻（`publishedAt`、`scheduledPublishAt`、`endedAt`）は、workspaceの `time` が `serde` featureだけを有効にしているため、`OffsetDateTime` のserde tuple `[year, ordinal, hour, minute, second, nanosecond, offset_h, offset_m, offset_s]` として保存されている。RFC 3339へ変換したとは表記しない。Rustは日付の実在（閏年のordinal 366等）、時刻範囲（秒は0–59）、offset範囲（±25時、±59分、±59秒）と、offset各部の符号が混在しないことを検査する。この形はaudit-coreの試験で `OffsetDateTime` の実serialize結果と照合して固定している。audit系crateでは `time/serde-human-readable` を有効にしない。

## envelope（CloudEvents 1.0.2 structured JSON）

属性は `specversion`（`"1.0"`）、`id`、`source`、`type`、`subject`、`time`（`utc_timestamp`）、`datacontenttype`（`"application/json"`）、`dataschema`（`"urn:knowledge-platform:audit:payload:v1"`）、`data` だけである。未知の属性・extension attributeは拒否する。

payload v1（`data`）は `schema_version`、`event_class`、`action`（= `type`）、`actor {issuer, principal_id}`、`service_executor`（任意）、`resource {type, id, version_id?}`、`result`、`reason_code`（任意）、`reason`（任意）、`correlation`、`details`、`extensions`（v1では `{}` のみ）、`provenance` である。

- 大きさ：上限はPostgreSQLの `jsonb::text` の長さで32 KiB（`JSONB_TEXT_LIMIT` = 32768 byte）である。jsonbは `{"a": 1, "b": [1, 2]}` のように、memberの `:` とすべての `,` の後に空白を1つ入れる。文字列のescapeはcompactなserdeと同じ（`"` `\` `\b` `\f` `\n` `\r` `\t` は2 byte、他のC0制御文字は `\u00xx`、それ以外はそのまま）である。したがってjsonb長 = compact長 + 構造上の `:` と `,` の数である。Rustは `jsonb_text_len` でこの長さを計算し、Storeと同じ32 KiBで判定する。compact長の別上限は置かない（compact長は常にjsonb長以下）。試験は、各catalog entryの最大envelope（全field、最長値、escapeで2倍になる文字）が上限内に収まることを確認する（2026-10時点でrelay最大は約3.6 KB、control最大は閉じたkindへの置換後で `audit.access.intent_opened` の約9.7 KB。relay・controlとも上限の半分以下であることを試験で確認する）。
- 文字列：すべて512 byte以下、制御文字（Cc）なし。
- principal（`actor`、`service_executor` の各部、legacyの `principal` kind）：下記の文字規則に従う。
- `resource.id` / `resource.version_id` / uuid系のdetails：nil UUIDは、clientが選ぶIDなら `nil_client_id`、それ以外は `invalid_resource` / `invalid_field`（下記）。
- `correlation`：`operation_id` と `publish_operation_id` はcatalogの写像元fieldと一致しなければならない（`document.version.publication.cancelled` は `publish_operation_id` だけ）。`source_correlation_id` はstagingの `trace_id` 列で、canonicalな小文字UUIDに限る（それ以外は `invalid_source_correlation`）。`trace_id`（W3C、32桁hex、非ゼロ）は予約であり、adapterの `trace_id: true` の場合だけ許す。現在のadapterはすべて `false` なので、legacy relay経路（`document-audit-outbox-v0`）では `trace_id` を `invalid_correlation` で拒否する。
- `reason.utf8_bytes`：0以上1,048,576（DocumentのHTTP body上限）以下。legacy投影の `reason_bytes` とraw envelopeの両方で検査する。
- `provenance`：sourceのadapterに従う。`source_format` と `adapter_version` はadapterの値と一致し、`source_commitment`（小文字hex 64桁）と `registration`（`trigger` / `backfill` / `repair`）はadapterの要否に従う。Document adapterは4 memberすべて必須、control eventは `{source_format, adapter_version}` だけである。

### principalの文字規則（`rust_only:principal_charset`）

principalの各部（issuer、principal id）は、次をすべて満たす。

- 256 byte以下（UTF-8）、空でない。
- 先頭と末尾がUnicodeの `White_Space` でない（したがって空白だけの値も拒否する。Documentの `PrincipalRef::new` のtrimに合わせる）。
- 次の文字を含まない。
  - 制御文字：Unicode General_Category `Cc`（U+0000–001F、U+007F–009F）。READMEとコードで「制御文字」はこの範囲を指す。
  - `Bidi_Control`：U+061C、U+200E、U+200F、U+202A–202E、U+2066–2069。
  - U+2028 LINE SEPARATOR、U+2029 PARAGRAPH SEPARATOR。
  - U+FEFF（BOM）。
  - TAG block U+E0000–E007F。
  - noncharacter：U+FDD0–FDEF と、各planeの U+xxFFFE / U+xxFFFF。

それ以外の書式文字（ZWJ等）は許す。識別子として正当な用途があり、拒否されたstaging行は永久にquarantineされるためである。生成schemaは `minLength`・`maxLength` とCcの除外だけを表す（より緩い）ので、上の追加規則による拒否は `rust_only:principal_charset` に分類する。principal文字集合の方針（OIDC `sub`、PRECIS等）はidentity adapter／IdPへの引継ぎ事項である。

### nil UUIDのclient指定ID（`nil_client_id`）

Documentのproducerは、clientが選ぶIDにnil UUIDを受け付ける（`spec/api/schemas/document/commands.yaml`）。clientが選ぶのは、`CreateFolder.folderId`（folderのID）と `VersionWrite.targetVersionId`（新しいversionのID）、およびそれらを参照する値である。document id（初回versionのIDを含む）、revision、policy、content item、representationのIDはserverが生成し、operation idはUUIDv7として検査される。

catalogはclientが選ぶIDだけに印を付ける：fieldの `client_chosen: true`（version id・folder idのfield。`documentVersionId`、`baseDocumentVersionId`、`withdrawnDocumentVersionId`、`formerCurrentVersionId`、`resultingCurrentVersionId`、`document_version_id`、`base_version_id`、`target_version_id`、`from_folder_id`、`to_folder_id`、`folder_id`、`parent_folder_id`、`from_parent_id`、`to_parent_id`、`access_policy.changed` の `target_id`）、entryの `client_chosen_resource_ids: ["Folder"]`（`folder.*` と `access_policy.changed`）と `client_chosen_version_id: true`（versionを持つrelay entry）。

- relay経路で、印の付いたfield・`resource.id`・`resource.version_id` がnil UUIDの場合だけ `nil_client_id` で拒否する。これはproducerから到達できるquarantineであり、replayでは解消しない。改ざんと区別するため別codeにしている。
- 印の無いID（serverが生成するもの）のnil UUIDは、producerからは到達できないので `invalid_field` / `invalid_resource` とする。control event（Store・relay_control経路）にはclientが選ぶIDが無いので、nil UUIDは常に `invalid_field` である（`authorization.denied` の `resource.id` だけはnilを許す）。
- Documentへの引継ぎ：client指定IDのnil拒否（可能ならOperationIdと同じUUIDv7の検査）、`commands.yaml` の該当schema、必要ならDBのCHECK制約。
- 生成schemaもnil UUIDを拒否する（`$defs/uuid`）ので、`rust_only` ではない。

## 自由記述reasonを複製しない規則

`caller_text` のtype（`document.version.withdrawn`、`document.publication.ended`、`document.metadata.changed`、`document.moved`、`folder.created`、`folder.renamed`、`folder.moved`）では、claim関数（SQL）が `data - 'reason'` を返し、`reason_kind`（`jsonb_typeof`）と `reason_bytes` だけを渡す。投影は `reason: {provided: true, utf8_bytes, text_retained: "source_systems"}` だけを作る。

- `data` に `reason` が残っていれば `unknown_field` で拒否する（理由文はどこにも複製しない）。
- 理由が文字列でなければ `reason_not_string`、`caller_text` なのに理由が無い、または `reason_bytes` が0–1,048,576の範囲外なら `invalid_reason`。
- `absent` のtype（通常の `access_policy.changed` を含む）は `reason` を出さない。`provided: false` とも書かない。
- withdrawn・endedの `data.actor` は列のactorと一致を検証し（不一致は `actor_mismatch`）、detailsから除く。
- `serviceExecutor`（published・terminal）は `service_executor {issuer, principal_id}` へ持ち上げ、detailsから除く。

## legacy投影の順序

`project(row)` の判定順は、source digest（`source_digest_mismatch`）、oversize（`source_row_too_large`）、`data` がobjectであること、control・catalogの受付、reason要約、data member、重複actor、service executor、envelope全体の検証である。整合性検査の失敗が常に優先する。正直な行がoversizeかつ改変済みになることは無いので、`oversize` と `!source_intact` が同時なら改変（`source_digest_mismatch`）として扱い、`audit.integrity.source_mismatch_detected` の対象にする。`data_kind` はquarantine報告のためのclaim契約の列であり、検証は `data` そのものを見る。

`DocumentStagingProjection` のDebug表示は、`event_id` がUUIDの場合と `event_type` がcatalogのtypeの場合だけ値を出し、それ以外は `<invalid len=N>` と表示する（quarantineされた不正行の列をlogに出さない）。

## adapter_versionの規律とgolden pin

`audit-adapter-golden.json` は次の形である。

```json
{"algorithm": "rust-compact-sorted-sha256", "source_format": "document-audit-outbox-v0",
 "versions": {"1": {"<fixture名>@<入力行hashの先頭16桁>": "<sha256 hex>"}}}
```

- digestは `sha256(AuditEnvelope::to_json_string())`（compact、keyはbyte順）である。Rust投影の固定であり、Storeの `kp-audit-jsonb-sha256-v1`（jsonb textのdigest）ではない。Store側の固定はunit Bの試験（`crates/audit-store-postgres/tests/store_golden.rs`、`tests/data/store-envelope-golden.json`）が行い、entryのkey（Storeが投影する入力行のhash）と判定（`tests/common/mod.rs` の `check_golden`）をこの節と共有する。
- entryのkeyは `<fixture名>@<sha256(入力のclaim行の全列をkey順のcompact JSONにしたもの)の先頭16桁>` である。fixtureの追加・改名・入力の編集は新しいentryの追加になり、既存entryを書き換えない（古いentryは履歴として残す）。
- 試験（`tests/golden_projection.rs`）は次を確認する。
  - 現在の `LEGACY_ADAPTER_VERSION` のsectionがあり、全acceptance fixtureの現在のkeyを含み、digestが一致すること。
  - 現在より新しいversionのsectionが無いこと。entryの形が `<fixture>@<16桁hex>` → sha256 hexであること。
  - 現在より古いsectionは、試験の `FROZEN_SECTION_DIGESTS`（sectionのcanonical JSONのsha256）で凍結されていること。凍結digestの無い旧section、digestの変わった旧section、削除された凍結sectionは失敗する。旧sectionの編集・削除には、この定数の変更という2つ目の目に見える変更が要る。
- 投影の出力（adapterのcodeと、出力に影響するcatalog属性：event_class、details allowlistとkind、持ち上げるfield、reason扱い、subject形、correlation写像）を変えたら、`LEGACY_ADAPTER_VERSION` とcatalogのDocument adapterの `adapter_version` を同時に上げ、新しいversion sectionを追加し、1つ前のsectionのdigestを `FROZEN_SECTION_DIGESTS` に加える。
- 失敗messageは問題のentryだけを示す。新しいkeyはdigest付きで（追加用）、digestの変わったkeyはkeyだけを「projection output changed: bump LEGACY_ADAPTER_VERSION and add a new version section; never edit an existing version's entries」とともに示す（変わったdigestは示さない）。
- `LEGACY_ADAPTER_VERSION` とcatalogのadapter_versionの一致も試験で確認する。

## schema生成とRust⊂schema

```
AUDIT_SCHEMA_BLESS=1 cargo test -p audit-core --test schema_contract   # 再生成（書いた後に必ず失敗する）
cargo test -p audit-core --test schema_contract                        # byte一致の確認
```

`AUDIT_SCHEMA_BLESS=1` はschemaを書き出してから「blessed; rerun without AUDIT_SCHEMA_BLESS」で失敗する（blessした実行が成功扱いにならない）。環境変数 `CI` が設定されている場合はblessを拒否する。

生成schemaはJSON Schema 2020-12で、共有部分を `$defs` に1回だけ定義し、typeごとに `if`/`then` で `$defs/event.<type>` を適用する。provenanceはadapterごとに `$defs/provenance.<source_format>`（`adapter_version` は `const`）である。`correlation.trace_id` は、adapterが許す場合だけpropertyに現れる。

関係は「Rustが受理するものはschemaも受理する」である。試験は次を確認する。

- 全acceptance fixtureと全control fixtureがschemaに適合すること。
- 拒否表のうちschemaも拒否するものには分類を付けず、schemaが受理してしまうものには必ず `rust_only:<category>` を付けること。拒否表はcodeとfield（位置名）の組で比較する。
- 変異試験：accepted fixture（relay・control）の全member・全配列要素について、削除、固定の置換値（null、真偽、0、±1、`i64::MAX`、256、1.5、空文字列、空白付き、Bidi文字、513 byte文字列、nil UUID、別UUID、hex、時刻、`[]`、`[1]`、`{}`）、objectごとの未知member追加を1つずつ適用し、Rustが受理した変異体はすべてschemaも受理すること。

| `rust_only` 分類 | Rustだけが検査する制約 |
|---|---|
| `float_integer` | `2.0`・`2e0` の拒否（JSON Schemaでは整数とみなされる） |
| `utf8_bytes` | byte単位の上限（schemaの `maxLength` は文字数） |
| `principal_charset` | principalの文字規則（Bidi制御、U+2028/2029、BOM、TAG、noncharacter、前後の空白）。`principal_ref` kindのcontrol fieldも同じ |
| `subject_binding` | subjectのplaceholderと `resource` / `details` の一致 |
| `resource_binding` | `bindings` の一致 |
| `correlation_binding` | `correlation` と写像元fieldの一致 |
| `reason_code_binding` | `reason_code` と写像元fieldの一致 |
| `calendar` | 実在しない日付、`legacy_time` のoffset符号混在 |
| `duplicate_key` | 重複key（schemaは解析後の値しか見ない） |
| `origin_path` | 提出経路（relay/store/relay_control）とcatalogのoriginの一致 |
| `int8_range` | `int8_text` のi64範囲（schemaは19桁までの10進表記だけを見る） |
| `db_role_actor` | control eventの `db_role` actorと `session_role` の一致 |

envelope全体の32 KiB（jsonb text）上限もRustだけが検査する。

## control event

`origin: "store"`（source `urn:knowledge-platform:audit-store`）と `origin: "relay_control"`（source `urn:knowledge-platform:audit-relay`）のentryである。type名は `audit.` で始め、resourceは `{"type": "AuditStore", "id": "audit-store"}`、subjectは `audit-store`、`version_id` は持たない。envelopeは `crates/audit-store-postgres` のSQLが組み立て、`provenance` はadapterに従い `{source_format: "audit-store-control-v1" | "audit-relay-control-v1", adapter_version: 1}`（commitment・registrationなし）である。detailsは必ず `session_role`（呼出元の `session_user`、`db_role` kind）を持つ。

- actor：束縛された主体があれば、その（issuer, principal_id）である。束縛された主体が無い場合（`denial_code: unbound` の `audit.access.denied`、owner loginからの `bootstrap_administrator` 等）は、`{issuer: "db_role", principal_id: <session_user>}` とする。Rustは、control経路でissuerが `db_role` のactorについて、principal_idが `db_role` kindで `details.session_role` と等しいことを検査する（不一致は `invalid_actor`）。`db_role` の文法に合わないlogin名（空白、大文字、Bidi文字等を含む引用符付きの名前）は、Storeが `bind_principal`・`posture_check` で拒否し、記録しない（unit Bの責務）。relay eventのactorはproducerのデータなので、この規則は適用しない。
- 読者が渡すfilter（`filter_event_types`、`filter_source`、`filter_actor_issuer`、`filter_actor_principal_id`、`filter_resource_id`）とprincipal値のfield（`target_issuer`、`target_principal_id`）は閉じたkindで記録する。`open_access` は、これらのkindに合わない入力を `audit.access.denied`（`invalid_input`、入力値は記録しない）として扱い。event typeのfilterは、登録済みtypeとcontrol typeに限定する（unit Bの責務）。actorのfilter値は、上限付きのprincipal文字列としてchainに残る（受容した残余）。

| type | origin | class | 主なdetails |
|---|---|---|---|
| `audit.access.intent_opened` | store | DATA_ACCESS | operation、型付きfilter（`filter_seq_after` / `filter_seq_through` を含む）、filter_digest、watermark、page_size、max_pages、include_control、期限、token digest |
| `audit.access.denied` | store | SECURITY | operation（`ingest`、`report_regression`、`declare_recovery_pending` を含む）、denial_code（unbound / insufficient_capability / invalid_input / self_grant / not_source_service）、required_capability |
| `audit.access.closed` | store | DATA_ACCESS | intent seq、返した件数、page数、page digestのdigest |
| `audit.access_policy.changed` | store | ACCESS_POLICY | change（granted / revoked / bound / unbound / bootstrap / reapplied）、対象主体、capability、db_role |
| `audit.retention.policy_changed` | store | CONFIGURATION | policy_id、revision、selector（event type・sourceは各16件まで）、selector_digest、retain_days（NULLまたは1以上） |
| `audit.retention.expired` | store | PRIVILEGED_OPERATION | policy_id、revision、selector snapshot、retain_days、cutoff、effective_cutoff、tx_time、limit、count、first_seq / last_seq（count=0ならnull）、expired_set_digest |
| `audit.retention.expire_refused` | store | PRIVILEGED_OPERATION | policy_id、expected_revision、current_revision、refusal（stale_revision / not_expirable / held）、retain_days（NULLまたは1以上）、cutoff、tx_time |
| `audit.body.purged` | store | PRIVILEGED_OPERATION | target_seq、target_event_id、purge_reason_code |
| `audit.integrity.verified` | store | SYSTEM_AUDIT | trigger（verify / checkpoint）、from/to seq、watermark、checked、head（`head_seq`、`head_epoch`、`head_chain`）、outcome、違反code別件数 |
| `audit.integrity.conflict_detected` | store | SECURITY | event_id、既存seq・origin、conflict_kind、commitment一致の有無、adapter_version |
| `audit.recovery.epoch_started` | store | SYSTEM_AUDIT | old_epoch、new_epoch、restored_head_seq、restored_head_chain、照合checkpoint（epoch/seq/chain、nullable）と分類（match / ahead / store_behind / mismatch / epoch_mismatch）、classification（restore / planned_move / regression）、identity_range_digest、lost_from_seq、lost_upper_seq、lost_upper_known、regressionの証拠（報告seq・event_id・digest・報告者のdb role・時刻・報告時head。nullable）、旧/新fingerprint（system identifier、database oid、timeline。`int8_text`） |
| `audit.delivery.replay_requested` | relay_control | PRIVILEGED_OPERATION | event_id、quarantine_code（replayで解除する旧quarantine code） |
| `audit.reconciliation.completed` | relay_control | SYSTEM_AUDIT | run_id、mode、watermark、id_set_digest、class別件数（`count_<class>`、`replay_record_lost`・`relay_catalog_skew` を含む）、repair件数 |
| `audit.integrity.source_mismatch_detected` | relay_control | SECURITY | event_id、mismatch_code（source_digest_mismatch / actor_mismatch） |

- `checkpoint` は独立したtypeを持たず、`audit.integrity.verified` を `trigger: "checkpoint"` で再利用する（明示的な再利用）。
- `rebind_fingerprint` は設計から削除した。計画的な移動（pg_upgrade、dump/restore移行、promotion）は `begin_recovery_epoch` で扱い、`audit.recovery.epoch_started` に `classification: "planned_move"`、空の消失範囲（`lost_upper_seq = restored_head_seq`、`lost_from_seq = restored_head_seq + 1`）を記録する。
- relay経路（`Origin::Relay`）は、`audit.*` type（field `type`）、control source（field `source`）、`AuditStore` resource（field `data.resource.type`）を `control_type_forbidden` で常に拒否する。control eventはそれぞれ `Origin::Store` / `Origin::RelayControl` の経路でだけ受理する。
- 試験は、control entryごとに受理されるfixture（最小・全field）と、拒否（relay経路、他のcontrol経路、他のsource_format、legacy format、commitment・registrationの付与、`audit-store` 以外のresource id、version_id、Document source、trace_id）を、codeとfieldの組で確認する。Storeの試験は、SQLが生成したすべてのcontrol eventをこの経路で検証する。
- relay control eventのdetailsは `RelayControl::details()`（`session_role` を除く）が作り、試験でcatalogに適合することを確認する。

## Store port（`audit_core::port`）

`AuditStore` traitは、runtime非依存（boxed `Send` future、tokio型なし）でcontent-freeである。

| method | 用途 |
|---|---|
| `ingest(envelope)` | relay-originのenvelopeを保存する。実装は最初に `precheck_ingest(envelope)` を呼び、`envelope.origin()`（検証した経路）が `Relay` でなければround tripせずに `Rejected { control_type_forbidden }` を返す |
| `probe(&ProbeExpectation)` | ingestと同じgate（head lock、書込可否、fingerprint、recovery_pending、posture、ingest主体の束縛、(source, adapter_version, types) の登録、最後にack済みのreceipt identity）を、保存せずに確認する。gateの結果は `StoreStatus` に入り、`Err` はprobe自体の外部障害である |
| `lookup_receipts(event_ids)` / `list_source_receipts(source, after_seq, limit)` / `lookup_control_receipts(seqs)` | content-freeなreceipt（seq、event_id、origin、type、envelope digest、commitment、expired、epoch／control対象event_id、code）。1回あたり最大1000件。adapterは読んだ行を `ReceiptRow::decode(RawReceiptRow)` / `ControlReceiptRow::decode(RawControlReceiptRow)` で検査する |
| `record_relay_control(RelayControl)` | `replay_requested` / `reconciliation.completed` / `source_mismatch_detected` を記録し、（seq, epoch）を返す。source_mismatchは (event_id, code) で冪等 |
| `report_regression(ReceiptIdentity)` | ack済みのreceiptが解決できないことを報告する。StoreはHead lockの下で再確認してからrecovery_pendingを設定する |

receiptとstatusが持つStore由来の文字列は検査済みの型である。typeは `EventTypeName`（catalogの文法、128 byte以下）、control receiptのcodeは `BoundedCode`（`[a-z0-9_]{1,64}`）。decoderは、seq・epochが1未満、未知のorigin、文法外のtype、32 byteでないdigest・commitment、上限外のcode（control receiptではcontrol originと `audit.*` typeも要求）を `Outage { store_other }` とする。列どうしの整合も検査し、食い違う行も `store_other` とする：originとtypeの系統（relayの行は `audit.*` でなく、controlの行は `audit.*`。このcrateのcatalogにあるtypeはcatalogのorigin）、controlの行はcommitmentを持たず失効しない、control receiptでは `audit.delivery.replay_requested` と `audit.integrity.source_mismatch_detected` が対象event_idとcodeを持ち、`audit.reconciliation.completed` が対象を持たずmode（`read_only` / `repair`）をcodeに持ち、それ以外のcontrol typeはcodeを持たない。未検査の行（`RawReceiptRow` 等）のDebugは検査に通った値だけを表示する。

`StoreStatus` は head_seq、recovery_epoch、`state`（`Operational` / `RecoveryMode`（fingerprint不一致かrecovery_pending、`store_recovery_required`）/ `PostureInvalid`（`store_posture_invalid`）/ `ReadOnly`（`store_read_only`））、missing_types（`EventTypeName`。文法外の名前はadapterが `store_other` にする）、regression_detected、last_verified_seq を持つ。`admission()` は、state、regression（`store_regressed`）、未登録type（`store_unregistered_type`）の順に判定する。

### 失敗の分類（設計§6.3、二分類）

- Terminal（quarantine）：`StoreError::Conflict { .. }` と `StoreError::Rejected { code, .. }` だけである。両variantは `port` moduleの外では作れない印 `Verdict` を持ち、crateの外ではingestの構造化された結果行（`IngestRow::into_result`）と、ingest前のorigin検査（`precheck_ingest`）からだけ得られる。この印は、terminalを生む経路をdecoderに集めるための慣行の補助である。`IngestRow` のfieldはpublicなので、adapterが誤って例外経路から `IngestRow` を作ればterminalになり得る。adapterは `IngestRow` を `audit_store.ingest` の結果列からだけ作る。これはunit Bの試験（全SQLSTATE・通信エラーがOutageになること）とreviewで担保する。外部から `Verdict` を作れないことは、compile_fail例で確認する。
- Outage（試行を返却して保留）：`StoreError::Outage { code }`。それ以外のすべて。`is_outage() == !is_terminal()` を全variantで試験する。

`classify_sqlstate` は全域関数で、どの入力もoutageになる。

| SQLSTATE | OutageCode |
|---|---|
| `25006` | `store_read_only` |
| `57014` | `store_timeout` |
| `57P..`（57P01–57P05と将来のcode） | `store_shutdown` |
| `40001` / `40P01` / `55P03` | `store_serialization` / `store_deadlock` / `store_lock_unavailable` |
| `08...` / `53...` | `store_connection` / `store_resources` |
| `42...` | `store_deploy_mismatch` |
| `58...` / `XX...` / `54...` | `store_internal` |
| それ以外（形式不正・空を含む） | `store_other` |

このほかのOutageCodeは `store_transport`、`store_outcome_unknown`、`store_recovery_required`、`store_regressed`、`store_posture_invalid`、`store_unregistered_type`、`store_denied`（ingestのloginが登録済みsource serviceの主体でない）である。全5文字 `[0-9A-Z]` のSQLSTATEがterminalにならないことを網羅試験で確認する。

`IngestRow { status, seq, envelope_digest, adapter_version, code }` の解釈：

| status | 結果 |
|---|---|
| `stored` / `duplicate` / `duplicate_expired` / `duplicate_reprojected` | receipt。seq（≥1）、32 byteのdigest、adapter_version（≥1）が欠けている、またはcodeを持つ行は不正な結果行として `store_other`（streakに数える） |
| `conflict` | `Conflict` |
| `rejected` | `Rejected { code }`。codeが無い・`[a-z0-9_]{1,64}` でなければ `store_other` |
| `recovery_required` | `store_recovery_required` |
| `outage` | codeのOutageCode（`store_` 接頭辞の有無を問わない。`unregistered_type` → `store_unregistered_type`）、不明なら `store_other` |
| `denied` | `store_denied` |
| その他 | `store_other` |

`OutageCode::counts_toward_outage_streak()` は `store_internal` と `store_other`（残余の予期しない失敗。ingest・receiptの不正な結果行を含む）だけが真である。gateの結果、既知のSQLSTATE class、transport、timeout、結果不明はStore全体の状態なので `outage_streak` に数えない。`store_outcome_unknown` はtransport層の不明（文を送った後、結果を受け取る前に接続が切れた等）に限り、届いた結果行には使わない。不正なreceiptを返し続けるStoreの不具合は、`store_other` としてstreakで有界になる。

## hash chain

- `envelope_digest = sha256(jsonb::text のbyte列)`（`kp-audit-jsonb-sha256-v1`）
- `chain = sha256("kp-audit-chain-v1" || prev_chain || int8send(seq) || uuid_send(event_id) || envelope_digest)`
- `GENESIS = sha256("kp-audit-chain-genesis-v1")`。seq 1の `prev_chain` である。
- `expired_set_digest = sha256("kp-audit-expired-set-v1" || int8send(seq_1) || … || int8send(seq_n))`。`audit.retention.expired` が印を付けたseqを昇順に連結する（空集合は接頭辞だけのhash）。SQLでは `sha256('kp-audit-expired-set-v1'::bytea || coalesce(string_agg(int8send(seq), ''::bytea ORDER BY seq), ''::bytea))` である。

SQL実装と照合するための固定vector（`crates/audit-core/tests/chain_export.rs`）：

| 値 | hex |
|---|---|
| `GENESIS` | `9ae4e1d7942ce318770de897b2a336edc2a9e700f8733f658dcdc2e563aff344` |
| `envelope_digest('{"example":"envelope"}')` | `c147ac99fc21ba6cc69a47812b1bb2415e353995e22b65d6a2b58a5222dd5f6f` |
| seq 1、event `0199a1b2-0000-7000-8000-000000000001`、上のdigest | `babbd336ec2c8556082424a253afb6d8e88ef5a63c9cbcab6cea8f441ad528e1` |
| seq 2、event `0199a1b2-0000-7000-8000-000000000002`、digest = `0x11` × 32 | `1496cf0e490216c2ca995db8ac0a340562a691cf1d2f955865624900b4856106` |
| `expired_set_digest([])` | `156d594bbdfb392f0b9d899bf2342920d15ce007185b736b7323675c4eed4582` |
| `expired_set_digest([3, 5, 9])` | `66efae7170a5138bff29bb1c633cd75e5eb7345491ca9c354aea98c0e08a45e6` |

## export検証

export行は次の10 keyを持つ（閉じた集合、重複key不可）。

```
{"seq","event_id","origin","envelope_digest","prev_chain","chain","recovery_epoch","expired","expired_by_seq","envelope"}
```

`envelope` はStoreの `jsonb::text` の原文（失効済み・identity chainでは `null`）で、audit-coreはRawValueで原文を保持してhashする。`expired_by_seq` は本文を消したcontrol eventのseqで、`expired = true` ⇔ `expired_by_seq` が非null（identity chainの行も同じ）。SQLは `e.expired_by_seq` をそのまま出力する。`origin`、`recovery_epoch`、`expired`、`expired_by_seq` はchainの対象外なので、本文付きのanchor検証でだけ、chainに入った本文で裏付ける。

- `verify_export(text, Anchor)`：genesisまたは信頼済みcheckpointから、次を検査する。
  - seqの連続性（seqの加算は `checked_add`。overflowは `Malformed`）、`prev_chain` の連鎖、chainの再計算、本文digest、`envelope.id = event_id`、本文の有無と `expired` の整合。
  - origin：本文のtypeがcatalogにあれば、そのoriginと一致し、本文のsourceがcatalogのsourceと一致する（store → audit-store、relay_control → audit-relay）。catalogに無いtypeは構造規則（`audit.*` typeと専用source）で判定する。不一致は `OriginMismatch`。
  - 失効：origin=relay以外の行の失効は `ExpiryOnControlEvent`。`expired_by_seq` は自分のseqより大きい（そうでなければ `ExpiryEvidenceMissing`）。参照先がexport内なら、そのseqはorigin=storeの本文付き `audit.retention.expired` か `audit.body.purged` でなければならない（そうでなければ `ExpiryEvidenceMissing`）。
    - retention：参照する行の集合について、count、first_seq、last_seq、`expired_set_digest` が一致する（不一致は `ExpiryEvidenceMismatch`）。参照されないretention行も空集合と照合する。first_seqがanchor以前の場合は、見えている行が範囲内に収まることだけを確認し、未検証として数える。
    - purge：target_seqの1行だけが参照し、その `event_id` が `target_event_id` と一致する。
    - 参照先がexportの範囲外の場合は、黙って通さず `ExportReport.unverified_expiry_evidence` に数える。
    - 失効行はすべて `ExportReport::expired_rows()`（`ExpiredRowEvidence { seq, evidence_seq, verified }`、seq順）に列挙する。`assess_recovery` はこの `evidence_seq` をcheckpointと比べる。
    - DB外の失効検証が示すのは集合の整合（件数、seq範囲、`expired_set_digest`、purgeの対象）だけであり、retentionの適格性（selector、cutoff、policy）ではない。selector・cutoff・identity列はexportにもchainにも入っていない。
  - epoch：減少は `EpochRegressed`、+1以外の増加は `EpochSkipped`。増加はorigin=storeの本文付き `audit.recovery.epoch_started` の行で起き、そのdetailsが次を満たさなければならない（満たさなければ `UnattestedEpochChange`）。
    - `old_epoch` / `new_epoch` が列の値と一致する。
    - `0 ≤ restored_head_seq <` その行のseq、`lost_from_seq = restored_head_seq + 1`、`lost_upper_seq ≥ restored_head_seq`、`classification` が restore / planned_move / regression のいずれか。
    - 検証経路上にあれば `restored_head_chain` がそのseqのchainと一致する。`restored_head_seq` がanchorより前の場合は比較できないので、黙って通さず `ExportReport.unverified_restored_heads` に数える。
    - 遷移は `EpochTransition { seq, old_epoch, new_epoch, attestation }` として列挙し、`attestation`（`EpochAttestation`）に本文の restored_head_seq、restored_head_chain、lost_from_seq、lost_upper_seq、classification を保持する。この場合 `epochs_authenticated = true`。
    - epochが変わらない行にorigin=storeの `audit.recovery.epoch_started` があれば `EpochStartedWithoutTransition` とする（chainの対象外のepoch列を旧epochへ書き換えてrecoveryを隠すことを防ぐ）。
- `verify_export_complete(text, Anchor, watermark)`：manifestのwatermark Wまでの完全な本文付きexportを検証する。`verify_export` の検査に加えて、headがちょうどW（そうでなければ `WatermarkMismatch`）で、`expired_by_seq` がWを超える行が無い（あれば `ExpiryEvidenceMissing`）ことを要求する。完全なexportは参照する証拠をすべて含むので、失効したcontrol行のoriginをrelayへ書き換える偽装を閉じる（証拠はretention・purgeの本文でなければならず、それらはrelay行だけを失効させる）。
- `verify_identity_chain(text, Anchor)`：本文の無いidentity chainを同様に検査する（seq、chain、epochの単調性と+1、失効行の `expired_by_seq`）。本文が無いので、epochは `epochs_authenticated = false`（遷移の `attestation` は `None`）、失効はすべて `unverified_expiry_evidence` に数える。`verify_identity_chain_complete(text, Anchor, watermark)` は、`verify_export_complete` と同じくheadがちょうどWで、Wを超える `expired_by_seq` が無いことも要求する。
- `ChainIntegrity::of(&検証結果)`：chain自体について確立したことの区分で、真正性の主張ではない（真正性は `assess_recovery` がcheckpointから判定する）。`Intact`（anchorから連続し、seq・prev_chainの連鎖・chainの再計算がすべて一致。identity chainではepochと失効は未証明のまま）、`Broken(ExportError)`（欠落・入替・書換え・連鎖切れ・epoch規則違反・不正な行。最初の行を示す）、`Unanchored`（filter付きの部分集合。chainについて何も示さない）。`audit-admin` はexportのmanifestと検証失敗時の出力に `chain_integrity`（`intact` / `broken` / `unanchored`）を出す。
- `verify_export_subset(text)`：filter付きexportの行単位の整合だけを見る。結果は常に `anchored: false` であり、真正性の根拠にならない。
- `ExportReport` は `epoch_transitions()`、`expired_rows()`、`epoch_at(seq)`、`chain_at(seq)` を持つ。
- `compare_checkpoint(report, checkpoint)`：`Match` / `Mismatch` / `EpochMismatch` / `StoreBehind` / `Ahead` / `BeforeAnchor` / `Unanchored`。checkpointは（epoch, seq, chain）で、`checkpoint.epoch` はそのseqの行の `recovery_epoch`（取得時の `publication_head.recovery_epoch`）である。chainは一致するがepochが異なる場合は、書換えではなく `EpochMismatch`（epoch列または帯域外記録の改変）とする。`Ahead` はそのcheckpointまでしか真正性を示さない。前方部分（`seq_through`）を検証する場合は、export headまでのcheckpointだけを渡す（それより後は `StoreBehind`）。

## recovery epochのDB外判定

epochの規則：epoch 1から始まり、増加は常に+1で、`audit.recovery.epoch_started` の行（新しいepochの最初の行、通常は復元head+1）で起きる。

`assess_recovery(report, checkpoints, records)` は、anchor付きで検証したexport（identity chainを含む）を、帯域外のcheckpointと遷移記録（`RecoveryRecord { old_epoch, new_epoch, restored_head_seq, restored_head_chain, lost_upper }`）で判定する（設計§8）。`lost_upper = restored_head_seq` は消失の無い計画的な移動である。

記録は、次をすべて満たす場合に遷移を説明する。

- epochが一致し、`0 ≤ restored_head_seq <` 遷移のseq。
- 復元headと遷移の間の行が消失範囲に入る（`遷移のseq - 1 ≤ lost_upper`）。計画的な移動では、遷移のseqが `restored_head_seq + 1` に限られる。
- 本文付きexportでは、記録の restored_head_seq、restored_head_chain、lost_upper が `epoch_started` 本文の値と等しく、本文の分類が `planned_move` なら消失範囲が空である。
- 復元headが検証経路上にあれば、そのchainが一致する。

一致しない記録は `UnverifiedRecovery` になる。ただし、anchorより前のrecoveryの記録（`new_epoch ≤ anchor.epoch`）は `records_before_anchor` に、検証したheadより後のrecoveryの記録（`old_epoch ≥ head.epoch`。その遷移は経路の終端より後にあり、例えばより後のrestoreより前に取ったexport）は `records_after_head` に列挙するだけで、判定に影響しない（帯域外の記録全体を渡してcheckpointから検証できる）。経路が通るepochの遷移の記録は、exportの遷移と一致しなければならない。記録された遷移を消す巻戻しは、その遷移より後に取ったcheckpoint（`StoreBehind` / `Mismatch` / `EpochMismatch`）で検出する。説明された遷移でも、復元headがanchorより前でchainを比較できなければ `EpochReview.restored_head_verified = false` とし、`UnverifiedRecovery` とする（より前のanchorから検証すれば確認できる）。

真正性は、headと一致する帯域外checkpoint（`Match`）がある場合だけ主張する。`authenticated_through` は `Match` または `Ahead` のcheckpointの最大seqである。anchorより前のcheckpoint（`BeforeAnchor`）と、anchorと同じseqのcheckpoint（比較結果は事実どおり `findings` に残す）は中立で、判定にも `authenticated_through` にも影響しない。anchorは前提として信頼する起点なので、同じ位置のcheckpointはexportの行について何も確認せず、anchorと食い違う場合も帯域外の入力どうしの食い違いであってexportの改ざんの証拠ではない（`Tampered` にしない）。

| verdict（良い順） | 意味 |
|---|---|
| `Authentic` | anchorから連続し、headが帯域外checkpoint（seq、chain、epoch）と一致し、消失を伴うrecoveryが無く（計画的な移動は記録があれば含めてよい）、認証範囲内の全失効行の証拠が認証範囲内で検証済み |
| `AuthenticThrough { seq }` | 帯域外checkpointが経路を `seq`（headより前）までしか確認しない。`seq` より後の行は認証されない（chainは公開のsha256なので、exportを持つ誰でも延長できる） |
| `UnverifiedExpiry` | 経路は確認されたが、認証範囲内の行の本文削除が確認できない（証拠が `authenticated_through` より後・exportの外・anchorより前の集合にある、または本文の無いexport）。件数は `unconfirmed_expiries` |
| `NoCheckpoint` | 経路を確認する帯域外checkpointが無い（真正とは主張しない） |
| `Lost` | 差異が帯域外に記録されたrecoveryの消失範囲（restored_head_seq, lost_upper]だけで説明できる（authenticとはしない） |
| `UnverifiedRecovery` | recovery epochに帯域外記録が無い、記録・本文と食い違う、復元headを確認できない、またはcheckpointのepochだけが異なる（改変の疑い） |
| `Tampered` | 復元head以下、またはrecoveryの無い位置で帯域外checkpointと食い違う |
| `Unanchored` | filter付きの部分集合 |

`unverified_expiry_evidence > 0` のreportは決して `Authentic` にならない（設計§8:457は範囲外の証拠を件数として報告する規則であり、それを判定にも反映する方針は本trackの修正で採用した。実行状況に記録）。真正性を検証するには、headが帯域外checkpointと一致するexportを使う。checkpointまでを出力する（`seq_through = checkpoint.seq`、設計§10.3）か、checkpointをwatermark Wで取った場合はWまでの完全なexportを `verify_export_complete` で検証する。checkpointより後に証拠がある失効は、checkpointまでのexportでは `unconfirmed_expiries` に数える（後の証拠は次のcheckpointで確認する）。

全recovery epochと消失範囲は `epochs` に列挙され、人の確認対象になる。

## 拒否code

`envelope_too_large`、`invalid_json`、`duplicate_key`、`invalid_envelope`、`unknown_event_type`、`control_type_forbidden`、`invalid_source`、`invalid_subject`、`invalid_resource`、`nil_client_id`、`invalid_result`、`invalid_actor`、`invalid_service_executor`、`unknown_field`、`missing_field`、`invalid_field`、`invalid_correlation`、`invalid_source_correlation`、`invalid_reason`、`reason_not_string`、`actor_mismatch`、`source_row_too_large`、`source_digest_mismatch`、`invalid_provenance`、`invalid_extensions`。拒否は、codeと、catalogのfield名または固定の位置名（`type`、`source`、`data.resource.type`、`data.resource.id` 等）だけを持つ。payloadの値は含めない。

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
