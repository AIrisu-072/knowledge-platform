# audit-store-postgres

Audit Infrastructure v1の監査Store（[設計](../../docs/superpowers/specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) §7–§11、[決定記録](../../docs/decisions/2026-10-07-audit-envelope-store-integrity.md) D2/D3）のPostgreSQL実装である。別database・`audit_store` schema・migration ledger `audit_store_sqlx_migrations` を使う（`_sqlx_migrations` には触れない）。

- `PostgresAuditStore`：relayが使う `audit_core::AuditStore` portの全実装（ingest、probe、report_regression、receipt参照、relay control記録）と、relay用の追加参照 `store_status()`・`lookup_lost_ranges()`。失敗は二値で、terminal（`conflict`、`rejected:<code>`）は `audit_core::IngestRow::into_result` だけが作る。それ以外（SQLSTATE、通信断、timeout、未登録type、recovery mode、posture違反、行の形の不正）はすべて `StoreError::Outage{code}`（配送保留）になる。`origin() != Relay` のenvelopeはDBへ送らず、audit-coreの `precheck_ingest` が `rejected:control_type_forbidden` にする。receipt・control receiptの行は列の値を `RawReceiptRow` / `RawControlReceiptRow` に読み、`ReceiptRow::decode` / `ControlReceiptRow::decode` だけで検査する（不正・不整合な行は `store_other`）。`IngestRow` は `audit_store.ingest` の結果列からだけ作る（`decode_ingest_row`、`tests/store_ingest.rs` で確認）。
- `admin::AuditAdmin`：調査・export・検証・retention・権限・recoveryのSQL関数のwrapper。
- `files`：export（JSONL）・manifest・checkpointを mode 0600 で新規作成する。chain exportは最初のintentのwatermarkまでintentを繰り返し、offlineでaudit-coreの `verify_export_complete` / `verify_identity_chain_complete`（watermarkまで欠けがなく、Wを超える失効証拠を指す行が無いこと）で検証する（`files::verify_chain_export`）。manifestの `chain_integrity` は `intact` / `unanchored`（audit-coreの `ChainIntegrity`）で、chainが壊れていればfileを書かず、`audit-admin` は `{"chain_integrity":"broken","error":…}` を出して失敗する。
- intentがWを固定した後に `expire` / `purge_body` がcommitすると、W以下の行の本文がpage読取り前に消え、その行はWより後の証拠を指す。このexportを完全とは扱わない：`complete: false`、`expired_after_watermark` に件数を出し、その行は未検証の失効証拠として数える（`assess_recovery` は `Authentic` にしない）。証拠を含めて検証するには、改めてexportする（`tests/store_integrity.rs` で、intent → expire → 読取りの順に決定的に試験する）。
- `assess`：export directoryのDB外の総合判定（`audit-admin assess`、DB接続なし。下記）。
- bin `audit-admin`：運用CLI。終了codeは 0 成功（`assess` は `authentic`）、1 失敗、2 使い方の誤り（`assess` の `store_behind` を含む）、3 postureまたは検証の違反（`posture`、`verify`、`verify --recovery`）、4 `assess` の要確認、5 `assess` の拒否（下記）。

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
- ingest経路の拒否（未束縛、source serviceでない）は `denied`（outage）で、連続する同じcode・同じactorの拒否はsession roleごとに1分1件へまとめて記録する（`denial_streaks`。actorは記録時に解決した主体を保持する）。間引きはしない：まとめた件数は次の記録の `suppressed_since_last` としてchainに残り、codeかactorが変わるときと、そのloginの成功で連続が終わるときにも、未記録の件数を1件の記録（件数はその記録自身を除いた数）として先に書く。連続が止まった場合も、集約window（1分）を過ぎた全loginの未記録件数を、次のすべての追記（control event、relay eventのingest）とrelayのprobeの前に、そのlogin・code・actorの記録として書く（`flush_denial_streaks`）。verify・checkpoint・開示intent・expire・purgeは、記録の前に未記録件数をwindowに関係なくすべて書く（証拠より前に全拒否がchainにある）。未記録の件数は `store_status` の `denials_pending` に出る（relay healthの `stored.denials_pending`、警報 `store_denials_pending`）。各記録は自身と `suppressed_since_last` 件の拒否を表す。recovery中は従来どおり記録しない（連続はそのまま残る）。
- replay（`audit.delivery.replay_requested`）とrepair mode の `audit.reconciliation.completed` は、`audit_store_ingest` を持つlogin（relay service）からは記録しない（`insufficient_capability` で拒否し記録する）。operator自身のlogin（relay_control + reconciler）だけが記録でき、特権操作のactorは操作したoperatorになる（設計§6.4、§10.1）。
- `audit-admin` は migrate・bootstrap-admin・bind・unbind・register-source-service 以外で、superuserと `audit_store_owner` のmemberのsessionを拒否する。`options` または `options[<設定>]` を含むURLと、空でない環境変数 `PGOPTIONS` は接続前に拒否し（session設定でtimeoutやsynchronous_commitを変えられるため）、接続後に `SHOW synchronous_commit` が `on` でなければ拒否する。URLはDebug・errorに出さない。
- 表への直接DMLはどのroleにも与えない。guard triggerは事故防止で、境界は権限である（ownerとsuperuserは迂回できる。DB外のcheckpoint照合で検出する）。表の権限を迂回する定義済みrole（`pg_read_all_data`：全本文・intentを読める、`pg_write_all_data`：definer GUCを設定して権限表を書ける、`pg_maintain`）をこのDBへ接続できる非superuserのloginが持つと、postureは `predefined_role_member` を報告する。server上のfileとprogramへのaccess（`pg_read_server_files`・`pg_write_server_files`・`pg_execute_server_program`：clusterの全DBのdata fileを読み書きでき、`COPY ... PROGRAM` でOSから迂回できる）は、このDBへ接続できるかに関係なく、すべての非superuserのloginについて `predefined_role_member` を報告する。列単位の権限も `column_privilege` で報告する。capability loginのtimeoutは設定済みで0でないこと（DB単位の設定がrole全体の設定に優先する）を確認する。**backup（`pg_dump`）はownerのmemberかsuperuserで実行する**（`pg_read_all_data` のbackup loginも、server fileのroleを持つloginも使わない）。
- **REPLICATION属性のloginはこの境界の外にある**：streaming replicationやbase backup（`pg_basebackup`）のloginは、replication protocolでWAL・data fileとしてclusterの全内容（全本文・intent・権限表）をどの権限にも依らず読める。postureは、このDBへ接続できるかに関係なく、REPLICATION属性を持つすべての非superuserのloginを `replication_login` として報告する（server fileのroleと同じ。posture違反の間はingest・publicationが止まる）。replicationはsuperuserと同じく境界の外の操作として、superuserのloginで専用のreplication基盤（`pg_hba.conf` の `replication` 行でreplica・backup hostだけを許可する）に限って行い、その基盤への到達と資格情報をStoreの境界とは別に管理する。

