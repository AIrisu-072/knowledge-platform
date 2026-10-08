# Audit Infrastructure v1：実行状況

## 2026-10-08 — 単位B 文書の確定（運用手順・設計改訂4・計画）（worktree branch、未push）

- branch `worktree-agent-a07ac0dcd80b1fa0d`（`a4a3f6f` の上、最新main `6a34de3` を取り込み済み）。commit：`79af4fa` 運用手順 [audit-delivery-store.md](../../operations/audit-delivery-store.md) を新規作成、`9620adb` 設計を改訂4へ更新、本commitで計画・状況・active pointerを更新。codeは `2955a8e` から変えていない。
- 単位Bの内容：
  - Store（`crates/audit-store-postgres`）：別DB・`audit_store` schema・専用ledger、idempotentなingestと構造化verdict、2段階開示とaudit-of-audit、verify・checkpoint・検証被覆、retention・purge・epoch後の再適用、権限・束縛・source service、posture、recovery mode・epoch（帯域外期待値とpreview）、拒否の集約、bin `audit-admin`（DB外の総合判定 `assess` を含む）。
  - relay（`crates/audit-relay`）：Document DBの `audit_relay`（登録trigger、guard、`BEGIN ATOMIC` digest、definer関数）、`outbox-delivery` runnerへのadapter、circuit breaker・gate、relay側保留のbackoff、reconcile・replay・repair、health（circuit・警報）、進捗行、bin `audit-relay`。
  - audit-coreへの加法変更と、catalogへのVIEW/RESET 2 typeの加法登録（既存typeは不変）。
- review：
  1. 独立review（Store・relay × security・correctness、`46ed5f6`）：指摘を修正前に失敗する再現試験とともに反映（`7c02d58` まで）。Minor 4件を見送り（下記）。
  2. 確認review round1（`478a66f`）：NO-GO。Important I1–I4、Minor M1–M3を全件反映（`2a8b62f`〜`f97a909`）。
  3. 確認review round2：Minor m1–m5を全件反映（`3fb4071`〜`2955a8e`）。m1–m5の修正後の再reviewは未実施。
- 見送り（承認状態：awaiting requester decision）：purgeとlegal holdの優先、`register_source_service` の記録へのsource fieldの追加（kindの判断）、ackのepochをreceiptで持つこと、repair modeの記録の計画件数（実適用件数はCLI出力だけ）。
- 設計からの差分：設計の改訂4（10項目、各箇所に「（改訂4）」）。承認状態：依頼者の指示（hard requirements）の範囲内で本trackが採用、単位Bのreviewで確認（依頼者による個別承認ではない）。
- 検証（`a4a3f6f`、codeは `2955a8e` と同一）：`cargo test --locked --no-fail-fast -p audit-core -p audit-store-postgres -p audit-relay` exit 0（275 passed、1 ignored：core 152＋doc 2、Store 71、relay 50＋ignored 1）。docs commit後（`9620adb` と本commitの作業tree）：`cargo fmt --all -- --check`、`cargo clippy --locked -p audit-core -p audit-store-postgres -p audit-relay --all-targets -- -D warnings`、`cargo metadata --locked`、`cargo run --quiet --locked -p architecture-lint -- check`（architecture: pass）、`mise.toml` の `repo:policy` と同じ検査 PASS、追加した相対linkの存在確認（欠落0）、運用手順・設計の識別子（環境変数・command・関数・code）をsourceと照合（placeholder以外の欠落なし）。本番credential・migration・deployは行っていない。
- 次のexact action：このbranchを `claude/cool-darwin-7xh893`（現在 `643cc85`、HEADの祖先）へpushし、単位BのDraft PRを作る → exact-head hosted CI → main統合 → main CI確認 → 単位C（Document producer E2E受入・handoff）。

## 2026-10-08 — 単位B 確認review round2のMinor指摘（m1–m5）の反映（worktree branch、未push）

