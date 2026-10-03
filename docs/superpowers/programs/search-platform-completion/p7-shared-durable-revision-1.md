# P7 共有 durable generation 基盤 — 設計改訂 1

Status: **DRAFT / 独立再審査・実装計画反映・実 DB 検証前**（2026-09-30）。[元設計](p7-shared-durable-design.md) SHA-256 `99a989e30ec77ea5f79915a3db22ecd31b910decbabc825c2ae8bca6f1041a3b` は保存し、[独立レビュー](p7-shared-durable-review.md)の 5 件だけを本書で補う。抵触時は本書を元設計 §§2–5 の該当条項に優先する。P1/P3/P6 Freeze、P3 build guard と FK-safe cleanup、P1 Archive binding は引き続き拘束する。これは production code、Graph backend 選定、P6 全縦断、P7 最終 runtime/HTTP/運用の完了記録ではない。

| レビュー指摘 | 本改訂の拘束箇所 |
| --- | --- |
| 1. event/Source epoch に束縛されない READY 候補 | §1 の `stage_origin`、完了 transaction、`ReuseCurrent` |
| 2. full guard と target の不変束縛・失効 fence 不足 | §2 の target binding、DB DML fence、cleanup |
| 3. P3 `stage_full` の別 commit 経路 | §3 の transaction-bound 登録と production port |
| 4. durable pin の owner scope 不在 | §4 の不変 scope reference と current gate |
| 5. Source kind と Remote desired 集合の権威 | §5 の kind 制約、host snapshot、global serial lock |

## 1. Event candidate origin と完了条件

`search_generation` の登録時に `stage_origin IN ('EVENT','MANUAL')` を固定し、`stage_event_id` と `stage_source_epoch` は **EVENT の場合だけ両方 NOT NULL**、MANUAL の場合は両方 NULL とする。`stage_source_epoch > 0`、`stage_origin`/event ID/epoch は INSERT 後不変であり、builder role に更新権限を与えない。`source_id`、generation key、`source_snapshot`、`activation_epoch`、manifest/key/guard binding も同じ登録 transaction で固定する。EVENT 登録は private `EventCandidateHandle` と P6 の `SearchDeliveryFence` を要求し、locked outbox event の configured Source route、event ID、現行 Source owner token/epoch/DB expiry を照合してから行う。MANUAL 登録は別の private `ManualBuildHandle`/port とし、outbox row を取らず、EVENT handle に変換できない。いずれの handle も SQLx-free application の opaque 値であり、HTTP/provider/通常 builder が任意値から生成できない。

`0002` の `search_generation` 生成 SQL は少なくとも次の complete CHECK を含む。既存の `stage_event_id?`/`stage_source_epoch?` を単なる任意列のまま残さない。

```sql
stage_origin text NOT NULL CHECK (stage_origin IN ('EVENT','MANUAL')),
CONSTRAINT stage_origin_complete CHECK (
  (stage_origin = 'EVENT' AND stage_event_id IS NOT NULL
    AND stage_source_epoch IS NOT NULL AND stage_source_epoch > 0)
  OR (stage_origin = 'MANUAL' AND stage_event_id IS NULL
    AND stage_source_epoch IS NULL)
)
```

`CompleteEventRequest` は概念上 `PublishCandidate { event_candidate_handle, expected_current, verified_bundle }` と `ReuseCurrent { expected_current, expected_manifest_digest, expected_bundle_digest }` に分ける。元 P6-S01 の単一 `candidate` field を production の無条件入力として流用しない。`PublishCandidate` は outbox row → Source row → P7/Graph generation key 順 → guard → lease → receipt の既定 lock 順を守る **一つの短い PostgreSQL transaction** で次を再検査する。