## 実装上の判断

- 束縛（`principal_bindings`）・権限（`access_grants`）・source service（`source_services`）は削除しない履歴表である。unbind・revokeは `unbound_seq` / `revoked_seq` を一度だけ設定し、再束縛・再付与は新しい行を作る。
- control eventは監査catalogのstore originの11種とrelay_control originの3種で、すべてcatalogで検証する（`tests/control_catalog.rs`）。`checkpoint` は `audit.integrity.verified`（`trigger: "checkpoint"`）として記録し、その記録自身の位置（epoch, seq, chain）を返す。計画的移動はこのcheckpointがheadと一致することで判定する。
- `retention` の拒否（stale_revision / not_expirable / held）は `audit.retention.expire_refused` として記録する。`audit.retention.expired` は `expired_set_digest`（`sha256('kp-audit-expired-set-v1' || int8send(seq)…)`、audit-coreと照合）を持つ。
- fingerprintは `pg_control_system().system_identifier`、DB oid、`pg_walfile_name(pg_current_wal_lsn())` の先頭8桁（timeline）を、それぞれint8の10進表記（`audit.recovery.epoch_started` の `int8_text` kind）で持つ。standbyではtimelineを `standby` とし、publicationは閉じたままになる（epochも開始できない）。
- `ingest` の結果は構造化行 `(status, seq, envelope_digest, adapter_version, code)`。順序は head lock → recovery（`recovery_required`）→ source service（`denied`）→ posture（`outage`/`posture_invalid`）→ 構造検査（`rejected`）→ `registered_types`（source・type・adapter_version・source_formatの一致、無ければ `outage`/`unregistered_type`）→ source別のservice確認 → 重複/衝突/保存。recovery中は拒否も記録しない。
- `probe(source, adapter_version, types, last_ack)` はstate（operational / recovery_mode / posture_invalid / read_only）、未登録type、last_ackが解決しないこと（regression）、検証の被覆（`last_verified_seq`、下記）を返す。read_onlyはlockを取らずに判定する。
- `store_status` / `probe` の検証状態は最新の記録ではなく被覆である（`verification_coverage`）。被覆が依存する `audit.integrity.verified` は `record_verified` だけが書くので、`record_verified` がhead lockの下で同じ関数で `verification_state`（1行）を更新し、`probe`（relayのcycleごと）と `store_status` はその行を読む（毎回の再帰走査をしない）。`last_verified_seq` は、最後の違反以後の `ok` の検証のうちgenesisから連続して（重なりを含めて）つながる範囲の最大seq、`last_verified_outcome` は、違反の後にgenesisからその走査のheadまで（`from_seq` 1、`to_seq` = watermark）の `ok` の検証が1件記録されるまで `violations` のままである。部分範囲の検証は、つなげてheadへ届いても違反を解除しない。`head_seq - last_verified_seq` が検証の遅れになる。
- `report_regression` は解決しないreceiptだけを `recovery_pending`（reason `regression`、報告されたseq・event_id・digest・報告者・時刻・当時のhead）にする。`declare_recovery_pending(incident_code)` はmaintainerが既知の事故で同じ状態にする（reason `declared`）。どちらもrecovery中に使え、解除は `begin_recovery_epoch` だけである。
- 権限確認を伴う関数は `denied` を例外ではなく結果行で返す（拒否記録をrollbackさせないため）。`read_page` の拒否は例外（42501、messageはcode）で、記録しない。
- `synchronous_commit=on` は関数レベルのSETではcommit前に戻るため、関数定義には付けず（posture違反 `function_synchronous_commit`）、全writerが最初に呼ぶ `lock_head()` でtransaction-localに設定する。DBとLOGIN roleの既定値もpostureで確認する。
- `read_page` はintentの記録がWALでflushされるまで開示しない。最初の呼出しで `pg_current_wal_insert_lsn()` を一度だけ取り、flush位置がそれ以上になるまで最大約2秒待ち、間に合わなければ再試行可能な `intent_not_durable`（40001）を返す（`AuditAdmin::read_page` は再試行する）。commitを伴わないWAL（読取りのheap pruning、vacuum）は後のcommit・WAL pageの充填・checkpointまでflushされないので、flushが遅れていれば内容の無い非transactionalなWAL message（`pg_logical_emit_message`、固定prefix `kp-audit-store-durable`、空の内容、flush付き）でflushしてから開示する（EXECUTEが剥奪されていれば待つだけ）。
- tokenは `gen_random_uuid()` 2個（CSPRNG由来244 bit）を連結した32 byteのhexで、Storeにはsha256だけを保存する。filter・範囲・page sizeはchainされたintent本体から読み直し、`access_intents` の行と照合する。
- filterは設計§10.3のallowlistに `seq_after`（排他の下限）と `seq_through`（包含の上限、watermark以下）を加えた。verify/identity_chainは連続chainを読むためこの2つだけを許す。値は閉じた文法（event_type、source urn、resource_ref、主体文字集合、event type listは1–16件）以外を `invalid_input` で拒否し、記録に任意文字列を残さない。event typeは、文法に合っても登録済みrelay type（`registered_types`）かcatalogのcontrol type（`is_control_type`、catalogとの一致を試験）でなければ拒否する（設計§10.2）。retention selectorのevent typeは登録済みrelay typeだけを許す。issuer `db_role` への権限付与（`change_access`）も拒否する。
- `verify(from, to)` は範囲（最大10,000,000行）をhead lockなしで単一snapshotで走査し、記録するときだけlockを取る。head照合は範囲の終端がheadのときだけ行う。範囲は `1 ≤ from ≤ to ≤ head` でなければならず、headを越える `to`・headより後の `from`・空の範囲は `invalid_input` で拒否する（存在しない行を欠落として `violations` を記録しないため）。記録する `head_seq` / `head_epoch` / `head_chain` は範囲内で実在する最後の行のものである（終端の行が欠けていれば、その前の行）。
- `audit-admin verify` / `verify --recovery` は結果を出力した後、`outcome` が `ok` でなければ（`violations`）終了code 3で終わる（`posture` と同じ。定期実行の失敗検知用）。
- export行は10 key（`expired_by_seq` を含む）。`audit-admin export --identity-chain` は最初のintentでwatermarkを固定し、以降 `{seq_after, seq_through: W}` で残りを読み、manifestに全intentを列挙する。
- recovery mode（fingerprint不一致または `recovery_pending`）で通るのは `probe`、`store_status`、`posture_check`、`lookup_receipts`、`list_source_receipts`、`lookup_control_receipts`、`lookup_lost_ranges`、`verify_recovery`、`identity_chain_recovery_page`、`report_regression`、`declare_recovery_pending`、`begin_recovery_epoch` だけで、他はすべて `store_recovery_required`（KA001）になる（`tests/store_recovery.rs` で全関数を確認）。recovery用の関数はrecovery外では `not_in_recovery` で拒否する。
- `begin_recovery_epoch(checkpoint?, relay_max_seq?, expected)` は復元chainを再検証し、classification（`regression` / 計画的移動 `planned_move` / `restore`）、checkpointの分類（match / ahead / mismatch / epoch_mismatch / store_behind。chainが一致してepochだけ異なる場合が `epoch_mismatch`）、identity範囲digest、消失範囲 `(restored_head, max(checkpoint seq, relay最大seq, 報告seq, restored_head)]`、regressionの証拠、旧/新fingerprintを記録する。旧 `rebind_fingerprint` は廃止し、計画的移動も同じepochとして扱う。checkpoint・relay最大seq・regression報告のいずれも無い場合（例：新しいDBへのrestoreで記録が無い）、上限は不明で `lost_upper_known: false`（`lost_upper_seq` は復元head）を記録する（NULLにはしない）。
- `expected` は帯域外のrecovery記録（旧epoch、復元headのseqとchain、消失範囲の上限。上限が不明ならNULL）である。Storeが計算した実際の値と4つとも（上限の既知・不明を含めて）一致した場合だけepochを開始し、食い違えば `refused`/`expectation_mismatch` で何も変えない。期待値なしの呼出しはpreview（`refused`/`expectation_required`）で、実際の値だけを返す（`AuditAdmin::preview_recovery_epoch`、`audit-admin begin-recovery-epoch --preview`）。recovery中の拒否は記録しない。
- `audit-admin begin-recovery-epoch` は、`--preview` でも開始の成功後でも、2行目に帯域外のrecovery記録の1行（`kp-audit-recovery-records-v1`、下記）をそのまま出力する。previewの行は開始前に追記する記録、成功後の行はStoreが確認した記録で、両者は同じ値になる（同じ行を2回追記しても判定は変わらない）。
- epoch後は `access_reapply_pending` になり、本文を開示する操作（investigate、export、本文を返すverify）は `open_access` でも開いている tokenの `read_page` でも `access_reapply_pending`（55000）で拒否する。開くのは本文を含まないidentity chainと、DB内の `verify` だけである。administratorの `record_access_reapplied` と、retentionの再適用の両方で解除される。retentionの再適用は、有効なpolicyそれぞれについて、epoch後に現行revisionで、policyのcutoff（`tx_time - retain_days`。要求cutoffで狭めない）まで期限切れの本文を残さず失効させた `expire`（`count < limit`）が記録されたときに満たされる（有効なholdによる `held` の拒否も満たす。restoreで戻った失効済み本文が残っている間は開示しない）。有効なpolicyが無いことは `confirm_retention_reapplied` で記録する。
- `resolve_intent` はtokenの操作のcapabilityに加え、control eventを可視にしたintent（`include_control`）では `administer` も読取りごとに確認する（取消しは開いているtokenにも効く。設計§10.3）。
- `expire` のcutoffは、記録の `utc_timestamp`（年0001–9999）で表せる範囲だけを受け付ける（範囲外は `invalid_input`）。
- `registered_types` の変更は後続migrationで `SET LOCAL audit_store.write_context = 'migration'` を設定して行う。
- `legal_holds` はv1の予約で、追加する関数は無い。有効なholdが1件でもあれば `expire` は `held` になる。

