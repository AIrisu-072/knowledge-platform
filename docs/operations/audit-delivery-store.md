# Audit配送・保存 運用手順（Audit Infrastructure v1）

Status: 単位B（`crates/audit-relay`、`crates/audit-store-postgres`）のsourceに合わせた手順（2026-10-08）。**本番credential・本番migration・server deploy・本番運用は本trackで実施も検証もしていない。** 確認できたのは、合成データとPostgreSQL 18.6（testcontainers `postgres:18.6-bookworm`）での試験だけである。sourceで確認できない事項は「未確認」、sourceからの推論は「推論」と記す。設計は[配送・保存・検証設計](../superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md)、決定は[決定記録](../decisions/2026-10-07-audit-envelope-store-integrity.md)。

表記：`<...>` はplaceholderである。接続URL・password・hostnameはsecret管理から環境変数へ注入し、shell履歴・repository・log・ticketに残さない。両CLIは結果をJSONでstdoutに出し、errorはcodeだけをstderrに出す（URL・credentialは出さない）。

## 1. 責任範囲と4つの証拠

根拠：設計 §1・§12、`crates/audit-relay/src/health.rs`

| 証拠 | 意味 | 見る場所（`audit-relay health`） |
|---|---|---|
| produced | Documentの業務transactionでstagingされ、同じtransactionで配送登録された | `produced` |
| delivered | relayがStoreのreceiptでackした | `delivered` |
| stored | Storeが受理し、hash chainに載せた | `stored.head_seq` |
| verified | Store内検証の被覆（genesisから連続して `ok` の範囲） | `verified`。真正性は§9のDB外判定だけで主張する |

- Auditは通常log・trace・metric・Domain Business Event・業務集計の正本・Personal Memoryの代わりではない。samplingしない。配送はat-least-once、ingestはevent IDとsource commitmentでidempotent（`duplicate` / `duplicate_reprojected` / `duplicate_expired`、食い違いは `conflict`）。
- 理由文はStoreへ複製しない。`{provided, utf8_bytes, text_retained}` だけが届く。
- 範囲外：Document producer、Search・Organizationのsource（設計 §13）。

## 2. 前提

根拠：両crateのREADME、`crates/audit-relay/src/session.rs`、`crates/audit-store-postgres/src/session.rs`、`src/bin/*.rs`

- PostgreSQL 18。試験は18.6だけ。StoreはDocument DBとは別のdatabase（別serverも可）。relayは、sourceとStoreの `system_identifier` と `current_database()` が両方一致すると起動しない。
- migration ledger：Document DBは `audit_relay` schemaと `audit_relay_sqlx_migrations`、Store DBは `audit_store` schemaと `audit_store_sqlx_migrations`。Documentの `_sqlx_migrations` には書かない。
- 全sessionで `SHOW synchronous_commit` が `on` であること。URLの `options` / `options[<設定>]` と、空でない環境変数 `PGOPTIONS` は接続前に拒否される。
- binary：`audit-admin`（crate `audit-store-postgres`）、`audit-relay`（crate `audit-relay`）。build例：`cargo build --locked --release -p audit-store-postgres -p audit-relay`（本番の配布方法は未確認）。
- export・checkpointのfileは新規作成だけで、mode 0600、CLIを実行したuserの所有になる。

## 3. 初期導入

根拠：`crates/audit-relay/migrations/0001_audit_relay_v1.sql`、`crates/audit-relay/src/lib.rs`、`crates/audit-store-postgres/migrations/0001_audit_store_v1.sql`、両crateの `sql/*.sql`

順序：**Documentのmigration → `audit-relay migrate` → `sql/roles.sql`（Document）**、**`audit-admin migrate` → `sql/roles.sql` → LOGIN role → `sql/privileges.sql`（Store）→ 束縛・権限**。`audit-relay run` には両方の完了が要る。

### 3.1 Document DB

1. Documentのmigrationを既存の手順で適用する。
2. superuserで `AUDIT_RELAY_MIGRATE_DATABASE_URL=<DOCUMENT_MIGRATOR_URL> audit-relay migrate` を実行する（出力 `{"migrated": true, "ledger": "audit_relay_sqlx_migrations"}`）。
   - 事前検査：`public.audit_outbox_events` と `public._sqlx_migrations` の存在、15列の名前と型（`audit_relay::STAGING_COLUMNS`）。失敗すれば何も作らない。
   - 全体が1 transactionで、stagingを `SHARE ROW EXCLUSIVE` でlockする（`lock_timeout` 10秒）。業務のINSERTがcommitまで待つので、**業務の停止時間帯に行う**。
   - 既存行は `registration_kind='backfill'` で登録し、旧 `attempt_count` / `delivered_at` は `legacy_*` に残してpendingにする。適用後 `audit-relay health --forecast` でquarantineの見込みをcode別に見る。
   - superuserでなくstagingのownerで実行した場合は、そのroleの `audit_relay_owner` membershipを実行後にREVOKEする（残るとposture `owner_member` で `run` が起動しない）。
3. DB owner（superuser）が `crates/audit-relay/sql/roles.sql` を適用する。LOGIN roleを作り、capability role（`audit_relay_worker` か `audit_relay_operator`）をちょうど1つGRANTし、roles.sqlを再適用してtimeoutを設定する（30s/5s/60s）。
   ```sql
   CREATE ROLE <relay_worker_login> LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;  -- passwordはpsqlの \password で設定
   GRANT audit_relay_worker TO <relay_worker_login>;
   ```

### 3.2 Store DB

