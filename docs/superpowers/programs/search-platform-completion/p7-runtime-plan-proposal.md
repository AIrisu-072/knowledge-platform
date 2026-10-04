# P7 Production Runtime — bounded implementation plan 提案

Status: **PROPOSAL / 独立設計 review 前 / 実装着手前**（2026-09-30）。[設計提案](p7-runtime-design-completion.md) と [P7 shared freeze](p7-shared-durable-freeze.md) を入力とする。review 指摘を修正し runtime freeze を固定してから code task を dispatch する。`spec/` が規範正本。Graph/Vector/backend、SLO、配備先の未決定値をこの計画で既定化しない。

## 1. 依存の切り方と sole writer

P7-01〜12 は [共有基盤 plan](p7-shared-durable-plan.md) の既存 task **alias** であり、再実装しない。`P7-Rxx` は runtime/deployment/qualification の後続 task 名とする。P5-08 の四 route factory、P5-07 の socket 送出、P6-I04 の worker wiring、P6-S06/S07 の delivery 縦断、P7-12 の低層 restore も alias として消費する。最終 P1/P2/P3/P4/P5/P6 receipt を `P7-R01` の入力に置かず、accepted **code slice/independent local review** を順次消費して最後に全 receipt を fan-in する。`P5-08 → P7-R02`、`P6-I04 → P7-R02`、`P7-01〜11 → P7-R02`、`P7-R02 → P7-R05/12 qualification`、`P5/P6 final receipt → P7-R09` の向きにし、P5/P6 最終 receipt → P7 設計 → P5/P6 実装という cycle を作らない。現行 `task-graph.yaml` の `p7-design` が final receipt を要求する箇所は、この proposal/design review と最終 receipt 照合の二 node に分ける提案である（本 worker は graph を編集しない）。

| shared file/owner | 直列化 |
| --- | --- |
| `crates/search-runtime/src/lib.rs` と runtime crate manifest | P7-01〜12、P6-I04、P5-08、P7-R01〜06 が同時編集しない。各 task の公開 module 登録は単一 integration writer window。 |
| root `Cargo.toml` / `Cargo.lock`、`Dockerfile`、`mise.toml` | parent が予約した一 writer window。runtime 用 dependency は既存 pin を優先し、追加前に公式 API/ライセンス/資格を確認。OCI/mise は P7-R06 だけ。 |
| `crates/search-application/src/{ports,lib}.rs` | P1/P2/P4/P5/P6/P7 shared の受理済み変更後に integration writer が一度だけ合わせる。P7-R は新 actor mint/Source ledger を足さない。 |
| `crates/search-api-http`、`crates/outbox-delivery`、`crates/search-source-document` | 各 P5/P6/P1 owner が実装。P7-R は追加 handler/ack/parser/Source 正本を編集せず、公開 factory/port を消費する。 |
| `spec/`、`task-graph.yaml`、run journal/status | normative writer と parent の専用責務。本 plan の worker は触れない。 |

DB grant は P7-03 `sql/roles.sql`、P6-I04 の generic/Search role split に従う。runtime は migration role、registration、builder、coordinator、reader、GC、generic delivery の実接続を startup で別々に検査する。Search coordinator に generic `delivered_at` UPDATE を与えず、generic role に Source pointer/receipt UPDATE を与えない。P7-Rxx は新 grant を独自決定しない。必要な grant 差分は P7-03/P6-I04 の sole writer に返し、実 PostgreSQL role test と独立 review 後だけ採る。

**共通 task protocol:** 指定する focused test の安全条件を先に assertion にし、未配線または既存実装で期待どおり RED を記録する。最小 GREEN と同じ test、対象 crate strict Clippy、fmt を実行し、real PostgreSQL 18.6/別接続・local TCP・実 filesystem・必要な別 OS process を使う。fake-only GREEN は qualification ではない。各 task の branch/head、入力 freeze hash、test command/result、role、fixture hash、未接続 gate を receipt に残し、独立 read-only reviewer がコードと失敗時の閉じ方を確認する。1 task ごとに full CI を回さず、統合後に focused `mise run verify:fast` と最終 exact-head hosted gateを行う。

## 2. Tasks

