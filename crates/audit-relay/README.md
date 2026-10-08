# audit-relay

Audit Infrastructure v1の配送（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §5・§6・§12、[決定記録](../../docs/decisions/2026-10-07-audit-envelope-store-integrity.md) D4）。Documentの `public.audit_outbox_events` を、`crates/audit-store-postgres` の監査Storeへat-least-onceで届ける。

- Document DBに `audit_relay` schema（ledger `audit_relay_sqlx_migrations`）を置く。Documentの `_sqlx_migrations` には書かない。
- 配送は `outbox-delivery` の `DeliveryRunner` を変更せずに使う（`RelayOutboxStore`・`AuditDeliveryHandler`・`BreakerAdmission`）。
- Storeへの呼出しはすべて `audit_core::AuditStore`（ingest、probe、report_regression、receipt参照、relay control event）で行い、`store::RelayStore` はそれに `store_status` と `lookup_lost_ranges` だけを足す。失敗の分類・control eventの形・reconcileのcountはaudit-coreの型（`StoreError`、`RelayControlKind`、`ReconcileCounts`）をそのまま使う。
- staging行の理由文はclaim関数（SQL）が取り除き、Storeへは `{provided, utf8_bytes, text_retained}` だけが届く。

## migrateの順序

| 手順 | 実行者 | 内容 |
|---|---|---|
| 1 | Document migrator | Documentのmigration（`_sqlx_migrations`） |
| 2 | superuser（`AUDIT_RELAY_MIGRATE_DATABASE_URL`） | `audit-relay migrate`。事前検査（stagingの列・型、Document ledger）→ `audit_relay_owner`（NOLOGIN・非superuser）とschema → backfill → 登録trigger・guard。superuserでなくstagingのownerで実行した場合は、そのroleが持つ `audit_relay_owner` のmembershipを実行後に必ずREVOKEする（残っている間、postureは `owner_member` を報告し、`run`・`reconcile`・`replay` は起動しない。設計§5.3：Documentのowner・runtime roleには `audit_relay` への権限を与えない） |
| 3 | DB owner | `sql/roles.sql`：`audit_relay_worker` / `audit_relay_operator` とEXECUTE行列、LOGIN roleのtimeout。LOGIN role追加後とDocument DBのrestore後に再実行する |
| 4 | Store側 | `audit-store-postgres` の手順。relay serviceのStore loginを `service/audit-relay`（Documentのsource serviceとして登録済み）に束縛する |

- migrate中は `audit_outbox_events` を `SHARE ROW EXCLUSIVE` でlockするため、業務のINSERTはcommitまで待つ（`lock_timeout` 10秒）。
- `audit_outbox_events` の被digest列（`event_id`〜`occurred_at`、`resource_type` を含む）をDROP・型変更するDocument migrationは、`BEGIN ATOMIC` のdigest関数の依存で失敗する（意図した制約）。`DROP COLUMN ... CASCADE` は禁止。nullable列のADDは影響しない。

## roleとcredential

| command | `AUDIT_SOURCE_DATABASE_URL`（Document） | `AUDIT_STORE_DATABASE_URL`（Store） |
|---|---|---|
| `run` | `audit_relay_worker` を持つservice login（circuit状態の報告 `report_runtime` を含む） | relay service：`audit_store_ingest`＋`audit_store_relay_control`＋`audit_store_reconciler`、`service/audit-relay` に束縛 |
| `health` / `reconcile` | worker（read-only。`health --forecast` はworkerだけ：配送前の内容を投影する `preview_pending` はworkerにだけ与える） | operator：`audit_store_relay_control`＋`audit_store_reconciler`（ingestは持たない）、本人の主体に束縛 |
| `reconcile --repair` / `replay` | operator本人の `audit_relay_operator` login | 同上（operator本人のStore login）。`audit_store_ingest` を持つStore loginは拒否する（CLIとStoreの両方。特権操作の記録のactorを操作したoperatorにするため） |

- operatorのStore loginはprobeできないので、`health` の `stored.missing_types`（catalog skew）は `null` になる。relay serviceのloginで実行すると、Storeが未登録のcatalog typeを返し、空でなければ `store_catalog_skew` を警報する。

