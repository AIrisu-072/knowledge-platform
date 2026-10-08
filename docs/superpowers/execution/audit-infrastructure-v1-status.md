# Audit Infrastructure v1：実行状況

## 2026-10-08 — 単位B 独立review（Store・relay × security・correctness）の指摘反映（worktree branch、未push）

- branch `worktree-agent-a07ac0dcd80b1fa0d`（`46ed5f6` の上）。container再起動で中断した前回の未commit変更を見直して引き継ぎ、残りを実装した。各blocking指摘は、先に再現試験を書き、修正前のcodeで失敗することを確認してから直した。
- Store（`audit-store-postgres`）：
  - verifyの本文開示：`access_reapply_pending` の間は、`open_access('verify')` と、開いているverify tokenの `read_page` も55000で拒否する（identity chainとDB内 `verify` は可）。
  - verify範囲：`1 ≤ from ≤ to ≤ head` 以外（headを越えるto、headより後のfrom、空範囲）を `invalid_input` で拒否し、偽の `violations` を記録しない。記録のhead（seq・epoch・chain）は範囲内で実在する最後の行のもの。
  - 検証状態：`store_status` / `probe` は最新記録ではなく被覆（`verification_coverage`）。`last_verified_seq` は最後の違反以後のgenesisから連続する `ok` の範囲、`last_verified_outcome` は違反後にgenesisから走査headまでの `ok` 検証が1件あるまで `violations`。
  - posture：`pg_read_all_data` / `pg_write_all_data` / `pg_maintain` を持つ非superuser login（`predefined_role_member`）、列権限（`column_privilege`）、0のtimeoutを違反にする。backupはowner memberかsuperuserで行う（README）。
  - 拒否の集約：間引かない。まとめた件数を `audit.access.denied` の任意field `suppressed_since_last`（catalogへ加法追加、schema再生成）で記録し、code変更時・成功時にも未記録分を先に記録する。
  - retentionの再適用：epoch後、現行revisionで `count < limit` かつeffective cutoffがpolicy floor（`tx_time - retain_days`、UTC日）の `expire` があるときだけ満たす。
  - Minor：`options[...]`・`PGOPTIONS` の拒否、`administer` の取消しを開いているinclude_control tokenへ適用、expireのcutoffを年0001–9999に限定、`audit-admin verify` は違反で終了code 3、replay・repair modeのreconciliationはingest loginから記録しない（`insufficient_capability`）。
- relay（`audit-relay`）：
  - posture：`audit_relay_owner` のmember（`owner_member`）、capability role・loginのstaging直接読取（`staging_read`、列・`pg_read_all_data` を含む）、relay表へのアクセス（`table_access`）、staging/relay表のrow security（`row_security`）、triggerの関数・event・WHEN・列の差替え（`trigger_missing`）、0のtimeoutを違反にする。`source_schema.rs` のowner memberの試験を違反の期待へ直した。
  - `preview_pending` はworkerだけに与える（operatorは42501）。
  - relay側の保留（catalog skew、projection、source）は行ごとの `relay_hold_count` で指数backoff（`backoff_min×2^(n−1)`、上限 `backoff_max`）、claimは保留していない行を先に取る、Storeの障害状態に触れず `relay_held` として報告する。
  - `reconcile --repair` は、Storeの現在epochより古いepochのreceiptだけをpendingへ戻す（SQL関数も同じfence）。同じepochで失われたreceiptは警報のまま残し、gateが後退を報告する。
  - `status()` に `max_referenced_store_seq`（epochごと：receipt、source mismatch（epochを記録するよう `note_mismatch` に引数追加）、replay・repairのcontrol event、historyのreceipt）、healthに `stored.relay_max_seq`（`--relay-max-seq` の入力）。restore試験はreplayの後にackが無い場合へ変更。
  - Minor：`delivery_history` を（control_epoch, control_seq）で一意に、reconcileのclaimも（epoch, seq）、replay・`reconcile --repair` はingestを持つStore loginを拒否、0のtimeoutをposture違反、`staging_rows_hidden` 警報（registered > staged）、既定poll 250 msと処理量の目安（README）、relay試験（機微keyは保留されStoreへ届かない、purge後のreconcile ok・`duplicate_expired` 再配送）。
