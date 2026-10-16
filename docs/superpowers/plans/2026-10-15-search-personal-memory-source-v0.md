# Search Personal Memory Source v0 実装計画

- 状態: PROPOSED / 設計判断待ち。計画の承認・実装の開始・依存の採用・配備の承認ではない。
- 設計: [Search Personal Memory Source v0 設計](../specs/2026-10-15-search-personal-memory-source-v0-design.md)（以下「設計」）。状況: [Capability Execution Status](../execution/search-personal-memory-source-v0-status.md)。
- 基点: main `ead6830e28bd18b93cd71e2087818ebfdab37a2b`。各 PR は、その時点の最新 main から branch を切る。
- 進め方（KP の手順）: Draft PR → exact head の全 CI → Ready → 所有者の明示の承認（例「CI 成功後に統合していいです。」）→ merge → merge 後の main CI の確認。1 PR を 1 つの統合単位とし、各 PR は書いた担当と別の read-only reviewer の独立 review を経てから Ready にする。
- 進行中の作業との関係: 未 merge の Draft PR #85（日本語 bigram ほか）・#86（検証基盤）・#99（世代の部品化、#85 を含む）の上に乗る（設計 §2.1）。2026-10-15 の読み取りでは、#85 と #86 は `CONFLICTING`、#99 は `MERGEABLE` だが base が #85 の branch で、題は「作業中」のまま。所有者が承認したのは #99 の設計で、PR ではない。これらの PR の範囲の file は、その担当の作業を待ってから触る。#86 と #99 の branch は他の作業が更新しているので、決して触らない。migration の番号は #85・#99 が 0010〜0013 を使うので、各 PR を作る時点で main と進行中の PR を確かめて決める。#85・#99 は 2026-10-29 を判断日として待ち、それまでは #85・#99 に依らない PR を先に進める（owner decision 第 2 回、§4 の 1）。
- 拘束する決定: 依頼者の決定、lead decision 2026-10-15、owner decisions 2026-10-15（第 2 回）（設計の冒頭）。この計画に効くのは、KP-2 の基準の固定、v0 の 1 つの database、Memory に GPU を使わない、pin の意味を変えない、P0〜P6 を全部 v9.0.0 に入れ最適化は予算を外したときだけ、配備はすべてこの Mac（RTX 5090 には配備しない）、本番の予算はこの Mac の軽い qualification・規模は RTX 5090 の重い計測、#85・#99 は 2026-10-29 まで待つ、の 8 つ。
- 重い作業の場所: build・試験は hosted CI で行い、規模の計測は RTX 5090 の機械（ssh は依頼者の承認後）で行う。この Mac は本番の置き場で、ここでは配備（KP-6）と軽い qualification（KP-7）だけを行い、1 分 load を 20 以下に保つ（cargo の job 数を絞る。長い build を Mac で回さない。release binary は hosted CI の macOS の job で作る）。RTX 5090 の機械は #86 も使うので、使う時間を #86 の担当と調整する。
- データ: 合成データと公開データ（MIRACL 日本語、Apache-2.0）だけを使う。実際の Memory 本文・会話・認証情報は使わない。
- 試験の名前は下の「受入」に固定する。受入では `cargo test -p <crate> -- --list` にその名前が出ることと、exact head の CI で通ったことの両方を確かめる（filter が 0 件に当たって緑になるのを防ぐため）。

## 1. PR の一覧と順序

| PR | branch（案） | 内容 | 依存 | 並行 |
|---|---|---|---|---|
| KP-0 | `design/personal-memory-source-v0` | 設計・計画・状況の文書（この PR） | なし | — |
| KP-1 | `feat/search-server-binary-*` | 常駐の `search_server` binary と EdDSA assertion の資格情報検証器 | KP-0 の承認 | KP-2・KP-3 と並行 |
| KP-2 | `measure/search-memory-lexical-*` | Memory の日本語字句の計測（#85 の bigram で保存前照合の基準を満たすか） | KP-0 の承認、#85 の merge | KP-1・KP-3 と並行 |
| KP-2L | `measure/search-memory-lindera-*` | KP-2 の基準を満たさないときだけ: Lindera（辞書を file で置く方式）の計測と選定 | KP-2 の結果 | — |
| KP-3 | `feat/search-personal-memory-kind-*` | `PersonalMemory` の Source 種類・登録・可視性・永続の門・既存 route からの分離 | KP-0 の承認（protocol `personal-memory-source/2` は担当の間で合意済み、所有者の承認はまだ。設計 §4） | KP-1・KP-2 と並行 |
| KP-4 | `feat/search-memory-ingestion-*` | 変更列の取り込み、部品方式の永続世代（字句と Vector を同じ構築で公開）、抑止・忘却の物理的な消去（強制の書き直しと期限）・確認の返送、再同期、Memory の字句 schema | KP-3、#99 の merge | KP-5 の開発・KP-6 の手順書と並行 |
| KP-5 | `feat/search-memory-api-*` | `/v1/memory/search`・`expand`・`status` と隔離の試験 | KP-3（merge は KP-4 の後） | KP-4 と並行に開発 |
| KP-6 | `ops/search-memory-server-macos-*` | この Mac への配備（launchd、専用の利用者、macOS の PostgreSQL）の手順と実施記録、macOS の release binary の CI の job | KP-1、KP-4、KP-5 | 手順書と CI の job は KP-1 の後に KP-4 と並行 |
| KP-7 | `measure/search-memory-budget-mac-*` | この Mac での軽い qualification と本番の予算の固定（1 分 load 20 以下） | KP-4、KP-5、KP-6、KP-2（と KP-2L） | KP-7R と並行 |
| KP-7R | `measure/search-memory-scale-rtx5090-*` | RTX 5090 の機械での重い計測（規模と同時の負荷、機材つきの数値。予算には使わない） | KP-4、KP-5、ssh の承認（owner の Tailscale の承認待ち） | KP-6・KP-7 と並行 |