1. Outbox lease token/DB expiry/未完了、Source owner token/`fence_epoch`/DB expiry、event route の `SourceId`、ownership ACTIVE/tenant/activation を照合する。Source lease の `epoch` と outbox `event_id` は DB 行から取得し、呼出し側の handle だけを信用しない。
2. `search_generation.stage_origin='EVENT'`、`stage_event_id=outbox.event_id`、`stage_source_epoch=Source.fence_epoch`、candidate Source/key、保存 `source_snapshot`、P1 manifest・全 payload・bundle receipt の snapshot/key、`activation_epoch=Source/ownership/identity` を完全一致させる。full または incremental の **その target** に発行した token/fence が保存 target binding と guard row に一致し、DB clock で未失効であることも照合する。外部 lexical preflight 後も DB 内の READY/Graph READY、immutable artifact binding を再読する。異なる event、古い epoch、MANUAL origin、別 Source/activation/snapshot/guard は pointer・receipt を一切書かず `Lost` または明示 integrity failure とする。
3. expected current の key/二 digest/revision、`last_published_epoch <= epoch` を条件付き CAS し、同 commit で `(source_id,event_id)` receipt を P6 の epoch 単調・同 epoch 完全一致規則で書く。CAS 成功時に該当 guard のみを DELETE する。失敗時は rollback して guard を維持する。generic `delivered_at` は P6 の後続 fenced ack にだけ委ねる。

`ReuseCurrent` は **候補 stage を受け取らない**。同じ outbox/Source fence の transaction で、その時点の Source current pointer が `expected_current` と key・manifest/bundle digest・revision で一致し、同一 Source/activation の P7 READY、P3 READY、P1 bundle と外部 artifact が検証可能である場合だけ、その current key を receipt に記録する。MANUAL key を任意候補として渡して公開する経路はない。元の historical receipt や GC 済み key は証拠にならず、current READY が消えた・別 activation に属する・artifact 不可用なら `Unchanged`/`Duplicate` を返さず再読/再構築へ進める。manual rebuild 自体は pending event を ack しない。`ReuseCurrent` に full/incremental guard は要求しないが、現在の READY と Source 正本の current access/retention gate は省かない。

DB は `stage_origin` の allowlist と event/epoch complete-or-null CHECK、immutable trigger、registration/completion 専用 role で上記経路を裏付ける。`PublishCandidate` の再検査と pointer/receipt 書込みを別接続・別 transaction に分けない。commit 応答不明は元設計の `CompletionUnknown` であり成功に変換しない。

## 2. Full target の不変 guard binding と失効 fence

P7 `search_generation` に `build_kind IN ('FULL','INCREMENTAL')`、`full_guard_token UUID`、`full_build_fence BIGINT` を追加する。FULL は token/fence が両方非 NULL かつ fence > 0、INCREMENTAL は両方 NULL とする complete CHECK を設ける。FULL 行の `(source_id,generation_id,full_guard_token,full_build_fence)` に UNIQUE、`full_guard_token` と `(source_id,full_build_fence)` に一意制約を置き、`search_generation_full_guard(source_id,target_generation_id,guard_token,build_fence)` から target の 4 列への `ON DELETE RESTRICT` 複合 FK を張る。guard が別 target の token/fence を借りることを許さない。target の build kind/token/fence は登録後、FAILED/DELETING/READY を含め不変である。P3 incremental の target/base binding と同じ Source `build_fence_seq` を Source row lock 下で一度だけ増やす。SQL の CHECK/FK だけでは時刻を保証しないため、以下を DB trigger、role、private port の三層で強制する。

```sql
CONSTRAINT full_binding_complete CHECK (
  (build_kind = 'FULL' AND full_guard_token IS NOT NULL
    AND full_build_fence IS NOT NULL AND full_build_fence > 0)
  OR (build_kind = 'INCREMENTAL' AND full_guard_token IS NULL
    AND full_build_fence IS NULL)
),
FOREIGN KEY (source_id,target_generation_id,guard_token,build_fence)
  REFERENCES search_generation
    (source_id,generation_id,full_guard_token,full_build_fence)
  ON DELETE RESTRICT
```

後段の複合 FK は `search_generation_full_guard` 側の定義であり、前段の CHECK は `search_generation` 側の定義である。両表を同じ transaction で作り、target だけまたは guard だけを commit しない。