1. 空のdatabaseを作る（作成時のownerやencodingは未確認）。
2. superuserで `AUDIT_STORE_MIGRATE_DATABASE_URL=<STORE_MIGRATOR_URL> audit-admin migrate`（出力 `{"status":"migrated"}`）。`audit_store_owner`（NOLOGIN・非superuser）を作り、Document用のsource service `service/audit-relay` を登録する。
3. DB ownerが `crates/audit-store-postgres/sql/roles.sql` を適用し、LOGIN roleを作ってcapability roleをGRANTし（role名は `[a-z_][a-z0-9_$]{0,62}`）、`sql/privileges.sql` を適用する。**privileges.sqlはLOGIN role追加後とrestore後に毎回再適用する。**
4. ownerのmemberであるDBA login（capability roleを持たず、束縛しない）で：
   ```sh
   AUDIT_STORE_DATABASE_URL=<DBA_LOGIN_URL> audit-admin bootstrap-admin --db-role <admin_login> --issuer <ISSUER> --principal <PRINCIPAL_ID>
   audit-admin bind --db-role <relay_service_login> --issuer service --principal audit-relay
   audit-admin bind --db-role <operator_login> --issuer <ISSUER> --principal <PRINCIPAL_ID>
   ```
   bootstrapは1回だけ（2回目は `administrator_exists`）。再束縛には先に `unbind --db-role R` が要る。issuer `db_role` は拒否される。Document以外のsourceを足すときだけ `register-source-service --issuer I --principal P --source S`。
5. 管理者が自分のloginで `audit-admin grant --issuer I --principal P --capability <investigate|export|verify|administer|maintain>`。自分自身への付与は拒否され、記録される。
6. verifier・admin・maintainerのloginで `audit-admin posture` が `{"status":"clean"}`（exit 0）になることを確かめる。違反は1行1件で出てexit 3（§5.4）。

### 3.3 loginとcommand

正本は `crates/audit-store-postgres/sql/roles.sql` と `crates/audit-relay/sql/roles.sql`（EXECUTEの行列）である。実行にはDB層のcapability roleと、束縛された主体のAudit権限の両方が要る。

| login | DB | capability role | Store束縛 / Audit権限 | 使うcommand |
|---|---|---|---|---|
| relay worker | Document | `audit_relay_worker` | — | `run`、`health`（`--forecast` はworkerだけ）、読取専用 `reconcile` |
| operator（人ごと） | Document | `audit_relay_operator` | — | `replay`、`reconcile --repair` |
| relay service | Store | `audit_store_ingest`＋`audit_store_relay_control`＋`audit_store_reconciler` | `service/audit-relay` / 不要 | `run` |
| operator（人ごと） | Store | `audit_store_relay_control`＋`audit_store_reconciler`（ingestなし） | 本人 / 不要 | `health`、`reconcile`、`replay` |
| DBA | Store | `audit_store_owner` のmemberだけ | しない | `bootstrap-admin`、`bind`、`unbind`、`register-source-service` |
| 調査者 | Store | `audit_store_reader` / `audit_store_verifier` | 本人 / investigate・export・verify | `investigate`、`export`、`verify`、`checkpoint` |
| 管理者 | Store | `audit_store_admin` | 本人 / administer | `grant`、`revoke`、`set-retention`、`record-access-reapplied` |
| maintainer | Store | `audit_store_maintainer` | 本人 / maintain | `expire`、`purge-body`、`declare-recovery-pending`、`begin-recovery-epoch`、`confirm-retention-reapplied` |

- `replay` と `reconcile --repair` は `audit_store_ingest` を持つStore loginを拒否する（記録のactorを操作したoperatorにするため）。
- `audit-admin` はowner用の4 command以外で、superuserと `audit_store_owner` のmemberを拒否する。`audit-relay` は `migrate` 以外でsuperuserと `audit_relay_owner`（Store側は `audit_store_owner`）のmemberを拒否する。

## 4. relayの運転

根拠：`crates/audit-relay/src/config.rs`、`src/bin/audit_relay.rs`、`src/monitor.rs`、`src/breaker.rs`、`crates/outbox-delivery/src/policy.rs`、`tests/runtime.rs`

### 4.1 起動と停止

```sh
AUDIT_SOURCE_DATABASE_URL=<AUDIT_SOURCE_DATABASE_URL> AUDIT_STORE_DATABASE_URL=<AUDIT_STORE_DATABASE_URL> audit-relay run
```

- 起動拒否：特権session、同一database、URLの `options`・`PGOPTIONS`、`synchronous_commit` が `on` でない、`audit_relay.posture_check()` の違反、設定値の範囲外。
- 停止：SIGTERMまたはCtrl-Cで新しいclaimを止め、`AUDIT_RELAY_DRAIN_MS` の範囲でdrainする。正常終了（exit 0）ではstdoutに `{cycles, claimed, settled, lost, reaped, outages}` を出す。
- **停止要求がrunnerの処理中に届くと、exit 1と `audit-relay: delivery stopped: outbox store result is unknown` になる。これは想定内である**：結果を確認できない行はleaseの失効後に再claimされ、Storeのidempotencyでackへ収束する。monitorは最後の進捗行を出し、circuitの報告を削除してから終わる。
- restartは試行の履歴を戻さない。process監視（systemd等）と複数instanceの同時運転は未確認。

### 4.2 設定（環境変数、正の整数）