**critical path**: KP-0 → KP-3 → KP-4（#99 の merge を待つ）→ KP-5 → KP-6 → KP-7。#99 が KP-3 より遅れれば、#99 の merge が critical path に入る。#99 は #85 の branch の上に積まれている（base が #85）ので、#85 の merge も critical path に入りうる（#85 → #99 → KP-4）。KP-6・KP-7 はこの Mac で行うので ssh を待たない。KP-7R は ssh の承認を待つが、KP-6・KP-7 を止めない。AP の 02 は KP-7R を release の条件に入れた（P10.3-M6 の規模の証拠）ので、RTX 5090 への ssh の承認は v9.0.0 の前に要る。#85・#99 は他の担当の Draft なので、この計画は工数の見積りを置かない。待つことと判断日 2026-10-29 は §4 の 1。

**並行に動かせるもの**（担当を分け、worktree も分ける）: KP-0 の承認後すぐに KP-1（worktree A）と KP-3（worktree B）を同時に始められる。KP-2（worktree C）は #85 の merge 後に始める。KP-3 の merge 後は、KP-4（#99 の merge 後、worktree B を作り直す）、KP-5 の開発（worktree D、合成の世代 fixture で進める）、KP-6 の手順書と macOS の release binary の CI の job（worktree E。KP-1 の merge 後）を並行できる。KP-5 の merge は KP-4 の後にする（世代の形が KP-4 で決まるため）。KP-7 と KP-7R は KP-5 の後に並行できる（KP-7R は ssh の承認の後）。#85・#99 を待つ間（判断日 2026-10-29）に進めるのは KP-1・KP-3・KP-5 の開発・KP-6 の手順書と CI の job である。

**共有する file の担当**: `crates/search-application/src/source_registration.rs`・`crates/search-runtime/src/host_inventory.rs` は KP-3 だけが触る。`spec/api/search-openapi.yaml` は、KP-3 が Source 種類の enum に `memory` を足すだけ、KP-5 が route を足す。KP-5 は KP-3 の merge 後に作るので同時には触らない。`spec/errors/search-api-error-registry.yaml` は KP-5 だけが触る。`crates/search-runtime/src/vector_runtime.rs`・`vector_store.rs`・`durable_read.rs` は #99 の範囲なので、KP-4 は #99 の merge 後に触る。

## 2. 各 PR

### KP-0 設計と計画（この PR）

- 成果物: `docs/superpowers/specs/2026-10-15-search-personal-memory-source-v0-design.md`、本書、`docs/superpowers/execution/search-personal-memory-source-v0-status.md`。
- 受入: 文書だけの差分で、hosted CI の `policy` job と `required-check` が通る。設計 §2 の file:line が基点 commit で実在する（独立 reviewer が 76 件を開いて確かめた。2026-10-15 の p0-review も約 70 件を開いた。指摘の反映は設計 §8 と状況文書に記録）。所有者が設計を承認する（owner decisions 第 2 回を反映した版で）。
- rollback: revert するだけ。製品に影響しない。

### KP-1 Search server binary と資格情報検証器

- 範囲: `crates/search-runtime/src/bin/search_server.rs`（`serve`・`migrate`・`check`・`inventory publish`。`memory resync` は KP-4 で足す）、`crates/search-runtime/src/credential.rs`（EdDSA の compact JWS を検める `SearchCredentialVerifierPort` の本番実装、設計 §3.9）、設定の型と検証（model・字句の root などの path を設定で与える。DB の接続情報などの秘密は、環境変数のほかに、専用の利用者だけが読める file からも読めるようにする。この Mac では launchd の plist に秘密を書かないため、設計 §3.9）、`Dockerfile` の `search-server` target、`docs/operations/search-server.md`（起動と設定だけ）。Document Source が 0 件でも起動できる組み立て。
- 依存の追加: `ring` 0.17.14 を `search-runtime` の直接依存にする（既に rustls 経由で lock graph にある、`Cargo.lock:4762-4763`）。`spec/selection/library-tool-selection-v0.md` に用途を足し、cargo-deny・OSV を通す。
- 所有: `crates/search-runtime/src/bin/search_server.rs`、`credential.rs`、`Dockerfile`。
- 受入（試験名）:
  - `search-runtime` の unit: `credential::tests::accepts_valid_eddsa_assertion`、`rejects_unknown_kid`、`rejects_wrong_audience`、`rejects_expired`、`rejects_iat_beyond_skew`、`rejects_non_eddsa_alg`、`rejects_missing_memory_scope`、`rejects_lifetime_over_300s`、`never_reads_identity_from_body`。
  - 実 PostgreSQL 18.6（testcontainers）の `crates/search-runtime/tests/search_server.rs`: `refuses_start_without_issuer_keys`、`fixed_401_challenge_without_token`、`serves_v1_sources_with_valid_assertion`、`starts_with_empty_document_namespace`、`reads_secrets_from_owner_only_file`（権限が 0600 でない秘密の file では起動しない）。
  - hosted CI の container build で `search_server` の release binary が作れる。
