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
| 2 | superuser（`AUDIT_RELAY_MIGRATE_DATABASE_URL`） | `audit-relay migrate`。事前検査（stagingの列・型、Document ledger）→ `audit_relay_owner`（NOLOGIN・非superuser）とschema → backfill → 登録trigger・guard |
| 3 | DB owner | `sql/roles.sql`：`audit_relay_worker` / `audit_relay_operator` とEXECUTE行列、LOGIN roleのtimeout。LOGIN role追加後とDocument DBのrestore後に再実行する |
| 4 | Store側 | `audit-store-postgres` の手順。relay serviceのStore loginを `service/audit-relay`（Documentのsource serviceとして登録済み）に束縛する |

- migrate中は `audit_outbox_events` を `SHARE ROW EXCLUSIVE` でlockするため、業務のINSERTはcommitまで待つ（`lock_timeout` 10秒）。
- `audit_outbox_events` の被digest列（`event_id`〜`occurred_at`、`resource_type` を含む）をDROP・型変更するDocument migrationは、`BEGIN ATOMIC` のdigest関数の依存で失敗する（意図した制約）。`DROP COLUMN ... CASCADE` は禁止。nullable列のADDは影響しない。

## roleとcredential

| command | `AUDIT_SOURCE_DATABASE_URL`（Document） | `AUDIT_STORE_DATABASE_URL`（Store） |
|---|---|---|
| `run` | `audit_relay_worker` を持つservice login | relay service：`audit_store_ingest`＋`audit_store_relay_control`＋`audit_store_reconciler`、`service/audit-relay` に束縛 |
| `health` / `reconcile` | worker（read-only） | operator：`audit_store_relay_control`＋`audit_store_reconciler`（ingestは持たない）、本人の主体に束縛 |
| `reconcile --repair` / `replay` | operator本人の `audit_relay_operator` login | 同上（operator本人のStore login） |

- operatorのStore loginはprobeできないので、`health` の `stored.missing_types`（catalog skew）は `null` になる。relay serviceのloginで実行すると、Storeが未登録のcatalog typeを返し、空でなければ `store_catalog_skew` を警報する。

- capability roleは表の権限を持たず、definer関数（`SECURITY DEFINER`、`search_path = pg_catalog, pg_temp`、owner `audit_relay_owner`）だけを実行する。PUBLICには何も与えない。
- `migrate` 以外は、superuserと `audit_relay_owner`（Store側は `audit_store_owner`）のmemberのsessionを拒否する。sourceとStoreが同一database（`system_identifier` と `current_database()`）なら起動しない。URLの `options` を拒否し、全接続で `synchronous_commit = on` を確認する。URL・credentialはerrorに出さない。
- `run`・`reconcile`・`replay` は `audit_relay.posture_check()` に違反がある間は起動しない（`health` は警報として出す）。

## command

```text
audit-relay migrate
audit-relay run                                  # SIGTERM / Ctrl-C で有界にdrainして停止
audit-relay health [--forecast] [--reconcile]    # JSON。produced/delivered/stored/verifiedを分ける
audit-relay reconcile [--repair]                 # 1 runにつき audit.reconciliation.completed を1件記録
audit-relay replay --event-id <uuid>             # Storeへ replay_requested を記録してから戻す
```

`run` の設定は環境変数（`AUDIT_RELAY_BATCH_SIZE` 32、`AUDIT_RELAY_MAX_IN_FLIGHT` 4、`AUDIT_RELAY_LEASE_MS` 30000、`AUDIT_RELAY_RENEW_MS` 9000、`AUDIT_RELAY_INGEST_TIMEOUT_MS` lease/3未満 など。`src/config.rs`）。policyの既定は試行16、lease 1–120秒、backoff 1–300秒、`outage_streak` 上限64。

## 失敗の扱い