- 見送ったMinor（理由）：
  - `purge_body` と有効なlegal hold：holdとprohibited content purgeの優先は方針判断（v1にholdを作る関数は無い）。依頼者の判断待ち。
  - `register_source_service` の記録へのsource追加：catalogの `source_urn` はcatalogのsourceだけを受けるが、`registered_types` はcatalogに先行するsourceをmigrationで持ち得る（試験 `idempotency_outcomes_and_conflicts` で不適合になった）。kindの判断が要る。
  - ackのepoch（probe時のepoch）：IngestReceiptにepochが無く、portの変更か配送ごとの追加照会が要る。relay停止を前提とする手順で発生しないので、READMEの限界に記載。
  - repairの計画件数（`repaired_*`）：catalogの意味の変更か2件目のcontrol eventが要る。READMEの限界に記載。
- 設計改訂3からの差分（承認状態：依頼者の修正指示（hard requirements）の範囲内で本trackが採用、修正確認review未実施）。設計本文（delivery_history、replay手順、access_reapply_pending、role行列、open_access、restore、health）へ反映済み：
  - `access_reapply_pending` は本文を返すverifyも閉じる。retentionの再適用は期限切れ本文が残らない `expire` を要する。
  - healthのverifiedは被覆（違反はgenesisからheadまでの再検証まで残る）。
  - `delivery_history` の一意性は（epoch, control seq）。`preview_pending` はworkerの行列へ。`--relay-max-seq` は参照する最大seq。
- ローカル検証：`cargo test -p audit-core -p audit-store-postgres -p audit-relay` 全PASS（core 147＋doc 2、Store 64、relay 44＋ignored 1）、clippy `--all-targets -D warnings`・`cargo fmt --all -- --check`・architecture-lint PASS。
- 次のexact action：修正確認review（security・correctness）→ 指摘反映 → 最新mainから作り直したbranchへ移してDraft PR → exact-head CI。

## 2026-10-07 — 単位B（Store・relay）の改訂core追従と確認review引継事項の実装（worktree branch、未push）

- branch `worktree-agent-a07ac0dcd80b1fa0d`（基点 `0ee9cf5`＝単位B統合branchに単位A `74795be` をmerge）。commit：
  - `dc5b8fc`：改訂audit-core API（RawReceiptRow／RawControlReceiptRow→decode、EventTypeName／BoundedCode、precheck_ingest）、catalog追補（head_seq、epoch_mismatch、count_relay_catalog_skew必須）、timelineのint8表記、relayのrelay_catalog_skew class
  - `224222b`：event type filterを登録済みtype・control typeへ限定、selectorは登録済みrelay type、db_role issuerへの付与拒否、begin_recovery_epochの期待値照合（preview／expectation_mismatch）
  - `297e18e`：audit-core（受領行・IngestRowの相互整合、ChainIntegrity、verify_identity_chain_complete、anchor位置のcheckpointとhead以降の記録の中立扱い）
  - `f8eef15`：exportの完全性検証をverify_export_completeへ移行、watermark後の失効を被覆扱いしない（complete=false、expired_after_watermark）、chain_integrityの出力
  - `17d6be4`：Store golden pinを入力行hashのkeyへ移行（digest不変）
  - `fdb6929`：IngestRowが結果列からだけ作られることの構造試験
  - `e691b85`：control event全14種・全判別値の発行とcore検証
- ローカル検証（`e691b85`）：`cargo test -p audit-core -p audit-store-postgres -p audit-relay` 全PASS（core 147＋doc 2、Store 60、relay 40＋ignored 1）、clippy `-D warnings`・fmt・architecture-lint・`cargo metadata --locked` PASS。sqlxはruntime queryのみ（offline dataは不要）。
- 設計改訂3・単位Aからの差分（承認状態：依頼者の実装指示の範囲内で本trackが採用、独立review未実施）：
  - `assess_recovery`：anchorと同じseqのcheckpointは中立（単位Aの試験は比較結果を残したまま判定を中立へ変更）、headより後の遷移の記録（`old_epoch ≥ head.epoch`）は `records_after_head` で中立（単位Aの試験はUnverifiedRecoveryを期待していた）
  - `begin_recovery_epoch` は帯域外記録の期待値（旧epoch、復元head seq・chain、消失上限）を必須にした（SQL署名・CLI・`AuditAdmin` APIの変更）
  - reconcileの `relay_catalog_skew` をpendingと排他のclassにした（`audit_relay.delivery_view` に `last_error_code` を追加）
  - export中のexpire競合は、snapshotではなく「Wより後の証拠を被覆しない」方式（complete=false）で扱う