- rollback: merge commit を revert する。schema の変更は無い。

### KP-2 Memory の日本語字句の計測

- 範囲: `experiments/search-memory-lexical-poc/`（新規。製品 code から分ける KP の規則どおり）と、計測を hosted で回す workflow `.github/workflows/search-memory-qualification.yml`（`measure/search-memory-**` の push だけで動く。既存の `search-p3-qualification.yml` は `measure/search-p3-**` だけで動くため、`:7-8`）。製品の analyzer は #85 の `tantivy-0.26.2-cjk-bigram-v1` をそのまま使う。
- データ: 合成の日本語 Memory コーパス（1,000 件以上、問い 60 以上、重複・矛盾の組を含む。合成であることを README に書く）と、既存の MIRACL 日本語 dev の公開レーン（`experiments/search-vector-model-poc/scripts/prepare_public_ja.py` で作り直す。本文は repository に入れない）。
- 手順: (1) lead decision 2026-10-15 で固定した基準（設計 §3.4。重複・矛盾の組について、L＋V の Recall@20 の 95% 区間の下限 0.90）を、計測の前に `gate.md` として最初の commit に書く。基準は計測の結果を見て変えない。(2) その後の commit で計測と報告。順序は commit 履歴で示す。(3) Mac では回さない。
- 受入: `gate.md` の commit が計測結果の commit より前にある。`report.md` と `measurement-*.json` に、方式ごとの Recall@20・nDCG@10・該当なしの誤検出・paired bootstrap の 95% 区間・索引 byte・問い合わせ p95 がある。基準を機械的に当てた結論（bigram のまま進む／KP-2L に進む）が報告にある。索引側の e5 の接頭辞（`passage: ` と `query: `）と Memory 用の τ も同じ報告で決める（設計 §3.5）。
- rollback: 実験だけなので revert で済む。

### KP-2L Lindera の計測と選定（KP-2 の基準を満たさないときだけ）

- 範囲: `experiments/search-memory-lexical-poc/` に Lindera 6.2.0＋IPADIC（file から読む辞書）の候補を足して同じ基準で計測する。選ぶなら `spec/selection/library-tool-selection-v0.md` §13 の改訂と `docs/decisions/` の決定記録（辞書の入手元と SHA-256、COPYING 全文の同梱、build 時にダウンロードしないこと）、続く製品 PR（`search-tantivy` の adapter と辞書の照合）。
- 受入: KP-2 と同じ報告の形に Lindera の行が加わる。選ぶ場合は cargo-deny・OSV・license の検査が通る。製品 PR では `analyzer::ja_morph_refuses_wrong_dictionary_digest` と `analyzer::ja_morph_matches_poc_golden`（`experiments/search-discovery-poc/tests/data/tokenizer-6.2.0-golden.json` の 59 入力・257 token と一致）。
- rollback: analyzer は世代の manifest の `analyzer_version` で選ぶので、設定を戻せば次の世代から元の analyzer で作り直される。

### KP-3 PersonalMemory の Source 種類と登録

- 範囲: `search-core`（`ResourceKind::Memory`、`ResourceBody::Memory`）、`search-application`（`SourceKind::PersonalMemory`、`RegistrationNamespace::PersonalMemory`、登録型 `ServerPersonalMemoryRegistrationConfig`・`PersonalMemorySourceRegistration`、永続化 DTO と digest の tag、可視性の規則、`SourceRegistrationCatalog::try_new` の第 3 namespace）、`search-api-http`（Source 種類の enum の追加と、`memory.search` の scope を既存の 4 route で 403 にすること）、`search-runtime`（`host_inventory` の第 3 namespace、新しい migration、永続世代の門 `generation_registration.rs:167` と `graph_coordination.rs:47` を Document と PersonalMemory に開く。Remote は拒否のまま）。設計 §3.1 の「compile が通ってしまう箇所」をすべて扱う。
- migration: `0002_search_source_ownership_v1.sql:130` の `CHECK (source_kind IN ('DOCUMENT','REMOTE'))` と、`0007_host_registration_inventory_v1.sql:19-21, 29-33` の Document・Remote に固定した列と `schema_version` を広げる。追加だけでは済まない。番号は作る時点で決める。
- 所有: `source_registration.rs`、`host_inventory.rs`、`generation_registration.rs`、`graph_coordination.rs`、`discover_route.rs` の該当 2 か所、新しい migration、OAS の Source 種類の enum。
- 受入（試験名）:
  - `search-application`: `personal_memory_registration::requires_owner_principal`、`::endpoint_loopback_http_else_https`、`::digest_stable_v1`、`::no_provider_conversion`（登録型に `Deserialize` が無いことを compile 時の検査で確かめる）、`memory_visibility::grants_only_owner_with_memory_scope`、`::document_scope_cannot_see_memory_source`、`::memory_scope_cannot_see_document_source`、`source_registration::catalog_accepts_three_namespaces`、`discover_route::memory_sources_not_in_discover`。
  - 実 PostgreSQL の `search-runtime`: `host_inventory::personal_memory_namespace_publish_reconcile`、`generation_registration::memory_allowed_remote_refused`、`graph_coordination::memory_allowed_remote_refused`、`server_e2e::memory_source_absent_from_existing_routes`、`server_e2e::memory_scope_forbidden_on_existing_routes`。
  - 既存の `search-application`・`search-runtime`・`search-api-http` の全試験が通る（数を PR に書く）。