## restore（設計§11）

1. globals（`roles.sql`、LOGIN role）を先に用意し、`pg_restore --exit-on-error --single-transaction` で新しいDBへ復元する（`--no-owner` 等は使わない）。
2. `sql/privileges.sql` を再適用する。違反がある間、`begin_recovery_epoch` は `store_posture_invalid` で拒否する。
3. `audit-admin verify --recovery`、`audit-admin export --identity-chain --recovery --dir D --checkpoint <最新の帯域外checkpoint>` でDB外照合する。
4. `audit-admin begin-recovery-epoch --checkpoint <file> --relay-max-seq <N> --preview` で復元head・消失範囲を確認し（`N` は `audit-relay health` の `stored.relay_max_seq`：復元したepochでrelayが参照する最大のStore seq。ackだけでなくreplay・repair・source mismatchの記録を含む）、出力の2行目（`kp-audit-recovery-records-v1` の1行）を帯域外のrecovery記録fileへ追記してから、`--expect-old-epoch` `--expect-head-seq` `--expect-head-chain` `--expect-lost-upper` に記録の値（`lost_upper` が `null` なら `unknown`）を渡して `begin-recovery-epoch` を実行する（食い違えば `expectation_mismatch` で拒否される）。成功時の2行目は追記した行と同じである。checkpointもrelay最大seqも無い開始は上限不明（`lost_upper: null`）のepochになり、`assess` は真正としない。
5. `audit-admin record-access-reapplied`（administrator）と、retentionの再実行（期限切れの本文が残らなくなるまで `audit-admin expire` を繰り返す）または `audit-admin confirm-retention-reapplied`（maintainer）。
6. relayの再開・再配送の後、新しいcheckpointを取り、そのcheckpointまでのexportを `audit-admin assess --recovery-records <file>` で判定する（消失を伴うrecoveryの後は `lost`、記録の無いepochは `unverified_recovery`）。