| 変数 | 既定 | 制約 |
|---|---|---|
| `AUDIT_SOURCE_DATABASE_URL` / `AUDIT_STORE_DATABASE_URL` | 必須 | workerのlogin / relay serviceのlogin |
| `AUDIT_RELAY_BATCH_SIZE` / `AUDIT_RELAY_REAP_BATCH` | 32 / 32 | 1–32 |
| `AUDIT_RELAY_MAX_IN_FLIGHT` | 4 | 1–8 |
| `AUDIT_RELAY_LEASE_MS` | 30000 | 1000–120000 |
| `AUDIT_RELAY_RENEW_MS` | 9000 | lease/3未満 |
| `AUDIT_RELAY_MAX_PROCESSING_MS` / `AUDIT_RELAY_DRAIN_MS` | 900000 / 30000 | 24時間以下 / MAX_PROCESSING以下 |
| `AUDIT_RELAY_POLL_MS` | 250 | lease以下 |
| `AUDIT_RELAY_INGEST_TIMEOUT_MS` | lease/3 − 1000（最小100） | lease/3未満 |
| `AUDIT_RELAY_BREAKER_INITIAL_MS` / `AUDIT_RELAY_BREAKER_MAX_MS` | 1000 / 60000 | initial ≤ max |
| `AUDIT_RELAY_PROGRESS_MS` | 10000 | 進捗行の最小間隔 |

- 処理量の目安：1 processで約 `MAX_IN_FLIGHT / (poll + Store往復)` 件/秒（既定で約15件/秒）。滞留は `health` の `pending`・`oldest_pending_age_seconds` に出る。
- DBの配送policy（`audit_relay.delivery_policy` revision 1）：試行16回、lease 1–120秒、backoff 1–300秒、`outage_streak` 上限64。relayはcodeの既定値（`RelayPolicy::default`）を使う。運用中にpolicyを変える手順は未確認。

### 4.3 進捗行（stderr）

値は固定code・件数だけで、payload・subject・actor・resource・event id・event type・reasonは出ない（`tests/runtime.rs`）。

```text
audit-relay: event=circuit circuit=closed gate=ok was_circuit=half_open was_gate=unknown outage_streak=0 delivered=1 duplicate=0 held=0 outage=0 quarantined=0
audit-relay: event=progress circuit=closed gate=ok outage_streak=0 delivered=32 duplicate=0 held=0 outage=0 quarantined=0
```

- `event=circuit`：circuit状態かgateが変わったとき。`event=progress`：処理があったときだけ、`AUDIT_RELAY_PROGRESS_MS` に最大1行。`event=final`：停止時に未出力の件数があれば1行。idleのrelayは最初のgate結果の後は何も出さない。
- 件数：`delivered`（stored）、`duplicate`、`held`（relay側の保留）、`outage`（Store障害の保留）、`quarantined`。

### 4.4 失敗の扱い

| 区分 | 例 | 扱い |
|---|---|---|
| 終端（quarantine） | Storeの `conflict`・`rejected_<code>`、relayの `source_digest_mismatch`・`actor_mismatch`・catalog不適合、`outage_suspected_event_specific`、`delivery_unknown_at_limit` | 保持される。§6のreplay・repairまで進まない |
| Store障害（保留） | 通信断、timeout、全SQLSTATE、結果不明、`store_recovery_required`・`store_regressed`・`store_posture_invalid`・`store_unregistered_type`・`store_denied` | 試行を返却し、`outage_streak` に応じてbackoff（最大300秒）。breakerが開く |
| relay側の保留 | `relay_catalog_skew`、`relay_projection_invalid`、`relay_source_unavailable` | 試行を返却。行ごとの `relay_hold_count` で指数backoff（`backoff_min×2^(n−1)`、上限 `backoff_max`）。claimは保留していない行を先に取る。breakerとStoreの障害状態に触れない |

breakerはStore障害で開き、指数cooldown（INITIAL→MAX）の後にhalf-open（1 claimずつ）になり、ingestの構造化された結果行を得たときだけ閉じる。claim前のgateの順序：検知済みの後退（sticky）→ Storeの状態（recovery・posture・read-only）→ 後退 → 未登録type。

## 5. health

根拠：`crates/audit-relay/src/health.rs`、`crates/audit-relay/README.md`、migrationの `status()` / `report_runtime` / `posture_check()`、`crates/audit-store-postgres/README.md`

```sh
audit-relay health [--forecast] [--reconcile]   # 両URLが要る。posture違反は拒否せず警報にする
```

### 5.1 出力

- `produced`：`staged`、`registered`、`unregistered`、`registration`（trigger/backfill/repair別）、`legacy_marked`。
- `delivered`：`delivered`、`pending`、`leased`、`retry_waiting`、`outage_held`、`relay_held`、`catalog_skew_held`、`quarantined`（code別）、`oldest_pending_age_seconds`、`max_acked_store_seq`、`max_referenced_store_seq`（epochごと）、`replayed`。
- `stored`：`gate`、`available`、`head_seq`、`recovery_epoch`、`recovery_mode`、`recovery_pending`、`access_reapply_pending`、`posture_ok`、`missing_types`、`denials_pending`、`relay_max_seq`（Storeの現在epochでrelayが参照する最大seq。§10.2の `--relay-max-seq`）。Storeのloginがoperatorだとprobeできないので `missing_types` は `null`。
- `circuit`：各 `run` processが1秒ごとに標本化し、変化時と10秒ごとに `audit_relay.report_runtime` で報告した状態。`running`（60秒以内に報告）、`stale`（それより古い行＝強制終了したprocess）、`state`（runningのうち最悪の `open` → `half_open` → `closed`、runningが無ければ `null`）、`gate`、`outage_streak`、`outages`、`last_report_age_seconds`。
- `verified`：`last_verified_seq`（最後の違反以後の `ok` の検証がgenesisから連続して覆う最大seq）、`outcome`（違反の後、genesisから走査headまでの検証が `ok` になるまで `violations` のまま）、`unverified_events`。
- `installation`：`installed`（trigger・guard・digest関数）、`policy_revision`、`posture_violations`（code）。