- rollback: merge commit を revert する。DB は、Memory の行が無いことを確かめてから、制約を元に戻す migration を前進で足す。その前に、Memory の namespace を含まない inventory を公開し直す（手順を PR に書く）。

### KP-4 変更列の取り込みと永続世代

- 範囲: 新しい crate `crates/search-source-memory`（`personal-memory-source/2` の S1〜S5 の decode と上限、固定 endpoint の transport、KP indexer の要求ごとの EdDSA JWS）、`crates/search-runtime/src/memory_runtime.rs`（取り込み task、#99 の部品方式による世代の構築と公開、抑止、確認の返送、再同期、backup 復元の検出）、Memory の字句 schema（本文の field と keyword の field、`analyzer_version` は #85 の bigram）、Memory 関係の Graph mapping、Vector の保守を Memory Source に広げる、新しい migration（取り込み状態、抑止、確認の outbox）、`search_server memory resync`。試験用の合成提供者（Remote と同じく `synthetic-loopback-test-only` feature の下だけで compile）。`spec/architecture/dependency-rules.toml` に新 crate の境界を足す。KP indexer の署名鍵の渡し方（環境変数か file か）をここで決める。
- 字句と Vector を同じ構築で公開する（設計 §3.3 の手順 4、§3.5 の lane の規則）: Memory の世代は、字句・Graph・Unit の部品と Vector の部品がそろってから pointer を動かす。埋め込みのモデルが使えないときだけ Vector なしで公開する。#99 の Vector の構築は別の loop で今の世代に対して動く（#99 の設計 `:59`）ので、公開の前の READY の世代に対して Vector を作れるかをこの PR の最初に確かめ、作れなければ設計 §3.5 のとおり lane を `null` にする形で進め、PR の説明に書く。
- 忘却の物理的な消去（設計 §3.3）: (a) 公開の直後に、抑止した記憶を含む古い世代を時限で回収する（次の公開を待たない）。(b) 抑止した記憶の文書を持つ字句の segment（Unit 索引と Resource 索引）を、それを除いて書き直してから新しい世代に入れる（1 segment の統合。hard link しない）。search server が古い世代の reader を閉じたことを確かめる。(c) Unit 部品・Vector の部品と値・Graph 部品・取り込み状態の digest の行を消す。(a)〜(c) が成り立ってから S4 の `purged` を送る。書き直しは保守の窓を待たず、その記憶を外す次の世代の構築で行い、他に変更が来なくても設定の目標時間 `purge_target`（既定 30 分の見込み、KP-7 で予算に固定し直す）の中で構築と回収を走らせる。protocol の上限は `suppressed` から 24 時間で、過ぎても終えていなければ `/v1/memory/status` の `purge` を `overdue` にし、終えた時点で遅れても `purged` を送る。この PR が merge されるまで KP は `purged` を送らない。
- protocol の写し: 合意した `personal-memory-source/2` の JSON 例と schema を `spec/api/personal-memory-source-v2/` に置き、AP 側の正本（agent-platform の `docs/design/personal-memory/03-core-schema-api.md` §5.4・§5.5）の path・commit と、設計 §4 に書いた契約の範囲の SHA-256 を冒頭に書く。KP の conformance 試験はこの写しを読む。正本の契約の範囲が変わったら版を上げる。
- 所有: `crates/search-source-memory/`、`memory_runtime.rs`、新しい migration、`vector_runtime.rs` の Memory 用の変更（#99 の merge 後）。
- 受入（試験名、`crates/search-runtime/tests/memory_ingest.rs`、実 PostgreSQL）: `initial_snapshot_builds_generation`、`upsert_batch_publishes_incremental_generation`、`generation_publishes_lexical_and_vector_together`、`remove_suppresses_immediately`、`suppression_applies_to_pinned_generation`、`purge_rewrites_segments_holding_suppressed`（新しい世代のどの segment も、抑止した記憶の文書を削除の印つきでも持たない。古い segment の file を hard link していない）、`purged_leaves_no_canary`（Source の lexical root の全 segment を削除の印を無視して開き、stored の本文と語の辞書に canary が無い。PostgreSQL の行にも canary の本文と digest が無い。忘却の前に同じ検査が canary を見つける対照を先に置く。doc store は lz4 で圧縮されるので file の byte 列だけを探す検査にしない）、`purge_completes_without_further_updates`（忘却の後に更新が来なくても目標時間の中で `purged` が届く）、`purge_overdue_reported`（注入した時計で 24 時間を過ぎても (a)〜(c) を終えられない状態を作り、status が `overdue` になり、`purged` を送らず、終えた後に遅れて `purged` が届く）、`logs_have_no_body_or_digest`（log を捕捉し、canary を意図して書いた行が見つかる対照を置く）、`token_expired_triggers_full_resync`、`digest_mismatch_is_unknown_not_absence`、`provider_failure_is_unknown_not_absence`、`space_id_mismatch_rejected`、`relation_endpoints_same_source`、`unknown_relation_kind_is_unknown`、`duplicate_records_idempotent`、`sigkill_resumes_from_checkpoint`（子 process を SIGKILL して再起動）、`restored_backup_blocks_until_resync`、`change_rebuilds_only_its_source`、`unchanged_items_share_segments`、`vector_stage_evicted_after_pointer_move`。`search-source-memory` の unit: `protocol::{rejects_oversized_token, rejects_body_over_limit, rejects_non_utf8_body, rejects_unknown_fields, rejects_protocol_v1}`、`transport::{refuses_redirect, refuses_non_loopback_http, pins_configured_address}`。合成 1,000 件の取り込みにかかった時間を PR に記録する（予算ではない）。
- rollback: merge commit を revert する。DB は追加の表だけなので残しても害が無い。取り込みが止まると AP からは Search が使えないように見え、AP は照合できない内容を active にしない（設計 v2 §6）。