- branch `worktree-agent-a07ac0dcd80b1fa0d`（`d2380ca` の上）。各指摘を試験とともに1 commitずつ：
  - `3fb4071` m1：relay postureは `audit_relay` の表の列の権限（ownerでない全role・PUBLIC。relaclにもhas_table_privilegeにも現れずcommitment saltを読める）を `column_privilege`（object `表.列 role`）として報告する（`source_schema.rs`：operator loginへの `commitment_salt` のSELECT、PUBLICへの `store_seq` のUPDATE）。
  - `d66a04d` m4：relay postureの `table_access` は、能力を持たないloginについてはDocument DBへのCONNECTがある場合だけ報告する（`predefined_role_member` と同じ条件。能力role・そのloginとserver file・programのroleは接続に関係なく報告）。`source_schema.rs` でCONNECTの無い `pg_write_all_data` loginが報告されず（修正前は `table_access` が出ることを確認）、server fileのroleは報告され、CONNECTを与えると報告されることを確かめる。
  - `a5bfb8b` m2：方針は「報告する」。Store・relayの両postureで、REPLICATION属性を持つ非superuserのloginを接続に関係なく `replication_login` として報告する（server fileのroleと同じ）。両READMEに、REPLICATION loginは境界の外にあり、superuserのloginで専用のreplication基盤（`pg_hba.conf` の `replication` 行をreplica・backup hostに限る）に限ることを記載（`store_access.rs`：新規loginと既存loginへの属性付与、`source_schema.rs`：CONNECTの無いlogin）。
  - `4338fb2` m3：assessの `store_behind` は、manifestがexportを切ったこと（最初のintentの `seq_through`、または最初のintentのwatermarkがcheckpointのseq以上）を示す場合だけに適用し、切っていないexportのheadを越えるcheckpoint（古いexportか、Storeが行を失った）はaudit-coreの判定（同じepochでは `tampered`、終了code 5）のままにする。出力に常にaudit-coreの判定 `underlying_verdict`（`broken` ではnull）を出す（`cli_assess.rs`：`--seq-through` exportは `store_behind`／`underlying_verdict` `tampered`／終了code 2、切っていないidentity chain exportは `tampered`／5。単体試験でwatermarkによる切断も確認）。README・telemetry仕様・CLIのhelpを更新。
  - `2955a8e` m5：`purge_body` が証拠の前に保留中の全拒否連続を書く試験（`store_ingest.rs`：2つのloginのburst後に `purge_body(event_id, 'adapter_defect')`、`denials_pending` 0、各loginの件数がchainで全件、各flushのseqがpurgeの証拠より前）。
- ローカル検証（`2955a8e`）：`cargo metadata --locked` PASS、`cargo test -p audit-core -p audit-store-postgres -p audit-relay` 全PASS（core 152＋doc 2、Store 71、relay 50＋ignored 1。試験は既存関数内への追加のため件数は不変）、`cargo clippy --locked -p audit-core -p audit-store-postgres -p audit-relay --all-targets -- -D warnings`・`cargo fmt --all -- --check`・`cargo run --quiet --locked -p architecture-lint -- check` PASS。
- 設計からの差分（承認状態：依頼者の修正指示の範囲内で本trackが採用、確認review未実施）：REPLICATION loginの報告（replicationはsuperuserで行う）、assessの `underlying_verdict`（加法）と `store_behind` の適用条件の縮小。migration 0001（Store・relay、未release）はその場で編集した。
- 次のexact action：修正確認review（security・correctness、m1–m5を含む）→ 指摘反映 → 最新mainから作り直したbranchへ移してDraft PR → exact-head CI。

## 2026-10-08 — 文書詳細表示・未読戻しのevent type 2種をcatalogへ加法登録（worktree branch、未push）