### P7-R01 — typed config と secret reference

**Files:** `crates/search-runtime/src/{config,secrets}.rs`、`crates/search-runtime/tests/config_contract.rs`。`lib.rs` 登録は予約 window。**Consumes:** P4/P5/P6/P7 frozen config/limit/retention、P7-02 complete desired type、host injection contract。**Produces:** validated `RuntimeConfigSnapshot`、redacted `SecretRef`/resolver port、startup validation receipt。Graph/Vector 設定は選定 receipt と一致する enum だけを許す。

**RED→GREEN:** unknown/duplicate field、env override の禁止 key、relative/cross-device index root、`client <= operation <= dependency`、P6 policy 外 lease、欠落/異 tenant desired namespace、Debug/serialization/error に secret 値混入、未解決必須 secret の起動を拒否する named tests `config_rejects_unbounded_or_cross_device_paths` / `secret_value_never_enters_config_or_diagnostics`。`cargo test --locked -p search-runtime --test config_contract` RED→GREEN。**Real qualification/review:** 実一時 directory/mount と mock *reference resolver* を使い、値の保持・log capture を監査する。secret 実値・外部 credential は読まない。独立 reviewer は precedence/privilege/NO_RETENTION policy を確認。

### P7-R02 — 一つの production factory と component graph

**Files:** `crates/search-runtime/src/{composition,service}.rs`、`crates/search-runtime/tests/runtime_wiring.rs`、`lib.rs` は sole integration window。**Consumes:** P7-01〜11 accepted code/review、P1 real Document/Unit/lexical、P3 採択済み Graph、P4 Remote TCP/retention、P5-01〜08 accepted四 route factory、P6 generic+Search bridge と I04 worker wiring、P2 mode decision。**Produces:** `ProductionRuntimeFactory`/`SearchRuntime`、一つの host-owned Source registry/identity/pin/current chain、同一 process API+worker start path。P5/P6 factory を複製しない。

**RED→GREEN:** `production_factory_rejects_memory_or_missing_trusted_port`、`document_and_remote_share_one_catalog_and_current_gate`、`disabled_vector_keeps_neutral_core_and_required_routes`、採択時の wrong vector bundle refusal を先に作る。`cargo test --locked -p search-runtime --test runtime_wiring` RED→GREEN。**Real qualification/review:** Domain+Search+P7+Graph migrations の disposable PG、実 Document FS、P1 extractor、local TCP Remote、P5 四 route、P6 event→receipt→generic ack を同時に起動する。各 component が実 adapter でなければ未合格。独立 reviewer は第二 actor/Source mint、test fake、PG/file READY 省略、event ack owner を確認。

### P7-R03 — startup admission、health、graceful shutdown

**Files:** `crates/search-runtime/src/{startup,health,shutdown}.rs`、`crates/search-runtime/src/bin/search_service.rs`、`crates/search-runtime/tests/health_lifecycle.rs`。P6-I04 bin は消費し二重 worker にしない。**Consumes:** R01/R02、P7-12 recovery verifier の公開 port、P5 Problem/partial、P6 drain。**Produces:** liveness/readiness/管理用 Source diagnostics、ordered admission、bounded SIGTERM path。

**RED→GREEN:** `registry_or_role_mismatch_blocks_listen_and_claim`、`one_remote_outage_keeps_safe_local_route_ready`、`missing_current_index_fails_source_without_false_partial`、`sigterm_stops_new_claim_and_leaves_unknown_unacked` を RED→GREEN。`cargo test --locked -p search-runtime --test health_lifecycle`。**Real qualification/review:** 実 PG、local TCP 断、file rename/corrupt、独立 process SIGTERM/kill、別 role 接続で public health の hidden Source 非開示と管理面分離を測る。独立 reviewer は全体 unready と Source degraded の混同、readiness freshness 誤認、lease drain を確認。

### P7-R04 — safe observability と Audit 接続

**Files:** `crates/search-runtime/src/{observability,audit}.rs`、`crates/search-runtime/tests/observability_privacy.rs`。P5/P6/P1 の既存 instrumentation port を接続し、別 pipeline を増やさない。**Consumes:** R02/R03、P4/P5 retention/disclosure、P6-G08、`spec/operations/observability-audit-requirements-v0.md`。**Produces:** stage trace/low-cardinality metrics、Audit transactional/配送 failure path、operator dashboard query 契約。