### KP-5 Memory の問い合わせ API

- 範囲: `search-api-http`（3 route、DTO、`spec/api/search-openapi.yaml` の route 追加、`spec/errors/search-api-error-registry.yaml` の既存 code の `operations` への追加。新しい code は作らない、設計 §3.7）、`search-application`（`memory_query.rs`: filter の閉じた演算子、S1 の優先連結 L→V、`expand` の予算・`exclude`・`after`、期限と node 数の数え上げ）、`search-runtime`（Memory の世代に対する backend）。
- 所有: OAS の Memory の route、error 台帳、`memory_query.rs`、route と DTO。
- 受入（試験名）: `crates/search-runtime/tests/memory_api.rs`（実 PostgreSQL・実 TCP）の `complete_empty_vs_unavailable`、`filters_applied_before_ranking`、`applies_to_formula_project_and_skills`、`generation_watermark_returned`、`lanes_report_each_retriever_position`（要求した retriever ごとに lane があり、字句の lane の位置は世代の `appliedThrough` と同じ）、`vector_lane_absent_is_partial_not_complete`（応答に使った世代に READY な Vector 部品が無いとき、`status` は `partial`、Vector の lane は `null`、gap は `AVAILABILITY`。決して `complete` にしない。レビューの `vector_lag_reported_as_gap` に当たる）、`status_reports_lanes_and_purge_state`、`generation_changed_is_409`、`expand_budget_frontier`、`expand_stateless_continuation`、`expand_unknown_seed_no_error`、`assertion_owner_binds_source`、`document_scope_forbidden_on_memory_routes`、`other_owner_sees_not_found`、`scores_independent_of_other_owner`、`no_totals_in_any_route`、`trace_has_no_counts`、`problem_codes_fixed`、`logs_have_no_query_body_or_digest`（log を捕捉し、問いの文・canary の本文・summary・digest の 16 進が出ない。canary を意図して書いた行が見つかる対照を置く）、`memory_routes_create_no_evaluation_lease`（3 route を呼んでも `search_evaluation_lease` の行が増えない。対照として、既存の評価の pin を 1 回作ると 1 行増えることを見る。設計 §3.7）。`crates/search-api-http/tests/` の `memory_contract::routes_match_openapi`、`memory_contract::unknown_fields_rejected`。
- rollback: merge commit を revert する。AP からは route が無くなり、AP はそれを利用不可として扱う。

### KP-6 この Mac への配備（launchd）