同じDBでのregression・既知の事故は、`report_regression`（relay）または `audit-admin declare-recovery-pending --incident-code CODE`（maintainer）から手順3以降を行う。

## DB外の総合判定（`audit-admin assess`）

```text
audit-admin assess --dir D --checkpoint FILE [--anchor FILE] [--recovery-records FILE]
```

- DBに接続しない（`AUDIT_STORE_DATABASE_URL` は不要）。`audit-admin export` が書いたdirectory（`export.jsonl` と `manifest.json`）をaudit-coreで改めて検証し、帯域外のcheckpoint（`kp-audit-checkpoint-v1`）と帯域外のrecovery記録で `audit_core::assess_recovery` の総合判定を出す。
- manifestから使うのは作り方（operation、最初のintentの `seq_after`、watermark、anchor付きかfilter付きか）だけで、結果の主張は信用しない。anchorはgenesis、またはexportの `seq_after` にある帯域外checkpoint（`--anchor`、または同じseqの `--checkpoint`）で、manifestのcheckpointは使わない。genesisより後から始まるexportにそのcheckpointが無ければ使い方の誤り（終了code 2）。manifestの `rows`・`head` が検証結果と食い違えば `broken`。
- 出力は1行のJSONで、判定code・seq・epoch・chain値・件数だけを持つ（本文・主体・resourceは出さない）：`verdict`、`authenticated_through`、`chain_integrity`、`error`（`broken` の最初の行。位置だけ）、`operation`、`anchor_seq`、`head`、`rows`、`complete`、`findings`（checkpointの比較結果ごとの件数）、`findings_neutral`（anchor以前・anchor位置のcheckpoint）、`epochs_total`、`epochs_unrecorded`、`epochs`（最大64件：遷移のseq、旧/新epoch、記録の有無、復元headの確認、復元head、消失範囲の上限とその既知・不明 `lost_upper_known`、分類）、`unmatched_records`、`records_before_anchor`、`records_after_head`、`unconfirmed_expiries`、`unverified_expiry_evidence`、`expired_after_watermark`。
- 終了code：