### 5.2 警報

| alarm | 意味 | 対処 |
|---|---|---|
| `store_unavailable` | `stored.gate` が通信断等 | Storeの復旧を待つ（自動で排出される） |
| `store_recovery_required` / `store_regressed` | fingerprint不一致・`recovery_pending` / ack済みreceiptがStoreに無い | §10 |
| `store_posture_invalid` | Storeのposture違反 | `audit-admin posture`、§5.4 |
| `store_catalog_skew` | relayのcatalogが期待するtypeをStoreが未登録（relay serviceのloginで見たときだけ）。`run` は全面的にclaimを止める | §7 |
| `catalog_skew_held` / `relay_held` | relayのcatalogより新しいtype・keyの行 / relay側で保留中の行 | §7 |
| `outage_held` | Store障害で保留中の行がある | Store側の原因を除く |
| `circuit_open` | runningのrelayのcircuitが開いている。`circuit.gate` が原因code（operator loginのhealthでもStore側の版ずれは `store_unregistered_type` として見える） | gateに応じて対処 |
| `store_denials_pending` | Storeがまとめた拒否のうちchainに未記録の件数がある＝あるloginが繰り返し拒否されている | 束縛・source service登録を確認（§5.5） |
| `quarantined` | quarantine行がある（code別は `delivered.quarantined`） | §6 |
| `unregistered_rows` | stagingはあるが配送登録が無い（triggerの喪失など） | `audit-relay reconcile --repair` |
| `staging_rows_hidden` | 登録件数 > 見えるstaging件数（row security等） | 権限・row securityを調べる |
| `repair_registered` | repair登録の行がある（登録以前の改変は検出できない） | 記録として確認する |
| `installation_incomplete` | trigger・guard・digest関数の欠落・無効化 | 配送を止めて調査する（再導入手順は未確認） |
| `relay_posture_invalid` | `audit_relay.posture_check()` の違反 | §5.4 |
| `verification_failed` | 検証の被覆が `violations` | §8・§9 |
| `--reconcile` 時：`unaudited_replay`、`delivered_missing`、`digest_mismatch`、`store_only`、`source_tampered`、`quarantined_conflict`、`replay_record_lost`、`relay_catalog_skew`、`reconcile_unavailable` | §6の分類 | §6 |

### 5.3 Store停止（配送遅延）とstaging失敗（業務rollback）の区別

- **Store停止**：業務は成功する。`delivered.pending`・`outage_held`・`oldest_pending_age_seconds` が増え、`stored.available=false`、Store系の警報と `circuit_open` が出る。試行回数は消費されない。復旧後に自動で排出される。
- **staging失敗**：Documentの業務operation自体がerrorで終わり、業務の変更もstagingも配送登録もrollbackされる（登録triggerの失敗も同じ）。行が残らないので **relayとhealthからは見えない**。検知はDocument側のerror応答・log・試験による（Document側で何をどう監視するかは本trackでは未確認）。
- `unregistered_rows` は「業務とstagingはcommitしたが配送登録が無い」状態で、staging失敗とは別の事象である。

### 5.4 posture違反

`audit-admin posture`（Store）は1行1件 `{"violation":..., "object":...}` を出してexit 3、relayは `health` の `installation.posture_violations` に出す。違反の間、Storeはingest・開示を止め（`store_posture_invalid`）、`begin-recovery-epoch` も拒否する。relayは `run`・`reconcile`・`replay` を起動しない。

| violation | 内容 | 対処 |
|---|---|---|
| `predefined_role_member` | DBへ接続できる非superuser loginが `pg_read_all_data`・`pg_write_all_data`・`pg_maintain` を持つ。`pg_read_server_files`・`pg_write_server_files`・`pg_execute_server_program` は接続に関係なく全非superuser login | REVOKEする。backupは§10.1のloginで行う |
| `replication_login` | REPLICATION属性の非superuser login（接続に関係なく） | 属性を外す。replicationはsuperuserで専用基盤（`pg_hba.conf` の `replication` 行をreplica・backup hostに限る）に限る |
| `owner_member`（relay） / `owner_member_has_capability`・`owner_member_bound`（Store） | ownerのmember（relay：superuser以外の全role） / Store ownerのmemberがcapability roleか束縛を持つ | membershipをREVOKE、束縛をunbind |
| `staging_read`（relay） | capability role・そのloginがstagingを直接読める（表・列の権限、`pg_read_all_data`） | 権限をREVOKE |
| `table_access`（relay） | capability role・loginが `audit_relay` の表を読み書きできる。または接続できる他のloginが書ける | 権限をREVOKE |
| `column_privilege` | 表の列単位の権限（ownerでない全role・PUBLIC） | 列権限をREVOKE |
| `ingest_member_not_source_service`（Store） | ingest roleのmemberがsource serviceとして束縛されていない | `bind --issuer service --principal audit-relay` |
| `login_timeouts_missing`、`*_synchronous_commit_*`、ACL・PUBLIC・search_path系 | role設定・EXECUTE行列のずれ | `roles.sql`・`privileges.sql` を再適用 |
| `row_security`、`trigger_missing`（relay） | stagingのrow security / triggerの差替え | 配送を止めて調査する |

### 5.5 拒否の集約

