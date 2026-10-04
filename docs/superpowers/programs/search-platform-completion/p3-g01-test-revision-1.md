# P3-G01 改訂 interface の検証計画 1

Status: **test design only / 未実行**。`p3-g01-test-proposal.md` を置き換える。G01 の SQLx-free compile gate と、G03/P7/G04/G06/G07/G08 の実 PostgreSQL gate を混同しない。P3-P04 backend 資格、Graph READY/publish、production 実装、RED/GREEN の達成は本書から主張しない。P4 の App/Cargo 専有 slot と P3 の backend 選定・host storage floor を守る。

## 1. G01: SQLx-free compile/type gate のみ

対象は `crates/search-application/tests/graph_generation_port.rs` と公開 API の compile-fail docs。`search-application` 外の integration test crate から以下を**実際に型検査**する。`BoxFuture` は `Result<T,SearchError>` を一度だけ包む。

| named case | assertion |
| --- | --- |
| `all_graph_ports_compile_without_sqlx` | pure in-test adapter が `DurableGraphGenerationPort` の full/copy/verify/delta/validate/recover、`GraphSourceMappingValidatorPort`、`GraphLeaseVerifierPort`、`GenerationScopedGraphAccessPort`、`PinnedGraphRetrievalPort` の全 method を実装し、`GraphBuildRef`、`GraphStageReport`、`GraphReadLease` と既存 P4 `AuthorizedSourceScope` / `TrustedDiscoveryBinding` を使用する。App の `Cargo.toml`/exports に `sqlx`, `PgPool`, `PgConnection` は不要。 |
| `public_reference_is_constructible_but_not_a_grant` | 外部 test crate が `RegisteredFullBuildHandle::from_identifiers`、`BuildGuardHandle::from_identifiers`、`GraphReadLease::from_identifiers` を任意 UUID から構築できることを positive compile で示す。これらは identity の round-trip だけを検査し、admission/READY/pin の成功 assertion を置かない。 |
| `raw_layout_and_old_authority_api_are_absent` | compile-fail examples で外部 struct literal/private field 書換え、旧 `TrustedGraphRegistrationHostPort`/`GraphRegistrationIssuer`/`GraphReadLeaseIssuer`、旧 autonomous `stage_full(manifest,retention,mapping_digest,...)`、旧 `validate_ready`、caller `expected_digest` 付き mapping validator の呼出しを E0432/E0599/E0451 等で拒否する。production trait に parent INSERT や READY promotion method が存在しないことを確認する。 |
| `graph_record_shape_keeps_frozen_meaning` | `GraphSourceMapping`、`GraphResourceRecord`、`GraphIncrementalDelta`、closure proof、receipt の Source/key/n-ary/temporal/owner field を型検査し、`GraphSourceMappingReceipt` の digest が `graph_schema_version` と別 field であることを確認する。 |

初回 RED は未実装の G01 module/port による compile failure として保存し、G01 実装後に同じ command を GREEN にする。source grep や `Cargo.toml` の目視だけを compile 証拠にしない。純粋 fake adapter が返す `OperationFailed` は型境界の確認だけで、DB admission を試験したことにはならない。

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 \
  cargo test -p search-application --locked --test graph_generation_port -- --test-threads=1