- Full target の P7 payload/receipt/lexical 子表 DML は親 BUILDING 行を `FOR UPDATE` で lock し、対応する full guard row を lock して **target に保存された token/fence との一致**、`expires_at > clock_timestamp()` を確認する。P3 Graph を接続した場合、その full Graph parent と resource/relation/participant DML にも同じ guard/binding fence を掛ける。通常 builder は guard INSERT/UPDATE/DELETE、target binding UPDATE、無 guard parent INSERT を直接実行できない。batch は commit 直前にも DB clock を確認し、期限切れなら全 rollback する。
- 各 trusted batch port は `FullBuildHandle` の source/key/token/fence を target と guard の保存値に照合する。trigger も handle を信じず DB の親・guard・expiry を検査する。通常 builder に与える直接 child DML 権限はこの trigger 下だけに限定し、guard 失効後の直接 SQL も拒否する。
- `validate_ready` は P7/Graph target と guard を同じ接続・既定 lock 順で再検査し、token/fence/DB expiry と全 receipt を確認してから READY にする。Graph READY と P7 READY はそれぞれの実体検証を要し、一方の READY だけで publish できない。READY 後の子変更は guard の有無にかかわらず拒否する。
- `renew_full_guard(handle, bounded_ttl)` は Source→target→guard の一 transaction で、保存 token/fence と未失効を DB clock で確認したときだけ延長する。失効済み guard は復活しない。`publish_if_current` と §1 の EVENT publish は同じ guard/DB expiry を pointer CAS の transaction 内で確認し、成功した CAS と同 commit で guard を消す。CAS 敗北では guard を残す。
- 期限切れ/abort は Source→target generation→guard→lease を lock し、current でも pin 中でもないことを再確認する。同じ transaction で P7/Graph target を `DELETING` → **一致 guard DELETE** → Graph participant/relation/resource と P7 子行 DELETE → Graph/P7 parent DELETE の順に行う。失敗は guard と target を含め全 rollback。失効後に同じ target へ guard を再発行しない。成功 cleanup 後も恒久 `search_generation_identity` が key 再利用を拒むため、新 build は新 generation key と新 fence を要する。

Graph の full parent にも同じ immutable token/fence を記録し、選定後の PG Graph schema では P7 target への複合 FK と mutation trigger を適用する。Graph が未選定の間は P7 Graph 非依存の schema/port 局所実装だけを許し、Graph READY/publish は閉じる。別 backend を選ぶ場合は対応する原子的 fence protocol の独立審査まで接続しない。

## 3. P3 `stage_full` と登録 transaction の唯一の入口

production の full 登録は P7 coordinator が所有する一接続・一 transaction に限定する。EVENT のときは outbox row を最初に lock し、MANUAL のときは Source row から始める。その後は Source `FOR UPDATE` → ownership/activation/retention 検査 →新規 identity INSERT → full target の P7 BUILDING INSERT →（選定済み PG Graph の）Graph BUILDING INSERT → full guard INSERT → commit である。Source snapshot、manifest、activation、同じ key/token/fence を P7/Graph parent に保存し、**PG Graph production 接続時は** Graph row と guard の双方がない状態を外部へ commit しない。Graph INSERT/guard INSERT の失敗、key collision、fence overflow は全 rollback し、可視の無保護 target を残さない。Graph row は P7 の private transaction-bound `GraphRepository::register_full_on(&mut PgConnection, registered_target)` のような method だけが作る。別 pool/接続で `stage_full` に parent を作らせない。

P3-G01/G04 の production port は、上記 commit で発行した `RegisteredFullBuildHandle` を入力にする `stage_full_registered(handle, resources, relations, ...)` 相当へ狭める。この port は既存 BUILDING parent に **batch 子行だけを追加**し、各 batch で P7/Graph parent と §2 の full guard を DB 再検証する。handle の field は private、DB を再読せず handle 所持だけで許可しない。P3 plan の旧 `stage_full(manifest,retention,...)->GraphStage` が parent を自律 INSERT する形は isolated PoC/fixture 専用の別 port とし、production composition root へ export/配線せず、production DB role に親 INSERT 権限を与えない。P3 `stage_incremental` は凍結 guard 追補の base/target 手順を保ち、共有 target 登録だけ P7 の同一 transaction-bound 接続へ統合する。P3-G01/G04/C01 の実装計画には、この signature と role/constructor 境界を明示してから着手する。

P3-P04 の backend 再判定と `GraphReceiptMappingV1` の P1 staged-input/P3 content の二つの canonical encoder・golden vector が成立するまでは、Graph production migration、Graph READY、共有 publish を有効にしない。P3 isolated PoC の pass をこの登録経路の検証とみなさない。

## 4. Durable pin の保存された発行先