ingest経路（`ingest`・`probe`・`report_regression`）の拒否（未束縛、source serviceでない）は、loginごとに同じcode・actorの連続を1分に1件の `audit.access.denied` へまとめる。間引きはしない：各記録は自身と `suppressed_since_last` 件の拒否を表し、拒否の総数は記録ごとの `1 + suppressed_since_last` の和に等しい。codeやactorが変わるとき、そのloginが成功したとき、連続が止まって1分を過ぎた後の次の追記・probeの前に、未記録分を先に書く。verify・checkpoint・開示intent・expire・purgeは、記録の前に全loginの未記録分を書く。未記録の件数は `audit-admin status` の `denials_pending` に出る。recovery中は記録しない。他の拒否は1件ずつ記録される。

## 6. reconcile・replay・repair

根拠：`crates/audit-relay/src/reconcile.rs`、`src/replay.rs`、`crates/audit-relay/README.md`

```sh
audit-relay reconcile            # read_only：分類し、audit.reconciliation.completed を1件記録する
audit-relay reconcile --repair   # repair：先に記録し、許された遷移だけを行う（operator本人のlogin）
audit-relay replay --event-id <EVENT_UUID>
```

- 出力：`run_id`、`mode`、`watermark`、`store_epoch`、`counts`、`id_set_digest`、`planned`、`applied`、`control_seq`、`control_epoch`。Storeがrecovery modeの間は記録できないので実行できない。

| class | `--repair` |
|---|---|
| `ok`、`pending`、`quarantined`、`relay_catalog_skew` | 何もしない |
| `delivered_missing` | Storeの現在epochより古いepochのreceiptだけpendingへ戻す（履歴を保存）。同じepochで失われたものは未報告の後退なので警報のまま残す |
| `quarantined_stored` | `delivery_unknown_at_limit` でStoreに同じcommitmentがある行を、SQLのfence付きでackする |
| `unregistered` | `registration_kind='repair'` で登録する |
| `digest_mismatch`、`quarantined_conflict`、`store_only`、`source_tampered` | 触れない。調査する |
| `unaudited_replay` | replay・repairの履歴がStoreのcontrol eventに1対1で対応しない。直接SQLのreplayを疑う |
| `replay_record_lost` | 対応する記録がStoreの宣言した消失範囲にある（記録として確認する） |

replayはStoreへ `audit.delivery.replay_requested`（解除する `quarantine_code` を含む）を記録してから、1 transactionで履歴を保存し試行予算を戻す（出力 `{event_id, previous_quarantine_code, control_seq, control_epoch}`）。対象はquarantinedの行だけで、先に原因を直す（例：`conflict` ならadapter_versionの上げ忘れを直したrelayをdeployする）。`Refused{control_seq}` は「Storeに記録したがdeliveryが変わっていた」で、healthで状態を確かめる。`audit_relay.replay` をSQLで直接呼ばない。repair modeの記録の件数は計画値で、実際の適用件数はCLIの `applied` にだけ出る。

## 7. 新しいaudit typeの導入とDocument migration

根拠：`spec/telemetry/README.md`（adapter_versionの規律）、決定記録 D4、`crates/audit-relay/README.md`、`src/breaker.rs`

- 新しいtypeは、catalog（`spec/telemetry/audit-event-catalog.json`）・audit-core・golden fixture・Storeの `registered_types`（後続migrationで `SET LOCAL audit_store.write_context = 'migration'`）を同時に用意する。既存の出力が変わる場合だけ `LEGACY_ADAPTER_VERSION` を上げる。
- **配備順：Store（`audit-admin migrate` で `registered_types` を追加）→ relay（新catalog）→ Document producer**。
  - relayがStoreより先：relayのcatalogが期待するtypeをStoreが知らず、gateが `store_unregistered_type` で閉じ、**全配送が止まる**（`circuit_open`、relay serviceのloginなら `store_catalog_skew`）。
  - producerがrelayより先：そのtypeの行だけが `relay_catalog_skew` で保留され（`catalog_skew_held`・`relay_held`）、他の行は配送される。
  - どちらの場合も行はstagingと配送登録に残り、試行を消費せず、quarantineもされない。何も失われず、配備が揃えば自動で配送される（replayは要らない）。
- `public.audit_outbox_events` に触れるDocument migrationにはAuditのreviewが必須である。digest対象列（`event_id`〜`occurred_at`、`resource_type` を含む）のDROP・型変更は `BEGIN ATOMIC` のdigest関数への依存でmigrateが失敗する（意図した制約）。`DROP COLUMN ... CASCADE` は禁止。nullable列のADDは影響しない。
- stagingのUPDATE・DELETE・TRUNCATEは55000で拒否される。データ修正の正規手順は定義していない（未確認）。

## 8. 閲覧・export・verify・checkpoint

根拠：`src/bin/audit_admin.rs`、`src/files.rs`、`src/admin.rs`、migrationの `open_access` / `read_page`、設計 §8・§10

開示は2段階：`open_access` が `audit.access.intent_opened` を記録してwatermark Wを固定し、10分有効のtokenを返す。`read_page` はintentのcommitがWALで永続化された後に、同じ `session_user` の現在の束縛と権限があるときだけ開示する。`close_access` が件数とpage digestを記録する。CLIは3段階をすべて行う。

| 操作 | command | DB role / Audit権限 |
|---|---|---|
| 調査 | `audit-admin investigate [--filter JSON] [--page-size N] [--max-pages N]`（既定50/1） | reader / investigate |
| export | `audit-admin export --dir D [--filter JSON] [--page-size N] [--max-pages N]`（既定1000/100） | reader / export |
| chain export | `audit-admin export --dir D --operation verify [--seq-after N] [--seq-through N] [--checkpoint FILE]` | verifier / verify |
| identity chain | `audit-admin export --dir D --identity-chain [...]`（本文なし） | verifier / verify |
| DB内verify | `audit-admin verify [--from N] [--to N]`（違反ならexit 3） | verifier / verify |
| checkpoint | `audit-admin checkpoint --out FILE` | verifier / verify |