| code | verdict |
|---|---|
| 0 | `authentic` |
| 4（要確認） | `authentic_through`、`unverified_expiry`、`no_checkpoint`、`lost`、`unverified_recovery` |
| 5（拒否） | `tampered`、`unanchored`、`broken`（chainの検証失敗、manifestの不正・不一致） |
| 2（使い方） | `store_behind`：checkpointがexportの最後のseqより先（headと同じかより後のepoch）。exportが古いか `--seq-through` で切ったものか、Storeが行を失ったかをexportだけでは区別できない。checkpointはexportの最後のseq以前のものを使う（checkpointまで改めてexportしてもheadが届かなければStoreが行を失っている） |
| 1 | fileを読めない、checkpoint fileやrecovery記録fileの形式が不正 |

帯域外のrecovery記録file（`kp-audit-recovery-records-v1`）はJSON lines（1行1遷移、epoch順に追記、空行は無視）で、各行は閉じたkey集合（全key必須）の `{"format":"kp-audit-recovery-records-v1","old_epoch":N,"new_epoch":N+1,"restored_head_seq":S,"restored_head_chain":"<64桁の小文字hex>","lost_upper":U}`（`0 ≤ S ≤ U`、消失範囲は `(S, U]`、`U = S` は消失の無い計画的な移動）である。Storeが上限を知らないrecovery（`lost_upper_known: false`）では `"lost_upper":null` で、復元head以後のすべてが消失した可能性を表し、そのepochは `lost` になる（`authentic` にならない）。重複keyや不正な行が1つでもあれば読込みを失敗させる（終了code 1）。`begin-recovery-epoch` が2行目に出す行をそのまま追記する。DB・backupとは別の場所に、checkpointと一緒に保管する。