- capability roleは表の権限を持たず、definer関数（`SECURITY DEFINER`、`search_path = pg_catalog, pg_temp`、owner `audit_relay_owner`）だけを実行する。PUBLICには何も与えない。
- `migrate` 以外は、superuserと `audit_relay_owner`（Store側は `audit_store_owner`）のmemberのsessionを拒否する。sourceとStoreが同一database（`system_identifier` と `current_database()`）なら起動しない。URLの `options`・`options[<設定>]` と空でない `PGOPTIONS` を拒否し、全接続で `synchronous_commit = on` を確認する。URL・credentialはerrorに出さない。
- `run`・`reconcile`・`replay` は `audit_relay.posture_check()` に違反がある間は起動しない（`health` は警報として出す）。postureはEXECUTE行列・所有者・schema/表の権限に加え、次を違反として報告する：`audit_relay` の表の列の権限（ownerでない全role・PUBLIC。表の権限に現れずcommitment saltの読取りや受領列の書込みを許すため。`column_privilege`、objectは `表.列 role`）、`audit_relay_owner` のmember（superuser以外のすべてのrole。`owner_member`）、capability roleまたはそのloginがstagingを直接読めること（表・列の権限、`pg_read_all_data`。`staging_read`）、capability roleまたはそのloginが `audit_relay` の表を読み書きできること、およびこのDBへ接続できる（CONNECT）superuserと `audit_relay_owner` 以外のすべてのloginが `audit_relay` の表を書けること（INSERT/UPDATE/DELETE/TRUNCATE。capabilityを持たないDocumentのloginが `pg_write_all_data` で最初の配送受領を偽造できるため。`table_access`）、このDBへ接続できる非superuserのloginの `pg_read_all_data`・`pg_write_all_data`・`pg_maintain`（Storeのpostureと同じ。`predefined_role_member`、objectは `login:role`）、接続できるかに関係なくすべての非superuserのloginの `pg_read_server_files`・`pg_write_server_files`・`pg_execute_server_program`（OSのfile・programからの迂回。`predefined_role_member`）、同じく接続できるかに関係なくREPLICATION属性を持つすべての非superuserのlogin（replication protocolでclusterの全内容を読める。`replication_login`）、stagingのrow security（定義者関数から行が見えなくなる。`row_security`）、登録・guard・書き手triggerが別の関数・event・WHEN条件で作り直されていること（`trigger_missing`）、capability loginのtimeoutが未設定または0であること（`login_timeouts_missing`）。
- 多層防御として、`audit_relay` の5つの表（deliveries、delivery_history、delivery_policy、delivery_progress、relay_runtime）には書き手を確かめるtrigger（`*_writer`、SECURITY INVOKERの `audit_relay.guard_writer`）がある。行は定義者関数（`audit_relay_owner` として実行）とmigrationだけが書くので、`current_user` が `audit_relay_owner` でもsuperuserでもない直接の書込みを42501で拒否する（どのsessionでも設定できる `audit_relay.transition` を設定しても通らない）。境界はpostureであり、superuserはこの境界の外にある。
- **Document DBのbackup（`pg_dump`）はsuperuserで実行する**：`pg_read_all_data` のbackup loginは `predefined_role_member`、`audit_relay_owner` のmemberは `owner_member` として報告され、`run` が起動しない（server fileのroleを持つloginも同じ）。
- **REPLICATION属性のloginはこの境界の外にある**：streaming replicationやbase backup（`pg_basebackup`）のloginは、replication protocolでWAL・data fileとしてclusterの全内容（staging行、commitment salt、受領）をどの権限にも依らず読める。非superuserのREPLICATION loginは `replication_login` として報告され、`run` が起動しない。replicationはsuperuserと同じく境界の外の操作として、superuserのloginで専用のreplication基盤に限って行う。そのsuperuserは `pg_hba.conf` の `replication` 行（replica・backup hostだけ）にしか一致させず、同じuserに一致する `all`・database名の行を置かない（replica hostからのSQL接続を許すと、その資格情報で配送前のstagingや配送台帳を書き換えられる。物理replication接続はreplication commandだけを受け付けSQLを実行できず、database指定のreplication接続（logical replication）はdatabaseの行で照合されるので拒否される）。その基盤への到達と資格情報をrelayのDocument DB境界とは別に管理する。