- 置き場: owner の Apple silicon の Mac（owner decision 第 2 回）。RTX 5090 には配備しない。ssh は要らない。macOS で動くことは未確認なので、この PR で確かめる（KP の CI は macOS で `cargo check` だけ、設計 §3.9）。
- 範囲: `docs/operations/search-memory-server-macos.md`（導入、macOS の PostgreSQL（推奨は素の PostgreSQL を launchd で。18.6 を使えるかを確かめ、使えなければ 18 の最新の minor と差を記録。代わりの案は OrbStack の container）、v0 は 1 つの database（lead decision）、role と migration、Unix socket と `peer` 認証の可否、専用の macOS の利用者と LaunchDaemon（`UserName`）、data の directory を専用の利用者の 0700 にすること、モデルの配置と SHA-256 の照合、`127.0.0.1` だけの待ち受け、秘密を plist に書かず 0600 の file に置くこと、`Nice`・thread 数・cache の上限の設定、本人の space の inventory 公開、KP indexer の鍵の作成と AP への公開鍵と `source_grants` の登録、状態確認、眠りから起きたときの確認、再同期、KP の data を Time Machine などの backup から外す手順（外せない場合の扱い、設計 §3.9）、backup 復元時の手順、撤去）、同じ Mac に本番と別の staging の instance（別の port・別の database・別の data root・合成の space だけ）を立てて消す手順（AP-6 の merge の前の E2E と KP-7 が使う。本番の Memory の Source には触らない）、`tools/search-memory-server/macos/`（launchd の plist の雛形。秘密を含めない）、hosted CI の `macos-15` で `search_server` の release binary を作り SHA-256 とともに成果物にする job。
- 実施（この Mac で、owner の承認の後）: Mac の機種・macOS の版・CPU・RAM・disk の空き・FileVault の状態を読み取りで記録してから導入する。記録するもの: binary とモデルの SHA-256、`search_server check` の出力、合成 space での `/v1/memory/status`、再起動と眠りから起きた後の永続と再開、導入の前後の 1 分 load。状況文書に書く。この Mac で build するときは `-p search-runtime --bin search_server` に絞り `CARGO_BUILD_JOBS=2` で、load を見ながら回す。
- 受入: 手順書が上記の項目を持ち、実施記録が状況文書にある。手順どおりに撤去して元に戻せることも 1 回確かめる。staging の instance を本番と並べて立て、消した後に本番の `/v1/memory/status` が変わらないことを 1 回確かめる。macOS の CI の job が release binary と SHA-256 を出す。
- rollback: `launchctl bootout` で service を止めて plist を外し、KP の data の directory と PostgreSQL の data を消す。Search の状態は AP から作り直せる。撤去の後、AP からは KP が使えない（AP は照合できない内容を active にしない）。

### KP-7 この Mac での軽い qualification と本番の予算の固定

- 範囲: `experiments/search-memory-bench/`（合成の日本語 Memory 生成器: 4 KiB 以下の分布と日本語の長文の境界例、関係の平均 0・2・10、project と skill。1 つの session の driver と試験用の鍵での assertion 発行。1 分 load の見張り）、報告 `docs/superpowers/execution/search-memory-qualification-mac-<日付>.md` と raw JSON。設計 §3.10 の表の「この Mac の軽い qualification」の列をすべて測る（忘却の `suppressed` から `purged` までの時間を含む）。計り方は #86・#99 の方法に合わせる。
- 負荷の約束: 1 分 load を一定の間隔で読み、18 を超えたら計測を止める（20 より上げない）。同時の負荷はかけない。本番の `search_server` とは別の process・別の port・別の database・別の字句の root で、合成データだけを使う。本番の Memory の Source には触らない。報告に最大の load と、止めた回があればその記録を書く。
- 予算の固定: 計測の後、本書に「予算（この Mac）」節を足し、絶対値（ms・RSS・byte、`suppressed` から `purged` までの時間。protocol の T = 24 時間より短い値）を書く。予算はこの Mac の数値だけから決める。hosted CI の機械は配備先と違うので、CI には件数に比例した処理が問い合わせに入っていないことなどの相対的な検査を入れ、絶対値の確認は release ごとにこの Mac の軽い qualification で行う（設計 v2 §10.3）。最適化の PR は、計測が予算を外したときだけ作る（依頼者の決定 2）。
- 受入: 報告に、Mac の構成、版、繰り返し回数、最大の load、p50・p95、idle と peak の RSS、起動時間、byte、忘却の時間がある。予算の節が本書にある。設計 §3.8 の隔離試験が同じ build で通っている。
- rollback: 文書と bench だけなので revert で済む。

### KP-7R RTX 5090 の機械での重い計測（機材つき）

- 範囲: KP-7 と同じ `experiments/search-memory-bench/` に、規模と同時の負荷の driver を足す。報告は `docs/superpowers/execution/search-memory-scale-rtx5090-<日付>.md` と raw JSON で、KP-7 の報告と分ける。設計 §3.10 の表の「RTX 5090 の重い計測」の列をすべて測る（1k・10k・100k 件、同時 1・4・16、世代の切替の止まり、全件の再同期）。
- 実施（ssh の承認後、#86 の担当と時間を調整して）: 機械の OS・kernel・CPU・RAM・disk・GPU とドライバ・vLLM の状態を読み取りで記録してから測る。vLLM が動いている状態と止めた状態を分けて記録する。KP の server と PostgreSQL は計測のための一時的なもので、計測の後に撤去する（この機械には配備しない）。
- 数値の扱い: 機材を明記した参考値で、この Mac の予算の合否には使わない。AP の P6 の規模の証拠に使い、AP の 02 はこれを release の条件に入れた（P10.3-M6）。
- 受入: 報告に、機材、版、繰り返し回数、vLLM の状態、p50・p95・p99、RSS、byte、全件の再同期時間がある。計測の後に一時的な server と data を撤去したことの記録がある。
- rollback: 文書と bench だけなので revert で済む。

