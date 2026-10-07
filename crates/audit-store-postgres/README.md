# audit-store-postgres

Audit Infrastructure v1の監査Store（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §7–§11、[決定記録](../../docs/decisions/2026-10-07-audit-envelope-store-integrity.md) D2/D3）のPostgreSQL実装である。別database・`audit_store` schema・migration ledger `audit_store_sqlx_migrations` を使う（`_sqlx_migrations` には触れない）。

- `PostgresAuditStore`：relayが使う `audit_core::AuditStore` portの全実装（ingest、probe、report_regression、receipt参照、relay control記録）と、relay用の追加参照 `store_status()`・`lookup_lost_ranges()`。失敗は二値で、terminal（`conflict`、`rejected:<code>`）は `audit_core::IngestRow::into_result` だけが作る。それ以外（SQLSTATE、通信断、timeout、未登録type、recovery mode、posture違反、行の形の不正）はすべて `StoreError::Outage{code}`（配送保留）になる。`origin() != Relay` のenvelopeはDBへ送らず、audit-coreの `precheck_ingest` が `rejected:control_type_forbidden` にする。receipt・control receiptの行は列の値を `RawReceiptRow` / `RawControlReceiptRow` に読み、`ReceiptRow::decode` / `ControlReceiptRow::decode` だけで検査する（不正・不整合な行は `store_other`）。`IngestRow` は `audit_store.ingest` の結果列からだけ作る（`decode_ingest_row`、`tests/store_ingest.rs` で確認）。
- `admin::AuditAdmin`：調査・export・検証・retention・権限・recoveryのSQL関数のwrapper。
- `files`：export（JSONL）・manifest・checkpointを mode 0600 で新規作成する。chain exportは最初のintentのwatermarkまでintentを繰り返し、offlineで完全性（watermarkまで欠けがないこと）を検証する。
- bin `audit-admin`：運用CLI。

## 適用順とrole

| 手順 | 実行者 | 内容 |
|---|---|---|
| 1 | migrator（superuser、`AUDIT_STORE_MIGRATE_DATABASE_URL`） | `audit-admin migrate`。NOLOGIN・非superuserの `audit_store_owner` を作り、全objectをownerへ移し、PUBLICから剥奪する |
| 2 | DB owner | `sql/roles.sql`：capability role（NOLOGIN）とEXECUTE行列 |
| 3 | DB owner | operatorごと・serviceごとにLOGIN roleを作りcapability roleをGRANTする（role名は `[a-z_][a-z0-9_$]{0,62}`） |
| 4 | DB owner | `sql/privileges.sql`：DBのPUBLIC CONNECT/TEMP剥奪、capability roleへのCONNECT、`ALTER DATABASE … SET synchronous_commit = on`、各LOGIN roleのtimeoutと `synchronous_commit = on`。LOGIN roleの追加後とrestore後に毎回再実行する |
| 5 | ownerのmember | `audit-admin bootstrap-admin` / `bind`：LOGIN roleを主体（issuer, principal_id）へ束縛する。relay serviceは `service` / `audit-relay` など |
| 6 | ownerのmember | `audit-admin register-source-service`：service主体をrelay sourceのsource serviceとして登録する（Document用 `service/audit-relay` はmigrationが登録済み） |
| 7 | 管理者 | `audit-admin grant`：主体へAudit上の権限（investigate/export/verify/administer/maintain）を付与する |

| capability role | 関数 |
|---|---|
| `audit_store_ingest` | `ingest`、`probe`、`report_regression` |
| `audit_store_reconciler` | `lookup_receipts`、`list_source_receipts`、`lookup_control_receipts`、`lookup_lost_ranges`、`store_status` |
| `audit_store_relay_control` | `record_relay_control` |
| `audit_store_reader` | `open_access`（investigate/export）、`read_page`、`close_access` |
| `audit_store_verifier` | `open_access`（verify/identity_chain）、`read_page`、`close_access`、`verify`、`checkpoint`、`verify_recovery`、`identity_chain_recovery_page` |
| `audit_store_admin` | `change_access`、`set_retention_policy`、`record_access_reapplied` |
| `audit_store_maintainer` | `expire`、`purge_body`、`confirm_retention_reapplied`、`declare_recovery_pending`、`begin_recovery_epoch`、`verify_recovery`、`identity_chain_recovery_page` |
| ownerのmemberのみ | `bootstrap_administrator`、`bind_principal`、`unbind_principal`、`register_source_service` |
| verifier・admin・maintainer | `store_status`、`posture_check`（content-free） |

LOGIN roleの割当（設計§10.1）：relay serviceはingest + relay_control + reconciler、relay operatorはrelay_control + reconcilerだけ（ingestを持たない）。ingest roleのmemberはsource serviceとして束縛されていなければならず（`posture_check` の `ingest_member_not_source_service`）、ownerのmemberはcapability roleを持てない。