## command

```text
audit-relay migrate
audit-relay run                                  # SIGTERM / Ctrl-C で有界にdrainして停止。stderrに進捗行
audit-relay health [--forecast] [--reconcile]    # JSON。produced/delivered/stored/verifiedとcircuitを分ける。Storeへ接続できなくても報告する
audit-relay reconcile [--repair]                 # 1 runにつき audit.reconciliation.completed を1件記録
audit-relay replay --event-id <uuid>             # Storeへ replay_requested を記録してから戻す
```

### `health` のcircuit（設計§12）

`health` は別processなので、`run` のcircuit breakerは各 `run` processがDocument DBへ報告した状態で示す（`audit_relay.relay_runtime`、workerだけが実行できる `audit_relay.report_runtime`）。各processは1秒ごとにbreakerを標本化し、変化したとき、および変化が無くても10秒ごとに自分の行（process起動時の乱数id。他には出さない）を更新し、正常停止で削除する。報告できなくても配送は止めない（次の標本で再試行）。行は固定code・件数だけで、1日報告の無い行は次の報告が削除する。

| key | 意味 |
|---|---|
| `circuit.running` / `circuit.stale` | 60秒以内に報告したprocess数 / それより古い行の数（強制終了したprocess） |
| `circuit.state` | runningのうち最も悪い状態（`open` → `half_open` → `closed`）。runningが無ければ `null` |
| `circuit.gate` | その状態のprocessの最後のgate code（`ok`、`unknown`、Storeのoutage code、`store_regressed`、`source_unavailable`） |
| `circuit.outage_streak` | 最後の構造化ingest verdict以後のStore障害の連続回数（runningの最大）。行ごとの `outage_streak`（policyの上限64）とは別 |
| `circuit.outages` | 起動以後のStore障害の回数（runningの合計） |
| `circuit.last_report_age_seconds` | runningの最新の報告からの秒数 |

`state` が `open` のとき警報 `circuit_open` を出す。

`stored.denials_pending` はStoreがまとめてまだchainに書いていない拒否の件数（`store_status` の `denials_pending`）で、0より大きい間は警報 `store_denials_pending` を出す（あるloginが繰り返し拒否されている。Storeは止まった連続も集約window後の次の追記・probeと証拠記録の前に全件書く）。

### `run` の進捗行（stderr）

`run` はstderrへ有界な進捗行を出す。値は固定code・件数だけで、payload・subject・actor・resource・event id・event type・reasonは出さない（`tests/runtime.rs` で合成eventの値が出ないことを確かめる）。

```text
audit-relay: event=circuit circuit=closed gate=ok was_circuit=half_open was_gate=unknown outage_streak=0 delivered=1 duplicate=0 held=0 outage=0 quarantined=0
audit-relay: event=progress circuit=closed gate=ok outage_streak=0 delivered=32 duplicate=0 held=0 outage=0 quarantined=0
```

- `event=circuit`：circuit状態かgateが変わったとき（1秒ごとの標本。breakerがgateを評価し直すのはcooldownごとに最大1回）。
- `event=progress`：前の行以後に処理があったときだけ、`AUDIT_RELAY_PROGRESS_MS`（既定10000）に最大1行。idleのrelayは最初のgate結果の後は何も出さない。
- `event=final`：停止時に未出力の件数が残っていれば1行。
- 件数は前の行以後のhandlerの結果：`delivered`（stored）、`duplicate`（duplicate*）、`held`（relay側の保留）、`outage`（Store障害の保留）、`quarantined`。
- 停止要求がrunnerの処理中に届くと、runner（`outbox-delivery`）は結果を確認できないとして `audit-relay: delivery stopped: outbox store result is unknown` で終了code 1になる（停止の安全性は変わらず、leaseの失効後に再claimされる）。その場合もmonitorは最後の行を出し、circuitの報告を削除してから終わる。