- 対象：main PR #106（migration 0012、`current_read_state.rs`）が必須Auditとして書く `document.version.detail_viewed`（VIEW）と `document.version.marked_unread`（RESET）。既存typeは変えていない（rename・意味変更なし）。commit `5e06e15`（catalog・audit-core・Store登録）と、その後のrelay試験・docsのcommit。
- catalog：両typeとも `DATA_ACCESS`、resource `Document`、version必須（client指定version）、result `success`、subject `document/{resource.id}`、reason `absent`。detailsはproducerの5 key（`document_version_id` uuid・client_chosen、`operation_id` uuid、`expected_read_state_revision`、`resulting_read_state_revision`、`trigger`）が必須で、VIEWだけ `first_record`（boolean、必須）。triggerはtypeごとの1値enum（`detail_display` / `user_reset`）。`operation_id` を `correlation.operation_id` へ写し、`document_version_id` を `resource.version_id` へ束縛する。
- 新kind（加法）：`safe_counter`（0..=2^53−1）・`positive_safe_counter`（1..=2^53−1）。0012のCHECK（expected 0..9007199254740991、resulting 1..9007199254740991）に合わせた。schemaは `AUDIT_SCHEMA_BLESS=1` で再生成（追加のみ、削除行0）。
- adapter_versionの判断：既存fixture 31件のdigestは不変（Rust・Storeの両golden）。新typeの行は追加前のcatalogでは投影されず `relay_catalog_skew` で保留されるだけで、その版の保存済み出力が無い。よって `adapter_version` は1のまま、section 1へ新fixture 3件（`detail_viewed/first_record`、`detail_viewed/after_reset`、`marked_unread`）のentryを追記した。規則は `spec/telemetry/README.md`（adapter_versionの規律）に記載。
- Store：`registered_types`（migration 0001、未releaseのためその場で編集）へ2行追加。件数の試験は21→23（`store_ingest.rs`、`store_port.rs`）。配備順はStoreの登録が先（relayだけ先に更新するとprobeが `store_unregistered_type` でrelay全体を止める）。
- 試験：投影の受理（3形、最大revision 2^53−1）と拒否（未知key、型・範囲外revision、他typeのtrigger、RESETの `first_record`、VIEWの `first_record` 欠落、nil／束縛違反のversion、nil・大文字のoperation_id、subject・result・resource・reason列）を `legacy_projection.rs`、catalogの形をproducerと照合する試験を `catalog_contract.rs`、schema関係の拒否5件を `common::envelope_rejections` に追加。relayの `every_producer_shape_reaches_the_store_without_reason_text` は、producerと同じ形の3行（初回VIEW、RESET、RESET後のVIEW）を加えて9行すべてが `stored`、本文のdetailsがstagingの `data` と一致、correlationがoperation_id、healthの警報なし・forecast空を確認する。
- ローカル検証：`cargo metadata --locked` PASS、`cargo test --locked --no-fail-fast -p audit-core -p audit-store-postgres -p audit-relay` 全PASS（core 152＋doc 2〔unit 34、catalog 22、chain 49、envelope 14、golden 5、legacy 16、schema 7、port 5〕、Store 71、relay 50＋ignored 1）、`cargo clippy --locked … --all-targets -- -D warnings`・`cargo fmt --all -- --check`・`cargo run --quiet --locked -p architecture-lint -- check` PASS。
- 設計からの差分（承認状態：依頼者の指示の範囲内で本trackが採用、確認review未実施）：新kind 2種、型追加ではadapter_versionを上げない規則の明文化、設計§2・§4.3の件数注記とkind表。
- 次のexact action：修正確認review（security・correctness、今回のcatalog追加を含む）→ 指摘反映 → 最新mainから作り直したbranchへ移してDraft PR → exact-head CI。

## 2026-10-08 — 最新main（`6a34de3`）を単位Bへ取り込み（worktree branch、未push）

- `git merge origin/main`（merge commit `a538215`、rebaseなし）。競合なし。`active.md` はAudit節と文書・Desktop側の新しい節を両方保持。`Cargo.toml`・`Cargo.lock` はmain側に変更なし、`dependency-rules.toml` はmainの `desktop_shell` 境界の追加のみ。単位Bはmainに対しDocument・Search・Organization・work・outbox-delivery・appsを変更していない。
- ローカル検証（merge後）：`cargo metadata --locked` PASS、`cargo test --locked --no-fail-fast -p audit-core -p audit-store-postgres -p audit-relay` 全PASS（core 148＋doc 2、Store 71、relay 50＋ignored 1。merge前と同数）、clippy `-D warnings --all-targets`・`cargo fmt --all -- --check`・`architecture-lint check` PASS。relayの `source_schema`（preflight・backfill・digest等9件）はDocument migration 0012まで適用したDBで通る。
- D4確認：mainの新しいmigrationは `0012_document_current_read_state.sql` だけで、`document_read_states` への列追加と `document_read_state_operations` の新設のみ。`public.audit_outbox_events` の列・制約・triggerには触れない（Auditのreview対象外）。新producer（`current_read_state.rs`）は既存列だけをINSERTする（`trace_id`・`reason` なし）。
- 新event type `document.version.detail_viewed`・`document.version.marked_unread` はcatalogに未登録。producerと同じ形の行を一時試験（未commit）で流すと、relayは両方を `relay_catalog_skew` でhold（quarantine・配送・保存なし、attempt 0、breaker Closed）、healthは `catalog_skew_held`・`relay_held` の警報とforecast `{"relay_catalog_skew": 2}`。
- 次のexact action：2つの新event typeをcatalog（加法）へ登録し、skewが解消して保存・検証されることを試験する。その後、修正確認review → Draft PR → exact-head CI。