- 実行には、DB層のcapability roleと、束縛された主体のAudit権限の両方が要る。主体は `session_user` から解決し、引数では受け取らない。未束縛・権限不足・入力不正は `audit.access.denied` に記録される（入力値は記録しない）。未束縛sessionと所有者操作のactorは issuer `db_role`、principal_id = session roleで、issuer `db_role` への束縛・登録は拒否する。
- ingest経路の拒否（未束縛、source serviceでない）は `denied`（outage）で、連続する拒否はsession roleごとに1分1件へ集約して記録する（`denial_streaks`）。
- `audit-admin` は migrate・bootstrap-admin・bind・unbind・register-source-service 以外で、superuserと `audit_store_owner` のmemberのsessionを拒否する。`options` を含むURLは接続前に拒否し、接続後に `SHOW synchronous_commit` が `on` でなければ拒否する。URLはDebug・errorに出さない。
- 表への直接DMLはどのroleにも与えない。guard triggerは事故防止で、境界は権限である（ownerとsuperuserは迂回できる。DB外のcheckpoint照合で検出する）。

## 実装上の判断

- 束縛（`principal_bindings`）・権限（`access_grants`）・source service（`source_services`）は削除しない履歴表である。unbind・revokeは `unbound_seq` / `revoked_seq` を一度だけ設定し、再束縛・再付与は新しい行を作る。
- control eventは監査catalogのstore originの11種とrelay_control originの3種で、すべてcatalogで検証する（`tests/control_catalog.rs`）。`checkpoint` は `audit.integrity.verified`（`trigger: "checkpoint"`）として記録し、その記録自身の位置（epoch, seq, chain）を返す。計画的移動はこのcheckpointがheadと一致することで判定する。
- `retention` の拒否（stale_revision / not_expirable / held）は `audit.retention.expire_refused` として記録する。`audit.retention.expired` は `expired_set_digest`（`sha256('kp-audit-expired-set-v1' || int8send(seq)…)`、audit-coreと照合）を持つ。
- fingerprintは `pg_control_system().system_identifier`、DB oid、`pg_walfile_name(pg_current_wal_lsn())` の先頭8桁（timeline）を、それぞれint8の10進表記（`audit.recovery.epoch_started` の `int8_text` kind）で持つ。standbyではtimelineを `standby` とし、publicationは閉じたままになる（epochも開始できない）。
- `ingest` の結果は構造化行 `(status, seq, envelope_digest, adapter_version, code)`。順序は head lock → recovery（`recovery_required`）→ source service（`denied`）→ posture（`outage`/`posture_invalid`）→ 構造検査（`rejected`）→ `registered_types`（source・type・adapter_version・source_formatの一致、無ければ `outage`/`unregistered_type`）→ source別のservice確認 → 重複/衝突/保存。recovery中は拒否も記録しない。
- `probe(source, adapter_version, types, last_ack)` はstate（operational / recovery_mode / posture_invalid / read_only）、未登録type、last_ackが解決しないこと（regression）、最後の `integrity.verified` のseqを返す。read_onlyはlockを取らずに判定する。
- `report_regression` は解決しないreceiptだけを `recovery_pending`（reason `regression`、報告されたseq・event_id・digest・報告者・時刻・当時のhead）にする。`declare_recovery_pending(incident_code)` はmaintainerが既知の事故で同じ状態にする（reason `declared`）。どちらもrecovery中に使え、解除は `begin_recovery_epoch` だけである。
- 権限確認を伴う関数は `denied` を例外ではなく結果行で返す（拒否記録をrollbackさせないため）。`read_page` の拒否は例外（42501、messageはcode）で、記録しない。
- `synchronous_commit=on` は関数レベルのSETではcommit前に戻るため、関数定義には付けず（posture違反 `function_synchronous_commit`）、全writerが最初に呼ぶ `lock_head()` でtransaction-localに設定する。DBとLOGIN roleの既定値もpostureで確認する。
- `read_page` はintentの記録がWALでflushされるまで開示しない。最初の呼出しで `pg_current_wal_insert_lsn()` を一度だけ取り、flush位置がそれ以上になるまで最大約2秒待ち、間に合わなければ再試行可能な `intent_not_durable`（40001）を返す（`AuditAdmin::read_page` は再試行する）。
- tokenは `gen_random_uuid()` 2個（CSPRNG由来244 bit）を連結した32 byteのhexで、Storeにはsha256だけを保存する。filter・範囲・page sizeはchainされたintent本体から読み直し、`access_intents` の行と照合する。
- filterは設計§10.3のallowlistに `seq_after`（排他の下限）と `seq_through`（包含の上限、watermark以下）を加えた。verify/identity_chainは連続chainを読むためこの2つだけを許す。値は閉じた文法（event_type、source urn、resource_ref、主体文字集合、event type listは1–16件）以外を `invalid_input` で拒否し、記録に任意文字列を残さない。event typeは、文法に合っても登録済みrelay type（`registered_types`）かcatalogのcontrol type（`is_control_type`、catalogとの一致を試験）でなければ拒否する（設計§10.2）。retention selectorのevent typeは登録済みrelay typeだけを許す。issuer `db_role` への権限付与（`change_access`）も拒否する。
- `verify(from, to)` は範囲（最大10,000,000行）をhead lockなしで単一snapshotで走査し、記録するときだけlockを取る。head照合は範囲の終端がheadのときだけ行う。
- export行は10 key（`expired_by_seq` を含む）。`audit-admin export --identity-chain` は最初のintentでwatermarkを固定し、以降 `{seq_after, seq_through: W}` で残りを読み、manifestに全intentを列挙する。
- recovery mode（fingerprint不一致または `recovery_pending`）で通るのは `probe`、`store_status`、`posture_check`、`lookup_receipts`、`list_source_receipts`、`lookup_control_receipts`、`lookup_lost_ranges`、`verify_recovery`、`identity_chain_recovery_page`、`report_regression`、`declare_recovery_pending`、`begin_recovery_epoch` だけで、他はすべて `store_recovery_required`（KA001）になる（`tests/store_recovery.rs` で全関数を確認）。recovery用の関数はrecovery外では `not_in_recovery` で拒否する。
- `begin_recovery_epoch(checkpoint?, relay_max_seq?, expected)` は復元chainを再検証し、classification（`regression` / 計画的移動 `planned_move` / `restore`）、checkpointの分類（match / ahead / mismatch / epoch_mismatch / store_behind。chainが一致してepochだけ異なる場合が `epoch_mismatch`）、identity範囲digest、消失範囲 `(restored_head, max(checkpoint seq, relay最大seq, 報告seq, restored_head)]`、regressionの証拠、旧/新fingerprintを記録する。旧 `rebind_fingerprint` は廃止し、計画的移動も同じepochとして扱う。
- `expected` は帯域外のrecovery記録（旧epoch、復元headのseqとchain、消失範囲の上限）である。Storeが計算した実際の値と4つとも一致した場合だけepochを開始し、食い違えば `refused`/`expectation_mismatch` で何も変えない。期待値なしの呼出しはpreview（`refused`/`expectation_required`）で、実際の値だけを返す（`AuditAdmin::preview_recovery_epoch`、`audit-admin begin-recovery-epoch --preview`）。recovery中の拒否は記録しない。
- epoch後は `access_reapply_pending` になり、investigate/exportは `access_reapply_pending`（55000）で拒否する（verify/identity_chainは可）。administratorの `record_access_reapplied` と、retentionの再適用（有効なpolicyそれぞれの現行revisionで `expire` を再実行するか、有効なpolicyが無いことを `confirm_retention_reapplied` で記録する）の両方で解除される。
- `registered_types` の変更は後続migrationで `SET LOCAL audit_store.write_context = 'migration'` を設定して行う。
- `legal_holds` はv1の予約で、追加する関数は無い。有効なholdが1件でもあれば `expire` は `held` になる。