`search_evaluation_lease` は元設計の key/evaluation/activation/二 digest/DB expiry に加え、`tenant_owner_key NOT NULL`、`actor_scope_ref NOT NULL`、`registration_revision NOT NULL`、`visibility_revision NOT NULL`、`access_revision NOT NULL` を保存する。正数 revision と bounded nonsecret reference を CHECK/port で検査し、pin INSERT 後は全 identity/scope/reference/digest 欄を immutable とする。`actor_scope_ref` は trusted host adapter が `TrustedSearchScope` と `TrustedDiscoveryBinding` の actor/session/evaluation scope に対して発行・再照合する非秘密の opaque reference であり、principal、session credential、access handle、本文、token を DB に保存しない。lease ID や evaluation ID の所持だけでは参照元 actor を証明しない。

この reference は lease TTL 内に別 process/restart 後も host authority が同じ trusted actor/evaluation scope に再解決できるか、解決不能なら当該 pin を fail closed にする。process-local pointer や `Instant` の byte 表現を保存しない。DB は revision の正数/BIGINT 範囲、reference の長さ上限と非空、scope field の immutable trigger を持つ。

`pin_current` は渡された `AuthorizedSourceScope` の tenant/actor scope reference、registration/visibility/access revision と Source/ownership の ACTIVE・tenant・activation を現在 gate で検査し、同じ transaction でその不変値を lease row に INSERT する。`renew_pin`、`verify_pin_before_return`、actor-facing `release_pin` は **pin と現在の trusted `AuthorizedSourceScope`/evaluation を両方受け取る** signature に改める。保存 row を lease ID で lock し、tenant owner、Source/key/evaluation、actor scope reference、registration/visibility/access revision、activation、二 digest、DB clock expiry を一致させたうえで P4 `CurrentSourceVisibilityPort`/host ledger と P5 現行 actor・Source gate を再実行する。`verify_pin_before_return` では、返却対象の item・field・Graph participant ごとの P5 final gate と Document Version/Part/raw/current Read も再実行する。P5 の source-neutral catalog 改訂は既存 P4 `TrustedSearchScope`/`AuthorizedSourceScope` の mint を再利用し、P7 専用の第二の actor/Source mint を作らない。Denied/Unknown/期限切れ/Source 再登録・visibility 変更では renew と結果返却を拒否し、別 actor/tenant の同じ lease ID を受け付けない。revocation 後の actor-facing release も成功を偽装せず、失効 lease の物理掃除は限定 coordinator GC role が別経路で行う。旧 generation を pin していても Source pointer が別 key に進むこと自体は revoke 条件にしないが、保存 key/receipt と Source の現在 authority は必ず検査する。remote RAM lease はこの表に置かない。

## 5. Global Source kind と Remote desired 集合の権威

`search_source_ownership.source_kind` は migration で `CHECK (source_kind IN ('DOCUMENT','REMOTE'))` を持つ。`source_id`、`tenant_owner_key`、`source_kind` は ACTIVE/TOMBSTONED を通して不変であり、UPDATE/DELETE trigger と role grant で守る。同一 SourceId を別 kind または別 tenant へ再登録する試みは tombstone 後も拒否する。Document 登録と Remote reconcile は **同じ `search_registration_serial` 一行を先頭に lock** し、同じ global ownership/Source ledger と lock 順を使う。Document 登録もこの lock の外に独立 catalog を確定しない。

現行 P4 `SourceRegistrationLedgerPort::reconcile(&BTreeMap<SourceId, RemoteSourceRegistration>)` の map 単独には「全 tenant の全 Remote」という証明がない。[P5 source-neutral 改訂 2](p5-api-contract-revision-2.md) §2 の `SourceRegistration::{Document,Remote}`、`RegistrationNamespace`、`CompleteDesiredRegistrations { namespace, deployment_revision, set_digest, registrations }`、`SourceRegistrationLedgerPort::reconcile` を **P7 production adapter の一つの契約**に採る。現行 P4 Remote-only trait はこの migration 後に production の別 ledger/port として残さない。両 variant の `SourceAuthorityDescriptor` は各登録から導出する view とし、第二の書換え可能な authority record や第二の actor/Source mint を作らない。`CompleteDesiredRegistrations` は trusted composition root の host-owned complete desired snapshot からのみ構築し、`namespace=Remote` なら全 tenant・全 Remote、`namespace=Document` なら全 tenant・全 Document の map を持つ。provider/request や呼出し map 自身から completeness の権威を作らない。