- quarantine（終端）は、Storeの構造化verdict（`conflict`、`rejected_<code>`。`audit_core::IngestRow::into_result` だけが作る）とrelay側の判定（`source_digest_mismatch`、`actor_mismatch`、catalog不適合）だけ。source改変は先に `audit.integrity.source_mismatch_detected` を記録し（event・codeごとに1回）、記録できなければ保留する。
- quarantine codeはStoreのcode形式 `[a-z0-9_]{1,64}` に従う（replayが `quarantine_code` として記録できるように）。relayの拒否codeはそのまま、Storeの拒否は `rejected_<code>` を64 byteで切る。
- それ以外（通信断、timeout、全SQLSTATE、結果不明、recovery mode、後退、posture違反、未登録type、ingest主体の拒否）は外部障害として試行を返却して保留し、circuit breakerを開く。breakerはingestの構造化結果でだけ閉じる。
- relayのcatalogより新しいtype・fieldは `relay_catalog_skew` として保留し、healthで警報する。reconcileはこの保留を独立したclass `relay_catalog_skew`（pendingとは別、`count_relay_catalog_skew`）として数え、警報を出す（`audit_relay.delivery_view` の `last_error_code` で判定する）。
- `outage_streak` は、audit-coreが残余とするoutage（`store_internal`、`store_other`。`OutageCode::counts_toward_outage_streak`）が、別の配送の成功を挟んで続いた場合だけ数える。上限で `outage_suspected_event_specific` としてquarantineする。
- claimの前のgate（`BreakerAdmission`）：catalogの期待（source、adapter_version、type一覧。`ProbeExpectation::from_catalog`）と、Storeの現在のrecovery epochで最後にackしたreceiptを渡してprobeする。epochが前回と違えばそのepochのack headで2回目のprobeをする。順序は、同じepochで検知済みの後退（sticky）→ Storeの状態（recovery mode、posture、read-only）→ 後退 → 未登録type（`store_unregistered_type`）。
- 後退（operationalなStoreが最後にackしたreceiptを解決できない。fingerprintで見えないin-place restoreなど）を検知すると、`audit_store.report_regression` で報告し（Storeが再確認して `recovery_pending` にする）、recovery epochが変わるまでclaimを止める。fingerprintで検知されたrestoreはrecovery modeとして止まり、operatorが `begin-recovery-epoch --relay-max-seq` でrelayの最大seqを記録する（`--preview` で確認した復元head・消失範囲を帯域外の記録に書き、その値を `--expect-*` で渡す。Storeは食い違う記録を拒否する）。

## 限界

- Document DBのsuperuser・`audit_relay` の所有者は、trigger・FKを迂回して配送前のstagingと配送台帳を改変できる。guardは事故とDDLを伴わない不正DMLの防止である（設計§5.3）。
- `reconcile` は配送履歴（replay・repair）をStoreのcontrol eventに照合する。replay行は、同じevent・同じrecovery epoch・同じquarantine codeの `audit.delivery.replay_requested` に1対1で（同じcontrol eventを2行が使えば2行目は不一致）、repair行は同じepochの `audit.reconciliation.completed`（mode `repair`）に対応しなければならない。対応しない行は、そのepochでStoreが宣言した消失範囲（`lookup_lost_ranges`）に入っていれば `replay_record_lost`、それ以外は `unaudited_replay`。`count_replay_record_lost` は `audit.reconciliation.completed` に記録される。
- `repair_ack_stored` のfenceは、渡されたcommitmentとserver側の再計算値の一致を確かめる。Storeの事実そのものはDocument側で検証できないので、CLIがStoreのreceiptを渡す。

## 試験

PostgreSQL 18.6（testcontainers `postgres:18.6-bookworm`）の1 containerに、Document DBとStore DBの2 databaseを作る。runtimeの経路はroles.sqlで作ったLOGIN roleで実行し、producerは合成行のINSERTで模擬する。子processのSIGKILL、`pg_dump`/`pg_restore` による復元も含む。