- filterのkey：`actor`、`event_ids`（100件まで）、`event_types`（1–16件、登録済みtypeかcontrol type）、`occurred_from`、`occurred_to`、`resource`、`seq_after`、`seq_through`、`source`。verify・identity chainは `seq_after` と `seq_through` だけ。control event（`audit.*`）はadminister権限の主体にだけ見える。
- `investigate` は本文をstdoutへ出す。terminalのlog・scroll bufferに注意する。
- exportは既存directoryへ `export.jsonl` と `manifest.json` を新規作成する。chainが壊れていればfileを書かず `{"chain_integrity":"broken","error":…}` を出してexit 1。manifestの `complete`・`expired_after_watermark`：intentがWを固定した後に `expire` / `purge-body` がcommitすると、W以下の行の本文が読取り前に消え、その証拠はWより後にある。そのexportは `complete: false` で、真正とは判定されない。**改めてexportする。**
- `verify` のDB内結果とchain列は真正性の証拠にならない（§9）。
- **checkpoint**：verifyを行い `audit.integrity.verified`（trigger `checkpoint`）を記録し、`ok` のときだけ `kp-audit-checkpoint-v1` のfile（`format, epoch, seq, chain, verified_through, genesis`）を作る。backupの直前（§10.1）と定期に取り、DB・backupとは別の場所・別の管理者の下に追記だけで保管する（同じ管理者のbackupは独立したanchorにならない）。具体的な保管先は未確認。

## 9. DB外の総合判定（`audit-admin assess`）

根拠：`crates/audit-store-postgres/src/assess.rs`、`src/files.rs`、`src/bin/audit_admin.rs`、`tests/cli_assess.rs`、`spec/telemetry/README.md`（recovery epochのDB外判定）

```sh
audit-admin assess --dir <EXPORT_DIR> --checkpoint <OOB_CHECKPOINT> [--anchor <OOB_CHECKPOINT_AT_SEQ_AFTER>] [--recovery-records <OOB_RECORDS>]
```

- DBに接続しない。export directoryをaudit-coreで検証し直し、帯域外のcheckpointとrecovery記録で `assess_recovery` の判定を出す。manifestからは作り方（operation、最初のintentの範囲、watermark）だけを使い、結果の主張は信用しない。genesisより後から始まるexportにはその位置のcheckpoint（`--anchor`）が要る（無ければexit 2）。
- 出力は1行のJSONで、判定code・seq・epoch・chain値・件数だけを持つ（本文・主体・resourceは出さない）。主なkey：`verdict`、`underlying_verdict`、`authenticated_through`、`chain_integrity`、`complete`、`findings`、`epochs`（最大64件）、`unverified_expiry_evidence`、`expired_after_watermark`。

| exit | verdict | 意味 |
|---|---|---|
| 0 | `authentic` | headが帯域外checkpointと一致し、消失が無く、失効の証拠も範囲内で検証済み |
| 4（要確認） | `authentic_through` / `unverified_expiry` / `no_checkpoint` / `lost` / `unverified_recovery` | checkpointまでだけ認証 / 失効の証拠が範囲外 / checkpointが無い / 帯域外記録の消失範囲で説明できる差異（上限不明を含む） / epochに帯域外記録が無い・食い違う |
| 5（拒否） | `tampered` / `unanchored` / `broken` | 改変 / filter付きの部分集合 / chain・manifestの検証失敗 |
| 2 | `store_behind` | manifestがexportを切った（`--seq-through`、または最初のintentのwatermarkがcheckpoint以上）のに、checkpointがexportの最後のseqより先。`underlying_verdict` にaudit-coreの判定（同じepochでは `tampered`）を出す。exportの最後のseq以前のcheckpointを使うか、checkpointまで改めてexportする |
| 1 | — | fileを読めない、checkpoint・recovery記録fileの形式不正 |

- 切っていないexportのheadより先にあるcheckpointは `store_behind` にせず、audit-coreの判定（同じepochでは `tampered`、exit 5）になる（古いexportか、Storeが行を失った。checkpoint以後に改めてexportしてもheadが届かなければStoreが行を失っている）。
- **recovery記録file（`kp-audit-recovery-records-v1`）**：JSON lines、1行1遷移、epoch順に追記、空行は無視。各行は全key必須の閉じた集合 `{"format":"kp-audit-recovery-records-v1","old_epoch":N,"new_epoch":N+1,"restored_head_seq":S,"restored_head_chain":"<64桁の小文字hex>","lost_upper":U}` で、消失範囲は `(S, U]`（`U = S` は消失なし）。**`"lost_upper":null` は上限不明**（Storeが範囲を限れなかった）を表し、そのepochは `lost` で決して `authentic` にならない。不正な行が1つでもあれば読込みが失敗する（exit 1）。行は `begin-recovery-epoch` が2行目に出す（§10.2）。incident参照などの付記はこの行に入れず、別に保管する。

## 10. backup・restore

根拠：設計 §11、両crateのREADME（restore、posture）、`crates/audit-relay/tests/recovery.rs`、`crates/audit-store-postgres/tests/store_recovery.rs`、migrationの `begin_recovery_epoch`

### 10.1 backup