## restore（設計§11）

1. globals（`roles.sql`、LOGIN role）を先に用意し、`pg_restore --exit-on-error --single-transaction` で新しいDBへ復元する（`--no-owner` 等は使わない）。
2. `sql/privileges.sql` を再適用する。違反がある間、`begin_recovery_epoch` は `store_posture_invalid` で拒否する。
3. `audit-admin verify --recovery`、`audit-admin export --identity-chain --recovery --dir D --checkpoint <最新の帯域外checkpoint>` でDB外照合する。
4. `audit-admin begin-recovery-epoch --checkpoint <file> --relay-max-seq <N> --preview` で復元head・消失範囲を確認し、その値を帯域外の記録へepoch遷移として追記してから、`--expect-old-epoch` `--expect-head-seq` `--expect-head-chain` `--expect-lost-upper` に記録の値を渡して `begin-recovery-epoch` を実行する（食い違えば `expectation_mismatch` で拒否される）。
5. `audit-admin record-access-reapplied`（administrator）と、retentionの再実行または `audit-admin confirm-retention-reapplied`（maintainer）。

同じDBでのregression・既知の事故は、`report_regression`（relay）または `audit-admin declare-recovery-pending --incident-code CODE`（maintainer）から手順3以降を行う。

## 試験

PostgreSQL 18.6（testcontainers `postgres:18.6-bookworm`）で、合成データとroles.sqlで作ったLOGIN roleだけを使う。`AUDIT_STORE_TEST_DATABASE_URL` に使い捨てのsuperuser serverを指定すると、試験ごとにdatabaseを作る（restore試験はcontainer内の `pg_dump` を使うため、この場合はskipする）。

`tests/data/store-envelope-golden.json` はaudit-coreの全受理fixture（event idはfixture名から導出）をStoreへ保存したときの `kp-audit-jsonb-sha256-v1` envelope digestをadapter_versionごとに固定する。append-onlyで、既存sectionは編集しない。digestが変わる変更には `LEGACY_ADAPTER_VERSION` の更新、`registered_types` の追加、新しいsectionが要る。更新を忘れたまま同じeventを再配送すると、Storeは上書きせず `conflict` にする（`tests/store_golden.rs`）。