- 次のexact action：独立review（security・correctness）→ 指摘反映 → 最新mainから作り直したbranchへ移してDraft PR → exact-head CI。

## 2026-10-07 — 単位A（event契約）の実装・review・exact-head CI

- 設計は改訂3（`8254d76`）で確定した。独立reviewは3回行った。
  - review 1：Critical 3件
  - 再review：Critical 1件
  - 最終review（設計2観点＋code 3観点）：Critical 1件（portが改訂1の三分類のまま）

  全件を反証付き検証の後に反映した。主な変更：
  - 閲覧intentのcommit判定は `pg_xact_status`＋flush待ちとする。
  - 主体はsession_userへ束縛し、role・credential行列を定める。
  - restore時の権限posture、recovery mode、帯域外のepoch記録。
  - 二分類の失敗model：Storeの構造化verdict以外は、すべて外部障害として試行を返却する。
  - rebindは廃止し、`planned_move` epochで扱う。
  - reconciler role。
  - ingestはsource service主体に限定する。
  - 失効証拠のDB外検証。
- 単位Aのcommit：
  - `7578dfc`：audit-core、catalog、生成schema
  - `cb22f89`：control 14種、DB外の復旧判定
  - `749c93d`：最終reviewの修正（二分類のport、golden投影pin、adapter定義、nil client ID、principal文字種、jsonb相当長、export行のexpired_by_seqと失効・epoch・originの検証）
  - `f369261`：最新main（Organization U1〜U4、local workspace runtime、Folderアクセス設定）を統合。active.mdのconflictは双方の節を保持して解消
- ローカル検証（`f369261`）：
  - `cargo test -p audit-core`：118件PASS（lib 28、catalog 13、chain/export 38、envelope 12、golden 2、legacy 15、schema 7、store_port 3）
  - clippy `-D warnings`、fmt、architecture-lint：PASS
  - `cargo metadata --locked`：PASS
- hosted CI：PR98のexact-head `f369261` で、CI run 37619375294がrequired-checkを含む全項目SUCCESS（Organization D2系はskip）。
- 計画からの逸脱（承認状態）：
  - control typeは12→14種：`access.closed`、`retention.expire_refused`。checkpointは `integrity.verified` の trigger=checkpoint で表す。fingerprint_reboundは改訂3で廃止。設計§4.5を正本の要約として更新済み。計画単位A手順2も14種へ修正済み。
    - 承認状態：依頼者の実装指示の範囲内で本trackが採用、設計改訂3・独立reviewで確認済み（依頼者による個別承認ではない）。
  - control eventのlist kind（`event_type_list`、`source_list`）の上限は16（32 KiB上限と設計§10.3のfilter event_types≤16に合わせる）。retention selectorも16件までになる（設計§9とREADME §kindに記載）。当初の自由文字列 `identifier_list` は、独立検証の指摘S2により閉じたkindへ置換した。
    - 承認状態：依頼者の実装指示の範囲内で本trackが採用、設計改訂3・独立reviewで確認済み（依頼者による個別承認ではない）。
  - golden pin：計画単位A手順7に追記（entryを `<fixture>@<入力行hash>` に変更、旧sectionはdigestで凍結）。現在のsection 1はkey形式だけを移行し、全31件のdigestは不変。
    - 承認状態：依頼者の実装指示の範囲内で本trackが採用、設計改訂3・独立reviewで確認済み（依頼者による個別承認ではない）。
  - DB外判定の厳格化（独立検証の指摘S1・S3への修正で追加。設計§8:457より厳しい）：`unverified_expiry_evidence > 0` または認証範囲外の失効証拠があるreportは `Authentic` にしない（`UnverifiedExpiry`）。headより前のcheckpointだけでは `AuthenticThrough { seq }`。
    - 承認状態：依頼者の修正指示の範囲内で本trackが採用。設計本文（§8）へ反映済み。修正後の確認は単位Aの最終確認reviewで行う。
  - 束縛主体の無いcontrol event（unboundの拒否、bootstrap）のactorを `{issuer: "db_role", principal_id: session_user}` と定めた（設計§10.2とREADME §control eventに記載）。
    - 承認状態：依頼者の修正指示の範囲内で本trackが採用。設計本文へ反映済み。修正後の確認は単位Aの最終確認reviewで行う。