1. `audit-relay run` を停止する（§4.1。exit 1の停止も想定内）。`audit-relay health` で `circuit.running` が0であることを確かめる。
2. Document DBを `pg_dump -Fc` する。**superuserで実行する**（`pg_read_all_data` のbackup loginはrelay postureの `predefined_role_member`、`audit_relay_owner` のmemberは `owner_member` になり、`run` が起動しなくなる）。Document DBにはstagingの理由文がある。
3. `audit-admin checkpoint --out <OOB_CHECKPOINT>` を取り、すぐにStore DBを `pg_dump -Fc` する（**ownerのmemberかsuperuser**で。`pg_read_all_data`・server file roleのloginは使わない）。間に他のStore操作が無ければdumpのheadとcheckpointが一致し、restore時に `match` になる（`recovery.rs` の手順）。dumpの後に取ったcheckpointはdumpのheadより先にあり、そのdumpのrestoreでは `store_behind` と消失範囲になる。
4. cluster移行に備えて `pg_dumpall --globals-only` も取る（role定義を含むので秘密として扱う）。
5. relayを再開する。

- 2つのdumpは**同じrelay停止区間の中で**取る。relayだけがDocumentのreceiptとStoreの行を結ぶので、停止中なら両dumpは互いに整合する（推論。2DBを組で戻す試験はしていない）。本手順ではDocument → Storeの順とする。
- dump fileはmode 0600、DB・logとは別の場所に置き、内容をlogに出さない。`pg_dump` はaudit-of-auditの対象外なので、権限で管理する。

### 10.2 Store restore（relayは手順5まで停止）

1. 復元先clusterにglobals（`roles.sql`、LOGIN role。cluster移行では `pg_dumpall --globals-only`）を先に作る。新しいDBへ `pg_restore --exit-on-error --single-transaction -d <NEW_STORE_DB> <STORE_DUMP_FILE>`。`--no-owner`・`--no-privileges`・`--no-acl`・`--role` は使わない。
2. `privileges.sql` を再適用し、`audit-admin posture` がcleanになることを確かめる。`audit-admin status` で `recovery_mode: true` を確かめる。fingerprintで検知できない復元（同じtimelineの物理・snapshot restore）は、接続を止めてから `audit-admin declare-recovery-pending --incident-code <CODE>`（maintain）を実行する。relayが後退を検知した場合は、relayの `report_regression` で既に `recovery_pending` になっている（同じDBで手順3から）。
3. `audit-admin verify --recovery`、`audit-admin export --identity-chain --recovery --dir <DIR> --checkpoint <LATEST_OOB_CHECKPOINT>` でDB外照合する（manifestの `checkpoint.comparison`）。
4. 期待値を見る（maintainerのlogin。`<N>` は `audit-relay health` の `stored.relay_max_seq`。`max_acked_store_seq` ではない）：
   ```sh
   audit-admin begin-recovery-epoch --checkpoint <LATEST_OOB_CHECKPOINT> --relay-max-seq <N> --preview
   ```
   1行目は `{"status":"preview","old_epoch",…,"restored_head_seq","restored_head_chain","classification","checkpoint_classification","lost_from_seq","lost_upper_seq","lost_upper_known"}`、**2行目が帯域外のrecovery記録の1行**である。2行目をそのまま帯域外のrecovery記録fileへ追記する。
5. 追記した行の値でepochを開始する：
   ```sh
   audit-admin begin-recovery-epoch --checkpoint <LATEST_OOB_CHECKPOINT> --relay-max-seq <N> \
     --expect-old-epoch <old_epoch> --expect-head-seq <restored_head_seq> \
     --expect-head-chain <restored_head_chain> --expect-lost-upper <lost_upper|unknown>
   ```
   `lost_upper` が `null` なら `--expect-lost-upper unknown` を渡す。1つでも食い違えば `audit-admin: audit store refused the call: expectation_mismatch`（exit 1）で何も変わらない。成功時の2行目は追記した行と同じ（同じ行の重複は判定を変えない）。`--checkpoint` も `--relay-max-seq` も無いと上限不明（`lost_upper: null`）のepochになり、`assess` は真正としないので、両方を渡す。
6. relayの `AUDIT_STORE_DATABASE_URL` を復元先に向けて再開し、operatorが `audit-relay reconcile --repair` を実行する（`delivered_missing` → pending → idempotentな再配送）。
7. 権限とretentionを再適用する（§10.3）。
8. `audit-admin verify`、新しい `checkpoint`、そのcheckpointまでのexportを `audit-admin assess --recovery-records <OOB_RECORDS>` で判定する（消失を伴うrecoveryの後は `lost`、記録の無いepochは `unverified_recovery`）。

- recovery modeで使えるのは `probe`、`store_status`、`posture_check`、receipt・lost rangeの参照、`verify_recovery`、`identity_chain_recovery_page`、`report_regression`、`declare_recovery_pending`、`begin_recovery_epoch` だけである。bind・grantはできないので、maintainer・verifierのloginと束縛はbackup時点で存在している必要がある。
- 計画的な移動（`pg_upgrade`、dump/restoreによる移行、計画的なpromotion）も同じ手順で、分類は `planned_move`（移動前のheadのcheckpointが `match`、消失範囲は空）になる。standbyではpublicationが閉じ、epochも開始できない。
- epochはchainを継続する。backup以降にStoreで生成されたcontrol event（閲覧intent、拒否、retention、replay、integrity、権限変更）は回復できない。RPOを縮めるにはWAL archiving/PITRを運用で選ぶ。

### 10.3 epoch後の権限・retentionの再適用

epoch開始後は `access_reapply_pending` になり、investigate・exportと本文を返すverifyが閉じる（identity chainとDB内verifyは開く）。次の両方で解除される。

