# audit-store-postgres

Audit Infrastructure v1の監査Store（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §7–§11、[決定記録](../../docs/decisions/2026-10-07-audit-envelope-store-integrity.md) D2/D3）のPostgreSQL実装である。別database・`audit_store` schema・migration ledger `audit_store_sqlx_migrations` を使う（`_sqlx_migrations` には触れない）。

- `PostgresAuditStore`：relayが使う `audit_core::AuditStore` port（idempotentなingest、probe）。構造化verdict（`rejected:<code>`、`conflict`）以外の失敗は、SQLSTATE・通信断・timeout・未登録type・recovery mode・posture違反を含めてすべてoutage（配送保留）になる。
- `admin::AuditAdmin`：調査・export・検証・retention・権限・recoveryのSQL関数のwrapper。
- `files`：export（JSONL）・manifest・checkpointを mode 0600 で新規作成する。
- bin `audit-admin`：運用CLI。

## 適用順とrole

| 手順 | 実行者 | 内容 |
|---|---|---|
| 1 | migrator（superuser、`AUDIT_STORE_MIGRATE_DATABASE_URL`） | `audit-admin migrate`。NOLOGIN・非superuserの `audit_store_owner` を作り、全objectをownerへ移し、PUBLICから剥奪する |
| 2 | DB owner | `sql/roles.sql`：capability role（NOLOGIN）とEXECUTE行列 |
| 3 | DB owner | operatorごと・serviceごとにLOGIN roleを作りcapability roleをGRANTする |
| 4 | DB owner | `sql/privileges.sql`：DBのPUBLIC CONNECT/TEMP剥奪、capability roleへのCONNECT、各LOGIN roleのtimeout。LOGIN roleの追加後とrestore後に毎回再実行する |
| 5 | ownerのmember | `audit-admin bootstrap-admin` / `bind`：LOGIN roleを主体（issuer, principal_id）へ束縛する |
| 6 | 管理者 | `audit-admin grant`：主体へAudit上の権限（investigate/export/verify/administer/maintain）を付与する |

| capability role | 関数 |
|---|---|
| `audit_store_ingest` | `ingest`、`probe`、`lookup_receipts`、`list_source_receipts`、`lookup_control_receipts` |
| `audit_store_relay_control` | `record_relay_control` |
| `audit_store_reader` | `open_access`（investigate/export）、`read_page`、`close_access` |
| `audit_store_verifier` | `open_access`（verify/identity_chain）、`read_page`、`close_access`、`verify`、`checkpoint`、`verify_recovery`、`identity_chain_recovery_page` |
| `audit_store_admin` | `change_access`、`set_retention_policy` |
| `audit_store_maintainer` | `expire`、`purge_body`、`begin_recovery_epoch`、`rebind_fingerprint`、`verify_recovery`、`identity_chain_recovery_page` |
| ownerのmemberのみ | `bootstrap_administrator`、`bind_principal`、`unbind_principal` |
| 全capability role | `store_status`（content-free） |
| verifier・admin・maintainer | `posture_check`（content-free） |

- 実行には、DB層のcapability roleと、束縛された主体のAudit権限の両方が要る。主体は `session_user` から解決し、引数では受け取らない。未束縛・権限不足・入力不正は `audit.access.denied` に記録される（入力値は記録しない）。
- `audit-admin` は migrate・bootstrap-admin・bind・unbind 以外で、superuserと `audit_store_owner` のmemberのsessionを拒否する。URLはDebug・errorに出さない。
- 表への直接DMLはどのroleにも与えない。guard triggerは事故防止で、境界は権限である（ownerとsuperuserは迂回できる。DB外のcheckpoint照合で検出する）。

## 実装上の判断

- 束縛（`principal_bindings`）と権限（`access_grants`）は削除しない履歴表である。unbind・revokeは `unbound_seq` / `revoked_seq` を一度だけ設定し、再束縛・再付与は新しい行を作る。
- control eventは設計§4.5の12種に、`audit.access.closed`（`close_access`）と `audit.recovery.fingerprint_rebound`（`rebind_fingerprint`）を加えた14種。`checkpoint` は `audit.integrity.verified`（`trigger: "checkpoint"`）として記録する。
- fingerprintは `pg_control_system().system_identifier`、DB oid、`pg_walfile_name(pg_current_wal_lsn())` の先頭8桁（timeline）。PostgreSQL 18.6ではいずれもPUBLIC実行可能で、非superuserのownerが計算できる。standbyではtimelineを `standby` とし、publicationは閉じたままになる。
- `ingest` の結果は構造化行 `(status, seq, envelope_digest, adapter_version, code)`。statusは stored / duplicate / duplicate_expired / duplicate_reprojected / conflict / rejected / recovery_required に加え、`registered_types` に無いtype（版ずれ）を `outage`（code `unregistered_type`）で返す。
- 権限確認を伴う関数は `denied` を例外ではなく結果行で返す（拒否記録をrollbackさせないため）。`read_page` の拒否は例外（42501、messageはcode）で、記録しない。
- `synchronous_commit=on` は関数レベルのSETではcommit前に戻るため、全writerが最初に呼ぶ `lock_head()` でtransaction-localに設定する。
- tokenは `gen_random_uuid()` 2個（CSPRNG由来244 bit）を連結した32 byteのhexで、Storeにはsha256だけを保存する。
- filterは設計§10.3のallowlistに `seq_after`（下限、排他）を加えた。verify/identity_chainは連続chainを読むため `seq_after` だけを許す。
- recovery modeでもcontent-freeな `store_status`・`posture_check`・receipt参照は使える。計画的移動の `rebind_fingerprint` はfingerprint不一致の状態で実行する必要があるため、許可3関数に加えて通る（posture・chain整合・maintain権限を要求し、旧/新fingerprintを記録する）。
- `begin_recovery_epoch` の消失範囲の上限は `max(checkpoint seq, relay最大seq, 復元head)`。主張値が復元head以下なら消失範囲は空として記録する。
- `registered_types` の変更は後続migrationで `SET LOCAL audit_store.write_context = 'migration'` を設定して行う。
- `legal_holds` はv1の予約で、追加する関数は無い。有効なholdが1件でもあれば `expire` は `held` になる。
- `verify` / `checkpoint` は書込を伴うので、設計§7.3どおり最初にhead lockを取り、走査の間保持する。その間のingestは最大5秒（関数の `lock_timeout`）待ってoutageとして保留される（失われない）。大きなStoreでは範囲指定の `verify --from/--to` を使う。

## restore（設計§11）

1. globals（`roles.sql`、LOGIN role）を先に用意し、`pg_restore --exit-on-error --single-transaction` で新しいDBへ復元する（`--no-owner` 等は使わない）。
2. `sql/privileges.sql` を再適用する。違反がある間、`begin_recovery_epoch` は `store_posture_invalid` で拒否する。
3. `audit-admin verify-recovery`、`audit-admin export-identity-chain-recovery --checkpoint <最新の帯域外checkpoint>` でDB外照合する。
4. 帯域外の記録へepoch遷移を追記し、`audit-admin begin-recovery-epoch --checkpoint <file> --relay-max-seq <N>`。

## 試験

PostgreSQL 18.6（testcontainers `postgres:18.6-bookworm`）で、合成データとroles.sqlで作ったLOGIN roleだけを使う。`AUDIT_STORE_TEST_DATABASE_URL` に使い捨てのsuperuser serverを指定すると、試験ごとにdatabaseを作る（restore試験はcontainer内の `pg_dump` を使うため、この場合はskipする）。