## 試験

PostgreSQL 18.6（testcontainers `postgres:18.6-bookworm`）で、合成データとroles.sqlで作ったLOGIN roleだけを使う。`tests/cli_assess.rs` は `audit-admin export` の出力をDB URLなしの `audit-admin assess` で判定し、判定class（authentic、export行の1行編集による `broken`、digest・chainを再計算した書換えの `tampered`、`authentic_through`、`no_checkpoint`、`unanchored`、`unverified_expiry`、headより先のcheckpointの `store_behind`、記録あり/なし/食い違いのrecovery epoch：`authentic`・`lost`・`unverified_recovery`、checkpointもrelay seqも無いrestoreの上限不明な記録：`lost`）と終了codeを確かめる。`tests/store_ingest.rs` は止まった拒否の連続が追記・probe・証拠記録の前に全件chainへ残ること、`tests/store_integrity.rs` はprobeが被覆を再計算しないこと（関数統計）を確かめる。`AUDIT_STORE_TEST_DATABASE_URL` に使い捨てのsuperuser serverを指定すると、試験ごとにdatabaseを作る（restore試験はcontainer内の `pg_dump` を使うため、この場合はskipする）。

`tests/data/store-envelope-golden.json` はaudit-coreの全受理fixture（event idはfixture名から導出）をStoreへ保存したときの `kp-audit-jsonb-sha256-v1` envelope digestをadapter_versionごとに固定する。entryは `<fixture>@<投影する入力行（導出したevent idを含む）のhashの先頭16桁>` で、判定はaudit-coreのprojection pinと共通（`crates/audit-core/tests/common/mod.rs` の `check_golden`）である。fixtureの入力を変えると新しいentryになり、再投影して追記するまで試験が失敗する。古いentryは履歴として残す。append-onlyで、既存のentry・sectionは編集しない（旧sectionは `FROZEN_SECTION_DIGESTS` で凍結する）。digestが変わる変更には `LEGACY_ADAPTER_VERSION` の更新、`registered_types` の追加、新しいsectionが要る。更新を忘れたまま同じeventを再配送すると、Storeは上書きせず `conflict` にする（`tests/store_golden.rs`）。
