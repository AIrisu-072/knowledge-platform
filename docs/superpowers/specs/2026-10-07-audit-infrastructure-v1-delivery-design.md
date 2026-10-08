# Audit Infrastructure v1：監査Outboxから監査Storeまでの配送・保存・検証 設計

Status: DESIGN REVISION 4（改訂3：独立review 1・再review・最終reviewの指摘を反映。改訂4：単位Bの実装で意図的に変えた点を反映、下記）。2026-10-07、基点main `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（push CI 37562024089 SUCCESS）。

改訂4（2026-10-08、単位B：`crates/audit-store-postgres`・`crates/audit-relay`）。該当箇所に「（改訂4）」と記す。承認状態：依頼者の実装・修正指示（hard requirements）の範囲内で本trackが採用し、単位Bの独立review・確認reviewで確認した（依頼者による個別承認ではない。確認review round2のMinor m1–m5の修正後の再reviewは未実施）。運用手順は [audit-delivery-store.md](../../operations/audit-delivery-store.md)。
1. §11：`begin_recovery_epoch` は帯域外の期待値（旧epoch、復元headのseq・chain、消失範囲の上限。不明はNULL）を引数に取り、Storeの実際の値と一致した場合だけepochを開始する。期待値なしの呼出しはpreview。
2. §10.1：role行列の正本を両crateの `sql/roles.sql` とし、古い行（reconciler、admin、maintainer、owner、relay worker・operator、command表）を直した。
3. §12：healthに `run` が報告するcircuit、追加の警報、`run` の進捗行を加えた。
4. §8・§10.4：export中のexpireとの競合は、snapshotではなく `complete: false`・`expired_after_watermark` で扱う。
5. §9：retention selectorのevent typeを登録済みrelay typeに限った。
6. §6.3：relay側の保留に行ごとの指数backoffを加えた。
7. §10.3：ingest経路の拒否の集約を、間引かない正確な件数（`suppressed_since_last`）で行う。
8. §8：帯域外のrecovery記録の形式 `kp-audit-recovery-records-v1` と、DB外の総合判定 `audit-admin assess` を定めた。
9. §12：検証の被覆は `record_verified` が保つ状態行から読む。
10. §7.3・§11：postureの追加の違反と、backupを行うlogin。

2026-10-08（単位C）：§2を初期状態（§2.1）と最終状態（§2.2）に分けた。設計内容の変更は無い。

本書は未統合Draft [PR44](https://github.com/AIrisu-072/knowledge-platform/pull/44)（設計）と [PR45](https://github.com/AIrisu-072/knowledge-platform/pull/45)（schema/legacy contract）を置き換える。旧設計の脅威分析・失敗モデルは入力として尊重する。ただし旧基点 `d71753d` は30 merge古く、旧方針「reasonを持つevent・通常ACL変更を配送しない」は今回の要求（取下げ・metadata/ACL変更をStoreまで届ける）と衝突する。旧PRのstackには依存しない。決定事項は [決定記録](../../decisions/2026-10-07-audit-envelope-store-integrity.md) に記す。

規範入力：
- `spec/operations/observability-audit-requirements-v0.md`（以下OA）§§2.4, 7.2, 12.4, 13–17, 19–22, 26–33
- `spec/data/transaction-consistency-requirements-v0.md`（TC）INV-09/10, T1–T10, §7（:797）
- `spec/architecture/architecture-contract-v0.md`（AC）§§4.4, 7, 13
- `spec/data/logical-data-model-v0.md`
- `spec/selection/library-tool-selection-v0.md`（LTS）

## 1. 不変条件

1. 必須AuditのstagingはDocumentの業務mutationと同一transactionで確定する。stagingまたは同時に行う配送登録が失敗した場合、業務はcommitしない（OA §22.2, TC INV-09）。
2. Storeが停止していても業務は継続し、staged eventは配送待ちとして残る。staging失敗（業務rollback）とStore障害（配送遅延）を、状態・health・試験で区別する。Store障害は試行回数を消費しない（§6.3）。
3. Auditはsamplingしない。配送はat-least-once、ingestはevent IDとsource commitmentでidempotentに行う。
4. Auditを、通常log・trace・metric・Domain Business Event・経営分析の業務正本・Personal Memoryの代わりにしない。Storeは調査証跡であり、業務集計の正本ではない。
5. 文書本文、検索query全文、credential/token、physical storage locator、ACL全文、顧客データ、Chat transcript、Personal Memory、内部思考、未確定Draft本文を、Storeへ無条件に保存しない。payloadはevent種別ごとのallowlistであり、自由記述fieldを持たない（OA:646, §19）。
6. 既存event typeとその意味を変えない。renameせず、additiveに進化させる。legacy rowに無い情報（理由文、actor種別、W3C trace、cancel側のoperation ID等）を、復元できたかのように表現しない。
7. produced（staged）／delivered（ack済み）／stored（Store受理）／verified（integrity検査済み）を、別々の証拠として扱う。
8. 本番credential・実データ・本番migration・deployは扱わない。

## 2. Capability matrix

凡例：実装・検証済み／実装不完全／仕様のみ／欠落／他担当待ち。§2.1は着手時の初期状態（更新しない）、§2.2は2026-10-08の最終状態である。

### 2.1 初期状態（main `d515aa3` 時点、2026-10-07）

「本設計」列は、本trackで埋める範囲を示す。

| Capability | main現状 | 根拠 | 本設計 |
|---|---|---|---|
| 生成（Document 21種。main `6a34de3` のVIEW/RESET追補で23種。現在の一覧の正本は `spec/telemetry/audit-event-catalog.json`） | 実装・検証済み。ただし一部の試験が欠落 | `document-repository-postgres/src` の13箇所がINSERTする。`authorization.denied` の試験は0件。`version.created/updated/rebased`、schedule系、`withdrawn`、`document.moved`、`folder.renamed`、`revision_comparison` にはaudit失敗の試験が無い | Producerは変更しない。E2E受入で代表operationを実証する |
| atomic staging | 実装・検証済み。ただし未試験の種別あり | 業務tx内でINSERTし、失敗すればrollbackする。failure trigger試験は8系統 | 配送登録をstagingと同一txで行い（§5）、登録失敗時に業務がrollbackすることを試験する |
| schema | 実装不完全 | SQL列は固定されているが、`data` のshapeもsizeも検証されない。`spec/telemetry/` は存在しない | catalogからschemaを生成し、runtimeで検証する（§4） |
| CloudEvents | 仕様のみ | OA §13。SDKはPOC REQUIRED | 自前のstructured JSON envelope（決定記録、§4.1） |
| actor/resource attribution | 実装・検証済み（Document内） | issuer/principal、resource_typeの3値、scheduler serviceExecutor（`service/scheduler`） | そのまま保持する。信頼境界はproducer roleとする（§4.6） |
| correlation | 実装不完全 | `trace_id` 列はNULLか、別のaudit ID／相関UUIDである。W3C traceparentは保存されない（TC INV-10・AC §13の `trace_id` は未充足） | `operation_id` / `publish_operation_id` / `source_correlation_id` に明示的に写像する。`trace_id` は予約のみ。Documentへのhandoffとする（§4.2, §13） |
| 配送（dispatcher） | 欠落 | 読み手が存在しない | Audit専用の配送状態と既存runnerで実装する（§6） |
| retry/backoff | 欠落（列のみ存在） | `attempt_count` は常に0 | lease、指数backoff、外部障害時の試行返却（§6） |
| idempotency | 欠落 | source側のPKのみ | Storeの永続identityを `(event_id, source commitment)` で照合する（§7.2） |
| terminal failure | 欠落 | — | quarantine、監査付きreplay、履歴保持（§6.4） |
| Store | 仕様のみ（DEFERRED） | LTS:268 | 既存のPostgreSQL/SQLxで実装する。別DB・別schema・別ledgerとし、本番前に再選定する（決定記録） |
| append-only | 欠落 | stagingにtrigger・grant・TRUNCATE防止が無い | staging guard、配送台帳guard、Store guard、role行列（§5, §7, §10） |
| integrity | 欠落 | — | event digest＋write-time hash chain＋外部checkpoint（決定記録、§8） |
| retention | 仕様のみ | 年数は固定しない（OA §26） | 版付きの方針データとし、既定では失効しない。期限到達の処理は特権maintenance（§9） |
| access/export | 仕様のみ | OA §27 | Audit専用の権限、DB session主体の束縛、2段階開示（§10） |
| 閲覧の監査（audit-of-audit） | 仕様のみ | OA:842 | 開示前に意図（intent）をcommitする（§10.3） |
| backup/restore | 仕様のみ | Linux guideは単一DBの `pg_dump` のみ | 2DBのbackup契約、復元検知gate、recovery epoch、reconcile（§11） |
| minimization | 実装不完全 | 7種が自由記述の `reason` を保持し、withdraw/endには上限が無い | 理由文は複製しない。提供有無とbyte数のみ記録する（§4.4） |
| health/reconciliation/restart | 欠落 | — | produced/delivered/stored/verifiedを分けたhealthとreconcile（§12） |
| Search audit配送 | 他担当待ち | `search_audit_outbox_events` はSearch所有で、R04Aが未実装 | source adapterの契約をhandoffする（§13） |
| Organization attribution | 他担当待ち | `work.event_staging` にissuer・role・delegationが無い | versioned extensionの接続点とhandoff（§13） |

### 2.2 2026-10-08 最終状態（main `dba8168`＋単位C）

- 対象：単位A（PR #98、main `643cc85`）、単位B（PR #113、main `dba8168`）、単位C（`crates/audit-acceptance`、本表と同じ統合単位でmain未統合）。
- 「実装・検証済み」は、PostgreSQL 18.6（testcontainers）と合成データの試験で確かめたことを指す。単位A・Bはexact-headのhosted CIで成功した。単位Cはlocalで6試験PASS（約20秒）で、hosted CIは統合時に確かめる。**本番導入（deploy、本番migration、credential、role分離）はどの行も未実施**で、Document PoCのruntimeはrelayを起動しない。
- 根拠のpathは `crates/` からの相対。T1〜T5は `audit-acceptance/tests/acceptance/` の `journey.rs`（T1）・`staging_failure.rs`（T2）・`store_outage.rs`（T3）・`relay_crash.rs`（T4）・`store_restore.rs`（T5）で、Documentの実producer（productionのapplication service・repository・file storage）から書いた行を使う。handoffは [引継ぎ文書](../handoffs/audit-infrastructure-v1-organization-handoff.md)。

| Capability | 分類 | 根拠 | 残る欠け |
|---|---|---|---|
| CloudEvents envelope＋版付きpayload | 実装・検証済み | `audit-core/src/envelope.rs`、`audit-core/tests/envelope_contract.rs`、`golden_projection.rs`、T1（23種40件をStoreで受理） | SDKは使わない（決定D1）。`extensions` 名前空間は未実装（v1は空objectのみ） |
| schema検証 | 実装・検証済み | catalogと生成schema（`spec/telemetry/`）、`audit-core/tests/schema_contract.rs`・`catalog_contract.rs`・`legacy_projection.rs`、Storeの構造検査（`audit-store-postgres/tests/store_ingest.rs` `structural_rejections_are_verdict_rows_and_version_skew_is_an_outage`） | 標準JSON Schemaで表せない制約はRustだけが拒否する（`rust_only:*` に分類済み） |
| bounded metadata | 実装・検証済み | jsonb text 32 KiB（`envelope_contract.rs` `size_limit_is_measured_on_the_jsonb_rendering`・`maximal_envelopes_of_every_entry_fit_the_jsonb_limit`）、claim projectionの列1024 B・`data` 16 KiB（`audit-relay/tests/source_schema.rs` `claim_projection_is_total_and_never_carries_the_reason`）、principal各部256 B、list上限 | Documentはprincipalの長さ・文字種の上限を持たない（Documentへのhandoff） |
| AuditStore port＋backend | 実装・検証済み | `audit-core/src/port.rs`・`tests/store_port.rs`、`audit-store-postgres`（`tests/store_ingest.rs` ほか） | backendはPostgreSQLの暫定採用（決定D2、本番前に再選定：依頼者） |
| 配送：claim/lease/delivery/ack/retry/backoff | 実装・検証済み | `audit-relay/tests/source_schema.rs` `claims_are_leased_and_stale_tokens_are_fenced`、`delivery.rs` `store_down_holds_without_consuming_attempts_then_drains`・`relay_side_holds_back_off_and_never_starve_deliverable_rows`・`outage_streak_counts_residual_errors_only_after_progress`、T1・T3 | relayの複数instance同時運転、本番のtimeout値・処理量は未検証（運用手順§12） |
| 配送：duplicate | 実装・検証済み | `delivery.rs` `concurrent_duplicates_store_once_and_stale_acks_are_lost`・`reprojection_acks_the_original_receipt_and_a_forgotten_bump_conflicts`、T4 | — |
| 配送：restart・dispatcher crash | 実装・検証済み | `audit-relay/tests/recovery.rs` `kill9_after_the_store_commit_redelivers_as_a_duplicate`、T4（子processのrelayをSIGKILLし、`relay::run` で再起動） | process監視（systemd等）と再起動方針は未検証。T4は `audit-relay` binaryではなく同じlibrary入口 |
| 配送：commit結果不明 | 実装・検証済み | `delivery.rs` `commit_unknown_retries_converge_to_a_duplicate`、`store_ingest.rs` `adapter_maps_every_non_verdict_failure_to_an_outage` | — |
| 配送：terminal failure（quarantine・replay） | 実装・検証済み | `delivery.rs` `invalid_rows_quarantine_and_catalog_skew_is_held`、`recovery.rs` `final_attempt_crash_quarantines_and_only_the_fenced_repair_acks`・`replay_is_audited_and_direct_sql_replay_is_detected` | 実producer行でのquarantine・replayは単位Cで繰り返していない（単位Bの合成行） |
| reconciliation・repair | 実装・検証済み | `recovery.rs` `store_only_and_unregistered_rows_are_reported_and_repaired`・`repair_never_resets_rows_missing_in_their_own_epoch`、T4（read-only reconcileが全行ok）、T5（`reconcile --repair`） | 記録の `repaired_*` は計画件数（実適用件数はCLI出力だけ。依頼者判断待ち） |
| event IDの一意性とidempotentなingest | 実装・検証済み | `store_ingest.rs` `idempotency_outcomes_and_conflicts`、`audit-store-postgres/tests/store_golden.rs` `a_forgotten_bump_is_a_conflict_not_an_overwrite`、T1（event_idごとに1件） | commitmentを持たないadapterの規則（§7.2）は実装済みだが、該当adapterがまだ無い |
| 安定した順序 | 実装・検証済み（Storeへのcommit順） | `store_ingest.rs` `head_lock_serializes_publication_in_commit_order`、T1（受領seqがrelay台帳と一致） | seqは業務の因果順ではない（`occurred_at` を保持）。relayのclaim順は配送順を保証しない |
| 業務repositoryからのupdate/delete不可 | 実装・検証済み | staging guard（`source_schema.rs` `staging_and_ledger_guards_refuse_mutation`）、Storeのappend-only（`store_ingest.rs` `append_only_for_every_role_including_the_owner_path`）、Document production codeにstagingのUPDATE/DELETEは無い | guardは事故防止で、superuserと非superuserのstaging表ownerはtriggerを無効化・削除できる（postureはownerを報告しない。事後に未登録行・`source_digest` で検出。handoff §5.3）。本番role分離は未実施 |
| retention・特権maintenance（expire・purge） | 実装・検証済み | `audit-store-postgres/tests/store_integrity.rs` `retention_follows_policy_revisions_cutoffs_holds_and_keeps_tombstones`、`store_recovery.rs` `reapply_needs_a_drained_retention_run_and_verify_disclosure_stays_closed`、T5（epoch後の再適用） | 年数は固定しない（policyは運用が設定）。source stagingのcleanupはv1に無い |
| legal hold | 実装不完全（拡張境界のみ） | `audit_store.legal_holds` の予約表。有効なholdが1件でもあれば `expire` は `held`（試験は表へ直接挿入） | holdの作成・解除の関数・CLIは未実装。`purge_body` はholdを見ず、purgeとholdの優先は依頼者判断待ち |
| integrity metadata | 実装・検証済み | envelope digest・salt付きsource commitment・hash chain・外部checkpoint（`audit-core/tests/chain_export.rs`、`store_integrity.rs`、`audit-store-postgres/tests/cli_assess.rs`）、T1（`assess` が `authentic`）、T5（`lost`・`unverified_recovery`） | D3の限界（DB ownerは全体を再計算できる。署名・WORM・外部anchorは将来）。帯域外記録の保管先と管理者分離は未検証 |
| 監査の閲覧・調査・export・設定・verifyの認可とaudit-of-audit | 実装・検証済み | `audit-store-postgres/tests/store_access.rs`（`role_matrix_refuses_every_function_outside_the_role`、`unbound_and_unauthorized_principals_are_denied_and_recorded`、`read_page_needs_a_committed_intent_and_a_clean_transaction`、`control_events_are_visible_only_with_administer_and_close_is_recorded`、`self_grant_is_refused_and_bind_unbind_are_owner_only`）、`control_catalog.rs` | 認証境界はDB login（本番identityは未確立）。`pg_dump`・owner/superuserの直接読取・recovery中の読取・export fileは対象外（§10.3）。二人承認なし。調査はfilter付きの読取で全文検索は無い |
| Document producerのE2E接続 | 実装・検証済み（単位C） | T1（23種すべて）：作成・WORKING更新・rebase・公開・予約と取消・取下げ・公開終了・metadata、ACL変更（通常・bootstrap）、folder・文書の操作、初回既読・VIEW/RESET、原本アクセス、Diff・revision比較、拒否（management・既読）、`service/scheduler` による予約公開と予約の終端 | HTTP層・identity adapter、DSI/Diff worker binary（合成実装で代替）、`DueScheduler` 本体（同じ順の呼出しで代替。PoCの `StaticRequesterResolver` は `poc` 主体だけ）は通していない。`authorization.denied` は業務transactionの外で書かれる（Documentへのhandoff） |
| 失敗：staging（Outbox INSERT）・配送登録の失敗 | 実装・検証済み | T2、`source_schema.rs` `registration_failure_rolls_back_the_business_write` | staging失敗をDocument側で監視する手段は無い（運用手順§12） |
| 失敗：Store停止 | 実装・検証済み | T3（業務継続、試行返却、`store_unavailable`・`circuit_open`・`outage_held`、復旧後1回ずつ）、`delivery.rs` `read_only_lock_and_statement_timeouts_and_version_skew_are_outages` | T3は同じcluster内のdatabaseの接続拒否で、別hostの停止・network分断は未試験。障害直後はhealthが次のprobeまで `circuit` open・gate okを示し得る |
| 失敗：保存後・ack前のcrash、duplicate | 実装・検証済み | T4、`recovery.rs` `kill9_after_the_store_commit_redelivers_as_a_duplicate` | — |
| 失敗：不正schema | 実装・検証済み | `delivery.rs` `invalid_rows_quarantine_and_catalog_skew_is_held`、`envelope_contract.rs`、`legacy_projection.rs` | — |
| 失敗：改ざん | 実装・検証済み | `store_integrity.rs`（`modified_body_is_detected`、`full_rewrite_passes_in_database_but_fails_offline` ほか）、`chain_export.rs`、`delivery.rs` `source_mismatch_records_the_control_event_first_and_once`、`store_access.rs` `tampering_with_a_stored_intent_is_detected` | D3の限界（上記） |
| 失敗：無権限の閲覧・export | 実装・検証済み | `store_access.rs`（上記）、`audit-store-postgres/tests/cli.rs` `operator_commands_write_private_files_and_refuse_privileged_sessions` | 拒否の記録は呼出側のROLLBACKで消える（§10.3、拒否は何も開示しない） |
| 失敗：retention | 実装・検証済み | `store_integrity.rs` `retention_follows_policy_revisions_cutoffs_holds_and_keeps_tombstones` | — |
| 失敗：restart・recovery（backup/restore） | 実装・検証済み | `store_recovery.rs`、`recovery.rs` `store_restore_into_a_new_database_gates_until_a_new_epoch`・`an_in_place_restore_is_detected_as_store_regressed`、T5 | Document DBとStore DBを組で戻すrestoreは未検証。Document DB restore・in-place restoreは合成行だけ。fingerprintは同じtimelineの物理restoreを検知しない |
| 失敗：機微payloadの拒否 | 実装・検証済み | `delivery.rs` `sensitive_data_keys_are_held_and_never_ingested`、`legacy_projection.rs` `free_text_and_unknown_members_are_quarantined`・`reason_text_never_passes_through`、T1（理由文・本文・storage locator・ACLにだけ現れる主体がStoreの全表（`pg_dump`）とexportに無い） | withdraw/endの理由文に上限が無い（Documentへのhandoff。Storeへは複製しない） |
| produced／delivered／stored／verifiedの区別 | 実装・検証済み | `audit-relay/src/health.rs`、`audit-relay/tests/runtime.rs`、T1（4値が別の値）、T3（produced＝registeredでstaging失敗と区別） | — |
| 検証記録の無限再帰の防止 | 実装・検証済み | `store_integrity.rs` `verify_records_once_and_never_recurses`（記録はWより後のseq） | — |
| correlation（W3C trace） | 実装不完全 | `operation_id`・`publish_operation_id`・`source_correlation_id` へ写像。`trace_id` は予約で全adapterが `false` | W3C traceparent列はDocumentに無い（TC INV-10・AC §13は未充足。Documentへのhandoff） |
| catalogとproducerの整合 | 実装・検証済み | PR #106の2種を登録、`authorization.denied` の `action_code` をproducerのsourceと照合（`audit-core/tests/catalog_contract.rs` `authorization_denied_action_codes_cover_every_producer_code`）、Document migration互換（`audit-acceptance/tests/acceptance/document_migration.rs`、`source_schema.rs` `digest_function_tracks_digested_columns`） | 新しいtype名はCIでは検出せず、relayが `relay_catalog_skew` で保留しhealthが警報する（配備順は運用手順§7） |
| Organizationへのhandoff | 他担当待ち | 引継ぎ文書§2–§3（帰属、versioned extension hook、問いO1–O12） | Organizationの判断（O1–O12）。`extensions` 名前空間・新resource種別はその後のAudit変更 |
| Searchへのhandoff | 他担当待ち | 引継ぎ文書§4。`search_audit_outbox_events` は未配送 | Searchのsource接続、R04A-Dと決定D4の調整（S1） |
| 本番導入 | 欠落（本trackの範囲外） | — | deploy・本番migration・credential・process監視・本番role分離（runtime loginをstaging表のownerにしない）・Store基盤の再選定は未実施（依頼者・Document） |

## 3. 構成とcrate

| 構成要素 | 置き場所 | 責務 |
|---|---|---|
| `crates/audit-core` | pure（sqlx/tokio/fs/document-*/work-*/search-*に非依存） | catalog、envelope、検証、legacy投影、chain計算、port trait、export検証 |
| `crates/audit-store-postgres` | Store DB（`audit_store` schema、ledger `audit_store_sqlx_migrations`） | `AuditStore` portのPostgreSQL実装、SQL関数（ingest・開示・検証・retention・権限・status）、bin `audit-admin` |
| `crates/audit-relay` | Document DB（`audit_relay` schema、ledger `audit_relay_sqlx_migrations`） | 配送登録trigger、staging/台帳guard、配送SQL関数、`outbox-delivery` runnerへのadapter、reconcile、replay、health、bin `audit-relay` |

- 3 crateに分ける理由：Storeは配送方式に依存せず（`outbox-delivery` に依存しない）、2つのcrateはそれぞれ別のDB・ledger・roleを扱う。将来Search/Organizationのsource adapterがStore crateだけを使える。AC:476の過剰細分化には当たらない。
- `spec/architecture/dependency-rules.toml` に境界を追加する。
  - `audit_core`：sqlx、tokio、axum、document-*、work-*、search-*、`std::fs`、`std::path`、`tokio::fs` を禁止する。
  - store/relay：document-*（dev-dependencyを除く）、search-*、work-*、axum を禁止する。
- `include_str!` で読み込むのは `spec/` 配下か各crate配下だけにする（container-buildでcopyされる範囲）。migration directoryには `build.rs`（`rerun-if-changed`）を置く。
- Store DBは `AUDIT_STORE_DATABASE_URL` で別DB・別serverを指せる。relayは起動時に、sourceとStoreが同一database（system_identifierとdatabase名がともに一致）であれば拒否する。試験ではsourceとStoreを同じcluster内の別databaseに分ける。cross-database transactionは使わない。
- Document の `_sqlx_migrations` へはAudit行を書かない（document-server/organization-serverの厳格なcompatibility checkを維持する）。
- Document producer、`outbox_events`、`crates/outbox-delivery`、Search crate/migration、Work schema、GUI/Tauriは変更しない。

## 4. Event contract

### 4.1 CloudEvents envelope（決定記録 D1）

`cloudevents-sdk` はPOC REQUIRED（LTS:265）なので、productionには入れない。CloudEvents 1.0.2 structured JSON formatに従う閉じたenvelopeを `audit-core` で実装する。SDKの型をDomainへ漏らさない。LTS §6.3のPoC受入項目（round-trip、schema整合、id/source/type/subject/timeの安定、event IDによるidempotency）を、conformance試験として採用する。

| 属性 | 値 |
|---|---|
| `specversion` | `"1.0"` |
| `id` | 既存の監査event UUID（変更しない） |
| `source` | 既存値 `urn:knowledge-platform:document-platform`。Store内部のcontrolは `urn:knowledge-platform:audit-store`、relayのcontrolは `urn:knowledge-platform:audit-relay` |
| `type` | 既存の短いdotted type（例 `document.version.published`）。OA §13の逆DNS例は例示であり、OA:646「既存Document eventの型と意味を維持」を優先する |
| `subject` | 既存のsubject |
| `time` | `occurred_at` をUTC RFC3339、マイクロ秒、末尾 `Z` で表したもの |
| `datacontenttype` | `"application/json"` |
| `dataschema` | `"urn:knowledge-platform:audit:payload:v1"` |
| `data` | payload v1（§4.2） |

- 未知の属性・extension attributeは拒否する。重複keyは拒否する（PR45のparserを流用する）。
- 大きさの上限は、PostgreSQLのjsonb text（正本、`": "` と `", "` の区切りを含む）が32 KiB以下であることとする。Rustは同じ規則でjsonb相当の長さを計算し、32 KiB（`JSONB_TEXT_LIMIT`）以下を要求する。Storeでも同じ上限を検査する。

### 4.2 payload v1

```json
{
  "schema_version": 1,
  "event_class": "CONTENT_LIFECYCLE",
  "action": "document.version.withdrawn",
  "actor": {"issuer": "poc", "principal_id": "poc-human"},
  "service_executor": {"issuer": "service", "principal_id": "scheduler"},
  "resource": {"type": "Document", "id": "<uuid>", "version_id": "<uuid>"},
  "result": "success",
  "reason_code": "<closed code>",
  "reason": {"provided": true, "utf8_bytes": 42, "text_retained": "source_systems"},
  "correlation": {"operation_id": "<uuid>", "publish_operation_id": "<uuid>", "source_correlation_id": "<uuid>"},
  "details": {"<allowlisted legacy key>": "<typed value>"},
  "extensions": {},
  "provenance": {"source_format": "document-audit-outbox-v0", "adapter_version": 1, "source_commitment": "<hex>", "registration": "trigger"}
}
```

- `event_class` は、OA §14の8 class（SECURITY、PRIVILEGED_OPERATION、CONTENT_LIFECYCLE、ACCESS_POLICY、DATA_ACCESS、SEARCH_ACCESS、CONFIGURATION、SYSTEM_AUDIT）のいずれか。種別ごとの割当はcatalogが決める。
- `actor` はstaging列のissuer/principalである。`invocation_kind` はlegacy rowに保存されていないため出さない。名前から推測しない。
- `service_executor` は、legacyの `details.serviceExecutor`（published/terminal）がある場合だけ持ち上げる。requester（`actor`）と区別する（TC:484）。持ち上げた値はdetailsからは除く。
- `resource.type` は `Document` / `Folder` / `AccessPolicy` / `AuditStore`。`resource.id` はUUIDとし、nil UUIDは `authorization.denied`（AccessPolicy、subject `authorization/denied`）だけに許す。`AuditStore` の場合は固定文字列 `audit-store` とする。
- `reason_code` は、catalogが閉じたcode fieldを指定する種別（`terminalReason`、deniedの `reason_code`）だけに付ける。
- `correlation`：
  - `operation_id` は、そのeventを作ったcommandの呼出側idempotency keyである。published・scheduled・terminalでは `publishOperationId`、management系では `operation_id` / `operationId` から写像する。
  - `publish_operation_id` は、publish予約・実行を識別する `publishOperationId` である。cancelledでは `publish_operation_id` だけを設定し、`operation_id` は設定しない（cancel自身のIDはpayloadに無い。ledgerを結合して補完しない）。
  - `source_correlation_id` は、stagingの `trace_id` 列の値で、canonicalな小文字UUIDに限る（現producerはUUIDかNULLのみ）。それ以外の値は `invalid_source_correlation` としてquarantineする。W3C traceとは名乗らない。
  - `trace_id`（W3Cの32桁hex、非ゼロ）は予約fieldである。将来Documentがtraceparent列を追加した場合だけ、その専用列から設定する。legacy `trace_id` 列からの推測は禁止する。legacy adapter（`document-audit-outbox-v0`）の経路では、`trace_id` を拒否する。
- `details` は種別ごとのallowlistであり、legacy key名を変えない。型はcatalogのkindで検証する。
- `extensions` は、v1では空objectのみを許す。名前空間はcatalogに登録した後に許可する（§13）。
- `provenance.registration` は、配送登録の経路（`trigger` / `backfill` / `repair`）である。repair登録は、登録前の改変を検出できない（§5.1）。
- `provenance.source_commitment` は、salt付きのsource commitmentである（§5.2）。Document sourceでは必須とする。他のadapterでは任意とする（NO_RETENTION由来のsourceでは計算しない。commitmentが無い場合のduplicate規則は§7.2）。

### 4.3 catalog・schema・検証

- 正本は `spec/telemetry/audit-event-catalog.json`。adapterごと（source、source_format、adapter_version、commitment・registrationの要否、trace_idの可否）の定義と、`registered_types` の元になる (source, type, adapter_version) の一覧もcatalogが持つ。各typeについて、source、event_class、origin（`relay` / `store` / `relay_control`）、許可するresource種別、version要否、result、subject形の一覧、fields、required、reason扱い、reason_code_field、service_executor_field、operation_id_field、publish_operation_id_fieldを持つ。
  - subject形は、placeholderを `resource.id` / `resource.version_id` / details fieldへ束縛した形の一覧である。例：`document.version.created` は `document/{resource.id}`（初回作成）と `document/{resource.id}/version/{resource.version_id}` の2形を許す。
- kind（JSON整数は `is_i64`/`is_u64` のみ。浮動小数・指数表記は拒否。下表はDocument用の主なkindである。control用のkind（resource_ref、event_type(_list)、source_urn/list、db_role、principal_ref、int8_text、code、nullable_positive_counter）を含む正本は `spec/telemetry/README.md` §kind）：

  | kind | 内容 |
  |---|---|
  | `uuid` / `nullable_uuid` | canonicalな小文字UUID |
  | `counter` / `nullable_counter` / `positive_counter` | i64整数 |
  | `safe_counter` / `positive_safe_counter` | 0（positiveは1）以上2^53−1以下の整数（Documentの既読状態revision） |
  | `boolean` | 真偽値 |
  | `enum` / `nullable_enum` / `enum_list` | 閉じた値集合 |
  | `digest` / `nullable_digest` | 0–255の整数32個 |
  | `principal` | `{identityProvider, principalId}`、各部256 byte以下 |
  | `legacy_time` | 旧time serde配列 `[year, ordinal 1..=366, hour, minute, second, nanosecond 0..=999_999_999, offset_h, offset_m, offset_s]`。RFC3339へ変換したとは表記しない |
  | `utc_timestamp` | control用、RFC3339・µs・Z |
  | `uuid_list` | 最大100件 |
  | `hex_digest` | 64桁の小文字hex |

  自由文字列のkindは無い。
- adapter_versionの規律（§7.2）：`spec/telemetry/audit-adapter-golden.json` に、(source_format, adapter_version) ごとに代表fixtureの投影結果のdigestを固定する（version sectionは追加のみで、既存sectionは編集しない）。投影を変えて版を上げ忘れると、試験が失敗する。Store側でも、同じfixtureのjsonb digestを固定する。
- nil UUIDのclient指定ID（folderId、targetVersionId等）は、producerが受け付けても `nil_client_id` としてquarantineする（Documentへの引継ぎ事項）。
- principalの各部は、制御文字（Unicode Cc）に加え、Bidi制御、U+2028/2029、BOM、TAG、noncharacter、空白のみ・前後の空白を拒否する（標準JSON Schemaでは表せないので `rust_only:principal_charset`）。
- `reason.utf8_bytes` は、DocumentのHTTP body上限（1,048,576）以下とする。
- audit-coreの試験で、workspaceの `time` featureのもとで `OffsetDateTime` のserde出力が上記の9要素配列であることを固定する。audit系crateでは `time/serde-human-readable` を有効にしない。
- `spec/telemetry/audit-event.schema.json` はcatalogからRustで生成する（`$defs`/`$ref` で共有部分を1回だけ定義する）。差分があれば試験が失敗する（生成の再現性。OA §29）。
  - 関係：「Rustが受理するものはschemaも受理する」。
  - byte上限、subjectとresourceの束縛、32 KiB等、標準JSON Schemaで表せない制約によるRust側だけの拒否は、fixtureに `rust_only:<category>` の分類を付けて区別する。
  - jsonschemaはdev-dependencyとしてだけ使う。

### 4.4 自由記述reasonの扱い（PR44/45からの変更）

対象は `document.version.withdrawn`、`document.publication.ended`、`document.metadata.changed`、`document.moved`、`folder.created`、`folder.renamed`、`folder.moved`。

- 理由文はStoreへ複製しない。claim関数（SQL、§5.4）が `data - 'reason'` を取り、`reason: {provided: true, utf8_bytes, text_retained: "source_systems"}` だけを作る。Storeの読者は、理由文を取得できない。正確なbyte数による長さの区別を除けば、推測した理由文を確認することもできない（source commitmentはsalt付き。§5.2）。
- 理由文の所在（v1で開示する手段は提供しない。読めるのはDocument DBのroleを持つ者だけで、audit-of-auditの対象外）：

  | event | 理由文の所在 |
  |---|---|
  | withdrawn | `document_version_operations.result.reason`。fallback revisionを作った場合は `document_revisions.reason` にもある |
  | publication.ended | `document_publication_end_operations.reason` |
  | metadata.changed | revisionを持つ文書では `document_revisions.reason`。未公開の文書ではstagingとDomain `outbox_events.payload` のみ |
  | `document.moved` と `folder.*` | 業務ledgerには無い。stagingとDomain `outbox_events.payload` のみ |

  この所在が、staging append-only化（§5.3）とsource cleanupを提供しない理由の一つである。将来の開示機能は、「Storeへ閲覧記録をcommitしてから開示する」Document側の関数として別途設計する。
- `reason` が非文字列なら `reason_not_string` としてquarantineする。
- 通常の `access_policy.changed` は、sourceがreasonを記録しないので `reason` を出さない（`provided: false` とも書かない）。「無い」を「空」と誤表現しない。
- withdrawn・endedで重複する `data.actor` は、staging列のactorと一致することを検証し、不一致なら `actor_mismatch` でquarantineする。detailsからは除く。
- 上限付きの理由文保存を将来有効化する場合は、別policy・別承認とする（OA:668の扱いに準じる）。

### 4.5 Store・relayのcontrol event

catalogに `origin: store` / `relay_control` として登録する。relayのRust validatorと `ingest` はこれらを拒否する（§7.2）。

正本はcatalogで、ここは要約である。

| type | origin | class | 主なdetails |
|---|---|---|---|
| `audit.access.intent_opened` | store | DATA_ACCESS | 操作（investigate/export/verify/identity_chain）、型付きfilterの全値、filter_digest、watermark、page_size、max_pages、可視範囲、期限、token digest、session_role |
| `audit.access.closed` | store | DATA_ACCESS | intent seq、返した件数、page digest |
| `audit.access.denied` | store | SECURITY | 操作、拒否code（unbound/insufficient_capability/invalid_input/not_source_service）、session_role |
| `audit.access_policy.changed` | store | ACCESS_POLICY | 対象（db_roleまたは主体）、capability、granted/revoked/bound/unbound、bootstrap |
| `audit.retention.policy_changed` | store | CONFIGURATION | policy_id、revision、selector、retain_days（nullable） |
| `audit.retention.expired` | store | PRIVILEGED_OPERATION | policy_id、revision、selector snapshot、retain_days、入力cutoff、effective_cutoff、transaction時刻、count、first/last seq、expired_set_digest |
| `audit.retention.expire_refused` | store | PRIVILEGED_OPERATION | policy_id、revision、理由（stale/not_expirable/held） |
| `audit.body.purged` | store | PRIVILEGED_OPERATION | target_seq、target_event_id、purge_reason_code |
| `audit.integrity.verified` | store | SYSTEM_AUDIT | trigger（verify/checkpoint）、from/to seq、checked数、違反code別件数、head（epoch, seq, chain）、outcome |
| `audit.integrity.conflict_detected` | store | SECURITY | event_id、既存seq、commitment一致の有無 |
| `audit.recovery.epoch_started` | store | SYSTEM_AUDIT | 旧/新epoch、復元head（seq, chain）、照合checkpoint、分類（restore/planned_move/regression）、identity範囲digest、消失範囲（from、上限、上限既知か）、regressionの証拠、旧/新fingerprint |
| `audit.delivery.replay_requested` | relay_control | PRIVILEGED_OPERATION | event_id、旧quarantine code |
| `audit.reconciliation.completed` | relay_control | SYSTEM_AUDIT | run id、watermark、class別件数（固定field）、repair件数、照合ID集合digest |
| `audit.integrity.source_mismatch_detected` | relay_control | SECURITY | event_id、code（source_digest_mismatch / actor_mismatch） |

control eventのenvelopeはSQLが組み立てる。試験では、生成されたcontrol eventをすべてRustのcatalogで検証し、SQLの組み立てがcatalogに適合することを担保する。`provenance` は `{source_format: "audit-store-control-v1" | "audit-relay-control-v1", adapter_version: 1}`（commitmentなし）とする。

### 4.6 帰属の信頼境界

stagingへのINSERT権限を持つrole（producer、現PoCではowner）は、任意のactor/resourceを記録できる。relayが検出するのは、行内の不整合（`data.actor` と列の不一致、subjectとresourceの束縛違反等）だけである。producer側でのattestationは将来課題とする。

## 5. Source側（Document DB）：配送登録とstaging保護

### 5.1 配送登録

- `audit_relay.deliveries` を置く。列は次のとおり。
  - event_id（PK、FKで `audit_outbox_events` へ）、registered_at、registration_kind、source_digest、commitment_salt、legacy_attempt_count、legacy_delivered_at
  - 配送状態：available_at、attempt_count、attempt_limit、lease_token/owner/expires_at、last_attempt_at、last_error_code、last_outage_code、outage_streak、last_outage_generation、last_outage_at
  - receipt：delivered_at、store_seq、store_envelope_digest、store_outcome
  - quarantine：quarantined_at、quarantine_code
  - replay_count
- `audit_relay.delivery_history` は、replay/repair前の終端証拠を保存するappend-only表である（Store control eventの（epoch, seq）を持ち、（epoch, control seq）はUNIQUE。restore後はseqが再利用されるため）。`audit_relay.delivery_progress` はsingletonで、ackのたびに増えるsuccess_generationを持つ（§6.3）。`audit_relay.delivery_policy` はsingletonで、revisionを持つ。
- 登録trigger：`AFTER INSERT ON public.audit_outbox_events FOR EACH ROW` で、同一transaction内にdeliveries行を作る。
  - 関数は `SECURITY DEFINER`、`SET search_path = pg_catalog, pg_temp`、全objectをschema修飾し、本体の冒頭で `TG_RELID = 'public.audit_outbox_events'::regclass AND TG_OP = 'INSERT' AND TG_LEVEL = 'ROW'` を確認する。
  - 登録が失敗すると元のINSERTが失敗し、producerは既存どおりrollbackする。producer roleに新しい権限は要らない（trigger関数のEXECUTEはPUBLICから剥奪する。triggerの発火にはEXECUTE権限が要らない）。
- 既存行の扱い：migration transactionの最初で `SET LOCAL lock_timeout`、READ COMMITTED、`LOCK TABLE public.audit_outbox_events IN SHARE ROW EXCLUSIVE MODE` を取り、backfillしてからtriggerを作成する。lockはcommitまで同時INSERTを待たせるので、backfillとtrigger作成の間に漏れは生じない。migration試験で未登録0件を確認する。migration中は業務INSERTが待機する（運用手順に明記）。backfillの後、quarantine見込み件数をcode別に報告する（`audit-relay health`）。
- 既存の `attempt_count>0` / `delivered_at` 非NULL行は配送済みとみなさず、その値を `legacy_*` に記録してpendingにする。
- 運用中にtriggerが失われた場合は、reconcileが未登録行（anti-join）を検出する。`--repair` で `registration_kind='repair'` として登録し、件数をcontrol eventに記録する。repair登録のsource digestはrepair時点の値である。そのため登録以前の改変は検出できないことを、provenance（`registration_kind`）とhealthに表示する。

### 5.2 source digestとsource commitment

- `audit_relay.source_digest(o public.audit_outbox_events)` はSQL標準の `BEGIN ATOMIC` 本体を持つ関数とする（依存列を追跡させ、Document側の該当列のDROP/型変更をmigrate時に失敗させる。意図的な制約として決定記録に記す）。計算式は次のとおり。

  ```
  sha256(convert_to(jsonb_build_array('kp-audit-source-v1', event_id, event_type, source, subject,
         actor_identity_provider, actor_principal_id, resource_type, resource_id, resource_version_id,
         result, trace_id, data,
         to_char(occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'))::text, 'UTF8'))
  ```

  session TimeZoneに依存せず、jsonbのtext出力は決定的である。登録時にdeliveriesへ保存し、claim時にserver側で再計算・比較する（`source_intact`）。不一致なら `source_digest_mismatch` でquarantineし、`audit.integrity.source_mismatch_detected` を記録する。理由文を含む全列が対象なので、理由文をclientへ出さずに改変を検出できる。
- Storeへ送るのは `source_commitment = sha256('kp-audit-source-commitment-v1' || commitment_salt || source_digest)` である。`commitment_salt` は登録時に生成する32 byteの乱数（`uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid())`）で、deliveriesにだけ置く。
  - Storeの読者は、Storeに見えるfieldと理由文の候補からcommitmentを再計算できない。
  - 同じ行の再配送では同じcommitmentになるので、idempotencyの鍵に使える。

### 5.3 staging・台帳の保護と限界

- staging：`BEFORE UPDATE OR DELETE ... FOR EACH ROW` と `BEFORE TRUNCATE ... FOR EACH STATEMENT` で拒否する（SQLSTATE 55000）。productionコードにUPDATE/DELETEは無い。v1はsource cleanupを提供しない。
- deliveriesのFKにより、ownerがtriggerを無効化していてもDELETE/TRUNCATEはできない（`DROP TABLE` にはCASCADEが要る）。データ修正の正規手順は運用手順に記す。
- deliveries：DELETE/TRUNCATEは拒否する。event_id、registered_at、registration_kind、source_digest、commitment_salt、legacy_*は変更不可とする。receipt列とquarantine列は一度だけ設定でき、replay/repairの遷移関数（履歴を書く）経由でのみ戻せる。guardのGUC条件は事故防止であり、境界ではない。
- 限界：外部checkpointとreconcileが保護するのはingest後のeventだけである。配送前のstaging・deliveriesは、Document DBのsuperuser、または `audit_relay` を所有・書込できる主体が、`session_replication_role` 等でtrigger・FKを迂回して改変・削除し得る。guardが防ぐのは事故とDDLを伴わない不正DMLである。
  - 要件：`audit_relay` はAudit専用のNOLOGIN・非superuserのowner（`audit_relay_owner`）が所有する。Documentのowner・runtime roleには `audit_relay` への権限を与えない。
  - 現在のv1 PoCは、単一のsuperuser `KP_DATABASE_URL` でmigrateとserveを行っており、この要件を満たさない。そのため保護の対象は事故防止の範囲にとどまる。本番role分離はhandoff事項とする。

### 5.4 claim時のprojection（SQL側、definer関数内）

- projectionは全域的（total）に作る。行の内容によって例外を起こさない（CASE guardで型を確かめてから演算する。例：`data` がscalar/arrayのときの `jsonb - text`）。どの失敗も行ごとのcodeになる。lockは `FOR UPDATE OF d SKIP LOCKED`（deliveriesのみ）とする。
- `source_intact` は、text列をNULLにする場合でも、切り詰めない全行で計算する。source_digest_mismatchは、source_row_too_largeより優先する。

- stagingの各text列は、1024 byteを超えると全体をNULLにして `source_row_too_large` とする。
- `data` がobjectであることを確認し、`data - 'reason'` が16 KiB以下の場合だけ返す。
- `reason_kind`、`reason_bytes`、`source_intact`、`source_commitment` を同時に返す。
- 理由文や大きな値はclientへ出さない。relay worker roleは `public.audit_outbox_events` へのSELECT権限を持たない。

## 6. 配送（dispatcher）

### 6.1 既存 `outbox-delivery` の再利用と責務差

`DeliveryRunner`、`DeliveryPolicy`、`DeliveryConfig`、`retry_delay_within`、`ClaimAdmission` を、変更せずにlibraryとして使う。`PostgresOutboxStore`（`outbox_events` 固定）・`ErrorCode` の拡張・`DeliveryRoute` の追加は行わない。

| 観点 | 汎用P6（`outbox_events`） | Audit relay |
|---|---|---|
| source | Domain `outbox_events` | `audit_outbox_events`（将来は他sourceのadapter） |
| 配送状態の所有者 | 汎用delivery role | `audit_relay.deliveries`（Audit専用owner・worker role）。汎用roleは引き続き42501 |
| 宛先 | Search bridge | Audit Store（別DB可）、idempotent ingest |
| 完了の証拠 | `outbox_events.delivered_at` | `deliveries.delivered_at` とStore receipt（seq、envelope digest、outcome） |
| 終端 | `dead_lettered_at` と7種のcode | quarantine、Audit固有の詳細code、replay履歴 |
| 投入前の関門 | Source lease | Store可用性・書込可能性のcircuit breaker |

`ErrorCode` は閉じた7種なので、Audit固有の詳細と、外部障害かどうかの区別は、同一process内の有界な `DeliveryLedger`（keyは `(event_id, lease_token)`）でhandlerからstore実装へ渡す。processがcrashして失われた場合は、汎用codeになる（安全側）。runnerのsettle_successはApplied/KnownNoopだけで呼ばれる（runner.rs:860）。ledgerにreceiptが無ければackを拒否する（`StoreUnknown`）。

### 6.2 状態と手順

状態遷移：
- pending → leased → delivered
- leased → pending（retry待ち、`available_at`）
- pending/leased → quarantined
- quarantined → pending（replay、§6.4）
- delivered → pending（repair、delivered_missingに限る。§12）

relay workerは、`audit_relay.claim` / `renew` / `settle_success` / `settle_failure` / `reap_exhausted` / `status` のdefiner関数だけを呼ぶ。

1. admission：circuit breakerが閉じている（直近のprobe成功）ときだけclaimする。probeは `audit_store.probe()` で、次を確認する。
   - head lockのno-op（`lock_timeout` 付き）
   - `transaction_read_only=off`
   - `pg_is_in_recovery()=false`
   - fingerprintの一致
   - 束縛・権限（ingestと同じ判定）と、relayが送るcatalogの (source, type, adapter_version) が `registered_types` にあること（`probe(expected)`。不足があれば `store_catalog_skew` としてclaimしない）
   - relayが最後にack済みの（store_seq, event_id, envelope digest）のidentity照合（後退検知、§11。seqの大小比較は使わない）
2. claim：DB clock、`FOR UPDATE SKIP LOCKED`、新しいlease token、attempt上限は初回claimで固定する。
3. handler：
   1. `source_intact` を確認する。
   2. legacy投影と検証（catalog）を行う。
   3. `AuditStore::ingest`（timeoutはlease/3未満）を呼ぶ。
4. receipt：結果がStored/Duplicate/DuplicateReprojected/DuplicateExpiredならledgerへ記録し、Applied/KnownNoopを返す。
5. ack：fencedに行う（token一致、lease未失効、未終端）。`delivered_at`、Storeが返したseq・envelope digest・outcomeを記録する。

Store保存後・ack前にcrashした場合は、lease失効後の再配送でStoreがDuplicateを返してackへ収束する。commit結果が不明な場合も同様である。staleなtokenはrenew・ack・failのいずれもできない。

### 6.3 失敗の分類（網羅的な既定）

- Terminal（quarantine）は、次のいずれかに限る。
  - Storeの明示的な判定：`ingest` が構造化された結果行で返す `rejected:<code>` と `conflict`
  - relay側の判定：`source_digest_mismatch`、`actor_mismatch`、catalog不適合（Rustの検証拒否）。ただし `unknown_event_type` / `unknown_field` は例外とする（下記の版ずれ保留）
- それ以外は、すべて外部障害として扱い、試行を返却して保留する。具体例：
  - transport/timeout、結果不明
  - SQLSTATEの全class：08・53・25006・57P0x・57014・40001・40P01・55P03・42xxx・58xxx・XX・54xxxなど。未知のcodeや形式不正も含む
  - `ingest` の `outage:unregistered_type`（Store側の版ずれ）と `denied:<code>`（ingestを呼んだloginが登録済みsource serviceの主体でない場合）
  - `store_recovery_required` / `store_regressed` / `store_posture_invalid`
- relay側の版ずれ（Documentのdeployがrelayより先で、未知のtypeやkeyが来る場合）：`relay_catalog_skew` として保留し、healthで警報を出す。quarantineしない。Storeへは何も保存しないので、最小化は保たれる。
  - （改訂4）relay側の保留（`relay_catalog_skew`、`relay_projection_invalid`、`relay_source_unavailable`）は試行を返却し、行ごとの `relay_hold_count` で指数backoff（`backoff_min × 2^(回数−1)`、`backoff_max` で頭打ち）する。claimは保留していない行を先に取る。`outage_streak` に数えず、Storeの障害状態（`last_outage_*`、`outage_held`）に触れず、breakerを開きも閉じもしない。healthは `relay_held` で示す。Storeの結果・replay・repairで回数は0に戻る。
- store実装の `settle_failure` は、ledgerに外部障害の印があれば、fence付きで次を行う。
  1. `attempt_count` を1戻す。
  2. 上限付きbackoffを設定する（最大300 s。`outage_streak` に基づき、store側で計算する。runnerがattemptに基づいて計算するbackoffは使わない）。
  3. `last_outage_code` と `last_outage_at` を記録する。
- `outage_streak` を加算するのは、「残余の予期しない」code（`Internal` / `Other`）の場合だけとする。さらに、永続化されたsuccess_generationが、その行の前回の外部障害以降に進んでいる場合に限る（他の行は成功しているのに、この行だけが失敗し続ける状況）。`outage_streak` がpolicyの上限（既定64）に達したら、`outage_suspected_event_specific` としてquarantineする。Store全体の障害（gateの結果、既知のSQLSTATE class、transport）は数えない。成功時には `outage_streak` を0へ戻す。
- circuit breaker：
  - 外部障害で開き、指数cooldownの後にhalf-openとなる。closed以外の状態では、`max_claims_per_permit` は1を返す。
  - 構造化された結果行（stored/duplicate系/conflict/rejected）を得たら閉じる。これはStoreが動いていることの証拠になる。
  - ingestの結果を得ずにpermitが解放された場合は、half-openへ戻す。
  - probeの失敗は `Ok(None)`（claimしない）として扱い、他のDeliveryErrorにはしない。permitの `preflight` / `renew` は常にUpdatedを返す（進行中のclaimをLostにしない）。
- source改変の検出：`audit.integrity.source_mismatch_detected` を先に記録する（(event_id, code) で冪等）。Storeに記録できない間は、外部障害として保留し、quarantineしない。
- 結果不明のまま上限に達した場合：reaperが `delivery_unknown_at_limit` としてquarantineする。Storeに保存済みであれば、reconcileが `quarantined_stored` として分類し、`--repair` がSQL側のfence付きでackする（§12）。

policyの既定値：attempt 16、lease 30 s、backoff 1–300 s、batch 32、in-flight 4、outage_streak上限64（`DeliveryConfig::validate` の範囲内）。`statement_timeout` はrole単位で設定する（関数レベルの `SET` では、実行中の呼出しを制限できない）。

### 6.4 quarantineとreplay

quarantineは削除でも成功でもなく、保持される終端証拠である。`audit-relay replay --event-id`（operatorの本人login）は次の順に処理する。

1. Storeの `record_relay_control`（operatorのStore login。主体は本人）で `audit.delivery.replay_requested` を記録し、返されたseqを得る。記録できなければ中止する。
2. `audit_relay.replay(event_id, control_seq)` を1 transactionで実行する。
   1. 行を `FOR UPDATE` でlockし、quarantined・未leaseであることを確認する。
   2. 直前の終端証拠（attempt_count、attempt_limit、quarantine_code、quarantined_at、last_attempt_at）と、Store control eventの（epoch, seq）を `delivery_history` に追加する（（epoch, control seq）はUNIQUE。1件のreplay記録で複数のreplayを裏付けることはできない）。
   3. `attempt_count=0`、`attempt_limit=NULL`、`outage_streak=0`、quarantine列とlease列をNULL、`available_at=now()`、`replay_count+1` にする。

信頼の限界：Document側の関数は、Storeの事実（ackのreceipt、replayのcontrol seq）をDB内で検証できない。「Storeに記録してから戻す」という順序はCLIが守る。直接SQLで行われたreplayは、reconcileの `unaudited_replay`（`delivery_history` の（epoch, seq）がStore上で、origin=relay_control・type=replay_requested・同じevent_id・同じ旧codeに1対1で解決できない）として検出する。宣言済みの消失範囲（§11）にあるseqは、`replay_record_lost` として区別する。restartは試行履歴をresetしない。resetするのは監査付きのreplay/repairだけである。

## 7. Audit Store（`audit_store` schema）

### 7.1 表

- `publication_head`（singleton）：
  - last_seq、last_chain、recovery_epoch
  - store_fingerprint：(`pg_control_system().system_identifier`, database oid, 現timeline)
  - recovery_pending：`report_regression`（relayがidentity照合の不一致を報告する）または `declare_recovery_pending`（maintain）が設定する。その証拠（報告されたidentity、session_user、時刻、報告時のhead）も保持する。解除するのは `begin_recovery_epoch` だけである。
  - access_reapply_pending：`begin_recovery_epoch` が設定する。設定中は、本文を開示する操作（investigate、export、本文を返すverify）を、open_accessでも開いているtokenのread_pageでも拒否する（identity chainとDB内のverifyは開いたまま）。administerの主体が権限の再適用を記録し、maintainの主体が現行retentionで、policyのcutoffまで期限切れの本文を残さず `expire` を再実行すると解除される（§11）。
  - `last_chain` の初期値は `GENESIS = sha256('kp-audit-chain-genesis-v1')`、epochは1から始める。
- `events`（永続identity、削除しない）：
  - seq PK、event_id UNIQUE
  - origin（`relay` / `store` / `relay_control`。definer関数だけが設定し、envelopeからは取らない）
  - source、event_type、event_class、subject、occurred_at、stored_at
  - actor_issuer、actor_principal_id、resource_type、resource_id、result
  - envelope_digest、digest_algorithm、source_commitment、adapter_version
  - prev_chain、chain、recovery_epoch、expired_at、expired_by_seq
  - ingested_by_db_role（参考情報。chainの対象外）
  - digest/chain列はすべてNOT NULLかつ32 byteのCHECK。`seq=1` ならprev_chain=GENESISのCHECK。
- `event_bodies`：seq PK、FK、envelope jsonb。retention・purgeでのみ削除する。
- `principal_bindings`：db_role（PK）、issuer、principal_id。ownerだけが管理する（§10.2）。
- `access_grants`：issuer、principal_id、capability。
- `access_intents`：intent seq PK、token digest、session db_role、主体、操作、filter（型付き）、filter_digest、watermark W、page_size、max_pages、visibility（control eventを含むか）、期限、`creating_xid xid8 DEFAULT pg_current_xact_id()`。append-only（UPDATE・DELETE・TRUNCATEを拒否）。
- `retention_policies`：(policy_id, revision) PK、selector、retain_days（NULL=無期限、`>0`）、created_seq。追加のみで、UPDATE・DELETEはしない。
- `legal_holds`（予約）
- `registered_types`：(source, type, adapter_version)。migrationでcatalogから投入し、試験でcatalogとの一致を確認する。
- tombstoneに残す列：seq、event_id、origin、source、type、class、subject、occurred_at、actor、resource、result、各digest、commitment、chain、epoch、expiry印。「誰が何をいつしたか」を長期に残す証跡として、意図的に保持する（DC:470/473。TC:994の暫定的な答え）。chainとdigestはこれらの列に依存しないので、将来のmigrationで失効行をNULL化できる。試験で、失効行に残る列の集合を固定する。

### 7.2 ingest（`audit_store.ingest(envelope jsonb)`、definer関数）

1. 本体の最初で `PERFORM pg_catalog.set_config('synchronous_commit','on',true)` と `SET LOCAL lock_timeout` を実行する（関数レベルの `SET synchronous_commit` 句は、関数の終了時に戻ってcommitに効かないため使わない）。最初に `publication_head` を `FOR UPDATE` でlockする（全publicationの共通lock）。呼出元の `session_user` を束縛から解決し、登録済みのsource service主体（v1は `service/audit-relay`）でなければ `denied:not_source_service` を返す（Store全体の外部障害として扱う）。fingerprint・recovery_pending・postureを照合し、異常があれば `store_recovery_required` とする。
2. 構造検査：
   - specversion、id形式、閉じた属性集合
   - envelopeのtextが32 KiB以下
   - `source` が `urn:knowledge-platform:audit-store` / `audit-relay` でないこと
   - typeが `audit.` で始まらないこと
   - `resource.type` が `AuditStore` でないこと
   - (source, type, adapter_version) が `registered_types` にあること（無い場合は版ずれ。外部障害の結果を返す）
   - `source=document-platform` なら `provenance.source_commitment` が必須であること
3. 列はenvelopeから導出し、引数の値は信用しない。
4. `envelope_digest = sha256(convert_to(envelope::text,'UTF8'))`（`kp-audit-jsonb-sha256-v1`）。
5. event_idが既存の場合、既存行がorigin=relayで、source・typeが同じ場合に限ってduplicateの候補とする。それ以外は `conflict`。判定はcommitmentとenvelope digestを明示的に比較する。

   | 条件 | 結果 |
   |---|---|
   | commitment一致・envelope digest一致 | `duplicate`（失効済みなら `duplicate_expired`。本文は復活させない） |
   | commitment一致・adapter_version違い | `duplicate_reprojected`（新しいseqは振らず、元のreceiptを返す。relayは返されたdigestでackする） |
   | commitment一致・adapter_version同じ・envelope digest違い | `conflict`（投影の非決定性、またはadapter_versionの上げ忘れ） |
   | commitment違い | `conflict`、`audit.integrity.conflict_detected` を記録する |
   | commitmentを持たないadapter（将来） | duplicateはenvelope digestの完全一致の場合だけ。それ以外はconflict |

6. 新規の場合：`seq = last_seq+1`、`chain = sha256('kp-audit-chain-v1' || prev_chain || int8send(seq) || uuid_send(event_id) || envelope_digest)`。events、bodies、headを同一transactionで更新する。
7. 結果は構造化された行 `(status, seq, envelope_digest, adapter_version, code)` で返す。検証拒否とconflictは例外にしない。

`adapter_version` は、投影の全体（adapterのcodeと、出力に影響するcatalog属性：event_class、details allowlistとkind、持ち上げるfield、reason扱い、subject形、correlation写像）を識別する。既存の (source, type) の出力を変える変更では、必ず値を上げる。試験では、(source_format, adapter_version) ごとに代表fixtureのenvelope digestを固定し、上げ忘れをCIで検出する。

head lockをcommitまで保持するので、seqの公開はcommit順になる。可視のhead値は閉じたwatermarkである。直列化は意図したtrade-off（Auditの量では許容範囲。競合を試験する）。seqはStoreへのcommit順であり、業務の因果順ではない。`occurred_at` は保持する。

### 7.3 append-onlyと関数の規律

- 表への直接DML権限は誰にも与えない。変更はすべてdefiner関数経由とする。
  - eventsのUPDATEは、`expired_at` / `expired_by_seq` をNULLから設定する場合だけ許す（retention/purge関数がfunction-levelの `SET audit_store.maintenance_context` で示す）。eventsのDELETE・TRUNCATEは常に拒否する。
  - bodiesのDELETEはretention/purge関数経由のみ許し、UPDATE・TRUNCATEは拒否する。
  - GUCの条件は事故防止であり、境界は権限である。
- 全definer関数（両schema）は次を守る。
  1. `SET search_path = pg_catalog, pg_temp`。
  2. 全objectをschema修飾する。
  3. 非superuserのNOLOGIN owner（`audit_store_owner` / `audit_relay_owner`）が所有する。migrationは `SET ROLE audit_*_owner` の状態でobjectを作るか、作成後にownerを変更する。
  4. 各migrationの末尾で `REVOKE ALL ON ALL FUNCTIONS/TABLES/SEQUENCES IN SCHEMA ... FROM PUBLIC` を再実行する。`ALTER DEFAULT PRIVILEGES FOR ROLE audit_*_owner REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC`（IN SCHEMAを付けない全体形）も実行する。roleへGRANTしてもPUBLICの既定EXECUTEは消えないので、明示的に剥奪する。
  5. filterは静的SQLで処理する（jsonbから型付きで抽出し、未知keyを拒否し、`= ANY($n)` で照合する。LIKE・正規表現・動的SQLは使わない）。
  6. 書込を伴う関数は、最初の文で `publication_head FOR UPDATE` を取り、その後に状態を読む（lock順：head → policy/grant → identity）。例外として、`verify` / `checkpoint` はlockを取らずにWを読み、seq≤Wを1つのsnapshotで走査し、結果を追記するときだけhead lockを取る（commit順のpublicationなので、W以下の集合は安定している）。範囲（from/to）は有界とする。
  8. 書込を伴う関数はすべて、本体で `set_config('synchronous_commit','on',true)` を実行する。どの関数のproconfigにも `synchronous_commit` を含めない（試験で確認する）。`ALTER DATABASE` / `ALTER ROLE` でも `synchronous_commit=on` を設定する。ただしsessionはこれを上書きできる。relayと `audit-admin` は、接続後に `SHOW synchronous_commit` が `on` であることを確かめ、`options` を含む接続URLを拒否する。
  7. 読取専用の経路（`read_page`、recovery関数）では、`xmin` の比較を使わない（§10.3）。
- `audit_store.posture_check()`（content-free）は、次の違反を返す。
  - ownerが非superuserでないもの
  - search_pathが固定されていないもの
  - PUBLIC EXECUTE（aclexplodeのgrantee 0、またはproaclがNULL）
  - §10.1の行列と異なるACL
  - databaseのPUBLIC CONNECT/TEMP
  - login roleのtimeout設定の欠落（role設定は既定値であり、sessionが上書きできる。posture_checkが確かめるのは既定値だけである）
  - `synchronous_commit` のdatabase/role設定の欠落・弱化
  - capability roleのmembership（ingestは `service/audit-relay` に束縛されたloginだけ、ownerのmembershipはbootstrap用loginだけ）
  - 表・sequence・schemaのACL（relacl、nspacl）と `pg_default_acl`
  - 試験では各roleが許可されない関数で42501になること、temp tableによるshadowingが失敗することを確認する。
  - （改訂4）Store・relayの両postureは、表の権限を迂回する定義済みrole（このDBへ接続できる非superuser loginの `pg_read_all_data`・`pg_write_all_data`・`pg_maintain`、接続に関係なく全非superuser loginの `pg_read_server_files`・`pg_write_server_files`・`pg_execute_server_program`。`predefined_role_member`）、REPLICATION属性の非superuser login（`replication_login`）、列単位の権限（`column_privilege`）、0または未設定のtimeoutも報告する。relayはさらに `audit_relay_owner` のmember（`owner_member`）、capability role・loginのstaging直接読取（`staging_read`）、`audit_relay` の表への直接アクセス（`table_access`）、row security、triggerの差替えを報告する。違反の一覧と対処は運用手順 §5.4。

## 8. Integrity（決定記録 D3：方式比較と選択）

| 方式 | 検出できるもの | 限界・運用 |
|---|---|---|
| A. event digestのみ | 偶発的な改変（期待digestを信頼できる場合） | 削除、digestの同時改変、順序入替を検出できない |
| B. write-time hash chain＋event digest＋外部checkpoint | 改変、削除（seq欠番・chain断）、順序入替、checkpoint以前の全体書換え | DB ownerはchainを全体再計算でき、最後のcheckpoint以降の任意のsuffixを失わせることも、それを「復旧」として装うこともできる。v1はこれを可視化し、帯域外の記録との照合に依存させるだけで、抵抗にはDが要る |
| C. 検証時のMerkle/rolling checkpoint | Bと同等（checkpoint時点） | checkpointの作成がO(n)。行単位の自己検証性が無い |
| D. 署名checkpoint／WORM／外部anchor | DB ownerへの耐性 | 鍵管理・新しい運用基盤が要る（v1範囲外、将来拡張） |

選択：B。
- ingestはcommit順を確定するためにhead lockで既に直列化されているので、chainのcostはinsertごとのsha256 1回で済む。checkpointは（epoch, seq, chain）の1組になる。
- retention：tombstoneがdigest/chainを保持するので、本文を削除してもchainは検証可能なままである。
- migration：chainは本文ではなくdigestに依存する。
- 業務transactionをまたぐchainは作らない。

検証の種類と信頼の規則：
- `audit_store.verify`（DB内の高速検査）：
  - 開始時のheadをW（上限）に固定し、1つのquery（events LEFT JOIN bodies、READ COMMITTED、単一snapshot）で範囲を走査する。
  - 検査項目：seqの連続性、prev_chainの連鎖、chainの再計算（`IS DISTINCT FROM`）、本文digest、envelopeの属性とidentity列の一致、`envelope.id = event_id`、headの整合。
  - retention整合（集合の意味で判定する）：
    - 失効の印がある ⇔ 本文が無い。
    - `expired_by_seq` は、自分より後のseqのorigin=store eventを指す。その対象は次のどちらかである。
      - (a) `audit.retention.expired`：その件数と `expired_set_digest`（印付けたseqの昇順 `int8send` の連結のsha256）が、参照する行の集合と一致し、selectorと実効cutoffが対象行のidentity列と整合する。
      - (b) `audit.body.purged`：記録した対象seqが、その1行と一致する。
    - 参照先の本文は存在しなければならない。無ければ `retention_evidence_missing` を違反として報告する（skipしない）。
  - 結果は1件の `audit.integrity.verified`（origin=store）に記録する。この記録はWより後のseqになるので、検証は再帰しない。
- 真正性の主張（`Authentic`）は、audit-coreがDB外で連続したseq範囲のchainを再計算し、headが帯域外に保管したcheckpointと（epoch, seq, chain）で完全に一致した場合だけ行う。範囲の起点はgenesisか既に信頼したcheckpointである。headがcheckpointより先にある場合は、checkpointまでの `AuthenticThrough { seq }` にとどめる。失効の証拠が検証範囲の外にある場合は `UnverifiedExpiry` とする。完全なexportの検証は `verify_export_complete`（manifestのwatermarkとheadの一致を要求）で行う。
  - DB内verifyの結果やchain列は、真正性の証拠にならない。
  - （改訂4）exportとexpireの競合：最初のintentがWを固定した後に `expire` / `purge_body` がcommitすると、W以下の行の本文が読取り前に消え、その行はWより後の証拠を指す。snapshotで読取りを固定する代わりに、このexportを完全と扱わない：manifestは `complete: false` と `expired_after_watermark`（件数）を出し、その行を未検証の失効証拠として数える（`assess_recovery` は `Authentic` にしない）。証拠を含めて検証するには改めてexportする。
  - `audit-admin export --identity-chain` は、本文を含まない行を出力する。行の形式は§10.4のexport行と同じ10 key（seq, event_id, origin, envelope_digest, prev_chain, chain, recovery_epoch, expired, expired_by_seq, envelope）で、`envelope` は常に `null` とする（DB外の検証は、この閉じたkey集合以外を不正な行として拒否する）。
  - filter付きexportは「anchorのない部分集合」と表示する。
  - DB外の検証は、本文付きexportで次も確かめる。
    - 失効行の `expired_by_seq` が、export内の `audit.retention.expired`（件数・seq範囲・expired_set_digestが参照集合と一致）または `audit.body.purged`（対象seqが一致）を指すこと。範囲外を指すものは「未検証の証拠」として件数を報告する。
    - 失効はorigin=relayの行だけにあること。
    - 行のoriginがbodyのtype・sourceと整合すること。
    - epochの増加は+1ずつで、各増加がorigin=storeの `audit.recovery.epoch_started` の行に載っていること。
    - checkpointの比較では、chainに加えてepochも比べる。
- recovery epochの扱い（計画的な移動 `planned_move` も、消失範囲が空のepochとして扱う）：
  - 帯域外のcheckpoint記録（operatorがrestore時に、旧/新epoch、復元head、消失範囲、incident参照を追記する）にも同じ遷移がある場合に限り、DB外の検証は置換範囲内の不一致を「lost（recovery epoch k）」と報告する。この場合も「authentic」とはしない。
  - 帯域外の記録が無い、または食い違う場合は `unverified_recovery`（改変の疑い）とする。
  - 復元head以下のseqでの不一致は、常に改変として報告する。
  - verifierは全recovery epochと消失範囲を、人の確認対象として列挙する。
  - （改訂4）帯域外のrecovery記録の形式は `kp-audit-recovery-records-v1`（JSON lines、1行1遷移、全key必須の閉じた集合 `format, old_epoch, new_epoch, restored_head_seq, restored_head_chain, lost_upper`）とする。消失範囲は `(restored_head_seq, lost_upper]` で、上限が不明なら `lost_upper` は `null`（そのepochは `lost` で、`Authentic` にならない）。incident参照はこの行に含めず、別に保管する。行は `audit-admin begin-recovery-epoch` が出力する（§11）。
  - （改訂4）DB外の総合判定は `audit-admin assess --dir D --checkpoint FILE [--anchor FILE] [--recovery-records FILE]`（DB接続なし）で行う。exportを検証し直し、manifestからは作り方だけを使う。終了codeは0 `authentic`、4（要確認：`authentic_through`、`unverified_expiry`、`no_checkpoint`、`lost`、`unverified_recovery`）、5（`tampered`、`unanchored`、`broken`）、2 `store_behind`（manifestがexportを切ったことを示し、checkpointがその最後のseqより先。audit-coreの判定を `underlying_verdict` に併せて出す）、1（入力fileの不正）。
- healthの「verified」は、origin=storeの `audit.integrity.verified` だけを数える。

## 9. Retention

- `retention_policies` は版付きで不変とする。`set_retention_policy`（administer）は新しいrevisionを追加し、control eventに全内容を記録する。
  - 既定では行が無いので失効しない。年数は固定しない（OA §26）。
  - selectorの文法は、event_types（catalogの文法、最大16）／event_classes／sources（catalogのsource、最大16）の列挙である。origin=relayのeventだけを対象にできる。
  - （改訂4）selectorのevent_typesは、文法に合っても、Storeの `registered_types` にある登録済みrelay typeでなければ拒否する（`audit.*` のcontrol typeは不可）。sourcesは登録済みrelay source、event_classesは8 classに限る。各keyは1–16件で、少なくとも1つのkeyが要る。不正なselectorは `invalid_input` として記録して拒否する。
- `expire(policy_id, expected_revision, cutoff, limit≤1000)`（maintain）の手順：
  1. lock順はhead → policy → identity。最新revisionが `expected_revision` と一致しなければstaleとし、何も削除せず試行を記録する。
  2. `retain_days IS NULL` なら `not_expirable` とし、何も削除せず試行を記録する。
  3. legal holdが1件でも有効なら削除しない（v1の拡張境界。hold内容の判定規則は将来の拡張）。
  4. 実効cutoffを `effective_cutoff = least($cutoff, transaction_timestamp() - make_interval(days => p.retain_days))` として1回だけ計算する。
  5. 対象の述語：

     ```sql
     e.origin = 'relay'
     AND e.occurred_at < effective_cutoff
     AND body存在 AND e.expired_at IS NULL AND selector一致
     ```

     origin=store/relay_controlのcontrol event（verify・recoveryの根拠）は、v1では失効しない。
  6. `audit.retention.expired` を先に記録し、そのseqで対象を印付けてから本文を削除する（`expired_set_digest = sha256('kp-audit-expired-set-v1' || 印付けたseqの昇順int8beの連結)`）。記録する内容は、policy_id、revision、selector snapshot、retain_days、入力cutoff、effective_cutoff、transaction時刻、count、seq範囲、`expired_set_digest`。identity・digest・chainは残す。
- 失効済みeventの再配送は `duplicate_expired` を返し、本文は復活しない。
- `purge_body(event_id, purge_reason_code)`（maintain、origin=relayのみ）：最小化の失敗（adapterの不具合で禁止情報が入った等）への個別対処である。`audit.body.purged`（対象seq、event_id、code）を先に記録し、その行に印を付ける。
- source stagingの削除（破壊的なcleanup）はv1では提供しない。Storeの失効は、source側のcopyを消したことを意味しない。

## 10. 調査・export・設定変更の認可と監査

### 10.1 role・credential行列（DB層）

roleの作成はtemplate（`crates/audit-store-postgres/sql/roles.sql`、`crates/audit-relay/sql/roles.sql`）で行い、DB ownerが適用する。capability role（NOLOGIN）をlogin roleへGRANTする。

（改訂4）EXECUTE行列の正本は上記2つの `sql/roles.sql` であり、`posture_check()` はそこからのずれを違反として報告する。下表はその要約で、単位Bの実装に合わせて古い行を直した。

| capability role | EXECUTE可能な関数 |
|---|---|
| `audit_store_ingest` | `ingest`、`probe`、`report_regression`（`service/audit-relay` に束縛されたloginだけが持つ） |
| `audit_store_reconciler` | `lookup_receipts`、`list_source_receipts`、`lookup_control_receipts`、`lookup_lost_ranges`、`store_status`（content-free。serviceのloginとoperatorのloginに付与） |
| `audit_store_relay_control` | `record_relay_control`（seqを返す） |
| `audit_store_reader` | `open_access`（investigate/export）、`read_page`、`close_access` |
| `audit_store_verifier` | `open_access`（verify/identity_chain）、`read_page`、`close_access`、`verify`、`checkpoint`、`verify_recovery`、`identity_chain_recovery_page` |
| `audit_store_admin` | `change_access`、`set_retention_policy`、`record_access_reapplied` |
| `audit_store_maintainer` | `expire`、`purge_body`、`confirm_retention_reapplied`、`begin_recovery_epoch`、`declare_recovery_pending`、`verify_recovery`、`identity_chain_recovery_page` |
| verifier・admin・maintainer | `store_status`、`posture_check`（content-free） |
| owner（`audit_store_owner`）のmemberのみ | `bootstrap_administrator`、`bind_principal`、`unbind_principal`、`register_source_service` |
| `audit_relay_worker` | `claim`、`renew`、`settle_success`、`settle_failure`、`reap_exhausted`、`mismatch_seq`、`note_mismatch`、`report_runtime`（`run` のcircuit状態）、`preview_pending`（healthの見込み。配送前の内容を投影するのでworkerだけ）、`status`、`policy`、`acked_head`、`posture_check`、読取専用の `reconcile_page` / `reconcile_history_page` / `lookup_deliveries` |
| `audit_relay_operator` | `replay`、`repair_ack_stored`、`repair_reset_missing`、`register_missing`、`status`、`policy`、`acked_head`、`posture_check`、`reconcile_page` / `reconcile_history_page` / `lookup_deliveries` |

- PUBLICにはどの関数のEXECUTEも与えない。Store DBのCONNECT・TEMPもPUBLICから剥奪する。
- Store roleには、`idle_in_transaction_session_timeout`、`statement_timeout`、`lock_timeout` をrole単位で設定する。
- `audit_relay_owner` には、Document ownerが `public.audit_outbox_events` へのSELECT・REFERENCESを与える。

| command | 接続 |
|---|---|
| `audit-relay run` | `AUDIT_SOURCE_DATABASE_URL`＝`audit_relay_worker` を持つlogin。`AUDIT_STORE_DATABASE_URL`＝`audit_store_ingest`＋`audit_store_relay_control`＋`audit_store_reconciler` を持つloginで、`service/audit-relay` に束縛（改訂4：reconcilerも持つ。同じloginで読取専用のreconcile・healthも実行できる） |
| `audit-relay replay` / `reconcile --repair` | operator本人のDocument login（`audit_relay_operator`）とStore login（`audit_store_relay_control`＋`audit_store_reconciler`、本人の主体に束縛）。（改訂4）`audit_store_ingest` を持つStore loginはCLIとStoreの両方で拒否する |
| `audit-relay reconcile`（定期・読取専用）/ `health` | Document側はworkerのlogin（`health --forecast` はworkerだけ）。Store側はoperatorのlogin（relay_control＋reconciler）か、serviceのlogin（改訂4：healthの `store_catalog_skew` の判定はprobeできるserviceのloginだけ） |
| `audit-admin` | operator本人のStore login |
| `migrate` | 別のURL（runtimeでは使わない） |

- migrate・bootstrap・bind・unbind・register-source-serviceを除くcommandは、起動時にsessionが `rolsuper`、または `audit_*_owner` のmemberであれば拒否する（多層防御）。
- Document側の `reconcile_page` 等は、source_digestとsaltを返さない（`source_intact` はserver側で計算した真偽値だけを返す）。

### 10.2 主体の束縛（認証境界）

- 本番identityが確立するまでは、DB login roleが認証境界である。各関数は、`session_user`（SECURITY DEFINER内でも呼出元のloginを指す）を `principal_bindings` で（issuer, principal_id）へ解決する。
  - 未束縛なら `audit.access.denied`（unbound）を記録してdeniedを返す。
  - 呼出側から主体を引数で渡すことは無い。
  - v1では、operatorごとに1つのDB login roleを持つ。
- 束縛（`bind_principal` / `unbind_principal`）はowner roleのmemberだけが行う（`pg_has_role(session_user, 'audit_store_owner', 'MEMBER')`、head lockの後）。administerは束縛を変えられないので、自分が誰として振る舞うかを変更できない。束縛は上書きせず、変更にはunbindを先に記録する。
- 権限（Audit上の責務。Organization roleではない）は、`investigate`、`export`、`verify`、`administer`、`maintain`。DB層のcapability roleを持ち、かつ束縛された主体にAudit上の権限がある場合だけ実行できる。Document ACLは流用しない。
- `change_access` は、呼出者自身の主体への付与を拒否し、その試行を記録する。2人の管理者の共謀と二人承認はv1の範囲外とする（すべての変更は主体とsession_userで記録され、検出できる）。
- control eventには、主体と `session_user` の両方を記録する。主体に束縛されていないsession（未束縛の拒否、bootstrap）では、actorを `{issuer: "db_role", principal_id: <session role>}` とする（role名は `^[a-z_][a-z0-9_$]{0,62}$`）。control eventのdetailsは、閉じたkind（source、resource ref、db_role、enum等）と、文法で制限したkind（event type、principal）だけで構成し、自由記述fieldを持たない。event typeのfilter・selectorは、登録済みtypeとcontrol typeに限定する。actorのfilter値は、上限付きのprincipal文字列として残る（受容した残余）。
- 最初の管理者：`bootstrap_administrator(db_role, issuer, principal_id)` はownerのmemberだけが呼べる（`current_user` は使わない）。head lockの後にadministerが0件であることを確認して成功する。最後の管理者を失った場合は、同じ経路で監査付きのlockout回復とする。
- `audit.access.*` などcontrol eventの閲覧：investigate・exportのどちらでも、administer権限を持つ場合だけ含める（intentの時点で可視範囲を固定する）。

### 10.3 開示の2段階（fail-closed）

1. `open_access(op, filter, page_size, max_pages)`
   1. 主体・権限（opに応じたcapability role：investigate/exportはreader、verify/identity_chainはverifier）・入力を検査する。filter allowlist：event_types≤16、source、actor、resource、occurred範囲、event_ids≤100、`seq_after`（排他的下限）、`seq_through`（上限、W以下）。identity_chainと、検証用のexportでは、`seq_after` / `seq_through` だけを許し、全origin（control eventを含む）を対象にする。page sizeはinvestigate 1–100、export・identity_chain 1–1000。max_pagesは最大100。大きなStoreでは、intentを順に開いて連続範囲を検証する（各intentを記録する）。`access_reapply_pending` の間は、investigate/exportと本文を返すverifyを拒否する。
   2. watermark W（現在head以下。clientは指定できない）と可視範囲を固定する。
   3. `audit.access.intent_opened`（op、型付きfilterの全値、filter_digest、W、page_size、max_pages、可視範囲、期限）を記録する。
   4. `access_intents` に、`creating_xid = pg_current_xact_id()`（savepoint内でも最上位transactionのID）とともに保存する。
   5. 期限付きのtoken（既定10分）を返す。

   拒否・入力不正は、入力を検証した後、bounded shapeのみを `audit.access.denied` として記録する。

   （改訂4）relayのcycleごとに繰り返されるingest経路（`ingest`、`probe`、`report_regression`）の拒否（unbound、not_source_service）は、loginごとに同じcode・actorの連続を1分に1件の記録へまとめる。間引きはしない：各記録は自身と任意field `suppressed_since_last`（catalogへ加法追加）件の拒否を表し、拒否の総数は記録ごとの `1 + suppressed_since_last` の和に等しい。codeかactorが変わるとき、そのloginが成功したとき、連続が止まって集約window（1分）を過ぎた後の次の追記（control event、relay eventのingest）とprobeの前に、未記録の件数を先に記録する。verify・checkpoint・開示intent・expire・purgeは、記録の前に全loginの未記録分をwindowに関係なく記録する（証拠より前に全拒否がchainにある）。未記録の件数は `store_status` の `denials_pending` に出る。recovery中は記録しない。
2. `read_page(token, after_seq)`。次の条件をすべて満たす場合だけデータを返す。
   - 入口で `pg_current_xact_id_if_assigned() IS NULL`（現transactionがまだ何も書いていない。savepointで書いて戻した場合も拒否される）であること。
   - `pg_xact_status(intent.creating_xid) IS NOT DISTINCT FROM 'committed'`（NULLや実行中は拒否）であること。
   - filter・W・page_size・max_pages・可視範囲・opは、chainに入ったintent eventの本文から取り、`access_intents` の写しと一致することを確かめること。
   - intentのcommitが永続化済みであること：committed判定の後に `X = pg_current_wal_insert_lsn()` を1回だけ取り、`pg_current_wal_flush_lsn() >= X` になるまで短い間隔で待つ（上限約2秒）。上限に達したら、再試行可能な `intent_not_durable` として何も返さない（commit recordはclogの更新より前に挿入されるので、commit_end_lsn ≤ X が成り立つ）。
   - 同じ `session_user` のものであること。
   - 期限内であること。
   - 現在の束縛と権限が有効であること（取消しは開いているtokenにも効く）。

   開示する集合は「(filter ∩ seq_after<seq≤min(seq_through, W) ∩ 可視範囲) を、identity行（失効行を含む）のseq順に並べた先頭 max_pages×page_size 行」である。after_seqを任意に選んでも、この集合の外は返さない（state無しで有界）。read_pageはREAD ONLYなので、ここでの拒否はerrorとして返り、記録されない（拒否は何も開示しない）。READ ONLY設定やautocommitは呼出側の慣行であり、保証はserver側の上記の判定が担う。
3. `close_access(token, returned_count, page_digests)`：件数とpage digestを記録する（任意）。開示の範囲は、chainに入ったintent eventで既に確定している。

- 拒否の記録は、呼出側がBEGIN/ROLLBACKで包めば消せる。拒否は何も開示しないので、この限界を明記する。adminとmaintainの操作（変更系）は、変更とcontrol eventが同一transactionで成否を共にする。
- audit-of-auditの範囲は関数経由の読取に限る。次は対象外であり、運用上の権限管理で守る。
  - `pg_dump` によるbackup（Store・Document DBの両方。Document DBにはstagingの理由文がある）
  - owner/superuserによる直接の読取
  - recovery mode中のcontent-freeな読取（§11）
  - CLI hostに出力したexport file

  export fileとbackupは、mode 0600とし、logに内容を出さず、保管場所を運用手順に記す。`checkpoint` は `verify` 権限の操作として記録する。

### 10.4 export形式

- export行の形式：

  ```
  {"seq":…,"event_id":…,"origin":…,"envelope_digest":…,"prev_chain":…,"chain":…,"recovery_epoch":…,"expired":…,"expired_by_seq":…,"envelope":<jsonb text原文>}
  ```

  SQLで `envelope::text` を連結して生成し、serdeでの往復はしない。identity chain（§8の `--identity-chain`、§11の `identity_chain_recovery_page`）も同じ10 keyの行で、`envelope` を常に `null` とする。
- `audit-core` のexport検証は、RawValueで原文を保持し、sha256とchainを再計算する。
- manifestには、件数、seq範囲、watermark、page digest、intentのseq、照合したcheckpoint、GENESIS定数を記す。（改訂4）検証の結果として `chain_integrity`（`intact` / `unanchored`）、`complete`、`expired_after_watermark`、`unverified_expiry_evidence` も記す（exportとexpireの競合は§8）。
- relay用の `lookup_receipts` / `list_source_receipts` / `lookup_control_receipts` は、content-free（seq、epoch、event_id、origin、type、envelope digest、commitment、expired、control対象event_id）である。DB role（reconciler）で制限し、呼出ごとのcontrol eventは作らない（audit-of-auditの例外：本文を含まない）。reconcileは1 runにつき1件の `audit.reconciliation.completed` を記録する。healthは、content-freeで監査対象外の `store_status()` を使う。

## 11. Backup / restore 契約

- 対象は2つ：Document DB（staging＋`audit_relay`）とStore DB。どちらも既存PostgreSQLの `pg_dump -Fc` / `pg_restore` で扱い、新しい基盤は要らない。Document DBのbackupはrelayを停止してから取る。backupの後とその定期にcheckpointを取る。
- 外部checkpointの記録（`audit-admin checkpoint` の出力JSONと、restore時のepoch遷移の追記）は、DBとは別の場所に保管する。同じ管理者が持つbackupは独立anchorではない。
- （改訂4）backupのlogin：Document DBの `pg_dump` はsuperuserで行う（relayのpostureは `pg_read_all_data` を持つloginを `predefined_role_member`、`audit_relay_owner` のmemberを `owner_member` として報告し、`run` が起動しなくなる）。Store DBの `pg_dump` は `audit_store_owner` のmemberかsuperuserで行う。どちらもREPLICATION属性の非superuser loginを使わない（§7.3）。
- restoreの前提：
  1. 復元先clusterに、globals（`roles.sql`、cluster移行時は `pg_dumpall --globals-only`。`principal_bindings` が参照するlogin roleを含む）を先に作る。
  2. `pg_restore --exit-on-error --single-transaction` で復元する。`--no-owner`・`--no-privileges`・`--no-acl`・`--role` は禁止する（roleが無ければrestoreを失敗させ、PUBLIC剥奪の消失を防ぐ）。
  3. 復元後に、冪等な `privileges.sql`（database ACL、role設定、PUBLIC剥奪）を再適用する。`posture_check()` が違反を返す間は、`begin_recovery_epoch` が `store_posture_invalid` で拒否する。
- 復元の検知とrecovery mode：
  - fingerprint：(system_identifier, database oid, timeline) が `publication_head` と異なる場合、またはrecovery_pendingの場合をrecovery modeとする。
    - system_identifierは別clusterへの論理restoreを検知する。
    - oidは同じcluster内でのrestoreを検知する。
    - timelineはPITR・promotionを検知する。
    - 残る限界：同じtimelineでのcrash recoveryのみの物理restore・snapshot restoreは、fingerprintでは検知できない。
  - 後退検知：relayは、最後にack済みの（store_seq, event_id, envelope digest）を `lookup_receipts` でidentity照合する。不一致または欠落なら `store_regressed` とし、`report_regression` でStoreへ報告してrecovery_pendingを設定させる。Storeのrecovery epochが変わるまで、claimを止める（自動では解除しない）。同じtimelineでの物理restore・snapshot restore、async commitの消失など、fingerprintで検知できない場合も、この経路か、operatorの `declare_recovery_pending` でrecovery modeへ入る。operatorが把握しているrestoreでは、接続を止めてから `declare_recovery_pending` を実行し、その後に接続を再開する。ingest権限の保持者による悪用は、サービス停止にとどまる。
  - recovery mode中は、publicationのすべての関数（ingest、record_relay_control、open_access、read_page、verify、checkpoint、change_access、bind/unbind、set_retention_policy、expire、purge_body、close_access）が `store_recovery_required` を返す。許可されるのは次に限る（網羅的な一覧）。
    - content-freeの状態取得：`probe`、`store_status`、`posture_check`、`lookup_receipts`、`list_source_receipts`、`lookup_control_receipts`（本文を返さず、recovery状態を報告する）
    - `verify_recovery()`：READ ONLYでverifyと同じ検査を行い、何も追記しない。
    - `identity_chain_recovery_page(after_seq, limit)`：content-free、intentなし、本文なし。行は§10.4のexport行と同じ10 keyで、`envelope` は常に `null`（§8の `--identity-chain` と同じ形式）とする。
    - `report_regression` / `declare_recovery_pending`（recovery_pendingの設定のみ）
    - `begin_recovery_epoch`

    recovery mode中の読取は、呼出ごとの監査記録を残さない（content-freeであり、restoreを行う者は既にownerの権限を持つため）。`verify_recovery` と `identity_chain_recovery_page` は、recovery modeでない場合は `not_in_recovery` を返す。
  - 計画的な移動（`pg_upgrade`、dump/restoreによる移行、計画的なpromotion）も、消失範囲が空の `begin_recovery_epoch`（分類 `planned_move`、照合checkpoint＝移動前のhead）で扱う。fingerprintを書き換える別の経路は設けない。
- Store restoreの手順（relayは手順4まで停止）：
  1. 前提に従って新しいDBへrestoreし、権限を再適用する。
  2. `audit-admin verify --recovery` で内部整合を確認する。
  3. `audit-admin export --identity-chain --recovery` で、最新の外部checkpointとDB外で照合する。
  4. 帯域外の記録へepoch遷移を追記し、`begin_recovery_epoch(checkpoint)` を実行する。この関数は次を行う。
     - fingerprint不一致かrecovery_pendingでなければ拒否する（解除するのはこの関数だけ）。
     - head lockの下で、自らverifyを再走査し、復元headを計算する。
     - control eventの記録：
       - 照合したcheckpointと照合の分類
       - recovery identity範囲のdigest
       - 消失したseq範囲：(復元head, max(checkpoint seq, relayの最大store_seq)]。relayの最大store_seqは、そのepochでrelayが参照する最大のStore seq（receiptに加え、source mismatch・replay・repairのcontrol event）である（`audit-relay health` の `stored.relay_max_seq`）。上限は「主張値」として記録し、復元headより小さい値は拒否する
       - 旧/新fingerprint
     - 新fingerprintを設定する。
     - （改訂4）署名は `begin_recovery_epoch(checkpoint_epoch, checkpoint_seq, checkpoint_chain, relay_max_seq, expected_old_epoch, expected_head_seq, expected_head_chain, expected_lost_upper)` とする（checkpointとrelay最大seqは任意）。期待値は帯域外のrecovery記録（§8）で、Storeがhead lockの下で計算した実際の値と4つとも（上限の既知・不明を含めて。不明はNULL）一致した場合だけepochを開始する。食い違えば `refused`/`expectation_mismatch` で何も変えない。期待値を4つとも省いた呼出しはpreview（`refused`/`expectation_required`）で、実際の値（旧/新epoch、復元headのseq・chain、分類、checkpointの分類、消失範囲の下限・上限・上限既知か）だけを返す。CLIは `audit-admin begin-recovery-epoch [--checkpoint FILE] [--relay-max-seq N] (--preview | --expect-old-epoch N --expect-head-seq N --expect-head-chain HEX --expect-lost-upper N|unknown)` で、previewでも開始後でも2行目に記録の1行（`kp-audit-recovery-records-v1`）を出す。operatorはpreviewの2行目を帯域外の記録へ追記してから、その値で開始する。上限は `max(復元head, checkpoint seq, relay最大seq, regressionの報告seq)` で、checkpoint・relay最大seq・regressionの報告のいずれも無い場合は不明（`lost_upper_known: false`、記録では `lost_upper: null`）になる。
  5. `audit-relay reconcile --repair` で、delivered_missingをpendingへ戻す（履歴を保存する）。
  6. 再配送する（idempotent）。
  7. 通常の記録付きverifyを実行し、新しいcheckpointを取る。

  epochはchainを継続する（genesisへ戻さない）。
- 限界：
  - Store内で生成されたcontrol event（閲覧intent、拒否、retention、replay、integrity、権限変更）は、backup以降の分がsourceを持たないため回復できない。RPOを縮めたい場合は、WAL archiving/PITRを運用上で選択する（新しい依存は不要）。
  - 復元後は `access_reapply_pending` が設定され、investigate/exportと本文を返すverifyは閉じたままになる。帯域外の記録から権限の失効・束縛解除・retention revisionを再適用し、それをcontrol eventとして記録し、現行retentionで期限切れの本文が残らなくなるまで `expire` を再実行すると開く。失効より前の古いbackupをrestoreすると失効済みの本文が戻り得るので、restore後に現行retentionを再適用して記録する。
- source（Document DB）を古いbackupへ戻した場合、Storeにあってsourceに無いeventが生じ得る。reconcileはこれを `store_only` として報告し、自動では削除しない。Document DBのrestoreでも、`audit_relay` に対して同じ前提（globals、権限の再適用、`audit_relay.posture_check()`）を適用する。違反がある間、relayの `run` / `replay` / `repair` は起動を拒否し、healthで警報を出す。

## 12. Health・reconciliation・restart

- `audit-relay health`（JSON）：
  - produced：staging件数、登録件数、未登録件数、repair登録件数
  - pending / leased / retry待ち / quarantined（code別）、最古pendingの経過時間、delivered件数
  - Store可用性：circuit状態、`store_recovery_required` / `store_regressed` / `store_posture_invalid`
  - Store head seq（stored）
  - 検証の被覆（verified）：最後の違反以後の `ok` の `audit.integrity.verified`（origin=store）がgenesisから連続して覆う最大seqと時刻、verification lag。違反は、その後にgenesisから走査時のheadまでの検証が `ok` になるまで報告し続ける（部分範囲・空の範囲では解除しない）
  - 導入状態：登録trigger、guard trigger、digest関数の存在、policy revision
  - reconcileの警報（unaudited_replay、delivered_missing、digest_mismatch、store_only、relay_catalog_skew、store_catalog_skew、audit_relayのposture違反）
  - （改訂4）circuit：healthは別processなので、各 `audit-relay run` processが1秒ごとにbreakerを標本化し、変化時と少なくとも10秒ごとに自分の行（process起動時の乱数id）を `audit_relay.report_runtime`（workerだけ）で `audit_relay.relay_runtime` へ報告し、正常停止で削除する。報告できなくても配送は止めない。healthの `circuit` は `running`（60秒以内に報告）・`stale`、runningのうち最悪の `state`（`open` → `half_open` → `closed`、runningが無ければnull）とその `gate`、`outage_streak`（最後の構造化ingest verdict以後のStore障害の連続）、`outages`、`last_report_age_seconds` を出し、`open` で `circuit_open` を警報する。
  - （改訂4）追加の値と警報：`delivered.relay_held` と `relay_held`（§6.3のrelay側の保留）、`stored.denials_pending` と `store_denials_pending`（§10.3）、`stored.relay_max_seq`（§11の入力）、`staging_rows_hidden`（登録件数がrelayに見えるstaging件数を超える）、`circuit_open`。`store_catalog_skew` はprobeできるrelay serviceのloginで実行したときだけ判定する（operatorのloginでは `stored.missing_types` がnull）。
  - （改訂4）`run` はstderrへ有界な進捗行を出す：`event=circuit`（circuit状態かgateの変化）、`event=progress`（処理があったときだけ、`AUDIT_RELAY_PROGRESS_MS`（既定10000）に最大1行）、`event=final`（停止時に未出力の件数があれば）。値は固定codeと件数（`delivered`、`duplicate`、`held`、`outage`、`quarantined`）だけである。
  - （改訂4）検証の被覆は、被覆が依存する `audit.integrity.verified` を書く `record_verified` がhead lockの下で同じ関数で1行の状態（`verification_state`）を更新し、`probe`（relayのcycleごと）と `store_status` はその行を読む（毎回再計算しない）。意味は上記の被覆と同じである。

  labelは固定codeのみとし、principal・resource・payloadを出さない。
- `audit-relay reconcile`（service identityで定期実行するread-only版と、operatorが実行する `--repair` 版）：deliveriesとStore receiptを照合する。event_idのbatch照会と、`list_source_receipts` によるseq順のpagingを組み合わせ、commitmentも比較して次に分類する。

  | class | 意味 |
  |---|---|
  | ok | 一致 |
  | delivered_missing | relayはackしたがStoreに無い |
  | digest_mismatch | digestまたはcommitmentが一致しない |
  | quarantined_stored | `delivery_unknown_at_limit` でquarantineされ、Storeのcommitmentが一致する |
  | quarantined_conflict | quarantine済みでStoreのcommitmentが異なる |
  | pending | 配送待ち |
  | quarantined | quarantine済み |
  | unregistered | 配送登録が無い |
  | source_tampered | digestを再計算すると一致しない |
  | store_only | Storeにあるがsourceに無い |
  | unaudited_replay | replayの記録がStoreで1対1に確認できない |
| replay_record_lost | replayの記録が宣言済みの消失範囲にある |
| relay_catalog_skew | relayのcatalogに無いtype・keyのため保留中 |

- `--repair` は次の処理だけを行う。新しいIDの生成や事実の書換えはしない。
  - delivered_missingをpendingへ戻す（履歴を保存）。
  - quarantined_storedをStore receiptでackする（履歴を保存）。SQL関数 `repair_ack_stored` 側で次をfenceする。
    - `quarantine_code='delivery_unknown_at_limit'` であること
    - 渡されたcommitmentがserver側で再計算した値と一致すること
  - unregisteredを登録する。

  conflict・source_digest_mismatch・actor_mismatch・検証拒否のcodeは、ackしない。store_only、digest_mismatch、quarantined_conflictにも触れない。結果は `audit.reconciliation.completed` に記録する。
- restartは試行履歴をresetしない。lease失効後に再claimする。shutdownではclaimを止め、有界にdrainし、未完了分はlease失効に任せる。

## 13. 境界とhandoff

- Document（決定記録 D4）：TC:797に基づき、Auditの経路（`audit_relay` ledger）が `public.audit_outbox_events` に、登録trigger・append-only guard・deliveries FK・`BEGIN ATOMIC` digest関数を追加する。
  - migrate順：Document ledger → `audit-relay migrate`。
  - `audit-relay migrate` は事前検査（preflight）として、`audit_outbox_events` の必要な列と型、Document ledgerの存在を確認する。
  - migrationを実行するroleは、`audit_outbox_events` のownerまたはsuperuserとし、`audit_relay_owner` へSELECT・REFERENCESを与える。
  - この表に触れる今後のDocument migrationには、Auditのreviewが要る。`DROP COLUMN ... CASCADE` を禁止する（digest関数が消え、producerのINSERTがruntimeで失敗するため）。
  - Documentへの引継ぎ：
    - `authorization.denied` の対象範囲と試験
  - client指定IDにnil UUIDを受け付けないこと（現在はquarantine。§4.3）
  - principal/issuerの文字種方針（identity adapter・IdPの責任。Storeはbidi等を拒否する）
    - 取下げ・公開終了時のschedule terminal audit
    - withdraw/endの理由文の上限
    - 通常ACLのreason未保存
    - W3C traceparent列の追加（TC INV-10・AC §13のtrace_id充足）
    - principal/issuerの長さ上限（256 byte）
    - 本番でのrole分離（serveを非superuserで行う）
    - 理由文を開示する機能の要否
- Organization：Role/Delegation/WorkItem/Workflowの意味をここで定義しない。現在の帰属情報を保持する。将来の接続点は次の3つ。
  1. source adapter（source_format/adapter_versionで識別）
  2. catalogに登録した `extensions` 名前空間（例 `org.work.v1`）
  3. Organization所有のmigrationで追加する配送登録

  詳細は `docs/superpowers/handoffs/audit-infrastructure-v1-organization-handoff.md` に記す。
- Search：`search_audit_outbox_events` とR04AはSearch担当の領域である。adapter契約を渡す。内容は次のとおり。
  - 配送登録：Search所有のmigration、またはSearchが付与する未配送index＋grant
  - class/resultの写像
  - commitmentを持たないadapterでのduplicate規則（§7.2）
  - NO_RETENTION由来のfieldについて、digest・長さ・hidden Source IDを計算せず保持しない（OA SD-O1/O2）

## 14. 試験と受入

合成データのみを使う。PostgreSQL 18.6（testcontainers。sourceとStoreは別database）で行う。roleの経路を確かめる試験は、superuserではなくroles.sqlで作った各loginで実行する。

1. schema互換：
   - acceptされること：main producerのpayloadの全形。
   - 拒否されること：機微key、未知key、型違反、浮動小数、上限超過、重複key、UUID以外のcorrelation、control偽装。
   - legacy_time配列の形。
   - 生成schemaの再現性と、Rust⊂schemaの関係。
   - (source_format, adapter_version) ごとに、envelope digestを固定したfixture（`spec/telemetry/audit-adapter-golden.json`、Store側はjsonb digest）。
   - 失敗分類：全SQLSTATEが外部障害になり、terminalは構造化verdictだけであること。
2. Store：
   - idempotency：duplicate、duplicate_reprojected、duplicate_expired、conflict（commitment違い、origin/source/type違い）。
   - append-only：UPDATE/DELETE/TRUNCATE/直接INSERTの拒否。
   - head直列化と遅延commit。genesis。
   - chain検証と、改変・削除・順序入替・retention偽装・purge偽装の検出。
   - DB外のidentity chain照合と外部checkpoint照合。suffixを切り詰めてepoch_startedを捏造した場合の判定（帯域外記録なし→unverified_recovery、あり→lost）。
   - role行列（42501）、posture_check、`search_path` とshadowingの回帰。
   - 束縛されない主体・権限の無い主体のdenied記録と非開示。自己昇格の拒否。bind/unbindがowner専用であること。bootstrapの一回性・並行性。
   - 開示：
     - 同一transaction内で、SAVEPOINT→RELEASE、SAVEPOINTを開いたまま、ROLLBACK TO、DO＋EXCEPTION、open_access内部のEXCEPTIONの各経路でread_pageが拒否されること。
     - 後続のREAD ONLY transactionでは成功すること。
     - 任意のafter_seqで上限を超えないこと。
     - 権限の取消しが開いているtokenに効くこと。
     - access_intentsのfilterを改ざんすると検出されること。
   - export検証。
   - retention：NULL policyのnot_expirable、cutoff前後と実効cutoff、stale revision、hold、control eventが対象外であること、tombstoneの列集合、purge。
   - 検証の非再帰。
   - fingerprint gateとrecovery mode（別container・同じoidへのrestore、網羅的な一覧以外の拒否、headが不変であること、epoch_startedがrestored_head+1に来ること）、posture違反での拒否、`report_regression` / `declare_recovery_pending` によるrecovery mode、`planned_move` のepoch、`access_reapply_pending` によるinvestigate/exportの閉鎖と解除、recovery関数がrecovery外で `not_in_recovery` を返すこと。
   - 永続性：ingest・open_accessのcommit時に `synchronous_commit=on` が効いていること。async commitのintentでは、flushまでread_pageが開示しないこと。proconfigに `synchronous_commit` を含む関数が無いこと。
   - ingestを呼んだloginがsource service主体でない場合の `denied`。reconcilerの読取経路。
   - control eventのcatalog適合。
3. 配送：
   - 登録triggerの失敗で業務がrollbackすること。
   - Store停止（接続拒否）、read-only、lock timeout、statement timeout、未登録type（版ずれ）で、業務は継続し、pendingが増え、試行を消費せず、復旧後に排出されること。
   - breakerは構造化された結果を得るまで閉じないこと。特定eventだけが残余の予期しないエラーを起こし続ける場合の上限（Store全体の障害は数えないこと）。
   - relay側の版ずれ（未知type・key）が保留と警報になり、quarantineされないこと。
   - claim projectionが、scalar/arrayの `data` でも例外を起こさないこと。改変かつoversizeの行が `source_digest_mismatch` になること。
   - `audit_relay.posture_check()` の違反で、run/replay/repairが起動を拒否すること。
   - 不正schemaのquarantine、source改変の検出とcontrol event、staleなlease、commit結果不明。
   - 保存後・ack前のcrash（子processをkill -9してrestart）。最終試行での保存後crashと、`quarantined_stored` のfence付き修復（conflict codeは修復されないこと）。
   - 同時重複、adapter_version違いの再配送、上げ忘れでconflictになることとその修復。
   - replay（試行予算の再設定と履歴）、直接SQLでのreplayが `unaudited_replay` として検出されること、reconcile/repair、store_only。
   - backup → restore → gate → epoch → 再配送。
   - 同一databaseの拒否、superuser/owner sessionでの起動拒否、deliveries guard。
4. Document E2E：作成、公開、予約公開（scheduler attribution `service/scheduler`）、取下げ、metadata変更、folder操作、文書移動、ACL変更（通常・bootstrap）、初回既読、原本アクセス、Diffアクセス、revision比較、拒否（management・既読）、公開終了を、実producer経由でStoreまで届ける。
   - 確認すること：actor/resource/correlation、理由文が複製されていないこと、chainの検証。
   - staging失敗時に業務がrollbackすること。
   - Document migrationをrelay migrationの後に追加する場合（nullable列のADD）の互換。