**RED→GREEN:** `collector_loss_does_not_ack_or_break_business_commit`、`required_audit_creation_failure_rolls_back_its_transaction`、`hidden_source_and_no_retention_payload_absent_from_all_sinks` を先に RED、`cargo test --locked -p search-runtime --test observability_privacy` GREEN。**Real qualification/review:** local OTLP receiver/停止、Audit outbox store、実 HTTP/Remote provider success/error/disconnect、captured logs/trace/metric/audit/temp files を検査する。metric label の ID、高 card、Query/body/secret の漏出と Audit/OTel lifetime を独立 reviewer が確認。

### P7-R05 — app-level rebuild/restart/backup 運用

**Files:** `crates/search-runtime/src/rebuild.rs`、`crates/search-runtime/src/bin/search_admin.rs`、`crates/search-runtime/tests/runtime_recovery.rs`、`docs/operations/search-runtime-runbook.md`（新規）。P7-12 の低層 `recovery.rs` と `process_restore.rs` は変更しない。**Consumes:** R02/R03、P7-12、P1-B03、P3-G08、P6-S07 の accepted evidence。**Produces:** Source 正本から full index/Graph rebuild を呼ぶ operator flow、API/worker 再入条件、別 DB/別 index root restore runbook。

**RED→GREEN:** `full_rebuild_uses_new_key_and_preserves_old_pin`、`db_only_restore_remains_unready_until_index_restored`、`provider_outage_and_unknown_ack_replay_do_not_infer_absence` を RED→GREEN。`cargo test --locked -p search-runtime --test runtime_recovery`。**Real qualification/review:** 実 current Version/Part/raw を含む合成 Document、PostgreSQL dump→別 disposable DB restore、index root 複製/欠落、Graph rebuild、process kill/restart、outbox再配送を通す。旧 key/receipt/guard/actor authority と結果一致を確認。独立 reviewer は manual rebuild の ack 禁止、破損の隔離、partial data loss を確認。

### P7-R06 — deployment-ready local artifact

**Files:** `Dockerfile` の Search target、`mise.toml` の focused Search image task、`deploy/search-runtime/{config.example.toml,README.md}`、`crates/search-runtime/tests/container_smoke.rs`。root Cargo/lock と同時編集せず sole packaging writer window。**Consumes:** R01〜05、P1 Linux extraction worker/pin、P5 API、P6 worker、選定 Graph/Vector binaries、role/schema/runbook。**Produces:** non-root Linux OCI/binary、immutable build manifest/digest/SBOM、config/secret *reference* template、disposable local smoke command。live target の選定やデプロイは含まない。

**RED→GREEN:** `image_has_real_service_worker_and_parser_assets`、`container_rejects_missing_secret_ref_or_schema_and_drains_signal` を先に RED、`mise run container:search-smoke` と同名 test で GREEN。**Real qualification/review:** build した image を disposable PG/local TCP、bind mount/data/index、実 role、健康 probe、SIGTERM で実起動し四 route と outbox、restart 後 query を確認。image digest、SBOM、binary/asset hash を保存。独立 reviewer は既存 `Dockerfile` bootstrap target の回帰、root 権限、秘密値混入、host 特有 path、未資格依存を確認。

### P7-R07 — capacity envelope と負荷試験

**Files:** `experiments/search-runtime-capacity/{README.md,run.py,analyze.py}`、`docs/superpowers/programs/search-platform-completion/capacity.md`（graph 既存 artifact）。production source は測定時に編集しない。**Consumes:** R02〜06 local artifact、P1/P2/P3/P4/P6 quality・選定 receipt、実行前 host disk/RAM  inventory。**Produces:** reproducible synthetic/public corpus manifest、small/medium/large envelope、raw run metadata と集計。固定件数は事前に埋めない。