- 管理者：帯域外の記録から、backup以後の権限の取消し（`revoke`）・retention revision（`set-retention`）を再適用し、DBAが束縛の解除（`unbind`）を再適用してから、`audit-admin record-access-reapplied` を実行する。
- maintainer：**有効なpolicy（最新revisionの `retain_days` がnone以外）それぞれ**について、epoch後に現行revisionで、policyのcutoff（実行時刻 − `retain_days`、UTC日）まで期限切れの本文を残さず失効させる。
  ```sh
  audit-admin expire --policy-id <POLICY_ID> --expected-revision <LATEST_REVISION> --cutoff <NOW_UTC_YYYY-MM-DDTHH:MM:SS.ffffffZ> --limit 1000
  ```
  `--cutoff` は実行時刻以降にする（それより前だと実効cutoffがpolicyのcutoffより狭まり、再適用として数えない）。`expired_count` が `--limit` 未満になるまで繰り返す。有効なholdによる `held` の拒否も再適用を満たす。有効なpolicyが無ければ `audit-admin confirm-retention-reapplied` を実行する（満たさない間は `retention_not_reapplied`）。

### 10.4 Document DB restore

- 同じ前提（globals、`pg_restore --exit-on-error --single-transaction`）で復元してから `crates/audit-relay/sql/roles.sql` を再適用し、`audit-relay health` の `installation.posture_violations` が空になることを確かめる。違反の間、`run`・`replay`・`reconcile` は起動しない。
- 古いDocument backupへ戻すと、Storeにあってsourceに無いevent（`store_only`）が生じ得る。reconcileは報告するが自動では削除しない。

## 11. retentionとpurge

根拠：設計 §9、migrationの `normalize_selector` / `set_retention_policy` / `expire` / `purge_body`

- 既定ではpolicyが無く、何も失効しない。年数は固定しない。
- `audit-admin set-retention --policy-id <ID> --selector '<JSON>' --retain-days <1–365000|none>`（administer）：毎回新しいrevisionを追加し、全内容をcontrol eventに記録する（出力 `{seq, revision}`）。`policy_id` は `[a-z0-9_]{1,64}`。selectorは `event_types`（**Storeの `registered_types` にある登録済みrelay typeだけ**。`audit.*` は不可）、`event_classes`（8 class）、`sources`（登録済みrelay source）で、各1–16件、少なくとも1つ。`none` は `not_expirable`。
- `audit-admin expire --policy-id <ID> --expected-revision <N> --cutoff <YYYY-MM-DDTHH:MM:SS.ffffffZ> --limit <1–1000>`（maintain）：実効cutoffは `least(cutoff, 実行時刻 − retain_days)` で、対象はorigin=relayだけ。`audit.retention.expired` を先に記録してから本文を削除し、identity・digest・chainは残す。拒否（`stale_revision` / `not_expirable` / `held`）は `audit.retention.expire_refused` として記録される。出力 `{status, seq, expired_count, effective_cutoff}`。`expired_count` が `limit` と等しい間は繰り返す。
- 失効済みeventを再配送すると `duplicate_expired` で、本文は戻らない。Documentのstagingは削除されない（v1はsource cleanupを提供しない）。
- `audit-admin purge-body --event-id <UUID> --reason-code <minimization_failure|prohibited_content|adapter_defect>`（maintain、origin=relayのみ）：最小化の失敗への個別対処。
- **legal hold**：`audit_store.legal_holds` はv1の予約された境界で、holdを作る・解除する関数もCLIも無い。有効なhold（`released_at IS NULL`）が1件でもあれば `expire` は `held` になる。`purge-body` はholdを見ない。purgeとholdの優先関係は依頼者の判断待ちである。

## 12. 制限と未検証事項

- 本trackは本番credential・本番migration・deploy・server運用を実施も検証もしていない。証拠はPostgreSQL 18.6（testcontainers）上の合成データの試験だけである。
- D3の限界：DB owner・superuserはchainを全体にわたって再計算でき、最後のcheckpoint以降のsuffixを消して「recovery」に見せかけることもできる。検出できるのは帯域外のcheckpoint・recovery記録との照合だけで、抵抗には署名・WORM・外部anchor（将来）が要る。
- guard triggerは事故防止で、境界は権限である。Document DBのsuperuserと `audit_relay` のownerは配送前のstagingと配送台帳を改変し得る。現在のDocument PoCは単一superuserでmigrateとserveを行っており、role分離を満たさない（handoff事項）。
- fingerprintは同じtimelineの物理restore・snapshot restoreを検知できない（relayの後退検知か `declare-recovery-pending` に依存）。
- audit-of-auditの対象外：`pg_dump`、owner・superuserの直接読取、recovery mode中の読取、CLI hostのexport file、REPLICATION loginの読取。拒否の記録は、呼出側がROLLBACKすると消える。
- repair登録のsource digestはrepair時点の値で、登録以前の改変は検出できない。ackに記録するStoreのepochはclaim前のprobeで観測した値である（relay停止を前提とする手順では差は生じない）。
- 依頼者の判断待ち：purgeとlegal holdの優先、`register-source-service` の記録へのsource fieldの追加（kindの判断）、ackのepochをreceiptで持つこと、repairの実適用件数をchainへ記録すること。
- 未検証：PostgreSQL 18.6以外、process監視（systemd等）と再起動方針、relayの複数instance同時運転、本番のtimeout値と処理量、Store DBの作成parameter、配送policyの変更手順、`registered_types` を追加する後続migration（v1はmigration 0001だけ）、stagingのデータ修正手順、帯域外のcheckpoint・recovery記録の保管先と管理者分離、Document DBとStore DBを組で戻すrestore、staging失敗をDocument側で監視する手段、`installation_incomplete` からの再導入、Search（`search_audit_outbox_events`）・Organizationのsource接続（設計 §13）。