## 2026-10-08 — 単位B 確認review（NO-GO、`478a66f`）の指摘反映（worktree branch、未push）

- branch `worktree-agent-a07ac0dcd80b1fa0d`（`478a66f` の上）。各指摘は先に再現試験を書き、修正前に失敗することを確認してから直した（括弧内は修正前の失敗）。commit：
  - `2a8b62f` I4：新DBへのrestoreでcheckpoint・relay seq・regressionが無いと `lost_upper_known` がNULL（previewが `missing value`、epoch_startedがcatalog違反）→ coalesceでfalse（`store_recovery.rs` の新試験、previewで `Protocol("missing value")`）。
  - `63202ac` I3：recovery記録の行は上限不明を `"lost_upper":null`（必須key）で示し、開始は `--expect-lost-upper unknown`（SQLは期待値NULL＝不明）でだけ一致する。audit-coreは `RecoveryRecord`・`EpochAttestation` に `lost_upper_known` を加え（加法）、本文のboolean必須、不明な上限は `Lost`（記録と本文の既知・不明が食い違えば不一致）。assessの `epochs` にも出す（audit-coreの試験で `Authentic`、`cli_assess` で行が数値のまま）。
  - `c24f2b5` I1：集約windowを過ぎた全loginの未記録の拒否件数を、すべての追記（control event、relay ingest）とprobeの前にそのlogin・code・actor（記録時に保持）の記録として書く。verify・checkpoint・開示intent・expire・purgeは記録の前に全件書く。`store_status.denials_pending`、relay healthの `stored.denials_pending` と警報 `store_denials_pending`（`store_ingest.rs`：7件のburst後に無関係な追記で1件しかchainに無い）。
  - `74e30bb` I2・M1：relay postureは `audit_relay_owner` とsuperuser以外の全loginのrelay表への書込み（`table_access`）、接続できるloginの `pg_read_all_data`/`pg_write_all_data`/`pg_maintain`、接続に関係なく全loginのserver file・programのrole（`predefined_role_member`）を報告する（Storeも後者を追加）。relay表5つに `current_user` で書き手を確かめるSECURITY INVOKERの `guard_writer` を追加し、定義者関数以外の受領偽造を42501で拒否する（`source_schema.rs`：偽造loginでpostureが空、`store_access.rs`：非接続loginのserver file roleが未報告）。
  - `bd20cd0` 追加修正：`read_page` の `await_durable` は、後続commitの無いWAL（heap pruning等）が残るとidleなStoreで `intent_not_durable` を返し続けた（M2の試験で再現）。遅れていれば内容の無い非transactional WAL message（flush付き）でflushしてから開示する。
  - `fbf08da` M2：同じ以後のepochでcheckpointがexportのheadを越える場合は `tampered` ではなく `store_behind`（終了code 2、真正にしない）。checkpointはexportの最後のseq以前を使う（`cli_assess.rs`：`--seq-through` exportが `tampered`/5）。
  - `f97a909` M3：probe・store_statusは `record_verified` がhead lockの下で更新する `verification_state` を読む（意味は同じ関数で不変。関数統計でprobe 5回に再計算5回）。
- ローカル検証（`f97a909`＋docs）：`cargo test --locked --no-fail-fast -p audit-core -p audit-store-postgres -p audit-relay` 全PASS（core 148＋doc 2、Store 71、relay 50＋ignored 1）、`cargo clippy --locked … --all-targets -- -D warnings`・`cargo fmt --all -- --check`・`cargo run --quiet --locked -p architecture-lint -- check` PASS。
- 設計からの差分（承認状態：依頼者の修正指示の範囲内で本trackが採用、確認review未実施）：recovery記録形式の `lost_upper: null`、assessの `store_behind`（終了code 2）、Document DBのbackupはsuperuserで行う（relay postureが `pg_read_all_data` のloginを報告するため）、`read_page` の自己flush。
- 次のexact action：修正確認review（security・correctness）→ 指摘反映 → 最新mainから作り直したbranchへ移してDraft PR → exact-head CI。

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