`run` の設定は環境変数（`AUDIT_RELAY_BATCH_SIZE` 32、`AUDIT_RELAY_MAX_IN_FLIGHT` 4、`AUDIT_RELAY_LEASE_MS` 30000、`AUDIT_RELAY_RENEW_MS` 9000、`AUDIT_RELAY_POLL_MS` 250、`AUDIT_RELAY_INGEST_TIMEOUT_MS` lease/3未満、`AUDIT_RELAY_PROGRESS_MS` 10000 など。`src/config.rs`）。policyの既定は試行16、lease 1–120秒、backoff 1–300秒、`outage_streak` 上限64。runnerは1 cycleで最大 `MAX_IN_FLIGHT` 件（breakerがhalf-openの間は1件）をclaimし、処理を待ってからpoll間隔だけ休むので、1 processの処理量は約 `MAX_IN_FLIGHT / (poll + Store往復)` 件/秒（既定で約15件/秒）である。継続的にこれを超える場合は `MAX_IN_FLIGHT`（runnerの上限8）を上げるか、pollを短くするか、relay processを増やす。滞留は `health` の `pending`・`oldest_pending_age_seconds` に出る。

## 失敗の扱い

- `health` はStoreへ接続できない場合（transport・timeout、SQLSTATE class 08・53・57、55000：`ALLOW_CONNECTIONS false` 等）も終了せず、その障害codeを `stored.gate` に入れ `stored.available: false` と警報 `store_unavailable` を出す（`relay::connect_for_health`、`store::UnreachableStore`。同一databaseの検査は比べる相手が無いので省く。healthは何も書かない）。認証失敗・TLSの失敗・未対応の認証方式・存在しないdatabase・URLとsessionの検査は障害ではなく、exit 1のままである。`run`・`reconcile`・`replay` は両DBへ接続できなければ起動しない（exit 1）。起動後のStore障害は保留して復旧後に排出するが、停止中に起動・再起動した `run` は終了するので、process監視が再起動する。
- quarantine（終端）は、Storeの構造化verdict（`conflict`、`rejected_<code>`。`audit_core::IngestRow::into_result` だけが作る）とrelay側の判定（`source_digest_mismatch`、`actor_mismatch`、catalog不適合）だけ。source改変は先に `audit.integrity.source_mismatch_detected` を記録し（event・codeごとに1回）、記録できなければ保留する。
- quarantine codeはStoreのcode形式 `[a-z0-9_]{1,64}` に従う（replayが `quarantine_code` として記録できるように）。relayの拒否codeはそのまま、Storeの拒否は `rejected_<code>` を64 byteで切る。
- それ以外（通信断、timeout、全SQLSTATE、結果不明、recovery mode、後退、posture違反、未登録type、ingest主体の拒否）は外部障害として試行を返却して保留し、circuit breakerを開く。breakerはingestの構造化結果でだけ閉じる。
- relayのcatalogより新しいtype・field（未知のdata keyを含む）は `relay_catalog_skew` として保留し、healthで警報する。reconcileはこの保留を独立したclass `relay_catalog_skew`（pendingとは別、`count_relay_catalog_skew`）として数え、警報を出す（`audit_relay.delivery_view` の `last_error_code` で判定する）。
- relay側の保留（Storeに届いていない：`relay_catalog_skew`、`relay_projection_invalid`、`relay_source_unavailable`）は試行を返却し、streakに数えず、Storeの障害状態（`last_outage_*`、`outage_held`）に触れない。行ごとの保留回数 `relay_hold_count` で指数backoff（`backoff_min × 2^(回数−1)`、`backoff_max` で頭打ち）し、claimは保留していない行を先に取る（保留行が大量にあっても配送できる行を待たせない）。healthは `delivered.relay_held` と警報 `relay_held` で示す。Storeの結果・replay・repairで回数は0に戻る。breakerを開きも閉じもしない（half-openの許可は解放され、次のclaimに使われる）。
- `outage_streak` は、audit-coreが残余とするoutage（`store_internal`、`store_other`。`OutageCode::counts_toward_outage_streak`）が、別の配送の成功を挟んで続いた場合だけ数える。上限で `outage_suspected_event_specific` としてquarantineする。
- claimの前のgate（`BreakerAdmission`）：catalogの期待（source、adapter_version、type一覧。`ProbeExpectation::from_catalog`）と、Storeの現在のrecovery epochで最後にackしたreceiptを渡してprobeする。epochが前回と違えばそのepochのack headで2回目のprobeをする。順序は、同じepochで検知済みの後退（sticky）→ Storeの状態（recovery mode、posture、read-only）→ 後退 → 未登録type（`store_unregistered_type`）。
- 後退（operationalなStoreが最後にackしたreceiptを解決できない。fingerprintで見えないin-place restoreなど）を検知すると、`audit_store.report_regression` で報告し（Storeが再確認して `recovery_pending` にする）、recovery epochが変わるまでclaimを止める。fingerprintで検知されたrestoreはrecovery modeとして止まり、operatorが `begin-recovery-epoch --relay-max-seq` でrelayの最大seqを記録する（`--preview` で確認した復元head・消失範囲を帯域外の記録に書き、その値を `--expect-*` で渡す。Storeは食い違う記録を拒否する）。`--relay-max-seq` には、復元先のStoreへ向けた `health` の `stored.relay_max_seq` を使う（Storeへ接続できないときは同じ値を `delivered.max_referenced_store_seq["<old_epoch>"]` で読む）：復元したepochでrelayが参照する最大のStore seq（receiptに加え、source mismatch・replay・repairのcontrol event、historyに残したreceipt。`status()` の `max_referenced_store_seq` をepochごとに持つ）。`max_acked_store_seq`（全epochのreceiptの最大）では、ackの後に記録されたreplayが消失範囲から外れ、`unaudited_replay` に誤分類される。
- `reconcile --repair` の delivered_missing → pending は、Storeの現在のrecovery epochより古いepochのreceiptだけに行う（宣言されたrecoveryの後）。同じepochで失われたreceiptは未報告の後退（fingerprintで見えないin-place restore・削除）なので戻さず、delivered_missingの警報のまま残す（SQL関数 `repair_reset_missing` も同じfenceで拒否する）。relayのgateが後退を報告し、recovery epochを開始してから修復する。
- source mismatchの記録は（seq, recovery epoch）を持ち、`delivery_history` のcontrol eventは（epoch, seq）で一意である（restore後にseqが再利用されるため）。