**RED→GREEN:** harness の計測列欠落・seed/fixture/build hash 相違・実行中 capacity 下限で fail する `python3 -m unittest discover -s experiments/search-runtime-capacity/tests` を先に RED→GREEN。**Real qualification/review:** 実 OCI/process と実 PG/FS/Graph/Remote TCP に対し、規模・同時数・warm/cold・故障を宣言して複数反復する。index/extraction/outbox throughput、Search/Discover p50/p95/p99、Graph、Remote fanout、memory/disk、rebuild/restart、選定時 Vector、該当時 tokens と stage別 quality を測る。reviewer は synthetic/実業務の外挿、sampling、cache、host swap/容量、安全停止、推定値の混入を監査。これは SLO 保証ではない。

### P7-R08 — measured operational SLO proposal と runbook 閾値

**Files:** `docs/superpowers/programs/search-platform-completion/p7-operational-slo-proposal.md`、R05 runbook の alert 表（同 writer window）。**Consumes:** R07 capacity、P1/P3/P4/P6 fault/quality distributions、想定業務 traffic/concurrency/重要 Source。**Produces:** SLI の分母・除外条件・計測窓、`proposed operational SLO`、proposed alert と容量増設/再測定 trigger。**RED→GREEN:** traceable input がない percentile/threshold、benchmark 観測と保証値を一欄にした行を検出する document assertion を先に失敗させ、測定 receipt へリンクした表だけを受理する。**Real qualification/review:** synthetic envelope と故障データから提案値を計算し、業務前提未確定なら「提案未確定・必要な追加測定/入力」を明記。独立 reviewer が保証表現と SLO 発明を除去する。対外保証への昇格は本 task の成果ではない。

### P7-R09 — full runtime qualification、独立 review、receipt fan-in

**Files:** `docs/superpowers/programs/search-platform-completion/{p7-runtime-qualification,p7-runtime-code-review,p7-runtime-receipt}.md` は qualification worker、独立 reviewer、parent が**別 writer**で順に所有。code 修正は各失敗の元 task/owner に返す。**Consumes:** R01〜08、P7-01〜12、P1/P2/P3/P4/P5/P6 final receipt、P5 real socket E2E、P6 Search ack E2E。**Produces:** final exact-head local/hosted gate へ渡す P7 判定と既知の外部境界。

**RED→GREEN:** first integrated run で unmet criterion を列挙し、該当 owner の focused RED→fix→GREEN と独立再 review に戻す。最終 real qualification は Document→Unit→lexical/Graph→API、Remote outage/NO_RETENTION、identity revocation、outbox producer→Search receipt→generic ack、rebuild/restart/restore、degraded/health/Audit/OTel/secret/privacy、Docker smoke、R07測定を exact code SHA で照合。対象全体後の `mise run verify:fast`、必要な `mise run verify`、最終 Draft PR の required hosted checks は一度の決定的 gate とし、結果を別 evidence に記す。**Independent review:** whole-program security reviewer は unauthorized existence/stale access、remote prompt injection/SSRF、parser bomb、Graph跨ぎ、tenant/Source混合、NO_RETENTION、outbox replay、API限界を確認。P1/P2 blocking finding は修正・再審査。P7 receipt は local test、runtime load、review、hosted exact-head、live deployment を別欄にする。

## 3. 作業順と未解決条件

`R01` は runtime design GO 後に既存 core producer と並行可能。`R02` は P7-01〜11、採択済み P3、P5-08 factory、P6-I04 wiring の受理済み code slice を待つ。`R03/R04` は R02 後に独立ファイルで進め、`R05` は P7-12/real rebuild、`R06` は app path、`R07→R08→R09` は実測順とする。各 shared writer を直列化する。P5/P6 final receipt と P7-12 を早期 design/code task の前提にしないため graph cycle を避ける。P3/Vector conditional branch は非採択なら明示 `Disabled/代替経路` receipt を消費し、mandatory neutral core と実 Graph path を残す。

Graph backend、Vector 採否、host identity/secret resolver、timeout/容量/alert/SLO 数値は各 bounded selection/qualification task が決める。live production target の選定、third-party credential/payment/契約、不可逆 migration、license/legal blocker、安全性の重大な人間判断に到達した場合だけ依頼文 §20 の Hard Stop とする。Draft PR の merge/live deploy/production migration は本計画の実行操作に含めない。
