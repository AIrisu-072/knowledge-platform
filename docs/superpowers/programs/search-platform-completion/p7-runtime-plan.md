<a id="p7-production-runtime--bounded-implementation-plan-freeze"></a>
# P7 本番ランタイムの範囲限定実装計画と凍結記録

[翻訳元の固定原文（commit 0ecf486719e3c9d71242e289a7564ad6d1032b3c）](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-plan.md)

本書は意味保存の日本語訳であり、原設計の再承認・資格追加ではありません。既存承認hashは当時の原文・証拠を指し、訳文hashではありません。以下の状態・次の作業は当時の記録で、現在の実行指示ではありません。本文中の元の行番号も当時の原文を参照しており、訳文の行番号ではありません。旧見出しへのリンクは明示的なIDで維持しています。

状態：**FROZEN PLAN / 未実行**（2026-10-01）。[P7ランタイムの設計・計画凍結記録](p7-runtime-freeze.md)を、実装責務、依存順、受入証拠として具体化したものです。確認項目や指定された試験は将来のゲートであり、本書はコード、データベース移行、実行時動作、性能、CIについてのPASSの検証記録ではありません。

<a id="1-exact-inputs優先順位作業境界"></a>
## 1. 厳密な入力、優先順位、作業境界

直接入力のSHA-256は、[設計改訂2の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-design-revision-2.md)の`415d5a1648702670eb5737eecfe16581b7565191f2e64409c143236a00e95461`、[計画改訂2の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-plan-revision-2.md)の`336894fcfaba7adf2e67474b56357ddfa7e9ad4502e1a0206616cb4743d9cb6b`、[独立レビュー改訂2の固定原文](https://github.com/AIrisu-072/knowledge-platform/blob/0ecf486719e3c9d71242e289a7564ad6d1032b3c/docs/superpowers/programs/search-platform-completion/p7-runtime-architecture-review-revision-2.md)の`1070a12609b408c655b03ef0da4e20b819093dca7befb8691941953b36a46172`です。現在の日本語版は、それぞれ[設計改訂2](p7-runtime-design-revision-2.md)、[計画改訂2](p7-runtime-plan-revision-2.md)、[独立レビュー改訂2](p7-runtime-architecture-review-revision-2.md)を参照してください。これらのハッシュは、`feat/search-platform-completion-core@80a47960d025e4dfdea1eacade28b15d218725ff`の未コミット作業ツリーで照合したものです。元文書、改訂1、P7共有基盤の凍結記録・計画の原文の来歴と適用順は、[凍結記録の第1節](p7-runtime-freeze.md#1-authority-と-exact-provenance)に固定しています。`spec/`が規範の正本です。

P7-Rxxは、P7-01〜12の同一PostgreSQL上のSource、current（現在の世代）、READY（準備完了状態）、pin（世代の固定）、guard（構築中の保護）、Searchイベント処理記録（Search receipt）、P5-07/08のソケットと四つのルート、P6-I04/S06/S07の汎用配送処理を、**既存責務を参照する別名として利用**します。第二のSource台帳・currentポインター、actor発行処理、P6 Domain確認応答、Graph READYは作りません。P1〜P6の最終検証記録を集約するのはR09だけです。ここでいう検証記録・証拠のreceiptと、DB上のSearchイベント処理記録は別物です。

共有の`lib.rs`、ルートのCargo設定・ロックファイル、P5 `src/api.rs`、Domain/Searchの移行ファイルは、親担当が一つずつ書き込み担当期間を割り当てて直列化します。各作業は、限定したファイルの所有者による対象を絞ったRED→GREEN、必要な実PostgreSQL・別接続・別プロセス・実ソケット、対象の厳格なClippyとfmt、独立した読み取り専用レビューの順に記録します。容量の事前許可を通らない計算は実行せず、タスクごとに全CIを繰り返しません。

<a id="2-dependency-gated-owner-plan"></a>
## 2. 依存条件と専任の所有者を定めた計画

| タスクと唯一の所有者 | 予定ファイルと成果 | 依存条件・実装受入 |
| --- | --- | --- |
| **P7-R01**：ホスト設定 | `crates/search-runtime/src/{config,secrets}.rs`、`tests/config_contract.rs`。版管理された`HostRegistrationInputV1`、登録情報一覧への参照、値を秘匿した`SecretRef` | 独立したテナント一覧、登録0件のテナント、Document/Remoteの全DTO、エポック・単調増加する改訂番号・宣言件数を、信頼されたホスト入力として検証します。未解決の秘密情報、不完全な入力・所有者不明の入力、安全でないパス・期限は起動を拒否します。公開処理は作りません。試験は`config_rejects_unowned_or_partial_inventory_ref`、`secret_value_never_enters_config_or_diagnostics`です。 |
| **P7-R04A-S**：Auditスキーマ・追記処理 | Domain `crates/document-repository-postgres/migrations/0010_audit_delivery_v0.sql`、`crates/search-runtime/src/audit/{model,source_store}.rs`、`sql/audit_roles.sql`、`tests/audit_source_schema.rs` | Domain `0009`とSearch `0001`〜`0003`の後に行います。Documentのイベント列は変更せず、配送状態列だけを追加します。別の`search_audit_outbox_events`、版管理されたポリシー、閉じたクラス・型・actor・対象・結果・理由の集合、トランザクションに結び付いた`append_search_audit_on(&mut PgConnection, TypedSearchAuditEvent)`、実ロールを扱います。未知の型、任意の内容、不正な対象、イベントのUPDATE/DELETE、P6の汎用Audit確認応答を拒否します。**W1のポリシーと既存形式用デコーダーを含めます**。 |
| **P7-R01P**：ホスト登録情報一覧の公開処理 | Search `crates/search-runtime/migrations/0004_host_registration_inventory_v1.sql`、`src/host_inventory_publish.rs`、`tests/host_inventory_publish.rs` | R01の信頼された入力、R04A-Sの追記処理、P7-03の受入済み`0003`とロールの後に行います。全テナント・両名前空間の変更不能な行、ダイジェスト、headのCAS（比較交換）、`host.registration.changed` Auditを、一接続・一トランザクションで扱います。`host_inventory_publish_atomic_all_tenants_and_namespaces`、テナント一覧・名前空間の欠落、古い状態・同じ改訂番号で異なるダイジェスト、二つの書き込み処理の衝突、Audit失敗時の全ロールバック、コミット結果不明時の別接続での再読を、実PostgreSQLで証明します。認証情報の値は保存しません。 |
| **P7-R02**：読み取り専用ホストアダプターと単一カタログ | `src/{host_registration,composition}.rs`、`tests/runtime_wiring.rs` | R01Pの実公開処理、P7-02の唯一の`PgPool`台帳、P5-08の四つのルート、P6-I04の該当コード部分、P7-12のcurrent走査、P3で採択済みのGraphを待ちます。ホストのhead・テナント一覧・DTOを別接続で再読し、Document/Remote `capture(...).await` → 台帳の突合せ → `SourceRegistrationCatalog::try_new(...).await` → PostgreSQL・current・走査の検査の後だけ、リスナーとclaim（処理権の取得）を有効にします。改訂1の起動・再起動に関する四つの指定試験を、**実公開処理の出力**でRED→GREENにします。合成した定義データだけの結果を資格の証明にしません。 |
| **P7-R04A-C**：Auditポリシー操作 | `src/audit/admin.rs`、`tests/audit_policy_transaction.rs` | R04A-Sの後に行います。版管理されたポリシー状態と型付きAuditを、同一トランザクションで変更します。試験は`audit_policy_change_insert_failure_rolls_back_revision`です。監査を伴わないファイル・環境変数の動的再読み込みを、有効なポリシーにしません。 |
| **P5-06/08の既存所有者**：拒否処理の接続口 | `crates/search-api-http/src/auth.rs`、`crates/search-runtime/src/api.rs`、該当するP5の試験 | R04A-Sの拒否記録用ポートの後に、予約した統合担当期間で行います。重要な認証・認可拒否を独立した記録トランザクションへ送り、Audit INSERT失敗時も401/403と対象の非開示を維持します。試験は`search_denial_audit_failure_still_denies_without_target_or_token`です。R04AはP5ルーターを編集しません。 |
| **P7-R05**：管理・復旧操作 | Search `migrations/0005_search_maintenance_v1.sql`、`src/rebuild.rs`、`src/bin/search_admin.rs`、`tests/runtime_recovery.rs`、運用手順書 | Search `0004`とR02の後に行います。保守・実行許可・結果記録と、完全性の問題がある対象の隔離について、唯一の書き込み担当となります。外部作用前の実行許可＋Audit、結果状態＋Auditは、それぞれ一トランザクションで確定します。`management_audit_failure_rolls_back_admission_and_prevents_side_effect`、`maintenance_result_audit_failure_never_reports_success`、別DB・別索引ルートへの復元、旧current・pinの保持を扱います。P7-11のGCとP7-12の復元は再実装しません。 |
| **P7-R03**：起動から停止までの制御・準備完了判定 | `src/{startup,health,shutdown}.rs`、`src/bin/search_service.rs`、`tests/health_lifecycle.rs` | 起動処理の骨格は先行できます。最終的な準備完了判定は、R02/R05、Audit生成元・必須の生成処理の接続口・実ロール、P7-12の走査、P5/P6の受付停止と処理終了、および**W2の新規DB初期構築**を待ちます。不一致、コミット結果不明、未記録の完全性問題がある場合は、リスナー・claimを無効にし、公開ヘルス情報は固定コードだけにします。安全な一部のRemote障害だけを、actorに見せる部分的な結果とします。 |
| **P7-R04A-D**：Audit配送 | `src/audit/{sink_pg,worker}.rs`、`sql/audit_sink.sql`、`tests/audit_delivery.rs` | R04A-SとDocument/Searchの生成処理の後に行います。両Audit生成元から別PostgreSQLの配送先へ、生成元タグ付き・版管理された形式をデコードして送ります。Audit専用のリース・フェンス・再試行・DLQ・確認応答、`event_id`の一意性と異内容時の停止、配送先・生成元のコミット結果不明時の再読を扱います。`audit_sink_outage_replays_without_domain_ack`、`audit_unknown_commit_idempotent_settle`、`audit_roles_enforce_separate_acks`を、別接続・別配送先DBで証明します。 |
| **P7-R04O/R04P**：プライバシー・通信方式の選定 | R04O：`src/{observability,telemetry_policy}.rs`、`tests/observability_privacy.rs`。R04P：`experiments/search-otlp-transport-poc/` | R04Oは、`SinkKind × RetentionMode × VisibilityClass`の許可一覧、全配送先・成功・エラー・取消・切断・期限・エクスポーター・保持ハンドルの検査用標識、P4/P5の二つのリースとソケットの終端を検査します。R04Pは、隔離したHTTP/protobuf対gRPCのPoC、厳密な機能設定・ロックファイル、ライセンス・脆弱性情報、Collector、停止時の挙動を独立レビューします。親担当の採択と規範上の厳密な版固定が済むまで、本番CargoやR06イメージへ入れません。 |
| **P7-R06**：ローカル配置可能な成果物 | Search OCIターゲット、`mise.toml`のタスク、`deploy/search-runtime/`の参照設定、`tests/container_smoke.rs` | R01〜R05、R04A-D/O、R04Pで採択・固定した内容、P1/P3/P5/P6の実アダプターの後に行います。root以外で動く成果物を、使い捨てPostgreSQL・ローカルTCP・実ロール・SIGTERMでスモーク試験します。試験は`image_rejects_unqualified_otlp_transport`です。本番へのデプロイと本番データベース移行は対象外です。 |
| **P7-R07→R08**：処理容量とSLO提案 | `experiments/search-runtime-capacity/`、`capacity.md`、`p7-operational-slo-proposal.md` | 同一候補のビルド、R06成果物、P3-P04の測定契約を入力とします。変更不能な負荷定義には、テナント・Source・Document・Unit・Graph・データ内容・処理の分岐数・HTTPとワーカーの並行数・未処理件数・コールド/ウォーム状態・障害を含めます。範囲を限定した予備測定、ディスク・RSS・時間の事前許可、途中停止、`NOT_ADMITTED`を検証します。100→1,000→3,000の関係グループは予算条件を通過した場合だけ測定し、P3固有の資格条件は縮小しません。R08は実測と業務前提が対応する範囲だけ、運用SLOの提案を記載します。 |
| **P7-R09**：最終資格判定と証拠の集約 | `p7-runtime-qualification.md`、`p7-runtime-code-review.md`、`p7-runtime-receipt.md`を、資格判定担当、独立レビュアー、親担当が順に所有 | R01〜R08とP7-01〜12、P1〜P6の最終検証記録、P3の採択、P2のモード決定、P5の実ソケット、P6の結果不明なCOMMITとフェンス付き確認応答、P7-12の別DB復元、**W1/W2**を、厳密に特定したコードSHAで照合します。対象を絞った修正は元の所有者へ戻し、最後のホスト側の当該headに対するゲートを別証拠で判定します。ローカル、実行時動作、レビュー、ホスト側CI、本番稼働は別欄にします。 |

<a id="3-w1w2-の-mandatory-acceptance"></a>
## 3. W1/W2の必須受入条件

**W1：`document.version.read_confirmed`。** `spec/operations/observability-audit-requirements-v0.md:540–597`に従い、初回の明示的な既読確認を必須Auditクラスとして、R04A-Sのクラスポリシー・Documentの既存形式用デコーダーと、R04A-Dの配送先への投影処理に含めます。Documentの所有者は、既存の`crates/document-repository-postgres/src/read_state.rs:146–164`の生成処理を、`tests/read_state_transaction.rs`などの実PostgreSQL試験で、このクラスに固有の条件として検証します。`read_confirmed_audit_insert_failure_rolls_back_first_read`をRED→GREENとし、Audit INSERT失敗時に初回の既読状態がコミットされず、成功応答も返らず、Audit行も残らないことを別接続で確認します。重複した確認を初回イベントとして二重生成しません。R04A-S/Dはデコーダー・クラスの許可と配送先への投影、Documentの所有者は業務状態のロールバック、R09は両方の検証記録の照合を担当します。その他のDocument必須クラスと、P5/R05/R03のクラス別ロールバック・拒否試験も省きません。

**W2：全ファイルが存在する新規DBの初期構築。** R03を移行・起動許可の統合担当、R09を独立受入の担当として、新規の使い捨てPostgreSQLにDomain `0009` → Search `0001`〜`0003` → Domain `0010` → Search `0004`の順で実際に適用し、観測します。Search `0005`はその後、R05に従います。Domain `_sqlx_migrations`とSearch `search_runtime_sqlx_migrations`のバージョン・チェックサムを段階ごとに照合し、移行実行側、Audit追記・配送側、ホスト登録情報一覧の公開・読み取り側、Source登録、API/claimの実接続ロール・権限付与と、過大な権限の拒否を確認します。`fresh_bootstrap_domain_search_audit_inventory_order`と`fresh_bootstrap_rejects_checksum_or_role_mismatch`をRED→GREENにします。現行の`search-runtime::migrate`は単一の`sqlx::migrate!("./migrations")`入口なので、全移行ファイルが揃った状態での呼び出しだけを、所定の段階順の証拠にしません。順序を保証する起動・移行手順を、限定した書き込み担当期間で実装し、逆順・欠落・チェックサムのずれ・ロール不一致がある場合は、リスナー・claim・準備完了状態を有効にしません。これは本番データベース移行の実行資格ではありません。

<a id="4-completion-boundary-と次の-exact-action"></a>
## 4. 完了とみなす範囲と当時の次の具体的な作業

五つのアーキテクチャ指摘は設計上解決しましたが、上表の試験は未実行です。当時の順序は、最初にR04A-SとP7-03のスキーマ・ロールのゲート、およびR01の信頼された入力を実装受入し、次にR01Pの実公開処理とAudit失敗・コミット結果不明の異常系試験へ進む、というものでした。R02は実公開処理・台帳・current・走査が揃ってから組み立て、R03/R09はW2を新規DBで判定します。各生成処理は、W1を含むクラス別の同一トランザクション・ロールバックを、所有者の試験で証明します。R04Pの採択、P3-P04のバックエンド選定、P2のVector・ランタイムモード、P1〜P6の最終検証記録が未達なら、R06/R09の該当する本番ゲートは閉じたままにします。

R09の最終判断まで、本番稼働の準備完了、SLO保証、マージ、本番データベース移行、本番へのデプロイを宣言しません。失敗した資格や不明な資格は、元の所有者による対象を絞った修正と独立した再レビューへ戻します。