```

## 2. 実 PostgreSQL: public/fake ID と full 登録・retention

G03/P7-07/P7-06/G04 の選定済み backend gate。disposable PG、実 `search_registration` / `search_builder` / `search_coordinator` / `search_reader` role、別接続を使う。各失敗で P7/Graph parent、guard、子行、cursor、pointer/receipt の commit 前後を検査する。`issue_full` に storage call がないという空の assertion は置かない。

| named case | fault injection と期待結果 |
| --- | --- |
| `forged_public_full_reference_cannot_write_children` | public constructor で random key/token/fence を作り、実 builder stage に渡す。保存 target/Graph parent/guard がないため `FenceLost`、子行 0。既存別 target の token/fence を移した ref も拒否。direct SQL child INSERT も trigger/role で拒否。 |
| `nil_or_cross_source_full_reference_fails_closed` | nil SourceId、nil generation、nil token、0 fence、`key.source_id` と保存 target Source の相違を入れる。内部形不正は `OperationFailed`、保存 row とずれる stale ref は `FenceLost`、子行 0。 |
| `full_registration_is_one_commit_with_guard_and_graph_key` | 同じ登録 transaction の Graph INSERT/guard INSERT に別々の fault を注入し、P7 target/Graph parent/identity/guard が部分 commit されないことを別接続で確認。別接続で親を登録する旧経路は存在しない。 |
| `wrong_activation_snapshot_manifest_or_build_kind_rejected` | 旧 activation、異なる Source snapshot/manifest、FULL↔INCREMENTAL kind の偽 ref、Graph/P7 親不一致を別々に注入。stale activation は `FenceLost`、保存 row 同士の矛盾は `OperationFailed`、子行 0。 |
| `expired_full_guard_rejects_late_child_write_and_ready` | batch 開始時と commit 直前に DB clock を進める。expiry 後の通常 stage/直接 child SQL/P7-08 READY を拒否し rollback。失効 guard を同 target に再発行しない。 |
| `retention_grant_is_checked_from_current_source` | `SessionOnly`、`NoRetention`、`CacheWithExpiry`、retention 不明/失効をそれぞれ拒否し durable 子行 0。`PersistentResource` の許可 field と、`PersistentDiscoveryMetadata` の metadata 限定を正例にする。metadata grant から本文 Unit/embedding を保存しようとする負例を含める。request DTO の retention 値は判定に使わない。 |
| `registration_host_error_propagates_without_child_write` | Source/ownership host の `SourceUnavailable` と内部 `OperationFailed` を別々に注入し、variant と ID-free message を保持して child/pointer/receipt を書かない。DB commit 応答不明は `CompletionUnknown` とし、成功に読み替えない。 |

`SourceUnavailable` は現在 Source/retention が durable build を許さない場合、`FenceLost` は stale/expired target と guard、`OperationFailed` は内部 row/role/shape の不整合に用いる。host が返す `SearchError` は variant のまま伝播する。connector failure を synthetic `InvalidRequest` に変換しない。実 role 不足は試験不備として区別し、権限が十分な fixture だけで positive を判定する。

## 3. 実 PostgreSQL: incremental、mapping、READY 非昇格

| owner / named case | fault injection と期待結果 |
| --- | --- |
| G03/G06 `forged_incremental_base_or_target_cannot_copy` | 別 Source base/target、base=target、別 target token/fence、存在しない base、未 READY base、FAILED target、旧 activation target を試す。copy 前に拒否し、base/target/guard の保存 binding と count が不変。 |
| G06 `stale_base_receipt_guard_and_cursor_roll_back` | base manifest/snapshot/mapping/content digest/count の一欄ずつ変更、期限切れ/旧 guard、copy phase・delta phase・sequence の forged cursor、committed cursor 先行を注入。毎 batch と commit 直前に DB 行を再読し、wrong cursor は `OperationFailed`、guard 失効は `FenceLost`。partial copy を成功扱いせず、検証不能なら新 key で再構築。pointer revision の前進だけでは有効 guard を失効させない。 |
| G06/G08 `unproven_relation_closure_requires_full_rebuild` | resource 不変の relation-only same-ID participant/qualifier 変更、旧/新 relation ID 欠落、partial Source proof を試す。旧 incidence 除去と n-ary attachment 一致を確認し、証明不能な delta を READY にしない。 |
| G04/G08 `mapping_receipt_is_not_source_authority` | public fake validator が任意 `GraphSourceMappingReceipt` を返しても、登録済み Source validator と保存 rows からの owner/kind/native ID/Source/key/snapshot/mapping digest 再計算に失敗させる。Document/FolderPlacement owner 入替、Knowledge Version 対応欠落、`TemporalProjection.resource_ref` 相違も拒否。 |
| P7-08 `graph_stage_report_cannot_promote_ready` | `validate_staged` の report だけ、Graph 単独 READY、P7 片側 READY、wrong graph schema version、P1 staged-input digest と P3 content digest の取り違えを拒否。P7-08 が同一接続で P1/P3 両 encoder・全 rows/receipt/guard/expiry を再検査した時だけ READY。旧 autonomous `validate_ready`/親 `stage_full` は production composition/role から不可。 |
| G08 `recover_ready_is_read_only_and_fails_corrupt_row` | 保存 manifest/mapping/content/owner/participant/count を一欄ずつ破損・欠落させ、`recover_ready` は Graph/P7/pointer を変更せず error。旧 key を別 key に rebind しない。 |

## 4. 実 PostgreSQL: read lease・現在 actor/Source

G07/P7-10 は `retrieve_pinned` に G01 `GraphReadLease`、`TrustedDiscoveryBinding`、P4 `AuthorizedSourceScope` を渡し、read 前と return 前の双方を観測する。production reader は P7 wrapper の concrete `GraphLeaseVerifierPort` と現在 Source access port を固定し、caller が fake verifier/access port を引数で差し込めないことを確認する。verifier fake の成功だけを read admission の正例にしない。

| named case | fault injection と期待結果 |
| --- | --- |
| `forged_or_foreign_read_lease_cannot_retrieve` | public constructor の random/nil lease、同 lease ID の別 Source/key/evaluation、別 tenant/actor/session、wrong host scope reference を投入。DB row・current host scope に合わず結果全体を拒否。 |
| `expired_or_revoked_scope_returns_no_partial_hits` | read 前/途中/return 前の DB expiry、ownership tombstone、activation・registration/visibility/access revision 変更、host reference の restart 後解決不能、Source/actor current gate Denied/Unknown を一件ずつ試す。partial hits を返さず、return 前は新 transaction の DB clock を用いる。 |
| `old_key_pin_survives_pointer_advance_only` | 有効 lease と同一保存 key/二 digest/current actor/Source authority があれば pointer だけ新 key に進んでも旧 key read を許す。旧 key が GC/破損、scope が失効、digest が違う場合は暗黙転送せず拒否。 |
| `read_role_and_current_access_are_enforced` | `search_reader` の直接 DML 拒否、全 participant の P5 final gate と Document current Version/Part/raw/Read 再評価、hidden relation を visible budget に算入しないことを確認。 |
| `read_host_error_propagates_and_closes_result` | P4 `CurrentSourceVisibilityPort` の `OperationFailed("trusted scope unavailable")`、host scope reference 解決時の `SourceUnavailable`、P5 current gate の error を個別に注入し、variant を保存して全結果を閉じる。fake verifier だけを渡す公開 bypass がないことも確認。 |

`InvalidRequest` は公開 plan/budget/limit に限り、stale/expired lease は `FenceLost`、現在 Source/actor 利用不可は `SourceUnavailable`、保存 row 矛盾・wrong internal scope 構造は `OperationFailed`。P4 の `OperationFailed("trusted scope unavailable")` と他 host error を上書きしない。read 失敗は常に結果全体を閉じ、外部への ID-free mapping は P5 の別 gate で確認する。

## 5. 実行・証拠境界

G01 は上記 App focused compile test の RED→GREEN だけを持つ。G03/P7-07 は schema/role/registration、G04/G06/G08 は Graph 子行・mapping・recovery、P7-08 は READY、G07/P7-10 は read/pin の各 named 実 DB gate を担当する。実 DB では同一 role と別 role の独立接続、必要な barrier/fault injection、SQLSTATE と transaction rollback、DB clock を記録する。各 gate は対応 code と migration があり、P3-P04 が backend を選定し storage floor/専有 slot が解放された後にだけ実行する。成功値は局所 gate に限定し、backend qualification・P7 runtime/HTTP・exact-head CI に流用しない。