- 単位Aの修正確認review（`fa197c0` 対象、security・correctnessの2観点）：両観点ともGO（Critical/Importantなし）。Minorの扱い：
  - 文書が強すぎる主張をしていた2件（自由記述kindの有無、terminal verdictの型保証）は、単位Aで訂正した。
  - 次の件は単位B・後続へ引継ぐ：
    - open_accessのevent type filterを登録済み・control typeへ限定する
    - export中のexpireとの競合（watermark後の失効証拠）
    - 受領行decodeの相互整合
    - begin_recovery_epochの期待値（restored head、lost upper）の照合
    - identity chain検証の判定区分
    - anchor時点のcheckpointとhead以降の記録を中立に扱う
    - golden pinを入力変更とともに再投影する
    - IngestRowを結果列からだけ作ることの試験
- 単位B（Store・relay）：
  - Store crate（39件）とrelay crate（36件）は別worktreeで実装済み。
  - 改訂3と新しいcore APIへの追従は、別worktreeで実施中（未push）。
- 次のexact action：
  1. 単位Aのfix確認reviewを完了する。
  2. PR98の本文を更新し、mainへmergeする。
  3. main CIを確認する。
  4. branchを最新mainから作り直し、単位BのDraft PRを作る。

## 2026-10-07 — 最新main基点で再開、設計改訂1を再review中

- 基点はmain `d515aa38085c9ed7e41f8103d9c1a6c576025fd4`（push CI 37562024089 SUCCESS）、branchは `claude/cool-darwin-7xh893`、Draft [PR98](https://github.com/AIrisu-072/knowledge-platform/pull/98)。旧Draft PR44（設計）・PR45（schema）は基点が30 merge古く、方針（reasonを持つeventを配送しない）が今回の要求と衝突するため、stackとしては使わない。PR45のcrateは現mainで24/24 PASSした（調査時の一時worktreeで確認）。重複key parser・catalog形式の考え方だけを流用する。PR44/45は、置換PRが統合可能になった時点で理由を付けてcloseする。
- 調査範囲：7並列reader＋critic。producer 21種（INSERT 13箇所）、staging DDL・権限、汎用 `outbox-delivery`、規範要求、PR45、runtime/CI、Organization/Searchの境界。主な事実：
  - 配送・Store・integrity・retention・閲覧監査はいずれも欠落している。
  - 自由記述 `reason` を持つeventが7種あり、withdraw/endの理由文には上限が無い。
  - 通常のACL変更はreasonを持たない。
  - `authorization.denied` の試験は0件。
  - W3C trace_idは保存されていない。
  - Searchは別のaudit outboxを持ち、未配送である。
  - Workの `event_staging` にはconsumerが無い。
- 設計：[配送・保存・検証設計](../specs/2026-10-07-audit-infrastructure-v1-delivery-design.md) 改訂1、[決定記録](../../decisions/2026-10-07-audit-envelope-store-integrity.md)、[実装計画](../plans/2026-10-07-audit-infrastructure-v1-delivery.md)。
- 独立review 1（security / correctness / spec、指摘ごとに反証を試みる検証付き）の結果：
  - Critical 3件：閲覧記録がrollbackで消える、`retain_days=NULL` で本文が削除される、主体を引数で詐称できる。
  - Important 20件超：search_path/PUBLIC EXECUTE、role未定義、ingestの偽装、salt無しdigest、genesis、再投影の偽conflict、障害時の試行消費、replayの試行予算、restore gate、retentionの偽装、correlation写像ほか。
  - 反証された指摘は0件。全件を改訂1へ反映した。再reviewは実行中。
- 検証：設計文書のみ。PR98の旧head `5ad05d5` はhosted CI全項目SUCCESS（設計候補のみで、実装の資格ではない）。
- 環境：Docker daemonを起動した。Docker Hubが429（rate limit）を返したため、同一imageを公式mirror `mirror.gcr.io/library/postgres:18.6-bookworm` から取得し、`postgres:18.6-bookworm` としてtagした（digest `sha256:afc7e2d4…`）。repositoryにAudit固有の安全停止の記録は無い。既存のSearch G07 socket停止等は、他担当の事項として変更しない。
- 次のexact action：再reviewの指摘を反映して設計を確定する → `crates/audit-core` と `spec/telemetry` をTDDで実装する（単位A）→ 独立review → PR98 exact-head CI。