## 3. AP 側との接点（cross-repo 計画へ渡す）

| KP の PR | AP 側に要るもの | 時期 |
|---|---|---|
| KP-3・KP-4 | `personal-memory-source/2`（2026-10-15 に担当の間で合意、所有者の承認はまだ。設計 §4）。AP の S1〜S5（items・changes・items:batchGet・acks・acks/last）と、KP indexer の JWS の検証と `source_grants`。忘却の receipt が `suppressed` と `purged` を設計 §3.3 の意味で扱うこと | KP-4 の実 E2E の前に AP 側の実装 |
| KP-1・KP-5 | AP が本人ごとの EdDSA assertion を発行する（claim は設計 §3.9）。AP が `lanes` の最小の位置で未反映の差分を数え、Vector の lane が欠けた `partial` の扱いを決めること | KP-5 の実 E2E の前 |
| KP-6 | AP の Memory API を同じこの Mac に置く（loopback、設計 §4・§3.9）。本人の space を作ったときにこの Mac の上で owner が `search_server inventory publish` で登録する手順と、KP indexer の公開鍵を AP の `source_subscriber_keys` に入れて `source_grants` を足す手順を、AP の配備の手順書（AP-7・O-3）と KP-6 の手順書に入れる。AP の service も眠りで止まること、AP の data の控え（外の disk など別の場所へ）は AP の手順書が持つ。toolbox は KP を触らない | KP-6 の実施の前 |
| KP-7 | AP の P2（Memory API と Search の E2E）と P6（性能）が、この Mac の予算として KP-7 の数値を使う | v9.0.0 の判断の前 |
| KP-7R | AP の P6 の規模の証拠として KP-7R の数値（機材つき）を使う。この Mac の予算には使わない | v9.0.0 の前（AP の 02 が release の条件に入れた。P10.3-M6） |

## 4. リスクと先に決める順番

1. #99・#85 の merge の時期（KP-4 の開始を決める）。owner decision 第 2 回: 待つ。判断日は 2026-10-29 で、それまでに #85・#99 が動かなければ owner が決め直す（そのときの選択肢は「待ち続ける」か「main の方式で先に作って後で移す」か）。それまでは #85・#99 に依らない KP-1・KP-3・KP-5 の開発・KP-6 の手順書と CI の job を先に進める。#86 と #99 の branch は他の作業が更新しているので触らない。2026-10-15 の状態: #85 は Draft で `CONFLICTING`、#99 は Draft で base が #85 の branch、題は「作業中」（設計 §2.1）。#99 には所有者の判断待ち（GPU で埋め込む方法、割当て器）と未達の合格条件（worker 2 GB）が残る。KP-1・KP-2（#85 の後）・KP-3 は #99 を待たずに進められる。main の方式で先に作る場合の代価: (i) 世代ごとに Source の件数に比例した作り直しと再検証が起きる（設計 G14。Memory の件数での時間は未計測）。(ii) 公開済みの Vector の stage が RAM に溜まる（設計 G7）。(iii) KP-4 の migration が #85・#99 の 0010〜0013 と番号でぶつかり、#99 の merge の後に `memory_runtime.rs`・migration・忘却の試験を部品方式へ作り直す手戻りが出る。利点は、#99 を待たずに KP-4 を始められることと、main では世代ごとに字句の directory を全件から作る（#99 の設計 `:12`）ので、忘却した記憶の文書が後の世代の segment に残らず、設計 §3.3 の強制の書き直しが要らないことである。
2. 世代の切替の止まり（#86 の計測で p99 2〜3 秒、1 万文書）。Memory は pin の意味を変えない（lead decision 2026-10-15）ので、前の世代で答える方式は入れない。Memory は本人ごとで Unit が少ないので、まず KP-7（この Mac、1 つの session）と KP-7R（同時の負荷）で測り、この Mac の予算を外したときだけ対策する。
3. 日本語の字句の質（KP-2）。基準は固定済み（lead decision）。bigram で基準を満たせば辞書を使わない。
4. この Mac の負荷（KP-6・KP-7）。本番の置き場が owner の作業機なので、KP の常駐も 1 分 load 20 以下の約束の中に入る。macOS には cgroups が無いので、KP の設定（thread 数、同時に作る世代 1 つ、cache の上限、`Nice`）で抑え、KP-7 の軽い qualification で確かめる。Memory は v0 で GPU（Metal を含む）を使わない（lead decision）。
5. この Mac が眠ると KP・PostgreSQL・AP の Memory API が止まり、別の PC から使えない（設計 §3.9）。眠りを止めるかは owner の設定の判断。
6. RTX 5090 で vLLM・#86 と資源を取り合うこと（KP-7R だけ）。計測の後に一時的な server を撤去する。ssh の承認を待つ。KP-7R は release の条件（AP の 02）なので、承認が遅れれば v9.0.0 も遅れる。
7. AP の署名鍵が漏れたときの影響（設計 §3.9）。KP は登録済みの owner principal の Source にしか grant を返さず、鍵は `kid` で入れ替えられるようにする。
8. 忘却の物理的な消去（設計 §3.3）。#99 の統合の条件が未決のまま、Memory だけ強制の書き直しを入れる。KP-4 が実装するまで `purged` は送らないので、AP の忘却の receipt は `suppressed` で止まる。