## 限界

- Document DBのsuperuser・`audit_relay` の所有者は、trigger・FKを迂回して配送前のstagingと配送台帳を改変できる。guardは事故とDDLを伴わない不正DMLの防止である（設計§5.3）。
- `reconcile` は配送履歴（replay・repair）をStoreのcontrol eventに照合する。replay行は、同じevent・同じrecovery epoch・同じquarantine codeの `audit.delivery.replay_requested` に1対1で（同じcontrol eventを2行が使えば2行目は不一致）、repair行は同じepochの `audit.reconciliation.completed`（mode `repair`）に対応しなければならない。対応しない行は、そのepochでStoreが宣言した消失範囲（`lookup_lost_ranges`）に入っていれば `replay_record_lost`、それ以外は `unaudited_replay`。`count_replay_record_lost` は `audit.reconciliation.completed` に記録される。
- `repair_ack_stored` のfenceは、渡されたcommitmentとserver側の再計算値の一致を確かめる。Storeの事実そのものはDocument側で検証できないので、CLIがStoreのreceiptを渡す。
- ackに記録するStoreのrecovery epochは、claim前のprobeで観測したepochである。probeとingestの間で `begin_recovery_epoch` が完了した場合（手順ではrelayを止めて行う）、そのackは1つ前のepochとして記録される。
- repair modeの `audit.reconciliation.completed` は、修復の前に記録する計画件数（`repaired_*`）を持つ。実際に適用した件数はCLIの出力（`applied`）にだけ出る。

## 試験

PostgreSQL 18.6（testcontainers `postgres:18.6-bookworm`）の1 containerに、Document DBとStore DBの2 databaseを作る。runtimeの経路はroles.sqlで作ったLOGIN roleで実行し、producerは合成行のINSERTで模擬する。子processのSIGKILL、`pg_dump`/`pg_restore` による復元も含む。