`RegistrationSetDigest` の canonical encoder は namespace/version の domain separator（Remote は `remote-desired-set:v1`）と length-framed、SourceId UUID bytes 昇順の各 entry を用いる。tenant、`SourceKind`、SourceId、registration/visibility revision と各 variant の **全 server-owned DTO field** を固定 tag・順序・長さで符号化する。Remote は provider/endpoint/modes/authority/resource kind/access/retention/freshness/lineage/limits、Document は adapter ref/allowed kind/local mode/enumeration/retention を含める。JSONB 物理 byte 列や部分 map の hash は正本 digest にしない。adapter は desired の namespace と全 key・全 variant・全 DTO 値を host authority の **同 revision の complete snapshot** と exact equality で照合し、両側で canonical digest を再計算して `set_digest` と一致させる。host authority は同じ trusted composition root の登録正本であり、別の actor/Source trust mint ではない。tenant/kind/revision/visibility が一つでも違う、部分 tenant map、余分な key、欠けた登録、snapshot 未確定なら **変更前に一原子失敗**とする。

`search_registration_serial` は一つの global lock row のまま、Document と Remote **各 namespace ごとの** `deployment_revision`/full desired-set digest を保存する。adapter はこの行を最初に lock し、host の当該 revision/digest が commit 直前にも current であることを再照合する。保存 revision より古いもの、同 revision 異 digest は拒否し、同 revision・同 digest・同 map は冪等とする。新 revision だけを一 transaction で適用し、tombstone 対象を `source_kind = desired.namespace` の既存行に厳密限定する。特に Remote reconcile は complete Remote map にない Remote のみ tombstone にし、`DOCUMENT` 行を predicate に入れない。Document reconcile も Remote 行を変更しない。同名 SourceId が反対 namespace の desired にあれば kind 衝突で全 rollback とする。serial row の当該 revision/digest、ownership/Source の activation/fence 変更は同じ commit とし、両 namespace の同時登録は global serial lock で直列化する。reconcile 後も `is_current` は保存 DTO 全体と現在 activation/visibility を照合する。host complete snapshot を検証できない配備は production 起動を拒否する。

## 6. 計画と検証 gate

P7 実装計画、P6-S03/bridge、P3-G01/G04/C01 の該当 task に上記 signature/schema/role/lock 条件を反映する。各 named case は実 PostgreSQL、実 builder/coordinator/reader role、独立接続・必要な barrier/fault injection で **RED→GREEN** を残す。SQLx 0.9.0 の pinned `Migrator::dangerous_set_table_name` を Search ledger に使い、独自 migrator は追加しない。P6-I03 の Search `0001` writer と P7 `0002+` writer、Domain/Graph の別 ledger は元設計どおりである。

| 指摘 | 必須 named real-PG regression |
| --- | --- |
| Event origin | `wrong_event_candidate_cannot_publish_or_write_receipt`; `old_epoch_or_manual_candidate_cannot_complete_event`; `reuse_current_requires_current_ready_after_gc` |
| Full guard | `expired_full_guard_rejects_late_child_write_and_ready`; `full_target_guard_cannot_be_reissued`; `guard_delete_then_child_failure_rolls_back_full_target` |
| Graph registration | `full_registration_is_one_commit_with_guard_and_graph_key`; `graph_stage_failure_leaves_no_visible_unprotected_target`; `isolated_stage_full_cannot_publish_without_p7_guard` |
| Pin scope | `pin_cannot_transfer_between_actor_scopes`; `registration_or_visibility_change_revokes_old_pin_read`; `foreign_tenant_cannot_renew_same_source_pin` |
| Ownership/reconcile | `remote_reconcile_never_tombstones_document`; `partial_tenant_map_cannot_tombstone_foreign_remote`; `document_remote_same_source_id_is_rejected_after_tombstone`; `partial_or_stale_remote_desired_set_is_atomic_failure` |

さらに P1 composite v1 golden vector、実 lexical Unit の双方向 seal/復元、P3 `GraphReceiptMappingV1` の staged input/content 両 encoder vector、unknown commit、current/pin/guard/GC の競合、旧 ownership row migration/backfill 失敗、別 DB restore を focused receipt に残す。P3-P04 再審査前は Graph 非依存の共有 schema/port に限る局所判定とし、Graph production 接続や最終 P7 GO と混同しない。