## 5. 外部資料（2026-10-15 確認。設計 §3.4・§3.5 の出典）

- Lindera 6.2.0 は crates.io で MIT（https://crates.io/crates/lindera/6.2.0 、https://docs.rs/crate/lindera-ipadic/6.2.0/source/Cargo.toml ）。辞書データは IPADIC の条件に従う。NAIST は著作権表示と段落の同梱を、ICOT は NO WARRANTY 節の添付を求める（https://github.com/taku910/mecab/blob/master/mecab-ipadic/COPYING ）。`lindera-ipadic` 6.2.0 の NOTICE.txt は NAIST の著作権表示の行を欠く（https://github.com/lindera/lindera/blob/v6.2.0/lindera-ipadic/NOTICE.txt 。手元の crates.io 配布物でも確認した）。
- `embed-ipadic` は 9 file を無圧縮で `include_bytes!` する（https://github.com/lindera/lindera/blob/v6.2.0/lindera-dictionary/src/macros.rs#L73-L94 ）。v6.2.0 の release asset の中身から、埋め込みの増分は約 47.5 MB と算出した（https://github.com/lindera/lindera/releases/tag/v6.2.0 。実測ではない）。圧縮 feature は v3.0.0 で削除された（https://github.com/lindera/lindera/pull/655 ）。
- build 時のダウンロードは MD5 で照合し、`LINDERA_BUILD_DICTIONARY_CACHE_DIR` を置けば offline で build できる（https://github.com/lindera/lindera/blob/v6.2.0/lindera-dictionary/src/assets.rs#L252-L333 、https://github.com/lindera/lindera/blob/v6.2.0/lindera/README.md ）。同じ cache を使う同時 build の衝突は issue #1206 で open。
- 辞書を file から読む `load_dictionary`（mmap が既定）が 6.x にある（https://github.com/lindera/lindera/blob/v6.2.0/lindera/src/dictionary.rs#L261-L405 ）。IPADIC の file 辞書での CLI 1 回の peak RSS は 60.7 MB（5.1.0 の計測、https://github.com/lindera/lindera/pull/888 。6.x では未確認）。
- IPADIC の辞書の build は release で 2.73 秒から 1.22 秒に縮んだ（i7-1185G7、https://github.com/lindera/lindera/pull/772 ）。UniDic は 2.3〜2.5 秒・peak RSS 1.45 GB（https://github.com/lindera/lindera/pull/776 ）。
- `lindera-tantivy` の最新 5.0.1 は tantivy ^0.26.1・lindera ^5.0.1 に依存し、6.2.0 への対応は PR #110 が open（https://crates.io/crates/lindera-tantivy/5.0.1 、https://github.com/lindera/lindera-tantivy/pull/110 ）。
- vibrato は MIT OR Apache-2.0、最終版 0.5.2（2025-03-19）、IPADIC の辞書は展開後 47.8 MB で全体を RAM に読む（https://crates.io/crates/vibrato 、https://github.com/daac-tools/vibrato/releases/tag/v0.5.0 、https://github.com/o24s/vibrato-rkyv ）。
- sudachi.rs は crates.io に無く git 依存、v0.7 は不安定と README が明記、SudachiDict は small 115.3 MB・core 202.2 MB・full 330.5 MB（https://github.com/WorksApplications/sudachi.rs 、http://sudachi.s3-website-ap-northeast-1.amazonaws.com/sudachidict/v1 ）。
- tantivy 0.26.2 の `NgramTokenizer` は position を常に 0 にする（https://github.com/quickwit-oss/tantivy/blob/0.26.2/src/tokenizer/ngram_tokenizer.rs ）。#85 の bigram analyzer は自前で、これとは別物である。
- multilingual-e5-small はパラメータ 117,654,272、384 次元、最大 512 token、MIT、`query: `・`passage: ` の接頭辞が必須で、対称なタスクでは `query: ` を勧める。`model.safetensors` は 470,641,600 B（https://huggingface.co/intfloat/multilingual-e5-small 、https://huggingface.co/intfloat/multilingual-e5-small/tree/main ）。
- RTX 5090 は compute capability 12.0、sm_120 の compiler 対応は CUDA 12.8 から、12.8 の Linux ドライバの下限は 570.26（https://developer.nvidia.com/cuda-gpus 、https://docs.nvidia.com/cuda/archive/12.8.0/cuda-features-archive/index.html 、https://docs.nvidia.com/cuda/cuda-toolkit-release-notes/index.html ）。Candle 0.11.0 は cudarc ^0.19.8 を使い（https://crates.io/crates/candle-core/0.11.0 ）、compute capability 120 は sm_120a で build される（https://docs.rs/crate/cudaforge/0.1.6/source/src/compute_cap.rs ）。Candle の sm_120 対応の issue #2966 は open、RTX 5090 の Docker 上で失敗する報告 #3358 も open（https://github.com/huggingface/candle/issues/2966 、https://github.com/huggingface/candle/issues/3358 ）。
